//! A fila de mensagens de uma conversa: o que você mandou enquanto o turno rodava. A pílula da
//! faixa do turno abre esta lista, que mostra cada mensagem esperando, deixa tirar uma de volta
//! para a caixa e, com o chat parado, entregar tudo agora.

use gpui_kit::component::{
    Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        CabecalhoPopover, CorpoPopover, LarguraPopover, Selo, TamanhoBotao, Tom, aviso, botao_fantasma, botao_icone,
        contagem, popover, variante_fantasma,
    },
};
use crate::{
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, esp, r, raio},
};

/// Quanto de cada mensagem a lista mostra numa linha só.
const LETRAS_DA_PREVIA: usize = 140;

/// A primeira linha de uma mensagem da fila, cortada no limite.
pub(super) fn previa(texto: &str) -> String {
    let linha = texto.trim().lines().next().unwrap_or("").trim();
    if linha.chars().count() <= LETRAS_DA_PREVIA {
        return linha.to_string();
    }
    format!("{}…", linha.chars().take(LETRAS_DA_PREVIA).collect::<String>())
}

/// O que a pílula da faixa diz: com o turno rodando, só a contagem; parado, o que a fila é.
pub(super) fn rotulo(n: usize, rodando: bool, estreita: bool) -> String {
    match (rodando, estreita) {
        (true, _) => "na fila".to_string(),
        (false, true) => contagem(n, "na fila", "na fila"),
        (false, false) => contagem(n, "mensagem esperando na fila", "mensagens esperando na fila"),
    }
}

impl Janela {
    /// A pílula clicável da fila, com o número e o popover da lista. `rodando`: o turno está em
    /// andamento (a fila entra quando ele acabar); parado, o rodapé traz o "Entregar agora".
    pub(super) fn pilula_da_fila(
        &self,
        chat: &str,
        painel: u64,
        n: usize,
        rodando: bool,
        estreita: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let dica: SharedString = if rodando {
            "Ver a fila: o que você mandou enquanto o turno roda, e tirar o que não quer mais".into()
        } else {
            "Ver a fila desta conversa e tirar o que não quer mais".into()
        };
        let gatilho = Button::new(("pilula-fila", painel as usize))
            .custom(variante_fantasma(cx))
            .h(r(22.))
            .px(r(6.))
            .rounded(raio::PILULA)
            .tooltip(dica)
            .child(
                h_flex()
                    .gap(r(4.))
                    .child(Selo::contagem(n))
                    .child(div().metadado().text_color(Cores::apagado()).child(rotulo(n, rodando, estreita))),
            );
        let (motor, janela, id) = (self.motor.clone(), cx.entity().downgrade(), chat.to_string());
        popover(("pop-fila", painel as usize))
            .anchor(Anchor::TopLeft)
            .offset(px(8.))
            .trigger(gatilho)
            .content(move |_, window, cx| {
                let Some(m) = motor.clone() else {
                    return div().into_any_element();
                };
                corpo_da_fila(&m, &janela, &id, painel, rodando, window, cx)
            })
            .into_any_element()
    }

    /// Tira uma mensagem da fila. O texto volta para a caixa quando ela está vazia; com algo
    /// escrito lá, ele iria no lugar do que você está escrevendo, então só sai da fila e o aviso
    /// diz qual era.
    pub(super) fn tirar_da_fila(&mut self, chat: String, indice: usize, painel: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let Some(texto) = motor.update(cx, |m, cx| m.tirar_da_fila(&chat, indice, cx)) else {
            return;
        };
        let caixa_vazia = self
            .paineis
            .iter()
            .find(|p| p.id == painel)
            .is_some_and(|p| p.composer.read(cx).value().trim().is_empty());
        if caixa_vazia {
            self.escrever_na_caixa(painel, &texto, window, cx);
            window.push_notification(aviso(Tom::Info, "Tirei da fila; o texto voltou para a caixa."), cx);
        } else {
            window.push_notification(aviso(Tom::Info, format!("Tirei da fila: “{}”", previa(&texto))), cx);
        }
        cx.notify();
    }
}

