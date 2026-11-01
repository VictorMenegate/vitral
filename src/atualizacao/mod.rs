//! A versão embutida no binário e os comandos que respondem sem abrir janela. A atualização
//! automática entra aqui (especificação em `distribuicao.md`, seção 3).
//!
//! De onde veio o binário vem da compilação: `VITRAL_CANAL` ("estavel" só no release do GitHub
//! Actions; sem ele, compilado no PC) e `VITRAL_COMMIT`.

pub mod assinatura;

use serde_json::json;

use crate::modelo::FORMATO_DADOS;

/// A versão deste binário (a do Cargo.toml).
pub const VERSAO: &str = env!("CARGO_PKG_VERSION");

/// "estavel" no que sai do release; "pc" no que foi compilado no PC.
pub fn canal() -> &'static str {
    match option_env!("VITRAL_CANAL") {
        Some(canal) if !canal.is_empty() => canal,
        _ => "pc",
    }
}

/// O commit de onde o binário saiu (vazio no compilado no PC).
pub fn commit() -> &'static str {
    option_env!("VITRAL_COMMIT").unwrap_or("")
}

/// "linux-x86_64", o mesmo sufixo dos pacotes do release.
pub fn plataforma() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// O que `--versao --json` responde. O atualizador roda o binário novo com isso antes de trocar de
/// versão: um binário zerado, sem biblioteca ou de outro pacote não responde igual.
pub fn json() -> String {
    json!({
        "versao": VERSAO,
        "formato_dados": FORMATO_DADOS,
        "canal": canal(),
        "commit": commit(),
        "plataforma": plataforma(),
    })
    .to_string()
}

/// Os comandos que respondem sem janela, antes do GPUI (servem sem tela: no runner do GitHub e no
/// atualizador). Devolve o código de saída quando o argumento é um deles.
pub fn sem_janela(argumentos: &[String]) -> Option<i32> {
    let tem = |nome: &str| argumentos.iter().any(|a| a == nome);
    if tem("--versao") || tem("--version") {
        if tem("--json") {
            println!("{}", json());
        } else {
            println!("vitral {VERSAO}");
        }
        return Some(0);
    }
    if tem("--instalar") {
        eprintln!("Esta versão ainda não se instala sozinha; abra o vitral direto desta pasta.");
        return Some(2);
    }
    None
}

/// O rodapé do menu do logo.
pub fn rotulo() -> String {
    format!("Vitral {VERSAO}")
}

/// A dica do rodapé: de onde veio este binário.
pub fn origem() -> String {
    origem_de(canal(), commit())
}

fn origem_de(canal: &str, commit: &str) -> String {
    let curto = commit.get(..7).unwrap_or(commit);
    match (canal == "pc", curto.is_empty()) {
        (true, true) => "Compilado no PC".to_string(),
        (true, false) => format!("Compilado no PC · commit {curto}"),
        (false, true) => format!("Canal {canal}"),
        (false, false) => format!("Commit {curto}"),
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn json_da_versao_tem_o_que_o_atualizador_confere() {
        let valor: serde_json::Value = serde_json::from_str(&json()).unwrap();
        assert_eq!(valor["versao"], VERSAO);
        assert_eq!(valor["formato_dados"], FORMATO_DADOS);
        assert_eq!(valor["canal"], canal());
        assert_eq!(valor["commit"], commit());
        assert_eq!(valor["plataforma"], plataforma());
        assert!(!canal().is_empty());
    }

    #[test]
    fn so_os_comandos_sem_janela_sao_atendidos_antes_do_gpui() {
        let args = |lista: &[&str]| lista.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(sem_janela(&args(&[])), None);
        assert_eq!(sem_janela(&args(&["/home/victor/Projetos"])), None);
        assert_eq!(sem_janela(&args(&["--vitrine"])), None);
        assert_eq!(sem_janela(&args(&["--versao"])), Some(0));
    }

    #[test]
    fn a_dica_da_versao_diz_de_onde_veio() {
        assert_eq!(origem_de("pc", ""), "Compilado no PC");
        assert_eq!(origem_de("pc", "5109ec3aa0"), "Compilado no PC · commit 5109ec3");
        assert_eq!(origem_de("estavel", "5109ec3aa0b1"), "Commit 5109ec3");
        assert_eq!(origem_de("estavel", ""), "Canal estavel");
        assert_eq!(rotulo(), format!("Vitral {}", env!("CARGO_PKG_VERSION")));
    }
}
