//! O que fazer com `vitral <pasta>` e `vitral --nova <destino> [mensagem]`, ao abrir ou quando outra
//! abertura entrega o pedido pelo socket.

use std::path::{Path, PathBuf};

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use super::{
    Janela, Vista,
    componentes::{Tom, aviso},
};
use crate::{
    instancia::Abertura,
    migracao,
    modelo::{ID_MESTRE, Reforco},
    motor::Resolucao,
};

/// Para onde a pasta do argumento leva, na primeira regra que servir.
#[derive(Clone, Debug, PartialEq)]
enum DestinoDaPasta {
    /// Uma pasta antiga que a migração trouxe: a visão de todos os projetos.
    Todos,
    /// A pasta já é de um projeto: ele.
    Projeto(String),
    /// "Novo projeto" com a pasta já ligada. `antigo`: ela tem conversas do Vitral antigo, que
    /// esta versão só trouxe na primeira abertura.
    NovoProjeto { antigo: bool },
}

fn destino_da_pasta(pasta: &Path, migradas: &[PathBuf], projeto: Option<String>, antigo: bool) -> DestinoDaPasta {
    if migradas.iter().any(|m| m == pasta) {
        return DestinoDaPasta::Todos;
    }
    match projeto {
        Some(id) => DestinoDaPasta::Projeto(id),
        None => DestinoDaPasta::NovoProjeto { antigo },
    }
}

/// Uma pasta ligada que contém a pasta nova, ou que fica dentro dela.
fn vizinha<'a>(pasta: &Path, ligadas: impl Iterator<Item = &'a Path>) -> Option<&'a Path> {
    ligadas.into_iter().find(|l| l.starts_with(pasta) || pasta.starts_with(l))
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
            let destino = destino_da_pasta(
                &pasta,
                &migracao::raizes_migradas(&m.dados),
                m.projeto_da_pasta(&pasta),
                migracao::tem_vitral_antigo(&pasta),
            );
            match destino {
                DestinoDaPasta::Todos => {
                    self.mudar_vista(Vista::Geral, cx);
                    self.mostrar_lista(window, cx);
                }
                DestinoDaPasta::Projeto(id) => {
                    self.mudar_vista(Vista::Projeto(id), cx);
                    self.mostrar_lista(window, cx);
                }
                DestinoDaPasta::NovoProjeto { antigo } => {
                    let ligadas: Vec<PathBuf> =
                        m.projetos.iter().flat_map(|p| p.pastas.iter().map(|l| l.caminho.clone())).collect();
                    if antigo {
                        // O aviso fica na conversa do Mestre, que é quem fala dos dados de antes.
                        let texto = "Esta pasta tem conversas do Vitral antigo, que esta versão só traz na primeira abertura.";
                        motor.update(cx, |m, cx| {
                            m.avisar_no_mestre(texto);
                            cx.notify();
                        });
                    }
                    if let Some(outra) = vizinha(&pasta, ligadas.iter().map(PathBuf::as_path)) {
                        let texto = format!("{} já está ligada a um projeto e fica junto desta.", outra.display());
                        window.push_notification(aviso(Tom::Atencao, texto), cx);
                    }
                    self.dialogo_projeto_novo(String::new(), Some(pasta), window, cx);
                }
            }
        }
        if let Some((destino, mensagem)) = abertura.nova {
            let resolvido = motor.read(cx).projeto_por_nome(&destino);
            match (destino.eq_ignore_ascii_case("mestre"), resolvido) {
                (false, Resolucao::Um(id)) => match mensagem {
                    Some(mensagem) => self.comecar_conversa(&id, mensagem, window, cx),
                    None => self.nova_conversa(&id, window, cx),
                },
                // O Mestre abre no painel ativo; a mensagem, se veio, vai a ele.
                (true, _) => {
                    self.abrir_mestre(window, cx);
                    if let Some(mensagem) = mensagem {
                        motor.update(cx, |m, cx| m.enviar_do_usuario(ID_MESTRE, mensagem, Vec::new(), Reforco::default(), cx));
                    }
                }
                // Sem mensagem, o destino que não é um projeto (ou é mais de um) fica citado na caixa
                // do Mestre; com mensagem, o Mestre decide para onde vai.
                (false, _) => match mensagem {
                    None => self.pedir_ao_mestre(&format!("#{destino} "), window, cx),
                    Some(mensagem) => {
                        self.abrir_mestre(window, cx);
                        let texto = format!("#{destino} {mensagem}");
                        motor.update(cx, |m, cx| m.enviar_do_usuario(ID_MESTRE, texto, Vec::new(), Reforco::default(), cx));
                    }
                },
            }
        }
        cx.notify();
    }
}

#[cfg(test)]
mod testes {
    use std::path::{Path, PathBuf};

    use super::{DestinoDaPasta, destino_da_pasta, vizinha};

    #[test]
    fn a_pasta_do_argumento_na_primeira_regra_que_serve() {
        let migradas = [PathBuf::from("/home/v/Projetos")];
        let pasta = Path::new("/home/v/Projetos");
        assert_eq!(destino_da_pasta(pasta, &migradas, Some("x".into()), true), DestinoDaPasta::Todos);
        let loja = Path::new("/home/v/Projetos/loja");
        assert_eq!(destino_da_pasta(loja, &migradas, Some("loja".into()), false), DestinoDaPasta::Projeto("loja".into()));
        assert_eq!(destino_da_pasta(loja, &migradas, None, true), DestinoDaPasta::NovoProjeto { antigo: true });
        assert_eq!(destino_da_pasta(loja, &[], None, false), DestinoDaPasta::NovoProjeto { antigo: false });
        let ligadas = [PathBuf::from("/home/v/Projetos/loja/api"), PathBuf::from("/home/v/site")];
        let caminhos = || ligadas.iter().map(PathBuf::as_path);
        assert_eq!(vizinha(loja, caminhos()), Some(Path::new("/home/v/Projetos/loja/api")));
        assert_eq!(vizinha(Path::new("/home/v/site/docs"), caminhos()), Some(Path::new("/home/v/site")));
        assert_eq!(vizinha(Path::new("/home/v/outro"), caminhos()), None);
    }
}
