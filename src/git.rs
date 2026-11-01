//! Operações de git das tarefas: cópia isolada (worktree), diff, commit e junção.
//! Todas bloqueiam: rode fora da thread da interface.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Result, bail};

use crate::{
    modelo::Verificacao,
    processos::{self, novo_comando},
};

/// Sem user.name/user.email no git, os commits do Vitral saem com uma identidade própria.
fn sem_identidade() -> bool {
    static SEM: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SEM.get_or_init(|| {
        let valor = |chave: &str| {
            novo_comando("git")
                .args(["config", chave])
                .output()
                .map(|s| !String::from_utf8_lossy(&s.stdout).trim().is_empty())
                .unwrap_or(false)
        };
        !(valor("user.name") && valor("user.email"))
    })
}

fn git(pasta: &Path, args: &[&str]) -> Result<String> {
    let mut comando = novo_comando("git");
    // Sem isso, "ação.txt" sai como "a\303\247\303\243o.txt" (entre aspas) e nenhum caminho bate.
    comando.arg("-C").arg(pasta).args(["-c", "core.quotePath=false"]);
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

/// O projeto é a raiz de um repositório git com pelo menos um commit.
pub fn usa_git(projeto: &Path) -> bool {
    let Ok(topo) = git(projeto, &["rev-parse", "--show-toplevel"]) else {
        return false;
    };
    let mesmo = |a: &Path, b: &Path| dunce::canonicalize(a).ok() == dunce::canonicalize(b).ok();
    mesmo(Path::new(&topo), projeto) && git(projeto, &["rev-parse", "HEAD"]).is_ok()
}

/// Erro de um git que achou o índice preso por outro (o caminho sai igual em qualquer idioma).
fn ocupado(erro: &str) -> bool {
    erro.contains("index.lock'")
}

thread_local! {
    /// As vezes que esta thread segura agora: `na_vez_do_projeto` não é reentrante.
    static SEGURADAS: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Solta a marca da vez mesmo quando o closure entra em pânico.
struct Soltar(PathBuf);

impl Drop for Soltar {
    fn drop(&mut self) {
        SEGURADAS.with(|s| s.borrow_mut().retain(|c| *c != self.0));
    }
}

/// Roda `f` na vez do repositório: as junções, as cópias novas e as limpezas do Vitral num mesmo
/// repositório vão uma de cada vez (dois git juntos disputam o index.lock e leem um HEAD que já
/// ficou velho). A chave é o `git-common-dir` (a origem e todas as cópias dela têm o mesmo); sem
/// git, a pasta. Não é reentrante: chamar de novo dentro de `f` travaria, então dá pânico.
pub fn na_vez_do_projeto<T>(projeto: &Path, f: impl FnOnce() -> T) -> T {
    type Travas = std::collections::HashMap<PathBuf, std::sync::Arc<std::sync::Mutex<()>>>;
    static TRAVAS: std::sync::OnceLock<std::sync::Mutex<Travas>> = std::sync::OnceLock::new();
    let chave = raiz_comum(projeto).unwrap_or_else(|| dunce::canonicalize(projeto).unwrap_or_else(|_| projeto.to_path_buf()));
    let reentrou = SEGURADAS.with(|s| s.borrow().contains(&chave));
    assert!(!reentrou, "na_vez_do_projeto chamada dentro dela mesma ({})", chave.display());
    let trava = TRAVAS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(chave.clone())
        .or_default()
        .clone();
    let _vez = trava.lock().unwrap_or_else(|e| e.into_inner());
    SEGURADAS.with(|s| s.borrow_mut().push(chave.clone()));
    let _soltar = Soltar(chave);
    f()
}

pub struct CopiaIsolada {
    pub caminho: PathBuf,
    pub branch: String,
    pub base: String,
    /// O projeto tinha mudanças sem commit, que a cópia não vê.
    pub sujo: bool,
    /// O branch da origem quando a cópia nasceu (`None` com o HEAD solto).
    pub ramo: Option<String>,
}

pub fn criar_worktree(projeto: &Path, destino: &Path, branch: &str) -> Result<CopiaIsolada> {
    let base = git(projeto, &["rev-parse", "HEAD"])?;
    let ramo = ramo_atual(projeto);
    // Só o que está no git: __pycache__ e afins (sem .gitignore) davam aviso falso em toda tarefa.
    let sujo = !git(projeto, &["status", "--porcelain", "--untracked-files=no"])?.is_empty();
    if let Some(pai) = destino.parent() {
        std::fs::create_dir_all(pai)?;
    }
    // Registros de cópias cujas pastas sumiram (ex.: .vitral apagado) travariam o `worktree add`.
    let _ = git(projeto, &["worktree", "prune"]);
    // Nome ocupado por uma cópia ou um branch que ficaram de antes: usa t3-2, t3-3… sem apagar nada.
    let nome = destino
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let livre = (1..=20)
        .map(|n| match n {
            1 => (destino.to_path_buf(), branch.to_string()),
            n => (destino.with_file_name(format!("{nome}-{n}")), format!("{branch}-{n}")),
        })
        .find(|(pasta, branch)| {
            !pasta.exists()
                && git(projeto, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
                    .is_err()
        });
    let Some((destino, branch)) = livre else {
        bail!("não há nome livre para a cópia em {}", destino.display());
    };
    let destino_texto = destino.to_string_lossy().to_string();
    git(
        projeto,
        &["worktree", "add", "-b", &branch, &destino_texto, "HEAD"],
    )?;
    Ok(CopiaIsolada {
        caminho: destino,
        branch,
        base,
        sujo,
        ramo,
    })
}

/// Refaz a cópia de um chat que já rodou no mesmo caminho (o `--resume` do Claude Code vale pela
/// pasta): a partir do branch antigo, se ele ainda existe, ou do HEAD da origem com o mesmo nome
/// (ou com sufixo só no branch). Uma pasta solta no caminho vai para `<nome>.antiga-<hora>`, nunca
/// é apagada. Devolve também se o branch antigo existia.
pub fn recriar_worktree(origem: &Path, destino: &Path, branch: &str) -> Result<(CopiaIsolada, bool)> {
    let _ = git(origem, &["worktree", "prune"]);
    let sujo = !git(origem, &["status", "--porcelain", "--untracked-files=no"])?.is_empty();
    let ramo = ramo_atual(origem);
    let existe = |b: &str| git(origem, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{b}")]).is_ok();
    if destino.exists() {
        let canonico = dunce::canonicalize(destino).unwrap_or_else(|_| destino.to_path_buf());
        let registradas = git(origem, &["worktree", "list", "--porcelain"])?;
        let registrada = registradas
            .lines()
            .filter_map(|l| l.strip_prefix("worktree "))
            .any(|p| Path::new(p) == canonico || Path::new(p) == destino);
        if registrada {
            let atual = git(destino, &["symbolic-ref", "--quiet", "--short", "HEAD"]).unwrap_or_else(|_| branch.to_string());
            let base = git(origem, &["merge-base", &atual, "HEAD"]).or_else(|_| git(origem, &["rev-parse", "HEAD"]))?;
            let copia = CopiaIsolada { caminho: destino.to_path_buf(), branch: atual, base, sujo, ramo };
            return Ok((copia, true));
        }
        let nome = destino.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let antiga = destino.with_file_name(format!("{nome}.antiga-{}", crate::modelo::agora()));
        std::fs::rename(destino, &antiga)
            .map_err(|e| anyhow::anyhow!("não liberou {} para a cópia: {e}", destino.display()))?;
    }
    if let Some(pai) = destino.parent() {
        std::fs::create_dir_all(pai)?;
    }
    let caminho = destino.to_string_lossy().to_string();
    let antigo = existe(branch);
    let (branch, base) = if antigo {
        git(origem, &["worktree", "add", &caminho, branch])?;
        let base = git(origem, &["merge-base", branch, "HEAD"]).or_else(|_| git(origem, &["rev-parse", "HEAD"]))?;
        (branch.to_string(), base)
    } else {
        let livre = (1..=20)
            .map(|n| if n == 1 { branch.to_string() } else { format!("{branch}-{n}") })
            .find(|b| !existe(b))
            .ok_or_else(|| anyhow::anyhow!("não há nome livre para o branch {branch}"))?;
        git(origem, &["worktree", "add", "-b", &livre, &caminho, "HEAD"])?;
        (livre, git(origem, &["rev-parse", "HEAD"])?)
    };
    Ok((CopiaIsolada { caminho: destino.to_path_buf(), branch, base, sujo, ramo }, antigo))
}

/// O branch em que a pasta está (`None` com o HEAD solto ou sem git).
pub fn ramo_atual(pasta: &Path) -> Option<String> {
    git(pasta, &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok().filter(|r| !r.is_empty())
}

/// O `git-common-dir` canônico: o mesmo na origem e em todas as cópias dela. É a chave da vez e o
/// que diz se uma cópia ainda pertence ao repositório da origem.
pub fn raiz_comum(pasta: &Path) -> Option<PathBuf> {
    let comum = git(pasta, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).ok()?;
    dunce::canonicalize(Path::new(&comum)).ok()
}

/// A pasta tem o commit: é o mesmo repositório da cópia que nasceu dele (ou um clone).
pub fn tem_commit(pasta: &Path, commit: &str) -> bool {
    git(pasta, &["cat-file", "-e", &format!("{commit}^{{commit}}")]).is_ok()
}

/// "Localizar pasta…" com cópias: a origem mudou de lugar e as cópias voltam a apontar para ela
/// (`git worktree repair`). Devolve as que continuam fora do repositório dela.
pub fn reparar_copias(origem: &Path, copias: &[PathBuf]) -> Vec<PathBuf> {
    if copias.is_empty() {
        return Vec::new();
    }
    let caminhos: Vec<String> = copias.iter().map(|c| c.to_string_lossy().to_string()).collect();
    let mut args = vec!["worktree", "repair"];
    args.extend(caminhos.iter().map(String::as_str));
    let _ = git(origem, &args);
    let raiz = raiz_comum(origem);
    copias.iter().filter(|c| raiz.is_none() || raiz_comum(c) != raiz).cloned().collect()
}

/// A raiz do repositório onde a pasta está (pode ser uma pasta acima dela).
pub fn topo_do_repositorio(pasta: &Path) -> Option<PathBuf> {
    let topo = git(pasta, &["rev-parse", "--show-toplevel"]).ok()?;
    dunce::canonicalize(Path::new(&topo)).ok()
}

/// Onde a cópia está em relação à origem: commits que só ela tem, commits da origem que ela ainda
/// não tem e mudanças sem commit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EstadoCopia {
    pub a_frente: u32,
    pub atras: u32,
    pub suja: bool,
}

pub fn estado_copia(copia: &Path, origem: &Path) -> Result<EstadoCopia> {
    let alvo = git(origem, &["rev-parse", "HEAD"])?;
    let contagem = git(copia, &["rev-list", "--left-right", "--count", &format!("HEAD...{alvo}")])?;
    let mut numeros = contagem.split_whitespace().map(|n| n.parse::<u32>().unwrap_or(0));
    let (a_frente, atras) = (numeros.next().unwrap_or(0), numeros.next().unwrap_or(0));
    Ok(EstadoCopia { a_frente, atras, suja: suja(copia)? })
}

/// Mudanças sem commit, fora o lixo de build e de testes (o mesmo que o commit deixa de fora).
fn suja(pasta: &Path) -> Result<bool> {
    let lixo = ["__pycache__/", ".pyc", "node_modules/", ".pytest_cache/", ".DS_Store", ".claude/worktrees/"];
    let status = git(pasta, &["status", "--porcelain", "-z"])?;
    Ok(separar_nulos(&status)
        .iter()
        .filter(|l| l.len() > 3)
        .any(|l| !lixo.iter().any(|x| l[3..].contains(x))))
}

/// Depois de juntar: a cópia anda até o HEAD novo da origem (ela já tem tudo o que ele tem) e esse
/// passa a ser o ponto de partida do próximo diff. Se não der para andar (alguém refez a história
/// da origem), a cópia fica e a base vira o ponto comum dos dois. Devolve a base e se andou.
pub fn depois_de_juntar(copia: &Path, origem: &Path) -> Result<(String, bool)> {
    let alvo = git(origem, &["rev-parse", "HEAD"])?;
    if git(copia, &["merge", "--ff-only", "-q", &alvo]).is_ok() {
        return Ok((alvo, true));
    }
    Ok((git(copia, &["merge-base", "HEAD", &alvo])?, false))
}

/// Traz para a cópia o que entrou na origem, quando ela não tem nada só dela (nem commit, nem
/// mudança sem commit): um `merge --ff-only`. Diz se andou.
pub fn sincronizar_copia(copia: &Path, origem: &Path) -> Result<bool> {
    let estado = estado_copia(copia, origem)?;
    if estado.a_frente > 0 || estado.suja || estado.atras == 0 {
        return Ok(false);
    }
    let alvo = git(origem, &["rev-parse", "HEAD"])?;
    git(copia, &["merge", "--ff-only", "-q", &alvo])?;
    Ok(true)
}

/// O commit do fim de cada vez na pasta própria (e o ponto antes de apagar uma cópia com trabalho):
/// tudo, menos o lixo, e só se mudou alguma coisa. Diz se fez o commit.
pub fn commit_de_turno(pasta: &Path, mensagem: &str) -> Result<bool> {
    adicionar(pasta, "-A")?;
    if git(pasta, &["diff", "--cached", "--name-only"])?.is_empty() {
        return Ok(false);
    }
    git(pasta, &["commit", "-q", "--no-verify", "-m", mensagem])?;
    Ok(true)
}

/// O ponto na pasta de um chat antes de um ajudante nascer dela ou ser juntado nela: o mesmo commit
/// do fim de cada vez (tudo, menos o lixo, e só se mudou alguma coisa). Assim a cópia do ajudante
/// parte do trabalho do pai, e a junção não esbarra no que o pai ainda não tinha posto em commit.
/// Roda dentro da vez do repositório e não a pede.
pub fn checkpoint(pasta: &Path, mensagem: &str) -> Result<bool> {
    commit_de_turno(pasta, mensagem)
}

/// Quantos arquivos a cópia mudou desde a base e quantas linhas entraram e saíram (os novos
/// contam; um binário conta o arquivo, sem linhas).
pub fn contar_mudancas(copia: &Path, base: &str) -> Result<(u32, u32, u32)> {
    adicionar(copia, "-N")?;
    let texto = git(copia, &["diff", "--numstat", base, "--", "."])?;
    let (mut arquivos, mut mais, mut menos) = (0, 0, 0);
    for linha in texto.lines() {
        let mut campos = linha.split('\t');
        let (a, b) = (campos.next().unwrap_or("-"), campos.next().unwrap_or("-"));
        arquivos += 1;
        mais += a.parse::<u32>().unwrap_or(0);
        menos += b.parse::<u32>().unwrap_or(0);
    }
    Ok((arquivos, mais, menos))
}

/// O resultado de juntar a cópia de um chat de projeto na origem.
pub enum Juntada {
    /// Entrou (ou não havia o que juntar); `base` é o ponto de partida novo da cópia e `andou`, se
    /// ela foi até o HEAD novo da origem.
    Feita { commits: Option<String>, base: String, andou: bool },
    Falhou(Juncao),
    /// A origem está em outro branch que o de quando a cópia nasceu: junta só com confirmação.
    RamoTrocado { alvo: String, atual: String },
    /// A cópia não é mais do repositório da origem (pasta movida, outro clone no lugar).
    OutroRepositorio,
}

/// Junta a cópia na origem sem apagá-la, para o chat continuar nela, e poda os pontos de volta do
/// grupo. Roda dentro da vez do repositório e não a pede de novo.
pub fn juntar(
    origem: &Path,
    copia: &Path,
    branch: &str,
    base: &str,
    mensagem: &str,
    ramo_alvo: Option<&str>,
    grupo: &str,
) -> Result<Juntada> {
    if raiz_comum(copia).is_none() || raiz_comum(copia) != raiz_comum(origem) {
        return Ok(Juntada::OutroRepositorio);
    }
    if let Some(alvo) = ramo_alvo {
        let atual = ramo_atual(origem).unwrap_or_else(|| "HEAD solto".into());
        if atual != alvo {
            return Ok(Juntada::RamoTrocado { alvo: alvo.to_string(), atual });
        }
    }
    let commits = match aprovar(origem, copia, branch, base, mensagem)? {
        Juncao::Juntou { commits } => Some(commits),
        Juncao::SemMudancas => None,
        outra => return Ok(Juntada::Falhou(outra)),
    };
    let (base, andou) = depois_de_juntar(copia, origem)?;
    podar_pontos(origem, grupo, PONTOS_GUARDADOS);
    Ok(Juntada::Feita { commits, base, andou })
}

/// Quantos pontos de volta cada chat guarda (os mais novos).
pub const PONTOS_GUARDADOS: usize = 30;

/// Apaga os pontos de volta mais antigos do grupo ("t12"), ficando com os `manter` mais novos.
pub fn podar_pontos(repositorio: &Path, grupo: &str, manter: usize) {
    let prefixo = format!("refs/vitral/pontos/{grupo}/");
    let refs = git(repositorio, &["for-each-ref", "--sort=-committerdate", "--format=%(refname)", &prefixo])
        .unwrap_or_default();
    let apagar: String = refs.lines().skip(manter).map(|nome| format!("delete {nome}\n")).collect();
    if !apagar.is_empty() {
        let _ = git_com_entrada(repositorio, &["update-ref", "--stdin"], &apagar);
    }
}

/// O que a abertura achou de uma junção que o Vitral fechado (ou a queda) cortou no meio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JuncaoCortada {
    /// A ponta da cópia já está na origem e nada ficou de fora: a junção conta como feita.
    Entrou,
    NaoEntrou,
}

/// Desfaz o que a junção cortada deixou na origem (o merge parado com a ponta da cópia, um
/// index.lock de antes do boot sem nenhum git rodando) e diz se ela chegou a entrar.
pub fn recuperar_juncao(origem: &Path, copia: &Path, branch: &str) -> Result<JuncaoCortada> {
    let ponta = git(origem, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).ok();
    if let (Ok(parado), Some(ponta)) = (git(origem, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]), &ponta)
        && parado == *ponta
    {
        git(origem, &["merge", "--abort"])?;
    }
    let boot = momento_do_boot();
    for pasta in [origem, copia].into_iter().filter(|p| p.is_dir()) {
        let Ok(trava) = git(pasta, &["rev-parse", "--path-format=absolute", "--git-path", "index.lock"]) else {
            continue;
        };
        let antiga = std::fs::metadata(&trava)
            .and_then(|m| m.modified())
            .ok()
            .zip(boot)
            .is_some_and(|(feita, boot)| feita < boot);
        if antiga && !git_rodando() {
            let _ = std::fs::remove_file(&trava);
        }
    }
    let entrou = ponta.is_some_and(|p| git(origem, &["merge-base", "--is-ancestor", &p, "HEAD"]).is_ok())
        && copia.is_dir()
        && !suja(copia)?;
    Ok(if entrou { JuncaoCortada::Entrou } else { JuncaoCortada::NaoEntrou })
}

/// Quando o computador ligou (o `btime` de `/proc/stat`).
fn momento_do_boot() -> Option<std::time::SystemTime> {
    let texto = std::fs::read_to_string("/proc/stat").ok()?;
    let segundos: u64 = texto.lines().find_map(|l| l.strip_prefix("btime "))?.trim().parse().ok()?;
    Some(std::time::UNIX_EPOCH + Duration::from_secs(segundos))
}

/// Há algum `git` rodando agora (de quem for).
fn git_rodando() -> bool {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()))
        .any(|e| std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| c.trim() == "git"))
}

/// Lixo de build e de testes que nunca entra no commit de uma tarefa, mesmo sem .gitignore. As
/// cópias que os agentes de um fluxo (Ultracode) criam em `.claude/worktrees` também não: viram
/// repositório dentro do commit.
const SEM_LIXO: [&str; 7] = [
    ".",
    ":(glob,exclude)**/__pycache__/**",
    ":(glob,exclude)**/*.pyc",
    ":(glob,exclude)**/node_modules/**",
    ":(glob,exclude)**/.pytest_cache/**",
    ":(glob,exclude)**/.DS_Store",
    ":(glob,exclude)**/.claude/worktrees/**",
];

fn adicionar(copia: &Path, opcao: &str) -> Result<String> {
    let mut args = vec!["add", opcao, "--"];
    args.extend(SEM_LIXO);
    git(copia, &args)
}

/// Diff da cópia em relação à base, incluindo arquivos novos ainda sem commit.
#[cfg(test)]
pub fn diff(copia: &Path, base: &str) -> Result<String> {
    // `add -N` marca os arquivos novos para que apareçam no diff sem mudar o conteúdo.
    adicionar(copia, "-N")?;
    git(copia, &["diff", base, "--", "."])
}

