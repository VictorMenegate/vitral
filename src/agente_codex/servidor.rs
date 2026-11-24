//! JSON-RPC do `codex app-server`, conferido com o schema gerado pela CLI instalada e com turnos
//! de verdade no 0.169.0 (24/11/2026). O canal fica aberto durante o turno: `turn/steer` recebe
//! texto e imagens sem interromper o trabalho. A confirmação volta ao motor; uma recusa devolve só
//! aquela mensagem à fila.
//!
//! Os pedidos de aprovação (`item/commandExecution/requestApproval` e
//! `item/fileChange/requestApproval`) viram o mesmo cartão dos pedidos do Claude Code
//! (`Evento::Permissao`); a resposta volta ao Codex por uma thread, como `accept`, `decline` ou,
//! sem resposta nenhuma, `cancel`. Medido no 0.169.0: o id do pedido é um número; o
//! `availableDecisions` do comando não lista o `decline`, que mesmo assim vale (o item volta
//! `declined` e o turno segue); o pedido de edição só traz o `itemId`, e o diff vem antes, no
//! `item/started` do `fileChange`; e o `serverRequest/resolved` chega logo depois de cada
//! resposta (ou quando o turno para antes dela), e aí o cartão sai.
//!
//! Os comandos de barra são os que o app-server oferece: `/compact` é o `thread/compact/start`
//! (responde `{}` e abre um turno com o item `contextCompaction`, medido no 0.169.0), `/review` é o
//! `review/start` (o resultado vem no item `exitedReviewMode`) e cada skill da pasta
//! (`skills/list`) é um "/nome", que vai como a entrada `skill` com o texto "$nome …".
//!
//! O uso vem no `thread/tokenUsage/updated`, a cada chamada ao modelo (medido no 0.169.0): o
//! `last.totalTokens` é o que a conversa ocupa agora (o contexto, como o Codex mostra), o
//! `modelContextWindow` é a janela, e o `total` soma a conversa inteira, então o gasto do turno é o
//! `total` do fim menos o de antes da primeira chamada dele. O Codex não diz custo.

use std::{collections::HashMap, io::{self, Write}, path::{Path, PathBuf}, sync::{Arc, Mutex}};
use serde_json::{Value, json};
use crate::{claude::Evento, mcp::Resposta};

pub fn entrada(texto: &str, imagens: &[PathBuf]) -> Vec<Value> {
    std::iter::once(json!({"type":"text", "text":texto}))
        .chain(imagens.iter().map(|p| json!({"type":"localImage", "path":p}))).collect()
}

pub struct Canal {
    saida: Option<Box<dyn Write + Send>>,
    thread: String,
    turno: String,
    concluido: bool,
    proximo: u64,
    pendentes: HashMap<String, String>,
    aguardando_inicio: Vec<(String, Vec<Value>)>,
    /// Os pedidos de aprovação esperando o cartão, pelo id do JSON-RPC (em texto): o canal de cada
    /// um, que o `serverRequest/resolved` fecha. Quem responde tira daqui antes de escrever.
    aprovacoes: HashMap<String, smol::channel::Receiver<Resposta>>,
}

impl Canal {
    pub fn novo(saida: Box<dyn Write + Send>) -> Self {
        Self { saida: Some(saida), thread: String::new(), turno: String::new(), concluido: false,
            proximo: 0, pendentes: HashMap::new(), aguardando_inicio: Vec::new(), aprovacoes: HashMap::new() }
    }
    fn escrever(&mut self, valor: Value) -> io::Result<()> {
        let saida = self.saida.as_mut().ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "Codex fechado"))?;
        writeln!(saida, "{valor}")?;
        saida.flush()
    }
    pub fn aberta(&self) -> bool { self.saida.is_some() && !self.concluido }
    pub fn fechar(&mut self) { self.saida = None; self.concluido = true; }
    pub fn dirigir(&mut self, texto: &str, imagens: &[PathBuf]) -> io::Result<()> {
        if !self.aberta() { return Err(io::Error::new(io::ErrorKind::BrokenPipe, "Turno concluído")); }
        self.proximo += 1;
        let id = format!("vitral-steer-{}", self.proximo);
        let input = entrada(texto, imagens);
        if self.turno.is_empty() {
            self.aguardando_inicio.push((id.clone(), input));
        } else {
            self.steer(&id, input)?;
        }
        self.pendentes.insert(id, texto.to_string());
        Ok(())
    }
    fn steer(&mut self, id: &str, input: Vec<Value>) -> io::Result<()> {
        self.escrever(json!({"id":id, "method":"turn/steer", "params":{
            "threadId":self.thread, "expectedTurnId":self.turno, "input":input,
        }}))
    }
    fn ativar(&mut self, turno: &str) -> io::Result<()> {
        self.turno = turno.to_string();
        for (id, input) in std::mem::take(&mut self.aguardando_inicio) { self.steer(&id, input)?; }
        Ok(())
    }
    pub fn interromper(&mut self) -> io::Result<()> {
        self.concluido = true;
        if self.turno.is_empty() { self.fechar(); return Ok(()); }
        self.escrever(json!({"id":"vitral-interrupt", "method":"turn/interrupt", "params":{
            "threadId":self.thread, "turnId":self.turno,
        }}))
    }
}

/// O id de um pedido do JSON-RPC em texto (o Codex manda números; o `requestId` do
/// `serverRequest/resolved` vem no mesmo tipo).
fn chave_do_id(id: &Value) -> String {
    id.as_str().map_or_else(|| id.to_string(), str::to_string)
}

/// O que o turno pede ao app-server, pelo texto da mensagem.
#[derive(Clone, Debug, PartialEq)]
enum Pedido {
    /// Uma mensagem (`turn/start`).
    Turno,
    /// O `/compact` (`thread/compact/start`; o texto depois dele não vai: o Codex não aceita foco).
    Compactar,
    /// O `/review` (`review/start`), com o alvo: as mudanças sem commit, ou o que o texto pede.
    Revisar(Value),
}

/// Os comandos de barra que o app-server do Codex atende por conta própria (as skills somam).
pub const COMANDOS: [&str; 2] = ["compact", "review"];

