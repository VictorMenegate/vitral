//! O que o Mestre mostra fora do texto: os cartões da conversa dele (`Item::Cartao`, no molde do
//! cartão do plano) — "Vou dividir assim", a sugestão de projeto e a memória, abertos com os botões
//! ou recolhidos numa linha depois de decididos —, as linhas do que ele espera de você ("Esperando
//! você" e o sino), o "Resumo do Mestre" e a faixa "Enquanto você esteve fora".

use gpui_kit::component::{Disableable as _, ElementExt as _, Icon, WindowExt as _, h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        CartaoDeLista, Papel as Quem, QuadradoDeProjeto, Selo, Sinal, Simbolo, TamanhoBotao, Tom, aviso, botao_fantasma,
        botao_icone, botao_primario, contagem, pilula_da_tarefa,
    },
    icone_projeto,
};
use crate::{
    modelo::{Cartao, Chat, Divisao, EstadoCartao, ID_MESTRE, Item, Papel, Parte, SugestaoProjeto, TipoCartao},
    motor::{Motor, PrecisaDeVoce},
    tema::{Cores, Lucide, Tipo as _, dentro_da_borda, esp, r, raio, sombra_elevado},
};

/// Uma linha do que o Mestre espera de você: um cartão aberto ou um par de chats parado no freio
/// de ciclo.
pub(super) struct EsperaDoMestre {
    /// Única entre as linhas ("cartao-3", "ciclo-mestre-12").
    pub chave: String,
    pub cartao: Option<u64>,
    /// O chat a abrir (o do par que não é o Mestre; no cartão, o Mestre).
    pub chat: String,
    pub texto: String,
}

/// "Divisão 3 espera Começar", "Criar o projeto Receitas?", "Quer lembrar: …".
pub(super) fn frase_do_cartao(c: &Cartao) -> String {
    match &c.tipo {
        TipoCartao::Divisao(d) => {
            let n = d.partes.iter().filter(|p| !p.removida).count();
            format!("Divisão {} espera Começar ({})", c.id, contagem(n, "parte", "partes"))
        }
        TipoCartao::Projeto(s) => format!("Criar o projeto {}?", s.nome),
        TipoCartao::Memoria { texto } => format!("Quer lembrar: {}", uma_linha(texto)),
        TipoCartao::Pr { tarefa } => format!("PR da tarefa {tarefa}"),
        TipoCartao::Confirmar(c) => format!("Confirmar: {}", uma_linha(&c.resumo)),
        TipoCartao::Desconhecido(_) => format!("Cartão {} aberto", c.id),
    }
}

/// O número com que o usuário vê o chat ("12"), ou o título quando não há.
fn numero(c: &Chat) -> String {
    match c.tarefa {
        Some(n) => n.to_string(),
        None => c.id.clone(),
    }
}

/// "chat 12 “Checkout”" (o Mestre fica "Mestre").
pub(super) fn nome_do_chat(c: &Chat) -> String {
    if c.papel == Papel::Mestre {
        "Mestre".into()
    } else {
        format!("chat {} “{}”", numero(c), c.titulo)
    }
}

fn uma_linha(texto: &str) -> String {
    let linha = texto.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    if linha.chars().count() > 120 { format!("{}…", linha.chars().take(120).collect::<String>()) } else { linha.to_string() }
}

/// O que o Mestre espera de você: os cartões abertos (na ordem) e os pares travados no freio.
pub(super) fn esperas_do_mestre(motor: &Motor) -> Vec<EsperaDoMestre> {
    let Some(mestre) = motor.mestre_id().map(str::to_string) else {
        return Vec::new();
    };
    let mut linhas: Vec<EsperaDoMestre> = motor
        .cartoes()
        .iter()
        .filter(|c| c.aberto() && c.chat == ID_MESTRE)
        .map(|c| EsperaDoMestre { chave: format!("cartao-{}", c.id), cartao: Some(c.id), chat: mestre.clone(), texto: frase_do_cartao(c) })
        .collect();
    for (a, b) in motor.ciclos_travados() {
        let (Some(ca), Some(cb)) = (motor.chat(&a), motor.chat(&b)) else {
            continue;
        };
        let alvo = if ca.papel == Papel::Mestre { cb } else { ca };
        linhas.push(EsperaDoMestre {
            chave: format!("ciclo-{a}-{b}"),
            cartao: None,
            chat: alvo.id.clone(),
            texto: format!("Muitas mensagens seguidas entre {} e {}: escreva para um dos dois", nome_do_chat(ca), nome_do_chat(cb)),
        });
    }
    linhas
}

/// O que precisa de você em tudo, com o Mestre (os cartões abertos e a espera dele contam como uma
/// família) e cada par travado no freio de ciclo.
pub(super) fn precisa_com_o_mestre(motor: &Motor) -> PrecisaDeVoce {
    let mut conta = motor.precisa_de_voce(None);
    if let Some(mestre) = motor.mestre_id().and_then(|id| motor.chat(id)) {
        let espera = motor.espera_do_chat(mestre).filter(|e| *e != crate::motor::Espera::Nova);
        let cartoes = motor.cartoes().iter().any(|c| c.aberto());
        match espera {
            Some(e) => conta.somar(e, e == crate::motor::Espera::Erro),
            None if cartoes => conta.somar(crate::motor::Espera::Pedido, false),
            None => {}
        }
    }
    for _ in motor.ciclos_travados() {
        conta.somar(crate::motor::Espera::Pedido, false);
    }
    conta
}

