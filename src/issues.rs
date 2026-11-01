//! O que é comum às issues do Linear e do GitHub: os tipos que o motor e a interface usam, e as
//! funções puras de nomes (ramo, título e corpo do PR), referências e confiança dos autores.
// Provisório: o motor passa a usar estes módulos no G4.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FonteIssue {
    #[default]
    Linear,
    Github,
}

/// A issue que originou o chat ou que foi ligada a ele (`Tarefa.issue`). Só ids e nomes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IssueLigada {
    pub fonte: FonteIssue,
    /// O uuid no Linear; o número no GitHub.
    pub id: String,
    /// "ENG-123" ou "#45".
    pub chave: String,
    pub titulo: String,
    pub url: String,
    /// "dono/nome", no GitHub.
    pub repositorio: Option<String>,
    pub time_id: Option<String>,
    /// O estado para onde o Vitral a moveu ao iniciar (para não mover em dobro).
    pub movida_para: Option<String>,
}

/// O tipo do estado de uma issue. No Linear vem do `type` do estado do time; no GitHub, do
/// `state` e do `stateReason`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TipoEstado {
    Triagem,
    #[default]
    Backlog,
    AFazer,
    EmAndamento,
    Concluida,
    Cancelada,
    Duplicada,
    Aberta,
    Fechada,
}

impl TipoEstado {
    pub fn do_linear(tipo: &str) -> Self {
        match tipo {
            "triage" => Self::Triagem,
            "unstarted" => Self::AFazer,
            "started" => Self::EmAndamento,
            "completed" => Self::Concluida,
            "canceled" => Self::Cancelada,
            "duplicate" => Self::Duplicada,
            _ => Self::Backlog,
        }
    }

