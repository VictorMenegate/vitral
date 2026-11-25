//! As menções de arquivo ("@src/ui/fila.rs#L120-134") como texto e como pílula (dono, 16/11/2026:
//! "faz as menções ficarem de uma forma melhor no chat"). O texto que vai ao agente continua o
//! caminho inteiro, com as linhas no formato das menções do Claude Code (`#L120-134`); na tela,
//! a menção vira uma pílula com o ícone do tipo, o nome curto e as linhas ("fila.rs:120-134"),
//! no balão da mensagem enviada e na caixa enquanto se escreve.

use std::ops::Range;

use gpui_kit::component::text::{InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast};
use gpui_kit::component::tooltip::{ManagedTooltipExt as _, Tooltip};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tema::{Cores, FONTE_MONO, Lucide, Tipo as _, esp, linear, r, raio};

/// Uma menção: o caminho como vai no texto (relativo à pasta do chat, ou inteiro) e as linhas
/// (a primeira e a última; iguais numa linha só).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mencao {
    pub caminho: String,
    pub linhas: Option<(u32, u32)>,
    /// O pedaço exato, quando a menção é de parte das linhas (` «…»` depois delas).
    pub trecho: Option<String>,
}

/// O texto da menção que vai na mensagem: "@src/x.rs", "@src/x.rs#L12", "@src/x.rs#L12-20". Um
/// caminho com espaço vai entre aspas, e as linhas depois delas: `@"minha pasta/x.rs"#L12`.
pub fn texto_da_mencao(caminho: &str, linhas: Option<(u32, u32)>) -> String {
    let inicio = if caminho.contains(char::is_whitespace) { format!("@\"{caminho}\"") } else { format!("@{caminho}") };
    match linhas {
        Some((a, b)) if b > a => format!("{inicio}#L{a}-{b}"),
        Some((a, _)) => format!("{inicio}#L{a}"),
        None => inicio,
    }
}

/// A menção com o pedaço exato de parte das linhas: "@src/x.rs#L12 «cabeNaFabrica(c)»". As quebras
/// do trecho vão como `\n` (a pílula da caixa é uma linha só); com « ou » dentro dele não cabe no
/// formato, e vai só com as linhas.
pub fn texto_da_mencao_com(caminho: &str, linhas: Option<(u32, u32)>, trecho: Option<&str>) -> String {
    let base = texto_da_mencao(caminho, linhas);
    match trecho.filter(|t| !t.trim().is_empty() && !t.contains(['«', '»']) && linhas.is_some()) {
        Some(t) => format!("{base} «{}»", t.replace('\n', "\\n")),
        None => base,
    }
}

/// O trecho que vem logo depois de uma menção (` «…»`): ele e quantos bytes ocupa.
fn ler_trecho(depois: &str) -> Option<(String, usize)> {
    let dentro = depois.strip_prefix(" «")?;
    let fim = dentro.find('»')?;
    let trecho = &dentro[..fim];
    (!trecho.trim().is_empty() && !trecho.contains(['«', '\n'])).then(|| (trecho.replace("\\n", "\n"), " «".len() + fim + "»".len()))
}

/// O começo do trecho numa linha só, para a pílula: sem o recuo e cortado em `maximo` letras.
pub fn trecho_curto(trecho: &str, maximo: usize) -> String {
    let linha = trecho.trim_start().lines().next().unwrap_or("").trim_end();
    if linha.chars().count() > maximo {
        let mut curto: String = linha.chars().take(maximo).collect();
        curto.push('…');
        curto
    } else if trecho.trim().lines().count() > 1 {
        format!("{linha}…")
    } else {
        linha.to_string()
    }
}

/// O que a pílula mostra: o nome curto e as linhas ("fila.rs:120-134"); a pasta, com a barra.
pub fn rotulo_da_mencao(caminho: &str, linhas: Option<(u32, u32)>) -> String {
    let pasta = caminho.ends_with('/');
    let sem_barra = caminho.trim_end_matches('/');
    let nome = sem_barra.rsplit('/').next().unwrap_or(sem_barra);
    let nome = if pasta { format!("{nome}/") } else { nome.to_string() };
    match linhas {
        Some((a, b)) if b > a => format!("{nome}:{a}-{b}"),
        Some((a, _)) => format!("{nome}:{a}"),
        None => nome,
    }
}

