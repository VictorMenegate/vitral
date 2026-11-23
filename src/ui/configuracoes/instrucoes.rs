//! O diálogo **Suas instruções** (Configurações > Agentes): o texto que entra no fim do prompt de
//! sistema de todos os chats, depois do que o Vitral manda (`prompts::com_instrucoes`). Salvar
//! grava no `config.json`; vale a partir da próxima mensagem de cada chat.

use gpui_kit::component::{
    Disableable as _, WindowExt as _,
    input::{Textarea, TextareaState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::{
    motor::Motor,
    prompts::LIMITE_DAS_INSTRUCOES,
    tema::{Cores, Tipo as _, esp},
    ui::{
        componentes::{CabecalhoDialogo, RodapeDialogo, TamanhoBotao, botao_fantasma, botao_primario, largura},
        dialogos,
    },
};

pub(super) fn abrir(motor: Entity<Motor>, window: &mut Window, cx: &mut App) {
    let atual = motor.read(cx).config.instrucoes.clone();
    let tinha = !atual.trim().is_empty();
    let campo = cx.new(|cx| {
        TextareaState::new(window, cx)
            .auto_grow(8, 16)
            .placeholder("Rode os testes antes de dizer que terminou.\nMensagens de commit em português.")
            .default_value(atual)
    });
    // A contagem embaixo do campo acompanha o que se digita.
    cx.observe(&campo, |_, cx| cx.refresh_windows()).detach();
    let foco = campo.read(cx).focus_handle(cx);
    window.open_dialog(cx, move |dialogo, window, cx| {
        let total = campo.read(cx).value().trim().chars().count();
        let passou = total > LIMITE_DAS_INSTRUCOES;
        let gravar = |texto: Option<Entity<TextareaState>>| {
            let motor = motor.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                let novo = texto.as_ref().map(|campo| campo.read(cx).value().trim().to_string()).unwrap_or_default();
                motor.update(cx, |m, cx| m.mudar_config(|c| c.instrucoes = novo, cx));
                window.close_dialog(cx);
            }
        };
        dialogos::moldura(dialogo, largura::M, window)
            .title(dialogos::topo(
                CabecalhoDialogo::new("Suas instruções").subtitulo(div().font_weight(FontWeight::NORMAL).child("Para todos os chats")),
            ))
            .child(
                v_flex()
                    .gap(esp::E12)
                    .child(div().corpo_ui().text_color(Cores::tenue()).child(
                        "Entram no fim do prompt de sistema de cada chat, do Claude e do Codex, depois do que o Vitral já \
                         manda. Valem a partir da próxima mensagem.",
                    ))
                    .child(Textarea::new(&campo))
                    .child(
                        div()
                            .metadado()
                            .text_color(if passou { Cores::vermelho() } else { Cores::tenue() })
                            .child(format!("{total} de {LIMITE_DAS_INSTRUCOES} caracteres")),
                    ),
            )
            .footer(
                RodapeDialogo::new()
                    .when(tinha, |rodape| {
                        rodape.esquerda(
                            botao_fantasma("instrucoes-apagar", None, "Apagar", TamanhoBotao::Controle, window, cx).on_click(gravar(None)),
                        )
                    })
                    .cancelar(
                        botao_fantasma("instrucoes-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .principal(
                        botao_primario("instrucoes-salvar", None, "Salvar", TamanhoBotao::Controle, window, cx)
                            .disabled(passou)
                            .on_click(gravar(Some(campo.clone()))),
                    ),
            )
    });
    window.focus(&foco, cx);
}
