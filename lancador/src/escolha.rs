//! Qual `vitral.exe` o lançador abre, a partir da raiz da instalação (`%LOCALAPPDATA%\Programs\Vitral`):
//!
//! ```text
//! Vitral.exe          o lançador
//! versoes.txt         "atual=0.3.0\r\nanterior=0.2.0\r\n" (o instalador e o atualizador gravam)
//! app-0.2.0\vitral.exe
//! app-0.3.0\vitral.exe
//! ```
//!
//! A ordem: `app-<atual>`, depois `app-<anterior>` e, sem nenhuma das duas (uma instalação que caiu
//! no meio, um `versoes.txt` perdido), a maior `app-<versão>` que tiver o `vitral.exe`. Só a std e
//! sem nada do lançador: o atualizador do Windows pode trazer este arquivo com `#[path]` para ler o
//! `versoes.txt` e ordenar as versões do mesmo jeito.

use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
};

/// O binário de cada versão, dentro de `app-<versão>\`.
pub const BINARIO: &str = "vitral.exe";
pub const VERSOES: &str = "versoes.txt";

/// O que o `versoes.txt` diz. Um valor que não é versão fica de fora: assim nenhum texto do arquivo
/// vira um caminho fora de `app-<versão>` (`..`, barras, letra de disco).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Versoes {
    pub atual: Option<String>,
    pub anterior: Option<String>,
}

impl Versoes {
    /// `chave=valor` por linha, com CRLF ou LF, espaços em volta e o BOM de quem editou no Bloco de
    /// Notas; o resto do arquivo não conta.
    pub fn ler(texto: &str) -> Versoes {
        let mut versoes = Versoes::default();
        for linha in texto.trim_start_matches('\u{feff}').lines() {
            let Some((chave, valor)) = linha.split_once('=') else { continue };
            let valor = valor.trim();
            if Versao::ler(valor).is_none() {
                continue;
            }
            match chave.trim() {
                "atual" => versoes.atual = Some(valor.to_string()),
                "anterior" => versoes.anterior = Some(valor.to_string()),
                _ => {}
            }
        }
        versoes
    }
}

/// `raiz\app-<versão>\vitral.exe`.
pub fn binario(raiz: &Path, versao: &str) -> PathBuf {
    raiz.join(format!("app-{versao}")).join(BINARIO)
}

/// O `vitral.exe` a abrir, ou None quando não há nenhum.
pub fn escolher(raiz: &Path) -> Option<PathBuf> {
    let versoes = Versoes::ler(&fs::read_to_string(raiz.join(VERSOES)).unwrap_or_default());
    [versoes.atual, versoes.anterior]
        .into_iter()
        .flatten()
        .map(|versao| binario(raiz, &versao))
        .find(|exe| tem_binario(exe))
        .or_else(|| maior_pasta(raiz))
}

/// Um arquivo vazio é o que sobra de um `vitral.exe` que a queda de energia pegou no meio: não
/// serve.
fn tem_binario(exe: &Path) -> bool {
    fs::metadata(exe).is_ok_and(|m| m.is_file() && m.len() > 0)
}

/// A maior `app-<versão>` com o `vitral.exe`. As pastas de nome que não é versão (`app-velho`, as
/// `.novo-*` do atualizador) ficam de fora.
fn maior_pasta(raiz: &Path) -> Option<PathBuf> {
    let candidatas = fs::read_dir(raiz).ok()?.flatten().filter_map(|entrada| {
        let versao = entrada.file_name().to_str()?.strip_prefix("app-")?.to_string();
        let exe = entrada.path().join(BINARIO);
        (Versao::ler(&versao).is_some() && tem_binario(&exe)).then_some((versao, exe))
    });
    let candidatas: Vec<(String, PathBuf)> = candidatas.collect();
    candidatas.into_iter().max_by(|(a, _), (b, _)| Versao::ler(a).cmp(&Versao::ler(b))).map(|(_, exe)| exe)
}

/// `X.Y.Z[-pré]` na ordem do semver: a pré-lançamento vem antes da versão dela, e nos
/// identificadores da pré os números comparam como número (`rc.10` > `rc.9`) e vêm antes do texto.
/// Sem o `+build`, que o release nunca usa.
#[derive(Debug)]
pub struct Versao<'a> {
    numeros: [u64; 3],
    pre: Vec<&'a str>,
}

