//! O menu da conversa, o mesmo no clique direito e no "…". Os grupos vêm sempre nesta ordem: abrir
//! ou ir, criar ou editar, organizar, copiar, fora do app, configurar e, por último, o perigoso.

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Janela;
use crate::{
    motor::Motor,
    tema::Lucide,
};

/// Item que chama um método da janela.
fn acao(
    rotulo: impl Into<SharedString>,
    icone: impl Into<gpui_kit::component::Icon>,
    janela: &WeakEntity<Janela>,
    f: impl Fn(&mut Janela, &mut Window, &mut Context<Janela>) + 'static,
) -> PopupMenuItem {
    let janela = janela.clone();
    PopupMenuItem::new(rotulo).icon(icone).on_click(move |_, window, cx| {
        let _ = janela.update(cx, |this, cx| f(this, window, cx));
    })
}

/// A parte mínima do menu da conversa (F33): abrir ao lado, renomear, fixar e arquivar. Abrir a pasta no VS Code e no terminal saíram daqui a pedido do dono (12/11/2026).
/// Exportar e continuar no terminal entram com o F33 inteiro. Sem
/// `ao_lado` (o "⋯" do próprio painel, que já tem "Nova conversa ao lado"), começa no renomear.
pub(super) fn menu_conversa(
    menu: PopupMenu,
    janela: WeakEntity<Janela>,
    motor: &Entity<Motor>,
    chat: &str,
    ao_lado: bool,
    _window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    let Some(c) = motor.read(cx).chat(chat) else {
        return menu;
    };
    let (fixado, arquivado) = (c.fixado, c.arquivado);
    // "Marcar como não lido" só onde há o que marcar: lida, com alguma coisa dentro, sem turno
    // rodando (o ponto azul não vale para quem está trabalhando).
    let pode_marcar_nao_lido = !arquivado && !c.itens.is_empty() && !motor.read(cx).nao_lido(c) && !motor.read(cx).execucao(chat).rodando();
    let cabe = janela
        .upgrade()
        .map(|j| {
            let j = j.read(cx);
            j.cabe_outro_painel() && !j.aberta_em_painel(chat)
        })
        .unwrap_or(false);
    let id = chat.to_string();
    // As ideias guardadas para o futuro desta conversa (as que ainda não foram feitas).
    let ideias = motor
        .read(cx)
        .quadro()
        .ideias
        .iter()
        .filter(|i| i.chat.as_deref() == Some(chat) && i.coluna != crate::ideias::Coluna::Feitas)
        .count();
    let rotulo_ideias = if ideias == 0 { "Ideias deste chat".to_string() } else { format!("Ideias deste chat ({ideias})") };
    let (i1, i2, i3, i4, i5, i6) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id);
    let (m1, m2, m3) = (motor.clone(), motor.clone(), motor.clone());
    menu.when(ao_lado, |menu| {
        menu.item(
            acao("Abrir ao lado", Lucide::Columns2, &janela, move |this, window, cx| {
                this.abrir_ao_lado(i1.clone(), None, window, cx)
            })
            .disabled(!cabe),
        )
        .separator()
    })
    .item(acao("Renomear…", Lucide::Pencil, &janela, move |this, window, cx| {
        this.dialogo_renomear(i2.clone(), window, cx)
    }))
    .item(acao(rotulo_ideias, Lucide::Lightbulb, &janela, move |this, window, cx| {
        this.abrir_quadro_de_ideias_do_chat(i5.clone(), window, cx)
    }))
    .separator()
    .when(pode_marcar_nao_lido, |menu| {
        menu.item(
            PopupMenuItem::new("Marcar como não lido")
                .icon(Lucide::CircleDot)
                .on_click(move |_, _, cx| m3.update(cx, |m, cx| m.marcar_nao_lido(&i6, cx))),
        )
    })
    .item(
        PopupMenuItem::new(if fixado { "Desafixar" } else { "Fixar" })
            .icon(if fixado { Lucide::PinOff } else { Lucide::Pin })
            .on_click(move |_, _, cx| m1.update(cx, |m, cx| m.alternar_fixado(&i3, cx))),
    )
    .item(acao(
        if arquivado { "Desarquivar" } else { "Arquivar" },
        if arquivado { Lucide::ArchiveRestore } else { Lucide::Archive },
        &janela,
        move |this, window, cx| {
            if arquivado {
                // Desarquivar não avisa.
                m2.update(cx, |m, cx| m.alternar_arquivado(&i4, cx));
                return;
            }
            this.arquivar_do_menu(i4.clone(), window, cx)
        },
    ))
}