fn pedido_de(input: &[Value]) -> Pedido {
    let texto = input.first().and_then(|e| e["text"].as_str()).unwrap_or_default().trim();
    let (comando, resto) = texto.split_once(char::is_whitespace).map_or((texto, ""), |(c, r)| (c, r.trim()));
    match (comando, resto) {
        ("/compact", _) => Pedido::Compactar,
        ("/review", "") => Pedido::Revisar(json!({"type":"uncommittedChanges"})),
        ("/review", instrucoes) => Pedido::Revisar(json!({"type":"custom","instructions":instrucoes})),
        _ => Pedido::Turno,
    }
}

pub struct Protocolo {
    canal: Arc<Mutex<Canal>>,
    parametros: Value,
    input: Vec<Value>,
    esforco: Option<String>,
    leitor: super::Leitor,
    modelo: String,
    fim: Option<Evento>,
    /// As mudanças de cada item `fileChange` aberto, pelo id do item: o pedido de aprovação da
    /// edição só traz o id, e o cartão mostra o diff que veio no começo do item.
    mudancas: HashMap<String, Vec<Value>>,
    pedido: Pedido,
    /// As skills da pasta (nome e arquivo), quando o `skills/list` respondeu (vazia se falhou).
    skills: Option<Vec<(String, String)>>,
    /// A conversa abriu e o turno espera a lista das skills (a mensagem começa com "/").
    esperando_skills: bool,
    /// O `total` de tokens da conversa antes da primeira chamada deste turno e o da última
    /// ([entrada, cache lido, cache gravado, saída]).
    tokens_antes: Option<[u64; 4]>,
    tokens_agora: Option<[u64; 4]>,
    /// O tamanho da janela de contexto do modelo (`modelContextWindow`).
    janela: Option<u64>,
}

/// [entrada, cache lido, cache gravado, saída] de um `TokenUsageBreakdown`.
fn partes_do_uso(uso: &Value) -> [u64; 4] {
    ["inputTokens", "cachedInputTokens", "cacheWriteInputTokens", "outputTokens"].map(|chave| uso[chave].as_u64().unwrap_or(0))
}

impl Protocolo {
    pub fn novo(canal: Arc<Mutex<Canal>>, parametros: Value, input: Vec<Value>, esforco: Option<String>) -> Self {
        let pedido = pedido_de(&input);
        Self { canal, parametros, input, esforco, leitor: super::Leitor::default(), modelo: String::new(), fim: None,
            mudancas: HashMap::new(), pedido, skills: None, esperando_skills: false, tokens_antes: None, tokens_agora: None,
            janela: None }
    }

    /// Os tokens que o turno somou à conversa (`None` sem uso medido nele).
    fn gasto_do_turno(&self) -> Option<crate::motor::Tokens> {
        let (antes, agora) = (self.tokens_antes?, self.tokens_agora?);
        let [entrada, cache_lido, cache_criado, saida]: [u64; 4] = std::array::from_fn(|i| agora[i].saturating_sub(antes[i]));
        let tokens = crate::motor::Tokens { entrada: entrada.saturating_sub(cache_lido), cache_lido, cache_criado, saida };
        (tokens.total() > 0).then_some(tokens)
    }

    /// Os comandos de barra desta pasta, para o menu "/": os do app-server e as skills.
    fn comandos(&self) -> Vec<String> {
        COMANDOS.iter().map(|c| c.to_string()).chain(self.skills.iter().flatten().map(|(nome, _)| nome.clone()))
            .fold(Vec::new(), |mut lista, c| { if !lista.contains(&c) { lista.push(c); } lista })
    }

