//! A chave do Linear: no chaveiro do sistema (Secret Service, pelo `oo7`) e na memória do Vitral
//! (`Zeroizing`). Nunca em arquivo, argumento, variável de ambiente, log ou aviso. O chaveiro só é
//! destravado por ação do usuário: a leitura de fundo com ele trancado não abre diálogo nenhum.
// Provisório: o motor passa a usar estes módulos no G2.
#![allow(dead_code)]

use std::time::Duration;

use zeroize::Zeroizing;

/// Uma chave pessoal de API do Linear, já validada.
pub struct Chave(Zeroizing<String>);

pub const CHAVE_INVALIDA: &str =
    "Isto não parece uma chave do Linear (começa com lin_api_, sem espaços nem aspas).";

impl Chave {
    /// Valida o que o usuário colou: depois do trim, `lin_api_` seguido só de letras e números,
    /// com mais de 20 caracteres. Isso barra aspas e quebras de linha, que estragariam a
    /// configuração do curl (a chave vai entre aspas no stdin dele).
    pub fn linear(texto: &str) -> Result<Chave, &'static str> {
        let texto = texto.trim();
        let valida = texto.len() > 20
            && texto.strip_prefix("lin_api_").is_some_and(|resto| !resto.is_empty() && resto.chars().all(|c| c.is_ascii_alphanumeric()));
        if valida { Ok(Chave(Zeroizing::new(texto.to_string()))) } else { Err(CHAVE_INVALIDA) }
    }

    pub fn texto(&self) -> &str {
        &self.0
    }

    /// Os 4 últimos caracteres, para "chave …a1b2".
    pub fn final_(&self) -> String {
        let texto = self.texto();
        texto[texto.len().saturating_sub(4)..].to_string()
    }
}

impl std::fmt::Debug for Chave {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Chave(…{})", self.final_())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErroChaveiro {
    /// Trancado, e a leitura não podia destravar (ou o usuário recusou a senha).
    Trancado,
    Indisponivel(String),
    TempoEsgotado,
}

impl ErroChaveiro {
    pub fn para_usuario(&self) -> String {
        match self {
            Self::Trancado => "O chaveiro está trancado.".into(),
            Self::Indisponivel(motivo) => format!("O chaveiro do sistema não respondeu ({motivo})."),
            Self::TempoEsgotado => "O chaveiro do sistema não respondeu a tempo.".into(),
        }
    }
}

/// Como o item aparece em Senhas e chaves (e no `secret-tool lookup application vitral …`).
const ROTULO: &str = "Vitral · chave do Linear";
const ATRIBUTOS: [(&str, &str); 3] = [("application", "vitral"), ("servico", "linear"), ("tipo", "chave-api")];
/// Inclui o tempo de o usuário digitar a senha do chaveiro.
const LIMITE: Duration = Duration::from_secs(60);

async fn com_limite<T>(f: impl Future<Output = Result<T, ErroChaveiro>>) -> Result<T, ErroChaveiro> {
    smol::future::or(f, async {
        smol::Timer::after(LIMITE).await;
        Err(ErroChaveiro::TempoEsgotado)
    })
    .await
}

/// `destravar = false`: chaveiro trancado dá `Err(Trancado)`, sem abrir diálogo nenhum.
pub async fn ler_chave_linear(destravar: bool) -> Result<Option<Chave>, ErroChaveiro> {
    com_limite(sistema::ler(destravar)).await
}

/// Grava (ou troca) a chave. Por ação do usuário: pode pedir a senha do chaveiro.
pub async fn gravar_chave_linear(chave: &Chave) -> Result<(), ErroChaveiro> {
    com_limite(sistema::gravar(chave)).await
}

pub async fn apagar_chave_linear() -> Result<(), ErroChaveiro> {
    com_limite(sistema::apagar()).await
}

#[cfg(target_os = "linux")]
mod sistema {
    use super::{ATRIBUTOS, Chave, ErroChaveiro, ROTULO};

    fn indisponivel(erro: oo7::Error) -> ErroChaveiro {
        match erro {
            oo7::Error::DBus(oo7::dbus::Error::Dismissed) => ErroChaveiro::Trancado,
            outro => ErroChaveiro::Indisponivel(outro.to_string()),
        }
    }

    fn atributos() -> Vec<(&'static str, &'static str)> {
        ATRIBUTOS.to_vec()
    }

