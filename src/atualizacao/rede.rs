//! HTTP com o ureq (rustls com as raízes embutidas, sem OpenSSL; lê o `HTTPS_PROXY` do ambiente):
//! o manifesto e a assinatura como texto curto, e o pacote direto para o disco, com o tamanho e o
//! SHA-256 conferidos no caminho. Nenhum identificador do PC vai junto, só a versão no User-Agent.

use std::{
    fs,
    io::{Read as _, Write as _},
    path::Path,
    time::Duration,
};

use sha2::{Digest as _, Sha256};

pub struct Rede {
    agente: ureq::Agent,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Resposta {
    /// 304: o mesmo manifesto da última vez (o ETag bateu).
    NaoMudou,
    /// 404: nada publicado ali.
    NaoExiste,
    Dados { bytes: Vec<u8>, etag: Option<String> },
}

#[derive(Debug)]
pub enum Baixar {
    /// Sem conexão, prazo, servidor fora: tenta de novo mais tarde.
    Rede(String),
    /// Veio com outro tamanho ou outra soma que a do manifesto.
    Diferente,
    Disco(std::io::Error),
}

impl Rede {
    /// Conexão em 15 s, e uma leitura parada por mais de 60 s desiste.
    pub fn nova(agente_de_usuario: &str) -> Rede {
        let config = ureq::Agent::config_builder()
            .user_agent(agente_de_usuario)
            .timeout_resolve(Some(Duration::from_secs(15)))
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_per_read(Some(Duration::from_secs(60)))
            .max_redirects(10)
            .build();
        Rede { agente: config.new_agent() }
    }

    /// Um texto curto (manifesto, assinatura), no máximo `limite` bytes. Com `etag`, o servidor
    /// pode responder que não mudou.
    pub fn texto(&self, url: &str, limite: u64, etag: Option<&str>) -> Result<Resposta, String> {
        let mut pedido = self.agente.get(url);
        if let Some(etag) = etag {
            pedido = pedido.header("If-None-Match", etag);
        }
        let mut resposta = match pedido.call() {
            Ok(resposta) => resposta,
            Err(ureq::Error::StatusCode(404)) => return Ok(Resposta::NaoExiste),
            Err(erro) => return Err(erro.to_string()),
        };
        if resposta.status() == 304 {
            return Ok(Resposta::NaoMudou);
        }
        let etag = resposta.headers().get("etag").and_then(|v| v.to_str().ok()).map(str::to_string);
        let bytes = resposta.body_mut().with_config().limit(limite).read_to_vec().map_err(|e| e.to_string())?;
        Ok(Resposta::Dados { bytes, etag })
    }

    /// Baixa `url` para `destino` (criado do zero), calculando o SHA-256 enquanto grava, e confere
    /// tamanho e soma. Mais bytes que `tamanho` param o download na hora. `progresso` recebe os
    /// bytes já gravados.
    pub fn arquivo(&self, url: &str, destino: &Path, tamanho: u64, sha256: &str, progresso: &mut dyn FnMut(u64)) -> Result<(), Baixar> {
        let resposta = self.agente.get(url).call().map_err(|e| Baixar::Rede(e.to_string()))?;
        let anunciado = resposta.headers().get("content-length").and_then(|v| v.to_str().ok()?.parse::<u64>().ok());
        if anunciado.is_some_and(|n| n != tamanho) {
            return Err(Baixar::Diferente);
        }
        let mut leitor = resposta.into_body().into_reader().take(tamanho + 1);
        let mut saida = fs::File::create(destino).map_err(Baixar::Disco)?;
        let mut soma = Sha256::new();
        let mut feito = 0u64;
        let mut bloco = vec![0u8; 64 * 1024];
        loop {
            let lidos = match leitor.read(&mut bloco) {
                Ok(0) => break,
                Ok(lidos) => lidos,
                Err(erro) if erro.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(erro) => return Err(Baixar::Rede(erro.to_string())),
            };
            feito += lidos as u64;
            if feito > tamanho {
                return Err(Baixar::Diferente);
            }
            soma.update(&bloco[..lidos]);
            saida.write_all(&bloco[..lidos]).map_err(Baixar::Disco)?;
            progresso(feito);
        }
        if feito != tamanho || hexa(&soma.finalize()) != sha256 {
            return Err(Baixar::Diferente);
        }
        saida.sync_all().map_err(Baixar::Disco)
    }
}

/// Confere um arquivo já baixado (de uma abertura anterior que caiu antes de instalar).
pub fn confere(caminho: &Path, tamanho: u64, sha256: &str) -> bool {
    let Ok(mut arquivo) = fs::File::open(caminho) else { return false };
    if arquivo.metadata().map(|m| m.len()).ok() != Some(tamanho) {
        return false;
    }
    let mut soma = Sha256::new();
    let mut bloco = vec![0u8; 64 * 1024];
    loop {
        match arquivo.read(&mut bloco) {
            Ok(0) => break,
            Ok(lidos) => soma.update(&bloco[..lidos]),
            Err(_) => return false,
        }
    }
    hexa(&soma.finalize()) == sha256
}

fn hexa(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn hexa_e_conferencia_de_arquivo() {
        assert_eq!(hexa(&Sha256::digest(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        let pasta = crate::atualizacao::pacote::testes::pasta_de_teste("confere");
        let caminho = pasta.join("a");
        fs::write(&caminho, b"").unwrap();
        assert!(confere(&caminho, 0, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));
        assert!(!confere(&caminho, 1, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));
        assert!(!confere(&pasta.join("nao-existe"), 0, ""));
        fs::remove_dir_all(&pasta).unwrap();
    }

    /// HTTPS de verdade (rustls com as raízes embutidas e o ring): `cargo test -- --ignored
    /// https_de_verdade`, com `VITRAL_TESTE_URL=<endereço>` para outro (o manifesto do GitHub, por
    /// exemplo). Fica fora da suíte: depende da rede.
    #[test]
    #[ignore]
    fn https_de_verdade() {
        let url = std::env::var("VITRAL_TESTE_URL").unwrap_or_else(|_| "https://index.crates.io/config.json".into());
        let rede = Rede::nova("Vitral/teste");
        match rede.texto(&url, 64 * 1024, None).unwrap() {
            Resposta::Dados { bytes, .. } => assert!(!bytes.is_empty()),
            outra => panic!("{url}: {outra:?}"),
        }
    }
}
