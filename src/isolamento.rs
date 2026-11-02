//! Chats isolados: o `claude` dos chats expostos a texto de terceiros (e o que o Vitral roda nas
//! cópias deles) dentro do `bwrap`, sem o barramento de sessão (e com ele o chaveiro e o
//! `secret-tool`), sem o agente ssh, o `~/.ssh` e o login do gh. Com `--unshare-pid`, o chat não
//! enxerga o processo do Vitral, então não lê a chave do Linear na memória dele. A rede continua
//! aberta (o MCP do Vitral é em 127.0.0.1); os sockets abstratos (o X11 em `@/tmp/.X11-unix/X0`, o
//! ICE), que vivem na rede e não no disco, ficam de fora pelo escopo do Landlock. O que o git do
//! Vitral (fora do bwrap) lê e executa do repositório fica só leitura lá dentro (`travas_do_git`).
//! Só no Linux: nos outros sistemas, e sem o bwrap funcionando, o chat isolado roda só com o
//! ambiente e os redutores.

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

use crate::{processos, remoto::Executor};

/// Como um chat isolado (e o que o Vitral roda na cópia dele) roda.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modo {
    /// Dentro do bwrap, com o escopo do Landlock e o ambiente.
    Bwrap,
    /// Sem o bwrap (ele não funciona aqui, ou fora do Linux): só o ambiente e os redutores.
    SoAmbiente,
}

impl Modo {
    /// O modo de um chat isolado, conforme o `testar()` da abertura.
    pub fn com_bwrap(funciona: bool) -> Self {
        if funciona { Modo::Bwrap } else { Modo::SoAmbiente }
    }
}

/// Redutores nos chats isolados (o `bash -c` passa por eles; a barreira é o bwrap).
pub const BLOQUEADAS: [&str; 8] = [
    "Bash(git push:*)",
    "Bash(git push)",
    "Bash(gh pr:*)",
    "Bash(gh issue:*)",
    "Bash(gh api:*)",
    "Bash(gh auth:*)",
    "Bash(secret-tool:*)",
    "Bash(ssh-add:*)",
];

/// O bwrap funciona aqui? Roda um `true` dentro dele, com limite de 5 s, uma vez ao abrir.
pub fn testar(e: &dyn Executor) -> bool {
    #[cfg(target_os = "linux")]
    {
        let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").and_then(|r| r.into_string().ok()) else {
            return false;
        };
        let args: Vec<String> = ["--die-with-parent", "--unshare-pid", "--dev-bind", "/", "/", "--proc", "/proc", "--tmpfs"]
            .iter()
            .map(|a| a.to_string())
            .chain([runtime, "true".to_string()])
            .collect();
        let ambiente = crate::remoto::Ambiente::default();
        let limite = std::time::Duration::from_secs(5);
        e.rodar("bwrap", &args, &ambiente, Path::new("/"), None, limite).is_ok_and(|s| s.codigo == Some(0))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = e;
        false
    }
}

/// O que do git fica preso dentro do bwrap. O git do Vitral roda fora dele (com o barramento, o
/// chaveiro e as credenciais) nas mesmas pastas, e lê o `.git` comum da origem e de todas as
/// cópias: um `core.fsmonitor`, um filtro, um `credential.helper` ou um `pre-push` gravado por um
/// chat isolado rodaria fora do isolamento.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TravasDoGit {
    /// Montadas sobre si mesmas, graváveis como antes: um ponto de montagem não se move nem se
    /// apaga, então o `.git` (ou a cópia inteira) não troca por outro com outra configuração. Os
    /// de cima primeiro.
    pub fixas: Vec<PathBuf>,
    /// Só leitura: no diretório comum, a configuração (`config`, `config.worktree`), os ganchos e o
    /// `info/` (os atributos); na cópia, o arquivo `.git`, o `commondir` e o `gitdir` (que apontam o
    /// diretório comum) e o `config.worktree` dela. O índice, os objetos e os refs continuam
    /// graváveis: commit, add e stash funcionam.
    pub so_leitura: Vec<PathBuf>,
}

