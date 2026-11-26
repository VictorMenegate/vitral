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
    /// O ramo em que a pasta está (`None` num repositório que não deu para ler).
    pub ramo: Option<RamoDaPasta>,
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
    let ramo = ramo_no_disco(&raiz).or_else(|| ramo_pelo_git(&raiz));
    Some(RetratoDaPasta { raiz, ramo, mudancas, cortado })
}

/// Onde a pasta está no git: o ramo ou, com a cabeça solta, o commit curto, e se ela é uma cópia
/// (worktree) de outro repositório.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RamoDaPasta {
    /// "main", "feat/x" ou, com a cabeça solta, o commit curto ("1a2b3c4").
    pub nome: String,
    /// A cabeça está solta: o `nome` é um commit, não um ramo.
    pub solta: bool,
    pub worktree: bool,
}

/// Quantas letras do commit aparecem com a cabeça solta (as do `git rev-parse --short`).
const COMMIT_CURTO: usize = 7;

/// O que o arquivo `HEAD` diz: o ramo ("ref: refs/heads/feat/x") ou o commit curto e `true`, com a
/// cabeça solta. `None` no que não é uma coisa nem outra (o "refs/heads/.invalid" que o formato
/// reftable deixa no lugar: lá quem sabe o ramo é o git).
fn ler_head(conteudo: &str) -> Option<(String, bool)> {
    let conteudo = conteudo.trim();
    if let Some(referencia) = conteudo.strip_prefix("ref:") {
        let referencia = referencia.trim();
        let nome = referencia.strip_prefix("refs/heads/").unwrap_or(referencia);
        return (!nome.is_empty() && nome != ".invalid").then(|| (nome.to_string(), false));
    }
    let commit = conteudo.len() >= COMMIT_CURTO && conteudo.chars().all(|c| c.is_ascii_hexdigit());
    commit.then(|| (conteudo[..COMMIT_CURTO].to_string(), true))
}

/// A pasta do git de uma cópia, tirada do arquivo `.git` dela ("gitdir: /repo/.git/worktrees/x";
/// um caminho relativo conta a partir da pasta do arquivo).
fn pasta_do_git_no_arquivo(conteudo: &str, pasta: &Path) -> Option<PathBuf> {
    let caminho = conteudo.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    (!caminho.is_empty()).then(|| pasta.join(caminho))
}

/// O ramo da pasta lido direto dos arquivos do git (o `.git` da pasta ou de uma acima, e o `HEAD`
/// dele), sem abrir processo: barato o bastante para uma dica ou para conferir a cada poucos
/// segundos. `None` fora de um repositório.
pub fn ramo_no_disco(pasta: &Path) -> Option<RamoDaPasta> {
    let (raiz, ponto_git) = pasta.ancestors().map(|p| (p, p.join(".git"))).find(|(_, g)| g.exists())?;
    // Num worktree (e num submódulo) o `.git` é um arquivo que aponta a pasta de verdade; só a do
    // worktree tem o `commondir`, que leva ao repositório de onde ele saiu.
    let (pasta_do_git, worktree) = if ponto_git.is_dir() {
        (ponto_git, false)
    } else {
        let pasta_do_git = pasta_do_git_no_arquivo(&std::fs::read_to_string(&ponto_git).ok()?, raiz)?;
        let worktree = pasta_do_git.join("commondir").is_file();
        (pasta_do_git, worktree)
    };
    let (nome, solta) = ler_head(&std::fs::read_to_string(pasta_do_git.join("HEAD")).ok()?)?;
    Some(RamoDaPasta { nome, solta, worktree })
}

/// O mesmo pelo git, para o repositório que os arquivos não contam (o formato reftable).
fn ramo_pelo_git(raiz: &Path) -> Option<RamoDaPasta> {
    let (nome, solta) = match git(raiz, &["symbolic-ref", "--short", "-q", "HEAD"]) {
        Ok(ramo) if !ramo.is_empty() => (ramo, false),
        _ => (git(raiz, &["rev-parse", "--short", "HEAD"]).ok()?, true),
    };
    let pastas = git(raiz, &["rev-parse", "--path-format=absolute", "--git-dir", "--git-common-dir"]).ok()?;
    let mut pastas = pastas.lines();
    let worktree = pastas.next() != pastas.next();
    Some(RamoDaPasta { nome, solta, worktree })
}

