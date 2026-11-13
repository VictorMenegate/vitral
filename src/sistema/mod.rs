//! Abrir coisas fora do Vitral (editor, terminal, gerenciador de arquivos, Lixeira), o bash dos
//! comandos de Rodar e o que precisa estar instalado.
//!
//! - No Linux (`linux.rs`): o VS Code, o Ptyxis, o `gio` e o bash do sistema.
//! - No Windows (`windows.rs`): o VS Code, um console novo (PowerShell, ou o Git Bash para rodar um
//!   comando), a Lixeira do Explorer e o bash do Git para Windows (o mesmo que o Claude Code usa).

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

#[cfg(not(windows))]
mod linux;
#[cfg(not(windows))]
use linux as plataforma;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as plataforma;

pub use plataforma::{abrir_no_editor, abrir_terminal};

/// As marcas de uma sessão do Claude Code que um programa aberto pelo Vitral não deve herdar (ele
/// pode ter sido aberto de dentro de uma).
const MARCAS_DO_CLAUDE: [&str; 3] = ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SSE_PORT"];

/// Sem o Git Bash, o comando escrito para o bash vai para o PowerShell, com este aviso.
#[cfg(windows)]
const SEM_GIT_BASH: &str =
    "(Sem o Git Bash, rodei no PowerShell um comando escrito para o bash. Instale o Git para Windows: git-scm.com.)";

/// Abre a pasta (ou o arquivo) no gerenciador de arquivos. No Linux pelo `file://` (o portal do
/// GNOME); no Windows pelo Explorer, porque `file://C:\…` não é uma URL.
pub fn abrir_pasta(caminho: &Path, cx: &gpui_kit::App) {
    #[cfg(windows)]
    cx.open_with_system(caminho);
    #[cfg(not(windows))]
    cx.open_url(&format!("file://{}", caminho.display()));
}

/// O bash dos comandos de Rodar. No Linux, o do sistema. No Windows, o do Git para Windows, por
/// caminho absoluto (`bash_do_git`); None sem ele.
pub fn bash() -> Option<PathBuf> {
    if cfg!(windows) {
        let pasta = |variavel: &str| std::env::var_os(variavel).filter(|v| !v.is_empty()).map(PathBuf::from);
        let instalacoes: Vec<PathBuf> = [pasta("ProgramFiles"), pasta("LOCALAPPDATA").map(|p| p.join("Programs"))]
            .into_iter()
            .flatten()
            .map(|p| p.join("Git"))
            .collect();
        bash_do_git(pasta("CLAUDE_CODE_GIT_BASH_PATH"), &instalacoes, std::env::var_os("PATH").as_deref())
    } else {
        Some(PathBuf::from("bash"))
    }
}

/// O `bash.exe` do Git para Windows, na ordem do Claude Code: a variável
/// `CLAUDE_CODE_GIT_BASH_PATH`, o `bin\bash.exe` de cada instalação (`%ProgramFiles%\Git` e a só do
/// usuário, em `%LOCALAPPDATA%\Programs\Git`) e, pelo `git.exe` do PATH, `..\bin\bash.exe` (o
/// `Git\cmd\git.exe`) ou `..\..\bin\bash.exe` (o `Git\mingw64\bin\git.exe`). Nunca um `bash.exe`
/// solto do PATH: o de `C:\Windows\System32` é o do WSL, que roda em outro sistema.
fn bash_do_git(variavel: Option<PathBuf>, instalacoes: &[PathBuf], path: Option<&OsStr>) -> Option<PathBuf> {
    let bash = |git: &Path| git.join("bin").join("bash.exe");
    let pelo_git = path.into_iter().flat_map(std::env::split_paths).filter(|pasta| pasta.join("git.exe").is_file()).flat_map(|pasta| {
        let acima = pasta.parent().map(Path::to_path_buf);
        let duas_acima = acima.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        [acima, duas_acima].into_iter().flatten().map(|git| bash(&git))
    });
    variavel
        .into_iter()
        .chain(instalacoes.iter().map(|git| bash(git)))
        .chain(pelo_git)
        .find(|candidato| candidato.is_file())
}

/// O programa no PATH (no Windows, o `.exe`).
fn no_path(nome: &str) -> Option<PathBuf> {
    let nome = if cfg!(windows) { format!("{nome}.exe") } else { nome.to_string() };
    std::env::split_paths(&std::env::var_os("PATH")?).map(|pasta| pasta.join(&nome)).find(|p| p.is_file())
}