/// As linhas escritas depois do caminho: "#L12", "#L12-20", "#L12-L20", ":12", ":12-20".
fn ler_linhas(sufixo: &str) -> Option<(u32, u32)> {
    let resto = sufixo.strip_prefix("#L").or_else(|| sufixo.strip_prefix(':'))?;
    let numero = |s: &str| s.parse::<u32>().ok().filter(|n| *n > 0);
    match resto.split_once('-') {
        Some((a, b)) => {
            let (a, b) = (numero(a)?, numero(b.trim_start_matches('L'))?);
            Some((a.min(b), a.max(b)))
        }
        None => numero(resto).map(|a| (a, a)),
    }
}

/// O que pode fazer parte do caminho de uma menção.
fn do_caminho(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '~' | '+' | '#' | ':' | '@')
}

/// A menção que começa em `texto` (pelo "@"): ela e quantos bytes ocupa. A pontuação da frase
/// em volta fica de fora ("@a.rs," e "(@a.rs)").
pub fn ler_mencao(texto: &str) -> Option<(Mencao, usize)> {
    let resto = texto.strip_prefix('@')?;
    if let Some(entre_aspas) = resto.strip_prefix('"') {
        return ler_mencao_entre_aspas(entre_aspas);
    }
    let fim = resto.find(|c: char| !do_caminho(c)).unwrap_or(resto.len());
    let bruto = resto[..fim].trim_end_matches(['.', ',', ';', ':', '!', '?']);
    if bruto.is_empty() {
        return None;
    }
    // O sufixo de linhas: "#L…" ou o primeiro ":" depois da última barra seguido de número.
    let barra = bruto.rfind('/').map_or(0, |i| i + 1);
    let corte = bruto[barra..]
        .find('#')
        .or_else(|| bruto[barra..].find(':'))
        .map(|i| barra + i);
    let (caminho, linhas) = match corte {
        Some(i) => match ler_linhas(&bruto[i..]) {
            Some(linhas) => (&bruto[..i], Some(linhas)),
            None => (bruto, None),
        },
        None => (bruto, None),
    };
    let caminho = caminho.trim_end_matches(['#', ':']);
    if caminho.is_empty() || caminho.contains('#') || caminho.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let tamanho = 1 + bruto.len();
    let (trecho, a_mais) = match ler_trecho(&texto[tamanho..]).filter(|_| linhas.is_some()) {
        Some((t, n)) => (Some(t), n),
        None => (None, 0),
    };
    Some((Mencao { caminho: caminho.to_string(), linhas, trecho }, tamanho + a_mais))
}

/// A menção `@"caminho com espaço"`, já sem o `@"`: o que está entre as aspas (na mesma linha) e,
/// logo depois delas, as linhas ("#L3-5"). Devolve também quantos bytes a menção inteira ocupa.
fn ler_mencao_entre_aspas(depois_das_aspas: &str) -> Option<(Mencao, usize)> {
    let fim = depois_das_aspas.find(['"', '\n'])?;
    if !depois_das_aspas[fim..].starts_with('"') || depois_das_aspas[..fim].trim().is_empty() {
        return None;
    }
    let caminho = depois_das_aspas[..fim].to_string();
    let depois = &depois_das_aspas[fim + 1..];
    let sufixo = &depois[..depois.find(|c: char| !do_caminho(c)).unwrap_or(depois.len())];
    let sufixo = sufixo.trim_end_matches(['.', ',', ';', ':', '!', '?']);
    let linhas = ler_linhas(sufixo);
    let usado = if linhas.is_some() { sufixo.len() } else { 0 };
    let (trecho, a_mais) = match ler_trecho(&depois[usado..]).filter(|_| linhas.is_some()) {
        Some((t, n)) => (Some(t), n),
        None => (None, 0),
    };
    Some((Mencao { caminho, linhas, trecho }, 2 + fim + 1 + usado + a_mais))
}