    /// Já começou ou terminou: o Vitral não a move ao iniciar um agente.
    pub fn iniciada_ou_encerrada(self) -> bool {
        matches!(self, Self::EmAndamento | Self::Concluida | Self::Cancelada | Self::Duplicada | Self::Fechada)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EstadoIssue {
    pub id: String,
    pub nome: String,
    pub tipo: TipoEstado,
    /// A cor que o usuário escolheu no Linear (só para o ponto; o texto não usa).
    pub cor: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pessoa {
    pub id: String,
    pub nome: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Etiqueta {
    pub nome: String,
    pub cor: Option<String>,
}

/// Uma linha da lista de issues.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IssueResumo {
    pub fonte: FonteIssue,
    pub id: String,
    pub chave: String,
    pub titulo: String,
    pub estado: EstadoIssue,
    pub responsavel: Option<Pessoa>,
    pub etiquetas: Vec<Etiqueta>,
    /// Do Linear: 0 sem, 1 urgente, 2 alta, 3 média, 4 baixa.
    pub prioridade: Option<u8>,
    pub atualizada: String,
    pub criada: String,
    pub url: String,
    /// O `branchName` do Linear.
    pub ramo_sugerido: Option<String>,
    pub autor: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComentarioIssue {
    pub autor: String,
    pub corpo: String,
    pub criado: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct IssueDetalhe {
    pub resumo: IssueResumo,
    pub descricao: String,
    /// Do mais antigo para o mais novo.
    pub comentarios: Vec<ComentarioIssue>,
    pub ciclo: Option<String>,
    pub projeto: Option<String>,
    pub time: Option<String>,
    pub repositorio: Option<String>,
}

/// O recorte da lista de issues do popover. No Linear: Minhas, Do time e Ciclo atual; no GitHub,
/// Minhas e Todas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Recorte {
    #[default]
    Minhas,
    DoTime,
    CicloAtual,
    Todas,
}

/// O filtro da lista de issues (lembrado por projeto em `interface.json`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct FiltroIssues {
    pub recorte: Recorte,
    /// As fechadas nos últimos 30 dias, no lugar das abertas.
    pub fechadas: bool,
    /// A busca no servidor (Enter com 3 ou mais caracteres); o filtro local fica na interface.
    pub termo: Option<String>,
}

/// Uma issue ou PR citado num texto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Referencia {
    Linear { chave: String },
    /// `pr` só quando a URL é de PR (`/pull/N`); "#45" pode ser qualquer um dos dois.
    Github { repositorio: Option<String>, numero: u64, pr: bool },
}

/// Quanto o Vitral confia no autor de um comentário ou revisão de PR.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confianca {
    /// Tem acesso ao repositório (ou é um bot de revisão conhecido): pode disparar ajuste.
    Confiavel,
    /// Sem acesso ao repositório: só vira item em "Esperando você".
    Terceiro,
    /// Outros bots (deploy, CI): só aparecem na linha do tempo.
    Informativo,
}

// ---------- referências ----------

fn nome_valido(parte: &str) -> bool {
    !parte.is_empty()
        && parte != "."
        && parte != ".."
        && parte.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// "dono/nome" com caracteres que o GitHub aceita (e que podem ir num caminho da API).
pub fn repositorio_valido(repositorio: &str) -> bool {
    repositorio.split_once('/').is_some_and(|(dono, nome)| nome_valido(dono) && nome_valido(nome) && !dono.starts_with('-'))
}

/// `dono` e `nome` do repositório no GitHub a partir da URL de um remote:
/// `https://github.com/O/R(.git)`, `git@github.com:O/R(.git)` e `ssh://git@github.com/O/R(.git)`.
/// Outro host, ou outro formato, dá `None`.
pub fn parse_remoto(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let caminho = if let Some(resto) = url.strip_prefix("git@") {
        let (host, caminho) = resto.split_once(':')?;
        host.eq_ignore_ascii_case("github.com").then_some(caminho)?
    } else {
        let resto = ["https://", "http://", "ssh://"].iter().find_map(|p| url.strip_prefix(p))?;
        let (autoridade, caminho) = resto.split_once('/')?;
        // Sem usuário e senha na frente (https://x@github.com) e sem porta.
        let host = autoridade.rsplit_once('@').map_or(autoridade, |(_, h)| h);
        host.eq_ignore_ascii_case("github.com").then_some(caminho)?
    };
    let caminho = caminho.trim_end_matches('/');
    let caminho = caminho.strip_suffix(".git").unwrap_or(caminho);
    let (dono, nome) = caminho.split_once('/')?;
    (nome_valido(dono) && nome_valido(nome)).then(|| (dono.to_string(), nome.to_string()))
}

/// Uma chave do Linear: letras maiúsculas e números (começando por letra), hífen e o número.
fn chave_linear(texto: &str) -> Option<String> {
    let (time, numero) = texto.split_once('-')?;
    let time_ok = time.len() <= 10
        && time.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && time.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    let numero_ok = !numero.is_empty() && numero.chars().all(|c| c.is_ascii_digit()) && !numero.starts_with('0');
    (time_ok && numero_ok).then(|| texto.to_string())
}

fn numero(texto: &str) -> Option<u64> {
    (!texto.is_empty() && texto.chars().all(|c| c.is_ascii_digit())).then(|| texto.parse().ok())?.filter(|n| *n > 0)
}

/// "ENG-123", "#45", "o/r#45", a URL de uma issue ou PR no GitHub ou a de uma issue no Linear.
pub fn parse_referencia(texto: &str) -> Option<Referencia> {
    let texto = texto.trim();
    if let Some(resto) = texto.strip_prefix("https://github.com/") {
        let resto = resto.split(['#', '?']).next()?;
        let partes: Vec<&str> = resto.trim_end_matches('/').split('/').collect();
        let [dono, nome, tipo, n, ..] = partes[..] else { return None };
        let pr = match tipo {
            "pull" => true,
            "issues" => false,
            _ => return None,
        };
        (nome_valido(dono) && nome_valido(nome)).then_some(())?;
        return Some(Referencia::Github { repositorio: Some(format!("{dono}/{nome}")), numero: numero(n)?, pr });
    }
    if let Some(resto) = texto.strip_prefix("https://linear.app/") {
        let partes: Vec<&str> = resto.split(['#', '?']).next()?.split('/').collect();
        let [_, "issue", chave, ..] = partes[..] else { return None };
        return chave_linear(&chave.to_ascii_uppercase()).map(|chave| Referencia::Linear { chave });
    }
    if let Some((repositorio, n)) = texto.split_once('#') {
        if repositorio.is_empty() {
            return Some(Referencia::Github { repositorio: None, numero: numero(n)?, pr: false });
        }
        let (dono, nome) = repositorio.split_once('/')?;
        (nome_valido(dono) && nome_valido(nome)).then_some(())?;
        return Some(Referencia::Github { repositorio: Some(repositorio.to_string()), numero: numero(n)?, pr: false });
    }
    chave_linear(texto).map(|chave| Referencia::Linear { chave })
}

// ---------- ramos ----------

pub const PREFIXO_PADRAO: &str = "vitral/";
const TETO_DO_RAMO: usize = 60;

/// Minúsculas sem acento, com um hífen no lugar de cada trecho que não é letra ou número.
fn sem_acento(texto: &str) -> String {
    let mut s = String::new();
    for c in texto.to_lowercase().chars() {
        let c = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    s.trim_matches('-').to_string()
}

/// Só `[a-z0-9/-]`, sem trechos vazios, sem hífen ou barra nas pontas e no máximo 60 caracteres.
fn limpar_ramo(texto: &str) -> String {
    let partes: Vec<String> = texto.split('/').map(sem_acento).filter(|p| !p.is_empty()).collect();
    cortar_ramo(&partes.join("/"), TETO_DO_RAMO)
}

fn cortar_ramo(ramo: &str, limite: usize) -> String {
    // Só ASCII aqui: bytes e caracteres são a mesma coisa.
    ramo.get(..limite.min(ramo.len())).unwrap_or(ramo).trim_end_matches(['-', '/']).to_string()
}

/// O prefixo dos ramos de uma pasta: "" vale `vitral/`; sem separador no fim, ganha uma barra.
fn limpar_prefixo(prefixo: &str) -> String {
    let limpo = limpar_ramo(prefixo);
    if limpo.is_empty() {
        return PREFIXO_PADRAO.to_string();
    }
    let separador = if prefixo.trim_end().ends_with('-') { '-' } else { '/' };
    format!("{limpo}{separador}")
}

/// O nome do ramo da tarefa `n`. Issue do Linear com o prefixo padrão: o `branchName` do Linear;
/// com prefixo próprio: `<prefixo><chave>-<título>`. Issue do GitHub: `<prefixo><número>-<título>`.
/// Sem issue: `<prefixo>t<n>-<título>` (o de sempre, com o prefixo padrão). Se o nome já existe
/// (`existe` olha os ramos locais e os do remoto), ganha `-t<n>`.
pub fn ramo_da_tarefa(
    n: u64,
    issue: Option<&IssueLigada>,
    branch_name_linear: Option<&str>,
    prefixo: &str,
    titulo: &str,
    existe: &dyn Fn(&str) -> bool,
) -> String {
    let prefixo = limpar_prefixo(prefixo);
    let titulo = crate::dados::slug(titulo).trim_matches('-').to_string();
    let com_titulo = |inicio: String| if titulo.is_empty() { inicio } else { format!("{inicio}-{titulo}") };
    let do_linear = branch_name_linear.map(limpar_ramo).filter(|r| !r.is_empty() && prefixo == PREFIXO_PADRAO);
    let nome = match issue {
        Some(issue) if issue.fonte == FonteIssue::Linear => match do_linear {
            Some(ramo) => ramo,
            None => com_titulo(format!("{prefixo}{}", issue.chave.to_lowercase())),
        },
        Some(issue) => {
            let numero = issue.chave.trim_start_matches('#');
            com_titulo(format!("{prefixo}{numero}"))
        }
        None => com_titulo(format!("{prefixo}t{n}")),
    };
    let nome = limpar_ramo(&nome);
    if !existe(&nome) {
        return nome;
    }
    let mut tentativa = String::new();
    for vez in 1..=20 {
        let sufixo = if vez == 1 { format!("-t{n}") } else { format!("-t{n}-{vez}") };
        tentativa = format!("{}{sufixo}", cortar_ramo(&nome, TETO_DO_RAMO.saturating_sub(sufixo.len())));
        if !existe(&tentativa) {
            break;
        }
    }
    tentativa
}

/// O prefixo que os ramos de PR de alguém costumam ter: o maior que termina em `/` ou `-` e é
/// comum a pelo menos 3 deles, cobrindo pelo menos metade. `feature/vitor-x`, `feature/vitor-y` e
/// `feature/vitor-z` sugerem `feature/vitor-`.
pub fn prefixo_sugerido(ramos: &[String]) -> Option<String> {
    let mut unicos: Vec<&str> = ramos.iter().map(String::as_str).filter(|r| !r.is_empty()).collect();
    unicos.sort_unstable();
    unicos.dedup();
    let mut candidatos: Vec<&str> = unicos
        .iter()
        .flat_map(|ramo| ramo.char_indices().filter(|(_, c)| matches!(c, '/' | '-')).map(|(i, _)| &ramo[..=i]))
        .collect();
    candidatos.sort_unstable();
    candidatos.dedup();
    candidatos
        .into_iter()
        .filter(|prefixo| {
            let cobertos = unicos.iter().filter(|r| r.starts_with(prefixo)).count();
            cobertos >= 3 && cobertos * 2 >= unicos.len()
        })
        .max_by_key(|prefixo| prefixo.len())
        .map(str::to_string)
}

// ---------- textos da issue e do PR ----------

/// Corta em `limite` bytes, sem partir caractere, e marca com "…".
fn cortar(texto: &str, limite: usize) -> String {
    let texto = texto.trim();
    if texto.len() <= limite {
        return texto.to_string();
    }
    let mut fim = limite;
    while !texto.is_char_boundary(fim) {
        fim -= 1;
    }
    format!("{}…", texto[..fim].trim_end())
}

/// Os critérios de aceite escritos como caixas de marcar (`- [ ] …`, também `* [x] …`), até 12.
pub fn criterios_do_corpo(corpo: &str) -> Vec<String> {
    corpo
        .lines()
        .filter_map(|linha| {
            let resto = linha.trim_start().strip_prefix(['-', '*', '+'])?.trim_start();
            let resto = ["[ ]", "[x]", "[X]"].iter().find_map(|caixa| resto.strip_prefix(caixa))?;
            let texto = resto.trim();
            (!texto.is_empty()).then(|| cortar(texto, 300))
        })
        .take(12)
        .collect()
}

/// O objetivo da tarefa que nasce de uma issue: chave e título, a descrição (até 8000) e os
/// últimos 5 comentários (até 1500 cada).
pub fn objetivo_da_issue(d: &IssueDetalhe) -> String {
    let mut texto = format!("{}: {}", d.resumo.chave, d.resumo.titulo.trim());
    let descricao = cortar(&d.descricao, 8000);
    if !descricao.is_empty() {
        texto.push_str("\n\n");
        texto.push_str(&descricao);
    }
    let recentes: Vec<&ComentarioIssue> =
        d.comentarios.iter().filter(|c| !c.corpo.trim().is_empty()).rev().take(5).collect();
    if !recentes.is_empty() {
        texto.push_str("\n\nComentários recentes:");
        for comentario in recentes.into_iter().rev() {
            let autor = if comentario.autor.is_empty() { "alguém" } else { comentario.autor.as_str() };
            texto.push_str(&format!("\n- {autor}: {}", cortar(&comentario.corpo, 1500)));
        }
    }
    texto
}

/// O título do PR: "ENG-123: Corrige o login" no Linear, "Corrige o login (#45)" no GitHub e o
/// título do chat sem issue. O chat de uma issue se chama "ENG-123 · …": a chave não se repete.
pub fn titulo_do_pr(titulo_chat: &str, issue: Option<&IssueLigada>) -> String {
    let Some(issue) = issue else {
        return titulo_chat.trim().to_string();
    };
    let base = titulo_chat.trim();
    let base = base
        .strip_prefix(issue.chave.as_str())
        .map(|resto| resto.trim_start_matches([' ', '·', ':', '-']).trim())
        .unwrap_or(base);
    let base = if base.is_empty() { issue.titulo.trim() } else { base };
    match issue.fonte {
        FonteIssue::Linear => format!("{}: {base}", issue.chave),
        FonteIssue::Github => format!("{base} ({})", issue.chave),
    }
}

pub struct DadosDoCorpo<'a> {
    /// O resumo da entrega, ou o objetivo, ou as mensagens do usuário desde a última junção.
    pub resumo: &'a str,
    pub como_verificou: &'a str,
    /// O comando de verificação do Vitral e se passou.
    pub verificacao: Option<(&'a str, bool)>,
    pub issue: Option<&'a IssueLigada>,
}

/// O corpo do PR, sem gastar turno: o que muda, como foi verificado e a linha que fecha a issue.
pub fn corpo_do_pr(d: &DadosDoCorpo) -> String {
    let mut corpo = format!("## O que muda\n{}\n\n## Como foi verificado\n", cortar(d.resumo, 1500));
    let mut verificado = Vec::new();
    if !d.como_verificou.trim().is_empty() {
        verificado.push(d.como_verificou.trim().to_string());
    }
    if let Some((comando, passou)) = d.verificacao {
        verificado.push(format!("`{comando}` {}.", if passou { "passou" } else { "falhou" }));
    }
    if verificado.is_empty() {
        verificado.push("Sem verificação registrada.".to_string());
    }
    corpo.push_str(&verificado.join("\n"));
    corpo.push('\n');
    if let Some(issue) = d.issue {
        let linha = match issue.fonte {
            FonteIssue::Linear => format!("Fixes {}", issue.chave),
            FonteIssue::Github => format!("Closes {}", issue.chave),
        };
        corpo.push_str(&format!("\n{linha}\n"));
    }
    corpo
}

/// Os bots de revisão que contam como revisor de verdade (com ou sem `[bot]`). "copilot" é o
/// autor dos comentários em linha do copilot-pull-request-reviewer.
const BOTS_DE_REVISAO: [&str; 7] = [
    "copilot-pull-request-reviewer",
    "copilot",
    "coderabbitai",
    "cursor",
    "greptile-apps",
    "chatgpt-codex-connector",
    "devin-ai-integration",
];

/// `assoc` é o `author_association`, `tipo` o `user.type` ("User" ou "Bot") e `tem_path` diz se é
/// comentário em linha (não muda a decisão hoje).
pub fn confianca(assoc: &str, login: &str, tipo: &str, tem_path: bool) -> Confianca {
    let _ = tem_path;
    let minusculo = login.to_ascii_lowercase();
    let sem_bot = minusculo.strip_suffix("[bot]").unwrap_or(&minusculo);
    if matches!(assoc, "OWNER" | "MEMBER" | "COLLABORATOR") || BOTS_DE_REVISAO.contains(&sem_bot) {
        Confianca::Confiavel
    } else if tipo.eq_ignore_ascii_case("Bot") || minusculo.ends_with("[bot]") {
        Confianca::Informativo
    } else {
        Confianca::Terceiro
    }
}

/// Os índices das issues que batem com a busca local: cada palavra tem de aparecer na chave, no
/// título, numa etiqueta ou no responsável, sem diferença de maiúsculas e acentos.
pub fn aplicar_filtro_local(lista: &[IssueResumo], busca: &str) -> Vec<usize> {
    let palavras: Vec<String> = busca.split_whitespace().map(sem_acento).filter(|p| !p.is_empty()).collect();
    lista
        .iter()
        .enumerate()
        .filter(|(_, issue)| {
            let mut onde = format!("{} {}", issue.chave, issue.titulo);
            for etiqueta in &issue.etiquetas {
                onde.push(' ');
                onde.push_str(&etiqueta.nome);
            }
            if let Some(pessoa) = &issue.responsavel {
                onde.push(' ');
                onde.push_str(&pessoa.nome);
            }
            let onde = sem_acento(&onde);
            palavras.iter().all(|p| onde.contains(p.as_str()))
        })
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn parse_remoto_aceita_os_tres_formatos_do_github() {
        let par = |d: &str, n: &str| Some((d.to_string(), n.to_string()));
        assert_eq!(parse_remoto("https://github.com/VictorMenegate/top-bar-app-color.git"), par("VictorMenegate", "top-bar-app-color"));
        assert_eq!(parse_remoto("https://github.com/VictorMenegate/HUB"), par("VictorMenegate", "HUB"));
        assert_eq!(parse_remoto("https://github.com/VictorMenegate/HUB/"), par("VictorMenegate", "HUB"));
        assert_eq!(parse_remoto("git@github.com:cli/cli.git"), par("cli", "cli"));
        assert_eq!(parse_remoto("ssh://git@github.com/cli/cli.git"), par("cli", "cli"));
        assert_eq!(parse_remoto("https://x-access@github.com/o/r.git"), par("o", "r"));
        assert_eq!(parse_remoto("  https://github.com/o/r.js.git\n"), par("o", "r.js"));
        assert_eq!(parse_remoto("https://gitlab.com/o/r.git"), None);
        assert_eq!(parse_remoto("git@bitbucket.org:o/r.git"), None);
        assert_eq!(parse_remoto("/home/victor/repo.git"), None);
        assert_eq!(parse_remoto("https://github.com/o"), None);
        assert_eq!(parse_remoto("https://github.com/o/r/extra"), None);
        assert_eq!(parse_remoto("https://github.com/o/r;rm -rf"), None);
    }

    #[test]
    fn parse_referencia_reconhece_chaves_numeros_e_urls() {
        let linear = |c: &str| Some(Referencia::Linear { chave: c.into() });
        let github = |r: Option<&str>, numero, pr| Some(Referencia::Github { repositorio: r.map(String::from), numero, pr });
        assert_eq!(parse_referencia("ENG-123"), linear("ENG-123"));
        assert_eq!(parse_referencia(" ENG2-7 "), linear("ENG2-7"));
        assert_eq!(parse_referencia("#45"), github(None, 45, false));
        assert_eq!(parse_referencia("o/r#45"), github(Some("o/r"), 45, false));
        assert_eq!(parse_referencia("https://github.com/cli/cli/pull/14845"), github(Some("cli/cli"), 14845, true));
        assert_eq!(
            parse_referencia("https://github.com/cli/cli/issues/14882#issuecomment-1"),
            github(Some("cli/cli"), 14882, false)
        );
        assert_eq!(parse_referencia("https://linear.app/eita/issue/ENG-162/sentry-39-problemas"), linear("ENG-162"));
        assert_eq!(parse_referencia("https://linear.app/eita/issue/eng-162"), linear("ENG-162"));
        for nada in ["eng-123", "utf-8", "ENG-", "ENG-012", "#", "#abc", "#0", "o/#4", "https://github.com/o/r/commit/abc", "", "texto"] {
            assert_eq!(parse_referencia(nada), None, "{nada}");
        }
    }

    fn ligada(fonte: FonteIssue, chave: &str, titulo: &str) -> IssueLigada {
        IssueLigada { fonte, chave: chave.into(), titulo: titulo.into(), ..Default::default() }
    }

    #[test]
    fn ramo_da_tarefa_em_cada_caso() {
        let nunca = |_: &str| false;
        let linear = ligada(FonteIssue::Linear, "ENG-123", "Corrige o login");
        let github = ligada(FonteIssue::Github, "#45", "Corrige o login");
        // Linear com o prefixo padrão: o branchName do Linear.
        assert_eq!(
            ramo_da_tarefa(3, Some(&linear), Some("victor/eng-123-corrige-o-login"), "", "Corrige o login", &nunca),
            "victor/eng-123-corrige-o-login"
        );
        assert_eq!(
            ramo_da_tarefa(3, Some(&linear), Some("victor/ENG-123_corrige"), "vitral/", "x", &nunca),
            "victor/eng-123-corrige"
        );
        // Sem branchName, ou com prefixo próprio: o prefixo e a chave.
        assert_eq!(ramo_da_tarefa(3, Some(&linear), None, "", "Corrige o login", &nunca), "vitral/eng-123-corrige-o-login");
        assert_eq!(
            ramo_da_tarefa(3, Some(&linear), Some("victor/eng-123-corrige-o-login"), "feature/vitor-", "Corrige o login", &nunca),
            "feature/vitor-eng-123-corrige-o-login"
        );
        assert_eq!(ramo_da_tarefa(3, Some(&github), None, "", "Corrige o login", &nunca), "vitral/45-corrige-o-login");
        assert_eq!(ramo_da_tarefa(3, Some(&github), None, "Feature/Vitor-", "Corrige o login", &nunca), "feature/vitor-45-corrige-o-login");
        // Sem issue é o nome de hoje; prefixo sem separador ganha uma barra.
        assert_eq!(ramo_da_tarefa(7, None, None, "", "Ajusta a cor da barra", &nunca), "vitral/t7-ajusta-a-cor-da-barra");
        assert_eq!(ramo_da_tarefa(7, None, None, "minhas", "Ajusta", &nunca), "minhas/t7-ajusta");
        // Acentos e símbolos.
        assert_eq!(ramo_da_tarefa(2, None, None, "", "Ação: configuração já!", &nunca), "vitral/t2-acao-configuracao-ja");
        // Até 60 caracteres, sem hífen pendurado.
        let longo = ramo_da_tarefa(9, Some(&linear), Some(&format!("victor/eng-123-{}", "palavra-".repeat(12))), "", "", &nunca);
        assert!(longo.len() <= 60 && !longo.ends_with('-'), "{longo}");
        // Colisão: -t<N>, sem passar de 60.
        let ocupados = ["vitral/45-corrige-o-login".to_string()];
        let existe = |nome: &str| ocupados.iter().any(|o| o == nome);
        assert_eq!(ramo_da_tarefa(12, Some(&github), None, "", "Corrige o login", &existe), "vitral/45-corrige-o-login-t12");
        let longo_existe = |nome: &str| nome.len() > 50 && !nome.ends_with("-t9");
        let cortado = ramo_da_tarefa(9, Some(&linear), Some(&format!("victor/eng-123-{}", "palavra-".repeat(12))), "", "", &longo_existe);
        assert!(cortado.ends_with("-t9") && cortado.len() <= 60, "{cortado}");
        for nome in [longo, cortado] {
            assert!(nome.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '/' || c == '-'), "{nome}");
        }
    }

    #[test]
    fn prefixo_sugerido_pelos_ramos_anteriores() {
        let ramos = |r: &[&str]| r.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            prefixo_sugerido(&ramos(&["feature/vitor-x", "feature/vitor-y", "feature/vitor-z"])),
            Some("feature/vitor-".into())
        );
        // Os 6 PRs do jcm, com um de outro formato: o prefixo cobre mais da metade.
        let jcm = ramos(&[
            "feature/vitor-e2e-painel",
            "feature/vitor-relatorios",
            "feature/vitor-agenda",
            "feature/vitor-login",
            "fix/vitor-botao",
            "feature/vitor-filtros",
        ]);
        assert_eq!(prefixo_sugerido(&jcm), Some("feature/vitor-".into()));
        // Menos de 3, ou menos da metade, não sugere.
        assert_eq!(prefixo_sugerido(&ramos(&["feature/vitor-x", "feature/vitor-y"])), None);
        assert_eq!(prefixo_sugerido(&ramos(&["a/x", "a/y", "a/z", "b1", "b2", "b3", "b4"])), None);
        assert_eq!(prefixo_sugerido(&ramos(&["feature/vitor-x", "feature/vitor-x", "feature/vitor-x"])), None);
        assert_eq!(prefixo_sugerido(&[]), None);
    }

    #[test]
    fn criterios_objetivo_e_titulo() {
        let corpo = "Contexto\n- [ ] entra com Google\n  * [x] mostra o erro\n- [ ]   \n- item comum\n[ ] sem marcador";
        assert_eq!(criterios_do_corpo(corpo), ["entra com Google", "mostra o erro"]);
        let muitos: String = (0..20).map(|i| format!("- [ ] c{i}\n")).collect();
        assert_eq!(criterios_do_corpo(&muitos).len(), 12);

        let comentarios: Vec<ComentarioIssue> = (1..=7)
            .map(|i| ComentarioIssue { autor: format!("p{i}"), corpo: format!("c{i}"), ..Default::default() })
            .collect();
        let detalhe = IssueDetalhe {
            resumo: IssueResumo { chave: "ENG-5".into(), titulo: "Corrige o login".into(), ..Default::default() },
            descricao: "d".repeat(9000),
            comentarios,
            ..Default::default()
        };
        let objetivo = objetivo_da_issue(&detalhe);
        assert!(objetivo.starts_with("ENG-5: Corrige o login\n\nddd"));
        assert!(objetivo.contains("…\n\nComentários recentes:\n- p3: c3\n"), "{objetivo}");
        assert!(objetivo.ends_with("- p7: c7"));
        assert!(!objetivo.contains("p2: c2"));
        assert!(objetivo.len() < 8200);

        let linear = ligada(FonteIssue::Linear, "ENG-123", "Login quebrado");
        let github = ligada(FonteIssue::Github, "#45", "Login quebrado");
        assert_eq!(titulo_do_pr("ENG-123 · Corrige o login", Some(&linear)), "ENG-123: Corrige o login");
        assert_eq!(titulo_do_pr("Corrige o login", Some(&github)), "Corrige o login (#45)");
        assert_eq!(titulo_do_pr("ENG-123", Some(&linear)), "ENG-123: Login quebrado");
        assert_eq!(titulo_do_pr("  Corrige o login ", None), "Corrige o login");
    }

    #[test]
    fn corpo_do_pr_no_formato_combinado() {
        let issue = ligada(FonteIssue::Linear, "ENG-123", "x");
        let corpo = corpo_do_pr(&DadosDoCorpo {
            resumo: "Troca o botão do Google por um que funciona no Safari.",
            como_verificou: "Testei no Safari e no Firefox.",
            verificacao: Some(("cargo test", true)),
            issue: Some(&issue),
        });
        assert_eq!(
            corpo,
            "## O que muda\nTroca o botão do Google por um que funciona no Safari.\n\n\
             ## Como foi verificado\nTestei no Safari e no Firefox.\n`cargo test` passou.\n\n\
             Fixes ENG-123\n"
        );
        let github = ligada(FonteIssue::Github, "#45", "x");
        let simples = corpo_do_pr(&DadosDoCorpo { resumo: "Muda.", como_verificou: "", verificacao: None, issue: Some(&github) });
        assert_eq!(simples, "## O que muda\nMuda.\n\n## Como foi verificado\nSem verificação registrada.\n\nCloses #45\n");
        let sem_issue = corpo_do_pr(&DadosDoCorpo { resumo: "Muda.", como_verificou: "", verificacao: Some(("npm test", false)), issue: None });
        assert_eq!(sem_issue, "## O que muda\nMuda.\n\n## Como foi verificado\n`npm test` falhou.\n");
    }

    #[test]
    fn confianca_dos_autores() {
        assert_eq!(confianca("OWNER", "VictorMenegate", "User", false), Confianca::Confiavel);
        assert_eq!(confianca("COLLABORATOR", "clodoaldo", "User", true), Confianca::Confiavel);
        assert_eq!(confianca("CONTRIBUTOR", "alguem", "User", true), Confianca::Terceiro);
        assert_eq!(confianca("FIRST_TIME_CONTRIBUTOR", "novo", "User", false), Confianca::Terceiro);
        assert_eq!(confianca("NONE", "cloudflare-workers-and-pages[bot]", "Bot", false), Confianca::Informativo);
        assert_eq!(confianca("NONE", "vercel", "Bot", true), Confianca::Informativo);
        assert_eq!(confianca("NONE", "coderabbitai[bot]", "Bot", true), Confianca::Confiavel);
        assert_eq!(confianca("CONTRIBUTOR", "copilot-pull-request-reviewer[bot]", "Bot", false), Confianca::Confiavel);
        assert_eq!(confianca("CONTRIBUTOR", "Copilot", "Bot", true), Confianca::Confiavel);
    }

    #[test]
    fn filtro_local_ignora_maiusculas_e_acentos() {
        let issue = |chave: &str, titulo: &str| IssueResumo { chave: chave.into(), titulo: titulo.into(), ..Default::default() };
        let mut com_etiqueta = issue("ENG-3", "Exportar");
        com_etiqueta.etiquetas.push(Etiqueta { nome: "Segurança".into(), cor: None });
        let lista = [issue("ENG-1", "Botão de exportação"), issue("ENG-2", "Login"), com_etiqueta];
        assert_eq!(aplicar_filtro_local(&lista, "BOTAO export"), [0]);
        assert_eq!(aplicar_filtro_local(&lista, "eng-2"), [1]);
        assert_eq!(aplicar_filtro_local(&lista, "seguranca"), [2]);
        assert_eq!(aplicar_filtro_local(&lista, "  "), [0, 1, 2]);
    }
}
