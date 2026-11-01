//! Linear por GraphQL, com o `curl` como subprocesso (nenhuma crate HTTP). A chave e o corpo vão
//! pelo stdin (`-K -`), nunca no argv nem no ambiente; o ambiente do curl é limpo, o endereço é
//! fixo e o `-q` vem primeiro, para nenhum `~/.curlrc` ligar o modo verboso.
// Provisório: o motor passa a usar estes módulos no G3.
#![allow(dead_code)]

use std::{path::Path, time::Duration};

use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use zeroize::Zeroizing;

use crate::{
    issues::{ComentarioIssue, EstadoIssue, Etiqueta, FiltroIssues, FonteIssue, IssueDetalhe, IssueResumo, Pessoa, Recorte, TipoEstado},
    modelo::LinearDoProjeto,
    remoto::{Ambiente, ErroRemoto, Executor, classificar_curl},
    segredos::Chave,
};

pub trait Transporte: Send + Sync {
    fn postar(&self, chave: &Chave, corpo_json: &str) -> Result<RespostaHttp, ErroRemoto>;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RespostaHttp {
    pub status: u16,
    pub cabecalhos: Vec<(String, String)>,
    pub corpo: String,
}

/// O transporte de verdade: o curl pelo executor.
pub struct Curl<'a>(pub &'a dyn Executor);

const ENDERECO: &str = "https://api.linear.app/graphql";
/// O `--max-time 20` do curl, com folga.
const LIMITE: Duration = Duration::from_secs(25);

pub fn argumentos_curl() -> Vec<String> {
    [
        "-q",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--tlsv1.2",
        "--max-time",
        "20",
        "--connect-timeout",
        "8",
        "--compressed",
        "-D",
        "-",
        "-K",
        "-",
    ]
    .map(String::from)
    .to_vec()
}

/// Limpo: só o caminho, a casa, a língua e o proxy (sem `SSLKEYLOGFILE`, `CURL_HOME` e o resto).
pub fn ambiente_curl() -> Ambiente {
    let mut manter = vec!["PATH", "HOME", "LANG", "HTTPS_PROXY", "https_proxy", "NO_PROXY", "no_proxy"];
    if cfg!(windows) {
        // Sem ela, a rede do Windows não abre no processo filho.
        manter.push("SYSTEMROOT");
    }
    Ambiente { limpar: true, manter, definir: Vec::new(), remover: Vec::new() }
}

/// Um valor entre aspas na configuração do curl.
fn entre_aspas(texto: &str) -> String {
    let mut saida = String::with_capacity(texto.len() + 2);
    saida.push('"');
    for c in texto.chars() {
        match c {
            '\\' => saida.push_str("\\\\"),
            '"' => saida.push_str("\\\""),
            '\n' => saida.push_str("\\n"),
            '\r' => saida.push_str("\\r"),
            '\t' => saida.push_str("\\t"),
            c => saida.push(c),
        }
    }
    saida.push('"');
    saida
}

/// O que vai no stdin do curl: endereço, cabeçalhos (a chave sem "Bearer") e o corpo.
pub fn configuracao_curl(chave: &Chave, corpo_json: &str) -> Zeroizing<String> {
    let mut texto = Zeroizing::new(String::new());
    texto.push_str(&format!("url = {}\n", entre_aspas(ENDERECO)));
    // A chave já passou pela validação (só letras e números depois de lin_api_): não precisa de
    // escape, e assim nenhuma cópia dela fica fora do Zeroizing.
    texto.push_str("header = \"Authorization: ");
    texto.push_str(chave.texto());
    texto.push_str("\"\n");
    texto.push_str("header = \"Content-Type: application/json\"\n");
    texto.push_str(&format!("header = \"User-Agent: Vitral/{}\"\n", env!("CARGO_PKG_VERSION")));
    texto.push_str(&format!("data-binary = {}\n", entre_aspas(corpo_json)));
    texto
}

/// A saída do `curl -D -`: um ou mais blocos de cabeçalho (um 100 Continue, um redirecionamento)
/// e o corpo. Vale o último bloco.
pub fn ler_resposta(saida: &str) -> Result<RespostaHttp, ErroRemoto> {
    let mut resto = saida;
    let mut ultimo = None;
    while resto.starts_with("HTTP/") {
        let (bloco, depois) = match (resto.find("\r\n\r\n"), resto.find("\n\n")) {
            (Some(i), _) => (&resto[..i], &resto[i + 4..]),
            (None, Some(i)) => (&resto[..i], &resto[i + 2..]),
            (None, None) => (resto, ""),
        };
        let mut linhas = bloco.lines();
        let status = linhas
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or_else(|| ErroRemoto::Outro("resposta do Linear com status ilegível".into()))?;
        let cabecalhos = linhas
            .filter_map(|l| l.split_once(':'))
            .map(|(nome, valor)| (nome.trim().to_string(), valor.trim().to_string()))
            .collect();
        ultimo = Some((status, cabecalhos));
        resto = depois;
    }
    let (status, cabecalhos) = ultimo.ok_or_else(|| ErroRemoto::Outro("resposta do Linear sem cabeçalho HTTP".into()))?;
    Ok(RespostaHttp { status, cabecalhos, corpo: resto.to_string() })
}

