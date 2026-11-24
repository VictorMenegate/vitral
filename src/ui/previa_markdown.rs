//! A prévia de um .md no painel de arquivo: o frontmatter YAML do alto vira uma tabelinha de
//! chave e valor (todo SKILL.md tem um) e os links relativos abrem o arquivo no painel.

use std::path::Path;

/// As linhas do frontmatter (a chave e o valor, na ordem do arquivo) e o resto do texto. Sem
/// frontmatter, ou com um que não é uma lista de chaves, nada de linhas e o texto inteiro.
pub(super) fn frontmatter(texto: &str) -> (Vec<(String, String)>, &str) {
    let nada = (Vec::new(), texto);
    let sem_marca = texto.strip_prefix('\u{feff}').unwrap_or(texto);
    let traco = |linha: &str| linha.trim_end_matches(['\n', '\r', ' ', '\t']) == "---";
    let mut linhas = sem_marca.split_inclusive('\n');
    let Some(primeira) = linhas.next().filter(|l| traco(l)) else {
        return nada;
    };
    let mut fim = primeira.len();
    let mut dentro: Vec<&str> = Vec::new();
    let mut fechou = false;
    for linha in linhas {
        fim += linha.len();
        if traco(linha) {
            fechou = true;
            break;
        }
        dentro.push(linha.trim_end_matches(['\n', '\r']));
    }
    if !fechou {
        return nada;
    }
    match chaves_e_valores(&dentro) {
        Some(pares) if !pares.is_empty() => (pares, &sem_marca[fim..]),
        _ => nada,
    }
}

/// As chaves do primeiro nível de um YAML simples, cada uma com o valor em texto. Não é um leitor
/// de YAML inteiro: serve ao frontmatter de um documento (nome, descrição, uma lista, um mapa
/// pequeno). `None` quando o texto não é uma lista de chaves.
fn chaves_e_valores(linhas: &[&str]) -> Option<Vec<(String, String)>> {
    // A chave, o valor da linha dela e as linhas recuadas que vêm depois.
    let mut pares: Vec<(String, String, Vec<&str>)> = Vec::new();
    for linha in linhas {
        let recuada = linha.starts_with([' ', '\t']);
        if linha.trim().is_empty() {
            // A linha em branco só conta dentro de um valor de várias linhas.
            if let Some((_, _, resto)) = pares.last_mut().filter(|(_, _, resto)| !resto.is_empty()) {
                resto.push("");
            }
            continue;
        }
        if recuada {
            pares.last_mut()?.2.push(linha);
            continue;
        }
        if linha.starts_with('#') {
            continue;
        }
        let (chave, valor) = match linha.split_once(": ") {
            Some((chave, valor)) => (chave, valor.trim()),
            None => (linha.strip_suffix(':')?, ""),
        };
        let chave = sem_aspas(chave.trim());
        if chave.is_empty() || chave.starts_with('-') || chave.contains(char::is_whitespace) {
            return None;
        }
        pares.push((chave, valor.to_string(), Vec::new()));
    }
    Some(pares.into_iter().map(|(chave, valor, resto)| (chave, valor_em_texto(&valor, &resto))).collect())
}

/// O valor de uma chave em texto: o da linha dela, o bloco dobrado (">") ou literal ("|"), a
/// lista (os itens separados por vírgula) ou o mapa de dentro (como veio, sem o recuo).
fn valor_em_texto(valor: &str, resto: &[&str]) -> String {
    // As linhas em branco do fim não são do valor.
    let resto = &resto[..resto.iter().rposition(|l| !l.trim().is_empty()).map_or(0, |i| i + 1)];
    let recuo = resto.iter().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
    let sem_recuo = || resto.iter().map(|l| l.get(recuo..).unwrap_or("").trim_end());
    let bloco = |marca: char| valor.strip_prefix(marca).is_some_and(|r| r.chars().all(|c| matches!(c, '-' | '+') || c.is_ascii_digit()));
    if bloco('|') {
        return sem_recuo().collect::<Vec<_>>().join("\n");
    }
    if bloco('>') {
        return dobrado(sem_recuo());
    }
    if !valor.is_empty() {
        // O valor simples que continua nas linhas de baixo.
        let juntas = std::iter::once(valor).chain(resto.iter().map(|l| l.trim())).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
        return if resto.is_empty() { item_em_texto(valor) } else { juntas };
    }
    let itens: Vec<&str> = resto.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if !itens.is_empty() && itens.iter().all(|l| *l == "-" || l.starts_with("- ")) {
        return itens.iter().map(|l| sem_aspas(l[1..].trim())).collect::<Vec<_>>().join(", ");
    }
    sem_recuo().collect::<Vec<_>>().join("\n")
}