/// Arquivos gerados (lockfiles, minificados, snapshots) que o resumo do diff para o coordenador
/// deixa de fora. Só na leitura: no commit eles entram (não vão para SEM_LIXO).
const GERADOS: [&str; 8] = [
    "**/*.lock",
    "**/package-lock.json",
    "**/pnpm-lock.yaml",
    "**/npm-shrinkwrap.json",
    "**/*.min.js",
    "**/*.min.css",
    "**/*.map",
    "**/*.snap",
];

/// O caminho é de um arquivo gerado (lockfile, minificado, mapa, snapshot), pelos mesmos padrões
/// de `GERADOS`: o Revisar recolhe esses arquivos.
pub fn eh_gerado(caminho: &str) -> bool {
    let nome = caminho.trim_end_matches('/').rsplit('/').next().unwrap_or(caminho);
    GERADOS.iter().any(|padrao| {
        let padrao = padrao.trim_start_matches("**/");
        match padrao.strip_prefix('*') {
            Some(fim) => nome.len() > fim.len() && nome.ends_with(fim),
            None => nome == padrao,
        }
    })
}

/// Diff da tarefa para o coordenador, dentro de `orcamento` caracteres: o diff inteiro toma o
/// contexto dele a cada rodada, e um lockfile no começo empurrava o código para depois do corte.
/// Com `arquivos`, vem o diff completo deles (até os gerados). Sem, os gerados viram uma linha e,
/// se não couber, vai o --stat e os arquivos inteiros que couberem, primeiro os `apontados` pelo
/// revisor. O coordenador roda em `projeto`: um caminho absoluto de lá vale o mesmo que o da cópia.
pub fn diff_para_coordenador(
    copia: &Path,
    projeto: &Path,
    base: &str,
    arquivos: &[String],
    apontados: &[String],
    orcamento: usize,
) -> Result<String> {
    adicionar(copia, "-N")?;
    let relativo = |caminho: &String| -> String {
        let caminho = Path::new(caminho.trim());
        let caminho = [copia, projeto]
            .iter()
            .find_map(|pasta| caminho.strip_prefix(pasta).ok())
            .unwrap_or(caminho);
        caminho.to_string_lossy().trim_start_matches("./").to_string()
    };
    let leia = format!("leia direto em {}/<arquivo>", copia.display());
    let pedidos: Vec<String> = arquivos.iter().map(relativo).filter(|a| !a.is_empty()).collect();
    // O git recusa um caminho de fora da cópia ("outside repository") e o diff inteiro falharia.
    let fora: Vec<&str> = pedidos.iter().map(String::as_str).filter(|a| Path::new(a).is_absolute()).collect();
    if !fora.is_empty() {
        return Ok(format!(
            "{} não {} dentro do projeto. Peça os arquivos pelo caminho relativo à raiz do projeto \
             (ex.: src/main.rs).",
            fora.join(", "),
            if fora.len() == 1 { "fica" } else { "ficam" }
        ));
    }
    let com = |inicio: &[&str], especificacoes: &[String]| {
        let mut args = inicio.to_vec();
        args.push("--");
        args.extend(especificacoes.iter().map(String::as_str));
        git(copia, &args)
    };
    if !pedidos.is_empty() {
        let literais: Vec<String> = pedidos.iter().map(|a| format!(":(literal){a}")).collect();
        let texto = com(&["diff", base], &literais)?;
        return Ok(if texto.trim().is_empty() {
            format!("Nenhuma mudança em {} em relação à base.", pedidos.join(", "))
        } else if texto.len() > 60_000 {
            format!("{}\nPara ver o resto, {leia}.", inicio_do_texto(&texto, 60_000))
        } else {
            texto
        });
    }
    let sem_gerados: Vec<String> = std::iter::once(".".to_string())
        .chain(GERADOS.iter().map(|g| format!(":(glob,exclude){g}")))
        .collect();
    let so_gerados: Vec<String> = GERADOS.iter().map(|g| format!(":(glob){g}")).collect();
    let texto = com(&["diff", base], &sem_gerados)?;
    let gerados = contagens(&com(&["diff", "--numstat", "-z", base], &so_gerados)?);
    let omitidos = (!gerados.is_empty()).then(|| {
        let lista: Vec<String> = gerados.iter().map(|(c, n)| format!("{c} ({n})")).collect();
        format!("Omitidos (gerados; peça pelo nome se precisar): {}.", lista.join(", "))
    });
    if texto.trim().is_empty() {
        return Ok(match omitidos {
            Some(omitidos) => format!("Só mudaram arquivos gerados. {omitidos}"),
            None => "Nenhuma mudança em relação à base.".into(),
        });
    }
    if texto.len() <= orcamento {
        return Ok(match omitidos {
            Some(omitidos) => format!("{texto}\n\n{omitidos}"),
            None => texto,
        });
    }

    let estatistica = inicio_do_texto(&com(&["diff", "--stat=120", base], &sem_gerados)?, orcamento / 4);
    let contagem: std::collections::HashMap<String, String> =
        contagens(&com(&["diff", "--numstat", "-z", base], &sem_gerados)?).into_iter().collect();
    let blocos = blocos_do_diff(&texto);
    let apontados: Vec<String> = apontados.iter().map(relativo).collect();
    let mut ordem: Vec<usize> = (0..blocos.len()).collect();
    ordem.sort_by_key(|&i| !apontados.contains(&blocos[i].0));
    // Arquivo nunca sai cortado no meio: ou vem inteiro, ou vai para a lista dos que faltam.
    let mut sobra = orcamento.saturating_sub(estatistica.len());
    let (mut mostrados, mut faltam) = (Vec::new(), Vec::new());
    for i in ordem {
        if blocos[i].1.len() <= sobra {
            sobra -= blocos[i].1.len();
            mostrados.push(i);
        } else {
            faltam.push(i);
        }
    }
    let mut corpo = String::new();
    if mostrados.is_empty() && !faltam.is_empty() {
        // Nem um arquivo cabe inteiro: vai o começo do primeiro.
        let (caminho, bloco) = &blocos[faltam.remove(0)];
        corpo.push_str(&format!(
            "{}\n(O diff de {caminho} não cabe; leia o arquivo inteiro em {}/{caminho}.)\n",
            inicio_do_texto(bloco, sobra.max(orcamento / 2)),
            copia.display()
        ));
    }
    mostrados.sort();
    for i in mostrados {
        corpo.push_str(blocos[i].1);
        if !corpo.ends_with('\n') {
            corpo.push('\n');
        }
    }
    let mut resposta = format!(
        "O diff tem {} caracteres, mais do que os {orcamento} de uma vez; vai um resumo.\n\nArquivos \
         alterados (sem os gerados):\n{estatistica}\n\n{corpo}",
        texto.len()
    );
    if !faltam.is_empty() {
        faltam.sort();
        let lista: Vec<String> = faltam
            .iter()
            .take(40)
            .map(|&i| match contagem.get(&blocos[i].0) {
                Some(numeros) => format!("{} ({numeros})", blocos[i].0),
                None => blocos[i].0.clone(),
            })
            .collect();
        let mais = match faltam.len() {
            0..=40 => String::new(),
            n => format!(" e mais {}", n - 40),
        };
        resposta.push_str(&format!(
            "\nFaltam {} arquivo(s): {}{mais}. Peça com ver_diff (tarefa, arquivos: [\"{}\"]) ou {leia}.\n",
            faltam.len(),
            lista.join(", "),
            blocos[faltam[0]].0
        ));
    }
    if let Some(omitidos) = omitidos {
        resposta.push_str(&format!("\n{omitidos}\n"));
    }
    Ok(resposta)
}

/// Linhas de `git diff --numstat -z`: o caminho e "+a −r" ("binário" quando não há contagem).
fn contagens(numstat: &str) -> Vec<(String, String)> {
    let mut campos = numstat.split('\0');
    let mut lista = Vec::new();
    while let Some(campo) = campos.next() {
        let mut partes = campo.splitn(3, '\t');
        let (Some(mais), Some(menos), Some(caminho)) = (partes.next(), partes.next(), partes.next()) else {
            continue;
        };
        // Renomeado: o caminho vem vazio, e o nome antigo e o novo nos campos seguintes.
        let caminho = if caminho.is_empty() {
            campos.next();
            campos.next().unwrap_or_default().to_string()
        } else {
            caminho.to_string()
        };
        let numeros = if mais == "-" {
            "binário".to_string()
        } else {
            format!("+{mais} −{menos}")
        };
        lista.push((caminho, numeros));
    }
    lista
}

/// O diff em blocos por arquivo (cada um começa em `diff --git `), com o caminho de cada um.
fn blocos_do_diff(texto: &str) -> Vec<(String, &str)> {
    let mut inicios = Vec::new();
    let mut posicao = 0;
    for linha in texto.split_inclusive('\n') {
        if linha.starts_with("diff --git ") {
            inicios.push(posicao);
        }
        posicao += linha.len();
    }
    inicios
        .iter()
        .enumerate()
        .map(|(n, &inicio)| {
            let bloco = &texto[inicio..inicios.get(n + 1).copied().unwrap_or(texto.len())];
            let mut linhas = bloco.lines();
            let cabecalho = linhas
                .next()
                .and_then(|l| l.strip_prefix("diff --git "))
                .map(caminho_do_cabecalho)
                .unwrap_or_default();
            // `+++ b/x` vale mais; num arquivo apagado é `--- a/x`; binário só tem o cabeçalho.
            let (mut novo, mut antigo) = (None, None);
            for linha in linhas.take_while(|l| !l.starts_with("@@")) {
                if linha.starts_with("+++ ") {
                    novo = caminho_do_diff(linha);
                } else if linha.starts_with("--- ") {
                    antigo = caminho_do_diff(linha);
                }
            }
            (novo.or(antigo).unwrap_or(cabecalho), bloco)
        })
        .collect()
}

/// O mesmo diff com o arquivo inteiro em volta das mudanças: o Revisar dobra o que não mudou e
/// qualquer linha que o revisor apontar está no patch.
pub fn diff_completo(copia: &Path, base: &str) -> Result<String> {
    adicionar(copia, "-N")?;
    git(copia, &["diff", "--unified=100000", base, "--", "."])
}

/// Arquivos que a cópia mudou em relação à base (inclui os novos).
pub fn arquivos_mudados(copia: &Path, base: &str) -> Vec<String> {
    let _ = adicionar(copia, "-N");
    git(copia, &["diff", "--name-only", "-z", base, "--", "."])
        .map(|t| separar_nulos(&t))
        .unwrap_or_default()
}

/// Saída de um comando git com `-z`: um campo por NUL.
fn separar_nulos(texto: &str) -> Vec<String> {
    texto.split('\0').filter(|c| !c.is_empty()).map(str::to_string).collect()
}

/// Base com que comparar a cópia de uma tarefa. Se o trabalhador juntou o projeto na cópia (o
/// `git merge` pedido num conflito), passa a ser o ponto juntado; com a base original, o que
/// outras tarefas já juntaram pareceria trabalho desta (e "Descartar" desfaria esse trabalho).
pub fn base_efetiva(projeto: &Path, copia: &Path, base: &str) -> String {
    let Ok(atual) = git(projeto, &["rev-parse", "HEAD"]) else {
        return base.to_string();
    };
    let mut melhor = base.to_string();
    // Um merge ainda sem commit (MERGE_HEAD) também conta: a pasta já tem o que ele trouxe.
    for topo in ["HEAD", "MERGE_HEAD"] {
        // Só avança: se o projeto voltou no histórico ou trocou de branch, fica a base original.
        if let Ok(comum) = git(copia, &["merge-base", topo, &atual])
            && git(copia, &["merge-base", "--is-ancestor", &melhor, &comum]).is_ok()
        {
            melhor = comum;
        }
    }
    melhor
}

/// Volta um arquivo da cópia ao estado da base (apaga se ele é novo) e confere que voltou. Num
/// arquivo renomeado, o nome antigo volta junto (o Revisar só lista o novo); devolve esse nome.
pub fn descartar_arquivo(copia: &Path, base: &str, arquivo: &str) -> Result<Option<String>> {
    // Literal: "pages/[id].tsx" como padrão também pegaria "pages/i.tsx".
    let literal = format!(":(literal){arquivo}");
    let antigo = origem_da_renomeacao(copia, base, arquivo);
    let literal_antigo = antigo.as_ref().map(|a| format!(":(literal){a}"));
    if let Some(literal_antigo) = &literal_antigo {
        git(copia, &["checkout", base, "--", literal_antigo])?;
    }
    if git(copia, &["cat-file", "-e", &format!("{base}:{arquivo}")]).is_ok() {
        git(copia, &["checkout", base, "--", &literal])?;
    } else {
        let no_indice = git(copia, &["rm", "-f", "-q", "--cached", "--", &literal]).is_ok();
        let no_disco = match std::fs::remove_file(copia.join(arquivo)) {
            Ok(()) => true,
            Err(erro) if erro.kind() == std::io::ErrorKind::NotFound => false,
            Err(erro) => bail!("não deu para apagar {arquivo}: {erro}"),
        };
        if !no_indice && !no_disco {
            bail!("{arquivo} não está na cópia");
        }
    }
    let mut conferir = vec!["diff", "--quiet", base, "--", literal.as_str()];
    conferir.extend(literal_antigo.as_deref());
    if git(copia, &conferir).is_err() {
        bail!("{arquivo} continua diferente da base");
    }
    Ok(antigo)
}

/// Nome antigo de `arquivo` quando o diff da cópia o mostra como renomeado. Sem `-M`: a detecção
/// é a mesma do diff que o Revisar mostra (a do git do usuário).
fn origem_da_renomeacao(copia: &Path, base: &str, arquivo: &str) -> Option<String> {
    let texto = git(copia, &["diff", "--name-status", "-z", base, "--", "."]).ok()?;
    let mut campos = texto.split('\0');
    while let Some(situacao) = campos.next() {
        let primeiro = campos.next()?;
        // Renomeado (R) e copiado (C) trazem dois caminhos: o de antes e o de agora.
        if situacao.starts_with(['R', 'C']) {
            let novo = campos.next()?;
            if situacao.starts_with('R') && novo == arquivo {
                return Some(primeiro.to_string());
            }
        }
    }
    None
}

