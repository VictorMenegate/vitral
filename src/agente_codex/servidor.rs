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
}

impl Protocolo {
    pub fn novo(canal: Arc<Mutex<Canal>>, parametros: Value, input: Vec<Value>, esforco: Option<String>) -> Self {
        Self { canal, parametros, input, esforco, leitor: super::Leitor::default(), modelo: String::new(), fim: None,
            mudancas: HashMap::new() }
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
        if let Some(id) = v["id"].as_str() {
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
                    }
                    "vitral-thread" => {
                        let thread = v["result"]["thread"]["id"].as_str().ok_or_else(|| io::Error::other("O Codex não devolveu a conversa"))?;
                        canal.thread = thread.to_string();
                        self.modelo = v["result"]["model"].as_str().unwrap_or_default().to_string();
                        eventos.push(Evento::Sessao(thread.to_string()));
                        let mut params = json!({"threadId":thread, "input":self.input, "effort":self.esforco});
                        // A política vai de novo no turno: a da conversa retomada pode ser outra.
                        if let Some(politica) = self.parametros.get("approvalPolicy") {
                            params["approvalPolicy"] = politica.clone();
                        }
                        canal.escrever(json!({"id":"vitral-turn", "method":"turn/start", "params":params}))?;
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
                    eventos.push(Evento::Inicio { modelo: self.modelo.clone(), comandos: Vec::new(), versao: None, fluxos: None });
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
                    let normal = normalizar(item);
                    eventos.extend(self.leitor.linha(&json!({"type":if terminou {"item.completed"} else {"item.started"}, "item":normal})));
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
                "turn/completed" => {
                    canal.concluido = true;
                    let turno = &p["turn"];
                    let erro = turno["status"] != "completed";
                    let texto = if erro { turno["error"]["message"].as_str().unwrap_or("O turno foi interrompido.").to_string() }
                        else { std::mem::take(&mut self.leitor.ultima_fala) };
                    self.fim = Some(Evento::Fim { erro, texto, custo_usd: 0., janela: None });
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
