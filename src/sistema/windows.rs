//! No Windows: VS Code, um console (PowerShell, ou o Git Bash para rodar um comando) e a Lixeira.

use std::{
    os::windows::{ffi::OsStrExt as _, process::CommandExt as _},
    path::{Path, PathBuf},
    process::Command,
};

use windows_sys::Win32::{
    Foundation::ERROR_ACCESS_DENIED,
    System::Threading::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_CONSOLE, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW},
    UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING, SHFILEOPSTRUCTW,
        SHFileOperationW,
    },
};

use crate::processos::novo_comando;

/// Fora do grupo e de um job do Vitral: o programa continua aberto se ele fechar. Com `console`,
/// numa janela de console própria (no Windows 11, ela abre no Terminal); sem, sem janela nenhuma
/// (o VS Code e o Explorer abrem a deles). Um job que não deixa sair (o Vitral aberto de dentro de
/// outro programa) recusa o `CREATE_BREAKAWAY_FROM_JOB`: aí vai sem ele.
fn soltar(comando: &mut Command, console: bool) {
    let janela = if console { CREATE_NEW_CONSOLE } else { CREATE_NO_WINDOW };
    for variavel in super::MARCAS_DO_CLAUDE {
        comando.env_remove(variavel);
    }
    comando.creation_flags(janela | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
    let aberto = comando.spawn().or_else(|erro| {
        if erro.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
            comando.creation_flags(janela | CREATE_NEW_PROCESS_GROUP);
            comando.spawn()
        } else {
            Err(erro)
        }
    });
    if let Err(erro) = aberto {
        eprintln!("vitral: não abriu {:?}: {erro}", comando.get_program());
    }
}

/// O `Code.exe` da instalação do usuário (a padrão), depois o de todos os usuários; sem nenhum, o
/// `code` do PATH, que é um `.cmd` e precisa do cmd (sem janela).
pub fn abrir_no_editor(pasta: &Path) {
    let instalado = [("LOCALAPPDATA", r"Programs\Microsoft VS Code\Code.exe"), ("ProgramFiles", r"Microsoft VS Code\Code.exe")]
        .into_iter()
        .filter_map(|(variavel, dentro)| Some(PathBuf::from(std::env::var_os(variavel)?).join(dentro)))
        .find(|code| code.is_file());
    match instalado {
        Some(code) => soltar(novo_comando(code).arg(pasta), false),
        None => soltar(novo_comando("cmd.exe").arg("/c").arg("code").arg(pasta), false),
    }
}

/// Abre um console na pasta: sem `comando`, o PowerShell; com, o Git Bash roda ele e fica aberto
/// depois (os comandos, como o instalador do Codex, são escritos para o bash). Sem o Git Bash, o PowerShell tenta
/// assim mesmo, com um aviso. O comando não passa pelo `wt.exe`, que trata `;` como separador de
/// abas.
pub fn abrir_terminal(pasta: &Path, comando: Option<&str>) {
    let mut terminal = match (comando, super::bash()) {
        (Some(comando), Some(bash)) => {
            let mut terminal = novo_comando(bash);
            terminal.arg("-lc").arg(format!("{comando}; exec bash")).env("CHERE_INVOKING", "1");
            terminal
        }
        (Some(comando), None) => {
            let mut terminal = novo_comando("powershell.exe");
            let aviso = format!("Write-Host '{}'", super::SEM_GIT_BASH.replace('\'', "''"));
            terminal.args(["-NoExit", "-Command", &format!("{aviso}; {comando}")]);
            terminal
        }
        (None, _) => {
            let mut terminal = novo_comando("powershell.exe");
            terminal.arg("-NoExit");
            terminal
        }
    };
    terminal.current_dir(pasta);
    soltar(&mut terminal, true);
}

/// Manda o arquivo ou a pasta para a Lixeira (dá para restaurar pelo Explorer). Nunca apaga: o
/// `FOF_WANTNUKEWARNING` faz o Windows perguntar quando ela não cabe na Lixeira (seria apagada de
/// vez), e o "não" deixa tudo no lugar. Sem `FOF_ALLOWUNDO` aceito, devolve o erro.
pub fn para_a_lixeira(caminho: &Path) -> anyhow::Result<()> {
    let absoluto = std::path::absolute(caminho)?;
    // Uma lista de caminhos, cada um terminado em zero, e mais um zero no fim.
    let mut de: Vec<u16> = absoluto.as_os_str().encode_wide().collect();
    de.extend([0, 0]);
    let mut operacao = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: de.as_ptr(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT | FOF_WANTNUKEWARNING) as u16,
        ..Default::default()
    };
    // SAFETY: `de` vive até o fim da chamada e termina com dois zeros; o resto é nulo ou número.
    let codigo = unsafe { SHFileOperationW(&mut operacao) };
    if operacao.fAnyOperationsAborted != 0 {
        anyhow::bail!("a Lixeira não foi usada e {} ficou no lugar", caminho.display());
    }
    if codigo == 0 && caminho.symlink_metadata().is_err() {
        return Ok(());
    }
    anyhow::bail!("a Lixeira não aceitou {} (erro {codigo:#x} do Windows)", caminho.display())
}

/// A Lixeira no Explorer.
pub fn abrir_lixeira() {
    soltar(novo_comando("explorer.exe").arg("shell:RecycleBinFolder"), false);
}
