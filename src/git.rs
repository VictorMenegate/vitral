//! Operações de git do Vitral: o comando do git (`novo_git`), o caminho relativo em texto e de onde
//! saiu uma cópia isolada das versões de antes (a carga põe o chat de volta na origem). Todas
//! bloqueiam: rode fora da thread da interface.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Result, bail};

use crate::processos::novo_comando;

/// No Windows, `core.longpaths` em todo git do Vitral: as cópias ficam em `.vitral\worktrees\…`, e
/// um `node_modules` lá dentro passa fácil dos 260 caracteres de caminho do Windows.
const CAMINHOS_LONGOS: &[&str] = if cfg!(windows) { &["-c", "core.longpaths=true"] } else { &[] };

/// O `git` de todo comando do Vitral: o `novo_comando` com os caminhos longos do Windows. O teste
/// `todo_git_sai_daqui` barra o git aberto direto pelo `novo_comando` fora daqui.
pub fn novo_git() -> std::process::Command {
    let mut comando = novo_comando("git");
    comando.args(CAMINHOS_LONGOS);
    comando
}

/// O `git -C pasta`.
pub fn git_em(pasta: &Path) -> std::process::Command {
    let mut comando = novo_git();
    comando.arg("-C").arg(pasta);
    comando
}

/// O caminho relativo como o git escreve, com `/`: no Windows o `Path` usa `\`, e o texto vai para o
/// git ou é comparado com o que ele devolve.
pub fn caminho_em_texto(caminho: &Path) -> String {
    let texto = caminho.to_string_lossy();
    if cfg!(windows) { texto.replace('\\', "/") } else { texto.into_owned() }
}

/// Sem user.name/user.email no git, os commits do Vitral saem com uma identidade própria.
fn sem_identidade() -> bool {
    static SEM: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SEM.get_or_init(|| {
        let valor = |chave: &str| {
            novo_git()
                .args(["config", chave])
                .output()
                .map(|s| !String::from_utf8_lossy(&s.stdout).trim().is_empty())
                .unwrap_or(false)
        };
        !(valor("user.name") && valor("user.email"))
    })
}

fn git(pasta: &Path, args: &[&str]) -> Result<String> {
    let mut comando = git_em(pasta);
    // Sem isso, "ação.txt" sai como "a\303\247\303\243o.txt" (entre aspas) e nenhum caminho bate.
    comando.args(["-c", "core.quotePath=false"]);
    if sem_identidade() {
        comando.args([
            "-c",
            "user.name=Vitral",
            "-c",
            "user.email=vitral@localhost",
        ]);
    }
    comando
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        // O `git status` do próprio Vitral não segura o index.lock no meio de uma junção.
        .env("GIT_OPTIONAL_LOCKS", "0");
    let mut saida = comando.output()?;
    // Outro git na mesma pasta (o usuário, um editor, um agente) segura o index.lock por um
    // instante: espera um pouco antes de desistir.
    for _ in 0..3 {
        if saida.status.success() || !ocupado(&String::from_utf8_lossy(&saida.stderr)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
        saida = comando.output()?;
    }
    if !saida.status.success() {
        bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&saida.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&saida.stdout)
        .trim_end()
        .to_string())
}

/// Erro de um git que achou o índice preso por outro (o caminho sai igual em qualquer idioma).
fn ocupado(erro: &str) -> bool {
    erro.contains("index.lock'")
}

/// No Windows, o tamanho do arquivo: os hard links contam de novo (o total é aproximado).
#[cfg(not(unix))]
fn no_disco(dados: &std::fs::Metadata) -> (u64, Option<(u64, u64)>) {
    (dados.len(), None)
}

pub fn inicio_do_texto(texto: &str, limite: usize) -> String {
    if texto.len() <= limite {
        return texto.to_string();
    }
    let mut fim = limite;
    while !texto.is_char_boundary(fim) {
        fim -= 1;
    }
    format!(
        "{}\n… (cortado: {} caracteres a mais)",
        &texto[..fim],
        texto.len() - fim
    )
}

/// O repositório de onde uma cópia (worktree) saiu: a pasta do `git-common-dir`, canônica.
pub fn repositorio_da_copia(copia: &Path) -> Option<PathBuf> {
    let comum = git(copia, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).ok()?;
    let comum = dunce::canonicalize(Path::new(&comum)).ok()?;
    match comum.file_name() {
        Some(nome) if nome == ".git" => comum.parent().map(Path::to_path_buf),
        _ => Some(comum),
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Todo git do Vitral sai de `novo_git` (no Windows, com os caminhos longos).
    #[test]
    fn todo_git_sai_daqui() {
        let raiz = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        let mut erros = Vec::new();
        for caminho in crate::tema::testes::arquivos_rs(raiz) {
            let texto = std::fs::read_to_string(&caminho).unwrap();
            for (numero, linha) in texto.lines().enumerate() {
                let definicao = linha.trim() == concat!("let mut comando = novo_comando(", "\"git\");");
                if linha.contains(concat!("novo_comando(", "\"git\")")) && !(definicao && caminho.ends_with("git.rs")) {
                    erros.push(format!("{}:{}: {}", caminho.display(), numero + 1, linha.trim()));
                }
            }
        }
        assert!(erros.is_empty(), "git aberto fora de git::novo_git:\n{}", erros.join("\n"));
    }

    #[test]
    fn caminho_relativo_com_barra_normal() {
        assert_eq!(caminho_em_texto(&Path::new("src").join("ui").join("mod.rs")), "src/ui/mod.rs");
        assert_eq!(caminho_em_texto(Path::new("relatório.md")), "relatório.md");
    }
}
