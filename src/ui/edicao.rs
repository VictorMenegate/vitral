//! A edição de uma chamada (o Edit, o MultiEdit e o Write do Claude; o `fileChange` do Codex)
//! lida do detalhe gravado no item e posta sobre o arquivo como ele está agora (dono, 16/11/2026:
//! "tinha que ter como abrir pra ver o arquivo que editou e ver a edição"): as linhas que entraram
//! marcadas em verde, as que saíram em vermelho entre elas, como um diff no próprio arquivo, e o
//! começo de cada mudança, para as setas do painel passarem de uma à outra.
//!
//! O detalhe vem em dois formatos: o do Claude ("- " no que saiu e "+ " no que entrou, cada troca
//! do MultiEdit separada por uma linha "…", até 60 linhas de cada lado e "… mais N linhas" no
//! resto) e o diff unificado do Codex ("@@ -a,b +c,d @@", com contexto). Cada trecho é achado no
//! arquivo pelo lado novo dele (o contexto e o que entrou); o que não se acha mais (o arquivo
//! mudou depois) aparece solto no fim, com uma nota.

use gpui_kit::{Context, Window};

use crate::modelo::Item;

/// O que uma linha do diff é.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marca {
    Igual,
    Entra,
    Sai,
}

/// Um trecho de uma edição: as linhas na ordem do diff e, quando ele diz, a linha do arquivo novo
/// onde começa (1 é a primeira).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trecho {
    pub linhas: Vec<(Marca, String)>,
    pub inicio: Option<usize>,
    /// Linhas que entraram e saíram além das guardadas (o "… mais N linhas" de um lado cortado).
    pub entram_a_mais: usize,
    pub saem_a_mais: usize,
}

/// Acima disto (linhas de um lado × do outro), a troca não passa pela subsequência comum: fica o
/// começo e o fim iguais, e o meio inteiro sai e entra.
const TETO_DA_COMPARACAO: usize = 250_000;

/// Os trechos de um detalhe gravado, nos dois formatos. Um detalhe cortado no tamanho
/// ("… (cortado)") rende o que veio antes do corte.
pub fn trechos(detalhe: &str) -> Vec<Trecho> {
    let linhas: Vec<&str> = detalhe.lines().take_while(|l| *l != "… (cortado)").collect();
    if linhas.iter().any(|l| l.starts_with("@@")) {
        trechos_do_unificado(&linhas)
    } else {
        trechos_do_claude(&linhas)
    }
}

/// "@@ -3,4 +3,5 @@": o começo e o tamanho do lado novo e o tamanho do velho (sem número, 1).
fn cabecalho(linha: &str) -> Option<(usize, usize, usize)> {
    let mut partes = linha.trim_start_matches('@').split_whitespace();
    let velho = partes.next()?.strip_prefix('-')?;
    let novo = partes.next()?.strip_prefix('+')?;
    let medida = |s: &str| -> Option<(usize, usize)> {
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let ((_, n_velho), (inicio, n_novo)) = (medida(velho)?, medida(novo)?);
    Some((inicio, n_velho, n_novo))
}

fn trechos_do_unificado(linhas: &[&str]) -> Vec<Trecho> {
    let mut saida: Vec<Trecho> = Vec::new();
    // O que falta ler do trecho aberto (velho, novo); sem números no cabeçalho, até o próximo "@@".
    let mut faltam: Option<(usize, usize)> = None;
    let mut aberto = false;
    for linha in linhas {
        if linha.starts_with("@@") {
            let lido = cabecalho(linha);
            saida.push(Trecho { inicio: lido.map(|(c, _, _)| c.max(1)), ..Trecho::default() });
            faltam = lido.map(|(_, v, n)| (v, n));
            aberto = true;
            continue;
        }
        if !aberto {
            continue;
        }
        if faltam == Some((0, 0)) {
            // O trecho acabou: o que vem depois ("Moved to: …", "\ No newline") não é dele.
            aberto = false;
            continue;
        }
        let Some(trecho) = saida.last_mut() else { continue };
        let (marca, texto) = match linha.as_bytes().first() {
            Some(b' ') => (Marca::Igual, &linha[1..]),
            Some(b'-') => (Marca::Sai, &linha[1..]),
            Some(b'+') => (Marca::Entra, &linha[1..]),
            Some(b'\\') => continue,
            // Linha vazia de contexto (há quem tire o espaço do fim).
            None => (Marca::Igual, ""),
            Some(_) => {
                aberto = false;
                continue;
            }
        };
        if let Some((velho, novo)) = faltam.as_mut() {
            match marca {
                Marca::Igual => {
                    *velho = velho.saturating_sub(1);
                    *novo = novo.saturating_sub(1);
                }
                Marca::Sai => *velho = velho.saturating_sub(1),
                Marca::Entra => *novo = novo.saturating_sub(1),
            }
        }
        trecho.linhas.push((marca, texto.to_string()));
    }
    saida.retain(|t| !t.linhas.is_empty());
    saida
}

/// "- x" ou "-" (a linha vazia), "+ x" ou "+".
fn sem_sinal<'a>(linha: &'a str, sinal: char) -> Option<&'a str> {
    let resto = linha.strip_prefix(sinal)?;
    Some(resto.strip_prefix(' ').unwrap_or(resto))
}

