//! Arquivos citados na resposta do assistente viram links: `produtos/views.py`, `src/motor.rs:120`
//! e `[o teste](tests/versao.rs#L40)` abrem no painel da direita, na linha; uma imagem abre no visor.
//!
//! Só vira link o que está no disco (a pasta do chat é a base dos caminhos relativos), então
//! "1.5", "e.g." e um nome que o assistente só imaginou ficam como texto. O link carrega o caminho
//! inteiro (`vitral:arquivo/<linha>/<caminho>`); quem decide se ele abre é a janela, que só aceita
//! arquivo dentro de uma pasta que o Vitral conhece, ou imagem.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// Maior caminho que vale a pena conferir no disco.
const MAX_CAMINHO: usize = 260;

/// Por quanto tempo vale a resposta do disco: a conversa redesenha a cada quadro, e o arquivo que
/// o assistente acabou de criar tem de virar link logo.
const VALIDADE: Duration = Duration::from_secs(3);

/// "src/x.rs:42", "src/x.rs:42:7", "src/x.rs:10-20" e "src/x.rs#L42" viram o caminho e a linha.
pub fn separar_linha(texto: &str) -> (&str, Option<u32>) {
    let numero = |s: &str| {
        let fim = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        s[..fim].parse::<u32>().ok().filter(|n| *n > 0)
    };
    if let Some((caminho, ancora)) = texto.split_once('#') {
        let linha = ancora.strip_prefix('L').and_then(numero);
        return (caminho, linha);
    }
    // O primeiro ":" depois da última barra: "C:\x" não aparece aqui, e "a/b.rs:10:2" tem dois.
    let barra = texto.rfind('/').map_or(0, |i| i + 1);
    match texto[barra..].find(':') {
        Some(i) => {
            let (caminho, resto) = texto.split_at(barra + i);
            match numero(&resto[1..]) {
                Some(n) => (caminho, Some(n)),
                None => (texto, None),
            }
        }
        None => (texto, None),
    }
}

/// O texto tem cara de caminho de arquivo: sem espaço, com uma extensão ou uma barra, e com letra
/// (o "0.6.0" e o "12/10" ficam de fora antes de ir ao disco).
pub fn parece_caminho(texto: &str) -> bool {
    let (caminho, _) = separar_linha(texto);
    if caminho.len() < 3 || caminho.len() > MAX_CAMINHO || caminho.contains("://") || caminho.starts_with('-') {
        return false;
    }
    let permitido = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '~' | '@' | '+' | '[' | ']' | '(' | ')');
    if !caminho.chars().all(permitido) {
        return false;
    }
    let nome = caminho.rsplit('/').next().unwrap_or(caminho);
    let com_extensao = nome.rsplit_once('.').is_some_and(|(antes, ext)| {
        !antes.is_empty() && (1..=10).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) && ext.chars().any(|c| c.is_ascii_alphabetic())
    });
    let com_barra = caminho.contains('/') && !nome.is_empty();
    (com_extensao || com_barra) && caminho.chars().any(char::is_alphabetic)
}

/// O alvo de um link `vitral:arquivo/…`: a linha (0 é nenhuma) e o caminho, com tudo o que não é
/// letra, dígito, ponto, hífen ou sublinhado em `%XX` (a barra inclusive: o link tem três partes).
pub fn link_de(caminho: &str, linha: Option<u32>) -> String {
    let mut alvo = format!("vitral:arquivo/{}/", linha.unwrap_or(0));
    for byte in caminho.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_') {
            alvo.push(byte as char);
        } else {
            alvo.push_str(&format!("%{byte:02X}"));
        }
    }
    alvo
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

