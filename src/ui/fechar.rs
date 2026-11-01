//! Fechar a janela ou trocar de pasta mata os processos do Claude: com conversas trabalhando, o
//! Vitral pergunta antes.

use std::path::PathBuf;

use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Confirmacao, TomConfirmacao, confirmar},
};
use crate::tema::Lucide;

/// O × da janela passa pela mesma pergunta do Alt+F4 (o `remove_window` não passa por ela). Não
/// serve dentro de um `cx.listener` da Janela, que já estaria emprestada.
pub(super) fn fechar_janela(window: &mut Window, cx: &mut App) {
    let janela = window
        .root::<gpui_kit::base::Root>()
        .flatten()
        .and_then(|raiz| raiz.read(cx).view().clone().downcast::<Janela>().ok());
    if janela.is_none_or(|j| j.update(cx, |this, cx| this.pode_fechar(window, cx))) {
        window.remove_window();
    }
}

impl Janela {
    /// Alt+F4, o × do Panorama e o "Sair" do dash chegam pelo compositor.
    pub(super) fn vigiar_fechamento(window: &mut Window, cx: &mut Context<Self>) {
        let fraca = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            fraca
                .update(cx, |this, cx| this.pode_fechar(window, cx))
                .unwrap_or(true)
        });
    }

    fn pode_fechar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.pode_parar_tudo(false, |window, _| window.remove_window(), window, cx)
    }

    /// Escolher outra pasta de trabalho também para tudo o que roda nesta.
    pub(super) fn trocar_raiz(&mut self, raiz: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let fraca = cx.weak_entity();
        let destino = raiz.clone();
        let seguir = move |window: &mut Window, cx: &mut App| {
            let _ = fraca.update(cx, |this, cx| this.abrir_raiz(destino.clone(), window, cx));
        };
        if self.pode_parar_tudo(true, seguir, window, cx) {
            self.abrir_raiz(raiz, window, cx);
        }
    }

    /// Com conversas trabalhando, abre o alerta e devolve false; o "mesmo assim" faz `seguir`. Esc e o
    /// outro botão deixam tudo como está.
    fn pode_parar_tudo(
        &mut self,
        trocar_pasta: bool,
        seguir: impl Fn(&mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let n = self.motor.as_ref().map_or(0, |m| m.read(cx).trabalhando());
        if n == 0 {
            return true;
        }
        // Alt+F4 de novo com o alerta aberto não abre outro.
        if self.confirmando_saida && window.has_active_dialog(cx) {
            return false;
        }
        self.confirmando_saida = true;
        let (titulo, param) = if n == 1 {
            ("1 conversa está trabalhando".to_string(), "ela para")
        } else {
            (format!("{n} conversas estão trabalhando"), "elas param")
        };
        let (acao, ok, cancelar, depois) = if trocar_pasta {
            ("trocar de pasta", "Trocar mesmo assim", "Ficar nesta pasta", "Ao voltar a esta pasta")
        } else {
            ("fechar", "Fechar mesmo assim", "Continuar aberto", "Ao abrir o Vitral de novo")
        };
        let texto = format!(
            "Se {acao} agora, {param} no meio. {depois}, o que parou fica em Esperando você, com o \
             botão Retomar."
        );
        let fraca = cx.weak_entity();
        let liberar: Rc<dyn Fn(&mut App)> = Rc::new(move |cx| {
            let _ = fraca.update(cx, |this, _| this.confirmando_saida = false);
        });
        // Fechar mesmo assim pede o clique: o Enter fica com o "continuar".
        confirmar(
            Confirmacao {
                tom: TomConfirmacao::Atencao,
                icone: Lucide::TriangleAlert.into(),
                titulo: titulo.into(),
                texto: texto.into(),
                detalhes: Vec::new(),
                ok: ok.into(),
                cancelar: cancelar.into(),
                ok_perigo: true,
            },
            seguir,
            Some(liberar),
            window,
            cx,
        );
        false
    }
}