fn trechos_do_claude(linhas: &[&str]) -> Vec<Trecho> {
    let mut saida = Vec::new();
    for bloco in linhas.split(|l| *l == "…") {
        let (mut saem, mut entram) = (Vec::new(), Vec::new());
        let (mut saem_a_mais, mut entram_a_mais) = (0, 0);
        let mut ultimo = Marca::Igual;
        for linha in bloco {
            if let Some(n) = linha.strip_prefix("… mais ").and_then(|r| r.split(' ').next()).and_then(|n| n.parse::<usize>().ok()) {
                match ultimo {
                    Marca::Sai => saem_a_mais += n,
                    Marca::Entra => entram_a_mais += n,
                    Marca::Igual => {}
                }
            } else if let Some(texto) = sem_sinal(linha, '-') {
                saem.push(texto.to_string());
                ultimo = Marca::Sai;
            } else if let Some(texto) = sem_sinal(linha, '+') {
                entram.push(texto.to_string());
                ultimo = Marca::Entra;
            }
        }
        if saem.is_empty() && entram.is_empty() {
            continue;
        }
        // Sem nada saindo, é o arquivo inteiro (o Write, ou o Edit que cria): começa na linha 1.
        let inicio = saem.is_empty().then_some(1);
        saida.push(Trecho { linhas: comparar(&saem, &entram), inicio, entram_a_mais, saem_a_mais });
    }
    saida
}

/// A troca de `antes` por `depois` linha a linha: o que ficou igual (a maior subsequência comum),
/// o que saiu e o que entrou, com as saídas antes das entradas em cada mudança.
fn comparar(antes: &[String], depois: &[String]) -> Vec<(Marca, String)> {
    let (mut i, mut j) = (0, 0);
    casar(antes, depois, TETO_DA_COMPARACAO)
        .into_iter()
        .map(|marca| {
            let linha = if marca == Marca::Entra { depois[j].clone() } else { antes[i].clone() };
            i += usize::from(marca != Marca::Entra);
            j += usize::from(marca != Marca::Sai);
            (marca, linha)
        })
        .collect()
}

