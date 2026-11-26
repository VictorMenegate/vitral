//! Painel da direita, aba do chat: "Este chat" no jeito do Linear (25/11/2026), em grupos com um
//! rótulo pequeno e linhas de 26, sem cartões: Agora (o estado, o modelo, o modo, o esforço e a
//! conta), Pasta (onde ele trabalha e o ramo), Passos, Nesta conversa, Mudanças, Na pasta e
//! Revisão.

use gpui_kit::component::{
    ElementExt as _, Icon, h_flex, scroll::ScrollableElement as _, tooltip::{ManagedTooltipExt as _, Tooltip},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Gaveta, Janela,
    componentes::{AnelGirando, Marca, Reflexo as _, Selo, Simbolo, icone_do_modo, sinal_do_chat},
};
use crate::{
    modelo::{Agente, Chat, Execucao},
    motor::Motor,
    tema::{Cores, FONTE_MONO, Lucide, esp, linear, r, raio},
};

/// Abaixo desta largura (em rem, uns 330 px a 100%) o painel da direita fica no desenho estreito.
const LIMITE_ESTREITA: f32 = 22.;

/// Um grupo do painel da direita: o rótulo de 11 no tênue (com a contagem apagada e, à direita, o
/// que o grupo quiser) e as linhas, com 1 entre elas e 10 depois do grupo (protótipo Linear).
pub(super) fn grupo_da_direita(
    titulo: impl Into<SharedString>,
    contagem: Option<String>,
    canto: Option<AnyElement>,
    linhas: Vec<AnyElement>,
) -> AnyElement {
    v_flex()
        .flex_none()
        .gap(linear::GRUPO_LINHAS)
        .mb(linear::ANTES_DA_SECAO)
        .child(
            h_flex()
                .px(esp::E8)
                .mb(linear::GRUPO_LINHAS)
                .gap(esp::E6)
                .text_size(r(11.))
                .line_height(r(14.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(Cores::tenue())
                .child(div().flex_none().child(titulo.into()))
                .children(contagem.map(|n| div().flex_none().text_color(Cores::apagado()).child(n)))
                .child(div().flex_1())
                .children(canto),
        )
        .children(linhas)
        .into_any_element()
}

/// Uma linha do painel da direita: 26 de altura, 8 dos lados, 7 entre as partes, em 12,5.
pub(super) fn linha_da_direita(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(linear::PASSO_VAO)
        .h(r(26.))
        .px(esp::E8)
        .rounded(raio::CONTROLE)
        .text_size(r(12.5))
        .line_height(r(16.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(Cores::texto_da_fala())
}

/// A coluna de 15 do símbolo de uma linha.
pub(super) fn simbolo_da_linha(e: impl IntoElement) -> Div {
    div().flex_none().size(r(15.)).flex().items_center().justify_center().child(e)
}

/// O texto em mono de uma linha (caminhos, nomes de arquivo, ramos): 11,5, cortado.
pub(super) fn mono_da_linha(t: impl Into<SharedString>) -> Div {
    div().min_w_0().truncate().font_family(FONTE_MONO).text_size(r(11.5)).child(t.into())
}

/// O +N −M de uma linha de arquivo.
pub(super) fn mais_e_menos(entram: usize, saem: usize) -> Div {
    h_flex()
        .flex_none()
        .gap(linear::PASSO_VAO)
        .when(entram > 0, |s| s.child(div().text_color(Cores::verde()).child(format!("+{entram}"))))
        .when(saem > 0, |s| s.child(div().text_color(Cores::vermelho()).child(format!("−{saem}"))))
}

impl Janela {
    pub(super) fn painel_do_chat(
        &mut self,
        motor_entidade: &Entity<Motor>,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let motor = motor_entidade.read(cx);
        let chat = self.selecionado.as_ref().and_then(|id| motor.chat(id));
        let mut grupos: Vec<AnyElement> = Vec::new();
        if let Some(chat) = chat {
            grupos.push(self.grupo_agora(motor, chat, window));
            grupos.push(self.grupo_pasta(motor, chat, window, cx));
            if !motor.passos(&chat.id).is_empty() {
                grupos.push(passos(motor.passos(&chat.id), window));
            }
            // O sumário da conversa: uma linha por mensagem sua, para ir até ela (`sumario.rs`).
            grupos.extend(self.secao_do_sumario(chat, cx));
            grupos.extend(mudancas(chat, window));
            grupos.extend(self.secao_git(chat, window, cx));
            grupos.push(self.grupo_revisao(motor, chat, window, cx));
        }
        // Ao trocar de chat, o painel volta ao topo.
        let atual = self.selecionado.clone();
        if std::mem::replace(&mut self.este_chat_rolado, atual.clone()) != atual {
            self.rolagem_este_chat.set_offset(point(px(0.), px(0.)));
        }
        let rolado = self.rolagem_este_chat.offset().y < px(0.);
        let cabecalho = self.barra_de_abas(motor, super::abas_direita::AbaDireita::Chat, rolado, "arrastar-este-chat", window, cx);
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            // Ancorado, é a última peça do cartão das conversas: fecha os cantos da direita e
            // emenda na conversa da esquerda pela divisória (ver `aresta_encostada`).
            .when(self.gaveta == Some(Gaveta::Direita), |s| s.rounded(crate::tema::raio_da_peca()))
            .when(self.gaveta != Some(Gaveta::Direita), |s| s.bg(Cores::cartao()).rounded_r(crate::tema::raio_da_peca()))
            .when_some(crate::tema::bordas_finas().filter(|_| self.gaveta != Some(Gaveta::Direita)), |s, anel| s.shadow(vec![anel]))
            .overflow_hidden()
            .child(cabecalho)
            .child(
                div()
                    .id("painel-este-chat")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.rolagem_este_chat)
                    .flex()
                    .flex_col()
                    // 6 dos lados e 12 no pé, os do protótipo.
                    .px(esp::E6)
                    .children(grupos)
                    .child(div().flex_none().h(esp::E12))
                    .vertical_scrollbar(&self.rolagem_este_chat),
            )
            .when(self.gaveta != Some(Gaveta::Direita), |s| s.aresta_encostada(crate::tema::Esquerda::Emendada))
            .on_prepaint(self.medir_direita(cx))
    }

    /// Guarda se o painel da direita ficou estreito (abaixo de 22 rem, com folga) quando isso
    /// muda: no quadro seguinte, porque avisar a janela durante o desenho não a redesenha.
    pub(super) fn medir_direita(
        &self,
        cx: &Context<Self>,
    ) -> impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static {
        let janela = cx.entity().downgrade();
        let (antes, compacta_antes, largura_antes) = (self.direita_estreita, self.direita_compacta, self.direita_largura);
        move |limites, window, _| {
            let rem = limites.size.width / window.rem_size();
            let estreita = fica_estreita(rem, antes);
            let compacta = fica_compacta(rem, compacta_antes);
            // A largura, de meio em meio rem, para o cabeçalho do arquivo decidir o que cabe nele
            // (dono, 16/11/2026: "59 linhas · JavaScr" ficava por baixo do Mencionar).
            let largura = if (rem - largura_antes).abs() >= 0.5 { (rem * 2.).floor() / 2. } else { largura_antes };
            if estreita == antes && compacta == compacta_antes && largura == largura_antes {
                return;
            }
            window.on_next_frame(move |_, cx| {
                let _ = janela.update(cx, |this, cx| {
                    this.direita_estreita = estreita;
                    this.direita_compacta = compacta;
                    this.direita_largura = largura;
                    cx.notify();
                });
            });
        }
    }

    /// Agora: o que o chat faz (trabalhando ou o que espera), o agente e o modelo, o modo, o
    /// esforço e a conta. O modelo e o esforço abrem o seletor deles na caixa.
    fn grupo_agora(&self, motor: &Motor, chat: &Chat, window: &Window) -> AnyElement {
        let mut linhas: Vec<AnyElement> = Vec::new();
        let id = |nome: &str| SharedString::from(format!("este-chat-{nome}-{}", chat.id));
        match motor.execucao(&chat.id) {
            Execucao::Rodando { desde } => linhas.push(
                linha_da_direita(id("estado"))
                    .child(simbolo_da_linha(AnelGirando::new(15.).cor(Cores::destaque()).vivo(format!("este-chat-{}", chat.id))))
                    .child(div().min_w_0().truncate().child(format!("Trabalhando · {}", super::duracao(desde))))
                    .into_any_element(),
            ),
            _ => {
                if let Some(sinal) = sinal_do_chat(motor, chat) {
                    let rotulo = match sinal {
                        super::componentes::Sinal::Erro => "Falhou",
                        super::componentes::Sinal::Pedido => "Esperando você",
                        super::componentes::Sinal::Parada => "Parada no meio",
                        super::componentes::Sinal::Nova => "Resposta nova",
                        super::componentes::Sinal::Rodando => "Trabalhando",
                    };
                    linhas.push(
                        linha_da_direita(id("estado"))
                            .child(simbolo_da_linha(sinal.ponto(8.)))
                            .child(div().min_w_0().truncate().child(rotulo))
                            .into_any_element(),
                    );
                }
            }
        }
        let clicavel = |l: Stateful<Div>| l.cursor_pointer().hover(|s| s.bg(Cores::hover()));
        linhas.push(
            clicavel(linha_da_direita(id("modelo")))
                .child(simbolo_da_linha(Marca::do_agente(chat.agente).glifo(15.)))
                .child(div().min_w_0().truncate().child(super::composer::modelo_por_extenso(motor, chat)))
                .dica_no_ponteiro(|window, cx| Tooltip::new("Trocar o modelo").action(&super::composer::AbrirModelo, Some("Vitral")).build(window, cx))
                .on_click(|_, window, cx| window.dispatch_action(Box::new(super::composer::AbrirModelo), cx))
                .into_any_element(),
        );
        let modo = motor.permissao_de(chat);
        let (icone, cor) = icone_do_modo(&modo);
        linhas.push(
            linha_da_direita(id("modo"))
                .child(simbolo_da_linha(Simbolo::Icone(icone, Some(cor)).desenhar(15., window)))
                .child(div().min_w_0().truncate().child(super::composer::nome_do_modo(&modo)))
                .dica_no_ponteiro(|window, cx| Tooltip::new("Modo de permissão\nShift+Tab na caixa troca").build(window, cx))
                .into_any_element(),
        );
        let (atual, padrao, usa) = super::composer::esforco_do_chat(motor, chat);
        if usa {
            let nivel = if atual == 0 { padrao.unwrap_or(0) } else { atual };
            let rotulo = match (atual, padrao) {
                (0, Some(p)) => format!("Esforço auto · {}", super::composer::ESFORCOS[p].1.to_lowercase()),
                (0, None) => "Esforço automático".to_string(),
                (n, _) => format!("Esforço {}", super::composer::ESFORCOS[n].1.to_lowercase()),
            };
            linhas.push(
                clicavel(linha_da_direita(id("esforco")))
                    .child(simbolo_da_linha(Simbolo::Sinal { nivel: nivel as u8, total: 5 }.desenhar(15., window)))
                    .child(div().min_w_0().truncate().child(rotulo))
                    .dica_no_ponteiro(|window, cx| Tooltip::new("Trocar o esforço").action(&super::composer::AbrirEsforco, Some("Vitral")).build(window, cx))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(super::composer::AbrirEsforco), cx))
                    .into_any_element(),
            );
        }
        // A conta é do Claude: o Codex e o Gemini usam a deles.
        if chat.agente == Agente::Claude {
            let conta = chat
                .conta
                .as_ref()
                .and_then(|id| motor.contas.iter().find(|c| &c.id == id).cloned())
                .unwrap_or_else(|| motor.conta_padrao());
            linhas.push(
                linha_da_direita(id("conta"))
                    .child(simbolo_da_linha(super::componentes::icone(Lucide::CircleUserRound, 15., window)).text_color(Cores::tenue()))
                    .child(div().min_w_0().truncate().child(conta.nome))
                    .into_any_element(),
            );
        }
        grupo_da_direita("Agora", None, None, linhas)
    }

    /// Pasta: onde o chat trabalha (o clique abre a pasta) e o ramo do git. A conversa trazida do
    /// Claude Code diz de onde veio; a pasta que sumiu fica em vermelho.
    fn grupo_pasta(&mut self, motor: &Motor, chat: &Chat, window: &Window, cx: &Context<Self>) -> AnyElement {
        let pasta = chat.cwd.clone();
        let sumiu = !pasta.exists();
        let mut linhas: Vec<AnyElement> = Vec::new();
        let (curta, dica): (String, SharedString) = if motor.sem_pasta(chat) {
            ("Sem pasta".to_string(), "Um chat sem pasta, numa pasta vazia só dele: os arquivos que ele criar ficam lá.".into())
        } else if sumiu {
            (super::dialogos::com_til(&pasta), "A pasta não foi encontrada (disco desmontado ou pasta renomeada)".into())
        } else {
            (super::dialogos::com_til(&pasta), format!("{}\nO clique abre a pasta", pasta.display()).into())
        };
        let abrir = pasta.clone();
        linhas.push(
            linha_da_direita(SharedString::from(format!("este-chat-pasta-{}", chat.id)))
                .when(!sumiu, |l| l.cursor_pointer().hover(|s| s.bg(Cores::hover())).on_click(move |_, _, cx| crate::sistema::abrir_pasta(&abrir, cx)))
                .when(sumiu, |l| l.text_color(Cores::vermelho()))
                .child(simbolo_da_linha(super::componentes::icone(Lucide::Folder, 15., window)).text_color(Cores::tenue()))
                .child(mono_da_linha(curta))
                .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                .into_any_element(),
        );
        if let Some(ramo) = self.ramo_da_pasta(&chat.cwd, cx) {
            let dica = SharedString::from(super::ramo::onde_esta(&ramo));
            let icone = if ramo.solta { Lucide::GitCommitHorizontal } else { Lucide::GitBranch };
            linhas.push(
                linha_da_direita(SharedString::from(format!("este-chat-ramo-{}", chat.id)))
                    .child(simbolo_da_linha(super::componentes::icone(icone, 15., window)).text_color(Cores::tenue()))
                    .child(mono_da_linha(ramo.nome.clone()))
                    .when(ramo.worktree, |s| s.child(Selo::cor("worktree", Cores::tenue())))
                    .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                    .into_any_element(),
            );
        }
        if let Some(i) = &chat.importada {
            linhas.push(
                div().px(esp::E8).pt(esp::E2).text_size(r(12.)).line_height(r(16.)).text_color(Cores::tenue()).child(crate::importar::descricao(i)).into_any_element(),
            );
        }
        grupo_da_direita("Pasta", None, None, linhas)
    }

    /// Revisão: quem confere os turnos que mudam bastante (a revisão automática, ligada na barra de
    /// baixo) e, com ele no limite do plano, até quando.
    fn grupo_revisao(&self, motor: &Motor, chat: &Chat, window: &Window, cx: &Context<Self>) -> AnyElement {
        let outro = chat.agente.outro();
        let ligada = motor.config.revisar_edicoes;
        let mut linhas: Vec<AnyElement> = Vec::new();
        linhas.push(
            linha_da_direita(SharedString::from(format!("este-chat-revisao-{}", chat.id)))
                .when(!ligada, |l| l.text_color(Cores::tenue()))
                .child(simbolo_da_linha(Marca::do_agente(outro).glifo(15.)).when(!ligada, |s| s.opacity(0.55)))
                .child(div().min_w_0().truncate().child(if ligada { format!("{} revisa", outro.nome()) } else { "Desligada".to_string() }))
                .into_any_element(),
        );
        if ligada && let Some(limite) = self.limite_do_agente(outro, cx) {
            linhas.push(
                linha_da_direita(SharedString::from(format!("este-chat-limite-{}", chat.id)))
                    .text_color(Cores::tenue())
                    .child(simbolo_da_linha(div().size(r(7.)).rounded_full().bg(Cores::espera())))
                    .child(div().min_w_0().truncate().child(limite))
                    .into_any_element(),
            );
        }
        let _ = window;
        grupo_da_direita("Revisão", None, None, linhas)
    }
}

/// Mudanças (dono, 16/11/2026: rodada de uso, item 5.1): os arquivos que o chat editou ou criou,
/// os deste turno primeiro, com o nome em mono, o +N −M e o "novo". O clique abre o arquivo na
/// edição. Sai das chamadas de edição: o que um comando mudou fica de fora (a dica do rótulo diz).
fn mudancas(chat: &Chat, window: &Window) -> Option<AnyElement> {
    let (neste, antes) = super::mensagens::mudancas_do_chat(&chat.itens);
    if neste.is_empty() && antes.is_empty() {
        return None;
    }
    let linhas: Vec<AnyElement> = neste
        .iter()
        .chain(antes.iter())
        .enumerate()
        .map(|(i, arquivo)| linha_mudanca(chat, arquivo, format!("mudanca-{i}"), window))
        .collect();
    let n = linhas.len();
    Some(grupo_da_direita("Mudanças", Some(n.to_string()), None, linhas))
}

/// Uma linha de Mudanças: o ícone do tipo, o nome em mono, o +N −M e o "novo"; a pasta na dica.
/// Um arquivo que saiu do disco fica apagado e sem clique.
fn linha_mudanca(chat: &Chat, arquivo: &super::mensagens::ArquivoMudado, id: String, window: &Window) -> AnyElement {
    let inteiro = super::links_arquivo::resolver(&chat.cwd, &arquivo.caminho);
    let relativo = inteiro
        .as_deref()
        .and_then(|i| std::path::Path::new(i).strip_prefix(&chat.cwd).ok().map(crate::git::caminho_em_texto))
        .unwrap_or_else(|| arquivo.caminho.clone());
    let nome = relativo.rsplit_once('/').map_or(relativo.as_str(), |(_, n)| n).to_string();
    let icone = super::seletor_arquivos::icone_do_arquivo(&nome);
    let existe = inteiro.is_some();
    // O clique abre o arquivo já na edição que o chat fez nele (dono, 16/11/2026: "ver a edição").
    let alvo = inteiro.map(|_| super::edicao::AbrirEdicao { chat: chat.id.clone(), itens: arquivo.itens.clone(), caminho: None });
    let dica = SharedString::from(if existe { relativo.clone() } else { format!("{relativo} (não está mais no disco)") });
    linha_da_direita(SharedString::from(id))
        .when(existe, |s| s.cursor_pointer().hover(|s| s.bg(Cores::hover())))
        .when(!existe, |s| s.text_color(Cores::tenue()))
        .child(simbolo_da_linha(super::componentes::icone(icone, 15., window)).text_color(Cores::tenue()))
        .child(mono_da_linha(nome).flex_shrink(1.))
        .child(mais_e_menos(arquivo.entram, arquivo.saem))
        .when(arquivo.novo, |s| s.child(Selo::cor("novo", Cores::verde())))
        .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        .when_some(alvo, |s, alvo| s.on_click(move |_, window, cx| window.dispatch_action(Box::new(alvo.clone()), cx)))
        .into_any_element()
}

/// O painel da direita fica estreito abaixo de 22 rem e só volta acima de 22,5 (a folga evita
/// trocar de desenho a cada pixel com a largura parada no limite).
fn fica_estreita(rem: f32, antes: bool) -> bool {
    if antes {
        rem < LIMITE_ESTREITA + 0.5
    } else {
        rem < LIMITE_ESTREITA - 0.5
    }
}

/// As abas da direita ficam só com o símbolo abaixo de 520 px a 100% (em rem) e só voltam acima
/// de 520 + 7,5 (a mesma folga de meio rem).
fn fica_compacta(rem: f32, antes: bool) -> bool {
    let limite = super::abas_direita::LIMITE_DAS_ABAS / 15.;
    if antes { rem < limite + 0.5 } else { rem < limite }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: ele traria o `test` do gpui, e o #[test] viraria recursão.
    use super::fica_estreita;

    #[test]
    fn largura_estreita_com_folga() {
        assert!(fica_estreita(20., false));
        assert!(!fica_estreita(21.8, false));
        assert!(fica_estreita(22.3, true));
        assert!(!fica_estreita(22.6, true));
        // 380 px a 100% (25,3 rem) é normal; a 150% (16,9 rem), estreito.
        assert!(!fica_estreita(380. / 15., false));
        assert!(fica_estreita(380. / 22.5, false));
    }
}

/// Um passo da lista do agente: o símbolo do estado e o texto (o que ele faz agora, no gerúndio
/// dele e em destaque). É a linha da seção Passos e a do popover da pílula dos passos.
pub(super) fn linha_do_passo(p: &crate::motor::Passo) -> Div {
    use crate::motor::EstadoDoPasso::*;
    let (simbolo, cor, texto) = match p.estado {
        Feito => (Lucide::CircleCheck, Cores::verde(), p.texto.clone()),
        Fazendo => (Lucide::CircleDot, Cores::destaque_texto(), p.fazendo.clone()),
        Falta => (Lucide::Circle, Cores::apagado(), p.texto.clone()),
    };
    h_flex()
        .items_start()
        .gap(linear::PASSO_VAO)
        .child(div().flex_none().mt(linear::GRUPO_LINHAS).text_color(cor).child(Icon::new(simbolo).size(r(15.))))
        .child(
            div()
                .min_w_0()
                .text_size(r(12.5))
                .line_height(r(17.))
                .text_color(if p.estado == Fazendo { Cores::forte() } else if p.estado == Feito { Cores::tenue() } else { Cores::texto_da_fala() })
                .child(texto),
        )
}

/// Passos: a lista que o agente mantém (o TodoWrite), com o que está fazendo em destaque.
fn passos(lista: &[crate::motor::Passo], _: &Window) -> AnyElement {
    use crate::motor::EstadoDoPasso::*;
    let feitos = lista.iter().filter(|p| p.estado == Feito).count();
    let linhas = lista
        .iter()
        .map(|p| div().px(esp::E8).py(esp::E4).child(linha_do_passo(p)).into_any_element())
        .collect();
    grupo_da_direita("Passos", Some(format!("{feitos} de {}", lista.len())), None, linhas)
}
