//! Chats isolados: o `claude` dos chats expostos a texto de terceiros (e o que o Vitral roda nas
//! cópias deles) dentro do `bwrap`, sem o barramento de sessão (e com ele o chaveiro e o
//! `secret-tool`), sem o agente ssh, o `~/.ssh` e o login do gh. Com `--unshare-pid`, o chat não
//! enxerga o processo do Vitral, então não lê a chave do Linear na memória dele. A rede continua
//! aberta (o MCP do Vitral é em 127.0.0.1). Só no Linux.
// Provisório: o motor passa a usar estes módulos no G3.
#![allow(dead_code)]

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::remoto::Executor;

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

/// Por que não há isolamento, quando não há.
pub const INDISPONIVEL_NESTE_SISTEMA: &str = "não disponível neste sistema";

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
    let pastas: [PathBuf; 3] = [home.join(".ssh"), home.join(".config/gh"), home.join(".local/share/keyrings")];
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

#[cfg(test)]
mod testes {
    use super::*;
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