impl Transporte for Curl<'_> {
    fn postar(&self, chave: &Chave, corpo_json: &str) -> Result<RespostaHttp, ErroRemoto> {
        let configuracao = configuracao_curl(chave, corpo_json);
        let pasta = Path::new("/");
        let saida = self.0.rodar("curl", &argumentos_curl(), &ambiente_curl(), pasta, Some(configuracao.as_bytes()), LIMITE)?;
        if saida.codigo != Some(0) {
            return Err(classificar_curl(saida.codigo, None, &[], &saida.erros()));
        }
        ler_resposta(&saida.texto())
    }
}

/// O `data` de uma resposta e quantos pedidos ainda cabem na hora (abaixo de 100, as listas só
/// atualizam com ⟳).
#[derive(Clone, Debug, PartialEq)]
pub struct Resposta {
    pub dados: Value,
    pub restantes: Option<u32>,
}

/// Uma consulta. HTTP 200 com `errors` não vazio é erro, classificado pelo primeiro
/// `extensions.code`.
pub fn graphql(t: &dyn Transporte, chave: &Chave, consulta: &str, variaveis: Value) -> Result<Value, ErroRemoto> {
    graphql_completo(t, chave, consulta, variaveis).map(|r| r.dados)
}

pub fn graphql_completo(t: &dyn Transporte, chave: &Chave, consulta: &str, variaveis: Value) -> Result<Resposta, ErroRemoto> {
    let corpo = json!({ "query": consulta, "variables": variaveis }).to_string();
    let resposta = t.postar(chave, &corpo)?;
    let valor: Value = serde_json::from_str(&resposta.corpo).unwrap_or(Value::Null);
    let com_erros = valor["errors"].as_array().is_some_and(|e| !e.is_empty());
    if resposta.status != 200 || com_erros {
        return Err(classificar_curl(Some(0), Some(resposta.status), &resposta.cabecalhos, &resposta.corpo));
    }
    if !valor["data"].is_object() {
        return Err(ErroRemoto::Outro("resposta do Linear sem data".into()));
    }
    let restantes = resposta
        .cabecalhos
        .iter()
        .find(|(nome, _)| nome.eq_ignore_ascii_case("x-ratelimit-requests-remaining"))
        .and_then(|(_, valor)| valor.parse().ok());
    Ok(Resposta { dados: valor["data"].clone(), restantes })
}

// ---------- consultas (validadas contra o schema na pesquisa) ----------

const EU: &str = "query { viewer { id name displayName } organization { id name urlKey } }";

const TIMES: &str = "query { teams(first: 100) { nodes { id key name color cyclesEnabled } } }";

const DO_TIME: &str =
    "query($id: String!) { team(id: $id) { id key name states(first: 50) { nodes { id name type color position } } } }";

const CAMPOS_DA_LISTA: &str = "id identifier title priority priorityLabel url branchName updatedAt createdAt \
     state { id name type color } assignee { id name } team { id key } cycle { id number } \
     labels(first: 3) { nodes { name color } }";

const ISSUES: &str = "query($filtro: IssueFilter, $depois: String) { \
     issues(first: 50, after: $depois, orderBy: updatedAt, filter: $filtro) { \
     nodes { CAMPOS } pageInfo { hasNextPage endCursor } } }";

/// Os campos da busca são os que a pesquisa validou no `IssueSearchResult`.
const BUSCAR: &str = "query($termo: String!, $time: String) { \
     searchIssues(term: $termo, teamId: $time, first: 25) { \
     nodes { id identifier title url branchName priority state { id name type color } } } }";

const VER_ISSUE: &str = "query($id: String!) { issue(id: $id) { \
     id identifier title description url branchName priority priorityLabel createdAt updatedAt \
     state { id name type color } assignee { id name } creator { id name } team { id key name } \
     project { id name } cycle { id number name } labels(first: 20) { nodes { name color } } \
     comments(first: 50) { nodes { id body createdAt url user { id name } botActor { name } } } } }";

/// O que os efeitos de iniciar conferem antes de mexer (seção 5.6): o estado, o responsável e os
/// estados do time da issue (o "Automático" é o `started` de menor posição do time dela).
const AO_INICIAR: &str = "query($id: String!) { issue(id: $id) { id identifier state { id name type color } \
     assignee { id } team { id key states(first: 50) { nodes { id name type color position } } } } }";

const MUDAR_ESTADO: &str = "mutation($id: String!, $estado: String!) { \
     issueUpdate(id: $id, input: { stateId: $estado }) { success issue { id identifier state { id name type color } } } }";

const ATRIBUIR: &str = "mutation($id: String!, $pessoa: String) { \
     issueUpdate(id: $id, input: { assigneeId: $pessoa }) { success issue { id identifier assignee { id name } } } }";

/// O `id` é do Vitral (UUID v4): repetir com o mesmo id não duplica o comentário.
const COMENTAR: &str = "mutation($id: String, $issue: String!, $corpo: String!) { \
     commentCreate(input: { id: $id, issueId: $issue, body: $corpo }) { success comment { id url } } }";

const LIGAR_PR: &str = "mutation($issue: String!, $url: String!) { \
     attachmentLinkGitHubPR(issueId: $issue, url: $url) { success } }";

const LIGAR_URL: &str = "mutation($issue: String!, $url: String!, $titulo: String) { \
     attachmentLinkURL(issueId: $issue, url: $url, title: $titulo) { success } }";

// ---------- filtros ----------

