//! As línguas da interface (26/11/2026; dono: "faz várias línguas pra escolher", com inglês e
//! espanhol além do português).
//!
//! O texto em português é a própria chave: `tr!("Novo chat")` devolve "Novo chat" em português,
//! sem consultar nada, e a tradução de `locales/<língua>/*.json` nas outras (o `build.rs` junta os
//! arquivos de cada língua num só). Assim o código continua legível, os testes (que rodam em
//! português) não mudam, e um texto sem tradução aparece em português em vez de sumir.
//!
//! Com argumentos, `tr!("Fila · {n}", n = total)`: a tradução leva os mesmos `{nome}`.
//!
//! A escolha fica em `Config.lingua` ("" segue o sistema; "pt", "en" ou "es"). Fora das três, o
//! Vitral cai no inglês. Os pedidos que o Vitral manda aos agentes continuam em português (com a
//! língua da resposta pedida no prompt de sistema).

use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{
        LazyLock,
        atomic::{AtomicU8, Ordering},
    },
};

/// As línguas da interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lingua {
    Portugues,
    Ingles,
    Espanhol,
}

impl Lingua {
    pub const TODAS: [Lingua; 3] = [Lingua::Portugues, Lingua::Ingles, Lingua::Espanhol];

    /// O código guardado na configuração.
    pub fn codigo(self) -> &'static str {
        match self {
            Lingua::Portugues => "pt",
            Lingua::Ingles => "en",
            Lingua::Espanhol => "es",
        }
    }

    /// O nome na própria língua (a lista de escolha mostra cada uma no jeito dela).
    pub fn nome(self) -> &'static str {
        match self {
            Lingua::Portugues => "Português",
            Lingua::Ingles => "English",
            Lingua::Espanhol => "Español",
        }
    }

    /// A língua dos textos do kit de componentes (o kit tem pt-BR e en; o espanhol fica no inglês
    /// até o kit ter o dele).
    fn do_kit(self) -> &'static str {
        match self {
            Lingua::Portugues => "pt-BR",
            Lingua::Ingles | Lingua::Espanhol => "en",
        }
    }

    /// A frase que vai ao agente para ele responder nesta língua (vazio em português).
    pub fn pedido_ao_agente(self) -> &'static str {
        match self {
            Lingua::Portugues => "",
            Lingua::Ingles => "The user reads the Vitral in English: answer in English unless they write in another language.",
            Lingua::Espanhol => "El usuario usa el Vitral en español: responde en español salvo que escriba en otro idioma.",
        }
    }
}

static ATUAL: AtomicU8 = AtomicU8::new(0);

static INGLES: LazyLock<HashMap<String, String>> = LazyLock::new(|| carregar(include_str!(concat!(env!("OUT_DIR"), "/en.json"))));
static ESPANHOL: LazyLock<HashMap<String, String>> = LazyLock::new(|| carregar(include_str!(concat!(env!("OUT_DIR"), "/es.json"))));

fn carregar(json: &str) -> HashMap<String, String> {
    serde_json::from_str(json).unwrap_or_default()
}

/// A língua de agora.
pub fn lingua() -> Lingua {
    match ATUAL.load(Ordering::Relaxed) {
        1 => Lingua::Ingles,
        2 => Lingua::Espanhol,
        _ => Lingua::Portugues,
    }
}

/// A língua do sistema: português e espanhol pelo começo do código ("pt_BR", "es-AR"); o resto,
/// inglês.
pub fn do_sistema() -> Lingua {
    da_etiqueta(&sys_locale::get_locale().unwrap_or_default())
}

fn da_etiqueta(etiqueta: &str) -> Lingua {
    let etiqueta = etiqueta.to_lowercase();
    if etiqueta.starts_with("pt") {
        Lingua::Portugues
    } else if etiqueta.starts_with("es") {
        Lingua::Espanhol
    } else {
        Lingua::Ingles
    }
}

/// A língua de uma escolha da configuração ("" é a do sistema).
pub fn da_configuracao(valor: &str) -> Lingua {
    Lingua::TODAS.into_iter().find(|l| l.codigo() == valor).unwrap_or_else(do_sistema)
}

/// Passa a interface para `lingua` (e os textos do kit junto). Devolve se mudou.
pub fn definir(lingua: Lingua) -> bool {
    let codigo = match lingua {
        Lingua::Portugues => 0,
        Lingua::Ingles => 1,
        Lingua::Espanhol => 2,
    };
    let antes = ATUAL.swap(codigo, Ordering::Relaxed);
    gpui_kit::component::set_locale(lingua.do_kit());
    antes != codigo
}

