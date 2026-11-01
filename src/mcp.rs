//! Servidor MCP local (HTTP, JSON-RPC) que dá aos chats as ferramentas do Vitral.
//!
//! Cada chat recebe uma URL própria, `/mcp/<papel>/<chat>/<token>`, então o servidor sabe
//! quem está chamando. As chamadas de ferramenta vão para o motor (thread da interface) e a
//! requisição HTTP espera a resposta; pedidos de permissão podem esperar o usuário.

use std::{sync::Arc, thread};

use anyhow::Result;
use serde_json::{Value, json};
use smol::channel::{Sender, bounded};

use crate::modelo::Papel;

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
    pub fn url(&self, papel: Papel, chat: &str) -> String {
        let papel = match papel {
            Papel::Coordenador => "coordenador",
            Papel::Trabalhador => "trabalhador",
        };
        format!(
            "http://127.0.0.1:{}/mcp/{papel}/{chat}/{}",
            self.porta, self.token
        )
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
        [mcp, papel, chat, t] if mcp == "mcp" && t == token => {
            let papel = if papel == "coordenador" {
                Papel::Coordenador
            } else {
                Papel::Trabalhador
            };
            Some((papel, chat.clone()))
        }
        _ => None,
    };
    let Some((papel, chat)) = rota else {
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
                .filter_map(|m| responder(m, papel, &chat, enviar))
                .collect();
            (!respostas.is_empty()).then(|| Value::Array(respostas))
        }
        m => responder(m, papel, &chat, enviar),
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
    papel: Papel,
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
            "instructions": "Ferramentas do Vitral para coordenar chats, tarefas e revisões.",
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": ferramentas(papel) }),
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
                "content": [{ "type": "text", "text": resposta.texto }],
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

