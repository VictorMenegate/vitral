//! As chaves que assinam as versões do Vitral e a conferência da assinatura (minisign, Ed25519).
//! O `vitral-versao.json` e o `SHA256SUMS` de cada release saem assinados pela principal no GitHub
//! Actions. A reserva fica fora do GitHub e só assina se a principal se perder ou vazar, numa
//! versão que troca as chaves daqui. As públicas das duas ficam em `pacote/`.

use anyhow::{Context as _, Result, bail};
use minisign_verify::{PublicKey, Signature};

/// Principal (o secret MINISIGN_SECRET_KEY do environment `release`) e reserva: uma basta.
pub const CHAVES: [&str; 2] = [
    include_str!("../../pacote/vitral.pub"),
    include_str!("../../pacote/vitral-reserva.pub"),
];

/// Confere a assinatura (o texto do `.minisig`) dos `dados` com as chaves do app e devolve o
/// comentário confiável ("vitral 0.2.0 vitral-versao.json"). Só vale o formato com pré-hash: a
/// assinatura legada é recusada.
pub fn conferir(dados: &[u8], minisig: &str) -> Result<String> {
    conferir_com(&CHAVES, dados, minisig)
}

/// Só no build de desenvolvimento (a bancada, com um release falso local): aceita também a chave
/// pública do arquivo em `VITRAL_ATUALIZACAO_CHAVE`. O que sai do release não tem isto.
#[cfg(debug_assertions)]
pub fn conferir_com_a_de_teste(dados: &[u8], minisig: &str) -> Result<String> {
    let caminho = std::env::var_os("VITRAL_ATUALIZACAO_CHAVE").context("sem VITRAL_ATUALIZACAO_CHAVE")?;
    let teste = std::fs::read_to_string(caminho).context("a chave de teste não se lê")?;
    conferir_com(&[CHAVES[0], CHAVES[1], &teste], dados, minisig)
}

fn conferir_com(chaves: &[&str], dados: &[u8], minisig: &str) -> Result<String> {
    let assinatura = Signature::decode(minisig).context("a assinatura não se lê")?;
    for chave in chaves {
        let chave = PublicKey::decode(chave).context("chave pública inválida")?;
        if chave.verify(dados, &assinatura, false).is_ok() {
            return Ok(assinatura.trusted_comment().to_string());
        }
    }
    bail!("a assinatura não confere")
}

#[cfg(test)]
mod testes {
    use super::*;

    // Feitas uma vez com uma chave só de teste (a assinatura com o rsign2; a legada com o openssl),
    // cuja secreta não foi guardada. Nunca assinar uma fixture com a principal.
    const TESTE: &str = include_str!("teste/teste.pub");
    const DADOS: &[u8] = include_bytes!("teste/vitral-versao.json");
    const ASSINATURA: &str = include_str!("teste/vitral-versao.json.minisig");
    const LEGADA: &str = include_str!("teste/vitral-versao.json.legado.minisig");

    fn base64(chave: &str) -> &str {
        chave.lines().nth(1).unwrap().trim()
    }

    #[test]
    fn aceita_a_assinatura_da_chave_de_teste() {
        let comentario = conferir_com(&[TESTE], DADOS, ASSINATURA).unwrap();
        assert_eq!(comentario, "vitral 0.2.0-rc.1 vitral-versao.json");
    }

    #[test]
    fn recusa_um_byte_trocado() {
        for i in [0, DADOS.len() / 2, DADOS.len() - 1] {
            let mut trocado = DADOS.to_vec();
            trocado[i] ^= 1;
            assert!(conferir_com(&[TESTE], &trocado, ASSINATURA).is_err(), "byte {i}");
        }
        // O comentário confiável também vai assinado.
        let outro = ASSINATURA.replace("vitral 0.2.0-rc.1", "vitral 9.9.9");
        assert_ne!(outro, ASSINATURA);
        assert!(conferir_com(&[TESTE], DADOS, &outro).is_err());
    }

    #[test]
    fn recusa_outra_chave() {
        assert!(conferir(DADOS, ASSINATURA).is_err());
        assert!(conferir_com(&[CHAVES[1]], DADOS, ASSINATURA).is_err());
    }

    #[test]
    fn qualquer_uma_das_chaves_basta() {
        assert!(conferir_com(&[CHAVES[0], TESTE], DADOS, ASSINATURA).is_ok());
        assert!(conferir_com(&[TESTE, CHAVES[1]], DADOS, ASSINATURA).is_ok());
    }

    #[test]
    fn recusa_assinatura_legada() {
        // A fixture vale como legada; quem recusa é a regra do app.
        let chave = PublicKey::decode(TESTE).unwrap();
        assert!(chave.verify(DADOS, &Signature::decode(LEGADA).unwrap(), true).is_ok());
        assert!(conferir_com(&[TESTE], DADOS, LEGADA).is_err());
    }

    #[test]
    fn recusa_assinatura_que_nao_se_le() {
        assert!(conferir_com(&[TESTE], DADOS, "").is_err());
        assert!(conferir_com(&[TESTE], DADOS, "untrusted comment: x\nnada\n").is_err());
    }

    #[test]
    fn as_chaves_do_app_se_leem_e_o_instalar_usa_a_principal() {
        for chave in CHAVES {
            assert!(PublicKey::decode(chave).is_ok());
        }
        assert_ne!(base64(CHAVES[0]), base64(CHAVES[1]));
        assert!(CHAVES.iter().all(|c| base64(c) != base64(TESTE)));
        let instalar = include_str!("../../pacote/linux/instalar.sh");
        assert!(instalar.contains(&format!("CHAVE=\"{}\"", base64(CHAVES[0]))));
    }
}