/// Desfaz as aspas que o git põe em nomes com caracteres especiais (`"a\303\247\303\243o.txt"`).
fn tirar_aspas(texto: &str) -> String {
    let Some(dentro) = texto.strip_prefix('"').and_then(|t| t.strip_suffix('"')) else {
        return texto.to_string();
    };
    let mut bytes = Vec::with_capacity(dentro.len());
    let mut resto = dentro.bytes().peekable();
    while let Some(byte) = resto.next() {
        if byte != b'\\' {
            bytes.push(byte);
            continue;
        }
        match resto.next() {
            Some(digito @ b'0'..=b'7') => {
                let mut valor = u32::from(digito - b'0');
                for _ in 0..2 {
                    match resto.peek() {
                        Some(&d @ b'0'..=b'7') => {
                            valor = valor * 8 + u32::from(d - b'0');
                            resto.next();
                        }
                        _ => break,
                    }
                }
                bytes.push(valor as u8);
            }
            Some(letra) => bytes.push(match letra {
                b'a' => 7,
                b'b' => 8,
                b't' => b'\t',
                b'n' => b'\n',
                b'v' => 11,
                b'f' => 12,
                b'r' => b'\r',
                outro => outro,
            }),
            None => bytes.push(b'\\'),
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Caminho no cabeçalho `diff --git a/x b/x` (sem o prefixo). É provisório: o certo vem do
/// `+++ b/x` (veja caminho_do_diff), mas arquivo binário só tem o cabeçalho.
pub fn caminho_do_cabecalho(resto: &str) -> String {
    if resto.ends_with('"')
        && let Some(inicio) = resto.rfind(" \"b/")
    {
        let caminho = tirar_aspas(&resto[inicio + 1..]);
        return caminho.strip_prefix("b/").unwrap_or(&caminho).to_string();
    }
    resto.split_once(" b/").map_or(resto, |(_, b)| b).to_string()
}

/// Caminho das linhas `--- a/x` e `+++ b/x` do diff; None em outras linhas e em /dev/null.
pub fn caminho_do_diff(linha: &str) -> Option<String> {
    let (resto, prefixo) = match linha.strip_prefix("+++ ") {
        Some(resto) => (resto, "b/"),
        None => (linha.strip_prefix("--- ")?, "a/"),
    };
    // O git põe um TAB no fim quando o nome tem espaço.
    let caminho = tirar_aspas(resto.trim_end_matches('\t'));
    caminho.strip_prefix(prefixo).map(str::to_string)
}

/// Copia para a cópia nova os arquivos fora do git (ex.: .env) que batem com os padrões.
pub fn copiar_arquivos_locais(projeto: &Path, copia: &Path, padroes: &[String]) -> Vec<String> {
    let mut copiados = Vec::new();
    let Ok(entradas) = std::fs::read_dir(projeto) else {
        return copiados;
    };
    for entrada in entradas.flatten() {
        let nome = entrada.file_name().to_string_lossy().to_string();
        let bate = padroes.iter().any(|p| match p.strip_suffix('*') {
            Some(prefixo) => nome.starts_with(prefixo),
            None => nome == *p,
        });
        let destino = copia.join(&nome);
        if bate && entrada.path().is_file() && !destino.exists() && std::fs::copy(entrada.path(), &destino).is_ok() {
            copiados.push(nome);
        }
    }
    copiados
}

pub fn diff_stat(copia: &Path, base: &str) -> Result<String> {
    adicionar(copia, "-N")?;
    git(copia, &["diff", "--stat", base, "--", "."])
}

pub enum Juncao {
    Juntou {
        commits: String,
    },
    SemMudancas,
    /// A pasta do projeto tem mudanças sem commit (do usuário, ou de alguém que escreveu fora
    /// da própria cópia).
    ProjetoSujo {
        arquivos: Vec<String>,
    },
    /// Arquivos fora do git na pasta do projeto com o nome de arquivos que a tarefa cria: o git
    /// não os sobrescreve.
    SoltosNoProjeto {
        arquivos: Vec<String>,
    },
    Conflito {
        arquivos: Vec<String>,
        detalhe: String,
        /// Commit do projeto que o trabalhador precisa juntar na cópia dele.
        alvo: String,
    },
    /// Outro git segurava a pasta (index.lock, ou um merge que não é deste): nada foi juntado.
    Ocupado {
        detalhe: String,
    },
}

/// Um `git merge` que falhou só é conflito quando o git parou no meio deste merge (e então ele
/// é desfeito); outro git segurando a pasta é "ocupado", e o resto é erro comum.
fn falha_do_merge(pasta: &Path, mesclado: &str, alvo: String, erro: anyhow::Error) -> Result<Juncao> {
    let detalhe = erro.to_string();
    let em_merge = git(pasta, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).ok();
    if em_merge.is_some() && em_merge == git(pasta, &["rev-parse", mesclado]).ok() {
        let arquivos = git(pasta, &["diff", "--name-only", "--diff-filter=U"])
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect();
        let _ = git(pasta, &["merge", "--abort"]);
        return Ok(Juncao::Conflito {
            arquivos,
            detalhe,
            alvo,
        });
    }
    if em_merge.is_some() || ocupado(&detalhe) {
        return Ok(Juncao::Ocupado { detalhe });
    }
    Err(erro)
}

/// Faz o commit do que sobrou na cópia e junta o branch dela no branch atual do projeto.
pub fn aprovar(
    projeto: &Path,
    copia: &Path,
    branch: &str,
    base: &str,
    mensagem: &str,
) -> Result<Juncao> {
    // Os arquivos copiados da origem (tarefa.arquivos_copiados) entram aqui quando o modelo os
    // tiver guardados.
    commitar(copia, mensagem, &[])?;
    let desde = base_efetiva(projeto, copia, base);
    let commits = git(copia, &["log", "--oneline", &format!("{desde}..HEAD")])?;
    if commits.is_empty() {
        return Ok(Juncao::SemMudancas);
    }
    // Mudanças sem commit na pasta do projeto travam o merge; não é conflito entre tarefas. Os
    // arquivos fora do git (o __pycache__ dos testes que o coordenador roda ali) não travam.
    let sujeira: Vec<String> = git(projeto, &["status", "--porcelain", "--untracked-files=no"])?
        .lines()
        .map(|l| tirar_aspas(l.get(3..).unwrap_or(l)))
        .collect();
    if !sujeira.is_empty() {
        return Ok(Juncao::ProjetoSujo { arquivos: sujeira });
    }
    // Primeiro traz para a cópia o que já entrou no projeto (outras tarefas aprovadas). Se houver
    // conflito, ele fica na cópia, onde o trabalhador resolve, e o projeto nem é tocado.
    let atual = git(projeto, &["rev-parse", "HEAD"])?;
    let atualizar = [
        "merge",
        "--no-edit",
        "-m",
        "Atualiza com o projeto",
        atual.as_str(),
    ];
    if let Err(erro) = git(copia, &atualizar) {
        return falha_do_merge(copia, &atual, atual.clone(), erro);
    }
    // Confere antes: o texto do erro do git muda com o idioma.
    let soltos = soltos_no_caminho(projeto, copia, &atual)?;
    if !soltos.is_empty() {
        return Ok(Juncao::SoltosNoProjeto { arquivos: soltos });
    }
    match git(
        projeto,
        &["merge", "--no-ff", "--no-edit", "-m", mensagem, branch],
    ) {
        Ok(_) => Ok(Juncao::Juntou { commits }),
        Err(erro) => {
            // Relido: é o que o trabalhador precisa trazer, mesmo que alguém tenha feito commit.
            let alvo = git(projeto, &["rev-parse", "HEAD"]).unwrap_or(atual);
            falha_do_merge(projeto, branch, alvo, erro)
        }
    }
}

/// Arquivos fora do git no projeto que a junção teria de sobrescrever (ou que estão no caminho de
/// uma pasta que a tarefa cria). Os ignorados pelo .gitignore o git sobrescreve sem reclamar.
fn soltos_no_caminho(projeto: &Path, copia: &Path, atual: &str) -> Result<Vec<String>> {
    let novos = separar_nulos(&git(
        copia,
        &["diff", "--name-only", "--no-renames", "--diff-filter=A", "-z", atual, "HEAD"],
    )?);
    if novos.is_empty() {
        return Ok(Vec::new());
    }
    let soltos = separar_nulos(&git(projeto, &["ls-files", "--others", "--exclude-standard", "-z"])?);
    let dentro = |a: &str, pasta: &str| a.strip_prefix(pasta).is_some_and(|r| r.starts_with('/'));
    Ok(soltos
        .into_iter()
        .filter(|s| novos.iter().any(|n| s == n || dentro(s, n) || dentro(n, s)))
        .collect())
}

/// O que entrou no commit inicial de "Ativar git".
pub struct RepositorioNovo {
    pub arquivos: usize,
    pub bytes: u64,
    /// O .gitignore que o Vitral criou; vazio quando o projeto já tinha um.
    pub ignorados: Vec<String>,
}

impl RepositorioNovo {
    /// Texto da notificação depois de ativar o git.
    pub fn resumo(&self, projeto: &str) -> String {
        let mut texto = format!(
            "Git ativado em {projeto}: {} arquivo{} ({}) no commit inicial.",
            self.arquivos,
            if self.arquivos == 1 { "" } else { "s" },
            tamanho(self.bytes)
        );
        if !self.ignorados.is_empty() {
            let mut nomes: Vec<&str> = Vec::new();
            for padrao in &self.ignorados {
                let nome = padrao.trim_matches('/');
                if !padrao.starts_with(['!', '*']) && !nome.contains('*') && !nomes.contains(&nome) {
                    nomes.push(nome);
                }
            }
            let mais = if nomes.len() > 4 { " e outros" } else { "" };
            nomes.truncate(4);
            texto.push_str(&format!(" Criei um .gitignore que deixa de fora {}{mais}.", nomes.join(", ")));
        }
        texto
    }
}

/// Tamanho legível em pt-BR ("820 KB", "1,3 MB", "2,4 GB").
pub fn tamanho(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    match bytes {
        0..1024 => format!("{bytes} bytes"),
        1024..1_048_576 => format!("{} KB", bytes.div_ceil(1024)),
        1_048_576..1_073_741_824 => format!("{:.1} MB", bytes as f64 / MB).replace('.', ","),
        _ => format!("{:.1} GB", bytes as f64 / MB / 1024.0).replace('.', ","),
    }
}

/// O que uma entrada ocupa no disco e, se é um arquivo com mais de um nome (hard link), quem ele é
/// (disco e inode), para contar uma vez só.
#[cfg(unix)]
fn no_disco(dados: &std::fs::Metadata) -> (u64, Option<(u64, u64)>) {
    use std::os::unix::fs::MetadataExt as _;
    (dados.blocks() * 512, (!dados.is_dir() && dados.nlink() > 1).then(|| (dados.dev(), dados.ino())))
}

/// No Windows, o tamanho do arquivo: os hard links contam de novo (o total é aproximado).
#[cfg(not(unix))]
fn no_disco(dados: &std::fs::Metadata) -> (u64, Option<(u64, u64)>) {
    (dados.len(), None)
}

/// Espaço que a pasta ocupa no disco, sem seguir links e contando uma vez só os arquivos com hard
/// link (o node_modules do pnpm aponta para a loja dele).
pub fn espaco_ocupado(pasta: &Path) -> u64 {
    let mut vistos = std::collections::HashSet::new();
    let mut pendentes = vec![pasta.to_path_buf()];
    let mut total = 0;
    while let Some(caminho) = pendentes.pop() {
        let Ok(dados) = std::fs::symlink_metadata(&caminho) else {
            continue;
        };
        let (ocupa, quem) = no_disco(&dados);
        if quem.is_some_and(|quem| !vistos.insert(quem)) {
            continue;
        }
        total += ocupa;
        if dados.is_dir()
            && let Ok(entradas) = std::fs::read_dir(&caminho)
        {
            pendentes.extend(entradas.flatten().map(|e| e.path()));
        }
    }
    total
}

/// Quantos arquivos a pasta tem e quanto ocupa (como `espaco_ocupado`), parando em `limite`
/// entradas: o `node_modules` de um projeto pode ter centenas de milhares. Devolve se contou tudo.
pub fn medir_pasta(pasta: &Path, limite: usize) -> (usize, u64, bool) {
    let mut vistos = std::collections::HashSet::new();
    let mut pendentes = vec![pasta.to_path_buf()];
    let (mut arquivos, mut bytes, mut entradas) = (0, 0, 0);
    while let Some(caminho) = pendentes.pop() {
        entradas += 1;
        if entradas > limite {
            return (arquivos, bytes, false);
        }
        let Ok(dados) = std::fs::symlink_metadata(&caminho) else {
            continue;
        };
        let (ocupa, quem) = no_disco(&dados);
        if dados.is_dir() {
            bytes += ocupa;
            if let Ok(lidas) = std::fs::read_dir(&caminho) {
                pendentes.extend(lidas.flatten().map(|e| e.path()));
            }
            continue;
        }
        arquivos += 1;
        if quem.is_some_and(|quem| !vistos.insert(quem)) {
            continue;
        }
        bytes += ocupa;
    }
    (arquivos, bytes, true)
}

/// O .gitignore de um projeto que ainda não tem: segredos, lixo de testes e, pela pilha que
/// aparece na raiz, ambientes e pastas de build.
fn padroes_ignorados(projeto: &Path) -> Vec<String> {
    let mut padroes: Vec<String> = [
        ".env",
        ".env.*",
        "!.env.example",
        ".DS_Store",
        "*.log",
        "__pycache__/",
        "*.pyc",
        "node_modules/",
        ".pytest_cache/",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let tem = |nome: &str| projeto.join(nome).exists();
    let mut juntar = |novos: &[&str]| {
        for padrao in novos {
            if !padroes.iter().any(|p| p == padrao) {
                padroes.push(padrao.to_string());
            }
        }
    };
    if tem("Cargo.toml") {
        juntar(&["/target/"]);
    }
    if tem("package.json") {
        juntar(&["/dist/", "/build/", ".next/", "coverage/"]);
    }
    if tem("pyproject.toml") || tem("requirements.txt") || tem("setup.py") {
        juntar(&[
            ".venv/",
            "venv/",
            ".mypy_cache/",
            ".ruff_cache/",
            ".coverage",
            "htmlcov/",
            "*.egg-info/",
            "/dist/",
            "/build/",
        ]);
    }
    // Ambiente virtual do Python com qualquer nome.
    let mut ambientes: Vec<String> = std::fs::read_dir(projeto)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("pyvenv.cfg").is_file())
        .map(|e| format!("/{}/", e.file_name().to_string_lossy()))
        .filter(|p| !padroes.iter().any(|q| q.trim_start_matches('/') == p.trim_start_matches('/')))
        .collect();
    ambientes.sort();
    padroes.extend(ambientes);
    padroes
}

/// Cria um repositório no projeto com um commit de tudo que existe. Sem .gitignore, cria um antes,
/// para que .env, ambientes e pastas de build não entrem no histórico.
pub fn iniciar_repositorio(projeto: &Path) -> Result<RepositorioNovo> {
    let arquivo = projeto.join(".gitignore");
    let ignorados = if arquivo.exists() {
        Vec::new()
    } else {
        let padroes = padroes_ignorados(projeto);
        let texto = format!(
            "# Criado pelo Vitral ao ativar o git: segredos, ambientes e pastas de build ficam fora.\n{}\n",
            padroes.join("\n")
        );
        std::fs::write(&arquivo, texto)?;
        padroes
    };
    git(projeto, &["init", "-q"])?;
    adicionar(projeto, "-A")?;
    git(
        projeto,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Estado inicial (Vitral)",
        ],
    )?;
    let arquivos = separar_nulos(&git(projeto, &["ls-files", "-z"])?);
    let bytes = arquivos
        .iter()
        .filter_map(|a| std::fs::symlink_metadata(projeto.join(a)).ok())
        .map(|m| m.len())
        .sum();
    Ok(RepositorioNovo {
        arquivos: arquivos.len(),
        bytes,
        ignorados,
    })
}

#[cfg(test)]
pub fn remover_worktree(projeto: &Path, copia: &Path, branch: &str) {
    let _ = limpar_copia(projeto, copia, &projeto.with_extension("area"), branch, None, &[], false);
}

/// Apaga a cópia de uma tarefa: a pasta (com node_modules, .venv e target/), o branch e os pontos
/// de volta dela. `grupo` é o da tarefa ("t12"): o ramo pode ter qualquer nome (o do Linear, o
/// prefixo da pasta); sem ele, vale o que o ramo `vitral/t<N>-…` diz, como antes. `pontos_antigos`
/// são os gravados antes de os pontos irem para o grupo da tarefa. `area` é a pasta onde o Vitral
/// cria as cópias deste projeto. Com `manter_branch`, só a pasta sai (o branch e os pontos ficam,
/// para a cópia ser refeita dele). Sem a origem, ou com o git dela falhando, nada é apagado à mão:
/// a limpeza fica para depois.
pub fn limpar_copia(
    projeto: &Path,
    copia: &Path,
    area: &Path,
    branch: &str,
    grupo: Option<&str>,
    pontos_antigos: &[String],
    manter_branch: bool,
) -> Result<()> {
    if !projeto.is_dir() {
        bail!("a pasta {} não foi encontrada; a cópia fica até ela voltar", projeto.display());
    }
    let caminho = copia.to_string_lossy().to_string();
    if let Err(erro) = git(projeto, &["worktree", "remove", "--force", "--force", &caminho])
        && copia.exists()
    {
        // Com submódulo o git não remove. À mão, só o que é mesmo uma cópia: `.git` é um arquivo,
        // ou a pasta é das cópias do Vitral (uma limpeza cortada pode ter apagado o `.git` antes).
        // E só com o git da origem respondendo: senão o erro pode ser dele, não da cópia.
        let origem_responde = git(projeto, &["rev-parse", "--git-dir"]).is_ok();
        if !origem_responde || (!copia.join(".git").is_file() && copia.parent() != Some(area)) {
            bail!("não apagou {}: {erro:#}", copia.display());
        }
        std::fs::remove_dir_all(copia).map_err(|erro| anyhow::anyhow!("não apagou {}: {erro}", copia.display()))?;
    }
    podar(projeto);
    if manter_branch {
        return Ok(());
    }
    let _ = git(projeto, &["branch", "-D", branch]);
    let grupo = grupo.map(str::to_string).or_else(|| grupo_do_branch(branch)).map(|g| format!("refs/vitral/pontos/{g}/"));
    let mut apagar = String::new();
    let refs = git(projeto, &["for-each-ref", "--format=%(refname)", "refs/vitral/pontos/"]).unwrap_or_default();
    for nome in refs.lines() {
        let antigo = nome
            .strip_prefix("refs/vitral/pontos/")
            .is_some_and(|sha| pontos_antigos.iter().any(|p| p == sha));
        if antigo || grupo.as_deref().is_some_and(|g| nome.starts_with(g)) {
            apagar.push_str(&format!("delete {nome}\n"));
        }
    }
    if !apagar.is_empty() {
        git_com_entrada(projeto, &["update-ref", "--stdin"], &apagar)?;
    }
    Ok(())
}

/// Apaga os pontos de volta (`refs/vitral/pontos/<grupo>/`) dos grupos dados ("t12"): são do
/// Vitral e só serviam às conversas que saíram.
pub fn apagar_pontos(repositorio: &Path, grupos: &[String]) {
    let refs = git(repositorio, &["for-each-ref", "--format=%(refname)", "refs/vitral/pontos/"]).unwrap_or_default();
    let apagar: String = refs
        .lines()
        .filter(|nome| grupos.iter().any(|g| nome.starts_with(&format!("refs/vitral/pontos/{g}/"))))
        .map(|nome| format!("delete {nome}\n"))
        .collect();
    if !apagar.is_empty() {
        let _ = git_com_entrada(repositorio, &["update-ref", "--stdin"], &apagar);
    }
}

/// Esquece as cópias cujas pastas sumiram (apagadas à mão): o registro delas segurava o branch.
pub fn podar(projeto: &Path) {
    let _ = git(projeto, &["worktree", "prune"]);
}

/// Grupo dos pontos de volta da cópia de uma tarefa pelo nome do branch: "t12" em
/// "vitral/t12-titulo". Só a alternativa de quando a tarefa não diz o grupo (as fotos antigas).
fn grupo_do_branch(branch: &str) -> Option<String> {
    let resto = branch.strip_prefix("vitral/t")?;
    let numero: String = resto.chars().take_while(char::is_ascii_digit).collect();
    let depois = &resto[numero.len()..];
    (!numero.is_empty() && (depois.is_empty() || depois.starts_with('-'))).then(|| format!("t{numero}"))
}

fn git_com_entrada(pasta: &Path, args: &[&str], entrada: &str) -> Result<()> {
    use std::io::Write as _;
    let mut filho = novo_comando("git")
        .arg("-C")
        .arg(pasta)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut escrita) = filho.stdin.take() {
        escrita.write_all(entrada.as_bytes())?;
    }
    let saida = filho.wait_with_output()?;
    if !saida.status.success() {
        bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&saida.stderr).trim());
    }
    Ok(())
}

/// Roda o comando de verificação (ou de preparação) da tarefa na cópia, com limite de tempo.
pub fn verificar(pasta: &Path, comando: &str) -> Verificacao {
    let inicio = Instant::now();
    let (passou, saida) = rodar_com_limite(pasta, comando, Duration::from_secs(15 * 60));
    Verificacao {
        comando: comando.to_string(),
        passou,
        saida,
        duracao_s: Some(inicio.elapsed().as_secs()),
    }
}

