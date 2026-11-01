//! Operações de git das tarefas: cópia isolada (worktree), diff, commit e junção.
//! Todas bloqueiam: rode fora da thread da interface.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Result, bail};

use crate::{
    modelo::Verificacao,
    processos::{self, Escopo},
};

/// Sem user.name/user.email no git, os commits do Vitral saem com uma identidade própria.
fn sem_identidade() -> bool {
    static SEM: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SEM.get_or_init(|| {
        let valor = |chave: &str| {
            Command::new("git")
                .args(["config", chave])
                .output()
                .map(|s| !String::from_utf8_lossy(&s.stdout).trim().is_empty())
                .unwrap_or(false)
        };
        !(valor("user.name") && valor("user.email"))
    })
}

fn git(pasta: &Path, args: &[&str]) -> Result<String> {
    let mut comando = Command::new("git");
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
    let mesmo = |a: &Path, b: &Path| a.canonicalize().ok() == b.canonicalize().ok();
    mesmo(Path::new(&topo), projeto) && git(projeto, &["rev-parse", "HEAD"]).is_ok()
}

/// Erro de um git que achou o índice preso por outro (o caminho sai igual em qualquer idioma).
fn ocupado(erro: &str) -> bool {
    erro.contains("index.lock'")
}

/// Roda `f` na vez do projeto: as junções e as cópias novas do Vitral num mesmo projeto vão uma
/// de cada vez (dois git juntos disputam o index.lock e leem um HEAD que já ficou velho).
pub fn na_vez_do_projeto<T>(projeto: &Path, f: impl FnOnce() -> T) -> T {
    type Travas = std::collections::HashMap<PathBuf, std::sync::Arc<std::sync::Mutex<()>>>;
    static TRAVAS: std::sync::OnceLock<std::sync::Mutex<Travas>> = std::sync::OnceLock::new();
    let chave = projeto.canonicalize().unwrap_or_else(|_| projeto.to_path_buf());
    let trava = TRAVAS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(chave)
        .or_default()
        .clone();
    let _vez = trava.lock().unwrap_or_else(|e| e.into_inner());
    f()
}

pub struct CopiaIsolada {
    pub caminho: PathBuf,
    pub branch: String,
    pub base: String,
    /// O projeto tinha mudanças sem commit, que a cópia não vê.
    pub sujo: bool,
}

pub fn criar_worktree(projeto: &Path, destino: &Path, branch: &str) -> Result<CopiaIsolada> {
    let base = git(projeto, &["rev-parse", "HEAD"])?;
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
    })
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
            bail!("{arquivo} não está na cópia da tarefa");
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
    adicionar(copia, "-A")?;
    if !git(copia, &["diff", "--cached", "--name-only"])?.is_empty() {
        git(copia, &["commit", "-m", mensagem])?;
    }
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

