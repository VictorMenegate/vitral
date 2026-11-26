//! Arquivos citados na resposta do assistente viram links: `produtos/views.py`, `src/motor.rs:120`,
//! `src/motor.rs#L10-L20`, `file:///p/x.rs` e `[o teste](tests/versao.rs#L40)` abrem no painel da
//! direita, na linha (ou com as linhas marcadas); uma imagem abre no visor. Na saída de um comando,
//! `src/x.rs:120:5`, `src/x.ts(12,5)` e `"x.py", line 12` também (`links_da_saida`). E o comando de
//! barra citado em código em linha (`/revisar`) vira link que o põe na caixa (`vitral:comando/…`).
//!
//! Só vira link o que está no disco (a pasta do chat é a base dos caminhos relativos), então
//! "1.5", "e.g." e um nome que o assistente só imaginou ficam como texto. O caminho que não existe
//! como foi escrito é procurado, pelo fim dele, entre os arquivos da pasta do chat (`Ligador`):
//! "mensagens.rs:120" abre o `src/ui/mensagens.rs`, se só um termina assim. O link carrega o
//! caminho inteiro (`vitral:arquivo/<linhas>/<caminho>`); quem decide se ele abre é a janela, que
//! só aceita arquivo dentro de uma pasta que o Vitral conhece, ou imagem.

use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

/// Maior caminho que vale a pena conferir no disco.
const MAX_CAMINHO: usize = 260;

/// Por quanto tempo vale a resposta do disco: a conversa redesenha a cada quadro, e o arquivo que
/// o assistente acabou de criar tem de virar link logo.
const VALIDADE: Duration = Duration::from_secs(3);

/// As linhas de uma citação: a primeira e a última (iguais quando é uma só).
pub type Faixa = (u32, u32);

/// O número (maior que zero) do começo de `s` e quantos bytes ele ocupa.
fn numero(s: &str) -> Option<(u32, usize)> {
    let fim = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    Some((s[..fim].parse::<u32>().ok().filter(|n| *n > 0)?, fim))
}

/// A faixa de `a` até `b` (ou só `a`), na ordem.
fn faixa_de(a: u32, b: Option<u32>) -> Faixa {
    let b = b.unwrap_or(a);
    (a.min(b), a.max(b))
}

/// "src/x.rs:42", "src/x.rs:42:7" (a coluna fica de fora), "src/x.rs:10-20", "src/x.rs#L42",
/// "src/x.rs#L10-L20", "src/x.rs#L10-20" e "src/x.rs#L10C4-L20C2" viram o caminho e as linhas.
pub fn separar_faixa(texto: &str) -> (&str, Option<Faixa>) {
    if let Some((caminho, ancora)) = texto.split_once('#') {
        let faixa = ancora.strip_prefix(['L', 'l']).and_then(numero).map(|(a, n)| {
            // Depois da linha: a coluna ("C4") e, com hífen, a última linha ("-L20", "-20").
            let resto = &ancora[1 + n..];
            let resto = resto.strip_prefix(['C', 'c']).map_or(resto, |c| c.trim_start_matches(|d: char| d.is_ascii_digit()));
            let ate = resto.strip_prefix('-').map(|r| r.strip_prefix(['L', 'l']).unwrap_or(r)).and_then(numero);
            faixa_de(a, ate.map(|(b, _)| b))
        });
        return (caminho, faixa);
    }
    // O primeiro ":" depois da última barra: "C:\x" não aparece aqui, e "a/b.rs:10:2" tem dois.
    let barra = texto.rfind('/').map_or(0, |i| i + 1);
    let Some(i) = texto[barra..].find(':') else {
        return (texto, None);
    };
    let (caminho, resto) = texto.split_at(barra + i);
    match numero(&resto[1..]) {
        Some((a, n)) => {
            let ate = resto[1 + n..].strip_prefix('-').and_then(numero);
            (caminho, Some(faixa_de(a, ate.map(|(b, _)| b))))
        }
        None => (texto, None),
    }
}

/// O caminho e a primeira linha de uma citação (ver `separar_faixa`).
pub fn separar_linha(texto: &str) -> (&str, Option<u32>) {
    let (caminho, faixa) = separar_faixa(texto);
    (caminho, faixa.map(|(primeira, _)| primeira))
}

/// Separa as partes de um caminho: a barra e, no Windows, também a invertida (o Claude no Windows
/// repete a saída das ferramentas com `C:\Users\…\src\main.rs`; auditoria de 26/11/2026).
fn e_barra(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

/// O texto tem cara de caminho de arquivo: sem espaço, com uma extensão ou uma barra, e com letra
/// (o "0.6.0" e o "12/10" ficam de fora antes de ir ao disco).
pub fn parece_caminho(texto: &str) -> bool {
    let (caminho, _) = separar_linha(texto);
    if caminho.len() < 3 || caminho.len() > MAX_CAMINHO || caminho.contains("://") || caminho.starts_with('-') {
        return false;
    }
    // No Windows, a barra invertida e o ":" da unidade ("C:\x\y.rs") também.
    let permitido = |c: char| {
        c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '~' | '@' | '+' | '[' | ']' | '(' | ')') || (cfg!(windows) && matches!(c, '\\' | ':'))
    };
    if !caminho.chars().all(permitido) {
        return false;
    }
    let nome = caminho.rsplit(e_barra).next().unwrap_or(caminho);
    let com_extensao = nome.rsplit_once('.').is_some_and(|(antes, ext)| {
        !antes.is_empty() && (1..=10).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) && ext.chars().any(|c| c.is_ascii_alphabetic())
    });
    let com_barra = caminho.contains(e_barra) && !nome.is_empty();
    (com_extensao || com_barra) && caminho.chars().any(char::is_alphabetic)
}

/// O texto com tudo o que não é letra, dígito, ponto, hífen ou sublinhado em `%XX` (a barra
/// inclusive: o link tem três partes).
pub(super) fn codificado(texto: &str) -> String {
    let mut saida = String::with_capacity(texto.len() + 16);
    for byte in texto.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_') {
            saida.push(byte as char);
        } else {
            saida.push_str(&format!("%{byte:02X}"));
        }
    }
    saida
}

/// O alvo de um link `vitral:arquivo/…`: as linhas ("0" é nenhuma, "12" uma, "10-20" de uma a
/// outra) e o caminho em `%XX`.
pub fn link_com_faixa(caminho: &str, faixa: Option<Faixa>) -> String {
    let linhas = match faixa {
        Some((a, b)) if b > a => format!("{a}-{b}"),
        Some((a, _)) => a.to_string(),
        None => "0".to_string(),
    };
    format!("vitral:arquivo/{linhas}/{}", codificado(caminho))
}

/// O alvo de um link `vitral:arquivo/…` de uma linha só (ou de nenhuma).
pub fn link_de(caminho: &str, linha: Option<u32>) -> String {
    link_com_faixa(caminho, linha.map(|l| (l, l)))
}

/// O alvo do link de um comando de barra citado: `vitral:comando/<chat>/<nome>`, os dois em `%XX`.
/// O chat vai junto porque o clique não diz de que painel veio: com três conversas lado a lado, o
/// comando tem de cair na caixa da conversa em que foi citado.
pub fn link_do_comando(chat: &str, nome: &str) -> String {
    format!("vitral:comando/{}/{}", codificado(chat), codificado(nome))
}

/// Tem cara de nome de comando ou de skill ("revisar", "mattpocock-skills:tdd").
fn nome_de_comando(nome: &str) -> bool {
    !nome.is_empty() && nome.len() <= 80 && nome.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
}

/// O chat e o comando de um link `vitral:comando/<chat>/<nome>`.
pub fn ler_comando(url: &str) -> Option<(String, String)> {
    let (chat, nome) = url.trim().strip_prefix("vitral:comando/")?.split_once('/')?;
    let (chat, nome) = (caminho_do_link(chat)?, caminho_do_link(nome)?);
    nome_de_comando(&nome).then_some((chat, nome))
}

/// O que fica na caixa quando o comando citado é clicado: o comando e, depois dele, o rascunho
/// que estava lá (vira o argumento; nada se perde). Outro comando já na caixa dá lugar a este, e
/// fica o que vinha depois do nome dele.
pub fn com_o_comando(caixa: &str, comando: &str) -> String {
    let rascunho = caixa.trim();
    let argumento = match rascunho.strip_prefix('/') {
        Some(resto) => resto.split_once(char::is_whitespace).map_or("", |(_, depois)| depois.trim_start()),
        None => rascunho,
    };
    format!("/{comando} {argumento}")
}