/// Os passos que levam de `antes` a `depois` (linhas, ou os pedaços de uma linha): `Igual` anda
/// nos dois, `Sai` só em `antes`, `Entra` só em `depois`. É a maior subsequência comum; acima do
/// `teto` (um lado × o outro) fica o começo e o fim iguais, e o meio inteiro sai e entra.
fn casar<T: PartialEq>(antes: &[T], depois: &[T], teto: usize) -> Vec<Marca> {
    let (n, m) = (antes.len(), depois.len());
    if n.saturating_mul(m) > teto {
        let comeco = antes.iter().zip(depois).take_while(|(a, b)| a == b).count();
        let fim = antes[comeco..].iter().rev().zip(depois[comeco..].iter().rev()).take_while(|(a, b)| a == b).count();
        let mut saida = vec![Marca::Igual; comeco];
        saida.extend(std::iter::repeat_n(Marca::Sai, n - comeco - fim));
        saida.extend(std::iter::repeat_n(Marca::Entra, m - comeco - fim));
        saida.extend(std::iter::repeat_n(Marca::Igual, fim));
        return saida;
    }
    // tabela[i][j]: o tamanho da subsequência comum de antes[i..] e depois[j..].
    let mut tabela = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            tabela[i][j] = if antes[i] == depois[j] { tabela[i + 1][j + 1] + 1 } else { tabela[i + 1][j].max(tabela[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut saida = Vec::with_capacity(n + m);
    while i < n || j < m {
        if i < n && j < m && antes[i] == depois[j] {
            saida.push(Marca::Igual);
            i += 1;
            j += 1;
        } else if i < n && (j == m || tabela[i + 1][j] >= tabela[i][j + 1]) {
            saida.push(Marca::Sai);
            i += 1;
        } else {
            saida.push(Marca::Entra);
            j += 1;
        }
    }
    saida
}

/// Uma linha do diff como a trilha e o pedido de permissão o mostram (rodada do Paseo, item 1:
/// antes o lado antigo vinha inteiro em vermelho e o novo inteiro em verde, e uma palavra trocada
/// em 10 linhas eram 20 linhas coloridas).
#[derive(Clone, Debug, PartialEq)]
pub enum LinhaDoDiff {
    /// Uma linha que não mudou, de contexto.
    Igual(String),
    /// Uma linha que saiu, e os trechos dela (em bytes) que a linha nova trocou; sem trecho, a
    /// linha inteira é a mudança.
    Sai(String, Vec<std::ops::Range<usize>>),
    /// Uma linha que entrou, e os trechos dela que são novos.
    Entra(String, Vec<std::ops::Range<usize>>),
    /// Linhas iguais que ficaram de fora entre duas mudanças; com 0, a divisa entre duas trocas
    /// (as do MultiEdit, os trechos de um diff unificado).
    Pulo(usize),
    /// "… mais 40 linhas", "… (cortado)".
    Nota(String),
}

/// Quantas linhas iguais ficam em volta de cada mudança na trilha.
const CONTEXTO_DA_TRILHA: usize = 3;
/// Duas linhas juntas maiores que isto não ganham a palavra marcada (como no Paseo).
const MAX_DA_LINHA_MARCADA: usize = 1024;
/// O teto da comparação por palavras de um par de linhas.
const TETO_DAS_PALAVRAS: usize = 40_000;

/// O diff de um detalhe gravado, nos dois formatos, para a trilha: só o que mudou, com 3 linhas
/// de contexto, e a palavra trocada marcada. Vazio quando o detalhe não é um diff.
pub fn diff_da_trilha(detalhe: &str) -> Vec<LinhaDoDiff> {
    let mut saida = diff_dos_trechos(&trechos(detalhe), CONTEXTO_DA_TRILHA);
    if !saida.is_empty() && detalhe.lines().any(|l| l == "… (cortado)") {
        saida.push(LinhaDoDiff::Nota("… (cortado)".into()));
    }
    saida
}

/// O diff de uma troca (o `old_string` e o `new_string` de um Edit), para o pedido de permissão.
pub fn diff_da_troca(antes: &str, depois: &str, contexto: usize) -> Vec<LinhaDoDiff> {
    let linhas = |t: &str| t.lines().map(str::to_string).collect::<Vec<_>>();
    let trecho = Trecho { linhas: comparar(&linhas(antes), &linhas(depois)), ..Trecho::default() };
    diff_dos_trechos(&[trecho], contexto)
}

/// Os trechos, um depois do outro, cada um só com o que mudou e `contexto` linhas iguais em
/// volta, e com a palavra trocada marcada nas linhas que saíram e entraram em pares.
pub fn diff_dos_trechos(trechos: &[Trecho], contexto: usize) -> Vec<LinhaDoDiff> {
    let mut saida = Vec::new();
    for trecho in trechos {
        let mudou: Vec<usize> = trecho.linhas.iter().enumerate().filter(|(_, (m, _))| *m != Marca::Igual).map(|(i, _)| i).collect();
        if mudou.is_empty() && trecho.entram_a_mais + trecho.saem_a_mais == 0 {
            continue;
        }
        if !saida.is_empty() {
            saida.push(LinhaDoDiff::Pulo(0));
        }
        // A mudança mais perto de cada linha, antes ou depois dela: a linha fica se está no contexto.
        let perto = |i: usize| {
            let depois = mudou.partition_point(|&m| m < i);
            let dist = |m: Option<&usize>| m.map_or(usize::MAX, |&m| m.abs_diff(i));
            dist(mudou.get(depois)).min(dist(depois.checked_sub(1).and_then(|a| mudou.get(a)))) <= contexto
        };
        let (primeira, ultima) = (mudou.first().copied().unwrap_or(0), mudou.last().copied().unwrap_or(0));
        let mut puladas = 0;
        for (i, (marca, texto)) in trecho.linhas.iter().enumerate() {
            let texto = texto.replace('\t', "    ");
            match marca {
                Marca::Igual if !perto(i) => puladas += usize::from(i > primeira && i < ultima),
                _ => {
                    if puladas > 0 {
                        saida.push(LinhaDoDiff::Pulo(puladas));
                        puladas = 0;
                    }
                    saida.push(match marca {
                        Marca::Igual => LinhaDoDiff::Igual(texto),
                        Marca::Sai => LinhaDoDiff::Sai(texto, Vec::new()),
                        Marca::Entra => LinhaDoDiff::Entra(texto, Vec::new()),
                    });
                }
            }
        }
        let a_mais = trecho.entram_a_mais.max(trecho.saem_a_mais);
        if a_mais > 0 {
            saida.push(LinhaDoDiff::Nota(format!("… mais {a_mais} linhas")));
        }
    }
    marcar_palavras(&mut saida);
    saida
}

/// Em cada mudança em que saíram e entraram o mesmo número de linhas, a primeira que saiu é
/// comparada com a primeira que entrou, e assim por diante: o que difere dentro delas fica marcado.
fn marcar_palavras(linhas: &mut [LinhaDoDiff]) {
    let mut i = 0;
    while i < linhas.len() {
        let saem = linhas[i..].iter().take_while(|l| matches!(l, LinhaDoDiff::Sai(..))).count();
        let entram = linhas[i + saem..].iter().take_while(|l| matches!(l, LinhaDoDiff::Entra(..))).count();
        if saem > 0 && saem == entram {
            for k in i..i + saem {
                let (antes, depois) = linhas.split_at_mut(k + 1);
                if let (LinhaDoDiff::Sai(a, em_a), LinhaDoDiff::Entra(d, em_d)) = (&mut antes[k], &mut depois[saem - 1]) {
                    (*em_a, *em_d) = palavras_trocadas(a, d);
                }
            }
        }
        i += (saem + entram).max(1);
    }
}

/// Os pedaços de uma linha: cada palavra (letras, números e "_"), cada sequência de brancos e
/// cada sinal sozinho, com o lugar (em bytes) de cada um.
fn pedacos(linha: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    #[derive(PartialEq)]
    enum Tipo {
        Palavra,
        Branco,
        Sinal,
    }
    let tipo = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            Tipo::Palavra
        } else if c.is_whitespace() {
            Tipo::Branco
        } else {
            Tipo::Sinal
        }
    };
    let mut saida: Vec<(std::ops::Range<usize>, &str)> = Vec::new();
    let mut anterior: Option<Tipo> = None;
    for (i, c) in linha.char_indices() {
        let este = tipo(c);
        let fim = i + c.len_utf8();
        match saida.last_mut() {
            Some((faixa, _)) if este != Tipo::Sinal && anterior.as_ref() == Some(&este) => faixa.end = fim,
            _ => saida.push((i..fim, "")),
        }
        anterior = Some(este);
    }
    for (faixa, texto) in &mut saida {
        *texto = &linha[faixa.clone()];
    }
    saida
}

/// O que difere entre duas linhas, em trechos de bytes de cada uma. Sem trecho nenhum quando as
/// linhas são grandes demais ou têm menos da metade das palavras em comum: aí a linha inteira já
/// diz que mudou, e marcar quase tudo só faria barulho.
fn palavras_trocadas(antes: &str, depois: &str) -> (Vec<std::ops::Range<usize>>, Vec<std::ops::Range<usize>>) {
    let nada = (Vec::new(), Vec::new());
    if antes.len() + depois.len() > MAX_DA_LINHA_MARCADA {
        return nada;
    }
    let (a, d) = (pedacos(antes), pedacos(depois));
    if a.len().saturating_mul(d.len()) > TETO_DAS_PALAVRAS {
        return nada;
    }
    let textos_a: Vec<&str> = a.iter().map(|(_, t)| *t).collect();
    let textos_d: Vec<&str> = d.iter().map(|(_, t)| *t).collect();
    let passos = casar(&textos_a, &textos_d, TETO_DAS_PALAVRAS);
    let conta = |p: &[(std::ops::Range<usize>, &str)]| p.iter().filter(|(_, t)| !t.trim().is_empty()).count();
    let (mut i, mut j, mut em_comum) = (0, 0, 0);
    // Os trechos de cada lado e se o último pedaço igual desde o trecho anterior era só branco.
    let (mut em_a, mut em_d): (Vec<std::ops::Range<usize>>, Vec<std::ops::Range<usize>>) = (Vec::new(), Vec::new());
    let (mut so_branco_a, mut so_branco_d) = (false, false);
    let juntar = |trechos: &mut Vec<std::ops::Range<usize>>, faixa: &std::ops::Range<usize>, so_branco: bool| match trechos.last_mut() {
        Some(ultimo) if ultimo.end == faixa.start || so_branco => ultimo.end = faixa.end,
        _ => trechos.push(faixa.clone()),
    };
    for passo in passos {
        match passo {
            Marca::Igual => {
                let branco = a[i].1.trim().is_empty();
                em_comum += usize::from(!branco);
                // Um branco igual entre dois pedaços trocados não separa o trecho.
                so_branco_a = branco && em_a.last().is_some_and(|u| u.end == a[i].0.start);
                so_branco_d = branco && em_d.last().is_some_and(|u| u.end == d[j].0.start);
                i += 1;
                j += 1;
            }
            Marca::Sai => {
                juntar(&mut em_a, &a[i].0, so_branco_a);
                so_branco_a = false;
                i += 1;
            }
            Marca::Entra => {
                juntar(&mut em_d, &d[j].0, so_branco_d);
                so_branco_d = false;
                j += 1;
            }
        }
    }
    if em_comum * 2 < conta(&a).max(conta(&d)) {
        return nada;
    }
    (em_a, em_d)
}

/// O diff em texto, como a tela o mostra, para o Copiar.
pub fn texto_do_diff(linhas: &[LinhaDoDiff]) -> String {
    linhas
        .iter()
        .map(|linha| match linha {
            LinhaDoDiff::Igual(t) => format!("  {t}"),
            LinhaDoDiff::Sai(t, _) => format!("- {t}"),
            LinhaDoDiff::Entra(t, _) => format!("+ {t}"),
            LinhaDoDiff::Pulo(0) => "…".to_string(),
            LinhaDoDiff::Pulo(n) => format!("… {}", super::componentes::contagem(*n, "linha igual", "linhas iguais")),
            LinhaDoDiff::Nota(t) => t.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Quantas linhas entram e saem nos trechos (com as que passaram do corte).
pub fn contar(trechos: &[Trecho]) -> (usize, usize) {
    trechos.iter().fold((0, 0), |(e, s), t| {
        let entram = t.linhas.iter().filter(|(m, _)| *m == Marca::Entra).count() + t.entram_a_mais;
        let saem = t.linhas.iter().filter(|(m, _)| *m == Marca::Sai).count() + t.saem_a_mais;
        (e + entram, s + saem)
    })
}

/// Uma fileira do painel com a edição à vista.
#[derive(Clone, Debug, PartialEq)]
pub enum Fileira {
    /// Uma linha do arquivo de agora (0 é a primeira) e se ela entrou nesta edição.
    Arquivo { linha: usize, entrou: bool },
    /// Uma linha que a edição tirou: não está mais no arquivo.
    Saiu(String),
    /// Uma linha de um trecho que não se acha mais no arquivo de agora.
    Solta(Marca, String),
    /// Uma nota no meio ("… mais 12 linhas saíram", "Não está mais no arquivo").
    Nota(String),
}

/// O começo de uma mudança (as linhas seguidas que entraram ou saíram): a fileira dela e as
/// linhas do arquivo de agora que ela ocupa (1 é a primeira), para mencionar o trecho.
#[derive(Clone, Debug, PartialEq)]
pub struct Mudanca {
    pub fileira: usize,
    pub linhas: Option<(usize, usize)>,
}

/// O arquivo com a edição posta nele.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vista {
    pub fileiras: Vec<Fileira>,
    pub mudancas: Vec<Mudanca>,
    /// Trechos que não se acham mais no arquivo (ele mudou depois da edição).
    pub perdidos: usize,
}

/// A linha como o painel a compara: tabulação em 4 espaços (é como ele desenha) e sem o branco do
/// fim (uma ferramenta tira, outra não).
fn normal(linha: &str) -> String {
    linha.replace('\t', "    ").trim_end().to_string()
}

/// Onde o lado novo do trecho está no arquivo: a posição mais perto da linha que o diff diz (ou a
/// primeira), fora das já ocupadas por outro trecho.
fn achar(novo: &[String], arquivo: &[String], inicio: Option<usize>, ocupadas: &[(usize, usize)]) -> Option<usize> {
    if novo.is_empty() || novo.len() > arquivo.len() {
        return None;
    }
    let livre = |p: usize| ocupadas.iter().all(|&(a, b)| p + novo.len() <= a || p >= b);
    let lugares = (0..=arquivo.len() - novo.len()).filter(|&p| livre(p) && arquivo[p..p + novo.len()] == *novo);
    match inicio {
        Some(linha) => lugares.min_by_key(|p| p.abs_diff(linha.saturating_sub(1))),
        None => lugares.into_iter().next(),
    }
}

/// Acompanha as mudanças enquanto as fileiras saem: cada sequência de linhas que entraram ou
/// saíram vira uma `Mudanca`, com as linhas do arquivo que ela ocupa.
#[derive(Default)]
struct Montagem {
    vista: Vista,
    /// A mudança aberta: a fileira dela e as linhas do arquivo que entraram (primeira, última).
    aberta: Option<(usize, Option<(usize, usize)>)>,
}

impl Montagem {
    fn igual(&mut self, fileira: Fileira, proxima: Option<usize>) {
        self.fechar(proxima);
        self.vista.fileiras.push(fileira);
    }

    /// Uma linha que entrou ou saiu; `linha`, a do arquivo (1 é a primeira) quando ela está nele.
    fn mudou(&mut self, fileira: Fileira, linha: Option<usize>) {
        let indice = self.vista.fileiras.len();
        let (_, linhas) = self.aberta.get_or_insert((indice, None));
        if let Some(l) = linha {
            *linhas = Some(linhas.map_or((l, l), |(a, b)| (a.min(l), b.max(l))));
        }
        self.vista.fileiras.push(fileira);
    }

    /// Fecha a mudança aberta; a que só tirou linhas fica com a linha de depois dela (`proxima`).
    fn fechar(&mut self, proxima: Option<usize>) {
        if let Some((fileira, linhas)) = self.aberta.take() {
            let linhas = linhas.or(proxima.map(|l| (l, l)));
            self.vista.mudancas.push(Mudanca { fileira, linhas });
        }
    }
}

/// Põe os trechos sobre as linhas do arquivo de agora.
pub fn vista(trechos: &[Trecho], arquivo: &[impl AsRef<str>]) -> Vista {
    let arquivo: Vec<String> = arquivo.iter().map(|l| normal(l.as_ref())).collect();
    let total = arquivo.len();
    // Onde cada trecho achado começa e termina no arquivo (com as linhas a mais do corte).
    let mut achados: Vec<(usize, usize, usize)> = Vec::new();
    let mut perdidos: Vec<usize> = Vec::new();
    for (t, trecho) in trechos.iter().enumerate() {
        let novo: Vec<String> = trecho.linhas.iter().filter(|(m, _)| *m != Marca::Sai).map(|(_, l)| normal(l)).collect();
        let ocupadas: Vec<(usize, usize)> = achados.iter().map(|&(a, b, _)| (a, b)).collect();
        match achar(&novo, &arquivo, trecho.inicio, &ocupadas) {
            Some(p) => achados.push((p, (p + novo.len() + trecho.entram_a_mais).min(total), t)),
            None => perdidos.push(t),
        }
    }
    achados.sort();
    let mut m = Montagem::default();
    let mut cursor = 0;
    for &(comeco, fim, t) in &achados {
        for linha in cursor..comeco {
            m.igual(Fileira::Arquivo { linha, entrou: false }, Some(linha + 1));
        }
        let trecho = &trechos[t];
        let mut f = comeco;
        let ultima_saida = trecho.linhas.iter().rposition(|(marca, _)| *marca == Marca::Sai);
        for (k, (marca, texto)) in trecho.linhas.iter().enumerate() {
            match marca {
                Marca::Igual => {
                    m.igual(Fileira::Arquivo { linha: f, entrou: false }, Some(f + 1));
                    f += 1;
                }
                Marca::Entra => {
                    m.mudou(Fileira::Arquivo { linha: f, entrou: true }, Some(f + 1));
                    f += 1;
                }
                Marca::Sai => {
                    m.mudou(Fileira::Saiu(texto.clone()), None);
                    if Some(k) == ultima_saida && trecho.saem_a_mais > 0 {
                        m.mudou(Fileira::Nota(format!("… mais {} saíram", super::componentes::contagem(trecho.saem_a_mais, "linha", "linhas"))), None);
                    }
                }
            }
        }
        while f < fim {
            m.mudou(Fileira::Arquivo { linha: f, entrou: true }, Some(f + 1));
            f += 1;
        }
        m.fechar((f < total).then_some(f + 1).or((f > 0).then_some(f)));
        cursor = f.max(cursor);
    }
    for linha in cursor..total {
        m.igual(Fileira::Arquivo { linha, entrou: false }, Some(linha + 1));
    }
    m.fechar(None);
    if !perdidos.is_empty() {
        let nota = if perdidos.len() == trechos.len() {
            "O arquivo mudou depois desta edição. O que ela trocou:".to_string()
        } else {
            format!("{} não está mais no arquivo:", super::componentes::contagem(perdidos.len(), "trecho", "trechos"))
        };
        m.igual(Fileira::Nota(nota), None);
        for (i, &t) in perdidos.iter().enumerate() {
            if i > 0 {
                m.igual(Fileira::Nota("…".into()), None);
            }
            for (marca, texto) in &trechos[t].linhas {
                match marca {
                    Marca::Igual => m.igual(Fileira::Solta(*marca, texto.clone()), None),
                    _ => m.mudou(Fileira::Solta(*marca, texto.clone()), None),
                }
            }
            m.fechar(None);
        }
    }
    let mut vista = m.vista;
    vista.perdidos = perdidos.len();
    vista
}

/// Os trechos de uma chamada de edição gravada (vazio quando ela não guardou o diff).
pub fn trechos_do_item(item: &Item) -> Vec<Trecho> {
    match item {
        Item::Ferramenta { detalhe: Some(detalhe), erro: false, .. } => trechos(detalhe),
        _ => Vec::new(),
    }
}

/// A edição pedida para o painel: os trechos (de uma chamada ou de várias no mesmo arquivo) e
/// quanto entrou e saiu. `sem_registro`: a chamada é de antes de guardar o diff, e o painel mostra
/// o arquivo como está agora, com um aviso.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edicao {
    pub trechos: Vec<Trecho>,
    pub entram: usize,
    pub saem: usize,
    pub sem_registro: bool,
}

/// Abre no painel da direita o arquivo de uma ou mais chamadas de edição de um chat, já na edição:
/// os índices são dos itens do chat; `caminho`, quando vem, é o arquivo (o registro antigo do
/// Codex guardava vários numa chamada só).
#[derive(Clone, PartialEq, Debug, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = vitral, no_json)]
pub struct AbrirEdicao {
    pub chat: String,
    pub itens: Vec<usize>,
    pub caminho: Option<String>,
}

impl super::Janela {
    pub(super) fn abrir_edicao(&mut self, pedido: &AbrirEdicao, window: &mut Window, cx: &mut Context<Self>) {
        use super::componentes::{Tom, aviso};
        use gpui_kit::component::WindowExt as _;
        let Some(motor) = self.motor.clone() else { return };
        let (cwd, itens, caminho, raizes) = {
            let m = motor.read(cx);
            let Some(chat) = m.chat(&pedido.chat) else { return };
            let itens: Vec<Item> = pedido.itens.iter().filter_map(|&i| chat.itens.get(i).cloned()).collect();
            let caminho = pedido.caminho.clone().or_else(|| {
                itens.iter().find_map(|i| match i {
                    Item::Ferramenta { arquivo: Some(a), .. } => Some(a.clone()),
                    _ => None,
                })
            });
            (chat.cwd.clone(), itens, caminho, m.chats.iter().map(|c| c.cwd.clone()).collect::<Vec<_>>())
        };
        let Some(caminho) = caminho else { return };
        let escrito = std::path::Path::new(&caminho);
        let inteiro = if escrito.is_absolute() { escrito.to_path_buf() } else { cwd.join(escrito) };
        let inteiro = dunce::canonicalize(&inteiro).unwrap_or(inteiro);
        let de_fora = || Some((inteiro.parent()?.to_path_buf(), inteiro.file_name()?.to_string_lossy().into_owned()));
        let Some((raiz, relativo)) = super::links_arquivo::raiz_do_arquivo(&inteiro, raizes.into_iter()).or_else(de_fora) else {
            window.push_notification(aviso(Tom::Atencao, format!("Não deu para abrir {caminho}.")), cx);
            return;
        };
        let trechos: Vec<Trecho> = itens.iter().flat_map(trechos_do_item).collect();
        let (mut entram, mut saem) = contar(&trechos);
        // O +N −M gravado quando a chamada chegou vale mais que o do detalhe, que é cortado.
        let gravados: Vec<(usize, usize)> = itens.iter().filter_map(super::mensagens::linhas_da_edicao_gravada).collect();
        if gravados.len() == itens.len() && !gravados.is_empty() {
            (entram, saem) = gravados.iter().fold((0, 0), |(e, s), (e2, s2)| (e + e2, s + s2));
        }
        let sem_registro = trechos.is_empty();
        self.edicao_pedida = Some(std::rc::Rc::new(Edicao { trechos, entram, saem, sem_registro }));
        self.abrir_arquivo(raiz, relativo, window, cx);
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn igual(t: &str) -> (Marca, String) {
        (Marca::Igual, t.into())
    }
    fn entra(t: &str) -> (Marca, String) {
        (Marca::Entra, t.into())
    }
    fn sai(t: &str) -> (Marca, String) {
        (Marca::Sai, t.into())
    }

    #[test]
    fn a_troca_do_claude_vira_o_diff_com_o_que_ficou_igual() {
        let t = trechos("- fn a() {\n-     1\n- }\n+ fn a() {\n+     2\n+ }");
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].linhas, vec![igual("fn a() {"), sai("    1"), entra("    2"), igual("}")]);
        assert_eq!(t[0].inicio, None);
        assert_eq!(contar(&t), (1, 1));
        // A linha vazia ("- " e "+") e o MultiEdit, separado por "…".
        let t = trechos("- a\n- \n+ b\n…\n- c\n+ d");
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].linhas, vec![sai("a"), sai(""), entra("b")]);
        assert_eq!(t[1].linhas, vec![sai("c"), entra("d")]);
    }

    #[test]
    fn o_write_comeca_no_topo_e_o_corte_conta_o_resto() {
        let t = trechos("+ um\n+ dois\n… mais 40 linhas");
        assert_eq!(t[0].linhas, vec![entra("um"), entra("dois")]);
        assert_eq!((t[0].inicio, t[0].entram_a_mais), (Some(1), 40));
        assert_eq!(contar(&t), (42, 0));
        let t = trechos("- x\n… mais 3 linhas\n+ y");
        assert_eq!((t[0].saem_a_mais, t[0].entram_a_mais), (3, 0));
        // O detalhe cortado no tamanho rende o que veio antes.
        let t = trechos("+ a\n+ b\n… (cortado)");
        assert_eq!(t[0].linhas, vec![entra("a"), entra("b")]);
    }

    #[test]
    fn le_o_diff_unificado_do_codex() {
        let diff = "--- a/x.rs\n+++ b/x.rs\n@@ -1,3 +1,3 @@\n use a;\n-fn b() {}\n+fn c() {}\n \n@@ -10 +10,2 @@\n-x\n+y\n+z\n\\ No newline at end of file\n\nMoved to: /p/y.rs";
        let t = trechos(diff);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].linhas, vec![igual("use a;"), sai("fn b() {}"), entra("fn c() {}"), igual("")]);
        assert_eq!(t[0].inicio, Some(1));
        assert_eq!(t[1].linhas, vec![sai("x"), entra("y"), entra("z")]);
        assert_eq!(t[1].inicio, Some(10));
        assert_eq!(contar(&t), (3, 2));
        // O apagado (o diff só de saídas que o Vitral monta) começa no "+0".
        let t = trechos("@@ -1,2 +0,0 @@\n-x\n-y");
        assert_eq!((t[0].linhas.clone(), t[0].inicio), (vec![sai("x"), sai("y")], Some(1)));
    }

    #[test]
    fn a_edicao_fica_no_arquivo_com_as_saidas_entre_as_linhas() {
        let arquivo = ["use a;", "", "fn a() {", "    2", "}", "fim"];
        let t = trechos("- fn a() {\n-     1\n- }\n+ fn a() {\n+     2\n+ }");
        let v = vista(&t, &arquivo);
        assert_eq!(
            v.fileiras,
            vec![
                Fileira::Arquivo { linha: 0, entrou: false },
                Fileira::Arquivo { linha: 1, entrou: false },
                Fileira::Arquivo { linha: 2, entrou: false },
                Fileira::Saiu("    1".into()),
                Fileira::Arquivo { linha: 3, entrou: true },
                Fileira::Arquivo { linha: 4, entrou: false },
                Fileira::Arquivo { linha: 5, entrou: false },
            ]
        );
        assert_eq!(v.mudancas, vec![Mudanca { fileira: 3, linhas: Some((4, 4)) }]);
        assert_eq!(v.perdidos, 0);
    }

    #[test]
    fn duas_mudancas_e_a_que_so_tirou_fica_com_a_linha_de_depois() {
        let arquivo = ["a", "b", "c", "d", "e"];
        let t = trechos("@@ -1,4 +1,3 @@\n a\n-x\n b\n-y\n+c\n d");
        let v = vista(&t, &arquivo);
        assert_eq!(v.mudancas, vec![Mudanca { fileira: 1, linhas: Some((2, 2)) }, Mudanca { fileira: 3, linhas: Some((3, 3)) }]);
        assert_eq!(v.fileiras[1], Fileira::Saiu("x".into()));
        assert_eq!(v.fileiras.len(), 7);
    }

    #[test]
    fn a_linha_do_diff_desempata_e_o_corte_marca_o_resto() {
        // "}" aparece duas vezes: o diff diz linha 4.
        let arquivo = ["x", "}", "y", "}", "z"];
        let t = trechos("@@ -4 +4 @@\n-{\n+}");
        let v = vista(&t, &arquivo);
        assert_eq!(v.mudancas[0].linhas, Some((4, 4)));
        // O Write cortado: as 2 guardadas e mais 2 marcadas.
        let arquivo = ["um", "dois", "tres", "quatro", "cinco"];
        let v = vista(&trechos("+ um\n+ dois\n… mais 2 linhas"), &arquivo);
        let entraram: Vec<usize> =
            v.fileiras.iter().filter_map(|f| matches!(f, Fileira::Arquivo { entrou: true, .. }).then_some(0)).collect();
        assert_eq!(entraram.len(), 4);
        assert_eq!(v.mudancas, vec![Mudanca { fileira: 0, linhas: Some((1, 4)) }]);
    }

    #[test]
    fn o_trecho_que_sumiu_do_arquivo_aparece_solto_no_fim() {
        let arquivo = ["a", "b"];
        let t = trechos("- velho\n+ novo");
        let v = vista(&t, &arquivo);
        assert_eq!(v.perdidos, 1);
        assert_eq!(v.fileiras[2], Fileira::Nota("O arquivo mudou depois desta edição. O que ela trocou:".into()));
        assert_eq!(v.fileiras[3], Fileira::Solta(Marca::Sai, "velho".into()));
        assert_eq!(v.mudancas, vec![Mudanca { fileira: 3, linhas: None }]);
        // Tabulação e branco no fim não atrapalham.
        let v = vista(&trechos("- x\n+ \tfoo  "), &["    foo"]);
        assert_eq!(v.perdidos, 0);
    }

    // ---- O diff da trilha (rodada do Paseo, item 1): só o que mudou, com contexto, e a palavra
    // trocada marcada dentro da linha. ----

    fn ctx(t: &str) -> LinhaDoDiff {
        LinhaDoDiff::Igual(t.into())
    }
    fn detalhe_da_troca(antes: &[String], depois: &[String]) -> String {
        let lado = |linhas: &[String], sinal: &str| linhas.iter().map(|l| format!("{sinal} {l}")).collect::<Vec<_>>().join("\n");
        format!("{}\n{}", lado(antes, "-"), lado(depois, "+"))
    }

    #[test]
    fn uma_palavra_trocada_em_dez_linhas_mostra_uma_linha_com_tres_de_contexto() {
        let antes: Vec<String> = (1..=10).map(|i| format!("linha {i}")).collect();
        let mut depois = antes.clone();
        depois[5] = "linha seis".into();
        let diff = diff_da_trilha(&detalhe_da_troca(&antes, &depois));
        assert_eq!(
            diff,
            vec![
                ctx("linha 3"),
                ctx("linha 4"),
                ctx("linha 5"),
                LinhaDoDiff::Sai("linha 6".into(), vec![6..7]),
                LinhaDoDiff::Entra("linha seis".into(), vec![6..10]),
                ctx("linha 7"),
                ctx("linha 8"),
                ctx("linha 9"),
            ]
        );
        // Antes eram 10 vermelhas e 10 verdes.
        let mudadas = diff.iter().filter(|l| matches!(l, LinhaDoDiff::Sai(..) | LinhaDoDiff::Entra(..))).count();
        assert_eq!(mudadas, 2);
    }

    #[test]
    fn duas_mudancas_longe_uma_da_outra_tem_um_pulo_no_meio() {
        let antes: Vec<String> = (1..=20).map(|i| format!("l{i}")).collect();
        let mut depois = antes.clone();
        depois[1] = "L2".into();
        depois[17] = "L18".into();
        let diff = diff_da_trilha(&detalhe_da_troca(&antes, &depois));
        // l1, a troca do l2, l3 a l5, o pulo de l6 a l14 (9 linhas), l15 a l17, a troca do l18, l19 e l20.
        assert_eq!(diff[0], ctx("l1"));
        assert_eq!(diff[5], ctx("l5"));
        assert_eq!(diff[6], LinhaDoDiff::Pulo(9));
        assert_eq!(diff[7], ctx("l15"));
        assert_eq!(diff.len(), 14);
        // Mudanças perto uma da outra dividem o contexto, sem pulo.
        let mut perto = antes.clone();
        perto[1] = "L2".into();
        perto[6] = "L7".into();
        let diff = diff_da_trilha(&detalhe_da_troca(&antes, &perto));
        assert!(!diff.iter().any(|l| matches!(l, LinhaDoDiff::Pulo(_))));
    }

    #[test]
    fn a_palavra_que_entrou_e_marcada_e_linhas_sem_nada_em_comum_nao() {
        // Só entrou um pedaço: nada marcado no que saiu.
        assert_eq!(
            diff_da_troca("    total = subtotal", "    total = subtotal - desconto", 3),
            vec![
                LinhaDoDiff::Sai("    total = subtotal".into(), vec![]),
                LinhaDoDiff::Entra("    total = subtotal - desconto".into(), vec![20..31]),
            ]
        );
        // Duas palavras seguidas trocadas viram um trecho só (o espaço entre elas vai junto).
        assert_eq!(
            diff_da_troca("let a = foo bar;", "let a = baz qux;", 3),
            vec![LinhaDoDiff::Sai("let a = foo bar;".into(), vec![8..15]), LinhaDoDiff::Entra("let a = baz qux;".into(), vec![8..15])]
        );
        // Linhas que não se parecem: a linha inteira já diz que mudou, sem marcar palavra.
        assert_eq!(
            diff_da_troca("fn a() {", "pub struct Coisa;", 3),
            vec![LinhaDoDiff::Sai("fn a() {".into(), vec![]), LinhaDoDiff::Entra("pub struct Coisa;".into(), vec![])]
        );
        // Acentos: os trechos caem em fronteira de letra.
        let diff = diff_da_troca("// não é ação", "// não é reação", 3);
        let LinhaDoDiff::Entra(texto, trechos) = &diff[1] else { panic!("{diff:?}") };
        assert_eq!(trechos.iter().map(|t| &texto[t.clone()]).collect::<Vec<_>>(), ["reação"]);
        // Três linhas trocadas por três: cada uma com o par dela.
        let diff = diff_da_troca("a = 1\nb = 2\nc = 3", "a = 10\nb = 20\nc = 30", 3);
        assert_eq!(diff[0], LinhaDoDiff::Sai("a = 1".into(), vec![4..5]));
        assert_eq!(diff[3], LinhaDoDiff::Entra("a = 10".into(), vec![4..6]));
        assert_eq!(diff[5], LinhaDoDiff::Entra("c = 30".into(), vec![4..6]));
        // Tabulação vira 4 espaços, e os trechos contam no texto já trocado.
        assert_eq!(
            diff_da_troca("\tx = 1", "\tx = 2", 3),
            vec![LinhaDoDiff::Sai("    x = 1".into(), vec![8..9]), LinhaDoDiff::Entra("    x = 2".into(), vec![8..9])]
        );
    }

    #[test]
    fn o_multiedit_separa_as_trocas_e_o_write_fica_inteiro() {
        let diff = diff_da_trilha("- a\n+ b\n…\n- c\n+ d");
        assert_eq!(
            diff,
            vec![
                LinhaDoDiff::Sai("a".into(), vec![]),
                LinhaDoDiff::Entra("b".into(), vec![]),
                LinhaDoDiff::Pulo(0),
                LinhaDoDiff::Sai("c".into(), vec![]),
                LinhaDoDiff::Entra("d".into(), vec![]),
            ]
        );
        // O Write: tudo entrou, sem contexto para cortar, e o que passou do corte vira nota.
        let diff = diff_da_trilha("+ um\n+ dois\n… mais 40 linhas");
        assert_eq!(
            diff,
            vec![LinhaDoDiff::Entra("um".into(), vec![]), LinhaDoDiff::Entra("dois".into(), vec![]), LinhaDoDiff::Nota("… mais 40 linhas".into())]
        );
        // O detalhe cortado no tamanho avisa no fim.
        assert_eq!(diff_da_trilha("+ a\n… (cortado)").last(), Some(&LinhaDoDiff::Nota("… (cortado)".into())));
        // O que não é diff (um detalhe de outra ferramenta) não rende nada: a trilha mostra como veio.
        assert!(diff_da_trilha("$ ls -la").is_empty());
    }

    #[test]
    fn o_diff_unificado_do_codex_ganha_a_palavra_marcada() {
        let diff = diff_da_trilha("--- a/x.rs\n+++ b/x.rs\n@@ -1,3 +1,3 @@\n use a;\n-fn b() {}\n+fn c() {}\n fim\n@@ -10 +10 @@\n-x\n+y");
        assert_eq!(
            diff,
            vec![
                ctx("use a;"),
                LinhaDoDiff::Sai("fn b() {}".into(), vec![3..4]),
                LinhaDoDiff::Entra("fn c() {}".into(), vec![3..4]),
                ctx("fim"),
                LinhaDoDiff::Pulo(0),
                LinhaDoDiff::Sai("x".into(), vec![]),
                LinhaDoDiff::Entra("y".into(), vec![]),
            ]
        );
    }

    #[test]
    fn a_troca_grande_demais_respeita_o_teto_e_o_texto_copiado_e_um_diff() {
        // 600 × 600 linhas passa do teto da comparação: o começo e o fim iguais, o meio sai e entra.
        let antes: Vec<String> = (0..600).map(|i| format!("a{i}")).collect();
        let mut depois = antes.clone();
        depois[300] = "outra".into();
        let diff = diff_da_troca(&antes.join("\n"), &depois.join("\n"), 3);
        assert_eq!(diff.len(), 8);
        assert_eq!(diff[3], LinhaDoDiff::Sai("a300".into(), vec![]));
        // O Copiar leva o diff como a tela o mostra.
        let diff = diff_da_trilha("- fn a() {\n-     1\n- }\n+ fn a() {\n+     2\n+ }\n…\n- x\n+ y");
        assert_eq!(texto_do_diff(&diff), "  fn a() {\n-     1\n+     2\n  }\n…\n- x\n+ y");
    }
}
