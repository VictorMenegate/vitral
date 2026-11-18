//! Painel da direita, aba do chat: "Este chat", a pasta onde ele trabalha, com Abrir pasta (e o
//! aviso, se ela sumiu).

use gpui_kit::component::{
    ElementExt as _, Icon, StyledExt as _, h_flex, scroll::ScrollableElement as _, tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Gaveta, Janela,
    componentes::{Alerta, BotaoCopiar, Reflexo as _, TamanhoBotao, Tom, botao_fantasma},
    sem_arrastar,
};
use crate::{
    modelo::Chat,
    motor::Motor,
    tema::{Cores, FONTE_MONO, Lucide, Tipo as _, dentro_da_borda, esp, r, raio},
};

/// Abaixo desta largura (em rem, uns 330 px a 100%) o painel da direita fica no desenho estreito.
const LIMITE_ESTREITA: f32 = 22.;

impl Janela {
    pub(super) fn painel_do_chat(
        &mut self,
        motor_entidade: &Entity<Motor>,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let motor = motor_entidade.read(cx);
        let chat = self.selecionado.as_ref().and_then(|id| motor.chat(id));
        let mut filhos: Vec<AnyElement> = Vec::new();
        if let Some(chat) = chat {
            // Os passos do agente, quando ele anotou uma lista; depois as Mudanças; a pasta e o
            // Abrir pasta descem para o pé (item 5.1).
            if !motor.passos(&chat.id).is_empty() {
                filhos.push(passos(motor.passos(&chat.id)));
            }
            // O sumário da conversa: uma linha por mensagem sua, para ir até ela (`sumario.rs`).
            filhos.extend(self.secao_do_sumario(chat, cx));
            filhos.push(self.mudancas(chat, window));
            filhos.extend(self.secao_git(chat, window, cx));
            filhos.push(self.este_chat(motor, chat, window, cx));
        }
        // Ao trocar de chat, o painel volta ao topo.
        let atual = self.selecionado.clone();
        if std::mem::replace(&mut self.este_chat_rolado, atual.clone()) != atual {
            self.rolagem_este_chat.set_offset(point(px(0.), px(0.)));
        }

        // Com a lista rolada, uma borda separa o cabeçalho, como na conversa.
        let rolado = self.rolagem_este_chat.offset().y < px(0.);
        let cabecalho = self.barra_de_abas(motor, super::abas_direita::AbaDireita::Chat, rolado, "arrastar-este-chat", window, cx);
        // O respiro de cima e o de baixo vão nos próprios filhos: com padding, a área rolável
        // somava os 20 ao máximo (por causa da barra, absoluta) e rolava sozinha.
        let filhos = filhos
            .into_iter()
            .enumerate()
            .map(|(i, filho)| div().flex_none().mt(if i == 0 { esp::E4 } else { r(12.) }).child(filho));
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            // Ancorado, é a última peça da fileira das conversas: encosta no topo e na direita da
            // janela e emenda na conversa da esquerda pela divisória (ver `aresta_encostada`).
            .when(self.gaveta == Some(Gaveta::Direita), |s| s.rounded(crate::tema::raio_da_peca()))
            .when(self.gaveta != Some(Gaveta::Direita), |s| s.bg(Cores::cartao()))
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
                    .px(esp::E12)
                    // Sem encolher: o que passa da altura rola, em vez de espremer os cartões.
                    .children(filhos)
                    .child(div().flex_none().h(esp::E16))
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

    /// A seção Mudanças (dono, 16/11/2026: rodada de uso, item 5.1): os arquivos que o chat editou
    /// ou criou, em "Neste turno" e "Antes", com o ícone, o nome, a pasta em tom tênue, o +N −M e o
    /// selo "novo". O clique abre o arquivo à direita. Sai das chamadas de edição: o que um comando
    /// mudou fica de fora, e a nota no pé diz isso.
    fn mudancas(&self, chat: &Chat, window: &Window) -> AnyElement {
        let (neste, antes) = super::mensagens::mudancas_do_chat(&chat.itens);
        let titulo = |t: &'static str, n: usize| {
            div()
                .flex()
                .items_center()
                .gap(esp::E6)
                .h(r(24.))
                .metadado()
                .font_semibold()
                .text_color(Cores::tenue())
                .child(t)
                .child(div().font_family(FONTE_MONO).text_color(Cores::apagado()).child(n.to_string()))
        };
        let mut corpo = div().flex().flex_col().gap(esp::E2);
        if neste.is_empty() && antes.is_empty() {
            corpo = corpo.child(
                div().corpo_ui().text_color(Cores::tenue()).child("Este chat ainda não editou nem criou arquivo."),
            );
        }
        for (rotulo, lista, grupo) in [("Neste turno", &neste, "neste"), ("Antes", &antes, "antes")] {
            if lista.is_empty() {
                continue;
            }
            corpo = corpo.child(titulo(rotulo, lista.len()));
            for (i, arquivo) in lista.iter().enumerate() {
                corpo = corpo.child(linha_mudanca(chat, arquivo, format!("mudanca-{grupo}-{i}"), window));
            }
        }
        div()
            .flex()
            .flex_col()
            .gap(esp::E8)
            .p(dentro_da_borda(12.))
            .rounded(raio::CARTAO)
            .bg(Cores::cartao_elevado())
            .border_1()
            .border_color(Cores::linha())
            .reflexo(raio::cartao())
            .child(div().rotulo().text_color(Cores::forte()).child("Mudanças"))
            .child(corpo)
            .into_any_element()
    }