/// O caminho de volta do `%XX`; `None` quando não é UTF-8 ou tem um `%` solto.
pub fn caminho_do_link(codificado: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(codificado.len());
    let mut resto = codificado.as_bytes();
    while let Some((primeiro, depois)) = resto.split_first() {
        if *primeiro == b'%' {
            let par = std::str::from_utf8(depois.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(par, 16).ok()?);
            resto = &depois[2..];
        } else {
            bytes.push(*primeiro);
            resto = depois;
        }
    }
    String::from_utf8(bytes).ok().filter(|c| !c.is_empty())
}

/// Quem responde pelo que o texto cita: o arquivo (o caminho como foi escrito, sem a linha, vira o
/// caminho inteiro, se ele existe) e o comando de barra (o nome vira o link dele, se o chat tem o
/// comando: `link_do_comando`).
struct Citado<'a> {
    arquivo: &'a mut dyn FnMut(&str) -> Option<String>,
    comando: &'a mut dyn FnMut(&str) -> Option<String>,
}

/// Troca, num texto, os arquivos citados por links (ver `ligar_resposta`), sem ligar comando
/// nenhum: é o do seu balão.
pub fn ligar_arquivos(texto: &str, resolver: &mut dyn FnMut(&str) -> Option<String>) -> String {
    ligar_resposta(texto, resolver, &mut |_| None)
}

/// Troca, no texto do assistente, os arquivos citados por links e o comando de barra citado em
/// código em linha (`/revisar`) pelo link que o põe na caixa. `arquivo` recebe o caminho como foi
/// escrito (sem a linha) e devolve o caminho inteiro, se o arquivo existe; `comando` recebe o
/// nome e devolve o link dele (`link_do_comando`), se o chat tem um comando com esse nome. Blocos
/// de código, títulos e código indentado ficam como estão; um link que já aponta para a web também.
pub fn ligar_resposta(
    texto: &str,
    arquivo: &mut dyn FnMut(&str) -> Option<String>,
    comando: &mut dyn FnMut(&str) -> Option<String>,
) -> String {
    // Sem ponto nem barra não há o que ligar: a maioria das frases curtas sai por aqui.
    if !texto.contains(['.', '/']) {
        return texto.to_string();
    }
    let resolver = &mut Citado { arquivo, comando };
    let mut saida = String::with_capacity(texto.len() + 64);
    let mut cerca: Option<&str> = None;
    for (i, linha) in texto.split('\n').enumerate() {
        if i > 0 {
            saida.push('\n');
        }
        let aparada = linha.trim_start();
        let marca = ["```", "~~~"].into_iter().find(|m| aparada.starts_with(m));
        match (cerca, marca) {
            (None, Some(m)) => {
                cerca = Some(m);
                saida.push_str(linha);
                continue;
            }
            (Some(aberta), Some(m)) if aberta == m => {
                cerca = None;
                saida.push_str(linha);
                continue;
            }
            (Some(_), _) => {
                saida.push_str(linha);
                continue;
            }
            (None, None) => {}
        }
        let titulo = aparada.starts_with('#') && aparada.trim_start_matches('#').starts_with(' ');
        let indentado = linha.starts_with("    ") || linha.starts_with('\t');
        if titulo || indentado {
            saida.push_str(linha);
        } else {
            ligar_linha(linha, resolver, &mut saida);
        }
    }
    saida
}

/// O texto começa por `file://`.
fn e_file(texto: &str) -> bool {
    texto.get(..7).is_some_and(|esquema| esquema.eq_ignore_ascii_case("file://"))
}

/// O caminho de um `file:///home/v/a%20b.rs`: sem o esquema e com os `%XX` desfeitos.
fn caminho_do_file(url: &str) -> Option<String> {
    let resto = if e_file(url) { &url[7..] } else { return None };
    let resto = resto.strip_prefix("localhost").unwrap_or(resto);
    if !resto.starts_with('/') || resto.len() < 2 {
        return None;
    }
    let caminho = caminho_do_link(resto).unwrap_or_else(|| resto.to_string());
    // "file:///C:/x": no Windows, sem a barra da frente.
    let bytes = caminho.as_bytes();
    let com_unidade = bytes.len() > 3 && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' && bytes[3] == b'/';
    Some(if com_unidade { caminho[1..].to_string() } else { caminho })
}

/// O caminho e as linhas de uma citação ("src/x.rs:12:5", "src/x.rs#L10-L20",
/// "file:///p/x.rs#L3"); `None` quando o texto não tem cara de caminho.
fn citado(escrito: &str) -> Option<(Cow<'_, str>, Option<Faixa>)> {
    let (parte, faixa) = separar_faixa(escrito);
    if let Some(caminho) = caminho_do_file(parte) {
        return (caminho.len() <= MAX_CAMINHO).then_some((Cow::Owned(caminho), faixa));
    }
    parece_caminho(escrito).then_some((Cow::Borrowed(parte), faixa))
}

fn alvo_de(escrito: &str, resolver: &mut dyn FnMut(&str) -> Option<String>) -> Option<String> {
    let (caminho, faixa) = citado(escrito)?;
    Some(link_com_faixa(&resolver(&caminho)?, faixa))
}

fn ligar_linha(linha: &str, resolver: &mut Citado, saida: &mut String) {
    let mut comum = String::new();
    let mut i = 0;
    while i < linha.len() {
        let resto = &linha[i..];
        // Código em linha: `src/x.rs:42` vira o link com o código dentro.
        if resto.starts_with('`') {
            let n = resto.len() - resto.trim_start_matches('`').len();
            let marca = &resto[..n];
            if let Some(fim) = resto[n..].find(marca) {
                ligar_palavras(&comum, resolver.arquivo, saida);
                comum.clear();
                let dentro = resto[n..n + fim].trim();
                let inteiro = &resto[..n + fim + n];
                // O comando vem antes do arquivo: `/init` é o comando do chat, não um arquivo da raiz.
                let comando = dentro.strip_prefix('/').filter(|nome| nome_de_comando(nome)).and_then(|nome| (resolver.comando)(nome));
                let alvo = comando.or_else(|| alvo_de(dentro, resolver.arquivo));
                match alvo {
                    Some(alvo) => saida.push_str(&format!("[{inteiro}]({alvo})")),
                    None => saida.push_str(inteiro),
                }
                i += n + fim + n;
                continue;
            }
        }
        // Link do markdown: o que aponta para um arquivo daqui ("[x](src/x.rs#L4)") passa a abrir
        // no Vitral; o resto fica inteiro.
        if resto.starts_with('[')
            && let Some(meio) = resto.find("](")
            && let Some(fim) = resto[meio..].find(')')
        {
            ligar_palavras(&comum, resolver.arquivo, saida);
            comum.clear();
            let url = resto[meio + 2..meio + fim].trim();
            let escrito = url.split_whitespace().next().unwrap_or(url).trim_matches(['<', '>']);
            let imagem = i > 0 && linha[..i].ends_with('!');
            match (imagem, alvo_de(escrito, resolver.arquivo)) {
                (false, Some(alvo)) => saida.push_str(&format!("{}({alvo})", &resto[..meio + 1])),
                _ => saida.push_str(&resto[..meio + fim + 1]),
            }
            i += meio + fim + 1;
            continue;
        }
        if resto.starts_with('<')
            && let Some(fim) = resto.find('>')
            && resto[1..fim].contains("://")
        {
            ligar_palavras(&comum, resolver.arquivo, saida);
            comum.clear();
            // O endereço entre < > fica como está; o `file://` de um arquivo que existe abre aqui.
            let dentro = &resto[1..fim];
            match alvo_de(dentro, resolver.arquivo).filter(|_| e_file(dentro)) {
                Some(alvo) => saida.push_str(&format!("[{dentro}]({alvo})")),
                None => saida.push_str(&resto[..fim + 1]),
            }
            i += fim + 1;
            continue;
        }
        let c = resto.chars().next().unwrap_or_default();
        comum.push(c);
        i += c.len_utf8().max(1);
    }
    ligar_palavras(&comum, resolver.arquivo, saida);
}

