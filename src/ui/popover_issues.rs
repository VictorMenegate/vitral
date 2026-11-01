//! O popover das issues de um projeto (Linear e GitHub), aberto pelos botões do cabeçalho da
//! conversa (e pelo chip da issue ligada ao chat): a lista com os filtros e a seleção de até 3, o
//! detalhe com o popover de estado e as ações ("Iniciar agente", "Só a cópia", "Anexar a esta
//! conversa"), e os estados vazios (conectar o Linear daqui, o chaveiro, o gh sem login, nada no
//! filtro). Tudo é lido do motor a cada desenho; a lista e o detalhe atualizam ao abrir e no ⟳,
//! sem consulta periódica (parado, o popover não redesenha nada).
//!
//! Também moram aqui o anexo de issue na caixa (o chip acima do texto, com o bloco que o motor lê
//! antes do envio) e o chip das issues anexadas na bolha do usuário.

use std::{
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant, SystemTime},
};

use gpui_kit::component::{
    Disableable as _, Selectable, Sizable as _, Size, StyledExt as _, WindowExt as _,
    button::Button,
    checkbox::Checkbox,
    h_flex,
    input::{InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    spinner::Spinner,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

use super::{
    Janela,
    componentes::{
        Alerta, BotaoRedondo, CabecalhoDeSecao, CartaoDeLista, Chip, Confirmacao, ControleSegmentado, EstadoVazio,
        GRUPO_DO_CARTAO, GatilhoChip, LarguraPainel, LarguraPopover, Marca, Segmento, Selo, Simbolo, TamanhoBotao, TamanhoChip, Tom,
        TomConfirmacao, VarianteRedonda, aviso, barra_de_rolagem, botao_fantasma, botao_icone, botao_primario,
        campo_de_busca, confirmar, contagem, desenho_teclas, icone, popover,
    },
    conexoes::abrir_conectar_linear,
    gravar_interface, sem_arrastar, tempo_relativo,
};
use crate::{
    issues::{
        EstadoIssue, FiltroIssues, FonteIssue, IssueDetalhe, IssueLigada, IssueResumo, Recorte, Referencia, TipoEstado,
        aplicar_filtro_local, parse_referencia,
    },
    linear::DadosDoTime,
    mcp::Resposta,
    modelo::{Chat, ID_MESTRE, Papel as PapelDoChat},
    motor::{
        AnexoIssue, Carregando, EscopoIssues, EstadoConexao, EstadoGithub, ModoIssue, Motor, OpcoesDaIssue, Provedor,
        ligada_de,
    },
    remoto::EstadoRede,
    tema::{Cores, Lucide, Tipo as _, esp, r, raio, tam},
};

actions!(vitral, [VoltarIssues]);

/// Até quantas issues de uma vez ("Iniciar agentes (n)").
pub const MAXIMO_DO_LOTE: usize = 3;
const NO_MAXIMO: &str = "No máximo 3 de uma vez";

/// O que o popover de estado de uma issue do GitHub manda ao `mudar_estado_issue` (no Linear vai
/// o id do estado do time).
pub const GITHUB_ABERTA: &str = "aberta";
pub const GITHUB_CONCLUIDA: &str = "concluida";
pub const GITHUB_NAO_PLANEJADA: &str = "nao_planejada";

/// O popover vai até min(640, 80% da janela) de altura; abaixo de 420, os menus de dentro (o time,
/// o estado) não cabem nele, e um clique num menu que passa da borda fecharia o popover junto.
const ALTURA_MAXIMA: f32 = 640.;
const ALTURA_MINIMA: f32 = 420.;
/// Quantos comentários o detalhe mostra (os últimos).
const COMENTARIOS: usize = 5;

/// O que abriu o popover no cabeçalho: um dos dois botões ou o chip da issue ligada ao chat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gatilho {
    Linear,
    Github,
    Chip,
}

/// De onde o popover foi aberto.
#[derive(Clone, Debug, PartialEq)]
pub struct Origem {
    pub painel: u64,
    pub chat: String,
    /// No Mestre o projeto vem do chip "Projeto".
    pub mestre: bool,
    /// Chat de projeto (ou avulso): "Anexar a esta conversa" vale.
    pub pode_anexar: bool,
}

enum Tela {
    Lista,
    Detalhe(IssueResumo),
}

/// O que fica lembrado de cada projeto no `interface.json` ("issues"): o filtro de cada fonte e o
/// repositório escolhido, num projeto com mais de um. A fonte vem do botão que abriu.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lembrado {
    pub linear: FiltroIssues,
    pub github: FiltroIssues,
    pub repositorio: Option<String>,
}

/// Um repositório do GitHub com issues numa pasta do projeto.
#[derive(Clone, Debug, PartialEq)]
struct RepoDaPasta {
    pasta: PathBuf,
    repositorio: String,
}

/// Uma issue anexada na caixa, com o bloco que vai ao Claude (`None` enquanto o motor lê).
#[derive(Clone, Debug)]
pub struct AnexoDeIssue {
    pub issue: IssueResumo,
    pub repositorio: Option<String>,
    pub bloco: Option<Result<String, String>>,
}

// ---------- funções puras ----------

/// A mesma issue (a fonte e o id; a chave muda quando a issue troca de time no Linear).
fn mesma(a: &IssueResumo, b: &IssueResumo) -> bool {
    a.fonte == b.fonte && a.id == b.id
}

/// Marca ou desmarca a issue no lote; a 4ª não entra. Devolve se mudou.
fn alternar_no_lote(lote: &mut Vec<IssueResumo>, issue: &IssueResumo) -> bool {
    if let Some(i) = lote.iter().position(|s| mesma(s, issue)) {
        lote.remove(i);
        return true;
    }
    if lote.len() >= MAXIMO_DO_LOTE {
        return false;
    }
    lote.push(issue.clone());
    true
}

/// O nome da prioridade do Linear (0 é sem prioridade).
fn nome_da_prioridade(p: u8) -> Option<&'static str> {
    match p {
        1 => Some("Urgente"),
        2 => Some("Alta"),
        3 => Some("Média"),
        4 => Some("Baixa"),
        _ => None,
    }
}

/// A data ISO do Linear e do GitHub em segundos desde 1970.
fn instante(iso: &str) -> Option<u64> {
    let data = chrono::DateTime::parse_from_rfc3339(iso.trim()).ok()?;
    u64::try_from(data.timestamp()).ok()
}

/// "há 3 d", "há 5 min", "agora".
fn idade(iso: &str) -> Option<String> {
    let quando = tempo_relativo(instante(iso)?);
    Some(if quando == "agora" { quando } else { format!("há {quando}") })
}

/// O subtítulo da linha: chave, estado, prioridade, uma etiqueta e a idade.
fn subtitulo_da_issue(issue: &IssueResumo, estado: &EstadoIssue, idade: Option<String>) -> String {
    let mut partes = vec![issue.chave.clone()];
    if !estado.nome.is_empty() {
        partes.push(estado.nome.clone());
    }
    if let Some(p) = issue.prioridade.and_then(nome_da_prioridade) {
        partes.push(p.to_string());
    }
    if let Some(e) = issue.etiquetas.first().filter(|e| !e.nome.is_empty()) {
        partes.push(e.nome.clone());
    }
    partes.extend(idade);
    partes.join(" · ")
}

/// "Victor · Alta · bug · Ciclo 14 · atualizada há 3 d", só leitura, no detalhe.
fn meta_do_detalhe(issue: &IssueResumo, detalhe: Option<&IssueDetalhe>) -> String {
    let mut partes = Vec::new();
    partes.push(match &issue.responsavel {
        Some(p) if !p.nome.is_empty() => p.nome.clone(),
        _ => "Sem responsável".to_string(),
    });
    if let Some(p) = issue.prioridade.and_then(nome_da_prioridade) {
        partes.push(p.to_string());
    }
    partes.extend(issue.etiquetas.iter().take(2).map(|e| e.nome.clone()).filter(|n| !n.is_empty()));
    if let Some(ciclo) = detalhe.and_then(|d| d.ciclo.clone()) {
        partes.push(ciclo);
    }
    if let Some(projeto) = detalhe.and_then(|d| d.projeto.clone()) {
        partes.push(projeto);
    }
    if let Some(i) = idade(&issue.atualizada) {
        partes.push(format!("atualizada {i}"));
    }
    partes.join(" · ")
}

/// As três escolhas do GitHub, no vocabulário comum.
fn estados_github() -> Vec<EstadoIssue> {
    let estado = |id: &str, nome: &str, tipo| EstadoIssue { id: id.into(), nome: nome.into(), tipo, cor: None };
    vec![
        estado(GITHUB_ABERTA, "Aberta", TipoEstado::Aberta),
        estado(GITHUB_CONCLUIDA, "Fechada (concluída)", TipoEstado::Concluida),
        estado(GITHUB_NAO_PLANEJADA, "Fechada (não planejada)", TipoEstado::Cancelada),
    ]
}

/// O estado do GitHub que o popover marca: a issue vem com "OPEN"/"CLOSED" e o tipo.
fn id_github(estado: &EstadoIssue) -> &'static str {
    match estado.tipo {
        TipoEstado::Aberta => GITHUB_ABERTA,
        TipoEstado::Concluida | TipoEstado::Fechada => GITHUB_CONCLUIDA,
        _ => GITHUB_NAO_PLANEJADA,
    }
}

/// A ordem do fluxo no Linear (triagem, backlog, a fazer, em andamento, concluída, cancelada).
fn ordem_do_tipo(t: TipoEstado) -> u8 {
    match t {
        TipoEstado::Triagem => 0,
        TipoEstado::Backlog => 1,
        TipoEstado::AFazer | TipoEstado::Aberta => 2,
        TipoEstado::EmAndamento => 3,
        TipoEstado::Concluida | TipoEstado::Fechada => 4,
        TipoEstado::Cancelada => 5,
        TipoEstado::Duplicada => 6,
    }
}

/// Os estados de um time do Linear na ordem do fluxo, e a posição dentro de cada tipo.
fn estados_do_time(dados: &DadosDoTime) -> Vec<EstadoIssue> {
    let mut estados: Vec<_> = dados.estados.iter().collect();
    estados.sort_by(|a, b| {
        ordem_do_tipo(a.estado.tipo).cmp(&ordem_do_tipo(b.estado.tipo)).then(a.posicao.total_cmp(&b.posicao))
    });
    estados.into_iter().map(|e| e.estado.clone()).collect()
}

/// "#rrggbb" do Linear (a cor que o usuário escolheu para o estado ou a etiqueta).
fn cor_do_linear(texto: &str) -> Option<Hsla> {
    let hex = texto.trim().strip_prefix('#')?;
    (hex.len() == 6).then_some(())?;
    Some(rgb(u32::from_str_radix(hex, 16).ok()?).into())
}

/// Só `http` e `https` saem para o navegador: um link de texto de terceiros (a descrição, um
/// comentário) com outro esquema não abre nada.
pub fn link_seguro(url: &str) -> bool {
    let url = url.trim();
    ["http://", "https://"].iter().any(|p| url.len() > p.len() && url[..p.len()].eq_ignore_ascii_case(p))
}

/// As imagens do markdown viram link (`![x](u)` → `[imagem: x](u)`): o popover não baixa nada de
/// fora sozinho.
fn imagens_viram_link(texto: &str) -> String {
    let mut saida = String::with_capacity(texto.len());
    let mut resto = texto;
    while let Some(i) = resto.find("![") {
        saida.push_str(&resto[..i]);
        let depois = &resto[i + 2..];
        // O texto alternativo vai até o `](`.
        match depois.find("](") {
            Some(fim) if !depois[..fim].contains('\n') => {
                let alt = depois[..fim].trim();
                saida.push_str(&format!("[imagem{}](", if alt.is_empty() { String::new() } else { format!(": {alt}") }));
                resto = &depois[fim + 2..];
            }
            _ => {
                saida.push_str("![");
                resto = depois;
            }
        }
    }
    saida.push_str(resto);
    saida
}

/// O recorte que vale para a fonte: no GitHub, Minhas e Todas; no Linear, sem time, só Minhas.
fn recorte_valido(fonte: FonteIssue, recorte: Recorte, tem_time: bool, tem_ciclos: bool) -> Recorte {
    match (fonte, recorte) {
        (FonteIssue::Github, Recorte::Minhas | Recorte::Todas) => recorte,
        (FonteIssue::Github, _) => Recorte::Todas,
        (FonteIssue::Linear, Recorte::Todas) if tem_time => Recorte::DoTime,
        (FonteIssue::Linear, Recorte::CicloAtual) if tem_time && !tem_ciclos => Recorte::DoTime,
        (FonteIssue::Linear, Recorte::DoTime | Recorte::CicloAtual) if tem_time => recorte,
        (FonteIssue::Linear, _) => Recorte::Minhas,
    }
}

/// O título do estado vazio da lista: "Nenhuma issue aberta atribuída a você".
fn texto_sem_itens(fonte: FonteIssue, filtro: &FiltroIssues, time: Option<&str>, repo: Option<&str>, busca: &str) -> String {
    let estado = if filtro.fechadas { "fechada nos últimos 30 dias" } else { "aberta" };
    if !busca.is_empty() {
        return format!("Nenhuma issue {estado} com “{busca}”");
    }
    let onde = match (fonte, filtro.recorte) {
        (_, Recorte::Minhas) => "atribuída a você".to_string(),
        (FonteIssue::Linear, Recorte::CicloAtual) => "no ciclo atual".to_string(),
        (FonteIssue::Linear, _) => time.map_or("no time".to_string(), |t| format!("no time {t}")),
        (FonteIssue::Github, _) => repo.map_or("no repositório".to_string(), |r| format!("em {r}")),
    };
    format!("Nenhuma issue {estado} {onde}")
}

/// "12 de 12 · atualizado há 1 min".
fn texto_do_rodape(mostradas: usize, total: usize, atualizada: Option<Duration>, tem_mais: bool) -> String {
    let mut texto = format!("{mostradas} de {total}{}", if tem_mais { "+" } else { "" });
    if let Some(passou) = atualizada {
        let s = passou.as_secs();
        let quando = if s < 60 { "agora".to_string() } else if s < 3600 { format!("há {} min", s / 60) } else { format!("há {} h", s / 3600) };
        texto.push_str(&format!(" · atualizado {quando}"));
    }
    texto
}

/// O texto que "Só a cópia" põe na caixa, selecionado.
fn texto_da_copia(chave: &str) -> String {
    format!("Resolva a issue {chave}.")
}

/// O que "Mandar ao Mestre" põe na caixa dele.
fn pedido_ao_mestre(chave_do_projeto: &str, chave: &str) -> String {
    format!("#{chave_do_projeto} pegue a {chave}")
}

