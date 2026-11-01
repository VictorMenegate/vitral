//! Fechar a janela, trocar de pasta ou reiniciar para a versão nova mata os processos do Claude:
//! com conversas trabalhando, o Vitral pergunta antes.

use std::path::PathBuf;

use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Confirmacao, TomConfirmacao, confirmar},
};
use crate::{atualizacao, tema::Lucide};

/// O que vai parar as conversas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Parar {
    Fechar,
    TrocarPasta,
    Reiniciar,
}

/// "1 conversa está trabalhando", "2 conversas estão trabalhando".
pub(super) fn conversas_trabalhando(n: usize) -> String {
    if n == 1 { "1 conversa está trabalhando".to_string() } else { format!("{n} conversas estão trabalhando") }
}

/// A pergunta com `n` conversas trabalhando. O "mesmo assim" pede o clique: o Enter fica com o
/// "continuar".
pub(super) fn confirmacao_de_parar(n: usize, parar: Parar) -> Confirmacao {
    let param = if n == 1 { "ela para" } else { "elas param" };
    let (acao, ok, cancelar, depois) = match parar {
        Parar::TrocarPasta => ("trocar de pasta", "Trocar mesmo assim", "Ficar nesta pasta", "Ao voltar a esta pasta"),
        Parar::Fechar => ("fechar", "Fechar mesmo assim", "Continuar aberto", "Ao abrir o Vitral de novo"),
        Parar::Reiniciar => ("reiniciar", "Reiniciar mesmo assim", "Continuar aberto", "Quando o Vitral abrir de novo"),
    };
    Confirmacao {
        tom: TomConfirmacao::Atencao,
        icone: Lucide::TriangleAlert.into(),
        titulo: conversas_trabalhando(n).into(),
        texto: format!(
            "Se {acao} agora, {param} no meio. {depois}, o que parou fica em Esperando você, com o botão Retomar."
        )
        .into(),
        detalhes: Vec::new(),
        ok: ok.into(),
        cancelar: cancelar.into(),
        ok_perigo: true,
    }
}

/// A Janela que está na raiz da janela (o × e os avisos chegam sem ela emprestada).
fn janela_da_raiz(window: &mut Window, cx: &mut App) -> Option<Entity<Janela>> {
    window
        .root::<gpui_kit::base::Root>()
        .flatten()
        .and_then(|raiz| raiz.read(cx).view().clone().downcast::<Janela>().ok())
}

/// O × da janela passa pela mesma pergunta do Alt+F4 (o `remove_window` não passa por ela). Não
/// serve dentro de um `cx.listener` da Janela, que já estaria emprestada.
pub(super) fn fechar_janela(window: &mut Window, cx: &mut App) {
    let janela = janela_da_raiz(window, cx);
    if janela.is_none_or(|j| j.update(cx, |this, cx| this.pode_fechar(window, cx))) {
        window.remove_window();
    }
}

/// O Reiniciar do aviso e das Configurações: a mesma pergunta do fechar, e então a versão nova.
/// Não serve dentro de um `cx.listener` da Janela.
pub(super) fn reiniciar_pela_janela(window: &mut Window, cx: &mut App) {
    match janela_da_raiz(window, cx) {
        Some(janela) => janela.update(cx, |this, cx| this.reiniciar(window, cx)),
        None => atualizacao::reiniciar(cx),
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
        self.pode_parar_tudo(Parar::Fechar, |window, _| window.remove_window(), window, cx)
    }

    /// Reinicia na versão nova (ou na atual), perguntando antes se há conversa trabalhando. As
    /// tarefas cortadas ficam paradas, com Retomar, como numa queda.
    pub(super) fn reiniciar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pode_parar_tudo(Parar::Reiniciar, |_, cx| atualizacao::reiniciar(cx), window, cx) {
            atualizacao::reiniciar(cx);
        }
    }

    /// Escolher outra pasta de trabalho também para tudo o que roda nesta.
    pub(super) fn trocar_raiz(&mut self, raiz: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let fraca = cx.weak_entity();
        let destino = raiz.clone();
        let seguir = move |window: &mut Window, cx: &mut App| {
            let _ = fraca.update(cx, |this, cx| this.abrir_raiz(destino.clone(), window, cx));
        };
        if self.pode_parar_tudo(Parar::TrocarPasta, seguir, window, cx) {
            self.abrir_raiz(raiz, window, cx);
        }
    }

    /// Com conversas trabalhando, abre o alerta e devolve false; o "mesmo assim" faz `seguir`. Esc e o
    /// outro botão deixam tudo como está.
    fn pode_parar_tudo(
        &mut self,
        parar: Parar,
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
        let fraca = cx.weak_entity();
        let liberar: Rc<dyn Fn(&mut App)> = Rc::new(move |cx| {
            let _ = fraca.update(cx, |this, _| this.confirmando_saida = false);
        });
        confirmar(confirmacao_de_parar(n, parar), seguir, Some(liberar), window, cx);
        false
    }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: o `gpui_kit::*` traz um `test` próprio que encobriria o da biblioteca.
    use super::{Parar, confirmacao_de_parar, conversas_trabalhando};

    #[test]
    fn a_pergunta_tem_o_plural_certo_e_o_enter_fica_com_o_continuar() {
        assert_eq!(conversas_trabalhando(1), "1 conversa está trabalhando");
        assert_eq!(conversas_trabalhando(2), "2 conversas estão trabalhando");
        let uma = confirmacao_de_parar(1, Parar::Reiniciar);
        assert_eq!(uma.titulo, "1 conversa está trabalhando");
        assert!(uma.texto.starts_with("Se reiniciar agora, ela para no meio."), "{}", uma.texto);
        let duas = confirmacao_de_parar(2, Parar::Reiniciar);
        assert_eq!(duas.titulo, "2 conversas estão trabalhando");
        assert!(duas.texto.contains("elas param no meio") && duas.texto.contains("Retomar"), "{}", duas.texto);
        assert_eq!((duas.ok.as_ref(), duas.cancelar.as_ref()), ("Reiniciar mesmo assim", "Continuar aberto"));
        for parar in [Parar::Fechar, Parar::TrocarPasta, Parar::Reiniciar] {
            // O OK perigoso é o que deixa o Enter só fechar (`confirmar`).
            assert!(confirmacao_de_parar(3, parar).ok_perigo, "{parar:?}");
        }
        assert_eq!(confirmacao_de_parar(2, Parar::TrocarPasta).cancelar, "Ficar nesta pasta");
    }
}