/// As entradas do `git status --porcelain=v1 -z`: "XY caminho\0" (o renomeado traz o nome
/// antigo no campo seguinte).
fn mudancas_do_status(status: &str) -> Vec<MudancaNaPasta> {
    let mut campos = status.split('\0').filter(|c| !c.is_empty());
    let mut saida = Vec::new();
    while let Some(campo) = campos.next() {
        // "XY caminho": os dois códigos são ASCII; um campo que não começa assim (o nome antigo
        // de um renomeado fora de lugar, por exemplo) não é cortado no meio de um caractere.
        let (Some(xy), Some(caminho)) = (campo.get(..2), campo.get(3..)) else {
            continue;
        };
        if !xy.is_ascii() || campo.as_bytes()[2] != b' ' || caminho.is_empty() {
            continue;
        }
        // O renomeado e o copiado trazem o nome antigo no campo seguinte, seja qual for o estado
        // que sai daqui ("RD": renomeado e depois apagado): ele é consumido sempre, senão vira
        // uma entrada própria.
        let renomeado = xy.contains('R') || xy.contains('C');
        if renomeado {
            let _ = campos.next();
        }
        let estado = if xy == "??" || xy.contains('A') {
            EstadoNoGit::Novo
        } else if xy.contains('D') {
            EstadoNoGit::Apagado
        } else if renomeado {
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

/// O diff unificado de um arquivo desde o último commit (vazio se não há o que mostrar). Com
/// `sem_espaco`, o `-w` do git: a linha que só mudou de recuo ou de espaço no fim fica de fora.
pub fn diff_do_arquivo(raiz: &Path, caminho: &str, sem_espaco: bool) -> String {
    let mut args = vec!["diff", "HEAD", "-U3", "--no-color"];
    if sem_espaco {
        args.push("-w");
    }
    args.extend(["--", caminho]);
    git(raiz, &args).unwrap_or_default()
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

/// Uma cópia (worktree) de um repositório, como o `git worktree list` a conta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Copia {
    pub caminho: PathBuf,
    /// O ramo; `None` com a cabeça solta.
    pub ramo: Option<String>,
    /// O commit curto em que ela está.
    pub commit: String,
    /// A pasta dela não existe mais (o git a marca "prunable").
    pub apagada: bool,
    /// A primeira da lista: o repositório de onde as outras saíram.
    pub principal: bool,
}

/// As cópias de uma saída do `git worktree list --porcelain`: um bloco por cópia ("worktree
/// <pasta>", "HEAD <commit>", "branch refs/heads/<ramo>" ou "detached", e "prunable" na que teve a
/// pasta apagada), separados por uma linha vazia. O repositório sem pasta de trabalho ("bare")
/// fica de fora: não há onde um chat trabalhar nele.
fn ler_copias(saida: &str) -> Vec<Copia> {
    let mut copias: Vec<Copia> = Vec::new();
    let mut sem_pasta_de_trabalho = false;
    let mut primeira = true;
    for linha in saida.lines().chain([""]) {
        let (chave, valor) = linha.split_once(' ').unwrap_or((linha, ""));
        match chave {
            "worktree" => {
                copias.push(Copia { caminho: PathBuf::from(valor), ramo: None, commit: String::new(), apagada: false, principal: primeira });
                (primeira, sem_pasta_de_trabalho) = (false, false);
            }
            // O fim do bloco.
            "" => {
                if std::mem::take(&mut sem_pasta_de_trabalho) {
                    copias.pop();
                }
            }
            "bare" => sem_pasta_de_trabalho = true,
            _ => {
                let Some(copia) = copias.last_mut() else { continue };
                match chave {
                    "HEAD" => copia.commit = valor.chars().take(COMMIT_CURTO).collect(),
                    "branch" => copia.ramo = Some(valor.strip_prefix("refs/heads/").unwrap_or(valor).to_string()),
                    "prunable" => copia.apagada = true,
                    _ => {}
                }
            }
        }
    }
    copias
}

/// As cópias do repositório da pasta que ainda estão no disco (vazio fora de um repositório).
pub fn copias_do_repositorio(pasta: &Path) -> Vec<Copia> {
    let saida = git(pasta, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    ler_copias(&saida).into_iter().filter(|c| !c.apagada && c.caminho.is_dir()).collect()
}

/// As cópias dos repositórios de várias pastas, sem repetir. A pasta que já apareceu como cópia
/// não chama o git de novo; a que está dentro de uma cópia chama, porque pode ser outro
/// repositório (o `central/` dentro da raiz do workspace, que também é um).
pub fn copias_das_pastas(pastas: &[PathBuf]) -> Vec<Copia> {
    let mut copias: Vec<Copia> = Vec::new();
    for pasta in pastas {
        if copias.iter().any(|c| c.caminho == *pasta) {
            continue;
        }
        for copia in copias_do_repositorio(pasta) {
            if !copias.iter().any(|c| c.caminho == copia.caminho) {
                copias.push(copia);
            }
        }
    }
    copias
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
    fn renomeado_e_apagado_consome_o_nome_antigo() {
        // `git mv ação.txt nova.txt` e depois apagar `nova.txt`: o nome antigo vem no campo
        // seguinte e não pode virar entrada (nem ser cortado no meio do "ç").
        let status = "RD nova.txt\0ação.txt\0 M depois.rs\0CM copia.rs\0original.rs\0";
        let lidas: Vec<(String, EstadoNoGit)> = mudancas_do_status(status).into_iter().map(|m| (m.caminho, m.estado)).collect();
        assert_eq!(
            lidas,
            [
                ("nova.txt".to_string(), EstadoNoGit::Apagado),
                ("depois.rs".to_string(), EstadoNoGit::Mudado),
                ("copia.rs".to_string(), EstadoNoGit::Renomeado),
            ]
        );
        // Um campo solto com acento logo no começo é ignorado, sem pânico.
        assert!(mudancas_do_status("ção.txt\0").is_empty());
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
        assert!(diff_do_arquivo(&retrato.raiz, "a.txt", false).contains("+tres"));
        desfazer_arquivo(&retrato.raiz, "a.txt").unwrap();
        assert_eq!(std::fs::read_to_string(pasta.join("a.txt")).unwrap(), "um\ndois\n");
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn le_o_head_com_ramo_e_com_a_cabeca_solta() {
        assert_eq!(ler_head("ref: refs/heads/main\n"), Some(("main".into(), false)));
        assert_eq!(ler_head("ref: refs/heads/feat/ramo-à-vista\n"), Some(("feat/ramo-à-vista".into(), false)));
        assert_eq!(ler_head("4b825dc642cb6eb9a060e54bf8d69288fbee4904\n"), Some(("4b825dc".into(), true)));
        // O reftable deixa um HEAD de enfeite; lixo não é ramo nem commit.
        assert_eq!(ler_head("ref: refs/heads/.invalid\n"), None);
        assert_eq!(ler_head("não é um head"), None);
        assert_eq!(ler_head(""), None);
        let pasta = Path::new("/p/wt/x");
        assert_eq!(pasta_do_git_no_arquivo("gitdir: /p/repo/.git/worktrees/x\n", pasta), Some(PathBuf::from("/p/repo/.git/worktrees/x")));
        assert_eq!(pasta_do_git_no_arquivo("gitdir: ../../repo/.git/worktrees/x\n", pasta), Some(pasta.join("../../repo/.git/worktrees/x")));
        assert_eq!(pasta_do_git_no_arquivo("outra coisa", pasta), None);
    }

    /// Um repositório de verdade num ramo, com a cabeça solta e num worktree: os arquivos e o git
    /// contam a mesma coisa.
    #[test]
    fn ramo_com_cabeca_solta_e_em_worktree() {
        let base = std::env::temp_dir().join(format!("vitral-git-ramo-{}", crate::modelo::novo_id()));
        let pasta = base.join("repo");
        std::fs::create_dir_all(pasta.join("src")).unwrap();
        git(&pasta, &["init", "-q"]).unwrap();
        git(&pasta, &["checkout", "-q", "-b", "principal"]).unwrap();
        std::fs::write(pasta.join("src/a.txt"), "um\n").unwrap();
        git(&pasta, &["add", "."]).unwrap();
        git(&pasta, &["commit", "-q", "-m", "primeiro"]).unwrap();
        let no_ramo = RamoDaPasta { nome: "principal".into(), solta: false, worktree: false };
        assert_eq!(ramo_no_disco(&pasta), Some(no_ramo.clone()));
        // De uma subpasta, o `.git` é o de cima.
        assert_eq!(ramo_no_disco(&pasta.join("src")), Some(no_ramo.clone()));
        assert_eq!(ramo_pelo_git(&pasta), Some(no_ramo.clone()));
        assert_eq!(retrato_da_pasta(&pasta).unwrap().ramo, Some(no_ramo));

        // O worktree num ramo dele.
        let copia = base.join("wt").join("copia");
        git(&pasta, &["worktree", "add", "-q", "-b", "feat/outro", &copia.to_string_lossy()]).unwrap();
        let na_copia = RamoDaPasta { nome: "feat/outro".into(), solta: false, worktree: true };
        assert_eq!(ramo_no_disco(&copia), Some(na_copia.clone()));
        assert_eq!(ramo_no_disco(&copia.join("src")), Some(na_copia.clone()));
        assert_eq!(ramo_pelo_git(&copia), Some(na_copia.clone()));
        assert_eq!(retrato_da_pasta(&copia).unwrap().ramo, Some(na_copia));

        // A cabeça solta: o commit curto no lugar do ramo.
        git(&pasta, &["checkout", "-q", "--detach"]).unwrap();
        let inteiro = git(&pasta, &["rev-parse", "HEAD"]).unwrap();
        let solta = ramo_no_disco(&pasta).unwrap();
        assert!(solta.solta && !solta.worktree);
        assert_eq!(solta.nome.len(), 7);
        assert!(inteiro.starts_with(&solta.nome));
        let pelo_git = ramo_pelo_git(&pasta).unwrap();
        assert!(pelo_git.solta && pelo_git.nome.starts_with(&solta.nome));

        // Fora de um repositório, nada.
        assert_eq!(ramo_no_disco(&base.join("wt")), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn le_o_worktree_list_com_ramo_cabeca_solta_e_copia_apagada() {
        let saida = "worktree /p/central\nHEAD 1111111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\nbranch refs/heads/main\n\n\
                     worktree /p/wt/pesagem\nHEAD 2222222bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\nbranch refs/heads/feat/pesagem\nlocked\n\n\
                     worktree /p/wt/solta\nHEAD 3333333ccccccccccccccccccccccccccccccccc\ndetached\n\n\
                     worktree /p/wt/sumiu\nHEAD 4444444ddddddddddddddddddddddddddddddddd\nbranch refs/heads/velho\nprunable gitdir file points to non-existent location\n";
        let copia = |caminho: &str, ramo: Option<&str>, commit: &str, apagada: bool, principal: bool| Copia {
            caminho: PathBuf::from(caminho),
            ramo: ramo.map(str::to_string),
            commit: commit.into(),
            apagada,
            principal,
        };
        assert_eq!(
            ler_copias(saida),
            [
                copia("/p/central", Some("main"), "1111111", false, true),
                copia("/p/wt/pesagem", Some("feat/pesagem"), "2222222", false, false),
                copia("/p/wt/solta", None, "3333333", false, false),
                copia("/p/wt/sumiu", Some("velho"), "4444444", true, false),
            ]
        );
        // O repositório sem pasta de trabalho sai; as cópias dele ficam. Sem a linha vazia no fim também lê.
        let nu = "worktree /p/central.git\nbare\n\nworktree /p/wt/a\nHEAD 5555555eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee\nbranch refs/heads/a";
        assert_eq!(ler_copias(nu), [copia("/p/wt/a", Some("a"), "5555555", false, false)]);
        assert!(ler_copias("").is_empty());
    }

    /// As cópias de um repositório de verdade, de qualquer uma delas, sem a que foi apagada.
    #[test]
    fn copias_de_um_repositorio_de_verdade() {
        let base = std::env::temp_dir().join(format!("vitral-git-copias-{}", crate::modelo::novo_id()));
        let pasta = base.join("repo");
        std::fs::create_dir_all(pasta.join("src")).unwrap();
        git(&pasta, &["init", "-q"]).unwrap();
        git(&pasta, &["checkout", "-q", "-b", "principal"]).unwrap();
        std::fs::write(pasta.join("src/a.txt"), "um\n").unwrap();
        git(&pasta, &["add", "."]).unwrap();
        git(&pasta, &["commit", "-q", "-m", "primeiro"]).unwrap();
        let (um, dois) = (base.join("wt").join("um"), base.join("wt").join("dois"));
        git(&pasta, &["worktree", "add", "-q", "-b", "feat/um", &um.to_string_lossy()]).unwrap();
        git(&pasta, &["worktree", "add", "-q", "--detach", &dois.to_string_lossy()]).unwrap();
        // O git escreve o caminho canônico (no macOS, o /tmp é um link).
        let (pasta, um, dois) = (dunce::canonicalize(&pasta).unwrap(), dunce::canonicalize(&um).unwrap(), dunce::canonicalize(&dois).unwrap());
        let resumo = |copias: &[Copia]| -> Vec<(PathBuf, Option<String>, bool)> {
            copias.iter().map(|c| (dunce::canonicalize(&c.caminho).unwrap(), c.ramo.clone(), c.principal)).collect()
        };
        // O git lista o repositório primeiro e as cópias pelo nome.
        let esperado = vec![
            (pasta.clone(), Some("principal".to_string()), true),
            (dois.clone(), None, false),
            (um.clone(), Some("feat/um".to_string()), false),
        ];
        // De dentro de uma cópia a lista é a mesma, e várias pastas do mesmo repositório não repetem.
        assert_eq!(resumo(&copias_do_repositorio(&um)), esperado);
        assert_eq!(resumo(&copias_das_pastas(&[pasta.join("src"), um.clone(), pasta.clone()])), esperado);
        assert!(copias_do_repositorio(&dois).iter().all(|c| c.commit.len() == 7));
        // Um repositório dentro de outro é outro repositório: as cópias dele entram também.
        let dentro = pasta.join("dentro");
        std::fs::create_dir_all(&dentro).unwrap();
        git(&dentro, &["init", "-q"]).unwrap();
        let dentro = dunce::canonicalize(&dentro).unwrap();
        let com_o_de_dentro = copias_das_pastas(&[pasta.clone(), dentro.clone()]);
        assert_eq!(com_o_de_dentro.len(), 4);
        assert_eq!(dunce::canonicalize(&com_o_de_dentro[3].caminho).unwrap(), dentro);
        // A cópia com a pasta apagada some da lista.
        std::fs::remove_dir_all(&um).unwrap();
        assert_eq!(resumo(&copias_do_repositorio(&pasta)), [esperado[0].clone(), esperado[1].clone()]);
        assert!(copias_do_repositorio(&base).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn caminho_relativo_com_barra_normal() {
        assert_eq!(caminho_em_texto(&Path::new("src").join("ui").join("mod.rs")), "src/ui/mod.rs");
        assert_eq!(caminho_em_texto(Path::new("relatório.md")), "relatório.md");
    }

    #[test]
    fn o_diff_sem_espaco_em_branco_deixa_de_fora_o_que_so_mudou_de_recuo() {
        let pasta = std::env::temp_dir().join(format!("vitral-git-espaco-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        git(&pasta, &["init", "-q"]).unwrap();
        git(&pasta, &["config", "core.autocrlf", "false"]).unwrap();
        let longe: String = (0..20).map(|i| format!("linha {i}\n")).collect();
        std::fs::write(pasta.join("a.txt"), format!("um\n{longe}dois\n")).unwrap();
        std::fs::write(pasta.join("b.txt"), "se x {\nfaz\n}\n").unwrap();
        git(&pasta, &["add", "."]).unwrap();
        git(&pasta, &["commit", "-q", "-m", "primeiro"]).unwrap();
        // No a.txt, uma linha só ganhou recuo e outra, longe dela, mudou de verdade; no b.txt, só recuo.
        std::fs::write(pasta.join("a.txt"), format!("    um\n{longe}tres\n")).unwrap();
        std::fs::write(pasta.join("b.txt"), "se x {\n\tfaz  \n}\n").unwrap();
        let raiz = retrato_da_pasta(&pasta).unwrap().raiz;
        let inteiro = diff_do_arquivo(&raiz, "a.txt", false);
        assert!(inteiro.contains("+    um") && inteiro.contains("+tres"), "{inteiro}");
        let enxuto = diff_do_arquivo(&raiz, "a.txt", true);
        assert!(!enxuto.contains("+    um") && enxuto.contains("+tres"), "{enxuto}");
        assert!(!diff_do_arquivo(&raiz, "b.txt", false).is_empty());
        assert_eq!(diff_do_arquivo(&raiz, "b.txt", true), "");
        let _ = std::fs::remove_dir_all(&pasta);
    }
}
