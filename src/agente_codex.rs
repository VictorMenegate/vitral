//! O Codex como agente de um chat, pelo `codex app-server`: JSON-RPC com entrada aberta durante
//! o turno, `turn/steer` para novas mensagens e diffs em `fileChange` (`servidor.rs`). A tradução
//! entrega os mesmos eventos do Claude Code, para a conversa não depender de quem respondeu.
//! O formato do `exec` abaixo continua como base do tradutor e dos testes de compatibilidade.
//!
//! Conferido no Codex 0.169.0 (13/11/2026), com o login do ChatGPT:
//! - `codex exec - --json --skip-git-repo-check -C <pasta> -c sandbox_mode="…"` lê o pedido da
//!   entrada (o `-`) e escreve uma linha JSON por evento: `thread.started{thread_id}`,
//!   `turn.started`, `item.started|completed{item}`, `turn.completed{usage}`; no erro, `error{message}`
//!   e `turn.failed{error{message}}`, com código de saída 1;
//! - `item.type`: `agent_message{text}`, `command_execution{command, aggregated_output, exit_code}`,
//!   `file_change{changes[{path, kind}]}` (sem o diff) e `error{message}` (aviso, o turno segue);
//! - `codex exec resume <thread_id> - --json …` continua a conversa (não aceita `-C` nem `-s`: a
//!   pasta é a do processo e o sandbox vai por `-c`);
//! - o `usage.input_tokens` do `turn.completed` soma as chamadas do turno, então não é o tamanho do
//!   contexto e não vira `Evento::Contexto`;
//! - no `exec` ninguém pergunta nada: o que o Codex pode fazer é o sandbox, e não há pedido de
//!   permissão no meio do turno.

use std::{
    collections::HashSet,
    io::Read as _,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::Duration,
};

use serde_json::{Value, json};

use crate::{
    claude::{Evento, Opcoes},
    imagens_recebidas, processos,
};

pub mod servidor;

/// Configurações só deste processo; o modelo, a pasta e a retomada vão no pedido JSON-RPC.
pub fn argumentos_servidor(opcoes: &Opcoes) -> Vec<String> {
    let antigos = argumentos(opcoes, &[]);
    std::iter::once("app-server".to_string()).chain(
        antigos.windows(2).filter(|p| p[0] == "-c").flat_map(|p| p.to_vec())
    ).collect()
}

pub fn parametros_servidor(opcoes: &Opcoes) -> Value {
    let mut parametros = json!({
        "cwd":opcoes.cwd, "model":opcoes.modelo, "sandbox":sandbox_de(&opcoes.permissao),
        "approvalPolicy":aprovacao_de(&opcoes.permissao), "developerInstructions":opcoes.prompt_sistema,
    });
    if opcoes.retomar { parametros["threadId"] = json!(opcoes.sessao); }
    parametros
}

/// Como o turno abre a conversa no app-server, e com quê: `thread/start` (nova), `thread/resume`
/// (a do chat) ou `thread/fork` (uma cópia de outra, até antes de um turno).
pub fn abertura(opcoes: &Opcoes) -> (&'static str, Value) {
    let mut parametros = parametros_servidor(opcoes);
    match &opcoes.bifurcar {
        Some(origem) if !opcoes.retomar => {
            parametros["threadId"] = json!(origem);
            if let Some(turno) = &opcoes.cortar_em {
                parametros["beforeTurnId"] = json!(turno);
            }
            // A resposta não precisa trazer o histórico inteiro (o Codex avisa que encher a
            // resposta com ele ficou para trás).
            parametros["excludeTurns"] = json!(true);
            ("thread/fork", parametros)
        }
        _ if parametros.get("threadId").is_some() => ("thread/resume", parametros),
        _ => ("thread/start", parametros),
    }
}

/// O nome com que se chama o Codex quando ele não foi achado (o PATH resolve).
const NOME_DO_CODEX: &str = if cfg!(windows) { "codex.exe" } else { "codex" };

/// Os nomes do Codex neste sistema. No Windows, o `codex.exe` do instalador oficial (`install.ps1`)
/// e o `codex.cmd` do npm: só o `.cmd` era procurado, e o Codex instalado pelo instalador oficial
/// nunca aparecia (dono, 24/11/2026: "no windows não tem o gemini e o codex").
fn nomes_do_codex(windows: bool) -> &'static [&'static str] {
    if windows { &["codex.exe", "codex.cmd"] } else { &["codex"] }
}

/// Onde os instaladores põem o Codex, além do PATH: `~/.local/bin` (o instalador do Linux) e, no
/// Windows, `%LOCALAPPDATA%\Programs\OpenAI\Codex\bin` (o `install.ps1`) e `%APPDATA%\npm`.
fn pastas_do_codex(windows: bool, home: &Path, local: Option<PathBuf>, roaming: Option<PathBuf>) -> Vec<PathBuf> {
    let mut pastas = vec![home.join(".local").join("bin")];
    if windows {
        pastas.extend(local.map(|l| l.join("Programs").join("OpenAI").join("Codex").join("bin")));
        pastas.extend(roaming.map(|r| r.join("npm")));
    }
    pastas
}

/// Quanto se espera o `codex debug models` responder.
const PRAZO_DOS_MODELOS: Duration = Duration::from_secs(20);

/// O `codex` do PATH ou o de `~/.local/bin` (apps abertos pela dock nem sempre têm esse PATH).
pub fn binario() -> PathBuf {
    binario_em(std::env::var_os("PATH"), dirs::home_dir().unwrap_or_default())
}

fn binario_em(path: Option<std::ffi::OsString>, home: PathBuf) -> PathBuf {
    let windows = cfg!(windows);
    let pastas = pastas_do_codex(windows, &home, std::env::var_os("LOCALAPPDATA").map(PathBuf::from), std::env::var_os("APPDATA").map(PathBuf::from));
    crate::sistema::achar_programa(path.as_deref(), nomes_do_codex(windows), &pastas).unwrap_or_else(|| PathBuf::from(NOME_DO_CODEX))
}

