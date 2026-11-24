//! A seta ↑ na caixa vazia com "Percorrer as enviadas": traz o texto das mensagens que você já
//! enviou neste chat, uma por vez (↑ a mais antiga, ↓ volta até a caixa vazia), sem desfazer nada
//! na conversa. Editar o texto trazido encerra o percurso e devolve as setas ao cursor. A lógica é
//! pura, testada no fim.

use crate::modelo::Item;

/// Os textos que o usuário enviou, do mais recente ao mais antigo, sem repetir o mesmo texto em
/// seguida.
pub fn enviadas_do_chat(itens: &[Item]) -> Vec<String> {
    let mut textos: Vec<String> = Vec::new();
    for item in itens.iter().rev() {
        let Item::Usuario { texto, .. } = item else {
            continue;
        };
        let texto = texto.trim();
        if !texto.is_empty() && textos.last().is_none_or(|ultimo| ultimo != texto) {
            textos.push(texto.to_string());
        }
    }
    textos
}

/// Onde o percurso está: `pos` é a posição na lista de enviadas (0 é a mais recente), e `mostrado`
/// o texto que pusemos na caixa (se a caixa mudou, o percurso acabou).
#[derive(Clone, Debug, Default)]
pub struct Percurso {
    pos: Option<usize>,
    mostrado: String,
}

impl Percurso {
    /// O percurso está valendo para o que está na caixa agora (nada mudou desde o que pusemos).
    pub fn ativo(&self, caixa: &str) -> bool {
        self.pos.is_some() && caixa == self.mostrado
    }

    /// ↑: o texto que a caixa passa a ter, ou `None` quando nada muda (caixa com texto que não é
    /// do percurso, ou já na mais antiga).
    pub fn subir(&mut self, enviadas: &[String], caixa: &str) -> Option<String> {
        let proxima = if self.ativo(caixa) {
            self.pos.map_or(0, |p| p + 1)
        } else if caixa.is_empty() {
            0
        } else {
            self.pos = None;
            return None;
        };
        let texto = enviadas.get(proxima)?.clone();
        self.pos = Some(proxima);
        self.mostrado = texto.clone();
        Some(texto)
    }

    /// ↓: a mais nova que a de agora e, depois da mais nova, a caixa vazia; `None` fora do percurso.
    pub fn descer(&mut self, enviadas: &[String], caixa: &str) -> Option<String> {
        if !self.ativo(caixa) {
            self.pos = None;
            return None;
        }
        match self.pos? {
            0 => {
                self.pos = None;
                self.mostrado.clear();
                Some(String::new())
            }
            p => {
                let texto = enviadas.get(p - 1)?.clone();
                self.pos = Some(p - 1);
                self.mostrado = texto.clone();
                Some(texto)
            }
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn lista(textos: &[&str]) -> Vec<String> {
        textos.iter().map(|t| t.to_string()).collect()
    }

    fn usuario(texto: &str) -> Item {
        Item::Usuario { texto: texto.into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0, marca: None }
    }

    #[test]
    fn as_enviadas_vem_da_mais_nova_sem_repetir_em_seguida() {
        let itens = vec![usuario("a"), usuario("b"), usuario("b"), usuario("  "), usuario("c"), usuario("b")];
        assert_eq!(enviadas_do_chat(&itens), ["b", "c", "b", "a"]);
    }

    #[test]
    fn sobe_pelas_enviadas_e_para_na_mais_antiga() {
        let e = lista(&["c", "b", "a"]);
        let mut p = Percurso::default();
        assert_eq!(p.subir(&e, ""), Some("c".into()));
        assert_eq!(p.subir(&e, "c"), Some("b".into()));
        assert_eq!(p.subir(&e, "b"), Some("a".into()));
        assert_eq!(p.subir(&e, "a"), None, "já na mais antiga");
        assert!(p.ativo("a"));
    }

    #[test]
    fn desce_ate_a_caixa_vazia() {
        let e = lista(&["c", "b", "a"]);
        let mut p = Percurso::default();
        p.subir(&e, "");
        p.subir(&e, "c");
        assert_eq!(p.descer(&e, "b"), Some("c".into()));
        assert_eq!(p.descer(&e, "c"), Some(String::new()));
        assert!(!p.ativo(""), "voltou ao começo");
        assert_eq!(p.descer(&e, ""), None);
        assert_eq!(p.subir(&e, ""), Some("c".into()), "recomeça pela mais nova");
    }

    #[test]
    fn editar_o_texto_trazido_devolve_as_setas_ao_cursor() {
        let e = lista(&["c", "b"]);
        let mut p = Percurso::default();
        p.subir(&e, "");
        assert!(!p.ativo("c editado"));
        assert_eq!(p.subir(&e, "c editado"), None);
        assert_eq!(p.descer(&e, "c editado"), None);
        assert!(!p.ativo("c"), "o percurso acabou de vez");
    }

    #[test]
    fn caixa_com_texto_que_nao_e_do_percurso_nao_sobe() {
        let e = lista(&["c"]);
        let mut p = Percurso::default();
        assert_eq!(p.subir(&e, "rascunho"), None);
    }

    #[test]
    fn sem_enviadas_nao_faz_nada() {
        let mut p = Percurso::default();
        assert_eq!(p.subir(&[], ""), None);
        assert!(!p.ativo(""));
    }
}