/// O texto corrido: só a palavra com barra e extensão ("src/ui/mod.rs:120") e o `file://` viram
/// link. Um nome solto ("config.json") fica como texto: no meio da frase é cedo demais para
/// adivinhar.
fn ligar_palavras(trecho: &str, resolver: &mut dyn FnMut(&str) -> Option<String>, saida: &mut String) {
    if !trecho.contains(e_barra) {
        saida.push_str(trecho);
        return;
    }
    let mut resto = trecho;
    while !resto.is_empty() {
        let inicio = resto.find(|c: char| !c.is_whitespace()).unwrap_or(resto.len());
        saida.push_str(&resto[..inicio]);
        resto = &resto[inicio..];
        let fim = resto.find(char::is_whitespace).unwrap_or(resto.len());
        let palavra = &resto[..fim];
        resto = &resto[fim..];
        // A pontuação da frase em volta fica fora do link: "(src/x.rs)," e "src/x.rs.".
        let miolo = palavra.trim_start_matches(['(', '"', '\'', '*', '_']);
        let antes = &palavra[..palavra.len() - miolo.len()];
        let limpo = miolo.trim_end_matches(['.', ',', ';', ':', ')', '"', '\'', '!', '?', '*', '_']);
        let depois = &miolo[limpo.len()..];
        let (caminho, _) = separar_linha(limpo);
        let cara = e_file(limpo) || (caminho.contains(e_barra) && caminho.rsplit(e_barra).next().is_some_and(|n| n.contains('.')) && !limpo.contains("://"));
        match cara.then(|| alvo_de(limpo, resolver)).flatten() {
            Some(alvo) => saida.push_str(&format!("{antes}[{limpo}]({alvo}){depois}")),
            None => saida.push_str(palavra),
        }
    }
}

/// O que o disco disse de um caminho.
#[derive(Clone, Copy, PartialEq)]
enum NoDisco {
    Arquivo,
    Pasta,
    Nada,
}

thread_local! {
    static VISTOS: RefCell<HashMap<PathBuf, (Instant, NoDisco)>> = RefCell::new(HashMap::new());
}

/// O caminho como foi escrito, inteiro: `~/` é a pasta pessoal, o absoluto vale como está e o
/// relativo parte da pasta do chat.
fn inteiro(base: &Path, escrito: &str) -> Option<PathBuf> {
    Some(if let Some(resto) = escrito.strip_prefix("~/") {
        dirs::home_dir()?.join(resto)
    } else {
        let bruto = PathBuf::from(escrito);
        if bruto.is_absolute() { bruto } else { base.join(bruto) }
    })
}

/// O que há no disco nesse caminho. A resposta vale por alguns segundos (a conversa pergunta a
/// cada quadro).
fn no_disco(caminho: &Path) -> NoDisco {
    VISTOS.with(|vistos| {
        let mut vistos = vistos.borrow_mut();
        let agora = Instant::now();
        if let Some((quando, visto)) = vistos.get(caminho)
            && agora.duration_since(*quando) < VALIDADE
        {
            return *visto;
        }
        if vistos.len() > 4000 {
            vistos.clear();
        }
        let visto = match std::fs::metadata(caminho) {
            Ok(m) if m.is_dir() => NoDisco::Pasta,
            Ok(_) => NoDisco::Arquivo,
            Err(_) => NoDisco::Nada,
        };
        vistos.insert(caminho.to_path_buf(), (agora, visto));
        visto
    })
}

/// O caminho inteiro de um arquivo citado, se ele está no disco (ver `inteiro`).
pub fn resolver(base: &Path, escrito: &str) -> Option<String> {
    let caminho = inteiro(base, escrito)?;
    (no_disco(&caminho) == NoDisco::Arquivo).then(|| caminho.to_string_lossy().into_owned())
}

/// O caminho inteiro de uma menção que está no disco: arquivo ou pasta (a pasta sai com a barra
/// no fim, como o "@" a escreve).
pub fn resolver_mencao(base: &Path, escrito: &str) -> Option<String> {
    let caminho = inteiro(base, escrito.trim_end_matches('/'))?;
    let texto = caminho.to_string_lossy().into_owned();
    match no_disco(&caminho) {
        NoDisco::Arquivo => Some(texto),
        NoDisco::Pasta => Some(format!("{}/", texto.trim_end_matches('/'))),
        NoDisco::Nada => None,
    }
}

/// O Markdown sem os links `vitral:…` que `ligar_arquivos` pôs nele, para copiar (a tabela de uma
/// resposta, que o kit devolve em Markdown já com os links): o caminho volta a ser código em
/// linha, como o agente costuma escrevê-lo, e o rótulo que não é caminho ("[o relatório](…)")
/// fica só o texto. Os links da web ficam como estão.
pub fn sem_links_do_vitral(markdown: &str) -> String {
    let mut saida = String::with_capacity(markdown.len());
    let mut resto = markdown;
    while let Some(meio) = resto.find("](vitral:") {
        let (Some(abre), Some(fecha)) = (resto[..meio].rfind('['), resto[meio..].find(')').map(|f| meio + f)) else { break };
        let rotulo = &resto[abre + 1..meio];
        saida.push_str(&resto[..abre]);
        if !rotulo.contains('`') && parece_caminho(separar_linha(rotulo).0) {
            saida.push_str(&format!("`{rotulo}`"));
        } else {
            saida.push_str(rotulo);
        }
        resto = &resto[fecha + 1..];
    }
    saida.push_str(resto);
    saida
}

/// Um link `vitral:arquivo/…` clicado no texto do assistente (a janela abre o arquivo).
#[derive(Clone, PartialEq, Debug, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = vitral, no_json)]
pub struct AbrirLinkVitral(pub String);

/// O link pode abrir: `http(s)` sempre; `vitral:arquivo/…` e `vitral:comando/…` só no texto do
/// assistente (é ele que `ligar_resposta` gera); o resto (`file://`, `javascript:`, caminhos
/// soltos) nunca.
pub fn link_permitido(url: &str, do_assistente: bool) -> bool {
    let url = url.trim();
    let comeca = |p: &str| url.len() > p.len() && url[..p.len()].eq_ignore_ascii_case(p);
    if comeca("http://") || comeca("https://") {
        return true;
    }
    do_assistente && comeca("vitral:") && (ler_link(url).is_some() || ler_comando(url).is_some())
}

/// O caminho inteiro e a linha de um `vitral:arquivo/<linha>/<caminho em %XX>` ou de uma menção,
/// `vitral:mencao/<linhas>/<caminho em %XX>` (a primeira das linhas "120-134").
pub fn ler_link(url: &str) -> Option<(String, Option<u32>)> {
    // O trecho da menção (`?t=…`) não faz parte do caminho.
    let url = url.trim().split("?t=").next().unwrap_or_default();
    let resto = url.strip_prefix("vitral:arquivo/").or_else(|| url.strip_prefix("vitral:mencao/"))?;
    let (linha, caminho) = resto.split_once('/')?;
    let primeira = linha.split('-').next().unwrap_or(linha);
    Some((caminho_do_link(caminho)?, primeira.parse::<u32>().ok().filter(|n| *n > 0)))
}

/// As linhas "120-134" de um link `vitral:…` (a primeira e a última), quando ele marca mais de uma.
pub fn ler_faixa(url: &str) -> Option<(u32, u32)> {
    let url = url.trim().split("?t=").next().unwrap_or_default();
    let resto = url.strip_prefix("vitral:arquivo/").or_else(|| url.strip_prefix("vitral:mencao/"))?;
    let (linhas, _) = resto.split_once('/')?;
    let (a, b) = linhas.split_once('-')?;
    let (a, b) = (a.parse::<u32>().ok().filter(|n| *n > 0)?, b.parse::<u32>().ok().filter(|n| *n > 0)?);
    Some((a.min(b), a.max(b)))
}

/// A pasta conhecida mais funda que contém o arquivo, e o caminho dele dentro dela (com "/").
pub fn raiz_do_arquivo(arquivo: &Path, pastas: impl Iterator<Item = PathBuf>) -> Option<(PathBuf, String)> {
    pastas
        .filter(|p| !p.as_os_str().is_empty())
        .filter_map(|p| dunce::canonicalize(&p).ok())
        .filter(|p| arquivo.starts_with(p) && arquivo != p)
        .max_by_key(|p| p.components().count())
        .and_then(|raiz| {
            let relativo = arquivo.strip_prefix(&raiz).ok()?;
            let relativo = relativo.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            Some((raiz, relativo))
        })
}

impl super::Janela {
    /// Quem acha os arquivos citados nas conversas da pasta `cwd` (ver `Ligador`): com a lista de
    /// arquivos da pasta, se ela já foi lida (o "@" e a aba Arquivos a leem). A lista que fez
    /// falta no quadro anterior é lida agora, em segundo plano; quando chega, a janela redesenha
    /// e a citação vira link.
    pub(super) fn ligador(&self, cwd: &Path, cx: &gpui_kit::Context<Self>) -> Ligador {
        for pasta in listas_pedidas() {
            self.ler_arquivos(&pasta, cx);
        }
        let cache = self.cache_arquivos.borrow();
        // A primeira leitura em andamento ainda não tem o que oferecer.
        let lidos = cache.get(cwd).filter(|lidos| lidos.lidos_em.is_some() || !lidos.lista.is_empty());
        let velha = lidos.is_some_and(|lidos| lidos.lidos_em.is_none_or(|quando| quando.elapsed() >= LISTA_VELHA));
        Ligador::novo(cwd, lidos.map(|lidos| lidos.lista.clone()), velha)
    }