/// As menções do texto, com onde cada uma está: "@" no começo ou depois de um espaço ou de um
/// parêntese, fora de bloco de código e de código em linha. O que não tem cara de arquivo (o
/// "@claude" da dupla, o "@12" de um chat) sai aqui também; quem decide é o disco.
pub fn mencoes_no_texto(texto: &str) -> Vec<(Range<usize>, Mencao)> {
    let mut achadas = Vec::new();
    let mut cerca = false;
    let mut inicio_da_linha = 0;
    for linha in texto.split_inclusive('\n') {
        let aparada = linha.trim_start();
        if aparada.starts_with("```") || aparada.starts_with("~~~") {
            cerca = !cerca;
        } else if !cerca {
            let mut crase = false;
            let mut anterior: Option<char> = None;
            for (i, c) in linha.char_indices() {
                if c == '`' {
                    crase = !crase;
                } else if c == '@' && !crase && anterior.is_none_or(|a| a.is_whitespace() || a == '(')
                    && let Some((mencao, tamanho)) = ler_mencao(&linha[i..])
                {
                    let comeco = inicio_da_linha + i;
                    achadas.push((comeco..comeco + tamanho, mencao));
                }
                anterior = Some(c);
            }
        }
        inicio_da_linha += linha.len();
    }
    // Uma menção que começa dentro de outra (o "@" do meio de um caminho) não conta.
    let mut fim = 0;
    achadas.retain(|(faixa, _)| {
        let fica = faixa.start >= fim;
        if fica {
            fim = faixa.end;
        }
        fica
    });
    achadas
}

/// O endereço da pílula: `vitral:mencao/<linhas>/<caminho inteiro em %XX>`, com "120-134", "120"
/// ou "0" nas linhas. O clique abre o arquivo no painel da direita, com as linhas marcadas.
pub fn link_da_mencao(inteiro: &str, linhas: Option<(u32, u32)>) -> String {
    let base = super::links_arquivo::link_de(inteiro, None);
    let codificado = base.strip_prefix("vitral:arquivo/0/").unwrap_or_default();
    let faixa = match linhas {
        Some((a, b)) if b > a => format!("{a}-{b}"),
        Some((a, _)) => a.to_string(),
        None => "0".into(),
    };
    format!("vitral:mencao/{faixa}/{codificado}")
}

/// O endereço da pílula com o trecho: o de sempre e `?t=` com o trecho em `%XX`.
pub fn link_da_mencao_com(inteiro: &str, linhas: Option<(u32, u32)>, trecho: Option<&str>) -> String {
    let base = link_da_mencao(inteiro, linhas);
    match trecho.filter(|t| !t.is_empty()) {
        Some(t) => format!("{base}?t={}", super::links_arquivo::codificado(t)),
        None => base,
    }
}

/// O trecho de um endereço de pílula (`?t=`), se ele tem.
pub fn trecho_do_link(link: &str) -> Option<String> {
    let (_, codificado) = link.split_once("?t=")?;
    let bytes = codificado.as_bytes();
    let mut saida = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let par = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            saida.push(u8::from_str_radix(par, 16).ok()?);
            i += 3;
        } else {
            saida.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(saida).ok().filter(|t| !t.is_empty())
}

/// O caminho inteiro e as linhas de um endereço `vitral:mencao/…` (ou `vitral:arquivo/…`).
pub fn ler_link_da_mencao(link: &str) -> Option<(String, Option<(u32, u32)>)> {
    let link = link.split("?t=").next().unwrap_or(link);
    let (inteiro, linha) = super::links_arquivo::ler_link(link)?;
    Some((inteiro, super::links_arquivo::ler_faixa(link).or(linha.map(|l| (l, l)))))
}

/// Escapa o que o markdown leria como marca dentro do texto de um link.
fn sem_marcas(texto: &str) -> String {
    let mut saida = String::with_capacity(texto.len() + 4);
    for c in texto.chars() {
        if matches!(c, '\\' | '*' | '_' | '[' | ']' | '`' | '<' | '>') {
            saida.push('\\');
        }
        saida.push(c);
    }
    saida
}

/// Troca, no texto de uma mensagem, as menções de arquivo (ou pasta) que existem por links
/// `vitral:mencao/…`, que o balão desenha como pílula. `resolver` recebe o caminho como foi escrito
/// e devolve o caminho inteiro, se existe.
pub fn ligar_mencoes(texto: &str, resolver: &mut dyn FnMut(&str) -> Option<String>) -> String {
    if !texto.contains('@') {
        return texto.to_string();
    }
    let mut saida = String::with_capacity(texto.len() + 64);
    let mut ultimo = 0;
    for (faixa, mencao) in mencoes_no_texto(texto) {
        let Some(inteiro) = resolver(&mencao.caminho) else { continue };
        saida.push_str(&texto[ultimo..faixa.start]);
        let link = link_da_mencao_com(&inteiro, mencao.linhas, mencao.trecho.as_deref());
        saida.push_str(&format!("[{}]({link})", sem_marcas(&texto[faixa.clone()])));
        ultimo = faixa.end;
    }
    saida.push_str(&texto[ultimo..]);
    saida
}

