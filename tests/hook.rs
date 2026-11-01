//! `vitral --hook-verificacao`: o hook Stop dos ajudantes no Windows, em exec form (sem shell). Roda a
//! verificação da tarefa (no Git Bash, no Windows) e sai com 2 e o fim da saída no stderr quando ela
//! falha; na segunda parada seguida (`stop_hook_active`) ou sem verificação, sai com 0. Sem janela e
//! sem tocar nos dados.

use std::{
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn rodar(verificacao: Option<&str>, entrada: &str) -> (Option<i32>, String) {
    let pasta = std::env::temp_dir();
    let mut comando = Command::new(env!("CARGO_BIN_EXE_vitral"));
    comando
        .arg("--hook-verificacao")
        .current_dir(&pasta)
        .env_remove("VITRAL_VERIFICACAO")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(verificacao) = verificacao {
        comando.env("VITRAL_VERIFICACAO", verificacao);
    }
    let mut filho = comando.spawn().unwrap();
    filho.stdin.take().unwrap().write_all(entrada.as_bytes()).unwrap();
    let limite = Instant::now() + Duration::from_secs(60);
    while filho.try_wait().unwrap().is_none() {
        if Instant::now() > limite {
            let _ = filho.kill();
            panic!("vitral --hook-verificacao não saiu em 60 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let saida = filho.wait_with_output().unwrap();
    (saida.status.code(), String::from_utf8_lossy(&saida.stderr).into_owned())
}

#[test]
fn hook_de_verificacao_faz_o_trabalhador_continuar_so_uma_vez() {
    let (codigo, erro) = rodar(Some("echo falhou de verdade; exit 1"), r#"{"stop_hook_active":false}"#);
    assert_eq!(codigo, Some(2), "{erro}");
    assert!(erro.starts_with("A verificação da tarefa falhou (`echo falhou de verdade; exit 1`)"), "{erro}");
    assert!(erro.contains("falhou de verdade"), "{erro}");
    // A segunda parada seguida não roda de novo; sem verificação, nada a fazer.
    assert_eq!(rodar(Some("exit 1"), r#"{"session_id":"x","stop_hook_active": true}"#).0, Some(0));
    assert_eq!(rodar(None, r#"{"stop_hook_active":false}"#).0, Some(0));
    // Passou: sai com 0 e nada no stderr. O bash é o de login, como no Revisar.
    let (codigo, erro) = rodar(Some("test -n \"$PATH\" && true"), r#"{"stop_hook_active":false}"#);
    assert_eq!((codigo, erro.as_str()), (Some(0), ""));
}