/// Lê a saída enquanto o comando roda (com 64 KiB parados no pipe ele travaria até o limite) e,
/// no fim, derruba o que ele deixou em segundo plano, que seguraria o pipe e gastaria CPU.
fn rodar_com_limite(pasta: &Path, comando: &str, limite: Duration) -> (bool, String) {
    let abrir = || -> std::io::Result<_> {
        // Um pipe só para stdout e stderr: a saída fica intercalada como no terminal.
        let (leitor, escritor) = std::io::pipe()?;
        let mut processo = processos::comando("bash", "verificacao");
        processo
            .args(["-lc", comando])
            .current_dir(pasta)
            .stdin(Stdio::null())
            .stdout(escritor.try_clone()?)
            .stderr(escritor);
        // O Command guarda a ponta de escrita: o `abrir` solta, senão o pipe nunca fecha.
        let (filho, escopo) = processos::abrir(processo)?;
        Ok((leitor, filho, escopo))
    };
    let (mut leitor, mut filho, escopo) = match abrir() {
        Ok(aberto) => aberto,
        Err(erro) => return (false, format!("não rodou: {erro}")),
    };
    let guardado = Arc::new(Mutex::new(Vec::new()));
    let leitura = std::thread::spawn({
        let guardado = guardado.clone();
        move || {
            let mut bloco = [0u8; 8192];
            loop {
                match leitor.read(&mut bloco) {
                    Ok(0) => break,
                    Ok(n) => {
                        // Só o fim interessa (o começo de um `npm install` não diz nada).
                        let mut texto = guardado.lock().unwrap_or_else(|e| e.into_inner());
                        texto.extend_from_slice(&bloco[..n]);
                        if texto.len() > 32 * 1024 {
                            let corte = texto.len() - 16 * 1024;
                            texto.drain(..corte);
                        }
                    }
                    Err(erro) if erro.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        }
    });
    let status = processos::esperar(&mut filho, &escopo, limite);
    // Quem escapou do grupo (setsid) e ainda segura o pipe não prende a resposta.
    let folga = Instant::now() + Duration::from_secs(6);
    while !leitura.is_finished() && Instant::now() < folga {
        std::thread::sleep(Duration::from_millis(20));
    }
    let bytes = std::mem::take(&mut *guardado.lock().unwrap_or_else(|e| e.into_inner()));
    let texto = final_do_texto(String::from_utf8_lossy(&bytes).trim_end(), 4000);
    match status {
        Some(status) => (status.success(), texto),
        None => {
            let prazo = match limite.as_secs() {
                s @ 0..60 => format!("{s} s"),
                s => format!("{} minutos", s / 60),
            };
            let aviso = format!("(passou de {prazo} e foi interrompido)");
            (false, if texto.is_empty() { aviso } else { format!("{texto}\n{aviso}") })
        }
    }
}

pub fn final_do_texto(texto: &str, limite: usize) -> String {
    if texto.len() <= limite {
        return texto.to_string();
    }
    let mut inicio = texto.len() - limite;
    while !texto.is_char_boundary(inicio) {
        inicio += 1;
    }
    format!("…{}", &texto[inicio..])
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

/// Foto dos arquivos da pasta (rastreados e novos, sem lixo) num commit solto, sem mexer no
/// índice nem no histórico: vira um ponto de volta antes de cada turno. A referência fica no grupo
/// da tarefa (`refs/vitral/pontos/t<N>/`), que limpar_copia apaga e a poda depois de cada vez
/// encurta: o ramo da cópia pode ter qualquer nome. Sem grupo, vale o do ramo `vitral/t<N>-…`.
pub fn foto(pasta: &Path, grupo: Option<&str>) -> Result<String> {
    let grupo = grupo.map(str::to_string).or_else(|| ramo_atual(pasta).and_then(|branch| grupo_do_branch(&branch)));
    let commit = foto_solta(pasta)?;
    let nome = match &grupo {
        Some(grupo) => format!("refs/vitral/pontos/{grupo}/{commit}"),
        None => format!("refs/vitral/pontos/{commit}"),
    };
    // Uma referência guarda o commit da limpeza automática do git.
    git(pasta, &["update-ref", &nome, &commit])?;
    Ok(commit)
}

/// Foto sem referência, para comparar na hora (o git só apaga um commit solto depois de 2 semanas).
fn foto_solta(pasta: &Path) -> Result<String> {
    let indice = std::env::temp_dir().join(format!("vitral-indice-{}", crate::modelo::novo_id()));
    let resultado = (|| {
        let com_indice = |args: &[&str]| -> Result<String> {
            let saida = novo_comando("git")
                .arg("-C")
                .arg(pasta)
                .args(args)
                .env("GIT_INDEX_FILE", &indice)
                .output()?;
            if !saida.status.success() {
                bail!("{}", String::from_utf8_lossy(&saida.stderr).trim());
            }
            Ok(String::from_utf8_lossy(&saida.stdout).trim().to_string())
        };
        // Parte do índice atual: o git só relê os arquivos que mudaram (rápido).
        if let Ok(atual) = git(pasta, &["rev-parse", "--path-format=absolute", "--git-path", "index"]) {
            let _ = std::fs::copy(atual, &indice);
        }
        let mut args = vec!["add", "-A", "--"];
        args.extend(SEM_LIXO);
        com_indice(&args)?;
        let arvore = com_indice(&["write-tree"])?;
        git(pasta, &["commit-tree", &arvore, "-m", "ponto de volta do Vitral"])
    })();
    let _ = std::fs::remove_file(&indice);
    resultado
}

/// Volta os arquivos da pasta ao que estavam na foto: restaura o que mudou e apaga o que foi
/// criado depois. O histórico e os commits não mudam.
pub fn voltar_para(pasta: &Path, foto_antiga: &str) -> Result<usize> {
    let agora = foto_solta(pasta)?;
    let diferencas = git(
        pasta,
        &["diff", "--name-status", "--no-renames", "-z", foto_antiga, &agora],
    )?;
    let campos = separar_nulos(&diferencas);
    let (mut mexidos, mut falharam) = (0, Vec::new());
    for par in campos.chunks_exact(2) {
        let (estado, arquivo) = (par[0].as_str(), par[1].as_str());
        let destino = pasta.join(arquivo);
        let voltou = if estado == "A" {
            match std::fs::remove_file(&destino) {
                Ok(()) => true,
                Err(erro) => erro.kind() == std::io::ErrorKind::NotFound,
            }
        } else {
            novo_comando("git")
                .arg("-C")
                .arg(pasta)
                .args(["show", &format!("{foto_antiga}:{arquivo}")])
                .output()
                .ok()
                .filter(|c| c.status.success())
                .is_some_and(|conteudo| {
                    destino.parent().is_none_or(|pai| std::fs::create_dir_all(pai).is_ok())
                        && std::fs::write(&destino, conteudo.stdout).is_ok()
                })
        };
        if voltou {
            mexidos += 1;
        } else {
            falharam.push(arquivo.to_string());
        }
    }
    if !falharam.is_empty() {
        bail!("{}", volta_parcial(mexidos, &falharam));
    }
    Ok(mexidos)
}

/// O erro do voltar no tempo com falha: quantos voltaram (se algum) e quais não, no singular ou
/// no plural.
fn volta_parcial(mexidos: usize, falharam: &[String]) -> String {
    let quais = falharam.join(", ");
    let nao = if falharam.len() == 1 { "este não" } else { "estes não" };
    match mexidos {
        0 => quais,
        1 => format!("1 arquivo voltou, mas {nao}: {quais}"),
        n => format!("{n} arquivos voltaram, mas {nao}: {quais}"),
    }
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

/// A pasta é um repositório git ou uma cópia (worktree) de um.
pub fn e_repositorio(pasta: &Path) -> bool {
    git(pasta, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|s| s == "true")
}

/// O commit em que a pasta está (o ponto de partida de um pedido na pasta própria).
pub fn cabeca(pasta: &Path) -> Option<String> {
    git(pasta, &["rev-parse", "HEAD"]).ok().filter(|h| !h.is_empty())
}

/// Os arquivos rastreados com mudanças sem commit (o que faz uma junção recusar).
pub fn arquivos_sem_commit(pasta: &Path) -> Vec<String> {
    git(pasta, &["status", "--porcelain", "--untracked-files=no"])
        .map(|t| t.lines().filter_map(|l| l.get(3..)).map(str::to_string).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod testes {
    use super::*;

    fn repositorio() -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-teste-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        git(&pasta, &["init", "-q"]).unwrap();
        // O Git para Windows vem com core.autocrlf=true, e os testes comparam o conteúdo byte a byte.
        git(&pasta, &["config", "core.autocrlf", "false"]).unwrap();
        std::fs::write(pasta.join("a.txt"), "um\n").unwrap();
        git(&pasta, &["add", "-A"]).unwrap();
        git(&pasta, &["commit", "-q", "-m", "inicio"]).unwrap();
        pasta
    }

    #[test]
    fn voltar_ao_ponto_restaura_e_apaga_o_que_veio_depois() {
        let pasta = repositorio();
        std::fs::write(pasta.join("a.txt"), "dois\n").unwrap();
        let ponto = foto(&pasta, None).unwrap();
        std::fs::write(pasta.join("a.txt"), "tres\n").unwrap();
        std::fs::write(pasta.join("novo.txt"), "x\n").unwrap();
        std::fs::create_dir_all(pasta.join("sub")).unwrap();
        std::fs::write(pasta.join("sub/b.txt"), "y\n").unwrap();
        let mexidos = voltar_para(&pasta, &ponto).unwrap();
        assert_eq!(mexidos, 3);
        assert_eq!(std::fs::read_to_string(pasta.join("a.txt")).unwrap(), "dois\n");
        assert!(!pasta.join("novo.txt").exists());
        assert!(!pasta.join("sub/b.txt").exists());
        // O índice e o histórico de verdade não mudam.
        assert_eq!(git(&pasta, &["log", "--oneline"]).unwrap().lines().count(), 1);
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn volta_parcial_diz_quantos_e_quais_no_singular_ou_plural() {
        let um = vec!["a.txt".to_string()];
        let dois = vec!["a.txt".to_string(), "b.txt".to_string()];
        assert_eq!(volta_parcial(0, &dois), "a.txt, b.txt");
        assert_eq!(volta_parcial(1, &um), "1 arquivo voltou, mas este não: a.txt");
        assert_eq!(volta_parcial(2, &dois), "2 arquivos voltaram, mas estes não: a.txt, b.txt");
    }

    #[test]
    fn voltar_ao_ponto_com_acento_conta_certo() {
        let pasta = repositorio();
        std::fs::write(pasta.join("ação.txt"), "antes\n").unwrap();
        let ponto = foto(&pasta, None).unwrap();
        std::fs::write(pasta.join("ação.txt"), "depois\n").unwrap();
        std::fs::write(pasta.join("configuração.md"), "x\n").unwrap();
        assert_eq!(voltar_para(&pasta, &ponto).unwrap(), 2);
        assert_eq!(std::fs::read_to_string(pasta.join("ação.txt")).unwrap(), "antes\n");
        assert!(!pasta.join("configuração.md").exists());
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn nomes_especiais_saem_legiveis_e_descartar_volta_de_verdade() {
        let projeto = repositorio();
        let copia = criar_worktree(&projeto, &projeto.with_extension("copia"), "vitral/t1-nomes").unwrap();
        let (pasta, base) = (&copia.caminho, &copia.base);
        // No Windows, aspas não valem em nome de arquivo.
        let nomes: Vec<&str> = ["a.txt", "ação.txt", "com\"aspas.txt", "meu arquivo.txt", "pasta/configuração.md"]
            .into_iter()
            .filter(|nome| !(cfg!(windows) && nome.contains('"')))
            .collect();
        std::fs::create_dir_all(pasta.join("pasta")).unwrap();
        for nome in &nomes {
            std::fs::write(pasta.join(nome), "mudou\n").unwrap();
        }
        let mut mudados = arquivos_mudados(pasta, base);
        mudados.sort();
        assert_eq!(mudados, nomes);
        // O que a Revisão lê do diff: cabeçalho (binários) e `+++ b/` (o resto).
        let texto = diff(pasta, base).unwrap();
        let mut cabecalhos: Vec<String> = texto
            .lines()
            .filter_map(|l| l.strip_prefix("diff --git "))
            .map(caminho_do_cabecalho)
            .collect();
        let mut mais: Vec<String> = texto.lines().filter_map(caminho_do_diff).collect();
        cabecalhos.sort();
        mais.sort();
        mais.dedup();
        assert_eq!(cabecalhos, nomes);
        assert_eq!(mais, nomes);
        for nome in nomes {
            descartar_arquivo(pasta, base, nome).unwrap();
        }
        assert!(arquivos_mudados(pasta, base).is_empty());
        assert_eq!(std::fs::read_to_string(pasta.join("a.txt")).unwrap(), "um\n");
        assert!(!pasta.join("ação.txt").exists());
        assert!(descartar_arquivo(pasta, base, "não-existe.txt").is_err());
        remover_worktree(&projeto, pasta, &copia.branch);
        let _ = std::fs::remove_dir_all(&projeto);
    }

    #[test]
    fn descartar_um_renomeado_traz_o_nome_antigo_de_volta() {
        let projeto = repositorio();
        let original = "primeira linha do arquivo\nsegunda linha\nterceira linha\n";
        std::fs::write(projeto.join("velho.txt"), original).unwrap();
        git(&projeto, &["add", "-A"]).unwrap();
        git(&projeto, &["commit", "-q", "-m", "velho"]).unwrap();
        let copia = criar_worktree(&projeto, &projeto.with_extension("copia"), "vitral/t4-renomear").unwrap();
        let (pasta, base) = (&copia.caminho, &copia.base);
        std::fs::rename(pasta.join("velho.txt"), pasta.join("novo.txt")).unwrap();
        // O Revisar mostra uma entrada só, com o nome novo.
        let texto = diff_completo(pasta, base).unwrap();
        assert!(texto.contains("rename from velho.txt") && texto.contains("rename to novo.txt"), "{texto}");
        assert_eq!(descartar_arquivo(pasta, base, "novo.txt").unwrap().as_deref(), Some("velho.txt"));
        assert!(!pasta.join("novo.txt").exists());
        assert_eq!(std::fs::read_to_string(pasta.join("velho.txt")).unwrap(), original);
        assert!(arquivos_mudados(pasta, base).is_empty());
        assert!(git(pasta, &["status", "--porcelain"]).unwrap().is_empty());
        remover_worktree(&projeto, pasta, &copia.branch);
        let _ = std::fs::remove_dir_all(&projeto);
    }

    #[test]
    fn caminhos_do_diff_com_aspas_espaco_e_dev_null() {
        assert_eq!(tirar_aspas(r#""configura\303\247\303\243o.md""#), "configuração.md");
        assert_eq!(tirar_aspas(r#""a\"b\\c\td""#), "a\"b\\c\td");
        assert_eq!(tirar_aspas("simples.txt"), "simples.txt");
        assert_eq!(
            caminho_do_cabecalho(r#""a/a\303\247\303\243o.txt" "b/a\303\247\303\243o.txt""#),
            "ação.txt"
        );
        assert_eq!(caminho_do_cabecalho("a/meu arquivo.txt b/meu arquivo.txt"), "meu arquivo.txt");
        assert_eq!(caminho_do_diff("+++ b/meu arquivo.txt\t").as_deref(), Some("meu arquivo.txt"));
        assert_eq!(caminho_do_diff("--- a/x.rs").as_deref(), Some("x.rs"));
        assert_eq!(caminho_do_diff("+++ /dev/null"), None);
        assert_eq!(caminho_do_diff("-- comentário"), None);
    }

    #[test]
    fn depois_do_merge_pedido_no_conflito_a_base_e_o_que_veio_do_projeto() {
        let projeto = repositorio();
        let c1 = criar_worktree(&projeto, &projeto.with_extension("c1"), "vitral/t1-conflito").unwrap();
        let c2 = criar_worktree(&projeto, &projeto.with_extension("c2"), "vitral/t2-conflito").unwrap();
        std::fs::write(c1.caminho.join("a.txt"), "da tarefa 1\n").unwrap();
        std::fs::write(c1.caminho.join("b.txt"), "novo da tarefa 1\n").unwrap();
        let juncao = aprovar(&projeto, &c1.caminho, &c1.branch, &c1.base, "t1").unwrap();
        assert!(matches!(juncao, Juncao::Juntou { .. }));
        remover_worktree(&projeto, &c1.caminho, &c1.branch);

        std::fs::write(c2.caminho.join("a.txt"), "da tarefa 2\n").unwrap();
        let Juncao::Conflito { alvo, .. } = aprovar(&projeto, &c2.caminho, &c2.branch, &c2.base, "t2").unwrap()
        else {
            panic!("devia dar conflito");
        };
        // Sem merge na cópia, vale a base original, mesmo com o projeto à frente.
        assert_eq!(base_efetiva(&projeto, &c2.caminho, &c2.base), c2.base);
        // O trabalhador roda o `git merge` pedido e resolve; ainda sem commit já vale o alvo.
        assert!(git(&c2.caminho, &["merge", &alvo]).is_err());
        std::fs::write(c2.caminho.join("a.txt"), "das duas\n").unwrap();
        assert_eq!(base_efetiva(&projeto, &c2.caminho, &c2.base), alvo);
        git(&c2.caminho, &["add", "a.txt"]).unwrap();
        git(&c2.caminho, &["commit", "-q", "--no-edit"]).unwrap();
        let base = base_efetiva(&projeto, &c2.caminho, &c2.base);
        assert_eq!(base, alvo);
        // b.txt é da tarefa 1: não aparece como desta nem some ao descartar.
        assert_eq!(arquivos_mudados(&c2.caminho, &base), ["a.txt"]);
        descartar_arquivo(&c2.caminho, &base, "b.txt").unwrap();
        assert!(c2.caminho.join("b.txt").exists());

        let Juncao::Juntou { commits } = aprovar(&projeto, &c2.caminho, &c2.branch, &c2.base, "t2").unwrap()
        else {
            panic!("devia juntar");
        };
        assert!(!commits.lines().any(|l| l.ends_with(" t1")), "{commits}");
        assert_eq!(std::fs::read_to_string(projeto.join("a.txt")).unwrap(), "das duas\n");
        assert!(projeto.join("b.txt").exists());
        remover_worktree(&projeto, &c2.caminho, &c2.branch);
        let _ = std::fs::remove_dir_all(&projeto);
    }

    #[test]
    fn ativar_git_deixa_segredos_e_ambientes_de_fora_e_lixo_solto_nao_trava_a_juncao() {
        let projeto = std::env::temp_dir().join(format!("vitral-init-{}", crate::modelo::novo_id()));
        for pasta in ["__pycache__", ".venv", "ambiente", "src"] {
            std::fs::create_dir_all(projeto.join(pasta)).unwrap();
        }
        for (arquivo, conteudo) in [
            ("app.py", "print(1)\n"),
            ("requirements.txt", "pytest\n"),
            (".env", "SENHA=1\n"),
            (".env.example", "SENHA=\n"),
            ("__pycache__/app.pyc", "lixo"),
            (".venv/pyvenv.cfg", "home = /usr\n"),
            ("ambiente/pyvenv.cfg", "home = /usr\n"),
            ("src/ação.py", "x = 1\n"),
        ] {
            std::fs::write(projeto.join(arquivo), conteudo).unwrap();
        }
        let novo = iniciar_repositorio(&projeto).unwrap();
        let mut rastreados = separar_nulos(&git(&projeto, &["ls-files", "-z"]).unwrap());
        rastreados.sort();
        assert_eq!(rastreados, [".env.example", ".gitignore", "app.py", "requirements.txt", "src/ação.py"]);
        assert_eq!(novo.arquivos, 5);
        assert!(novo.ignorados.contains(&"/ambiente/".to_string()));
        assert!(novo.resumo("p").contains("Criei um .gitignore que deixa de fora .env"), "{}", novo.resumo("p"));
        // Com .gitignore já existente, ele fica como está.
        assert!(iniciar_repositorio(&projeto).unwrap().ignorados.is_empty());

        let copia = criar_worktree(&projeto, &projeto.with_extension("copia"), "vitral/t1-init").unwrap();
        // Testes rodados na pasta do projeto deixam arquivos soltos que nenhum .gitignore cobre.
        std::fs::write(projeto.join("saida-dos-testes.txt"), "x").unwrap();
        assert!(!criar_worktree(&projeto, &projeto.with_extension("outra"), "vitral/t2-init").unwrap().sujo);
        std::fs::write(copia.caminho.join("novo.py"), "y = 2\n").unwrap();
        let juncao = aprovar(&projeto, &copia.caminho, &copia.branch, &copia.base, "t1").unwrap();
        assert!(matches!(juncao, Juncao::Juntou { .. }));
        remover_worktree(&projeto, &copia.caminho, &copia.branch);
        remover_worktree(&projeto, &projeto.with_extension("outra"), "vitral/t2-init");
        let _ = std::fs::remove_dir_all(&projeto);
    }

    #[test]
    fn localizar_a_origem_movida_religa_as_copias() {
        let projeto = repositorio();
        let copia = criar_worktree(&projeto, &projeto.with_extension("copia"), "vitral/t1-movida").unwrap();
        let movida = projeto.with_extension("movida");
        std::fs::rename(&projeto, &movida).unwrap();
        assert!(raiz_comum(&copia.caminho).is_none(), "a cópia aponta para a pasta antiga");
        // Outro repositório (o `repositorio()` no mesmo segundo daria o mesmo commit).
        let outro = projeto.with_extension("outro");
        std::fs::create_dir_all(&outro).unwrap();
        git(&outro, &["init", "-q"]).unwrap();
        std::fs::write(outro.join("b.txt"), "outro\n").unwrap();
        git(&outro, &["add", "-A"]).unwrap();
        git(&outro, &["commit", "-q", "-m", "outro"]).unwrap();
        assert!(tem_commit(&movida, &copia.base));
        assert!(!tem_commit(&outro, &copia.base), "outro repositório não tem a base da cópia");
        assert!(reparar_copias(&movida, &[copia.caminho.clone()]).is_empty());
        assert_eq!(raiz_comum(&copia.caminho), raiz_comum(&movida));
        assert_eq!(reparar_copias(&outro, &[copia.caminho.clone()]), [copia.caminho.clone()]);
        assert_eq!(raiz_comum(&copia.caminho), raiz_comum(&movida), "o repair de outro repositório não a tira do dela");
        remover_worktree(&movida, &copia.caminho, &copia.branch);
        let _ = std::fs::remove_dir_all(&movida);
        let _ = std::fs::remove_dir_all(&outro);
    }

    #[test]
    fn copias_dos_agentes_do_fluxo_nao_entram_no_commit() {
        let pasta = repositorio();
        let agente = pasta.join(".claude/worktrees/agente-1");
        std::fs::create_dir_all(&agente).unwrap();
        std::fs::write(agente.join("a.txt"), "do agente\n").unwrap();
        std::fs::write(pasta.join(".claude/settings.json"), "{}\n").unwrap();
        std::fs::write(pasta.join("b.txt"), "da tarefa\n").unwrap();
        adicionar(&pasta, "-A").unwrap();
        let indice = separar_nulos(&git(&pasta, &["ls-files", "-z"]).unwrap());
        assert!(indice.iter().any(|a| a == "b.txt") && indice.iter().any(|a| a == ".claude/settings.json"));
        assert!(!indice.iter().any(|a| a.starts_with(".claude/worktrees")), "{indice:?}");
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn arquivo_solto_no_caminho_de_um_novo_da_tarefa_nao_vira_conflito() {
        let projeto = repositorio();
        let copia = criar_worktree(&projeto, &projeto.with_extension("copia"), "vitral/t1-solto").unwrap();
        std::fs::create_dir_all(copia.caminho.join("docs")).unwrap();
        std::fs::write(copia.caminho.join("novo.txt"), "da tarefa\n").unwrap();
        std::fs::write(copia.caminho.join("docs/guia.md"), "da tarefa\n").unwrap();
        std::fs::write(projeto.join("novo.txt"), "do usuário\n").unwrap();
        std::fs::write(projeto.join("docs"), "um arquivo onde a tarefa cria uma pasta\n").unwrap();
        std::fs::write(projeto.join("outro.txt"), "não atrapalha\n").unwrap();
        let Juncao::SoltosNoProjeto { mut arquivos } =
            aprovar(&projeto, &copia.caminho, &copia.branch, &copia.base, "t1").unwrap()
        else {
            panic!("devia avisar dos arquivos soltos");
        };
        arquivos.sort();
        assert_eq!(arquivos, ["docs", "novo.txt"]);
        // O projeto não foi tocado; tirando os soltos, junta.
        assert_eq!(std::fs::read_to_string(projeto.join("novo.txt")).unwrap(), "do usuário\n");
        std::fs::remove_file(projeto.join("novo.txt")).unwrap();
        std::fs::remove_file(projeto.join("docs")).unwrap();
        let juncao = aprovar(&projeto, &copia.caminho, &copia.branch, &copia.base, "t1").unwrap();
        assert!(matches!(juncao, Juncao::Juntou { .. }));
        remover_worktree(&projeto, &copia.caminho, &copia.branch);
        let _ = std::fs::remove_dir_all(&projeto);
    }

    // A verificação roda no bash: no Windows ela vai para o Git Bash (Lote W2).
    #[cfg(unix)]
    #[test]
    fn verificacao_com_saida_grande_nao_trava_e_guarda_o_fim() {
        let pasta = std::env::temp_dir();
        let inicio = Instant::now();
        let v = verificar(&pasta, "yes linha | head -c 300000; echo FIM; exit 0");
        assert!(v.passou, "{}", v.saida);
        assert!(inicio.elapsed() < Duration::from_secs(10));
        assert!(v.saida.starts_with('…') && v.saida.ends_with("FIM"), "{}", v.saida);
        assert!(v.saida.len() <= 4010);
        assert!(v.duracao_s.is_some());

        let v = verificar(&pasta, "echo a; echo b >&2; exit 3");
        assert!(!v.passou);
        assert!(v.saida.ends_with("a\nb"), "{}", v.saida);
    }

    #[cfg(unix)]
    #[test]
    fn verificacao_derruba_o_que_ficou_em_segundo_plano() {
        let pasta = std::env::temp_dir().join(format!("vitral-verif-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        let inicio = Instant::now();
        // O servidor de teste fica com o pipe aberto: antes, a verificação nunca voltava.
        let v = verificar(&pasta, "sleep 120 & echo $! > servidor; echo pronto");
        assert!(v.passou && v.saida.ends_with("pronto"), "{}", v.saida);
        assert!(inicio.elapsed() < Duration::from_secs(5));
        let servidor: u32 = std::fs::read_to_string(pasta.join("servidor")).unwrap().trim().parse().unwrap();
        assert!(processos::morre_logo(servidor));

        let (passou, saida) = rodar_com_limite(&pasta, "echo comecei; sleep 120", Duration::from_secs(1));
        assert!(!passou);
        assert!(saida.starts_with("comecei") && saida.ends_with("(passou de 1 s e foi interrompido)"), "{saida}");

        if processos::usa_systemd() {
            // Nem quem se solta do grupo com setsid escapa do escopo.
            let v = verificar(&pasta, "setsid sleep 120 & echo $! > solto; echo ok");
            assert!(v.passou, "{}", v.saida);
            let solto: u32 = std::fs::read_to_string(pasta.join("solto")).unwrap().trim().parse().unwrap();
            assert!(processos::morre_logo(solto));
        }
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn gerados_pelo_nome_do_arquivo() {
        for gerado in ["Cargo.lock", "web/package-lock.json", "pnpm-lock.yaml", "dist/app.min.js",
            "estilo.min.css", "app.js.map", "testes/__snapshots__/tela.snap", "npm-shrinkwrap.json"]
        {
            assert!(eh_gerado(gerado), "{gerado}");
        }
        for fonte in ["src/main.rs", "package.json", "lock", ".lock", "app.js", "mapa.rs", "lockfile.txt"] {
            assert!(!eh_gerado(fonte), "{fonte}");
        }
    }

    #[test]
    fn diff_para_coordenador_deixa_os_gerados_de_fora_e_resume_o_que_nao_cabe() {
        let projeto = repositorio();
        std::fs::create_dir_all(projeto.join("src")).unwrap();
        std::fs::write(projeto.join("Cargo.lock"), "versao 1\n").unwrap();
        std::fs::write(projeto.join("src/main.rs"), "fn main() {}\n").unwrap();
        git(&projeto, &["add", "-A"]).unwrap();
        git(&projeto, &["commit", "-q", "-m", "lock"]).unwrap();
        let copia = criar_worktree(&projeto, &projeto.with_extension("copia"), "vitral/t3-diff").unwrap();
        let (pasta, base) = (&copia.caminho, &copia.base);
        let lock: String = (0..3000).map(|n| format!("dependencia-{n} = \"1.0\"\n")).collect();
        std::fs::write(pasta.join("Cargo.lock"), &lock).unwrap();
        std::fs::write(pasta.join("src/main.rs"), "fn main() { println!(\"oi\"); }\n").unwrap();
        std::fs::create_dir_all(pasta.join("web")).unwrap();
        std::fs::write(pasta.join("web/package-lock.json"), "{}\n").unwrap();

        // O lockfile vem antes de src/ no diff: sem tirar, ele enchia o corte sozinho.
        let texto = diff_para_coordenador(pasta, &projeto, base, &[], &[], 20_000).unwrap();
        assert!(texto.contains("+fn main() { println!(\"oi\"); }"), "{texto}");
        assert!(!texto.contains("dependencia-1"), "{texto}");
        assert!(texto.contains("Cargo.lock (+3000 −1)") && texto.contains("web/package-lock.json (+1 −0)"), "{texto}");
        // Pedido pelo nome, o gerado vem (cortado em 60 mil, com o caminho para ler o resto).
        let pedido = diff_para_coordenador(pasta, &projeto, base, &["Cargo.lock".into()], &[], 20_000).unwrap();
        assert!(pedido.contains("diff --git a/Cargo.lock b/Cargo.lock") && pedido.contains("+dependencia-1 "));
        assert!(pedido.contains("Para ver o resto, leia direto em"), "{}", final_do_texto(&pedido, 300));
        let absoluto = pasta.join("src/main.rs").to_string_lossy().to_string();
        let main = diff_para_coordenador(pasta, &projeto, base, &[absoluto], &[], 20_000).unwrap();
        assert!(main.contains("b/src/main.rs") && !main.contains("Cargo.lock"), "{main}");
        // O coordenador roda no projeto: o caminho absoluto de lá também vale.
        let do_projeto = projeto.join("src/main.rs").to_string_lossy().to_string();
        let main = diff_para_coordenador(pasta, &projeto, base, &[do_projeto], &[], 20_000).unwrap();
        assert!(main.contains("b/src/main.rs") && !main.contains("Cargo.lock"), "{main}");
        // Um caminho absoluto fora do projeto (no Windows, "/etc/hosts" não é absoluto).
        let de_fora = if cfg!(windows) { r"C:\Windows\win.ini" } else { "/etc/hosts" };
        let fora = diff_para_coordenador(pasta, &projeto, base, &[de_fora.into()], &[], 20_000).unwrap();
        assert!(fora.contains(&format!("{de_fora} não fica dentro do projeto")), "{fora}");

        // Grande demais: --stat, os arquivos que cabem inteiros (o apontado primeiro) e o que falta.
        for nome in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            let linhas: String = (0..700).map(|n| format!("{nome} linha {n}\n")).collect();
            std::fs::write(pasta.join(nome), linhas).unwrap();
        }
        let resumo = diff_para_coordenador(pasta, &projeto, base, &[], &["d.txt".into()], 20_000).unwrap();
        assert!(resumo.contains("vai um resumo") && resumo.contains("Arquivos alterados"), "{resumo}");
        assert!(resumo.contains("+d.txt linha 0\n") && resumo.contains("+d.txt linha 699\n"), "{resumo}");
        assert!(resumo.contains("+fn main()"), "{resumo}");
        assert_eq!(resumo.matches("diff --git ").count(), 2, "{resumo}");
        assert!(resumo.contains("Faltam 3 arquivo(s): a.txt (+700 −1), b.txt (+700 −0), c.txt (+700 −0)."), "{resumo}");
        assert!(resumo.contains("Cargo.lock (+3000 −1)"));
        assert!(resumo.len() < 22_000, "{}", resumo.len());
        // Nem um arquivo cabe inteiro: vai o começo do primeiro e onde ler o resto.
        descartar_arquivo(pasta, base, "src/main.rs").unwrap();
        let cortado = diff_para_coordenador(pasta, &projeto, base, &[], &[], 5_000).unwrap();
        assert!(cortado.contains("+a.txt linha 0\n") && cortado.contains("não cabe; leia o arquivo inteiro"), "{cortado}");
        assert!(cortado.contains("Faltam 3 arquivo(s): b.txt"), "{cortado}");
        remover_worktree(&projeto, pasta, &copia.branch);
        let _ = std::fs::remove_dir_all(&projeto);
    }

    #[test]
    fn limpar_copia_apaga_pasta_branch_e_so_os_pontos_da_tarefa() {
        let projeto = repositorio();
        let copia = criar_worktree(&projeto, &projeto.with_extension("t7"), "vitral/t7-limpar").unwrap();
        let outra = criar_worktree(&projeto, &projeto.with_extension("t8"), "vitral/t8-outra").unwrap();
        let pacote = copia.caminho.join("node_modules/pacote");
        std::fs::create_dir_all(&pacote).unwrap();
        // Conteúdo que não comprime, para o espaço no disco ser o tamanho de verdade.
        let mut semente = 12345u64;
        let bytes: Vec<u8> = (0..10_000)
            .map(|_| {
                semente = semente.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (semente >> 56) as u8
            })
            .collect();
        std::fs::write(pacote.join("index.js"), &bytes).unwrap();
        std::fs::hard_link(pacote.join("index.js"), pacote.join("mesmo.js")).unwrap();
        let ocupado = espaco_ocupado(&pacote);
        if cfg!(unix) {
            assert!((10_000..20_000).contains(&ocupado), "o hard link conta uma vez só: {ocupado}");
        } else {
            // No Windows, o tamanho de cada nome: o hard link conta de novo.
            assert_eq!(ocupado, 20_000);
        }

        std::fs::write(copia.caminho.join("a.txt"), "mudou\n").unwrap();
        let ponto = foto(&copia.caminho, None).unwrap();
        std::fs::write(copia.caminho.join("a.txt"), "de novo\n").unwrap();
        foto(&copia.caminho, None).unwrap();
        std::fs::write(outra.caminho.join("b.txt"), "da outra\n").unwrap();
        let da_outra = foto(&outra.caminho, None).unwrap();
        // Pontos da versão que não agrupava: um desta tarefa e um que não é dela.
        std::fs::write(copia.caminho.join("a.txt"), "antigo\n").unwrap();
        let antigo = foto_solta(&copia.caminho).unwrap();
        let alheio = foto_solta(&projeto).unwrap();
        for sha in [&antigo, &alheio] {
            git(&projeto, &["update-ref", &format!("refs/vitral/pontos/{sha}"), sha]).unwrap();
        }
        let refs = || git(&projeto, &["for-each-ref", "--format=%(refname)", "refs/vitral/"]).unwrap();
        assert!(refs().contains(&format!("refs/vitral/pontos/t7/{ponto}")), "{}", refs());
        assert_eq!(refs().lines().count(), 5, "{}", refs());
        // Voltar no tempo compara com uma foto sem referência.
        voltar_para(&copia.caminho, &ponto).unwrap();
        assert_eq!(refs().lines().count(), 5, "{}", refs());

        let area = projeto.with_extension("area");
        limpar_copia(&projeto, &copia.caminho, &area, &copia.branch, None, std::slice::from_ref(&antigo), false).unwrap();
        assert!(!copia.caminho.exists());
        assert!(git(&projeto, &["rev-parse", "--verify", "-q", "refs/heads/vitral/t7-limpar"]).is_err());
        let sobrou = refs();
        assert_eq!(sobrou.lines().count(), 2, "{sobrou}");
        assert!(sobrou.contains(&format!("pontos/t8/{da_outra}")) && sobrou.contains(&format!("pontos/{alheio}")));

        // Pasta apagada à mão: a limpeza esquece o registro e apaga o branch.
        std::fs::remove_dir_all(&outra.caminho).unwrap();
        limpar_copia(&projeto, &outra.caminho, &area, &outra.branch, None, &[], false).unwrap();
        assert_eq!(git(&projeto, &["worktree", "list", "--porcelain"]).unwrap().matches("worktree ").count(), 1);
        assert!(git(&projeto, &["branch", "--list", "vitral/*"]).unwrap().is_empty());
        assert_eq!(refs().lines().count(), 1, "{}", refs());
        // Uma pasta que não é cópia de tarefa fica onde está.
        let estranha = projeto.with_extension("estranha");
        std::fs::create_dir_all(&estranha).unwrap();
        std::fs::write(estranha.join("importante.txt"), "x").unwrap();
        assert!(limpar_copia(&projeto, &estranha, &area, "vitral/t9-x", None, &[], false).is_err());
        assert!(estranha.join("importante.txt").exists());
        let _ = std::fs::remove_dir_all(&estranha);

        // Limpeza cortada que já tinha apagado o `.git` da cópia: o git não remove, e a pasta só
        // sai à mão porque fica entre as cópias do Vitral.
        let cortada = criar_worktree(&projeto, &area.join("t10"), "vitral/t10-cortada").unwrap();
        std::fs::create_dir_all(cortada.caminho.join("node_modules/pacote")).unwrap();
        std::fs::remove_file(cortada.caminho.join(".git")).unwrap();
        podar(&projeto);
        limpar_copia(&projeto, &cortada.caminho, &area, &cortada.branch, None, &[], false).unwrap();
        assert!(!cortada.caminho.exists());
        assert!(git(&projeto, &["branch", "--list", "vitral/*"]).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&area);
        let _ = std::fs::remove_dir_all(&projeto);
    }

    /// O aceite do G2: com um ramo de qualquer nome, a foto vai para o grupo da tarefa e a
    /// limpeza da cópia a apaga.
    #[test]
    fn foto_de_ramo_livre_fica_no_grupo_da_tarefa() {
        let projeto = repositorio();
        let copia = criar_worktree(&projeto, &projeto.with_extension("t7"), "feature/vitor-x").unwrap();
        std::fs::write(copia.caminho.join("a.txt"), "mudou\n").unwrap();
        let ponto = foto(&copia.caminho, Some("t7")).unwrap();
        let refs = || git(&projeto, &["for-each-ref", "--format=%(refname)", "refs/vitral/"]).unwrap();
        assert_eq!(refs(), format!("refs/vitral/pontos/t7/{ponto}"));
        // Sem o grupo, um ramo que não é `vitral/t<N>` não diz de quem é a foto: ela fica solta.
        let solta = foto(&copia.caminho, None).unwrap_or_default();
        assert!(refs().contains(&format!("refs/vitral/pontos/{solta}")), "{}", refs());
        let area = projeto.with_extension("area");
        limpar_copia(&projeto, &copia.caminho, &area, &copia.branch, Some("t7"), &[], false).unwrap();
        assert!(!copia.caminho.exists());
        assert!(!refs().contains("pontos/t7/"), "{}", refs());
        let _ = std::fs::remove_dir_all(&projeto);
    }

    #[test]
    fn grupo_dos_pontos_e_tamanho_legivel() {
        assert_eq!(grupo_do_branch("vitral/t12-arrumar-o-menu").as_deref(), Some("t12"));
        assert_eq!(grupo_do_branch("vitral/t5-").as_deref(), Some("t5"));
        assert_eq!(grupo_do_branch("vitral/t5").as_deref(), Some("t5"));
        assert_eq!(grupo_do_branch("vitral/t12x"), None);
        assert_eq!(grupo_do_branch("main"), None);
        assert_eq!(tamanho(1536), "2 KB");
        assert_eq!(tamanho(5 * 1024 * 1024 / 2), "2,5 MB");
        assert_eq!(tamanho(3 * 1024 * 1024 * 1024 / 2), "1,5 GB");
    }

    #[test]
    fn copia_isolada_aprova_e_junta() {
        let projeto = repositorio();
        let destino = projeto.with_extension("copia");
        let copia = criar_worktree(&projeto, &destino, "vitral/t1-teste").unwrap();
        std::fs::write(copia.caminho.join("b.txt"), "novo\n").unwrap();
        std::fs::create_dir_all(copia.caminho.join("__pycache__")).unwrap();
        std::fs::write(copia.caminho.join("__pycache__/x.pyc"), "lixo").unwrap();
        assert_eq!(arquivos_mudados(&copia.caminho, &copia.base), vec!["b.txt".to_string()]);
        let juncao = aprovar(&projeto, &copia.caminho, &copia.branch, &copia.base, "teste").unwrap();
        assert!(matches!(juncao, Juncao::Juntou { .. }));
        assert!(projeto.join("b.txt").exists());
        assert!(!projeto.join("__pycache__").exists());
        remover_worktree(&projeto, &copia.caminho, &copia.branch);
        let _ = std::fs::remove_dir_all(&projeto);
    }

    #[test]
    fn copia_com_nome_ja_registrado_ganha_sufixo() {
        let projeto = repositorio();
        let destino = projeto.with_extension("t1");
        let primeira = criar_worktree(&projeto, &destino, "vitral/t1-teste").unwrap();
        // A pasta some (ex.: .vitral apagado), mas o branch e o registro continuam no git.
        std::fs::remove_dir_all(&primeira.caminho).unwrap();
        let segunda = criar_worktree(&projeto, &destino, "vitral/t1-teste").unwrap();
        assert_eq!(segunda.branch, "vitral/t1-teste-2");
        assert_eq!(segunda.caminho, projeto.with_extension("t1-2"));
        assert!(segunda.caminho.join("a.txt").exists());
        // Pasta ocupada por outra coisa, com branch livre: também não apaga nada.
        git(&projeto, &["branch", "-D", "vitral/t1-teste"]).unwrap();
        std::fs::create_dir_all(&destino).unwrap();
        let terceira = criar_worktree(&projeto, &destino, "vitral/t1-teste").unwrap();
        assert_eq!(terceira.caminho, projeto.with_extension("t1-3"));
        assert_eq!(terceira.branch, "vitral/t1-teste-3");
        for copia in [&segunda, &terceira] {
            remover_worktree(&projeto, &copia.caminho, &copia.branch);
        }
        let _ = std::fs::remove_dir_all(&destino);
        let _ = std::fs::remove_dir_all(&projeto);
    }

    #[test]
    fn git_ocupado_nao_vira_conflito_e_conflito_de_verdade_e_desfeito() {
        let projeto = repositorio();
        let copia = criar_worktree(&projeto, &projeto.with_extension("ocupado"), "vitral/t1-o").unwrap();
        std::fs::write(copia.caminho.join("b.txt"), "novo\n").unwrap();
        // Outro git segurando o índice do projeto: espera, desiste e não chama de conflito.
        let trava = projeto.join(".git/index.lock");
        std::fs::write(&trava, "").unwrap();
        let juncao = aprovar(&projeto, &copia.caminho, &copia.branch, &copia.base, "teste").unwrap();
        assert!(matches!(juncao, Juncao::Ocupado { .. }));
        std::fs::remove_file(&trava).unwrap();
        let juncao = aprovar(&projeto, &copia.caminho, &copia.branch, &copia.base, "teste").unwrap();
        assert!(matches!(juncao, Juncao::Juntou { .. }));
        remover_worktree(&projeto, &copia.caminho, &copia.branch);

        // Conflito de verdade: lista o arquivo e não deixa merge pela metade na cópia.
        let copia = criar_worktree(&projeto, &projeto.with_extension("conflito"), "vitral/t2-c").unwrap();
        std::fs::write(copia.caminho.join("a.txt"), "da tarefa\n").unwrap();
        std::fs::write(projeto.join("a.txt"), "do projeto\n").unwrap();
        git(&projeto, &["commit", "-q", "-am", "outra tarefa"]).unwrap();
        let juncao = aprovar(&projeto, &copia.caminho, &copia.branch, &copia.base, "teste").unwrap();
        let Juncao::Conflito { arquivos, alvo, .. } = juncao else {
            panic!("devia ser conflito");
        };
        assert_eq!(arquivos, vec!["a.txt".to_string()]);
        assert_eq!(alvo, git(&projeto, &["rev-parse", "HEAD"]).unwrap());
        assert!(git(&copia.caminho, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).is_err());
        remover_worktree(&projeto, &copia.caminho, &copia.branch);
        let _ = std::fs::remove_dir_all(&projeto);
    }

    #[test]
    fn na_vez_do_projeto_um_de_cada_vez() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DENTRO: AtomicUsize = AtomicUsize::new(0);
        let pasta = std::env::temp_dir().join(format!("vitral-vez-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        let fios: Vec<_> = (0..4)
            .map(|_| {
                let pasta = pasta.clone();
                std::thread::spawn(move || {
                    na_vez_do_projeto(&pasta, || {
                        assert_eq!(DENTRO.fetch_add(1, Ordering::SeqCst), 0);
                        std::thread::sleep(Duration::from_millis(30));
                        DENTRO.fetch_sub(1, Ordering::SeqCst);
                    })
                })
            })
            .collect();
        for fio in fios {
            fio.join().unwrap();
        }
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    #[should_panic(expected = "dentro dela mesma")]
    fn na_vez_nao_e_reentrante() {
        let projeto = repositorio();
        let copia = criar_worktree(&projeto, &projeto.with_extension("reentra"), "vitral/t1-r").unwrap();
        // A cópia tem a mesma vez que a origem: pedir a dela dentro da da origem travaria.
        na_vez_do_projeto(&projeto, || na_vez_do_projeto(&copia.caminho, || ()));
    }

    fn commit_em(pasta: &Path, arquivo: &str, texto: &str) {
        std::fs::write(pasta.join(arquivo), texto).unwrap();
        git(pasta, &["add", "-A"]).unwrap();
        git(pasta, &["commit", "-q", "-m", arquivo]).unwrap();
    }

    fn limpar(projeto: &Path, copias: &[&Path]) {
        for copia in copias {
            let _ = std::fs::remove_dir_all(copia);
        }
        let _ = std::fs::remove_dir_all(projeto);
    }

    #[test]
    fn duas_juncoes_seguidas_levam_so_o_que_e_novo() {
        let projeto = repositorio();
        let copia = criar_worktree(&projeto, &projeto.with_extension("t1"), "vitral/t1-juntar").unwrap();
        assert_eq!(copia.ramo.as_deref(), ramo_atual(&projeto).as_deref());
        std::fs::write(copia.caminho.join("b.txt"), "primeira\n").unwrap();
        assert_eq!(contar_mudancas(&copia.caminho, &copia.base).unwrap(), (1, 1, 0));
        let juntar_agora = |base: &str| {
            na_vez_do_projeto(&projeto, || {
                juntar(&projeto, &copia.caminho, &copia.branch, base, "teste", copia.ramo.as_deref(), "t1")
            })
            .unwrap()
        };
        let Juntada::Feita { commits: Some(_), base, andou: true } = juntar_agora(&copia.base) else {
            panic!("a primeira junção devia entrar e a cópia andar");
        };
        assert!(projeto.join("b.txt").exists() && copia.caminho.join("b.txt").exists());
        assert_eq!(base, git(&projeto, &["rev-parse", "HEAD"]).unwrap());
        assert_eq!(estado_copia(&copia.caminho, &projeto).unwrap(), EstadoCopia::default());

        std::fs::write(copia.caminho.join("c.txt"), "segunda\n").unwrap();
        let diff = diff(&copia.caminho, &base).unwrap();
        assert!(diff.contains("c.txt") && !diff.contains("b.txt"), "{diff}");
        let Juntada::Feita { commits: Some(commits), .. } = juntar_agora(&base) else {
            panic!("a segunda junção devia entrar");
        };
        assert_eq!(commits.lines().count(), 1, "{commits}");
        assert!(projeto.join("c.txt").exists());
        // Origem em outro branch: só com confirmação.
        git(&projeto, &["checkout", "-q", "-b", "outro"]).unwrap();
        let ramo = ramo_atual(&copia.caminho).unwrap();
        assert!(matches!(
            juntar(&projeto, &copia.caminho, &ramo, &base, "x", copia.ramo.as_deref(), "t1").unwrap(),
            Juntada::RamoTrocado { atual, .. } if atual == "outro"
        ));
        limpar(&projeto, &[&copia.caminho]);
    }

    #[test]
    fn depois_de_juntar_sem_avancar_fica_no_ponto_comum() {
        let projeto = repositorio();
        let copia = criar_worktree(&projeto, &projeto.with_extension("ff"), "vitral/t2-ff").unwrap();
        commit_em(&copia.caminho, "da-copia.txt", "c\n");
        commit_em(&projeto, "da-origem.txt", "o\n");
        let (base, andou) = depois_de_juntar(&copia.caminho, &projeto).unwrap();
        assert!(!andou);
        assert_eq!(base, copia.base);
        limpar(&projeto, &[&copia.caminho]);
    }

    #[test]
    fn estado_da_copia_a_frente_atras_e_suja() {
        let projeto = repositorio();
        let copia = criar_worktree(&projeto, &projeto.with_extension("estado"), "vitral/t3-e").unwrap();
        let estado = || estado_copia(&copia.caminho, &projeto).unwrap();
        assert_eq!(estado(), EstadoCopia::default());
        std::fs::create_dir_all(copia.caminho.join("__pycache__")).unwrap();
        std::fs::write(copia.caminho.join("__pycache__/x.pyc"), "lixo").unwrap();
        assert!(!estado().suja, "lixo de teste não suja a cópia");
        std::fs::write(copia.caminho.join("solto.txt"), "x").unwrap();
        assert!(estado().suja);
        std::fs::remove_file(copia.caminho.join("solto.txt")).unwrap();
        commit_em(&copia.caminho, "c.txt", "c\n");
        commit_em(&projeto, "o.txt", "o\n");
        assert_eq!(estado(), EstadoCopia { a_frente: 1, atras: 1, suja: false });
        limpar(&projeto, &[&copia.caminho]);
    }

    #[test]
    fn copia_limpa_anda_com_a_origem_e_a_com_trabalho_nao() {
        let projeto = repositorio();
        let copia = criar_worktree(&projeto, &projeto.with_extension("sinc"), "vitral/t4-s").unwrap();
        assert!(!sincronizar_copia(&copia.caminho, &projeto).unwrap(), "nada novo na origem");
        commit_em(&projeto, "o.txt", "o\n");
        std::fs::write(copia.caminho.join("meu.txt"), "x").unwrap();
        assert!(!sincronizar_copia(&copia.caminho, &projeto).unwrap(), "a cópia tem mudança sem commit");
        std::fs::remove_file(copia.caminho.join("meu.txt")).unwrap();
        assert!(sincronizar_copia(&copia.caminho, &projeto).unwrap());
        assert!(copia.caminho.join("o.txt").exists());
        assert_eq!(git(&copia.caminho, &["rev-parse", "HEAD"]).unwrap(), git(&projeto, &["rev-parse", "HEAD"]).unwrap());
        limpar(&projeto, &[&copia.caminho]);
    }

    #[test]
    fn recriar_no_mesmo_caminho_tira_a_pasta_solta_do_lugar() {
        let projeto = repositorio();
        let destino = projeto.with_extension("t5");
        let area = projeto.with_extension("area5");
        let copia = criar_worktree(&projeto, &destino, "vitral/t5-recriar").unwrap();
        commit_em(&copia.caminho, "feito.txt", "trabalho\n");
        limpar_copia(&projeto, &destino, &area, &copia.branch, None, &[], true).unwrap();
        assert!(!destino.exists());
        std::fs::create_dir_all(&destino).unwrap();
        std::fs::write(destino.join("importante.txt"), "não apague").unwrap();

        let (nova, existia) = recriar_worktree(&projeto, &destino, &copia.branch).unwrap();
        assert!(existia);
        assert_eq!((nova.caminho.as_path(), nova.branch.as_str()), (destino.as_path(), copia.branch.as_str()));
        assert!(destino.join("feito.txt").exists(), "a cópia volta do branch antigo");
        let mae = destino.parent().unwrap();
        let nome = format!("{}.antiga-", destino.file_name().unwrap().to_string_lossy());
        let antiga = std::fs::read_dir(mae)
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().starts_with(&nome))
            .expect("a pasta solta foi para o lado")
            .path();
        assert!(antiga.join("importante.txt").exists());

        // Sem o branch: do HEAD da origem, com o mesmo nome e no mesmo caminho.
        limpar_copia(&projeto, &destino, &area, &copia.branch, None, &[], false).unwrap();
        let (nova, existia) = recriar_worktree(&projeto, &destino, &copia.branch).unwrap();
        assert!(!existia && nova.branch == copia.branch && nova.caminho == destino);
        assert!(!destino.join("feito.txt").exists());
        limpar(&projeto, &[&destino, &antiga]);
    }

    #[test]
    fn juncao_cortada_desfaz_o_merge_parado_e_conta_a_que_ja_entrou() {
        let projeto = repositorio();
        let copia = criar_worktree(&projeto, &projeto.with_extension("t6"), "vitral/t6-cortada").unwrap();
        std::fs::write(copia.caminho.join("a.txt"), "da cópia\n").unwrap();
        git(&copia.caminho, &["commit", "-q", "-am", "copia"]).unwrap();
        commit_em(&projeto, "a.txt", "da origem\n");
        assert!(git(&projeto, &["merge", "--no-ff", "--no-edit", &copia.branch]).is_err());
        assert!(git(&projeto, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).is_ok());
        assert_eq!(recuperar_juncao(&projeto, &copia.caminho, &copia.branch).unwrap(), JuncaoCortada::NaoEntrou);
        assert!(git(&projeto, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).is_err(), "o merge parado saiu");
        assert_eq!(std::fs::read_to_string(projeto.join("a.txt")).unwrap(), "da origem\n");

        let outra = criar_worktree(&projeto, &projeto.with_extension("t7"), "vitral/t7-entrou").unwrap();
        commit_em(&outra.caminho, "novo.txt", "x\n");
        git(&projeto, &["merge", "--no-ff", "--no-edit", &outra.branch]).unwrap();
        assert_eq!(recuperar_juncao(&projeto, &outra.caminho, &outra.branch).unwrap(), JuncaoCortada::Entrou);
        // Com mudança sem commit na cópia, ficou coisa de fora.
        std::fs::write(outra.caminho.join("depois.txt"), "y").unwrap();
        assert_eq!(recuperar_juncao(&projeto, &outra.caminho, &outra.branch).unwrap(), JuncaoCortada::NaoEntrou);
        limpar(&projeto, &[&copia.caminho, &outra.caminho]);
    }

    #[test]
    fn limpeza_recusada_com_a_origem_fora_do_lugar() {
        let projeto = repositorio();
        let copia = criar_worktree(&projeto, &projeto.with_extension("t8"), "vitral/t8-longe").unwrap();
        let movido = projeto.with_extension("movido");
        std::fs::rename(&projeto, &movido).unwrap();
        let area = copia.caminho.parent().unwrap().to_path_buf();
        assert!(limpar_copia(&projeto, &copia.caminho, &area, &copia.branch, None, &[], false).is_err());
        assert!(copia.caminho.join("a.txt").exists(), "a cópia continua inteira");
        limpar(&movido, &[&copia.caminho]);
    }

    #[test]
    fn commit_de_turno_so_com_mudanca_e_poda_dos_pontos() {
        let projeto = repositorio();
        let commits = || git(&projeto, &["rev-list", "--count", "HEAD"]).unwrap();
        assert!(!commit_de_turno(&projeto, "Vitral: nada").unwrap());
        std::fs::create_dir_all(projeto.join("__pycache__")).unwrap();
        std::fs::write(projeto.join("__pycache__/x.pyc"), "lixo").unwrap();
        assert!(!commit_de_turno(&projeto, "Vitral: só lixo").unwrap());
        std::fs::write(projeto.join("b.txt"), "b\n").unwrap();
        assert!(commit_de_turno(&projeto, "Vitral: chat").unwrap());
        assert_eq!(commits(), "2");

        for i in 0..5 {
            std::fs::write(projeto.join("b.txt"), format!("{i}\n")).unwrap();
            foto(&projeto, Some("t3")).unwrap();
        }
        foto(&projeto, Some("t4")).unwrap();
        let grupo = |g: &str| {
            git(&projeto, &["for-each-ref", "--format=%(refname)", &format!("refs/vitral/pontos/{g}/")])
                .unwrap()
                .lines()
                .count()
        };
        assert_eq!(grupo("t3"), 5);
        podar_pontos(&projeto, "t3", 2);
        assert_eq!((grupo("t3"), grupo("t4")), (2, 1));
        limpar(&projeto, &[]);
    }

    #[test]
    fn ajudante_parte_do_trabalho_do_pai_e_junta_no_branch_dele() {
        let origem = repositorio();
        let pai = criar_worktree(&origem, &origem.with_extension("t1"), "vitral/t1-pai").unwrap();
        assert!(!checkpoint(&pai.caminho, "Vitral: nada").unwrap());
        std::fs::write(pai.caminho.join("do-pai.txt"), "pai\n").unwrap();
        // A cópia do ajudante nasce de outra cópia, depois do ponto: vê o que o pai ainda não juntou.
        let ajudante = na_vez_do_projeto(&pai.caminho, || {
            assert!(checkpoint(&pai.caminho, "Vitral: ponto antes do ajudante #2").unwrap());
            criar_worktree(&pai.caminho, &origem.with_extension("t2"), "vitral/t2-ajudante").unwrap()
        });
        assert_eq!(ajudante.base, git(&pai.caminho, &["rev-parse", "HEAD"]).unwrap());
        assert_eq!(ajudante.ramo.as_deref(), Some("vitral/t1-pai"));
        assert!(ajudante.caminho.join("do-pai.txt").exists() && !origem.join("do-pai.txt").exists());
        assert!(!ajudante.sujo);

        // O ajudante entrega; o pai mexeu de novo sem commit. Num único na_vez: o ponto e a junção.
        std::fs::write(ajudante.caminho.join("do-ajudante.txt"), "ajudante\n").unwrap();
        std::fs::write(pai.caminho.join("do-pai.txt"), "pai de novo\n").unwrap();
        let juncao = na_vez_do_projeto(&pai.caminho, || {
            assert!(checkpoint(&pai.caminho, "Vitral: ponto antes de juntar o ajudante #2").unwrap());
            aprovar(&pai.caminho, &ajudante.caminho, &ajudante.branch, &ajudante.base, "Ajudante #2").unwrap()
        });
        assert!(matches!(juncao, Juncao::Juntou { .. }));
        assert_eq!(ramo_atual(&pai.caminho).as_deref(), Some("vitral/t1-pai"));
        assert!(pai.caminho.join("do-ajudante.txt").exists());
        assert_eq!(std::fs::read_to_string(pai.caminho.join("do-pai.txt")).unwrap(), "pai de novo\n");
        assert!(!origem.join("do-ajudante.txt").exists(), "a origem só recebe no Juntar do pai");
        limpar_copia(&pai.caminho, &ajudante.caminho, &origem.with_extension("area"), &ajudante.branch, None, &[], false).unwrap();
        assert!(!ajudante.caminho.exists());

        // O Juntar do pai leva tudo à origem.
        let feita = na_vez_do_projeto(&origem, || {
            juntar(&origem, &pai.caminho, &pai.branch, &pai.base, "Chat 1", pai.ramo.as_deref(), "t1").unwrap()
        });
        assert!(matches!(feita, Juntada::Feita { commits: Some(_), .. }));
        assert!(origem.join("do-ajudante.txt").exists() && origem.join("do-pai.txt").exists());
        limpar(&origem, &[&pai.caminho]);
    }
}

#[cfg(test)]
mod testes_copia {
    use super::*;

    #[test]
    fn copia_arquivos_locais_que_batem_com_os_padroes() {
        let base = std::env::temp_dir().join(format!("vitral-copia-{}", crate::modelo::novo_id()));
        let (projeto, copia) = (base.join("p"), base.join("c"));
        std::fs::create_dir_all(&projeto).unwrap();
        std::fs::create_dir_all(&copia).unwrap();
        std::fs::write(projeto.join(".env"), "A=1").unwrap();
        std::fs::write(projeto.join(".env.local"), "B=2").unwrap();
        std::fs::write(projeto.join("outro.txt"), "x").unwrap();
        let mut copiados = copiar_arquivos_locais(&projeto, &copia, &[".env*".to_string()]);
        copiados.sort();
        assert_eq!(copiados, vec![".env".to_string(), ".env.local".to_string()]);
        assert!(!copia.join("outro.txt").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}

// ---------- PR e rede (integração com o GitHub) ----------
//
// O git de rede (fetch, push) roda pelo `remoto::Executor`, com limite de tempo, sem nenhuma
// pergunta e com as mensagens em inglês (`LC_ALL=C`, para reconhecer a recusa). Nada de rede
// roda dentro de `na_vez_do_projeto`, que fica para o que é local e curto; fetches e pushes de um
// mesmo repositório vão um de cada vez pela `na_vez_da_rede`. O push nunca usa `-u` (gravaria no
// `.git/config` que as cópias compartilham).

pub use pr_e_rede::*;

// Provisório: o motor passa a usar este bloco no G4.
#[allow(dead_code)]
mod pr_e_rede {
    use super::*;
    use crate::remoto::{self, ErroRemoto, Executor, SaidaBruta};

    const LIMITE_FETCH: Duration = Duration::from_secs(60);
    const LIMITE_PUSH: Duration = Duration::from_secs(120);
    const LIMITE_LOCAL: Duration = Duration::from_secs(120);

    fn ambiente_de_rede() -> remoto::Ambiente {
        let definir = [
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_SSH_COMMAND", "ssh -o BatchMode=yes -o ConnectTimeout=10"),
            ("GIT_HTTP_LOW_SPEED_LIMIT", "1000"),
            ("GIT_HTTP_LOW_SPEED_TIME", "30"),
            ("LC_ALL", "C"),
        ];
        remoto::Ambiente {
            limpar: false,
            manter: Vec::new(),
            definir: definir.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(),
            remover: vec!["SSH_ASKPASS"],
        }
    }

    fn git_rede(e: &dyn Executor, pasta: &Path, args: &[&str], limite: Duration) -> Result<SaidaBruta, ErroRemoto> {
        let todos: Vec<String> = ["-c", "core.quotePath=false"].iter().chain(args).map(|a| a.to_string()).collect();
        e.rodar("git", &todos, &ambiente_de_rede(), pasta, None, limite)
    }

    /// Um git local que precisa da saída com qualquer código (o `merge-tree` sai com 1 no conflito).
    fn git_bruto(pasta: &Path, args: &[&str]) -> Result<SaidaBruta> {
        let mut todos: Vec<String> = vec!["-c".into(), "core.quotePath=false".into()];
        if sem_identidade() {
            todos.extend(["-c", "user.name=Vitral", "-c", "user.email=vitral@localhost"].map(String::from));
        }
        todos.extend(args.iter().map(|a| a.to_string()));
        let ambiente = remoto::Ambiente {
            definir: vec![("GIT_TERMINAL_PROMPT".into(), "0".into()), ("GIT_OPTIONAL_LOCKS".into(), "0".into())],
            ..Default::default()
        };
        remoto::Real
            .rodar("git", &todos, &ambiente, pasta, None, LIMITE_LOCAL)
            .map_err(|erro| anyhow::anyhow!("git {}: {}", args.join(" "), erro.para_usuario("git")))
    }

    /// Nome de remote, ramo ou refspec que vai como argumento: nunca começa com "-".
    fn conferir_argumento(texto: &str) -> Result<(), ErroRemoto> {
        if texto.is_empty() || texto.starts_with('-') || texto.chars().any(char::is_whitespace) {
            return Err(ErroRemoto::Validacao(format!("nome inválido para o git: {texto:?}")));
        }
        Ok(())
    }

    /// A trava de rede do repositório (a chave é o `git-common-dir`, a mesma da origem e de todas as
    /// cópias). Sai do git: chame fora da thread da interface.
    pub fn trava_da_rede(repo: &Path) -> Arc<smol::lock::Mutex<()>> {
        type Travas = std::collections::HashMap<PathBuf, Arc<smol::lock::Mutex<()>>>;
        static TRAVAS: std::sync::OnceLock<Mutex<Travas>> = std::sync::OnceLock::new();
        let chave = raiz_comum(repo).unwrap_or_else(|| dunce::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf()));
        TRAVAS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).entry(chave).or_default().clone()
    }

    /// Espera a vez de rede do repositório: dois fetches juntos brigariam pelo lock das refs. A guarda
    /// vai para dentro da tarefa de fundo.
    pub async fn na_vez_da_rede(repo: &Path) -> smol::lock::MutexGuardArc<()> {
        trava_da_rede(repo).lock_arc().await
    }

    /// O commit de uma cópia: tudo menos o lixo e os arquivos copiados da origem (`excluir`, que
    /// nunca entram no commit, nem se um `git add` os tiver posto no índice). Diz se fez o commit.
    pub fn commitar(copia: &Path, mensagem: &str, excluir: &[String]) -> Result<bool> {
        commitar_com(copia, mensagem, excluir, false)
    }

    fn commitar_com(copia: &Path, mensagem: &str, excluir: &[String], sem_ganchos: bool) -> Result<bool> {
        let fora: Vec<String> = excluir.iter().map(|a| format!(":(exclude,literal){a}")).collect();
        let mut args = vec!["add", "-A", "--"];
        args.extend(SEM_LIXO);
        args.extend(fora.iter().map(String::as_str));
        git(copia, &args)?;
        if !excluir.is_empty() {
            let literais: Vec<String> = excluir.iter().map(|a| format!(":(literal){a}")).collect();
            let mut tirar = vec!["reset", "-q", "--"];
            tirar.extend(literais.iter().map(String::as_str));
            // Sem nenhum commit ainda o reset falha; aí o add com exclude já bastou.
            let _ = git(copia, &tirar);
        }
        if git(copia, &["diff", "--cached", "--name-only"])?.is_empty() {
            return Ok(false);
        }
        let mut commit = vec!["commit", "-q", "-m", mensagem];
        if sem_ganchos {
            commit.push("--no-verify");
        }
        git(copia, &commit)?;
        Ok(true)
    }

    /// Uma cópia nova que parte de `ponto` (`origin/main` no modo PR); sem ponto, do HEAD da origem,
    /// como `criar_worktree`. A base gravada é o sha do ponto, e o ramo nasce sem upstream.
    pub fn criar_worktree_de(projeto: &Path, destino: &Path, ramo: &str, ponto: Option<&str>) -> Result<CopiaIsolada> {
        let Some(ponto) = ponto else {
            return criar_worktree(projeto, destino, ramo);
        };
        let base = git(projeto, &["rev-parse", "--verify", "--quiet", &format!("{ponto}^{{commit}}")])
            .map_err(|_| anyhow::anyhow!("{ponto} não existe nesta máquina"))?;
        let ramo_da_origem = ramo_atual(projeto);
        let sujo = !git(projeto, &["status", "--porcelain", "--untracked-files=no"])?.is_empty();
        if let Some(pai) = destino.parent() {
            std::fs::create_dir_all(pai)?;
        }
        let _ = git(projeto, &["worktree", "prune"]);
        let nome = destino.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let livre = (1..=20)
            .map(|n| match n {
                1 => (destino.to_path_buf(), ramo.to_string()),
                n => (destino.with_file_name(format!("{nome}-{n}")), format!("{ramo}-{n}")),
            })
            .find(|(pasta, ramo)| {
                !pasta.exists() && git(projeto, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{ramo}")]).is_err()
            });
        let Some((destino, ramo)) = livre else {
            bail!("não há nome livre para a cópia em {}", destino.display());
        };
        // Pelo sha, e não pelo nome: partindo de um ramo remoto, o git gravaria o upstream no
        // .git/config compartilhado.
        git(projeto, &["worktree", "add", "-b", &ramo, &destino.to_string_lossy(), &base])?;
        Ok(CopiaIsolada { caminho: destino, branch: ramo, base, sujo, ramo: ramo_da_origem })
    }

    /// O `recriar_worktree` de uma cópia que parte de `ponto` (o modo PR, ao refazer depois de
    /// fechar o Vitral no meio): sem o ramo antigo, ele renasce do ponto, nunca do HEAD da origem.
    /// Com o ramo, ou sem ponto, é o `recriar_worktree` de sempre.
    pub fn recriar_worktree_de(origem: &Path, destino: &Path, branch: &str, ponto: Option<&str>) -> Result<(CopiaIsolada, bool)> {
        let existe = git(origem, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).is_ok();
        match ponto {
            Some(ponto) if !existe && !destino.exists() => criar_worktree_de(origem, destino, branch, Some(ponto)).map(|c| (c, false)),
            _ => recriar_worktree(origem, destino, branch),
        }
    }

    /// `fetch --no-tags <remoto> <refspecs…>` (rede).
    pub fn buscar(e: &dyn Executor, pasta: &Path, remoto: &str, refspecs: &[&str]) -> Result<(), ErroRemoto> {
        conferir_argumento(remoto)?;
        for refspec in refspecs {
            conferir_argumento(refspec)?;
        }
        let mut args = vec!["fetch", "--no-tags", "--quiet", remoto];
        args.extend(refspecs);
        let saida = git_rede(e, pasta, &args, LIMITE_FETCH)?;
        if saida.codigo == Some(0) {
            Ok(())
        } else {
            Err(remoto::classificar_git(saida.codigo, &saida.erros()))
        }
    }

    /// Commits da `base` local que ainda não estão no GitHub (a cópia do modo PR parte do GitHub).
    pub fn commits_locais_fora(projeto: &Path, remoto: &str, base: &str) -> u32 {
        git(projeto, &["rev-list", "--count", &format!("refs/remotes/{remoto}/{base}..refs/heads/{base}")])
            .ok()
            .and_then(|t| t.trim().parse().ok())
            .unwrap_or(0)
    }

    /// Com que comparar a cópia: no modo PR, o ponto comum com `<remoto>/<base>` (depois de trazer o
    /// main, o trabalho dos outros não entra no diff); senão, o `base_efetiva` de sempre.
    pub fn base_da_tarefa(projeto: &Path, copia: &Path, base: &str, remoto_base: Option<&str>) -> String {
        match remoto_base {
            Some(alvo) => git(copia, &["merge-base", alvo, "HEAD"]).unwrap_or_else(|_| base.to_string()),
            None => base_efetiva(projeto, copia, base),
        }
    }

    /// Os arquivos que dariam conflito ao juntar `alvo` no HEAD da cópia, sem mexer em nada
    /// (`merge-tree --write-tree`). Vazio = junta limpo.
    pub fn simular_mescla(copia: &Path, alvo: &str) -> Result<Vec<String>> {
        let saida = git_bruto(copia, &["merge-tree", "--write-tree", "--name-only", "-z", alvo, "HEAD"])?;
        match saida.codigo {
            Some(0) => Ok(Vec::new()),
            // A árvore, os arquivos em conflito (um por NUL) e, depois de um NUL a mais, as mensagens.
            Some(1) => {
                let texto = saida.texto();
                let arquivos = texto.split("\0\0").next().unwrap_or_default();
                Ok(arquivos.split('\0').skip(1).filter(|a| !a.is_empty()).map(str::to_string).collect())
            }
            _ => bail!("git merge-tree: {}", saida.erros().trim()),
        }
    }

    /// Junta `alvo` na cópia (`merge --no-edit`). Conflito: desfaz o merge e devolve os arquivos.
    pub fn trazer(copia: &Path, alvo: &str) -> Result<Juncao> {
        let antes = git(copia, &["rev-parse", "HEAD"])?;
        if let Err(erro) = git(copia, &["merge", "--no-edit", alvo]) {
            return falha_do_merge(copia, alvo, alvo.to_string(), erro);
        }
        let depois = git(copia, &["rev-parse", "HEAD"])?;
        if antes == depois {
            return Ok(Juncao::SemMudancas);
        }
        Ok(Juncao::Juntou { commits: git(copia, &["log", "--oneline", &format!("{antes}..{depois}")])? })
    }

    /// O resultado de mandar a cópia para o GitHub.
    #[derive(Clone, Debug, PartialEq)]
    pub enum Empurrao {
        /// Foi; o sha enviado.
        Enviado(String),
        /// O ramo no GitHub tem commits que a cópia não tem (não é fast-forward).
        Rejeitado(String),
        /// A proteção de push do GitHub achou um segredo (`GH013`); os lugares, sem o segredo.
        SegredoRecusado(String),
        Erro(ErroRemoto),
    }

    /// `push <remoto> <HEAD>:refs/heads/<ramo>` (rede). Vai o sha do HEAD de agora, o que foi
    /// verificado, e nunca com `-u`.
    pub fn empurrar(e: &dyn Executor, copia: &Path, remoto: &str, ramo: &str) -> Empurrao {
        if let Err(erro) = conferir_argumento(remoto).and(conferir_argumento(ramo)) {
            return Empurrao::Erro(erro);
        }
        let sha = match git(copia, &["rev-parse", "HEAD"]) {
            Ok(sha) => sha,
            Err(erro) => return Empurrao::Erro(ErroRemoto::Outro(erro.to_string())),
        };
        let destino = format!("{sha}:refs/heads/{ramo}");
        match git_rede(e, copia, &["push", "--porcelain", remoto, &destino], LIMITE_PUSH) {
            Ok(saida) => interpretar_push(&sha, &saida),
            Err(erro) => Empurrao::Erro(erro),
        }
    }

    fn interpretar_push(sha: &str, saida: &SaidaBruta) -> Empurrao {
        if saida.codigo == Some(0) {
            return Empurrao::Enviado(sha.to_string());
        }
        let (stdout, stderr) = (saida.texto(), saida.erros());
        let tudo = format!("{stdout}\n{stderr}");
        let regra = ["GH013", "GH009", "push declined due to repository rule violations"].iter().any(|t| tudo.contains(t));
        if regra && tudo.to_lowercase().contains("secret") {
            let lugares: Vec<&str> = stderr
                .lines()
                .filter_map(|l| l.trim_start_matches("remote:").trim().strip_prefix("path:"))
                .map(str::trim)
                .collect();
            let texto = if lugares.is_empty() { final_do_texto(stderr.trim(), 1500) } else { lugares.join(", ") };
            return Empurrao::SegredoRecusado(remoto::sem_segredos(&texto));
        }
        // No --porcelain, a recusa local é `!` com `[rejected]` (a do servidor é `[remote rejected]`).
        let rejeitado = stdout.lines().any(|l| l.starts_with('!') && l.contains("[rejected]"));
        if rejeitado {
            return Empurrao::Rejeitado(remoto::sem_segredos(&final_do_texto(tudo.trim(), 1500)));
        }
        if regra {
            return Empurrao::Erro(ErroRemoto::Validacao(remoto::sem_segredos(&final_do_texto(stderr.trim(), 300))));
        }
        Empurrao::Erro(remoto::classificar_git(saida.codigo, &stderr))
    }

    /// Um segredo que a cópia acrescentaria ao PR.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Achado {
        pub arquivo: String,
        /// A linha nova; `None` quando é o arquivo inteiro (um `.env`, uma chave).
        pub linha: Option<u32>,
        pub tipo: &'static str,
    }

    impl Achado {
        pub fn descricao(&self) -> String {
            match self.linha {
                Some(linha) => format!("linha {linha} de {} ({})", self.arquivo, self.tipo),
                None => format!("{} ({})", self.arquivo, self.tipo),
            }
        }
    }

    /// Arquivo que costuma guardar segredo: `.env*` (menos os de exemplo), `*.pem`, `*.p12`, `*.pfx`
    /// e chaves ssh privadas.
    fn arquivo_de_segredo(caminho: &str) -> bool {
        let nome = caminho.rsplit('/').next().unwrap_or(caminho);
        let exemplo = [".env.example", ".env.sample", ".env.template"].contains(&nome);
        let env = nome.starts_with(".env") && !exemplo;
        let certificado = [".pem", ".p12", ".pfx"].iter().any(|ext| nome.ends_with(ext));
        let chave_ssh = ["id_rsa", "id_ed25519", "id_ecdsa", "id_dsa"].iter().any(|p| nome.starts_with(p)) && !nome.ends_with(".pub");
        env || certificado || chave_ssh
    }

    /// O que parece segredo no que a cópia mudou desde a base: arquivos novos ou mudados com nome de
    /// segredo e linhas acrescentadas com token, `Authorization:` ou chave privada. Os arquivos
    /// copiados da origem (`excluir`) não entram no commit e ficam de fora.
    pub fn segredos_no_diff(copia: &Path, base: &str, excluir: &[String]) -> Vec<Achado> {
        let _ = adicionar(copia, "-N");
        let mut achados = Vec::new();
        let situacao = git(copia, &["diff", "--name-status", "--no-renames", "-z", base, "--", "."]).unwrap_or_default();
        for par in separar_nulos(&situacao).chunks_exact(2) {
            let (estado, arquivo) = (par[0].as_str(), par[1].as_str());
            if estado != "D" && arquivo_de_segredo(arquivo) && !excluir.iter().any(|x| x == arquivo) {
                achados.push(Achado { arquivo: arquivo.to_string(), linha: None, tipo: "arquivo de segredo" });
            }
        }
        let diff = git(copia, &["diff", "-U0", "--no-color", "--no-ext-diff", base, "--", "."]).unwrap_or_default();
        let (mut arquivo, mut linha, mut no_cabecalho) = (String::new(), 0u32, false);
        for texto in diff.lines() {
            if texto.starts_with("diff --git ") {
                no_cabecalho = true;
                arquivo.clear();
                continue;
            }
            if no_cabecalho {
                if let Some(caminho) = texto.strip_prefix("+++ ").and_then(|_| caminho_do_diff(texto)) {
                    arquivo = caminho;
                }
                if !texto.starts_with("@@") {
                    continue;
                }
                no_cabecalho = false;
            }
            if let Some(resto) = texto.strip_prefix("@@ ") {
                // "@@ -a,b +c,d @@": a primeira linha nova é c.
                linha = resto
                    .split_whitespace()
                    .find_map(|p| p.strip_prefix('+'))
                    .and_then(|p| p.split(',').next())
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
                continue;
            }
            if let Some(novo) = texto.strip_prefix('+') {
                if !arquivo.is_empty() && !excluir.iter().any(|x| *x == arquivo) {
                    for faixa in remoto::achar_segredos(novo) {
                        achados.push(Achado { arquivo: arquivo.clone(), linha: Some(linha), tipo: remoto::tipo_do_segredo(&novo[faixa]) });
                    }
                }
                linha += 1;
            }
        }
        achados
    }

    /// Algum caminho em `.github/`: o push dispararia workflows com os segredos do repositório.
    pub fn mexe_em_workflows(arquivos: &[String]) -> bool {
        arquivos.iter().any(|a| a.starts_with(".github/"))
    }

    /// Como ficou a cópia depois do PR mesclado.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Recomeco {
        /// A cópia está em `ramo`, a partir de `<remoto>/<base>` (o sha em `base`), com os commits
        /// feitos depois do PR por cima e as mudanças sem commit de volta.
        Feito { ramo: String, base: String, commits_levados: usize, voltaram_mudancas: bool },
        /// O PR foi mesclado com commits que esta cópia não tem: nada mudou.
        FaltamCommits,
        /// Os commits depois do PR não couberam em cima da base: a cópia ficou no ramo antigo, como
        /// estava.
        Conflito { arquivos: Vec<String> },
    }

    /// Depois do PR mesclado (por qualquer pessoa): traz a base e o head final do PR (rede, fora da
    /// trava) e, se a cópia tem tudo o que foi mesclado, passa ao `ramo_novo` a partir da base nova.
    /// Pede a vez do projeto: não chame dentro dela.
    #[allow(clippy::too_many_arguments)]
    pub fn recomecar_depois_do_pr(
        e: &dyn Executor,
        copia: &Path,
        remoto: &str,
        base: &str,
        numero: u64,
        head_final: &str,
        ramo_novo: &str,
        excluir: &[String],
        grupo: &str,
    ) -> Result<Recomeco> {
        let do_pr = format!("refs/pull/{numero}/head");
        let trouxe = match buscar(e, copia, remoto, &[base, &do_pr]) {
            // Fora do GitHub (ou sem o ref do PR) vale só a base.
            Err(ErroRemoto::NaoEncontrado(_)) => buscar(e, copia, remoto, &[base]),
            outro => outro,
        };
        trouxe.map_err(|erro| anyhow::anyhow!("{}", erro.para_usuario("GitHub")))?;
        if git(copia, &["merge-base", "--is-ancestor", head_final, "HEAD"]).is_err() {
            return Ok(Recomeco::FaltamCommits);
        }
        recomecar_da_base(copia, remoto, base, head_final, ramo_novo, excluir, grupo)
    }

    /// O recomeço em si, sem rede e sem stash (o `refs/stash` é de todas as cópias): guarda o HEAD em
    /// `refs/vitral/antes-do-recomeco/<grupo>`, põe as mudanças sem commit num commit temporário, leva
    /// o que veio depois de `head_final` para cima de `<remoto>/<base>` e devolve as mudanças. Também é
    /// o "Recomeçar a partir de origin/main" do usuário.
    pub fn recomecar_da_base(
        copia: &Path,
        remoto: &str,
        base: &str,
        head_final: &str,
        ramo_novo: &str,
        excluir: &[String],
        grupo: &str,
    ) -> Result<Recomeco> {
        na_vez_do_projeto(copia, || {
            let alvo = git(copia, &["rev-parse", "--verify", &format!("refs/remotes/{remoto}/{base}^{{commit}}")])?;
            if git(copia, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{ramo_novo}")]).is_ok() {
                bail!("o ramo {ramo_novo} já existe");
            }
            let antigo = ramo_atual(copia);
            git(copia, &["update-ref", &format!("refs/vitral/antes-do-recomeco/{grupo}"), "HEAD"])?;
            let temporario = suja(copia)? && commitar_com(copia, "vitral: mudanças em andamento", excluir, true)?;
            let depois_do_pr = git(copia, &["rev-list", &format!("{head_final}..HEAD")])?.lines().count();
            let desfazer_temporario = || -> Result<()> {
                if temporario {
                    git(copia, &["reset", "-q", "--mixed", "HEAD~1"])?;
                }
                Ok(())
            };
            if depois_do_pr > 0 {
                if let Err(erro) = git(copia, &["rebase", "--onto", &alvo, head_final]) {
                    let arquivos: Vec<String> = git(copia, &["diff", "--name-only", "--diff-filter=U"])
                        .unwrap_or_default()
                        .lines()
                        .map(str::to_string)
                        .collect();
                    let _ = git(copia, &["rebase", "--abort"]);
                    desfazer_temporario()?;
                    if arquivos.is_empty() {
                        return Err(erro);
                    }
                    return Ok(Recomeco::Conflito { arquivos });
                }
                match &antigo {
                    Some(_) => git(copia, &["branch", "-m", ramo_novo])?,
                    None => git(copia, &["checkout", "-q", "-b", ramo_novo])?,
                };
            } else {
                git(copia, &["checkout", "-q", "-B", ramo_novo, &alvo])?;
                // Seguro: o histórico está no PR e no ref de antes do recomeço.
                if let Some(antigo) = antigo.as_deref().filter(|a| *a != ramo_novo) {
                    let _ = git(copia, &["branch", "-D", antigo]);
                }
            }
            desfazer_temporario()?;
            Ok(Recomeco::Feito {
                ramo: ramo_novo.to_string(),
                base: alvo,
                commits_levados: depois_do_pr - usize::from(temporario),
                voltaram_mudancas: temporario,
            })
        })
    }

    /// Apaga o ramo do PR no GitHub, só se ele ainda está em `sha_esperado` (`--force-with-lease`).
    /// Um ramo que já não existe conta como apagado.
    pub fn apagar_ramo_remoto(e: &dyn Executor, pasta: &Path, remoto: &str, ramo: &str, sha_esperado: &str) -> Result<(), ErroRemoto> {
        conferir_argumento(remoto)?;
        conferir_argumento(ramo)?;
        conferir_argumento(sha_esperado)?;
        let lease = format!("--force-with-lease=refs/heads/{ramo}:{sha_esperado}");
        let apagar = format!(":refs/heads/{ramo}");
        let saida = git_rede(e, pasta, &["push", "--porcelain", &lease, remoto, &apagar], LIMITE_PUSH)?;
        let tudo = format!("{}\n{}", saida.texto(), saida.erros());
        if saida.codigo == Some(0) || tudo.contains("remote ref does not exist") {
            return Ok(());
        }
        if tudo.contains("stale info") || tudo.contains("[rejected]") {
            // O lease também recusa quando o ramo já não existe (o GitHub apagou ao mesclar).
            let ref_remoto = format!("refs/heads/{ramo}");
            let conferido = git_rede(e, pasta, &["ls-remote", "--exit-code", remoto, &ref_remoto], LIMITE_FETCH)?;
            if conferido.codigo == Some(2) {
                return Ok(());
            }
            return Err(ErroRemoto::Conflito(format!("o ramo {ramo} mudou no GitHub; não foi apagado")));
        }
        Err(remoto::classificar_git(saida.codigo, &saida.erros()))
    }

    #[cfg(test)]
    mod testes_pr {
        use super::*;
        use crate::remoto::{Real, falso};

        /// Uma origem bare local fazendo de GitHub, o projeto (clone dela) e um clone "site" para as
        /// mudanças de outras pessoas.
        struct Bancada {
            raiz: PathBuf,
            origem: PathBuf,
            projeto: PathBuf,
            site: PathBuf,
        }

        impl Bancada {
            fn nova() -> Self {
                let raiz = std::env::temp_dir().join(format!("vitral-pr-{}", crate::modelo::novo_id()));
                let (origem, projeto, site) = (raiz.join("origem.git"), raiz.join("projeto"), raiz.join("site"));
                std::fs::create_dir_all(&projeto).unwrap();
                git(&projeto, &["init", "-q", "-b", "main"]).unwrap();
                std::fs::write(projeto.join("a.txt"), "um\n").unwrap();
                git(&projeto, &["add", "-A"]).unwrap();
                git(&projeto, &["commit", "-q", "-m", "inicio"]).unwrap();
                let texto = |p: &Path| p.to_string_lossy().to_string();
                git(&raiz, &["clone", "-q", "--bare", &texto(&projeto), &texto(&origem)]).unwrap();
                git(&projeto, &["remote", "add", "origin", &texto(&origem)]).unwrap();
                git(&projeto, &["fetch", "-q", "origin"]).unwrap();
                Self { raiz, origem, projeto, site }
            }

            fn nova_com_commit_local() -> Self {
                let b = Self::nova();
                commit(&b.projeto, "local.txt", "só aqui\n");
                b
            }

            fn copia(&self, nome: &str, ramo: &str) -> CopiaIsolada {
                criar_worktree_de(&self.projeto, &self.raiz.join(nome), ramo, Some("origin/main")).unwrap()
            }

            /// Um commit de outra pessoa, mandado direto para o ramo do GitHub.
            fn commit_no_site(&self, ramo: &str, arquivo: &str, conteudo: &str) -> String {
                let texto = |p: &Path| p.to_string_lossy().to_string();
                if !self.site.exists() {
                    git(&self.raiz, &["clone", "-q", &texto(&self.origem), &texto(&self.site)]).unwrap();
                }
                git(&self.site, &["fetch", "-q", "origin"]).unwrap();
                git(&self.site, &["checkout", "-q", "-B", ramo, &format!("origin/{ramo}")]).unwrap();
                std::fs::write(self.site.join(arquivo), conteudo).unwrap();
                git(&self.site, &["add", "-A"]).unwrap();
                git(&self.site, &["commit", "-q", "-m", &format!("do site: {arquivo}")]).unwrap();
                git(&self.site, &["push", "-q", "origin", &format!("HEAD:refs/heads/{ramo}")]).unwrap();
                git(&self.site, &["rev-parse", "HEAD"]).unwrap()
            }

            /// O "Squash and merge" do GitHub: um commit só no main com o conteúdo do PR, e o ref do PR.
            fn mesclar_por_squash(&self, numero: u64, ramo: &str) -> String {
                let texto = |p: &Path| p.to_string_lossy().to_string();
                if !self.site.exists() {
                    git(&self.raiz, &["clone", "-q", &texto(&self.origem), &texto(&self.site)]).unwrap();
                }
                git(&self.site, &["fetch", "-q", "origin"]).unwrap();
                let head = git(&self.site, &["rev-parse", &format!("origin/{ramo}")]).unwrap();
                git(&self.origem, &["update-ref", &format!("refs/pull/{numero}/head"), &head]).unwrap();
                git(&self.site, &["checkout", "-q", "-B", "main", "origin/main"]).unwrap();
                git(&self.site, &["merge", "-q", "--squash", &format!("origin/{ramo}")]).unwrap();
                git(&self.site, &["commit", "-q", "-m", &format!("PR (#{numero})")]).unwrap();
                git(&self.site, &["push", "-q", "origin", "main"]).unwrap();
                head
            }
        }

        impl Drop for Bancada {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.raiz);
            }
        }

        fn commit(pasta: &Path, arquivo: &str, conteudo: &str) {
            std::fs::write(pasta.join(arquivo), conteudo).unwrap();
            assert!(commitar(pasta, &format!("muda {arquivo}"), &[]).unwrap());
        }

        fn existe_ramo(pasta: &Path, ramo: &str) -> bool {
            git(pasta, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{ramo}")]).is_ok()
        }

        #[test]
        fn copia_do_modo_pr_parte_do_github_sem_upstream() {
            let b = Bancada::nova_com_commit_local();
            let copia = b.copia("c1", "vitral/t1-x");
            assert_eq!(copia.base, git(&b.projeto, &["rev-parse", "origin/main"]).unwrap());
            assert_ne!(copia.base, git(&b.projeto, &["rev-parse", "HEAD"]).unwrap());
            assert!(!copia.caminho.join("local.txt").exists());
            assert!(git(&b.projeto, &["config", "--get", "branch.vitral/t1-x.remote"]).is_err());
            assert_eq!(commits_locais_fora(&b.projeto, "origin", "main"), 1);
            // Sem ponto é o criar_worktree de sempre (HEAD local).
            let local = criar_worktree_de(&b.projeto, &b.raiz.join("c2"), "vitral/t2-y", None).unwrap();
            assert!(local.caminho.join("local.txt").exists());
            assert!(criar_worktree_de(&b.projeto, &b.raiz.join("c3"), "vitral/t3-z", Some("origin/nao-existe")).is_err());
        }

        #[test]
        fn refazer_a_copia_do_modo_pr_nao_parte_do_head_local() {
            let b = Bancada::nova_com_commit_local();
            let destino = b.raiz.join("c1");
            // A cópia não chegou a nascer (o Vitral fechou antes): nasce do ponto gravado.
            let (copia, existia) = recriar_worktree_de(&b.projeto, &destino, "feature/vitor-eng-5-x", Some("origin/main")).unwrap();
            assert!(!existia && !copia.caminho.join("local.txt").exists());
            assert_eq!(copia.base, git(&b.projeto, &["rev-parse", "origin/main"]).unwrap());
            // Com o ramo e a pasta já lá, é a mesma cópia (o `--resume` vale pela pasta).
            commit(&copia.caminho, "feito.txt", "feito\n");
            let (de_novo, existia) = recriar_worktree_de(&b.projeto, &destino, "feature/vitor-eng-5-x", Some("origin/main")).unwrap();
            assert!(existia && de_novo.caminho.join("feito.txt").exists());
            assert_eq!(de_novo.branch, "feature/vitor-eng-5-x");
        }

        #[test]
        fn empurrar_envia_recusa_fora_de_ordem_e_reconhece_o_segredo() {
            let b = Bancada::nova();
            let copia = b.copia("c1", "vitral/t1-x");
            commit(&copia.caminho, "feat.txt", "novo\n");
            let sha = git(&copia.caminho, &["rev-parse", "HEAD"]).unwrap();
            assert_eq!(empurrar(&Real, &copia.caminho, "origin", "vitral/t1-x"), Empurrao::Enviado(sha.clone()));
            assert_eq!(git(&b.origem, &["rev-parse", "refs/heads/vitral/t1-x"]).unwrap(), sha);
            assert!(git(&b.projeto, &["config", "--get", "branch.vitral/t1-x.remote"]).is_err());
            // Alguém mandou um commit no ramo pelo site: o próximo push é recusado, nada é trazido.
            b.commit_no_site("vitral/t1-x", "site.txt", "do site\n");
            commit(&copia.caminho, "feat.txt", "mais\n");
            assert!(matches!(empurrar(&Real, &copia.caminho, "origin", "vitral/t1-x"), Empurrao::Rejeitado(_)));
            assert!(!copia.caminho.join("site.txt").exists());
            // A proteção de push do GitHub (saída gravada de um GH013).
            let gh013 = "remote: error: GH013: Repository rule violations found for refs/heads/vitral/t1-x.\n\
                         remote: \n\
                         remote: - GITHUB PUSH PROTECTION\n\
                         remote:   —————————————————————————————————————————\n\
                         remote:     Resolve the following violations before pushing again\n\
                         remote: \n\
                         remote:     - Push cannot contain secrets\n\
                         remote: \n\
                         remote:       —— GitHub Personal Access Token ——————————————————————\n\
                         remote:        locations:\n\
                         remote:          - commit: 8728dbe67f6a8e6a3bfb1f2f6a2e2f8b1d8f7e1a\n\
                         remote:            path: config.rs:12\n\
                         To https://github.com/o/r.git\n";
            let saida = "To https://github.com/o/r.git\n!\tHEAD:refs/heads/vitral/t1-x\t[remote rejected] (push declined due to repository rule violations)\nDone\n";
            let e = falso::Falso::com(vec![falso::saida(1, saida, gh013)]);
            assert_eq!(empurrar(&e, &copia.caminho, "origin", "vitral/t1-x"), Empurrao::SegredoRecusado("config.rs:12".into()));
            let chamada = &e.chamadas()[0];
            assert_eq!(chamada.programa, "git");
            assert!(chamada.args.contains(&"--porcelain".to_string()) && !chamada.args.contains(&"-u".to_string()));
            assert!(chamada.ambiente.definir.contains(&("GIT_SSH_COMMAND".into(), "ssh -o BatchMode=yes -o ConnectTimeout=10".into())));
            assert!(chamada.ambiente.remover.contains(&"SSH_ASKPASS"));
            assert_eq!(chamada.limite, LIMITE_PUSH);
            assert!(matches!(empurrar(&e, &copia.caminho, "--upload-pack=x", "r"), Empurrao::Erro(ErroRemoto::Validacao(_))));
        }

        #[test]
        fn trazer_com_conflito_aborta_e_simular_nao_mexe() {
            let b = Bancada::nova();
            let copia = b.copia("c1", "vitral/t1-x");
            commit(&copia.caminho, "b.txt", "da cópia\n");
            // O main andou num arquivo que a cópia não mexeu: junta limpo.
            b.commit_no_site("main", "c.txt", "do main\n");
            buscar(&Real, &copia.caminho, "origin", &["main"]).unwrap();
            assert_eq!(simular_mescla(&copia.caminho, "origin/main").unwrap(), Vec::<String>::new());
            // Agora os dois mexem no a.txt.
            commit(&copia.caminho, "a.txt", "da cópia\n");
            b.commit_no_site("main", "a.txt", "do main\n");
            buscar(&Real, &copia.caminho, "origin", &["main"]).unwrap();
            let antes = git(&copia.caminho, &["rev-parse", "HEAD"]).unwrap();
            assert_eq!(simular_mescla(&copia.caminho, "origin/main").unwrap(), ["a.txt"]);
            assert_eq!(git(&copia.caminho, &["rev-parse", "HEAD"]).unwrap(), antes);
            assert!(git(&copia.caminho, &["status", "--porcelain"]).unwrap().is_empty());
            match trazer(&copia.caminho, "origin/main").unwrap() {
                Juncao::Conflito { arquivos, alvo, .. } => {
                    assert_eq!(arquivos, ["a.txt"]);
                    assert_eq!(alvo, "origin/main");
                }
                _ => panic!("devia dar conflito"),
            }
            assert!(git(&copia.caminho, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).is_err());
            assert_eq!(git(&copia.caminho, &["rev-parse", "HEAD"]).unwrap(), antes);
            assert_eq!(std::fs::read_to_string(copia.caminho.join("a.txt")).unwrap(), "da cópia\n");
            // Sem conflito, traz; de novo, não há o que trazer.
            let outra = b.copia("c2", "vitral/t2-y");
            commit(&outra.caminho, "e.txt", "da outra\n");
            b.commit_no_site("main", "d.txt", "mais\n");
            buscar(&Real, &outra.caminho, "origin", &["main"]).unwrap();
            assert!(matches!(trazer(&outra.caminho, "origin/main").unwrap(), Juncao::Juntou { .. }));
            assert!(matches!(trazer(&outra.caminho, "origin/main").unwrap(), Juncao::SemMudancas));
            // A base da tarefa no modo PR é o ponto comum com origin/main: o d.txt dos outros, que
            // veio com o merge, não entra no diff (com a base antiga, entraria).
            let base = base_da_tarefa(&b.projeto, &outra.caminho, &outra.base, Some("origin/main"));
            assert_eq!(arquivos_mudados(&outra.caminho, &base), ["e.txt"]);
            assert!(arquivos_mudados(&outra.caminho, &outra.base).contains(&"d.txt".to_string()));
        }

        #[test]
        fn commitar_deixa_de_fora_os_arquivos_copiados() {
            let b = Bancada::nova();
            let copia = b.copia("c1", "vitral/t1-x");
            let pasta = &copia.caminho;
            std::fs::write(pasta.join("segredo.local"), "SENHA=1\n").unwrap();
            std::fs::write(pasta.join("outro copiado.cfg"), "x\n").unwrap();
            std::fs::write(pasta.join("x.txt"), "x\n").unwrap();
            // Um `git add` do agente pôs um deles no índice.
            git(pasta, &["add", "segredo.local"]).unwrap();
            let excluir = ["segredo.local".to_string(), "outro copiado.cfg".to_string()];
            assert!(commitar(pasta, "tarefa", &excluir).unwrap());
            let no_commit = git(pasta, &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]).unwrap();
            assert_eq!(no_commit, "x.txt");
            assert!(pasta.join("segredo.local").exists());
            assert!(!commitar(pasta, "nada", &excluir).unwrap());
        }

        #[test]
        fn segredos_no_diff_acha_arquivos_e_linhas() {
            let b = Bancada::nova();
            let copia = b.copia("c1", "vitral/t1-x");
            let pasta = &copia.caminho;
            let token = concat!("gh", "p_0123456789abcdefghijABCDEFGHIJ012345");
            for (nome, conteudo) in [
                (".env.local", "A=1\n".to_string()),
                (".env.example", "A=\n".to_string()),
                ("id_rsa", "x\n".to_string()),
                ("id_rsa.pub", "ssh-rsa AAAA\n".to_string()),
                ("certs/servidor.pem", "x\n".to_string()),
                ("config.rs", format!("fn a() {{}}\n// nada\nconst T: &str = \"{token}\";\n")),
                ("copiado.env", "B=2\n".to_string()),
                (".env", format!("TOKEN={token}\n")),
            ] {
                std::fs::create_dir_all(pasta.join(nome).parent().unwrap()).unwrap();
                std::fs::write(pasta.join(nome), conteudo).unwrap();
            }
            // Um arquivo que já existia, com uma linha nova no meio.
            std::fs::write(pasta.join("a.txt"), "um\nAuthorization: Bearer xyz\n").unwrap();
            let achados = segredos_no_diff(pasta, &copia.base, &[".env".to_string()]);
            let descricoes: Vec<String> = achados.iter().map(Achado::descricao).collect();
            assert_eq!(
                descricoes,
                [
                    ".env.local (arquivo de segredo)",
                    "certs/servidor.pem (arquivo de segredo)",
                    "id_rsa (arquivo de segredo)",
                    "linha 2 de a.txt (cabeçalho Authorization)",
                    "linha 3 de config.rs (token do GitHub)",
                ]
            );
            assert!(mexe_em_workflows(&[".github/workflows/ci.yml".into()]));
            assert!(!mexe_em_workflows(&["src/github.rs".into(), "docs/.github.md".into()]));
        }

        #[test]
        fn apagar_ramo_remoto_so_com_o_sha_esperado() {
            let b = Bancada::nova();
            let copia = b.copia("c1", "vitral/t1-x");
            commit(&copia.caminho, "feat.txt", "novo\n");
            let Empurrao::Enviado(sha) = empurrar(&Real, &copia.caminho, "origin", "vitral/t1-x") else { panic!("não enviou") };
            let errado = git(&copia.caminho, &["rev-parse", "HEAD~1"]).unwrap();
            assert!(matches!(apagar_ramo_remoto(&Real, &copia.caminho, "origin", "vitral/t1-x", &errado), Err(ErroRemoto::Conflito(_))));
            assert!(existe_ramo(&b.origem, "vitral/t1-x"));
            assert_eq!(apagar_ramo_remoto(&Real, &copia.caminho, "origin", "vitral/t1-x", &sha), Ok(()));
            assert!(!existe_ramo(&b.origem, "vitral/t1-x"));
            // Já apagado (pelo GitHub, por exemplo) conta como apagado.
            assert_eq!(apagar_ramo_remoto(&Real, &copia.caminho, "origin", "vitral/t1-x", &sha), Ok(()));
        }

        /// Uma cópia com um PR enviado e mesclado por squash no GitHub: devolve a cópia e o head final.
        fn pr_mesclado(b: &Bancada) -> (CopiaIsolada, String) {
            let copia = b.copia("c1", "vitral/t1-x");
            commit(&copia.caminho, "feat.txt", "novo\n");
            commit(&copia.caminho, "feat.txt", "novo e melhor\n");
            assert!(matches!(empurrar(&Real, &copia.caminho, "origin", "vitral/t1-x"), Empurrao::Enviado(_)));
            let head = b.mesclar_por_squash(1, "vitral/t1-x");
            (copia, head)
        }

        fn recomecar(copia: &Path, head: &str) -> Recomeco {
            recomecar_depois_do_pr(&Real, copia, "origin", "main", 1, head, "vitral/t1-x-r2", &[], "t1").unwrap()
        }

        #[test]
        fn recomeco_limpo_vai_para_o_ramo_novo_sem_os_commits_antigos() {
            let b = Bancada::nova();
            let (copia, head) = pr_mesclado(&b);
            let pasta = &copia.caminho;
            let Recomeco::Feito { ramo, base, commits_levados, voltaram_mudancas } = recomecar(pasta, &head) else {
                panic!("devia recomeçar")
            };
            assert_eq!((ramo.as_str(), commits_levados, voltaram_mudancas), ("vitral/t1-x-r2", 0, false));
            assert_eq!(ramo_atual(pasta).as_deref(), Some("vitral/t1-x-r2"));
            assert_eq!(git(pasta, &["rev-parse", "HEAD"]).unwrap(), base);
            assert_eq!(base, git(pasta, &["rev-parse", "origin/main"]).unwrap());
            assert!(git(pasta, &["merge-base", "--is-ancestor", &head, "HEAD"]).is_err(), "os commits do PR voltaram");
            assert_eq!(std::fs::read_to_string(pasta.join("feat.txt")).unwrap(), "novo e melhor\n");
            assert!(!existe_ramo(&b.projeto, "vitral/t1-x"));
            assert_eq!(git(pasta, &["rev-parse", "refs/vitral/antes-do-recomeco/t1"]).unwrap(), head);
        }

        #[test]
        fn recomeco_leva_os_commits_novos_e_as_mudancas_sem_stash() {
            let b = Bancada::nova();
            let (copia, head) = pr_mesclado(&b);
            let pasta = &copia.caminho;
            // Outra cópia do mesmo repositório com um stash: o refs/stash é de todas.
            let vizinha = b.copia("c9", "vitral/t9-v");
            std::fs::write(vizinha.caminho.join("a.txt"), "na vizinha\n").unwrap();
            git(&vizinha.caminho, &["stash", "-q"]).unwrap();
            commit(pasta, "depois.txt", "depois do PR\n");
            let antes = git(pasta, &["rev-parse", "HEAD"]).unwrap();
            std::fs::write(pasta.join("a.txt"), "mudança sem commit\n").unwrap();
            std::fs::write(pasta.join("solto.txt"), "novo sem add\n").unwrap();
            let Recomeco::Feito { commits_levados, voltaram_mudancas, .. } = recomecar(pasta, &head) else {
                panic!("devia recomeçar")
            };
            assert_eq!((commits_levados, voltaram_mudancas), (1, true));
            let novos = git(pasta, &["log", "--format=%s", "origin/main..HEAD"]).unwrap();
            assert_eq!(novos, "muda depois.txt");
            assert!(git(pasta, &["merge-base", "--is-ancestor", &head, "HEAD"]).is_err());
            assert_eq!(std::fs::read_to_string(pasta.join("a.txt")).unwrap(), "mudança sem commit\n");
            let situacao = git(pasta, &["status", "--porcelain"]).unwrap();
            assert!(situacao.contains(" M a.txt") && situacao.contains("?? solto.txt"), "{situacao}");
            assert_eq!(git(pasta, &["rev-parse", "refs/vitral/antes-do-recomeco/t1"]).unwrap(), antes);
            // O stash da vizinha continua lá e volta inteiro.
            assert_eq!(git(&vizinha.caminho, &["stash", "list"]).unwrap().lines().count(), 1);
            git(&vizinha.caminho, &["stash", "pop", "-q"]).unwrap();
            assert_eq!(std::fs::read_to_string(vizinha.caminho.join("a.txt")).unwrap(), "na vizinha\n");
        }

        #[test]
        fn recomeco_com_conflito_desiste_e_mantem_o_ramo_antigo() {
            let b = Bancada::nova();
            let (copia, head) = pr_mesclado(&b);
            let pasta = &copia.caminho;
            // Depois do PR, a cópia e o main mexeram no a.txt.
            commit(pasta, "a.txt", "da cópia\n");
            b.commit_no_site("main", "a.txt", "do main\n");
            std::fs::write(pasta.join("c.txt"), "sem commit\n").unwrap();
            let antes = git(pasta, &["rev-parse", "HEAD"]).unwrap();
            assert_eq!(recomecar(pasta, &head), Recomeco::Conflito { arquivos: vec!["a.txt".into()] });
            assert_eq!(ramo_atual(pasta).as_deref(), Some("vitral/t1-x"));
            assert_eq!(git(pasta, &["rev-parse", "HEAD"]).unwrap(), antes);
            assert_eq!(std::fs::read_to_string(pasta.join("c.txt")).unwrap(), "sem commit\n");
            assert_eq!(git(pasta, &["status", "--porcelain"]).unwrap(), "?? c.txt");
            assert!(!existe_ramo(pasta, "vitral/t1-x-r2"));
            assert_eq!(git(pasta, &["rev-parse", "refs/vitral/antes-do-recomeco/t1"]).unwrap(), antes);
        }

        #[test]
        fn recomeco_sem_os_commits_de_fora_nao_mexe() {
            let b = Bancada::nova();
            let copia = b.copia("c1", "vitral/t1-x");
            commit(&copia.caminho, "feat.txt", "novo\n");
            assert!(matches!(empurrar(&Real, &copia.caminho, "origin", "vitral/t1-x"), Empurrao::Enviado(_)));
            // Alguém completou o PR pelo site antes de mesclar.
            b.commit_no_site("vitral/t1-x", "site.txt", "do site\n");
            let head = b.mesclar_por_squash(1, "vitral/t1-x");
            let antes = git(&copia.caminho, &["rev-parse", "HEAD"]).unwrap();
            assert_eq!(recomecar(&copia.caminho, &head), Recomeco::FaltamCommits);
            assert_eq!(git(&copia.caminho, &["rev-parse", "HEAD"]).unwrap(), antes);
            assert_eq!(ramo_atual(&copia.caminho).as_deref(), Some("vitral/t1-x"));
            // "Recomeçar a partir de origin/main", pelo usuário: o mesmo caminho a partir da base.
            let Recomeco::Feito { ramo, .. } =
                recomecar_da_base(&copia.caminho, "origin", "main", &head, "vitral/t1-x-r2", &[], "t1").unwrap()
            else {
                panic!("devia recomeçar")
            };
            assert_eq!(ramo, "vitral/t1-x-r2");
            assert!(copia.caminho.join("site.txt").exists());
        }

        #[test]
        fn a_vez_da_rede_e_por_repositorio() {
            let b = Bancada::nova();
            let copia = b.copia("c1", "vitral/t1-x");
            let outra = Bancada::nova();
            smol::block_on(async {
                let _vez = na_vez_da_rede(&b.projeto).await;
                // A cópia é do mesmo repositório: espera.
                assert!(trava_da_rede(&copia.caminho).try_lock().is_none());
                assert!(trava_da_rede(&outra.projeto).try_lock().is_some());
            });
            assert!(trava_da_rede(&copia.caminho).try_lock().is_some());
        }

        #[test]
        fn buscar_vai_pelo_executor_com_limite() {
            let e = falso::Falso::com(vec![
                falso::saida(0, "", ""),
                falso::saida(128, "", "fatal: unable to access 'https://github.com/o/r/': Could not resolve host: github.com"),
            ]);
            let pasta = Path::new("/");
            assert_eq!(buscar(&e, pasta, "origin", &["main", "refs/pull/3/head"]), Ok(()));
            assert!(matches!(buscar(&e, pasta, "origin", &["main"]), Err(ErroRemoto::SemRede(_))));
            let chamada = &e.chamadas()[0];
            assert_eq!(chamada.args, ["-c", "core.quotePath=false", "fetch", "--no-tags", "--quiet", "origin", "main", "refs/pull/3/head"]);
            assert_eq!(chamada.limite, LIMITE_FETCH);
            assert!(chamada.ambiente.definir.contains(&("GIT_HTTP_LOW_SPEED_TIME".into(), "30".into())));
            assert!(matches!(buscar(&e, pasta, "origin", &["--upload-pack=x"]), Err(ErroRemoto::Validacao(_))));
            assert_eq!(e.chamadas().len(), 2);
        }
    }
}
