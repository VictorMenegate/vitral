//! A aba PR do painel da direita (seção 4.3 da especificação): o cabeçalho do PR (o número, o link,
//! os anteriores, a pílula do estado, ⟳ e o ⋯), o título que edita no duplo clique, os chips do
//! ramo e da issue com os selos, as sub-abas (Visão geral, Arquivos, Checks), a linha "Falta: …" e
//! o rodapé com Mesclar e "Mesclar quando ficar pronto". Tudo é lido do motor a cada desenho; com a
//! aba à vista, o vigia consulta mais (`vigiar_aba_pr`), e parada a aba não redesenha nada.
//!
//! Também moram aqui as funções puras que as outras telas do PR usam: a habilitação das ações (a
//! tabela 4.3, `acoes_do_pr`, que o motor chama), o rótulo dos checks, os links que podem abrir
//! (texto de terceiros nunca abre `file://` nem `vitral:`) e os links vivos do texto do assistente
//! (`ligar_referencias`).

use crate::{
    github::{EstadoChecks, EstadoPr, InfoRepo, ResumoChecks},
    modelo::Pr,
    motor::{AcaoPr, metodos_permitidos},
};

// ---------- a habilitação das ações (tabela 4.3) ----------

/// O que ocupa o PR agora: Mesclar e as outras escritas esperam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ocupado {
    /// O chat está rodando (ou tem mensagens na fila).
    Chat,
    Publicando,
    Mesclando,
}

impl Ocupado {
    fn motivo(self) -> &'static str {
        match self {
            Ocupado::Chat => "Esperando o chat terminar",
            Ocupado::Publicando => "Publicando o PR…",
            Ocupado::Mesclando => "Mesclando o PR…",
        }
    }
}

/// O nome da base nos textos ("main"), com o que o PR diz.
fn nome_da_base(pr: &Pr) -> &str {
    if pr.base.trim().is_empty() { "a base" } else { pr.base.as_str() }
}

/// A simulação local deu conflito, ou o GitHub diz `DIRTY`.
pub fn tem_conflito(pr: &Pr, simulacao: Option<&Result<Vec<String>, String>>) -> bool {
    pr.mesclavel == "DIRTY" || simulacao.is_some_and(|s| s.as_ref().is_ok_and(|arquivos| !arquivos.is_empty()))
}

/// "2 checks falharam", "1 check falhou".
fn falharam(n: u32) -> String {
    if n == 1 { "1 check falhou".into() } else { format!("{n} checks falharam") }
}

/// Os métodos de mesclar que valem, com o padrão primeiro; sem o repositório, o squash.
pub fn metodos_do_pr(info: Option<&InfoRepo>) -> Vec<String> {
    let metodos: Vec<String> = info.map(|i| metodos_permitidos(i).into_iter().map(String::from).collect()).unwrap_or_default();
    if metodos.is_empty() { vec!["squash".into()] } else { metodos }
}

/// Por que Mesclar não está habilitado agora, na ordem da tabela 4.3 (vale o primeiro). Commit de
/// fora e `UNSTABLE` mesclam, com confirmação (`confirmacoes_para_mesclar`).
fn motivo_de_nao_mesclar(
    pr: &Pr,
    info: Option<&InfoRepo>,
    simulacao: Option<&Result<Vec<String>, String>>,
    ocupado: Option<Ocupado>,
) -> Option<String> {
    let base = nome_da_base(pr);
    let c = &pr.checks;
    match pr.estado {
        EstadoPr::Mesclado | EstadoPr::Fechado => return Some(format!("O PR está {}", pr.estado.nome())),
        EstadoPr::Rascunho => return Some("Rascunho: marque como pronto para revisão".into()),
        EstadoPr::Aberto => {}
    }
    if let Some(o) = ocupado {
        return Some(o.motivo().into());
    }
    if let Some(info) = info.filter(|i| !i.pode_escrever()) {
        let repo = if info.repositorio.is_empty() { &pr.repositorio } else { &info.repositorio };
        return Some(format!("Sua conta não pode mesclar em {repo}"));
    }
    if tem_conflito(pr, simulacao) {
        return Some(format!("Tem conflito com {base}: peça ao chat para resolver"));
    }
    if pr.mesclavel == "BEHIND" {
        return Some(format!("Atrás de {base}: atualize antes"));
    }
    if c.pendentes > 0 {
        return Some(format!("{} de {} checks terminaram", c.terminados, c.total));
    }
    if c.estado == EstadoChecks::Falhou || c.falhos > 0 {
        return Some(falharam(c.falhos));
    }
    if pr.mesclavel == "BLOCKED" {
        return Some("O GitHub ainda não permite (aprovação ou check obrigatório)".into());
    }
    None
}

/// As ações da aba PR e por que cada uma não está habilitada (a tabela 4.3). O Mesclar vem uma vez
/// por método permitido, o padrão primeiro (os outros vão no ⋯).
pub fn acoes_do_pr(
    pr: &Pr,
    info: Option<&InfoRepo>,
    simulacao: Option<&Result<Vec<String>, String>>,
    ocupado: Option<Ocupado>,
) -> Vec<(AcaoPr, Result<(), String>)> {
    let aberto = pr.aberto();
    let estado = || format!("O PR está {}", pr.estado.nome());
    let escreve = info.is_none_or(InfoRepo::pode_escrever);
    let sem_escrita = || format!("Sua conta não pode mesclar em {}", info.map_or(pr.repositorio.as_str(), |i| i.repositorio.as_str()));
    let livre = |r: Result<(), String>| match ocupado {
        Some(o) if r.is_ok() => Err(o.motivo().to_string()),
        _ => r,
    };
    let mesclar: Result<(), String> = match motivo_de_nao_mesclar(pr, info, simulacao, ocupado) {
        Some(m) => Err(m),
        None => Ok(()),
    };
    let mut acoes: Vec<(AcaoPr, Result<(), String>)> =
        metodos_do_pr(info).into_iter().map(|metodo| (AcaoPr::Mesclar { metodo }, mesclar.clone())).collect();
    let ship = match () {
        _ if !aberto => Err(estado()),
        _ if !escreve => Err(sem_escrita()),
        _ => Ok(()),
    };
    acoes.push((AcaoPr::MesclarQuandoPronto, ship));
    acoes.push((AcaoPr::AtualizarComBase, livre(if aberto { Ok(()) } else { Err(estado()) })));
    let conflito = tem_conflito(pr, simulacao);
    acoes.push((
        AcaoPr::PedirAoChat,
        livre(match () {
            _ if !aberto => Err(estado()),
            _ if !conflito => Err(format!("Sem conflito com {}", nome_da_base(pr))),
            _ => Ok(()),
        }),
    ));
    acoes.push((
        AcaoPr::ProntoParaRevisao,
        livre(if pr.estado == EstadoPr::Rascunho { Ok(()) } else { Err("O PR não é rascunho".into()) }),
    ));
    acoes.push((
        AcaoPr::VoltarParaRascunho,
        livre(if pr.estado == EstadoPr::Aberto { Ok(()) } else { Err(estado()) }),
    ));
    acoes.push((AcaoPr::Fechar, livre(if aberto { Ok(()) } else { Err(estado()) })));
    acoes.push((
        AcaoPr::Reabrir,
        livre(if pr.estado == EstadoPr::Fechado { Ok(()) } else { Err(estado()) }),
    ));
    acoes
}

/// O Mesclar pelo método padrão: o primeiro da lista de `acoes_do_pr`.
pub fn acao_mesclar_padrao(acoes: &[(AcaoPr, Result<(), String>)]) -> AcaoPr {
    acoes
        .iter()
        .find(|(a, _)| matches!(a, AcaoPr::Mesclar { .. }))
        .map_or_else(|| AcaoPr::Mesclar { metodo: "squash".into() }, |(a, _)| a.clone())
}

/// O resultado de uma ação na lista de `acoes_do_pr` (a primeira com o mesmo nome).
pub fn resultado(acoes: &[(AcaoPr, Result<(), String>)], quer: &AcaoPr) -> Result<(), String> {
    acoes
        .iter()
        .find(|(a, _)| a == quer)
        .map_or_else(|| Err("Ainda lendo o PR".into()), |(_, r)| r.clone())
}

/// O que ainda falta para o PR ficar pronto para mesclar, para a linha "Falta: …" acima do
/// rodapé. Vazia num PR aberto: "Pronto para mesclar".
pub fn falta_para_mesclar(pr: &Pr, simulacao: Option<&Result<Vec<String>, String>>) -> Vec<String> {
    let base = nome_da_base(pr);
    let c = &pr.checks;
    let mut falta = Vec::new();
    if pr.estado == EstadoPr::Rascunho {
        falta.push("marcar como pronto para revisão".into());
    }
    if tem_conflito(pr, simulacao) {
        falta.push(format!("resolver o conflito com {base}"));
    } else if pr.mesclavel == "BEHIND" {
        falta.push(format!("atualizar com {base}"));
    }
    if c.falhos > 0 {
        falta.push(falharam(c.falhos));
    }
    if c.pendentes > 0 {
        falta.push(format!("{} rodando", if c.pendentes == 1 { "1 check".to_string() } else { format!("{} checks", c.pendentes) }));
    }
    if pr.decisao == "CHANGES_REQUESTED" {
        falta.push("mudanças pedidas".into());
    } else if pr.decisao == "REVIEW_REQUIRED" {
        falta.push("aprovação".into());
    }
    match pr.conversas_abertas {
        0 => {}
        1 => falta.push("1 conversa aberta".into()),
        n => falta.push(format!("{n} conversas abertas")),
    }
    if falta.is_empty() && pr.mesclavel == "BLOCKED" {
        falta.push("aprovação ou check obrigatório".into());
    }
    falta
}

/// O que o usuário precisa confirmar antes de mesclar agora: os commits de fora (até o head que
/// ele vê) e os checks não obrigatórios que falharam (`UNSTABLE`).
pub fn confirmacoes_para_mesclar(pr: &Pr) -> Vec<crate::motor::Confirmado> {
    let mut lista = Vec::new();
    if !pr.commits_de_fora.is_empty() {
        lista.push(crate::motor::Confirmado::CommitsDeFora(pr.sha.clone()));
    }
    if pr.mesclavel == "UNSTABLE" {
        lista.push(crate::motor::Confirmado::Instavel);
    }
    lista
}

// ---------- os checks ----------

/// Como os checks aparecem: a fração do anel, o curto ("15/41") e a frase da dica.
#[derive(Clone, Debug, PartialEq)]
pub struct RotuloChecks {
    pub fracao: f32,
    pub curto: String,
    pub frase: String,
}

pub fn rotulo_checks(c: &ResumoChecks) -> RotuloChecks {
    let fracao = if c.total == 0 { 0. } else { c.terminados as f32 / c.total as f32 };
    match c.estado {
        _ if c.total == 0 => RotuloChecks { fracao: 0., curto: "sem checks".into(), frase: "Sem checks".into() },
        EstadoChecks::Pendente | EstadoChecks::Nenhum => {
            let falhas = if c.falhos > 0 { format!(" ({})", falharam(c.falhos)) } else { String::new() };
            RotuloChecks {
                fracao,
                curto: format!("{}/{}", c.terminados, c.total),
                frase: format!("{} de {} checks terminaram{falhas}", c.terminados, c.total),
            }
        }
        EstadoChecks::Passou => RotuloChecks {
            fracao: 1.,
            curto: format!("{}/{}", c.total, c.total),
            frase: if c.total == 1 { "1 check passou".into() } else { format!("{} checks passaram", c.total) },
        },
        EstadoChecks::Falhou => RotuloChecks {
            fracao,
            curto: if c.falhos == 1 { "1 falhou".into() } else { format!("{} falharam", c.falhos) },
            frase: format!("{} de {} checks falharam", c.falhos, c.total),
        },
    }
}

// ---------- links ----------

/// De onde vem o texto em que o link está.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrigemDoTexto {
    /// A descrição do PR, um comentário, uma issue: escrito por outra pessoa.
    Terceiros,
    /// A resposta do assistente, onde `ligar_referencias` põe os `vitral:`.
    Assistente,
}

/// O link pode abrir: `http(s)` sempre; `vitral:` só no texto do assistente (gerado por
/// `ligar_referencias`); o resto (`file://`, `javascript:`, caminhos) nunca.
pub fn link_permitido(url: &str, origem: OrigemDoTexto) -> bool {
    let url = url.trim();
    let comeca = |p: &str| url.len() > p.len() && url[..p.len()].eq_ignore_ascii_case(p);
    if comeca("http://") || comeca("https://") {
        return true;
    }
    origem == OrigemDoTexto::Assistente && comeca("vitral:") && ler_link_vitral(url).is_some()
}

/// Para onde um link `vitral:` leva.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkVitral {
    Pr { repositorio: String, numero: u64 },
    IssueLinear(String),
    IssueGithub { repositorio: String, numero: u64 },
}

/// `vitral:pr/O/R/12`, `vitral:issue/linear/ENG-123` e `vitral:issue/github/O/R/45`.
pub fn ler_link_vitral(url: &str) -> Option<LinkVitral> {
    let resto = url.trim().strip_prefix("vitral:")?;
    let partes: Vec<&str> = resto.split('/').collect();
    let numero = |s: &str| s.parse::<u64>().ok().filter(|n| *n > 0);
    let repo = |o: &str, r: &str| (!o.is_empty() && !r.is_empty()).then(|| format!("{o}/{r}"));
    match partes.as_slice() {
        ["pr", o, r, n] => Some(LinkVitral::Pr { repositorio: repo(o, r)?, numero: numero(n)? }),
        ["issue", "linear", chave] if chave_linear(chave) => Some(LinkVitral::IssueLinear(chave.to_string())),
        ["issue", "github", o, r, n] => Some(LinkVitral::IssueGithub { repositorio: repo(o, r)?, numero: numero(n)? }),
        _ => None,
    }
}

/// "ENG-123": de 2 a 10 letras maiúsculas ou dígitos (começando por letra), o hífen e o número.
fn chave_linear(texto: &str) -> bool {
    let Some((time, n)) = texto.split_once('-') else { return false };
    (2..=10).contains(&time.len())
        && time.starts_with(|c: char| c.is_ascii_uppercase())
        && time.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && !n.is_empty()
        && n.chars().all(|c| c.is_ascii_digit())
}

/// O que existe para os links vivos ligarem: só o que está em cache ou ligado ao projeto.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Referencias {
    /// O repositório do chat ("dono/nome"), que vale para "#12" e "PR #12".
    pub repositorio: Option<String>,
    /// Os PRs conhecidos ("dono/nome", número).
    pub prs: Vec<(String, u64)>,
    /// As issues do GitHub conhecidas ("dono/nome", número).
    pub issues_github: Vec<(String, u64)>,
    /// As chaves do Linear conhecidas.
    pub linear: Vec<String>,
}

impl Referencias {
    fn pr(&self, repo: &str, n: u64) -> bool {
        self.prs.iter().any(|(r, m)| r.eq_ignore_ascii_case(repo) && *m == n)
    }

    fn issue(&self, repo: &str, n: u64) -> bool {
        self.issues_github.iter().any(|(r, m)| r.eq_ignore_ascii_case(repo) && *m == n)
    }

    pub fn vazia(&self) -> bool {
        self.prs.is_empty() && self.issues_github.is_empty() && self.linear.is_empty()
    }
}

/// O link de "#12" no repositório do chat: o PR, senão a issue do GitHub.
fn link_do_numero(n: u64, refs: &Referencias) -> Option<String> {
    let repo = refs.repositorio.as_deref()?;
    if refs.pr(repo, n) {
        Some(format!("vitral:pr/{repo}/{n}"))
    } else if refs.issue(repo, n) {
        Some(format!("vitral:issue/github/{repo}/{n}"))
    } else {
        None
    }
}

/// Os dígitos no começo de `texto` (o número e quantos bytes ele tem).
fn digitos(texto: &str) -> Option<(u64, usize)> {
    let fim = texto.find(|c: char| !c.is_ascii_digit()).unwrap_or(texto.len());
    (fim > 0).then(|| texto[..fim].parse().ok().map(|n| (n, fim))).flatten()
}