/// A issue ligada (no chat ou no cabeçalho) como uma linha da lista, para abrir o detalhe. No
/// GitHub, o id leva o repositório ("dono/nome#45"): o detalhe não depende de uma lista aberta.
fn resumo_da_ligada(ligada: &IssueLigada) -> IssueResumo {
    let id = match (&ligada.fonte, &ligada.repositorio) {
        (FonteIssue::Github, Some(repo)) if !ligada.id.contains('#') => format!("{repo}#{}", ligada.id.trim_start_matches('#')),
        _ => ligada.id.clone(),
    };
    IssueResumo {
        fonte: ligada.fonte,
        id,
        chave: ligada.chave.clone(),
        titulo: ligada.titulo.clone(),
        url: ligada.url.clone(),
        ..Default::default()
    }
}

/// A issue da linha como a ligação (o anexo, "Ligar a ENG-123 a este chat?"): a do motor, com o
/// repositório da lista quando a URL não disse.
fn ligada_com_repositorio(issue: &IssueResumo, repositorio: Option<String>) -> IssueLigada {
    let mut ligada = ligada_de(issue);
    if ligada.fonte == FonteIssue::Github && ligada.repositorio.is_none() {
        ligada.repositorio = repositorio;
    }
    ligada
}

/// Os anexos de issue prontos para ir, ou por que ainda não: lendo, ou o motor não conseguiu ler.
fn issues_prontas(anexos: &[AnexoDeIssue]) -> Result<Vec<AnexoIssue>, String> {
    anexos
        .iter()
        .map(|a| match &a.bloco {
            Some(Ok(bloco)) => Ok(AnexoIssue { issue: ligada_com_repositorio(&a.issue, a.repositorio.clone()), bloco: bloco.clone() }),
            Some(Err(erro)) => Err(format!(
                "Não consegui ler a {} ({erro}). Tire o anexo ou clique nele para tentar de novo.",
                a.issue.chave
            )),
            None => Err(format!("Ainda lendo a {}; envie de novo em um instante.", a.issue.chave)),
        })
        .collect()
}

