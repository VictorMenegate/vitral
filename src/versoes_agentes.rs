//! As versões dos agentes instalados (Claude Code e Codex) e a mais nova publicada de cada um,
//! para o grupo **Instalados** das Configurações. A publicada vem das etiquetas do pacote de cada
//! um no npm (`dist-tags`), que acompanham o instalador nativo; é uma consulta curta, sem nenhum
//! dado do usuário, feita só quando a seção Agentes abre e guardada por uma hora. Com a
//! atualização do Vitral desligada pelo ambiente (`VITRAL_SEM_ATUALIZACAO=1`, a bancada), nada é
//! consultado.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::claude::Versao;

const ETIQUETAS_DO_CLAUDE: &str = "https://registry.npmjs.org/-/package/@anthropic-ai/claude-code/dist-tags";
const ETIQUETAS_DO_CODEX: &str = "https://registry.npmjs.org/-/package/@openai/codex/dist-tags";
/// As etiquetas do Codex passam de 12 KB (uma por plataforma e por alfa).
const LIMITE_DA_RESPOSTA: u64 = 256 * 1024;
/// Por quanto tempo uma consulta vale.
const VALE_POR: Duration = Duration::from_secs(60 * 60);

/// A primeira palavra com cara de versão num texto: "codex-cli 0.169.0" e "2.1.328 (Claude Code)"
/// dão `(0, 169, 0)` e `(2, 1, 328)`. Pré-lançamento ("0.173.0-alpha.4") não conta.
pub fn versao_no_texto(texto: &str) -> Option<Versao> {
    texto.split_whitespace().find_map(crate::claude::versao_de)
}

/// A versão que a etiqueta `canal` aponta no `dist-tags` de um pacote do npm.
pub fn publicada_em(json: &str, canal: &str) -> Option<Versao> {
    let valor: serde_json::Value = serde_json::from_str(json).ok()?;
    crate::claude::versao_de(valor.get(canal)?.as_str()?)
}

/// O canal de atualização do Claude Code (`autoUpdatesChannel` do `settings.json` dele): quem
/// escolheu "stable" compara com a estável; o resto, com a mais nova.
pub fn canal_do_claude(settings: Option<&str>) -> &'static str {
    let estavel = settings
        .and_then(|texto| serde_json::from_str::<serde_json::Value>(texto).ok())
        .is_some_and(|v| v.get("autoUpdatesChannel").and_then(|c| c.as_str()) == Some("stable"));
    if estavel { "stable" } else { "latest" }
}

/// A versão nova a mostrar: só quando as duas são conhecidas e a publicada é maior.
pub fn nova(instalada: Option<Versao>, publicada: Option<Versao>) -> Option<Versao> {
    publicada.filter(|p| instalada.is_some_and(|i| *p > i))
}

pub fn texto(v: Versao) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

/// A frase da versão nova do Claude Code, embaixo da linha dele (o Atualizar está ao lado).
pub fn frase_da_nova_do_claude(instalada: Option<Versao>, publicada: Option<Versao>) -> Option<String> {
    frase_da_nova(instalada, publicada)
}

/// A frase da versão nova de qualquer agente com o botão Atualizar ao lado.
pub fn frase_da_nova(instalada: Option<Versao>, publicada: Option<Versao>) -> Option<String> {
    nova(instalada, publicada).map(|n| format!("Há a {}. O Atualizar instala.", texto(n)))
}

/// O que se sabe das versões publicadas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Publicadas {
    pub claude: Option<Versao>,
    pub codex: Option<Versao>,
}

static GUARDADAS: Mutex<Option<(Instant, Publicadas)>> = Mutex::new(None);

/// A última consulta, se ainda vale (não consulta nada).
pub fn guardadas() -> Option<Publicadas> {
    let guardadas = GUARDADAS.lock().unwrap_or_else(|e| e.into_inner());
    guardadas.filter(|(quando, _)| quando.elapsed() < VALE_POR).map(|(_, p)| p)
}

/// Consulta as versões publicadas (bloqueia: chamar em segundo plano) e guarda. `VITRAL_AGENTES_PUBLICADOS`
/// ("claude=2.1.400,codex=0.200.0") responde no lugar da rede: é como a bancada mostra o "há a X".
pub fn consultar() -> Publicadas {
    if let Some(guardadas) = guardadas() {
        return guardadas;
    }
    let publicadas = match std::env::var("VITRAL_AGENTES_PUBLICADOS") {
        Ok(falsas) => do_ambiente(&falsas),
        Err(_) if std::env::var_os("VITRAL_SEM_ATUALIZACAO").is_some_and(|v| v == "1") => Publicadas::default(),
        Err(_) => da_rede(),
    };
    *GUARDADAS.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), publicadas));
    publicadas
}