/// Uma URL do GitHub de PR ou issue no começo de `texto`: o link `vitral:`, o rótulo e o tamanho.
fn url_do_github(texto: &str, refs: &Referencias) -> Option<(String, String, usize)> {
    let fim = texto.find(|c: char| c.is_whitespace() || matches!(c, ')' | '>' | ']' | '"' | '\'')).unwrap_or(texto.len());
    let url = texto[..fim].trim_end_matches(['.', ',', ';', ':', '!', '?']);
    let resto = url.strip_prefix("https://github.com/").or_else(|| url.strip_prefix("http://github.com/"))?;
    let partes: Vec<&str> = resto.split(['/', '#', '?']).collect();
    let (o, r, tipo, n) = match partes.as_slice() {
        [o, r, tipo, n, ..] => (*o, *r, *tipo, digitos(n)?.0),
        _ => return None,
    };
    let repo = format!("{o}/{r}");
    match tipo {
        "pull" if refs.pr(&repo, n) => Some((format!("vitral:pr/{repo}/{n}"), format!("PR #{n}"), url.len())),
        "issues" if refs.issue(&repo, n) => Some((format!("vitral:issue/github/{repo}/{n}"), format!("#{n}"), url.len())),
        _ => None,
    }
}

/// Liga as referências de um trecho de texto comum (fora de código e de links).
fn ligar_trecho(texto: &str, refs: &Referencias, saida: &mut String) {
    let mut i = 0;
    while i < texto.len() {
        let resto = &texto[i..];
        let antes = texto[..i].chars().next_back();
        let fronteira = antes.is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '/' | '&' | '-' | '#')));
        if fronteira && (resto.starts_with("https://github.com/") || resto.starts_with("http://github.com/")) {
            if let Some((link, rotulo, tamanho)) = url_do_github(resto, refs) {
                saida.push_str(&format!("[{rotulo}]({link})"));
                i += tamanho;
                continue;
            }
        }
        if fronteira && resto.starts_with("PR #")
            && let Some((n, tamanho)) = digitos(&resto[4..])
            && !resto[4 + tamanho..].starts_with(|c: char| c.is_alphanumeric())
            && let Some(repo) = refs.repositorio.as_deref().filter(|r| refs.pr(r, n))
        {
            saida.push_str(&format!("[PR #{n}](vitral:pr/{repo}/{n})"));
            i += 4 + tamanho;
            continue;
        }
        if fronteira && resto.starts_with('#')
            && let Some((n, tamanho)) = digitos(&resto[1..])
            && !resto[1 + tamanho..].starts_with(|c: char| c.is_alphanumeric())
            && let Some(link) = link_do_numero(n, refs)
        {
            saida.push_str(&format!("[#{n}]({link})"));
            i += 1 + tamanho;
            continue;
        }
        if fronteira && resto.starts_with(|c: char| c.is_ascii_uppercase()) {
            let fim = resto.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).unwrap_or(resto.len());
            let palavra = resto[..fim].trim_end_matches('-');
            if chave_linear(palavra) && refs.linear.iter().any(|k| k == palavra) {
                saida.push_str(&format!("[{palavra}](vitral:issue/linear/{palavra})"));
                i += palavra.len();
                continue;
            }
        }
        let c = resto.chars().next().unwrap_or_default();
        saida.push(c);
        i += c.len_utf8().max(1);
    }
}

/// Uma linha fora de bloco de código: o código em linha e os links que já existem ficam como
/// estão; o resto passa por `ligar_trecho`.
fn ligar_linha(linha: &str, refs: &Referencias, saida: &mut String) {
    let mut comum = String::new();
    let mut i = 0;
    while i < linha.len() {
        let resto = &linha[i..];
        // Código em linha: do acento até o mesmo número de acentos.
        if resto.starts_with('`') {
            let n = resto.len() - resto.trim_start_matches('`').len();
            let marca = &resto[..n];
            if let Some(fim) = resto[n..].find(marca) {
                ligar_trecho(&comum, refs, saida);
                comum.clear();
                saida.push_str(&resto[..n + fim + n]);
                i += n + fim + n;
                continue;
            }
        }
        // Link do markdown ("[texto](url)") e autolink ("<url>"): ficam inteiros.
        if resto.starts_with('[')
            && let Some(meio) = resto.find("](")
            && let Some(fim) = resto[meio..].find(')')
        {
            ligar_trecho(&comum, refs, saida);
            comum.clear();
            saida.push_str(&resto[..meio + fim + 1]);
            i += meio + fim + 1;
            continue;
        }
        if resto.starts_with('<')
            && let Some(fim) = resto.find('>')
            && resto[1..fim].contains("://")
        {
            ligar_trecho(&comum, refs, saida);
            comum.clear();
            saida.push_str(&resto[..fim + 1]);
            i += fim + 1;
            continue;
        }
        let c = resto.chars().next().unwrap_or_default();
        comum.push(c);
        i += c.len_utf8().max(1);
    }
    ligar_trecho(&comum, refs, saida);
}

/// Os links vivos: no texto do assistente, troca `#12`, `PR #12`, as URLs de PR e de issue do
/// GitHub e as chaves do Linear por links `vitral:`, só quando batem com algo que existe
/// (`Referencias`). Títulos, blocos de código e código em linha ficam como estão.
pub fn ligar_referencias(texto: &str, refs: &Referencias) -> String {
    if refs.vazia() {
        return texto.to_string();
    }
    let mut saida = String::with_capacity(texto.len() + 32);
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
        // Título ("# Título", "## 12 passos") e código indentado ficam como estão.
        let titulo = aparada.starts_with('#') && aparada.trim_start_matches('#').starts_with(' ');
        let indentado = linha.starts_with("    ") || linha.starts_with('\t');
        if titulo || indentado {
            saida.push_str(linha);
        } else {
            ligar_linha(linha, refs, &mut saida);
        }
    }
    saida
}


// ---------- a aba PR ----------

use std::collections::{HashMap, HashSet};

use gpui_kit::component::{
    Disableable as _, ElementExt as _, Icon, Sizable as _, Size, WindowExt as _,
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::ScrollableElement as _,
    switch::Switch,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Gaveta, Janela,
    abas_direita::AbaDireita,
    componentes::{
        Aba, Alerta, Anel, CabecalhoDeSecao, CartaoDeLista, Chip, Confirmacao, ControleSegmentado, Estado, EstadoVazio,
        Marca, PilulaDeEstado, Reflexo as _, Segmento, Selo, Simbolo, TamanhoBotao, TamanhoChip, Tom, TomConfirmacao,
        aviso, botao_fantasma, botao_icone, botao_primario, confirmar, contagem,
    },
    popover_issues::{barra_esqueleto, idade, instante, spinner, texto_de_fora},
    sem_arrastar, tempo_relativo,
};
use crate::{
    github::{ArquivoPr, Check, DetalhePr},
    issues::FonteIssue,
    mcp::Resposta,
    modelo::{PrAnterior, Tarefa},
    motor::{Carregando, EventoLinha, FonteDiff, Motor, Provedor, RodarDeNovo},
    remoto::EstadoRede,
    tema::{Cores, Lucide, Tipo as _, dentro_da_borda, esp, r, raio, tam},
};

/// Um link `vitral:` clicado no texto do assistente (a janela abre a aba PR ou a issue).
#[derive(Clone, PartialEq, Debug, serde::Deserialize, Action)]
#[action(namespace = vitral, no_json)]
pub struct AbrirLinkVitral(pub String);

/// Os PRs e as issues que os links vivos de um chat podem ligar: os das tarefas do projeto (no
/// Mestre, de todos), e o repositório do chat para "#12".
pub(super) fn referencias_do_chat(motor: &Motor, chat: &crate::modelo::Chat) -> Referencias {
    let mestre = chat.papel == crate::modelo::Papel::Mestre;
    let mut refs = Referencias::default();
    for t in motor.tarefas.iter().filter(|t| mestre || (!chat.projeto.is_empty() && t.projeto == chat.projeto)) {
        for m in prs_da_tarefa(t) {
            if !m.repositorio.is_empty() {
                refs.prs.push((m.repositorio.clone(), m.numero));
            }
        }
        if let Some(i) = &t.issue {
            match i.fonte {
                FonteIssue::Linear => refs.linear.push(i.chave.clone()),
                FonteIssue::Github => {
                    if let (Some(repo), Ok(n)) = (&i.repositorio, i.chave.trim_start_matches('#').parse::<u64>()) {
                        refs.issues_github.push((repo.clone(), n));
                    }
                }
            }
        }
    }
    if !mestre {
        let do_chat = chat.tarefa.and_then(|t| motor.tarefa(t)).and_then(|t| prs_da_tarefa(t).last().map(|m| m.repositorio.clone()));
        let da_pasta = || {
            motor.pasta_do_chat(chat).and_then(|p| motor.info_github(&p).valor().map(|i| i.repositorio.clone()))
        };
        refs.repositorio = do_chat.filter(|r| !r.is_empty()).or_else(da_pasta);
    }
    refs
}

/// Quantos eventos da linha do tempo aparecem antes de "Ver mais".
const EVENTOS: usize = 30;
/// Linhas de uma revisão ou comentário recolhido.
const LINHAS_RECOLHIDO: usize = 4;

/// As sub-abas do PR.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubAba {
    #[default]
    Visao,
    Arquivos,
    Checks,
}

/// O que a aba PR lembra enquanto o Vitral está aberto.
#[derive(Default)]
pub struct EstadoDoPainel {
    subaba: SubAba,
    /// O PR que a aba mostra em cada tarefa (o índice na lista dos anteriores com o atual no fim);
    /// sem nada, o último.
    vista: HashMap<u64, usize>,
    /// O título em edição: a tarefa e o campo (Enter grava, Esc desiste).
    editando: Option<(u64, Entity<InputState>)>,
    _edicao: Option<Subscription>,
    /// A linha do tempo inteira (passa de 30).
    tudo: bool,
    /// Os corpos abertos (revisões e comentários recolhidos em 4 linhas).
    abertos: HashSet<String>,
    rolagem: ScrollHandle,
    /// A tarefa cuja aba PR está à vista (o vigia consulta mais com ela).
    vigiada: Option<u64>,
}

/// Um PR da lista da aba (os anteriores e o atual).
struct Mostrado<'a> {
    repositorio: String,
    numero: u64,
    url: String,
    titulo: String,
    estado: EstadoPr,
    /// O atual, com o que o vigia acompanha; `None` num anterior.
    atual: Option<&'a Pr>,
    anterior: Option<&'a PrAnterior>,
}

/// "dono/nome" de `https://github.com/dono/nome/pull/12`.
fn repositorio_da_url(url: &str) -> Option<String> {
    let resto = url.trim().strip_prefix("https://github.com/")?;
    let mut partes = resto.split('/');
    let (dono, nome) = (partes.next()?, partes.next()?);
    (!dono.is_empty() && !nome.is_empty()).then(|| format!("{dono}/{nome}"))
}

/// Os PRs da tarefa na ordem da navegação: os anteriores (do mais antigo) e o atual no fim.
fn prs_da_tarefa(t: &Tarefa) -> Vec<Mostrado<'_>> {
    let repo_atual = t.pr.as_ref().map(|p| p.repositorio.clone());
    let mut lista: Vec<Mostrado> = t
        .prs_anteriores
        .iter()
        .map(|a| Mostrado {
            repositorio: repositorio_da_url(&a.url).or_else(|| repo_atual.clone()).unwrap_or_default(),
            numero: a.numero,
            url: a.url.clone(),
            titulo: a.titulo.clone(),
            estado: a.estado,
            atual: None,
            anterior: Some(a),
        })
        .collect();
    if let Some(p) = &t.pr {
        lista.push(Mostrado {
            repositorio: p.repositorio.clone(),
            numero: p.numero,
            url: p.url.clone(),
            titulo: p.titulo.clone(),
            estado: p.estado,
            atual: Some(p),
            anterior: None,
        });
    }
    lista
}

/// O ícone e a cor do estado do PR (o mesclado em violeta, a exceção documentada).
pub(super) fn icone_do_estado(estado: EstadoPr) -> (Lucide, Hsla) {
    match estado {
        EstadoPr::Aberto => (Lucide::GitPullRequest, Cores::verde()),
        EstadoPr::Rascunho => (Lucide::GitPullRequestDraft, Cores::tenue()),
        EstadoPr::Mesclado => (Lucide::GitMerge, Cores::violeta()),
        EstadoPr::Fechado => (Lucide::GitPullRequestClosed, Cores::vermelho()),
    }
}

/// A pílula do estado: aberto em verde, rascunho neutro, mesclado em violeta e fechado em vermelho.
pub(super) fn pilula_do_pr(estado: EstadoPr) -> PilulaDeEstado {
    match estado {
        EstadoPr::Aberto => PilulaDeEstado::new(Estado::Pronto).rotulo("aberto"),
        EstadoPr::Rascunho => PilulaDeEstado::new(Estado::Parado).rotulo("rascunho"),
        EstadoPr::Mesclado => PilulaDeEstado::new(Estado::Pronto).rotulo("mesclado").cor(Cores::violeta()),
        EstadoPr::Fechado => PilulaDeEstado::new(Estado::Erro).rotulo("fechado"),
    }
}

/// A cor dos checks: destaque pendente, verde passou, vermelho falhou.
pub(super) fn cor_dos_checks(c: &ResumoChecks) -> Hsla {
    match c.estado {
        EstadoChecks::Pendente => Cores::destaque(),
        EstadoChecks::Passou => Cores::verde(),
        EstadoChecks::Falhou => Cores::vermelho(),
        EstadoChecks::Nenhum => Cores::tenue(),
    }
}

/// O anel dos checks (só num PR aberto com checks).
pub(super) fn anel_dos_checks(id: impl Into<ElementId>, estado: EstadoPr, c: &ResumoChecks, diametro: f32) -> Option<Anel> {
    (matches!(estado, EstadoPr::Aberto | EstadoPr::Rascunho) && c.total > 0)
        .then(|| Anel::new(id, rotulo_checks(c).fracao).diametro(diametro).cor(cor_dos_checks(c)))
}

/// A dica do PR: "PR #12 · 15 de 41 checks terminaram", ou o estado.
pub(super) fn dica_do_pr(numero: u64, estado: EstadoPr, c: Option<&ResumoChecks>) -> String {
    match c.filter(|c| c.total > 0 && matches!(estado, EstadoPr::Aberto | EstadoPr::Rascunho)) {
        Some(c) => format!("PR #{numero} · {}", minuscula(&rotulo_checks(c).frase)),
        None => format!("PR #{numero} · {}", estado.nome()),
    }
}

fn minuscula(texto: &str) -> String {
    let mut letras = texto.chars();
    letras.next().map(|c| c.to_lowercase().chain(letras).collect()).unwrap_or_default()
}

/// O PR da tarefa que a aba e os chips mostram: o atual, senão o último anterior.
pub(super) fn pr_da_aba(t: &Tarefa) -> Option<(u64, EstadoPr, Option<&ResumoChecks>)> {
    match (&t.pr, t.prs_anteriores.last()) {
        (Some(p), _) => Some((p.numero, p.estado, Some(&p.checks))),
        (None, Some(a)) => Some((a.numero, a.estado, None)),
        _ => None,
    }
}

/// A aba "PR #12" da direita: o ícone do estado e, no aberto, o anel dos checks no fim; na
/// largura compacta, só o anel (ou o ícone). A dica dá o número e os checks.
pub(super) fn aba_do_pr(t: &Tarefa, compacta: bool) -> (Aba, SharedString) {
    let (numero, estado, checks) = pr_da_aba(t).unwrap_or((0, EstadoPr::Aberto, None));
    let (nome, cor) = icone_do_estado(estado);
    let anel = checks.and_then(|c| anel_dos_checks("anel-aba-pr", estado, c, 14.));
    let aba = Aba::new("aba-pr", format!("PR #{numero}"));
    let aba = match (compacta, anel) {
        (true, Some(anel)) => aba.so_simbolo(true).simbolo(Simbolo::Elemento(anel.into_any_element())),
        (true, None) => aba.so_simbolo(true).simbolo(Simbolo::icone_cor(nome, cor)),
        (false, Some(anel)) => aba.simbolo(Simbolo::icone_cor(nome, cor)).fim(anel),
        (false, None) => aba.simbolo(Simbolo::icone_cor(nome, cor)),
    };
    (aba, dica_do_pr(numero, estado, checks).into())
}

/// O `ChipDoPr`: o ícone do estado, o anel dos checks e "PR #12". O mesmo no cabeçalho da
/// conversa, no Distribuído e na Equipe; abre o chat na aba PR.
pub(super) fn chip_do_pr(id: impl Into<ElementId>, t: &Tarefa, tamanho: TamanhoChip, so_icone: bool) -> Option<Chip> {
    let (numero, estado, checks) = pr_da_aba(t)?;
    let id: ElementId = id.into();
    let (nome, cor) = icone_do_estado(estado);
    let anel = checks.and_then(|c| anel_dos_checks((id.clone(), "anel"), estado, c, 14.));
    let simbolo = match anel {
        Some(anel) if so_icone => Simbolo::Elemento(anel.into_any_element()),
        Some(anel) => Simbolo::Elemento(
            h_flex().flex_none().gap(esp::E4).child(Icon::new(nome).size(r(14.)).text_color(cor)).child(anel).into_any_element(),
        ),
        None => Simbolo::icone_cor(nome, cor),
    };
    Some(
        Chip::new(id, format!("PR #{numero}"))
            .simbolo(simbolo)
            .tamanho(tamanho)
            .so_icone(so_icone)
            .dica(format!("{}\nAbrir na aba PR", dica_do_pr(numero, estado, checks))),
    )
}