/// A parte da divisão que criou (ou recebeu) o chat, com o número do cartão.
pub(super) fn parte_do_chat<'a>(motor: &'a Motor, chat: &str) -> Option<(u64, &'a Parte)> {
    motor.cartoes().iter().rev().find_map(|c| {
        let d = c.divisao()?;
        d.partes.iter().find(|p| p.criada.as_ref().is_some_and(|criada| criada.chat == chat)).map(|p| (c.id, p))
    })
}

/// O chat é do Mestre: ele criou, ou o pedido aberto é dele. A dica diz qual parte.
pub(super) fn do_mestre(motor: &Motor, chat: &Chat) -> Option<String> {
    let dono = chat.tarefa.and_then(|t| motor.tarefa(t)).is_some_and(|t| t.coordenador == ID_MESTRE);
    if chat.criado_por != ID_MESTRE && !dono {
        return None;
    }
    Some(match parte_do_chat(motor, &chat.id) {
        Some((cartao, p)) => format!("Criado pelo Mestre: {} (divisão {cartao})", p.titulo),
        None if dono => "Pedido do Mestre aberto neste chat".into(),
        None => "Criado pelo Mestre".into(),
    })
}

/// As respostas do Mestre que vêm logo depois do aviso de que uma divisão terminou: ganham o
/// rótulo "Resumo do Mestre". O turno que ainda roda fica de fora.
pub(super) fn resumos_do_mestre(itens: &[Item], rodando: bool) -> Vec<(usize, u64)> {
    let mut resumos = Vec::new();
    let mut i = 0;
    while i < itens.len() {
        let divisao = match &itens[i] {
            Item::Entrega { de, texto, .. } if de == "Vitral" => divisao_terminada(texto),
            _ => None,
        };
        i += 1;
        let Some(divisao) = divisao else {
            continue;
        };
        // O que entrou junto no mesmo turno (as notas) vem logo depois; a resposta é o último texto
        // antes da próxima mensagem.
        while i < itens.len() && matches!(itens[i], Item::Entrega { .. }) {
            i += 1;
        }
        let mut ultimo = None;
        let mut j = i;
        while j < itens.len() && !matches!(itens[j], Item::Usuario { .. } | Item::Entrega { .. }) {
            if matches!(itens[j], Item::Assistente { .. }) {
                ultimo = Some(j);
            }
            j += 1;
        }
        if let Some(ultimo) = ultimo.filter(|_| !(rodando && j == itens.len())) {
            resumos.push((ultimo, divisao));
        }
        i = j;
    }
    resumos
}

/// "A divisão 3 terminou: …" (com ou sem o "[Vitral] " na frente) dá o 3.
fn divisao_terminada(texto: &str) -> Option<u64> {
    let resto = texto.trim_start().trim_start_matches("[Vitral]").trim_start().strip_prefix("A divisão ")?;
    let fim = resto.find(|c: char| !c.is_ascii_digit())?;
    resto[fim..].starts_with(" terminou").then(|| resto[..fim].parse().ok()).flatten()
}

/// O quadrado do projeto de uma parte, ou o balão dos Avulsos.
fn simbolo_do_projeto(motor: &Motor, projeto: &str, lado: f32) -> AnyElement {
    if projeto.is_empty() {
        return div()
            .flex_none()
            .size(r(lado))
            .flex()
            .items_center()
            .justify_center()
            .rounded(raio::do_icone(lado))
            .bg(Cores::hover())
            .child(svg().path(Lucide::MessageCircle.path()).size(r((lado * 18. / 32.).max(12.))).text_color(Cores::tenue()))
            .into_any_element();
    }
    let (icone, cor) = icone_projeto::visual_do(motor, projeto);
    QuadradoDeProjeto::new(icone, cor, lado).into_any_element()
}

/// A moldura de um cartão aberto: o elevado com a borda na tinta da cor.
fn moldura(cor: Hsla) -> Div {
    v_flex()
        .w_full()
        .rounded(raio::CARTAO)
        .bg(Cores::cartao_elevado())
        .border_1()
        .border_color(Cores::tinta_borda(cor))
        .shadow(sombra_elevado())
}

/// O cabeçalho tingido: o ícone na cor, o título e, embaixo, o detalhe.
fn cabecalho(icone: Lucide, cor: Hsla, titulo: impl Into<SharedString>, detalhe: Option<String>) -> Div {
    h_flex()
        .flex_none()
        .min_h(r(44.))
        .px(dentro_da_borda(12.))
        .py(esp::E6)
        .gap(esp::E8)
        .bg(Cores::tinta(cor))
        .rounded_t(raio::CARTAO - px(1.))
        .child(Icon::new(icone).size(r(16.)).flex_none().text_color(if cor == Cores::destaque() { Cores::destaque_texto() } else { cor }))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().titulo_cartao().text_color(Cores::forte()).truncate().child(titulo.into()))
                .children(detalhe.map(|d| div().metadado().text_color(Cores::tenue()).truncate().child(d))),
        )
}

/// A fileira de botões do pé do cartão.
fn rodape() -> Div {
    h_flex().flex_wrap().gap(esp::E8).px(dentro_da_borda(12.)).pt(esp::E12).pb(dentro_da_borda(12.))
}

