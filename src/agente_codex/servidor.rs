//! JSON-RPC do `codex app-server`, conferido com o schema gerado pela CLI instalada.
//! O canal fica aberto durante o turno: `turn/steer` recebe texto e imagens sem interromper o
//! trabalho. A confirmação volta ao motor; uma recusa devolve só aquela mensagem à fila.

use std::{collections::HashMap, io::{self, Write}, path::PathBuf, sync::{Arc, Mutex}};
use serde_json::{Value, json};
use crate::claude::Evento;

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
}

impl Canal {
    pub fn novo(saida: Box<dyn Write + Send>) -> Self {
        Self { saida: Some(saida), thread: String::new(), turno: String::new(), concluido: false,
            proximo: 0, pendentes: HashMap::new(), aguardando_inicio: Vec::new() }
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

pub struct Protocolo {
    canal: Arc<Mutex<Canal>>,
    parametros: Value,
    input: Vec<Value>,
    esforco: Option<String>,
    leitor: super::Leitor,
    modelo: String,
    fim: Option<Evento>,
}

impl Protocolo {
    pub fn novo(canal: Arc<Mutex<Canal>>, parametros: Value, input: Vec<Value>, esforco: Option<String>) -> Self {
        Self { canal, parametros, input, esforco, leitor: super::Leitor::default(), modelo: String::new(), fim: None }
    }
    pub fn iniciar(&mut self) -> io::Result<()> {
        self.canal.lock().unwrap_or_else(|e| e.into_inner()).escrever(json!({"id":"vitral-init", "method":"initialize", "params":{
            "clientInfo":{"name":"vitral", "version":env!("CARGO_PKG_VERSION")},
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
        let mut canal = self.canal.lock().unwrap_or_else(|e| e.into_inner());
        if v.get("id").is_some() && v.get("method").is_some() {
            return canal.escrever(json!({"id":v["id"],"error":{"code":-32601,"message":"Este pedido ainda não é suportado pelo Vitral."}}));
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
                        canal.escrever(json!({"id":"vitral-turn", "method":"turn/start", "params":{
                            "threadId":thread, "input":self.input, "effort":self.esforco,
                        }}))?;
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
                    let normal = normalizar(item);
                    eventos.extend(self.leitor.linha(&json!({"type":if terminou {"item.completed"} else {"item.started"}, "item":normal})));
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