/// "4 min", "38 s": quanto um check levou, pelas datas ISO.
fn duracao_do_check(c: &Check) -> Option<String> {
    let (inicio, fim) = (instante(&c.inicio)?, instante(&c.fim)?);
    Some(crate::ui::componentes::duracao_curta(fim.checked_sub(inicio)?))
}

/// A ordem dos checks na lista: falhou, rodando, passou, pulado.
fn ordem_do_check(c: &Check) -> u8 {
    match c.balde.as_str() {
        "fail" => 0,
        "pending" => 1,
        "pass" => 2,
        "cancel" => 3,
        _ => 4,
    }
}

/// O ícone e a cor de um check pelo balde.
fn icone_do_check(c: &Check) -> (Lucide, Hsla) {
    match c.balde.as_str() {
        "fail" => (Lucide::CircleX, Cores::vermelho()),
        "pending" => (Lucide::LoaderCircle, Cores::destaque_texto()),
        "pass" => (Lucide::CircleCheck, Cores::verde()),
        "cancel" => (Lucide::CircleX, Cores::tenue()),
        _ => (Lucide::CircleDashed, Cores::tenue()),
    }
}

/// O ícone e a cor de um arquivo do PR pelo tipo da mudança.
fn icone_do_arquivo(a: &ArquivoPr) -> (Lucide, Hsla) {
    match a.tipo.as_str() {
        "ADDED" => (Lucide::FilePlus, Cores::verde()),
        "DELETED" => (Lucide::FileMinus, Cores::vermelho()),
        "RENAMED" | "COPIED" => (Lucide::FileSymlink, Cores::tenue()),
        _ => (Lucide::FilePen, Cores::tenue()),
    }
}

/// "+12 −3" com as cores.
fn mais_menos(mais: u64, menos: u64) -> AnyElement {
    h_flex()
        .flex_none()
        .gap(esp::E4)
        .mono_pequeno()
        .child(div().text_color(Cores::verde()).child(format!("+{mais}")))
        .child(div().text_color(Cores::vermelho()).child(format!("−{menos}")))
        .into_any_element()
}

/// Um selo com a explicação na dica.
fn selo_com_dica(id: impl Into<SharedString>, selo: Selo, dica: impl Into<SharedString>) -> AnyElement {
    let dica = dica.into();
    div()
        .id(ElementId::Name(id.into()))
        .flex_none()
        .child(selo)
        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        .into_any_element()
}

/// O aviso de uma resposta do motor (pronto ou erro).
fn avisar(resposta: &Resposta, window: &mut Window, cx: &mut App) {
    let tom = if resposta.erro { Tom::Erro } else { Tom::Pronto };
    let texto = resposta.texto.lines().next().unwrap_or_default().to_string();
    if !texto.is_empty() {
        window.push_notification(aviso(tom, texto), cx);
    }
}

/// Espera a resposta de uma ação e avisa.
pub(super) fn responder(recebe: smol::channel::Receiver<Resposta>, window: &mut Window, cx: &mut App) {
    window
        .spawn(cx, async move |cx| {
            let Ok(resposta) = recebe.recv().await else {
                return;
            };
            let _ = cx.update(|window, cx| avisar(&resposta, window, cx));
        })
        .detach();
}

/// Uma linha de corpo de texto de terceiros recolhida em 4 linhas, com "Ver tudo".
fn corpo_recolhido(id: String, texto: &str, aberto: bool, alternar: impl Fn(&mut Window, &mut App) + 'static, window: &Window, cx: &App) -> AnyElement {
    let longo = texto.lines().count() > LINHAS_RECOLHIDO || texto.len() > 320;
    v_flex()
        .gap(esp::E2)
        .child(texto_de_fora(SharedString::from(id.clone()), texto, (!aberto).then_some(LINHAS_RECOLHIDO), cx))
        .when(longo, |s| {
            s.child(
                h_flex().ml(r(-8.)).child(
                    botao_fantasma(SharedString::from(format!("{id}-ver")), None, if aberto { "Recolher" } else { "Ver tudo" }, TamanhoBotao::Linha, window, cx)
                        .on_click(move |_, window, cx| alternar(window, cx)),
                ),
            )
        })
        .into_any_element()
}

impl Janela {
    /// O chip do PR no cabeçalho de um chat de projeto (só o anel na largura estreita); abre a aba
    /// PR.
    pub(super) fn chip_do_pr_no_cabecalho(
        &self,
        motor: &Motor,
        chat: &crate::modelo::Chat,
        painel_id: u64,
        nivel: super::componentes::LarguraPainel,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        use super::componentes::LarguraPainel;
        if chat.papel != crate::modelo::Papel::Projeto || nivel == LarguraPainel::Minimo {
            return None;
        }
        let t = chat.tarefa.and_then(|t| motor.tarefa(t))?;
        // Só o anel abaixo de 520: o título da conversa e o chip da issue também precisam do lugar.
        let so_icone = nivel != LarguraPainel::Largo;
        let alvo = chat.id.clone();
        let chip = chip_do_pr(("chip-pr", painel_id as usize), t, TamanhoChip::Medio, so_icone)?
            .on_click(cx.listener(move |this, _, window, cx| this.abrir_aba_pr(&alvo, window, cx)))
            .construir(window, cx);
        Some(sem_arrastar().flex_none().child(chip).into_any_element())
    }

    /// Um link `vitral:` do texto do assistente: o PR abre na aba PR do chat dele (no PR certo,
    /// mesmo anterior); a issue do próprio chat abre no popover; as outras, no navegador.
    pub(super) fn abrir_link_vitral(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else { return };
        match ler_link_vitral(url) {
            Some(LinkVitral::Pr { repositorio, numero }) => {
                let achado = {
                    let m = motor.read(cx);
                    m.tarefas.iter().find_map(|t| {
                        let i = prs_da_tarefa(t).iter().position(|p| p.numero == numero && p.repositorio.eq_ignore_ascii_case(&repositorio))?;
                        let chat = m.chats.iter().find(|c| c.tarefa == Some(t.id))?;
                        Some((chat.id.clone(), t.id, i))
                    })
                };
                match achado {
                    Some((chat, tarefa, i)) => {
                        self.pr.vista.insert(tarefa, i);
                        self.pr.subaba = SubAba::Visao;
                        self.abrir_aba_pr(&chat, window, cx);
                    }
                    None => cx.open_url(&format!("https://github.com/{repositorio}/pull/{numero}")),
                }
            }
            Some(LinkVitral::IssueLinear(chave)) => self.abrir_issue_do_link(&motor, |i| i.fonte == FonteIssue::Linear && i.chave == chave, window, cx),
            Some(LinkVitral::IssueGithub { repositorio, numero }) => {
                let chave = format!("#{numero}");
                self.abrir_issue_do_link(
                    &motor,
                    |i| i.fonte == FonteIssue::Github && i.chave == chave && i.repositorio.as_deref().is_some_and(|r| r.eq_ignore_ascii_case(&repositorio)),
                    window,
                    cx,
                )
            }
            None => {}
        }
    }

    /// A issue de um link vivo: a do chat do painel ativo abre no popover dele; outra, no navegador.
    fn abrir_issue_do_link(
        &mut self,
        motor: &Entity<Motor>,
        e_ela: impl Fn(&crate::issues::IssueLigada) -> bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(painel) = self.paineis.get(self.ativo).map(|p| (p.id, p.chat.clone())) else { return };
        let (do_chat, url) = {
            let m = motor.read(cx);
            let ligada = |t: &Tarefa| t.issue.as_ref().filter(|i| e_ela(i)).cloned();
            let do_chat = painel.1.as_deref().and_then(|c| m.chat(c)).and_then(|c| c.tarefa).and_then(|t| m.tarefa(t)).and_then(ligada);
            let qualquer = do_chat.clone().or_else(|| m.tarefas.iter().find_map(ligada));
            (do_chat.is_some(), qualquer.map(|i| i.url))
        };
        if do_chat {
            self.abrir_issues(painel.0, super::popover_issues::Gatilho::Chip, window, cx);
        } else if let Some(url) = url.filter(|u| link_permitido(u, OrigemDoTexto::Terceiros)) {
            cx.open_url(&url);
        }
    }

    /// A tarefa cuja aba PR está à vista agora (a direita ao lado ou na gaveta, na aba PR).
    pub(super) fn pr_a_vista(&self, motor: &Motor) -> Option<u64> {
        let a_vista = self.direita_visivel() || self.gaveta == Some(Gaveta::Direita);
        if !a_vista || self.aba_visivel(motor) != AbaDireita::Pr {
            return None;
        }
        self.selecionado.as_ref().and_then(|id| motor.chat(id)).and_then(|c| c.tarefa)
    }

    /// Conta ao motor quando a aba PR aparece ou some (o vigia consulta mais com ela à vista).
    /// Depois do desenho, para o motor não mudar no meio dele.
    pub(super) fn vigiar_pr_a_vista(&mut self, agora: Option<u64>, cx: &mut Context<Self>) {
        let antes = self.pr.vigiada;
        if antes == agora {
            return;
        }
        self.pr.vigiada = agora;
        let Some(motor) = self.motor.clone() else { return };
        cx.defer(move |cx| {
            motor.update(cx, |m, cx| {
                if let Some(id) = antes {
                    m.vigiar_aba_pr(id, false, cx);
                }
                if let Some(id) = agora {
                    m.vigiar_aba_pr(id, true, cx);
                }
            });
        });
    }

    /// A aba PR inteira: a faixa das abas, o cabeçalho do PR, as sub-abas, o conteúdo que rola e o
    /// rodapé.
    pub(super) fn painel_do_pr(&mut self, motor_e: &Entity<Motor>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let motor = motor_e.read(cx);
        let na_gaveta = self.gaveta == Some(Gaveta::Direita);
        let rolado = self.pr.rolagem.offset().y < px(0.);
        let barra = self.barra_de_abas(motor, AbaDireita::Pr, rolado, "arrastar-pr", window, cx);
        let tarefa = self.selecionado.as_ref().and_then(|id| motor.chat(id)).and_then(|c| c.tarefa).and_then(|t| motor.tarefa(t));
        let corpo = match tarefa {
            Some(t) => self.corpo_do_pr(motor, t, window, cx),
            None => div().into_any_element(),
        };
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .rounded(raio::PECA)
            .when(!na_gaveta, |s| s.bg(Cores::cartao()).reflexo(raio::PECA))
            .overflow_hidden()
            .child(barra)
            .child(corpo)
            .on_prepaint(self.medir_direita(cx))
            .into_any_element()
    }