impl<'a> Versao<'a> {
    pub fn ler(texto: &'a str) -> Option<Versao<'a>> {
        let (base, pre) = match texto.split_once('-') {
            Some((base, pre)) => (base, Some(pre)),
            None => (texto, None),
        };
        let mut partes = base.split('.');
        let mut numeros = [0u64; 3];
        for numero in &mut numeros {
            let parte = partes.next()?;
            if parte.is_empty() || !parte.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            *numero = parte.parse().ok()?;
        }
        if partes.next().is_some() {
            return None;
        }
        let pre: Vec<&str> = match pre {
            Some(pre) => pre.split('.').collect(),
            None => Vec::new(),
        };
        let valido = |p: &&str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        pre.iter().all(valido).then_some(Versao { numeros, pre })
    }
}

impl Ord for Versao<'_> {
    fn cmp(&self, outra: &Self) -> Ordering {
        self.numeros.cmp(&outra.numeros).then_with(|| match (self.pre.is_empty(), outra.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(&outra.pre) {
                    let ordem = match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(a), Ok(b)) => a.cmp(&b),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => a.cmp(b),
                    };
                    if ordem != Ordering::Equal {
                        return ordem;
                    }
                }
                self.pre.len().cmp(&outra.pre.len())
            }
        })
    }
}

impl PartialEq for Versao<'_> {
    fn eq(&self, outra: &Self) -> bool {
        self.cmp(outra) == Ordering::Equal
    }
}

impl Eq for Versao<'_> {}

