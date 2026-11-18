//! O Linear do quadro de ideias, por GraphQL (`https://api.linear.app/graphql`). A chave é a
//! pessoal de quem usa (Linear > Settings > Security & access > Personal API keys), num arquivo só
//! dela na configuração do Vitral, e vai no cabeçalho `Authorization` sem o "Bearer" (é como a doc
//! do Linear pede para chave pessoal; o "Bearer" é só do OAuth).
//!
//! O quadro é o de um time (`TIME_PADRAO`, o EFI, Eficiência Operacional, escolha do dono em
//! 18/11/2026), mas só com as issues da etiqueta `ETIQUETA` ("Ideia"): o quadro guarda as ideias
//! que o dono anota para o futuro, e não as tarefas do time (dono, 18/11/2026: "essas ideias é pra
//! ser coisas do futuro que vou colocando pra não esquecer"). As colunas são os tipos de estado da
//! issue (`coluna_do_tipo`), a ordem dentro da coluna é o `sortOrder` do Linear, a descrição é o
//! pedido que vai para o chat.
//! Tudo aqui bloqueia: o motor chama em segundo plano.

use std::{
    io::Read as _,
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::{Value, json};

use crate::ideias::Coluna;

const ENDERECO: &str = "https://api.linear.app/graphql";
/// O time do quadro quando o `ideias.json` não diz outro.
pub const TIME_PADRAO: &str = "EFI";
/// A etiqueta que marca as issues do quadro (criada no time se ainda não existir).
pub const ETIQUETA: &str = "Ideia";
/// A cor da etiqueta quando o quadro a cria (o violeta das ideias).
const COR_DA_ETIQUETA: &str = "#8b5cf6";
/// Onde criar a chave pessoal.
pub const PAGINA_DA_CHAVE: &str = "https://linear.app/settings/account/security";
/// As concluídas que o quadro mostra: as dos últimos 30 dias.
const FEITAS_DESDE: &str = "-P30D";
/// Páginas de 100 issues: o quadro para em 400.
const PAGINAS: usize = 4;

/// Um estado do fluxo do time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Estado {
    pub id: String,
    pub nome: String,
    /// `triage`, `backlog`, `unstarted`, `started`, `completed` ou `canceled`.
    pub tipo: String,
    pub posicao: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Time {
    pub id: String,
    pub chave: String,
    pub nome: String,
    pub estados: Vec<Estado>,
}

impl Time {
    /// O estado em que um cartão solto numa coluna fica: o primeiro do fluxo com o tipo dela.
    pub fn estado_da_coluna(&self, coluna: Coluna) -> Option<&Estado> {
        let tipo = tipo_da_coluna(coluna);
        self.estados.iter().filter(|e| e.tipo == tipo).min_by(|a, b| a.posicao.total_cmp(&b.posicao))
    }
}

/// Uma issue como o quadro a usa.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Issue {
    pub id: String,
    /// "EFI-123".
    pub identificador: String,
    pub titulo: String,
    pub descricao: String,
    pub url: String,
    pub ordem: f64,
    pub estado: String,
    pub coluna: Coluna,
    pub responsavel: Option<String>,
}

/// Em que coluna fica uma issue pelo tipo do estado dela; cancelada fica de fora.
pub fn coluna_do_tipo(tipo: &str) -> Option<Coluna> {
    match tipo {
        "triage" | "backlog" => Some(Coluna::Ideias),
        "unstarted" => Some(Coluna::Proximas),
        "started" => Some(Coluna::Fazendo),
        "completed" => Some(Coluna::Feitas),
        _ => None,
    }
}

fn tipo_da_coluna(coluna: Coluna) -> &'static str {
    match coluna {
        Coluna::Ideias => "backlog",
        Coluna::Proximas => "unstarted",
        Coluna::Fazendo => "started",
        Coluna::Feitas => "completed",
    }
}

// ---------- a chave ----------

pub fn arquivo_da_chave(config: &Path) -> PathBuf {
    config.join("linear.chave")
}

pub fn ler_chave(config: &Path) -> Option<String> {
    let chave = std::fs::read_to_string(arquivo_da_chave(config)).ok()?;
    Some(chave.trim().to_string()).filter(|c| !c.is_empty())
}