/// O texto na língua de agora (o `tr!`).
pub fn traduzir(texto: &'static str) -> Cow<'static, str> {
    let tabela = match lingua() {
        Lingua::Portugues => return Cow::Borrowed(texto),
        Lingua::Ingles => &*INGLES,
        Lingua::Espanhol => &*ESPANHOL,
    };
    match tabela.get(texto) {
        Some(traducao) => Cow::Owned(traducao.clone()),
        None => Cow::Borrowed(texto),
    }
}

/// O texto com os `{nome}` trocados pelos valores (o `tr!` com argumentos).
pub fn traduzir_com(texto: &'static str, valores: &[(&str, String)]) -> String {
    let mut saida = traduzir(texto).into_owned();
    for (nome, valor) in valores {
        saida = saida.replace(&format!("{{{nome}}}"), valor);
    }
    saida
}

/// `tr!("texto")` e `tr!("texto {n}", n = valor)`: o texto da interface na língua escolhida, como
/// `SharedString` (serve direto num `.child` e onde se pede `impl Into<SharedString>`).
#[macro_export]
macro_rules! tr {
    ($texto:literal) => {
        gpui_kit::SharedString::from($crate::i18n::traduzir($texto))
    };
    ($texto:literal, $($nome:ident = $valor:expr),+ $(,)?) => {
        gpui_kit::SharedString::from($crate::i18n::traduzir_com($texto, &[$((stringify!($nome), ($valor).to_string())),+]))
    };
}

/// O texto tem tradução em `lingua` (o teste que acusa `tr!` sem tradução).
#[cfg(test)]
pub fn tem_traducao(lingua: Lingua, texto: &str) -> bool {
    match lingua {
        Lingua::Portugues => true,
        Lingua::Ingles => INGLES.contains_key(texto),
        Lingua::Espanhol => ESPANHOL.contains_key(texto),
    }
}

#[cfg(test)]
mod testes {
    use super::{Lingua, da_etiqueta, traduzir_com};

    #[test]
    fn a_lingua_do_sistema_cai_no_ingles_fora_das_tres() {
        assert_eq!(da_etiqueta("pt_BR.UTF-8"), Lingua::Portugues);
        assert_eq!(da_etiqueta("es-AR"), Lingua::Espanhol);
        assert_eq!(da_etiqueta("de_DE"), Lingua::Ingles);
        assert_eq!(da_etiqueta(""), Lingua::Ingles);
    }

    /// Todo `tr!("…")` do código tem tradução em inglês e em espanhol: um texto novo sem ela
    /// apareceria em português no meio da interface traduzida.
    #[test]
    fn todo_tr_tem_traducao() {
        let raiz = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        let mut faltam = Vec::new();
        let mut pilha = vec![raiz.to_path_buf()];
        while let Some(pasta) = pilha.pop() {
            for entrada in std::fs::read_dir(&pasta).unwrap().flatten() {
                let caminho = entrada.path();
                if caminho.is_dir() {
                    pilha.push(caminho);
                    continue;
                }
                if caminho.extension().is_none_or(|e| e != "rs") || caminho.ends_with("i18n.rs") {
                    continue;
                }
                let texto = std::fs::read_to_string(&caminho).unwrap();
                for (i, _) in texto.match_indices("tr!(\"") {
                    // O `include_str!(` também termina em "tr!(": só vale o `tr!` solto.
                    if texto[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let resto = &texto[i + 5..];
                    let mut chave = String::new();
                    let mut chars = resto.chars();
                    while let Some(c) = chars.next() {
                        match c {
                            '"' => break,
                            '\\' => match chars.next() {
                                Some('n') => chave.push('\n'),
                                Some('"') => chave.push('"'),
                                Some('\\') => chave.push('\\'),
                                Some('\n') => {
                                    // A continuação de linha do Rust: pula o recuo.
                                    while chars.clone().next().is_some_and(char::is_whitespace) {
                                        chars.next();
                                    }
                                }
                                Some('u') => {
                                    let hex: String = chars.by_ref().skip(1).take_while(|c| *c != '}').collect();
                                    if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                                        chave.push(c);
                                    }
                                }
                                Some(outro) => chave.push(outro),
                                None => break,
                            },
                            outro => chave.push(outro),
                        }
                    }
                    for lingua in [Lingua::Ingles, Lingua::Espanhol] {
                        if !super::tem_traducao(lingua, &chave) {
                            faltam.push(format!("{} ({}): {chave}", caminho.strip_prefix(raiz).unwrap().display(), lingua.codigo()));
                        }
                    }
                }
            }
        }
        assert!(faltam.is_empty(), "tr! sem tradução:\n{}", faltam.join("\n"));
    }

    /// Em português o texto é a chave, e os `{nome}` se trocam.
    #[test]
    fn em_portugues_o_texto_e_a_chave() {
        assert_eq!(traduzir_com("Fila · {n}", &[("n", "3".into())]), "Fila · 3");
    }
}