/// As travas do repositório de `pasta` (procurado de `pasta` para cima). Sem repositório, nada.
/// Cria os `hooks/` e `info/` que faltarem no diretório comum (vazios, como o `git init` deixa),
/// para o chat não criá-los lá dentro.
pub fn travas_do_git(pasta: &Path, home: &Path) -> TravasDoGit {
    let mut travas = TravasDoGit::default();
    let Some(raiz) = pasta.ancestors().find(|p| p.join(".git").exists()) else {
        return travas;
    };
    let ponto_git = raiz.join(".git");
    let real = |p: &Path| dunce::canonicalize(p).ok();
    let (gitdir, comum) = if ponto_git.is_dir() {
        (ponto_git.clone(), ponto_git.clone())
    } else {
        // Uma cópia (git worktree): "gitdir: <origem>/.git/worktrees/<nome>", e nele o `commondir`.
        let Some(gitdir) = std::fs::read_to_string(&ponto_git)
            .ok()
            .and_then(|t| t.lines().next().and_then(|l| l.strip_prefix("gitdir:")).map(|g| raiz.join(g.trim())))
        else {
            return travas;
        };
        let comum = std::fs::read_to_string(gitdir.join("commondir")).map_or_else(|_| gitdir.clone(), |c| gitdir.join(c.trim()));
        travas.so_leitura.extend([ponto_git.clone(), gitdir.join("commondir"), gitdir.join("gitdir"), gitdir.join("config.worktree")]);
        (gitdir, comum)
    };
    // O `commondir` é relativo ("../.."): o caminho de verdade, para achar a origem.
    let (gitdir, comum) = (real(&gitdir).unwrap_or(gitdir), real(&comum).unwrap_or(comum));
    for pasta in ["hooks", "info"] {
        let _ = std::fs::create_dir_all(comum.join(pasta));
    }
    travas.so_leitura.extend(["config", "config.worktree", "hooks", "info"].map(|nome| comum.join(nome)));
    // A raiz da pasta, o gitdir, o comum e a origem dele; nunca `/` nem algo acima do HOME (montar
    // de novo por cima tiraria o que o bwrap escondeu ali).
    let origem = comum.parent().filter(|_| comum.file_name().is_some_and(|n| n == ".git"));
    let home = real(home).unwrap_or_else(|| home.to_path_buf());
    let mut fixas: Vec<PathBuf> = [Some(raiz), Some(gitdir.as_path()), Some(comum.as_path()), origem]
        .into_iter()
        .flatten()
        .filter_map(real)
        .filter(|p| p.parent().is_some() && !home.starts_with(p))
        .collect();
    fixas.sort_by_key(|p| p.components().count());
    fixas.dedup();
    travas.fixas = fixas;
    travas.so_leitura = travas.so_leitura.iter().filter_map(|p| real(p)).collect();
    travas.so_leitura.dedup();
    travas
}

