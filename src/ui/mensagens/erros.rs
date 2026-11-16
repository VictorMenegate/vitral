//! A leitura dos erros da conversa: o que deu errado, em português, o que fazer, e o texto do agente
//! como veio. Tudo puro (sem janela): o cartão de erro (`Janela::cartao_de_erro`) e a faixa acima
//! da caixa só desenham o que sai daqui (balão e cartão de erro, 16/11/2026).
//!
//! Os textos do próprio Vitral vêm de `motor.rs` e `motor/conversas.rs`, que não mudam: a leitura é
//! na tela e vale para as conversas já gravadas.

use gpui_kit::Hsla;

use crate::modelo::{Agente, Item};
use crate::motor::{falha_de_login, nome_curto_do_agente};
use crate::tema::{Cores, Lucide};

/// O começo do aviso de um turno que o agente terminou com erro (`motor.rs`, no `Fim`).
pub(super) const PREFIXO_DO_TURNO: &str = "O turno terminou com erro: ";
/// O começo do aviso de um processo que caiu sem terminar o turno (`motor.rs`, no `Saiu`).
const PREFIXO_DO_PROCESSO: &str = "Falhou: ";

const PASTA_SUMIU: &str = "A pasta deste chat não existe mais (";
const SEM_CODEX: &str = "O Codex não está instalado";
const ABRIR: &str = "Não consegui abrir o ";
const AGENTES: &str = "Os agentes em segundo plano não terminaram";
const ULTRACODE: &str = "O Claude Code não ligou o Ultracode";
const ULTRACODE_ANTIGO: &str = "Este Claude Code (";
const CICLO: &str = "Muitas mensagens seguidas entre";
const TODAS_NO_LIMITE: &str = "Todas as contas chegaram ao limite do plano";
const PRIMEIRA_VOLTA: &str = "a primeira volta ";

/// Os textos de erro que o Vitral escreve: a faixa não os lê como texto do agente.
const PREFIXOS_DO_VITRAL: [&str; 7] = [PASTA_SUMIU, SEM_CODEX, ABRIR, AGENTES, ULTRACODE, ULTRACODE_ANTIGO, CICLO];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum TipoDeErro {
    Limite,
    Login,
    Rede,
    Sobrecarga,
    Contexto,
    Processo,
    PastaSumiu,
    SemCodex,
    Abrir,
    Agentes,
    Ultracode,
    Ciclo,
    Vitral,
    Desconhecido,
}

#[derive(Debug, PartialEq)]
pub(super) struct ErroLido {
    pub tipo: TipoDeErro,
    pub agente: Agente,
    pub titulo: String,
    pub frase: Option<String>,
    /// O texto do agente como veio (inglês), sem o prefixo do Vitral. `None` nos textos do Vitral.
    pub cru: Option<String>,
    /// O cru começa fechado atrás de "Detalhes" (erro conhecido) ou aberto (desconhecido).
    pub recolhido: bool,
    /// "12:30": quando o limite volta, na hora local de 24 h.
    pub volta: Option<String>,
    /// A URL do texto que mostra o uso (a primeira com "usage" no caminho); nunca a de assinar.
    pub url_de_uso: Option<String>,
    /// O que o Copiar leva: o texto cru do agente, não a frase do Vitral.
    pub para_copiar: String,
}

impl ErroLido {
    fn do_vitral(tipo: TipoDeErro, agente: Agente, titulo: impl Into<String>, frase: Option<String>, texto: &str) -> Self {
        ErroLido {
            tipo,
            agente,
            titulo: titulo.into(),
            frase,
            cru: None,
            recolhido: false,
            volta: None,
            url_de_uso: None,
            para_copiar: texto.to_string(),
        }
    }
}

