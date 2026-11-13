//! No Linux: VS Code, terminal (Ptyxis) e a Lixeira do GNOME.

use std::{path::Path, process::Command};

use crate::processos::novo_comando;

fn soltar(comando: &mut Command) {
    // Fora do grupo do Vitral: o programa continua aberto se o Vitral fechar.
    use std::os::unix::process::CommandExt;
    comando.process_group(0);
    for variavel in super::MARCAS_DO_CLAUDE {
        comando.env_remove(variavel);
    }
    if let Err(erro) = comando.spawn() {
        eprintln!("vitral: não abriu {:?}: {erro}", comando.get_program());
    }
}

pub fn abrir_no_editor(pasta: &Path) {
    let editor = ["/usr/bin/code", "code"]
        .into_iter()
        .find(|e| !e.starts_with('/') || Path::new(e).exists())
        .unwrap_or("code");
    soltar(novo_comando(editor).arg(pasta));
}

/// Abre um terminal na pasta; com `comando`, roda ele e deixa o shell aberto depois.
pub fn abrir_terminal(pasta: &Path, comando: Option<&str>) {
    let mut terminal = novo_comando("ptyxis");
    match comando {
        Some(comando) => {
            let pasta = pasta.display().to_string().replace('\'', "'\\''");
            terminal.args(["--", "bash", "-lc", &format!("cd '{pasta}' && {comando}; exec bash")]);
        }
        None => {
            terminal.arg("--new-window").arg("--working-directory").arg(pasta);
        }
    }
    soltar(&mut terminal);
}
