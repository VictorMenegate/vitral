//! O que fazer com `vitral <pasta>` e `vitral --nova <pasta> [mensagem]`, ao abrir ou quando outra
//! abertura entrega o pedido pelo socket.

use std::path::{Path, PathBuf};

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Tom, aviso},
};
use crate::{instancia::Abertura, migracao};

/// Para onde a pasta do argumento leva, na primeira regra que servir.
#[derive(Clone, Debug, PartialEq)]
enum DestinoDaPasta {
    /// Uma pasta antiga que a migração trouxe: a lista dos chats.
    Lista,
    /// O chat mais recente que trabalha na pasta.
    Chat(String),
    /// Um chat novo nela. `antigo`: ela tem conversas do Vitral antigo, que esta versão só trouxe
    /// na primeira abertura.
    Novo { antigo: bool },
}

fn destino_da_pasta(pasta: &Path, migradas: &[PathBuf], chat: Option<String>, antigo: bool) -> DestinoDaPasta {
    if migradas.iter().any(|m| m == pasta) {
        return DestinoDaPasta::Lista;
    }
    match chat {
        Some(id) => DestinoDaPasta::Chat(id),
        None => DestinoDaPasta::Novo { antigo },
    }
}

impl Janela {
    pub(super) fn aplicar_abertura(&mut self, abertura: Abertura, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        if motor.read(cx).somente_leitura().is_some() {
            return;
        }
        if let Some(pasta) = abertura.pasta {
            let m = motor.read(cx);
            // O chat da pasta mexido por último; o arquivado só se não houver outro.
            let chat = m
                .chats
                .iter()
                .filter(|c| c.cwd == pasta)
                .max_by_key(|c| (!c.arquivado, c.atualizado_em))
                .filter(|c| !c.arquivado)
                .map(|c| c.id.clone());
            match destino_da_pasta(&pasta, &migracao::raizes_migradas(&m.dados), chat, migracao::tem_vitral_antigo(&pasta)) {
                DestinoDaPasta::Lista => self.mostrar_lista(window, cx),
                DestinoDaPasta::Chat(id) => self.selecionar(Some(id), window, cx),
                DestinoDaPasta::Novo { antigo } => {
                    if antigo {
                        let texto = "Esta pasta tem conversas do Vitral antigo, que esta versão só traz na primeira abertura.";
                        window.push_notification(aviso(Tom::Info, texto), cx);
                    }
                    self.nova_conversa_na_pasta(Some(pasta), window, cx);
                }
            }
        }
        if let Some((destino, mensagem)) = abertura.nova {
            let pasta = PathBuf::from(&destino);
            if pasta.is_absolute() && pasta.is_dir() {
                match mensagem {
                    Some(mensagem) => self.comecar_conversa(pasta, mensagem, window, cx),
                    None => self.nova_conversa_na_pasta(Some(pasta), window, cx),
                }
            } else {
                window.push_notification(aviso(Tom::Atencao, format!("\"{destino}\" não é uma pasta.")), cx);
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod testes {
    use std::path::{Path, PathBuf};

    use super::{DestinoDaPasta, destino_da_pasta};

    #[test]
    fn a_pasta_do_argumento_na_primeira_regra_que_serve() {
        let migradas = [PathBuf::from("/home/v/Projetos")];
        let pasta = Path::new("/home/v/Projetos");
        assert_eq!(destino_da_pasta(pasta, &migradas, Some("x".into()), true), DestinoDaPasta::Lista);
        let loja = Path::new("/home/v/Projetos/loja");
        assert_eq!(destino_da_pasta(loja, &migradas, Some("abc".into()), false), DestinoDaPasta::Chat("abc".into()));
        assert_eq!(destino_da_pasta(loja, &migradas, None, true), DestinoDaPasta::Novo { antigo: true });
        assert_eq!(destino_da_pasta(loja, &[], None, false), DestinoDaPasta::Novo { antigo: false });
    }
}
