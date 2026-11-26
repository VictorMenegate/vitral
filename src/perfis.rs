//! Perfis de agente: um nome para um conjunto de escolhas da caixa de mensagem (quem responde, o
//! modelo, o modo de permissão e o esforço), mais uma nota de quando usar. Saíram do app no
//! redesenho Linear (dono, 25/11/2026: "tira os perfis"): sem a tela nas Configurações e sem o
//! grupo no seletor de modelo. Os que existem ficam no `config.json` (`Config.perfis`), para não
//! se perderem se voltarem.

use serde::{Deserialize, Serialize};

use crate::modelo::Agente;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Perfil {
    pub id: String,
    pub nome: String,
    /// Quem responde.
    pub agente: Agente,
    /// O modelo do agente (a família ou a versão exata do Claude, o slug do Codex); vazio é o
    /// padrão do agente.
    pub modelo: String,
    /// O modo de permissão (`manual`, `acceptEdits`, `plan`…); vazio é o das Configurações.
    pub modo: String,
    /// O esforço (`low`…`max`); vazio é o automático.
    pub esforco: String,
    /// Quando usar: a segunda linha no seletor.
    pub nota: String,
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::modelo::Config;

    #[test]
    fn config_antiga_abre_sem_perfis_e_a_nova_guarda_todos_os_campos() {
        let antiga: Config = serde_json::from_str(r#"{"permissao":"manual"}"#).unwrap();
        assert!(antiga.perfis.is_empty());
        let mut config = Config::default();
        let revisor = Perfil {
            id: "a1".into(),
            nome: "Revisor".into(),
            agente: Agente::Codex,
            modelo: "gpt-5.1-codex".into(),
            modo: "plan".into(),
            esforco: "high".into(),
            nota: "Para rever um diff antes do merge".into(),
        };
        config.perfis.push(revisor.clone());
        let volta: Config = serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(volta.perfis, vec![revisor]);
    }
}
