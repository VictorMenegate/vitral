//! Fechar a janela ou reiniciar para a versão nova mata os processos do Claude: com conversas
//! trabalhando, o Vitral pergunta antes.

use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{ChaveAviso as _, Confirmacao, Tom, TomConfirmacao, aviso, confirmar},
};
use crate::{atualizacao, tema::Lucide};

/// O que vai parar as conversas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Parar {
    Fechar,
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

/// A pergunta com texto do editor que não foi para o arquivo (`nome`: o arquivo; `motivo`: por
/// que não foi): fechar guarda o texto numa cópia em `pasta`, e as conversas trabalhando, se houver,
/// entram como detalhe. O "mesmo assim" pede o clique, como na outra.
pub(super) fn confirmacao_de_texto_nao_salvo(nome: &str, motivo: &str, pasta: &str, n: usize, parar: Parar) -> Confirmacao {
    let (acao, ok) = match parar {
        Parar::Fechar => ("Fechando", "Fechar mesmo assim"),
        Parar::Reiniciar => ("Reiniciando", "Reiniciar mesmo assim"),
    };
    let mut detalhes = Vec::new();
    if n > 0 {
        let param = if n == 1 { "ela para" } else { "elas param" };
        detalhes.push((Lucide::TriangleAlert.into(), format!("{}: {param} no meio.", conversas_trabalhando(n)).into(), true));
    }
    Confirmacao {
        tom: TomConfirmacao::Atencao,
        icone: Lucide::TriangleAlert.into(),
        titulo: "Texto não salvo".into(),
        texto: format!("{nome}: {motivo}. {acao} agora, o que você escreveu fica numa cópia em {pasta}.").into(),
        detalhes,
        ok: ok.into(),
        cancelar: "Continuar aberto".into(),
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
    if janela.as_ref().is_none_or(|j| j.update(cx, |this, cx| this.pode_fechar(window, cx))) {
        if let Some(j) = janela {
            j.update(cx, |this, cx| this.gravar_antes_de_fechar(cx));
        }
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
                .update(cx, |this, cx| {
                    let pode = this.pode_fechar(window, cx);
                    if pode {
                        this.gravar_antes_de_fechar(cx);
                    }
                    pode
                })
                .unwrap_or(true)
        });
    }

    fn pode_fechar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let fraca = cx.weak_entity();
        self.pode_parar_tudo(
            Parar::Fechar,
            move |window, cx| {
                let _ = fraca.update(cx, |this, cx| this.gravar_antes_de_fechar(cx));
                window.remove_window();
            },
            window,
            cx,
        )
    }

    /// O que o `on_app_quit` gravaria, antes de a janela sair pelo × ou pelo Alt+F4 (o
    /// `remove_window` não passa por ele): o lido, os rascunhos e o arranjo (conferência na
    /// bancada, 16/11/2026).
    fn gravar_antes_de_fechar(&mut self, cx: &mut Context<Self>) {
        self.gravar_ao_sair(cx);
        self.gravar_editor_ao_sair(cx);
        self.gravar_arranjo_ao_sair();
        if let Some(motor) = &self.motor {
            motor.update(cx, |m, _| m.gravar_antes_de_fechar());
        }
    }

    /// Reinicia na versão nova (ou na atual), perguntando antes se há conversa trabalhando. Os
    /// turnos cortados ficam parados, com Retomar, como numa queda.
    pub(super) fn reiniciar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pode_parar_tudo(Parar::Reiniciar, |_, cx| atualizacao::reiniciar(cx), window, cx) {
            atualizacao::reiniciar(cx);
        }
    }

    /// Com conversas trabalhando, ou com texto do editor que não pôde ir para o arquivo, abre o
    /// alerta e devolve false; o "mesmo assim" faz `seguir` (o texto do editor vai para a cópia no
    /// `gravar_editor_ao_sair`). Esc e o outro botão deixam tudo como está.
    fn pode_parar_tudo(
        &mut self,
        parar: Parar,
        seguir: impl Fn(&mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let n = self.motor.as_ref().map_or(0, |m| m.read(cx).trabalhando());
        let em_risco = self.texto_do_editor_em_risco(cx);
        if n == 0 && em_risco.is_none() {
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
        let confirmacao = match &em_risco {
            Some((nome, motivo)) => {
                let pasta = self.pasta_nao_salvos(cx).map_or_else(|| "a pasta de dados do Vitral".into(), |p| super::boas_vindas::com_til(&p));
                confirmacao_de_texto_nao_salvo(nome, motivo, &pasta, n, parar)
            }
            None => confirmacao_de_parar(n, parar),
        };
        // Com texto em risco, a cópia vem antes de fechar; se ela não sai, nada fecha.
        let copiar = em_risco.is_some();
        let fraca = cx.weak_entity();
        let seguir = move |window: &mut Window, cx: &mut App| {
            if copiar {
                let copiou = fraca.update(cx, |this, cx| this.copiar_texto_do_editor(cx)).unwrap_or(Ok(()));
                if let Err(erro) = copiou {
                    let frase = format!("Não deu para guardar a cópia do texto ({erro}). O Vitral continua aberto.");
                    window.push_notification(aviso(Tom::Erro, frase).title("Texto não salvo").autohide(false).chave("salvar-arquivo"), cx);
                    return;
                }
            }
            seguir(window, cx);
        };
        confirmar(confirmacao, seguir, Some(liberar), window, cx);
        false
    }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: o `gpui_kit::*` traz um `test` próprio que encobriria o da biblioteca.
    use super::{Parar, confirmacao_de_parar, confirmacao_de_texto_nao_salvo, conversas_trabalhando};

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
        for parar in [Parar::Fechar, Parar::Reiniciar] {
            // O OK perigoso é o que deixa o Enter só fechar (`confirmar`).
            assert!(confirmacao_de_parar(3, parar).ok_perigo, "{parar:?}");
        }
        let fechar = confirmacao_de_parar(1, Parar::Fechar);
        assert_eq!((fechar.ok.as_ref(), fechar.cancelar.as_ref()), ("Fechar mesmo assim", "Continuar aberto"));
        assert!(fechar.texto.starts_with("Se fechar agora, ela para no meio. Ao abrir o Vitral de novo"), "{}", fechar.texto);
    }

    /// Com texto do editor que não foi para o arquivo, a pergunta diz qual arquivo, por quê e
    /// onde o texto vai ficar; as conversas trabalhando entram como detalhe.
    #[test]
    fn a_pergunta_do_texto_nao_salvo_diz_onde_ele_fica() {
        let so_texto = confirmacao_de_texto_nao_salvo("carrinho.py", "o arquivo mudou no disco", "~/.local/share/vitral/nao_salvos", 0, Parar::Fechar);
        assert_eq!(so_texto.titulo, "Texto não salvo");
        assert!(
            so_texto.texto.starts_with("carrinho.py: o arquivo mudou no disco. Fechando agora, o que você escreveu fica numa cópia em ~/.local/share/vitral/nao_salvos."),
            "{}",
            so_texto.texto
        );
        assert!(so_texto.detalhes.is_empty() && so_texto.ok_perigo);
        assert_eq!((so_texto.ok.as_ref(), so_texto.cancelar.as_ref()), ("Fechar mesmo assim", "Continuar aberto"));
        let com_conversas = confirmacao_de_texto_nao_salvo("a.rs", "a gravação falhou", "/d", 2, Parar::Reiniciar);
        assert!(com_conversas.texto.contains("Reiniciando agora"), "{}", com_conversas.texto);
        assert_eq!(com_conversas.detalhes.len(), 1);
        assert_eq!(com_conversas.detalhes[0].1.as_ref(), "2 conversas estão trabalhando: elas param no meio.");
        assert_eq!(com_conversas.ok.as_ref(), "Reiniciar mesmo assim");
    }
}