    fn corpo_do_pr(&self, motor: &Motor, t: &Tarefa, window: &Window, cx: &Context<Self>) -> AnyElement {
        let lista = prs_da_tarefa(t);
        let Some(ultimo) = lista.len().checked_sub(1) else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(EstadoVazio::new(Lucide::GitPullRequest, "Este chat não tem PR"))
                .into_any_element();
        };
        let indice = self.pr.vista.get(&t.id).copied().filter(|i| *i <= ultimo).unwrap_or(ultimo);
        let m = &lista[indice];
        let detalhe = motor.detalhe_pr(&m.repositorio, m.numero);
        let cabecalho = self.cabecalho_do_pr(motor, t, &lista, indice, &detalhe, window, cx);
        let checks = motor.checks_do_pr(&m.repositorio, m.numero);
        let n_arquivos = detalhe.valor().map(|d| d.arquivos.len().max(d.arquivos_mudados as usize));
        let subabas = self.subabas(m, n_arquivos, &checks, cx);
        let conteudo: Vec<AnyElement> = match self.pr.subaba {
            SubAba::Visao => self.visao_geral(motor, t, m, &detalhe, window, cx),
            SubAba::Arquivos => self.arquivos_do_pr(motor, t, m, &detalhe, window, cx),
            SubAba::Checks => self.lista_de_checks(t, m, &checks, window, cx),
        };
        let rodape = self.rodape_do_pr(motor, t, m, window, cx);
        v_flex()
            .flex_1()
            .min_h_0()
            .child(v_flex().flex_none().px(esp::E12).pt(esp::E4).gap(esp::E8).child(cabecalho).child(subabas))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("pr-conteudo")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.pr.rolagem)
                            .px(esp::E12)
                            .child(v_flex().pt(esp::E4).pb(esp::E16).children(conteudo)),
                    )
                    .vertical_scrollbar(&self.pr.rolagem),
            )
            .children(rodape)
            .into_any_element()
    }

    /// O cabeçalho do PR: a primeira linha (estado, número, link, os anteriores, a pílula, ⟳ e o ⋯),
    /// o título e os chips com os selos.
    #[allow(clippy::too_many_arguments)]
    fn cabecalho_do_pr(
        &self,
        motor: &Motor,
        t: &Tarefa,
        lista: &[Mostrado],
        indice: usize,
        detalhe: &Carregando<DetalhePr>,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let m = &lista[indice];
        let tarefa = t.id;
        let det = detalhe.valor();
        let (nome, cor) = icone_do_estado(m.estado);
        let link = link_permitido(&m.url, OrigemDoTexto::Terceiros).then(|| {
            let url = m.url.clone();
            botao_icone("pr-abrir-fora", Lucide::ExternalLink, "Abrir no GitHub", TamanhoBotao::Linha, window, cx)
                .on_click(move |_, _, cx| cx.open_url(&url))
        });
        let navegar = (lista.len() > 1).then(|| {
            let ir = |id: &'static str, icone: Lucide, dica: &'static str, para: Option<usize>| {
                let b = botao_icone(id, icone, dica, TamanhoBotao::Linha, window, cx);
                match para {
                    Some(i) => b.on_click(cx.listener(move |this, _, _, cx| {
                        this.pr.vista.insert(tarefa, i);
                        this.pr.editando = None;
                        cx.notify();
                    })),
                    None => b.disabled(true),
                }
            };
            h_flex()
                .flex_none()
                .child(ir("pr-anterior", Lucide::ChevronLeft, "PR anterior", indice.checked_sub(1)))
                .child(div().metadado().text_color(Cores::tenue()).child(format!("{}/{}", indice + 1, lista.len())))
                .child(ir("pr-seguinte", Lucide::ChevronRight, "PR seguinte", (indice + 1 < lista.len()).then_some(indice + 1)))
        });
        let atualizar = {
            let (repo, n) = (m.repositorio.clone(), m.numero);
            let motor_e = self.motor.clone();
            botao_icone("pr-atualizar", Lucide::RefreshCw, "Atualizar", TamanhoBotao::Linha, window, cx).on_click(move |_, _, cx| {
                if let Some(motor) = &motor_e {
                    motor.update(cx, |m, cx| m.atualizar_pr(tarefa, &repo, n, cx));
                }
            })
        };
        let primeira = h_flex()
            .h(r(28.))
            .gap(esp::E6)
            .child(Icon::new(nome).size(r(16.)).flex_none().text_color(cor))
            .child(div().flex_none().rotulo().text_color(Cores::forte()).child(format!("#{}", m.numero)))
            .children(link)
            .children(navegar)
            .child(div().flex_1())
            .when(detalhe.carregando(), |s| s.child(spinner(12., window)))
            .child(pilula_do_pr(m.estado))
            .child(atualizar)
            .child(self.menu_do_pr(motor, t, m, window, cx));

        let titulo = det.map(|d| d.titulo.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| m.titulo.clone());
        let editando = self.pr.editando.as_ref().filter(|(id, _)| *id == tarefa && m.atual.is_some()).map(|(_, e)| e.clone());
        let linha_titulo: AnyElement = match editando {
            Some(campo) => div()
                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                    if ev.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.pr.editando = None;
                        this.pr._edicao = None;
                        cx.notify();
                    }
                }))
                .child(Input::new(&campo).with_size(Size::Medium))
                .into_any_element(),
            None => {
                let editavel = m.atual.is_some_and(Pr::aberto);
                let texto = titulo.clone();
                div()
                    .id("pr-titulo")
                    .titulo_cartao()
                    .text_color(Cores::forte())
                    .line_clamp(2)
                    .text_ellipsis()
                    .child(titulo)
                    .when(editavel, |s| {
                        s.cursor_text()
                            .tooltip(|window, cx| Tooltip::new("Clique duas vezes para mudar o título").build(window, cx))
                            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                                if ev.click_count() >= 2 {
                                    this.editar_titulo(tarefa, texto.clone(), window, cx);
                                }
                            }))
                    })
                    .into_any_element()
            }
        };

        // Os chips (o ramo, a issue) e os selos.
        let base = det.map(|d| d.base.clone()).filter(|b| !b.is_empty()).or_else(|| m.atual.map(|p| p.base.clone())).unwrap_or_default();
        let ramo = det.map(|d| d.ramo.clone()).filter(|b| !b.is_empty()).or_else(|| m.atual.map(|p| p.ramo.clone())).unwrap_or_default();
        let mut chips: Vec<AnyElement> = Vec::new();
        if !ramo.is_empty() {
            let rotulo = if base.is_empty() { ramo.clone() } else { format!("{base} ← {ramo}") };
            let copia = ramo.clone();
            chips.push(
                Chip::new("pr-ramo", rotulo)
                    .simbolo(Simbolo::icone(Lucide::GitBranch))
                    .tamanho(TamanhoChip::Pequeno)
                    .dica(format!("{ramo} → {base}\nClique para copiar o nome do ramo"))
                    .on_click(move |_, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copia.clone()));
                        window.push_notification(aviso(Tom::Pronto, "Nome do ramo copiado."), cx);
                    })
                    .construir(window, cx)
                    .ml(r(-8.))
                    .max_w(relative(1.))
                    .into_any_element(),
            );
        }
        if let Some(issue) = &t.issue {
            let simbolo = match issue.fonte {
                FonteIssue::Linear => Simbolo::Marca(Marca::Linear),
                FonteIssue::Github => Simbolo::icone_cor(Lucide::CircleDot, Cores::verde()),
            };
            let url = issue.url.clone();
            chips.push(
                Chip::new("pr-issue", issue.chave.clone())
                    .simbolo(simbolo)
                    .tamanho(TamanhoChip::Pequeno)
                    .dica(format!("{} · {}\nAbrir a issue", issue.chave, issue.titulo))
                    .on_click(move |_, _, cx| {
                        if link_permitido(&url, OrigemDoTexto::Terceiros) {
                            cx.open_url(&url);
                        }
                    })
                    .construir(window, cx)
                    .into_any_element(),
            );
        }
        if let Some(pr) = m.atual {
            let base = nome_da_base(pr).to_string();
            let simulacao = motor.simulacao(tarefa);
            match pr.decisao.as_str() {
                "CHANGES_REQUESTED" => chips.push(selo_com_dica("pr-selo-mudancas", Selo::cor("mudanças pedidas", Cores::ambar()), "Uma revisão pediu mudanças")),
                "APPROVED" => chips.push(selo_com_dica("pr-selo-aprovado", Selo::cor("aprovado", Cores::verde()), "O PR foi aprovado")),
                _ => {}
            }
            if pr.conversas_abertas > 0 {
                let n = pr.conversas_abertas as usize;
                chips.push(selo_com_dica("pr-selo-conversas", Selo::neutro(contagem(n, "conversa aberta", "conversas abertas")), "Conversas de revisão sem resolver"));
            }
            if tem_conflito(pr, simulacao.as_ref()) {
                let arquivos = simulacao.as_ref().and_then(|s| s.as_ref().ok()).filter(|a| !a.is_empty()).map(|a| a.join(", "));
                let dica = match arquivos {
                    Some(a) => format!("Conflito com {base} em {a}"),
                    None => format!("O GitHub diz que o PR tem conflito com {base}"),
                };
                chips.push(selo_com_dica("pr-selo-conflito", Selo::cor("conflito", Cores::vermelho()), dica));
            } else if pr.mesclavel == "BEHIND" {
                chips.push(selo_com_dica("pr-selo-atras", Selo::cor(format!("atrás de {base}"), Cores::ambar()), format!("{base} andou depois do PR: atualize antes de mesclar")));
            }
            if !pr.commits_de_fora.is_empty() {
                let dica = format!("Commits que não são do Vitral: {}\nO Vitral não mescla sozinho com eles", pr.commits_de_fora.join(", "));
                chips.push(selo_com_dica("pr-selo-de-fora", Selo::cor("commit de fora", Cores::ambar()), dica));
            }
            if !pr.acompanhando && pr.aberto() {
                chips.push(selo_com_dica("pr-selo-parado", Selo::neutro("sem acompanhar"), "O Vitral não avisa nada deste PR (só confere ao abrir)"));
            }
        }
        v_flex()
            .gap(esp::E4)
            .child(primeira)
            .child(linha_titulo)
            .when(!chips.is_empty(), |s| s.child(h_flex().flex_wrap().items_center().gap(esp::E6).pt(esp::E2).children(chips)))
            .into_any_element()
    }

    /// As sub-abas: Visão geral, Arquivos (com o número) e Checks (com o rótulo e o ponto da cor).
    fn subabas(&self, m: &Mostrado, n_arquivos: Option<usize>, checks: &Carregando<Vec<Check>>, cx: &Context<Self>) -> AnyElement {
        let resumo = match m.atual {
            Some(pr) if pr.checks.total > 0 => Some(pr.checks.clone()),
            _ => checks.valor().filter(|c| !c.is_empty()).map(|c| ResumoChecks::da_lista(c)),
        };
        let arquivos = match n_arquivos {
            Some(n) => Segmento::new(format!("Arquivos {n}")),
            None => Segmento::new("Arquivos"),
        };
        let checks = match &resumo {
            Some(c) => Segmento::new(format!("Checks {}", rotulo_checks(c).curto)).ponto(cor_dos_checks(c)).dica(rotulo_checks(c).frase),
            None => Segmento::new("Checks"),
        };
        let atual = match self.pr.subaba {
            SubAba::Visao => 0,
            SubAba::Arquivos => 1,
            SubAba::Checks => 2,
        };
        let janela = cx.entity().downgrade();
        ControleSegmentado::new("pr-subabas", [Segmento::new("Visão geral").icone(Lucide::Eye), arquivos, checks], atual, move |novo, _, cx| {
            let _ = janela.update(cx, |this, cx| {
                this.pr.subaba = match novo {
                    1 => SubAba::Arquivos,
                    2 => SubAba::Checks,
                    _ => SubAba::Visao,
                };
                this.pr.rolagem.set_offset(point(px(0.), px(0.)));
                cx.notify();
            });
        })
        .with_size(Size::Small)
        .compacto(self.direita_compacta)
        .largura_total(true)
        .into_any_element()
    }

    /// Começa a editar o título (duplo clique): Enter grava, Esc desiste.
    fn editar_titulo(&mut self, tarefa: u64, atual: String, window: &mut Window, cx: &mut Context<Self>) {
        let campo = cx.new(|cx| InputState::new(window, cx).default_value(atual));
        let assinatura = cx.subscribe_in(&campo, window, move |this, campo, evento: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = evento {
                let titulo = campo.read(cx).value().trim().to_string();
                this.pr.editando = None;
                this.pr._edicao = None;
                cx.notify();
                if titulo.is_empty() {
                    return;
                }
                let Some(motor) = this.motor.clone() else { return };
                let recebe = motor.update(cx, |m, cx| m.editar_titulo_pr(tarefa, titulo, cx));
                responder(recebe, window, cx);
            }
        });
        campo.update(cx, |c, cx| c.focus(window, cx));
        self.pr.editando = Some((tarefa, campo));
        self.pr._edicao = Some(assinatura);
        cx.notify();
    }
}


/// O que uma linha de "Esperando você" oferece.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcaoDaEspera {
    MandarFalha,
    /// "Mandar ao chat" dos comentários da revisão (ou de quem não tem acesso).
    MandarComentarios(Vec<u64>),
    Mesclar,
    AtualizarPr,
    /// Abre a aba PR.
    Ver,
    /// Abre o chat no cartão "Confirmar".
    Confirmar(u64),
    /// "Destravar e tentar": os efeitos da issue que ficaram sem a chave.
    Destravar,
}

/// O que o PR da tarefa espera de você (B21): só o que pede uma ação. Os itens do PR são os que o
/// motor deixou em `Pr.esperando` (ele põe e tira com os eventos do vigia: no "Mesclar quando ficar
/// pronto", a falha e a revisão vão sozinhas ao chat; no "Watch", viram item aqui), conferidos com o
/// PR de agora; a rodada pronta para atualizar e a issue que não deu para mover vêm da tarefa. O
/// último campo é o item do motor, que o × tira da lista.
#[allow(clippy::type_complexity)]
pub fn esperas_da_tarefa(
    t: &Tarefa,
    agora: u64,
    pronto_para_mesclar: bool,
) -> Vec<(String, super::componentes::Sinal, AcaoDaEspera, Option<crate::modelo::EsperaPr>)> {
    use super::componentes::Sinal;
    use crate::modelo::EsperaPr;
    let mut lista = Vec::new();
    if let Some(p) = &t.efeitos_issue
        && !p.feitos
        && let Some(motivo) = &p.pendente
        && let Some(issue) = &t.issue
    {
        lista.push((format!("Não consegui mover a {} ({motivo})", issue.chave), Sinal::Pedido, AcaoDaEspera::Destravar, None));
    }
    let Some(pr) = t.pr.as_ref().filter(|p| p.aberto()) else {
        return lista;
    };
    let n = pr.numero;
    if t.pronto_para_atualizar {
        lista.push((format!("Pronto para atualizar o PR #{n}"), Sinal::Pedido, AcaoDaEspera::AtualizarPr, None));
    }
    let arroba = |autores: &[String]| autores.iter().map(|a| format!("@{a}")).collect::<Vec<_>>().join(", ");
    for item in &pr.esperando {
        let (texto, sinal, acao) = match item {
            // Com a rodada que corrige pronta, a falha já foi tratada: só o Atualizar PR.
            EsperaPr::CiFalhou { .. } if t.pronto_para_atualizar || pr.checks.estado != EstadoChecks::Falhou => continue,
            EsperaPr::CiFalhou { .. } => (format!("CI falhou no PR #{n}"), Sinal::Erro, AcaoDaEspera::MandarFalha),
            EsperaPr::Revisao { ids, autores } if ids.is_empty() => {
                (format!("Revisão no PR #{n}: mudanças pedidas por {}", arroba(autores)), Sinal::Pedido, AcaoDaEspera::Ver)
            }
            EsperaPr::Revisao { ids, autores } => {
                let quantos = if ids.len() == 1 { "1 comentário".to_string() } else { format!("{} comentários", ids.len()) };
                (format!("Revisão no PR #{n} ({quantos} de {})", arroba(autores)), Sinal::Pedido, AcaoDaEspera::MandarComentarios(ids.clone()))
            }
            EsperaPr::Terceiro { id, autor, .. } => (
                format!("Comentário de @{autor} (sem acesso ao repositório) no PR #{n}"),
                Sinal::Pedido,
                AcaoDaEspera::MandarComentarios(vec![*id]),
            ),
            // O pronto de quando o vigia viu, se ainda vale com o PR de agora.
            EsperaPr::ProntoParaMesclar if !pronto_para_mesclar => continue,
            EsperaPr::ProntoParaMesclar => (format!("PR #{n} pronto para mesclar"), Sinal::Pedido, AcaoDaEspera::Mesclar),
            EsperaPr::CommitDeFora { .. } if pr.commits_de_fora.is_empty() => continue,
            EsperaPr::CommitDeFora { .. } => (format!("Chegou commit de fora no PR #{n}"), Sinal::Pedido, AcaoDaEspera::Ver),
            EsperaPr::ChecksParados if !pr.acompanhando || pr.checks.pendentes == 0 || pr.checks_progresso_em == 0 => continue,
            EsperaPr::ChecksParados => {
                let horas = agora.saturating_sub(pr.checks_progresso_em) / 3600;
                (format!("Checks parados há {} h no PR #{n}", horas.max(2)), Sinal::Parada, AcaoDaEspera::Ver)
            }
            EsperaPr::MesclaRecusada { motivo } => {
                (format!("O GitHub não aceitou mesclar o PR #{n}: {motivo}"), Sinal::Erro, AcaoDaEspera::Ver)
            }
            EsperaPr::Desconhecido(_) => continue,
        };
        lista.push((texto, sinal, acao, Some(item.clone())));
    }
    lista
}

/// Uma linha do PR em "Esperando você".
pub(super) struct EsperaDoPr {
    pub chave: String,
    pub chat: String,
    pub tarefa: u64,
    pub projeto: String,
    pub titulo: String,
    pub texto: String,
    pub sinal: super::componentes::Sinal,
    pub acao: AcaoDaEspera,
    /// O item do motor (`Pr.esperando`), que o × tira.
    pub item: Option<crate::modelo::EsperaPr>,
}

/// O que os PRs (e os "Confirmar" dos chats) esperam de você, num projeto ou em todos: uma linha
/// por chat, com o mais urgente (o "Confirmar" primeiro) e "(+N)" quando há mais.
pub(super) fn esperas_do_pr(motor: &Motor, projeto: Option<&str>) -> Vec<EsperaDoPr> {
    // As de um chat vêm juntas, na ordem dos chats; o "Confirmar" passa na frente das outras dele.
    let mut por_chat: Vec<EsperaDoPr> = Vec::new();
    let mut quantas: Vec<usize> = Vec::new();
    for e in esperas_soltas(motor, projeto) {
        match por_chat.iter().position(|p| p.chat == e.chat) {
            Some(i) => {
                quantas[i] += 1;
                if matches!(e.acao, AcaoDaEspera::Confirmar(_)) {
                    por_chat[i] = e;
                }
            }
            None => {
                por_chat.push(e);
                quantas.push(1);
            }
        }
    }
    for (e, n) in por_chat.iter_mut().zip(quantas) {
        if n > 1 {
            e.texto.push_str(&format!(" (+{})", n - 1));
        }
    }
    por_chat
}

/// Todas as esperas dos PRs, uma por motivo.
fn esperas_soltas(motor: &Motor, projeto: Option<&str>) -> Vec<EsperaDoPr> {
    let agora = crate::modelo::agora();
    let mut lista = Vec::new();
    for chat in motor.chats.iter().filter(|c| c.papel == crate::modelo::Papel::Projeto && !motor.arquivada(c)) {
        if projeto.is_some_and(|p| p != chat.projeto) {
            continue;
        }
        let Some(t) = chat.tarefa.and_then(|t| motor.tarefa(t)) else { continue };
        let pronto = t.pr.as_ref().is_some_and(|pr| {
            let acoes = motor.acoes_do_pr(t.id);
            falta_para_mesclar(pr, motor.simulacao(t.id).as_ref()).is_empty() && resultado(&acoes, &acao_mesclar_padrao(&acoes)).is_ok()
        });
        for (i, (texto, sinal, acao, item)) in esperas_da_tarefa(t, agora, pronto).into_iter().enumerate() {
            lista.push(EsperaDoPr {
                chave: format!("pr-{}-{i}", t.id),
                chat: chat.id.clone(),
                tarefa: t.id,
                projeto: chat.projeto.clone(),
                titulo: chat.titulo.clone(),
                texto,
                sinal,
                acao,
                item,
            });
        }
        for c in motor.cartoes().iter().filter(|c| c.aberto() && c.chat == chat.id) {
            if let crate::modelo::TipoCartao::Confirmar(conf) = &c.tipo {
                lista.push(EsperaDoPr {
                    chave: format!("confirmar-{}", c.id),
                    chat: chat.id.clone(),
                    tarefa: t.id,
                    projeto: chat.projeto.clone(),
                    titulo: chat.titulo.clone(),
                    texto: format!("Confirmar: {}", super::cartoes::acao_da_confirmacao(motor, conf)),
                    sinal: super::componentes::Sinal::Pedido,
                    acao: AcaoDaEspera::Confirmar(c.id),
                    item: None,
                });
            }
        }
    }
    lista
}

/// A mensagem de conflito que vai à caixa do chat ("Pedir ao chat"), a mesma da seção 6.2.
pub fn texto_do_conflito(base_remota: &str, arquivos: &[String]) -> String {
    let onde = if arquivos.is_empty() { String::new() } else { format!(" (arquivos {})", arquivos.join(", ")) };
    format!(
        "Deu conflito com o que entrou em {base_remota}{onde}. Rode `git merge {base_remota}` na sua cópia, resolva mantendo as duas mudanças, faça commit e rode os testes; depois avise."
    )
}