/// O que a pílula do balão guarda: o endereço, o caminho inteiro e as linhas.
struct DadosDaMencao {
    link: String,
    inteiro: String,
    linhas: Option<(u32, u32)>,
    trecho: Option<String>,
}

/// O plugin do texto do balão: o link `vitral:mencao/…` vira a pílula.
pub struct PilulaDeMencao;

impl MarkdownPlugin for PilulaDeMencao {
    fn name(&self) -> &str {
        "mencao-vitral"
    }

    fn parse(&self, node: &markdown_ast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let markdown_ast::Node::Link(link) = node else { return None };
        if !link.url.starts_with("vitral:mencao/") {
            return None;
        }
        let (inteiro, linhas) = ler_link_da_mencao(&link.url)?;
        let trecho = trecho_do_link(&link.url);
        let texto = texto_da_mencao_com(&inteiro, linhas, trecho.as_deref());
        Some(MarkdownNode::new("mencao-vitral", DadosDaMencao { link: link.url.clone(), inteiro, linhas, trecho }).text(texto))
    }

    fn render_inline(&self, node: &MarkdownNode, contexto: &InlineRenderContext, _: &mut Window, _: &mut App) -> Option<InlineElement> {
        let dados = node.data::<DadosDaMencao>()?;
        let id = node.source_range().map_or(0, |f| f.start);
        let pilula = pilula_de_mencao(
            SharedString::from(format!("mencao-{id}-{}", dados.link)),
            &dados.inteiro,
            dados.linhas,
            dados.trecho.as_deref(),
            true,
            false,
            contexto.line_height(),
        );
        let link = dados.link.clone();
        Some(InlineElement::new(
            pilula
                .cursor_pointer()
                .hover(|s| s.bg(Cores::forte().opacity(0.2)))
                .on_click(move |_, window, cx| {
                    window.dispatch_action(Box::new(super::links_arquivo::AbrirLinkVitral(link.clone())), cx)
                }),
        ))
    }
}