/// Há um Codex instalado neste computador.
pub fn instalado() -> bool {
    binario().is_file()
}

/// O sandbox do Codex que corresponde a cada modo de permissão do Vitral. Planejar só lê; o modo
/// sem perguntas roda sem sandbox; os outros escrevem só na pasta do chat (sem rede).
pub fn sandbox_de(permissao: &str) -> &'static str {
    match permissao {
        "plan" => "read-only",
        "bypassPermissions" => "danger-full-access",
        _ => "workspace-write",
    }
}

/// Quando o Codex pergunta, em cada modo de permissão do Vitral (o `approvalPolicy` do
/// app-server). Manual e Aceitar edições: `untrusted`, que pergunta antes de toda edição e de todo
/// comando que não seja só de leitura (no Aceitar edições, o Vitral responde sozinho às edições
/// dentro da pasta: `modelo::modo_permite`). Planejar e Automático: `on-request`, em que o
/// Codex trabalha dentro do sandbox sem perguntar e pede quando precisa sair dele (escrever no
/// Planejar, a rede ou fora da pasta no Automático). Sem perguntar: `never`.
pub fn aprovacao_de(permissao: &str) -> &'static str {
    match permissao {
        "bypassPermissions" => "never",
        "plan" | "auto" => "on-request",
        _ => "untrusted",
    }
}

/// O que cada modo de permissão quer dizer num chat do Codex (para a dica do seletor).
pub fn dica_da_permissao(permissao: &str) -> &'static str {
    match permissao {
        "plan" => "O Codex só lê, e pede permissão quando precisa escrever ou usar a rede.",
        "bypassPermissions" => "O Codex roda sem sandbox, com acesso ao computador todo e à rede.",
        "auto" => "O Codex trabalha na pasta do chat, sem rede, e só pede permissão para sair dela.",
        "acceptEdits" => "O Codex edita a pasta do chat sem perguntar e pede permissão antes de cada comando que não só lê.",
        _ => "O Codex pede permissão antes de cada edição e de cada comando que não só lê.",
    }
}

/// Os argumentos do `codex` para um turno (sem o binário). O pedido vai pela entrada padrão.
pub fn argumentos(opcoes: &Opcoes, imagens: &[PathBuf]) -> Vec<String> {
    let mut args: Vec<String> = vec!["exec".into()];
    if opcoes.retomar {
        args.extend(["resume".into(), opcoes.sessao.clone()]);
    }
    args.extend(["-".into(), "--json".into(), "--skip-git-repo-check".into()]);
    if !opcoes.retomar {
        args.extend(["-C".into(), opcoes.cwd.to_string_lossy().into_owned()]);
    }
    if let Some(modelo) = opcoes.modelo.as_deref().filter(|m| !m.trim().is_empty()) {
        args.extend(["-m".into(), modelo.to_string()]);
    }
    args.extend(["-c".into(), format!("sandbox_mode=\"{}\"", sandbox_de(&opcoes.permissao))]);
    if let Some(esforco) = opcoes.esforco.as_deref().filter(|e| ESFORCOS.contains(e)) {
        args.extend(["-c".into(), format!("model_reasoning_effort=\"{esforco}\"")]);
    }
    if !opcoes.prompt_sistema.is_empty() {
        args.extend(["-c".into(), format!("developer_instructions={}", json!(opcoes.prompt_sistema))]);
    }
    if let Some((endereco, _)) = opcoes.mcp_da_equipe.as_deref().and_then(crate::memoria_equipe::conexao_do_arquivo) {
        // A chave vai pelo ambiente, nunca pela linha de comando nem pelo config.toml da pessoa.
        args.extend([
            "-c".into(),
            format!("mcp_servers.ai-memory={{url={},bearer_token_env_var=\"VITRAL_MEMORIA_EQUIPE_TOKEN\"}}", json!(endereco)),
        ]);
    }
    // Por último: o `-i` do `exec` aceita vários arquivos e engoliria o que viesse depois.
    for imagem in imagens {
        args.extend(["-i".into(), imagem.to_string_lossy().into_owned()]);
    }
    args
}

/// Os esforços que o Vitral repassa ao Codex como estão (os mesmos nomes do Claude Code).
const ESFORCOS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// Traduz a saída de um `codex exec --json`, linha a linha. Guarda o pouco que atravessa linhas:
/// os itens já abertos (uma ferramenta só nasce uma vez), a última fala e o último erro.
#[derive(Default)]
pub struct Leitor {
    abertos: HashSet<String>,
    ultima_fala: String,
    erro: String,
}

impl Leitor {
    pub fn linha(&mut self, valor: &Value) -> Vec<Evento> {
        match valor["type"].as_str().unwrap_or_default() {
            "thread.started" => valor["thread_id"].as_str().map(|id| vec![Evento::Sessao(id.to_string())]).unwrap_or_default(),
            "item.started" => self.item(&valor["item"], false),
            "item.completed" => self.item(&valor["item"], true),
            // Os tokens do turno vão antes do fim (o Codex não diz custo).
            "turn.completed" => crate::motor::tokens_do_codex(valor).map(Evento::Gasto).into_iter().chain([Evento::Fim {
                erro: false,
                texto: std::mem::take(&mut self.ultima_fala),
                custo_usd: 0.,
                janela: None,
            }]).collect(),
            "error" => {
                self.erro = valor["message"].as_str().unwrap_or_default().to_string();
                Vec::new()
            }
            "turn.failed" => {
                let dito = valor["error"]["message"].as_str().unwrap_or_default();
                let texto = if dito.is_empty() { std::mem::take(&mut self.erro) } else { dito.to_string() };
                vec![Evento::Fim { erro: true, texto, custo_usd: 0., janela: None }]
            }
            _ => Vec::new(),
        }
    }