/// Lê um aviso de erro da conversa. `antes` é o aviso do Vitral que veio colado nele e diz o mesmo
/// problema (o login perdido sem outra conta, todas as contas no limite): os dois viram um cartão.
pub(super) fn ler_erro(texto: &str, agente_do_chat: Agente, antes: Option<&str>) -> ErroLido {
    let texto = texto.trim();
    if antes.is_none()
        && let Some(lido) = texto_do_vitral(texto, agente_do_chat)
    {
        return lido;
    }
    let (cru, do_processo) = match (texto.strip_prefix(PREFIXO_DO_TURNO), texto.strip_prefix(PREFIXO_DO_PROCESSO)) {
        (Some(cru), _) => (cru.trim(), false),
        (None, Some(cru)) => (cru.trim(), true),
        (None, None) => return outro_do_vitral(texto, agente_do_chat),
    };
    let agente = agente_do_erro(cru, agente_do_chat);
    if let Some(antes) = antes.map(str::trim) {
        return dobrado(antes, cru);
    }
    let nome = nome_curto_do_agente(agente);
    let mut lido = ErroLido {
        tipo: tipo_do_cru(cru),
        agente,
        titulo: String::new(),
        frase: None,
        cru: Some(cru.to_string()),
        recolhido: true,
        volta: None,
        url_de_uso: None,
        para_copiar: cru.to_string(),
    };
    match lido.tipo {
        TipoDeErro::Limite => {
            lido.titulo = match agente {
                Agente::Codex => "O Codex chegou ao limite de uso".into(),
                Agente::Claude => "O Claude chegou ao limite do plano".into(),
            };
            lido.volta = hora_de_volta(cru);
            lido.frase = Some(match &lido.volta {
                Some(hora) => format!("Volta às {hora}."),
                None => "Volta quando o limite renovar.".into(),
            });
            lido.url_de_uso = urls(cru).into_iter().find(|u| caminho_da_url(u).to_ascii_lowercase().contains("usage")).map(str::to_string);
        }
        TipoDeErro::Login => {
            let (titulo, frase) = match agente {
                Agente::Codex => ("O Codex pediu para entrar de novo", "Entre no Codex e tente outra vez."),
                Agente::Claude => ("O Claude Code perdeu o login desta conta", "Entre nela de novo e tente outra vez."),
            };
            (lido.titulo, lido.frase) = (titulo.into(), Some(frase.into()));
        }
        TipoDeErro::Rede => {
            lido.titulo = format!("Sem conexão com o {nome}");
            lido.frase = Some("A internet caiu ou o serviço não respondeu. Quando voltar, tente outra vez.".into());
        }
        TipoDeErro::Sobrecarga => {
            lido.titulo = if sobrecarregado(&cru.to_ascii_lowercase()) {
                format!("O serviço do {nome} está sobrecarregado")
            } else {
                format!("O serviço do {nome} respondeu com erro")
            };
            lido.frase = Some("Costuma passar em alguns minutos.".into());
        }
        TipoDeErro::Contexto => {
            lido.titulo = "A conversa passou do tamanho que o modelo aceita".into();
            lido.frase = Some(match agente {
                Agente::Claude => "Compacte a conversa e mande de novo.".into(),
                Agente::Codex => "Comece um chat novo ou mande uma mensagem mais curta.".into(),
            });
        }
        _ if do_processo => {
            // O processo caiu: sem nada no stderr, o motor escreve "o processo do X fechou sem
            // resposta"; com stderr, ele é o cru, aberto.
            lido.tipo = TipoDeErro::Processo;
            lido.recolhido = false;
            if cru.to_ascii_lowercase().contains("fechou sem resposta") {
                lido.titulo = format!("O {} fechou sem responder", agente.programa());
                lido.cru = None;
            } else {
                lido.titulo = format!("O {} fechou no meio do turno", agente.programa());
            }
        }
        _ => {
            lido.tipo = TipoDeErro::Desconhecido;
            lido.titulo = format!("O {nome} parou com um erro");
            lido.recolhido = false;
        }
    }
    lido
}

/// Os textos de erro que o próprio Vitral escreve, pelo começo exato.
fn texto_do_vitral(texto: &str, agente_do_chat: Agente) -> Option<ErroLido> {
    let lido = if let Some(resto) = texto.strip_prefix(PASTA_SUMIU) {
        let pasta = resto.rfind(')').map_or(resto, |fim| &resto[..fim]);
        let frase = format!("{pasta} não existe mais; a mensagem não foi enviada.");
        ErroLido::do_vitral(TipoDeErro::PastaSumiu, agente_do_chat, "A pasta deste chat sumiu", Some(frase), texto)
    } else if texto.starts_with(SEM_CODEX) {
        let frase = texto.contains("não foi enviada").then(|| "A mensagem não foi enviada.".to_string());
        ErroLido::do_vitral(TipoDeErro::SemCodex, Agente::Codex, SEM_CODEX, frase, texto)
    } else if texto.starts_with(ABRIR) {
        let (titulo, detalhe) = texto.split_once(": ").unwrap_or((texto, ""));
        let agente = agente_do_erro(titulo, agente_do_chat);
        let mut lido = ErroLido::do_vitral(TipoDeErro::Abrir, agente, titulo, None, texto);
        lido.cru = (!detalhe.trim().is_empty()).then(|| detalhe.trim().to_string());
        lido
    } else if texto.starts_with(AGENTES) {
        let frase = texto.split_once(": ").map(|(_, motivo)| maiuscula_inicial(motivo.trim()));
        ErroLido::do_vitral(TipoDeErro::Agentes, Agente::Claude, AGENTES, frase, texto)
    } else if texto.starts_with(ULTRACODE) || texto.starts_with(ULTRACODE_ANTIGO) {
        let titulo = if texto.starts_with(ULTRACODE) { "O Ultracode não ligou" } else { "Este Claude Code é antigo para o Ultracode" };
        let frase = texto.split_once(": ").map(|(_, resto)| maiuscula_inicial(resto.trim()));
        ErroLido::do_vitral(TipoDeErro::Ultracode, Agente::Claude, titulo, frase, texto)
    } else if texto.starts_with(CICLO) {
        ErroLido::do_vitral(TipoDeErro::Ciclo, agente_do_chat, "Conversa automática pausada", Some(texto.to_string()), texto)
    } else if let Some(nome) = conta_sem_login(texto) {
        ErroLido::do_vitral(
            TipoDeErro::Login,
            Agente::Claude,
            format!("A conta {nome} perdeu o login"),
            Some("Entre nela de novo e tente outra vez.".into()),
            texto,
        )
    } else {
        return None;
    };
    Some(lido)
}