    /// O comando de barra que o agente citou e você clicou: vai para a caixa do painel em que a
    /// conversa dele está (o ativo, se for ele), e quem envia é você. Só o comando que o chat tem.
    fn por_comando_na_caixa(&mut self, chat: &str, comando: &str, window: &mut gpui_kit::Window, cx: &mut gpui_kit::Context<Self>) {
        let Some(motor) = self.motor.clone() else { return };
        let do_chat = |p: &super::paineis::Painel| p.chat.as_deref() == Some(chat);
        let Some((painel, composer)) = self
            .paineis
            .get(self.ativo)
            .filter(|p| do_chat(p))
            .or_else(|| self.paineis.iter().find(|p| do_chat(p)))
            .map(|p| (p.id, p.composer.clone()))
        else {
            return;
        };
        let existe = motor.read(cx).chat(chat).is_some_and(|c| self.comandos_de(c).iter().any(|c| c == comando));
        if !existe {
            return;
        }
        let rascunho = composer.read(cx).value().to_string();
        self.por_na_caixa(painel, &com_o_comando(&rascunho, comando), window, cx);
    }

    /// Um link `vitral:arquivo/…` do texto do assistente: a imagem abre no visor; o resto, no painel
    /// da direita, na linha. O que está na pasta de um chat abre com ela como raiz; o que está fora,
    /// com a pasta do próprio arquivo. O `vitral:comando/…` põe o comando na caixa.
    pub(super) fn abrir_link_vitral(&mut self, url: &str, window: &mut gpui_kit::Window, cx: &mut gpui_kit::Context<Self>) {
        use super::componentes::{Tom, aviso};
        use gpui_kit::component::WindowExt as _;
        if let Some((chat, comando)) = ler_comando(url) {
            self.por_comando_na_caixa(&chat, &comando, window, cx);
            return;
        }
        let Some(motor) = self.motor.clone() else { return };
        let Some((caminho, linha)) = ler_link(url) else { return };
        let faixa = ler_faixa(url);
        let Ok(inteiro) = dunce::canonicalize(&caminho) else {
            window.push_notification(aviso(Tom::Erro, format!("{caminho} não existe mais")), cx);
            return;
        };
        // A pílula de uma pasta mencionada abre a pasta no gerenciador do sistema.
        if inteiro.is_dir() {
            crate::sistema::abrir_pasta(&inteiro, cx);
            return;
        }
        let chat = self.paineis.get(self.ativo).and_then(|p| p.chat.clone());
        if crate::claude::imagem_de_ver(&inteiro)
            && let Some(chat) = &chat
        {
            self.abrir_visor(chat, &inteiro, window, cx);
            return;
        }
        let raiz = {
            let m = motor.read(cx);
            raiz_do_arquivo(&inteiro, m.chats.iter().map(|c| c.cwd.clone()))
        };
        // Fora das pastas dos chats, abre do mesmo jeito, com a pasta do próprio arquivo como raiz
        // (dono, 13/11/2026: "tem que nada ter que abrir no VS Code").
        let de_fora = || {
            let pasta = inteiro.parent()?.to_path_buf();
            Some((pasta, inteiro.file_name()?.to_string_lossy().into_owned()))
        };
        let Some((raiz, relativo)) = raiz.or_else(de_fora) else {
            window.push_notification(aviso(Tom::Atencao, format!("Não deu para abrir {caminho}.")), cx);
            return;
        };
        self.arquivo_linha.set(linha);
        self.arquivo_marcada.set(linha);
        self.abrir_arquivo(raiz, relativo, window, cx);
        // A menção de um trecho (a pílula "fila.rs:120-134") abre com as linhas escolhidas, como
        // uma seleção feita no painel: dá para mencionar de novo (dono, 16/11/2026).
        if let Some((a, b)) = faixa {
            self.arquivo_selecao = Some((a, b));
            self.arquivo_marcada.set(None);
        }
        // A pílula de um pedaço de linha: o pedaço exato fica marcado, no lugar das linhas inteiras
        // (protótipo Linear, 25/11/2026).
        if let Some(trecho) = super::mencao::trecho_do_link(url)
            && let Some(linhas) = faixa.or(linha.map(|l| (l, l)))
            && let Ok(texto) = std::fs::read_to_string(&inteiro)
            && let Some(selecao) = super::painel_arquivo::selecao_do_trecho(&texto, linhas, &trecho)
        {
            self.selecao_no_codigo = Some(selecao);
            self.arquivo_selecao = None;
        }
    }
}

/// O pedaço de um caminho citado que dá para procurar pelo fim: relativo, sem o "./" da frente e
/// sem "..".
fn para_o_fim(escrito: &str) -> Option<&str> {
    let relativo = escrito.trim_start_matches("./");
    let serve = !relativo.is_empty() && !relativo.starts_with(['/', '~']) && !relativo.split('/').any(|parte| parte.is_empty() || parte == "..");
    serve.then_some(relativo)
}

/// O arquivo de `lista` (caminhos relativos à pasta, com "/") que termina com o caminho citado, a
/// partir do começo de um nome ("ui/mod.rs" acha "src/ui/mod.rs"; "od.rs" não acha nada), se for
/// um só.
pub fn pelo_fim<'a>(lista: &'a [String], escrito: &str) -> Option<&'a str> {
    let relativo = para_o_fim(escrito)?;
    let mut achados = lista.iter().filter(|arquivo| arquivo.strip_suffix(relativo).is_some_and(|antes| antes.is_empty() || antes.ends_with('/')));
    let primeiro = achados.next()?;
    achados.next().is_none().then_some(primeiro.as_str())
}

/// Com mais de um minuto, a lista de arquivos de uma pasta ainda vale, e uma leitura nova fica
/// pedida.
const LISTA_VELHA: Duration = Duration::from_secs(60);

thread_local! {
    /// As pastas cuja lista de arquivos fez falta num quadro: a janela as lê em segundo plano no
    /// seguinte (`Janela::ligador`).
    static PEDIDAS: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    /// O que a procura pelo fim já respondeu, por pasta e caminho citado (ver `VALIDADE`).
    static PELO_FIM: RefCell<HashMap<PathBuf, HashMap<String, (Instant, Option<String>)>>> = RefCell::new(HashMap::new());
}

/// As pastas cuja lista de arquivos fez falta desde a última pergunta (cada uma sai uma vez).
pub fn listas_pedidas() -> Vec<PathBuf> {
    PEDIDAS.with(|pedidas| std::mem::take(&mut *pedidas.borrow_mut()))
}

/// Quem acha o arquivo de uma citação: o caminho como foi escrito (a pasta do chat é a base) e,
/// se ele não existe, o arquivo da pasta que termina com ele, quando é um só ("mensagens.rs:120"
/// abre o `src/ui/mensagens.rs`; "mod.rs", que são vários, fica como texto).
#[derive(Clone)]
pub struct Ligador {
    base: PathBuf,
    /// Os arquivos da pasta (`seletor_arquivos::listar`), se já foram lidos.
    lista: Option<Rc<Vec<String>>>,
    /// A lista é de mais de um minuto atrás.
    velha: bool,
}

impl Ligador {
    pub fn novo(base: &Path, lista: Option<Rc<Vec<String>>>, velha: bool) -> Self {
        Ligador { base: base.to_path_buf(), lista, velha }
    }

    /// O caminho inteiro do arquivo citado, se ele está no disco. A procura pelo fim só acontece
    /// quando o caminho escrito não existe; sem a lista da pasta (ou com ela velha), a leitura
    /// fica pedida (`listas_pedidas`) e a citação vira link quando a lista chegar.
    pub fn resolver(&self, escrito: &str) -> Option<String> {
        if let Some(inteiro) = resolver(&self.base, escrito) {
            return Some(inteiro);
        }
        let relativo = para_o_fim(escrito)?;
        if self.lista.is_none() || self.velha {
            PEDIDAS.with(|pedidas| {
                let mut pedidas = pedidas.borrow_mut();
                if !pedidas.contains(&self.base) {
                    pedidas.push(self.base.clone());
                }
            });
        }
        // A lista cortada no teto não garante que o achado é o único.
        let lista = self.lista.as_ref().filter(|lista| lista.len() < super::seletor_arquivos::TETO)?;
        let agora = Instant::now();
        let guardado = PELO_FIM.with(|guardados| {
            let guardados = guardados.borrow();
            let (quando, achado) = guardados.get(&self.base)?.get(relativo)?;
            (agora.duration_since(*quando) < VALIDADE).then(|| achado.clone())
        });
        let achado = guardado.unwrap_or_else(|| {
            let achado = pelo_fim(lista, relativo).map(|arquivo| self.base.join(arquivo));
            let achado = achado.filter(|caminho| no_disco(caminho) == NoDisco::Arquivo).map(|caminho| caminho.to_string_lossy().into_owned());
            PELO_FIM.with(|guardados| {
                let mut guardados = guardados.borrow_mut();
                if guardados.values().map(HashMap::len).sum::<usize>() > 4000 {
                    guardados.clear();
                }
                guardados.entry(self.base.clone()).or_default().insert(relativo.to_string(), (agora, achado.clone()));
            });
            achado
        });
        achado
    }