impl Janela {
    /// O ⋯ do PR: copiar, rascunho, atualizar com a base, os outros métodos de mesclar,
    /// acompanhar e fechar ou reabrir (só no atual).
    fn menu_do_pr(&self, motor: &Motor, t: &Tarefa, m: &Mostrado, window: &Window, cx: &Context<Self>) -> AnyElement {
        let tarefa = t.id;
        let janela = cx.entity().downgrade();
        let (url, numero) = (m.url.clone(), m.numero);
        let atual = m.atual.cloned();
        let acoes = if atual.is_some() { motor.acoes_do_pr(tarefa) } else { Vec::new() };
        let motor_e = self.motor.clone();
        let ultimo = t.prs_anteriores.last().filter(|a| a.numero == numero);
        let recomecar = t.pr.is_none() && ultimo.is_some_and(|a| a.estado == EstadoPr::Mesclado) && t.worktree.is_some();
        let base_anterior = t.partir_de.clone().unwrap_or_else(|| "origin/main".into());
        let acao_sem_pr = {
            let motor_e = motor_e.clone();
            move |f: fn(&mut Motor, u64, &mut Context<Motor>) -> smol::channel::Receiver<Resposta>| {
                let motor_e = motor_e.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    if let Some(motor) = &motor_e {
                        let recebe = motor.update(cx, |m, cx| f(m, tarefa, cx));
                        responder(recebe, window, cx);
                    }
                }
            }
        };
        botao_icone("pr-mais", Lucide::Ellipsis, "Mais opções do PR", TamanhoBotao::Linha, window, cx)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, window, _| {
                menu = menu
                    .item(PopupMenuItem::new("Copiar link").icon(Lucide::Link).disabled(url.is_empty()).on_click({
                        let url = url.clone();
                        move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(url.clone()))
                    }))
                    .item(PopupMenuItem::new(format!("Copiar “#{numero}”")).icon(Lucide::Copy).on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(format!("#{numero}")))
                    }));
                let Some(pr) = atual.clone() else {
                    // O último PR mesclado com commits que a cópia não tem: ela ficou como estava, e o
                    // aviso do chat manda recomeçar daqui.
                    if recomecar {
                        menu = menu.separator().item(
                            PopupMenuItem::new(format!("Recomeçar a partir de {base_anterior}"))
                                .icon(Lucide::RotateCcw)
                                .on_click(acao_sem_pr(|m, id, cx| m.recomecar_do_main(id, cx))),
                        );
                    }
                    return menu.min_w(tam(window, 240.));
                };
                let pode = |a: &AcaoPr| resultado(&acoes, a).is_ok();
                let acao = |f: fn(&mut Motor, u64, &mut Context<Motor>) -> smol::channel::Receiver<Resposta>| {
                    let motor_e = motor_e.clone();
                    move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                        if let Some(motor) = &motor_e {
                            let recebe = motor.update(cx, |m, cx| f(m, tarefa, cx));
                            responder(recebe, window, cx);
                        }
                    }
                };
                let base = nome_da_base(&pr).to_string();
                menu = menu.separator();
                menu = if pr.estado == EstadoPr::Rascunho {
                    menu.item(
                        PopupMenuItem::new("Marcar como pronto para revisão")
                            .icon(Lucide::GitPullRequest)
                            .disabled(!pode(&AcaoPr::ProntoParaRevisao))
                            .on_click(acao(|m, id, cx| m.rascunho(id, false, cx))),
                    )
                } else {
                    menu.item(
                        PopupMenuItem::new("Voltar para rascunho")
                            .icon(Lucide::GitPullRequestDraft)
                            .disabled(!pode(&AcaoPr::VoltarParaRascunho))
                            .on_click(acao(|m, id, cx| m.rascunho(id, true, cx))),
                    )
                };
                menu = menu.item(
                    PopupMenuItem::new(format!("Atualizar com {base}"))
                        .icon(Lucide::GitBranch)
                        .disabled(!pode(&AcaoPr::AtualizarComBase))
                        .on_click(acao(|m, id, cx| m.atualizar_com_base(id, cx))),
                );
                if !pr.commits_de_fora.is_empty() {
                    menu = menu.item(
                        PopupMenuItem::new("Trazer as mudanças do GitHub")
                            .icon(Lucide::Download)
                            .on_click(acao(|m, id, cx| m.trazer_do_github(id, cx))),
                    );
                }
                // Os métodos que não são o padrão (o padrão é o botão do rodapé).
                let outros: Vec<(String, bool)> = acoes
                    .iter()
                    .filter_map(|(a, r)| match a {
                        AcaoPr::Mesclar { metodo } => Some((metodo.clone(), r.is_ok())),
                        _ => None,
                    })
                    .skip(1)
                    .collect();
                for (metodo, ok) in outros {
                    let rotulo = match metodo.as_str() {
                        "merge" => "Mesclar com commit de merge".to_string(),
                        "rebase" => "Mesclar com rebase".to_string(),
                        outro => format!("Mesclar com {outro}"),
                    };
                    let janela = janela.clone();
                    menu = menu.item(PopupMenuItem::new(rotulo).icon(Lucide::GitMerge).disabled(!ok).on_click(move |_, window, cx| {
                        let metodo = metodo.clone();
                        let _ = janela.update(cx, |this, cx| this.mesclar_pr(tarefa, Some(metodo), window, cx));
                    }));
                }
                menu = menu.separator();
                let acompanhando = pr.acompanhando;
                let motor_a = motor_e.clone();
                menu = menu.item(
                    PopupMenuItem::new(if acompanhando { "Parar de acompanhar" } else { "Acompanhar" })
                        .icon(if acompanhando { Lucide::BellOff } else { Lucide::Bell })
                        .on_click(move |_, _, cx| {
                            if let Some(motor) = &motor_a {
                                motor.update(cx, |m, cx| m.acompanhar(tarefa, !acompanhando, cx));
                            }
                        }),
                );
                if pr.estado == EstadoPr::Fechado {
                    menu = menu.item(
                        PopupMenuItem::new("Reabrir o PR")
                            .icon(Lucide::RotateCcw)
                            .disabled(!pode(&AcaoPr::Reabrir))
                            .on_click(acao(|m, id, cx| m.reabrir_pr(id, cx))),
                    );
                } else {
                    let janela = janela.clone();
                    menu = menu.item(
                        PopupMenuItem::new("Fechar o PR…")
                            .icon(Lucide::GitPullRequestClosed)
                            .disabled(!pode(&AcaoPr::Fechar))
                            .on_click(move |_, window, cx| {
                                let _ = janela.update(cx, |this, cx| this.confirmar_fechar_pr(tarefa, numero, window, cx));
                            }),
                    );
                }
                menu.min_w(tam(window, 240.))
            })
            .into_any_element()
    }

    /// A faixa fina de sem conexão ou de limite do GitHub (o que já veio continua na tela).
    fn faixa_da_rede_do_pr(&self, motor: &Motor, tarefa: u64, m: &Mostrado, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let texto = match motor.estado_rede(Provedor::Github) {
            EstadoRede::Ok => return None,
            EstadoRede::Fora { proxima_sonda, .. } => {
                let falta = proxima_sonda.saturating_duration_since(std::time::Instant::now()).as_secs();
                let quando = if falta < 60 { "em instantes".to_string() } else { format!("em {} min", falta.div_ceil(60)) };
                format!("Sem conexão com o GitHub · tentando de novo {quando}")
            }
            EstadoRede::Limite { ate } => {
                let falta = ate.saturating_duration_since(std::time::Instant::now());
                let hora = chrono::DateTime::<chrono::Local>::from(std::time::SystemTime::now() + falta).format("%H:%M");
                format!("Limite de pedidos do GitHub até {hora}; o Vitral espera sozinho")
            }
        };
        let fora = matches!(motor.estado_rede(Provedor::Github), EstadoRede::Fora { .. });
        let (motor_e, repo, n) = (self.motor.clone(), m.repositorio.clone(), m.numero);
        Some(
            h_flex()
                .gap(esp::E8)
                .pb(esp::E8)
                .child(
                    Chip::new("pr-rede", texto)
                        .simbolo(Simbolo::icone(Lucide::WifiOff))
                        .tamanho(TamanhoChip::Pequeno)
                        .alerta(Cores::ambar())
                        .construir(window, cx)
                        .flex_1()
                        .min_w_0(),
                )
                .when(fora, |s| {
                    s.child(botao_fantasma("pr-tentar-agora", None, "Tentar agora", TamanhoBotao::Linha, window, cx).on_click(move |_, _, cx| {
                        if let Some(motor) = &motor_e {
                            motor.update(cx, |m, cx| m.atualizar_pr(tarefa, &repo, n, cx));
                        }
                    }))
                })
                .into_any_element(),
        )
    }

    /// O PR mesclado ou fechado: quem e quando, e o que dá para fazer.
    fn estado_final(&self, motor: &Motor, t: &Tarefa, m: &Mostrado, ultimo: bool, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let tarefa = t.id;
        let abrir = link_permitido(&m.url, OrigemDoTexto::Terceiros).then(|| {
            let url = m.url.clone();
            botao_fantasma("pr-final-abrir", Some(Lucide::ExternalLink.into()), "Abrir no GitHub", TamanhoBotao::Linha, window, cx)
                .on_click(move |_, _, cx| cx.open_url(&url))
                .into_any_element()
        });
        let (icone, cor, texto, acao): (Lucide, Hsla, String, Option<AnyElement>) = match m.estado {
            EstadoPr::Mesclado => {
                let por = m.atual.and_then(|p| p.mesclado_por.clone()).or_else(|| m.anterior.and_then(|a| a.mesclado_por.clone()));
                let metodo = m.atual.and_then(|p| p.metodo_mesclado.clone());
                let quando = m.anterior.map(|a| a.em).filter(|em| *em > 0).map(|em| match tempo_relativo(em).as_str() {
                    "agora" => "agora".to_string(),
                    q => format!("há {q}"),
                });
                let mut texto = match por {
                    Some(login) => format!("Mesclado por {login}"),
                    None => "Mesclado".to_string(),
                };
                if let Some(q) = quando {
                    texto.push_str(&format!(" {q}"));
                }
                if let Some(metodo) = metodo {
                    texto.push_str(&format!(" ({metodo})"));
                }
                if let Some(issue) = &t.issue {
                    texto.push_str(&format!(" · issue {}", issue.chave));
                }
                // "Fechar a issue #45": o PR foi mesclado e não fechou a issue do GitHub do chat.
                let issue_aberta = t.issue.as_ref().filter(|i| i.fonte == FonteIssue::Github).and_then(|i| {
                    let n: u64 = i.chave.trim_start_matches('#').parse().ok()?;
                    let d = motor.detalhe_pr(&m.repositorio, m.numero);
                    let fechadas = &d.valor()?.issues_fechadas;
                    (!fechadas.contains(&n)).then(|| i.chave.clone())
                });
                let acao = issue_aberta.filter(|_| ultimo).map(|chave| {
                    let motor_e = self.motor.clone();
                    botao_fantasma("pr-fechar-issue", Some(Lucide::CircleCheck.into()), format!("Fechar a issue {chave}"), TamanhoBotao::Linha, window, cx)
                        .on_click(move |_, window, cx| {
                            if let Some(motor) = &motor_e {
                                let recebe = motor.update(cx, |m, cx| m.fechar_issue_do_pr(tarefa, cx));
                                responder(recebe, window, cx);
                            }
                        })
                        .into_any_element()
                });
                (Lucide::GitMerge, Cores::violeta(), texto, acao)
            }
            EstadoPr::Fechado => {
                let acao = ultimo.then(|| {
                    let motor_e = self.motor.clone();
                    botao_fantasma("pr-reabrir", Some(Lucide::RotateCcw.into()), "Reabrir", TamanhoBotao::Linha, window, cx)
                        .on_click(move |_, window, cx| {
                            if let Some(motor) = &motor_e {
                                let recebe = motor.update(cx, |m, cx| m.reabrir_pr(tarefa, cx));
                                responder(recebe, window, cx);
                            }
                        })
                        .into_any_element()
                });
                (Lucide::GitPullRequestClosed, Cores::vermelho(), "Fechado sem mesclar. A cópia e o ramo continuam.".into(), acao)
            }
            _ => return None,
        };
        Some(
            v_flex()
                .gap(esp::E4)
                .p(dentro_da_borda(12.))
                .mb(esp::E8)
                .rounded(raio::CARTAO)
                .bg(Cores::tinta(cor))
                .border_1()
                .border_color(Cores::tinta_borda(cor))
                .child(
                    h_flex()
                        .items_start()
                        .gap(esp::E8)
                        .child(div().flex_none().pt(esp::E2).child(Icon::new(icone).size(r(16.)).text_color(cor)))
                        .child(div().flex_1().min_w_0().corpo_ui().text_color(Cores::texto()).child(texto)),
                )
                .child(h_flex().flex_wrap().gap(esp::E4).ml(r(16.)).children(acao).children(abrir))
                .into_any_element(),
        )
    }

    /// Visão geral: a faixa da rede, o erro, o estado final, a descrição e a linha do tempo.
    fn visao_geral(
        &self,
        motor: &Motor,
        t: &Tarefa,
        m: &Mostrado,
        detalhe: &Carregando<DetalhePr>,
        window: &Window,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let mut v: Vec<AnyElement> = Vec::new();
        v.extend(self.faixa_da_rede_do_pr(motor, t.id, m, window, cx));
        if let Some(erro) = m.atual.and_then(|p| p.erro.clone()) {
            v.push(div().pb(esp::E8).child(Alerta::new(Tom::Erro, erro).compacto()).into_any_element());
        }
        let ultimo = m.atual.is_some() || (t.pr.is_none() && t.prs_anteriores.last().is_some_and(|a| a.numero == m.numero));
        v.extend(self.estado_final(motor, t, m, ultimo, window, cx));
        v.push(CabecalhoDeSecao::new("Descrição").recuo(0.).primeira(true).into_any_element());
        v.push(match (detalhe.valor(), detalhe.erro()) {
            (Some(d), _) if !d.corpo.trim().is_empty() => texto_de_fora(SharedString::from(format!("pr-corpo-{}-{}", m.repositorio, m.numero)), &d.corpo, None, cx),
            (Some(_), _) => div().metadado().text_color(Cores::tenue()).child("Sem descrição.").into_any_element(),
            (None, Some(erro)) => self.erro_da_leitura(t.id, m, "o PR", erro, window, cx),
            (None, None) => esqueleto(),
        });
        let linha = motor.linha_do_tempo(&m.repositorio, m.numero);
        let eventos = linha.valor().cloned().unwrap_or_default();
        let total = eventos.len();
        let mut cabecalho = CabecalhoDeSecao::new("Linha do tempo").recuo(0.).respiro(16.);
        if total > 0 {
            cabecalho = cabecalho.contagem(total);
        }
        if linha.carregando() {
            cabecalho = cabecalho.acao(spinner(12., window));
        }
        v.push(cabecalho.into_any_element());
        match (total, linha.erro()) {
            (0, Some(erro)) => v.push(self.erro_da_leitura(t.id, m, "a linha do tempo", erro, window, cx)),
            (0, None) if linha.valor().is_some() => v.push(div().metadado().text_color(Cores::tenue()).child("Nada ainda.").into_any_element()),
            (0, None) => v.push(esqueleto()),
            _ => {}
        }
        let nossos: &[u64] = m.atual.map_or(&[], |p| p.comentarios_nossos.as_slice());
        let mostrados = if self.pr.tudo { total } else { total.min(EVENTOS) };
        for (i, e) in eventos.iter().take(mostrados).enumerate() {
            v.push(self.evento(t.id, m.atual.is_some(), nossos, i, e, window, cx));
        }
        if mostrados < total {
            v.push(
                h_flex()
                    .pt(esp::E4)
                    .ml(r(-8.))
                    .child(
                        botao_fantasma("pr-ver-mais", None, format!("Ver mais ({})", total - mostrados), TamanhoBotao::Linha, window, cx)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pr.tudo = true;
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            );
        }
        v
    }

    /// Não deu para ler algo do PR: o erro e "Tentar de novo".
    fn erro_da_leitura(&self, tarefa: u64, m: &Mostrado, o_que: &str, erro: &str, window: &Window, cx: &Context<Self>) -> AnyElement {
        let (motor_e, repo, n) = (self.motor.clone(), m.repositorio.clone(), m.numero);
        Alerta::new(Tom::Atencao, format!("Não consegui ler {o_que}: {erro}"))
            .compacto()
            .acao(botao_fantasma(SharedString::from(format!("pr-ler-de-novo-{o_que}")), None, "Tentar de novo", TamanhoBotao::Linha, window, cx).on_click(
                move |_, _, cx| {
                    if let Some(motor) = &motor_e {
                        motor.update(cx, |m, cx| m.atualizar_pr(tarefa, &repo, n, cx));
                    }
                },
            ))
            .into_any_element()
    }

    /// Uma linha da linha do tempo: o ícone, quem e o quê, quando, os selos e o corpo recolhido.
    /// No hover de um comentário que não é do Vitral, "Mandar ao chat" (só no PR atual).
    #[allow(clippy::too_many_arguments)]
    fn evento(&self, tarefa: u64, atual: bool, nossos: &[u64], i: usize, e: &EventoLinha, window: &Window, cx: &Context<Self>) -> AnyElement {
        let chave = format!("pr-evento-{tarefa}-{i}");
        let grupo = SharedString::from(chave.clone());
        let sem_acesso_selo = || {
            selo_com_dica(
                format!("{chave}-sem-acesso"),
                Selo::neutro("sem acesso ao repositório"),
                "Quem escreveu não tem acesso ao repositório: o Vitral nunca ajusta sozinho por isso",
            )
        };
        let aberto = self.pr.abertos.contains(&chave);
        let alternar = {
            let (janela, chave) = (cx.entity().downgrade(), chave.clone());
            move |_: &mut Window, cx: &mut App| {
                let _ = janela.update(cx, |this, cx| {
                    if !this.pr.abertos.remove(&chave) {
                        this.pr.abertos.insert(chave.clone());
                    }
                    cx.notify();
                });
            }
        };
        let quando = |em: &str| idade(em).map(|i| div().flex_none().metadado().text_color(Cores::tenue()).child(i));
        let autor = |a: &str| div().flex_none().rotulo().text_color(Cores::texto()).child(if a.is_empty() { "alguém".to_string() } else { a.to_string() });
        let verbo = |v: &str| div().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(v.to_string());
        let (nome, cor, cabeca, corpo, comentario): (Lucide, Hsla, Div, Option<AnyElement>, Option<u64>) = match e {
            EventoLinha::Commit { sha, titulo, autor: quem, de_fora, em } => {
                let curto: String = sha.chars().take(7).collect();
                let cabeca = h_flex()
                    .gap(esp::E6)
                    .min_w_0()
                    .child(div().flex_none().mono_pequeno().text_color(Cores::tenue()).child(curto))
                    .child(div().flex_1().min_w_0().truncate().corpo_ui().text_color(Cores::texto()).child(titulo.clone()))
                    .when(*de_fora, |s| {
                        s.child(selo_com_dica(format!("{chave}-de-fora"), Selo::cor("de fora", Cores::ambar()), format!("Commit de {quem}, que não veio do Vitral")))
                    })
                    .children(quando(em));
                (Lucide::GitCommitHorizontal, Cores::tenue(), cabeca, None, None)
            }
            EventoLinha::Revisao { id, autor: quem, estado, corpo, em, sem_acesso } => {
                let (nome, cor, v) = match estado.as_str() {
                    "APPROVED" => (Lucide::CircleCheck, Cores::verde(), "aprovou"),
                    "CHANGES_REQUESTED" => (Lucide::CircleAlert, Cores::ambar(), "pediu mudanças"),
                    "DISMISSED" => (Lucide::CircleX, Cores::tenue(), "revisão descartada"),
                    _ => (Lucide::MessageSquare, Cores::tenue(), "revisou"),
                };
                let cabeca = h_flex().gap(esp::E6).min_w_0().child(autor(quem)).child(verbo(v)).when(*sem_acesso, |s| s.child(sem_acesso_selo())).child(div().flex_1()).children(quando(em));
                let corpo = (!corpo.trim().is_empty()).then(|| corpo_recolhido(chave.clone(), corpo, aberto, alternar.clone(), window, cx));
                (nome, cor, cabeca, corpo, Some(*id))
            }
            EventoLinha::Comentario { id, autor: quem, corpo, em, sem_acesso } => {
                let cabeca = h_flex().gap(esp::E6).min_w_0().child(autor(quem)).child(verbo("comentou")).when(*sem_acesso, |s| s.child(sem_acesso_selo())).child(div().flex_1()).children(quando(em));
                let corpo = Some(corpo_recolhido(chave.clone(), corpo, aberto, alternar.clone(), window, cx));
                (Lucide::MessageSquare, Cores::tenue(), cabeca, corpo, Some(*id))
            }
            EventoLinha::ComentarioEmLinha { id, autor: quem, arquivo, linha, trecho, corpo, em } => {
                let onde = match linha {
                    Some(l) => format!("{arquivo}:{l}"),
                    None => arquivo.clone(),
                };
                let cabeca = h_flex()
                    .gap(esp::E6)
                    .min_w_0()
                    .child(autor(quem))
                    .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis_middle().mono_pequeno().text_color(Cores::tenue()).child(onde))
                    .child(div().flex_1())
                    .children(quando(em));
                let trecho = (!trecho.trim().is_empty()).then(|| {
                    let ultimas: Vec<&str> = trecho.lines().collect();
                    let ultimas = ultimas[ultimas.len().saturating_sub(3)..].join("\n");
                    div()
                        .px(esp::E8)
                        .py(esp::E4)
                        .rounded(raio::CONTROLE)
                        .bg(Cores::recuo())
                        .mono_pequeno()
                        .text_color(Cores::tenue())
                        .overflow_hidden()
                        .child(ultimas)
                });
                let corpo = v_flex()
                    .gap(esp::E4)
                    .children(trecho)
                    .child(corpo_recolhido(chave.clone(), corpo, aberto, alternar.clone(), window, cx))
                    .into_any_element();
                (Lucide::MessageSquareQuote, Cores::tenue(), cabeca, Some(corpo), Some(*id))
            }
            EventoLinha::Bots { login, quantos } => {
                let n = *quantos as usize;
                let cabeca = h_flex()
                    .gap(esp::E6)
                    .min_w_0()
                    .child(div().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(format!("Bot {login} · {}", contagem(n, "comentário", "comentários"))));
                (Lucide::Bot, Cores::tenue(), cabeca, None, None)
            }
        };
        let mandar = comentario.filter(|id| atual && !nossos.contains(id)).map(|id| {
            let motor_e = self.motor.clone();
            div()
                .absolute()
                .top(esp::E4)
                .right_0()
                .invisible()
                .group_hover(grupo.clone(), |s| s.visible())
                .child(
                    botao_fantasma(SharedString::from(format!("{chave}-mandar")), Some(Lucide::MessageSquareQuote.into()), "Mandar ao chat", TamanhoBotao::Linha, window, cx)
                        .bg(Cores::cartao_elevado())
                        .tooltip("Mandar só este comentário ao chat, para ele ajustar")
                        .on_click(move |_, window, cx| {
                            if let Some(motor) = &motor_e {
                                let r = motor.update(cx, |m, cx| m.mandar_comentarios_ao_chat(tarefa, vec![id], cx));
                                avisar(&r, window, cx);
                            }
                        }),
                )
        });
        h_flex()
            .id(ElementId::Name(grupo.clone()))
            .group(grupo)
            .relative()
            .items_start()
            .gap(esp::E8)
            .py(esp::E6)
            .child(div().flex_none().size(r(20.)).flex().items_center().justify_center().child(Icon::new(nome).size(r(14.)).text_color(cor)))
            .child(v_flex().flex_1().min_w_0().gap(esp::E2).child(cabeca.min_h(r(20.))).children(corpo))
            .children(mandar)
            .into_any_element()
    }

    /// Arquivos: o total e uma linha por arquivo; o clique abre o diff dele.
    fn arquivos_do_pr(
        &self,
        motor: &Motor,
        t: &Tarefa,
        m: &Mostrado,
        detalhe: &Carregando<DetalhePr>,
        window: &Window,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let mut v: Vec<AnyElement> = Vec::new();
        v.extend(self.faixa_da_rede_do_pr(motor, t.id, m, window, cx));
        let Some(d) = detalhe.valor() else {
            v.push(match detalhe.erro() {
                Some(erro) => self.erro_da_leitura(t.id, m, "os arquivos", erro, window, cx),
                None => esqueleto(),
            });
            return v;
        };
        if d.arquivos.is_empty() {
            v.push(div().py(esp::E24).child(EstadoVazio::new(Lucide::FileCheck, "Nenhum arquivo mudou")).into_any_element());
            return v;
        }
        let (mais, menos) = d.arquivos.iter().fold((0, 0), |(a, b), f| (a + f.adicoes, b + f.remocoes));
        v.push(
            h_flex()
                .h(r(28.))
                .gap(esp::E8)
                .child(div().metadado().text_color(Cores::tenue()).child(contagem(d.arquivos.len(), "arquivo", "arquivos")))
                .child(mais_menos(mais, menos))
                .into_any_element(),
        );
        // Com a cópia, o diff é o local contra a base da tarefa (sem rede); sem ela, o do GitHub.
        let fonte = if m.atual.is_some() && t.worktree.as_ref().is_some_and(|w| w.is_dir()) && !t.copia_apagada() {
            FonteDiff::Tarefa(t.id)
        } else {
            FonteDiff::Pr { repositorio: m.repositorio.clone(), numero: m.numero }
        };
        for (i, a) in d.arquivos.iter().enumerate() {
            let (nome, cor) = icone_do_arquivo(a);
            let (fonte, caminho, tarefa) = (fonte.clone(), a.caminho.clone(), t.id);
            v.push(
                div()
                    .mx(r(-8.))
                    .child(
                        CartaoDeLista::new(("pr-arquivo", i), a.caminho.clone())
                            .uma_linha()
                            .titulo_no_meio()
                            .simbolo(Simbolo::icone_cor(nome, cor))
                            .meta(mais_menos(a.adicoes, a.remocoes))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.dialogo_diff_do_pr(tarefa, fonte.clone(), Some(caminho.clone()), window, cx)
                            })),
                    )
                    .into_any_element(),
            );
        }
        v
    }

    /// Checks: o anel com o resumo e "Rodar falhas de novo", e um check por linha (falhou,
    /// rodando, passou, pulado), com "Ver log" e "Mandar ao chat" nos que falharam.
    fn lista_de_checks(&self, t: &Tarefa, m: &Mostrado, checks: &Carregando<Vec<Check>>, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        let mut v: Vec<AnyElement> = Vec::new();
        let Some(lista) = checks.valor() else {
            v.push(match checks.erro() {
                Some(erro) => self.erro_da_leitura(t.id, m, "os checks", erro, window, cx),
                None => esqueleto(),
            });
            return v;
        };
        if lista.is_empty() {
            v.push(
                div()
                    .py(esp::E24)
                    .child(EstadoVazio::new(Lucide::CircleDashed, "Sem checks").texto("O repositório não rodou nada neste PR."))
                    .into_any_element(),
            );
            return v;
        }
        let resumo = ResumoChecks::da_lista(lista);
        let rotulo = rotulo_checks(&resumo);
        let rodando = lista.iter().filter(|c| c.balde == "pending").count();
        let mut partes = Vec::new();
        if resumo.falhos > 0 {
            partes.push(if resumo.falhos == 1 { "1 falhou".to_string() } else { format!("{} falharam", resumo.falhos) });
        }
        if rodando > 0 {
            partes.push(format!("{rodando} rodando"));
        }
        if partes.is_empty() {
            partes.push(rotulo.frase.clone());
        }
        let tarefa = t.id;
        let atual = m.atual.is_some();
        let compacta = self.direita_compacta;
        let rodar = (atual && resumo.falhos > 0).then(|| {
            let motor_e = self.motor.clone();
            let b = if compacta {
                botao_icone("pr-rodar-falhas", Lucide::RefreshCw, "Rodar falhas de novo", TamanhoBotao::Linha, window, cx)
            } else {
                botao_fantasma("pr-rodar-falhas", Some(Lucide::RefreshCw.into()), "Rodar falhas de novo", TamanhoBotao::Linha, window, cx)
            };
            b.on_click(move |_, window, cx| {
                if let Some(motor) = &motor_e {
                    let recebe = motor.update(cx, |m, cx| m.rodar_checks_de_novo(tarefa, RodarDeNovo::Falhas, cx));
                    responder(recebe, window, cx);
                }
            })
        });
        v.push(
            h_flex()
                .gap(esp::E12)
                .py(esp::E8)
                .child(Anel::new("pr-anel-checks", rotulo.fracao).diametro(28.).cor(cor_dos_checks(&resumo)))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().rotulo().text_color(Cores::forte()).child(rotulo.curto.clone()))
                        .child(div().truncate().metadado().text_color(Cores::tenue()).child(partes.join(" · "))),
                )
                .children(checks.carregando().then(|| spinner(12., window)))
                .children(rodar)
                .into_any_element(),
        );
        let mut ordenados: Vec<&Check> = lista.iter().collect();
        ordenados.sort_by_key(|c| ordem_do_check(c));
        for (i, c) in ordenados.into_iter().enumerate() {
            let (nome, cor) = icone_do_check(c);
            let mut meta: Vec<String> = Vec::new();
            if !c.workflow.is_empty() && c.workflow != c.nome {
                meta.push(c.workflow.clone());
            }
            match c.balde.as_str() {
                "pending" => meta.push("rodando".into()),
                "skipping" => meta.push("pulado".into()),
                "cancel" => meta.push("cancelado".into()),
                _ => meta.extend(duracao_do_check(c)),
            }
            let falhou = c.balde == "fail";
            let acoes = (falhou && atual).then(|| {
                let (repo, link) = (m.repositorio.clone(), c.link.clone());
                let (nome_check, link2) = (c.nome.clone(), c.link.clone());
                let motor_e = self.motor.clone();
                let ver = if compacta {
                    botao_icone(("pr-check-log", i), Lucide::ScrollText, "Ver log", TamanhoBotao::Linha, window, cx)
                } else {
                    botao_fantasma(("pr-check-log", i), Some(Lucide::ScrollText.into()), "Ver log", TamanhoBotao::Linha, window, cx)
                }
                .on_click(cx.listener(move |this, _, window, cx| this.dialogo_log(tarefa, repo.clone(), nome_check.clone(), link.clone(), window, cx)));
                let mandar = if compacta {
                    botao_icone(("pr-check-mandar", i), Lucide::MessageSquareQuote, "Mandar ao chat", TamanhoBotao::Linha, window, cx)
                } else {
                    botao_fantasma(("pr-check-mandar", i), Some(Lucide::MessageSquareQuote.into()), "Mandar ao chat", TamanhoBotao::Linha, window, cx)
                }
                .tooltip("Mandar esta falha ao chat (com o log), para ele corrigir")
                .on_click(move |_, window, cx| {
                    if let Some(motor) = &motor_e {
                        let recebe = motor.update(cx, |m, cx| m.mandar_falha_ao_chat(tarefa, Some(link2.clone()), cx));
                        responder(recebe, window, cx);
                    }
                });
                h_flex().flex_none().gap(esp::E4).child(ver).child(mandar)
            });
            v.push(
                h_flex()
                    .id(("pr-check", i))
                    .min_h(r(40.))
                    .gap(esp::E8)
                    .px(esp::E4)
                    .mx(r(-4.))
                    .rounded(raio::CONTROLE)
                    .hover(|s| s.bg(Cores::hover()))
                    .child(div().flex_none().size(r(20.)).flex().items_center().justify_center().child(Icon::new(nome).size(r(14.)).text_color(cor)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().truncate().corpo_ui().text_color(if falhou { Cores::forte() } else { Cores::texto() }).child(c.nome.clone()))
                            .when(!meta.is_empty(), |s| s.child(div().truncate().metadado().text_color(Cores::tenue()).child(meta.join(" · ")))),
                    )
                    .children(acoes)
                    .into_any_element(),
            );
        }
        v
    }

    /// O rodapé do PR atual aberto: "Falta: …" (ou "Pronto para mesclar"), o Mesclar pelo método
    /// padrão, "Mesclar quando ficar pronto" e abrir no GitHub. Na largura compacta, o interruptor
    /// vai numa linha só dele.
    fn rodape_do_pr(&self, motor: &Motor, t: &Tarefa, m: &Mostrado, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let pr = m.atual.filter(|p| p.aberto())?;
        let tarefa = t.id;
        let acoes = motor.acoes_do_pr(tarefa);
        let simulacao = motor.simulacao(tarefa);
        let padrao = acao_mesclar_padrao(&acoes);
        let mesclar = resultado(&acoes, &padrao);
        let AcaoPr::Mesclar { metodo } = padrao else { return None };
        let ship = resultado(&acoes, &AcaoPr::MesclarQuandoPronto);
        let compacta = self.direita_compacta;
        let falta = falta_para_mesclar(pr, simulacao.as_ref());
        let base = nome_da_base(pr).to_string();
        // Sem nada faltando no PR, o que ainda segura o Mesclar (a permissão, o chat que roda, a
        // publicação) fica no lugar do "Pronto para mesclar".
        let linha_falta = if let (true, Err(motivo)) = (falta.is_empty(), &mesclar) {
            h_flex()
                .gap(esp::E6)
                .child(Icon::new(Lucide::Clock).size(r(14.)).text_color(Cores::tenue()))
                .child(div().metadado().text_color(Cores::tenue()).child(motivo.clone()))
                .into_any_element()
        } else if falta.is_empty() {
            h_flex()
                .gap(esp::E6)
                .child(Icon::new(Lucide::CircleCheck).size(r(14.)).text_color(Cores::verde()))
                .child(div().metadado().text_color(Cores::verde()).child("Pronto para mesclar"))
                .into_any_element()
        } else {
            let texto = format!("Falta: {}", falta.join(" · "));
            div()
                .id("pr-falta")
                .metadado()
                .text_color(Cores::tenue())
                .line_clamp(2)
                .text_ellipsis()
                .child(texto.clone())
                .tooltip(move |window, cx| Tooltip::new(texto.clone()).build(window, cx))
                .into_any_element()
        };
        // O que resolve o primeiro bloqueio: o conflito vai ao chat, o atraso atualiza com a base.
        let resolver = if tem_conflito(pr, simulacao.as_ref()) && resultado(&acoes, &AcaoPr::PedirAoChat).is_ok() {
            let arquivos = simulacao.as_ref().and_then(|s| s.as_ref().ok()).cloned().unwrap_or_default();
            let base_remota = t.partir_de.clone().unwrap_or_else(|| format!("origin/{base}"));
            let texto = texto_do_conflito(&base_remota, &arquivos);
            Some(
                botao_fantasma("pr-pedir-ao-chat", Some(Lucide::MessageSquareQuote.into()), "Pedir ao chat", TamanhoBotao::Linha, window, cx)
                    .tooltip("Escreve na caixa do chat o pedido para resolver o conflito")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(painel) = this.paineis.get(this.ativo).map(|p| p.id) {
                            this.escrever_na_caixa(painel, &texto, window, cx);
                        }
                    }))
                    .into_any_element(),
            )
        } else if pr.mesclavel == "BEHIND" && resultado(&acoes, &AcaoPr::AtualizarComBase).is_ok() {
            let motor_e = self.motor.clone();
            Some(
                botao_fantasma("pr-atualizar-base", Some(Lucide::GitBranch.into()), format!("Atualizar com {base}"), TamanhoBotao::Linha, window, cx)
                    .on_click(move |_, window, cx| {
                        if let Some(motor) = &motor_e {
                            let recebe = motor.update(cx, |m, cx| m.atualizar_com_base(tarefa, cx));
                            responder(recebe, window, cx);
                        }
                    })
                    .into_any_element(),
            )
        } else {
            None
        };
        let rotulo = format!("Mesclar · {metodo}");
        let botao = match &mesclar {
            Ok(()) => botao_primario("pr-mesclar", Some(Lucide::GitMerge.into()), rotulo, TamanhoBotao::Barra, window, cx)
                .tooltip(if confirmacoes_para_mesclar(pr).is_empty() {
                    format!("Mesclar o PR #{} com {metodo}", pr.numero)
                } else {
                    "Mesclar (pede confirmação antes)".to_string()
                })
                .on_click(cx.listener(move |this, _, window, cx| this.mesclar_pr(tarefa, None, window, cx))),
            Err(motivo) => botao_primario("pr-mesclar", Some(Lucide::GitMerge.into()), rotulo, TamanhoBotao::Barra, window, cx)
                .disabled(true)
                .tooltip(motivo.clone()),
        };
        let ligado = pr.ao_ficar_pronto.is_some();
        let dica_ship: SharedString = match (&ship, ligado) {
            (_, true) => format!("Ligado: o Vitral corrige CI e revisão (até 3 rodadas) e mescla sozinho quando passar ({}).", pr.ao_ficar_pronto.clone().unwrap_or_default()).into(),
            (Ok(()), false) => "O Vitral corrige CI e revisão (até 3 rodadas), publica de novo e mescla sozinho quando passar.".into(),
            (Err(motivo), false) => motivo.clone().into(),
        };
        let motor_e = self.motor.clone();
        let interruptor = h_flex()
            .id("pr-ship")
            .flex_none()
            .gap(esp::E8)
            .cursor_pointer()
            .child(
                Switch::new("pr-ship-interruptor")
                    .checked(ligado)
                    .disabled(!ligado && ship.is_err())
                    .on_click(move |novo, _, cx| {
                        if let Some(motor) = &motor_e {
                            let novo = *novo;
                            motor.update(cx, |m, cx| m.armar_mescla(tarefa, novo, None, cx));
                        }
                    }),
            )
            .child(div().rotulo().text_color(if ligado { Cores::texto() } else { Cores::tenue() }).child("Mesclar quando ficar pronto"))
            .tooltip(move |window, cx| Tooltip::new(dica_ship.clone()).build(window, cx));
        let abrir = link_permitido(&pr.url, OrigemDoTexto::Terceiros).then(|| {
            let url = pr.url.clone();
            botao_icone("pr-rodape-abrir", Lucide::ExternalLink, "Abrir no GitHub", TamanhoBotao::Barra, window, cx).on_click(move |_, _, cx| cx.open_url(&url))
        });
        let botoes = h_flex().gap(esp::E8).child(sem_arrastar().flex_none().child(botao)).child(div().flex_1());
        let botoes = if compacta { botoes.children(abrir) } else { botoes.child(interruptor.into_any_element()).children(abrir) };
        let interruptor_em_cima = compacta.then(|| {
            let motor_e = self.motor.clone();
            h_flex()
                .gap(esp::E8)
                .child(
                    Switch::new("pr-ship-interruptor-compacto")
                        .checked(ligado)
                        .disabled(!ligado && ship.is_err())
                        .on_click(move |novo, _, cx| {
                            if let Some(motor) = &motor_e {
                                let novo = *novo;
                                motor.update(cx, |m, cx| m.armar_mescla(tarefa, novo, None, cx));
                            }
                        }),
                )
                .child(div().rotulo().text_color(if ligado { Cores::texto() } else { Cores::tenue() }).child("Mesclar quando ficar pronto"))
        });
        Some(
            v_flex()
                .flex_none()
                .gap(esp::E8)
                .px(esp::E12)
                .pt(esp::E12)
                .pb(esp::E12)
                .border_t_1()
                .border_color(Cores::borda())
                .child(h_flex().gap(esp::E8).child(div().flex_1().min_w_0().child(linha_falta)).children(resolver))
                .children(interruptor_em_cima)
                .child(botoes)
                .into_any_element(),
        )
    }

    /// Mesclar agora: com commit de fora ou `UNSTABLE`, confirma antes (ui/dialogos.rs).
    pub(super) fn mesclar_pr(&mut self, tarefa: u64, metodo: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else { return };
        let Some(pr) = motor.read(cx).pr_da_tarefa(tarefa).cloned() else { return };
        let confirmar_antes = confirmacoes_para_mesclar(&pr);
        if confirmar_antes.is_empty() {
            let recebe = motor.update(cx, |m, cx| m.mesclar_pelo_usuario(tarefa, metodo, Vec::new(), cx));
            responder(recebe, window, cx);
            return;
        }
        self.confirmar_mescla(tarefa, &pr, metodo, confirmar_antes, window, cx);
    }

    /// As linhas do PR em "Esperando você": o projeto, o chat, o que espera (com o ponto) e o botão;
    /// o clique abre o chat na aba PR.
    pub(super) fn linhas_do_pr(&self, motor: &Motor, esperas: Vec<EsperaDoPr>, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        esperas
            .into_iter()
            .map(|e| {
                let (tarefa, chat) = (e.tarefa, e.chat.clone());
                let id = |s: &str| SharedString::from(format!("espera-{}-{s}", e.chave));
                let motor_e = self.motor.clone();
                let botao = match &e.acao {
                    AcaoDaEspera::MandarFalha => botao_icone(id("acao"), Lucide::MessageSquareQuote, "Mandar a falha ao chat", TamanhoBotao::Linha, window, cx).on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        if let Some(motor) = &motor_e {
                            let recebe = motor.update(cx, |m, cx| m.mandar_falha_ao_chat(tarefa, None, cx));
                            responder(recebe, window, cx);
                        }
                    }),
                    AcaoDaEspera::MandarComentarios(ids) => {
                        let ids = ids.clone();
                        botao_icone(id("acao"), Lucide::MessageSquareQuote, "Mandar ao chat", TamanhoBotao::Linha, window, cx).on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            if let Some(motor) = &motor_e {
                                let r = motor.update(cx, |m, cx| m.mandar_comentarios_ao_chat(tarefa, ids.clone(), cx));
                                avisar(&r, window, cx);
                            }
                        })
                    }
                    AcaoDaEspera::Mesclar => botao_icone(id("acao"), Lucide::GitMerge, "Mesclar", TamanhoBotao::Linha, window, cx).on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.mesclar_pr(tarefa, None, window, cx)
                    })),
                    AcaoDaEspera::AtualizarPr => botao_icone(id("acao"), Lucide::GitPullRequest, "Atualizar PR", TamanhoBotao::Linha, window, cx).on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.atualizar_pr_do_usuario(tarefa, window, cx)
                    })),
                    AcaoDaEspera::Destravar => botao_icone(id("acao"), Lucide::LockKeyholeOpen, "Destravar e tentar", TamanhoBotao::Linha, window, cx).on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        if let Some(motor) = &motor_e {
                            let recebe = motor.update(cx, |m, cx| m.tentar_efeito_pendente(tarefa, cx));
                            responder(recebe, window, cx);
                        }
                    }),
                    AcaoDaEspera::Confirmar(cartao) => {
                        let (cartao, chat) = (*cartao, chat.clone());
                        botao_fantasma(id("acao"), None, "Ver", TamanhoBotao::Linha, window, cx).on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.ver_cartao(Some(cartao), &chat, window, cx)
                        }))
                    }
                    AcaoDaEspera::Ver => {
                        let chat = chat.clone();
                        botao_fantasma(id("acao"), None, "Ver", TamanhoBotao::Linha, window, cx).on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.abrir_aba_pr(&chat, window, cx)
                        }))
                    }
                };
                let (icone, cor) = super::icone_projeto::visual_do(motor, &e.projeto);
                let subtitulo = h_flex()
                    .gap(esp::E6)
                    .min_w_0()
                    .child(div().flex_none().child(e.sinal.ponto(6.)))
                    .child(div().flex_1().min_w_0().truncate().child(e.texto.clone()));
                let tirar = e.item.clone().map(|item| {
                    let motor_e = self.motor.clone();
                    botao_icone(id("tirar"), Lucide::X, "Tirar daqui", TamanhoBotao::Linha, window, cx).on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        if let Some(motor) = &motor_e {
                            motor.update(cx, |m, cx| m.dispensar_espera_do_pr(tarefa, &item, cx));
                        }
                    })
                });
                let mut cartao = CartaoDeLista::new(id("cartao"), e.titulo.clone())
                    .simbolo(Simbolo::Projeto(icone, cor))
                    .subtitulo(subtitulo)
                    .acao_fixa(botao)
                    .on_click(cx.listener(move |this, _, window, cx| this.abrir_aba_pr(&chat, window, cx)));
                if let Some(tirar) = tirar {
                    cartao = cartao.acao(tirar);
                }
                div().mt(esp::E2).child(cartao).into_any_element()
            })
            .collect()
    }

    /// "Atualizar PR" sem diálogo (a rodada verificada que espera, `pronto_para_atualizar`): manda
    /// o que está na cópia, sem mexer no título nem na descrição.
    pub(super) fn atualizar_pr_do_usuario(&mut self, tarefa: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else { return };
        let quando_pronto = motor.read(cx).pr_da_tarefa(tarefa).and_then(|p| p.ao_ficar_pronto.clone());
        let pedido = crate::motor::PedidoPr { quando_pronto, ..Default::default() };
        let recebe = motor.update(cx, |m, cx| m.publicar_pr_pelo_usuario(tarefa, pedido, cx));
        responder(recebe, window, cx);
    }

    /// "Fechar o PR…": confirma antes, e o PR fecha sem comentário.
    fn confirmar_fechar_pr(&mut self, tarefa: u64, numero: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else { return };
        confirmar(
            Confirmacao {
                tom: TomConfirmacao::Perigo,
                icone: Lucide::GitPullRequestClosed.into(),
                titulo: format!("Fechar o PR #{numero} sem mesclar?").into(),
                texto: "A cópia e o ramo continuam no Vitral; dá para reabrir depois.".into(),
                detalhes: Vec::new(),
                ok: "Fechar o PR".into(),
                cancelar: "Cancelar".into(),
                ok_perigo: true,
            },
            move |window, cx| {
                let recebe = motor.update(cx, |m, cx| m.fechar_pr(tarefa, None, cx));
                responder(recebe, window, cx);
            },
            None,
            window,
            cx,
        );
    }
}