/// Espaço que a pasta ocupa no disco, sem seguir links e contando uma vez só os arquivos com hard
/// link (o node_modules do pnpm aponta para a loja dele).
pub fn espaco_ocupado(pasta: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt as _;
    let mut vistos = std::collections::HashSet::new();
    let mut pendentes = vec![pasta.to_path_buf()];
    let mut total = 0;
    while let Some(caminho) = pendentes.pop() {
        let Ok(dados) = std::fs::symlink_metadata(&caminho) else {
            continue;
        };
        if !dados.is_dir() && dados.nlink() > 1 && !vistos.insert((dados.dev(), dados.ino())) {
            continue;
        }
        total += dados.blocks() * 512;
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
    use std::os::unix::fs::MetadataExt as _;
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
        if dados.is_dir() {
            bytes += dados.blocks() * 512;
            if let Ok(lidas) = std::fs::read_dir(&caminho) {
                pendentes.extend(lidas.flatten().map(|e| e.path()));
            }
            continue;
        }
        arquivos += 1;
        if dados.nlink() > 1 && !vistos.insert((dados.dev(), dados.ino())) {
            continue;
        }
        bytes += dados.blocks() * 512;
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
    let _ = limpar_copia(projeto, copia, &projeto.with_extension("area"), branch, &[]);
}

/// Apaga a cópia de uma tarefa: a pasta (com node_modules, .venv e target/), o branch e os pontos
/// de volta dela. `pontos_antigos` são os gravados antes de os pontos irem para o grupo da tarefa.
/// `area` é a pasta onde o Vitral cria as cópias deste projeto.
pub fn limpar_copia(
    projeto: &Path,
    copia: &Path,
    area: &Path,
    branch: &str,
    pontos_antigos: &[String],
) -> Result<()> {
    let caminho = copia.to_string_lossy().to_string();
    if let Err(erro) = git(projeto, &["worktree", "remove", "--force", "--force", &caminho])
        && copia.exists()
    {
        // Com submódulo o git não remove. À mão, só o que é mesmo uma cópia: `.git` é um arquivo,
        // ou a pasta é das cópias do Vitral (uma limpeza cortada pode ter apagado o `.git` antes).
        if !copia.join(".git").is_file() && copia.parent() != Some(area) {
            bail!("não apagou {}: {erro:#}", copia.display());
        }
        std::fs::remove_dir_all(copia).map_err(|erro| anyhow::anyhow!("não apagou {}: {erro}", copia.display()))?;
    }
    podar(projeto);
    let _ = git(projeto, &["branch", "-D", branch]);
    let grupo = grupo_do_branch(branch).map(|g| format!("refs/vitral/pontos/{g}/"));
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

/// Esquece as cópias cujas pastas sumiram (apagadas à mão): o registro delas segurava o branch.
pub fn podar(projeto: &Path) {
    let _ = git(projeto, &["worktree", "prune"]);
}

/// Grupo dos pontos de volta da cópia de uma tarefa: "t12" no branch "vitral/t12-titulo".
fn grupo_do_branch(branch: &str) -> Option<String> {
    let resto = branch.strip_prefix("vitral/t")?;
    let numero: String = resto.chars().take_while(char::is_ascii_digit).collect();
    let depois = &resto[numero.len()..];
    (!numero.is_empty() && (depois.is_empty() || depois.starts_with('-'))).then(|| format!("t{numero}"))
}

fn git_com_entrada(pasta: &Path, args: &[&str], entrada: &str) -> Result<()> {
    use std::io::Write as _;
    let mut filho = Command::new("git")
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
        let (mut processo, unidade) = processos::comando("bash", "verificacao");
        processo
            .args(["-lc", comando])
            .current_dir(pasta)
            .stdin(Stdio::null())
            .stdout(escritor.try_clone()?)
            .stderr(escritor);
        let filho = processo.spawn()?;
        // O Command guarda a ponta de escrita: sem soltá-la, o pipe nunca fecha.
        drop(processo);
        Ok((leitor, unidade, filho))
    };
    let (mut leitor, unidade, mut filho) = match abrir() {
        Ok(aberto) => aberto,
        Err(erro) => return (false, format!("não rodou: {erro}")),
    };
    let escopo = Escopo::novo(unidade, filho.id());
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
/// índice nem no histórico: vira um ponto de volta antes de cada turno. Na cópia de uma tarefa, a
/// referência fica no grupo dela (`refs/vitral/pontos/t<N>/`), que limpar_copia apaga.
pub fn foto(pasta: &Path) -> Result<String> {
    let commit = foto_solta(pasta)?;
    let grupo = git(pasta, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .and_then(|branch| grupo_do_branch(&branch));
    let nome = match grupo {
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
            let saida = Command::new("git")
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
            Command::new("git")
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

/// A pasta é um repositório git ou uma cópia (worktree) de um.
pub fn e_repositorio(pasta: &Path) -> bool {
    git(pasta, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|s| s == "true")
}

#[cfg(test)]
mod testes {
    use super::*;

    fn repositorio() -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-teste-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        git(&pasta, &["init", "-q"]).unwrap();
        std::fs::write(pasta.join("a.txt"), "um\n").unwrap();
        git(&pasta, &["add", "-A"]).unwrap();
        git(&pasta, &["commit", "-q", "-m", "inicio"]).unwrap();
        pasta
    }

    #[test]
    fn voltar_ao_ponto_restaura_e_apaga_o_que_veio_depois() {
        let pasta = repositorio();
        std::fs::write(pasta.join("a.txt"), "dois\n").unwrap();
        let ponto = foto(&pasta).unwrap();
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
        let ponto = foto(&pasta).unwrap();
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
        let nomes = ["a.txt", "ação.txt", "com\"aspas.txt", "meu arquivo.txt", "pasta/configuração.md"];
        std::fs::create_dir_all(pasta.join("pasta")).unwrap();
        for nome in nomes {
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
        let fora = diff_para_coordenador(pasta, &projeto, base, &["/etc/hosts".into()], &[], 20_000).unwrap();
        assert!(fora.contains("/etc/hosts não fica dentro do projeto"), "{fora}");

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
        assert!((10_000..20_000).contains(&ocupado), "o hard link conta uma vez só: {ocupado}");

        std::fs::write(copia.caminho.join("a.txt"), "mudou\n").unwrap();
        let ponto = foto(&copia.caminho).unwrap();
        std::fs::write(copia.caminho.join("a.txt"), "de novo\n").unwrap();
        foto(&copia.caminho).unwrap();
        std::fs::write(outra.caminho.join("b.txt"), "da outra\n").unwrap();
        let da_outra = foto(&outra.caminho).unwrap();
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
        limpar_copia(&projeto, &copia.caminho, &area, &copia.branch, std::slice::from_ref(&antigo)).unwrap();
        assert!(!copia.caminho.exists());
        assert!(git(&projeto, &["rev-parse", "--verify", "-q", "refs/heads/vitral/t7-limpar"]).is_err());
        let sobrou = refs();
        assert_eq!(sobrou.lines().count(), 2, "{sobrou}");
        assert!(sobrou.contains(&format!("pontos/t8/{da_outra}")) && sobrou.contains(&format!("pontos/{alheio}")));

        // Pasta apagada à mão: a limpeza esquece o registro e apaga o branch.
        std::fs::remove_dir_all(&outra.caminho).unwrap();
        limpar_copia(&projeto, &outra.caminho, &area, &outra.branch, &[]).unwrap();
        assert_eq!(git(&projeto, &["worktree", "list", "--porcelain"]).unwrap().matches("worktree ").count(), 1);
        assert!(git(&projeto, &["branch", "--list", "vitral/*"]).unwrap().is_empty());
        assert_eq!(refs().lines().count(), 1, "{}", refs());
        // Uma pasta que não é cópia de tarefa fica onde está.
        let estranha = projeto.with_extension("estranha");
        std::fs::create_dir_all(&estranha).unwrap();
        std::fs::write(estranha.join("importante.txt"), "x").unwrap();
        assert!(limpar_copia(&projeto, &estranha, &area, "vitral/t9-x", &[]).is_err());
        assert!(estranha.join("importante.txt").exists());
        let _ = std::fs::remove_dir_all(&estranha);

        // Limpeza cortada que já tinha apagado o `.git` da cópia: o git não remove, e a pasta só
        // sai à mão porque fica entre as cópias do Vitral.
        let cortada = criar_worktree(&projeto, &area.join("t10"), "vitral/t10-cortada").unwrap();
        std::fs::create_dir_all(cortada.caminho.join("node_modules/pacote")).unwrap();
        std::fs::remove_file(cortada.caminho.join(".git")).unwrap();
        podar(&projeto);
        limpar_copia(&projeto, &cortada.caminho, &area, &cortada.branch, &[]).unwrap();
        assert!(!cortada.caminho.exists());
        assert!(git(&projeto, &["branch", "--list", "vitral/*"]).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&area);
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