    /// O que muda quando a lista da pasta muda: quem guarda o que o ligador respondeu (a caixa
    /// de saída) refaz a conta.
    pub fn marca(&self) -> (usize, usize) {
        self.lista.as_ref().map_or((0, 0), |lista| (Rc::as_ptr(lista) as usize, lista.len()))
    }
}

/// Um arquivo citado na saída de um comando: onde está no texto (em bytes), o link que o abre e
/// o caminho inteiro (a dica).
#[derive(Clone, Debug, PartialEq)]
pub struct LinkNaSaida {
    pub faixa: Range<usize>,
    pub alvo: String,
    pub inteiro: String,
}

/// A maior linha da saída em que vale procurar arquivo (a de um JSON minificado não tem o que
/// ligar), e até quantos candidatos de uma linha vão ao disco.
const MAX_LINHA_DA_SAIDA: usize = 2000;
const MAX_CANDIDATOS_DA_LINHA: usize = 20;

/// A letra pode estar num caminho da saída de um comando. O parêntese e os dois pontos ficam de
/// fora: são eles que trazem a linha ("x.ts(12,5)", "x.rs:12:5").
fn letra_de_caminho(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '~' | '@' | '+' | '[' | ']')
}

/// O que vem logo depois de um caminho na saída: quantos bytes disso entram no link e a linha.
/// ":12", ":12:5" e ":12-20" (cargo, go, pytest, vitest) e "(12,5)" e "(12)" (tsc) entram;
/// `", line 12`, " line 12", ": line 12" e " on line 12" (Python e afins) dão a linha e ficam de
/// fora.
fn linha_depois(depois: &str) -> (usize, Option<Faixa>) {
    if let Some((a, n)) = depois.strip_prefix(':').and_then(numero) {
        let resto = &depois[1 + n..];
        if let Some((_, m)) = resto.strip_prefix(':').and_then(numero) {
            return (1 + n + 1 + m, Some((a, a)));
        }
        return match resto.strip_prefix('-').and_then(numero) {
            Some((b, m)) => (1 + n + 1 + m, Some(faixa_de(a, Some(b)))),
            None => (1 + n, Some((a, a))),
        };
    }
    if let Some((a, n)) = depois.strip_prefix('(').and_then(numero) {
        let mut tamanho = 1 + n;
        if let Some(coluna) = depois[tamanho..].strip_prefix(',') {
            let espacos = coluna.len() - coluna.trim_start_matches(' ').len();
            if let Some((_, m)) = numero(&coluna[espacos..]) {
                tamanho += 1 + espacos + m;
            }
        }
        return if depois[tamanho..].starts_with(')') { (tamanho + 1, Some((a, a))) } else { (0, None) };
    }
    let resto = depois.trim_start_matches(['"', '\'']);
    let resto = resto.strip_prefix([',', ':']).unwrap_or(resto);
    let resto = resto.strip_prefix(" on").unwrap_or(resto);
    match resto.strip_prefix(" line ").and_then(numero) {
        Some((a, _)) => (0, Some((a, a))),
        None => (0, None),
    }
}

/// Os arquivos citados numa linha da saída de um comando: com a linha depois (`linha_depois`),
/// basta o nome ter cara de arquivo ("x.go:3"); sem ela, só o caminho com barra e extensão (o
/// `git status`, o `find`). Endereço da web não entra; o `file://` é um arquivo. `resolver` é o
/// de `ligar_resposta`.
pub fn links_da_linha(linha: &str, resolver: &mut dyn FnMut(&str) -> Option<String>) -> Vec<LinkNaSaida> {
    let mut links = Vec::new();
    if linha.len() > MAX_LINHA_DA_SAIDA || !linha.contains(['.', '/']) {
        return links;
    }
    let (mut i, mut candidatos) = (0, 0);
    while i < linha.len() && candidatos < MAX_CANDIDATOS_DA_LINHA {
        let resto = &linha[i..];
        let Some(c) = resto.chars().next() else { break };
        if !letra_de_caminho(c) {
            i += c.len_utf8();
            continue;
        }
        let inicio = i;
        let mut fim = i + resto.find(|c: char| !letra_de_caminho(c)).unwrap_or(resto.len());
        let mut de_file = false;
        if linha[fim..].starts_with("://") {
            if !linha[inicio..fim].eq_ignore_ascii_case("file") {
                // Um endereço: vai inteiro, até o espaço.
                i = fim + linha[fim..].find(char::is_whitespace).unwrap_or(linha.len() - fim);
                continue;
            }
            let caminho = &linha[fim + 3..];
            fim += 3 + caminho.find(|c: char| !letra_de_caminho(c) && c != '%').unwrap_or(caminho.len());
            de_file = true;
        }
        i = fim;
        // O ponto do fim da frase não é do caminho.
        while fim > inicio && linha[inicio..fim].ends_with('.') {
            fim -= 1;
        }
        let escrito = &linha[inicio..fim];
        let (sufixo, faixa) = linha_depois(&linha[fim..]);
        let caminho = if de_file {
            caminho_do_file(escrito).map(Cow::Owned)
        } else {
            let com_barra_e_extensao = || escrito.contains('/') && escrito.rsplit('/').next().is_some_and(|nome| nome.contains('.'));
            (parece_caminho(escrito) && (faixa.is_some() || com_barra_e_extensao())).then_some(Cow::Borrowed(escrito))
        };
        let Some(caminho) = caminho else { continue };
        candidatos += 1;
        if let Some(inteiro) = resolver(&caminho) {
            links.push(LinkNaSaida { faixa: inicio..fim + sufixo, alvo: link_com_faixa(&inteiro, faixa), inteiro });
            i = i.max(fim + sufixo);
        }
    }
    links
}

/// Os arquivos citados nas últimas `ultimas` linhas da saída de um comando (as que a caixa mostra
/// fechada; conferir o disco para a saída inteira de um `find` pesaria), com a faixa de cada um
/// no texto todo.
pub fn links_da_saida(texto: &str, ultimas: usize, resolver: &mut dyn FnMut(&str) -> Option<String>) -> Vec<LinkNaSaida> {
    let pular = texto.lines().count().saturating_sub(ultimas);
    let mut links = Vec::new();
    let mut comeco = 0;
    for (n, linha) in texto.split('\n').enumerate() {
        if n >= pular {
            links.extend(links_da_linha(linha.trim_end_matches('\r'), resolver).into_iter().map(|mut link| {
                link.faixa = link.faixa.start + comeco..link.faixa.end + comeco;
                link
            }));
        }
        comeco += linha.len() + 1;
    }
    links
}


#[cfg(test)]
mod testes_dos_links_mais_espertos {
    use super::*;

    /// O disco de mentira: `src/ui/mod.rs` na pasta do chat (/p) e um arquivo com espaço no nome.
    fn ligar(texto: &str) -> String {
        ligar_resposta(texto, &mut no_disco_de_mentira, &mut |nome| {
            ["revisar", "compact", "clear", "mattpocock-skills:tdd"].contains(&nome).then(|| link_do_comando("c1", nome))
        })
    }

    fn no_disco_de_mentira(escrito: &str) -> Option<String> {
        match escrito {
            "src/ui/mod.rs" | "/p/src/ui/mod.rs" => Some("/p/src/ui/mod.rs".into()),
            "/p/meu projeto/a.rs" => Some("/p/meu projeto/a.rs".into()),
            "src/x.rs" | "./src/x.rs" => Some("/p/src/x.rs".into()),
            "src/app.ts" => Some("/p/src/app.ts".into()),
            "tests/test_x.py" => Some("/p/tests/test_x.py".into()),
            "/p/abs/y.py" => Some("/p/abs/y.py".into()),
            "x.go" | "./x.go" => Some("/p/x.go".into()),
            _ => None,
        }
    }

    const MOD: &str = "%2Fp%2Fsrc%2Fui%2Fmod.rs";

    // ── Item 3: as linhas, o file:// e a coluna ──