    fn item(&mut self, item: &Value, terminou: bool) -> Vec<Evento> {
        let id = item["id"].as_str().unwrap_or_default().to_string();
        let texto = |chave: &str| item[chave].as_str().unwrap_or_default().to_string();
        let falhou = item["status"] == "failed";
        // A chamada deste item já nasceu, numa linha de antes (o começo dele).
        let ja_aberta = self.abertos.contains(&id);
        // A chamada (uma vez só, no começo ou já no fim) e o resultado quando o item termina.
        let mut ferramenta = |nome: &str, entrada: Value, resultado: String, erro: bool| {
            let mut eventos = Vec::new();
            if self.abertos.insert(id.clone()) {
                eventos.push(Evento::Ferramenta { id: id.clone(), nome: nome.to_string(), entrada });
            }
            if terminou {
                eventos.push(Evento::ResultadoFerramenta { id: id.clone(), conteudo: resultado, erro, no_fundo: false });
            }
            eventos
        };
        match item["type"].as_str().unwrap_or_default() {
            "agent_message" if terminou => {
                self.ultima_fala = texto("text");
                vec![Evento::Texto(texto("text"))]
            }
            "reasoning" if terminou => vec![Evento::Pensamento(texto("text"))],
            "reasoning" => vec![Evento::Pensando],
            "command_execution" => {
                let saiu_com_erro = item["exit_code"].as_i64().is_some_and(|c| c != 0);
                ferramenta("Bash", json!({ "command": comando_limpo(&texto("command")) }), texto("aggregated_output"), saiu_com_erro || falhou)
            }
            // Uma linha por arquivo, com o diff daquele arquivo. Só no fim do item (dono, 16/11/2026:
            // "ver a edição"): o app-server pode abrir o item antes de o patch chegar inteiro (o
            // `item/fileChange/patchUpdated` vem no meio), e a linha nasceria com o diff pela metade.
            "file_change" if terminou => {
                let mudancas = item["changes"].as_array().cloned().unwrap_or_default();
                let mut eventos = Vec::new();
                for (i, mudanca) in mudancas.iter().enumerate() {
                    let id = if mudancas.len() == 1 { id.clone() } else { format!("{id}:{i}") };
                    let (nome, entrada) = entrada_da_mudanca(mudanca);
                    if self.abertos.insert(id.clone()) {
                        eventos.push(Evento::Ferramenta { id: id.clone(), nome: nome.into(), entrada });
                    }
                    eventos.push(Evento::ResultadoFerramenta { id, conteudo: resumo_das_mudancas(std::slice::from_ref(mudanca)), erro: falhou, no_fundo: false });
                }
                eventos
            }
            "mcp_tool_call" => {
                let nome = format!("mcp__{}__{}", texto("server"), texto("tool"));
                // O resultado traz os blocos do MCP (`McpToolCallResult { content, … }`): o texto
                // vira o resultado e a imagem (o print do navegador, em `data` e `mimeType`) sai à
                // parte, para o Vitral guardar. Antes ia o JSON inteiro, com o base64 dentro.
                let (resultado, imagens) = match &item["result"] {
                    Value::Null => (item["error"]["message"].as_str().unwrap_or_default().to_string(), Vec::new()),
                    Value::String(s) => (s.clone(), Vec::new()),
                    resultado if resultado["content"].is_array() => {
                        let (texto, imagens) = imagens_recebidas::partes_do_resultado(&resultado["content"]);
                        // Só o dado estruturado, sem bloco nenhum de texto: ele é o resultado.
                        match &resultado["structuredContent"] {
                            estruturado if texto.is_empty() && imagens.is_empty() && !estruturado.is_null() => (estruturado.to_string(), imagens),
                            _ => (texto, imagens),
                        }
                    }
                    outro => (outro.to_string(), Vec::new()),
                };
                let mut eventos = ferramenta(&nome, item["arguments"].clone(), resultado, falhou);
                if terminou && !imagens.is_empty() {
                    eventos.push(Evento::ImagensDoResultado { id: id.clone(), imagens });
                }
                eventos
            }
            // O `view_image` (conferido no app-server do 0.169.0, 23/11/2026): o item traz o
            // caminho da imagem no disco, sem base64. É a leitura de um arquivo, que a conversa
            // já mostra com a miniatura.
            "imageView" | "image_view" => ferramenta("Read", json!({ "file_path": texto("path") }), String::new(), falhou),
            // A imagem gerada (pelo schema do app-server do 0.169.0; sem turno de verdade para
            // conferir): `savedPath` é o arquivo que o Codex gravou e `result`, o base64. A
            // chamada que nasce aqui aponta para o arquivo; a que já nasceu no começo do item
            // (sem o caminho) ganha a imagem à parte, do base64 ou do arquivo.
            "imageGeneration" | "image_generation" => {
                let campo = |a: &str, b: &str| item[a].as_str().or(item[b].as_str()).filter(|t| !t.is_empty());
                let gravada = campo("savedPath", "saved_path");
                let mut entrada = json!({ "prompt": campo("revisedPrompt", "revised_prompt") });
                let recebida = match (gravada, item["result"].as_str().filter(|d| !d.is_empty())) {
                    _ if !terminou => None,
                    (Some(caminho), _) if !ja_aberta => {
                        entrada["file_path"] = json!(caminho);
                        None
                    }
                    (_, Some(dados)) => Some(imagens_recebidas::de_base64("image/png", dados)),
                    (Some(caminho), None) => imagens_recebidas::do_arquivo(Path::new(caminho)).map(Ok),
                    (None, None) => None,
                };
                let (conteudo, imagens) = match recebida {
                    Some(Ok(imagem)) => (String::new(), vec![imagem]),
                    Some(Err(marca)) => (marca, Vec::new()),
                    None => (String::new(), Vec::new()),
                };
                let mut eventos = ferramenta("ImageGeneration", entrada, conteudo, falhou || item["failure"].is_object());
                if !imagens.is_empty() {
                    eventos.push(Evento::ImagensDoResultado { id: id.clone(), imagens });
                }
                eventos
            }
            "web_search" => ferramenta("WebSearch", json!({ "query": texto("query") }), String::new(), falhou),
            _ => Vec::new(),
        }
    }
}

