//! `~/.config/vitral/atualizacoes.json`: o modo (Automáticas, Só avisar, Desligadas), a última
//! procura que deu certo, o ETag do manifesto e as falhas seguidas de cada versão. Quem grava é só
//! a thread da atualização. E quando procurar: 60 s depois de abrir, a cada 6 h com folga, nunca
//! antes de 1 h da última procura que deu certo, e 1 h depois de um erro de rede.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use super::pacote::gravar_atomico;

pub const PRIMEIRA: Duration = Duration::from_secs(60);
pub const INTERVALO: Duration = Duration::from_secs(6 * 3600);
/// Folga aleatória somada ao intervalo (para não procurar sempre no mesmo minuto).
pub const FOLGA: Duration = Duration::from_secs(10 * 60);
/// Abrir e fechar o Vitral várias vezes não vira várias procuras.
pub const ENTRE_PROCURAS: Duration = Duration::from_secs(3600);
pub const DEPOIS_DE_ERRO: Duration = Duration::from_secs(3600);
/// Na terceira falha seguida de uma versão (arquivo diferente, binário que não abre), para de
/// tentar até sair outra.
pub const DESISTE_DEPOIS: u32 = 3;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modo {
    /// Baixa e instala sozinho; vale quando reiniciar.
    #[default]
    Automaticas,
    /// Avisa da versão nova; a pessoa escolhe quando instalar.
    SoAvisar,
    /// Não procura sozinho.
    Desligadas,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub modo: Modo,
    /// Segundos desde 1970 da última procura que conseguiu ler o manifesto.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ultima_procura: Option<u64>,
    /// ETag do manifesto da última procura que não tinha nada a fazer, e a versão que procurou
    /// (outra versão rodando não confia no "não mudou").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag_de: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub falhas: BTreeMap<String, u32>,
    /// A versão que a pessoa pediu para não avisar mais.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignorada: Option<String>,
}

impl Config {
    /// Sem arquivo ou ilegível: o padrão (Automáticas, nunca procurou).
    pub fn ler(caminho: &Path) -> Config {
        fs::read(caminho).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn gravar(&self, caminho: &Path) -> io::Result<()> {
        let mut bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        bytes.push(b'\n');
        gravar_atomico(caminho, &bytes, 0o644)
    }
}

pub fn caminho() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("vitral").join("atualizacoes.json"))
}

/// Quanto esperar pela primeira procura desta abertura: 60 s, ou o que falta para completar 1 h
/// da última que deu certo.
pub fn primeira_espera(agora: u64, ultima: Option<u64>) -> Duration {
    let falta = ultima.map_or(Duration::ZERO, |ultima| {
        ENTRE_PROCURAS.saturating_sub(Duration::from_secs(agora.saturating_sub(ultima)))
    });
    falta.max(PRIMEIRA)
}

/// Depois de uma procura: 6 h mais a folga, ou 1 h se a rede falhou.
pub fn espera_depois(erro_de_rede: bool, folga: Duration) -> Duration {
    if erro_de_rede { DEPOIS_DE_ERRO } else { INTERVALO + folga.min(FOLGA) }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::atualizacao::pacote::testes::pasta_de_teste;

    #[test]
    fn grava_e_le_o_formato_da_especificacao() {
        let pasta = pasta_de_teste("config");
        let caminho = pasta.join("vitral/atualizacoes.json");
        assert_eq!(Config::ler(&caminho), Config::default());
        assert_eq!(Config::default().modo, Modo::Automaticas);
        let config = Config {
            modo: Modo::SoAvisar,
            ultima_procura: Some(1793400000),
            etag: Some("\"abc\"".into()),
            etag_de: Some("0.2.0".into()),
            falhas: BTreeMap::from([("0.3.0".into(), 1)]),
            ignorada: None,
        };
        config.gravar(&caminho).unwrap();
        assert_eq!(Config::ler(&caminho), config);
        let valor: serde_json::Value = serde_json::from_slice(&fs::read(&caminho).unwrap()).unwrap();
        assert_eq!(valor["modo"], "so_avisar");
        assert_eq!(valor["falhas"]["0.3.0"], 1);
        assert!(valor.get("ignorada").is_none());
        let desligadas: Config = serde_json::from_str(r#"{"modo":"desligadas"}"#).unwrap();
        assert_eq!(desligadas.modo, Modo::Desligadas);
        fs::write(&caminho, "{ quebrado").unwrap();
        assert_eq!(Config::ler(&caminho), Config::default());
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn nunca_procura_duas_vezes_na_mesma_hora() {
        let agora = 1_800_000_000;
        assert_eq!(primeira_espera(agora, None), PRIMEIRA);
        assert_eq!(primeira_espera(agora, Some(agora - 2 * 3600)), PRIMEIRA);
        assert_eq!(primeira_espera(agora, Some(agora - 3600)), PRIMEIRA);
        assert_eq!(primeira_espera(agora, Some(agora - 20 * 60)), Duration::from_secs(40 * 60));
        assert_eq!(primeira_espera(agora, Some(agora - 3590)), PRIMEIRA);
        // Relógio que voltou: a última "no futuro" conta como agora.
        assert_eq!(primeira_espera(agora, Some(agora + 600)), ENTRE_PROCURAS);
    }

    #[test]
    fn seis_horas_com_folga_ou_uma_hora_depois_de_erro() {
        assert_eq!(espera_depois(false, Duration::ZERO), INTERVALO);
        assert_eq!(espera_depois(false, Duration::from_secs(300)), INTERVALO + Duration::from_secs(300));
        assert_eq!(espera_depois(false, Duration::from_secs(99_999)), INTERVALO + FOLGA);
        assert_eq!(espera_depois(true, Duration::from_secs(300)), DEPOIS_DE_ERRO);
    }
}