    /// Começa o pedido do turno na conversa aberta: a mensagem (com a skill, se a barra for uma), o
    /// `/compact` ou o `/review`. A mensagem com barra espera a lista das skills.
    fn comecar(&mut self, canal: &mut Canal) -> io::Result<()> {
        let thread = canal.thread.clone();
        match &self.pedido {
            Pedido::Compactar => canal.escrever(json!({"id":"vitral-turn", "method":"thread/compact/start", "params":{"threadId":thread}})),
            Pedido::Revisar(alvo) => canal.escrever(json!({"id":"vitral-turn", "method":"review/start", "params":{"threadId":thread, "target":alvo}})),
            Pedido::Turno => {
                let texto = self.input.first().and_then(|e| e["text"].as_str()).unwrap_or_default().trim_start().to_string();
                if texto.starts_with('/') && self.skills.is_none() {
                    self.esperando_skills = true;
                    return Ok(());
                }
                self.esperando_skills = false;
                let mut input = self.input.clone();
                let nome = texto.strip_prefix('/').and_then(|r| r.split_whitespace().next()).unwrap_or_default();
                if let Some((nome, arquivo)) = self.skills.iter().flatten().find(|(n, _)| n == nome) {
                    input[0]["text"] = json!(format!("${}", &texto[1..]));
                    input.push(json!({"type":"skill", "name":nome, "path":arquivo}));
                }
                let mut params = json!({"threadId":thread, "input":input, "effort":self.esforco});
                // A política vai de novo no turno: a da conversa retomada pode ser outra.
                if let Some(politica) = self.parametros.get("approvalPolicy") {
                    params["approvalPolicy"] = politica.clone();
                }
                canal.escrever(json!({"id":"vitral-turn", "method":"turn/start", "params":params}))
            }
        }
    }
    pub fn iniciar(&mut self) -> io::Result<()> {
        // A API experimental é a que aceita o `beforeTurnId` do `thread/fork` (o corte exato da
        // conversa ao corrigir ou bifurcar); o Paseo abre do mesmo jeito.
        self.canal.lock().unwrap_or_else(|e| e.into_inner()).escrever(json!({"id":"vitral-init", "method":"initialize", "params":{
            "clientInfo":{"name":"vitral", "version":env!("CARGO_PKG_VERSION")},
            "capabilities":{"experimentalApi":true},
        }}))
    }
    pub fn linha(&mut self, v: &Value) -> Vec<Evento> {
        let mut eventos = Vec::new();
        let resultado = self.tratar(v, &mut eventos);
        if let Err(erro) = resultado {
            self.canal.lock().unwrap_or_else(|e| e.into_inner()).fechar();
            eventos.push(Evento::Fim { erro: true, texto: format!("Não consegui conversar com o Codex: {erro}"), custo_usd: 0., janela: None });
        }
        eventos
    }
    fn tratar(&mut self, v: &Value, eventos: &mut Vec<Evento>) -> io::Result<()> {
        let compartilhado = self.canal.clone();
        let mut canal = compartilhado.lock().unwrap_or_else(|e| e.into_inner());
        if let (Some(id), Some(metodo)) = (v.get("id"), v["method"].as_str()) {
            return match metodo {
                "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                    eventos.push(self.pedido_de_aprovacao(&mut canal, id.clone(), metodo, &v["params"]));
                    Ok(())
                }
                _ => canal.escrever(json!({"id":id,"error":{"code":-32601,"message":"Este pedido ainda não é suportado pelo Vitral."}})),
            };
        }
        if v["id"] == "vitral-skills" {
            // A lista que falha só deixa o menu sem as skills: o turno segue.
            let skills = v["result"]["data"].as_array().into_iter().flatten()
                .flat_map(|pasta| pasta["skills"].as_array().cloned().unwrap_or_default())
                .filter(|s| s["enabled"] != false)
                .filter_map(|s| Some((s["name"].as_str()?.to_string(), s["path"].as_str()?.to_string())))
                .collect();
            self.skills = Some(skills);
            if self.esperando_skills {
                self.comecar(&mut canal)?;
            }
        } else if let Some(id) = v["id"].as_str() {
            if let Some(texto) = canal.pendentes.remove(id) {
                eventos.push(Evento::DirecaoConfirmada { texto, aceita: v.get("error").is_none() });
            } else if v.get("error").is_some() && id != "vitral-interrupt" {
                return Err(io::Error::other(v["error"]["message"].as_str().unwrap_or("Pedido recusado")));
            } else {
                match id {
                    "vitral-init" => {
                        canal.escrever(json!({"method":"initialized","params":{}}))?;
                        let metodo = if self.parametros.get("threadId").is_some() { "thread/resume" } else { "thread/start" };
                        canal.escrever(json!({"id":"vitral-thread", "method":metodo, "params":self.parametros}))?;
                        // As skills da pasta: o menu "/" e a mensagem que chama uma.
                        let pasta = self.parametros["cwd"].clone();
                        canal.escrever(json!({"id":"vitral-skills", "method":"skills/list", "params":{"cwds":[pasta]}}))?;
                    }
                    "vitral-thread" => {
                        let thread = v["result"]["thread"]["id"].as_str().ok_or_else(|| io::Error::other("O Codex não devolveu a conversa"))?;
                        canal.thread = thread.to_string();
                        self.modelo = v["result"]["model"].as_str().unwrap_or_default().to_string();
                        eventos.push(Evento::Sessao(thread.to_string()));
                        self.comecar(&mut canal)?;
                    }
                    "vitral-turn" if canal.turno.is_empty() => {
                        if let Some(id) = v["result"]["turn"]["id"].as_str() { canal.ativar(id)?; }
                    }
                    _ => {}
                }
            }
        } else {
            let p = &v["params"];
            match v["method"].as_str().unwrap_or_default() {
                "turn/started" => {
                    if let Some(id) = p["turn"]["id"].as_str() { canal.ativar(id)?; }
                    eventos.push(Evento::Inicio { modelo: self.modelo.clone(), comandos: self.comandos(), versao: None, fluxos: None });
                }
                "item/agentMessage/delta" => eventos.push(Evento::TextoParcial(p["delta"].as_str().unwrap_or_default().to_string())),
                "item/started" | "item/completed" => {
                    let terminou = v["method"] == "item/completed";
                    let item = &p["item"];
                    if item["type"] == "fileChange" && let Some(id) = item["id"].as_str() {
                        if terminou {
                            self.mudancas.remove(id);
                        } else {
                            self.mudancas.insert(id.to_string(), item["changes"].as_array().cloned().unwrap_or_default());
                        }
                    }
                    match item["type"].as_str().unwrap_or_default() {
                        // O resumo da conversa (o `/compact`, ou sozinho com o contexto cheio): a
                        // linha do turno diz "Compactando…" e o marco entra no fim.
                        "contextCompaction" if terminou => {
                            eventos.push(Evento::Compactou { manual: self.pedido == Pedido::Compactar, antes: None });
                        }
                        "contextCompaction" => eventos.push(Evento::Compactando(true)),
                        // O fim do `/review`: o texto da revisão é a resposta do turno.
                        "exitedReviewMode" if terminou => {
                            let revisao = item["review"].as_str().unwrap_or_default().to_string();
                            if !revisao.trim().is_empty() {
                                self.leitor.ultima_fala = revisao.clone();
                                eventos.push(Evento::Texto(revisao));
                            }
                        }
                        _ => {
                            let normal = normalizar(item);
                            eventos.extend(self.leitor.linha(&json!({"type":if terminou {"item.completed"} else {"item.started"}, "item":normal})));
                        }
                    }
                }
                // O patch chega por partes num item já aberto: o cartão mostra o último.
                "item/fileChange/patchUpdated" => {
                    if let (Some(id), Some(mudancas)) = (p["itemId"].as_str(), p["changes"].as_array()) {
                        self.mudancas.insert(id.to_string(), mudancas.clone());
                    }
                }
                // O Codex fechou o pedido sem esperar o cartão (o turno parou, ou já foi respondido):
                // o canal do cartão fecha, e o motor tira o cartão.
                "serverRequest/resolved" => {
                    if let Some(recebe) = canal.aprovacoes.remove(&chave_do_id(&p["requestId"])) {
                        recebe.close();
                        eventos.push(Evento::PermissaoRetirada);
                    }
                }
                "thread/tokenUsage/updated" => {
                    let uso = &p["tokenUsage"];
                    self.janela = uso["modelContextWindow"].as_u64().or(self.janela);
                    if let Some(tokens) = uso["last"]["totalTokens"].as_u64().filter(|n| *n > 0) {
                        eventos.push(Evento::Contexto(tokens));
                    }
                    // Só o uso deste turno conta para o gasto dele (a retomada pode repetir o de antes).
                    if !canal.turno.is_empty() && p["turnId"].as_str() == Some(canal.turno.as_str()) {
                        let (total, ultima) = (partes_do_uso(&uso["total"]), partes_do_uso(&uso["last"]));
                        self.tokens_antes.get_or_insert(std::array::from_fn(|i| total[i].saturating_sub(ultima[i])));
                        self.tokens_agora = Some(total);
                    }
                }
                "turn/completed" => {
                    canal.concluido = true;
                    let turno = &p["turn"];
                    let erro = turno["status"] != "completed";
                    let texto = if erro { turno["error"]["message"].as_str().unwrap_or("O turno foi interrompido.").to_string() }
                        else { std::mem::take(&mut self.leitor.ultima_fala) };
                    // Os tokens do turno vão antes do fim, como os do `result` do Claude Code.
                    if let Some(tokens) = self.gasto_do_turno() {
                        eventos.push(Evento::Gasto(tokens));
                    }
                    self.fim = Some(Evento::Fim { erro, texto, custo_usd: 0., janela: self.janela });
                }
                _ => {}
            }
        }
        if canal.pendentes.is_empty() && let Some(fim) = self.fim.take() {
            canal.fechar();
            eventos.push(fim);
        }
        Ok(())
    }

    /// O pedido de aprovação vira o cartão do Vitral, com um canal de resposta. Uma thread espera a
    /// resposta e a escreve ao Codex: Permitir é `accept`, Negar é `decline` (o turno segue, como
    /// no Claude Code), e o cartão que sumiu sem resposta é `cancel`. O pedido que o Codex já
    /// resolveu (`serverRequest/resolved`) não recebe nada.
    fn pedido_de_aprovacao(&self, canal: &mut Canal, id: Value, metodo: &str, p: &Value) -> Evento {
        let (ferramenta, entrada) = if metodo == "item/commandExecution/requestApproval" {
            let mut entrada = json!({"command": super::comando_limpo(p["command"].as_str().unwrap_or_default())});
            if let Some(motivo) = p["reason"].as_str().filter(|m| !m.trim().is_empty()) {
                entrada["description"] = json!(motivo);
            }
            ("Bash".to_string(), entrada)
        } else {
            let pasta = Path::new(self.parametros["cwd"].as_str().unwrap_or_default());
            let mudancas = p["itemId"].as_str().and_then(|item| self.mudancas.get(item)).cloned().unwrap_or_default();
            let (ferramenta, mut entrada) = pedido_da_edicao(&mudancas, pasta);
            if entrada["file_path"].as_str().unwrap_or_default().is_empty()
                && let Some(raiz) = p["grantRoot"].as_str()
            {
                entrada["file_path"] = json!(raiz);
            }
            (ferramenta.to_string(), entrada)
        };
        let (envia, recebe) = smol::channel::bounded::<Resposta>(1);
        let chave = chave_do_id(&id);
        canal.aprovacoes.insert(chave.clone(), recebe.clone());
        let compartilhado = self.canal.clone();
        std::thread::spawn(move || {
            let decisao = match recebe.recv_blocking() {
                Ok(resposta) if serde_json::from_str::<Value>(&resposta.texto).is_ok_and(|r| r["behavior"] == "allow") => "accept",
                Ok(_) => "decline",
                Err(_) => "cancel",
            };
            let mut canal = compartilhado.lock().unwrap_or_else(|e| e.into_inner());
            // Resolvido pelo Codex enquanto esperava: não há mais a quem responder.
            if canal.aprovacoes.remove(&chave).is_some() {
                let _ = canal.escrever(json!({"id": id, "result": {"decision": decisao}}));
            }
        });
        Evento::Permissao { ferramenta, entrada, resposta: envia, chamada: p["itemId"].as_str().map(str::to_string) }
    }
}

