//! Processos que o Vitral abre. Os que ele precisa conseguir parar por inteiro (claude,
//! verificação, revisor) saem de [`comando`] e rodam com [`abrir`], que devolve o processo e o
//! [`Escopo`] dele: parar o escopo derruba também o que ele abriu, até os shells que o claude separa
//! (servidores, testes, cargo), e tudo morre se o Vitral morrer.
//!
//! - No Linux (`unix.rs`), um escopo do systemd do usuário; sem systemd, um grupo de processos.
//! - No Windows (`windows.rs`), um Job Object: encerrá-lo derruba a árvore inteira, e ele se encerra
//!   sozinho quando o último handle fecha (o Vitral morrer fecha todos).
//!
//! Os outros (git, gio, gsettings, notify-send) saem de [`novo_comando`]: no Windows, um
//! `Command::new` direto abriria uma janela de console a cada git. O teste `todo_processo_sai_daqui`
//! barra o `Command::new` fora desta pasta.

use std::{
    ffi::OsStr,
    ops::{Deref, DerefMut},
    process::{Child, Command, ExitStatus},
    time::{Duration, Instant},
};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as plataforma;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as plataforma;

pub use plataforma::{Escopo, abrir, codigo_de_saida, colher, comando, comando_leve, encerrar_todos, limpar_sobras};
#[cfg(test)]
pub use plataforma::morre_logo;
#[cfg(all(test, unix))]
pub use plataforma::usa_systemd;

/// Um processo pronto para [`abrir`]: argumentos, pasta, ambiente e pipes vão no `Command` de
/// dentro (pelo `DerefMut`), como num `Command` comum. Só o `abrir` põe para rodar.
pub struct Comando {
    comando: Command,
    /// O escopo do systemd onde ele vai rodar (None: o grupo de processos próprio).
    #[cfg(unix)]
    unidade: Option<String>,
}

impl Deref for Comando {
    type Target = Command;

    fn deref(&self) -> &Command {
        &self.comando
    }
}

impl DerefMut for Comando {
    fn deref_mut(&mut self) -> &mut Command {
        &mut self.comando
    }
}

/// O `Command` de todo processo do Vitral.
#[cfg(unix)]
pub fn novo_comando(programa: impl AsRef<OsStr>) -> Command {
    Command::new(programa)
}

/// O `Command` de todo processo do Vitral, sem janela de console (`CREATE_NO_WINDOW`): o Vitral não
/// tem console, e cada git, claude ou bash abriria uma janela preta piscando. A marca da reabertura
/// (`instancia::REABRINDO`) não passa adiante.
#[cfg(windows)]
pub fn novo_comando(programa: impl AsRef<OsStr>) -> Command {
    use std::os::windows::process::CommandExt as _;
    let mut comando = Command::new(programa);
    comando
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .env_remove(crate::instancia::REABRINDO);
    comando
}

/// Espera o processo terminar até o prazo; estourado, para tudo e dá 2 s antes de matar. No fim
/// derruba o que ficou rodando em segundo plano e colhe. None quando estourou o prazo.
pub fn esperar(filho: &mut Child, escopo: &Escopo, prazo: Duration) -> Option<ExitStatus> {
    let limite = Instant::now() + prazo;
    let mut estourou = false;
    while !escopo.terminou() {
        if Instant::now() > limite {
            estourou = true;
            escopo.parar();
            let folga = Instant::now() + Duration::from_secs(2);
            while !escopo.terminou() && Instant::now() < folga {
                std::thread::sleep(Duration::from_millis(100));
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    escopo.matar();
    let status = filho.wait().ok();
    if estourou { None } else { status }
}

#[cfg(test)]
mod testes {
    use std::path::Path;

    /// Todo processo sai de `novo_comando` (ou de `comando`). O `Command` da lista com busca do kit
    /// (`gpui_kit::component::command`) não é processo: vale no arquivo que importa ele e não usa o
    /// `std::process`.
    #[test]
    fn todo_processo_sai_daqui() {
        let raiz = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        let mut erros = Vec::new();
        for caminho in crate::tema::testes::arquivos_rs(raiz) {
            let relativo = caminho.strip_prefix(raiz).unwrap();
            if relativo.starts_with("processos") {
                continue;
            }
            let texto = std::fs::read_to_string(&caminho).unwrap();
            let paleta_do_kit = texto.contains("command::{") && !texto.contains("process::");
            for (numero, linha) in texto.lines().enumerate() {
                if linha.contains("Command::new") && !paleta_do_kit {
                    erros.push(format!("{}:{}: {}", relativo.display(), numero + 1, linha.trim()));
                }
            }
        }
        assert!(erros.is_empty(), "processo aberto fora de processos::novo_comando:\n{}", erros.join("\n"));
    }
}
