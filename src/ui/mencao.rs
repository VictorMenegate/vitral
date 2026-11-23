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

use crate::tema::{Cores, FONTE_MONO, Lucide, Tipo as _, esp, raio};

/// Uma menção: o caminho como vai no texto (relativo à pasta do chat, ou inteiro) e as linhas
/// (a primeira e a última; iguais numa linha só).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mencao {
    pub caminho: String,
    pub linhas: Option<(u32, u32)>,
}

/// O texto da menção que vai na mensagem: "@src/x.rs", "@src/x.rs#L12", "@src/x.rs#L12-20".
pub fn texto_da_mencao(caminho: &str, linhas: Option<(u32, u32)>) -> String {
    match linhas {
        Some((a, b)) if b > a => format!("@{caminho}#L{a}-{b}"),
        Some((a, _)) => format!("@{caminho}#L{a}"),
        None => format!("@{caminho}"),
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
    Some((Mencao { caminho: caminho.to_string(), linhas }, 1 + bruto.len()))
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

/// O caminho inteiro e as linhas de um endereço `vitral:mencao/…` (ou `vitral:arquivo/…`).
pub fn ler_link_da_mencao(link: &str) -> Option<(String, Option<(u32, u32)>)> {
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
        let link = link_da_mencao(&inteiro, mencao.linhas);
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
        let texto = texto_da_mencao(&inteiro, linhas);
        Some(MarkdownNode::new("mencao-vitral", DadosDaMencao { link: link.url.clone(), inteiro, linhas }).text(texto))
    }

    fn render_inline(&self, node: &MarkdownNode, contexto: &InlineRenderContext, _: &mut Window, _: &mut App) -> Option<InlineElement> {
        let dados = node.data::<DadosDaMencao>()?;
        let id = node.source_range().map_or(0, |f| f.start);
        let pilula = pilula_de_mencao(
            SharedString::from(format!("mencao-{id}-{}", dados.link)),
            &dados.inteiro,
            dados.linhas,
            false,
            contexto.line_height(),
        );
        let link = dados.link.clone();
        Some(InlineElement::new(
            pilula
                .cursor_pointer()
                .hover(|s| s.border_color(Cores::borda_forte()))
                .on_click(move |_, window, cx| {
                    window.dispatch_action(Box::new(super::links_arquivo::AbrirLinkVitral(link.clone())), cx)
                }),
        ))
    }
}

/// A pílula de uma menção, do tamanho da linha do texto em volta: o ícone do tipo, o nome curto em
/// mono e as linhas em tom tênue, no poço escuro (o mesmo do código do balão); o caminho inteiro
/// fica na dica. `escolhida`: na caixa, com a seleção por cima.
pub fn pilula_de_mencao(
    id: SharedString,
    inteiro: &str,
    linhas: Option<(u32, u32)>,
    escolhida: bool,
    altura_da_linha: Pixels,
) -> Stateful<Div> {
    let pasta = inteiro.ends_with('/') || std::path::Path::new(inteiro).is_dir();
    let icone = if pasta { Lucide::Folder } else { super::painel_arquivo::icone_do_arquivo(inteiro) };
    let rotulo = rotulo_da_mencao(inteiro, None);
    let faixa = linhas.map(|(a, b)| if b > a { format!(":{a}-{b}") } else { format!(":{a}") });
    let dica = SharedString::from(match linhas {
        Some((a, b)) if b > a => format!("{inteiro}\nlinhas {a} a {b}"),
        Some((a, _)) => format!("{inteiro}\nlinha {a}"),
        None => inteiro.to_string(),
    });
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(esp::E4)
        .h(altura_da_linha - px(2.))
        .px(esp::E6)
        .rounded(raio::CONTROLE)
        .border_1()
        .map(|s| if escolhida { s.bg(Cores::selecao()).border_color(Cores::destaque_borda()) } else { s.bg(Cores::recuo()).border_color(Cores::borda()) })
        .font_family(FONTE_MONO)
        .metadado()
        .child(div().flex_none().text_color(Cores::tenue()).child(super::mensagens::icone_de(icone, 12.)))
        .child(div().flex_none().whitespace_nowrap().text_color(Cores::texto()).child(rotulo))
        .when_some(faixa, |s, faixa| s.child(div().flex_none().whitespace_nowrap().text_color(Cores::tenue()).child(faixa)))
        .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
}

#[cfg(test)]
mod testes {
    // Sem o `super::*`: ele traria o `test` do gpui junto com o `gpui_kit::*`.
    use super::{Mencao, ler_mencao, ligar_mencoes, link_da_mencao, mencoes_no_texto, rotulo_da_mencao, texto_da_mencao};

    fn m(caminho: &str, linhas: Option<(u32, u32)>) -> Mencao {
        Mencao { caminho: caminho.into(), linhas }
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
