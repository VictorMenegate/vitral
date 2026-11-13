//! Servidor MCP local (HTTP, JSON-RPC) que dá aos chats as ferramentas do Vitral.
//!
//! Cada chat recebe uma URL própria, `/mcp/<chat>/<token>`, então o servidor sabe
//! quem está chamando. As chamadas de ferramenta vão para o motor (thread da interface) e a
//! requisição HTTP espera a resposta; pedidos de permissão podem esperar o usuário.

use std::{sync::Arc, thread};

use anyhow::Result;
use serde_json::{Value, json};
use smol::channel::{Sender, bounded};


pub struct Chamada {
    pub chat: String,
    pub ferramenta: String,
    pub argumentos: Value,
    pub resposta: Sender<Resposta>,
}

pub struct Resposta {
    pub texto: String,
    pub erro: bool,
}

impl Resposta {
    pub fn ok(texto: impl Into<String>) -> Self {
        Self {
            texto: texto.into(),
            erro: false,
        }
    }

    pub fn erro(texto: impl Into<String>) -> Self {
        Self {
            texto: texto.into(),
            erro: true,
        }
    }
}

pub struct Servidor {
    pub porta: u16,
    pub token: String,
}

impl Servidor {
    /// A URL do chat: o `tools/list` sai dela.
    pub fn url(&self, chat: &str) -> String {
        format!("http://127.0.0.1:{}/mcp/{chat}/{}", self.porta, self.token)
    }
}

pub fn iniciar(enviar: impl Fn(Chamada) + Send + Sync + 'static) -> Result<Servidor> {
    let servidor = tiny_http::Server::http("127.0.0.1:0").map_err(|e| anyhow::anyhow!("{e}"))?;
    let porta = servidor
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .unwrap_or_default();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let servidor = Arc::new(servidor);
    let enviar = Arc::new(enviar);
    let token_thread = token.clone();
    thread::spawn(move || {
        for requisicao in servidor.incoming_requests() {
            let enviar = enviar.clone();
            let token = token_thread.clone();
            thread::spawn(move || atender(requisicao, &token, &*enviar));
        }
    });
    Ok(Servidor { porta, token })
}

fn atender(mut requisicao: tiny_http::Request, token: &str, enviar: &dyn Fn(Chamada)) {
    let partes: Vec<String> = requisicao
        .url()
        .trim_matches('/')
        .split('/')
        .map(str::to_string)
        .collect();
    let rota = match partes.as_slice() {
        [mcp, chat, t] if mcp == "mcp" && t == token => Some(chat.clone()),
        _ => None,
    };
    let Some(chat) = rota else {
        let _ = requisicao.respond(tiny_http::Response::empty(404));
        return;
    };
    match requisicao.method() {
        tiny_http::Method::Post => {}
        tiny_http::Method::Delete => {
            let _ = requisicao.respond(tiny_http::Response::empty(200));
            return;
        }
        // Sem fluxo SSE do servidor para o cliente: o protocolo aceita 405 aqui.
        _ => {
            let _ = requisicao.respond(tiny_http::Response::empty(405));
            return;
        }
    }

    let mut corpo = String::new();
    if requisicao.as_reader().read_to_string(&mut corpo).is_err() {
        let _ = requisicao.respond(tiny_http::Response::empty(400));
        return;
    }
    let Ok(mensagem) = serde_json::from_str::<Value>(&corpo) else {
        let _ = requisicao.respond(tiny_http::Response::empty(400));
        return;
    };

    let resposta = match &mensagem {
        Value::Array(lote) => {
            let respostas: Vec<Value> = lote
                .iter()
                .filter_map(|m| responder(m, &chat, enviar))
                .collect();
            (!respostas.is_empty()).then(|| Value::Array(respostas))
        }
        m => responder(m, &chat, enviar),
    };
    let Some(resposta) = resposta else {
        let _ = requisicao.respond(tiny_http::Response::empty(202));
        return;
    };
    let cabecalho = tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap();
    let mut resposta =
        tiny_http::Response::from_string(resposta.to_string()).with_header(cabecalho);
    if mensagem["method"] == "initialize" {
        let sessao = uuid::Uuid::new_v4().to_string();
        resposta.add_header(tiny_http::Header::from_bytes("Mcp-Session-Id", sessao).unwrap());
    }
    let _ = requisicao.respond(resposta);
}

/// Responde a uma mensagem JSON-RPC. Notificações (sem `id`) não têm resposta.
fn responder(
    mensagem: &Value,
    chat: &str,
    enviar: &dyn Fn(Chamada),
) -> Option<Value> {
    let id = mensagem.get("id")?.clone();
    let metodo = mensagem["method"].as_str().unwrap_or_default();
    let resultado = match metodo {
        "initialize" => json!({
            "protocolVersion": mensagem["params"]["protocolVersion"]
                .as_str()
                .unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "vitral", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCOES,
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": ferramentas() }),
        "tools/call" => {
            let (envia, recebe) = bounded(1);
            enviar(Chamada {
                chat: chat.to_string(),
                ferramenta: mensagem["params"]["name"]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
                argumentos: mensagem["params"]["arguments"].clone(),
                resposta: envia,
            });
            let resposta = recebe
                .recv_blocking()
                .unwrap_or_else(|_| Resposta::erro("O Vitral fechou antes de responder."));
            json!({
                "content": [{ "type": "text", "text": cortar_resposta(resposta.texto) }],
                "isError": resposta.erro,
            })
        }
        _ => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("método desconhecido: {metodo}") },
            }));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": resultado }))
}