/// A chamada que o cartão mostra para um pedido de edição: com um arquivo, a mesma da conversa
/// (`entrada_da_mudanca`: o Edit com o diff, o Write com o conteúdo); com vários, um Edit com os
/// diffs um depois do outro e o caminho do primeiro de fora da pasta, se houver (o cartão avisa
/// "fora da pasta" e a regra de "permitir sempre" não cobre), ou do primeiro.
fn pedido_da_edicao(mudancas: &[Value], pasta: &Path) -> (&'static str, Value) {
    if let [mudanca] = mudancas {
        return super::entrada_da_mudanca(mudanca);
    }
    let caminho = |m: &Value| m["path"].as_str().unwrap_or_default().to_string();
    let fora = mudancas.iter().map(caminho).find(|c| !crate::modelo::dentro_da_pasta(Path::new(c), pasta));
    let diff = mudancas
        .iter()
        .map(|m| match super::entrada_da_mudanca(m) {
            (_, entrada) if entrada["diff"].is_string() => entrada["diff"].as_str().unwrap_or_default().to_string(),
            (_, entrada) => {
                let linhas: Vec<&str> = entrada["content"].as_str().unwrap_or_default().lines().collect();
                std::iter::once(format!("@@ -0,0 +1,{} @@", linhas.len())).chain(linhas.iter().map(|l| format!("+{l}"))).collect::<Vec<_>>().join("\n")
            }
        })
        .map(|d| d.trim_end_matches('\n').to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let arquivo = fora.or_else(|| mudancas.first().map(caminho)).unwrap_or_default();
    ("Edit", json!({"file_path": arquivo, "diff": diff}))
}

fn normalizar(item: &Value) -> Value {
    let mut v = item.clone();
    v["type"] = json!(match item["type"].as_str().unwrap_or_default() {
        "agentMessage" => "agent_message", "commandExecution" => "command_execution", "fileChange" => "file_change",
        "mcpToolCall" => "mcp_tool_call", "webSearch" => "web_search", outro => outro,
    });
    if item["status"] == "declined" { v["status"] = json!("failed"); }
    for (de, para) in [("aggregatedOutput","aggregated_output"),("exitCode","exit_code")] {
        if let Some(valor) = item.get(de) { v[para] = valor.clone(); }
    }
    if item["type"] == "reasoning" {
        v["text"] = json!(item["summary"].as_array().into_iter().flatten().chain(item["content"].as_array().into_iter().flatten())
            .filter_map(Value::as_str).collect::<Vec<_>>().join("\n\n"));
    }
    v
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::mcp::Resposta;

    #[derive(Clone)]
    struct Gravador(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Gravador {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> { self.0.lock().unwrap().extend(bytes); Ok(bytes.len()) }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    fn pedidos(g: &Gravador) -> Vec<Value> {
        String::from_utf8(g.0.lock().unwrap().clone()).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }
    fn aberto() -> (Protocolo, Arc<Mutex<Canal>>, Gravador) {
        let g = Gravador(Arc::default());
        let canal = Arc::new(Mutex::new(Canal::novo(Box::new(g.clone()))));
        let mut p = Protocolo::novo(canal.clone(), json!({"cwd":"/p", "sandbox":"read-only", "approvalPolicy":"never", "threadId":"sessao"}), entrada("pedido", &[]), None);
        p.iniciar().unwrap();
        p.linha(&json!({"id":"vitral-init","result":{}}));
        p.linha(&json!({"id":"vitral-thread","result":{"thread":{"id":"sessao"},"model":"gpt-teste"}}));
        p.linha(&json!({"method":"turn/started","params":{"threadId":"sessao","turn":{"id":"turno","status":"inProgress","items":[]}}}));
        (p, canal, g)
    }

    #[test]
    fn mensagem_no_turno_usa_steer_com_texto_imagem_e_id_do_turno() {
        let (mut p, canal, g) = aberto();
        canal.lock().unwrap().dirigir("ajuste", &[PathBuf::from("/p/foto.png")]).unwrap();
        let enviados = pedidos(&g);
        assert_eq!(enviados[2]["method"], "thread/resume");
        let steer = enviados.last().unwrap();
        assert_eq!(steer["method"], "turn/steer");
        assert_eq!(steer["params"]["threadId"], "sessao");
        assert_eq!(steer["params"]["expectedTurnId"], "turno");
        assert_eq!(steer["params"]["input"][1], json!({"type":"localImage","path":"/p/foto.png"}));
        let eventos = p.linha(&json!({"id":steer["id"],"result":{"turnId":"turno"}}));
        assert!(matches!(eventos.as_slice(), [Evento::DirecaoConfirmada { texto, aceita: true }] if texto == "ajuste"));
        assert!(canal.lock().unwrap().aberta());
    }

    #[test]
    fn mensagem_durante_a_abertura_espera_o_id_do_turno_sem_se_perder() {
        let g = Gravador(Arc::default());
        let canal = Arc::new(Mutex::new(Canal::novo(Box::new(g.clone()))));
        canal.lock().unwrap().dirigir("ainda abrindo", &[]).unwrap();
        let mut p = Protocolo::novo(canal, json!({"cwd":"/p"}), entrada("primeira", &[]), None);
        p.iniciar().unwrap();
        p.linha(&json!({"id":"vitral-init","result":{}}));
        p.linha(&json!({"id":"vitral-thread","result":{"thread":{"id":"nova"}}}));
        assert!(!pedidos(&g).iter().any(|v| v["method"] == "turn/steer"));
        p.linha(&json!({"method":"turn/started","params":{"turn":{"id":"t"}}}));
        let enviados = pedidos(&g);
        let ultimo = enviados.last().unwrap();
        assert_eq!(ultimo["method"], "turn/steer");
        assert_eq!(ultimo["params"]["input"][0]["text"], "ainda abrindo");
        assert_eq!(ultimo["params"]["threadId"], "nova");
    }

    #[test]
    fn fim_espera_a_confirmacao_e_a_rejeitada_pode_voltar_para_a_fila() {
        let (mut p, canal, g) = aberto();
        canal.lock().unwrap().dirigir("mensagem no fim", &[]).unwrap();
        let id = pedidos(&g).last().unwrap()["id"].clone();
        let eventos = p.linha(&json!({"method":"turn/completed","params":{"turn":{"id":"turno","status":"completed","items":[]}}}));
        assert!(!eventos.iter().any(|e| matches!(e, Evento::Fim { .. })));
        assert!(!canal.lock().unwrap().aberta());
        let eventos = p.linha(&json!({"id":id,"error":{"code":-32600,"message":"no active turn"}}));
        assert!(matches!(&eventos[0], Evento::DirecaoConfirmada { texto, aceita: false } if texto == "mensagem no fim"));
        assert!(matches!(eventos.last(), Some(Evento::Fim { erro: false, .. })));
    }

    /// Espera a resposta de id `id` chegar à entrada do Codex (a thread do cartão escreve).
    fn resposta_a(g: &Gravador, id: Value) -> Option<Value> {
        let fim = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(v) = pedidos(g).into_iter().find(|v| v["id"] == id && v.get("method").is_none()) {
                return Some(v);
            }
            if std::time::Instant::now() > fim {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// O pedido de aprovação de um comando como o Codex 0.169.0 mandou de verdade (24/11/2026):
    /// o id é um número, e o `availableDecisions` não lista o `decline`, que mesmo assim vale (o
    /// item volta `declined` e o turno segue).
    fn pedido_de_comando(id: u64, comando: &str) -> Value {
        json!({"method":"item/commandExecution/requestApproval","id":id,"params":{"kind":"command","threadId":"sessao",
            "turnId":"turno","itemId":format!("exec-{id}"),"startedAtMs":1,"environmentId":"local","command":comando,
            "cwd":"/p","commandActions":[{"type":"unknown","command":"touch b.txt"}],"proposedExecpolicyAmendment":["touch","b.txt"],
            "availableDecisions":["accept",{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["touch","b.txt"]}},"cancel"]}})
    }

    #[test]
    fn o_pedido_de_comando_vira_o_cartao_e_a_resposta_volta_ao_codex() {
        let (mut p, _, g) = aberto();
        let eventos = p.linha(&pedido_de_comando(0, "/bin/bash -lc 'touch b.txt'"));
        let [Evento::Permissao { ferramenta, entrada, resposta, .. }] = eventos.as_slice() else { panic!("{eventos:?}") };
        assert_eq!(ferramenta, "Bash");
        assert_eq!(entrada["command"], "touch b.txt", "o comando sai do embrulho do shell, como na conversa");
        resposta.try_send(Resposta::ok(json!({"behavior":"deny","message":"O usuário negou esta ação."}).to_string())).unwrap();
        assert_eq!(resposta_a(&g, json!(0)).expect("a recusa")["result"], json!({"decision":"decline"}));
        // O motivo que o Codex dá vai embaixo do comando, e o Permitir vira `accept`.
        let mut com_motivo = pedido_de_comando(1, "/bin/bash -lc 'curl exemplo.com'");
        com_motivo["params"]["reason"] = json!("precisa da rede");
        let eventos = p.linha(&com_motivo);
        let [Evento::Permissao { entrada, resposta, .. }] = eventos.as_slice() else { panic!("{eventos:?}") };
        assert_eq!(entrada["description"], "precisa da rede");
        resposta.try_send(Resposta::ok(json!({"behavior":"allow","updatedInput":entrada}).to_string())).unwrap();
        assert_eq!(resposta_a(&g, json!(1)).expect("a permissão")["result"], json!({"decision":"accept"}));
    }

    #[test]
    fn o_cartao_que_some_sem_resposta_cancela_o_pedido() {
        let (mut p, _, g) = aberto();
        let eventos = p.linha(&pedido_de_comando(4, "/bin/bash -lc ls"));
        drop(eventos);
        assert_eq!(resposta_a(&g, json!(4)).expect("a resposta")["result"], json!({"decision":"cancel"}));
    }

    /// A edição pede aprovação só com o id do item (`FileChangeRequestApprovalParams`); o diff veio
    /// antes, no `item/started` do `fileChange` (medido no 0.169.0: o item abre e o pedido vem
    /// logo em seguida).
    #[test]
    fn o_pedido_de_edicao_mostra_o_diff_do_item_aberto() {
        let (mut p, _, _) = aberto();
        let aberto = |id: &str, mudancas: Value| json!({"method":"item/started","params":{"threadId":"sessao","turnId":"turno","item":{
            "type":"fileChange","id":id,"changes":mudancas,"status":"inProgress"}}});
        let pedido = |n: u64, item: &str| json!({"method":"item/fileChange/requestApproval","id":n,"params":{
            "threadId":"sessao","turnId":"turno","itemId":item,"startedAtMs":1,"reason":null,"grantRoot":null}});
        assert!(p.linha(&aberto("fc-1", json!([{"path":"/p/a.rs","kind":{"type":"update","move_path":null},"diff":"@@ -1 +1 @@\n-antes\n+depois\n"}]))).is_empty());
        let eventos = p.linha(&pedido(7, "fc-1"));
        let [Evento::Permissao { ferramenta, entrada, .. }] = eventos.as_slice() else { panic!("{eventos:?}") };
        assert_eq!((ferramenta.as_str(), entrada["file_path"].as_str()), ("Edit", Some("/p/a.rs")));
        assert_eq!(entrada["diff"], "@@ -1 +1 @@\n-antes\n+depois\n");
        // O arquivo novo (o `add` traz o conteúdo, sem "@@"): um Write com o que vai ser gravado.
        p.linha(&aberto("fc-2", json!([{"path":"/p/a.txt","kind":{"type":"add"},"diff":"oi\n"}])));
        let eventos = p.linha(&pedido(8, "fc-2"));
        let [Evento::Permissao { ferramenta, entrada, .. }] = eventos.as_slice() else { panic!("{eventos:?}") };
        assert_eq!((ferramenta.as_str(), entrada["content"].as_str()), ("Write", Some("oi\n")));
        // Vários arquivos num pedido só, um fora da pasta: o cartão fica com o de fora (o aviso
        // "fora da pasta" aparece e não há regra de "permitir sempre" que o cubra).
        p.linha(&aberto("fc-3", json!([
            {"path":"/p/b.rs","kind":{"type":"update"},"diff":"@@ -1 +1 @@\n-b\n+B\n"},
            {"path":"/etc/hosts","kind":{"type":"update"},"diff":"@@ -1 +1 @@\n-h\n+H\n"},
        ])));
        let eventos = p.linha(&pedido(9, "fc-3"));
        let [Evento::Permissao { ferramenta, entrada, .. }] = eventos.as_slice() else { panic!("{eventos:?}") };
        assert_eq!((ferramenta.as_str(), entrada["file_path"].as_str()), ("Edit", Some("/etc/hosts")));
        let diff = entrada["diff"].as_str().unwrap();
        assert!(diff.contains("+B") && diff.contains("+H"), "{diff}");
        assert!(crate::modelo::regra_de_permissao(ferramenta, entrada, std::path::Path::new("/p")).is_none());
    }

    /// O `serverRequest/resolved` (o turno parou, ou o pedido foi respondido por outro caminho):
    /// o cartão sai da tela e ninguém responde ao Codex um pedido que ele já fechou.
    #[test]
    fn o_codex_resolver_o_pedido_tira_o_cartao_sem_resposta() {
        let (mut p, _, g) = aberto();
        let eventos = p.linha(&pedido_de_comando(3, "/bin/bash -lc 'touch b.txt'"));
        let [Evento::Permissao { resposta, .. }] = eventos.as_slice() else { panic!("{eventos:?}") };
        let resolvido = p.linha(&json!({"method":"serverRequest/resolved","params":{"threadId":"sessao","requestId":3}}));
        assert!(matches!(resolvido.as_slice(), [Evento::PermissaoRetirada]), "{resolvido:?}");
        assert!(resposta.is_closed(), "o canal do cartão fecha: o motor o tira da lista");
        assert!(resposta_a(&g, json!(3)).is_none(), "nada vai ao Codex depois de resolvido");
    }

    /// A política de aprovação vai na abertura da conversa e de novo no `turn/start` (o pedido
    /// dele pode trocar a da conversa retomada), e o initialize liga a API experimental (é ela que
    /// aceita o `beforeTurnId` do `thread/fork`).
    #[test]
    fn o_turno_leva_a_politica_de_aprovacao() {
        let g = Gravador(Arc::default());
        let canal = Arc::new(Mutex::new(Canal::novo(Box::new(g.clone()))));
        let mut p = Protocolo::novo(canal, json!({"cwd":"/p","sandbox":"workspace-write","approvalPolicy":"untrusted"}), entrada("oi", &[]), None);
        p.iniciar().unwrap();
        p.linha(&json!({"id":"vitral-init","result":{}}));
        p.linha(&json!({"id":"vitral-thread","result":{"thread":{"id":"t1"}}}));
        let enviados = pedidos(&g);
        assert_eq!(enviados[0]["params"]["capabilities"]["experimentalApi"], true);
        let turno = enviados.iter().find(|v| v["method"] == "turn/start").expect("turn/start");
        assert_eq!(turno["params"]["approvalPolicy"], "untrusted");
    }

    /// Um protocolo que retoma a conversa "sessao" com `texto`, já com a abertura respondida e as
    /// skills de `skills` (o `data` do `skills/list`).
    fn retomado_com(texto: &str, skills: Value) -> (Protocolo, Gravador) {
        let g = Gravador(Arc::default());
        let canal = Arc::new(Mutex::new(Canal::novo(Box::new(g.clone()))));
        let mut p = Protocolo::novo(canal, json!({"cwd":"/p","threadId":"sessao","approvalPolicy":"untrusted"}), entrada(texto, &[]), None);
        p.iniciar().unwrap();
        p.linha(&json!({"id":"vitral-init","result":{}}));
        p.linha(&json!({"id":"vitral-skills","result":{"data":skills}}));
        p.linha(&json!({"id":"vitral-thread","result":{"thread":{"id":"sessao"},"model":"gpt-teste"}}));
        (p, g)
    }

    /// O `/compact` é o `thread/compact/start` do app-server, e não uma mensagem: medido no
    /// 0.169.0 (24/11/2026), ele responde `{}` e abre um turno com um item `contextCompaction`.
    #[test]
    fn o_compact_resume_a_conversa_pelo_app_server() {
        let (mut p, g) = retomado_com("/compact", json!([]));
        let enviados = pedidos(&g);
        assert!(!enviados.iter().any(|v| v["method"] == "turn/start"), "o /compact não vai como mensagem");
        let compactar = enviados.iter().find(|v| v["method"] == "thread/compact/start").expect("thread/compact/start");
        assert_eq!(compactar["params"], json!({"threadId":"sessao"}));
        assert!(p.linha(&json!({"id":compactar["id"],"result":{}})).is_empty());
        let mut eventos = p.linha(&json!({"method":"turn/started","params":{"threadId":"sessao","turn":{"id":"t-compactar","status":"inProgress","items":[]}}}));
        for (metodo, item) in [("item/started", json!({"type":"contextCompaction","id":"cc"})), ("item/completed", json!({"type":"contextCompaction","id":"cc"}))] {
            eventos.extend(p.linha(&json!({"method":metodo,"params":{"threadId":"sessao","turnId":"t-compactar","item":item}})));
        }
        eventos.extend(p.linha(&json!({"method":"turn/completed","params":{"threadId":"sessao","turn":{"id":"t-compactar","status":"completed","items":[]}}})));
        let resumo: Vec<String> = eventos
            .iter()
            .filter_map(|e| match e {
                Evento::Inicio { .. } => Some("inicio".into()),
                Evento::Compactando(sim) => Some(format!("compactando:{sim}")),
                Evento::Compactou { manual, .. } => Some(format!("compactou:{manual}")),
                Evento::Fim { erro, .. } => Some(format!("fim:{erro}")),
                _ => None,
            })
            .collect();
        assert_eq!(resumo, ["inicio", "compactando:true", "compactou:true", "fim:false"]);
    }

    /// O `/review` é o `review/start` do app-server: sem texto, as mudanças que não foram para o
    /// commit; com texto, a revisão que o texto pede. O resultado vem no item `exitedReviewMode`.
    #[test]
    fn o_review_pede_a_revisao_ao_app_server() {
        let (_, g) = retomado_com("/review", json!([]));
        let revisar = pedidos(&g).into_iter().find(|v| v["method"] == "review/start").expect("review/start");
        assert_eq!(revisar["params"], json!({"threadId":"sessao","target":{"type":"uncommittedChanges"}}));
        let (mut p, g) = retomado_com("/review  olhe o cálculo do frete ", json!([]));
        let revisar = pedidos(&g).into_iter().find(|v| v["method"] == "review/start").expect("review/start");
        assert_eq!(revisar["params"]["target"], json!({"type":"custom","instructions":"olhe o cálculo do frete"}));
        p.linha(&json!({"method":"turn/started","params":{"threadId":"sessao","turn":{"id":"t-rev","status":"inProgress","items":[]}}}));
        let eventos = p.linha(&json!({"method":"item/completed","params":{"threadId":"sessao","turnId":"t-rev","item":{
            "type":"exitedReviewMode","id":"rv","review":"O frete ignora o CEP."}}}));
        assert!(matches!(eventos.as_slice(), [Evento::Texto(t)] if t == "O frete ignora o CEP."), "{eventos:?}");
        let fim = p.linha(&json!({"method":"turn/completed","params":{"threadId":"sessao","turn":{"id":"t-rev","status":"completed","items":[]}}}));
        assert!(matches!(fim.last(), Some(Evento::Fim { erro: false, texto, .. }) if texto == "O frete ignora o CEP."));
    }

    /// As skills que o Codex oferece na pasta (`skills/list`, conferido no 0.169.0) são comandos
    /// de barra: "/nome" vai como a entrada `skill` com o arquivo dela e o texto "$nome …", como o
    /// Codex faz quando se escolhe a skill. A lista vai no `Inicio`, para o menu "/".
    #[test]
    fn a_skill_vira_comando_de_barra() {
        let skills = json!([{"cwd":"/p","skills":[
            {"name":"revisar-pr","description":"Revisa o PR","path":"/h/.codex/skills/revisar-pr/SKILL.md","scope":"user","enabled":true},
            {"name":"desligada","description":"x","path":"/h/x/SKILL.md","scope":"user","enabled":false},
        ],"errors":[]}]);
        let (mut p, g) = retomado_com("/revisar-pr olhe o diff", skills.clone());
        let enviados = pedidos(&g);
        let listar = enviados.iter().find(|v| v["method"] == "skills/list").expect("skills/list");
        assert_eq!(listar["params"], json!({"cwds":["/p"]}));
        let turno = enviados.iter().find(|v| v["method"] == "turn/start").expect("turn/start");
        assert_eq!(turno["params"]["input"], json!([
            {"type":"text","text":"$revisar-pr olhe o diff"},
            {"type":"skill","name":"revisar-pr","path":"/h/.codex/skills/revisar-pr/SKILL.md"},
        ]));
        let eventos = p.linha(&json!({"method":"turn/started","params":{"turn":{"id":"t"}}}));
        assert!(matches!(eventos.as_slice(), [Evento::Inicio { comandos, .. }, ..] if *comandos == ["compact", "review", "revisar-pr"]), "{eventos:?}");
        // Uma barra que não é skill nem comando vai como texto, igual.
        let (_, g) = retomado_com("/etc/hosts tem o quê?", skills);
        let turno = pedidos(&g).into_iter().find(|v| v["method"] == "turn/start").expect("turn/start");
        assert_eq!(turno["params"]["input"], json!([{"type":"text","text":"/etc/hosts tem o quê?"}]));
    }

    /// A mensagem com barra espera a lista das skills para começar o turno; a lista que falha não
    /// para o turno.
    #[test]
    fn a_barra_espera_a_lista_das_skills_e_a_falha_dela_nao_para_o_turno() {
        let g = Gravador(Arc::default());
        let canal = Arc::new(Mutex::new(Canal::novo(Box::new(g.clone()))));
        let mut p = Protocolo::novo(canal, json!({"cwd":"/p"}), entrada("/revisar-pr", &[]), None);
        p.iniciar().unwrap();
        p.linha(&json!({"id":"vitral-init","result":{}}));
        p.linha(&json!({"id":"vitral-thread","result":{"thread":{"id":"t1"}}}));
        assert!(!pedidos(&g).iter().any(|v| v["method"] == "turn/start"), "ainda sem a lista");
        let eventos = p.linha(&json!({"id":"vitral-skills","error":{"code":-32600,"message":"sem skills"}}));
        assert!(!eventos.iter().any(|e| matches!(e, Evento::Fim { .. })), "{eventos:?}");
        let turno = pedidos(&g).into_iter().find(|v| v["method"] == "turn/start").expect("turn/start");
        assert_eq!(turno["params"]["input"], json!([{"type":"text","text":"/revisar-pr"}]));
    }

    /// O `thread/tokenUsage/updated` de um turno: `total` (a conversa inteira) e `last` (a última
    /// chamada ao modelo), cada um com [entrada, cache lido, saída].
    fn uso(turno: &str, total: [u64; 3], ultima: [u64; 3], janela: u64) -> Value {
        let parte = |[entrada, cache, saida]: [u64; 3]| json!({"totalTokens":entrada + saida,"inputTokens":entrada,
            "cachedInputTokens":cache,"cacheWriteInputTokens":0,"outputTokens":saida,"reasoningOutputTokens":0});
        json!({"method":"thread/tokenUsage/updated","params":{"threadId":"sessao","turnId":turno,
            "tokenUsage":{"total":parte(total),"last":parte(ultima),"modelContextWindow":janela}}})
    }

    /// Os números de um turno de verdade do 0.169.0 (gpt-6.2-sol, 24/11/2026, quatro chamadas):
    /// o contexto é o `last.totalTokens` da última chamada, a janela é o `modelContextWindow`, e o
    /// gasto do turno é o `total` do fim menos o de antes da primeira chamada.
    #[test]
    fn o_uso_do_codex_vira_o_contexto_e_os_tokens_do_turno() {
        let (mut p, _, _) = aberto();
        let chamadas = [
            ([16623, 0, 46], [16623, 0, 46]),
            ([33316, 16512, 79], [16693, 16512, 33]),
            ([50066, 33024, 102], [16750, 16512, 23]),
            ([66884, 49664, 132], [16818, 16640, 30]),
        ];
        let mut contextos = Vec::new();
        for (total, ultima) in chamadas {
            for e in p.linha(&uso("turno", total, ultima, 475_000)) {
                if let Evento::Contexto(n) = e { contextos.push(n) }
            }
        }
        assert_eq!(contextos, [16669, 16726, 16773, 16848]);
        let fim = p.linha(&json!({"method":"turn/completed","params":{"threadId":"sessao","turn":{"id":"turno","status":"completed","items":[]}}}));
        match fim.as_slice() {
            [Evento::Gasto(tokens), Evento::Fim { erro: false, janela, .. }] => {
                assert_eq!((tokens.entrada, tokens.cache_lido, tokens.cache_criado, tokens.saida), (66884 - 49664, 49664, 0, 132));
                assert_eq!(*janela, Some(475_000));
            }
            outros => panic!("{outros:?}"),
        }
    }

    /// Numa conversa retomada, o `total` já traz os turnos de antes: só o que este turno somou
    /// conta. O uso de outro turno (o que a retomada repete) muda o contexto e não o gasto.
    #[test]
    fn o_gasto_do_turno_nao_leva_os_turnos_de_antes() {
        let (mut p, _, _) = aberto();
        p.linha(&uso("turno-velho", [100_000, 90_000, 500], [20_000, 19_000, 100], 258_400));
        p.linha(&uso("turno", [116_623, 100_000, 546], [16_623, 10_000, 46], 258_400));
        p.linha(&uso("turno", [133_316, 116_512, 579], [16_693, 16_512, 33], 258_400));
        let fim = p.linha(&json!({"method":"turn/completed","params":{"turn":{"id":"turno","status":"completed","items":[]}}}));
        let [Evento::Gasto(tokens), Evento::Fim { .. }] = fim.as_slice() else { panic!("{fim:?}") };
        assert_eq!((tokens.entrada, tokens.cache_lido, tokens.saida), (33_316 - 26_512, 26_512, 79));
        // Sem nenhum uso no turno, nem gasto nem janela.
        let (mut p, _, _) = aberto();
        let fim = p.linha(&json!({"method":"turn/completed","params":{"turn":{"id":"turno","status":"completed","items":[]}}}));
        assert!(matches!(fim.as_slice(), [Evento::Fim { janela: None, .. }]), "{fim:?}");
    }

    #[test]
    fn a_edicao_do_codex_guarda_o_diff_daquele_item() {
        let (mut p, _, _) = aberto();
        let eventos = p.linha(&json!({"method":"item/completed","params":{"item":{
            "id":"edicao","type":"fileChange","status":"completed","changes":[{
                "path":"/p/a.rs","kind":{"type":"update"},"diff":"@@ -1 +1 @@\n-antes\n+depois"
            }]
        }}}));
        assert!(eventos.iter().any(|e| matches!(e, Evento::Ferramenta { entrada, .. } if entrada["diff"] == "@@ -1 +1 @@\n-antes\n+depois")));
    }
}