fn do_ambiente(texto: &str) -> Publicadas {
    let de = |nome: &str| {
        texto.split(',').find_map(|par| par.trim().strip_prefix(nome)?.strip_prefix('=')).and_then(crate::claude::versao_de)
    };
    Publicadas { claude: de("claude"), codex: de("codex") }
}

fn da_rede() -> Publicadas {
    use crate::atualizacao::rede::{Rede, Resposta};
    let rede = Rede::nova(&format!("Vitral/{}", crate::atualizacao::VERSAO));
    let etiquetas = |url: &str| match rede.texto(url, LIMITE_DA_RESPOSTA, None) {
        Ok(Resposta::Dados { bytes, .. }) => String::from_utf8(bytes).ok(),
        _ => None,
    };
    let settings = dirs::home_dir().and_then(|casa| std::fs::read_to_string(casa.join(".claude").join("settings.json")).ok());
    let canal = canal_do_claude(settings.as_deref());
    Publicadas {
        claude: etiquetas(ETIQUETAS_DO_CLAUDE).and_then(|json| publicada_em(&json, canal)),
        codex: etiquetas(ETIQUETAS_DO_CODEX).and_then(|json| publicada_em(&json, "latest")),
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn a_versao_sai_do_que_cada_agente_imprime() {
        assert_eq!(versao_no_texto("codex-cli 0.169.0\n"), Some((0, 169, 0)));
        assert_eq!(versao_no_texto("2.1.328 (Claude Code)"), Some((2, 1, 328)));
        assert_eq!(versao_no_texto("codex-cli 0.173.0-alpha.4"), None);
        assert_eq!(versao_no_texto("command not found"), None);
    }

    #[test]
    fn a_publicada_vem_da_etiqueta_do_canal() {
        let claude = r#"{"stable":"2.1.323","latest":"2.1.330","next":"2.1.330"}"#;
        assert_eq!(publicada_em(claude, "latest"), Some((2, 1, 330)));
        assert_eq!(publicada_em(claude, "stable"), Some((2, 1, 323)));
        let codex = r#"{"beta":"0.1.2505172116","alpha":"0.173.0-alpha.4","latest":"0.171.0"}"#;
        assert_eq!(publicada_em(codex, "latest"), Some((0, 171, 0)));
        // Etiqueta que falta, pré-lançamento e resposta que não é JSON: sem versão, sem pânico.
        assert_eq!(publicada_em(codex, "stable"), None);
        assert_eq!(publicada_em(codex, "alpha"), None);
        assert_eq!(publicada_em("<html>503</html>", "latest"), None);
    }

    #[test]
    fn o_canal_do_claude_segue_o_que_a_pessoa_escolheu() {
        assert_eq!(canal_do_claude(Some(r#"{"autoUpdatesChannel":"stable"}"#)), "stable");
        assert_eq!(canal_do_claude(Some(r#"{"autoUpdatesChannel":"latest"}"#)), "latest");
        assert_eq!(canal_do_claude(Some("{}")), "latest");
        assert_eq!(canal_do_claude(None), "latest");
    }

    #[test]
    fn so_ha_versao_nova_quando_a_publicada_e_maior_que_a_instalada() {
        assert_eq!(nova(Some((2, 1, 328)), Some((2, 1, 330))), Some((2, 1, 330)));
        assert_eq!(nova(Some((2, 1, 330)), Some((2, 1, 330))), None);
        // Instalada adiante da publicada (canal de teste): nada a dizer.
        assert_eq!(nova(Some((2, 2, 0)), Some((2, 1, 330))), None);
        // Sem uma das duas, não se inventa.
        assert_eq!(nova(None, Some((2, 1, 330))), None);
        assert_eq!(nova(Some((2, 1, 328)), None), None);
        // 0.9 < 0.10: compara número, não texto.
        assert_eq!(nova(Some((0, 9, 0)), Some((0, 10, 0))), Some((0, 10, 0)));
    }

    #[test]
    fn a_frase_do_claude_so_aparece_com_versao_nova() {
        assert_eq!(frase_da_nova_do_claude(Some((2, 1, 328)), Some((2, 1, 330))).as_deref(), Some("Há a 2.1.330. O Atualizar instala."));
        assert_eq!(frase_da_nova_do_claude(Some((2, 1, 330)), Some((2, 1, 330))), None);
        assert_eq!(frase_da_nova_do_claude(None, Some((2, 1, 330))), None);
    }

    #[test]
    fn a_bancada_responde_no_lugar_da_rede() {
        assert_eq!(do_ambiente("claude=2.1.400, codex=0.200.0"), Publicadas { claude: Some((2, 1, 400)), codex: Some((0, 200, 0)) });
        assert_eq!(do_ambiente("codex=0.200.0"), Publicadas { claude: None, codex: Some((0, 200, 0)) });
    }
}