/// A pílula de uma menção no jeito do protótipo Linear (25/11/2026): 20 de altura, no azul baixo
/// da caixa (no seu balão, o branco a 13%), o ícone de 12, o nome em mono, as linhas em tênue e,
/// com o trecho, "· " e o começo dele; o caminho inteiro (e o trecho) fica na dica. `escolhida`:
/// na caixa, com a seleção por cima.
pub fn pilula_de_mencao(
    id: SharedString,
    inteiro: &str,
    linhas: Option<(u32, u32)>,
    trecho: Option<&str>,
    no_balao: bool,
    escolhida: bool,
    altura_da_linha: Pixels,
) -> Stateful<Div> {
    let pasta = inteiro.ends_with('/') || std::path::Path::new(inteiro).is_dir();
    let icone = if pasta { Lucide::Folder } else { super::painel_arquivo::icone_do_arquivo(inteiro) };
    let rotulo = rotulo_da_mencao(inteiro, None);
    let faixa = linhas.map(|(a, b)| if b > a { format!("{a}-{b}") } else { a.to_string() });
    let mut dica = match linhas {
        Some((a, b)) if b > a => format!("{inteiro}\nlinhas {a} a {b}"),
        Some((a, _)) => format!("{inteiro}\nlinha {a}"),
        None => inteiro.to_string(),
    };
    // Com o trecho, a dica mostra as primeiras 8 linhas dele.
    if let Some(t) = trecho {
        let linhas: Vec<&str> = t.lines().collect();
        dica.push_str("\n\n");
        dica.push_str(&linhas.iter().take(8).copied().collect::<Vec<_>>().join("\n"));
        if linhas.len() > 8 {
            dica.push_str("\n…");
        }
    }
    let dica = SharedString::from(dica);
    let curto = trecho.map(|t| trecho_curto(t, 24));
    let (fundo, cor, cor_linhas, cor_trecho) = if no_balao {
        (Cores::forte().opacity(0.13), Cores::forte(), Cores::forte().opacity(0.7), Cores::forte().opacity(0.85))
    } else {
        (Cores::caixa_preta().blend(Cores::destaque().opacity(0.20)), Cores::destaque_texto(), Cores::tenue(), Cores::texto_da_fala())
    };
    let altura = px(20.).min(altura_da_linha - px(1.));
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(esp::E4)
        .h(altura)
        .pl(linear::CAIXA_BAIXO)
        .pr(linear::PASSO_VAO)
        .rounded(raio::de(18.))
        .bg(if escolhida { Cores::selecao() } else { fundo })
        .font_family(FONTE_MONO)
        .text_size(r(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(cor)
        .child(div().flex_none().child(super::mensagens::icone_de(icone, 12.)))
        .child(div().flex_none().whitespace_nowrap().child(rotulo))
        .when_some(faixa, |s, faixa| {
            s.child(div().flex_none().whitespace_nowrap().font_family(crate::tema::FONTE_UI).text_size(r(11.5)).tabular().text_color(cor_linhas).child(faixa))
        })
        .when_some(curto, |s, curto| {
            s.child(div().flex_none().text_size(r(11.5)).text_color(Cores::apagado()).child("·"))
                .child(div().min_w_0().max_w(r(190.)).truncate().text_size(r(11.5)).text_color(cor_trecho).child(curto))
        })
        .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
}

#[cfg(test)]
mod testes {
    // Sem o `super::*`: ele traria o `test` do gpui junto com o `gpui_kit::*`.
    use super::{
        Mencao, ler_link_da_mencao, ler_mencao, ligar_mencoes, link_da_mencao, link_da_mencao_com, mencoes_no_texto, rotulo_da_mencao,
        texto_da_mencao, texto_da_mencao_com, trecho_curto, trecho_do_link,
    };

    fn m(caminho: &str, linhas: Option<(u32, u32)>) -> Mencao {
        Mencao { caminho: caminho.into(), linhas, trecho: None }
    }

    /// Um pedaço de linha (ou de várias) vai com o texto exato entre « », logo depois das linhas:
    /// as linhas sozinhas não dizem qual parte delas (protótipo Linear, 25/11/2026).
    #[test]
    fn o_trecho_vai_entre_aspas_angulares_e_volta() {
        let texto = texto_da_mencao_com("src/x.rs", Some((12, 12)), Some("cabeNaFabrica(c)"));
        assert_eq!(texto, "@src/x.rs#L12 «cabeNaFabrica(c)»");
        let lido = Mencao { caminho: "src/x.rs".into(), linhas: Some((12, 12)), trecho: Some("cabeNaFabrica(c)".into()) };
        assert_eq!(ler_mencao(&format!("{texto} e o resto")), Some((lido, texto.len())));
        // Em várias linhas o trecho leva as quebras; com espaço no caminho, depois das aspas.
        let texto = texto_da_mencao_com("a b/y.rs", Some((3, 4)), Some("fim()\n  começo"));
        assert_eq!(texto, "@\"a b/y.rs\"#L3-4 «fim()\\n  começo»");
        assert_eq!(ler_mencao(&texto).map(|(m, n)| (m.trecho, n)), Some((Some("fim()\n  começo".into()), texto.len())));
        // Sem trecho, nada muda; com « » mas sem fechar, a menção fica sem ele.
        assert_eq!(texto_da_mencao_com("src/x.rs", Some((1, 2)), None), texto_da_mencao("src/x.rs", Some((1, 2))));
        assert_eq!(ler_mencao("@src/x.rs#L2 «sem fim").map(|(m, n)| (m.trecho, n)), Some((None, "@src/x.rs#L2".len())));
        // O trecho vai no endereço da pílula, e o endereço continua abrindo o arquivo nas linhas.
        let link = link_da_mencao_com("/casa/src/x.rs", Some((12, 12)), Some("a «b» c"));
        assert_eq!(trecho_do_link(&link).as_deref(), Some("a «b» c"));
        assert_eq!(ler_link_da_mencao(&link), Some(("/casa/src/x.rs".to_string(), Some((12, 12)))));
        assert_eq!(trecho_do_link(&link_da_mencao("/casa/src/x.rs", None)), None);
        // Na pílula, o começo do trecho numa linha só.
        assert_eq!(trecho_curto("  return c.material\n    .includes(x)", 14), "return c.mater…");
    }

    #[test]
    fn a_mencao_vai_como_texto_e_volta() {
        assert_eq!(texto_da_mencao("src/ui/fila.rs", Some((120, 134))), "@src/ui/fila.rs#L120-134");
        assert_eq!(texto_da_mencao("src/ui/fila.rs", Some((7, 7))), "@src/ui/fila.rs#L7");
        assert_eq!(texto_da_mencao("/casa/a.md", None), "@/casa/a.md");
        for (texto, mencao) in [
            ("@src/ui/fila.rs#L120-134", m("src/ui/fila.rs", Some((120, 134)))),
            ("@src/ui/fila.rs#L7", m("src/ui/fila.rs", Some((7, 7)))),
            ("@src/x.rs:12-20", m("src/x.rs", Some((12, 20)))),
            ("@src/x.rs#L20-L12", m("src/x.rs", Some((12, 20)))),
            ("@central/src/", m("central/src/", None)),
            ("@/casa/a.md", m("/casa/a.md", None)),
        ] {
            assert_eq!(ler_mencao(texto), Some((mencao, texto.len())), "{texto}");
        }
        // A pontuação da frase fica de fora; o "@" solto e o "@12" de chat não são caminho.
        assert_eq!(ler_mencao("@a.rs, e"), Some((m("a.rs", None), 5)));
        assert_eq!(ler_mencao("@a.rs:"), Some((m("a.rs", None), 5)));
        assert_eq!(ler_mencao("@ x"), None);
        assert_eq!(ler_mencao("@12"), None);
    }

    /// Caminho com espaço vai entre aspas, e volta inteiro (com as linhas depois das aspas).
    #[test]
    fn caminho_com_espaco_vai_entre_aspas() {
        assert_eq!(texto_da_mencao("minha pasta/a b.rs", Some((3, 5))), "@\"minha pasta/a b.rs\"#L3-5");
        assert_eq!(texto_da_mencao("minha pasta/", None), "@\"minha pasta/\"");
        assert_eq!(texto_da_mencao("a.rs", None), "@a.rs");
        let texto = "@\"minha pasta/a b.rs\"#L3-5, e";
        let tamanho = texto.find(',').unwrap();
        assert_eq!(ler_mencao(texto), Some((m("minha pasta/a b.rs", Some((3, 5))), tamanho)));
        assert_eq!(ler_mencao("@\"a b.rs\" e"), Some((m("a b.rs", None), 9)));
        // Aspas que não fecham, ou vazias, não são menção.
        assert_eq!(ler_mencao("@\"sem fim"), None);
        assert_eq!(ler_mencao("@\"\""), None);
        let frase = "veja @\"a b/c.rs\" e @d.rs";
        let achadas: Vec<String> = mencoes_no_texto(frase).into_iter().map(|(f, _)| frase[f].to_string()).collect();
        assert_eq!(achadas, ["@\"a b/c.rs\"", "@d.rs"]);
    }

    #[test]
    fn o_rotulo_e_o_nome_curto_com_as_linhas() {
        assert_eq!(rotulo_da_mencao("src/ui/fila.rs", Some((120, 134))), "fila.rs:120-134");
        assert_eq!(rotulo_da_mencao("src/ui/fila.rs", Some((9, 9))), "fila.rs:9");
        assert_eq!(rotulo_da_mencao("LEIA-ME.md", None), "LEIA-ME.md");
        assert_eq!(rotulo_da_mencao("central/src/", None), "src/");
    }

    #[test]
    fn acha_as_mencoes_fora_do_codigo() {
        let texto = "olha @src/a.rs#L3-5 e (@b.md), não `@c.rs`\n```\n@d.rs\n```\nvictor@eita.com @e.rs";
        let achadas: Vec<(String, Mencao)> = mencoes_no_texto(texto).into_iter().map(|(f, m)| (texto[f].to_string(), m)).collect();
        assert_eq!(
            achadas,
            vec![
                ("@src/a.rs#L3-5".into(), m("src/a.rs", Some((3, 5)))),
                ("@b.md".into(), m("b.md", None)),
                ("@e.rs".into(), m("e.rs", None)),
            ]
        );
    }

    #[test]
    fn so_o_que_existe_vira_pilula_no_balao() {
        let mut resolver = |c: &str| ["src/a_b.rs", "pasta/"].contains(&c).then(|| format!("/p/{c}"));
        assert_eq!(
            ligar_mencoes("veja @src/a_b.rs#L2-4 e @nao.rs e @pasta/", &mut resolver),
            "veja [@src/a\\_b.rs#L2-4](vitral:mencao/2-4/%2Fp%2Fsrc%2Fa_b.rs) e @nao.rs e [@pasta/](vitral:mencao/0/%2Fp%2Fpasta%2F)"
        );
        assert_eq!(ligar_mencoes("sem menção", &mut resolver), "sem menção");
        // O link volta ao caminho e às linhas.
        let link = link_da_mencao("/p/src/a.rs", Some((2, 4)));
        assert_eq!(super::super::links_arquivo::ler_link(&link), Some(("/p/src/a.rs".into(), Some(2))));
        assert_eq!(super::super::links_arquivo::ler_faixa(&link), Some((2, 4)));
    }
}