/// O filtro do GraphQL das issues do popover (pura). O time, se o projeto tem; "Minhas" sem time
/// vale para o workspace inteiro. As fechadas são as dos últimos 30 dias.
pub fn filtro_linear(f: &FiltroIssues, projeto: Option<&LinearDoProjeto>) -> Value {
    let mut filtro = serde_json::Map::new();
    if let Some(time) = projeto.map(|p| p.time_id.trim()).filter(|t| !t.is_empty()) {
        filtro.insert("team".into(), json!({ "id": { "eq": time } }));
    }
    match f.recorte {
        Recorte::Minhas => {
            filtro.insert("assignee".into(), json!({ "isMe": { "eq": true } }));
        }
        Recorte::CicloAtual => {
            filtro.insert("cycle".into(), json!({ "isActive": { "eq": true } }));
        }
        Recorte::DoTime | Recorte::Todas => {}
    }
    if f.fechadas {
        filtro.insert("state".into(), json!({ "type": { "in": ["completed", "canceled"] } }));
        filtro.insert("updatedAt".into(), json!({ "gt": "-P30D" }));
    } else {
        filtro.insert("state".into(), json!({ "type": { "nin": ["completed", "canceled", "duplicate"] } }));
    }
    Value::Object(filtro)
}

// ---------- tipos ----------

/// Uma página da lista e quantos pedidos ainda cabem na hora.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pagina {
    pub itens: Vec<IssueResumo>,
    /// O cursor da próxima página, se houver.
    pub proxima: Option<String>,
    pub restantes: Option<u32>,
}

/// O que os efeitos de iniciar precisam saber da issue agora.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AoIniciar {
    pub estado: EstadoIssue,
    /// O id do responsável, se tem.
    pub responsavel: Option<String>,
    pub time_id: String,
    pub estados: Vec<EstadoDoTime>,
}

