//! No Windows, embute no vitral.exe o ícone (o do Explorer, da barra de tarefas e do Alt+Tab) e a
//! versão (Propriedades › Detalhes). O manifesto (DPI por monitor, controles comuns 6) já vem do
//! GPUI: um segundo daria recurso duplicado. Fora do Windows, nada.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    pilha_do_windows();
    juntar_as_traducoes();
    // O `cfg` de um build.rs é o do sistema que compila; o Vitral do Windows só compila nele (os
    // shaders do GPUI precisam do fxc.exe).
    #[cfg(windows)]
    recursos_do_windows();
}

/// No Windows, a thread principal tem 1 MB de pilha (no Linux, 8 MB), e a montagem da interface
/// do GPUI é recursiva: com a árvore mais funda da 0.13.0 (as animações embrulham linhas, colunas e
/// seções), ela estourava e o Vitral fechava no primeiro clique, sem pânico e sem `queda.log`
/// (dono, 27/11/2026: "no Windows ele tá fechando sozinho quando clica ... em qualquer lugar").
/// 8 MB, como no Linux (e como o Zed faz no Windows). Pelo alvo, não pelo sistema que compila.
fn pilha_do_windows() {
    const PILHA: u32 = 8 * 1024 * 1024;
    let alvo = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let ambiente = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if alvo != "windows" {
        return;
    }
    if ambiente == "msvc" {
        println!("cargo:rustc-link-arg-bins=/STACK:{PILHA}");
    } else {
        println!("cargo:rustc-link-arg-bins=-Wl,--stack,{PILHA}");
    }
}

/// As traduções (`src/i18n.rs`): cada língua tem uma pasta `locales/<língua>/` com um JSON por
/// parte da interface ({"texto em português": "tradução"}), e aqui elas viram um JSON só por
/// língua no `OUT_DIR`, que o Vitral embute. Uma chave repetida em dois arquivos falha o build.
fn juntar_as_traducoes() {
    use std::{collections::BTreeMap, env, fs, path::PathBuf};
    let raiz = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("locales");
    println!("cargo:rerun-if-changed=locales");
    let saida = PathBuf::from(env::var("OUT_DIR").unwrap());
    for lingua in ["en", "es"] {
        let pasta = raiz.join(lingua);
        println!("cargo:rerun-if-changed={}", pasta.display());
        let mut arquivos: Vec<PathBuf> = fs::read_dir(&pasta)
            .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect())
            .unwrap_or_default();
        arquivos.sort();
        let mut tudo: BTreeMap<String, String> = BTreeMap::new();
        for arquivo in arquivos {
            println!("cargo:rerun-if-changed={}", arquivo.display());
            let texto = fs::read_to_string(&arquivo).unwrap_or_else(|e| panic!("{}: {e}", arquivo.display()));
            let parte: BTreeMap<String, String> = parse_json_simples(&texto).unwrap_or_else(|e| panic!("{}: {e}", arquivo.display()));
            for (chave, valor) in parte {
                if let Some(antes) = tudo.get(&chave)
                    && *antes != valor
                {
                    panic!("{}: \"{chave}\" já foi traduzido de outro jeito em outro arquivo", arquivo.display());
                }
                tudo.insert(chave, valor);
            }
        }
        let json = format!(
            "{{{}}}",
            tudo.iter().map(|(k, v)| format!("{}:{}", aspas(k), aspas(v))).collect::<Vec<_>>().join(",")
        );
        fs::write(saida.join(format!("{lingua}.json")), json).unwrap();
    }
}

/// Um JSON de um objeto só, com chaves e valores em texto (sem dependência no build).
fn parse_json_simples(texto: &str) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut chars = texto.trim().chars().peekable();
    let mut mapa = std::collections::BTreeMap::new();
    let pular = |c: &mut std::iter::Peekable<std::str::Chars>| {
        while c.peek().is_some_and(|x| x.is_whitespace()) {
            c.next();
        }
    };
    let ler_texto = |c: &mut std::iter::Peekable<std::str::Chars>| -> Result<String, String> {
        if c.next() != Some('"') {
            return Err("esperava aspas".into());
        }
        let mut s = String::new();
        loop {
            match c.next().ok_or("texto sem fim")? {
                '"' => return Ok(s),
                '\\' => match c.next().ok_or("escape sem fim")? {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    'u' => {
                        let hex: String = (0..4).filter_map(|_| c.next()).collect();
                        let n = u32::from_str_radix(&hex, 16).map_err(|e| e.to_string())?;
                        s.push(char::from_u32(n).ok_or("unicode inválido")?);
                    }
                    outro => s.push(outro),
                },
                outro => s.push(outro),
            }
        }
    };
    pular(&mut chars);
    if chars.next() != Some('{') {
        return Err("esperava {".into());
    }
    loop {
        pular(&mut chars);
        if chars.peek() == Some(&'}') {
            break;
        }
        let chave = ler_texto(&mut chars)?;
        pular(&mut chars);
        if chars.next() != Some(':') {
            return Err(format!("esperava : depois de {chave}"));
        }
        pular(&mut chars);
        let valor = ler_texto(&mut chars)?;
        mapa.insert(chave, valor);
        pular(&mut chars);
        match chars.next() {
            Some(',') => continue,
            Some('}') => break,
            _ => return Err("esperava , ou }".into()),
        }
    }
    Ok(mapa)
}

fn aspas(texto: &str) -> String {
    let mut s = String::from("\"");
    for c in texto.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\t' => s.push_str("\\t"),
            outro if (outro as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", outro as u32)),
            outro => s.push(outro),
        }
    }
    s.push('"');
    s
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