/// Só o dono lê (0600 no Unix); vazia, apaga o arquivo.
pub fn gravar_chave(config: &Path, chave: &str) -> std::io::Result<()> {
    let arquivo = arquivo_da_chave(config);
    if chave.trim().is_empty() {
        return match std::fs::remove_file(&arquivo) {
            Err(erro) if erro.kind() != std::io::ErrorKind::NotFound => Err(erro),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(config)?;
    crate::memoria_equipe::gravar_so_do_dono(&arquivo, chave.trim().as_bytes())
}

// ---------- os pedidos ----------

/// Um pedido GraphQL: o `data` da resposta, ou o erro em português.
fn pedir(chave: &str, consulta: &str, variaveis: Value) -> Result<Value, String> {
    if chave.trim().is_empty() {
        return Err("Falta a chave do Linear.".into());
    }
    let agente = ureq::Agent::config_builder()
        .user_agent(&format!("Vitral/{}", crate::atualizacao::VERSAO))
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        // Um redirecionamento não leva a chave a lugar nenhum: vira falha.
        .max_redirects(0)
        .build()
        .new_agent();
    let corpo = json!({ "query": consulta, "variables": variaveis });
    let mut resposta = agente
        .post(ENDERECO)
        .header("Authorization", chave.trim())
        .header("Content-Type", "application/json")
        .send(corpo.to_string().as_bytes())
        .map_err(|erro| format!("Não cheguei ao Linear: {erro}"))?;
    let codigo = resposta.status().as_u16();
    let mut texto = String::new();
    let _ = resposta.body_mut().as_reader().take(8 * 1024 * 1024).read_to_string(&mut texto);
    let valor: Value = serde_json::from_str(&texto).unwrap_or_default();
    resultado(codigo, valor)
}

fn resultado(codigo: u16, valor: Value) -> Result<Value, String> {
    if let Some(erro) = valor["errors"].as_array().and_then(|e| e.first()) {
        let texto = erro["message"].as_str().unwrap_or("erro sem mensagem");
        let tipo = erro["extensions"]["code"].as_str().unwrap_or_default();
        return Err(match tipo {
            "AUTHENTICATION_ERROR" => "O Linear recusou a chave.".into(),
            "RATELIMITED" => "O Linear pediu para esperar (limite de pedidos).".into(),
            _ => format!("O Linear respondeu: {texto}"),
        });
    }
    match codigo {
        200 if valor["data"].is_object() => Ok(valor["data"].clone()),
        200 => Err("O Linear respondeu algo que não entendi.".into()),
        400 | 401 => Err("O Linear recusou a chave.".into()),
        429 => Err("O Linear pediu para esperar (limite de pedidos).".into()),
        outro => Err(format!("O Linear respondeu com o código {outro}.")),
    }
}

const CAMPOS_DA_ISSUE: &str = "id identifier title description url sortOrder state { name type } assignee { displayName }";

fn issue_de(v: &Value) -> Option<Issue> {
    let coluna = coluna_do_tipo(v["state"]["type"].as_str()?)?;
    Some(Issue {
        id: v["id"].as_str()?.to_string(),
        identificador: v["identifier"].as_str().unwrap_or_default().to_string(),
        titulo: v["title"].as_str().unwrap_or_default().to_string(),
        descricao: v["description"].as_str().unwrap_or_default().to_string(),
        url: v["url"].as_str().unwrap_or_default().to_string(),
        ordem: v["sortOrder"].as_f64().unwrap_or_default(),
        estado: v["state"]["name"].as_str().unwrap_or_default().to_string(),
        coluna,
        responsavel: v["assignee"]["displayName"].as_str().map(str::to_string),
    })
}

/// O time pela chave curta ("EFI"), com os estados.
pub fn time(chave: &str, codigo: &str) -> Result<Time, String> {
    let dados = pedir(
        chave,
        "query Time($chave: String!) { teams(filter: { key: { eq: $chave } }) { nodes { id key name states { nodes { id name type position } } } } }",
        json!({ "chave": codigo }),
    )?;
    let time = dados["teams"]["nodes"].as_array().and_then(|t| t.first()).ok_or_else(|| format!("A chave não vê o time {codigo} no Linear."))?;
    let estados = time["states"]["nodes"]
        .as_array()
        .map(|lista| {
            lista
                .iter()
                .filter_map(|e| {
                    Some(Estado {
                        id: e["id"].as_str()?.to_string(),
                        nome: e["name"].as_str().unwrap_or_default().to_string(),
                        tipo: e["type"].as_str().unwrap_or_default().to_string(),
                        posicao: e["position"].as_f64().unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Time {
        id: time["id"].as_str().unwrap_or_default().to_string(),
        chave: time["key"].as_str().unwrap_or(codigo).to_string(),
        nome: time["name"].as_str().unwrap_or(codigo).to_string(),
        estados,
    })
}

/// As issues do quadro: as da etiqueta, abertas e concluídas nos últimos 30 dias (sem as canceladas).
pub fn issues(chave: &str, time: &str) -> Result<Vec<Issue>, String> {
    let consulta = format!(
        "query Quadro($time: ID!, $depois: String) {{ issues(first: 100, after: $depois, filter: {{ team: {{ id: {{ eq: $time }} }}, \
         labels: {{ some: {{ name: {{ eqIgnoreCase: \"{ETIQUETA}\" }} }} }}, \
         or: [{{ state: {{ type: {{ in: [\"triage\", \"backlog\", \"unstarted\", \"started\"] }} }} }}, \
         {{ completedAt: {{ gt: \"{FEITAS_DESDE}\" }} }}] }}) {{ nodes {{ {CAMPOS_DA_ISSUE} }} pageInfo {{ hasNextPage endCursor }} }} }}"
    );
    let mut todas = Vec::new();
    let mut depois: Option<String> = None;
    for _ in 0..PAGINAS {
        let dados = pedir(chave, &consulta, json!({ "time": time, "depois": depois }))?;
        let pagina = &dados["issues"];
        todas.extend(pagina["nodes"].as_array().into_iter().flatten().filter_map(issue_de));
        match (pagina["pageInfo"]["hasNextPage"].as_bool(), pagina["pageInfo"]["endCursor"].as_str()) {
            (Some(true), Some(cursor)) => depois = Some(cursor.to_string()),
            _ => break,
        }
    }
    Ok(todas)
}

/// O id da etiqueta do quadro no time: a do time ou a do workspace; sem nenhuma, cria a do time.
fn etiqueta_do_quadro(chave: &str, time: &str) -> Result<String, String> {
    let dados = pedir(
        chave,
        "query Etiqueta($nome: String!) { issueLabels(filter: { name: { eqIgnoreCase: $nome } }) { nodes { id team { id } } } }",
        json!({ "nome": ETIQUETA }),
    )?;
    let achadas = dados["issueLabels"]["nodes"].as_array().cloned().unwrap_or_default();
    let do_time = achadas.iter().find(|e| e["team"]["id"].as_str() == Some(time));
    let do_workspace = achadas.iter().find(|e| e["team"].is_null());
    if let Some(id) = do_time.or(do_workspace).and_then(|e| e["id"].as_str()) {
        return Ok(id.to_string());
    }
    let dados = pedir(
        chave,
        "mutation NovaEtiqueta($entrada: IssueLabelCreateInput!) { issueLabelCreate(input: $entrada) { success issueLabel { id } } }",
        json!({ "entrada": { "name": ETIQUETA, "teamId": time, "color": COR_DA_ETIQUETA } }),
    )?;
    dados["issueLabelCreate"]["issueLabel"]["id"].as_str().map(str::to_string).ok_or_else(|| "O Linear não criou a etiqueta Ideia.".into())
}

/// Cria a issue no estado dado, no alto da coluna (`ordem`), com a etiqueta do quadro.
pub fn criar(chave: &str, time: &str, estado: &str, titulo: &str, descricao: &str, ordem: f64) -> Result<Issue, String> {
    let etiqueta = etiqueta_do_quadro(chave, time)?;
    let consulta = format!(
        "mutation Criar($entrada: IssueCreateInput!) {{ issueCreate(input: $entrada) {{ success issue {{ {CAMPOS_DA_ISSUE} }} }} }}"
    );
    let entrada = json!({
        "teamId": time, "stateId": estado, "title": titulo, "description": descricao, "sortOrder": ordem, "labelIds": [etiqueta],
    });
    let dados = pedir(chave, &consulta, json!({ "entrada": entrada }))?;
    issue_de(&dados["issueCreate"]["issue"]).ok_or_else(|| "O Linear não devolveu a issue criada.".into())
}

/// Muda os campos dados (`title`, `description`, `stateId`, `sortOrder`).
pub fn atualizar(chave: &str, id: &str, campos: Value) -> Result<(), String> {
    let dados = pedir(
        chave,
        "mutation Mudar($id: String!, $entrada: IssueUpdateInput!) { issueUpdate(id: $id, input: $entrada) { success } }",
        json!({ "id": id, "entrada": campos }),
    )?;
    dados["issueUpdate"]["success"].as_bool().filter(|s| *s).map(|_| ()).ok_or_else(|| "O Linear não aceitou a mudança.".into())
}

/// Arquiva (o "Apagar" do quadro; o Linear guarda e dá para desarquivar).
pub fn arquivar(chave: &str, id: &str, arquivar: bool) -> Result<(), String> {
    let (mutacao, campo) = if arquivar { ("issueArchive", "issueArchive") } else { ("issueUnarchive", "issueUnarchive") };
    let dados = pedir(chave, &format!("mutation Arquivar($id: String!) {{ {mutacao}(id: $id) {{ success }} }}"), json!({ "id": id }))?;
    dados[campo]["success"].as_bool().filter(|s| *s).map(|_| ()).ok_or_else(|| "O Linear não arquivou a issue.".into())
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn colunas_pelos_tipos_de_estado() {
        assert_eq!(coluna_do_tipo("triage"), Some(Coluna::Ideias));
        assert_eq!(coluna_do_tipo("backlog"), Some(Coluna::Ideias));
        assert_eq!(coluna_do_tipo("unstarted"), Some(Coluna::Proximas));
        assert_eq!(coluna_do_tipo("started"), Some(Coluna::Fazendo));
        assert_eq!(coluna_do_tipo("completed"), Some(Coluna::Feitas));
        assert_eq!(coluna_do_tipo("canceled"), None);
        let estado = |id: &str, tipo: &str, posicao: f64| Estado { id: id.into(), nome: id.into(), tipo: tipo.into(), posicao };
        let time = Time {
            estados: vec![estado("revisao", "started", 3.), estado("andamento", "started", 2.), estado("backlog", "backlog", 0.)],
            ..Default::default()
        };
        assert_eq!(time.estado_da_coluna(Coluna::Fazendo).map(|e| e.id.as_str()), Some("andamento"));
        assert_eq!(time.estado_da_coluna(Coluna::Ideias).map(|e| e.id.as_str()), Some("backlog"));
        assert_eq!(time.estado_da_coluna(Coluna::Feitas), None);
    }

    #[test]
    fn issue_lida_da_resposta() {
        let v = json!({
            "id": "u1", "identifier": "EFI-7", "title": "T", "description": null, "url": "https://linear.app/x",
            "sortOrder": -12.5, "state": { "name": "In Progress", "type": "started" }, "assignee": { "displayName": "victor" }
        });
        let i = issue_de(&v).unwrap();
        assert_eq!((i.identificador.as_str(), i.coluna, i.ordem, i.descricao.as_str()), ("EFI-7", Coluna::Fazendo, -12.5, ""));
        assert_eq!(i.responsavel.as_deref(), Some("victor"));
        assert!(issue_de(&json!({ "id": "u2", "state": { "type": "canceled" } })).is_none());
    }

    #[test]
    fn erros_do_linear_em_portugues() {
        let recusada = json!({ "errors": [{ "message": "Authentication required", "extensions": { "code": "AUTHENTICATION_ERROR" } }] });
        assert_eq!(resultado(400, recusada), Err("O Linear recusou a chave.".into()));
        assert_eq!(resultado(200, json!({ "data": { "a": 1 } })), Ok(json!({ "a": 1 })));
        assert_eq!(resultado(500, json!(null)), Err("O Linear respondeu com o código 500.".into()));
    }
}
