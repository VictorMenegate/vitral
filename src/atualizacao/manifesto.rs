//! O `vitral-versao.json` de cada release (`pacote/manifesto.py`): lido só depois de a assinatura
//! conferir, validado campo a campo, e a decisão de instalar (só uma versão maior; pré-lançamento
//! só no canal de teste).

use std::collections::BTreeMap;

use anyhow::{Context as _, Result, bail, ensure};
use semver::Version;
use serde::Deserialize;

/// O formato do manifesto que este app entende. Um maior só avisa (instalar pela página).
pub const FORMATO: u64 = 1;
pub const LIMITE_MANIFESTO: u64 = 64 * 1024;
pub const LIMITE_ASSINATURA: u64 = 4 * 1024;
/// O maior pacote que o app baixa (o do Linux tem uns 20 MB).
pub const LIMITE_PACOTE: u64 = 200 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Manifesto {
    formato: u64,
    app: String,
    versao: String,
    pagina: String,
    formato_dados: u32,
    #[serde(default)]
    notas: String,
    arquivos: BTreeMap<String, Arquivo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Arquivo {
    pub nome: String,
    pub tamanho: u64,
    pub sha256: String,
}

/// A versão publicada, com o pacote desta plataforma.
#[derive(Debug, Clone, PartialEq)]
pub struct Publicada {
    pub versao: Version,
    pub notas: String,
    pub pagina: String,
    pub formato_dados: u32,
    pub pacote: Arquivo,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Lido {
    Versao(Publicada),
    /// Manifesto de um formato que este app não entende: só dá para avisar.
    FormatoNovo { versao: Version, pagina: String },
}

/// Valida o manifesto (já com a assinatura conferida; `comentario` é o comentário confiável dela,
/// "vitral <versão> vitral-versao.json").
pub fn ler(bytes: &[u8], comentario: &str, plataforma: &str) -> Result<Lido> {
    let valor: serde_json::Value = serde_json::from_slice(bytes).context("o manifesto não é JSON")?;
    ensure!(valor["app"] == "vitral", "o manifesto não é do Vitral");
    let texto = valor["versao"].as_str().context("o manifesto não diz a versão")?;
    let versao = Version::parse(texto).with_context(|| format!("versão inválida: {texto}"))?;
    ensure!(versao.build.is_empty(), "versão com metadado de build: {texto}");
    ensure!(
        comentario == format!("vitral {texto} vitral-versao.json"),
        "a assinatura é de outro arquivo ({comentario})"
    );
    let formato = valor["formato"].as_u64().context("o manifesto não diz o formato")?;
    if formato > FORMATO {
        let pagina = valor["pagina"].as_str().unwrap_or_default().to_string();
        return Ok(Lido::FormatoNovo { versao, pagina });
    }
    let manifesto: Manifesto = serde_json::from_value(valor).context("falta campo no manifesto")?;
    ensure!(manifesto.formato == FORMATO && manifesto.app == "vitral", "formato {} do manifesto", manifesto.formato);
    let Some(pacote) = manifesto.arquivos.get(plataforma) else {
        bail!("a {versao} não tem pacote para {plataforma}");
    };
    let nome = format!("vitral-{versao}-{plataforma}.tar.gz");
    ensure!(pacote.nome == nome, "nome fora do padrão: {}", pacote.nome);
    ensure!(
        pacote.sha256.len() == 64 && pacote.sha256.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "SHA-256 inválido"
    );
    ensure!(pacote.tamanho > 0, "pacote vazio");
    ensure!(
        pacote.tamanho <= LIMITE_PACOTE,
        "o pacote tem {} MB (o limite é {} MB)",
        pacote.tamanho / (1024 * 1024),
        LIMITE_PACOTE / (1024 * 1024)
    );
    Ok(Lido::Versao(Publicada {
        pacote: pacote.clone(),
        versao,
        notas: manifesto.notas,
        pagina: manifesto.pagina,
        formato_dados: manifesto.formato_dados,
    }))
}

/// Instalar (ou avisar) a `publicada`? Só se é maior que a que roda, não é pré-lançamento (fora do
/// canal de teste), não foi recusada (a volta da saúde) nem ignorada pela pessoa.
pub fn e_nova(rodando: &Version, publicada: &Version, aceita_pre: bool, recusadas: &[String], ignorada: Option<&str>) -> bool {
    let texto = publicada.to_string();
    publicada > rodando
        && (aceita_pre || publicada.pre.is_empty())
        && !recusadas.contains(&texto)
        && ignorada != Some(texto.as_str())
}

#[cfg(test)]
mod testes {
    use super::*;

    const LINUX: &str = "linux-x86_64";
    const PONTA: &[u8] = include_bytes!("teste/ponta/vitral-versao.json");

    fn manifesto(mudar: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
        let mut valor: serde_json::Value = serde_json::from_slice(PONTA).unwrap();
        mudar(&mut valor);
        serde_json::to_vec(&valor).unwrap()
    }

    fn ler_09(bytes: &[u8]) -> Result<Lido> {
        ler(bytes, "vitral 0.9.0 vitral-versao.json", LINUX)
    }

    fn v(texto: &str) -> Version {
        Version::parse(texto).unwrap()
    }

    #[test]
    fn le_o_manifesto_que_o_release_gera() {
        let Lido::Versao(publicada) = ler_09(PONTA).unwrap() else { panic!() };
        assert_eq!(publicada.versao, v("0.9.0"));
        assert_eq!(publicada.pagina, "https://github.com/VictorMenegate/vitral/releases/tag/v0.9.0");
        assert_eq!(publicada.formato_dados, 1);
        assert_eq!(publicada.notas, "Versão de teste do atualizador.");
        assert_eq!(publicada.pacote.nome, "vitral-0.9.0-linux-x86_64.tar.gz");
        assert_eq!(publicada.pacote.tamanho, include_bytes!("teste/ponta/vitral-0.9.0-linux-x86_64.tar.gz").len() as u64);
        // O de antes (lote 2) também se lê.
        let antigo = include_bytes!("teste/vitral-versao.json");
        assert!(ler(antigo, "vitral 0.2.0-rc.1 vitral-versao.json", LINUX).is_ok());
    }

    #[test]
    fn recusa_manifesto_com_campo_faltando_ou_errado() {
        let casos: Vec<(&str, Vec<u8>)> = vec![
            ("sem versão", manifesto(|m| drop(m.as_object_mut().unwrap().remove("versao")))),
            ("sem arquivos", manifesto(|m| drop(m.as_object_mut().unwrap().remove("arquivos")))),
            ("sem página", manifesto(|m| drop(m.as_object_mut().unwrap().remove("pagina")))),
            ("sem formato dos dados", manifesto(|m| drop(m.as_object_mut().unwrap().remove("formato_dados")))),
            ("sem soma", manifesto(|m| drop(m["arquivos"][LINUX].as_object_mut().unwrap().remove("sha256")))),
            ("outro app", manifesto(|m| m["app"] = "outro".into())),
            ("formato zero", manifesto(|m| m["formato"] = 0.into())),
            ("versão inválida", manifesto(|m| m["versao"] = "nove".into())),
            ("sem Linux", manifesto(|m| drop(m["arquivos"].as_object_mut().unwrap().remove(LINUX)))),
            ("nome de outra versão", manifesto(|m| m["arquivos"][LINUX]["nome"] = "vitral-0.8.0-linux-x86_64.tar.gz".into())),
            ("nome com caminho", manifesto(|m| m["arquivos"][LINUX]["nome"] = "../vitral-0.9.0-linux-x86_64.tar.gz".into())),
            ("nome de outro tipo", manifesto(|m| m["arquivos"][LINUX]["nome"] = "vitral-0.9.0-linux-x86_64.zip".into())),
            ("soma curta", manifesto(|m| m["arquivos"][LINUX]["sha256"] = "abc".into())),
            ("soma maiúscula", manifesto(|m| m["arquivos"][LINUX]["sha256"] = "F".repeat(64).into())),
            ("tamanho zero", manifesto(|m| m["arquivos"][LINUX]["tamanho"] = 0.into())),
            ("tamanho negativo", manifesto(|m| m["arquivos"][LINUX]["tamanho"] = (-1).into())),
            ("mais de 200 MB", manifesto(|m| m["arquivos"][LINUX]["tamanho"] = (LIMITE_PACOTE + 1).into())),
            ("não é JSON", b"<html>".to_vec()),
        ];
        for (nome, bytes) in casos {
            assert!(ler_09(&bytes).is_err(), "{nome} passou");
        }
        let no_limite = manifesto(|m| m["arquivos"][LINUX]["tamanho"] = LIMITE_PACOTE.into());
        assert!(ler_09(&no_limite).is_ok());
    }

    #[test]
    fn a_assinatura_tem_de_ser_deste_arquivo_e_desta_versao() {
        assert!(ler(PONTA, "vitral 0.9.0 SHA256SUMS", LINUX).is_err());
        assert!(ler(PONTA, "vitral 0.8.0 vitral-versao.json", LINUX).is_err());
        assert!(ler(PONTA, "", LINUX).is_err());
        assert!(ler(PONTA, "vitral 0.9.0 vitral-versao.json", "windows-x86_64").is_err());
    }

    /// O manifesto com o pacote do Windows (desde o Lote W3): cada sistema lê só o dele, e o Linux lê
    /// igual a antes (a 0.3.0-rc.5 instalada lê com este mesmo `ler`). Todos os pacotes precisam da
    /// mesma forma: um campo faltando no de outro sistema recusa o manifesto inteiro.
    #[test]
    fn o_pacote_do_windows_nao_muda_o_que_o_linux_le() {
        let windows = "windows-x86_64";
        let com_windows = manifesto(|m| {
            m["arquivos"][windows] =
                serde_json::json!({"nome": "vitral-0.9.0-windows-x86_64.tar.gz", "tamanho": 25_000_000, "sha256": "ab".repeat(32)});
        });
        assert_eq!(ler_09(&com_windows).unwrap(), ler_09(PONTA).unwrap());
        let Lido::Versao(publicada) = ler(&com_windows, "vitral 0.9.0 vitral-versao.json", windows).unwrap() else { panic!() };
        assert_eq!(publicada.pacote, Arquivo { nome: "vitral-0.9.0-windows-x86_64.tar.gz".into(), tamanho: 25_000_000, sha256: "ab".repeat(32) });
        let torto = manifesto(|m| m["arquivos"][windows] = serde_json::json!({"nome": "vitral-0.9.0-windows-x86_64.tar.gz"}));
        assert!(ler_09(&torto).is_err());
    }

    #[test]
    fn formato_novo_so_avisa() {
        let novo = manifesto(|m| {
            m["formato"] = 2.into();
            drop(m.as_object_mut().unwrap().remove("arquivos"));
        });
        let lido = ler_09(&novo).unwrap();
        assert_eq!(
            lido,
            Lido::FormatoNovo { versao: v("0.9.0"), pagina: "https://github.com/VictorMenegate/vitral/releases/tag/v0.9.0".into() }
        );
    }

    #[test]
    fn so_instala_versao_maior_e_estavel() {
        let nada: &[String] = &[];
        assert!(e_nova(&v("0.2.0"), &v("0.3.0"), false, nada, None));
        assert!(e_nova(&v("0.2.0"), &v("0.2.1"), false, nada, None));
        assert!(!e_nova(&v("0.3.0"), &v("0.3.0"), false, nada, None), "igual não instala");
        assert!(!e_nova(&v("0.3.0"), &v("0.2.9"), false, nada, None), "menor não instala");
        assert!(!e_nova(&v("0.2.0"), &v("0.3.0-rc.1"), false, nada, None), "pré-lançamento só no canal de teste");
        assert!(e_nova(&v("0.2.0"), &v("0.3.0-rc.1"), true, nada, None));
        assert!(e_nova(&v("0.3.0-rc.1"), &v("0.3.0-rc.2"), true, nada, None));
        // Do pré-lançamento para a versão dele, e do build de desenvolvimento para o release.
        assert!(e_nova(&v("0.2.0-rc.1"), &v("0.2.0"), false, nada, None));
        assert!(e_nova(&v("0.3.0-dev.5109ec3aa0"), &v("0.3.0"), false, nada, None));
        assert!(!e_nova(&v("0.2.0"), &v("0.3.0"), false, &["0.3.0".to_string()], None), "recusada");
        assert!(!e_nova(&v("0.2.0"), &v("0.3.0"), false, nada, Some("0.3.0")), "ignorada");
        assert!(e_nova(&v("0.2.0"), &v("0.3.1"), false, &["0.3.0".to_string()], Some("0.3.0")));
    }
}