/// Lê o que ficou lembrado das issues no `interface.json`.
fn ler_lembrados() -> (HashMap<String, Lembrado>, Option<String>) {
    let interface = super::zoom::interface();
    let lembrados = interface
        .as_ref()
        .and_then(|i| i.get("issues"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let mestre = interface.as_ref().and_then(|i| i.get("issues_do_mestre")?.as_str().map(str::to_string));
    (lembrados, mestre)
}

// ---------- o que se lê do motor ----------

/// Os repositórios do GitHub com issues nas pastas do projeto (pelo remote que vale em cada uma).
fn repos_com_issues(m: &Motor, projeto: &str) -> Vec<RepoDaPasta> {
    let mut repos: Vec<RepoDaPasta> = Vec::new();
    for pasta in m.pastas_do(projeto) {
        let info = m.info_github(&pasta.caminho);
        let Some(info) = info.valor() else { continue };
        if info.tem_issues && !repos.iter().any(|r| r.repositorio == info.repositorio) {
            repos.push(RepoDaPasta { pasta: pasta.caminho.clone(), repositorio: info.repositorio.clone() });
        }
    }
    repos
}

/// Alguma pasta do projeto tem um remote do GitHub (só o git local, sem rede).
fn tem_github(m: &Motor, projeto: &str) -> bool {
    m.pastas_do(projeto).iter().any(|p| m.remotos_github(&p.caminho).valor().is_some_and(|r| !r.is_empty()))
}

/// O time do Linear do projeto: o id e a chave.
fn time_do_projeto(m: &Motor, projeto: &str) -> Option<(String, String)> {
    let linear = m.projeto(projeto)?.linear.as_ref()?;
    (!linear.time_id.is_empty()).then(|| (linear.time_id.clone(), linear.time_chave.clone()))
}

/// O chat que trabalha na issue e o PR dele, se houver.
fn chat_da_issue(m: &Motor, issue: &IssueResumo) -> Option<(String, u64, Option<u64>)> {
    let tarefa = m.tarefa_da_issue(issue.fonte, &issue.id)?;
    let chat = m.chats.iter().find(|c| c.tarefa == Some(tarefa))?;
    Some((chat.id.clone(), tarefa, m.pr_da_tarefa(tarefa).map(|p| p.numero)))
}

/// O ponto (Linear, na cor do estado) ou o ícone (GitHub) da issue, em 16.
fn avatar_da_issue(fonte: FonteIssue, estado: &EstadoIssue, window: &Window) -> AnyElement {
    match fonte {
        FonteIssue::Github => {
            let (nome, cor) = match estado.tipo {
                TipoEstado::Aberta | TipoEstado::AFazer | TipoEstado::EmAndamento | TipoEstado::Backlog | TipoEstado::Triagem => {
                    (Lucide::CircleDot, Cores::verde())
                }
                TipoEstado::Concluida => (Lucide::CircleCheck, Cores::violeta()),
                _ => (Lucide::CircleCheck, Cores::tenue()),
            };
            div().flex_none().size(r(16.)).flex().items_center().justify_center().text_color(cor).child(icone(nome, 16., window)).into_any_element()
        }
        FonteIssue::Linear => div()
            .flex_none()
            .size(r(16.))
            .flex()
            .items_center()
            .justify_center()
            .child(ponto_do_estado(estado))
            .into_any_element(),
    }
}

/// O ponto do estado do Linear: a cor do usuário, ou uma do tipo quando ele não tem; o "a fazer" e
/// o backlog vazados. O texto do estado nunca leva a cor (pode não ter contraste).
fn ponto_do_estado(estado: &EstadoIssue) -> Selo {
    let padrao = match estado.tipo {
        TipoEstado::EmAndamento => Cores::ambar(),
        TipoEstado::Concluida | TipoEstado::Fechada => Cores::violeta(),
        TipoEstado::Aberta => Cores::verde(),
        _ => Cores::tenue(),
    };
    let cor = estado.cor.as_deref().and_then(cor_do_linear).unwrap_or(padrao);
    let selo = Selo::ponto(cor).lado(10.);
    if matches!(estado.tipo, TipoEstado::Backlog | TipoEstado::AFazer | TipoEstado::Triagem) { selo.vazado() } else { selo }
}

/// O símbolo da fonte num chip (a marca do Linear ou o CircleDot do GitHub).
fn simbolo_da_fonte(fonte: FonteIssue) -> Simbolo {
    match fonte {
        FonteIssue::Linear => Simbolo::Marca(Marca::Linear),
        FonteIssue::Github => Simbolo::icone_cor(Lucide::CircleDot, Cores::verde()),
    }
}

/// O chip pequeno de uma issue anexada, na bolha do usuário: a marca e a chave.
pub fn chips_da_bolha(chave: &str, referencias: &[String], window: &Window) -> Option<AnyElement> {
    (!referencias.is_empty()).then(|| {
        h_flex()
            .flex_wrap()
            .gap(esp::E6)
            .children(referencias.iter().enumerate().map(|(i, referencia)| {
                let linear = matches!(parse_referencia(referencia), Some(Referencia::Linear { .. }));
                let dica = SharedString::from(format!(
                    "{referencia} foi anexada a esta mensagem\nO texto dela foi junto, como dados de fora"
                ));
                h_flex()
                    .id(SharedString::from(format!("ref-{chave}-{i}")))
                    .h(r(24.))
                    .px(esp::E8)
                    .gap(esp::E6)
                    .rounded(raio::PILULA)
                    .bg(Cores::recuo())
                    .metadado()
                    .child(if linear {
                        Marca::Linear.disco(14.).into_any_element()
                    } else {
                        icone(Lucide::CircleDot, 14., window).text_color(Cores::verde()).into_any_element()
                    })
                    .child(div().text_color(Cores::texto()).font_weight(FontWeight::MEDIUM).child(referencia.clone()))
                    .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            }))
            .into_any_element()
    })
}

// ---------- o popover ----------

/// O estado do popover (um só para a janela: abre de um botão por vez).
pub struct PopoverIssues {
    motor: Entity<Motor>,
    janela: WeakEntity<Janela>,
    origem: Option<Origem>,
    projeto: String,
    fonte: FonteIssue,
    lembrados: HashMap<String, Lembrado>,
    /// O projeto que o Mestre olhou por último.
    projeto_do_mestre: Option<String>,
    busca: Entity<InputState>,
    /// A busca no servidor em vigor (Enter com 3 ou mais caracteres).
    termo: Option<String>,
    selecionadas: Vec<IssueResumo>,
    tela: Tela,
    /// O estado escolhido no popover de estado, até a lista voltar (volta ao anterior no erro).
    otimistas: HashMap<(FonteIssue, String), EstadoIssue>,
    /// O que deu errado na última ação (criar o chat), embaixo do detalhe.
    erro: Option<String>,
    rolagem: ScrollHandle,
    foco: FocusHandle,
    _assinaturas: Vec<Subscription>,
}

impl PopoverIssues {
    pub fn new(motor: Entity<Motor>, janela: WeakEntity<Janela>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let busca = cx.new(|cx| InputState::new(window, cx).placeholder("Filtrar…"));
        let assinaturas = vec![cx.subscribe(&busca, |este: &mut Self, _, evento: &InputEvent, cx| match evento {
            InputEvent::Change => {
                // Mexer no texto volta à lista do filtro; a busca no servidor é só com o Enter.
                let texto = este.busca.read(cx).value().trim().to_string();
                if este.termo.as_deref().is_some_and(|t| t != texto) {
                    este.termo = None;
                }
                este.rolagem.set_offset(point(px(0.), px(0.)));
                cx.notify();
            }
            InputEvent::PressEnter { .. } => este.buscar_no_servidor(cx),
            _ => {}
        })];
        let (lembrados, projeto_do_mestre) = ler_lembrados();
        Self {
            motor,
            janela,
            origem: None,
            projeto: String::new(),
            fonte: FonteIssue::Linear,
            lembrados,
            projeto_do_mestre,
            busca,
            termo: None,
            selecionadas: Vec::new(),
            tela: Tela::Lista,
            otimistas: HashMap::new(),
            erro: None,
            rolagem: ScrollHandle::new(),
            foco: cx.focus_handle(),
            _assinaturas: assinaturas,
        }
    }

    pub fn foco(&self) -> &FocusHandle {
        &self.foco
    }

    /// Abre na lista (ou no detalhe da issue do chip). No Mestre, o projeto é o último que ele
    /// olhou; a lista e o detalhe atualizam.
    pub fn abrir(
        &mut self,
        origem: Origem,
        projeto: String,
        fonte: FonteIssue,
        detalhe: Option<IssueResumo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let projeto = if origem.mestre {
            let m = self.motor.read(cx);
            self.projeto_do_mestre
                .clone()
                .filter(|p| m.projeto(p).is_some_and(|p| !p.arquivado))
                .or_else(|| {
                    // Pelo botão do GitHub, o primeiro projeto que tem GitHub.
                    let visiveis = m.projetos_visiveis();
                    let com_github = visiveis.iter().find(|p| fonte == FonteIssue::Github && tem_github(m, &p.id));
                    com_github.or(visiveis.first()).map(|p| p.id.clone())
                })
                .unwrap_or_default()
        } else {
            projeto
        };
        if self.projeto != projeto {
            self.selecionadas.clear();
            self.termo = None;
            self.busca.update(cx, |b, cx| b.set_value("", window, cx));
        }
        self.origem = Some(origem);
        self.projeto = projeto;
        self.fonte = fonte;
        self.tela = match detalhe {
            Some(issue) => Tela::Detalhe(issue),
            None => Tela::Lista,
        };
        self.otimistas.clear();
        self.erro = None;
        self.rolagem.set_offset(point(px(0.), px(0.)));
        self.atualizar(cx);
        cx.notify();
    }

    /// O ⟳ (e a abertura): a lista e, no detalhe, a issue.
    fn atualizar(&mut self, cx: &mut Context<Self>) {
        let (escopo, filtro) = {
            let m = self.motor.read(cx);
            (self.escopo(m), self.filtro_da_lista(m))
        };
        let detalhe = match &self.tela {
            Tela::Detalhe(issue) => Some((issue.fonte, issue.id.clone())),
            Tela::Lista => None,
        };
        self.motor.update(cx, |m, cx| {
            m.atualizar_issues(&escopo, &filtro, cx);
            if let Some((fonte, id)) = &detalhe {
                m.atualizar_detalhe_issue(*fonte, id, cx);
            }
        });
    }

    fn lembrado(&self) -> Lembrado {
        self.lembrados.get(&self.projeto).cloned().unwrap_or_default()
    }

    /// Muda o que fica lembrado do projeto e grava no `interface.json` (sem a busca no servidor).
    fn lembrar(&mut self, mudar: impl FnOnce(&mut Lembrado), cx: &mut Context<Self>) {
        let lembrado = self.lembrados.entry(self.projeto.clone()).or_default();
        mudar(lembrado);
        lembrado.linear.termo = None;
        lembrado.github.termo = None;
        if let Ok(valor) = serde_json::to_value(&self.lembrados) {
            gravar_interface("issues", valor, cx);
        }
        self.rolagem.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    fn repositorio(&self, m: &Motor) -> Option<RepoDaPasta> {
        let repos = repos_com_issues(m, &self.projeto);
        let lembrado = self.lembrado().repositorio;
        repos.iter().find(|r| Some(&r.repositorio) == lembrado.as_ref()).or(repos.first()).cloned()
    }

    fn escopo(&self, m: &Motor) -> EscopoIssues {
        let repositorio = match self.fonte {
            FonteIssue::Github => self.repositorio(m).map(|r| r.repositorio),
            FonteIssue::Linear => None,
        };
        EscopoIssues { projeto: self.projeto.clone(), fonte: self.fonte, repositorio }
    }

    /// O time tem ciclos (sem a lista dos times, supõe que sim).
    fn time_tem_ciclos(&self, m: &Motor, time: &str) -> bool {
        m.times_linear().valor().and_then(|times| times.iter().find(|t| t.id == time).map(|t| t.ciclos)).unwrap_or(true)
    }

    /// O filtro que a lista lê: com a busca no servidor em vigor, a lista dela (o motor guarda o
    /// resultado em `FiltroIssues { termo, ..Default }`); senão, o lembrado.
    fn filtro_da_lista(&self, m: &Motor) -> FiltroIssues {
        match &self.termo {
            Some(termo) => FiltroIssues { termo: Some(termo.clone()), ..Default::default() },
            None => self.filtro(m),
        }
    }

    /// O filtro lembrado do projeto (o que os controles mostram).
    fn filtro(&self, m: &Motor) -> FiltroIssues {
        let lembrado = self.lembrado();
        let mut filtro = match self.fonte {
            FonteIssue::Linear => lembrado.linear,
            FonteIssue::Github => lembrado.github,
        };
        let time = time_do_projeto(m, &self.projeto);
        let ciclos = time.as_ref().is_some_and(|(id, _)| self.time_tem_ciclos(m, id));
        filtro.recorte = recorte_valido(self.fonte, filtro.recorte, time.is_some(), ciclos);
        filtro.termo = None;
        filtro
    }

    fn mudar_filtro(&mut self, mudar: impl FnOnce(&mut FiltroIssues), cx: &mut Context<Self>) {
        let fonte = self.fonte;
        self.lembrar(
            |l| match fonte {
                FonteIssue::Linear => mudar(&mut l.linear),
                FonteIssue::Github => mudar(&mut l.github),
            },
            cx,
        );
    }

    /// Enter com 3 ou mais caracteres: busca no servidor, além do filtro local.
    fn buscar_no_servidor(&mut self, cx: &mut Context<Self>) {
        let texto = self.busca.read(cx).value().trim().to_string();
        if texto.chars().count() < 3 || self.termo.as_deref() == Some(texto.as_str()) {
            return;
        }
        self.termo = Some(texto.clone());
        let escopo = self.escopo(self.motor.read(cx));
        self.motor.update(cx, |m, cx| m.buscar_issues(&escopo, texto, cx));
        cx.notify();
    }

    fn limpar_filtros(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.termo = None;
        self.busca.update(cx, |b, cx| b.set_value("", window, cx));
        self.mudar_filtro(|f| f.fechadas = false, cx);
    }

    fn mudar_fonte(&mut self, fonte: FonteIssue, cx: &mut Context<Self>) {
        if self.fonte == fonte {
            return;
        }
        self.fonte = fonte;
        self.termo = None;
        self.tela = Tela::Lista;
        self.rolagem.set_offset(point(px(0.), px(0.)));
        self.atualizar(cx);
        cx.notify();
    }

    fn mudar_projeto(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.projeto == projeto {
            return;
        }
        self.projeto = projeto.clone();
        self.projeto_do_mestre = Some(projeto.clone());
        gravar_interface("issues_do_mestre", serde_json::Value::String(projeto), cx);
        self.selecionadas.clear();
        self.termo = None;
        self.busca.update(cx, |b, cx| b.set_value("", window, cx));
        self.tela = Tela::Lista;
        self.atualizar(cx);
        cx.notify();
    }

    fn alternar(&mut self, issue: &IssueResumo, cx: &mut Context<Self>) {
        if alternar_no_lote(&mut self.selecionadas, issue) {
            cx.notify();
        }
    }

    fn ver_detalhe(&mut self, issue: IssueResumo, cx: &mut Context<Self>) {
        let (fonte, id) = (issue.fonte, issue.id.clone());
        self.tela = Tela::Detalhe(issue);
        self.erro = None;
        self.motor.update(cx, |m, cx| m.atualizar_detalhe_issue(fonte, &id, cx));
        cx.notify();
    }

    /// Esc: do detalhe volta à lista; da lista, fecha.
    fn voltar_ou_fechar(&mut self, cx: &mut Context<Self>) {
        match self.tela {
            Tela::Detalhe(_) => {
                self.tela = Tela::Lista;
                self.erro = None;
                cx.notify();
            }
            Tela::Lista => self.fechar(cx),
        }
    }

    fn fechar(&mut self, cx: &mut Context<Self>) {
        let _ = self.janela.update(cx, |j, cx| j.fechar_popover_issues(cx));
    }

    fn estado_de(&self, issue: &IssueResumo) -> EstadoIssue {
        self.otimistas.get(&(issue.fonte, issue.id.clone())).cloned().unwrap_or_else(|| issue.estado.clone())
    }

    /// O popover de estado: aplica na hora (otimista) e volta ao anterior se der erro.
    fn mudar_estado(&mut self, issue: IssueResumo, novo: EstadoIssue, window: &mut Window, cx: &mut Context<Self>) {
        if self.estado_de(&issue).id == novo.id && issue.fonte == FonteIssue::Linear {
            return;
        }
        let chave = (issue.fonte, issue.id.clone());
        self.otimistas.insert(chave.clone(), novo.clone());
        cx.notify();
        let resposta = self.motor.update(cx, |m, cx| m.mudar_estado_issue(issue.fonte, &issue.id, novo.id.clone(), cx));
        cx.spawn_in(window, async move |este, cx| {
            let resposta = resposta.recv().await.unwrap_or_else(|_| Resposta::erro("O Vitral fechou no meio."));
            let _ = este.update_in(cx, |este, window, cx| {
                if resposta.erro {
                    este.otimistas.remove(&chave);
                    let texto = format!("A {} continua como estava: {}", issue.chave, resposta.texto);
                    // Com o popover aberto, o aviso do topo ficaria atrás dele: o erro vai no detalhe.
                    let aberto = este.janela.upgrade().is_some_and(|j| j.read(cx).issues_aberto.is_some());
                    if aberto {
                        este.erro = Some(texto);
                    } else {
                        window.push_notification(aviso(Tom::Erro, texto), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A pasta da issue: no GitHub, a do repositório; no Linear, a lembrada (o motor decide sem
    /// ela; com mais de uma pasta e nada lembrado, o botão pergunta antes).
    fn pasta_da_issue(&self, m: &Motor, issue: &IssueResumo) -> Option<PathBuf> {
        match issue.fonte {
            FonteIssue::Github => self.repositorio(m).map(|r| r.pasta),
            FonteIssue::Linear => m.projeto(&self.projeto).and_then(|p| p.linear.as_ref()).and_then(|l| l.pasta.clone()),
        }
    }

    /// As pastas para o "Em qual pasta?", quando o motor diz que é preciso perguntar (no Linear,
    /// com mais de uma e nenhuma lembrada).
    fn pastas_a_perguntar(&self, m: &Motor, issue: &IssueResumo) -> Vec<PathBuf> {
        if !m.precisa_escolher_pasta(&self.projeto, issue) {
            return Vec::new();
        }
        m.pastas_do(&self.projeto).iter().map(|p| p.caminho.clone()).collect()
    }

    /// "Iniciar agente" e "Só a cópia": cria o chat, fecha o popover e abre o chat (Ctrl+clique,
    /// ao lado). No "Só a cópia", a caixa nasce com a issue anexada e o pedido selecionado.
    fn iniciar(
        &mut self,
        issue: IssueResumo,
        modo: ModoIssue,
        pasta: Option<PathBuf>,
        ao_lado: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (pasta, repositorio) = {
            let m = self.motor.read(cx);
            (pasta.or_else(|| self.pasta_da_issue(m, &issue)), self.repositorio(m).map(|r| r.repositorio))
        };
        let projeto = self.projeto.clone();
        let criado = self.motor.update(cx, |m, cx| m.chat_da_issue(&projeto, issue.clone(), modo, OpcoesDaIssue { pasta }, cx));
        match criado {
            Ok(chat) => {
                self.erro = None;
                self.selecionadas.retain(|s| !mesma(s, &issue));
                let _ = self.janela.update(cx, |j, cx| {
                    j.fechar_popover_issues(cx);
                    j.abrir_chat_da_issue(&chat, ao_lado, window, cx);
                    if modo == ModoIssue::SoCopia {
                        j.anexar_issue(&chat, issue.clone(), repositorio, cx);
                        j.por_pedido_na_caixa(&texto_da_copia(&issue.chave), true, window, cx);
                    }
                });
            }
            Err(erro) => {
                self.erro = Some(erro);
                cx.notify();
            }
        }
    }

    /// "Iniciar agentes (n)" e "Só as cópias (n)": um de cada vez no motor; o resultado vem num aviso.
    fn iniciar_lote(&mut self, modo: ModoIssue, window: &mut Window, cx: &mut Context<Self>) {
        let lote = std::mem::take(&mut self.selecionadas);
        if lote.is_empty() {
            return;
        }
        let projeto = self.projeto.clone();
        let recebe = self.motor.update(cx, |m, cx| m.chats_das_issues(&projeto, lote, modo, cx));
        self.fechar(cx);
        window
            .spawn(cx, async move |cx| {
                let Ok(resultado) = recebe.recv().await else { return };
                let _ = cx.update(|window, cx| {
                    let tom = if resultado.falhas.is_empty() {
                        Tom::Pronto
                    } else if resultado.criados.is_empty() {
                        Tom::Erro
                    } else {
                        Tom::Atencao
                    };
                    window.push_notification(aviso(tom, resultado.texto()), cx);
                });
            })
            .detach();
    }

    /// "Anexar a esta conversa": o chip na caixa do chat que abriu o popover.
    fn anexar(&mut self, issue: IssueResumo, window: &mut Window, cx: &mut Context<Self>) {
        let Some(origem) = self.origem.clone() else { return };
        let repositorio = self.repositorio(self.motor.read(cx)).map(|r| r.repositorio);
        let _ = self.janela.update(cx, |j, cx| {
            j.fechar_popover_issues(cx);
            j.anexar_issue(&origem.chat, issue, repositorio, cx);
            j.focar_caixa_do_painel(origem.painel, window, cx);
        });
    }

    /// "Mandar ao Mestre": abre o Mestre com o pedido na caixa (quem envia é você).
    fn mandar_ao_mestre(&mut self, issue: &IssueResumo, window: &mut Window, cx: &mut Context<Self>) {
        let chave = self.motor.read(cx).projeto(&self.projeto).map(|p| p.chave()).unwrap_or_default();
        let texto = pedido_ao_mestre(&chave, &issue.chave);
        let _ = self.janela.update(cx, |j, cx| {
            j.fechar_popover_issues(cx);
            j.abrir_chat_da_issue(ID_MESTRE, false, window, cx);
            j.por_pedido_na_caixa(&texto, false, window, cx);
        });
    }

    fn abrir_chat(&mut self, chat: String, ao_lado: bool, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.janela.update(cx, |j, cx| {
            j.fechar_popover_issues(cx);
            j.abrir_chat_da_issue(&chat, ao_lado, window, cx);
        });
    }

    // ---------- o desenho ----------

    /// O que toma o lugar da lista: o Linear sem conexão (ou sem a chave), o gh sem login, o
    /// projeto sem GitHub.
    fn bloqueio(&self, m: &Motor, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let conexoes = m.conexoes();
        let eu = cx.entity().downgrade();
        let motor = self.motor.clone();
        match self.fonte {
            FonteIssue::Linear => {
                if conexoes.linear.is_none() {
                    let (conectar_eu, conectar_motor) = (eu.clone(), motor.clone());
                    let conectar = botao_primario("issues-conectar", None, "Conectar…", TamanhoBotao::Barra, window, cx)
                        .on_click(move |_, window, cx| {
                            let painel = conectar_eu.upgrade().and_then(|p| p.read(cx).origem.as_ref().map(|o| o.painel));
                            let janela = conectar_eu.upgrade().map(|p| p.read(cx).janela.clone());
                            let _ = conectar_eu.update(cx, |p, cx| p.fechar(cx));
                            // Conectado, o popover volta já com as issues.
                            let voltar: Option<Rc<dyn Fn(&mut Window, &mut App)>> = match (painel, janela) {
                                (Some(painel), Some(janela)) => Some(Rc::new(move |window: &mut Window, cx: &mut App| {
                                    let _ = janela.update(cx, |j, cx| j.abrir_issues(painel, Gatilho::Linear, window, cx));
                                })),
                                _ => None,
                            };
                            abrir_conectar_linear(conectar_motor.clone(), voltar, window, cx);
                        });
                    let (agora_eu, agora_motor) = (eu.clone(), motor.clone());
                    let agora_nao = botao_fantasma("issues-agora-nao", None, "Agora não", TamanhoBotao::Barra, window, cx)
                        .tooltip("Esconde o botão do Linear\nVolta em Configurações › Conexões")
                        .on_click(move |_, _, cx| {
                            let _ = agora_eu.update(cx, |p, cx| p.fechar(cx));
                            agora_motor.update(cx, |m, cx| m.mudar_config(|c| c.esconder_botao_linear = true, cx));
                        });
                    return Some(
                        EstadoVazio::new(Lucide::Unplug, "Conecte o Linear para ver suas issues aqui")
                            .texto("Com uma chave pessoal de API, guardada no chaveiro do sistema.")
                            .acao(conectar)
                            .acao(agora_nao)
                            .into_any_element(),
                    );
                }
                let destravar = |id: &'static str, rotulo: &'static str| {
                    let motor = motor.clone();
                    botao_fantasma(id, None, rotulo, TamanhoBotao::Barra, window, cx).on_click(move |_, _, cx| {
                        let _ = motor.update(cx, |m, cx| m.destravar_chaveiro(cx));
                    })
                };
                let vazio = match &conexoes.chave {
                    EstadoConexao::Pronta | EstadoConexao::Desconectada => return None,
                    EstadoConexao::NaoLida | EstadoConexao::Lendo => carregando("Lendo a chave do Linear no chaveiro…", window),
                    EstadoConexao::ChaveiroTrancado => EstadoVazio::new(Lucide::LockKeyholeOpen, "O chaveiro está trancado")
                        .texto("A chave do Linear fica nele. O sistema pede a senha para destravar.")
                        .acao(destravar("issues-destravar", "Destravar"))
                        .into_any_element(),
                    EstadoConexao::ChaveiroIndisponivel(motivo) => {
                        EstadoVazio::new(Lucide::TriangleAlert, "O chaveiro do sistema não respondeu")
                            .texto(motivo.clone())
                            .acao(destravar("issues-chaveiro-de-novo", "Tentar de novo"))
                            .into_any_element()
                    }
                    EstadoConexao::SemChave => {
                        let motor = motor.clone();
                        let eu = eu.clone();
                        EstadoVazio::new(Lucide::Unplug, "A chave não está mais no chaveiro")
                            .texto("A conta do Linear continua nas Conexões; cole a chave de novo.")
                            .acao(botao_fantasma("issues-reconectar", None, "Conectar de novo…", TamanhoBotao::Barra, window, cx).on_click(
                                move |_, window, cx| {
                                    let _ = eu.update(cx, |p, cx| p.fechar(cx));
                                    abrir_conectar_linear(motor.clone(), None, window, cx);
                                },
                            ))
                            .into_any_element()
                    }
                };
                Some(vazio)
            }
            FonteIssue::Github => {
                let vazio = match &conexoes.github {
                    EstadoGithub::Conectado(_) | EstadoGithub::SemConexao | EstadoGithub::Erro(_) => {
                        if !repos_com_issues(m, &self.projeto).is_empty() {
                            return None;
                        }
                        EstadoVazio::new(Lucide::Github, "Nenhuma pasta deste projeto tem issues no GitHub")
                            .texto("Só aparecem os repositórios com as issues ligadas.")
                            .into_any_element()
                    }
                    EstadoGithub::Conferindo => carregando("Conferindo o login do GitHub…", window),
                    EstadoGithub::SemGh => EstadoVazio::new(Lucide::Github, "O gh não está instalado")
                        .texto("Instale com sudo dnf install gh e abra o popover de novo.")
                        .acao(
                            botao_fantasma("issues-copiar-gh", Some(Lucide::Copy.into()), "Copiar o comando", TamanhoBotao::Barra, window, cx)
                                .on_click(|_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string("sudo dnf install gh".into()))),
                        )
                        .into_any_element(),
                    EstadoGithub::SemLogin(_) => EstadoVazio::new(Lucide::Github, "Entre no GitHub para ver PRs e issues")
                        .texto("O gh abre o navegador num terminal; ao voltar, o Vitral confere sozinho.")
                        .acao(
                            botao_primario("issues-entrar-github", None, "Entrar no GitHub", TamanhoBotao::Barra, window, cx)
                                .on_click(move |_, _, cx| motor.update(cx, |m, _| m.entrar_no_github())),
                        )
                        .into_any_element(),
                };
                Some(vazio)
            }
        }
    }

    /// Fonte, projeto (no Mestre), time ou repositório e o ⟳.
    fn cabecalho(&self, m: &Motor, completo: bool, window: &Window, cx: &Context<Self>) -> AnyElement {
        let eu = cx.entity().downgrade();
        let mestre = self.origem.as_ref().is_some_and(|o| o.mestre);
        let linear = !m.config.esconder_botao_linear;
        let github = if mestre { tem_github(m, &self.projeto) } else { !repos_com_issues(m, &self.projeto).is_empty() };
        let fonte: AnyElement = if linear && (github || self.fonte == FonteIssue::Github) {
            let atual = if self.fonte == FonteIssue::Linear { 0 } else { 1 };
            let eu = eu.clone();
            ControleSegmentado::new(
                "issues-fonte",
                [Segmento::new("Linear"), Segmento::new("GitHub").icone(Lucide::Github)],
                atual,
                move |i, _, cx| {
                    let fonte = if i == 0 { FonteIssue::Linear } else { FonteIssue::Github };
                    let _ = eu.update(cx, |p, cx| p.mudar_fonte(fonte, cx));
                },
            )
            .with_size(Size::Small)
            .no_flutuante(true)
            .into_any_element()
        } else {
            let (simbolo, nome) = match self.fonte {
                FonteIssue::Linear => (Marca::Linear.disco(16.).into_any_element(), "Issues do Linear"),
                FonteIssue::Github => (icone(Lucide::Github, 16., window).text_color(Cores::texto()).into_any_element(), "Issues do GitHub"),
            };
            h_flex()
                .gap(esp::E8)
                .child(simbolo)
                .child(div().text_size(r(14.)).line_height(r(20.)).font_semibold().text_color(Cores::texto()).child(nome))
                .into_any_element()
        };
        let atualizar = {
            let eu = eu.clone();
            botao_icone("issues-atualizar", Lucide::RefreshCw, "Atualizar", TamanhoBotao::Barra, window, cx).on_click(move |_, _, cx| {
                let _ = eu.update(cx, |p, cx| {
                    p.otimistas.clear();
                    p.atualizar(cx);
                });
            })
        };
        let escolha = match self.fonte {
            FonteIssue::Linear => Some(self.chip_do_time(m, window, cx)),
            FonteIssue::Github => self.chip_do_repositorio(m, window, cx),
        };
        v_flex()
            .gap(esp::E8)
            // O rótulo do chip na coluna do resto (o chip tem 8 por dentro).
            .when(mestre, |s| s.child(h_flex().ml(r(-8.)).child(self.chip_do_projeto(m, window, cx))))
            .child(
                h_flex()
                    .gap(esp::E8)
                    .min_h(r(28.))
                    .child(div().flex_none().child(fonte))
                    .child(div().flex_1())
                    .when(completo, |s| s.children(escolha).child(atualizar)),
            )
            .into_any_element()
    }

    /// O chip de um menu: o rótulo, a seta e o desenho de aberto do Chip.
    fn chip_de_menu(
        id: &'static str,
        rotulo: SharedString,
        simbolo: Option<fn() -> Simbolo>,
        mudado: bool,
        dica: Option<&'static str>,
        window: &Window,
        cx: &App,
    ) -> GatilhoChip {
        let chip = |aberto: bool| {
            let mut chip = Chip::new(id, rotulo.clone()).tamanho(TamanhoChip::Medio).mudado(mudado).seta(true).aberto(aberto);
            if let Some(s) = simbolo {
                chip = chip.simbolo(s());
            }
            if let Some(d) = dica.filter(|_| !aberto) {
                chip = chip.dica(d);
            }
            chip.construir(window, cx)
        };
        GatilhoChip::new(chip(false), chip(true))
    }

    /// "Time: ENG ⌄" (ou "Time: Escolher ⌄"): escolher o time grava no projeto.
    fn chip_do_time(&self, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let time = time_do_projeto(m, &self.projeto);
        let rotulo: SharedString = match &time {
            Some((_, chave)) => format!("Time: {chave}").into(),
            None => "Time: Escolher".into(),
        };
        let times = m.times_linear();
        let atual = time.map(|(id, _)| id);
        let (eu, motor, projeto) = (cx.entity().downgrade(), self.motor.clone(), self.projeto.clone());
        Self::chip_de_menu("issues-time", rotulo, None, atual.is_some(), Some("O time do Linear deste projeto"), window, cx)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, window, _| {
                match &times {
                    Carregando::Nada | Carregando::Carregando(None) => {
                        menu = menu.item(PopupMenuItem::new("Lendo os times…").disabled(true));
                    }
                    Carregando::Erro(erro, None) => {
                        menu = menu.item(PopupMenuItem::new(erro.clone()).disabled(true));
                    }
                    _ => {
                        for time in times.valor().into_iter().flatten() {
                            let (eu, motor, projeto, escolhido) = (eu.clone(), motor.clone(), projeto.clone(), time.clone());
                            let nome = format!("{} · {}", time.chave, time.nome);
                            menu = menu.item(PopupMenuItem::new(nome).checked(atual.as_deref() == Some(time.id.as_str())).on_click(
                                move |_, _, cx| {
                                    motor.update(cx, |m, cx| m.escolher_time(&projeto, &escolhido, cx));
                                    let _ = eu.update(cx, |p, cx| {
                                        p.atualizar(cx);
                                        cx.notify();
                                    });
                                },
                            ));
                        }
                    }
                }
                menu.min_w(tam(window, 220.)).max_h(tam(window, 280.)).scrollable(true)
            })
            .into_any_element()
    }

    /// "nome ⌄" do repositório, num projeto com mais de um com issues.
    fn chip_do_repositorio(&self, m: &Motor, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let repos = repos_com_issues(m, &self.projeto);
        if repos.len() < 2 {
            return None;
        }
        let atual = self.repositorio(m)?.repositorio;
        let nome = atual.rsplit('/').next().unwrap_or(&atual).to_string();
        let eu = cx.entity().downgrade();
        Some(
            Self::chip_de_menu("issues-repo", nome.into(), Some(|| Simbolo::icone(Lucide::Github)), true, Some("O repositório das issues"), window, cx)
                .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, window, _| {
                    for repo in &repos {
                        let (eu, escolhido) = (eu.clone(), repo.repositorio.clone());
                        menu = menu.item(PopupMenuItem::new(repo.repositorio.clone()).checked(repo.repositorio == atual).on_click(
                            move |_, _, cx| {
                                let escolhido = escolhido.clone();
                                let _ = eu.update(cx, |p, cx| {
                                    p.lembrar(|l| l.repositorio = Some(escolhido), cx);
                                    p.atualizar(cx);
                                });
                            },
                        ));
                    }
                    menu.min_w(tam(window, 220.))
                })
                .into_any_element(),
        )
    }

    /// "Projeto: nome ⌄", no Mestre.
    fn chip_do_projeto(&self, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let projetos: Vec<(String, String)> = m.projetos_visiveis().iter().map(|p| (p.id.clone(), p.nome.clone())).collect();
        let rotulo: SharedString = format!("Projeto: {}", m.nome_do_projeto(&self.projeto)).into();
        let (eu, atual) = (cx.entity().downgrade(), self.projeto.clone());
        Self::chip_de_menu("issues-projeto", rotulo, None, true, Some("De qual projeto são as issues"), window, cx)
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |mut menu, window, _| {
                for (id, nome) in &projetos {
                    let (eu, escolhido) = (eu.clone(), id.clone());
                    menu = menu.item(PopupMenuItem::new(nome.clone()).checked(*id == atual).on_click(move |_, window, cx| {
                        let _ = eu.update(cx, |p, cx| p.mudar_projeto(escolhido.clone(), window, cx));
                    }));
                }
                menu.min_w(tam(window, 220.)).max_h(tam(window, 280.)).scrollable(true)
            })
            .into_any_element()
    }

    /// O recorte (Minhas | Do time | Ciclo atual, ou Minhas | Todas) e "Abertas ⌄".
    fn filtros(&self, m: &Motor, filtro: &FiltroIssues, window: &Window, cx: &Context<Self>) -> AnyElement {
        let eu = cx.entity().downgrade();
        let time = time_do_projeto(m, &self.projeto);
        let (opcoes, recortes): (Vec<Segmento>, Vec<Recorte>) = match self.fonte {
            FonteIssue::Linear => {
                let sem_time = |s: Segmento| if time.is_some() { s } else { s.desabilitado("Escolha o time do projeto") };
                let ciclos = time.as_ref().is_some_and(|(id, _)| self.time_tem_ciclos(m, id));
                let ciclo = Segmento::new("Ciclo atual");
                let ciclo = if time.is_some() && !ciclos { ciclo.desabilitado("O time não usa ciclos") } else { sem_time(ciclo) };
                (
                    vec![Segmento::new("Minhas"), sem_time(Segmento::new("Do time")), ciclo],
                    vec![Recorte::Minhas, Recorte::DoTime, Recorte::CicloAtual],
                )
            }
            FonteIssue::Github => (vec![Segmento::new("Minhas"), Segmento::new("Todas")], vec![Recorte::Minhas, Recorte::Todas]),
        };
        let atual = recortes.iter().position(|r| *r == filtro.recorte).unwrap_or(0);
        let segmentado = {
            let eu = eu.clone();
            ControleSegmentado::new("issues-recorte", opcoes, atual, move |i, _, cx| {
                let recorte = recortes[i];
                let _ = eu.update(cx, |p, cx| {
                    p.mudar_filtro(|f| f.recorte = recorte, cx);
                    p.atualizar(cx);
                });
            })
            .with_size(Size::Small)
            .no_flutuante(true)
        };
        let fechadas = filtro.fechadas;
        let rotulo: SharedString = if fechadas { "Fechadas".into() } else { "Abertas".into() };
        let estado = Self::chip_de_menu("issues-abertas", rotulo, None, fechadas, None, window, cx).dropdown_menu_with_anchor(
            Anchor::TopRight,
            move |menu, window, _| {
                let escolher = |fechar: bool| {
                    let eu = eu.clone();
                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        let _ = eu.update(cx, |p, cx| {
                            p.mudar_filtro(|f| f.fechadas = fechar, cx);
                            p.atualizar(cx);
                        });
                    }
                };
                menu.item(PopupMenuItem::new("Abertas").checked(!fechadas).on_click(escolher(false)))
                    .item(PopupMenuItem::new("Fechadas nos últimos 30 dias").checked(fechadas).on_click(escolher(true)))
                    .min_w(tam(window, 240.))
            },
        );
        h_flex().gap(esp::E8).child(segmentado).child(div().flex_1()).child(estado).into_any_element()
    }

    /// O campo do filtro local, com o "/" que põe o foco nele e a dica do Enter.
    fn campo(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let texto = self.busca.read(cx).value().trim().to_string();
        let focado = self.busca.read(cx).focus_handle(cx).is_focused(window);
        let onde = match self.fonte {
            FonteIssue::Linear => "no Linear",
            FonteIssue::Github => "no GitHub",
        };
        let dica = if self.termo.is_some() {
            Some(format!("Resultados da busca {onde}"))
        } else if texto.chars().count() >= 3 {
            Some(format!("Enter busca {onde}"))
        } else {
            None
        };
        let barra = (texto.is_empty() && !focado).then(|| desenho_teclas(vec!["/".into()]));
        v_flex()
            .gap(esp::E4)
            .child(campo_de_busca("issues-busca", &self.busca, None, window, cx).children(barra))
            .when_some(dica, |s, d| s.child(div().metadado().text_color(Cores::tenue()).child(d)))
            .into_any_element()
    }

    /// A faixa fina de "sem conexão" ou de limite, em cima da lista (o cache continua à vista).
    fn faixa_da_rede(&self, m: &Motor, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let (provedor, nome) = match self.fonte {
            FonteIssue::Linear => (Provedor::Linear, "Linear"),
            FonteIssue::Github => (Provedor::Github, "GitHub"),
        };
        let texto = match m.estado_rede(provedor) {
            EstadoRede::Ok => return None,
            EstadoRede::Fora { proxima_sonda, .. } => {
                let falta = proxima_sonda.saturating_duration_since(Instant::now()).as_secs();
                let quando = if falta < 60 { "em instantes".to_string() } else { format!("em {} min", falta.div_ceil(60)) };
                format!("Sem conexão com o {nome} · tentando de novo {quando}")
            }
            EstadoRede::Limite { ate } => {
                let falta = ate.saturating_duration_since(Instant::now());
                let hora = chrono::DateTime::<chrono::Local>::from(SystemTime::now() + falta).format("%H:%M");
                format!("Limite de pedidos do {nome} até {hora}; o Vitral espera sozinho")
            }
        };
        let fora = matches!(m.estado_rede(provedor), EstadoRede::Fora { .. });
        let eu = cx.entity().downgrade();
        Some(
            h_flex()
                .gap(esp::E8)
                .child(
                    Chip::new("issues-rede", texto)
                        .simbolo(Simbolo::icone(Lucide::WifiOff))
                        .tamanho(TamanhoChip::Pequeno)
                        .alerta(Cores::ambar())
                        .construir(window, cx)
                        .flex_1()
                        .min_w_0(),
                )
                .when(fora, |s| {
                    s.child(
                        botao_fantasma("issues-tentar-agora", None, "Tentar agora", TamanhoBotao::Linha, window, cx).on_click(move |_, _, cx| {
                            let _ = eu.update(cx, |p, cx| p.atualizar(cx));
                        }),
                    )
                })
                .into_any_element(),
        )
    }

    fn lista(&self, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let bloqueio = self.bloqueio(m, window, cx);
        let cabecalho = self.cabecalho(m, bloqueio.is_none(), window, cx);
        if let Some(vazio) = bloqueio {
            return v_flex()
                .flex_1()
                .min_h_0()
                .gap(esp::E12)
                .child(cabecalho)
                .child(div().flex_1().flex().flex_col().justify_center().child(vazio))
                .into_any_element();
        }
        let escopo = self.escopo(m);
        let filtro = self.filtro(m);
        let visao = m.issues(&escopo, &self.filtro_da_lista(m));
        let busca = self.busca.read(cx).value().trim().to_string();
        let indices = aplicar_filtro_local(&visao.itens, &busca);
        let eu = cx.entity().downgrade();

        let corpo: AnyElement = if visao.itens.is_empty() && visao.carregando {
            v_flex().gap(esp::E2).children((0..3).map(|i| esqueleto(i))).into_any_element()
        } else if let (true, Some(erro)) = (visao.itens.is_empty(), visao.erro.clone()) {
            self.erro_da_lista(&erro, window, cx)
        } else if indices.is_empty() {
            let time = time_do_projeto(m, &self.projeto).map(|(_, chave)| chave);
            let titulo = texto_sem_itens(self.fonte, &filtro, time.as_deref(), escopo.repositorio.as_deref(), &busca);
            let tem_filtro = !busca.is_empty() || filtro.fechadas;
            let mut vazio = EstadoVazio::new(Lucide::CircleDashed, titulo);
            if tem_filtro {
                let eu = eu.clone();
                vazio = vazio.acao(botao_fantasma("issues-limpar", None, "Limpar filtros", TamanhoBotao::Barra, window, cx).on_click(
                    move |_, window, cx| {
                        let _ = eu.update(cx, |p, cx| {
                            p.limpar_filtros(window, cx);
                            p.atualizar(cx);
                        });
                    },
                ));
            } else if filtro.recorte == Recorte::Minhas && (self.fonte == FonteIssue::Github || time.is_some()) {
                let eu = eu.clone();
                let (rotulo, recorte) = match self.fonte {
                    FonteIssue::Linear => ("Ver as do time", Recorte::DoTime),
                    FonteIssue::Github => ("Ver todas", Recorte::Todas),
                };
                vazio = vazio.acao(botao_fantasma("issues-ver-todas", None, rotulo, TamanhoBotao::Barra, window, cx).on_click(
                    move |_, _, cx| {
                        let _ = eu.update(cx, |p, cx| {
                            p.mudar_filtro(|f| f.recorte = recorte, cx);
                            p.atualizar(cx);
                        });
                    },
                ));
            }
            vazio.into_any_element()
        } else {
            v_flex()
                .gap(esp::E2)
                .children(indices.iter().map(|&i| self.linha(m, &visao.itens[i], i, window, cx)))
                .into_any_element()
        };

        let rodape = (!visao.itens.is_empty()).then(|| {
            let texto = texto_do_rodape(indices.len(), visao.itens.len(), visao.atualizada.map(|a| a.elapsed()), visao.tem_mais);
            let mais = visao.tem_mais.then(|| {
                let (eu, escopo, filtro) = (eu.clone(), escopo.clone(), self.filtro_da_lista(m));
                botao_fantasma("issues-mais", None, "Carregar mais", TamanhoBotao::Linha, window, cx).on_click(move |_, _, cx| {
                    let _ = eu.update(cx, |p, cx| p.motor.update(cx, |m, cx| m.mais_issues(&escopo, &filtro, cx)));
                })
            });
            h_flex()
                .gap(esp::E8)
                .min_h(r(24.))
                .child(div().flex_1().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(texto))
                .when(visao.carregando, |s| s.child(spinner(12., window)))
                .children(mais)
        });
        // Um erro com a lista já na tela (a atualização falhou): o cache fica, o erro vai embaixo.
        let erro_por_cima = visao.erro.clone().filter(|_| !visao.itens.is_empty());

        v_flex()
            .flex_1()
            .min_h_0()
            .gap(esp::E12)
            .child(cabecalho)
            .child(self.filtros(m, &filtro, window, cx))
            .child(self.campo(window, cx))
            .children(self.faixa_da_rede(m, window, cx))
            .child(area_rolavel("issues-lista", &self.rolagem, corpo))
            .children(erro_por_cima.map(|e| Alerta::new(Tom::Atencao, e).compacto()))
            .children(rodape)
            .children(self.barra_do_lote(m, window, cx))
            .into_any_element()
    }

    /// O erro da lista vazia: com a ação que resolve, quando há uma.
    fn erro_da_lista(&self, erro: &str, window: &Window, cx: &Context<Self>) -> AnyElement {
        let eu = cx.entity().downgrade();
        let motor = self.motor.clone();
        let mut vazio = EstadoVazio::new(Lucide::TriangleAlert, "Não consegui ler as issues").texto(erro.to_string());
        if erro.starts_with("O Linear recusou a chave") {
            let eu = eu.clone();
            vazio = vazio.acao(botao_primario("issues-conectar-de-novo", None, "Conectar de novo…", TamanhoBotao::Barra, window, cx).on_click(
                move |_, window, cx| {
                    let _ = eu.update(cx, |p, cx| p.fechar(cx));
                    abrir_conectar_linear(motor.clone(), None, window, cx);
                },
            ));
        } else if erro.starts_with("Entre no GitHub") {
            vazio = vazio.acao(
                botao_primario("issues-entrar", None, "Entrar no GitHub", TamanhoBotao::Barra, window, cx)
                    .on_click(move |_, _, cx| motor.update(cx, |m, _| m.entrar_no_github())),
            );
        }
        vazio
            .acao(botao_fantasma("issues-tentar", Some(Lucide::RefreshCw.into()), "Tentar de novo", TamanhoBotao::Barra, window, cx).on_click(
                move |_, _, cx| {
                    let _ = eu.update(cx, |p, cx| p.atualizar(cx));
                },
            ))
            .into_any_element()
    }

    /// Uma issue da lista: a caixa de seleção (no hover; fixa com seleção), o ponto ou o ícone do
    /// estado, o título, o subtítulo e os selos do chat e do PR. Clique abre o detalhe; Ctrl+clique
    /// seleciona.
    fn linha(&self, m: &Motor, issue: &IssueResumo, i: usize, window: &Window, cx: &Context<Self>) -> AnyElement {
        let eu = cx.entity().downgrade();
        let estado = self.estado_de(issue);
        let marcada = self.selecionadas.iter().any(|s| mesma(s, issue));
        let cheio = !marcada && self.selecionadas.len() >= MAXIMO_DO_LOTE;
        let com_selecao = !self.selecionadas.is_empty();
        let caixa = {
            let (eu, issue) = (eu.clone(), issue.clone());
            Checkbox::new(("issue-marcar", i))
                .checked(marcada)
                .disabled(cheio)
                .when(cheio, |c| c.tooltip(NO_MAXIMO))
                .when(!cheio && !marcada, |c| c.tooltip("Selecionar (Ctrl+clique na linha)"))
                .on_click(move |_, _, cx| {
                    let _ = eu.update(cx, |p, cx| p.alternar(&issue, cx));
                })
        };
        let caixa = div()
            .id(("issue-marcar-area", i))
            .flex_none()
            .size(r(16.))
            .flex()
            .items_center()
            .justify_center()
            .when(!com_selecao, |d| d.opacity(0.).group_hover(GRUPO_DO_CARTAO, |s| s.opacity(1.)))
            // O clique na caixa não abre o detalhe.
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(caixa);
        let simbolo = h_flex().gap(esp::E8).child(caixa).child(avatar_da_issue(issue.fonte, &estado, window));
        let subtitulo = subtitulo_da_issue(issue, &estado, idade(&issue.atualizada));
        let mut cartao = CartaoDeLista::new(("issue", i), issue.titulo.clone())
            .simbolo(Simbolo::Elemento(simbolo.into_any_element()))
            .subtitulo(subtitulo)
            .selecionado(marcada);
        if let Some((chat, tarefa, pr)) = chat_da_issue(m, issue) {
            let eu = eu.clone();
            let selos = h_flex()
                .id(("issue-chat", i))
                .gap(esp::E4)
                .cursor_pointer()
                .child(Selo::neutro(format!("chat #{tarefa}")).icone(Lucide::MessageSquare, Cores::tenue()))
                .children(pr.map(|n| Selo::neutro(format!("PR #{n}")).icone(Lucide::GitPullRequest, Cores::tenue())))
                .tooltip(|window, cx| Tooltip::new("Abrir o chat desta issue\nCtrl+clique abre ao lado").build(window, cx))
                .on_click(move |ev, window, cx| {
                    cx.stop_propagation();
                    let ao_lado = ev.modifiers().control;
                    let _ = eu.update(cx, |p, cx| p.abrir_chat(chat.clone(), ao_lado, window, cx));
                });
            cartao = cartao.meta(selos);
        }
        let issue = issue.clone();
        cartao
            .on_click(move |ev, _, cx| {
                let _ = eu.update(cx, |p, cx| {
                    if ev.modifiers().control {
                        p.alternar(&issue, cx);
                    } else {
                        p.ver_detalhe(issue.clone(), cx);
                    }
                });
            })
            .into_any_element()
    }

    /// "2 selecionadas [Iniciar agentes (2)] [Só as cópias] [×]", só com seleção.
    fn barra_do_lote(&self, m: &Motor, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let n = self.selecionadas.len();
        if n == 0 {
            return None;
        }
        let eu = cx.entity().downgrade();
        let limpar = {
            let eu = eu.clone();
            botao_icone("issues-lote-limpar", Lucide::X, "Tirar a seleção", TamanhoBotao::Barra, window, cx).on_click(move |_, _, cx| {
                let _ = eu.update(cx, |p, cx| {
                    p.selecionadas.clear();
                    cx.notify();
                });
            })
        };
        let vagas = m.config.max_trabalhadores.max(1);
        let lote = |modo: ModoIssue| {
            let (id, rotulo, botao): (&'static str, String, Button) = match modo {
                ModoIssue::Agente => (
                    "issues-lote-agentes",
                    format!("Iniciar agentes ({n})"),
                    // Sem o ícone: a barra cabe numa linha nos 440.
                    botao_primario("issues-lote-agentes-botao", None, format!("Iniciar agentes ({n})"), TamanhoBotao::Barra, window, cx),
                ),
                ModoIssue::SoCopia => (
                    "issues-lote-copias",
                    format!("Só as cópias ({n})"),
                    botao_fantasma("issues-lote-copias-botao", None, "Só as cópias", TamanhoBotao::Barra, window, cx),
                ),
            };
            let chaves: Vec<(String, String)> = self.selecionadas.iter().map(|s| (s.chave.clone(), s.titulo.clone())).collect();
            let aviso_do_lote = match modo {
                ModoIssue::Agente => format!(
                    "Cada agente ocupa uma vaga; no máximo {} rodam ao mesmo tempo, os outros esperam. O modelo é o do projeto.",
                    contagem(vagas, "agente", "agentes")
                ),
                ModoIssue::SoCopia => "As cópias são preparadas uma de cada vez; nada muda nas issues até você mandar a primeira mensagem.".into(),
            };
            let eu = eu.clone();
            popover(id)
                .anchor(Anchor::BottomRight)
                .offset(px(8.))
                .trigger(botao)
                .content(move |_, window, cx| {
                    let eu = eu.clone();
                    let confirmar_lote =
                        botao_primario("issues-lote-confirmar", None, rotulo.clone(), TamanhoBotao::Barra, window, cx).on_click(move |_, window, cx| {
                            let _ = eu.update(cx, |p, cx| p.iniciar_lote(modo, window, cx));
                        });
                    v_flex()
                        .w(tam(window, 300.))
                        .gap(esp::E8)
                        .children(chaves.iter().map(|(chave, titulo)| {
                            h_flex()
                                .gap(esp::E8)
                                .metadado()
                                .child(div().flex_none().font_semibold().text_color(Cores::texto()).child(chave.clone()))
                                .child(div().min_w_0().truncate().text_color(Cores::tenue()).child(titulo.clone()))
                        }))
                        .child(div().metadado().text_color(Cores::tenue()).child(aviso_do_lote.clone()))
                        .child(h_flex().justify_end().child(confirmar_lote))
                })
                .into_any_element()
        };
        Some(
            h_flex()
                .flex_wrap()
                .gap(esp::E8)
                .pt(esp::E8)
                .border_t_1()
                .border_color(Cores::borda())
                .child(limpar)
                .child(div().flex_none().whitespace_nowrap().rotulo().text_color(Cores::texto()).child(contagem(n, "selecionada", "selecionadas")))
                .child(div().flex_1())
                .child(lote(ModoIssue::SoCopia))
                .child(lote(ModoIssue::Agente))
                .into_any_element(),
        )
    }

    /// Os estados possíveis da issue: os do time no Linear, ou os três do GitHub.
    fn estados_possiveis(&self, m: &Motor, issue: &IssueResumo, detalhe: Option<&IssueDetalhe>) -> Result<Vec<EstadoIssue>, String> {
        if issue.fonte == FonteIssue::Github {
            return Ok(estados_github());
        }
        let do_projeto = time_do_projeto(m, &self.projeto);
        let chave = detalhe.and_then(|d| d.time.clone());
        let id = match (&chave, &do_projeto) {
            (Some(chave), Some((id, ch))) if chave == ch => Some(id.clone()),
            (None, Some((id, _))) => Some(id.clone()),
            (Some(chave), _) => m.times_linear().valor().and_then(|times| times.iter().find(|t| &t.chave == chave).map(|t| t.id.clone())),
            (None, None) => None,
        };
        let Some(id) = id else {
            return Err("Lendo os estados do time…".into());
        };
        match m.dados_do_time(&id) {
            Carregando::Erro(erro, None) => Err(erro),
            dados => dados.valor().map(estados_do_time).filter(|e| !e.is_empty()).ok_or_else(|| "Lendo os estados do time…".into()),
        }
    }

    /// "● Em andamento ⌄": a única edição do v1, aplicada na hora.
    fn chip_do_estado(&self, m: &Motor, issue: &IssueResumo, detalhe: Option<&IssueDetalhe>, window: &Window, cx: &Context<Self>) -> AnyElement {
        let estado = self.estado_de(issue);
        let opcoes = self.estados_possiveis(m, issue, detalhe);
        let chip = |aberto: bool| {
            let simbolo = match issue.fonte {
                FonteIssue::Linear => Simbolo::Elemento(ponto_do_estado(&estado).into_any_element()),
                FonteIssue::Github => Simbolo::Elemento(avatar_da_issue(issue.fonte, &estado, window)),
            };
            let nome = if estado.nome.is_empty() { "Estado".to_string() } else { estado.nome.clone() };
            let mut chip = Chip::new("issue-estado", nome).simbolo(simbolo).tamanho(TamanhoChip::Medio).seta(true).aberto(aberto);
            if !aberto {
                chip = chip.dica("Mudar o estado\nVale na hora, no Linear ou no GitHub");
            }
            chip.construir(window, cx)
        };
        let atual = match issue.fonte {
            FonteIssue::Linear => estado.id.clone(),
            FonteIssue::Github => id_github(&estado).to_string(),
        };
        let (eu, issue) = (cx.entity().downgrade(), issue.clone());
        GatilhoChip::new(chip(false), chip(true))
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |mut menu, window, _| {
                match &opcoes {
                    Ok(estados) => {
                        for estado in estados {
                            let (eu, issue, novo) = (eu.clone(), issue.clone(), estado.clone());
                            let nome = estado.nome.clone();
                            let fonte = issue.fonte;
                            let desenho = estado.clone();
                            menu = menu.item(
                                PopupMenuItem::element(move |window, _| {
                                    h_flex()
                                        .gap(esp::E8)
                                        .child(avatar_da_issue(fonte, &desenho, window))
                                        .child(nome.clone())
                                })
                                .checked(estado.id == atual)
                                .on_click(move |_, window, cx| {
                                    let _ = eu.update(cx, |p, cx| p.mudar_estado(issue.clone(), novo.clone(), window, cx));
                                }),
                            );
                        }
                    }
                    Err(texto) => menu = menu.item(PopupMenuItem::new(texto.clone()).disabled(true)),
                }
                menu.min_w(tam(window, 220.)).max_h(tam(window, 300.)).scrollable(true)
            })
            .into_any_element()
    }

    fn detalhe(&self, m: &Motor, issue: &IssueResumo, window: &Window, cx: &Context<Self>) -> AnyElement {
        let eu = cx.entity().downgrade();
        let lido = m.detalhe_issue(issue.fonte, &issue.id);
        let detalhe = lido.valor().cloned();
        // O resumo que veio com o detalhe é o mais novo; sem ele, o da lista.
        let resumo = detalhe.as_ref().map(|d| d.resumo.clone()).filter(|r| !r.chave.is_empty()).unwrap_or_else(|| issue.clone());
        let onde = match issue.fonte {
            FonteIssue::Linear => "no Linear",
            FonteIssue::Github => "no GitHub",
        };
        let voltar = {
            let eu = eu.clone();
            botao_fantasma("issues-voltar", Some(Lucide::ArrowLeft.into()), "Issues", TamanhoBotao::Linha, window, cx)
                .ml(r(-8.))
                .tooltip("Voltar à lista (Esc)")
                .on_click(move |_, _, cx| {
                    let _ = eu.update(cx, |p, cx| p.voltar_ou_fechar(cx));
                })
        };
        let atualizar = {
            let (eu, fonte, id) = (eu.clone(), issue.fonte, issue.id.clone());
            botao_icone("issue-atualizar", Lucide::RefreshCw, "Atualizar", TamanhoBotao::Linha, window, cx).on_click(move |_, _, cx| {
                let _ = eu.update(cx, |p, cx| {
                    p.otimistas.clear();
                    p.motor.update(cx, |m, cx| m.atualizar_detalhe_issue(fonte, &id, cx));
                });
            })
        };
        let abrir_fora = link_seguro(&resumo.url).then(|| {
            let url = resumo.url.clone();
            botao_icone("issue-abrir", Lucide::ExternalLink, format!("Abrir {onde}"), TamanhoBotao::Linha, window, cx)
                .on_click(move |_, _, cx| cx.open_url(&url))
        });
        let topo = h_flex()
            .gap(esp::E4)
            .child(voltar)
            .child(div().flex_1())
            .when(lido.carregando(), |s| s.child(spinner(12., window)))
            .child(atualizar)
            .children(abrir_fora);

        let ligada = chat_da_issue(m, &resumo);
        let no_vitral: AnyElement = match &ligada {
            Some((chat, tarefa, pr)) => {
                let (eu, chat) = (eu.clone(), chat.clone());
                let rotulo = match pr {
                    Some(n) => format!("chat #{tarefa} · PR #{n}"),
                    None => format!("chat #{tarefa}"),
                };
                Chip::new("issue-chat", rotulo)
                    .simbolo(Simbolo::icone(Lucide::MessageSquare))
                    .tamanho(TamanhoChip::Medio)
                    .dica("Abrir o chat desta issue\nCtrl+clique abre ao lado")
                    .on_click(move |ev, window, cx| {
                        let ao_lado = ev.modifiers().control;
                        let _ = eu.update(cx, |p, cx| p.abrir_chat(chat.clone(), ao_lado, window, cx));
                    })
                    .construir(window, cx)
                    .ml(r(-8.))
                    .into_any_element()
            }
            None => div().metadado().text_color(Cores::tenue()).child("Ainda não está em nenhum chat.").into_any_element(),
        };

        let descricao: AnyElement = match &detalhe {
            Some(d) if !d.descricao.trim().is_empty() => texto_de_fora(
                SharedString::from(format!("issue-descricao-{}", resumo.id)),
                &d.descricao,
                None,
                cx,
            ),
            Some(_) => div().metadado().text_color(Cores::tenue()).child("Sem descrição.").into_any_element(),
            None => match lido.erro() {
                Some(erro) => Alerta::new(Tom::Atencao, format!("Não consegui ler a issue: {erro}")).compacto().into_any_element(),
                None => v_flex().gap(esp::E6).child(barra_esqueleto(0.9)).child(barra_esqueleto(0.7)).child(barra_esqueleto(0.8)).into_any_element(),
            },
        };
        let comentarios = detalhe.as_ref().map(|d| {
            let total = d.comentarios.len();
            let ultimos = &d.comentarios[total.saturating_sub(COMENTARIOS)..];
            let ver_todos = (total > COMENTARIOS && link_seguro(&resumo.url)).then(|| {
                let url = resumo.url.clone();
                botao_fantasma("issue-ver-comentarios", None, format!("Ver todos {onde}"), TamanhoBotao::Linha, window, cx)
                    .on_click(move |_, _, cx| cx.open_url(&url))
            });
            v_flex()
                .child({
                    let cabecalho = CabecalhoDeSecao::new("Comentários").recuo(0.).respiro(16.).acao(div().children(ver_todos));
                    if total > 0 { cabecalho.contagem(total) } else { cabecalho }
                })
                .when(total == 0, |s| s.child(div().metadado().text_color(Cores::tenue()).child("Nenhum comentário.")))
                .children(ultimos.iter().enumerate().map(|(i, c)| {
                    let autor = if c.autor.is_empty() { "alguém".to_string() } else { c.autor.clone() };
                    v_flex()
                        .gap(esp::E2)
                        .pt(if i == 0 { esp::E4 } else { esp::E12 })
                        .child(
                            h_flex()
                                .gap(esp::E6)
                                .child(div().rotulo().text_color(Cores::texto()).child(autor))
                                .children(idade(&c.criado).map(|i| div().metadado().text_color(Cores::tenue()).child(i))),
                        )
                        .child(texto_de_fora(SharedString::from(format!("issue-comentario-{}-{i}", resumo.id)), &c.corpo, Some(8), cx))
                }))
        });

        let corpo = v_flex()
            .child(
                h_flex()
                    .gap(esp::E8)
                    .child(div().rotulo().text_color(Cores::tenue()).child(resumo.chave.clone()))
                    .child(self.chip_do_estado(m, &resumo, detalhe.as_ref(), window, cx)),
            )
            .child(div().pt(esp::E6).titulo_cartao().text_color(Cores::forte()).child(resumo.titulo.clone()))
            .child(div().pt(esp::E4).metadado().text_color(Cores::tenue()).child(meta_do_detalhe(&resumo, detalhe.as_ref())))
            .child(CabecalhoDeSecao::new("No Vitral").recuo(0.).respiro(16.))
            .child(h_flex().child(no_vitral))
            .child(CabecalhoDeSecao::new("Descrição").recuo(0.).respiro(16.))
            .child(descricao)
            .children(comentarios);

        v_flex()
            .flex_1()
            .min_h_0()
            .gap(esp::E12)
            .child(topo)
            .child(area_rolavel("issue-detalhe", &self.rolagem, corpo.pb(esp::E8).into_any_element()))
            .children(self.erro.clone().map(|e| Alerta::new(Tom::Erro, e).compacto()))
            .child(self.acoes_do_detalhe(m, &resumo, ligada, window, cx))
            .into_any_element()
    }

    /// "Iniciar agente", "Só a cópia", "Anexar" e o ⋯; com a issue já num chat, "Abrir chat #12".
    fn acoes_do_detalhe(
        &self,
        m: &Motor,
        issue: &IssueResumo,
        ligada: Option<(String, u64, Option<u64>)>,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let eu = cx.entity().downgrade();
        let pastas = self.pastas_a_perguntar(m, issue);
        let origem = self.origem.clone();
        let pode_anexar = origem.as_ref().is_some_and(|o| o.pode_anexar);
        let mestre = origem.as_ref().is_some_and(|o| o.mestre);
        // Com mais de uma pasta e nenhuma lembrada, o botão pergunta "Em qual pasta?" antes.
        let com_pasta = |botao: Button, modo: ModoIssue, issue: IssueResumo| -> AnyElement {
            let eu = eu.clone();
            if pastas.is_empty() {
                return botao
                    .on_click(move |ev, window, cx| {
                        let ao_lado = ev.modifiers().control;
                        let _ = eu.update(cx, |p, cx| p.iniciar(issue.clone(), modo, None, ao_lado, window, cx));
                    })
                    .into_any_element();
            }
            let pastas = pastas.clone();
            botao
                .dropdown_menu_with_anchor(Anchor::BottomLeft, move |mut menu, window, _| {
                    menu = menu.item(PopupMenuItem::label("Em qual pasta?"));
                    for pasta in &pastas {
                        let (eu, issue, escolhida) = (eu.clone(), issue.clone(), pasta.clone());
                        let nome = pasta.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| pasta.display().to_string());
                        menu = menu.item(PopupMenuItem::new(nome).icon(Lucide::Folder).on_click(move |_, window, cx| {
                            let _ = eu.update(cx, |p, cx| p.iniciar(issue.clone(), modo, Some(escolhida.clone()), false, window, cx));
                        }));
                    }
                    menu.min_w(tam(window, 220.))
                })
                .into_any_element()
        };
        let principal: AnyElement = match &ligada {
            Some((chat, tarefa, _)) => {
                let (eu, chat) = (eu.clone(), chat.clone());
                botao_primario("issue-abrir-chat", Some(Lucide::MessageSquare.into()), format!("Abrir chat #{tarefa}"), TamanhoBotao::Barra, window, cx)
                    .tooltip("Ctrl+clique abre ao lado")
                    .on_click(move |ev, window, cx| {
                        let ao_lado = ev.modifiers().control;
                        let _ = eu.update(cx, |p, cx| p.abrir_chat(chat.clone(), ao_lado, window, cx));
                    })
                    .into_any_element()
            }
            None => com_pasta(
                botao_primario("issue-iniciar", Some(Lucide::Play.into()), "Iniciar agente", TamanhoBotao::Barra, window, cx)
                    .tooltip("Novo chat no projeto com esta issue e o agente já trabalhando\nCtrl+clique abre ao lado"),
                ModoIssue::Agente,
                issue.clone(),
            ),
        };
        let so_copia = ligada.is_none().then(|| {
            com_pasta(
                botao_fantasma("issue-so-copia", Some(Lucide::GitBranch.into()), "Só a cópia", TamanhoBotao::Barra, window, cx)
                    .tooltip("Novo chat com a cópia pronta e a issue anexada na caixa, sem mexer na issue"),
                ModoIssue::SoCopia,
                issue.clone(),
            )
        });
        let anexar = pode_anexar.then(|| {
            let (eu, issue) = (eu.clone(), issue.clone());
            botao_fantasma("issue-anexar", Some(Lucide::Paperclip.into()), "Anexar", TamanhoBotao::Barra, window, cx)
                .tooltip("Anexar a esta conversa\nO texto da issue vai junto na próxima mensagem")
                .on_click(move |_, window, cx| {
                    let _ = eu.update(cx, |p, cx| p.anexar(issue.clone(), window, cx));
                })
        });
        let mais = {
            let (eu, issue) = (eu.clone(), issue.clone());
            let outro = ligada.is_some();
            let onde = match issue.fonte {
                FonteIssue::Linear => "Abrir no Linear",
                FonteIssue::Github => "Abrir no GitHub",
            };
            botao_icone("issue-mais", Lucide::Ellipsis, "Mais opções da issue", TamanhoBotao::Barra, window, cx)
                .dropdown_menu_with_anchor(Anchor::BottomRight, move |mut menu, window, _| {
                    if outro {
                        let (eu, issue) = (eu.clone(), issue.clone());
                        menu = menu.item(PopupMenuItem::new("Iniciar outro agente").icon(Lucide::Play).on_click(move |_, window, cx| {
                            let _ = eu.update(cx, |p, cx| p.iniciar(issue.clone(), ModoIssue::Agente, None, false, window, cx));
                        }));
                    }
                    if !mestre {
                        let (eu, issue) = (eu.clone(), issue.clone());
                        menu = menu.item(PopupMenuItem::new("Mandar ao Mestre").icon(Lucide::Sparkles).on_click(move |_, window, cx| {
                            let _ = eu.update(cx, |p, cx| p.mandar_ao_mestre(&issue, window, cx));
                        }));
                    }
                    let (url, chave) = (issue.url.clone(), issue.chave.clone());
                    menu = menu
                        .item(PopupMenuItem::new("Copiar link").icon(Lucide::Link).disabled(url.is_empty()).on_click({
                            let url = url.clone();
                            move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(url.clone()))
                        }))
                        .item(PopupMenuItem::new(format!("Copiar “{chave}”")).icon(Lucide::Copy).on_click({
                            let chave = chave.clone();
                            move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(chave.clone()))
                        }));
                    if link_seguro(&url) {
                        menu = menu.item(PopupMenuItem::new(onde).external().on_click(move |_, _, cx| cx.open_url(&url)));
                    }
                    menu.min_w(tam(window, 220.))
                })
        };
        h_flex()
            .flex_wrap()
            .gap(esp::E8)
            .pt(esp::E12)
            .border_t_1()
            .border_color(Cores::borda())
            .child(principal)
            .children(so_copia)
            .children(anexar)
            .child(div().flex_1())
            .child(mais)
            .into_any_element()
    }
}

impl Render for PopoverIssues {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A margem de 16 de cada lado é do popover; aqui fica o conteúdo.
        let largura = (tam(window, LarguraPopover::P440.px()) - tam(window, 32.)).min(window.viewport_size().width - px(64.));
        let altura_max = tam(window, ALTURA_MAXIMA).min(window.viewport_size().height * 0.8) - tam(window, 32.);
        let altura_min = (tam(window, ALTURA_MINIMA) - tam(window, 32.)).min(altura_max);
        let motor = self.motor.clone();
        let m = motor.read(cx);
        let corpo = match &self.tela {
            Tela::Lista => self.lista(m, window, cx),
            Tela::Detalhe(issue) => self.detalhe(m, issue, window, cx),
        };
        let lista = matches!(self.tela, Tela::Lista);
        v_flex()
            .id("popover-issues")
            .key_context("PopoverIssues")
            .track_focus(&self.foco)
            .on_action(cx.listener(|este, _: &VoltarIssues, _, cx| este.voltar_ou_fechar(cx)))
            .on_key_down(cx.listener(move |este, evento: &KeyDownEvent, window, cx| {
                // "/" leva ao filtro, como no Linear.
                let busca = este.busca.read(cx).focus_handle(cx);
                if lista && evento.keystroke.key == "/" && !evento.keystroke.modifiers.modified() && !busca.is_focused(window) {
                    window.focus(&busca, cx);
                    cx.stop_propagation();
                }
            }))
            .w(largura)
            .min_h(altura_min)
            .max_h(altura_max)
            .child(corpo)
    }
}

/// Texto de terceiros (a descrição, um comentário) em markdown: as imagens viram link e só os
/// links `http(s)` abrem.
fn texto_de_fora(id: SharedString, texto: &str, linhas: Option<usize>, cx: &App) -> AnyElement {
    div()
        .corpo_ui()
        .text_color(Cores::texto())
        .child(
            super::mensagens::markdown(id, imagens_viram_link(texto), cx)
                .on_link_click(|url, _, _, cx| {
                    if link_seguro(url) {
                        cx.open_url(url);
                    }
                })
                .when_some(linhas, |t, n| t.max_lines(n)),
        )
        .into_any_element()
}

/// A parte que rola (a lista, o detalhe), com a barra fina: ocupa o que sobra até a altura máxima
/// do popover. É um filho flexível de coluna, e não `size_full`: o popover não tem altura
/// definida (só a máxima), e a porcentagem não resolveria. Os 8 de fora deixam o realce dos
/// cartões na guia dos menus.
fn area_rolavel(id: &'static str, rolagem: &ScrollHandle, corpo: AnyElement) -> AnyElement {
    v_flex()
        .relative()
        .flex_1()
        .min_h_0()
        .mx(r(-8.))
        .child(div().id(id).flex_1().min_h_0().w_full().overflow_y_scroll().track_scroll(rolagem).px(esp::E8).child(corpo))
        .child(barra_de_rolagem(SharedString::from(format!("{id}-barra")), rolagem))
        .into_any_element()
}

/// Spinner tênue.
fn spinner(lado: f32, window: &Window) -> AnyElement {
    Spinner::new().with_size(Size::Size(tam(window, lado))).color(Cores::tenue()).into_any_element()
}

/// Spinner de 12 e o texto, no lugar da lista.
fn carregando(texto: &'static str, window: &Window) -> AnyElement {
    h_flex()
        .justify_center()
        .gap(esp::E8)
        .py(esp::E24)
        .metadado()
        .text_color(Cores::tenue())
        .child(spinner(12., window))
        .child(texto)
        .into_any_element()
}

/// Uma barra do esqueleto (a `borda()` a 40%), na fração da largura.
fn barra_esqueleto(fracao: f32) -> Div {
    div().h(r(10.)).w(relative(fracao)).rounded_full().bg(Cores::borda().opacity(0.4))
}

/// Uma linha de 52 do esqueleto da primeira leitura.
fn esqueleto(i: usize) -> AnyElement {
    h_flex()
        .h(r(52.))
        .px(esp::E8)
        .gap(esp::E12)
        .child(div().flex_none().size(r(16.)))
        .child(div().flex_none().size(r(10.)).rounded_full().bg(Cores::borda().opacity(0.4)))
        .child(
            v_flex()
                .flex_1()
                .gap(esp::E6)
                .child(barra_esqueleto([0.8, 0.6, 0.7][i % 3]))
                .child(barra_esqueleto([0.45, 0.35, 0.5][i % 3]).h(r(8.))),
        )
        .into_any_element()
}

// ---------- na janela: os botões do cabeçalho, o anexo na caixa ----------

impl Janela {
    /// Os botões das issues no cabeçalho dos chats de projeto e do Mestre: o chip da issue ligada,
    /// o do Linear (menos se escondido em Conexões) e o do GitHub (com uma pasta com issues).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn botoes_de_issues(
        &self,
        motor: &Motor,
        chat: &Chat,
        painel_id: u64,
        nivel: LarguraPainel,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let entidade = self.issues.clone()?;
        let mestre = chat.papel == PapelDoChat::Mestre;
        if !(mestre || chat.papel == PapelDoChat::Projeto) || nivel == LarguraPainel::Minimo {
            return None;
        }
        let linear = !motor.config.esconder_botao_linear;
        let github = if mestre {
            motor.projetos_visiveis().iter().any(|p| tem_github(motor, &p.id))
        } else {
            !repos_com_issues(motor, &chat.projeto).is_empty()
        };
        let ligada = chat.tarefa.and_then(|t| motor.tarefa(t)).and_then(|t| t.issue.clone());
        let aberto = |g: Gatilho| self.issues_aberto == Some((painel_id, g));
        let mut botoes: Vec<AnyElement> = Vec::new();
        if let Some(ligada) = ligada.filter(|_| matches!(nivel, LarguraPainel::Medio | LarguraPainel::Largo)) {
            let mut chip = Chip::new(("issue-ligada", painel_id as usize), ligada.chave.clone())
                .simbolo(simbolo_da_fonte(ligada.fonte))
                .tamanho(TamanhoChip::Medio)
                .aberto(aberto(Gatilho::Chip));
            if !aberto(Gatilho::Chip) {
                chip = chip.dica(format!("{} · {}\nO detalhe da issue deste chat", ligada.chave, ligada.titulo));
            }
            botoes.push(self.popover_de_issues(&entidade, painel_id, Gatilho::Chip, chip.construir(window, cx), cx));
        }
        if linear {
            let time = time_do_projeto(motor, &chat.projeto).map(|(_, chave)| chave).filter(|_| !mestre);
            let dica = match time {
                Some(chave) => format!("Issues do Linear ({chave})"),
                None => "Issues do Linear".to_string(),
            };
            let botao = BotaoRedondo::new(("issues-linear", painel_id as usize), None, dica)
                .marca(Marca::Linear)
                .variante(VarianteRedonda::Plano)
                .diametro(28.)
                .selected(aberto(Gatilho::Linear));
            botoes.push(self.popover_de_issues(&entidade, painel_id, Gatilho::Linear, botao, cx));
        }
        if github {
            let botao = BotaoRedondo::new(("issues-github", painel_id as usize), Some(Lucide::Github.into()), "Issues do GitHub")
                .variante(VarianteRedonda::Plano)
                .diametro(28.)
                .selected(aberto(Gatilho::Github));
            botoes.push(self.popover_de_issues(&entidade, painel_id, Gatilho::Github, botao, cx));
        }
        (!botoes.is_empty()).then(|| sem_arrastar().flex().flex_none().items_center().gap(r(4.)).children(botoes).into_any_element())
    }

    /// Um popover das issues preso a um gatilho do cabeçalho (todos mostram o mesmo conteúdo).
    fn popover_de_issues(
        &self,
        entidade: &Entity<PopoverIssues>,
        painel_id: u64,
        gatilho: Gatilho,
        botao: impl Selectable + IntoElement + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        let aberto = self.issues_aberto == Some((painel_id, gatilho));
        let foco = entidade.read(cx).foco().clone();
        let (janela, conteudo) = (cx.entity().downgrade(), entidade.clone());
        popover(SharedString::from(format!("pop-issues-{painel_id}-{gatilho:?}")))
            .anchor(Anchor::TopRight)
            .offset(px(8.))
            .open(aberto)
            .on_open_change(move |abriu, window, cx| {
                let _ = janela.update(cx, |j, cx| {
                    if *abriu {
                        j.abrir_issues(painel_id, gatilho, window, cx);
                    } else if j.issues_aberto == Some((painel_id, gatilho)) {
                        j.fechar_popover_issues(cx);
                    }
                });
            })
            .track_focus(&foco)
            .trigger(botao)
            .content(move |_, _, _| conteudo.clone())
            .into_any_element()
    }

    /// Abre o popover do gatilho, no projeto do chat do painel.
    pub(super) fn abrir_issues(&mut self, painel_id: u64, gatilho: Gatilho, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(motor), Some(entidade)) = (self.motor.clone(), self.issues.clone()) else { return };
        let Some(chat) = self.paineis.iter().find(|p| p.id == painel_id).and_then(|p| p.chat.clone()) else { return };
        let (origem, projeto, fonte, detalhe) = {
            let m = motor.read(cx);
            let Some(c) = m.chat(&chat) else { return };
            let origem = Origem {
                painel: painel_id,
                chat: chat.clone(),
                mestre: c.papel == PapelDoChat::Mestre,
                pode_anexar: c.papel == PapelDoChat::Projeto,
            };
            let ligada = c.tarefa.and_then(|t| m.tarefa(t)).and_then(|t| t.issue.clone());
            let (fonte, detalhe) = match (gatilho, ligada) {
                (Gatilho::Chip, Some(l)) => (l.fonte, Some(resumo_da_ligada(&l))),
                (Gatilho::Github, _) => (FonteIssue::Github, None),
                _ => (FonteIssue::Linear, None),
            };
            (origem, c.projeto.clone(), fonte, detalhe)
        };
        self.issues_aberto = Some((painel_id, gatilho));
        entidade.update(cx, |p, cx| p.abrir(origem, projeto, fonte, detalhe, window, cx));
        cx.notify();
    }

    pub(super) fn fechar_popover_issues(&mut self, cx: &mut Context<Self>) {
        if self.issues_aberto.take().is_some() {
            cx.notify();
        }
    }

    /// Abre o chat que o popover criou ou apontou (Ctrl+clique: ao lado).
    fn abrir_chat_da_issue(&mut self, chat: &str, ao_lado: bool, window: &mut Window, cx: &mut Context<Self>) {
        if ao_lado {
            self.abrir_ao_lado(chat.to_string(), None, window, cx);
        } else {
            self.selecionar(Some(chat.to_string()), window, cx);
        }
    }

    /// Escreve na caixa do painel ativo, com o foco nela; `selecionado` deixa o texto todo
    /// selecionado ("Resolva a issue ENG-123." do "Só a cópia").
    fn por_pedido_na_caixa(&mut self, texto: &str, selecionado: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.composer_ativo() else { return };
        let texto = texto.to_string();
        composer.update(cx, |c, cx| {
            let fim = texto.len();
            c.set_value(texto, window, cx);
            c.set_selected_range(if selecionado { 0..fim } else { fim..fim }, cx);
        });
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    fn focar_caixa_do_painel(&mut self, painel_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.paineis.iter().position(|p| p.id == painel_id) {
            self.focar_painel(ix, window, cx);
        }
    }

    /// A issue entra na caixa do chat como um chip; o bloco dela é lido pelo motor em seguida.
    pub(super) fn anexar_issue(&mut self, chat: &str, issue: IssueResumo, repositorio: Option<String>, cx: &mut Context<Self>) {
        let lista = self.anexos_issue.entry(chat.to_string()).or_default();
        if lista.iter().any(|a| mesma(&a.issue, &issue)) {
            return;
        }
        lista.push(AnexoDeIssue { issue: issue.clone(), repositorio, bloco: None });
        self.ler_bloco_da_issue(chat, issue, cx);
        cx.notify();
    }

    /// Pede o bloco ao motor (de novo, no clique no chip que falhou).
    fn ler_bloco_da_issue(&mut self, chat: &str, issue: IssueResumo, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else { return };
        let recebe = motor.update(cx, |m, cx| m.bloco_da_issue(issue.fonte, &issue.id, cx));
        let chat = chat.to_string();
        cx.spawn(async move |este, cx| {
            let bloco = recebe.recv().await.unwrap_or_else(|_| Err("o Vitral fechou no meio".into()));
            let _ = este.update(cx, |este, cx| {
                if let Some(anexo) = este.anexos_issue.get_mut(&chat).and_then(|l| l.iter_mut().find(|a| mesma(&a.issue, &issue))) {
                    anexo.bloco = Some(bloco);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Os chips das issues anexadas na caixa: a marca e a chave, o spinner enquanto lê, o erro (o
    /// clique lê de novo) e o ×.
    pub(super) fn chips_de_issues_na_caixa(&self, chat: &str, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        let Some(anexos) = self.anexos_issue.get(chat) else { return Vec::new() };
        anexos
            .iter()
            .enumerate()
            .map(|(i, anexo)| {
                let (chat_tirar, chat_ler) = (chat.to_string(), chat.to_string());
                let (issue, issue_ler) = (anexo.issue.clone(), anexo.issue.clone());
                let (simbolo, dica): (AnyElement, String) = match &anexo.bloco {
                    None => (spinner(14., window), format!("{} · {}\nLendo a issue para mandar junto", issue.chave, issue.titulo)),
                    Some(Err(erro)) => (
                        icone(Lucide::TriangleAlert, 14., window).text_color(Cores::ambar()).into_any_element(),
                        format!("Não consegui ler a {}: {erro}\nClique para tentar de novo", issue.chave),
                    ),
                    Some(Ok(_)) => (
                        simbolo_da_fonte(issue.fonte).desenhar(14., window),
                        format!("{} · {}\nVai junto na próxima mensagem, como dados de fora", issue.chave, issue.titulo),
                    ),
                };
                let falhou = matches!(anexo.bloco, Some(Err(_)));
                div()
                    .id(("issue-anexada", i))
                    .flex()
                    .items_center()
                    .gap(esp::E6)
                    .h(r(28.))
                    .pl(esp::E8)
                    .pr(esp::E2)
                    .rounded(raio::PILULA)
                    .bg(Cores::hover())
                    .border_1()
                    .border_color(if falhou { Cores::tinta_borda(Cores::ambar()) } else { Cores::borda() })
                    .when(falhou, |s| {
                        s.cursor_pointer().on_click(cx.listener(move |este, _, _, cx| {
                            if let Some(a) = este.anexos_issue.get_mut(&chat_ler).and_then(|l| l.iter_mut().find(|a| mesma(&a.issue, &issue_ler))) {
                                a.bloco = None;
                            }
                            este.ler_bloco_da_issue(&chat_ler, issue_ler.clone(), cx);
                        }))
                    })
                    .child(div().flex_none().child(simbolo))
                    .child(div().flex_none().metadado().font_weight(FontWeight::MEDIUM).text_color(Cores::texto()).child(issue.chave.clone()))
                    .child(div().max_w(r(180.)).truncate().metadado().text_color(Cores::tenue()).child(issue.titulo.clone()))
                    .child(
                        BotaoRedondo::new(("tirar-issue", i), Some(Lucide::X.into()), "Tirar a issue")
                            .variante(VarianteRedonda::Plano)
                            .diametro(24.)
                            .on_click(cx.listener(move |este, _, _, cx| {
                                if let Some(lista) = este.anexos_issue.get_mut(&chat_tirar) {
                                    lista.retain(|a| !mesma(&a.issue, &issue));
                                }
                                cx.notify();
                            })),
                    )
                    .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                    .into_any_element()
            })
            .collect()
    }

    pub(super) fn tem_issue_anexada(&self, chat: &str) -> bool {
        self.anexos_issue.get(chat).is_some_and(|l| !l.is_empty())
    }

    /// As issues anexadas prontas para o envio, ou o aviso de por que ainda não.
    pub(super) fn issues_do_envio(&self, chat: &str) -> Result<Vec<AnexoIssue>, String> {
        issues_prontas(self.anexos_issue.get(chat).map_or(&[], Vec::as_slice))
    }

    /// Depois de enviar com a issue, num chat de projeto sem issue: pergunta uma vez se liga.
    pub(super) fn perguntar_ligacao(&mut self, motor: &Entity<Motor>, chat: &str, anexos: &[AnexoDeIssue], window: &mut Window, cx: &mut Context<Self>) {
        let sem_issue = {
            let m = motor.read(cx);
            m.chat(chat).filter(|c| c.papel == PapelDoChat::Projeto).and_then(|c| c.tarefa).and_then(|t| m.tarefa(t)).is_some_and(|t| t.issue.is_none())
        };
        let Some(anexo) = anexos.first().filter(|_| sem_issue) else { return };
        if !self.ligacoes_perguntadas.insert(chat.to_string()) {
            return;
        }
        let (motor, chat, ligada) = (motor.clone(), chat.to_string(), ligada_com_repositorio(&anexo.issue, anexo.repositorio.clone()));
        let chave = ligada.chave.clone();
        confirmar(
            Confirmacao {
                tom: TomConfirmacao::Neutro,
                icone: Lucide::Link.into(),
                titulo: format!("Ligar a {chave} a este chat?").into(),
                texto: "O ramo continua o mesmo; o PR vai citar a issue.".into(),
                detalhes: Vec::new(),
                ok: "Ligar".into(),
                cancelar: "Só contexto".into(),
                ok_perigo: false,
            },
            move |window, cx| {
                if let Err(erro) = motor.update(cx, |m, cx| m.ligar_issue(&chat, ligada.clone(), cx)) {
                    window.push_notification(aviso(Tom::Erro, format!("Não liguei a {}: {erro}", ligada.chave)), cx);
                }
            },
            None,
            window,
            cx,
        );
    }
}

#[cfg(test)]
mod testes {
    // Sem glob: o `gpui_kit::*` traz um `test` que tomaria o lugar do `#[test]`.
    use super::{
        AnexoDeIssue, Duration, EstadoIssue, FiltroIssues, FonteIssue, IssueLigada, IssueResumo, Lembrado, Recorte, TipoEstado,
        alternar_no_lote, estados_do_time, estados_github, id_github, imagens_viram_link, issues_prontas, link_seguro,
        meta_do_detalhe, pedido_ao_mestre, recorte_valido, resumo_da_ligada, subtitulo_da_issue, texto_da_copia,
        texto_do_rodape, texto_sem_itens,
    };
    use crate::issues::{Etiqueta, Pessoa};
    use crate::linear::{DadosDoTime, EstadoDoTime};

    fn issue(id: &str) -> IssueResumo {
        IssueResumo { id: id.into(), chave: format!("ENG-{id}"), titulo: format!("Issue {id}"), ..Default::default() }
    }

    #[test]
    fn o_lote_vai_ate_3_e_desmarca() {
        let mut lote = Vec::new();
        for id in ["1", "2", "3"] {
            assert!(alternar_no_lote(&mut lote, &issue(id)));
        }
        assert!(!alternar_no_lote(&mut lote, &issue("4")), "a 4ª não entra");
        assert_eq!(lote.len(), 3);
        // A mesma issue (fonte e id) desmarca, mesmo com outra chave.
        assert!(alternar_no_lote(&mut lote, &IssueResumo { chave: "OPS-2".into(), ..issue("2") }));
        assert_eq!(lote.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["1", "3"]);
        // O mesmo número no GitHub é outra issue.
        assert!(alternar_no_lote(&mut lote, &IssueResumo { fonte: FonteIssue::Github, ..issue("1") }));
        assert_eq!(lote.len(), 3);
    }

    #[test]
    fn subtitulo_e_meta_da_issue() {
        let estado = EstadoIssue { nome: "Em andamento".into(), tipo: TipoEstado::EmAndamento, ..Default::default() };
        let mut i = IssueResumo {
            chave: "ENG-162".into(),
            prioridade: Some(2),
            etiquetas: vec![Etiqueta { nome: "bug".into(), cor: None }, Etiqueta { nome: "login".into(), cor: None }],
            ..Default::default()
        };
        assert_eq!(subtitulo_da_issue(&i, &estado, Some("há 3 d".into())), "ENG-162 · Em andamento · Alta · bug · há 3 d");
        i.prioridade = Some(0);
        assert_eq!(subtitulo_da_issue(&i, &EstadoIssue::default(), None), "ENG-162 · bug");
        i.responsavel = Some(Pessoa { id: "u1".into(), nome: "Victor".into() });
        i.prioridade = Some(1);
        assert_eq!(meta_do_detalhe(&i, None), "Victor · Urgente · bug · login");
        i.responsavel = None;
        assert!(meta_do_detalhe(&i, None).starts_with("Sem responsável · "));
    }

    #[test]
    fn idade_pela_data_iso() {
        let ha = |segundos: u64| {
            let quando = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now() - Duration::from_secs(segundos));
            super::idade(&quando.to_rfc3339())
        };
        assert_eq!(ha(10).as_deref(), Some("agora"));
        assert_eq!(ha(5 * 60 + 3).as_deref(), Some("há 5 min"));
        assert_eq!(ha(3 * 86400 + 60).as_deref(), Some("há 3 d"));
        assert_eq!(super::idade("2026-10-01T12:00:00.000Z").is_some(), true);
        assert_eq!(super::idade("ontem"), None);
        assert_eq!(super::idade(""), None);
    }

    #[test]
    fn estados_do_github_e_do_time() {
        let ids: Vec<String> = estados_github().into_iter().map(|e| e.id).collect();
        // O que o motor (mudar_estado_issue) entende no GitHub.
        assert_eq!(ids, ["aberta", "concluida", "nao_planejada"]);
        let gh = |tipo| id_github(&EstadoIssue { tipo, ..Default::default() });
        assert_eq!((gh(TipoEstado::Aberta), gh(TipoEstado::Concluida), gh(TipoEstado::Cancelada)), ("aberta", "concluida", "nao_planejada"));
        assert_eq!(gh(TipoEstado::Fechada), "concluida");
        let estado = |id: &str, tipo, posicao| EstadoDoTime { estado: EstadoIssue { id: id.into(), nome: id.into(), tipo, cor: None }, posicao };
        let dados = DadosDoTime {
            estados: vec![
                estado("feito", TipoEstado::Concluida, 0.),
                estado("revisao", TipoEstado::EmAndamento, 2.),
                estado("fazendo", TipoEstado::EmAndamento, 1.),
                estado("backlog", TipoEstado::Backlog, 9.),
                estado("cancelada", TipoEstado::Cancelada, 0.),
                estado("triagem", TipoEstado::Triagem, 0.),
            ],
            ..Default::default()
        };
        let ordem: Vec<String> = estados_do_time(&dados).into_iter().map(|e| e.id).collect();
        assert_eq!(ordem, ["triagem", "backlog", "fazendo", "revisao", "feito", "cancelada"]);
    }

    #[test]
    fn so_http_abre_e_imagens_viram_link() {
        assert!(link_seguro("https://linear.app/eita/issue/ENG-1"));
        assert!(link_seguro("HTTP://exemplo.com"));
        for ruim in ["file:///etc/passwd", "javascript:alert(1)", "vitral:pr/o/r/1", "https://", "", "ftp://x", " data:text/html,x"] {
            assert!(!link_seguro(ruim), "{ruim}");
        }
        assert_eq!(imagens_viram_link("antes ![tela](https://x/y.png) depois"), "antes [imagem: tela](https://x/y.png) depois");
        assert_eq!(imagens_viram_link("![](https://x/y.png)"), "[imagem](https://x/y.png)");
        assert_eq!(imagens_viram_link("sem imagem [link](https://a) e !importante"), "sem imagem [link](https://a) e !importante");
        assert_eq!(imagens_viram_link("quebrado ![alt\nnada"), "quebrado ![alt\nnada");
    }

    #[test]
    fn recorte_segue_a_fonte_e_o_time() {
        use FonteIssue::{Github, Linear};
        assert_eq!(recorte_valido(Github, Recorte::DoTime, true, true), Recorte::Todas);
        assert_eq!(recorte_valido(Github, Recorte::Minhas, false, false), Recorte::Minhas);
        assert_eq!(recorte_valido(Linear, Recorte::DoTime, false, false), Recorte::Minhas, "sem time, só Minhas");
        assert_eq!(recorte_valido(Linear, Recorte::CicloAtual, true, false), Recorte::DoTime, "time sem ciclos");
        assert_eq!(recorte_valido(Linear, Recorte::CicloAtual, true, true), Recorte::CicloAtual);
        assert_eq!(recorte_valido(Linear, Recorte::Todas, true, true), Recorte::DoTime);
    }

    #[test]
    fn textos_da_lista() {
        let filtro = FiltroIssues::default();
        assert_eq!(texto_sem_itens(FonteIssue::Linear, &filtro, Some("ENG"), None, ""), "Nenhuma issue aberta atribuída a você");
        let do_time = FiltroIssues { recorte: Recorte::DoTime, ..Default::default() };
        assert_eq!(texto_sem_itens(FonteIssue::Linear, &do_time, Some("ENG"), None, ""), "Nenhuma issue aberta no time ENG");
        let todas = FiltroIssues { recorte: Recorte::Todas, fechadas: true, ..Default::default() };
        assert_eq!(
            texto_sem_itens(FonteIssue::Github, &todas, None, Some("VictorMenegate/HUB"), ""),
            "Nenhuma issue fechada nos últimos 30 dias em VictorMenegate/HUB"
        );
        assert_eq!(texto_sem_itens(FonteIssue::Github, &todas, None, None, "login"), "Nenhuma issue fechada nos últimos 30 dias com “login”");
        assert_eq!(texto_do_rodape(12, 12, Some(Duration::from_secs(70)), false), "12 de 12 · atualizado há 1 min");
        assert_eq!(texto_do_rodape(3, 50, Some(Duration::from_secs(5)), true), "3 de 50+ · atualizado agora");
        assert_eq!(texto_do_rodape(2, 2, None, false), "2 de 2");
        assert_eq!(texto_da_copia("ENG-123"), "Resolva a issue ENG-123.");
        assert_eq!(pedido_ao_mestre("vitral", "ENG-162"), "#vitral pegue a ENG-162");
    }

    #[test]
    fn a_issue_ligada_abre_o_detalhe_pelo_id_do_motor() {
        let ligada = |fonte, id: &str, repositorio: Option<&str>| IssueLigada {
            fonte,
            id: id.into(),
            chave: "#45".into(),
            repositorio: repositorio.map(String::from),
            ..Default::default()
        };
        // No GitHub, com o repositório: "dono/nome#45" (o detalhe não depende da lista).
        assert_eq!(resumo_da_ligada(&ligada(FonteIssue::Github, "45", Some("VictorMenegate/HUB"))).id, "VictorMenegate/HUB#45");
        assert_eq!(resumo_da_ligada(&ligada(FonteIssue::Github, "o/r#45", Some("o/r"))).id, "o/r#45");
        assert_eq!(resumo_da_ligada(&ligada(FonteIssue::Github, "45", None)).id, "45");
        assert_eq!(resumo_da_ligada(&ligada(FonteIssue::Linear, "uuid-1", None)).id, "uuid-1");
    }

    #[test]
    fn so_envia_com_o_bloco_lido() {
        let anexo = |bloco: Option<Result<String, String>>| AnexoDeIssue { issue: issue("5"), repositorio: None, bloco };
        let prontas = issues_prontas(&[anexo(Some(Ok("<issue>".into())))]).unwrap();
        assert_eq!((prontas[0].issue.chave.as_str(), prontas[0].issue.titulo.as_str(), prontas[0].bloco.as_str()), ("ENG-5", "Issue 5", "<issue>"));
        // No GitHub sem o repositório na URL, vale o da lista.
        let gh = AnexoDeIssue {
            issue: IssueResumo { fonte: FonteIssue::Github, url: String::new(), ..issue("45") },
            repositorio: Some("o/r".into()),
            bloco: Some(Ok("x".into())),
        };
        assert_eq!(issues_prontas(&[gh]).unwrap()[0].issue.repositorio.as_deref(), Some("o/r"));
        assert_eq!(issues_prontas(&[anexo(None)]).unwrap_err(), "Ainda lendo a ENG-5; envie de novo em um instante.");
        assert!(issues_prontas(&[anexo(Some(Err("sem rede".into())))]).unwrap_err().starts_with("Não consegui ler a ENG-5 (sem rede)."));
        assert!(issues_prontas(&[]).unwrap().is_empty());
    }

    #[test]
    fn o_lembrado_le_o_antigo_e_nao_grava_a_busca() {
        let lido: Lembrado = serde_json::from_str(r#"{"linear":{"recorte":"DoTime"},"nova_chave":1}"#).unwrap();
        assert_eq!(lido.linear.recorte, Recorte::DoTime);
        assert_eq!(lido.github, FiltroIssues::default());
        let vazio: Lembrado = serde_json::from_str("{}").unwrap();
        assert_eq!(vazio, Lembrado::default());
    }
}