/// O cartão decidido, numa linha: o ícone, o que aconteceu e, à direita, o detalhe.
fn linha_decidida(id: SharedString, icone: Lucide, cor: Hsla, texto: String, detalhe: Option<String>, aberto: Option<bool>) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .min_h(r(36.))
        .px(dentro_da_borda(12.))
        .gap(esp::E8)
        .rounded(raio::CARTAO)
        .bg(Cores::cartao_elevado())
        .border_1()
        .border_color(Cores::linha())
        .when(aberto.is_some(), |s| s.cursor_pointer().hover(|s| s.bg(Cores::hover())))
        .child(Icon::new(icone).size(r(14.)).flex_none().text_color(cor))
        .child(div().flex_1().min_w_0().truncate().corpo_ui().text_color(Cores::texto()).child(texto))
        .children(detalhe.map(|d| div().flex_none().metadado().text_color(Cores::tenue()).child(d)))
        .children(aberto.map(|a| {
            Icon::new(if a { Lucide::ChevronDown } else { Lucide::ChevronRight }).size(r(14.)).flex_none().text_color(Cores::tenue())
        }))
}

impl Janela {
    /// Um `Item::Cartao` da conversa do Mestre, conforme o tipo e o estado. Com o "Ver" de
    /// "Esperando você" pendente para ele, rola o painel até o cartão.
    pub(super) fn cartao_do_mestre(
        &self,
        motor: &Motor,
        painel: u64,
        rolagem: &ScrollHandle,
        id: u64,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let elemento = match motor.cartao(id) {
            None => linha_decidida(format!("cartao-{id}").into(), Lucide::Info, Cores::tenue(), format!("Cartão {id}"), None, None)
                .into_any_element(),
            Some(c) => match (&c.tipo, c.estado) {
                (TipoCartao::Divisao(d), EstadoCartao::Aberto) => self.divisao_aberta(motor, painel, c, d, window, cx),
                (TipoCartao::Divisao(d), _) => self.divisao_decidida(motor, c, d, window, cx),
                (TipoCartao::Projeto(s), EstadoCartao::Aberto) => self.sugestao_aberta(c, s, window, cx),
                (TipoCartao::Projeto(s), estado) => {
                    let criado = s.criado.clone().filter(|_| estado == EstadoCartao::Aceito);
                    let (icone, cor, texto) = match &criado {
                        Some(p) => (Lucide::FolderPlus, Cores::verde(), format!("Projeto {} criado", motor.nome_do_projeto(p))),
                        None => (Lucide::X, Cores::tenue(), format!("Sugestão do projeto {} recusada", s.nome)),
                    };
                    let linha = linha_decidida(format!("cartao-{id}").into(), icone, cor, texto, None, None);
                    match criado {
                        Some(p) => linha
                            .cursor_pointer()
                            .hover(|s| s.bg(Cores::hover()))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.abrir_vista(super::Vista::Projeto(p.clone()), window, cx)
                            }))
                            .into_any_element(),
                        None => linha.into_any_element(),
                    }
                }
                (TipoCartao::Memoria { texto }, EstadoCartao::Aberto) => self.memoria_aberta(c.id, texto, window, cx),
                (TipoCartao::Memoria { texto }, estado) => {
                    let (icone, cor, frase) = if estado == EstadoCartao::Aceito {
                        (Lucide::Brain, Cores::verde(), format!("Lembrado: {}", uma_linha(texto)))
                    } else {
                        (Lucide::X, Cores::tenue(), format!("Não lembrado: {}", uma_linha(texto)))
                    };
                    linha_decidida(format!("cartao-{id}").into(), icone, cor, frase, None, None).into_any_element()
                }
                // Provisório (G5): a linha de acompanhamento e o cartão de confirmação de verdade.
                (TipoCartao::Pr { tarefa }, _) => {
                    let texto = match motor.tarefa(*tarefa).and_then(|t| t.pr.as_ref()) {
                        Some(pr) => format!("PR #{} · {}", pr.numero, pr.titulo),
                        None => "PR (sem dados)".into(),
                    };
                    linha_decidida(format!("cartao-{id}").into(), Lucide::GitPullRequest, Cores::tenue(), texto, None, None)
                        .into_any_element()
                }
                (TipoCartao::Confirmar(confirmacao), _) => {
                    let texto = match &confirmacao.resultado {
                        Some(resultado) => format!("{} · {resultado}", uma_linha(&confirmacao.resumo)),
                        None => format!("Confirmar: {}", uma_linha(&confirmacao.resumo)),
                    };
                    linha_decidida(format!("cartao-{id}").into(), Lucide::ShieldQuestionMark, Cores::ambar(), texto, None, None)
                        .into_any_element()
                }
                (TipoCartao::Desconhecido(_), _) => {
                    linha_decidida(format!("cartao-{id}").into(), Lucide::Info, Cores::tenue(), "(cartão de uma versão mais nova)".into(), None, None)
                        .into_any_element()
                }
            },
        };
        if self.rolar_ao_cartao != Some((painel, id)) {
            return elemento;
        }
        let (rolagem, janela) = (rolagem.clone(), cx.entity().downgrade());
        div()
            .on_prepaint(move |limites, window, _| {
                // O cartão fica a 16 do topo das mensagens, sem passar do fim.
                let vista = rolagem.bounds();
                let atual = rolagem.offset();
                let alvo = (atual.y - (limites.origin.y - vista.origin.y) + px(16.)).clamp(-rolagem.max_offset().y, px(0.));
                rolagem.set_offset(point(atual.x, alvo));
                window.on_next_frame(move |_, cx| {
                    let _ = janela.update(cx, |this, cx| {
                        this.rolar_ao_cartao = None;
                        cx.notify();
                    });
                });
            })
            .child(elemento)
            .into_any_element()
    }

    /// "Pedir ao Mestre" (a lista vazia de um projeto): o Mestre no painel ativo com o texto na
    /// caixa, depois do que já estava escrito nela.
    pub(super) fn pedir_ao_mestre(&mut self, texto: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.abrir_mestre(window, cx);
        let Some(painel) = self.paineis.get(self.ativo).filter(|p| p.chat.as_deref() == Some(ID_MESTRE)) else {
            return;
        };
        let (id, antes) = (painel.id, painel.composer.read(cx).value().trim_end().to_string());
        let junto = if antes.is_empty() { texto.to_string() } else { format!("{antes} {texto}") };
        self.escrever_na_caixa(id, &junto, window, cx);
    }

    /// "Ver" num cartão do Mestre (em "Esperando você", no sino): abre o Mestre rolado até ele.
    pub(super) fn ver_cartao(&mut self, cartao: Option<u64>, chat: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.selecionar(Some(chat.to_string()), window, cx);
        let Some(cartao) = cartao else {
            return;
        };
        if let Some(painel) = self.paineis.get_mut(self.ativo) {
            painel.seguindo = false;
            self.rolar_ao_cartao = Some((painel.id, cartao));
        }
        cx.notify();
    }

    /// "Vou dividir assim": o resumo, uma linha por parte (o × tira e desfaz; o clique abre o
    /// pedido inteiro com os critérios), as perguntas do Mestre e Descartar, Ajustar e Começar.
    fn divisao_aberta(&self, motor: &Motor, painel: u64, c: &Cartao, d: &Divisao, window: &Window, cx: &Context<Self>) -> AnyElement {
        let id = c.id;
        let ficam = d.partes.iter().filter(|p| !p.removida).count();
        let detalhe = format!(
            "Divisão {id} · {}{}",
            contagem(ficam, "parte", "partes"),
            if ficam < d.partes.len() { format!(" (de {})", d.partes.len()) } else { String::new() }
        );
        let partes: Vec<AnyElement> = d.partes.iter().enumerate().map(|(i, p)| self.parte_aberta(motor, id, i, p, window, cx)).collect();
        let perguntas = (!d.perguntas.is_empty()).then(|| {
            v_flex()
                .gap(esp::E4)
                .px(dentro_da_borda(12.))
                .pt(esp::E12)
                .child(div().rotulo().text_color(Cores::tenue()).child("Antes de começar, o Mestre pergunta:"))
                .children(d.perguntas.iter().map(|q| div().corpo_ui().text_color(Cores::texto()).child(format!("• {q}"))))
        });
        let motor_e = self.motor.clone();
        let (m1, m2) = (motor_e.clone(), motor_e);
        let descartar = botao_fantasma(("descartar-divisao", id as usize), Some(Lucide::X.into()), "Descartar", TamanhoBotao::Controle, window, cx)
            .tooltip("O Mestre fica sabendo; nada começa")
            .on_click(move |_, window, cx| {
                if let Some(m) = &m1 {
                    m.update(cx, |m, cx| m.descartar_cartao(id, cx));
                    window.push_notification(aviso(Tom::Info, format!("Divisão {id} descartada")), cx);
                }
            });
        let ajustar = botao_fantasma(("ajustar-divisao", id as usize), Some(Lucide::Pencil.into()), "Ajustar", TamanhoBotao::Controle, window, cx)
            .tooltip("Escreva ao Mestre o que mudar; a próxima divisão substitui esta")
            .on_click(cx.listener(move |this, _, window, cx| this.escrever_na_caixa(painel, &format!("Ajuste na divisão {id}: "), window, cx)));
        let comecar = botao_primario(("comecar-divisao", id as usize), Some(Lucide::Play.into()), "Começar", TamanhoBotao::Controle, window, cx)
            .when(ficam == 0, |b| b.disabled(true).opacity(0.45).tooltip("Todas as partes foram tiradas"))
            .when(ficam > 0, |b| b.tooltip("Cria os chats (ou manda ao chat indicado) e manda o pedido de cada parte"))
            .on_click(move |_, window, cx| {
                let Some(m) = &m2 else {
                    return;
                };
                match m.update(cx, |m, cx| m.comecar_divisao(id, cx)) {
                    Ok(texto) => window.push_notification(aviso(Tom::Pronto, format!("Começada: {texto}")), cx),
                    Err(erro) => window.push_notification(aviso(Tom::Atencao, erro), cx),
                }
            });
        moldura(Cores::destaque())
            .child(cabecalho(Lucide::GitFork, Cores::destaque(), "Vou dividir assim", Some(detalhe)))
            .when(!d.resumo.trim().is_empty(), |s| {
                s.child(div().px(dentro_da_borda(12.)).pt(esp::E12).corpo_ui().text_color(Cores::texto()).child(d.resumo.trim().to_string()))
            })
            .child(v_flex().gap(esp::E4).px(dentro_da_borda(8.)).pt(esp::E8).children(partes))
            .children(perguntas)
            .child(rodape().child(descartar).child(div().flex_1()).child(ajustar).child(comecar))
            .into_any_element()
    }

    /// Uma parte do cartão aberto: o projeto, para onde vai, o pedido em duas linhas (o clique
    /// abre tudo), o aviso e o × (ou Desfazer, na tirada).
    fn parte_aberta(&self, motor: &Motor, cartao: u64, i: usize, p: &Parte, window: &Window, cx: &Context<Self>) -> AnyElement {
        let chave = format!("{ID_MESTRE}:cartao-{cartao}-parte-{i}");
        let aberta = self.expandidos.contains(&chave);
        let projeto = motor.nome_do_projeto(&p.projeto);
        let destino = match p.chat.as_deref().and_then(|c| motor.chat(c)) {
            Some(c) => format!("→ chat {} “{}”", numero(c), c.titulo),
            None => format!("→ chat novo “{}”", p.titulo),
        };
        let motor_e = self.motor.clone();
        let removida = p.removida;
        let alternar_parte = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            cx.stop_propagation();
            if let Some(m) = &motor_e
                && let Err(erro) = m.update(cx, |m, cx| m.tirar_parte(cartao, i, !removida, cx))
            {
                window.push_notification(aviso(Tom::Atencao, erro), cx);
            }
        };
        let botao = if removida {
            botao_fantasma(SharedString::from(format!("desfazer-{cartao}-{i}")), Some(Lucide::Undo2.into()), "Desfazer", TamanhoBotao::Linha, window, cx)
                .on_click(alternar_parte)
                .into_any_element()
        } else {
            botao_icone(SharedString::from(format!("tirar-{cartao}-{i}")), Lucide::X, "Tirar esta parte da divisão", TamanhoBotao::Linha, window, cx)
                .on_click(alternar_parte)
                .into_any_element()
        };
        let resumo_criterios = {
            let mut partes = vec![contagem(p.criterios.len(), "critério", "critérios")];
            if let Some(v) = p.verificacao.as_deref().filter(|v| !v.trim().is_empty()) {
                partes.push(format!("verificação: {}", uma_linha(v)));
            }
            partes.join(" · ")
        };
        let detalhes = aberta.then(|| {
            v_flex()
                .gap(esp::E4)
                .pt(esp::E4)
                .child(div().corpo_ui().text_color(Cores::texto()).child(p.pedido.trim().to_string()))
                .when(!p.criterios.is_empty(), |s| {
                    s.child(div().pt(esp::E4).rotulo().text_color(Cores::tenue()).child("Critérios"))
                        .children(p.criterios.iter().enumerate().map(|(n, c)| {
                            div().corpo_ui().text_color(Cores::texto()).child(format!("{}. {c}", n + 1))
                        }))
                })
                .when_some(p.verificacao.clone().filter(|v| !v.trim().is_empty()), |s, v| {
                    s.child(
                        h_flex()
                            .gap(esp::E6)
                            .pt(esp::E4)
                            .child(div().flex_none().rotulo().text_color(Cores::tenue()).child("Verificação"))
                            .child(div().min_w_0().truncate().mono_pequeno().text_color(Cores::codigo()).child(v)),
                    )
                })
                .when_some(p.pasta.as_ref(), |s, pasta| {
                    s.child(div().metadado().text_color(Cores::tenue()).child(format!("Pasta: {}", super::dialogos::com_til(pasta))))
                })
                .when(!p.arquivos.is_empty(), |s| {
                    s.child(div().metadado().text_color(Cores::tenue()).child(format!("Arquivos previstos: {}", p.arquivos.join(", "))))
                })
        });
        let titulo = h_flex()
            .gap(esp::E6)
            .min_w_0()
            .child(div().flex_none().max_w(r(140.)).truncate().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child(projeto))
            .child(div().flex_1().min_w_0().truncate().corpo_ui().text_color(Cores::texto()).when(removida, |d| d.line_through()).child(destino));
        let corpo = v_flex()
            .id(SharedString::from(format!("abrir-parte-{cartao}-{i}")))
            .flex_1()
            .min_w_0()
            .cursor_pointer()
            .child(titulo)
            .when(!aberta, |s| {
                s.child(div().metadado().text_color(Cores::tenue()).line_clamp(2).text_ellipsis().child(uma_linha(&p.pedido)))
                    .child(div().metadado().text_color(Cores::apagado()).truncate().child(resumo_criterios.clone()))
            })
            .children(detalhes)
            .when_some(p.aviso.clone(), |s, a| {
                s.child(
                    h_flex()
                        .gap(esp::E4)
                        .pt(esp::E2)
                        .items_start()
                        .child(div().flex_none().pt(esp::E2).child(Icon::new(Lucide::TriangleAlert).size(r(12.)).text_color(Cores::ambar())))
                        .child(div().min_w_0().metadado().text_color(Cores::ambar()).child(a)),
                )
            })
            .on_click(super::mensagens::alternar(chave.clone(), cx));
        h_flex()
            .items_start()
            .gap(esp::E8)
            .p(esp::E4)
            .rounded(raio::CONTROLE)
            .hover(|s| s.bg(Cores::hover()))
            .when(removida, |s| s.opacity(0.55))
            .child(div().flex_none().pt(esp::E2).child(simbolo_do_projeto(motor, &p.projeto, 20.)))
            .child(corpo)
            .child(div().flex_none().child(botao))
            .into_any_element()
    }

    /// A divisão decidida, numa linha ("Divisão 3 começada · 2 partes"); o clique abre o que foi
    /// para cada chat, com a pílula do estado de cada um.
    fn divisao_decidida(&self, motor: &Motor, c: &Cartao, d: &Divisao, window: &Window, cx: &Context<Self>) -> AnyElement {
        let id = c.id;
        let chave = format!("{ID_MESTRE}:cartao-{id}");
        let aberto = self.expandidos.contains(&chave);
        let comecadas: Vec<&Parte> = d.partes.iter().filter(|p| p.criada.is_some()).collect();
        let n = d.partes.iter().filter(|p| !p.removida).count();
        let (icone, cor, texto) = match c.estado {
            EstadoCartao::Aceito if d.por_referencia && comecadas.len() == 1 => {
                let destino = comecadas[0].criada.as_ref().and_then(|cr| motor.chat(&cr.chat)).map(nome_do_chat).unwrap_or_default();
                (Lucide::CircleCheck, Cores::verde(), format!("Mandado direto ao {destino}"))
            }
            EstadoCartao::Aceito if d.sem_perguntar => (Lucide::CircleCheck, Cores::verde(), format!("Divisão {id} começada direto · {}", contagem(n, "parte", "partes"))),
            EstadoCartao::Aceito => (Lucide::CircleCheck, Cores::verde(), format!("Divisão {id} começada · {}", contagem(n, "parte", "partes"))),
            EstadoCartao::Recusado => (Lucide::X, Cores::tenue(), format!("Divisão {id} descartada")),
            _ => (Lucide::RotateCcw, Cores::tenue(), format!("Divisão {id} substituída por outra")),
        };
        let terminadas = comecadas.iter().filter(|p| p.resultado.is_some()).count();
        let detalhe = (c.estado == EstadoCartao::Aceito && !comecadas.is_empty()).then(|| {
            if terminadas == comecadas.len() { "terminou".to_string() } else { format!("{terminadas} de {} terminaram", comecadas.len()) }
        });
        let linha = linha_decidida(format!("cartao-{id}").into(), icone, cor, texto, detalhe, Some(aberto)).on_click(super::mensagens::alternar(chave, cx));
        if !aberto {
            return linha.into_any_element();
        }
        let partes = d.partes.iter().enumerate().map(|(i, p)| {
            let id_linha = SharedString::from(format!("cartao-{id}-feita-{i}"));
            let criada = p.criada.as_ref().and_then(|cr| Some((motor.chat(&cr.chat)?, motor.tarefa(cr.tarefa))));
            let titulo = match criada {
                Some((chat, _)) => format!("{} · {}", motor.nome_do_projeto(&p.projeto), nome_do_chat(chat)),
                None => format!("{} · “{}”", motor.nome_do_projeto(&p.projeto), p.titulo),
            };
            let situacao: AnyElement = match (&criada, &p.resultado, &p.falha) {
                (_, Some(resultado), _) => {
                    let texto = match &p.detalhe {
                        Some(detalhe) => format!("{resultado} {detalhe}"),
                        None => resultado.clone(),
                    };
                    div().truncate().child(texto).into_any_element()
                }
                (Some((_, Some(t))), None, _) => pilula_da_tarefa(motor, t).compacta().into_any_element(),
                (None, None, Some(falha)) => div().truncate().text_color(Cores::vermelho()).child(format!("não começou: {falha}")).into_any_element(),
                (None, None, None) if p.removida => div().child("tirada antes de começar").into_any_element(),
                _ => div().child("não começou").into_any_element(),
            };
            let mut cartao = CartaoDeLista::new(id_linha, titulo)
                .simbolo(Simbolo::Elemento(simbolo_do_projeto(motor, &p.projeto, 30.)))
                .subtitulo(situacao);
            if let Some((chat, _)) = criada {
                let alvo = chat.id.clone();
                cartao = cartao.on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                    if ev.modifiers().control {
                        this.abrir_ao_lado(alvo.clone(), None, window, cx)
                    } else {
                        this.selecionar(Some(alvo.clone()), window, cx)
                    }
                }));
            }
            div().mt(esp::E2).child(cartao).into_any_element()
        });
        let _ = window;
        v_flex().w_full().child(linha).child(v_flex().px(esp::E4).pt(esp::E2).children(partes)).into_any_element()
    }

    /// "Criar o projeto “X”?": o motivo, a descrição e as pastas, com Agora não e Criar projeto…
    /// (o formulário vem preenchido).
    fn sugestao_aberta(&self, c: &Cartao, s: &SugestaoProjeto, window: &Window, cx: &Context<Self>) -> AnyElement {
        let id = c.id;
        let motor_e = self.motor.clone();
        let agora_nao = botao_fantasma(("sugestao-nao", id as usize), None, "Agora não", TamanhoBotao::Controle, window, cx)
            .on_click(move |_, _, cx| {
                if let Some(m) = &motor_e {
                    let tarefa = m.update(cx, |m, cx| m.responder_sugestao(id, None, cx));
                    tarefa.detach();
                }
            });
        let criar = botao_primario(("sugestao-criar", id as usize), Some(Lucide::FolderPlus.into()), "Criar projeto…", TamanhoBotao::Controle, window, cx)
            .on_click(cx.listener(move |this, _, window, cx| this.dialogo_projeto_da_sugestao(id, window, cx)));
        let pastas = if s.pastas.is_empty() {
            "Pasta própria (os arquivos ficam nos dados do Vitral)".to_string()
        } else {
            s.pastas.iter().map(|p| super::dialogos::com_til(p)).collect::<Vec<_>>().join(", ")
        };
        moldura(Cores::destaque())
            .child(cabecalho(Lucide::FolderPlus, Cores::destaque(), format!("Criar o projeto “{}”?", s.nome), Some("Sugestão do Mestre".into())))
            .child(
                v_flex()
                    .gap(esp::E4)
                    .px(dentro_da_borda(12.))
                    .pt(esp::E12)
                    .when(!s.motivo.trim().is_empty(), |d| d.child(div().corpo_ui().text_color(Cores::texto()).child(s.motivo.trim().to_string())))
                    .when(!s.descricao.trim().is_empty(), |d| {
                        d.child(div().metadado().text_color(Cores::tenue()).child(format!("Descrição: {}", s.descricao.trim())))
                    })
                    .child(div().metadado().text_color(Cores::tenue()).child(format!("Pastas: {pastas}"))),
            )
            .child(rodape().child(div().flex_1()).child(agora_nao).child(criar))
            .into_any_element()
    }

    /// "O Mestre quer lembrar": o texto e Não e Lembrar.
    fn memoria_aberta(&self, id: u64, texto: &str, window: &Window, cx: &Context<Self>) -> AnyElement {
        let responder = |aceitar: bool| {
            let motor_e = self.motor.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                if let Some(m) = &motor_e {
                    match m.update(cx, |m, cx| m.responder_memoria(id, aceitar, cx)) {
                        Ok(()) if aceitar => window.push_notification(aviso(Tom::Pronto, "Lembrado"), cx),
                        Ok(()) => {}
                        Err(erro) => window.push_notification(aviso(Tom::Atencao, erro), cx),
                    }
                }
            }
        };
        let nao = botao_fantasma(("memoria-nao", id as usize), None, "Não", TamanhoBotao::Controle, window, cx).on_click(responder(false));
        let lembrar = botao_primario(("memoria-sim", id as usize), Some(Lucide::Check.into()), "Lembrar", TamanhoBotao::Controle, window, cx)
            .on_click(responder(true));
        moldura(Cores::destaque())
            .child(cabecalho(Lucide::Brain, Cores::destaque(), "O Mestre quer lembrar", Some("Vai para a memória do Mestre (Memória do Mestre… no menu do Vitral)".into())))
            .child(
                div()
                    .mx(dentro_da_borda(12.))
                    .mt(esp::E12)
                    .pl(esp::E12)
                    .border_l_2()
                    .border_color(Cores::destaque_borda())
                    .corpo_ui()
                    .text_color(Cores::texto())
                    .child(texto.trim().to_string()),
            )
            .child(rodape().child(div().flex_1()).child(nao).child(lembrar))
            .into_any_element()
    }

    /// As linhas do Mestre em "Esperando você" (a visão geral): o círculo do Mestre, o que ele
    /// espera e Ver.
    pub(super) fn linhas_do_mestre(&self, esperas: Vec<EsperaDoMestre>, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        esperas
            .into_iter()
            .map(|e| {
                let (cartao, chat) = (e.cartao, e.chat.clone());
                let (cartao_v, chat_v) = (e.cartao, e.chat.clone());
                let ver = botao_fantasma(SharedString::from(format!("ver-{}", e.chave)), None, "Ver", TamanhoBotao::Linha, window, cx)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.ver_cartao(cartao_v, &chat_v, window, cx)
                    }));
                let sinal = Sinal::Pedido;
                let subtitulo = h_flex()
                    .gap(esp::E6)
                    .min_w_0()
                    .child(div().flex_none().child(sinal.ponto(6.)))
                    .child(div().flex_1().min_w_0().truncate().child(e.texto));
                let cartao_lista = CartaoDeLista::new(SharedString::from(format!("mestre-{}", e.chave)), "Mestre")
                    .simbolo(Simbolo::Papel(Quem::Mestre))
                    .subtitulo(subtitulo)
                    .acao_fixa(ver)
                    .on_click(cx.listener(move |this, _, window, cx| this.ver_cartao(cartao, &chat, window, cx)));
                div().mt(esp::E2).child(cartao_lista).into_any_element()
            })
            .collect()
    }

    /// "Enquanto você esteve fora": o que terminou, o que precisa de você e o que falhou ou parou
    /// desde que você saiu, com as linhas que abrem o chat, "Pedir ao Mestre um resumo" e o ×. Só
    /// na memória: some no × e é trocada na volta seguinte.
    pub(super) fn faixa_do_resumo(&self, motor: &Motor, onde: &'static str, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let resumo = motor.resumo_volta.as_ref()?;
        let desde = crate::modelo::quando_local(resumo.desde);
        let grupos = [("terminou", "Terminou", Cores::verde()), ("precisa", "Precisa de você", Cores::ambar()), ("falhou", "Falhou ou parou", Cores::vermelho())];
        let mut corpo = v_flex().gap(esp::E2);
        for (grupo, titulo, cor) in grupos {
            let linhas: Vec<_> = resumo.linhas.iter().filter(|l| l.grupo == grupo).collect();
            if linhas.is_empty() {
                continue;
            }
            corpo = corpo.child(
                h_flex()
                    .gap(esp::E6)
                    .pt(esp::E4)
                    .child(div().flex_none().size(r(6.)).rounded_full().bg(cor))
                    .child(div().rotulo().text_color(Cores::tenue()).child(titulo))
                    .child(div().metadado().text_color(Cores::apagado()).child(linhas.len().to_string())),
            );
            let mostradas = linhas.len().min(4);
            for (i, l) in linhas.iter().take(mostradas).enumerate() {
                let chat = (!l.chat.is_empty() && motor.chat(&l.chat).is_some()).then(|| l.chat.clone());
                let simbolo = if l.chat == ID_MESTRE {
                    Quem::Mestre.circulo(16.).into_any_element()
                } else {
                    simbolo_do_projeto(motor, &l.projeto, 16.)
                };
                corpo = corpo.child(
                    h_flex()
                        .id(SharedString::from(format!("resumo-{onde}-{grupo}-{i}")))
                        .min_h(r(28.))
                        .px(esp::E4)
                        .gap(esp::E8)
                        .rounded(raio::CONTROLE)
                        .when(chat.is_some(), |s| s.cursor_pointer().hover(|s| s.bg(Cores::hover())))
                        .child(div().flex_none().child(simbolo))
                        .child(div().flex_1().min_w_0().truncate().metadado().text_color(Cores::texto()).child(uma_linha(&l.texto)))
                        .when_some(chat, |s, chat| {
                            s.on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                                if ev.modifiers().control {
                                    this.abrir_ao_lado(chat.clone(), None, window, cx)
                                } else {
                                    this.selecionar(Some(chat.clone()), window, cx)
                                }
                            }))
                        }),
                );
            }
            if linhas.len() > mostradas {
                corpo = corpo.child(div().pl(esp::E24).metadado().text_color(Cores::tenue()).child(format!("e mais {}", linhas.len() - mostradas)));
            }
        }
        let motor_e = self.motor.clone();
        let (m1, m2) = (motor_e.clone(), motor_e);
        let fechar = botao_icone(SharedString::from(format!("fechar-resumo-{onde}")), Lucide::X, "Fechar o resumo", TamanhoBotao::Linha, window, cx)
            .on_click(move |_, _, cx| {
                if let Some(m) = &m1 {
                    m.update(cx, |m, cx| m.fechar_resumo(cx));
                }
            });
        let pedir = botao_fantasma(SharedString::from(format!("pedir-resumo-{onde}")), Some(Lucide::Orbit.into()), "Pedir ao Mestre um resumo", TamanhoBotao::Barra, window, cx)
            .tooltip("O Mestre lê o histórico e o estado e responde por projeto")
            .on_click(move |_, _, cx| {
                if let Some(m) = &m2 {
                    m.update(cx, |m, cx| m.pedir_resumo_ao_mestre(cx));
                }
            });
        Some(
            v_flex()
                .w_full()
                .p(dentro_da_borda(12.))
                .gap(esp::E4)
                .rounded(raio::CARTAO)
                .bg(Cores::cartao_elevado())
                .border_1()
                .border_color(Cores::tinta_borda(Cores::destaque()))
                .child(
                    h_flex()
                        .gap(esp::E8)
                        .child(Icon::new(Lucide::Clock).size(r(14.)).flex_none().text_color(Cores::destaque_texto()))
                        .child(
                            div()
                                .id(SharedString::from(format!("titulo-resumo-{onde}")))
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .rotulo()
                                .text_color(Cores::forte())
                                .child("Enquanto você esteve fora")
                                .tooltip(move |window, cx| Tooltip::new(format!("Desde {desde}")).build(window, cx)),
                        )
                        .child(div().flex_none().child(fechar)),
                )
                .child(corpo)
                .child(h_flex().ml(r(-12.)).pt(esp::E4).child(pedir))
                .into_any_element(),
        )
    }
}

