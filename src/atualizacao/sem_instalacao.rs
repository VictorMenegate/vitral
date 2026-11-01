//! Fora do Linux ainda não há instalação gerenciada (a do Windows é o Lote W4): a mesma API do
//! `linux.rs`, dizendo que nada está instalado. `Raiz::do_usuario` é None, então a procura e a saúde
//! nunca chegam a mexer no disco, e o `--instalar` só explica.

#![allow(dead_code, reason = "a mesma API do linux.rs; fora do Linux quase nada daqui é chamado")]

use std::{
    fmt, io,
    path::{Path, PathBuf},
};

pub use super::estado_app::{Abertura, EstadoApp, Volta};

#[derive(Debug, Clone)]
pub struct Raiz {
    pub dados: PathBuf,
    pub bin: Option<PathBuf>,
}

impl Raiz {
    pub fn do_usuario() -> Option<Raiz> {
        None
    }

    pub fn app(&self) -> PathBuf {
        self.dados.join("vitral").join(crate::dados::PASTA_DO_APP)
    }

    pub fn binario_atual(&self) -> PathBuf {
        self.app().join("atual").join("vitral")
    }
}

pub enum Origem<'a> {
    Pacote { arquivo: &'a Path, prefixo: &'a str },
    Binario(&'a Path),
}

#[derive(Debug)]
pub enum Falha {
    Binario(anyhow::Error),
    SemInstalacao,
}

impl fmt::Display for Falha {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Falha::Binario(erro) => write!(f, "a versão nova não respondeu ao teste: {erro:#}"),
            Falha::SemInstalacao => write!(f, "a instalação automática ainda não existe neste sistema"),
        }
    }
}

impl std::error::Error for Falha {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instalada {
    pub versao: String,
    pub formato_dados: u32,
}

pub struct Trava;

fn sem_instalacao() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, Falha::SemInstalacao.to_string())
}

/// Ninguém instala: a trava nunca é dada (a saúde segue sem contar a abertura).
pub fn travar(_app: &Path) -> io::Result<Option<Trava>> {
    Ok(None)
}

pub fn espaco_livre(_pasta: &Path) -> io::Result<u64> {
    Err(sem_instalacao())
}

pub fn instalar(_app: &Path, _origem: Origem, _versao: &str, _rodando: Option<&str>, _trava: &Trava) -> Result<Instalada, Falha> {
    Err(Falha::SemInstalacao)
}

pub fn integrar(_raiz: &Raiz, _caches: bool) -> io::Result<bool> {
    Ok(false)
}

pub fn limpar_restos(_app: &Path, _trava: &Trava) {}

pub(super) fn apontar(_app: &Path, _nome: &str, _versao: &str) -> io::Result<()> {
    Err(sem_instalacao())
}

pub fn versao_do_link(_app: &Path, _nome: &str) -> Option<String> {
    None
}

pub fn versao_atual(_app: &Path) -> Option<String> {
    None
}

pub fn versao_rodando(_app: &Path) -> Option<String> {
    None
}

/// `vitral --instalar` é do Linux.
pub fn instalar_por_comando() -> i32 {
    eprintln!("Não instalei o Vitral: o --instalar é do Linux. Neste sistema, use o pacote do release.");
    1
}
