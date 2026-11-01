//! O `app/estado.json` da instalação gerenciada ([`EstadoApp`]). Não depende do sistema: a saúde
//! decide com ele em qualquer um; quem instala é o `linux.rs`.

use std::{collections::BTreeMap, fs, io, path::Path};

use serde::{Deserialize, Serialize};

use super::pacote::gravar_atomico;

/// `app/estado.json`: a versão que ainda falta confirmar (`saude.rs`), as recusadas, o formato dos
/// dados de cada versão instalada (a volta só vai para uma que lê o disco) e a volta que a
/// anterior ainda vai contar ao abrir.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EstadoApp {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmar: Option<Confirmar>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recusadas: Vec<String>,
    pub formatos: BTreeMap<String, u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voltou: Option<Volta>,
}

/// Uma volta de versão: a recusada, a que ficou e se foi a saúde (ou a pessoa, nas Configurações).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Volta {
    pub de: String,
    pub para: String,
    #[serde(default)]
    pub automatica: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Confirmar {
    pub versao: String,
    #[serde(default)]
    pub aberturas: Vec<Abertura>,
}

/// Uma abertura que ainda não confirmou (sessão do PC e quando).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Abertura {
    pub sessao: String,
    pub quando: u64,
}

impl EstadoApp {
    /// Sem arquivo (ou ilegível) = nada a confirmar.
    pub fn ler(app: &Path) -> EstadoApp {
        fs::read(app.join("estado.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn gravar(&self, app: &Path) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        gravar_atomico(&app.join("estado.json"), &bytes, 0o644)
    }
}