/// O que falta instalar para o Vitral funcionar (o git, o Claude Code e, no Windows, o Git Bash) e
/// as pastas de chat no OneDrive, como avisos na janela.
pub fn avisos_de_instalacao(pastas: &[PathBuf]) -> Vec<String> {
    let mut avisos = Vec::new();
    let git = no_path("git").is_some();
    if !git {
        avisos.push(if cfg!(windows) {
            "Falta o Git para Windows: o Claude Code usa o Git Bash dele para rodar comandos. Instale pelo \
             git-scm.com (ou, no PowerShell, winget install Git.Git) e abra o Vitral de novo."
                .to_string()
        } else {
            "Falta o git: o Vitral mostra o estado dos repositórios com ele. Instale pelo gerenciador de pacotes do \
             sistema e abra o Vitral de novo."
                .to_string()
        });
    } else if bash().is_none() {
        avisos.push(
            "Achei o git, mas não o Git Bash: os comandos de Rodar vão para o PowerShell, e um comando escrito \
             para o bash pode falhar. Instale o Git para Windows completo (git-scm.com)."
                .to_string(),
        );
    }
    if !crate::claude::binario_claude().is_absolute() {
        avisos.push(if cfg!(windows) {
            "Falta o Claude Code: no PowerShell, rode irm https://claude.ai/install.ps1 | iex e abra o Vitral de novo."
                .to_string()
        } else {
            "Falta o Claude Code: no terminal, rode curl -fsSL https://claude.ai/install.sh | bash e abra o Vitral de \
             novo."
                .to_string()
        });
    }
    if cfg!(windows) {
        let nuvens: Vec<PathBuf> = ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"]
            .iter()
            .filter_map(|v| std::env::var_os(v).filter(|v| !v.is_empty()).map(PathBuf::from))
            .collect();
        for pasta in pastas.iter().filter(|p| no_onedrive(p, &nuvens)) {
            avisos.push(format!(
                "A pasta {} está no OneDrive: o node_modules e os builds iriam para a nuvem, e a sincronização segura \
                 arquivos no meio do git. Prefira uma pasta fora dele, como C:\\Projetos.",
                pasta.display()
            ));
        }
    }
    avisos
}

/// A pasta fica dentro de uma das pastas do OneDrive (as variáveis `OneDrive*` do Windows).
fn no_onedrive(pasta: &Path, nuvens: &[PathBuf]) -> bool {
    nuvens.iter().any(|nuvem| pasta.starts_with(nuvem))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn bash_do_git_nunca_e_o_do_system32() {
        let base = std::env::temp_dir().join(format!("vitral-bash-{}", crate::modelo::novo_id()));
        let (system32, git) = (base.join("System32"), base.join("Git"));
        for pasta in [system32.clone(), git.join("cmd"), git.join("bin"), git.join("mingw64").join("bin")] {
            std::fs::create_dir_all(pasta).unwrap();
        }
        // O bash.exe do WSL no PATH, sem git: nada.
        std::fs::write(system32.join("bash.exe"), "").unwrap();
        let path = |pastas: &[&Path]| std::env::join_paths(pastas).unwrap();
        assert_eq!(bash_do_git(None, &[], Some(&path(&[&system32]))), None);
        // O git.exe do PATH (Git\cmd) leva ao Git\bin\bash.exe, mesmo com o do WSL antes.
        let certo = git.join("bin").join("bash.exe");
        std::fs::write(&certo, "").unwrap();
        std::fs::write(git.join("cmd").join("git.exe"), "").unwrap();
        assert_eq!(bash_do_git(None, &[], Some(&path(&[&system32, &git.join("cmd")]))), Some(certo.clone()));
        // O git.exe de Git\mingw64\bin também.
        let mingw = git.join("mingw64").join("bin");
        std::fs::write(mingw.join("git.exe"), "").unwrap();
        assert_eq!(bash_do_git(None, &[], Some(&path(&[&mingw]))), Some(certo.clone()));
        // A instalação conhecida vem antes do PATH, e a variável do Claude Code antes de tudo (só se existe).
        assert_eq!(bash_do_git(None, std::slice::from_ref(&git), None), Some(certo.clone()));
        let escolhido = base.join("escolhido.exe");
        assert_eq!(bash_do_git(Some(escolhido.clone()), &[git.clone()], None), Some(certo.clone()));
        std::fs::write(&escolhido, "").unwrap();
        assert_eq!(bash_do_git(Some(escolhido.clone()), &[git.clone()], None), Some(escolhido));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn pasta_no_onedrive() {
        let nuvem = std::env::temp_dir().join("OneDrive - Empresa");
        assert!(no_onedrive(&nuvem.join("Documentos").join("loja"), std::slice::from_ref(&nuvem)));
        assert!(!no_onedrive(&std::env::temp_dir().join("OneDrive - Empresa2"), std::slice::from_ref(&nuvem)));
        assert!(!no_onedrive(&nuvem, &[]));
    }
}