/// Um erro do Vitral, em português, sem leitura própria: o título é o que vem antes do primeiro
/// ": " (até 80 caracteres) ou a primeira frase, e a frase é o resto.
fn outro_do_vitral(texto: &str, agente_do_chat: Agente) -> ErroLido {
    let (titulo, frase) = match partes_do_erro(texto) {
        (Some(titulo), resto) => (titulo, Some(resto)),
        (None, _) => match texto.split_once(". ") {
            Some((primeira, resto)) if !resto.trim().is_empty() => (primeira.to_string(), Some(resto.trim().to_string())),
            _ => (texto.trim_end_matches('.').to_string(), None),
        },
    };
    ErroLido::do_vitral(TipoDeErro::Vitral, agente_do_chat, titulo, frase, texto)
}

/// O título de um erro (o que vem antes do primeiro ": ") e o resto.
pub(super) fn partes_do_erro(texto: &str) -> (Option<String>, String) {
    match texto.split_once(": ") {
        Some((titulo, resto)) if !resto.trim().is_empty() && titulo.chars().count() <= 80 => {
            (Some(titulo.to_string()), resto.to_string())
        }
        _ => (None, texto.to_string()),
    }
}

/// O par de avisos do mesmo problema num cartão só: o do Vitral (`antes`) dá o título e a frase,
/// o do agente dá o cru.
fn dobrado(antes: &str, cru: &str) -> ErroLido {
    let mut lido = ErroLido {
        tipo: TipoDeErro::Login,
        agente: Agente::Claude,
        titulo: String::new(),
        frase: None,
        cru: Some(cru.to_string()),
        recolhido: true,
        volta: None,
        url_de_uso: None,
        para_copiar: format!("{antes}\n\n{cru}"),
    };
    if let Some(nome) = conta_sem_login(antes) {
        lido.titulo = format!("A conta {nome} perdeu o login");
        lido.frase = Some("Entre nela de novo e tente outra vez.".into());
    } else {
        lido.tipo = TipoDeErro::Limite;
        lido.titulo = TODAS_NO_LIMITE.into();
        let quando = antes.find(PRIMEIRA_VOLTA).map(|p| antes[p + PRIMEIRA_VOLTA.len()..].trim().trim_end_matches('.'));
        lido.frase = Some(match quando {
            Some(quando) if !quando.is_empty() => format!("A primeira volta {quando}."),
            _ => "Volta quando o limite renovar.".into(),
        });
        lido.url_de_uso = urls(cru).into_iter().find(|u| caminho_da_url(u).to_ascii_lowercase().contains("usage")).map(str::to_string);
    }
    lido
}

/// O nome da conta no aviso do Vitral "A conta {nome} perdeu o login no Claude Code. Entre...".
fn conta_sem_login(texto: &str) -> Option<&str> {
    let resto = texto.strip_prefix("A conta ")?;
    let fim = resto.find(" perdeu o login no Claude Code. Entre")?;
    Some(&resto[..fim])
}

/// Só as regras do texto do agente, nesta ordem: login, limite, contexto, rede, sobrecarga.
pub(super) fn tipo_do_cru(cru: &str) -> TipoDeErro {
    let t = cru.to_ascii_lowercase();
    let tem = |trechos: &[&str]| trechos.iter().any(|x| t.contains(x));
    // O limite de requisições ("rate limit") é sobrecarga passageira, nunca o limite do plano.
    let por_minuto = tem(&["rate limit", "rate_limit"]);
    if falha_de_login(cru) || tem(&["not logged in", "codex login", "please log in", "login required"]) {
        TipoDeErro::Login
    } else if !por_minuto && tem(&["usage limit", "hit your limit", "limit reached", "out of extra usage", "weekly limit", "quota"]) {
        TipoDeErro::Limite
    } else if tem(&[
        "prompt is too long",
        "context length",
        "context_length_exceeded",
        "context window",
        "maximum context",
        "too many tokens",
        "input is too long",
    ]) {
        TipoDeErro::Contexto
    } else if tem(&[
        "connection error",
        "connection refused",
        "fetch failed",
        "enotfound",
        "econnrefused",
        "econnreset",
        "etimedout",
        "eai_again",
        "getaddrinfo",
        "socket hang up",
        "stream disconnected",
        "network error",
        "unable to connect",
        "could not resolve host",
    ]) {
        TipoDeErro::Rede
    } else if sobrecarregado(&t)
        || tem(&["internal server error", "service unavailable", "bad gateway", "api error: 5", " 500", " 502", " 503"])
    {
        TipoDeErro::Sobrecarga
    } else {
        TipoDeErro::Desconhecido
    }
}

