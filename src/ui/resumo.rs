//! O que espera você além das conversas: os pares de chats parados no freio de ciclo (em
//! "Esperando você" e no sino). O cartão "Enquanto você esteve fora" saiu (dono, 26/11/2026).

use gpui_kit::component::h_flex;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{CartaoDeLista, Sinal, Simbolo, TamanhoBotao, botao_fantasma, simbolo_do_chat},
};
use crate::{
    modelo::Chat,
    motor::{Motor, PrecisaDeVoce},
    tema::esp,
};

/// Um par de chats parado no freio de ciclo, em "Esperando você".
pub(super) struct EsperaDeCiclo {
    /// Única entre as linhas ("ciclo-c7-c12").
    pub chave: String,
    /// O chat a abrir (o primeiro do par).
    pub chat: String,
    pub texto: String,
}

/// "chat a1b2c3 “Checkout”".
pub(super) fn nome_do_chat(c: &Chat) -> String {
    format!("chat {} “{}”", c.id, c.titulo)
}

/// Os pares travados no freio de ciclo.
pub(super) fn esperas_de_ciclo(motor: &Motor) -> Vec<EsperaDeCiclo> {
    motor
        .ciclos_travados()
        .into_iter()
        .filter_map(|(a, b)| {
            let (ca, cb) = (motor.chat(&a)?, motor.chat(&b)?);
            Some(EsperaDeCiclo {
                chave: format!("ciclo-{a}-{b}"),
                chat: ca.id.clone(),
                texto: format!("Muitas mensagens seguidas entre {} e {}: escreva para um dos dois", nome_do_chat(ca), nome_do_chat(cb)),
            })
        })
        .collect()
}

/// O que precisa de você em tudo, com cada par travado no freio de ciclo.
pub(super) fn precisa_em_tudo(motor: &Motor) -> PrecisaDeVoce {
    let mut conta = motor.precisa_de_voce();
    for _ in motor.ciclos_travados() {
        conta.somar(crate::motor::Espera::Pedido, false);
    }
    conta
}

impl Janela {
    /// As linhas dos pares travados em "Esperando você": o que espera e Ver.
    pub(super) fn linhas_de_ciclo(&self, motor: &Motor, esperas: Vec<EsperaDeCiclo>, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        esperas
            .into_iter()
            .map(|e| {
                let (chat, chat_v) = (e.chat.clone(), e.chat.clone());
                let ver = botao_fantasma(SharedString::from(format!("ver-{}", e.chave)), None, "Ver", TamanhoBotao::Linha, window, cx)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.selecionar(Some(chat_v.clone()), window, cx)
                    }));
                let subtitulo = h_flex()
                    .gap(esp::E6)
                    .min_w_0()
                    .child(div().flex_none().child(Sinal::Pedido.ponto(6.)))
                    .child(div().flex_1().min_w_0().truncate().child(e.texto));
                let titulo = motor.chat(&e.chat).map(|c| c.titulo.clone()).unwrap_or_default();
                let cartao_lista = CartaoDeLista::new(SharedString::from(e.chave), titulo)
                    .simbolo(Simbolo::Elemento(simbolo_do_chat(24.).into_any_element()))
                    .subtitulo(subtitulo)
                    .acao_fixa(ver)
                    .on_click(cx.listener(move |this, _, window, cx| this.selecionar(Some(chat.clone()), window, cx)));
                div().mt(esp::E2).child(cartao_lista).into_any_element()
            })
            .collect()
    }
}