/// O que vai no canto de um chat do Mestre na lista: o Orbit de 10, com a dica da parte.
pub(super) fn selo_do_mestre(id: SharedString, dica: String) -> AnyElement {
    div()
        .id(id)
        .flex_none()
        .child(Icon::new(Lucide::Orbit).size(r(10.)).text_color(Cores::destaque_texto()))
        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        .into_any_element()
}

/// O selo clicável "do Mestre ↩" no cabeçalho de um chat que ele criou: abre o Mestre.
pub(super) fn selo_do_mestre_no_chat(id: SharedString, dica: String, cx: &Context<Janela>) -> AnyElement {
    let dica = format!("{dica}\nClique para abrir o Mestre");
    let janela = cx.entity().downgrade();
    div()
        .id(id)
        .flex_none()
        .cursor_pointer()
        .rounded_full()
        .hover(|s| s.bg(Cores::hover()))
        .child(Selo::cor("do Mestre ↩", Cores::destaque_texto()).icone(Lucide::Orbit, Cores::destaque_texto()))
        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            let _ = janela.update(cx, |this, cx| this.abrir_mestre(window, cx));
        })
        .into_any_element()
}

#[cfg(test)]
mod testes {
    use super::{divisao_terminada, resumos_do_mestre};
    use crate::modelo::Item;