/// O serviço disse que está cheio (o título "está sobrecarregado"); o resto da sobrecarga é o
/// "respondeu com erro". `t` já em minúsculas.
fn sobrecarregado(t: &str) -> bool {
    ["overloaded", "529", "rate limit", "rate_limit", "429", "too many requests"].iter().any(|x| t.contains(x))
}

/// De quem é o erro: na dupla (o Claude e o Codex no mesmo chat) o texto diz melhor que o chat.
pub(super) fn agente_do_erro(cru: &str, agente_do_chat: Agente) -> Agente {
    let t = cru.to_ascii_lowercase();
    if ["chatgpt.com", "openai", "codex"].iter().any(|x| t.contains(x)) {
        Agente::Codex
    } else if ["claude", "anthropic"].iter().any(|x| t.contains(x)) {
        Agente::Claude
    } else {
        agente_do_chat
    }
}

/// Quando o limite volta, sempre "HH:MM" em 24 h: depois de "try again at " ou "resets " (com
/// "at " opcional), ou os segundos Unix depois de "|" no fim (formato antigo do Claude Code). Uma
/// data depois de "resets" ("Nov 18") dá `None`: a frase fica a genérica e o cru mostra o resto.
pub(super) fn hora_de_volta(cru: &str) -> Option<String> {
    let baixo = cru.to_ascii_lowercase();
    for gatilho in ["try again at ", "resets "] {
        if let Some(p) = baixo.find(gatilho) {
            let resto = &baixo[p + gatilho.len()..];
            return hora_no_comeco(resto.strip_prefix("at ").unwrap_or(resto));
        }
    }
    let (_, fim) = cru.trim_end().rsplit_once('|')?;
    let segundos: u64 = fim.parse().ok().filter(|_| fim.len() >= 9 && fim.bytes().all(|b| b.is_ascii_digit()))?;
    Some(crate::modelo::hora_local(segundos))
}

/// "3pm", "3:30 pm", "12:05 AM" ou "15:00" no começo de `t` (em minúsculas), em 24 h. Sem am/pm,
/// só com os minutos ("15:00"): um número solto não é hora.
fn hora_no_comeco(t: &str) -> Option<String> {
    let digitos = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
    let n = digitos(t);
    if n == 0 || n > 2 {
        return None;
    }
    let hora: u32 = t[..n].parse().ok()?;
    let mut resto = &t[n..];
    let mut minutos = None;
    if let Some(depois) = resto.strip_prefix(':') {
        if digitos(depois) != 2 {
            return None;
        }
        minutos = Some(depois[..2].parse::<u32>().ok()?);
        resto = &depois[2..];
    }
    let resto = resto.strip_prefix(' ').unwrap_or(resto);
    let meridiano = ["am", "pm"]
        .into_iter()
        .find(|m| resto.starts_with(m) && !resto[2..].starts_with(|c: char| c.is_ascii_alphabetic()));
    let hora = match meridiano {
        Some(m) if (1..=12).contains(&hora) => hora % 12 + if m == "pm" { 12 } else { 0 },
        Some(_) => return None,
        None if minutos.is_some() && hora < 24 => hora,
        None => return None,
    };
    let minutos = minutos.unwrap_or(0);
    (minutos < 60).then(|| format!("{hora:02}:{minutos:02}"))
}

/// As URLs de um texto, com a pontuação do fim devolvida ao texto: começam em "http://" ou
/// "https://" e vão até o primeiro espaço; saem do fim `. , ; : ! ? ' "` e um ")" quando a URL não
/// tem "(".
fn urls(texto: &str) -> Vec<&str> {
    let mut achadas = Vec::new();
    let mut resto = texto;
    while let Some((antes, url)) = proxima_url(resto) {
        achadas.push(url);
        resto = &resto[antes + url.len()..];
    }
    achadas
}

/// A próxima URL de `texto`: onde ela começa e ela, já sem a pontuação do fim.
fn proxima_url(texto: &str) -> Option<(usize, &str)> {
    let comeco = [texto.find("http://"), texto.find("https://")].into_iter().flatten().min()?;
    let depois = &texto[comeco..];
    let mut url = &depois[..depois.find(char::is_whitespace).unwrap_or(depois.len())];
    let mut sem_parentese = false;
    while let Some(c) = url.chars().last() {
        if ".,;:!?'\"".contains(c) || (c == ')' && !sem_parentese && !url.contains('(')) {
            sem_parentese |= c == ')';
            url = &url[..url.len() - c.len_utf8()];
        } else {
            break;
        }
    }
    Some((comeco, url))
}

/// O domínio da URL, sem "www.": o que vem depois de "://" até o primeiro "/", "?", "#" ou ":".
fn dominio(url: &str) -> &str {
    let depois = url.split_once("://").map_or(url, |(_, d)| d);
    let fim = depois.find(['/', '?', '#', ':']).unwrap_or(depois.len());
    let dominio = &depois[..fim];
    dominio.strip_prefix("www.").unwrap_or(dominio)
}