/// O bloco dobrado: as linhas viram uma só, e a linha em branco separa parágrafos.
fn dobrado<'a>(linhas: impl Iterator<Item = &'a str>) -> String {
    let mut saida = String::new();
    for linha in linhas {
        let linha = linha.trim();
        if linha.is_empty() {
            saida.push('\n');
        } else {
            if !saida.is_empty() && !saida.ends_with('\n') {
                saida.push(' ');
            }
            saida.push_str(linha);
        }
    }
    saida
}

/// O valor de uma linha: sem as aspas, e a lista entre colchetes com os itens separados por vírgula.
fn item_em_texto(valor: &str) -> String {
    match valor.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        Some(itens) => itens.split(',').map(|i| sem_aspas(i.trim())).filter(|i| !i.is_empty()).collect::<Vec<_>>().join(", "),
        None => sem_aspas(valor),
    }
}

fn sem_aspas(texto: &str) -> String {
    for aspa in ['"', '\''] {
        if texto.len() >= 2 && texto.starts_with(aspa) && texto.ends_with(aspa) {
            let dentro = &texto[1..texto.len() - 1];
            return if aspa == '"' { dentro.replace("\\\"", "\"") } else { dentro.replace("''", "'") };
        }
    }
    texto.to_string()
}

/// O markdown com os links para arquivos trocados pelo link que abre no painel
/// (`vitral:arquivo/…`). O caminho relativo parte da pasta do próprio .md; se não existe ali, da
/// pasta do chat. O que não é arquivo daqui (a web, uma âncora, um arquivo que não existe) fica
/// como está.
pub(super) fn ligar_links(texto: &str, pasta_do_md: &Path, raiz: &Path) -> String {
    use super::links_arquivo::{ligar_arquivos, resolver};
    ligar_arquivos(texto, &mut |escrito| resolver(pasta_do_md, escrito).or_else(|| resolver(raiz, escrito)))
}

/// Para onde vai o clique num link da prévia.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Destino {
    /// Um arquivo, que abre no painel.
    Arquivo,
    /// Um endereço da web, que abre no navegador.
    Web,
    /// Não abre: `file://`, `javascript:`, um comando de barra (o arquivo não manda na caixa).
    Nenhum,
}