/// Troca, no texto do assistente, os arquivos citados por links. `resolver` recebe o caminho como
/// foi escrito (sem a linha) e devolve o caminho inteiro, se o arquivo existe. Blocos de código,
/// títulos e código indentado ficam como estão; um link que já aponta para a web também.
pub fn ligar_arquivos(texto: &str, resolver: &mut dyn FnMut(&str) -> Option<String>) -> String {
    // Sem ponto nem barra não há o que ligar: a maioria das frases curtas sai por aqui.
    if !texto.contains(['.', '/']) {
        return texto.to_string();
    }
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

fn alvo_de(escrito: &str, resolver: &mut dyn FnMut(&str) -> Option<String>) -> Option<String> {
    if !parece_caminho(escrito) {
        return None;
    }
    let (caminho, linha) = separar_linha(escrito);
    Some(link_de(&resolver(caminho)?, linha))
}

fn ligar_linha(linha: &str, resolver: &mut dyn FnMut(&str) -> Option<String>, saida: &mut String) {
    let mut comum = String::new();
    let mut i = 0;
    while i < linha.len() {
        let resto = &linha[i..];
        // Código em linha: `src/x.rs:42` vira o link com o código dentro.
        if resto.starts_with('`') {
            let n = resto.len() - resto.trim_start_matches('`').len();
            let marca = &resto[..n];
            if let Some(fim) = resto[n..].find(marca) {
                ligar_palavras(&comum, resolver, saida);
                comum.clear();
                let dentro = &resto[n..n + fim];
                let inteiro = &resto[..n + fim + n];
                match alvo_de(dentro.trim(), resolver) {
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
            ligar_palavras(&comum, resolver, saida);
            comum.clear();
            let url = resto[meio + 2..meio + fim].trim();
            let escrito = url.split_whitespace().next().unwrap_or(url).trim_matches(['<', '>']);
            let imagem = i > 0 && linha[..i].ends_with('!');
            match (imagem, alvo_de(escrito, resolver)) {
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
            ligar_palavras(&comum, resolver, saida);
            comum.clear();
            saida.push_str(&resto[..fim + 1]);
            i += fim + 1;
            continue;
        }
        let c = resto.chars().next().unwrap_or_default();
        comum.push(c);
        i += c.len_utf8().max(1);
    }
    ligar_palavras(&comum, resolver, saida);
}

/// O texto corrido: só a palavra com barra e extensão ("src/ui/mod.rs:120") vira link. Um nome
/// solto ("config.json") fica como texto: no meio da frase é cedo demais para adivinhar.
fn ligar_palavras(trecho: &str, resolver: &mut dyn FnMut(&str) -> Option<String>, saida: &mut String) {
    if !trecho.contains('/') {
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
        let cara = caminho.contains('/') && caminho.rsplit('/').next().is_some_and(|n| n.contains('.')) && !limpo.contains("://");
        match cara.then(|| alvo_de(limpo, resolver)).flatten() {
            Some(alvo) => saida.push_str(&format!("{antes}[{limpo}]({alvo}){depois}")),
            None => saida.push_str(palavra),
        }
    }
}

thread_local! {
    static VISTOS: RefCell<HashMap<PathBuf, (Instant, bool)>> = RefCell::new(HashMap::new());
}

/// O caminho inteiro de um arquivo citado, se ele está no disco: `~/` é a pasta pessoal, o
/// absoluto vale como está e o relativo parte da pasta do chat. A resposta do disco vale por
/// alguns segundos (a conversa pergunta a cada quadro).
pub fn resolver(base: &Path, escrito: &str) -> Option<String> {
    let caminho = if let Some(resto) = escrito.strip_prefix("~/") {
        dirs::home_dir()?.join(resto)
    } else {
        let bruto = PathBuf::from(escrito);
        if bruto.is_absolute() { bruto } else { base.join(bruto) }
    };
    let existe = VISTOS.with(|vistos| {
        let mut vistos = vistos.borrow_mut();
        let agora = Instant::now();
        if let Some((quando, existe)) = vistos.get(&caminho)
            && agora.duration_since(*quando) < VALIDADE
        {
            return *existe;
        }
        if vistos.len() > 4000 {
            vistos.clear();
        }
        let existe = caminho.is_file();
        vistos.insert(caminho.clone(), (agora, existe));
        existe
    });
    existe.then(|| caminho.to_string_lossy().into_owned())
}

/// Um link `vitral:arquivo/…` clicado no texto do assistente (a janela abre o arquivo).
#[derive(Clone, PartialEq, Debug, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = vitral, no_json)]
pub struct AbrirLinkVitral(pub String);

/// O link pode abrir: `http(s)` sempre; `vitral:arquivo/…` só no texto do assistente (é ele que
/// `ligar_arquivos` gera); o resto (`file://`, `javascript:`, caminhos soltos) nunca.
pub fn link_permitido(url: &str, do_assistente: bool) -> bool {
    let url = url.trim();
    let comeca = |p: &str| url.len() > p.len() && url[..p.len()].eq_ignore_ascii_case(p);
    if comeca("http://") || comeca("https://") {
        return true;
    }
    do_assistente && comeca("vitral:") && ler_link(url).is_some()
}

/// O caminho inteiro e a linha de um `vitral:arquivo/<linha>/<caminho em %XX>`.
pub fn ler_link(url: &str) -> Option<(String, Option<u32>)> {
    let resto = url.trim().strip_prefix("vitral:arquivo/")?;
    let (linha, caminho) = resto.split_once('/')?;
    Some((caminho_do_link(caminho)?, linha.parse::<u32>().ok().filter(|n| *n > 0)))
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
    /// Um link `vitral:arquivo/…` do texto do assistente: a imagem abre no visor; o resto, no painel
    /// da direita, na linha. O que está na pasta de um chat abre com ela como raiz; o que está fora,
    /// com a pasta do próprio arquivo.
    pub(super) fn abrir_link_vitral(&mut self, url: &str, window: &mut gpui_kit::Window, cx: &mut gpui_kit::Context<Self>) {
        use super::componentes::{Tom, aviso};
        use gpui_kit::component::WindowExt as _;
        let Some(motor) = self.motor.clone() else { return };
        let Some((caminho, linha)) = ler_link(url) else { return };
        let Ok(inteiro) = dunce::canonicalize(&caminho) else {
            window.push_notification(aviso(Tom::Erro, format!("{caminho} não existe mais")), cx);
            return;
        };
        let chat = self.paineis.get(self.ativo).and_then(|p| p.chat.clone());
        if crate::claude::tipo_imagem(&inteiro).is_some()
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
    }
}

#[cfg(test)]
mod testes {
    use super::*;

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
