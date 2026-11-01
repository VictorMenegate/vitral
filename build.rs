//! No Windows, embute no vitral.exe o ícone (o do Explorer, da barra de tarefas e do Alt+Tab) e a
//! versão (Propriedades › Detalhes). O manifesto (DPI por monitor, controles comuns 6) já vem do
//! GPUI: um segundo daria recurso duplicado. Fora do Windows, nada.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // O `cfg` de um build.rs é o do sistema que compila; o Vitral do Windows só compila nele (os
    // shaders do GPUI precisam do fxc.exe).
    #[cfg(windows)]
    recursos_do_windows();
}

#[cfg(windows)]
fn recursos_do_windows() {
    use std::{env, path::PathBuf};

    println!("cargo:rerun-if-changed=assets/vitral.ico");
    let raiz = env::var("CARGO_MANIFEST_DIR").unwrap();
    // O rc aceita a barra normal, e assim o caminho não vira escape dentro das aspas.
    let icone = format!("{raiz}/assets/vitral.ico").replace('\\', "/");
    let numero = |parte: &str| env::var(format!("CARGO_PKG_VERSION_{parte}")).unwrap();
    let (maior, menor, correcao) = (numero("MAJOR"), numero("MINOR"), numero("PATCH"));
    let versao = env::var("CARGO_PKG_VERSION").unwrap();
    // Sem #include: os números do winver.h vão direto (VS_FFI_FILEFLAGSMASK, VOS_NT_WINDOWS32 e
    // VFT_APP), porque o rc fora do prompt do Visual Studio nem sempre acha os cabeçalhos. O texto
    // em UTF-8 (pragma) e a tabela em português do Brasil (0x0416), Unicode (1200).
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icone}"
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
      VALUE "InternalName", "vitral"
      VALUE "LegalCopyright", "Licença MIT"
      VALUE "OriginalFilename", "vitral.exe"
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
    let arquivo = PathBuf::from(env::var("OUT_DIR").unwrap()).join("vitral.rc");
    std::fs::write(&arquivo, rc).unwrap();
    embed_resource::compile(&arquivo, embed_resource::NONE).manifest_optional().unwrap();
}
