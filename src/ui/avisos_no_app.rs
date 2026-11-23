//! Quem espera você aparece onde você está olhando (dono, 16/11/2026: rodada de uso, item 3.1).
//! Com o Vitral na frente o dia inteiro, o pedido de permissão de um chat que não está em nenhum
//! painel aparecia só no pulso do sino, e o turno ficava parado minutos sem ninguém ver. Agora o
//! pedido, o plano e a falha de um chat fora de vista (a aba de fundo conta como fora) viram um
//! aviso no canto de baixo à direita, com a resposta ali mesmo. O aviso sai de `Motor::pendencias`
//! a cada desenho: fica até ser respondido, e some sozinho quando é respondido em outro lugar. Ele
//! nunca pega o foco, e nenhum botão dele responde ao Enter da caixa.

use std::collections::HashSet;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::{
    Janela, QualFlutuante,
    componentes::{Flutuante as _, TamanhoBotao, botao_fantasma, botao_icone, botao_primario, contagem},
    mensagens::{icone_de, trechos_perigosos},
};
use crate::{
    modelo::{Chat, Item},
    motor::Pendencia,
    tema::{Cores, Lucide, Tipo as _, esp, r, raio},
};

/// A partir de quantos avisos de uma vez eles viram um só, "N chats precisam de você".
const RAJADA: usize = 3;

/// A chave de um aviso: o pedido, ou a falha com o texto dela (uma falha nova volta a avisar).
pub(super) fn chave_do_aviso(p: &Pendencia) -> Option<String> {
    match p {
        Pendencia::Permissao { pedido, .. } | Pendencia::Plano { pedido, .. } | Pendencia::Pergunta { pedido, .. } => Some(pedido.clone()),
        Pendencia::Erro { chat, texto, .. } => Some(format!("erro-{chat}-{texto}")),
        Pendencia::Parada { .. } | Pendencia::Nova { .. } => None,
    }
}

/// O turno do chat foi começado por outro chat (uma entrega que não é do Vitral): esse nunca avisa.
pub(super) fn turno_de_outro_chat(chat: &Chat) -> bool {
    chat.itens
        .iter()
        .rev()
        .find(|i| matches!(i, Item::Usuario { .. } | Item::Entrega { .. }))
        .is_some_and(|i| matches!(i, Item::Entrega { de, .. } if de != "Vitral"))
}

/// A pendência entra no canto: pedido, plano ou falha, de chat fora de vista, que você não
/// dispensou no ×.
pub(super) fn entra_no_canto(p: &Pendencia, visiveis: &HashSet<String>, dispensados: &HashSet<String>, de_outro_chat: bool) -> bool {
    let Some(chave) = chave_do_aviso(p) else {
        return false;
    };
    !visiveis.contains(p.chat()) && !dispensados.contains(&chave) && !de_outro_chat
}

impl Janela {
    /// Os avisos do canto, por cima de tudo. `None` sem nada a avisar ou com a chave desligada.
    pub(super) fn avisos_no_canto(&mut self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let motor = self.motor.clone()?;
        let m = motor.read(cx);
        if !m.config.avisos_no_app {
            return None;
        }
        let visiveis: HashSet<String> = self.paineis.iter().filter_map(|p| p.chat.clone()).collect();
        let pendencias: Vec<Pendencia> = m
            .pendencias()
            .into_iter()
            .filter(|p| {
                let de_outro = m.chat(p.chat()).is_some_and(turno_de_outro_chat);
                entra_no_canto(p, &visiveis, &self.avisos_dispensados, de_outro)
            })
            .collect();
        // O × de um aviso que já foi respondido não precisa ficar guardado.
        let vivas: HashSet<String> = m.pendencias().iter().filter_map(chave_do_aviso).collect();
        self.avisos_dispensados.retain(|c| vivas.contains(c));
        if pendencias.is_empty() {
            return None;
        }
        let cartoes: Vec<AnyElement> = if pendencias.len() >= RAJADA {
            vec![self.aviso_da_rajada(pendencias.len(), window, cx)]
        } else {
            pendencias.iter().filter_map(|p| self.aviso_de(p, window, cx)).collect()
        };
        // Acima da caixa do último painel (o enviar fica livre) e longe dos anéis de uso, no alto.
        Some(
            div()
                .absolute()
                .right(esp::E24)
                .bottom(r(144.))
                .w(r(340.))
                .child(v_flex().gap(esp::E8).children(cartoes))
                .into_any_element(),
        )
    }