    /// "Este chat": a pasta onde o chat trabalha, direto, com Abrir pasta (sem ela, o aviso). A
    /// conversa trazida do Claude Code diz de onde veio; o chat sem pasta, que a pasta é só dele.
    fn este_chat(&self, motor: &Motor, chat: &Chat, window: &Window, cx: &Context<Self>) -> AnyElement {
        let pasta = chat.cwd.clone();
        let curta = super::dialogos::com_til(&pasta);
        let mut linhas: Vec<AnyElement> = Vec::new();
        let texto = |t: String| div().corpo_ui().text_color(Cores::texto()).child(t).into_any_element();
        let sumiu = !pasta.exists();
        if sumiu {
            let alerta = Alerta::new(Tom::Erro, format!("A pasta {curta} não foi encontrada (disco desmontado ou pasta renomeada)."));
            linhas.push(alerta.into_any_element());
        }
        let descricao = match &chat.importada {
            Some(i) => crate::importar::descricao(i),
            None if motor.sem_pasta(chat) => "Um chat sem pasta, numa pasta vazia só dele: os arquivos que ele criar ficam lá.".to_string(),
            // O caminho fica só na linha Pasta, logo abaixo.
            None => "Mexe direto na pasta, como o Claude Code no terminal.".to_string(),
        };
        linhas.push(texto(descricao));
        let caminho = pasta.display().to_string();
        linhas.push(linha_dado(format!("este-chat-pasta-{}", chat.id), "Pasta", curta, caminho.clone(), caminho, 56.).into_any_element());
        let abrir = (!sumiu).then(|| {
            botao_fantasma("este-chat-abrir", Some(Lucide::FolderOpen.into()), "Abrir pasta", TamanhoBotao::Barra, window, cx)
                .on_click(move |_, _, cx| crate::sistema::abrir_pasta(&pasta, cx))
        });
        div()
            .flex()
            .flex_col()
            .gap(esp::E8)
            .p(dentro_da_borda(12.))
            .rounded(raio::CARTAO)
            .bg(Cores::cartao_elevado())
            .border_1()
            .border_color(Cores::linha())
            .reflexo(raio::cartao())
            .children(linhas)
            .when_some(abrir, |s, abrir| s.child(sem_arrastar().flex().flex_wrap().gap(esp::E4).child(abrir)))
            .child(div().metadado().text_color(Cores::tenue()).child("O que um comando mudou não aparece em Mudanças."))
            .into_any_element()
    }
}