pub(super) fn destino_do_link(url: &str) -> Destino {
    let url = url.trim();
    let comeca = |prefixo: &str| url.len() > prefixo.len() && url.get(..prefixo.len()).is_some_and(|c| c.eq_ignore_ascii_case(prefixo));
    if comeca("http://") || comeca("https://") {
        Destino::Web
    } else if super::links_arquivo::ler_link(url).is_some() {
        Destino::Arquivo
    } else {
        Destino::Nenhum
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::ui::links_arquivo::ler_link;
    use std::path::PathBuf;

    fn linhas(texto: &str) -> Vec<(String, String)> {
        frontmatter(texto).0
    }

    fn par(chave: &str, valor: &str) -> (String, String) {
        (chave.to_string(), valor.to_string())
    }

    #[test]
    fn o_frontmatter_do_skill_vira_chave_e_valor() {
        let texto = "---\nname: easypanel\ndescription: Operar o EasyPanel da VPS pela CLI: deploy, env e volume.\nallowed-tools: [Bash, Read]\n---\n\n# EasyPanel\n\nTexto.\n";
        let (linhas, corpo) = frontmatter(texto);
        assert_eq!(
            linhas,
            [
                par("name", "easypanel"),
                par("description", "Operar o EasyPanel da VPS pela CLI: deploy, env e volume."),
                par("allowed-tools", "Bash, Read"),
            ]
        );
        assert_eq!(corpo, "\n# EasyPanel\n\nTexto.\n");
    }

    #[test]
    fn valores_de_varias_linhas_listas_e_aspas() {
        // O texto dobrado (">") junta as linhas; o literal ("|") as mantém.
        assert_eq!(
            linhas("---\ndescription: >\n  Linha um\n  linha dois\nlicense: MIT\n---\n"),
            [par("description", "Linha um linha dois"), par("license", "MIT")]
        );
        assert_eq!(linhas("---\nnota: |-\n  a\n  b\n---\n"), [par("nota", "a\nb")]);
        // A continuação de um valor simples, recuada.
        assert_eq!(linhas("---\ndescription: primeira parte\n  segunda parte\n---\n"), [par("description", "primeira parte segunda parte")]);
        // A lista em linhas.
        assert_eq!(linhas("---\ntags:\n  - loja\n  - \"frete grátis\"\n---\n"), [par("tags", "loja, frete grátis")]);
        // O mapa de dentro sai como veio, sem o recuo.
        assert_eq!(linhas("---\nmetadata:\n  version: 1\n  autor: v\n---\n"), [par("metadata", "version: 1\nautor: v")]);
        // As aspas saem; os dois pontos de dentro ficam.
        assert_eq!(linhas("---\nname: \"x: y\"\nargument-hint: '[arquivo]'\n---\n"), [par("name", "x: y"), par("argument-hint", "[arquivo]")]);
        // Comentário e linha em branco não são linhas da tabela.
        assert_eq!(linhas("---\n# nota\nname: x\n\nmodel: opus\n---\n"), [par("name", "x"), par("model", "opus")]);
        // O arquivo do Windows.
        let (linhas_crlf, corpo) = frontmatter("---\r\nname: x\r\n---\r\ncorpo\r\n");
        assert_eq!(linhas_crlf, [par("name", "x")]);
        assert_eq!(corpo, "corpo\r\n");
    }

    #[test]
    fn o_que_nao_e_frontmatter_fica_no_texto() {
        let inteiro = |texto: &str| {
            let (linhas, corpo) = frontmatter(texto);
            assert!(linhas.is_empty(), "{texto:?} deu {linhas:?}");
            assert_eq!(corpo, texto);
        };
        // Sem o "---" na primeira linha: o traço do meio é uma linha de separação.
        inteiro("# Título\n\n---\n\nname: x\n");
        // Sem fechar.
        inteiro("---\nname: x\n\n# Título\n");
        // Não é uma lista de chaves.
        inteiro("---\n- a\n- b\n---\ncorpo\n");
        inteiro("---\ntexto solto\n---\ncorpo\n");
        // Vazio.
        inteiro("---\n---\ncorpo\n");
        inteiro("");
    }

    /// Os alvos `vitral:arquivo/…` dos links do texto, com o caminho de verdade e a linha.
    fn alvos(texto: &str) -> Vec<(PathBuf, Option<u32>)> {
        texto
            .split("](vitral:")
            .skip(1)
            .map(|resto| {
                let url = format!("vitral:{}", &resto[..resto.find(')').unwrap()]);
                let (caminho, linha) = ler_link(&url).unwrap();
                (dunce::canonicalize(caminho).unwrap(), linha)
            })
            .collect()
    }

    #[test]
    fn os_links_relativos_do_markdown_abrem_o_arquivo() {
        let raiz = std::env::temp_dir().join(format!("vitral-teste-previa-{}", crate::modelo::novo_id()));
        let docs = raiz.join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::create_dir_all(raiz.join("loja")).unwrap();
        for arquivo in ["docs/guia.md", "docs/outro.md", "loja/carrinho.py"] {
            std::fs::write(raiz.join(arquivo), "x\n").unwrap();
        }
        let raiz = dunce::canonicalize(&raiz).unwrap();
        let docs = raiz.join("docs");
        let texto = "Veja [o outro](outro.md), [o carrinho](../loja/carrinho.py#L3), [a web](https://exemplo.com/a.md), \
                     [a seção](#instalar), [sumiu](nada.md) e ![o logo](logo.png).";
        let saida = ligar_links(texto, &docs, &raiz);
        // O relativo parte da pasta do .md; a linha do link vai junto.
        assert_eq!(alvos(&saida), [(docs.join("outro.md"), None), (raiz.join("loja/carrinho.py"), Some(3))]);
        assert!(saida.contains("[o outro](vitral:arquivo/"));
        // O que não é arquivo daqui fica como está.
        for igual in ["[a web](https://exemplo.com/a.md)", "[a seção](#instalar)", "[sumiu](nada.md)", "![o logo](logo.png)"] {
            assert!(saida.contains(igual), "{igual} mudou em {saida}");
        }
        // O caminho escrito a partir da pasta do chat também abre.
        assert_eq!(alvos(&ligar_links("O [carrinho](loja/carrinho.py).", &docs, &raiz)), [(raiz.join("loja/carrinho.py"), None)]);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn o_link_da_previa_so_abre_arquivo_e_a_web() {
        assert_eq!(destino_do_link("vitral:arquivo/0/%2Ftmp%2Fguia.md"), Destino::Arquivo);
        assert_eq!(destino_do_link("vitral:arquivo/12-20/%2Ftmp%2Fx.rs"), Destino::Arquivo);
        assert_eq!(destino_do_link("https://exemplo.com"), Destino::Web);
        assert_eq!(destino_do_link("http://exemplo.com"), Destino::Web);
        // Um .md não põe comando na caixa de ninguém, nem abre o que não é arquivo ou a web.
        assert_eq!(destino_do_link("vitral:comando/abc/compact"), Destino::Nenhum);
        assert_eq!(destino_do_link("file:///etc/passwd"), Destino::Nenhum);
        assert_eq!(destino_do_link("javascript:alert(1)"), Destino::Nenhum);
        assert_eq!(destino_do_link("outro.md"), Destino::Nenhum);
    }
}
