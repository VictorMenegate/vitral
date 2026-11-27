//! Contas do Codex (26/11/2026, no jeito do Zerosub do Paseo; dono: "coloca pra dar pra usar mais
//! contas nos outros agentes também"). A principal é o login normal do Codex (`$CODEX_HOME` ou
//! `~/.codex`). Cada conta extra tem uma pasta própria usada como `CODEX_HOME`, onde só o login
//! (`auth.json`) é dela: a configuração, as instruções, os prompts, as skills e as sessões são
//! links para a principal, e o `CODEX_SQLITE_HOME` aponta para ela, então as conversas aparecem em
//! todas as contas. No Windows os links são junções e hard links, sem pedir administrador
//! (`ligacoes`).
//!
//! O Codex não continua uma conversa noutra conta (o que ele guarda da conversa vai cifrado para
//! a conta): trocar a conta de um chat do Codex abre uma sessão nova, com a conversa até ali num
//! arquivo anexado à próxima mensagem (`Motor::definir_conta_codex`).
//!
//! O cadastro fica em `~/.config/vitral/contas-codex.json`; as pastas em `contas-codex/<id>`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// Id reservado da conta principal (o login normal do Codex).
pub const PRINCIPAL: &str = "principal";

/// O que fica de cada conta principal ligado nas extras: tudo menos o login.
const LIGADOS: [&str; 9] = ["config.toml", "AGENTS.md", "prompts", "skills", "rules", "sessions", "archived_sessions", "history.jsonl", "memories"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContaCodex {
    pub id: String,
    pub nome: String,
}

impl ContaCodex {
    /// A pasta usada como `CODEX_HOME` (`None` na principal, que usa a do próprio Codex).
    pub fn pasta(&self) -> Option<PathBuf> {
        (self.id != PRINCIPAL).then(|| pasta_das_contas().map(|p| p.join(&self.id))).flatten()
    }

    /// A conta tem o login guardado (só se confere que o `auth.json` existe; o conteúdo nunca é
    /// lido).
    pub fn logada(&self) -> bool {
        match self.pasta() {
            Some(pasta) => pasta.join("auth.json").exists(),
            None => crate::codex::raiz().is_some_and(|r| r.join("auth.json").exists()),
        }
    }
}

fn pasta_das_contas() -> Option<PathBuf> {
    Some(crate::contas::pasta_da_config()?.join("contas-codex"))
}

fn arquivo_cadastro() -> Option<PathBuf> {
    Some(crate::contas::pasta_da_config()?.join("contas-codex.json"))
}

/// Todas as contas do Codex, com a principal primeiro.
pub fn carregar() -> Vec<ContaCodex> {
    let mut extras: Vec<ContaCodex> = arquivo_cadastro()
        .and_then(|a| fs::read_to_string(a).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    extras.retain(|c| c.id != PRINCIPAL && c.pasta().is_some_and(|p| p.is_dir()));
    let mut todas = vec![ContaCodex { id: PRINCIPAL.into(), nome: "Principal".into() }];
    todas.extend(extras);
    todas
}

pub fn salvar(contas: &[ContaCodex]) -> Result<()> {
    let extras: Vec<&ContaCodex> = contas.iter().filter(|c| c.id != PRINCIPAL).collect();
    let arquivo = arquivo_cadastro().context("sem a pasta da configuração")?;
    crate::dados::gravar_json_seguro(&arquivo, &extras, true)
}

/// Cria (ou completa) a pasta da conta `id`: os links para a principal, sem o login.
pub fn preparar_pasta(id: &str) -> Result<PathBuf> {
    let pasta = pasta_das_contas().context("sem a pasta da configuração")?.join(id);
    fs::create_dir_all(&pasta)?;
    let original = crate::codex::raiz().context("sem a pasta do Codex")?;
    fs::create_dir_all(&original)?;
    ligar_na_principal(&original, &pasta)?;
    Ok(pasta)
}

/// Os links de `LIGADOS` da principal para a pasta da conta (só os que faltam). As sessões e o
/// histórico são criados na principal se ainda não existem: assim os da conta nova já vão para lá.
fn ligar_na_principal(original: &Path, pasta: &Path) -> Result<()> {
    for item in LIGADOS {
        let fonte = original.join(item);
        if matches!(item, "sessions" | "archived_sessions") && !fonte.exists() {
            fs::create_dir_all(&fonte)?;
        }
        if fonte.exists() && fs::symlink_metadata(pasta.join(item)).is_err() {
            crate::ligacoes::ligar_item(original, pasta, item)
                .with_context(|| format!("não liguei {item} da principal na conta"))?;
        }
    }
    Ok(())
}

/// Antes de cada turno do Codex: confere as ligações de todas as contas extras, como o
/// `contas::conferir_ligacoes` faz com as do Claude.
pub fn conferir_ligacoes() {
    let Some(original) = crate::codex::raiz() else {
        return;
    };
    for pasta in carregar().iter().filter_map(ContaCodex::pasta) {
        crate::ligacoes::conferir(&original, &pasta, &LIGADOS);
    }
}

/// Uma conta nova (o nome é "Codex 2", "Codex 3"…), com a pasta pronta para o login.
pub fn nova(contas: &[ContaCodex]) -> Result<ContaCodex> {
    let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let n = (2..).find(|n| !contas.iter().any(|c| c.nome == format!("Codex {n}"))).unwrap_or(2);
    preparar_pasta(&id)?;
    Ok(ContaCodex { id, nome: format!("Codex {n}") })
}

pub fn apagar_pasta(id: &str) {
    if id != PRINCIPAL
        && !id.is_empty()
        && !id.contains(['/', '\\'])
        && !id.starts_with('.')
        && let Some(pasta) = pasta_das_contas()
    {
        let _ = fs::remove_dir_all(pasta.join(id));
    }
}

/// O que o processo do Codex leva para usar a conta: o `CODEX_HOME` dela e o `CODEX_SQLITE_HOME`
/// da principal (as conversas ficam num lugar só). Na principal, nada.
pub fn ambiente(conta: &ContaCodex) -> Vec<(&'static str, PathBuf)> {
    let Some(pasta) = conta.pasta() else {
        return Vec::new();
    };
    let mut ambiente = vec![("CODEX_HOME", pasta)];
    if let Some(principal) = crate::codex::raiz() {
        ambiente.push(("CODEX_SQLITE_HOME", principal));
    }
    ambiente
}

/// O comando de entrar numa conta, para o terminal: o `codex login` com o `CODEX_HOME` dela.
pub fn comando_de_login(conta: &ContaCodex) -> String {
    let codex = crate::agente_codex::binario();
    match conta.pasta() {
        Some(pasta) if cfg!(windows) => format!("$env:CODEX_HOME='{}'; & '{}' login", pasta.display(), codex.display()),
        Some(pasta) => format!("CODEX_HOME='{}' '{}' login", pasta.display(), codex.display()),
        None if cfg!(windows) => format!("& '{}' login", codex.display()),
        None => format!("'{}' login", codex.display()),
    }
}

#[cfg(test)]
mod testes {
    use super::{ContaCodex, PRINCIPAL, ligar_na_principal};

    /// A conta extra liga a configuração e as sessões da principal e fica com o login só dela. No
    /// Linux os dois são links simbólicos; no Windows, a pasta é junção e o arquivo é link
    /// simbólico ou hard link (`ligacoes::ligar`): lá o que se confere é que os dois lados são o
    /// mesmo, gravando por um e lendo pelo outro.
    #[test]
    fn a_conta_extra_divide_tudo_menos_o_login() {
        let base = std::env::temp_dir().join(format!("vitral-codex-{}", crate::modelo::novo_id()));
        let (principal, conta) = (base.join("principal"), base.join("conta"));
        std::fs::create_dir_all(&principal).unwrap();
        std::fs::create_dir_all(&conta).unwrap();
        std::fs::write(principal.join("config.toml"), "model = \"x\"").unwrap();
        std::fs::write(principal.join("auth.json"), "{}").unwrap();
        ligar_na_principal(&principal, &conta).unwrap();
        #[cfg(unix)]
        {
            assert!(std::fs::symlink_metadata(conta.join("config.toml")).unwrap().file_type().is_symlink());
            assert!(std::fs::symlink_metadata(conta.join("sessions")).unwrap().file_type().is_symlink());
        }
        assert!(crate::ligacoes::mesmo_arquivo(&principal.join("config.toml"), &conta.join("config.toml")).unwrap());
        std::fs::write(conta.join("config.toml"), "model = \"y\"").unwrap();
        assert_eq!(std::fs::read_to_string(principal.join("config.toml")).unwrap(), "model = \"y\"");
        std::fs::write(conta.join("sessions").join("s.jsonl"), "{}").unwrap();
        assert!(principal.join("sessions").join("s.jsonl").is_file(), "a sessão da conta fica na principal");
        assert!(principal.join("sessions").is_dir(), "as sessões nascem na principal");
        assert!(!conta.join("auth.json").exists(), "o login não é ligado");
        // Ligar de novo não falha nem duplica.
        ligar_na_principal(&principal, &conta).unwrap();
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_principal_nao_tem_pasta_nem_ambiente() {
        let principal = ContaCodex { id: PRINCIPAL.into(), nome: "Principal".into() };
        assert!(principal.pasta().is_none());
        assert!(super::ambiente(&principal).is_empty());
    }
}
