//! A busca sem acento e sem caixa, comum ao Ctrl+F da conversa, à lista de chats e ao histórico do
//! "+" (dono, 16/11/2026: rodada de uso, item 4.3). O dono escreve sem acento, e os títulos do
//! Claude Code vêm acentuados: "integracao" tem de achar "integração".

use std::ops::Range;

/// A letra sem acento, para a busca não ligar para ele. Só o latino que aparece em português e
/// nos nomes vizinhos; o resto passa igual (minúsculas vêm depois, em `dobrar`).
pub(super) fn sem_acento(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'Á' | 'À' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'É' | 'È' | 'Ê' | 'Ë' => 'E',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'O',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
        'ç' => 'c',
        'Ç' => 'C',
        'ñ' => 'n',
        'Ñ' => 'N',
        _ => c,
    }
}

/// O texto dobrado para a busca (sem acento e em minúsculas) e, para cada byte dele, a faixa de
/// bytes da letra que o gerou: é assim que uma faixa achada volta a ser uma faixa do original,
/// mesmo quando dobrar muda o tamanho (o 'İ' vira duas letras).
pub(super) struct Dobrado {
    pub(super) texto: String,
    origem: Vec<(u32, u32)>,
}

pub(super) fn dobrar(texto: &str) -> Dobrado {
    let mut d = Dobrado { texto: String::with_capacity(texto.len()), origem: Vec::with_capacity(texto.len()) };
    for (i, c) in texto.char_indices() {
        let fim = i + c.len_utf8();
        for letra in sem_acento(c).to_lowercase() {
            d.texto.push(letra);
            d.origem.resize(d.texto.len(), (i as u32, fim as u32));
        }
    }
    d
}

impl Dobrado {
    /// A faixa do original que o trecho `faixa` do dobrado cobre (`faixa` nunca é vazia).
    pub(super) fn no_original(&self, faixa: Range<usize>) -> Range<usize> {
        self.origem[faixa.start].0 as usize..self.origem[faixa.end - 1].1 as usize
    }
}

/// O termo pronto para `faixas`: dobrado e sem os espaços das pontas. Vazio quer dizer sem busca.
/// É o texto de `dobrar` sem o mapa de volta ao original: a lista de arquivos dobra milhares de
/// caminhos a cada letra digitada.
pub fn normalizar(termo: &str) -> String {
    termo.trim().chars().flat_map(|c| sem_acento(c).to_lowercase()).collect()
}

/// Onde `termo` (já normalizado) aparece em `texto`, em bytes de `texto`, sem se sobrepor.
/// `limite` corta a conta: a lista da esquerda só quer as primeiras.
fn faixas_ate(texto: &str, termo: &str, limite: usize) -> Vec<Range<usize>> {
    if termo.is_empty() || limite == 0 || texto.len() < termo.len() {
        return Vec::new();
    }
    let d = dobrar(texto);
    let mut saida = Vec::new();
    let mut de = 0;
    while let Some(k) = d.texto[de..].find(termo) {
        let achado = de + k..de + k + termo.len();
        de = achado.end;
        saida.push(d.no_original(achado));
        if saida.len() == limite {
            break;
        }
    }
    saida
}

/// Todas as ocorrências de `termo` em `texto`.
pub fn faixas(texto: &str, termo: &str) -> Vec<Range<usize>> {
    faixas_ate(texto, termo, usize::MAX)
}

/// A primeira ocorrência de `termo` em `texto` (para quem só quer saber se casou e onde).
pub fn primeira(texto: &str, termo: &str) -> Option<Range<usize>> {
    faixas_ate(texto, termo, 1).pop()
}

/// As palavras de uma busca, já dobradas: "VIP cupons" vira ["vip", "cupons"].
pub(super) fn palavras(busca: &str) -> Vec<String> {
    normalizar(busca).split_whitespace().map(str::to_string).collect()
}

/// Todas as palavras aparecem no texto (já dobrado), em qualquer ordem.
pub(super) fn tem_todas(dobrado: &str, palavras: &[String]) -> bool {
    palavras.iter().all(|p| dobrado.contains(p.as_str()))
}

/// Onde cada palavra aparece no texto original (o realce marca todas), em ordem.
pub(super) fn faixas_das_palavras(texto: &str, palavras: &[String]) -> Vec<Range<usize>> {
    let mut todas: Vec<Range<usize>> = palavras.iter().flat_map(|p| faixas(texto, p)).collect();
    todas.sort_by_key(|f| (f.start, std::cmp::Reverse(f.end)));
    // Sem sobrepor: a de dentro de outra sai.
    let mut saida: Vec<Range<usize>> = Vec::new();
    for f in todas {
        match saida.last_mut() {
            Some(ultima) if f.start < ultima.end => ultima.end = ultima.end.max(f.end),
            _ => saida.push(f),
        }
    }
    saida
}

#[cfg(test)]
mod testes {
    use super::{faixas_das_palavras, palavras, tem_todas};

    #[test]
    fn palavras_soltas_sem_acento_em_qualquer_ordem() {
        let p = palavras("vip CUPONS");
        assert_eq!(p, ["vip", "cupons"]);
        assert!(tem_todas(&super::normalizar("Cupons para grupos VIP"), &p));
        assert!(tem_todas(&super::normalizar("Integração com a expedição"), &palavras("integracao expedicao")));
        assert!(!tem_todas(&super::normalizar("Cupons"), &p));
        let texto = "Cupons para grupos VIP";
        assert_eq!(faixas_das_palavras(texto, &p), [0..6, 19..22]);
        // Acento no texto original: a faixa é a do original.
        assert_eq!(faixas_das_palavras("Integração", &palavras("integracao")), [0..12]);
    }
}