    /// Três ou mais de uma vez viram um aviso só, que abre o sino.
    fn aviso_da_rajada(&self, n: usize, window: &Window, cx: &Context<Self>) -> AnyElement {
        let texto = format!("{} precisam de você", contagem(n, "chat", "chats"));
        let ver = botao_primario("aviso-rajada-ver", None, "Ver", TamanhoBotao::Barra, window, cx).on_click(cx.listener(|this, _, _, cx| {
            this.so_um_flutuante(QualFlutuante::Sino);
            this.caixa_aberta = true;
            cx.notify();
        }));
        moldura("aviso-rajada", cx)
            .child(
                h_flex()
                    .gap(esp::E8)
                    .child(icone_de(Lucide::Bell, 14.).text_color(Cores::espera()))
                    .child(div().flex_1().min_w_0().truncate().rotulo().text_color(Cores::forte()).child(texto))
                    .child(ver),
            )
            .into_any_element()
    }

    /// Um aviso: o título do chat (o clique abre), o que ele pede e os botões.
    fn aviso_de(&self, p: &Pendencia, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let motor = self.motor.clone()?;
        let m = motor.read(cx);
        let chat = m.chat(p.chat())?;
        let chave = chave_do_aviso(p)?;
        let (icone, cor, detalhe) = match p {
            Pendencia::Permissao { ferramenta, resumo, .. } => (Lucide::ShieldAlert, Cores::espera(), format!("Permissão · {ferramenta} · {resumo}")),
            Pendencia::Plano { .. } => (Lucide::Map, Cores::espera(), "Plano pronto para aprovar".to_string()),
            Pendencia::Pergunta { pergunta, .. } => (Lucide::MessageCircleQuestionMark, Cores::espera(), format!("Pergunta · {pergunta}")),
            Pendencia::Erro { texto, .. } => (
                Lucide::CircleX,
                Cores::vermelho(),
                format!("Falhou · {}", texto.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default()),
            ),
            _ => return None,
        };
        let id = |nome: &str| SharedString::from(format!("{nome}-{chave}"));
        let abrir = || {
            let chat = chat.id.clone();
            cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.selecionar(Some(chat.clone()), window, cx);
                this.focar_composer(window, cx);
            })
        };
        let mut botoes: Vec<AnyElement> = Vec::new();
        match p {
            Pendencia::Permissao { pedido, ferramenta, .. } => {
                let comando = m.permissoes.iter().find(|x| x.id == *pedido).and_then(|x| x.entrada["command"].as_str().map(str::to_string));
                // A regra do sino: comando perigoso ou comprido não se aprova daqui; o botão leva à
                // conversa, onde o cartão mostra o comando inteiro.
                let perigoso = ferramenta == "Bash" && comando.as_deref().is_some_and(|c| !trechos_perigosos(c).is_empty());
                let comprido = comando.as_deref().is_some_and(|c| c.lines().count() > 2 || c.chars().count() > 160);
                let (m1, m2, p1, p2) = (motor.clone(), motor.clone(), pedido.clone(), pedido.clone());
                botoes.push(
                    botao_fantasma(id("aviso-negar"), None, "Negar", TamanhoBotao::Barra, window, cx)
                        .on_click(move |_, _, cx| m1.update(cx, |m, cx| m.responder_permissao(&p1, false, None, cx)))
                        .into_any_element(),
                );
                botoes.push(if perigoso || comprido {
                    botao_fantasma(id("aviso-ver"), Some(Lucide::ArrowRight.into()), "Ver o comando…", TamanhoBotao::Barra, window, cx)
                        .on_click(abrir())
                        .into_any_element()
                } else {
                    botao_primario(id("aviso-permitir"), None, "Permitir", TamanhoBotao::Barra, window, cx)
                        .on_click(move |_, _, cx| m2.update(cx, |m, cx| m.responder_permissao(&p2, true, None, cx)))
                        .into_any_element()
                });
            }
            Pendencia::Plano { .. } => botoes.push(
                botao_primario(id("aviso-plano"), None, "Ver plano", TamanhoBotao::Barra, window, cx).on_click(abrir()).into_any_element(),
            ),
            // A pergunta se responde no cartão dela, na conversa.
            Pendencia::Pergunta { .. } => botoes.push(
                botao_primario(id("aviso-pergunta"), None, "Ver pergunta", TamanhoBotao::Barra, window, cx).on_click(abrir()).into_any_element(),
            ),
            Pendencia::Erro { pode_tentar, .. } => {
                botoes.push(botao_fantasma(id("aviso-abrir"), None, "Abrir", TamanhoBotao::Barra, window, cx).on_click(abrir()).into_any_element());
                if *pode_tentar {
                    let (motor, chat) = (motor.clone(), chat.id.clone());
                    botoes.push(
                        botao_primario(id("aviso-tentar"), Some(Lucide::RotateCw.into()), "Tentar de novo", TamanhoBotao::Barra, window, cx)
                            .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.tentar_de_novo(&chat, cx)))
                            .into_any_element(),
                    );
                }
            }
            _ => {}
        }
        let fechar = {
            let chave = chave.clone();
            botao_icone(id("aviso-fechar"), Lucide::X, "Dispensar este aviso", TamanhoBotao::Linha, window, cx).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.avisos_dispensados.insert(chave.clone());
                    cx.notify();
                },
            ))
        };
        Some(
            moldura(&chave, cx)
                .child(
                    h_flex()
                        .gap(esp::E8)
                        .child(icone_de(icone, 14.).text_color(cor))
                        .child(
                            div()
                                .id(id("aviso-titulo"))
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .rotulo()
                                .text_color(Cores::forte())
                                .cursor_pointer()
                                .hover(|s| s.text_color(Cores::destaque_texto()))
                                .child(chat.titulo.clone())
                                .on_click(abrir()),
                        )
                        .child(fechar),
                )
                .child(div().pl(esp::E24).min_w_0().line_clamp(2).metadado().text_color(Cores::tenue()).child(detalhe))
                .child(h_flex().justify_end().gap(esp::E6).children(botoes))
                .into_any_element(),
        )
    }
}

