//! Chats isolados: o `claude` dos chats expostos a texto de terceiros (e o que o Vitral roda nas
//! cópias deles) dentro do `bwrap`, sem o barramento de sessão (e com ele o chaveiro e o
//! `secret-tool`), sem o agente ssh, o `~/.ssh` e o login do gh. Com `--unshare-pid`, o chat não
//! enxerga o processo do Vitral, então não lê a chave do Linear na memória dele. A rede continua
//! aberta (o MCP do Vitral é em 127.0.0.1); os sockets abstratos (o X11 em `@/tmp/.X11-unix/X0`, o
//! ICE), que vivem na rede e não no disco, ficam de fora pelo escopo do Landlock. Só no Linux: nos
//! outros sistemas, e sem o bwrap funcionando, o chat isolado roda só com o ambiente e os redutores.

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

/// Os argumentos do bwrap até o `--`, antes do programa. O que não existe em `home` fica de fora
/// (o bwrap recusaria montar por cima).
pub fn argumentos(home: &Path, runtime: &Path, existe: &dyn Fn(&Path) -> bool) -> Vec<OsString> {
    if cfg!(not(target_os = "linux")) {
        return Vec::new();
    }
    let mut args: Vec<OsString> = ["--die-with-parent", "--unshare-pid", "--dev-bind", "/", "/", "--proc", "/proc", "--tmpfs"]
        .iter()
        .map(OsString::from)
        .collect();
    args.push(runtime.into());
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

/// Os argumentos do bwrap desta sessão (até o `--`), ou `None` quando o chat roda sem ele: no modo
/// só com o ambiente, fora do Linux ou sem o XDG_RUNTIME_DIR.
pub fn envoltorio(modo: Modo) -> Option<Vec<OsString>> {
    if modo != Modo::Bwrap || cfg!(not(target_os = "linux")) {
        return None;
    }
    let runtime = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?);
    let home = dirs::home_dir()?;
    Some(argumentos(&home, &runtime, &|p| p.exists()))
}

/// O processo de `programa` isolado, pelo `processos::comando` (o escopo do systemd conta o que
/// roda dentro do bwrap): com o bwrap, `bwrap <argumentos> -- programa` sob o escopo do Landlock;
/// sempre, com o ambiente de `ambiente()`. Quem chama acrescenta os argumentos do programa.
pub fn comando(programa: impl AsRef<OsStr>, tipo: &str, modo: Modo) -> processos::Comando {
    let mut comando = match envoltorio(modo) {
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
        let tudo = argumentos(home, runtime, &|_| true);
        let texto: Vec<String> = tudo.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(
            texto,
            [
                "--die-with-parent", "--unshare-pid", "--dev-bind", "/", "/", "--proc", "/proc", "--tmpfs", "/run/user/1000",
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
        let so_ssh = argumentos(home, runtime, &|p| p.ends_with(".ssh"));
        let texto: Vec<String> = so_ssh.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(texto[8..], ["/run/user/1000", "--tmpfs", "/home/fulano/.ssh", "--"]);
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
        let comando = comando("claude", "teste", Modo::SoAmbiente);
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
        let linha = linha(&comando("claude", "teste", Modo::Bwrap));
        let bwrap = linha.iter().position(|a| a == "bwrap").expect("o bwrap na linha");
        let depois = &linha[bwrap + 1..];
        assert_eq!(&depois[..4], ["--die-with-parent", "--unshare-pid", "--dev-bind", "/"], "{linha:?}");
        // O `--` fecha os argumentos do bwrap e o programa vem logo depois.
        assert_eq!(&depois[depois.len() - 2..], ["--", "claude"]);
        assert_eq!(envoltorio(Modo::SoAmbiente), None);
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
        let mut c = comando("bash", "teste", Modo::Bwrap);
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
