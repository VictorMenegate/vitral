//! `vitral --versao` responde sem abrir janela: sem DISPLAY nem WAYLAND_DISPLAY, como no runner do
//! GitHub e no atualizador, que confere o binário novo antes de trocar de versão.

use std::{
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn rodar(argumentos: &[&str]) -> Output {
    let mut filho = Command::new(env!("CARGO_BIN_EXE_vitral"))
        .args(argumentos)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let limite = Instant::now() + Duration::from_secs(10);
    while filho.try_wait().unwrap().is_none() {
        if Instant::now() > limite {
            let _ = filho.kill();
            let _ = filho.wait();
            panic!("vitral {argumentos:?} não saiu em 10 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    filho.wait_with_output().unwrap()
}

/// A do Cargo.toml, ou a de desenvolvimento do `ferramentas/instalar-dev.sh`.
fn versao() -> &'static str {
    option_env!("VITRAL_VERSAO").filter(|v| !v.is_empty()).unwrap_or(env!("CARGO_PKG_VERSION"))
}

fn texto(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn versao_sai_sem_janela() {
    let saida = rodar(&["--versao"]);
    assert!(saida.status.success(), "{}", texto(&saida.stderr));
    assert_eq!(texto(&saida.stdout), format!("vitral {}\n", versao()));
    assert_eq!(texto(&saida.stderr), "");
}

#[test]
fn versao_em_json_diz_canal_commit_e_formato() {
    let saida = rodar(&["--versao", "--json"]);
    assert!(saida.status.success(), "{}", texto(&saida.stderr));
    let valor: serde_json::Value = serde_json::from_slice(&saida.stdout).unwrap();
    assert_eq!(valor["versao"], versao());
    let canal = option_env!("VITRAL_CANAL").filter(|c| !c.is_empty()).unwrap_or("pc");
    assert_eq!(valor["canal"], canal);
    assert_eq!(valor["commit"], option_env!("VITRAL_COMMIT").unwrap_or(""));
    assert_eq!(valor["plataforma"], format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH));
    assert!(valor["formato_dados"].as_u64().is_some_and(|f| f >= 1), "{valor}");
}