    #[test]
    fn separa_as_linhas_da_citacao() {
        assert_eq!(separar_faixa("src/x.rs#L10-L20"), ("src/x.rs", Some((10, 20))));
        assert_eq!(separar_faixa("src/x.rs#L10-20"), ("src/x.rs", Some((10, 20))));
        assert_eq!(separar_faixa("src/x.rs#L10"), ("src/x.rs", Some((10, 10))));
        assert_eq!(separar_faixa("src/x.rs#L10C4-L20C2"), ("src/x.rs", Some((10, 20))));
        assert_eq!(separar_faixa("src/x.rs#L20-L10"), ("src/x.rs", Some((10, 20))));
        assert_eq!(separar_faixa("src/x.rs:12:5"), ("src/x.rs", Some((12, 12))));
        assert_eq!(separar_faixa("src/x.rs:12-20"), ("src/x.rs", Some((12, 20))));
        assert_eq!(separar_faixa("src/x.rs:12"), ("src/x.rs", Some((12, 12))));
        assert_eq!(separar_faixa("src/x.rs"), ("src/x.rs", None));
        assert_eq!(separar_faixa("src/x.rs#secao"), ("src/x.rs", None));
        assert_eq!(separar_faixa("src/x.rs#L0"), ("src/x.rs", None));
        assert_eq!(separar_faixa("src/x.rs:0"), ("src/x.rs:0", None));
        // O de antes continua valendo: a primeira linha.
        assert_eq!(separar_linha("src/x.rs#L10-L20"), ("src/x.rs", Some(10)));
        assert_eq!(separar_linha("src/x.rs:12:5"), ("src/x.rs", Some(12)));
    }

    #[test]
    fn o_link_leva_a_faixa_e_volta_com_ela() {
        assert_eq!(link_com_faixa("/p/a.rs", Some((10, 20))), "vitral:arquivo/10-20/%2Fp%2Fa.rs");
        assert_eq!(link_com_faixa("/p/a.rs", Some((10, 10))), "vitral:arquivo/10/%2Fp%2Fa.rs");
        assert_eq!(link_com_faixa("/p/a.rs", None), "vitral:arquivo/0/%2Fp%2Fa.rs");
        assert_eq!(link_com_faixa("/p/a.rs", Some((7, 7))), link_de("/p/a.rs", Some(7)));
        assert_eq!(ler_link("vitral:arquivo/10-20/%2Fp%2Fa.rs"), Some(("/p/a.rs".into(), Some(10))));
        assert_eq!(ler_faixa("vitral:arquivo/10-20/%2Fp%2Fa.rs"), Some((10, 20)));
        assert_eq!(ler_faixa("vitral:arquivo/10/%2Fp%2Fa.rs"), None);
        assert!(link_permitido("vitral:arquivo/10-20/%2Fp%2Fa.rs", true));
    }

    #[test]
    fn liga_a_faixa_a_coluna_e_o_file() {
        // `arquivo#L10-L20` e `arquivo#L10`: o link abre com as linhas.
        assert_eq!(ligar("Veja `src/ui/mod.rs#L10-L20`."), format!("Veja [`src/ui/mod.rs#L10-L20`](vitral:arquivo/10-20/{MOD})."));
        assert_eq!(ligar("[a janela](src/ui/mod.rs#L10-L20)"), format!("[a janela](vitral:arquivo/10-20/{MOD})"));
        assert_eq!(ligar("em src/ui/mod.rs#L10 e"), format!("em [src/ui/mod.rs#L10](vitral:arquivo/10/{MOD}) e"));
        // `arquivo:12:5`: a linha; a coluna fica no texto.
        assert_eq!(ligar("erro em src/ui/mod.rs:12:5."), format!("erro em [src/ui/mod.rs:12:5](vitral:arquivo/12/{MOD})."));
        assert_eq!(ligar("`src/ui/mod.rs:12-20`"), format!("[`src/ui/mod.rs:12-20`](vitral:arquivo/12-20/{MOD})"));
        // `file:///caminho`: no link do markdown, no código em linha, solto na frase e entre < >.
        assert_eq!(ligar("[o arquivo](file:///p/src/ui/mod.rs#L3)"), format!("[o arquivo](vitral:arquivo/3/{MOD})"));
        assert_eq!(ligar("`file:///p/src/ui/mod.rs`"), format!("[`file:///p/src/ui/mod.rs`](vitral:arquivo/0/{MOD})"));
        assert_eq!(
            ligar("abra file:///p/meu%20projeto/a.rs:7 agora"),
            "abra [file:///p/meu%20projeto/a.rs:7](vitral:arquivo/7/%2Fp%2Fmeu%20projeto%2Fa.rs) agora"
        );
        assert_eq!(ligar("<file:///p/src/ui/mod.rs>"), format!("[file:///p/src/ui/mod.rs](vitral:arquivo/0/{MOD})"));
        // O que não está no disco fica como veio; e o `file://` nunca vira link que abre sozinho.
        for igual in ["abra file:///nao/existe.rs agora", "[x](file:///nao/existe.rs)", "`file:///nao/existe.rs#L3`", "<file:///nao/existe.rs>", "file://", "`file://`"] {
            assert_eq!(ligar(igual), igual);
        }
        assert!(!link_permitido("file:///p/src/ui/mod.rs", true));
    }

    #[test]
    fn acha_pelo_fim_do_caminho_quando_e_um_so() {
        let lista = ["src/ui/mod.rs", "src/ui/mensagens.rs", "src/motor/mod.rs", "central/src/app.ts", "LEIA-ME.md"].map(String::from);
        assert_eq!(pelo_fim(&lista, "mensagens.rs"), Some("src/ui/mensagens.rs"));
        assert_eq!(pelo_fim(&lista, "ui/mensagens.rs"), Some("src/ui/mensagens.rs"));
        assert_eq!(pelo_fim(&lista, "./src/app.ts"), Some("central/src/app.ts"));
        assert_eq!(pelo_fim(&lista, "ui/mod.rs"), Some("src/ui/mod.rs"));
        assert_eq!(pelo_fim(&lista, "LEIA-ME.md"), Some("LEIA-ME.md"));
        assert_eq!(pelo_fim(&lista, "mod.rs"), None, "dois terminam assim");
        assert_eq!(pelo_fim(&lista, "ensagens.rs"), None, "só a partir do começo de um nome");
        assert_eq!(pelo_fim(&lista, "/src/ui/mod.rs"), None, "o absoluto que não existe não é procurado");
        assert_eq!(pelo_fim(&lista, "~/src/ui/mod.rs"), None);
        assert_eq!(pelo_fim(&lista, "../ui/mod.rs"), None);
        assert_eq!(pelo_fim(&lista, "nao.rs"), None);
        assert_eq!(pelo_fim(&lista, ""), None);
        assert_eq!(pelo_fim(&[], "mod.rs"), None);
    }

    #[test]
    fn o_ligador_confere_o_disco_e_depois_procura_pelo_fim() {
        let pasta = std::env::temp_dir().join(format!("vitral-ligador-{}", std::process::id()));
        std::fs::create_dir_all(pasta.join("src/ui")).unwrap();
        std::fs::write(pasta.join("src/ui/a.rs"), "fn a() {}").unwrap();
        let inteiro = pasta.join("src/ui/a.rs").to_string_lossy().into_owned();
        let lista = std::rc::Rc::new(vec!["src/ui/a.rs".to_string(), "docs/sumiu.md".to_string()]);
        let ligador = Ligador::novo(&pasta, Some(lista), false);
        // O caminho que existe como foi escrito não depende da lista.
        assert_eq!(ligador.resolver("src/ui/a.rs"), Some(inteiro.clone()));
        // O que não existe como foi escrito é procurado pelo fim, e só vale o que está no disco.
        assert_eq!(ligador.resolver("a.rs"), Some(inteiro.clone()));
        assert_eq!(ligador.resolver("ui/a.rs"), Some(inteiro.clone()));
        assert_eq!(ligador.resolver("sumiu.md"), None, "está na lista, mas não existe mais");
        assert_eq!(ligador.resolver("nao.rs"), None);
        assert!(listas_pedidas().is_empty(), "com a lista em dia não há o que pedir");
        // Sem a lista (a pasta ainda não foi lida), só o caminho exato; e a leitura fica pedida.
        let sem_lista = Ligador::novo(&pasta, None, false);
        assert_eq!(sem_lista.resolver("src/ui/a.rs"), Some(inteiro.clone()));
        assert!(listas_pedidas().is_empty(), "o caminho exato não pede a lista");
        assert_eq!(sem_lista.resolver("a.rs"), None);
        assert_eq!(listas_pedidas(), vec![pasta.clone()]);
        assert!(listas_pedidas().is_empty(), "o pedido sai uma vez");
        // Com a lista velha, vale o que ela tem e a releitura fica pedida.
        let velha = Ligador::novo(&pasta, Some(std::rc::Rc::new(vec!["src/ui/a.rs".to_string()])), true);
        assert_eq!(velha.resolver("a.rs"), Some(inteiro));
        assert_eq!(listas_pedidas(), vec![pasta.clone()]);
        let _ = std::fs::remove_dir_all(&pasta);
    }