fn ferramenta(nome: &str, descricao: &str, propriedades: Value, obrigatorias: &[&str]) -> Value {
    json!({
        "name": nome,
        "description": descricao,
        "inputSchema": {
            "type": "object",
            "properties": propriedades,
            "required": obrigatorias,
        },
    })
}

/// As ferramentas do Vitral, na ordem do `tools/list`. A mesma lista guarda o topo do
/// `tratar_ferramenta`: uma ferramenta fora dela não existe.
pub const FERRAMENTAS: &[&str] = &["listar_chats", "mensagem_para_chat", "permissao", "abrir_chat"];

pub fn pode(nome: &str) -> bool {
    FERRAMENTAS.contains(&nome)
}

/// Toda resposta de ferramenta tem no máximo isso: acima, o Claude Code a cortaria sozinho
/// (`MAX_MCP_OUTPUT_TOKENS`), sem dizer onde.
pub const MAX_RESPOSTA: usize = 40_000;

pub fn cortar_resposta(texto: String) -> String {
    if texto.chars().count() <= MAX_RESPOSTA {
        return texto;
    }
    let mut cortado: String = texto.chars().take(MAX_RESPOSTA).collect();
    cortado.push_str("\n(cortado: peça um trecho menor)");
    cortado
}

const INSTRUCOES: &str = "Ferramentas do Vitral para um chat: as conversas com os outros chats.";

fn texto_obrigatorio() -> Value {
    json!({ "type": "string" })
}

fn definicao(nome: &str) -> Option<Value> {
    Some(match nome {
        "listar_chats" => ferramenta(
            "listar_chats",
            "Lista os chats do Vitral, os mais recentes primeiro: id, título, se está rodando, esperando o \
             usuário ou na fila, a pasta e há quanto tempo mexeu.",
            json!({
                "ativos": { "type": "boolean", "description": "só os que estão rodando, esperando o usuário ou na fila" },
            }),
            &[],
        ),
        "mensagem_para_chat" => ferramenta(
            "mensagem_para_chat",
            "Manda uma mensagem para outro chat (o id de listar_chats). \
             Ela chega como uma nova mensagem para ele assim que ele estiver livre. Use para combinar \
             coisas entre chats.",
            json!({
                "chat": { "type": "string", "description": "id do chat de destino" },
                "texto": texto_obrigatorio(),
            }),
            &["chat", "texto"],
        ),
        "permissao" => ferramenta(
            "permissao",
            "Uso interno do Vitral para pedidos de permissão. Não chame diretamente.",
            json!({
                "tool_name": { "type": "string" },
                "input": { "type": "object" },
                "tool_use_id": { "type": "string" },
            }),
            &["tool_name", "input"],
        ),
        "abrir_chat" => ferramenta(
            "abrir_chat",
            "Abre um chat novo, à vista do usuário, para um assunto que não é o desta conversa, já com o pedido \
             enviado. Só quando o usuário pedir (\"abre um chat para isso\"). Ele trabalha sozinho, direto na \
             sua pasta, e o usuário continua a conversa por lá; nada volta para você sozinho (para combinar \
             algo, use mensagem_para_chat com o id devolvido).",
            json!({
                "titulo": { "type": "string", "description": "curto, vira o título do chat" },
                "pedido": { "type": "string", "description": "o pedido completo, com todo o contexto (o chat novo não vê esta conversa)" },
            }),
            &["titulo", "pedido"],
        ),
        _ => return None,
    })
}

fn ferramentas() -> Vec<Value> {
    FERRAMENTAS.iter().filter_map(|nome| definicao(nome)).collect()
}

#[cfg(test)]
mod testes {
    use super::*;

    fn nomes() -> Vec<String> {
        ferramentas().iter().filter_map(|f| f["name"].as_str().map(str::to_string)).collect()
    }

    #[test]
    fn o_chat_tem_as_ferramentas_dele() {
        assert_eq!(nomes(), ["listar_chats", "mensagem_para_chat", "permissao", "abrir_chat"]);
        for saiu in ["propor_divisao", "criar_tarefa", "ver_diff", "pedir_ajuste", "aprovar_tarefa", "cancelar_tarefa",
            "informar_progresso", "perguntar_coordenador", "revisar_minhas_mudancas", "anotar_decisao"]
        {
            assert!(!pode(saiu), "{saiu}");
        }
        assert!(!pode("nao_existe"));
        // Toda ferramenta da lista tem a definição.
        for nome in FERRAMENTAS {
            assert!(definicao(nome).is_some(), "{nome}");
        }
        let abrir = ferramentas().into_iter().find(|f| f["name"] == "abrir_chat").unwrap();
        assert_eq!(abrir["inputSchema"]["required"], json!(["titulo", "pedido"]));
    }

    #[test]
    fn resposta_grande_e_cortada_com_o_aviso() {
        let curto = "abc".to_string();
        assert_eq!(cortar_resposta(curto.clone()), curto);
        let grande = "á".repeat(MAX_RESPOSTA + 10);
        let cortado = cortar_resposta(grande);
        assert!(cortado.ends_with("(cortado: peça um trecho menor)"));
        assert_eq!(cortado.chars().filter(|c| *c == 'á').count(), MAX_RESPOSTA);
    }
}
