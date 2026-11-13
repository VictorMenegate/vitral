//! O que espera você além das conversas e o que aconteceu enquanto você esteve fora: os pares de
//! chats parados no freio de ciclo (em "Esperando você" e no sino) e a faixa "Enquanto você esteve
//! fora".

use gpui_kit::component::{Icon, h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{CartaoDeLista, Sinal, Simbolo, TamanhoBotao, botao_fantasma, botao_icone, simbolo_do_chat},
};
use crate::{
    modelo::Chat,
    motor::{Motor, PrecisaDeVoce},
    tema::{Cores, Lucide, Tipo as _, dentro_da_borda, esp, r, raio},
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

fn uma_linha(texto: &str) -> String {
    let linha = texto.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    if linha.chars().count() > 120 { format!("{}…", linha.chars().take(120).collect::<String>()) } else { linha.to_string() }
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

    /// "Enquanto você esteve fora": o que terminou, o que precisa de você e o que falhou ou parou
    /// desde que você saiu, com as linhas que abrem o chat e o ×. Só na memória: some no × e é
    /// trocada na volta seguinte.
    pub(super) fn faixa_do_resumo(&self, motor: &Motor, onde: &'static str, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let resumo = motor.resumo_volta.as_ref()?;
        let desde = crate::modelo::quando_local(resumo.desde);
        let grupos = [("terminou", "Terminou", Cores::verde()), ("precisa", "Precisa de você", Cores::espera()), ("falhou", "Falhou ou parou", Cores::vermelho())];
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
                corpo = corpo.child(
                    h_flex()
                        .id(SharedString::from(format!("resumo-{onde}-{grupo}-{i}")))
                        .min_h(r(28.))
                        .px(esp::E4)
                        .gap(esp::E8)
                        .rounded(raio::CONTROLE)
                        .when(chat.is_some(), |s| s.cursor_pointer().hover(|s| s.bg(Cores::hover())))
                        .child(div().flex_none().child(simbolo_do_chat(16.)))
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
        let fechar = botao_icone(SharedString::from(format!("fechar-resumo-{onde}")), Lucide::X, "Fechar o resumo", TamanhoBotao::Linha, window, cx)
            .on_click(move |_, _, cx| {
                if let Some(m) = &motor_e {
                    m.update(cx, |m, cx| m.fechar_resumo(cx));
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
                .into_any_element(),
        )
    }
}
