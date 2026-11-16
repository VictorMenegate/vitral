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
    let (n, m) = (antes.len(), depois.len());
    if n.saturating_mul(m) > TETO_DA_COMPARACAO {
        let comeco = antes.iter().zip(depois).take_while(|(a, b)| a == b).count();
        let fim = antes[comeco..].iter().rev().zip(depois[comeco..].iter().rev()).take_while(|(a, b)| a == b).count();
        let mut saida: Vec<(Marca, String)> = antes[..comeco].iter().map(|l| (Marca::Igual, l.clone())).collect();
        saida.extend(antes[comeco..n - fim].iter().map(|l| (Marca::Sai, l.clone())));
        saida.extend(depois[comeco..m - fim].iter().map(|l| (Marca::Entra, l.clone())));
        saida.extend(antes[n - fim..].iter().map(|l| (Marca::Igual, l.clone())));
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
            saida.push((Marca::Igual, antes[i].clone()));
            i += 1;
            j += 1;
        } else if i < n && (j == m || tabela[i + 1][j] >= tabela[i][j + 1]) {
            saida.push((Marca::Sai, antes[i].clone()));
            i += 1;
        } else {
            saida.push((Marca::Entra, depois[j].clone()));
            j += 1;
        }
    }
    saida
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
}