/// Uma linha de Mudanças: o ícone do tipo, o nome, a pasta em tom tênue, o +N −M e o "novo". O
/// clique abre o arquivo à direita, na edição; um arquivo que saiu do disco fica apagado e sem
/// clique.
fn linha_mudanca(chat: &Chat, arquivo: &super::mensagens::ArquivoMudado, id: String, window: &Window) -> AnyElement {
    let inteiro = super::links_arquivo::resolver(&chat.cwd, &arquivo.caminho);
    let relativo = inteiro
        .as_deref()
        .and_then(|i| std::path::Path::new(i).strip_prefix(&chat.cwd).ok().map(crate::git::caminho_em_texto))
        .unwrap_or_else(|| arquivo.caminho.clone());
    let (pasta, nome) = relativo.rsplit_once('/').unwrap_or(("", relativo.as_str()));
    let (pasta, nome) = (pasta.to_string(), nome.to_string());
    let icone = super::seletor_arquivos::icone_do_arquivo(&nome);
    let existe = inteiro.is_some();
    // O clique abre o arquivo já na edição que o chat fez nele (dono, 16/11/2026: "ver a edição").
    let alvo = inteiro.map(|_| super::edicao::AbrirEdicao { chat: chat.id.clone(), itens: arquivo.itens.clone(), caminho: None });
    let dica = SharedString::from(if existe { relativo.clone() } else { format!("{relativo} (não está mais no disco)") });
    div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .gap(esp::E8)
        .h(r(28.))
        .px(esp::E6)
        .rounded(raio::CONTROLE)
        .when(existe, |s| s.cursor_pointer().hover(|s| s.bg(Cores::hover())))
        .child(div().flex_none().text_color(Cores::tenue()).child(super::componentes::icone(icone, 14., window)))
        .child(
            div()
                .flex_shrink(1.)
                .min_w_0()
                .truncate()
                .corpo_ui()
                .text_color(if existe { Cores::texto() } else { Cores::tenue() })
                .child(nome),
        )
        .child(div().flex_1().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(pasta))
        .child(
            div()
                .flex()
                .flex_none()
                .gap(esp::E6)
                .font_family(FONTE_MONO)
                .metadado()
                .when(arquivo.entram > 0, |s| s.child(div().text_color(Cores::verde()).child(format!("+{}", arquivo.entram))))
                .when(arquivo.saem > 0, |s| s.child(div().text_color(Cores::vermelho()).child(format!("−{}", arquivo.saem)))),
        )
        .when(arquivo.novo, |s| s.child(super::componentes::Selo::cor("novo", Cores::verde())))
        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        .when_some(alvo, |s, alvo| {
            s.on_click(move |_, window, cx| window.dispatch_action(Box::new(alvo.clone()), cx))
        })
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

/// O Copiar de 24 (troca para o visto por 2 s).
fn copiar(id: String, valor: String) -> impl IntoElement {
    BotaoCopiar::new(SharedString::from(id), valor)
}

/// Uma linha de dado (28): o rótulo numa coluna fixa, o valor em mono cortado no começo (o fim do
/// caminho é o que importa), o valor inteiro na dica e copiar.
fn linha_dado(
    id: String,
    rotulo: &'static str,
    valor: String,
    dica: String,
    copia: String,
    largura_rotulo: f32,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(esp::E8)
        .h(r(28.))
        .child(div().flex_none().w(r(largura_rotulo)).metadado().text_color(Cores::tenue()).child(rotulo))
        .child(
            div()
                .id(SharedString::from(id.clone()))
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_start()
                .mono_pequeno()
                .text_color(Cores::texto())
                .child(valor)
                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx)),
        )
        .child(copiar(format!("{id}-copiar"), copia))
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

/// A seção Passos: a lista que o agente mantém (o TodoWrite), com o que está fazendo em destaque.
fn passos(lista: &[crate::motor::Passo]) -> AnyElement {
    use crate::motor::EstadoDoPasso::*;
    let feitos = lista.iter().filter(|p| p.estado == Feito).count();
    let linhas = lista.iter().map(|p| {
        let (simbolo, cor, texto) = match p.estado {
            Feito => (Lucide::CircleCheck, Cores::verde(), p.texto.clone()),
            Fazendo => (Lucide::CircleDot, Cores::destaque_texto(), p.fazendo.clone()),
            Falta => (Lucide::Circle, Cores::apagado(), p.texto.clone()),
        };
        h_flex()
            .items_start()
            .gap(esp::E8)
            .child(div().flex_none().mt(r(2.)).text_color(cor).child(Icon::new(simbolo).size(r(14.))))
            .child(
                div()
                    .min_w_0()
                    .corpo_ui()
                    .text_color(if p.estado == Fazendo { Cores::forte() } else if p.estado == Feito { Cores::tenue() } else { Cores::texto() })
                    .child(texto),
            )
    });
    div()
        .flex()
        .flex_col()
        .gap(esp::E8)
        .p(dentro_da_borda(12.))
        .rounded(raio::CARTAO)
        .bg(Cores::cartao_elevado())
        .border_1()
        .border_color(Cores::linha())
        .reflexo(raio::cartao())
        .child(
            h_flex()
                .gap(esp::E6)
                .child(div().rotulo().text_color(Cores::forte()).child("Passos"))
                .child(div().font_family(FONTE_MONO).metadado().text_color(Cores::apagado()).child(format!("{feitos} de {}", lista.len()))),
        )
        .child(div().flex().flex_col().gap(esp::E6).children(linhas))
        .into_any_element()
}