fn ferramentas(papel: Papel) -> Vec<Value> {
    let mut lista = vec![
        ferramenta(
            "listar_chats",
            "Lista os chats desta pasta de trabalho (id, projeto, papel, título e se está rodando).",
            json!({}),
            &[],
        ),
        ferramenta(
            "mensagem_para_chat",
            "Manda uma mensagem para outro chat (pelo id de listar_chats). Ela chega como uma nova \
             mensagem para ele assim que ele estiver livre. Use para combinar coisas entre chats \
             de projetos diferentes ou entre trabalhadores.",
            json!({
                "chat": { "type": "string", "description": "id do chat de destino" },
                "texto": { "type": "string" },
            }),
            &["chat", "texto"],
        ),
        ferramenta(
            "permissao",
            "Uso interno do Vitral para pedidos de permissão. Não chame diretamente.",
            json!({
                "tool_name": { "type": "string" },
                "input": { "type": "object" },
                "tool_use_id": { "type": "string" },
            }),
            &["tool_name", "input"],
        ),
    ];
    match papel {
        Papel::Coordenador => lista.extend([
            ferramenta(
                "criar_tarefa",
                "Cria uma tarefa e um chat trabalhador para ela. O trabalhador começa na hora, numa \
                 cópia isolada do projeto (git worktree) quando o projeto usa git. Quando ele terminar, \
                 o relatório chega para você automaticamente como mensagem: depois de criar as \
                 tarefas, encerre sua vez.",
                json!({
                    "titulo": { "type": "string", "description": "curto, aparece na barra lateral" },
                    "objetivo": { "type": "string", "description": "o que fazer, com o contexto que o trabalhador precisa (ele não vê esta conversa)" },
                    "criterios": { "type": "array", "items": { "type": "string" }, "description": "critérios de aceitação verificáveis" },
                    "verificacao": { "type": "string", "description": "comando de shell que o Vitral roda na cópia da tarefa quando o trabalhador termina (ex.: cargo test). Opcional." },
                    "modelo": { "type": "string", "description": "modelo do trabalhador (fable, opus, sonnet, haiku). Opcional." },
                }),
                &["titulo", "objetivo", "criterios"],
            ),
            ferramenta(
                "listar_tarefas",
                "Lista as tarefas deste coordenador com estado, rodada e verificação.",
                json!({}),
                &[],
            ),
            ferramenta(
                "ver_diff",
                "Mostra o que a tarefa mudou em relação ao ponto de partida (git diff, incluindo \
                 arquivos novos). Arquivos gerados (lockfiles, .min.js, .snap) ficam de fora numa \
                 linha, e um diff grande vem resumido: o --stat, os arquivos que cabem inteiros e a \
                 lista dos que faltam, para você pedir com `arquivos`.",
                json!({
                    "tarefa": { "type": "integer" },
                    "arquivos": { "type": "array", "items": { "type": "string" }, "description": "caminhos relativos ao projeto; vem o diff completo só desses arquivos, mesmo os gerados. Opcional." },
                }),
                &["tarefa"],
            ),
            ferramenta(
                "pedir_ajuste",
                "Devolve a tarefa ao trabalhador. Ele continua na mesma conversa e o novo relatório \
                 chega para você. O Vitral anexa sozinho os problemas do revisor e o fim da saída da \
                 verificação quando eles apontaram algo (ou o que você puser em `incluir`; [] não \
                 anexa nada). No `feedback` escreva só a sua decisão: o que priorizar, o que ignorar \
                 e o que mais espera. Não copie o que vai anexado.",
                json!({
                    "tarefa": { "type": "integer" },
                    "feedback": { "type": "string" },
                    "incluir": {
                        "type": "array",
                        "items": { "type": "string", "enum": ["revisor", "verificacao"] },
                        "description": "o que anexar; sem este campo, vai o que apontou problema",
                    },
                    "ignorar_problemas": {
                        "type": "array",
                        "items": { "type": "integer" },
                        "description": "números dos problemas do revisor (do relatório) que são falso positivo e não vão",
                    },
                }),
                &["tarefa", "feedback"],
            ),
            ferramenta(
                "aprovar_tarefa",
                "Aprova a tarefa: o Vitral faz o commit na cópia e junta o branch dela no projeto. Se \
                 houver conflito, nada é juntado e a resposta lista os arquivos. Recusa se a tarefa \
                 não está em revisão com o relatório desta rodada, se a verificação falhou ou se o \
                 revisor apontou problema de gravidade alta; para aprovar mesmo assim, forcar=true \
                 com um motivo (fica no commit e é mostrado ao usuário).",
                json!({
                    "tarefa": { "type": "integer" },
                    "nota": { "type": "string", "description": "mensagem do commit (opcional)" },
                    "forcar": { "type": "boolean", "description": "aprovar apesar da verificação falhando ou de problema grave do revisor (exige motivo)" },
                    "motivo": { "type": "string", "description": "por que a falha não vem desta tarefa (obrigatório com forcar)" },
                }),
                &["tarefa"],
            ),
            ferramenta(
                "cancelar_tarefa",
                "Cancela a tarefa e para o trabalhador. A cópia isolada fica guardada até o usuário \
                 limpar.",
                json!({
                    "tarefa": { "type": "integer" },
                    "motivo": { "type": "string" },
                }),
                &["tarefa"],
            ),
            ferramenta(
                "anotar_decisao",
                "Registra uma decisão ou convenção do projeto. As notas entram no contexto de todos os \
                 trabalhadores novos deste projeto.",
                json!({ "texto": { "type": "string" } }),
                &["texto"],
            ),
        ]),
        Papel::Trabalhador => lista.extend([
            ferramenta(
                "perguntar_coordenador",
                "Manda uma pergunta ao coordenador. A resposta chega como nova mensagem; se a dúvida \
                 bloquear o trabalho, encerre sua vez depois de perguntar.",
                json!({ "pergunta": { "type": "string" } }),
                &["pergunta"],
            ),
            ferramenta(
                "informar_progresso",
                "Atualiza a linha de progresso da sua tarefa na interface (uma frase curta).",
                json!({ "texto": { "type": "string" } }),
                &["texto"],
            ),
        ]),
    }
    lista
}