/// O comando como o modelo o escreveu: o Codex o entrega embrulhado no shell
/// (`/bin/bash -lc "printf 'oi' > a.txt"`).
fn comando_limpo(comando: &str) -> String {
    let Some(resto) = ["/bin/bash -lc ", "bash -lc ", "/bin/sh -c ", "/bin/zsh -lc "].iter().find_map(|p| comando.strip_prefix(p)) else {
        return comando.to_string();
    };
    let resto = resto.trim();
    let sem_aspas = |aspa: char| resto.strip_prefix(aspa).and_then(|r| r.strip_suffix(aspa));
    match (sem_aspas('"'), sem_aspas('\'')) {
        (Some(dentro), _) => dentro.replace("\\\"", "\"").replace("\\$", "$").replace("\\\\", "\\"),
        (_, Some(dentro)) => dentro.replace("'\\''", "'"),
        _ => resto.to_string(),
    }
}

/// O tipo de uma mudança: `"add"` no `exec` e `{"type":"add"}` no app-server.
fn tipo_da_mudanca(mudanca: &Value) -> &str {
    mudanca["kind"].as_str().or_else(|| mudanca["kind"]["type"].as_str()).unwrap_or("update")
}

/// A chamada que a conversa guarda para uma mudança do Codex, no formato das do Claude Code (o
/// `detalhe_ferramenta` e o `mudanca_da_ferramenta` servem às duas). Conferido no schema do
/// app-server do Codex 0.169.0 (`FileUpdateChange { path, kind, diff }`): no `update`, o `diff` é
/// o diff unificado do arquivo (com "Moved to: …" no fim quando ele muda de nome); no `add`, o
/// conteúdo do arquivo novo; no `delete`, o que o arquivo tinha. O arquivo novo vira um Write com
/// o conteúdo (antes as linhas sem "+" contavam como nada); o apagado, um diff só de saídas.
fn entrada_da_mudanca(mudanca: &Value) -> (&'static str, Value) {
    let caminho = mudanca["path"].clone();
    let diff = mudanca["diff"].as_str().unwrap_or_default();
    let unificado = diff.lines().any(|l| l.starts_with("@@"));
    match tipo_da_mudanca(mudanca) {
        "add" => {
            let conteudo = if unificado {
                diff.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")).map(|l| &l[1..]).collect::<Vec<_>>().join("\n")
            } else {
                diff.to_string()
            };
            ("Write", json!({ "file_path": caminho, "content": conteudo }))
        }
        "delete" if !unificado && !diff.is_empty() => {
            let linhas: Vec<&str> = diff.lines().collect();
            let mut saidas = format!("@@ -1,{} +0,0 @@", linhas.len());
            for linha in linhas {
                saidas.push_str("\n-");
                saidas.push_str(linha);
            }
            ("Edit", json!({ "file_path": caminho, "diff": saidas }))
        }
        _ => ("Edit", json!({ "file_path": caminho, "diff": diff })),
    }
}