/// A lista da fila: uma linha por mensagem, na ordem em que vão entrar, com o × de cada uma.
fn corpo_da_fila(
    motor: &Entity<Motor>,
    janela: &WeakEntity<Janela>,
    chat: &str,
    painel: u64,
    rodando: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let m = motor.read(cx);
    let fila: Vec<(usize, String, bool)> = m
        .fila_do_chat(chat)
        .into_iter()
        .map(|(ix, e)| (ix, previa(&e.texto), e.do_usuario()))
        .collect();
    if fila.is_empty() {
        return CorpoPopover::new(LarguraPopover::P360)
            .cabecalho(CabecalhoPopover::Escolha {
                simbolo: Some(super::componentes::Simbolo::Icone(Lucide::ListChecks.into(), Some(Cores::tenue()))),
                titulo: "Fila desta conversa".into(),
                valor: None,
                dica: None,
            })
            .child(div().corpo_ui().text_color(Cores::tenue()).child("A fila está vazia."))
            .into_any_element();
    }
    let total = fila.len();
    let linhas: Vec<AnyElement> = fila
        .into_iter()
        .enumerate()
        .map(|(posicao, (indice, texto, do_usuario))| {
            let (janela, chat) = (janela.clone(), chat.to_string());
            let tirar = botao_icone(
                ("tirar-da-fila", indice),
                Lucide::X,
                "Tirar da fila (o texto volta para a caixa, se ela estiver vazia)",
                TamanhoBotao::Linha,
                window,
                cx,
            )
            .on_click(move |_, window, cx| {
                let _ = janela.update(cx, |this, cx| this.tirar_da_fila(chat.clone(), indice, painel, window, cx));
            });
            h_flex()
                .gap(esp::E8)
                .min_h(r(28.))
                .child(
                    div()
                        .flex_none()
                        .size(r(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(raio::CONTROLE)
                        .bg(Cores::recuo())
                        .micro()
                        .text_color(Cores::tenue())
                        .child((posicao + 1).to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .corpo_ui()
                        .text_color(if do_usuario { Cores::texto() } else { Cores::tenue() })
                        .child(texto),
                )
                .child(tirar)
                .into_any_element()
        })
        .collect();
    let explicacao = if rodando {
        "Entram juntas no próximo turno, quando este terminar."
    } else {
        "Entram juntas na próxima mensagem, ou agora, no botão abaixo."
    };
    let entregar = (!rodando).then(|| {
        let (motor, chat) = (motor.clone(), chat.to_string());
        botao_fantasma("fila-entregar", Some(Lucide::Play.into()), "Entregar agora", TamanhoBotao::Barra, window, cx)
            .on_click(move |_, _, cx| {
                motor.update(cx, |m, cx| m.entregar_fila(&chat, cx));
            })
    });
    CorpoPopover::new(LarguraPopover::P360)
        .cabecalho(CabecalhoPopover::Escolha {
            simbolo: Some(super::componentes::Simbolo::Icone(Lucide::ListChecks.into(), Some(Cores::tenue()))),
            titulo: "Fila desta conversa".into(),
            valor: Some((contagem(total, "mensagem", "mensagens").into(), Cores::texto())),
            dica: None,
        })
        .child(v_flex().gap(esp::E4).children(linhas))
        .child(div().metadado().text_color(Cores::tenue()).child(explicacao))
        .when_some(entregar, |c, b| c.rodape(h_flex().justify_end().child(b)))
        .into_any_element()
}

#[cfg(test)]
mod testes {
    use super::{previa, rotulo};

    #[test]
    fn a_previa_e_a_primeira_linha_cortada() {
        assert_eq!(previa("  uma linha só \n e outra"), "uma linha só");
        assert_eq!(previa(""), "");
        let longa = "á".repeat(200);
        let cortada = previa(&longa);
        assert_eq!(cortada.chars().count(), 141);
        assert!(cortada.ends_with('…'));
    }

    #[test]
    fn a_pilula_diz_o_que_cabe_na_largura() {
        assert_eq!(rotulo(2, true, false), "na fila");
        assert_eq!(rotulo(1, false, false), "1 mensagem esperando na fila");
        assert_eq!(rotulo(3, false, false), "3 mensagens esperando na fila");
        assert_eq!(rotulo(3, false, true), "3 na fila");
    }
}