    /// O chaveiro padrão. Sem `destravar`, nada que abra um diálogo: o `Keyring::new()` do oo7
    /// cria a coleção padrão quando ela não existe (e o sistema pede uma senha nova), então a
    /// leitura de fundo procura a coleção pelo apelido e, sem ela, diz que não há chave (`None`).
    async fn abrir(destravar: bool) -> Result<Option<oo7::Keyring>, ErroChaveiro> {
        let chaveiro = if destravar || oo7::ashpd::is_sandboxed() {
            oo7::Keyring::new().await.map_err(indisponivel)?
        } else {
            let servico = oo7::dbus::Service::new().await.map_err(|e| indisponivel(e.into()))?;
            match servico.with_alias(oo7::dbus::Service::DEFAULT_COLLECTION).await.map_err(|e| indisponivel(e.into()))? {
                Some(colecao) => oo7::Keyring::DBus(colecao),
                None => return Ok(None),
            }
        };
        if chaveiro.is_locked().await.map_err(indisponivel)? {
            if !destravar {
                return Err(ErroChaveiro::Trancado);
            }
            chaveiro.unlock().await.map_err(indisponivel)?;
        }
        Ok(Some(chaveiro))
    }

    pub async fn ler(destravar: bool) -> Result<Option<Chave>, ErroChaveiro> {
        let Some(chaveiro) = abrir(destravar).await? else {
            return Ok(None);
        };
        let itens = chaveiro.search_items(&atributos()).await.map_err(indisponivel)?;
        let Some(item) = itens.into_iter().next() else {
            return Ok(None);
        };
        if item.is_locked().await.map_err(indisponivel)? {
            if !destravar {
                return Err(ErroChaveiro::Trancado);
            }
            item.unlock().await.map_err(indisponivel)?;
        }
        let segredo = item.secret().await.map_err(indisponivel)?;
        let texto = std::str::from_utf8(segredo.as_bytes())
            .map_err(|_| ErroChaveiro::Indisponivel("a chave guardada não é texto".into()))?;
        Chave::linear(texto)
            .map(Some)
            .map_err(|_| ErroChaveiro::Indisponivel("a chave guardada não é uma chave do Linear".into()))
    }

    /// O chaveiro aberto por ação do usuário (pode pedir a senha, ou criar a coleção padrão).
    async fn aberto() -> Result<oo7::Keyring, ErroChaveiro> {
        abrir(true).await?.ok_or_else(|| ErroChaveiro::Indisponivel("o chaveiro não abriu".into()))
    }

    pub async fn gravar(chave: &Chave) -> Result<(), ErroChaveiro> {
        let chaveiro = aberto().await?;
        let segredo = oo7::Secret::text(chave.texto());
        chaveiro.create_item(ROTULO, &atributos(), segredo, true).await.map_err(indisponivel)
    }

    pub async fn apagar() -> Result<(), ErroChaveiro> {
        let chaveiro = aberto().await?;
        chaveiro.delete(&atributos()).await.map_err(indisponivel)
    }
}

#[cfg(not(target_os = "linux"))]
mod sistema {
    use super::{Chave, ErroChaveiro};

    fn indisponivel() -> ErroChaveiro {
        ErroChaveiro::Indisponivel("não disponível neste sistema".into())
    }

    pub async fn ler(_destravar: bool) -> Result<Option<Chave>, ErroChaveiro> {
        Err(indisponivel())
    }

    pub async fn gravar(_chave: &Chave) -> Result<(), ErroChaveiro> {
        Err(indisponivel())
    }

    pub async fn apagar() -> Result<(), ErroChaveiro> {
        Err(indisponivel())
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    const BOA: &str = concat!("lin_", "api_ABCdef0123456789xyzWa1b2");

    #[test]
    fn a_chave_e_validada_antes_de_tudo() {
        assert_eq!(Chave::linear(&format!("  {BOA}\n")).unwrap().texto(), BOA);
        for ruim in [
            "",
            "lin_api_",
            "lin_api_curta",
            concat!("gh", "p_ABCdef0123456789xyzWa1b2"),
            concat!("Bearer lin_", "api_ABCdef0123456789xyzWa1b2"),
            concat!("\"lin_", "api_ABCdef0123456789xyzWa1b2\""),
            concat!("lin_", "api_ABCdef0123456789 xyzWa1b2"),
            concat!("lin_", "api_ABCdef0123456789\nxyzWa1b2"),
            concat!("lin_", "api_ABCdef0123456789\\xyzWa1b2"),
            concat!("lin_", "api_ABCdef0123456789_xyzWa1b2"),
            "lin_api_ABCdéf0123456789xyzWa1b2",
        ] {
            assert_eq!(Chave::linear(ruim).unwrap_err(), CHAVE_INVALIDA, "{ruim:?}");
        }
    }

    #[test]
    fn o_debug_da_chave_nao_mostra_o_segredo() {
        let chave = Chave::linear(BOA).unwrap();
        assert_eq!(chave.final_(), "a1b2");
        let texto = format!("{chave:?} {:?}", Some(&chave));
        assert_eq!(texto, "Chave(…a1b2) Some(Chave(…a1b2))");
        assert!(!texto.contains("lin_api_"));
    }
}