/// O esqueleto da primeira leitura: três linhas na `borda()` a 40%.
fn esqueleto() -> AnyElement {
    v_flex()
        .gap(esp::E8)
        .py(esp::E8)
        .child(barra_esqueleto(0.9))
        .child(barra_esqueleto(0.7))
        .child(barra_esqueleto(0.8))
        .into_any_element()
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: ele traria o `test` do GPUI no lugar do `#[test]` de sempre.
    use super::{
        LinkVitral, Ocupado, OrigemDoTexto, Referencias, acoes_do_pr, confirmacoes_para_mesclar, falta_para_mesclar,
        ler_link_vitral, ligar_referencias, link_permitido, metodos_do_pr, resultado, rotulo_checks, texto_do_conflito,
    };
    use crate::github::{EstadoChecks, EstadoPr, InfoRepo, ResumoChecks};
    use crate::modelo::Pr;
    use crate::motor::{AcaoPr, Confirmado};

    fn pr() -> Pr {
        Pr {
            repositorio: "VictorMenegate/top-bar-app-color".into(),
            numero: 12,
            base: "main".into(),
            estado: EstadoPr::Aberto,
            mesclavel: "CLEAN".into(),
            checks: ResumoChecks { estado: EstadoChecks::Passou, total: 41, terminados: 41, falhos: 0, pendentes: 0 },
            sha: "abc1234".into(),
            sha_publicado: "abc1234".into(),
            acompanhando: true,
            ..Default::default()
        }
    }

    fn info(permissao: &str) -> InfoRepo {
        InfoRepo {
            repositorio: "VictorMenegate/top-bar-app-color".into(),
            permissao: permissao.into(),
            metodo_padrao: "squash".into(),
            squash: true,
            merge: true,
            rebase: false,
            ..Default::default()
        }
    }

    /// O resultado do Mesclar pelo método padrão.
    fn mesclar(pr: &Pr, info: Option<&InfoRepo>, simulacao: Option<&Result<Vec<String>, String>>, ocupado: Option<Ocupado>) -> Result<(), String> {
        resultado(&acoes_do_pr(pr, info, simulacao, ocupado), &AcaoPr::Mesclar { metodo: "squash".into() })
    }

    #[test]
    fn mesclar_segue_a_tabela_inteira() {
        let i = info("ADMIN");
        let erro = |p: &Pr, sim: Option<&Result<Vec<String>, String>>, o| mesclar(p, Some(&i), sim, o).unwrap_err();
        // Estado do PR.
        let mut p = pr();
        p.estado = EstadoPr::Mesclado;
        assert_eq!(erro(&p, None, None), "O PR está mesclado");
        p.estado = EstadoPr::Fechado;
        assert_eq!(erro(&p, None, None), "O PR está fechado");
        p.estado = EstadoPr::Rascunho;
        assert_eq!(erro(&p, None, None), "Rascunho: marque como pronto para revisão");
        // Conflito: pela simulação local ou pelo DIRTY.
        let p = pr();
        let conflito = Ok(vec!["src/a.rs".to_string()]);
        assert_eq!(erro(&p, Some(&conflito), None), "Tem conflito com main: peça ao chat para resolver");
        let mut sujo = pr();
        sujo.mesclavel = "DIRTY".into();
        assert_eq!(erro(&sujo, Some(&Ok(Vec::new())), None), "Tem conflito com main: peça ao chat para resolver");
        // Atrás da base.
        let mut atras = pr();
        atras.mesclavel = "BEHIND".into();
        assert_eq!(erro(&atras, None, None), "Atrás de main: atualize antes");
        // Checks pendentes oferecem o Ship.
        let mut pendente = pr();
        pendente.checks = ResumoChecks { estado: EstadoChecks::Pendente, total: 41, terminados: 15, falhos: 0, pendentes: 26 };
        assert_eq!(erro(&pendente, None, None), "15 de 41 checks terminaram");
        assert_eq!(resultado(&acoes_do_pr(&pendente, Some(&i), None, None), &AcaoPr::MesclarQuandoPronto), Ok(()));
        // Checks falhos.
        let mut falhou = pr();
        falhou.checks = ResumoChecks { estado: EstadoChecks::Falhou, total: 41, terminados: 41, falhos: 2, pendentes: 0 };
        assert_eq!(erro(&falhou, None, None), "2 checks falharam");
        falhou.checks.falhos = 1;
        assert_eq!(erro(&falhou, None, None), "1 check falhou");
        // Bloqueado pelo GitHub.
        let mut bloqueado = pr();
        bloqueado.mesclavel = "BLOCKED".into();
        assert_eq!(erro(&bloqueado, None, None), "O GitHub ainda não permite (aprovação ou check obrigatório)");
        // Commit de fora e UNSTABLE mesclam, com confirmação.
        let mut de_fora = pr();
        de_fora.commits_de_fora = vec!["def5678".into()];
        de_fora.sha = "def5678".into();
        assert_eq!(mesclar(&de_fora, Some(&i), None, None), Ok(()));
        assert_eq!(confirmacoes_para_mesclar(&de_fora), vec![Confirmado::CommitsDeFora("def5678".into())]);
        let mut instavel = pr();
        instavel.mesclavel = "UNSTABLE".into();
        assert_eq!(mesclar(&instavel, Some(&i), None, None), Ok(()));
        assert_eq!(confirmacoes_para_mesclar(&instavel), vec![Confirmado::Instavel]);
        // Ocupado: o chat, a publicação, a mesclagem.
        assert_eq!(erro(&pr(), None, Some(Ocupado::Chat)), "Esperando o chat terminar");
        assert_eq!(erro(&pr(), None, Some(Ocupado::Publicando)), "Publicando o PR…");
        assert_eq!(erro(&pr(), None, Some(Ocupado::Mesclando)), "Mesclando o PR…");
        // Sem permissão de escrita.
        let leitura = info("READ");
        assert_eq!(
            mesclar(&pr(), Some(&leitura), None, None).unwrap_err(),
            "Sua conta não pode mesclar em VictorMenegate/top-bar-app-color"
        );
        // CLEAN e HAS_HOOKS mesclam, sem confirmação.
        assert_eq!(mesclar(&pr(), Some(&i), None, None), Ok(()));
        let mut ganchos = pr();
        ganchos.mesclavel = "HAS_HOOKS".into();
        assert_eq!(mesclar(&ganchos, Some(&i), None, None), Ok(()));
        assert!(confirmacoes_para_mesclar(&ganchos).is_empty());
    }

    #[test]
    fn os_metodos_e_as_outras_acoes() {
        let i = info("WRITE");
        let acoes = acoes_do_pr(&pr(), Some(&i), None, None);
        let metodos: Vec<&AcaoPr> = acoes.iter().map(|(a, _)| a).filter(|a| matches!(a, AcaoPr::Mesclar { .. })).collect();
        assert_eq!(metodos, [&AcaoPr::Mesclar { metodo: "squash".into() }, &AcaoPr::Mesclar { metodo: "merge".into() }]);
        assert_eq!(metodos_do_pr(None), ["squash"]);
        assert_eq!(resultado(&acoes, &AcaoPr::PedirAoChat), Err("Sem conflito com main".into()));
        assert_eq!(resultado(&acoes, &AcaoPr::VoltarParaRascunho), Ok(()));
        assert!(resultado(&acoes, &AcaoPr::ProntoParaRevisao).is_err());
        assert!(resultado(&acoes, &AcaoPr::Reabrir).is_err());
        let conflito = Ok(vec!["a".to_string()]);
        let acoes = acoes_do_pr(&pr(), Some(&i), Some(&conflito), None);
        assert_eq!(resultado(&acoes, &AcaoPr::PedirAoChat), Ok(()));
        // O chat rodando segura as escritas, mas o Ship arma.
        let acoes = acoes_do_pr(&pr(), Some(&i), Some(&conflito), Some(Ocupado::Chat));
        assert_eq!(resultado(&acoes, &AcaoPr::PedirAoChat), Err("Esperando o chat terminar".into()));
        assert_eq!(resultado(&acoes, &AcaoPr::AtualizarComBase), Err("Esperando o chat terminar".into()));
        assert_eq!(resultado(&acoes, &AcaoPr::MesclarQuandoPronto), Ok(()));
        let mut fechado = pr();
        fechado.estado = EstadoPr::Fechado;
        let acoes = acoes_do_pr(&fechado, Some(&i), None, None);
        assert_eq!(resultado(&acoes, &AcaoPr::Reabrir), Ok(()));
        assert_eq!(resultado(&acoes, &AcaoPr::MesclarQuandoPronto), Err("O PR está fechado".into()));
        assert_eq!(resultado(&acoes, &AcaoPr::Fechar), Err("O PR está fechado".into()));
        let mut rascunho = pr();
        rascunho.estado = EstadoPr::Rascunho;
        assert_eq!(resultado(&acoes_do_pr(&rascunho, Some(&i), None, None), &AcaoPr::ProntoParaRevisao), Ok(()));
    }

    #[test]
    fn a_linha_do_que_falta() {
        assert!(falta_para_mesclar(&pr(), None).is_empty());
        // O mesclar_pr pelo chat usa a mesma lista: sem checks (um repositório sem CI), UNSTABLE (o
        // cartão confirma) e o GitHub ainda calculando não faltam; quem confere é o mesclar.
        let mut sem_ci = pr();
        (sem_ci.checks, sem_ci.mesclavel) = (ResumoChecks::default(), "UNSTABLE".into());
        assert!(falta_para_mesclar(&sem_ci, None).is_empty());
        sem_ci.mesclavel.clear();
        assert!(falta_para_mesclar(&sem_ci, None).is_empty());
        let mut p = pr();
        p.checks = ResumoChecks { estado: EstadoChecks::Pendente, total: 41, terminados: 15, falhos: 2, pendentes: 26 };
        p.conversas_abertas = 1;
        p.decisao = "CHANGES_REQUESTED".into();
        assert_eq!(falta_para_mesclar(&p, None), ["2 checks falharam", "26 checks rodando", "mudanças pedidas", "1 conversa aberta"]);
        let mut atras = pr();
        atras.mesclavel = "BEHIND".into();
        assert_eq!(falta_para_mesclar(&atras, None), ["atualizar com main"]);
        assert_eq!(falta_para_mesclar(&atras, Some(&Ok(vec!["a".into()]))), ["resolver o conflito com main"]);
        let mut bloqueado = pr();
        bloqueado.mesclavel = "BLOCKED".into();
        assert_eq!(falta_para_mesclar(&bloqueado, None), ["aprovação ou check obrigatório"]);
    }

    #[test]
    fn rotulo_checks_em_cada_estado() {
        let c = |estado, total, terminados, falhos, pendentes| ResumoChecks { estado, total, terminados, falhos, pendentes };
        let r = rotulo_checks(&c(EstadoChecks::Pendente, 41, 15, 0, 26));
        assert_eq!((r.curto.as_str(), r.frase.as_str()), ("15/41", "15 de 41 checks terminaram"));
        assert!((r.fracao - 15. / 41.).abs() < 1e-6);
        let r = rotulo_checks(&c(EstadoChecks::Pendente, 41, 15, 2, 26));
        assert_eq!(r.frase, "15 de 41 checks terminaram (2 checks falharam)");
        let r = rotulo_checks(&c(EstadoChecks::Passou, 41, 41, 0, 0));
        assert_eq!((r.curto.as_str(), r.frase.as_str(), r.fracao), ("41/41", "41 checks passaram", 1.));
        assert_eq!(rotulo_checks(&c(EstadoChecks::Passou, 1, 1, 0, 0)).frase, "1 check passou");
        let r = rotulo_checks(&c(EstadoChecks::Falhou, 41, 41, 2, 0));
        assert_eq!((r.curto.as_str(), r.frase.as_str()), ("2 falharam", "2 de 41 checks falharam"));
        let r = rotulo_checks(&ResumoChecks::default());
        assert_eq!((r.curto.as_str(), r.frase.as_str(), r.fracao), ("sem checks", "Sem checks", 0.));
    }

    #[test]
    fn esperando_voce_so_com_acao() {
        use super::{AcaoDaEspera, esperas_da_tarefa};
        use crate::modelo::{EsperaPr, Tarefa};
        let agora = 100_000;
        let mut t = Tarefa { pr: Some(pr()), ..Default::default() };
        let textos = |t: &Tarefa, pronto: bool| esperas_da_tarefa(t, agora, pronto).into_iter().map(|e| e.0).collect::<Vec<_>>();
        // Passou, sem nada que o motor tenha deixado: nada espera você.
        assert!(esperas_da_tarefa(&t, agora, true).is_empty());
        // O pronto do vigia só aparece enquanto o PR de agora ainda está pronto.
        t.pr.as_mut().unwrap().esperando = vec![EsperaPr::ProntoParaMesclar];
        assert_eq!(esperas_da_tarefa(&t, agora, true).last().map(|e| e.2.clone()), Some(AcaoDaEspera::Mesclar));
        assert!(esperas_da_tarefa(&t, agora, false).is_empty());
        // CI falhou (no Watch o motor deixa o item); com a rodada que corrige pronta, só o Atualizar PR.
        let p = t.pr.as_mut().unwrap();
        p.checks = ResumoChecks { estado: EstadoChecks::Falhou, total: 4, terminados: 4, falhos: 1, pendentes: 0 };
        p.esperando = vec![EsperaPr::CiFalhou { chave: "abc".into(), falhos: vec!["test".into()] }];
        assert_eq!(textos(&t, false), ["CI falhou no PR #12"]);
        t.pronto_para_atualizar = true;
        assert_eq!(textos(&t, false), ["Pronto para atualizar o PR #12"]);
        t.pronto_para_atualizar = false;
        // A revisão de quem tem acesso e o comentário de quem não tem: "Mandar ao chat" só os deles.
        t.pr.as_mut().unwrap().esperando = vec![
            EsperaPr::Revisao { ids: vec![7, 8], autores: vec!["VictorMenegate".into()] },
            EsperaPr::Terceiro { id: 9, autor: "estranho".into(), url: String::new() },
            EsperaPr::Revisao { ids: Vec::new(), autores: vec!["x".into()] },
            EsperaPr::MesclaRecusada { motivo: "Base branch was modified".into() },
        ];
        let esperas = esperas_da_tarefa(&t, agora, false);
        assert_eq!(esperas[0].0, "Revisão no PR #12 (2 comentários de @VictorMenegate)");
        assert_eq!(esperas[0].2, AcaoDaEspera::MandarComentarios(vec![7, 8]));
        assert_eq!(esperas[1].0, "Comentário de @estranho (sem acesso ao repositório) no PR #12");
        assert_eq!(esperas[1].2, AcaoDaEspera::MandarComentarios(vec![9]));
        assert_eq!((esperas[2].0.as_str(), &esperas[2].2), ("Revisão no PR #12: mudanças pedidas por @x", &AcaoDaEspera::Ver));
        assert_eq!(esperas[3].0, "O GitHub não aceitou mesclar o PR #12: Base branch was modified");
        // Cada item do motor vai junto, para o × tirar só ele.
        assert!(esperas.iter().all(|e| e.3.is_some()));
        // O commit de fora e os checks parados há 2 h, conferidos com o PR de agora.
        let p = t.pr.as_mut().unwrap();
        p.commits_de_fora = vec!["abc1234".into()];
        p.checks = ResumoChecks { estado: EstadoChecks::Pendente, total: 4, terminados: 1, falhos: 0, pendentes: 3 };
        p.checks_progresso_em = agora - 3 * 3600;
        p.esperando = vec![EsperaPr::CommitDeFora { shas: vec!["abc1234".into()] }, EsperaPr::ChecksParados];
        assert_eq!(textos(&t, false), ["Chegou commit de fora no PR #12", "Checks parados há 3 h no PR #12"]);
        // Parado de acompanhar: nada de checks parados; o commit de fora trazido, nada dele.
        t.pr.as_mut().unwrap().acompanhando = false;
        t.pr.as_mut().unwrap().commits_de_fora.clear();
        assert!(esperas_da_tarefa(&t, agora, false).is_empty());
    }

    #[test]
    fn link_permitido_pela_origem() {
        use OrigemDoTexto::*;
        assert!(link_permitido("https://github.com/o/r/pull/1", Terceiros));
        assert!(link_permitido("HTTP://exemplo.com", Terceiros));
        assert!(!link_permitido("file:///etc/passwd", Terceiros));
        assert!(!link_permitido("file:///etc/passwd", Assistente));
        assert!(!link_permitido("javascript:alert(1)", Assistente));
        assert!(!link_permitido("/home/victor/.ssh/id_ed25519", Terceiros));
        assert!(!link_permitido("vitral:pr/o/r/12", Terceiros));
        assert!(link_permitido("vitral:pr/o/r/12", Assistente));
        assert!(link_permitido("vitral:issue/linear/ENG-123", Assistente));
        assert!(!link_permitido("vitral:qualquer/coisa", Assistente));
        assert!(!link_permitido("https://", Terceiros));
        assert_eq!(ler_link_vitral("vitral:pr/o/r/12"), Some(LinkVitral::Pr { repositorio: "o/r".into(), numero: 12 }));
        assert_eq!(
            ler_link_vitral("vitral:issue/github/o/r/45"),
            Some(LinkVitral::IssueGithub { repositorio: "o/r".into(), numero: 45 })
        );
        assert_eq!(ler_link_vitral("vitral:issue/linear/ENG-7"), Some(LinkVitral::IssueLinear("ENG-7".into())));
        assert_eq!(ler_link_vitral("vitral:issue/linear/eng-7"), None);
        assert_eq!(ler_link_vitral("vitral:pr/o/r/0"), None);
    }

    #[test]
    fn o_pedido_de_conflito_cita_a_base_e_os_arquivos() {
        assert_eq!(
            texto_do_conflito("origin/main", &["src/a.rs".into(), "b.rs".into()]),
            "Deu conflito com o que entrou em origin/main (arquivos src/a.rs, b.rs). Rode `git merge origin/main` na sua cópia, resolva mantendo as duas mudanças, faça commit e rode os testes; depois avise."
        );
        assert!(!texto_do_conflito("origin/main", &[]).contains("arquivos"));
    }

    fn refs() -> Referencias {
        Referencias {
            repositorio: Some("VictorMenegate/HUB".into()),
            prs: vec![("VictorMenegate/HUB".into(), 12)],
            issues_github: vec![("VictorMenegate/HUB".into(), 45)],
            linear: vec!["ENG-123".into()],
        }
    }

    #[test]
    fn ligar_referencias_so_liga_o_que_existe() {
        let r = refs();
        assert_eq!(
            ligar_referencias("Abri o PR #12 e o #13; fecha a #45 e a ENG-123 (a ENG-999 não).", &r),
            "Abri o [PR #12](vitral:pr/VictorMenegate/HUB/12) e o #13; fecha a [#45](vitral:issue/github/VictorMenegate/HUB/45) e a [ENG-123](vitral:issue/linear/ENG-123) (a ENG-999 não)."
        );
        assert_eq!(
            ligar_referencias("Veja https://github.com/VictorMenegate/HUB/pull/12.", &r),
            "Veja [PR #12](vitral:pr/VictorMenegate/HUB/12)."
        );
        assert_eq!(
            ligar_referencias("Outro: https://github.com/outro/repo/pull/12", &r),
            "Outro: https://github.com/outro/repo/pull/12"
        );
        // Título, código em linha, bloco de código e link que já existe ficam como estão.
        assert_eq!(ligar_referencias("# Título", &r), "# Título");
        assert_eq!(ligar_referencias("## 12 passos e o #12", &r), "## 12 passos e o #12");
        assert_eq!(ligar_referencias("rode `git log #12` e `ENG-123`", &r), "rode `git log #12` e `ENG-123`");
        assert_eq!(ligar_referencias("```\nPR #12\n```\n#12", &r), "```\nPR #12\n```\n[#12](vitral:pr/VictorMenegate/HUB/12)");
        assert_eq!(ligar_referencias("[o PR #12](https://exemplo.com)", &r), "[o PR #12](https://exemplo.com)");
        // Palavras que só parecem: "a#12", "#12a", "XENG-123".
        assert_eq!(ligar_referencias("a#12 #12a XENG-123", &r), "a#12 #12a XENG-123");
        // Sem nada conhecido, o texto volta igual.
        assert_eq!(ligar_referencias("PR #12", &Referencias::default()), "PR #12");
        // Acentos e emojis não quebram a leitura.
        assert_eq!(ligar_referencias("ação #12 ✓", &r), "ação [#12](vitral:pr/VictorMenegate/HUB/12) ✓");
    }
}