    // ── Item 4: a saída de comando ──

    /// Os trechos ligados de uma linha da saída e a linha (e o arquivo) de cada um.
    fn saida(linha: &str) -> Vec<(String, String)> {
        links_da_linha(linha, &mut no_disco_de_mentira)
            .into_iter()
            .map(|l| {
                let (inteiro, n) = ler_link(&l.alvo).expect("link de arquivo");
                assert_eq!(inteiro, l.inteiro);
                (linha[l.faixa].to_string(), format!("{inteiro}:{}", n.unwrap_or(0)))
            })
            .collect()
    }

    fn um(trecho: &str, alvo: &str) -> Vec<(String, String)> {
        vec![(trecho.to_string(), alvo.to_string())]
    }

    #[test]
    fn liga_os_arquivos_da_saida_de_comando() {
        // cargo e rustc, vitest, go, pytest: `arquivo:linha:coluna` e `arquivo:linha`.
        assert_eq!(saida("  --> src/x.rs:120:5"), um("src/x.rs:120:5", "/p/src/x.rs:120"));
        assert_eq!(saida(" ❯ src/app.ts:12:5"), um("src/app.ts:12:5", "/p/src/app.ts:12"));
        assert_eq!(saida("./x.go:3:1: syntax error"), um("./x.go:3:1", "/p/x.go:3"));
        assert_eq!(saida("tests/test_x.py:7: AssertionError"), um("tests/test_x.py:7", "/p/tests/test_x.py:7"));
        assert_eq!(saida("x.go:3: sem barra, mas com a linha"), um("x.go:3", "/p/x.go:3"));
        // tsc: `arquivo(linha,coluna)`.
        assert_eq!(saida("src/app.ts(12,5): error TS2322: Type 'x'"), um("src/app.ts(12,5)", "/p/src/app.ts:12"));
        assert_eq!(saida("src/app.ts(12): error"), um("src/app.ts(12)", "/p/src/app.ts:12"));
        // Python e afins: "line 12" depois do arquivo.
        assert_eq!(saida("  File \"tests/test_x.py\", line 12, in test_a"), um("tests/test_x.py", "/p/tests/test_x.py:12"));
        assert_eq!(saida("error at /p/abs/y.py line 9"), um("/p/abs/y.py", "/p/abs/y.py:9"));
        assert_eq!(saida("src/x.rs on line 4"), um("src/x.rs", "/p/src/x.rs:4"));
        // Sem linha: só o caminho com barra e extensão (o `git status`, o `find`).
        assert_eq!(saida("\tmodified:   src/x.rs"), um("src/x.rs", "/p/src/x.rs:0"));
        assert_eq!(saida("Compilando (src/x.rs)."), um("src/x.rs", "/p/src/x.rs:0"));
        assert_eq!(saida("em file:///p/abs/y.py:4 agora"), um("file:///p/abs/y.py:4", "/p/abs/y.py:4"));
        // Dois na mesma linha, cada um no seu lugar.
        assert_eq!(
            saida("src/x.rs:10: aviso; src/app.ts(2,1): erro"),
            vec![("src/x.rs:10".to_string(), "/p/src/x.rs:10".to_string()), ("src/app.ts(2,1)".to_string(), "/p/src/app.ts:2".to_string())]
        );
        // Acento antes não desloca a faixa (ela é em bytes da linha).
        assert_eq!(saida("ação → src/x.rs:1"), um("src/x.rs:1", "/p/src/x.rs:1"));
        // O que não liga: endereço da web, hora, caminho de módulo, o que não existe, nome solto sem linha.
        for nada in [
            "https://exemplo.com/src/x.rs:12",
            "12:30:45 terminou",
            "std::io::Error em x",
            "nao/existe.rs:3:1",
            "x.go sem linha fica como texto",
            "versão 0.9.5 em 12/10",
            "localhost:8080",
            "",
        ] {
            assert_eq!(saida(nada), Vec::new(), "{nada}");
        }
    }

    #[test]
    fn so_as_ultimas_linhas_da_saida_sao_conferidas() {
        let texto = (1..=50).map(|i| format!("src/x.rs:{i}: aviso")).collect::<Vec<_>>().join("\n");
        let mut consultas = 0;
        let links = links_da_saida(&texto, 40, &mut |c| {
            consultas += 1;
            no_disco_de_mentira(c)
        });
        assert_eq!(links.len(), 40);
        assert_eq!(&texto[links[0].faixa.clone()], "src/x.rs:11");
        assert_eq!(&texto[links[39].faixa.clone()], "src/x.rs:50");
        assert_eq!(ler_link(&links[39].alvo), Some(("/p/src/x.rs".into(), Some(50))));
        assert_eq!(consultas, 40, "as 10 linhas de cima nem vão ao disco");
        // Saída curta: todas as linhas.
        assert_eq!(links_da_saida("a\nsrc/x.rs:2\n", 40, &mut no_disco_de_mentira).len(), 1);
        assert!(links_da_saida("", 40, &mut no_disco_de_mentira).is_empty());
    }

    // ── Item 5: o comando de barra citado ──

    #[test]
    fn o_comando_de_barra_citado_vira_link() {
        assert_eq!(ligar("Rode `/revisar` antes do merge."), "Rode [`/revisar`](vitral:comando/c1/revisar) antes do merge.");
        assert_eq!(ligar("`/compact`"), "[`/compact`](vitral:comando/c1/compact)");
        assert_eq!(ligar("`/mattpocock-skills:tdd`"), "[`/mattpocock-skills:tdd`](vitral:comando/c1/mattpocock-skills%3Atdd)");
        // O arquivo e o comando na mesma frase.
        assert_eq!(
            ligar("Mexi em `src/ui/mod.rs`; rode `/revisar`."),
            format!("Mexi em [`src/ui/mod.rs`](vitral:arquivo/0/{MOD}); rode [`/revisar`](vitral:comando/c1/revisar).")
        );
        // Só o código em linha que é o comando inteiro, e só o que está na lista do chat.
        for igual in [
            "Use `/naoexiste` aqui.",
            "Rode /revisar antes.",
            "`/revisar agora`",
            "```\n/revisar\n```",
            "    `/revisar`",
            "# `/revisar`",
            "`/`",
            "`//revisar`",
            "`revisar`",
        ] {
            assert_eq!(ligar(igual), igual);
        }
        // No seu balão (o `ligar_arquivos`) não há comando para ligar.
        assert_eq!(ligar_arquivos("`/revisar`", &mut no_disco_de_mentira), "`/revisar`");
    }

    #[test]
    fn o_link_do_comando_vai_e_volta_e_so_vale_no_texto_do_agente() {
        // O link leva o chat: o clique não diz de que painel veio, e com três conversas lado a lado
        // o comando tem de cair na caixa da conversa em que foi citado.
        assert_eq!(link_do_comando("a1b2c3d4e5", "revisar"), "vitral:comando/a1b2c3d4e5/revisar");
        assert_eq!(ler_comando("vitral:comando/a1b2c3d4e5/revisar"), Some(("a1b2c3d4e5".into(), "revisar".into())));
        assert_eq!(ler_comando(&link_do_comando("c 1", "mattpocock-skills:tdd")), Some(("c 1".into(), "mattpocock-skills:tdd".into())));
        for nao in [
            "vitral:comando/",
            "vitral:comando/revisar",
            "vitral:comando/c1/",
            "vitral:comando//revisar",
            "vitral:comando/c1/a%2Fb",
            "vitral:comando/c1/a b",
            "vitral:comando/c1/%ZZ",
            "vitral:arquivo/0/a.rs",
            "/revisar",
        ] {
            assert_eq!(ler_comando(nao), None, "{nao}");
        }
        assert!(link_permitido("vitral:comando/c1/revisar", true));
        assert!(!link_permitido("vitral:comando/c1/revisar", false));
        assert!(!link_permitido("vitral:comando/", true));
        assert_eq!(ler_link("vitral:comando/c1/revisar"), None);
    }

