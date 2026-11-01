//! `vitral --instalar` com o binário de verdade, num HOME temporário (nunca o do PC): cria o layout,
//! o link `atual`, o `.desktop` com `Exec` no link e o link em `~/.local/bin`; reinstalar não
//! duplica nada.

use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn versao() -> &'static str {
    option_env!("VITRAL_VERSAO").filter(|v| !v.is_empty()).unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// Um HOME de mentira em `target/tmp`, apagado no fim.
struct Casa(PathBuf);

impl Casa {
    fn nova() -> Casa {
        let raiz = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("instalar-{}", std::process::id()));
        let _ = fs::remove_dir_all(&raiz);
        fs::create_dir_all(raiz.join(".local/bin")).unwrap();
        Casa(raiz)
    }

    fn rodar(&self, binario: &Path, argumentos: &[&str]) -> Output {
        let home = &self.0;
        let mut filho = Command::new(binario)
            .args(argumentos)
            .env("HOME", home)
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_BIN_HOME", home.join(".local/bin"))
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("WAYLAND_SOCKET")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let limite = Instant::now() + Duration::from_secs(60);
        while filho.try_wait().unwrap().is_none() {
            if Instant::now() > limite {
                let _ = filho.kill();
                let _ = filho.wait();
                panic!("vitral {argumentos:?} não saiu em 60 s");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        filho.wait_with_output().unwrap()
    }
}

impl Drop for Casa {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn texto(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Tudo o que existe debaixo de `pasta`, com o alvo dos links.
fn arvore(pasta: &Path) -> Vec<String> {
    let mut saida = Vec::new();
    let mut pilha = vec![pasta.to_path_buf()];
    while let Some(atual) = pilha.pop() {
        for entrada in fs::read_dir(&atual).unwrap() {
            let entrada = entrada.unwrap();
            let caminho = entrada.path();
            let relativo = caminho.strip_prefix(pasta).unwrap().display().to_string();
            let tipo = entrada.file_type().unwrap();
            if tipo.is_symlink() {
                saida.push(format!("{relativo} -> {}", fs::read_link(&caminho).unwrap().display()));
            } else if tipo.is_dir() {
                saida.push(format!("{relativo}/"));
                pilha.push(caminho);
            } else {
                saida.push(relativo);
            }
        }
    }
    saida.sort();
    saida
}

#[test]
fn instalar_cria_o_layout_e_o_menu_e_reinstalar_nao_duplica() {
    let casa = Casa::nova();
    let binario = Path::new(env!("CARGO_BIN_EXE_vitral"));
    let saida = casa.rodar(binario, &["--instalar"]);
    assert!(saida.status.success(), "{}", texto(&saida.stderr));
    assert_eq!(texto(&saida.stdout), format!("Vitral {} instalado. Abra pelo menu de aplicativos.\n", versao()));

    let dados = casa.0.join(".local/share");
    let app = dados.join("vitral/app");
    assert_eq!(fs::read_link(app.join("atual")).unwrap(), Path::new(versao()));
    let instalado = app.join(versao()).join("vitral");
    let metadados = fs::metadata(&instalado).unwrap();
    assert_eq!(metadados.permissions().mode() & 0o777, 0o755);
    assert_eq!(metadados.len(), fs::metadata(binario).unwrap().len());
    let desktop = fs::read_to_string(dados.join("applications/vitral.desktop")).unwrap();
    let exec = format!("Exec={} %f", app.join("atual/vitral").display());
    assert!(desktop.lines().any(|l| l == exec), "{desktop}");
    assert!(dados.join("icons/hicolor/scalable/apps/vitral.svg").is_file());
    assert!(dados.join("icons/hicolor/32x32/apps/vitral.svg").is_file());
    assert_eq!(fs::read_link(casa.0.join(".local/bin/vitral")).unwrap(), app.join("atual/vitral"));
    let estado: serde_json::Value = serde_json::from_slice(&fs::read(app.join("estado.json")).unwrap()).unwrap();
    assert_eq!(estado["confirmar"]["versao"], versao());
    assert!(estado["formatos"][versao()].as_u64().is_some());

    // Pelo link, como o menu abre.
    let pelo_link = casa.rodar(&app.join("atual/vitral"), &["--versao"]);
    assert_eq!(texto(&pelo_link.stdout), format!("vitral {}\n", versao()));

    let antes = arvore(&casa.0);
    let de_novo = casa.rodar(binario, &["--instalar"]);
    assert!(de_novo.status.success(), "{}", texto(&de_novo.stderr));
    assert_eq!(arvore(&casa.0), antes);

    // De dentro da pasta do app, só confere o menu.
    let de_dentro = casa.rodar(&app.join("atual/vitral"), &["--instalar"]);
    assert!(de_dentro.status.success(), "{}", texto(&de_dentro.stderr));
    assert!(texto(&de_dentro.stdout).contains("já está instalado"), "{}", texto(&de_dentro.stdout));
    assert_eq!(arvore(&casa.0), antes);
}