/// Quem é o dono da chave e de que workspace.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Eu {
    pub id: String,
    pub nome: String,
    pub nome_exibicao: String,
    pub workspace: String,
    pub url_key: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Time {
    pub id: String,
    pub chave: String,
    pub nome: String,
    pub cor: Option<String>,
    pub ciclos: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EstadoDoTime {
    pub estado: EstadoIssue,
    /// A ordem no fluxo do time ("começar" é o `started` de menor posição).
    pub posicao: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DadosDoTime {
    pub id: String,
    pub chave: String,
    pub nome: String,
    pub estados: Vec<EstadoDoTime>,
}

fn nulo<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

#[derive(Deserialize, Default)]
#[serde(default, bound(deserialize = "T: Deserialize<'de> + Default"))]
struct Nos<T> {
    #[serde(deserialize_with = "nulo")]
    nodes: Vec<T>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Objeto {
    #[serde(deserialize_with = "nulo")]
    id: String,
    #[serde(deserialize_with = "nulo")]
    name: String,
    #[serde(deserialize_with = "nulo")]
    key: String,
    #[serde(deserialize_with = "nulo")]
    display_name: String,
    #[serde(deserialize_with = "nulo")]
    url_key: String,
    color: Option<String>,
    cycles_enabled: bool,
    number: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct EstadoLinear {
    #[serde(deserialize_with = "nulo")]
    id: String,
    #[serde(deserialize_with = "nulo")]
    name: String,
    #[serde(rename = "type", deserialize_with = "nulo")]
    tipo: String,
    color: Option<String>,
    position: f64,
}

impl EstadoLinear {
    fn estado(&self) -> EstadoIssue {
        EstadoIssue { id: self.id.clone(), nome: self.name.clone(), tipo: TipoEstado::do_linear(&self.tipo), cor: self.color.clone() }
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ComentarioLinear {
    #[serde(deserialize_with = "nulo")]
    body: String,
    #[serde(deserialize_with = "nulo")]
    created_at: String,
    #[serde(deserialize_with = "nulo")]
    url: String,
    user: Option<Objeto>,
    bot_actor: Option<Objeto>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct IssueLinear {
    #[serde(deserialize_with = "nulo")]
    id: String,
    #[serde(deserialize_with = "nulo")]
    identifier: String,
    #[serde(deserialize_with = "nulo")]
    title: String,
    #[serde(deserialize_with = "nulo")]
    description: String,
    priority: Option<f64>,
    #[serde(deserialize_with = "nulo")]
    url: String,
    branch_name: Option<String>,
    #[serde(deserialize_with = "nulo")]
    updated_at: String,
    #[serde(deserialize_with = "nulo")]
    created_at: String,
    state: Option<EstadoLinear>,
    assignee: Option<Objeto>,
    creator: Option<Objeto>,
    team: Option<Objeto>,
    project: Option<Objeto>,
    cycle: Option<Objeto>,
    #[serde(deserialize_with = "nulo")]
    labels: Nos<Objeto>,
    #[serde(deserialize_with = "nulo")]
    comments: Nos<ComentarioLinear>,
}

impl IssueLinear {
    fn resumo(&self) -> IssueResumo {
        IssueResumo {
            fonte: FonteIssue::Linear,
            id: self.id.clone(),
            chave: self.identifier.clone(),
            titulo: self.title.clone(),
            estado: self.state.as_ref().map(EstadoLinear::estado).unwrap_or_default(),
            responsavel: self.assignee.as_ref().map(|p| Pessoa { id: p.id.clone(), nome: p.name.clone() }),
            etiquetas: self.labels.nodes.iter().map(|e| Etiqueta { nome: e.name.clone(), cor: e.color.clone() }).collect(),
            prioridade: self.priority.map(|p| p.clamp(0.0, 4.0) as u8),
            atualizada: self.updated_at.clone(),
            criada: self.created_at.clone(),
            url: self.url.clone(),
            ramo_sugerido: self.branch_name.clone().filter(|r| !r.is_empty()),
            autor: self.creator.as_ref().map(|c| c.name.clone()),
        }
    }

    fn detalhe(self) -> IssueDetalhe {
        let resumo = self.resumo();
        let mut comentarios: Vec<ComentarioIssue> = self
            .comments
            .nodes
            .into_iter()
            .map(|c| ComentarioIssue {
                autor: c.user.or(c.bot_actor).map(|a| a.name).unwrap_or_default(),
                corpo: c.body,
                criado: c.created_at,
                url: c.url,
            })
            .collect();
        // Do mais antigo para o mais novo; ficam os 10 últimos.
        comentarios.sort_by(|a, b| a.criado.cmp(&b.criado));
        let sobra = comentarios.len().saturating_sub(10);
        comentarios.drain(..sobra);
        IssueDetalhe {
            resumo,
            descricao: self.description,
            comentarios,
            ciclo: self.cycle.and_then(|c| match (c.name.is_empty(), c.number) {
                (false, _) => Some(c.name),
                (true, Some(n)) => Some(format!("Ciclo {n}")),
                (true, None) => None,
            }),
            projeto: self.project.map(|p| p.name).filter(|n| !n.is_empty()),
            time: self.team.map(|t| t.key).filter(|k| !k.is_empty()),
            repositorio: None,
        }
    }
}

fn ler<T: for<'de> Deserialize<'de>>(valor: &Value) -> Result<T, ErroRemoto> {
    T::deserialize(valor).map_err(|erro| ErroRemoto::Outro(format!("resposta do Linear fora do esperado: {erro}")))
}

/// Uma mutação que respondeu `success: false`.
fn conferir_sucesso(valor: &Value) -> Result<(), ErroRemoto> {
    if valor["success"] == true {
        Ok(())
    } else {
        Err(ErroRemoto::Validacao("o Linear não aceitou a mudança".into()))
    }
}

/// O Linear de uma sessão: o transporte e a chave (que só existe na memória do Vitral).
pub struct Linear<'a> {
    pub t: &'a dyn Transporte,
    pub chave: &'a Chave,
}

impl Linear<'_> {
    fn consultar(&self, consulta: &str, variaveis: Value) -> Result<Value, ErroRemoto> {
        graphql(self.t, self.chave, consulta, variaveis)
    }

    /// Conectar, "Testar" e a sonda.
    pub fn eu(&self) -> Result<Eu, ErroRemoto> {
        let dados = self.consultar(EU, json!({}))?;
        let (pessoa, organizacao): (Objeto, Objeto) = (ler(&dados["viewer"])?, ler(&dados["organization"])?);
        Ok(Eu {
            id: pessoa.id,
            nome: pessoa.name,
            nome_exibicao: pessoa.display_name,
            workspace: organizacao.name,
            url_key: organizacao.url_key,
        })
    }

    pub fn times(&self) -> Result<Vec<Time>, ErroRemoto> {
        let dados = self.consultar(TIMES, json!({}))?;
        let times: Nos<Objeto> = ler(&dados["teams"])?;
        Ok(times
            .nodes
            .into_iter()
            .map(|t| Time { id: t.id, chave: t.key, nome: t.name, cor: t.color, ciclos: t.cycles_enabled })
            .collect())
    }

    /// Os estados do fluxo do time (cache de 1 h no motor).
    pub fn do_time(&self, id: &str) -> Result<DadosDoTime, ErroRemoto> {
        let dados = self.consultar(DO_TIME, json!({ "id": id }))?;
        let time = &dados["team"];
        if time.is_null() {
            return Err(ErroRemoto::NaoEncontrado(format!("o time {id}")));
        }
        let cabecalho: Objeto = ler(time)?;
        let estados: Nos<EstadoLinear> = ler(&time["states"])?;
        let mut estados: Vec<EstadoDoTime> =
            estados.nodes.iter().map(|e| EstadoDoTime { estado: e.estado(), posicao: e.position }).collect();
        estados.sort_by(|a, b| a.posicao.total_cmp(&b.posicao));
        Ok(DadosDoTime { id: cabecalho.id, chave: cabecalho.key, nome: cabecalho.name, estados })
    }

    /// Uma página da lista (50), com o cursor da próxima se houver.
    pub fn listar_issues(&self, filtro: Value, depois: Option<&str>) -> Result<(Vec<IssueResumo>, Option<String>), ErroRemoto> {
        self.pagina(filtro, depois).map(|p| (p.itens, p.proxima))
    }

    /// A página com quantos pedidos ainda cabem na hora (abaixo de 100, as listas só atualizam
    /// com ⟳).
    pub fn pagina(&self, filtro: Value, depois: Option<&str>) -> Result<Pagina, ErroRemoto> {
        let consulta = ISSUES.replace("CAMPOS", CAMPOS_DA_LISTA);
        let resposta = graphql_completo(self.t, self.chave, &consulta, json!({ "filtro": filtro, "depois": depois }))?;
        let dados = resposta.dados;
        let issues: Nos<IssueLinear> = ler(&dados["issues"])?;
        let pagina = &dados["issues"]["pageInfo"];
        let proxima = (pagina["hasNextPage"] == true).then(|| pagina["endCursor"].as_str().map(String::from)).flatten();
        Ok(Pagina { itens: issues.nodes.iter().map(IssueLinear::resumo).collect(), proxima, restantes: resposta.restantes })
    }

    /// O estado, o responsável e os estados do time da issue (o uuid ou a chave).
    pub fn ao_iniciar(&self, id: &str) -> Result<AoIniciar, ErroRemoto> {
        let dados = self.consultar(AO_INICIAR, json!({ "id": id }))?;
        let issue = &dados["issue"];
        if issue.is_null() {
            return Err(ErroRemoto::NaoEncontrado(format!("a issue {id}")));
        }
        let estado: EstadoLinear = ler(&issue["state"])?;
        let responsavel = issue["assignee"]["id"].as_str().filter(|r| !r.is_empty()).map(String::from);
        let estados: Nos<EstadoLinear> = ler(&issue["team"]["states"])?;
        let mut estados: Vec<EstadoDoTime> =
            estados.nodes.iter().map(|e| EstadoDoTime { estado: e.estado(), posicao: e.position }).collect();
        estados.sort_by(|a, b| a.posicao.total_cmp(&b.posicao));
        Ok(AoIniciar {
            estado: estado.estado(),
            responsavel,
            time_id: issue["team"]["id"].as_str().unwrap_or_default().to_string(),
            estados,
        })
    }

    /// A busca no servidor (com Enter e 3 ou mais caracteres).
    pub fn buscar(&self, termo: &str, time: Option<&str>) -> Result<Vec<IssueResumo>, ErroRemoto> {
        let dados = self.consultar(BUSCAR, json!({ "termo": termo, "time": time }))?;
        let issues: Nos<IssueLinear> = ler(&dados["searchIssues"])?;
        Ok(issues.nodes.iter().map(IssueLinear::resumo).collect())
    }

    /// O detalhe, pelo uuid ou pela chave ("ENG-123").
    pub fn ver_issue(&self, id: &str) -> Result<IssueDetalhe, ErroRemoto> {
        let dados = self.consultar(VER_ISSUE, json!({ "id": id }))?;
        if dados["issue"].is_null() {
            return Err(ErroRemoto::NaoEncontrado(format!("a issue {id}")));
        }
        Ok(ler::<IssueLinear>(&dados["issue"])?.detalhe())
    }

    /// Move a issue para o estado; devolve o estado que ela ficou.
    pub fn mudar_estado(&self, id: &str, estado: &str) -> Result<EstadoIssue, ErroRemoto> {
        let dados = self.consultar(MUDAR_ESTADO, json!({ "id": id, "estado": estado }))?;
        let resultado = &dados["issueUpdate"];
        conferir_sucesso(resultado)?;
        Ok(ler::<EstadoLinear>(&resultado["issue"]["state"])?.estado())
    }

    /// `pessoa = None` tira o responsável.
    pub fn atribuir(&self, id: &str, pessoa: Option<&str>) -> Result<(), ErroRemoto> {
        let dados = self.consultar(ATRIBUIR, json!({ "id": id, "pessoa": pessoa }))?;
        conferir_sucesso(&dados["issueUpdate"])
    }

    /// Comenta com o id do Vitral (`id_cliente`, UUID v4) e devolve o id do comentário.
    pub fn comentar(&self, issue: &str, corpo: &str, id_cliente: &str) -> Result<String, ErroRemoto> {
        let dados = self.consultar(COMENTAR, json!({ "id": id_cliente, "issue": issue, "corpo": corpo }))?;
        let resultado = &dados["commentCreate"];
        conferir_sucesso(resultado)?;
        Ok(resultado["comment"]["id"].as_str().unwrap_or(id_cliente).to_string())
    }

    /// Liga o PR à issue. Sem a integração GitHub do Linear o `attachmentLinkGitHubPR` falha: cai
    /// para um anexo de link comum. Erro de rede ou de login não cai (vale para os dois).
    pub fn ligar_pr(&self, issue: &str, url: &str, titulo: &str) -> Result<(), ErroRemoto> {
        let pelo_github = self
            .consultar(LIGAR_PR, json!({ "issue": issue, "url": url }))
            .and_then(|dados| conferir_sucesso(&dados["attachmentLinkGitHubPR"]));
        match pelo_github {
            Ok(()) => Ok(()),
            Err(erro) if erro.transitorio() || erro == ErroRemoto::SemLogin => Err(erro),
            Err(_) => self.ligar_url(issue, url, titulo),
        }
    }

    pub fn ligar_url(&self, issue: &str, url: &str, titulo: &str) -> Result<(), ErroRemoto> {
        let dados = self.consultar(LIGAR_URL, json!({ "issue": issue, "url": url, "titulo": titulo }))?;
        conferir_sucesso(&dados["attachmentLinkURL"])
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::remoto::falso::{Falso, saida};

    const CHAVE: &str = concat!("lin_", "api_ABCdef0123456789xyzWa1b2");

    fn chave() -> Chave {
        Chave::linear(CHAVE).unwrap()
    }

    /// A resposta do curl com `-D -`: um 100 Continue antes do bloco de verdade.
    fn http(corpo: &str) -> Result<crate::remoto::SaidaBruta, ErroRemoto> {
        saida(
            0,
            &format!(
                "HTTP/2 100 \r\n\r\nHTTP/2 200 \r\ncontent-type: application/json; charset=utf-8\r\nx-ratelimit-requests-remaining: 1499\r\n\r\n{corpo}"
            ),
            "",
        )
    }

    #[test]
    fn configuracao_curl_escapa_e_a_chave_so_vai_no_stdin() {
        let e = Falso::com(vec![http(r#"{"data":{"ok":true}}"#)]);
        let corpo = json!({ "query": "q", "variables": { "corpo": "linha 1\nlinha \"2\" com \\ e ação" } }).to_string();
        let resposta = Curl(&e).postar(&chave(), &corpo).unwrap();
        assert_eq!(resposta.status, 200);
        let chamada = &e.chamadas()[0];
        assert_eq!(chamada.programa, "curl");
        assert_eq!(chamada.args[0], "-q");
        assert_eq!(chamada.args, argumentos_curl());
        assert!(chamada.args.windows(2).any(|p| p == ["--proto", "=https"]));
        assert!(chamada.args.windows(2).any(|p| p == ["-K", "-"]));
        assert_eq!(chamada.pasta, Path::new("/"));
        // Ambiente limpo: só o caminho, a casa, a língua e o proxy.
        assert!(chamada.ambiente.limpar);
        for fora in ["SSLKEYLOGFILE", "CURL_HOME", "LINEAR_API_KEY"] {
            assert!(!chamada.ambiente.manter.contains(&fora));
        }
        assert!(chamada.ambiente.manter.contains(&"HTTPS_PROXY"));
        assert!(chamada.ambiente.definir.is_empty());
        let texto_args = chamada.args.join(" ");
        assert!(!texto_args.contains("lin_api_") && !texto_args.contains("Authorization"));
        let entrada = String::from_utf8(chamada.entrada.clone().unwrap()).unwrap();
        let linhas: Vec<&str> = entrada.lines().collect();
        assert_eq!(linhas[0], "url = \"https://api.linear.app/graphql\"");
        assert_eq!(linhas[1], format!("header = \"Authorization: {CHAVE}\""));
        assert_eq!(linhas[2], "header = \"Content-Type: application/json\"");
        assert!(linhas[3].starts_with("header = \"User-Agent: Vitral/"));
        // No JSON o \n já é "\" + "n"; na configuração do curl, cada "\" e cada aspa ganham um "\".
        assert_eq!(
            linhas[4],
            r#"data-binary = "{\"query\":\"q\",\"variables\":{\"corpo\":\"linha 1\\nlinha \\\"2\\\" com \\\\ e ação\"}}""#
        );
        assert_eq!(linhas.len(), 5);
        assert_eq!(entre_aspas("a\nb\tc\r"), r#""a\nb\tc\r""#);
    }

    #[test]
    fn le_a_resposta_com_dois_blocos_de_cabecalho() {
        let saida = "HTTP/1.1 100 Continue\r\n\r\nHTTP/2 429 \r\nretry-after: 30\r\nX-RateLimit-Requests-Remaining: 0\r\n\r\n{\"errors\":[]}";
        let resposta = ler_resposta(saida).unwrap();
        assert_eq!(resposta.status, 429);
        assert_eq!(resposta.cabecalhos[0], ("retry-after".to_string(), "30".to_string()));
        assert_eq!(resposta.corpo, "{\"errors\":[]}");
        let so_n = ler_resposta("HTTP/2 200\ncontent-type: x\n\n{}").unwrap();
        assert_eq!((so_n.status, so_n.corpo.as_str()), (200, "{}"));
        assert!(ler_resposta("{}").is_err());
        assert!(ler_resposta("HTTP/2 xyz\r\n\r\n{}").is_err());
    }

    #[test]
    fn graphql_classifica_erros_e_le_o_limite() {
        let erro = |code: &str| format!(r#"{{"data":null,"errors":[{{"message":"falhou","extensions":{{"code":"{code}"}}}}]}}"#);
        let e = Falso::com(vec![
            http(r#"{"data":{"viewer":{"id":"u1"}}}"#),
            http(&erro("AUTHENTICATION_ERROR")),
            http(&erro("FORBIDDEN")),
            http(&erro("INVALID_INPUT")),
            saida(0, "HTTP/2 401 \r\n\r\n{\"errors\":[{\"message\":\"Authentication required\"}]}", ""),
            saida(0, "HTTP/2 429 \r\nRetry-After: 60\r\n\r\n{\"errors\":[{\"message\":\"x\",\"extensions\":{\"code\":\"RATELIMITED\"}}]}", ""),
            saida(6, "", "curl: (6) Could not resolve host: api.linear.app"),
            saida(28, "", "curl: (28) Operation timed out after 20001 milliseconds"),
        ]);
        let t = Curl(&e);
        let chave = chave();
        let resposta = graphql_completo(&t, &chave, "q", json!({})).unwrap();
        assert_eq!(resposta.dados["viewer"]["id"], "u1");
        assert_eq!(resposta.restantes, Some(1499));
        assert_eq!(graphql(&t, &chave, "q", json!({})), Err(ErroRemoto::SemLogin));
        assert!(matches!(graphql(&t, &chave, "q", json!({})), Err(ErroRemoto::SemPermissao(m)) if m == "falhou"));
        assert!(matches!(graphql(&t, &chave, "q", json!({})), Err(ErroRemoto::Validacao(_))));
        assert_eq!(graphql(&t, &chave, "q", json!({})), Err(ErroRemoto::SemLogin));
        assert!(matches!(graphql(&t, &chave, "q", json!({})), Err(ErroRemoto::Limite { ate: Some(_) })));
        assert!(matches!(graphql(&t, &chave, "q", json!({})), Err(ErroRemoto::SemRede(_))));
        assert_eq!(graphql(&t, &chave, "q", json!({})), Err(ErroRemoto::TempoEsgotado));
    }

    /// Respostas no formato do schema do Linear (sem chave real, não há amostra gravada).
    #[test]
    fn as_funcoes_do_linear_leem_as_respostas() {
        let eu = r#"{"data":{"viewer":{"id":"u1","name":"Victor","displayName":"victor"},"organization":{"id":"o1","name":"Eita","urlKey":"eita"}}}"#;
        let times = r##"{"data":{"teams":{"nodes":[{"id":"t1","key":"ENG","name":"Engenharia","color":"#5e6ad2","cyclesEnabled":true}]}}}"##;
        let do_time = r##"{"data":{"team":{"id":"t1","key":"ENG","name":"Engenharia","states":{"nodes":[
            {"id":"s3","name":"Done","type":"completed","color":"#5e6ad2","position":3},
            {"id":"s2","name":"In Progress","type":"started","color":"#f2c94c","position":2},
            {"id":"s1","name":"Todo","type":"unstarted","color":"#e2e2e2","position":1}]}}}}"##;
        let lista = r##"{"data":{"issues":{"nodes":[{"id":"i1","identifier":"ENG-162","title":"Sentry: 39 problemas","priority":2,
            "priorityLabel":"High","url":"https://linear.app/eita/issue/ENG-162/sentry","branchName":"victor/eng-162-sentry",
            "updatedAt":"2026-10-29T10:00:00.000Z","createdAt":"2026-10-28T10:00:00.000Z",
            "state":{"id":"s2","name":"In Progress","type":"started","color":"#f2c94c"},"assignee":{"id":"u1","name":"Victor"},
            "team":{"id":"t1","key":"ENG"},"cycle":{"id":"c1","number":14},"labels":{"nodes":[{"name":"bug","color":"#eb5757"}]}}],
            "pageInfo":{"hasNextPage":true,"endCursor":"cursor-2"}}}}"##;
        let comentarios: Vec<String> = (1..=12)
            .map(|i| format!(r#"{{"id":"c{i}","body":"comentário {i}","createdAt":"2026-10-{i:02}T00:00:00.000Z","url":"u","user":{{"id":"u1","name":"Victor"}},"botActor":null}}"#))
            .collect();
        let detalhe = format!(
            r##"{{"data":{{"issue":{{"id":"i1","identifier":"ENG-162","title":"Sentry","description":"Ver ![](https://uploads.linear.app/x.png)","url":"u","branchName":"victor/eng-162-sentry","priority":0,"state":{{"id":"s2","name":"In Progress","type":"started","color":"#f2c94c"}},"assignee":null,"creator":{{"id":"u2","name":"Ana"}},"team":{{"id":"t1","key":"ENG","name":"Engenharia"}},"project":{{"id":"p1","name":"Vitral"}},"cycle":{{"id":"c1","number":14,"name":null}},"labels":{{"nodes":[]}},"comments":{{"nodes":[{}]}}}}}}}}"##,
            comentarios.iter().rev().cloned().collect::<Vec<_>>().join(",")
        );
        let mudou = r##"{"data":{"issueUpdate":{"success":true,"issue":{"id":"i1","identifier":"ENG-162","state":{"id":"s2","name":"In Progress","type":"started","color":"#f2c94c"}}}}}"##;
        let comentou = r#"{"data":{"commentCreate":{"success":true,"comment":{"id":"uuid-do-vitral","url":"u"}}}}"#;
        let sem_integracao = r#"{"data":null,"errors":[{"message":"No GitHub integration","extensions":{"code":"INVALID_INPUT"}}]}"#;
        let ligou = r#"{"data":{"attachmentLinkURL":{"success":true}}}"#;
        let e = Falso::com(vec![
            http(eu),
            http(times),
            http(do_time),
            http(lista),
            http(&detalhe),
            http(mudou),
            http(comentou),
            http(sem_integracao),
            http(ligou),
            saida(6, "", "curl: (6) Could not resolve host"),
        ]);
        let t = Curl(&e);
        let chave = chave();
        let linear = Linear { t: &t, chave: &chave };
        let eu = linear.eu().unwrap();
        assert_eq!((eu.nome.as_str(), eu.workspace.as_str(), eu.url_key.as_str()), ("Victor", "Eita", "eita"));
        assert_eq!(linear.times().unwrap()[0].chave, "ENG");
        let time = linear.do_time("t1").unwrap();
        let ordem: Vec<TipoEstado> = time.estados.iter().map(|e| e.estado.tipo).collect();
        assert_eq!(ordem, [TipoEstado::AFazer, TipoEstado::EmAndamento, TipoEstado::Concluida]);
        let (issues, proxima) = linear.listar_issues(json!({ "assignee": { "isMe": { "eq": true } } }), None).unwrap();
        assert_eq!(proxima.as_deref(), Some("cursor-2"));
        let issue = &issues[0];
        assert_eq!((issue.chave.as_str(), issue.estado.tipo, issue.prioridade), ("ENG-162", TipoEstado::EmAndamento, Some(2)));
        assert_eq!(issue.ramo_sugerido.as_deref(), Some("victor/eng-162-sentry"));
        assert_eq!(issue.responsavel.as_ref().unwrap().nome, "Victor");
        let pedido: Value = serde_json::from_str(&{
            let entrada = String::from_utf8(e.chamadas()[3].entrada.clone().unwrap()).unwrap();
            let linha = entrada.lines().find_map(|l| l.strip_prefix("data-binary = ")).unwrap().to_string();
            // Desfaz o escape do curl para ler o JSON que vai.
            linha[1..linha.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
        })
        .unwrap();
        assert_eq!(pedido["variables"]["filtro"]["assignee"]["isMe"]["eq"], true);
        assert!(pedido["variables"]["depois"].is_null());
        assert!(pedido["query"].as_str().unwrap().contains("branchName"));
        let detalhe = linear.ver_issue("ENG-162").unwrap();
        assert_eq!(detalhe.comentarios.len(), 10);
        assert_eq!(detalhe.comentarios[0].corpo, "comentário 3");
        assert_eq!(detalhe.comentarios[9].corpo, "comentário 12");
        assert_eq!(detalhe.ciclo.as_deref(), Some("Ciclo 14"));
        assert_eq!((detalhe.time.as_deref(), detalhe.projeto.as_deref()), (Some("ENG"), Some("Vitral")));
        assert_eq!(detalhe.resumo.autor.as_deref(), Some("Ana"));
        assert_eq!(linear.mudar_estado("i1", "s2").unwrap().tipo, TipoEstado::EmAndamento);
        assert_eq!(linear.comentar("i1", "Em andamento no Vitral", "uuid-do-vitral").unwrap(), "uuid-do-vitral");
        // Sem a integração GitHub do Linear: cai para o anexo de link.
        assert_eq!(linear.ligar_pr("i1", "https://github.com/o/r/pull/12", "PR #12"), Ok(()));
        let chamadas = e.chamadas();
        assert!(String::from_utf8_lossy(chamadas[8].entrada.as_deref().unwrap()).contains("attachmentLinkURL"));
        // Sem rede não cai para o outro.
        assert!(matches!(linear.ligar_pr("i1", "u", "t"), Err(ErroRemoto::SemRede(_))));
        assert_eq!(e.chamadas().len(), 10);
        // A chave nunca aparece fora do stdin.
        for chamada in e.chamadas() {
            assert!(!format!("{:?}", chamada.args).contains("lin_api_"));
        }
    }

    #[test]
    fn filtro_linear_de_cada_recorte() {
        let time = LinearDoProjeto { time_id: "t-eng".into(), time_chave: "ENG".into(), ..Default::default() };
        let filtro = |recorte, fechadas, projeto: Option<&LinearDoProjeto>| {
            filtro_linear(&FiltroIssues { recorte, fechadas, termo: None }, projeto)
        };
        let abertas = json!({ "type": { "nin": ["completed", "canceled", "duplicate"] } });
        let do_time = json!({ "id": { "eq": "t-eng" } });
        // Sem time, "Minhas" é do workspace inteiro.
        assert_eq!(filtro(Recorte::Minhas, false, None), json!({ "assignee": { "isMe": { "eq": true } }, "state": abertas }));
        assert_eq!(
            filtro(Recorte::Minhas, false, Some(&time)),
            json!({ "team": do_time, "assignee": { "isMe": { "eq": true } }, "state": abertas })
        );
        assert_eq!(filtro(Recorte::DoTime, false, Some(&time)), json!({ "team": do_time, "state": abertas }));
        assert_eq!(
            filtro(Recorte::CicloAtual, false, Some(&time)),
            json!({ "team": do_time, "cycle": { "isActive": { "eq": true } }, "state": abertas })
        );
        assert_eq!(
            filtro(Recorte::DoTime, true, Some(&time)),
            json!({ "team": do_time, "state": { "type": { "in": ["completed", "canceled"] } }, "updatedAt": { "gt": "-P30D" } })
        );
        // O texto exato que vai ao Linear (a ordem é a da montagem).
        assert_eq!(
            filtro(Recorte::Minhas, false, Some(&time)).to_string(),
            r#"{"team":{"id":{"eq":"t-eng"}},"assignee":{"isMe":{"eq":true}},"state":{"type":{"nin":["completed","canceled","duplicate"]}}}"#
        );
        // O time vazio (projeto sem time escolhido) não filtra.
        assert!(filtro(Recorte::DoTime, false, Some(&LinearDoProjeto::default())).get("team").is_none());
    }

    #[test]
    fn ao_iniciar_le_o_estado_o_responsavel_e_os_estados_do_time() {
        let corpo = r##"{"data":{"issue":{"id":"i1","identifier":"ENG-5","state":{"id":"s1","name":"A fazer","type":"unstarted","color":"#ccc"},"assignee":null,"team":{"id":"t1","key":"ENG","states":{"nodes":[{"id":"s3","name":"Feito","type":"completed","color":null,"position":3},{"id":"s2","name":"Fazendo","type":"started","color":null,"position":2},{"id":"s1","name":"A fazer","type":"unstarted","color":null,"position":1}]}}}}}"##;
        let e = Falso::com(vec![http(corpo), http(r#"{"data":{"issue":null}}"#)]);
        let chave = chave();
        let linear = Linear { t: &Curl(&e), chave: &chave };
        let agora = linear.ao_iniciar("ENG-5").unwrap();
        assert_eq!((agora.estado.tipo, agora.responsavel, agora.time_id.as_str()), (TipoEstado::AFazer, None, "t1"));
        let ordem: Vec<&str> = agora.estados.iter().map(|e| e.estado.id.as_str()).collect();
        assert_eq!(ordem, ["s1", "s2", "s3"]);
        assert!(matches!(linear.ao_iniciar("ENG-404"), Err(ErroRemoto::NaoEncontrado(_))));
    }
}
