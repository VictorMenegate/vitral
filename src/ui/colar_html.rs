//! Colar HTML na caixa vira Markdown (dono, 24/11/2026; ideia do Paseo): o que se copia de um
//! navegador ou de um documento traz links, listas, negrito e código, e a caixa recebia só o texto
//! puro, sem nada disso. No Linux o GPUI só lia o `text/plain`; o `vendor/gpui-pre-linux` agora lê
//! também o `text/html` do Wayland e o guarda no `metadata` do item da área de transferência
//! (`ClipboardItem::metadata`). Aqui ficam as regras puras: HTML entra, Markdown sai.
//!
//! Só se converte quando o HTML tem forma que o texto puro perdeu (link, lista, negrito, título,
//! código…); copiar de um editor, que manda `<span>` e `<div>` com estilo, deixa a colagem como
//! sempre foi.

/// O Markdown do HTML copiado, se ele diz algo que o texto puro (`plano`) não diz.
pub fn markdown_da_colagem(plano: &str, html: &str) -> Option<String> {
    let markdown = html_para_markdown(html);
    let iguais = |a: &str, b: &str| a.split_whitespace().eq(b.split_whitespace());
    (!markdown.trim().is_empty() && !iguais(&markdown, plano)).then_some(markdown)
}

#[derive(Debug)]
enum No {
    Texto(String),
    Elemento { tag: String, attrs: Vec<(String, String)>, filhos: Vec<No> },
}

const VAZIOS: [&str; 8] = ["br", "hr", "img", "meta", "link", "input", "wbr", "col"];
const IGNORADOS: [&str; 7] = ["script", "style", "head", "title", "svg", "template", "noscript"];