/// O cartão flutuante de um aviso (opaco: fica por cima da conversa).
fn moldura(chave: &str, cx: &App) -> Stateful<Div> {
    v_flex()
        .id(SharedString::from(format!("aviso-no-app-{chave}")))
        .gap(esp::E6)
        .p(esp::E12)
        .flutuante(cx)
        .rounded(raio::CARTAO)
        .border_1()
        .border_color(Cores::borda_forte())
        .occlude()
}

#[cfg(test)]
mod testes {
    use std::collections::HashSet;

    use super::{chave_do_aviso, entra_no_canto, turno_de_outro_chat};
    use crate::{
        modelo::{Chat, Item},
        motor::Pendencia,
    };

    #[test]
    fn so_o_que_trava_trabalho_de_chat_fora_de_vista() {
        let pedido = Pendencia::Permissao { pedido: "p1".into(), chat: "a".into(), ferramenta: "Bash".into(), resumo: "ls".into() };
        let erro = Pendencia::Erro { chat: "b".into(), texto: "caiu".into(), pode_tentar: true };
        let nova = Pendencia::Nova { chat: "c".into() };
        let parada = Pendencia::Parada { chat: "d".into(), motivo: "x".into() };
        let nada: HashSet<String> = HashSet::new();
        assert!(entra_no_canto(&pedido, &nada, &nada, false));
        assert!(entra_no_canto(&erro, &nada, &nada, false));
        // A resposta nova e a parada ficam no sino: não travam trabalho agora.
        assert!(!entra_no_canto(&nova, &nada, &nada, false));
        assert!(!entra_no_canto(&parada, &nada, &nada, false));
        // À vista num painel, o cartão já está lá.
        let a_vista: HashSet<String> = ["a".to_string()].into();
        assert!(!entra_no_canto(&pedido, &a_vista, &nada, false));
        // Dispensado no ×, ou começado por outro chat, não volta.
        let dispensado: HashSet<String> = [chave_do_aviso(&pedido).unwrap()].into();
        assert!(!entra_no_canto(&pedido, &nada, &dispensado, false));
        assert!(!entra_no_canto(&pedido, &nada, &nada, true));
    }

    #[test]
    fn turno_de_outro_chat_e_a_entrega_que_nao_e_do_vitral() {
        let mut chat = Chat::novo("c".into(), "t", std::path::PathBuf::from("/tmp"));
        chat.itens.push(Item::Entrega { de: "loja (abc)".into(), texto: "faz".into(), em: 0 });
        assert!(turno_de_outro_chat(&chat));
        chat.itens.push(Item::Entrega { de: "Vitral".into(), texto: "continue".into(), em: 0 });
        assert!(!turno_de_outro_chat(&chat));
        chat.itens.push(Item::Usuario { texto: "oi".into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0, marca: None });
        assert!(!turno_de_outro_chat(&chat));
    }
}