/// Os argumentos do bwrap até o `--`, antes do programa. O que não existe em `home` fica de fora
/// (o bwrap recusaria montar por cima). `git`: as travas do repositório da pasta, montadas antes
/// do resto (o que o bwrap esconde fica por cima delas).
pub fn argumentos(home: &Path, runtime: &Path, existe: &dyn Fn(&Path) -> bool, git: &TravasDoGit) -> Vec<OsString> {
    if cfg!(not(target_os = "linux")) {
        return Vec::new();
    }
    let mut args: Vec<OsString> =
        ["--die-with-parent", "--unshare-pid", "--dev-bind", "/", "/", "--proc", "/proc"].iter().map(OsString::from).collect();
    for pasta in &git.fixas {
        args.extend([OsString::from("--bind"), pasta.into(), pasta.into()]);
    }
    for caminho in &git.so_leitura {
        args.extend([OsString::from("--ro-bind"), caminho.into(), caminho.into()]);
    }
    // A configuração global do git, que o git do Vitral também lê.
    let configuracao: [PathBuf; 2] = [home.join(".gitconfig"), home.join(".config/git")];
    for caminho in configuracao.iter().filter(|c| existe(c)) {
        args.extend([OsString::from("--ro-bind"), caminho.into(), caminho.into()]);
    }
    args.extend([OsString::from("--tmpfs"), runtime.into()]);
    // Os sockets do X11 e do ICE com caminho (os abstratos saem pelo Landlock): sem o cookie, que
    // fica no XDG_RUNTIME_DIR, o X já recusa; assim nem a conexão abre.
    let pastas: [PathBuf; 5] = [
        home.join(".ssh"),
        home.join(".config/gh"),
        home.join(".local/share/keyrings"),
        PathBuf::from("/tmp/.X11-unix"),
        PathBuf::from("/tmp/.ICE-unix"),
    ];
    for pasta in pastas.iter().filter(|p| existe(p)) {
        args.extend([OsString::from("--tmpfs"), pasta.into()]);
    }
    let arquivos: [PathBuf; 2] = [home.join(".git-credentials"), home.join(".netrc")];
    for arquivo in arquivos.iter().filter(|a| existe(a)) {
        args.extend([OsString::from("--ro-bind"), "/dev/null".into(), arquivo.into()]);
    }
    args.push("--".into());
    args
}

/// O ambiente do processo isolado: o que definir e o que tirar. O helper do git (o do gh, que
/// busca o token no chaveiro) fica anulado pelo `GIT_CONFIG_*`, que vale como `git -c`.
pub fn ambiente() -> (Vec<(String, String)>, Vec<&'static str>) {
    let mut definir: Vec<(String, String)> = [
        ("DBUS_SESSION_BUS_ADDRESS", "disabled:"),
        ("GIT_ASKPASS", "/bin/false"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_CONFIG_COUNT", "3"),
    ]
    .iter()
    .map(|(n, v)| (n.to_string(), v.to_string()))
    .collect();
    let helpers = ["credential.helper", "credential.https://github.com.helper", "credential.https://gist.github.com.helper"];
    for (i, chave) in helpers.iter().enumerate() {
        definir.push((format!("GIT_CONFIG_KEY_{i}"), chave.to_string()));
        definir.push((format!("GIT_CONFIG_VALUE_{i}"), String::new()));
    }
    let remover = vec!["SSH_AUTH_SOCK", "SSH_ASKPASS", "GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"];
    (definir, remover)
}

/// Os argumentos do bwrap desta sessão (até o `--`) para um processo em `pasta`, ou `None` quando o
/// chat roda sem ele: no modo só com o ambiente, fora do Linux ou sem o XDG_RUNTIME_DIR.
pub fn envoltorio(modo: Modo, pasta: &Path) -> Option<Vec<OsString>> {
    if modo != Modo::Bwrap || cfg!(not(target_os = "linux")) {
        return None;
    }
    let runtime = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?);
    let home = dirs::home_dir()?;
    Some(argumentos(&home, &runtime, &|p| p.exists(), &travas_do_git(pasta, &home)))
}

/// O processo de `programa` isolado, rodando em `pasta`, pelo `processos::comando` (o escopo do
/// systemd conta o que roda dentro do bwrap): com o bwrap, `bwrap <argumentos> -- programa` sob o
/// escopo do Landlock; sempre, com o ambiente de `ambiente()`. Quem chama acrescenta os argumentos
/// do programa e o `current_dir(pasta)`.
pub fn comando(programa: impl AsRef<OsStr>, tipo: &str, modo: Modo, pasta: &Path) -> processos::Comando {
    let mut comando = match envoltorio(modo, pasta) {
        Some(args) => {
            let mut comando = processos::comando("bwrap", tipo);
            comando.args(args).arg(programa.as_ref());
            #[cfg(target_os = "linux")]
            cortar_sockets_abstratos(&mut comando);
            comando
        }
        None => processos::comando(programa, tipo),
    };
    let (definir, remover) = ambiente();
    for variavel in remover {
        comando.env_remove(variavel);
    }
    for (nome, valor) in definir {
        comando.env(nome, valor);
    }
    comando
}