impl PartialOrd for Versao<'_> {
    fn partial_cmp(&self, outra: &Self) -> Option<Ordering> {
        Some(self.cmp(outra))
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as Ordem};

    /// Uma raiz de instalação de mentira na pasta temporária, apagada no fim do teste.
    struct Raiz(PathBuf);

    impl Raiz {
        fn nova() -> Raiz {
            static CONTADOR: AtomicUsize = AtomicUsize::new(0);
            let nome = format!("vitral-lancador-{}-{}", std::process::id(), CONTADOR.fetch_add(1, Ordem::SeqCst));
            let pasta = std::env::temp_dir().join(nome);
            let _ = fs::remove_dir_all(&pasta);
            fs::create_dir_all(&pasta).unwrap();
            Raiz(pasta)
        }

        /// `app-<versão>\vitral.exe` com conteúdo (ou vazio, o que a queda de energia deixa).
        fn versao(&self, versao: &str, conteudo: &[u8]) -> PathBuf {
            let exe = binario(&self.0, versao);
            fs::create_dir_all(exe.parent().unwrap()).unwrap();
            fs::write(&exe, conteudo).unwrap();
            exe
        }

        fn versoes(&self, texto: &str) {
            fs::write(self.0.join(VERSOES), texto).unwrap();
        }
    }

    impl Drop for Raiz {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn abre_a_atual() {
        let raiz = Raiz::nova();
        raiz.versao("0.2.0", b"MZ");
        let atual = raiz.versao("0.3.0", b"MZ");
        raiz.versao("0.4.0", b"MZ");
        // O que o instalador grava (o [Code] do vitral.iss).
        raiz.versoes("atual=0.3.0\r\nanterior=0.2.0\r\n");
        assert_eq!(escolher(&raiz.0), Some(atual), "a atual, mesmo com uma maior na pasta");
    }

    #[test]
    fn sem_a_atual_abre_a_anterior() {
        let raiz = Raiz::nova();
        let anterior = raiz.versao("0.2.0", b"MZ");
        raiz.versoes("atual=0.3.0\r\nanterior=0.2.0\r\n");
        assert_eq!(escolher(&raiz.0), Some(anterior.clone()), "a pasta da atual não existe");
        raiz.versao("0.3.0", b"");
        assert_eq!(escolher(&raiz.0), Some(anterior.clone()), "o vitral.exe da atual está vazio");
        fs::remove_file(binario(&raiz.0, "0.3.0")).unwrap();
        assert_eq!(escolher(&raiz.0), Some(anterior), "a pasta da atual está sem o vitral.exe");
    }

    #[test]
    fn sem_atual_nem_anterior_abre_a_maior_pasta() {
        let raiz = Raiz::nova();
        raiz.versao("0.9.0", b"MZ");
        let maior = raiz.versao("0.10.0", b"MZ");
        raiz.versao("0.10.0-rc.12", b"MZ");
        // Maiores, mas sem binário que sirva, ou de nome que não é versão.
        raiz.versao("0.11.0", b"");
        fs::create_dir_all(raiz.0.join("app-0.12.0")).unwrap();
        raiz.versao("velho", b"MZ");
        fs::create_dir_all(raiz.0.join(".novo-0.13.0")).unwrap();
        fs::write(raiz.0.join(".novo-0.13.0").join(BINARIO), b"MZ").unwrap();
        assert_eq!(escolher(&raiz.0), Some(maior.clone()), "sem o versoes.txt");
        raiz.versoes("atual=0.14.0\r\nanterior=0.13.0\r\n");
        assert_eq!(escolher(&raiz.0), Some(maior.clone()), "o versoes.txt aponta para versões que não estão lá");
        raiz.versoes("lixo");
        assert_eq!(escolher(&raiz.0), Some(maior), "o versoes.txt ilegível");
    }

    #[test]
    fn nada_para_abrir() {
        let raiz = Raiz::nova();
        assert_eq!(escolher(&raiz.0), None, "pasta vazia");
        raiz.versoes("atual=0.3.0\r\n");
        raiz.versao("0.3.0", b"");
        fs::create_dir_all(raiz.0.join("app-0.2.0")).unwrap();
        assert_eq!(escolher(&raiz.0), None, "só versões sem binário que sirva");
        assert_eq!(escolher(&raiz.0.join("nao-existe")), None, "raiz que não existe");
    }

    #[test]
    fn o_versoes_txt_nunca_sai_de_app_versao() {
        let raiz = Raiz::nova();
        let unica = raiz.versao("0.2.0", b"MZ");
        fs::create_dir_all(raiz.0.join("fora")).unwrap();
        fs::write(raiz.0.join("fora").join(BINARIO), b"MZ").unwrap();
        for texto in ["atual=../fora\r\n", "atual=..\\fora\r\n", "atual=C:\\fora\r\n", "atual=0.3.0/../../fora\r\n"] {
            raiz.versoes(texto);
            assert_eq!(escolher(&raiz.0), Some(unica.clone()), "{texto:?}");
        }
    }

    #[test]
    fn le_o_versoes_txt_com_lf_bom_e_espacos() {
        let esperado = Versoes { atual: Some("0.3.0".into()), anterior: Some("0.2.0-rc.1".into()) };
        assert_eq!(Versoes::ler("atual=0.3.0\r\nanterior=0.2.0-rc.1\r\n"), esperado);
        assert_eq!(Versoes::ler("\u{feff}anterior = 0.2.0-rc.1\n atual= 0.3.0 \n# comentário\nnova=1.0.0\n"), esperado);
        assert_eq!(Versoes::ler("atual=0.3.0"), Versoes { atual: Some("0.3.0".into()), anterior: None });
        assert_eq!(Versoes::ler(""), Versoes::default());
        assert_eq!(Versoes::ler("atual=\r\nanterior=nada\r\n"), Versoes::default());
    }

    #[test]
    fn ordem_das_versoes_e_a_do_semver() {
        let ordem = [
            "0.2.0-rc.1",
            "0.2.0-rc.2",
            "0.2.0-rc.10",
            "0.2.0",
            "0.3.0-dev.5109ec3aa0",
            "0.3.0-rc.1",
            "0.3.0",
            "0.10.0",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0",
        ];
        for par in ordem.windows(2) {
            let (a, b) = (Versao::ler(par[0]).unwrap(), Versao::ler(par[1]).unwrap());
            assert!(a < b, "{} < {}", par[0], par[1]);
        }
        for invalida in ["", "0.3", "0.3.0.1", "v0.3.0", "0.3.0-", "0.3.0-rc..1", "0.3.x", "0.3.0+build", "-1.0.0", "0.3.0-rc/1"] {
            assert!(Versao::ler(invalida).is_none(), "{invalida:?}");
        }
    }
}