/// O que mudou, um arquivo por linha.
fn resumo_das_mudancas(mudancas: &[Value]) -> String {
    mudancas
        .iter()
        .map(|m| {
            let verbo = match tipo_da_mudanca(m) {
                "add" => "criou",
                "delete" => "apagou",
                _ => "mudou",
            };
            format!("{verbo} {}", m["path"].as_str().unwrap_or_default())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Um modelo que o Codex instalado oferece.
#[derive(Clone, Debug, PartialEq)]
pub struct ModeloDoCodex {
    /// O que vai no `-m`.
    pub slug: String,
    /// O nome que o Codex mostra ("GPT-6.1-Sol").
    pub nome: String,
    pub descricao: String,
    /// O esforço de raciocínio que o modelo usa sem ninguém pedir (`default_reasoning_level`).
    pub esforco: String,
}

/// Os modelos que o Codex ofereceu da última vez que respondeu (vazio até lá).
static MODELOS: Mutex<Vec<ModeloDoCodex>> = Mutex::new(Vec::new());

pub fn modelos() -> Vec<ModeloDoCodex> {
    MODELOS.lock().map(|m| m.clone()).unwrap_or_default()
}

/// O nome de mostrar de um modelo do Codex; sem a lista, o próprio slug.
pub fn nome_do_modelo(slug: &str) -> String {
    modelos().into_iter().find(|m| m.slug == slug).map_or_else(|| slug.to_string(), |m| m.nome)
}

/// O esforço que este modelo do Codex usa no automático ("low"), pelo catálogo.
pub fn esforco_padrao(slug: &str) -> Option<String> {
    modelos().into_iter().find(|m| m.slug == slug).map(|m| m.esforco).filter(|e| !e.is_empty())
}

/// Pergunta ao Codex instalado quais modelos ele oferece (`codex debug models`, sem gastar nada do
/// plano) e guarda. Bloqueia até a resposta ou o prazo: chamar em segundo plano. Devolve `true` se
/// a lista mudou.
pub fn atualizar_modelos(pasta: &Path) -> bool {
    if !instalado() {
        return false;
    }
    let Some(novos) = perguntar_modelos(pasta).filter(|m| !m.is_empty()) else {
        return false;
    };
    let Ok(mut guardados) = MODELOS.lock() else {
        return false;
    };
    if *guardados == novos {
        return false;
    }
    *guardados = novos;
    true
}

fn perguntar_modelos(pasta: &Path) -> Option<Vec<ModeloDoCodex>> {
    std::fs::create_dir_all(pasta).ok()?;
    let mut comando = processos::comando_leve(binario(), "modelos-codex");
    comando.args(["debug", "models"]).current_dir(pasta).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let (mut filho, escopo) = processos::abrir(comando).ok()?;
    let mut saida = filho.stdout.take()?;
    // A leitura vai noutra thread para o prazo valer mesmo se o processo não disser nada.
    let (envia, recebe) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut tudo = Vec::new();
        let _ = saida.read_to_end(&mut tudo);
        let _ = envia.send(tudo);
    });
    let tudo = recebe.recv_timeout(PRAZO_DOS_MODELOS).ok();
    escopo.matar();
    let _ = filho.wait();
    Some(modelos_de(&serde_json::from_slice(&tudo?).ok()?))
}

/// Os modelos de mostrar do catálogo (`visibility: "list"`), na ordem em que o Codex os dá.
fn modelos_de(catalogo: &Value) -> Vec<ModeloDoCodex> {
    let texto = |m: &Value, chave: &str| m[chave].as_str().unwrap_or_default().trim().to_string();
    catalogo["models"]
        .as_array()
        .map(|lista| {
            lista
                .iter()
                .filter(|m| m["visibility"] == "list" && !texto(m, "slug").is_empty())
                .map(|m| {
                    let slug = texto(m, "slug");
                    let nome = Some(texto(m, "display_name")).filter(|n| !n.is_empty()).unwrap_or_else(|| slug.clone());
                    ModeloDoCodex { slug, nome, descricao: texto(m, "description"), esforco: texto(m, "default_reasoning_level") }
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod testes {
    use super::*;

    fn opcoes(retomar: bool, permissao: &str) -> Opcoes {
        Opcoes {
            agente: crate::modelo::Agente::Codex,
            cwd: PathBuf::from("/casa/loja"),
            sessao: "01a1d5d6".into(),
            retomar,
            bifurcar: None,
            cortar_em: None,
            modelo: None,
            permissao: permissao.into(),
            prompt_sistema: String::new(),
            prompt_arquivo: None,
            ferramentas: None,
            url_mcp: String::new(),
            bloqueadas: Vec::new(),
            so_mcp_vitral: true,
            esforco: None,
            ultracode: false,
            pasta_conta: None,
            memoria_nativa: false,
            sugestoes: false,
            mcp_da_equipe: None,
            eventos_de_hook: false,
            raciocinio_resumido: false,
        }
    }

    #[test]
    fn argumentos_do_primeiro_turno_e_da_retomada() {
        let novo = argumentos(&opcoes(false, "acceptEdits"), &[]);
        assert_eq!(novo, ["exec", "-", "--json", "--skip-git-repo-check", "-C", "/casa/loja", "-c", "sandbox_mode=\"workspace-write\""]);
        let mut o = opcoes(true, "bypassPermissions");
        o.modelo = Some("gpt-6.1-sol".into());
        o.esforco = Some("high".into());
        let retomado = argumentos(&o, &[PathBuf::from("/tmp/a.png")]);
        assert_eq!(
            retomado,
            [
                "exec",
                "resume",
                "01a1d5d6",
                "-",
                "--json",
                "--skip-git-repo-check",
                "-m",
                "gpt-6.1-sol",
                "-c",
                "sandbox_mode=\"danger-full-access\"",
                "-c",
                "model_reasoning_effort=\"high\"",
                "-i",
                "/tmp/a.png"
            ]
        );
        assert_eq!(sandbox_de("plan"), "read-only");
        // Esforço que o Codex não conhece pelo nome fica de fora (vale o padrão do modelo).
        o.esforco = Some("ultracode".into());
        assert!(!argumentos(&o, &[]).iter().any(|a| a.contains("reasoning_effort")));
    }

    /// Cada modo do Vitral é um par sandbox + política de aprovação do Codex. Os que pedem
    /// permissão (Manual, Aceitar edições, Planejar e Automático) deixam o Codex perguntar no meio
    /// do turno; o "Sem perguntar" não deixa.
    #[test]
    fn cada_modo_vira_um_sandbox_e_uma_politica_de_aprovacao() {
        for (modo, sandbox, politica) in [
            ("manual", "workspace-write", "untrusted"),
            ("acceptEdits", "workspace-write", "untrusted"),
            ("plan", "read-only", "on-request"),
            ("auto", "workspace-write", "on-request"),
            ("bypassPermissions", "danger-full-access", "never"),
        ] {
            let parametros = parametros_servidor(&opcoes(false, modo));
            assert_eq!((parametros["sandbox"].as_str(), parametros["approvalPolicy"].as_str()), (Some(sandbox), Some(politica)), "{modo}");
        }
    }

    /// A cópia da conversa (corrigir uma mensagem, continuar daqui em outro chat) abre com o
    /// `thread/fork` da conversa de origem, cortada antes do turno da mensagem (`beforeTurnId`,
    /// conferido no 0.169.0 com a API experimental). O fork não herda a configuração: a pasta, o
    /// modelo, as instruções, o sandbox e a política vão de novo.
    #[test]
    fn a_copia_da_conversa_abre_com_o_fork_e_a_configuracao_do_chat() {
        let mut o = opcoes(false, "manual");
        (o.bifurcar, o.cortar_em) = (Some("origem".into()), Some("turno-3".into()));
        o.modelo = Some("gpt-6.2-sol".into());
        o.prompt_sistema = "Responda em português.".into();
        let (metodo, p) = abertura(&o);
        assert_eq!(metodo, "thread/fork");
        assert_eq!((p["threadId"].as_str(), p["beforeTurnId"].as_str()), (Some("origem"), Some("turno-3")));
        assert_eq!((p["cwd"].as_str(), p["model"].as_str()), (Some("/casa/loja"), Some("gpt-6.2-sol")));
        assert_eq!(p["developerInstructions"], "Responda em português.");
        assert_eq!((p["sandbox"].as_str(), p["approvalPolicy"].as_str()), (Some("workspace-write"), Some("untrusted")));
        assert_eq!(p["excludeTurns"], true, "sem o histórico inteiro na resposta");
        // Sem corte: a conversa inteira.
        o.cortar_em = None;
        let (metodo, p) = abertura(&o);
        assert_eq!((metodo, p.get("beforeTurnId")), ("thread/fork", None));
        // Já com a conversa própria, retoma ela.
        o.retomar = true;
        let (metodo, p) = abertura(&o);
        assert_eq!((metodo, p["threadId"].as_str()), ("thread/resume", Some("01a1d5d6")));
        o.bifurcar = None;
        o.retomar = false;
        assert_eq!(abertura(&o).0, "thread/start");
    }

    #[test]
    fn memoria_da_equipe_vai_na_retomada_sem_expor_a_chave() {
        let pasta = std::env::temp_dir().join(format!("vitral-codex-memoria-{}", std::process::id()));
        crate::memoria_equipe::gravar_chave(&pasta, "aim_chave_privada").unwrap();
        let arquivo = crate::memoria_equipe::arquivo_do_mcp(&pasta, &pasta, "https://memoria.exemplo").unwrap();
        let mut o = opcoes(true, "plan");
        o.mcp_da_equipe = Some(arquivo);
        o.prompt_sistema = "Consulte a memória da equipe.\nSiga as instruções do projeto.".into();
        let args = argumentos(&o, &[]);
        assert!(args.iter().any(|a| a.contains("https://memoria.exemplo/mcp") && a.contains("VITRAL_MEMORIA_EQUIPE_TOKEN")));
        assert!(args.iter().any(|a| a.starts_with("developer_instructions=") && a.contains("memória da equipe")));
        assert!(!args.iter().any(|a| a.contains("aim_chave_privada")));
        std::fs::remove_dir_all(pasta).unwrap();
    }

    /// As linhas são as que o Codex 0.169.0 escreveu de verdade.
    #[test]
    fn traduz_um_turno_com_comando_e_arquivo() {
        let linhas = [
            r#"{"type":"thread.started","thread_id":"01a1d501"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"Vou criar o arquivo.\n"}}"#,
            r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"/bin/bash -lc \"printf 'oi' > a.txt\"","aggregated_output":"","exit_code":null,"status":"in_progress"}}"#,
            r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"/bin/bash -lc ls","aggregated_output":"a.txt\n","exit_code":0,"status":"completed"}}"#,
            r#"{"type":"item.started","item":{"id":"item_2","type":"file_change","changes":[{"path":"/casa/b.txt","kind":"add"}],"status":"in_progress"}}"#,
            r#"{"type":"item.completed","item":{"id":"item_2","type":"file_change","changes":[{"path":"/casa/b.txt","kind":"add"}],"status":"completed"}}"#,
            r#"{"type":"item.completed","item":{"id":"item_3","type":"agent_message","text":"feito"}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":51894,"output_tokens":92}}"#,
        ];
        let mut leitor = Leitor::default();
        let eventos: Vec<String> = linhas
            .iter()
            .flat_map(|l| leitor.linha(&serde_json::from_str(l).unwrap()))
            .map(|e| match e {
                Evento::Sessao(id) => format!("S:{id}"),
                Evento::Texto(t) => format!("T:{}", t.trim()),
                Evento::Ferramenta { id, nome, entrada } => format!("F:{id}:{nome}:{}", entrada["command"].as_str().or(entrada["file_path"].as_str()).unwrap_or_default()),
                Evento::ResultadoFerramenta { id, conteudo, erro, .. } => format!("R:{id}:{}:{erro}", conteudo.trim()),
                Evento::Gasto(tokens) => format!("G:{}:{}", tokens.entrada, tokens.saida),
                Evento::Fim { erro, texto, .. } => format!("FIM:{erro}:{texto}"),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(
            eventos,
            [
                "S:01a1d501",
                "T:Vou criar o arquivo.",
                "F:item_1:Bash:printf 'oi' > a.txt",
                "R:item_1:a.txt:false",
                "F:item_2:Write:/casa/b.txt",
                "R:item_2:criou /casa/b.txt:false",
                "T:feito",
                // Os tokens do turno, antes do fim.
                "G:51894:92",
                "FIM:false:feito",
            ]
        );
    }

    #[test]
    fn turno_que_falha_termina_com_o_erro_do_codex() {
        let linhas = [
            r#"{"type":"item.completed","item":{"id":"item_0","type":"error","message":"aviso que não para o turno"}}"#,
            r#"{"type":"error","message":"The 'x' model is not supported when using Codex with a ChatGPT account."}"#,
            r#"{"type":"turn.failed","error":{"message":"The 'x' model is not supported when using Codex with a ChatGPT account."}}"#,
        ];
        let mut leitor = Leitor::default();
        let eventos: Vec<Evento> = linhas.iter().flat_map(|l| leitor.linha(&serde_json::from_str(l).unwrap())).collect();
        assert!(matches!(eventos.as_slice(), [Evento::Fim { erro: true, texto, .. }] if texto.contains("not supported")));
    }

    /// O formato do app-server (Codex 0.169.0): o tipo num objeto e o diff de cada arquivo.
    #[test]
    fn cada_arquivo_do_codex_vira_uma_chamada_com_a_edicao() {
        let item = json!({ "type": "file_change", "id": "fc", "status": "completed", "changes": [
            { "path": "/p/a.rs", "kind": { "type": "update", "move_path": null }, "diff": "@@ -1,2 +1,2 @@\n fn a() {}\n-antes\n+depois\n" },
            { "path": "/p/novo.md", "kind": { "type": "add" }, "diff": "um\ndois\n" },
            { "path": "/p/velho.txt", "kind": { "type": "delete" }, "diff": "x\ny" },
        ] });
        let mut leitor = Leitor::default();
        assert!(leitor.linha(&json!({ "type": "item.started", "item": item })).is_empty(), "o diff só vale no fim do item");
        let eventos = leitor.linha(&json!({ "type": "item.completed", "item": item }));
        let chamadas: Vec<(String, String, Value)> = eventos
            .iter()
            .filter_map(|e| match e {
                Evento::Ferramenta { id, nome, entrada } => Some((id.clone(), nome.clone(), entrada.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(chamadas.iter().map(|(id, nome, _)| format!("{id}:{nome}")).collect::<Vec<_>>(), ["fc:0:Edit", "fc:1:Write", "fc:2:Edit"]);
        assert_eq!(chamadas[0].2["diff"], "@@ -1,2 +1,2 @@\n fn a() {}\n-antes\n+depois\n");
        assert_eq!(chamadas[1].2["content"], "um\ndois\n");
        assert_eq!(chamadas[2].2["diff"], "@@ -1,2 +0,0 @@\n-x\n-y");
        let resultados: Vec<&str> = eventos
            .iter()
            .filter_map(|e| match e {
                Evento::ResultadoFerramenta { conteudo, .. } => Some(conteudo.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(resultados, ["mudou /p/a.rs", "criou /p/novo.md", "apagou /p/velho.txt"]);
        // O arquivo novo conta as linhas que gravou; o detalhe é o de um Write.
        let (detalhe, _) = crate::claude::detalhe_ferramenta("Write", &chamadas[1].2);
        assert_eq!(detalhe.as_deref(), Some("+ um\n+ dois"));
        assert_eq!(crate::claude::mudanca_da_ferramenta("Write", &chamadas[1].2).map(|m| (m.entram, m.saem)), Some((2, 0)));
        assert_eq!(crate::claude::mudanca_da_ferramenta("Edit", &chamadas[2].2).map(|m| (m.entram, m.saem)), Some((0, 2)));
        assert!(crate::claude::write_criou(resultados[1]));
    }

    #[test]
    fn comando_sai_do_embrulho_do_shell() {
        assert_eq!(comando_limpo("/bin/bash -lc ls"), "ls");
        assert_eq!(comando_limpo("/bin/bash -lc \"echo \\\"oi\\\" > a.txt\""), "echo \"oi\" > a.txt");
        assert_eq!(comando_limpo("/bin/bash -lc 'git status'"), "git status");
        assert_eq!(comando_limpo("cargo test"), "cargo test");
    }

    #[test]
    fn catalogo_so_com_os_modelos_de_mostrar() {
        let catalogo = json!({ "models": [
            { "slug": "gpt-6.1-sol", "display_name": "GPT-6.1-Sol", "visibility": "list", "description": "Latest workhorse model.", "default_reasoning_level": "low" },
            { "slug": "codex-auto-review", "display_name": "Codex Auto Review", "visibility": "hide" },
            { "slug": "gpt-6-luna", "visibility": "list" },
        ] });
        let modelos = modelos_de(&catalogo);
        assert_eq!(modelos.iter().map(|m| (m.slug.as_str(), m.nome.as_str())).collect::<Vec<_>>(), [("gpt-6.1-sol", "GPT-6.1-Sol"), ("gpt-6-luna", "gpt-6-luna")]);
        assert_eq!((modelos[0].esforco.as_str(), modelos[1].esforco.as_str()), ("low", ""));
    }

    /// O `view_image` do Codex no app-server (conferido no 0.169.0, 23/11/2026): o item
    /// `imageView` traz o caminho da imagem no disco, sem base64. Vira a leitura desse arquivo,
    /// que a conversa já mostra com a miniatura.
    #[test]
    fn o_view_image_do_codex_vira_a_leitura_do_arquivo() {
        let item = json!({"type": "imageView", "id": "exec-52a6", "path": "/p/prints/a.png", "startedAtMs": 1, "completedAtMs": null});
        let mut leitor = Leitor::default();
        let comeco = leitor.linha(&json!({"type": "item.started", "item": item}));
        assert!(matches!(comeco.as_slice(), [Evento::Ferramenta { id, nome, entrada }]
            if id == "exec-52a6" && nome == "Read" && entrada["file_path"] == "/p/prints/a.png"));
        let fim = leitor.linha(&json!({"type": "item.completed", "item": item}));
        assert!(matches!(fim.as_slice(), [Evento::ResultadoFerramenta { id, conteudo, erro: false, .. }] if id == "exec-52a6" && conteudo.is_empty()));
        // A chamada guarda o arquivo: é por ele que a miniatura aparece.
        let (_, arquivo) = crate::claude::detalhe_ferramenta("Read", &json!({"file_path": "/p/prints/a.png"}));
        assert_eq!(arquivo.as_deref(), Some("/p/prints/a.png"));
    }

    /// A ferramenta MCP que devolve imagem (o print do navegador): o resultado do Codex traz os
    /// blocos do MCP, com a imagem em `data` e `mimeType`. O texto vira o resultado e a imagem
    /// sai à parte; o base64 não vai no texto (antes ia o JSON inteiro do resultado).
    #[test]
    fn a_imagem_do_mcp_do_codex_sai_do_texto_do_resultado() {
        let png: Vec<u8> = b"\x89PNG\r\n\x1a\n".iter().copied().chain((0..3000u32).map(|i| (i * 7 % 251) as u8)).collect();
        // "iVBORw0KGgo" é o começo de todo PNG em base64.
        let em_base64 = {
            const ALFABETO: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut saida = String::new();
            for bloco in png.chunks(3) {
                let b = [bloco[0], *bloco.get(1).unwrap_or(&0), *bloco.get(2).unwrap_or(&0)];
                let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
                for i in 0..4 {
                    saida.push(if i <= bloco.len() { ALFABETO[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
                }
            }
            saida
        };
        assert!(em_base64.starts_with("iVBORw0KGgo"));
        let item = json!({"type": "mcp_tool_call", "id": "mcp-1", "server": "chrome-devtools", "tool": "take_screenshot",
            "status": "completed", "arguments": {}, "error": null, "result": {
                "content": [{"type": "text", "text": "Took a screenshot."}, {"type": "image", "data": em_base64, "mimeType": "image/png"}],
                "structuredContent": null, "_meta": null,
            }});
        let mut leitor = Leitor::default();
        let eventos = leitor.linha(&json!({"type": "item.completed", "item": item}));
        match eventos.as_slice() {
            [Evento::Ferramenta { nome, .. }, Evento::ResultadoFerramenta { id, conteudo, erro: false, .. }, Evento::ImagensDoResultado { id: da_imagem, imagens }] => {
                assert_eq!(nome, "mcp__chrome-devtools__take_screenshot");
                assert_eq!((id.as_str(), da_imagem.as_str()), ("mcp-1", "mcp-1"));
                assert_eq!(conteudo, "Took a screenshot.");
                assert_eq!(imagens.len(), 1);
                assert_eq!(imagens[0].bytes, png);
            }
            outros => panic!("eventos inesperados: {outros:?}"),
        }

        // Sem imagem, o texto dos blocos (e não o JSON deles); a falha continua com a mensagem.
        let so_texto = json!({"type": "mcp_tool_call", "id": "mcp-2", "server": "s", "tool": "t", "status": "completed", "arguments": {},
            "result": {"content": [{"type": "text", "text": "um"}, {"type": "text", "text": "dois"}], "structuredContent": null}});
        let eventos = Leitor::default().linha(&json!({"type": "item.completed", "item": so_texto}));
        assert!(matches!(eventos.as_slice(), [Evento::Ferramenta { .. }, Evento::ResultadoFerramenta { conteudo, .. }] if conteudo == "um\ndois"));
        let falhou = json!({"type": "mcp_tool_call", "id": "mcp-3", "server": "s", "tool": "t", "status": "failed", "arguments": {},
            "result": null, "error": {"message": "servidor fora do ar"}});
        let eventos = Leitor::default().linha(&json!({"type": "item.completed", "item": falhou}));
        assert!(matches!(eventos.as_slice(), [Evento::Ferramenta { .. }, Evento::ResultadoFerramenta { conteudo, erro: true, .. }] if conteudo == "servidor fora do ar"));
    }

    /// A imagem que o Codex gera (`imageGeneration`, pelo schema do app-server do 0.169.0): com o
    /// arquivo que ele gravou (`savedPath`), a chamada aponta para ele; só com o base64
    /// (`result`), a imagem sai à parte, para o Vitral guardar.
    #[test]
    fn a_imagem_gerada_pelo_codex_aparece_na_chamada() {
        let gravada = json!({"type": "imageGeneration", "id": "ig-1", "status": "completed", "revisedPrompt": "um gato de óculos",
            "savedPath": "/casa/.codex/generated_images/gato.png", "result": "iVBORw0KGgoAAAA="});
        let eventos = Leitor::default().linha(&json!({"type": "item.completed", "item": gravada}));
        match eventos.as_slice() {
            [Evento::Ferramenta { nome, entrada, .. }, Evento::ResultadoFerramenta { conteudo, erro: false, .. }] => {
                assert_eq!(nome, "ImageGeneration");
                assert_eq!(entrada["file_path"], "/casa/.codex/generated_images/gato.png");
                assert_eq!(entrada["prompt"], "um gato de óculos");
                assert!(!conteudo.contains("iVBORw0KGgo"));
            }
            outros => panic!("eventos inesperados: {outros:?}"),
        }

        let so_base64 = json!({"type": "imageGeneration", "id": "ig-2", "status": "completed", "revisedPrompt": null, "result": "iVBORw0KGgoAAAA="});
        let eventos = Leitor::default().linha(&json!({"type": "item.completed", "item": so_base64}));
        assert!(matches!(eventos.as_slice(), [
            Evento::Ferramenta { nome, .. },
            Evento::ResultadoFerramenta { conteudo, .. },
            Evento::ImagensDoResultado { id, imagens },
        ] if nome == "ImageGeneration" && conteudo.is_empty() && id == "ig-2" && imagens.len() == 1 && imagens[0].extensao == "png"));
    }
}

#[cfg(test)]
mod testes_dos_nomes_no_windows {
    use super::{nomes_do_codex, pastas_do_codex};
    use std::path::{Path, PathBuf};

    #[test]
    fn no_windows_o_codex_e_o_exe_do_instalador_ou_o_cmd_do_npm() {
        assert_eq!(nomes_do_codex(true), &["codex.exe", "codex.cmd"]);
        assert_eq!(nomes_do_codex(false), &["codex"]);
        let pastas = pastas_do_codex(true, Path::new("/casa"), Some(PathBuf::from("/local")), Some(PathBuf::from("/roaming")));
        assert!(pastas.contains(&PathBuf::from("/local/Programs/OpenAI/Codex/bin")));
        assert!(pastas.contains(&PathBuf::from("/roaming/npm")));
        assert_eq!(pastas_do_codex(false, Path::new("/casa"), None, None), vec![PathBuf::from("/casa/.local/bin")]);
    }
}