/// O que vem depois do domínio ("/settings/usage").
fn caminho_da_url(url: &str) -> &str {
    let depois = url.split_once("://").map_or(url, |(_, d)| d);
    depois.find(['/', '?', '#']).map_or("", |p| &depois[p..])
}

/// O cru vira markdown seguro: as marcas do markdown escapadas, as URLs em link com o domínio de
/// rótulo (o `line_wrapper` do GPUI quebra em todo "/", e "chatgpt.com/settings/usage" quebraria no
/// meio) e as linhas do cru continuam linhas.
pub(super) fn erro_em_markdown(cru: &str) -> String {
    let mut saida = String::with_capacity(cru.len() + 16);
    let mut inicio = true;
    let mut resto = cru;
    while let Some((antes, url)) = proxima_url(resto) {
        escapar(&resto[..antes], &mut saida, &mut inicio);
        let rotulo = dominio(url);
        if rotulo.is_empty() {
            escapar(url, &mut saida, &mut inicio);
        } else {
            saida.push_str(&format!("[{rotulo}]({url})"));
            inicio = false;
        }
        resto = &resto[antes + url.len()..];
    }
    escapar(resto, &mut saida, &mut inicio);
    super::com_quebras_duras(&saida)
}

/// Escapa com "\\" as marcas do markdown, e o "-", o "+" e o ponto de "1." quando abrem uma linha
/// (senão o markdown faz lista). `inicio` diz se o trecho começa no começo de uma linha.
fn escapar(texto: &str, saida: &mut String, inicio: &mut bool) {
    let mut numero = false;
    for c in texto.chars() {
        if c == '\n' {
            saida.push(c);
            (*inicio, numero) = (true, false);
            continue;
        }
        if *inicio && (c == ' ' || c == '\t') {
            saida.push(c);
            continue;
        }
        if *inicio && c.is_ascii_digit() {
            saida.push(c);
            numero = true;
            continue;
        }
        let abre_lista = *inicio && (numero && c == '.' || !numero && (c == '-' || c == '+'));
        if abre_lista || "\\`*_[]<>#|~".contains(c) {
            saida.push('\\');
        }
        saida.push(c);
        (*inicio, numero) = (false, false);
    }
}

/// O ícone e a cor de cada erro: vermelho quando o turno não pôde seguir, azul-claro quando precisa
/// de você, âmbar quando seguiu com ressalva e neutro (`None`) quando ninguém errou e é só esperar.
pub(super) fn aparencia(tipo: TipoDeErro) -> (Lucide, Option<Hsla>) {
    match tipo {
        TipoDeErro::Limite => (Lucide::Gauge, Some(Cores::vermelho())),
        TipoDeErro::Login => (Lucide::KeyRound, Some(Cores::espera())),
        TipoDeErro::Rede => (Lucide::WifiOff, None),
        TipoDeErro::Sobrecarga => (Lucide::Clock, None),
        TipoDeErro::Contexto => (Lucide::Minimize, Some(Cores::vermelho())),
        TipoDeErro::Processo => (Lucide::Unplug, Some(Cores::vermelho())),
        TipoDeErro::PastaSumiu => (Lucide::FolderX, Some(Cores::vermelho())),
        TipoDeErro::SemCodex => (Lucide::Download, Some(Cores::vermelho())),
        TipoDeErro::Agentes => (Lucide::Workflow, Some(Cores::vermelho())),
        TipoDeErro::Ultracode => (Lucide::Zap, Some(Cores::ambar())),
        TipoDeErro::Ciclo => (Lucide::MessagesSquare, Some(Cores::espera())),
        TipoDeErro::Abrir | TipoDeErro::Vitral | TipoDeErro::Desconhecido => (Lucide::CircleX, Some(Cores::vermelho())),
    }
}

/// O erro do índice ainda é o último assunto da conversa: depois dele só há avisos, marcas de vez
/// e cartões (você ainda não mandou nada e nenhum turno respondeu). As ações do cartão só valem
/// nele. Um erro novo depois (a nova tentativa que falhou de novo) passa a ser o vivo: dois cartões
/// seguidos com os mesmos botões seriam o mesmo botão duas vezes.
pub(super) fn erro_vivo(itens: &[Item], indice: usize) -> bool {
    itens.get(indice + 1..).is_some_and(|depois| {
        depois.iter().all(|item| match item {
            Item::Aviso { erro, .. } => !*erro,
            Item::Vez { .. } | Item::Cartao { .. } | Item::Desconhecido(_) => true,
            _ => false,
        })
    })
}

/// O aviso do Vitral que vem colado no erro do turno e diz o mesmo problema: o login perdido sem
/// outra conta e todas as contas no limite.
pub(super) fn precede_o_erro(texto: &str) -> bool {
    conta_sem_login(texto).is_some() || texto.starts_with(&format!("{TODAS_NO_LIMITE}; {PRIMEIRA_VOLTA}"))
}