/// O escopo do Landlock no processo que vai abrir o bwrap (vale para tudo o que nascer dele).
#[cfg(target_os = "linux")]
fn cortar_sockets_abstratos(comando: &mut processos::Comando) {
    use std::os::unix::process::CommandExt as _;
    // SAFETY: entre o fork e o exec, só chamadas de sistema (nada que aloque ou trave).
    unsafe {
        comando.pre_exec(|| {
            escopo_sem_sockets_abstratos();
            Ok(())
        });
    }
}

/// Landlock com o escopo `LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET` (ABI 6, Linux 6.12): o processo e
/// os filhos não conectam em socket abstrato criado fora do domínio (o X11, o ICE, o barramento
/// do IBus); TCP (o 127.0.0.1 do MCP) e os sockets com caminho seguem como estão. Pede o
/// `no_new_privs`, que o bwrap liga de qualquer jeito. Num kernel sem ele, segue sem: é reforço,
/// a barreira é o bwrap.
#[cfg(target_os = "linux")]
fn escopo_sem_sockets_abstratos() {
    /// `struct landlock_ruleset_attr` da ABI 6 (o kernel aceita o tamanho de cada versão).
    #[repr(C)]
    struct Regras {
        acesso_fs: u64,
        acesso_rede: u64,
        escopo: u64,
    }
    const VERSAO: libc::c_ulong = 1; // LANDLOCK_CREATE_RULESET_VERSION
    const SOCKET_ABSTRATO: u64 = 1; // LANDLOCK_SCOPE_ABSTRACT_UNIX_SOCKET
    let regras = Regras { acesso_fs: 0, acesso_rede: 0, escopo: SOCKET_ABSTRATO };
    // SAFETY: as chamadas recebem ponteiros válidos (ou nulo, na consulta da versão) e o tamanho
    // exato da struct; um erro só deixa o processo sem o escopo.
    unsafe {
        let abi = libc::syscall(libc::SYS_landlock_create_ruleset, std::ptr::null::<Regras>(), 0 as libc::c_ulong, VERSAO);
        if abi < 6 {
            return;
        }
        let fd = libc::syscall(
            libc::SYS_landlock_create_ruleset,
            &regras as *const Regras,
            std::mem::size_of::<Regras>() as libc::c_ulong,
            0 as libc::c_ulong,
        );
        if fd < 0 {
            return;
        }
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1 as libc::c_ulong, 0 as libc::c_ulong, 0 as libc::c_ulong, 0 as libc::c_ulong) == 0 {
            libc::syscall(libc::SYS_landlock_restrict_self, fd, 0 as libc::c_ulong);
        }
        libc::close(fd as libc::c_int);
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    #[cfg(target_os = "linux")]
    use crate::remoto::falso::{Falso, saida};

    #[test]
    #[cfg(target_os = "linux")]
    fn argumentos_deixam_de_fora_o_que_nao_existe() {
        let home = Path::new("/home/fulano");
        let runtime = Path::new("/run/user/1000");
        let git = TravasDoGit { fixas: vec!["/p/app".into()], so_leitura: vec!["/p/app/.git/config".into()] };
        let tudo = argumentos(home, runtime, &|_| true, &git);
        let texto: Vec<String> = tudo.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(
            texto,
            [
                "--die-with-parent", "--unshare-pid", "--dev-bind", "/", "/", "--proc", "/proc",
                "--bind", "/p/app", "/p/app",
                "--ro-bind", "/p/app/.git/config", "/p/app/.git/config",
                "--ro-bind", "/home/fulano/.gitconfig", "/home/fulano/.gitconfig",
                "--ro-bind", "/home/fulano/.config/git", "/home/fulano/.config/git",
                "--tmpfs", "/run/user/1000",
                "--tmpfs", "/home/fulano/.ssh",
                "--tmpfs", "/home/fulano/.config/gh",
                "--tmpfs", "/home/fulano/.local/share/keyrings",
                "--tmpfs", "/tmp/.X11-unix",
                "--tmpfs", "/tmp/.ICE-unix",
                "--ro-bind", "/dev/null", "/home/fulano/.git-credentials",
                "--ro-bind", "/dev/null", "/home/fulano/.netrc",
                "--",
            ]
        );
        let so_ssh = argumentos(home, runtime, &|p| p.ends_with(".ssh"), &TravasDoGit::default());
        let texto: Vec<String> = so_ssh.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(texto[7..], ["--tmpfs", "/run/user/1000", "--tmpfs", "/home/fulano/.ssh", "--"]);
    }

    /// Uma origem com uma cópia (git worktree), como o Vitral prepara, em `pasta`.
    #[cfg(target_os = "linux")]
    fn origem_e_copia(pasta: &Path) -> (PathBuf, PathBuf) {
        let (origem, copia) = (pasta.join("app"), pasta.join("copias/t1"));
        std::fs::create_dir_all(&origem).unwrap();
        let git = |pasta: &Path, args: &[&str]| {
            let saida = crate::git::novo_git()
                .arg("-C")
                .arg(pasta)
                .args(["-c", "user.name=T", "-c", "user.email=t@t"])
                .args(args)
                .output()
                .unwrap();
            assert!(saida.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&saida.stderr));
        };
        git(&origem, &["init", "-q"]);
        std::fs::write(origem.join("a.txt"), "um\n").unwrap();
        git(&origem, &["add", "-A"]);
        git(&origem, &["commit", "-q", "-m", "inicio"]);
        git(&origem, &["worktree", "add", "-q", "-b", "vitral/t1", &copia.to_string_lossy()]);
        (origem, copia)
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn travas_do_git_da_copia_prendem_o_comum_e_o_ponteiro() {
        let raiz = std::env::temp_dir().join(format!("vitral-travas-{}", crate::modelo::novo_id()));
        let (origem, copia) = origem_e_copia(&raiz);
        // Sem `hooks/` (um clone pode vir sem): a trava cria a pasta, vazia.
        std::fs::remove_dir_all(origem.join(".git/hooks")).unwrap();
        let travas = travas_do_git(&copia.join("src"), Path::new("/home/fulano"));
        let real = |p: PathBuf| dunce::canonicalize(p).unwrap();
        let (origem, copia) = (real(origem), real(copia));
        let comum = origem.join(".git");
        let gitdir = comum.join("worktrees/t1");
        assert_eq!(travas.fixas, [origem.clone(), copia.clone(), comum.clone(), gitdir.clone()]);
        for preso in [copia.join(".git"), gitdir.join("commondir"), gitdir.join("gitdir"), comum.join("config"), comum.join("hooks"), comum.join("info")] {
            assert!(travas.so_leitura.contains(&preso), "{preso:?} fora de {:?}", travas.so_leitura);
        }
        assert!(comum.join("hooks").is_dir());
        // O que não existe fica de fora (o bwrap recusaria): o config.worktree, sem a extensão.
        assert!(!travas.so_leitura.iter().any(|p| p.ends_with("config.worktree")));
        // Na origem: o próprio `.git`; e nada acima do HOME (aqui, a origem é o HOME).
        let na_origem = travas_do_git(&origem, &origem);
        assert_eq!(na_origem.fixas, [comum.clone()]);
        assert!(na_origem.so_leitura.contains(&comum.join("config")));
        assert_eq!(travas_do_git(Path::new("/"), Path::new("/home/fulano")), TravasDoGit::default());
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// Manual, sem rede (`cargo test -- --ignored integracao_`): o bwrap de verdade numa cópia. Lá
    /// dentro, gravar um `core.fsmonitor`, um `pre-commit` ou um filtro no `.git` comum falha, e
    /// trocar o `.git` (ou o ponteiro da cópia) por outro também; o commit, o add e o stash do
    /// agente funcionam.
    #[test]
    #[ignore]
    #[cfg(target_os = "linux")]
    fn integracao_isolamento_prende_o_git_comum_da_copia() {
        use std::io::Read as _;
        let raiz = dirs::home_dir().unwrap().join(format!(".cache/vitral-trabalho/travas-{}", crate::modelo::novo_id()));
        let (origem, copia) = origem_e_copia(&raiz);
        let comum = origem.join(".git");
        let script = format!(
            "export GIT_AUTHOR_NAME=T GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=T GIT_COMMITTER_EMAIL=t@t; \
             tenta() {{ if eval \"$2\" >/dev/null 2>&1; then echo \"$1:gravou\"; else echo \"$1:barrado\"; fi; }}; \
             tenta fsmonitor 'git config core.fsmonitor /bin/evil'; \
             tenta filtro 'git config filter.x.clean /bin/evil'; \
             tenta config-direto 'echo \"[core] fsmonitor = /bin/evil\" >> {comum}/config'; \
             tenta pre-commit 'printf \"#!/bin/sh\\ntouch /tmp/evil\\n\" > {comum}/hooks/pre-commit'; \
             tenta atributos 'echo \"* filter=x\" > {comum}/info/attributes'; \
             tenta trocar-o-comum 'mv {comum} {comum}-velho'; \
             tenta trocar-o-ponteiro 'echo gitdir: /tmp/outro > .git'; \
             tenta trocar-a-copia 'mv {copia} {copia}-velha'; \
             echo b > b.txt; tenta add 'git add b.txt'; tenta commit 'git commit -q -m agente'; \
             echo c > a.txt; tenta stash 'git stash -q'; tenta stash-pop 'git stash pop -q'",
            comum = comum.display(),
            copia = copia.display(),
        );
        let mut c = comando("bash", "teste", Modo::Bwrap, &copia);
        c.current_dir(&copia).args(["-c", &script]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
        let (mut filho, escopo) = processos::abrir(c).unwrap();
        let mut saida = String::new();
        filho.stdout.take().unwrap().read_to_string(&mut saida).unwrap();
        processos::esperar(&mut filho, &escopo, std::time::Duration::from_secs(30));
        println!("{saida}");
        for barrado in ["fsmonitor", "filtro", "config-direto", "pre-commit", "atributos", "trocar-o-comum", "trocar-o-ponteiro", "trocar-a-copia"] {
            assert!(saida.contains(&format!("{barrado}:barrado")), "{barrado}: {saida}");
        }
        for gravou in ["add", "commit", "stash", "stash-pop"] {
            assert!(saida.contains(&format!("{gravou}:gravou")), "{gravou}: {saida}");
        }
        let config = std::fs::read_to_string(comum.join("config")).unwrap();
        assert!(!config.contains("evil") && !comum.join("hooks/pre-commit").exists());
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn ambiente_anula_o_helper_e_tira_o_agente() {
        let (definir, remover) = ambiente();
        let valor = |nome: &str| definir.iter().find(|(n, _)| n == nome).map(|(_, v)| v.as_str());
        assert_eq!(valor("DBUS_SESSION_BUS_ADDRESS"), Some("disabled:"));
        assert_eq!(valor("GIT_CONFIG_COUNT"), Some("3"));
        assert_eq!(valor("GIT_CONFIG_KEY_1"), Some("credential.https://github.com.helper"));
        assert_eq!(valor("GIT_CONFIG_VALUE_2"), Some(""));
        assert!(remover.contains(&"SSH_AUTH_SOCK") && remover.contains(&"GH_TOKEN"));
    }

    /// O programa e os argumentos do comando, com o systemd-run (se houver) na frente.
    fn linha(comando: &processos::Comando) -> Vec<String> {
        std::iter::once(comando.get_program()).chain(comando.get_args()).map(|a| a.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn comando_isolado_leva_o_ambiente_sem_agente_nem_barramento() {
        let comando = comando("claude", "teste", Modo::SoAmbiente, Path::new("/"));
        let ambiente: Vec<(String, Option<String>)> = comando
            .get_envs()
            .map(|(n, v)| (n.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned())))
            .collect();
        let valor = |nome: &str| ambiente.iter().find(|(n, _)| n == nome).map(|(_, v)| v.clone());
        assert_eq!(valor("DBUS_SESSION_BUS_ADDRESS"), Some(Some("disabled:".into())));
        assert_eq!(valor("GIT_CONFIG_KEY_0"), Some(Some("credential.helper".into())));
        assert_eq!(valor("SSH_AUTH_SOCK"), Some(None), "tirado do ambiente");
        assert_eq!(valor("GH_TOKEN"), Some(None));
        // Sem o bwrap, o programa roda direto (no escopo do systemd, se houver).
        let linha = linha(&comando);
        assert!(!linha.iter().any(|a| a == "bwrap"), "{linha:?}");
        assert_eq!(linha.last().map(String::as_str), Some("claude"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn comando_isolado_roda_dentro_do_bwrap() {
        if std::env::var_os("XDG_RUNTIME_DIR").is_none() || dirs::home_dir().is_none() {
            return;
        }
        let linha = linha(&comando("claude", "teste", Modo::Bwrap, Path::new("/")));
        let bwrap = linha.iter().position(|a| a == "bwrap").expect("o bwrap na linha");
        let depois = &linha[bwrap + 1..];
        assert_eq!(&depois[..4], ["--die-with-parent", "--unshare-pid", "--dev-bind", "/"], "{linha:?}");
        // O `--` fecha os argumentos do bwrap e o programa vem logo depois.
        assert_eq!(&depois[depois.len() - 2..], ["--", "claude"]);
        assert_eq!(envoltorio(Modo::SoAmbiente, Path::new("/")), None);
    }

    /// Manual, sem rede (`cargo test -- --ignored integracao_`): o escopo do Landlock e o bwrap no
    /// processo de verdade. Dentro: o socket abstrato do X11 não conecta, o barramento sumiu, o
    /// 127.0.0.1 continua.
    #[test]
    #[ignore]
    #[cfg(target_os = "linux")]
    fn integracao_isolamento_corta_o_x11_e_deixa_o_127() {
        use std::io::Read as _;
        let servidor = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let porta = servidor.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for conexao in servidor.incoming().flatten().take(1) {
                drop(conexao);
            }
        });
        let script = format!(
            "test -S \"$XDG_RUNTIME_DIR/bus\" && echo bus-aparece || echo sem-bus; \
             python3 -I -c 'import socket\ns=socket.socket(socket.AF_UNIX)\ntry:\n  s.connect(\"\\0/tmp/.X11-unix/X0\"); print(\"x11-conecta\")\nexcept Exception as e: print(\"x11-cortado\", type(e).__name__)'; \
             python3 -I -c 'import socket; socket.create_connection((\"127.0.0.1\", {porta}), 3); print(\"127-ok\")'"
        );
        let mut c = comando("bash", "teste", Modo::Bwrap, Path::new("/"));
        c.args(["-c", &script]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
        let (mut filho, escopo) = processos::abrir(c).unwrap();
        let mut saida = String::new();
        filho.stdout.take().unwrap().read_to_string(&mut saida).unwrap();
        processos::esperar(&mut filho, &escopo, std::time::Duration::from_secs(20));
        println!("{saida}");
        assert!(saida.contains("sem-bus"), "{saida}");
        assert!(saida.contains("x11-cortado"), "{saida}");
        assert!(saida.contains("127-ok"), "{saida}");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn testar_roda_o_bwrap_com_limite() {
        let e = Falso::com(vec![saida(0, "", ""), saida(1, "", "bwrap: No permissions to creating new namespace")]);
        if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
            return;
        }
        assert!(testar(&e));
        assert!(!testar(&e));
        let chamada = &e.chamadas()[0];
        assert_eq!(chamada.programa, "bwrap");
        assert_eq!(chamada.args.last().map(String::as_str), Some("true"));
        assert_eq!(chamada.limite, std::time::Duration::from_secs(5));
    }
}
