//! Servidor MCP local (HTTP, JSON-RPC) que dá aos chats as ferramentas do Vitral.
//!
//! Cada chat recebe uma URL própria, `/mcp/<papel>/<chat>/<token>`, então o servidor sabe
//! quem está chamando. As chamadas de ferramenta vão para o motor (thread da interface) e a
//! requisição HTTP espera a resposta; pedidos de permissão podem esperar o usuário.

use std::{sync::Arc, thread};

use anyhow::Result;
use serde_json::{Value, json};
use smol::channel::{Sender, bounded};

use crate::modelo::Papel::{self, Mestre as M, Projeto as P, Trabalhador as T};

pub struct Chamada {
    pub chat: String,
    /// O papel da rota por onde a chamada veio (o motor confere com o do chat).
    pub papel: Papel,
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
        let papel = papel.rota();
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
        [mcp, papel, chat, t] if mcp == "mcp" && t == token => Papel::da_rota(papel).map(|p| (p, chat.clone())),
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
            "instructions": instrucoes(papel),
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": ferramentas(papel) }),
        "tools/call" => {
            let (envia, recebe) = bounded(1);
            enviar(Chamada {
                chat: chat.to_string(),
                papel,
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

/// Quem pode chamar cada ferramenta, na ordem do `tools/list`. A mesma tabela guarda o topo do
/// `tratar_ferramenta`: uma ferramenta fora dela não existe para o papel. A conversa do Vitral
/// antigo (o coordenador) não roda mais e não tem nenhuma.
pub const ACESSO: &[(&str, &[Papel])] = &[
    ("listar_chats", &[M, P, T]),
    ("mensagem_para_chat", &[M, P, T]),
    ("permissao", &[P, T]),
    ("estado", &[M]),
    ("ver_chat", &[M]),
    ("historico", &[M]),
    ("propor_divisao", &[M]),
    ("sugerir_projeto", &[M]),
    ("criar_tarefa", &[P]),
    ("ver_diff", &[M, P]),
    ("pedir_ajuste", &[M, P]),
    ("aprovar_tarefa", &[M, P]),
    ("cancelar_tarefa", &[M, P]),
    ("anotar_decisao", &[M, P, T]),
    ("lembrar", &[M]),
    ("esquecer", &[M]),
    ("informar_progresso", &[P, T]),
    ("entregar_tarefa", &[P]),
    ("perguntar_coordenador", &[P, T]),
    ("revisar_minhas_mudancas", &[P]),
];

pub fn pode(papel: Papel, nome: &str) -> bool {
    ACESSO.iter().any(|(n, papeis)| *n == nome && papeis.contains(&papel))
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

fn instrucoes(papel: Papel) -> &'static str {
    match papel {
        Papel::Mestre => "Ferramentas do Vitral para o Mestre: ver o estado de todos os chats e projetos, propor a divisão do trabalho, decidir as entregas e guardar a memória.",
        Papel::Projeto => "Ferramentas do Vitral para um chat de projeto: progresso, decisões do projeto, entrega dos pedidos do Mestre, perguntas, o parecer do revisor e os ajudantes.",
        Papel::Trabalhador => "Ferramentas do Vitral para um ajudante: progresso e perguntas ao chat que o criou.",
        Papel::Coordenador => "Conversa do Vitral antigo, só para leitura.",
    }
}

fn texto_obrigatorio() -> Value {
    json!({ "type": "string" })
}

fn definicao(nome: &str, papel: Papel) -> Option<Value> {
    let mestre = papel == Papel::Mestre;
    let numero = if mestre {
        json!({ "type": "integer", "description": "número do chat (o de listar_chats)" })
    } else {
        json!({ "type": "integer", "description": "número do ajudante (o #N de criar_tarefa e do relatório)" })
    };
    Some(match nome {
        "listar_chats" => ferramenta(
            "listar_chats",
            "Lista os chats do Vitral, os mais recentes primeiro: número, projeto, papel, título, se está \
             rodando, esperando você ou na fila, o pedido aberto (rodada, revisão, verificação, revisor) e \
             há quanto tempo mexeu.",
            json!({
                "projeto": { "type": "string", "description": "só os chats deste projeto (nome, #chave ou id). Opcional." },
                "ativos": { "type": "boolean", "description": "só os que estão rodando, esperando você ou na fila" },
                "com_pedido": { "type": "boolean", "description": "só os que têm um pedido aberto" },
            }),
            &[],
        ),
        "mensagem_para_chat" => ferramenta(
            "mensagem_para_chat",
            if mestre {
                "Manda uma mensagem a um chat (o número de listar_chats). Ela chega como uma nova mensagem \
                 assim que ele estiver livre. Não cria pedido nem passa pelo revisor: para pedir trabalho, \
                 use propor_divisao."
            } else {
                "Manda uma mensagem para outro chat (o número ou o id de listar_chats; \"mestre\" é o Mestre). \
                 Ela chega como uma nova mensagem para ele assim que ele estiver livre. Use para combinar \
                 coisas entre chats."
            },
            json!({
                "chat": { "type": "string", "description": "número ou id do chat de destino, ou \"mestre\"" },
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
        "estado" => ferramenta(
            "estado",
            "O que está vivo agora: o que roda, o que espera o usuário (permissões, divisões e sugestões \
             abertas, ciclos entre chats), o que está na fila, os pedidos abertos por projeto, o que parou \
             no meio, as pastas não encontradas, as pastas com mudanças sem commit e o uso do plano.",
            json!({}),
            &[],
        ),
        "ver_chat" => ferramenta(
            "ver_chat",
            "Mostra um chat: estado, pedido, progresso e as últimas mensagens (ferramentas só pelo resumo). \
             Não acorda o chat.",
            json!({
                "chat": { "type": "string", "description": "número do chat, id ou \"mestre\"" },
                "ultimas": { "type": "integer", "description": "quantas mensagens (padrão 10)" },
            }),
            &["chat"],
        ),
        "historico" => ferramenta(
            "historico",
            "O que aconteceu no período, uma linha por ocorrência, com a hora: pedidos, entregas, junções, \
             conflitos, falhas, cancelamentos, divisões e projetos criados.",
            json!({
                "horas": { "type": "integer", "description": "quantas horas para trás (padrão 24)" },
                "projeto": { "type": "string", "description": "só deste projeto. Opcional." },
            }),
            &[],
        ),
        "propor_divisao" => ferramenta(
            "propor_divisao",
            "Propõe como dividir o pedido do usuário em partes, cada uma para um chat de projeto. O Vitral \
             mostra a divisão ao usuário (ou começa direto, quando ele pediu assim ou citou o destino) e \
             responde na hora; depois de chamar, encerre sua vez com uma frase. Uma vez por mensagem do \
             usuário.",
            json!({
                "resumo": { "type": "string", "description": "uma frase com o que vai ser feito" },
                "partes": {
                    "type": "array",
                    "description": "até 8 partes independentes",
                    "items": {
                        "type": "object",
                        "properties": {
                            "projeto": { "type": "string", "description": "nome, #chave ou id do projeto; \"avulsos\" para sem projeto" },
                            "pasta": { "type": "string", "description": "a pasta do projeto, quando ele tem mais de uma" },
                            "chat": { "type": "string", "description": "número de um chat do projeto, em conversa, que já cuida do assunto. Opcional: sem ele, nasce um chat novo." },
                            "titulo": { "type": "string", "description": "curto, vira o título do chat" },
                            "pedido": { "type": "string", "description": "o pedido completo, com todo o contexto (o chat não vê esta conversa); caminhos relativos à pasta do projeto" },
                            "criterios": { "type": "array", "items": { "type": "string" }, "description": "critérios de aceitação verificáveis" },
                            "verificacao": { "type": "string", "description": "comando de shell que prova que deu certo (ex.: cargo test). Opcional." },
                            "modelo": { "type": "string", "description": "modelo do chat (fable, opus, sonnet, haiku). Opcional." },
                            "arquivos": { "type": "array", "items": { "type": "string" }, "description": "arquivos que a parte deve mexer. Opcional." },
                        },
                        "required": ["projeto", "titulo", "pedido", "criterios"],
                    },
                },
                "perguntas": { "type": "array", "items": { "type": "string" }, "description": "dúvidas para o usuário responder antes de começar. Opcional." },
            }),
            &["resumo", "partes"],
        ),
        "sugerir_projeto" => ferramenta(
            "sugerir_projeto",
            "Sugere ao usuário criar um projeto para um assunto que vai se repetir e não tem onde ficar. O \
             usuário decide; encerre sua vez depois de chamar. Sem pastas, o projeto ganha uma pasta própria.",
            json!({
                "nome": texto_obrigatorio(),
                "descricao": { "type": "string", "description": "o que vai neste projeto (você usa isto para encaminhar)" },
                "instrucoes": { "type": "string", "description": "instruções para os chats do projeto. Opcional." },
                "icone": { "type": "string", "description": "nome de um ícone Lucide (ex.: book-open). Opcional." },
                "cor": { "type": "string", "description": "#rrggbb. Opcional." },
                "pastas": { "type": "array", "items": { "type": "string" }, "description": "pastas que já existem para ligar. Opcional." },
                "motivo": { "type": "string", "description": "por que um projeto (uma frase para o usuário)" },
                "pedido_pendente": { "type": "string", "description": "o pedido que fica esperando o projeto existir. Opcional." },
            }),
            &["nome", "descricao", "motivo"],
        ),
        "criar_tarefa" => ferramenta(
            "criar_tarefa",
            "Cria um ajudante para uma parte independente de um pedido grande: um chat novo que trabalha numa \
             cópia da sua pasta (feita a partir do que ela tem agora, com o que você ainda não juntou). Ele \
             começa na hora; quando terminar, o relatório chega para você automaticamente como mensagem. Depois \
             de criar os ajudantes, encerre sua vez, e não mexa nos arquivos que eles estão mudando.",
            json!({
                "titulo": { "type": "string", "description": "curto, aparece na barra lateral" },
                "objetivo": { "type": "string", "description": "o que fazer, com todo o contexto que o ajudante precisa (ele não vê esta conversa); caminhos relativos à pasta" },
                "criterios": { "type": "array", "items": { "type": "string" }, "description": "critérios de aceitação verificáveis" },
                "verificacao": { "type": "string", "description": "comando de shell que o Vitral roda na cópia do ajudante quando ele termina (ex.: cargo test). Opcional." },
                "modelo": { "type": "string", "description": "modelo do ajudante (fable, opus, sonnet, haiku). Opcional." },
            }),
            &["titulo", "objetivo", "criterios"],
        ),
        "ver_diff" if mestre => ferramenta(
            "ver_diff",
            "O que o chat mudou para o pedido: sem `arquivo`, o --stat e a lista dos arquivos; com `arquivo`, \
             o diff completo dele. Peça por arquivo, só os que importam.",
            json!({
                "tarefa": numero,
                "arquivo": { "type": "string", "description": "caminho relativo à pasta do projeto. Opcional." },
            }),
            &["tarefa"],
        ),
        "ver_diff" => ferramenta(
            "ver_diff",
            "Mostra o que um ajudante seu mudou desde que a cópia dele saiu da sua pasta (git diff, incluindo \
             arquivos novos). Arquivos gerados (lockfiles, .min.js, .snap) ficam de fora numa linha, e um diff \
             grande vem resumido: o --stat, os arquivos que cabem inteiros e a lista dos que faltam, para você \
             pedir com `arquivos`.",
            json!({
                "tarefa": numero,
                "arquivos": { "type": "array", "items": { "type": "string" }, "description": "caminhos relativos à pasta; vem o diff completo só desses arquivos, mesmo os gerados. Opcional." },
            }),
            &["tarefa"],
        ),
        "pedir_ajuste" => ferramenta(
            "pedir_ajuste",
            if mestre {
                "Devolve o pedido ao chat: ele continua na mesma conversa e entrega de novo. O Vitral anexa \
                 sozinho os problemas altos e médios do revisor e o fim da saída da verificação quando eles \
                 apontaram algo (ou o que você puser em `incluir`; [] não anexa nada). No `feedback` escreva \
                 só a sua decisão: o que está errado, onde e o que se espera. Não copie o que vai anexado."
            } else {
                "Devolve o trabalho ao ajudante. Ele continua na mesma conversa e o novo relatório chega para \
                 você. O Vitral anexa sozinho os problemas altos e médios do revisor e o fim da saída da \
                 verificação quando eles apontaram algo (ou o que você puser em `incluir`; [] não anexa nada). \
                 No `feedback` escreva só a sua decisão: o que priorizar, o que ignorar e o que mais espera. \
                 Não copie o que vai anexado."
            },
            json!({
                "tarefa": numero,
                "feedback": texto_obrigatorio(),
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
        "aprovar_tarefa" => ferramenta(
            "aprovar_tarefa",
            if mestre {
                "Aprova a entrega: o Vitral junta as mudanças do chat na pasta do projeto (ou, sem cópia, só \
                 fecha o pedido). Se houver conflito, nada é juntado e a resposta lista os arquivos. Recusa \
                 antes do relatório desta rodada, com a verificação falhando ou com problema de gravidade \
                 alta do revisor; para aprovar mesmo assim, forcar=true com um motivo (fica no commit e o \
                 usuário vê)."
            } else {
                "Aprova o ajudante: o Vitral guarda num commit o que a sua pasta tem, faz o commit na cópia \
                 dele e junta na sua pasta; a cópia dele sai e o chat dele vai para as arquivadas. Se houver \
                 conflito, nada é juntado e a resposta lista os arquivos. Recusa se o ajudante ainda não mandou \
                 o relatório desta rodada, se a verificação falhou ou se o revisor apontou problema de \
                 gravidade alta; para aprovar mesmo assim, forcar=true com um motivo (fica no commit e é \
                 mostrado ao usuário)."
            },
            json!({
                "tarefa": numero,
                "nota": { "type": "string", "description": "mensagem do commit (opcional)" },
                "forcar": { "type": "boolean", "description": "aprovar apesar da verificação falhando ou de problema grave do revisor (exige motivo)" },
                "motivo": { "type": "string", "description": "por que a falha não vem desta entrega (obrigatório com forcar)" },
            }),
            &["tarefa"],
        ),
        "cancelar_tarefa" => ferramenta(
            "cancelar_tarefa",
            if mestre {
                "Cancela o seu pedido a um chat: ele para, o pedido sai da fila e o chat volta a conversar com \
                 o usuário. A conversa e a cópia continuam."
            } else {
                "Cancela o ajudante e o para. A cópia dele fica guardada até o usuário limpar; nada dela entra \
                 na sua pasta."
            },
            json!({
                "tarefa": numero,
                "motivo": { "type": "string" },
            }),
            &["tarefa"],
        ),
        "anotar_decisao" if mestre => ferramenta(
            "anotar_decisao",
            "Registra uma decisão ou convenção de um projeto. As notas entram no contexto de todos os chats \
             dele, já no próximo turno.",
            json!({
                "projeto": { "type": "string", "description": "nome, #chave ou id do projeto" },
                "texto": texto_obrigatorio(),
            }),
            &["projeto", "texto"],
        ),
        "anotar_decisao" => ferramenta(
            "anotar_decisao",
            "Registra uma decisão ou convenção do projeto. As notas entram no contexto de todos os chats \
             deste projeto, já no próximo turno.",
            json!({ "texto": texto_obrigatorio() }),
            &["texto"],
        ),
        "lembrar" => ferramenta(
            "lembrar",
            "Guarda na sua memória uma preferência ou um fato duradouro dito pelo usuário. Num turno que não é \
             só do usuário, ele confirma antes.",
            json!({ "texto": texto_obrigatorio() }),
            &["texto"],
        ),
        "esquecer" => ferramenta(
            "esquecer",
            "Tira da sua memória a linha que contém o texto (uma só).",
            json!({ "texto": texto_obrigatorio() }),
            &["texto"],
        ),
        "informar_progresso" => ferramenta(
            "informar_progresso",
            "Atualiza a linha de progresso deste chat na interface (uma frase curta).",
            json!({ "texto": texto_obrigatorio() }),
            &["texto"],
        ),
        "entregar_tarefa" => ferramenta(
            "entregar_tarefa",
            "Entrega o pedido do Mestre (a mensagem [de: Mestre]): chame depois de fazer e verificar. O Vitral \
             roda a verificação e o revisor e manda ao Mestre. Encerre sua vez depois de chamar.",
            json!({
                "resumo": { "type": "string", "description": "o que mudou (arquivos) e o que ficou de fora" },
                "como_verificou": { "type": "string", "description": "o que você rodou e conferiu" },
                "criterios": {
                    "type": "array",
                    "description": "o estado de cada critério, pelo número dele no pedido",
                    "items": {
                        "type": "object",
                        "properties": {
                            "indice": { "type": "integer", "description": "o número do critério (1, 2…)" },
                            "estado": { "type": "string", "enum": ["feito", "parcial", "nao"] },
                            "evidencia": { "type": "string" },
                        },
                        "required": ["indice", "estado"],
                    },
                },
                "pendencias": { "type": "string", "description": "o que não deu para fazer ou ficou em dúvida. Opcional." },
            }),
            &["resumo", "como_verificou"],
        ),
        "perguntar_coordenador" => ferramenta(
            "perguntar_coordenador",
            "Manda uma pergunta a quem fez o pedido (o Mestre, ou o chat que criou este ajudante). A resposta \
             chega como nova mensagem; se a dúvida bloquear o trabalho, encerre sua vez depois de perguntar.",
            json!({ "pergunta": texto_obrigatorio() }),
            &["pergunta"],
        ),
        "revisar_minhas_mudancas" => ferramenta(
            "revisar_minhas_mudancas",
            "Pede a um revisor independente (uma sessão nova, só de leitura) que confira o que a sua \
             cópia mudou e ainda não foi juntado, e roda a verificação do pedido quando há uma. Espera \
             a vez (uma conferência por vez no Vitral) e devolve o veredito com os problemas, com \
             arquivo e linha. Não muda nada no chat.",
            json!({}),
            &[],
        ),
        _ => return None,
    })
}

fn ferramentas(papel: Papel) -> Vec<Value> {
    ACESSO
        .iter()
        .filter(|(_, papeis)| papeis.contains(&papel))
        .filter_map(|(nome, _)| definicao(nome, papel))
        .collect()
}

#[cfg(test)]
mod testes {
    use super::*;

    fn nomes(papel: Papel) -> Vec<String> {
        ferramentas(papel).iter().filter_map(|f| f["name"].as_str().map(str::to_string)).collect()
    }

    #[test]
    fn o_chat_de_projeto_tem_as_ferramentas_dele() {
        assert_eq!(
            nomes(Papel::Projeto),
            [
                "listar_chats", "mensagem_para_chat", "permissao", "criar_tarefa", "ver_diff", "pedir_ajuste",
                "aprovar_tarefa", "cancelar_tarefa", "anotar_decisao", "informar_progresso", "entregar_tarefa",
                "perguntar_coordenador", "revisar_minhas_mudancas"
            ]
        );
        // O ajudante não cria ajudante nem decide os dos outros.
        assert_eq!(
            nomes(Papel::Trabalhador),
            ["listar_chats", "mensagem_para_chat", "permissao", "anotar_decisao", "informar_progresso", "perguntar_coordenador"]
        );
        // A conversa do Vitral antigo não roda: nenhuma ferramenta.
        assert!(nomes(Papel::Coordenador).is_empty());
        let diff = ferramentas(Papel::Projeto).into_iter().find(|f| f["name"] == "ver_diff").unwrap();
        assert!(diff["inputSchema"]["properties"]["arquivos"].is_object());
    }

    #[test]
    fn cada_papel_so_ve_e_so_chama_o_que_a_tabela_deixa() {
        let mestre = nomes(Papel::Mestre);
        for nome in [
            "listar_chats", "mensagem_para_chat", "estado", "ver_chat", "historico", "propor_divisao",
            "sugerir_projeto", "ver_diff", "pedir_ajuste", "aprovar_tarefa", "cancelar_tarefa", "anotar_decisao",
            "lembrar", "esquecer",
        ] {
            assert!(mestre.contains(&nome.to_string()), "{nome}");
            assert!(pode(Papel::Mestre, nome), "{nome}");
        }
        for nome in ["permissao", "criar_tarefa", "listar_tarefas", "entregar_tarefa", "informar_progresso", "revisar_minhas_mudancas"] {
            assert!(!pode(Papel::Mestre, nome), "{nome}");
        }
        assert!(!pode(Papel::Projeto, "propor_divisao") && pode(Papel::Projeto, "aprovar_tarefa"));
        assert!(!pode(Papel::Trabalhador, "entregar_tarefa") && pode(Papel::Trabalhador, "anotar_decisao"));
        assert!(!pode(Papel::Trabalhador, "criar_tarefa") && !pode(Papel::Trabalhador, "aprovar_tarefa"));
        assert!(ACESSO.iter().all(|(_, papeis)| !papeis.contains(&Papel::Coordenador)));
        assert!(!pode(Papel::Coordenador, "criar_tarefa") && !pode(Papel::Coordenador, "listar_tarefas"));
        assert!(!pode(Papel::Mestre, "nao_existe"));
        // Toda linha da tabela tem a definição, em todo papel que pode chamá-la.
        for (nome, papeis) in ACESSO {
            for papel in *papeis {
                assert!(definicao(nome, *papel).is_some(), "{nome}");
            }
        }
        let diff = ferramentas(Papel::Mestre).into_iter().find(|f| f["name"] == "ver_diff").unwrap();
        assert!(diff["inputSchema"]["properties"]["arquivo"].is_object());
        let anotar = ferramentas(Papel::Mestre).into_iter().find(|f| f["name"] == "anotar_decisao").unwrap();
        assert_eq!(anotar["inputSchema"]["required"], json!(["projeto", "texto"]));
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