/// O ícone, a cor e o rótulo curto da faixa acima da caixa, do texto da execução (o cru, sem
/// prefixo).
pub(super) fn na_faixa(texto_da_execucao: &str, agente_do_chat: Agente) -> (Lucide, Option<Hsla>, String) {
    let texto = texto_da_execucao.trim();
    let agente = agente_do_erro(texto, agente_do_chat);
    let tipo = if PREFIXOS_DO_VITRAL.iter().any(|p| texto.starts_with(p)) { TipoDeErro::Desconhecido } else { tipo_do_cru(texto) };
    match tipo {
        TipoDeErro::Limite => {
            let base = match agente {
                Agente::Codex => "Limite de uso do Codex",
                Agente::Claude => "Limite do plano",
            };
            let rotulo = match hora_de_volta(texto) {
                Some(hora) => format!("{base} · volta {hora}"),
                None => base.to_string(),
            };
            (Lucide::Gauge, Some(Cores::vermelho()), rotulo)
        }
        TipoDeErro::Login => (Lucide::KeyRound, Some(Cores::espera()), format!("Sem login no {}", agente.programa())),
        TipoDeErro::Rede => (Lucide::WifiOff, None, "Sem conexão".into()),
        TipoDeErro::Sobrecarga => (Lucide::Clock, None, "Serviço sobrecarregado".into()),
        TipoDeErro::Contexto => (Lucide::Minimize, Some(Cores::vermelho()), "Conversa longa demais para o modelo".into()),
        _ => (Lucide::CircleX, Some(Cores::vermelho()), "O último turno falhou".into()),
    }
}