    fn entrega(texto: &str) -> Item {
        Item::Entrega { de: "Vitral".into(), texto: texto.into(), ponto: None }
    }

    fn assistente(texto: &str) -> Item {
        Item::Assistente { texto: texto.into() }
    }

    #[test]
    fn o_resumo_do_mestre_e_a_resposta_ao_aviso_da_divisao() {
        assert_eq!(divisao_terminada("A divisão 3 terminou: chat 12 juntado"), Some(3));
        assert_eq!(divisao_terminada("[Vitral] A divisão 12 terminou: x"), Some(12));
        assert_eq!(divisao_terminada("A divisão 3 ainda espera o Começar."), None);
        let itens = vec![
            Item::Usuario { texto: "faça".into(), anexos: vec![], ponto: None, ultrathink: false, ultracode: false, referencias: Vec::new() },
            assistente("Divisão mostrada."),
            entrega("A divisão 3 terminou: chat 12 juntado."),
            entrega("Chat 12 entregou e foi juntado sozinho."),
            Item::Ferramenta { id: "t".into(), nome: "mcp__vitral__estado".into(), resumo: "estado".into(), resultado: None, erro: false },
            assistente("Olhando."),
            assistente("Resumo por projeto: Ninee pronto."),
            entrega("Outra coisa do Vitral."),
            assistente("Resposta comum."),
        ];
        assert_eq!(resumos_do_mestre(&itens, false), vec![(6, 3)]);
        // O turno que ainda roda não ganha o rótulo antes de terminar.
        let rodando = &itens[..7];
        assert_eq!(resumos_do_mestre(rodando, true), vec![]);
        assert_eq!(resumos_do_mestre(rodando, false), vec![(6, 3)]);
    }
}
