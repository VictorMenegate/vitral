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

/// Como o `git status` vê um arquivo mudado na pasta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstadoNoGit {
    Mudado,
    /// Fora do git ainda (`??`) ou acrescentado.
    Novo,
    Apagado,
    Renomeado,
}

/// Um arquivo mudado desde o último commit: o caminho relativo à raiz do repositório e as linhas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MudancaNaPasta {
    pub caminho: String,
    pub estado: EstadoNoGit,
    pub entram: u32,
    pub saem: u32,
}

/// O que mudou num repositório desde o último commit, inclusive o que um comando mudou (o que a
/// lista de Mudanças do chat não vê).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetratoDaPasta {
    pub raiz: PathBuf,
    pub mudancas: Vec<MudancaNaPasta>,
    /// Mais que `MAXIMO_NO_RETRATO` arquivos: a lista mostra os primeiros.
    pub cortado: bool,
}

const MAXIMO_NO_RETRATO: usize = 200;

/// O retrato da pasta: `None` fora de um repositório.
pub fn retrato_da_pasta(pasta: &Path) -> Option<RetratoDaPasta> {
    let raiz = PathBuf::from(git(pasta, &["rev-parse", "--show-toplevel"]).ok()?);
    let status = git(&raiz, &["status", "--porcelain=v1", "-z", "--untracked-files=normal"]).ok()?;
    // Num repositório sem commit ainda o HEAD não existe: sem números, só a lista.
    let numeros = git(&raiz, &["diff", "HEAD", "--numstat", "-z"]).unwrap_or_default();
    let mut mudancas = mudancas_do_status(&status);
    let linhas = linhas_do_numstat(&numeros);
    for m in &mut mudancas {
        if let Some((entram, saem)) = linhas.iter().find(|(c, _, _)| *c == m.caminho).map(|(_, e, s)| (*e, *s)) {
            (m.entram, m.saem) = (entram, saem);
        }
    }
    let cortado = mudancas.len() > MAXIMO_NO_RETRATO;
    mudancas.truncate(MAXIMO_NO_RETRATO);
    Some(RetratoDaPasta { raiz, mudancas, cortado })
}

/// As entradas do `git status --porcelain=v1 -z`: "XY caminho\0" (o renomeado traz o nome
/// antigo no campo seguinte).
fn mudancas_do_status(status: &str) -> Vec<MudancaNaPasta> {
    let mut campos = status.split('\0').filter(|c| !c.is_empty());
    let mut saida = Vec::new();
    while let Some(campo) = campos.next() {
        if campo.len() < 4 {
            continue;
        }
        let (xy, caminho) = (&campo[..2], &campo[3..]);
        let estado = if xy == "??" || xy.contains('A') {
            EstadoNoGit::Novo
        } else if xy.contains('D') {
            EstadoNoGit::Apagado
        } else if xy.contains('R') || xy.contains('C') {
            let _ = campos.next();
            EstadoNoGit::Renomeado
        } else {
            EstadoNoGit::Mudado
        };
        saida.push(MudancaNaPasta { caminho: caminho.to_string(), estado, entram: 0, saem: 0 });
    }
    saida
}

/// O `git diff --numstat -z`: "entram\tsaem\tcaminho\0", ou, num renomeado, "entram\tsaem\t\0velho\0novo\0".
/// Binário vem com "-": conta zero.
fn linhas_do_numstat(numstat: &str) -> Vec<(String, u32, u32)> {
    let mut campos = numstat.split('\0');
    let mut saida = Vec::new();
    while let Some(campo) = campos.next() {
        let mut partes = campo.splitn(3, '\t');
        let (Some(e), Some(s), Some(caminho)) = (partes.next(), partes.next(), partes.next()) else {
            continue;
        };
        let caminho = if caminho.is_empty() {
            let _ = campos.next();
            campos.next().unwrap_or_default()
        } else {
            caminho
        };
        saida.push((caminho.to_string(), e.parse().unwrap_or(0), s.parse().unwrap_or(0)));
    }
    saida
}

/// O diff unificado de um arquivo desde o último commit (vazio se não há o que mostrar).
pub fn diff_do_arquivo(raiz: &Path, caminho: &str) -> String {
    git(raiz, &["diff", "HEAD", "-U3", "--no-color", "--", caminho]).unwrap_or_default()
}

/// Desfaz o que mudou num arquivo que o git conhece: ele volta ao último commit (o que estava
/// preparado para commit também sai).
pub fn desfazer_arquivo(raiz: &Path, caminho: &str) -> Result<()> {
    git(raiz, &["restore", "--source=HEAD", "--staged", "--worktree", "--", caminho])?;
    Ok(())
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
    fn le_o_status_e_os_numeros() {
        let status = " M src/a.rs\0?? novo.txt\0D  velho.rs\0R  b2.rs\0b.rs\0A  c.rs\0";
        let lidas: Vec<(String, EstadoNoGit)> = mudancas_do_status(status).into_iter().map(|m| (m.caminho, m.estado)).collect();
        assert_eq!(
            lidas,
            [
                ("src/a.rs".to_string(), EstadoNoGit::Mudado),
                ("novo.txt".to_string(), EstadoNoGit::Novo),
                ("velho.rs".to_string(), EstadoNoGit::Apagado),
                ("b2.rs".to_string(), EstadoNoGit::Renomeado),
                ("c.rs".to_string(), EstadoNoGit::Novo),
            ]
        );
        let numstat = "3\t1\tsrc/a.rs\0-\t-\timagem.png\00\t0\t\0b.rs\0b2.rs\0";
        assert_eq!(
            linhas_do_numstat(numstat),
            [("src/a.rs".to_string(), 3, 1), ("imagem.png".to_string(), 0, 0), ("b2.rs".to_string(), 0, 0)]
        );
    }

    #[test]
    fn retrato_de_um_repositorio_de_verdade() {
        let pasta = std::env::temp_dir().join(format!("vitral-git-retrato-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        git(&pasta, &["init", "-q"]).unwrap();
        // No Windows do CI o git converte o fim de linha ao restaurar: aqui, o arquivo volta igual.
        git(&pasta, &["config", "core.autocrlf", "false"]).unwrap();
        std::fs::write(pasta.join("a.txt"), "um\ndois\n").unwrap();
        git(&pasta, &["add", "a.txt"]).unwrap();
        git(&pasta, &["commit", "-q", "-m", "primeiro"]).unwrap();
        std::fs::write(pasta.join("a.txt"), "um\ntres\nquatro\n").unwrap();
        std::fs::write(pasta.join("b.txt"), "novo\n").unwrap();
        let retrato = retrato_da_pasta(&pasta).unwrap();
        let a = retrato.mudancas.iter().find(|m| m.caminho == "a.txt").unwrap();
        assert_eq!((a.estado, a.entram, a.saem), (EstadoNoGit::Mudado, 2, 1));
        assert!(retrato.mudancas.iter().any(|m| m.caminho == "b.txt" && m.estado == EstadoNoGit::Novo));
        assert!(diff_do_arquivo(&retrato.raiz, "a.txt").contains("+tres"));
        desfazer_arquivo(&retrato.raiz, "a.txt").unwrap();
        assert_eq!(std::fs::read_to_string(pasta.join("a.txt")).unwrap(), "um\ndois\n");
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn caminho_relativo_com_barra_normal() {
        assert_eq!(caminho_em_texto(&Path::new("src").join("ui").join("mod.rs")), "src/ui/mod.rs");
        assert_eq!(caminho_em_texto(Path::new("relatório.md")), "relatório.md");
    }
}