/// A primeira letra em maiúscula ("o processo caiu." vira "O processo caiu.").
fn maiuscula_inicial(texto: &str) -> String {
    let mut letras = texto.chars();
    match letras.next() {
        Some(primeira) => primeira.to_uppercase().chain(letras).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    const CODEX_DO_DONO: &str = "You've hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit \
        https://chatgpt.com/settings/usage to purchase more credits or try again at 12:30 PM.";

    fn do_turno(cru: &str) -> String {
        format!("{PREFIXO_DO_TURNO}{cru}")
    }

    fn aviso(texto: &str, erro: bool) -> Item {
        Item::Aviso { texto: texto.into(), erro, em: 0 }
    }

    fn usuario() -> Item {
        Item::Usuario { texto: "oi".into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0 }
    }

    #[test]
    fn limite_do_codex_do_dono() {
        let lido = ler_erro(&do_turno(CODEX_DO_DONO), Agente::Codex, None);
        assert_eq!(lido.tipo, TipoDeErro::Limite);
        assert_eq!(lido.agente, Agente::Codex);
        assert_eq!(lido.titulo, "O Codex chegou ao limite de uso");
        assert_eq!(lido.frase.as_deref(), Some("Volta às 12:30."));
        assert_eq!(lido.url_de_uso.as_deref(), Some("https://chatgpt.com/settings/usage"));
        assert!(lido.recolhido);
        assert_eq!(lido.para_copiar, CODEX_DO_DONO);
        assert_eq!(lido.cru.as_deref(), Some(CODEX_DO_DONO));
    }

    #[test]
    fn o_texto_diz_o_agente_na_dupla() {
        let lido = ler_erro(&do_turno(CODEX_DO_DONO), Agente::Claude, None);
        assert_eq!(lido.agente, Agente::Codex);
        assert_eq!(agente_do_erro("getaddrinfo ENOTFOUND api.anthropic.com", Agente::Codex), Agente::Claude);
        assert_eq!(agente_do_erro("algo", Agente::Codex), Agente::Codex);
    }

    #[test]
    fn hora_de_volta_em_24_horas() {
        assert_eq!(hora_de_volta("try again at 12:30 PM").as_deref(), Some("12:30"));
        assert_eq!(hora_de_volta("try again at 12:05 AM").as_deref(), Some("00:05"));
        assert_eq!(hora_de_volta("You've hit your limit · resets 3pm").as_deref(), Some("15:00"));
        assert_eq!(hora_de_volta("resets 3:30pm (America/Sao_Paulo)").as_deref(), Some("15:30"));
        assert_eq!(hora_de_volta("resets at 15:00").as_deref(), Some("15:00"));
        assert_eq!(
            hora_de_volta("Claude AI usage limit reached|1789000000"),
            Some(crate::modelo::hora_local(1789000000))
        );
        assert_eq!(hora_de_volta("resets Nov 18, 9am"), None);
        assert_eq!(hora_de_volta("resets 3"), None);
        assert_eq!(hora_de_volta("limit reached|123"), None);
    }

    #[test]
    fn login_e_limite_de_requisicoes() {
        assert_eq!(tipo_do_cru("OAuth token revoked"), TipoDeErro::Login);
        assert_eq!(tipo_do_cru("Failed to authenticate. API Error: 401"), TipoDeErro::Login);
        assert_eq!(tipo_do_cru("rate limit exceeded"), TipoDeErro::Sobrecarga);
        assert_eq!(tipo_do_cru("Rate limit reached for requests"), TipoDeErro::Sobrecarga);
        assert_eq!(tipo_do_cru("Not logged in. Run codex login"), TipoDeErro::Login);
        let lido = ler_erro(&do_turno("Not logged in"), Agente::Codex, None);
        assert_eq!(lido.titulo, "O Codex pediu para entrar de novo");
    }

    #[test]
    fn rede() {
        assert_eq!(tipo_do_cru("API Error: Connection error."), TipoDeErro::Rede);
        assert_eq!(tipo_do_cru("getaddrinfo ENOTFOUND api.anthropic.com"), TipoDeErro::Rede);
        let lido = ler_erro(&do_turno("API Error: Connection error."), Agente::Claude, None);
        assert_eq!(lido.titulo, "Sem conexão com o Claude");
        assert!(lido.recolhido);
        // Ninguém errou: o cartão fica neutro.
        assert!(aparencia(TipoDeErro::Rede).1.is_none());
        assert!(aparencia(TipoDeErro::Limite).1.is_some());
    }

    #[test]
    fn sobrecarga() {
        let lido = ler_erro(&do_turno("API Error: 529 {\"type\":\"overloaded_error\"}"), Agente::Claude, None);
        assert_eq!(lido.tipo, TipoDeErro::Sobrecarga);
        assert!(lido.titulo.contains("está sobrecarregado"), "{}", lido.titulo);
        let lido = ler_erro(&do_turno("API Error: 500 Internal server error"), Agente::Claude, None);
        assert_eq!(lido.tipo, TipoDeErro::Sobrecarga);
        assert!(lido.titulo.contains("respondeu com erro"), "{}", lido.titulo);
    }

    #[test]
    fn contexto() {
        assert_eq!(tipo_do_cru("Prompt is too long"), TipoDeErro::Contexto);
        let lido = ler_erro(&do_turno("Prompt is too long"), Agente::Claude, None);
        assert_eq!(lido.frase.as_deref(), Some("Compacte a conversa e mande de novo."));
    }

    #[test]
    fn processo_que_caiu() {
        let lido = ler_erro("Falhou: o processo do Claude Code fechou sem resposta", Agente::Claude, None);
        assert_eq!(lido.tipo, TipoDeErro::Processo);
        assert_eq!(lido.cru, None);
        assert_eq!(lido.titulo, "O Claude Code fechou sem responder");
        let lido = ler_erro("Falhou: Error: connect ECONNREFUSED", Agente::Claude, None);
        assert_eq!(lido.tipo, TipoDeErro::Rede);
        let lido = ler_erro("Falhou: panic qualquer", Agente::Claude, None);
        assert_eq!(lido.tipo, TipoDeErro::Processo);
        assert_eq!(lido.cru.as_deref(), Some("panic qualquer"));
        assert!(!lido.recolhido);
    }

    #[test]
    fn textos_do_motor() {
        let lido = ler_erro("A pasta deste chat não existe mais (~/x); a mensagem não foi enviada.", Agente::Claude, None);
        assert_eq!(lido.tipo, TipoDeErro::PastaSumiu);
        assert_eq!(lido.frase.as_deref(), Some("~/x não existe mais; a mensagem não foi enviada."));
        assert_eq!(lido.cru, None);

        let lido = ler_erro(
            "O Codex não está instalado neste computador (ou não está no PATH); a mensagem não foi enviada.",
            Agente::Codex,
            None,
        );
        assert_eq!(lido.tipo, TipoDeErro::SemCodex);
        assert_eq!(lido.frase.as_deref(), Some("A mensagem não foi enviada."));
        let lido = ler_erro("O Codex não está instalado neste computador (ou não está no PATH).", Agente::Claude, None);
        assert_eq!(lido.frase, None);

        let lido = ler_erro("Não consegui abrir o Claude Code: No such file", Agente::Claude, None);
        assert_eq!(lido.tipo, TipoDeErro::Abrir);
        assert_eq!(lido.titulo, "Não consegui abrir o Claude Code");
        assert_eq!(lido.cru.as_deref(), Some("No such file"));
        assert!(!lido.recolhido);

        let lido = ler_erro("Os agentes em segundo plano não terminaram: o processo caiu.", Agente::Claude, None);
        assert_eq!(lido.tipo, TipoDeErro::Agentes);
        assert_eq!(lido.frase.as_deref(), Some("O processo caiu."));

        let lido = ler_erro("O Claude Code não ligou o Ultracode: o modelo x não tem o esforço Extra.", Agente::Claude, None);
        assert_eq!(lido.tipo, TipoDeErro::Ultracode);
        assert_eq!(lido.titulo, "O Ultracode não ligou");
        assert_eq!(lido.frase.as_deref(), Some("O modelo x não tem o esforço Extra."));

        let lido = ler_erro("Muitas mensagens seguidas entre a e b; esperando você.", Agente::Claude, None);
        assert_eq!(lido.tipo, TipoDeErro::Ciclo);
        assert_eq!(lido.frase.as_deref(), Some("Muitas mensagens seguidas entre a e b; esperando você."));

        let lido = ler_erro(
            "A conta Pessoal perdeu o login no Claude Code. Entre nela de novo em Contas do Claude (menu do logo) e use Tentar de novo.",
            Agente::Claude,
            None,
        );
        assert_eq!(lido.tipo, TipoDeErro::Login);
        assert_eq!(lido.titulo, "A conta Pessoal perdeu o login");
        assert_eq!(lido.cru, None);

        let lido = ler_erro("O servidor interno do Vitral não abriu; não dá para rodar conversas.", Agente::Claude, None);
        assert_eq!(lido.tipo, TipoDeErro::Vitral);
        assert_eq!(lido.titulo, "O servidor interno do Vitral não abriu; não dá para rodar conversas");
    }

    #[test]
    fn desconhecido_fica_aberto() {
        let lido = ler_erro(&do_turno("algo estranho"), Agente::Claude, None);
        assert_eq!(lido.tipo, TipoDeErro::Desconhecido);
        assert_eq!(lido.titulo, "O Claude parou com um erro");
        assert!(!lido.recolhido);
        assert_eq!(lido.cru.as_deref(), Some("algo estranho"));
    }

    #[test]
    fn dois_avisos_viram_um_cartao() {
        let antes = "A conta Pessoal perdeu o login no Claude Code. Entre nela de novo em Contas do Claude (menu do logo) e use Tentar de novo.";
        assert!(precede_o_erro(antes));
        let lido = ler_erro(&do_turno("Failed to authenticate. API Error: 401"), Agente::Claude, Some(antes));
        assert_eq!(lido.tipo, TipoDeErro::Login);
        assert_eq!(lido.titulo, "A conta Pessoal perdeu o login");
        assert_eq!(lido.para_copiar, format!("{antes}\n\nFailed to authenticate. API Error: 401"));
        assert!(lido.recolhido);

        let antes = "Todas as contas chegaram ao limite do plano; a primeira volta às 15:00.";
        assert!(precede_o_erro(antes));
        let lido = ler_erro(&do_turno("Claude AI usage limit reached|1789000000"), Agente::Claude, Some(antes));
        assert_eq!(lido.tipo, TipoDeErro::Limite);
        assert_eq!(lido.titulo, "Todas as contas chegaram ao limite do plano");
        assert_eq!(lido.frase.as_deref(), Some("A primeira volta às 15:00."));
        assert!(!precede_o_erro("A conta Pessoal chegou ao limite do plano; continuando na conta Trabalho."));
    }

    #[test]
    fn erro_em_markdown_com_urls_curtas() {
        assert_eq!(
            erro_em_markdown(CODEX_DO_DONO),
            "You've hit your usage limit. Upgrade to Pro ([chatgpt.com](https://chatgpt.com/explore/pro)), visit \
             [chatgpt.com](https://chatgpt.com/settings/usage) to purchase more credits or try again at 12:30 PM."
        );
        assert_eq!(erro_em_markdown("a_b *c*"), "a\\_b \\*c\\*");
        assert_eq!(erro_em_markdown("(https://x.com/a)."), "([x.com](https://x.com/a)).");
        assert_eq!(erro_em_markdown("https://www.github.com"), "[github.com](https://www.github.com)");
        assert_eq!(erro_em_markdown("- a\n1. b\n+ c # d"), "\\- a  \n1\\. b  \n\\+ c \\# d");
        assert_eq!(erro_em_markdown("ver https://x.com/a(b) ok"), "ver [x.com](https://x.com/a(b)) ok");
    }

    #[test]
    fn so_o_ultimo_erro_e_vivo() {
        let erro = aviso(&do_turno("x"), true);
        assert!(erro_vivo(&[usuario(), erro.clone()], 1));
        assert!(!erro_vivo(&[erro.clone(), usuario()], 0));
        assert!(!erro_vivo(&[erro.clone(), aviso("Tentando de novo.", false), Item::Assistente { texto: "ok".into(), em: 0 }], 0));
        assert!(erro_vivo(&[erro.clone(), aviso("Tentando de novo.", false)], 0));
        assert!(!erro_vivo(&[erro.clone(), aviso("Tentando de novo.", false), erro.clone()], 0));
        assert!(erro_vivo(&[erro.clone(), aviso("Tentando de novo.", false), erro], 2));
    }

    #[test]
    fn faixa_curta() {
        let (icone, cor, rotulo) = na_faixa(CODEX_DO_DONO, Agente::Codex);
        assert_eq!(rotulo, "Limite de uso do Codex · volta 12:30");
        assert!(matches!(icone, Lucide::Gauge));
        assert!(cor.is_some());
        assert_eq!(na_faixa("algo", Agente::Claude).2, "O último turno falhou");
        assert_eq!(na_faixa("You've hit your limit · resets 3pm", Agente::Claude).2, "Limite do plano · volta 15:00");
        assert_eq!(na_faixa("OAuth token revoked", Agente::Claude).2, "Sem login no Claude Code");
        assert_eq!(na_faixa("Os agentes em segundo plano não terminaram: o processo caiu.", Agente::Claude).2, "O último turno falhou");
    }
}
