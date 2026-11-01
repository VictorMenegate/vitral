//! No Windows, embute no lançador o ícone (é o do atalho do Menu Iniciar, do pino da barra e de
//! Aplicativos instalados, que apontam para ele), a versão e o manifesto: `asInvoker` (nunca pede
//! administrador), a caixa de mensagem com o visual atual (controles comuns 6) e nítida em qualquer
//! escala (DPI por monitor). O vitral.exe recebe o manifesto do GPUI; o lançador não tem GPUI. Fora
//! do Windows, nada.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // O link.exe grava a hora da compilação no cabeçalho do .exe; com o /Brepro, um hash do
    // conteúdo. Assim o lançador só muda quando o código (ou o Rust) muda, e o atualizador só o
    // troca nesse caso.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/Brepro");
    }
    // O `cfg` de um build.rs é o do sistema que compila (o rc é o do Windows SDK).
    #[cfg(windows)]
    recursos_do_windows();
}

#[cfg(windows)]
fn recursos_do_windows() {
    use std::{env, fs, path::PathBuf};

    println!("cargo:rerun-if-changed=../assets/vitral.ico");
    let raiz = env::var("CARGO_MANIFEST_DIR").unwrap();
    let saida = PathBuf::from(env::var("OUT_DIR").unwrap());
    // O rc aceita a barra normal, e assim o caminho não vira escape dentro das aspas.
    let icone = format!("{raiz}/../assets/vitral.ico").replace('\\', "/");
    let numero = |parte: &str| env::var(format!("CARGO_PKG_VERSION_{parte}")).unwrap();
    let (maior, menor, correcao) = (numero("MAJOR"), numero("MINOR"), numero("PATCH"));
    let versao = env::var("CARGO_PKG_VERSION").unwrap();
    let manifesto = saida.join("lancador.manifest");
    fs::write(
        &manifesto,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="VictorMenegate.Vitral.Lancador" version="{maior}.{menor}.{correcao}.0"/>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application><supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}"/></application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#
        ),
    )
    .unwrap();
    let manifesto = manifesto.display().to_string().replace('\\', "/");
    // Os números do winver.h e do winuser.h direto (RT_MANIFEST = 24, VOS_NT_WINDOWS32, VFT_APP),
    // como no build.rs do app. O texto em UTF-8 (pragma) e a tabela em português do Brasil
    // (0x0416), Unicode (1200).
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icone}"
1 24 "{manifesto}"
1 VERSIONINFO
FILEVERSION {maior},{menor},{correcao},0
PRODUCTVERSION {maior},{menor},{correcao},0
FILEFLAGSMASK 0x3F
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "041604B0"
    BEGIN
      VALUE "CompanyName", "VictorMenegate"
      VALUE "FileDescription", "Vitral"
      VALUE "FileVersion", "{versao}"
      VALUE "InternalName", "vitral-lancador"
      VALUE "LegalCopyright", "Licença MIT"
      VALUE "OriginalFilename", "Vitral.exe"
      VALUE "ProductName", "Vitral"
      VALUE "ProductVersion", "{versao}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0416, 1200
  END
END
"#
    );
    let arquivo = saida.join("lancador.rc");
    fs::write(&arquivo, rc).unwrap();
    embed_resource::compile(&arquivo, embed_resource::NONE).manifest_required().unwrap();
}