    #[test]
    fn o_comando_clicado_vai_para_a_caixa_sem_perder_o_rascunho() {
        assert_eq!(com_o_comando("", "revisar"), "/revisar ");
        assert_eq!(com_o_comando("  \n", "revisar"), "/revisar ");
        // O rascunho vira o argumento do comando.
        assert_eq!(com_o_comando("olha o PR 12", "revisar"), "/revisar olha o PR 12");
        // Outro comando já na caixa: troca o nome e fica o resto.
        assert_eq!(com_o_comando("/compact foco no motor", "revisar"), "/revisar foco no motor");
        assert_eq!(com_o_comando("/compact", "revisar"), "/revisar ");
        assert_eq!(com_o_comando("/revisar ", "revisar"), "/revisar ");
        assert_eq!(com_o_comando("/revisar o PR", "revisar"), "/revisar o PR");
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Rodada do Paseo, item 4: a tabela copiada não leva os links internos do Vitral.
    #[test]
    fn o_markdown_copiado_sai_sem_os_links_do_vitral() {
        use super::sem_links_do_vitral;
        assert_eq!(
            sem_links_do_vitral("| [produtos/views.py](vitral:arquivo/0/%2Fhome%2Fx%2Fprodutos%2Fviews.py) | 12 |\n| `produtos/cupons.py` | 5 |"),
            "| `produtos/views.py` | 12 |\n| `produtos/cupons.py` | 5 |"
        );
        // Com a linha, com o código já no rótulo e com um rótulo que não é caminho.
        assert_eq!(sem_links_do_vitral("ver [src/x.rs:42](vitral:arquivo/42/%2Fp%2Fsrc%2Fx.rs) e [`a/b.rs`](vitral:arquivo/0/%2Fp%2Fa%2Fb.rs)"), "ver `src/x.rs:42` e `a/b.rs`");
        assert_eq!(sem_links_do_vitral("[o relatório](vitral:arquivo/0/%2Fp%2Fx.md) pronto"), "o relatório pronto");
        // Os links da web e o texto sem link ficam como estão.
        let web = "| [site](https://exemplo.com/a) | [x] |";
        assert_eq!(sem_links_do_vitral(web), web);
        assert_eq!(sem_links_do_vitral(""), "");
    }

    fn ligar(texto: &str) -> String {
        let mut resolver = |c: &str| {
            ["produtos/views.py", "tests.py", "src/ui/mod.rs", "docs/guia(1).md", "print.png"]
                .contains(&c)
                .then(|| format!("/p/{c}"))
        };
        ligar_arquivos(texto, &mut resolver)
    }

    #[test]
    fn separa_o_caminho_da_linha() {
        assert_eq!(separar_linha("src/x.rs"), ("src/x.rs", None));
        assert_eq!(separar_linha("src/x.rs:42"), ("src/x.rs", Some(42)));
        assert_eq!(separar_linha("src/x.rs:42:7"), ("src/x.rs", Some(42)));
        assert_eq!(separar_linha("src/x.rs:10-20"), ("src/x.rs", Some(10)));
        assert_eq!(separar_linha("src/x.rs#L42"), ("src/x.rs", Some(42)));
        assert_eq!(separar_linha("src/x.rs#L42-L50"), ("src/x.rs", Some(42)));
        assert_eq!(separar_linha("src/x.rs#secao"), ("src/x.rs", None));
        assert_eq!(separar_linha("src/x.rs:0"), ("src/x.rs:0", None));
        assert_eq!(separar_linha("a:b/x.rs"), ("a:b/x.rs", None));
    }

    #[test]
    fn so_o_que_tem_cara_de_caminho_vai_ao_disco() {
        for sim in ["tests.py", "produtos/views.py", "src/x.rs:42", "~/notas/a.md", "/tmp/print.png", "docs/guia(1).md", "bin/rodar"] {
            assert!(parece_caminho(sim), "{sim}");
        }
        for nao in ["0.6.0", "1.5", "12/10", "e.", "git log", "https://exemplo.com/a.png", "--force", "a", "request.session[\"x\"]", "x = y.z"] {
            assert!(!parece_caminho(nao), "{nao}");
        }
    }

    #[test]
    fn o_link_vai_e_volta() {
        let alvo = link_de("/home/v/meu projeto/a(1).rs", Some(12));
        assert_eq!(alvo, "vitral:arquivo/12/%2Fhome%2Fv%2Fmeu%20projeto%2Fa%281%29.rs");
        let codificado = alvo.rsplit('/').next().unwrap();
        assert_eq!(caminho_do_link(codificado).as_deref(), Some("/home/v/meu projeto/a(1).rs"));
        assert_eq!(link_de("/a/ação.md", None), "vitral:arquivo/0/%2Fa%2Fa%C3%A7%C3%A3o.md");
        assert_eq!(caminho_do_link("%2Fa%2Fa%C3%A7%C3%A3o.md").as_deref(), Some("/a/ação.md"));
        assert_eq!(caminho_do_link("a%2"), None);
        assert_eq!(caminho_do_link("a%ZZ"), None);
        assert_eq!(caminho_do_link("%FF"), None);
        assert_eq!(caminho_do_link(""), None);
    }

    #[test]
    fn liga_o_codigo_em_linha_o_link_e_a_palavra_com_barra() {
        assert_eq!(
            ligar("Mexi em `produtos/views.py` e em `tests.py:18`."),
            "Mexi em [`produtos/views.py`](vitral:arquivo/0/%2Fp%2Fprodutos%2Fviews.py) e em [`tests.py:18`](vitral:arquivo/18/%2Fp%2Ftests.py)."
        );
        assert_eq!(
            ligar("Veja [a janela](src/ui/mod.rs#L120) e [o site](https://exemplo.com/a.py)."),
            "Veja [a janela](vitral:arquivo/120/%2Fp%2Fsrc%2Fui%2Fmod.rs) e [o site](https://exemplo.com/a.py)."
        );
        assert_eq!(
            ligar("O erro está em src/ui/mod.rs:120, (produtos/views.py)."),
            "O erro está em [src/ui/mod.rs:120](vitral:arquivo/120/%2Fp%2Fsrc%2Fui%2Fmod.rs), ([produtos/views.py](vitral:arquivo/0/%2Fp%2Fprodutos%2Fviews.py))."
        );
        assert_eq!(ligar("[guia](docs/guia(1).md)"), "[guia](docs/guia(1).md)", "o parêntese no alvo fica como o markdown leu");
    }

    #[test]
    fn deixa_como_esta_o_que_nao_existe_ou_nao_e_frase() {
        // Não existe, nome solto na frase, URL, versão.
        for igual in [
            "Criei `nao/existe.py`.",
            "O tests.py falhou.",
            "Abra https://exemplo.com/produtos/views.py agora.",
            "Versão 0.6.0, em 12/10.",
            "# produtos/views.py",
            "    produtos/views.py",
            "```\nprodutos/views.py\n```",
            "![print](print.png)",
            "<https://exemplo.com/produtos/views.py>",
        ] {
            assert_eq!(ligar(igual), igual);
        }
        // Depois do bloco de código volta a ligar.
        assert_eq!(
            ligar("```\ntests.py\n```\n`tests.py`"),
            "```\ntests.py\n```\n[`tests.py`](vitral:arquivo/0/%2Fp%2Ftests.py)"
        );
        // Acento e emoji em volta não quebram.
        assert_eq!(ligar("ação ✓ `tests.py` é"), "ação ✓ [`tests.py`](vitral:arquivo/0/%2Fp%2Ftests.py) é");
    }

    #[test]
    fn resolve_pelo_disco_a_partir_da_pasta_do_chat() {
        let pasta = std::env::temp_dir().join(format!("vitral-links-{}", std::process::id()));
        std::fs::create_dir_all(pasta.join("src")).unwrap();
        std::fs::write(pasta.join("src/a.rs"), "fn a() {}").unwrap();
        assert_eq!(resolver(&pasta, "src/a.rs"), Some(pasta.join("src/a.rs").to_string_lossy().into_owned()));
        assert_eq!(resolver(&pasta, &pasta.join("src/a.rs").to_string_lossy()), Some(pasta.join("src/a.rs").to_string_lossy().into_owned()));
        assert_eq!(resolver(&pasta, "src"), None, "pasta não é arquivo");
        assert_eq!(resolver(&pasta, "src/nao.rs"), None);
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn link_permitido_pela_origem() {
        assert!(link_permitido("https://github.com/o/r/pull/1", false));
        assert!(link_permitido("HTTP://exemplo.com", false));
        assert!(!link_permitido("file:///etc/passwd", false));
        assert!(!link_permitido("file:///etc/passwd", true));
        assert!(!link_permitido("javascript:alert(1)", true));
        assert!(!link_permitido("/home/victor/.ssh/id_ed25519", false));
        assert!(!link_permitido("vitral:qualquer/coisa", true));
        assert!(link_permitido("vitral:arquivo/12/%2Fp%2Fa.rs", true));
        assert!(!link_permitido("vitral:arquivo/12/%2Fp%2Fa.rs", false));
        assert!(!link_permitido("vitral:arquivo/12/", true));
        assert!(!link_permitido("https://", false));
        assert_eq!(ler_link("vitral:arquivo/12/%2Fp%2Fa.rs"), Some(("/p/a.rs".into(), Some(12))));
        assert_eq!(ler_link("vitral:arquivo/0/a.rs"), Some(("a.rs".into(), None)));
    }
}