fn entidades(texto: &str) -> String {
    let mut saida = String::with_capacity(texto.len());
    let mut resto = texto;
    while let Some(i) = resto.find('&') {
        saida.push_str(&resto[..i]);
        resto = &resto[i..];
        let Some(fim) = resto[..resto.len().min(12)].find(';') else {
            saida.push('&');
            resto = &resto[1..];
            continue;
        };
        let nome = &resto[1..fim];
        let c = match nome {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "hellip" => Some('…'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "laquo" => Some('«'),
            "raquo" => Some('»'),
            _ => nome
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        match c {
            Some(c) => {
                saida.push(c);
                resto = &resto[fim + 1..];
            }
            None => {
                saida.push('&');
                resto = &resto[1..];
            }
        }
    }
    saida.push_str(resto);
    saida
}

/// Lê `nome="valor" outro='x' solto` de dentro de uma tag.
fn atributos(texto: &str) -> Vec<(String, String)> {
    let mut lista = Vec::new();
    let mut resto = texto.trim_start();
    while !resto.is_empty() {
        let fim_nome = resto.find(|c: char| c == '=' || c.is_whitespace() || c == '/').unwrap_or(resto.len());
        let nome = resto[..fim_nome].to_lowercase();
        resto = resto[fim_nome..].trim_start();
        let mut valor = String::new();
        if let Some(depois) = resto.strip_prefix('=') {
            let depois = depois.trim_start();
            if let Some(aspa) = depois.chars().next().filter(|c| matches!(c, '"' | '\'')) {
                let corpo = &depois[1..];
                let fim = corpo.find(aspa).unwrap_or(corpo.len());
                valor = entidades(&corpo[..fim]);
                resto = corpo.get(fim + 1..).unwrap_or("");
            } else {
                let fim = depois.find(char::is_whitespace).unwrap_or(depois.len());
                valor = entidades(&depois[..fim]);
                resto = &depois[fim..];
            }
        } else if nome.is_empty() {
            resto = resto.get(1..).unwrap_or("");
        }
        if !nome.is_empty() {
            lista.push((nome, valor));
        }
        resto = resto.trim_start();
    }
    lista
}

fn fecha_implicito(aberto: &str, novo: &str) -> bool {
    matches!((aberto, novo), ("li", "li") | ("p", "p") | ("p", "ul" | "ol" | "pre" | "div" | "blockquote" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6") | ("tr", "tr") | ("td" | "th", "td" | "th" | "tr"))
}

fn arvore(html: &str) -> Vec<No> {
    // Pilha de elementos abertos: (tag, attrs, filhos até agora).
    let mut pilha: Vec<(String, Vec<(String, String)>, Vec<No>)> = vec![(String::new(), Vec::new(), Vec::new())];
    fn fechar(pilha: &mut Vec<(String, Vec<(String, String)>, Vec<No>)>) {
        if pilha.len() > 1 {
            let (tag, attrs, filhos) = pilha.pop().expect("pilha com mais de um");
            pilha.last_mut().expect("raiz").2.push(No::Elemento { tag, attrs, filhos });
        }
    }
    let mut resto = html;
    while !resto.is_empty() {
        let Some(i) = resto.find('<') else {
            pilha.last_mut().expect("raiz").2.push(No::Texto(entidades(resto)));
            break;
        };
        if i > 0 {
            pilha.last_mut().expect("raiz").2.push(No::Texto(entidades(&resto[..i])));
        }
        resto = &resto[i..];
        if let Some(depois) = resto.strip_prefix("<!--") {
            resto = depois.find("-->").map_or("", |f| &depois[f + 3..]);
            continue;
        }
        let Some(fim) = resto.find('>') else {
            pilha.last_mut().expect("raiz").2.push(No::Texto(entidades(resto)));
            break;
        };
        let dentro = &resto[1..fim];
        resto = &resto[fim + 1..];
        if dentro.starts_with('!') || dentro.starts_with('?') {
            continue;
        }
        if let Some(nome) = dentro.strip_prefix('/') {
            let nome = nome.trim().to_lowercase();
            if let Some(pos) = pilha.iter().rposition(|(t, ..)| *t == nome) {
                while pilha.len() > pos {
                    fechar(&mut pilha);
                }
            }
            continue;
        }
        let corpo = dentro.trim_end_matches('/');
        let fim_nome = corpo.find(|c: char| c.is_whitespace()).unwrap_or(corpo.len());
        let tag = corpo[..fim_nome].to_lowercase();
        let attrs = atributos(&corpo[fim_nome..]);
        if IGNORADOS.contains(&tag.as_str()) {
            let fecho = format!("</{tag}");
            let achou = resto.to_lowercase().find(&fecho);
            resto = achou.and_then(|p| resto[p..].find('>').map(|f| &resto[p + f + 1..])).unwrap_or("");
            continue;
        }
        if pilha.last().is_some_and(|(aberto, ..)| fecha_implicito(aberto, &tag)) {
            fechar(&mut pilha);
        }
        if VAZIOS.contains(&tag.as_str()) {
            pilha.last_mut().expect("raiz").2.push(No::Elemento { tag, attrs, filhos: Vec::new() });
        } else {
            pilha.push((tag, attrs, Vec::new()));
        }
    }
    while pilha.len() > 1 {
        fechar(&mut pilha);
    }
    pilha.pop().map(|(_, _, filhos)| filhos).unwrap_or_default()
}

fn attr<'a>(attrs: &'a [(String, String)], nome: &str) -> Option<&'a str> {
    attrs.iter().find(|(n, _)| n == nome).map(|(_, v)| v.as_str())
}

fn texto_cru(nos: &[No]) -> String {
    nos.iter()
        .map(|n| match n {
            No::Texto(t) => t.clone(),
            No::Elemento { tag, filhos, .. } if tag == "br" => {
                let _ = filhos;
                "\n".to_string()
            }
            No::Elemento { filhos, .. } => texto_cru(filhos),
        })
        .collect()
}

#[derive(Default)]
struct Saida {
    texto: String,
    /// Um espaço no começo conta (o miolo de um negrito ou de um link, que o espaço de fora
    /// separa das palavras vizinhas).
    espaco_no_inicio: bool,
}

impl Saida {
    fn linha_nova(&mut self) {
        if !self.texto.is_empty() && !self.texto.ends_with('\n') {
            self.texto.push('\n');
        }
    }
    fn linha_em_branco(&mut self) {
        self.linha_nova();
        if !self.texto.is_empty() && !self.texto.ends_with("\n\n") {
            self.texto.push('\n');
        }
    }
    fn espaco(&mut self, t: &str) {
        let mut anterior_espaco = (self.texto.is_empty() && !self.espaco_no_inicio) || self.texto.ends_with([' ', '\n']);
        for c in t.chars() {
            if c.is_whitespace() {
                if !anterior_espaco {
                    self.texto.push(' ');
                    anterior_espaco = true;
                }
            } else {
                self.texto.push(c);
                anterior_espaco = false;
            }
        }
    }
}

/// Renderiza os filhos de um elemento em linha numa string à parte.
fn em_linha(nos: &[No], lista: &[(bool, usize)]) -> String {
    let mut s = Saida { espaco_no_inicio: true, ..Saida::default() };
    renderizar(nos, &mut s, lista);
    s.texto
}

/// Envolve `miolo` com `marca`, deixando o espaço das pontas fora dela ("**a **" não vale).
fn marcar(miolo: &str, marca: &str) -> String {
    let aparado = miolo.trim();
    if aparado.is_empty() {
        return miolo.to_string();
    }
    let antes = &miolo[..miolo.len() - miolo.trim_start().len()];
    let depois = &miolo[miolo.trim_end().len()..];
    format!("{antes}{marca}{aparado}{marca}{depois}")
}

/// `lista`: as listas em que se está, de fora para dentro: (ordenada, quantos itens já saíram).
fn renderizar(nos: &[No], s: &mut Saida, lista: &[(bool, usize)]) {
    let mut contadores: Vec<usize> = Vec::new();
    let _ = &mut contadores;
    for no in nos {
        match no {
            No::Texto(t) => s.espaco(t),
            No::Elemento { tag, attrs, filhos } => match tag.as_str() {
                "br" => {
                    while s.texto.ends_with(' ') {
                        s.texto.pop();
                    }
                    s.texto.push('\n');
                }
                "hr" => {
                    s.linha_em_branco();
                    s.texto.push_str("---");
                    s.linha_em_branco();
                }
                "img" => {
                    let alt = attr(attrs, "alt").unwrap_or("");
                    if let Some(src) = attr(attrs, "src").filter(|u| !u.starts_with("data:")) {
                        s.texto.push_str(&format!("![{alt}]({src})"));
                    } else if !alt.is_empty() {
                        s.espaco(alt);
                    }
                }
                "a" => {
                    let miolo = em_linha(filhos, lista);
                    let href = attr(attrs, "href").filter(|h| !h.is_empty() && !h.starts_with("javascript:") && !h.starts_with('#'));
                    match href {
                        Some(h) if miolo.trim().is_empty() => s.texto.push_str(h),
                        Some(h) if miolo.trim() == h => s.texto.push_str(h),
                        Some(h) => {
                            let aparado = miolo.trim();
                            let antes = &miolo[..miolo.len() - miolo.trim_start().len()];
                            let depois = &miolo[miolo.trim_end().len()..];
                            s.texto.push_str(&format!("{antes}[{aparado}]({h}){depois}"));
                        }
                        None => s.texto.push_str(&miolo),
                    }
                }
                "strong" | "b" => s.texto.push_str(&marcar(&em_linha(filhos, lista), "**")),
                "em" | "i" => s.texto.push_str(&marcar(&em_linha(filhos, lista), "*")),
                "del" | "s" | "strike" => s.texto.push_str(&marcar(&em_linha(filhos, lista), "~~")),
                "code" | "kbd" | "samp" | "tt" => {
                    let cru = texto_cru(filhos);
                    let cru = cru.trim();
                    if !cru.is_empty() {
                        let cerca = if cru.contains('`') { "``" } else { "`" };
                        s.texto.push_str(&format!("{cerca}{cru}{cerca}"));
                    }
                }
                "pre" => {
                    s.linha_em_branco();
                    let linguagem = filhos
                        .iter()
                        .find_map(|f| match f {
                            No::Elemento { tag, attrs, .. } if tag == "code" => attr(attrs, "class"),
                            _ => None,
                        })
                        .and_then(|c| c.split_whitespace().find_map(|p| p.strip_prefix("language-")))
                        .unwrap_or("");
                    let cru = texto_cru(filhos);
                    s.texto.push_str(&format!("```{linguagem}\n{}\n```", cru.trim_matches('\n')));
                    s.linha_em_branco();
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    let nivel = tag[1..].parse::<usize>().unwrap_or(1);
                    s.linha_em_branco();
                    let miolo = em_linha(filhos, lista);
                    s.texto.push_str(&format!("{} {}", "#".repeat(nivel), miolo.trim()));
                    s.linha_em_branco();
                }
                "blockquote" => {
                    s.linha_em_branco();
                    let miolo = em_linha(filhos, lista);
                    let citado: Vec<String> = miolo.trim().lines().map(|l| if l.is_empty() { ">".into() } else { format!("> {l}") }).collect();
                    s.texto.push_str(&citado.join("\n"));
                    s.linha_em_branco();
                }
                "ul" | "ol" => {
                    if lista.is_empty() {
                        s.linha_em_branco();
                    } else {
                        s.linha_nova();
                    }
                    let mut dentro = lista.to_vec();
                    dentro.push((tag == "ol", 0));
                    let n = dentro.len() - 1;
                    for filho in filhos {
                        if let No::Elemento { tag: t, filhos: itens, .. } = filho
                            && t == "li"
                        {
                            dentro[n].1 += 1;
                            let recuo = "  ".repeat(n);
                            let marca = if dentro[n].0 { format!("{}. ", dentro[n].1) } else { "- ".to_string() };
                            let corpo = em_linha(itens, &dentro);
                            let corpo = corpo.trim_end();
                            s.linha_nova();
                            s.texto.push_str(&format!("{recuo}{marca}{}", corpo.trim_start().replace('\n', "\n")));
                            s.linha_nova();
                        } else {
                            renderizar(std::slice::from_ref(filho), s, &dentro);
                        }
                    }
                    if lista.is_empty() {
                        s.linha_em_branco();
                    }
                }
                "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "figure" | "figcaption" | "table" | "tbody" | "thead" => {
                    s.linha_em_branco();
                    renderizar(filhos, s, lista);
                    s.linha_em_branco();
                }
                "tr" => {
                    s.linha_nova();
                    let celulas: Vec<String> = filhos
                        .iter()
                        .filter_map(|f| match f {
                            No::Elemento { tag, filhos, .. } if tag == "td" || tag == "th" => Some(em_linha(filhos, lista).trim().to_string()),
                            _ => None,
                        })
                        .collect();
                    s.texto.push_str(&celulas.join(" | "));
                    s.linha_nova();
                }
                _ => renderizar(filhos, s, lista),
            },
        }
    }
}

/// O HTML como Markdown: links, listas, negrito, itálico, código, títulos, citações.
pub fn html_para_markdown(html: &str) -> String {
    let mut s = Saida::default();
    renderizar(&arvore(html), &mut s, &[]);
    // Linhas com espaço sobrando e mais de uma linha em branco seguida.
    let mut saida = String::new();
    let mut brancas = 0;
    for linha in s.texto.trim().lines() {
        let linha = linha.trim_end();
        if linha.is_empty() {
            brancas += 1;
            if brancas > 1 {
                continue;
            }
        } else {
            brancas = 0;
        }
        saida.push_str(linha);
        saida.push('\n');
    }
    saida.trim_end().to_string()
}

#[cfg(test)]
mod testes {
    use super::{html_para_markdown as md, markdown_da_colagem};

    #[test]
    fn link_negrito_italico_e_codigo() {
        assert_eq!(
            md(r#"<p>Veja <a href="https://eita.com.br/x?a=1&amp;b=2">o <b>site</b></a>, é <em>bom</em> e use <code>cargo test</code>.</p>"#),
            "Veja [o **site**](https://eita.com.br/x?a=1&b=2), é *bom* e use `cargo test`."
        );
        // O espaço de dentro do negrito sai para fora das marcas.
        assert_eq!(md("<p>a<strong> b </strong>c</p>"), "a **b** c");
        // Link cujo texto é o próprio endereço, e link vazio.
        assert_eq!(md(r#"<a href="https://a.com">https://a.com</a>"#), "https://a.com");
        assert_eq!(md(r##"<a href="#topo">ir</a> <a href="javascript:x()">y</a>"##), "ir y");
    }

    #[test]
    fn listas_com_marcador_numero_e_recuo() {
        assert_eq!(md("<ul><li>um</li><li>dois<ul><li>dois.a</li></ul></li><li>três</li></ul>"), "- um\n- dois\n  - dois.a\n- três");
        assert_eq!(md("<ol><li>primeiro</li><li>segundo</li></ol>"), "1. primeiro\n2. segundo");
        assert_eq!(md("<p>Antes</p><ul><li>a</li></ul><p>Depois</p>"), "Antes\n\n- a\n\nDepois");
    }

    #[test]
    fn bloco_de_codigo_titulo_e_citacao() {
        assert_eq!(
            md("<pre><code class=\"language-rust\">fn main() {\n    println!(\"oi\");\n}\n</code></pre>"),
            "```rust\nfn main() {\n    println!(\"oi\");\n}\n```"
        );
        assert_eq!(md("<h2>Título <i>x</i></h2><p>texto</p>"), "## Título *x*\n\ntexto");
        assert_eq!(md("<blockquote><p>dito</p></blockquote>"), "> dito");
    }

    #[test]
    fn entidades_script_comentario_e_quebras() {
        assert_eq!(md("a&nbsp;b &lt;c&gt; &#233; &#x41; &copy"), "a b <c> é A &copy");
        assert_eq!(md("<!--StartFragment--><p>x</p><script>alert(1)</script><style>p{}</style><!--EndFragment-->"), "x");
        assert_eq!(md("linha<br>outra"), "linha\noutra");
        assert_eq!(md("<table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></table>"), "a | b\n1 | 2");
        assert_eq!(md(r#"<img src="https://x.com/a.png" alt="foto">"#), "![foto](https://x.com/a.png)");
    }

    #[test]
    fn html_malformado_nao_quebra() {
        assert_eq!(md("<p>aberto <b>sem fechar"), "aberto **sem fechar**");
        assert_eq!(md("1 < 2 e <"), "1 < 2 e <");
        assert_eq!(md(""), "");
    }

    /// Só vira Markdown quando o HTML diz mais que o texto puro: o `<span>` de um editor não conta.
    #[test]
    fn so_converte_quando_ha_forma_a_mais() {
        assert_eq!(markdown_da_colagem("Veja o site", r#"<p>Veja <a href="https://a.com">o site</a></p>"#).as_deref(), Some("Veja [o site](https://a.com)"));
        assert_eq!(markdown_da_colagem("let x = 1;", r#"<div style="font-family:mono"><span style="color:red">let</span> x = 1;</div>"#), None);
        assert_eq!(markdown_da_colagem("a  b\nc", "<p>a b c</p>"), None);
        assert_eq!(markdown_da_colagem("x", "<script>1</script>"), None);
    }
}
