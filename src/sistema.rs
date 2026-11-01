//! Abrir coisas fora do Vitral: VS Code, terminal (Ptyxis) e comandos do projeto.

use std::{path::Path, process::Command};

fn soltar(comando: &mut Command) {
    // Fora do grupo do Vitral: o programa continua aberto se o Vitral fechar.
    use std::os::unix::process::CommandExt;
    comando.process_group(0);
    for variavel in ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SSE_PORT"] {
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
    soltar(Command::new(editor).arg(pasta));
}

/// Abre um terminal na pasta; com `comando`, roda ele e deixa o shell aberto depois.
pub fn abrir_terminal(pasta: &Path, tarefa: Option<u64>, comando: Option<&str>) {
    let mut terminal = Command::new("ptyxis");
    match comando {
        Some(comando) => {
            let pasta = pasta.display().to_string().replace('\'', "'\\''");
            terminal.args(["--", "bash", "-lc", &format!("cd '{pasta}' && {comando}; exec bash")]);
        }
        None => {
            terminal.arg("--new-window").arg("--working-directory").arg(pasta);
        }
    }
    if let Some(id) = tarefa {
        terminal.env("VITRAL_PORT", crate::motor::porta_da_tarefa(id).to_string());
    }
    soltar(&mut terminal);
}

/// Manda o arquivo ou a pasta para a Lixeira do GNOME (`gio trash`, que dá para restaurar no
/// Arquivos). Nunca apaga: se a lixeira não aceita, devolve o erro e tudo fica no lugar.
pub fn para_a_lixeira(caminho: &Path) -> anyhow::Result<()> {
    let saida = Command::new("gio")
        .arg("trash")
        .arg("--")
        .arg(caminho)
        .output()
        .map_err(|erro| anyhow::anyhow!("o gio não abriu: {erro}"))?;
    if saida.status.success() && caminho.symlink_metadata().is_err() {
        return Ok(());
    }
    let motivo = String::from_utf8_lossy(&saida.stderr).trim().to_string();
    let motivo = motivo.strip_prefix("gio: ").unwrap_or(&motivo).to_string();
    if motivo.is_empty() {
        anyhow::bail!("a lixeira não aceitou {}", caminho.display());
    }
    anyhow::bail!("{motivo}")
}

/// A Lixeira no Arquivos do GNOME.
pub fn abrir_lixeira() {
    soltar(Command::new("gio").args(["open", "trash:///"]));
}
