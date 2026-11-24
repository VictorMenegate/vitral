//! O Gemini como agente de um chat, pelo Antigravity CLI (`agy acp`), o sucessor oficial do Gemini
//! CLI (que deixou de atender as contas pessoais em 18/06/2026). Fala o ACP (Agent Client
//! Protocol), JSON-RPC pela entrada e pela saída padrão, e a tradução entrega os mesmos eventos do
//! Claude Code, para a conversa não depender de quem respondeu.
//!
//! Conferido no agy 1.5.15 (23/11/2026), com uma conta Google pessoal (Antigravity Starter Quota):
//! - `initialize` e depois `session/new {cwd, mcpServers: []}`, ou `session/resume {sessionId, cwd,
//!   mcpServers}`, que reabre a conversa sem repetir o histórico; os dois devolvem o `sessionId`;
//! - o prompt de sistema vai em `_meta.antigravity.systemPrompt.append` (o agy anuncia isso no
//!   `initialize`; o `systemPrompt` da especificação ele não aceita);
//! - o modelo, o nível de raciocínio e o modo vêm em `configOptions` (ids `model`, `thought_level`
//!   e `mode`) e mudam por `session/set_config_option {sessionId, configId, value}`;
//! - `session/update` traz `agent_message_chunk`, `agent_thought_chunk`, `tool_call` e
//!   `tool_call_update` (com `name` e `args` do agy: `read_file{Path}`, `run_command{CommandLine}`,
//!   `write_file{Path, Content}`, `edit_file{Path, Target, Replacement}`), `usage_update{used, size}`;
//! - a resposta do `session/prompt` traz o `stopReason` e o `usage` (`inputTokens`, `outputTokens`,
//!   `cachedReadTokens`, `cachedWriteTokens`; o `inputTokens` já sem o cache);
//! - um `session/prompt` mandado com o turno rodando entra nele: o pedido anterior responde
//!   `end_turn` na hora, e o turno segue até a resposta do último;
//! - `session/request_permission` oferece `allow` (`allow_once`) e `deny` (`reject_once`);
//! - `session/cancel` (notificação) para o turno, que responde `stopReason: cancelled`;
//! - com a entrada fechada, o processo sai.

use std::{
    collections::{HashMap, HashSet},
    io::{self, Read as _, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::{Value, json};

use crate::{
    claude::{Evento, Opcoes},
    mcp::Resposta,
    processos,
};

/// No Windows o instalador põe um `agy.exe`.
const NOME_DO_AGY: &str = if cfg!(windows) { "agy.exe" } else { "agy" };

/// Quanto se espera o `agy models` responder.
const PRAZO_DOS_MODELOS: Duration = Duration::from_secs(30);

/// O `agy` do PATH ou o de `~/.local/bin` (apps abertos pela dock nem sempre têm esse PATH).
pub fn binario() -> PathBuf {
    binario_em(std::env::var_os("PATH"), dirs::home_dir().unwrap_or_default())
}

fn binario_em(path: Option<std::ffi::OsString>, home: PathBuf) -> PathBuf {
    let pastas = pastas_do_agy(cfg!(windows), &home, std::env::var_os("LOCALAPPDATA").map(PathBuf::from));
    crate::sistema::achar_programa(path.as_deref(), &[NOME_DO_AGY], &pastas).unwrap_or_else(|| PathBuf::from(NOME_DO_AGY))
}

/// Onde o instalador oficial põe o agy, além do PATH: `~/.local/bin` (Linux e Mac) e, no Windows,
/// `%LOCALAPPDATA%\agy\bin` (o `install.ps1`). O PATH do Vitral aberto não muda depois que o
/// instalador acrescenta a pasta, e o Gemini não aparecia até reiniciar (24/11/2026).
fn pastas_do_agy(windows: bool, home: &Path, local: Option<PathBuf>) -> Vec<PathBuf> {
    let mut pastas = vec![home.join(".local").join("bin")];
    if windows {
        pastas.extend(local.map(|l| l.join("agy").join("bin")));
    }
    pastas
}

/// Há um Antigravity CLI instalado neste computador.
pub fn instalado() -> bool {
    binario().is_file()
}

/// Os argumentos do `agy` (sem o binário): o servidor ACP, sem telemetria. O "Sem perguntar" é do
/// processo; os outros modos vão pela sessão (`modo_de`).
pub fn argumentos(opcoes: &Opcoes) -> Vec<String> {
    let mut args = vec!["acp".to_string(), "--disable-telemetry".to_string()];
    if opcoes.permissao == "bypassPermissions" {
        args.push("--dangerously-skip-permissions".into());
    }
    args
}

/// O modo do agy para cada modo de permissão do Vitral.
pub fn modo_de(permissao: &str) -> &'static str {
    match permissao {
        "acceptEdits" => "accept-edits",
        "plan" => "plan",
        "auto" => "auto",
        _ => "default",
    }
}

/// O que cada modo de permissão quer dizer num chat do Gemini (para a dica do seletor).
pub fn dica_da_permissao(permissao: &str) -> &'static str {
    match permissao {
        "acceptEdits" => "O Gemini edita arquivos sem perguntar; comandos ainda pedem.",
        "plan" => "O Gemini planeja com cuidado antes de executar.",
        "auto" => "O Gemini aprova edições e comandos seguros e pede antes do que não tem volta.",
        "bypassPermissions" => "O Gemini faz tudo sem pedir permissão.",
        _ => "O Gemini pede aprovação antes de editar e de rodar comandos.",
    }
}

/// O nível de raciocínio do agy (`low`, `medium`, `high`) para o esforço escolhido no Vitral.
pub fn nivel_de(esforco: &str) -> Option<&'static str> {
    match esforco {
        "low" => Some("low"),
        "medium" => Some("medium"),
        "high" | "xhigh" | "max" => Some("high"),
        _ => None,
    }
}

// ---- os modelos ----

/// Um modelo que o Antigravity oferece a esta conta.
#[derive(Clone, Debug, PartialEq)]
pub struct ModeloDoGemini {
    /// O que vai no `configId: "model"` da sessão ("gemini-3.8-flash").
    pub valor: String,
    /// O nome que o agy mostra ("Gemini 3.8 Flash").
    pub nome: String,
}

/// Os modelos da última vez que o agy respondeu (vazio até lá).
static MODELOS: Mutex<Vec<ModeloDoGemini>> = Mutex::new(Vec::new());

pub fn modelos() -> Vec<ModeloDoGemini> {
    MODELOS.lock().map(|m| m.clone()).unwrap_or_default()
}

/// O nome de mostrar de um modelo do Gemini; sem a lista, o próprio valor.
pub fn nome_do_modelo(valor: &str) -> String {
    modelos().into_iter().find(|m| m.valor == valor).map_or_else(|| valor.to_string(), |m| m.nome)
}

fn guardar_modelos(novos: Vec<ModeloDoGemini>) -> bool {
    if novos.is_empty() {
        return false;
    }
    let Ok(mut guardados) = MODELOS.lock() else {
        return false;
    };
    if *guardados == novos {
        return false;
    }
    *guardados = novos;
    true
}

/// Pergunta ao agy quais modelos esta conta tem (`agy models`, sem abrir conversa nem gastar
/// cota) e guarda. Bloqueia até a resposta ou o prazo: chamar em segundo plano. Devolve `true` se
/// a lista mudou.
pub fn atualizar_modelos(pasta: &Path) -> bool {
    if !instalado() || std::fs::create_dir_all(pasta).is_err() {
        return false;
    }
    let mut comando = processos::comando_leve(binario(), "modelos-gemini");
    comando.arg("models").current_dir(pasta).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let Ok((mut filho, escopo)) = processos::abrir(comando) else {
        return false;
    };
    let Some(mut saida) = filho.stdout.take() else {
        return false;
    };
    let (envia, recebe) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut tudo = String::new();
        let _ = saida.read_to_string(&mut tudo);
        let _ = envia.send(tudo);
    });
    let tudo = recebe.recv_timeout(PRAZO_DOS_MODELOS).ok();
    escopo.matar();
    let _ = filho.wait();
    tudo.is_some_and(|texto| guardar_modelos(modelos_da_lista(&texto)))
}

/// Os modelos do `agy models` (uma linha "slug<TAB>nome" por modelo). As variantes de um mesmo
/// modelo pelo nível de raciocínio ("gemini-3.8-flash-high", "-medium", "-low") viram um modelo
/// só, o da sessão ("gemini-3.8-flash"): o nível é o esforço da caixa. Um sufixo sozinho
/// ("gpt-oss-120b-medium") fica como está, que é o valor que a sessão aceita.
fn modelos_da_lista(texto: &str) -> Vec<ModeloDoGemini> {
    const NIVEIS: [(&str, &str); 3] = [("-high", " (High)"), ("-medium", " (Medium)"), ("-low", " (Low)")];
    let linhas: Vec<(String, String)> = texto
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(slug, nome)| (slug.trim().to_string(), nome.trim().to_string()))
        .filter(|(slug, _)| !slug.is_empty() && !slug.contains(' '))
        .collect();
    let base = |slug: &str| NIVEIS.iter().find_map(|(sufixo, _)| slug.strip_suffix(sufixo)).map(str::to_string);
    let variantes = |b: &str| linhas.iter().filter(|(s, _)| base(s).as_deref() == Some(b)).count();
    let mut modelos: Vec<ModeloDoGemini> = Vec::new();
    for (slug, nome) in &linhas {
        let (valor, nome) = match base(slug).filter(|b| variantes(b) > 1) {
            Some(b) => {
                let nome = NIVEIS.iter().find_map(|(_, n)| nome.strip_suffix(n)).unwrap_or(nome);
                (b, nome.to_string())
            }
            None => (slug.clone(), nome.clone()),
        };
        if !modelos.iter().any(|m| m.valor == valor) {
            modelos.push(ModeloDoGemini { valor, nome });
        }
    }
    modelos
}

/// Os modelos que a sessão oferece (a opção `model` das `configOptions`).
fn modelos_das_opcoes(opcoes: &Value) -> Vec<ModeloDoGemini> {
    opcao(opcoes, "model")
        .and_then(|o| o["options"].as_array())
        .map(|lista| {
            lista
                .iter()
                .filter_map(|o| {
                    let valor = o["value"].as_str()?.to_string();
                    let nome = o["name"].as_str().unwrap_or(&valor).to_string();
                    Some(ModeloDoGemini { valor, nome })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn opcao<'a>(opcoes: &'a Value, id: &str) -> Option<&'a Value> {
    opcoes.as_array()?.iter().find(|o| o["id"] == id)
}

/// A opção `id` aceita `valor` e ainda não está nele.
fn precisa_mudar(opcoes: &Value, id: &str, valor: &str) -> bool {
    opcao(opcoes, id).is_some_and(|o| {
        o["currentValue"] != valor && o["options"].as_array().is_some_and(|l| l.iter().any(|v| v["value"] == valor))
    })
}

// ---- o canal (a entrada do processo) ----

/// Os blocos de um pedido: o texto e as imagens (em base64, como o ACP pede).
pub fn blocos(texto: &str, imagens: &[PathBuf]) -> Vec<Value> {
    let mut blocos = vec![json!({"type": "text", "text": texto})];
    for caminho in imagens {
        let Some(tipo) = crate::claude::tipo_imagem(caminho) else { continue };
        let Ok(bytes) = std::fs::read(caminho) else { continue };
        blocos.push(json!({"type": "image", "mimeType": tipo, "data": crate::claude::base64(&bytes)}));
    }
    blocos
}

pub struct Canal {
    saida: Option<Box<dyn Write + Send>>,
    sessao: String,
    /// A sessão está aberta e configurada: os pedidos já podem ir.
    pronta: bool,
    concluido: bool,
    proximo: u64,
    /// Os `session/prompt` ainda sem resposta.
    abertos: Vec<String>,
    /// As mensagens mandadas no meio do turno, pelo id do pedido: o texto volta na confirmação.
    pendentes: HashMap<String, String>,
    /// As que chegaram antes de a sessão ficar pronta.
    aguardando: Vec<(String, Vec<Value>)>,
}

impl Canal {
    pub fn novo(saida: Box<dyn Write + Send>) -> Self {
        Self {
            saida: Some(saida),
            sessao: String::new(),
            pronta: false,
            concluido: false,
            proximo: 0,
            abertos: Vec::new(),
            pendentes: HashMap::new(),
            aguardando: Vec::new(),
        }
    }

    fn escrever(&mut self, valor: Value) -> io::Result<()> {
        let saida = self.saida.as_mut().ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "Gemini fechado"))?;
        writeln!(saida, "{valor}")?;
        saida.flush()
    }

    fn pedir(&mut self, id: &str, metodo: &str, parametros: Value) -> io::Result<()> {
        self.escrever(json!({"jsonrpc": "2.0", "id": id, "method": metodo, "params": parametros}))
    }

    fn prompt(&mut self, id: String, blocos: Vec<Value>) -> io::Result<()> {
        let sessao = self.sessao.clone();
        self.pedir(&id, "session/prompt", json!({"sessionId": sessao, "prompt": blocos}))?;
        self.abertos.push(id);
        Ok(())
    }

    pub fn aberta(&self) -> bool {
        self.saida.is_some() && !self.concluido
    }

    pub fn fechar(&mut self) {
        self.saida = None;
        self.concluido = true;
    }

    /// Mais uma mensagem no turno que está rodando (o agy a absorve nele).
    pub fn dirigir(&mut self, texto: &str, imagens: &[PathBuf]) -> io::Result<()> {
        if !self.aberta() {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "Turno concluído"));
        }
        self.proximo += 1;
        let id = format!("vitral-prompt-{}", self.proximo);
        let blocos = blocos(texto, imagens);
        if self.pronta {
            self.prompt(id.clone(), blocos)?;
        } else {
            self.aguardando.push((id.clone(), blocos));
        }
        self.pendentes.insert(id, texto.to_string());
        Ok(())
    }

    /// Para o turno: o `session/cancel`. Sem sessão ainda, fecha a entrada (o processo sai).
    pub fn interromper(&mut self) -> io::Result<()> {
        self.concluido = true;
        if self.sessao.is_empty() {
            self.fechar();
            return Ok(());
        }
        let sessao = self.sessao.clone();
        self.escrever(json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": sessao}}))
    }

    /// A sessão ficou pronta: vai o pedido inicial e o que esperava por ela.
    fn comecar(&mut self, inicial: Vec<Value>) -> io::Result<()> {
        self.pronta = true;
        self.prompt("vitral-prompt-0".into(), inicial)?;
        for (id, blocos) in std::mem::take(&mut self.aguardando) {
            self.prompt(id, blocos)?;
        }
        Ok(())
    }
}

// ---- o protocolo (a saída do processo) ----

pub struct Protocolo {
    canal: Arc<Mutex<Canal>>,
    cwd: PathBuf,
    retomar: Option<String>,
    modelo: Option<String>,
    nivel: Option<&'static str>,
    modo: &'static str,
    sistema: String,
    inicial: Vec<Value>,
    /// Os `set_config_option` ainda sem resposta: o pedido inicial espera por eles.
    configurando: usize,
    nome_do_modelo: String,
    /// As ferramentas já anunciadas (uma chamada só nasce uma vez).
    anunciadas: HashSet<String>,
    fala: String,
    pensamento: Option<String>,
    ultima_fala: String,
    janela: Option<u64>,
    erro: Option<String>,
    cancelado: bool,
    terminou: bool,
}

impl Protocolo {
    pub fn novo(canal: Arc<Mutex<Canal>>, opcoes: &Opcoes, texto: &str, imagens: &[PathBuf]) -> Self {
        Self {
            canal,
            cwd: opcoes.cwd.clone(),
            retomar: (opcoes.retomar && !opcoes.sessao.is_empty()).then(|| opcoes.sessao.clone()),
            modelo: opcoes.modelo.clone().filter(|m| !m.trim().is_empty()),
            nivel: opcoes.esforco.as_deref().and_then(nivel_de),
            modo: modo_de(&opcoes.permissao),
            sistema: opcoes.prompt_sistema.clone(),
            inicial: blocos(texto, imagens),
            configurando: 0,
            nome_do_modelo: String::new(),
            anunciadas: HashSet::new(),
            fala: String::new(),
            pensamento: None,
            ultima_fala: String::new(),
            janela: None,
            erro: None,
            cancelado: false,
            terminou: false,
        }
    }

    pub fn iniciar(&mut self) -> io::Result<()> {
        self.canal.lock().unwrap_or_else(|e| e.into_inner()).pedir(
            "vitral-init",
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
                "clientInfo": {"name": "vitral", "version": env!("CARGO_PKG_VERSION")},
            }),
        )
    }

    pub fn linha(&mut self, v: &Value) -> Vec<Evento> {
        let mut eventos = Vec::new();
        if let Err(erro) = self.tratar(v, &mut eventos) {
            self.canal.lock().unwrap_or_else(|e| e.into_inner()).fechar();
            if !self.terminou {
                self.terminou = true;
                eventos.push(Evento::Fim { erro: true, texto: erro.to_string(), custo_usd: 0., janela: self.janela });
            }
        }
        eventos
    }

    fn tratar(&mut self, v: &Value, eventos: &mut Vec<Evento>) -> io::Result<()> {
        // Um pedido do agente: a permissão vai para o cartão do Vitral; o resto ele não oferece.
        if let (Some(id), Some(metodo)) = (v.get("id"), v["method"].as_str()) {
            if metodo == "session/request_permission" {
                eventos.push(self.pedido_de_permissao(id.clone(), &v["params"]));
                return Ok(());
            }
            let mut canal = self.canal.lock().unwrap_or_else(|e| e.into_inner());
            return canal.escrever(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "O Vitral não oferece este pedido."}}));
        }
        if let Some(id) = v.get("id") {
            let id = id.as_str().map_or_else(|| id.to_string(), str::to_string);
            self.resposta(&id, v, eventos)?;
        } else if v["method"] == "session/update" {
            self.atualizacao(&v["params"]["update"], eventos);
        }
        // O turno acaba com a resposta do último pedido (o anterior responde quando o seguinte entra).
        let mut canal = self.canal.lock().unwrap_or_else(|e| e.into_inner());
        if canal.pronta && canal.abertos.is_empty() && !self.terminou {
            self.terminou = true;
            canal.fechar();
            drop(canal);
            self.fechar_blocos(eventos);
            let (erro, texto) = match (self.erro.take(), self.cancelado) {
                (Some(erro), _) => (true, erro),
                (None, true) => (true, "O turno foi interrompido.".to_string()),
                (None, false) => (false, std::mem::take(&mut self.ultima_fala)),
            };
            eventos.push(Evento::Fim { erro, texto, custo_usd: 0., janela: self.janela });
        }
        Ok(())
    }

    fn resposta(&mut self, id: &str, v: &Value, eventos: &mut Vec<Evento>) -> io::Result<()> {
        let erro = v.get("error").map(|e| e["message"].as_str().unwrap_or("Pedido recusado").to_string());
        let mut canal = self.canal.lock().unwrap_or_else(|e| e.into_inner());
        match id {
            "vitral-init" => {
                if let Some(erro) = erro {
                    return Err(io::Error::other(format!("O Gemini não abriu: {erro}")));
                }
                self.abrir_sessao(&mut canal)
            }
            "vitral-sessao" => {
                if let Some(erro) = erro {
                    if erro.contains("uthentication") {
                        return Err(io::Error::other(format!(
                            "O Gemini pede login (Authentication required). Entre com `{}`.",
                            login_sugerido()
                        )));
                    }
                    // A conversa que o agy não tem mais (apagada, ou de outra conta) não deixa o
                    // chat preso: abre uma nova, sem o que veio antes.
                    if self.retomar.take().is_some() {
                        return self.abrir_sessao(&mut canal);
                    }
                    return Err(io::Error::other(format!("O Gemini não abriu a conversa: {erro}")));
                }
                let resultado = &v["result"];
                let sessao = resultado["sessionId"].as_str().map(str::to_string).or_else(|| self.retomar.clone()).unwrap_or_default();
                if sessao.is_empty() {
                    return Err(io::Error::other("O Gemini não devolveu a conversa."));
                }
                canal.sessao = sessao.clone();
                eventos.push(Evento::Sessao(sessao.clone()));
                let opcoes = &resultado["configOptions"];
                guardar_modelos(modelos_das_opcoes(opcoes));
                let atual = opcao(opcoes, "model").and_then(|o| o["currentValue"].as_str()).unwrap_or_default().to_string();
                let mut mudar: Vec<(&str, String)> = Vec::new();
                if let Some(modelo) = self.modelo.clone().filter(|m| precisa_mudar(opcoes, "model", m)) {
                    mudar.push(("model", modelo));
                }
                if let Some(nivel) = self.nivel.filter(|n| precisa_mudar(opcoes, "thought_level", n)) {
                    mudar.push(("thought_level", nivel.to_string()));
                }
                if precisa_mudar(opcoes, "mode", self.modo) {
                    mudar.push(("mode", self.modo.to_string()));
                }
                let modelo = mudar.iter().find(|(id, _)| *id == "model").map_or(atual, |(_, m)| m.clone());
                self.nome_do_modelo = nome_do_modelo(&modelo);
                eventos.push(Evento::Inicio { modelo: self.nome_do_modelo.clone(), comandos: Vec::new(), versao: None, fluxos: None });
                self.configurando = mudar.len();
                for (opcao, valor) in mudar {
                    canal.pedir(
                        &format!("vitral-cfg-{opcao}"),
                        "session/set_config_option",
                        json!({"sessionId": sessao, "configId": opcao, "value": valor}),
                    )?;
                }
                if self.configurando == 0 {
                    canal.comecar(std::mem::take(&mut self.inicial))?;
                }
                Ok(())
            }
            outro if outro.starts_with("vitral-cfg-") => {
                // Uma opção recusada (o modelo que saiu da conta) não impede o turno: ele roda no
                // que a sessão já tinha.
                self.configurando = self.configurando.saturating_sub(1);
                if self.configurando == 0 && !canal.pronta {
                    canal.comecar(std::mem::take(&mut self.inicial))?;
                }
                Ok(())
            }
            outro if outro.starts_with("vitral-prompt-") => {
                canal.abertos.retain(|a| a != outro);
                if let Some(texto) = canal.pendentes.remove(outro) {
                    eventos.push(Evento::DirecaoConfirmada { texto, aceita: erro.is_none() });
                }
                drop(canal);
                if let Some(tokens) = tokens_do_turno(&v["result"]) {
                    eventos.push(Evento::Gasto(tokens));
                }
                match (erro, v["result"]["stopReason"].as_str().unwrap_or_default()) {
                    (Some(erro), _) if outro == "vitral-prompt-0" => self.erro = Some(erro),
                    (Some(_), _) => {}
                    (None, "cancelled") => self.cancelado = true,
                    (None, "refusal") => self.erro = Some("O modelo recusou o pedido.".into()),
                    (None, "max_tokens") => self.erro = Some("A resposta passou do tamanho máximo do modelo.".into()),
                    (None, "max_turn_requests") => self.erro = Some("O turno passou do limite de chamadas do modelo.".into()),
                    _ => {}
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// O `session/new`, ou o `session/resume` da conversa guardada, com o prompt de sistema.
    fn abrir_sessao(&self, canal: &mut Canal) -> io::Result<()> {
        let mut parametros = json!({"cwd": self.cwd, "mcpServers": []});
        if !self.sistema.is_empty() {
            parametros["_meta"] = json!({"antigravity": {"systemPrompt": {"append": self.sistema}}});
        }
        let metodo = match &self.retomar {
            Some(sessao) => {
                parametros["sessionId"] = json!(sessao);
                "session/resume"
            }
            None => "session/new",
        };
        canal.pedir("vitral-sessao", metodo, parametros)
    }

    fn atualizacao(&mut self, u: &Value, eventos: &mut Vec<Evento>) {
        match u["sessionUpdate"].as_str().unwrap_or_default() {
            "agent_message_chunk" => {
                let parte = u["content"]["text"].as_str().unwrap_or_default();
                if parte.is_empty() {
                    return;
                }
                if let Some(pensamento) = self.pensamento.take() {
                    eventos.push(Evento::Pensamento(pensamento));
                }
                self.fala.push_str(parte);
                eventos.push(Evento::TextoParcial(parte.to_string()));
            }
            "agent_thought_chunk" => {
                let parte = u["content"]["text"].as_str().unwrap_or_default();
                if !self.fala.is_empty() {
                    let fala = std::mem::take(&mut self.fala);
                    self.ultima_fala = fala.clone();
                    eventos.push(Evento::Texto(fala));
                }
                match &mut self.pensamento {
                    Some(pensamento) => pensamento.push_str(parte),
                    None => {
                        eventos.push(Evento::Pensando);
                        self.pensamento = Some(parte.to_string());
                    }
                }
            }
            "tool_call" | "tool_call_update" => {
                self.fechar_blocos(eventos);
                let id = u["toolCallId"].as_str().unwrap_or_default().to_string();
                if id.is_empty() {
                    return;
                }
                if self.anunciadas.insert(id.clone()) {
                    let (nome, entrada) = ferramenta_do_agy(u);
                    eventos.push(Evento::Ferramenta { id: id.clone(), nome, entrada });
                }
                if let status @ ("completed" | "failed") = u["status"].as_str().unwrap_or_default() {
                    let (conteudo, saiu_com_erro) = resultado_do_agy(u);
                    eventos.push(Evento::ResultadoFerramenta { id, conteudo, erro: status == "failed" || saiu_com_erro, no_fundo: false });
                }
            }
            "usage_update" => {
                if let Some(tamanho) = u["size"].as_u64() {
                    self.janela = Some(tamanho);
                }
                if let Some(usado) = u["used"].as_u64().filter(|n| *n > 0) {
                    eventos.push(Evento::Contexto(usado));
                }
            }
            _ => {}
        }
    }

    /// A fala e o raciocínio que estavam chegando terminaram (veio uma ferramenta ou o fim).
    fn fechar_blocos(&mut self, eventos: &mut Vec<Evento>) {
        if let Some(pensamento) = self.pensamento.take() {
            eventos.push(Evento::Pensamento(pensamento));
        }
        if !self.fala.is_empty() {
            let fala = std::mem::take(&mut self.fala);
            self.ultima_fala = fala.clone();
            eventos.push(Evento::Texto(fala));
        }
    }

    /// O pedido de permissão vai para o cartão do Vitral, com um canal de resposta. Uma thread
    /// espera a resposta e a escreve ao agy; sem resposta (o Parar, o chat fechado), o pedido
    /// volta como cancelado, como o ACP pede.
    fn pedido_de_permissao(&mut self, id: Value, p: &Value) -> Evento {
        let chamada = &p["toolCall"];
        let (ferramenta, entrada) = ferramenta_do_agy(chamada);
        let opcoes = p["options"].as_array().cloned().unwrap_or_default();
        let escolha = |tipos: &[&str], padrao: &str| {
            opcoes
                .iter()
                .find(|o| tipos.contains(&o["kind"].as_str().unwrap_or_default()))
                .and_then(|o| o["optionId"].as_str())
                .unwrap_or(padrao)
                .to_string()
        };
        let (permitir, negar) = (escolha(&["allow_once", "allow_always"], "allow"), escolha(&["reject_once", "reject_always"], "deny"));
        let (envia, recebe) = smol::channel::bounded::<Resposta>(1);
        let canal = self.canal.clone();
        std::thread::spawn(move || {
            let resultado = match recebe.recv_blocking() {
                Ok(resposta) => {
                    let permitiu = serde_json::from_str::<Value>(&resposta.texto).is_ok_and(|r| r["behavior"] == "allow");
                    json!({"outcome": {"outcome": "selected", "optionId": if permitiu { permitir } else { negar }}})
                }
                Err(_) => json!({"outcome": {"outcome": "cancelled"}}),
            };
            let _ = canal.lock().unwrap_or_else(|e| e.into_inner()).escrever(json!({"jsonrpc": "2.0", "id": id, "result": resultado}));
        });
        Evento::Permissao { ferramenta, entrada, resposta: envia }
    }
}

/// O comando de login a sugerir quando o agy pede.
pub fn login_sugerido() -> &'static str {
    "agy acp --login"
}

/// Os tokens de uma resposta do `session/prompt` (o `inputTokens` do agy já vem sem o cache).
fn tokens_do_turno(resultado: &Value) -> Option<crate::motor::Tokens> {
    let n = |chave: &str| resultado["usage"][chave].as_u64().unwrap_or(0);
    let tokens = crate::motor::Tokens {
        entrada: n("inputTokens"),
        cache_lido: n("cachedReadTokens"),
        cache_criado: n("cachedWriteTokens"),
        saida: n("outputTokens"),
    };
    (tokens.total() > 0).then_some(tokens)
}

/// A chamada do agy no formato das do Claude Code (o `detalhe_ferramenta` e o
/// `mudanca_da_ferramenta` servem a todos): ler, rodar, escrever e editar viram Read, Bash,
/// Write e Edit; o resto vai com o nome do agy e o que ele diz no título.
fn ferramenta_do_agy(chamada: &Value) -> (String, Value) {
    let args = if chamada["args"].is_object() { &chamada["args"] } else { &chamada["rawInput"] };
    let campo = |chave: &str| args[chave].as_str().unwrap_or_default();
    let caminho = || {
        let dito = campo("Path");
        if dito.is_empty() {
            chamada["locations"][0]["path"].as_str().unwrap_or_default().to_string()
        } else {
            dito.to_string()
        }
    };
    match chamada["name"].as_str().unwrap_or_default() {
        "read_file" => ("Read".into(), json!({"file_path": caminho()})),
        "run_command" => {
            let mut entrada = json!({"command": campo("CommandLine")});
            if !campo("toolSummary").is_empty() {
                entrada["description"] = json!(campo("toolSummary"));
            }
            ("Bash".into(), entrada)
        }
        "write_file" => ("Write".into(), json!({"file_path": caminho(), "content": campo("Content")})),
        "edit_file" => (
            "Edit".into(),
            json!({"file_path": caminho(), "old_string": campo("Target"), "new_string": campo("Replacement")}),
        ),
        outro => {
            let nome = if outro.is_empty() { chamada["kind"].as_str().unwrap_or("ferramenta") } else { outro };
            let mut entrada = if args.is_object() { args.clone() } else { json!({}) };
            if let Some(titulo) = chamada["title"].as_str().filter(|t| !t.is_empty()) {
                entrada["description"] = json!(titulo);
            }
            (nome.to_string(), entrada)
        }
    }
}

/// O que a chamada devolveu (o texto, ou o que mudou num arquivo) e se o comando saiu com erro.
fn resultado_do_agy(u: &Value) -> (String, bool) {
    let saiu_com_erro = u["rawOutput"]["exit_code"].as_i64().is_some_and(|c| c != 0);
    let partes: Vec<String> = u["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| match c["type"].as_str()? {
            "content" => c["content"]["text"].as_str().map(str::to_string),
            "diff" => {
                let verbo = if c["oldText"].is_string() { "mudou" } else { "criou" };
                Some(format!("{verbo} {}", c["path"].as_str().unwrap_or_default()))
            }
            _ => None,
        })
        .filter(|t| !t.is_empty())
        .collect();
    let texto = if partes.is_empty() { u["rawOutput"]["output"].as_str().unwrap_or_default().to_string() } else { partes.join("\n") };
    (texto.replace("\r\n", "\n"), saiu_com_erro)
}

/// Quanto se espera o `agy -p /usage` responder (leva uns 3 s).
const PRAZO_DO_USO: Duration = Duration::from_secs(20);

/// A cota do Gemini no Antigravity: cada grupo de modelos tem a sua janela (hoje semanal), e ela
/// gasta na proporção do custo dos tokens. O `agy -p /usage --output-format json` diz o que resta
/// sem abrir conversa nem gastar cota (é o que o Paseo lê). Até 24/11/2026 o Vitral achava que o
/// Antigravity não dizia a cota, e o anel do Gemini não existia (dono: "coloca o uso do gemini").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsoGemini {
    pub grupos: Vec<GrupoDoGemini>,
    pub medido_em: Option<i64>,
}

/// Um grupo de modelos e a janela dele: "Gemini" ou "Claude e GPT", a fração já usada e quando
/// renova.
#[derive(Clone, Debug, PartialEq)]
pub struct GrupoDoGemini {
    pub nome: String,
    pub janela: String,
    pub fracao: Option<f64>,
    pub renova: Option<i64>,
}

impl UsoGemini {
    /// O grupo dos modelos do Gemini (o do anel de fora).
    pub fn do_gemini(&self) -> Option<&GrupoDoGemini> {
        self.grupos.iter().find(|g| g.nome.to_lowercase().contains("gemini")).or(self.grupos.first())
    }
    /// O outro grupo (Claude e GPT pelo Antigravity), o do anel de dentro.
    pub fn dos_outros(&self) -> Option<&GrupoDoGemini> {
        let gemini = self.do_gemini();
        self.grupos.iter().find(|g| Some(*g) != gemini)
    }
    pub fn bloqueado(&self) -> bool {
        self.do_gemini().and_then(|g| g.fracao).is_some_and(|f| f >= 1.0)
    }
}

/// Pergunta a cota ao agy, rodando em `pasta` (a das perguntas, vazia). `None` sem agy, sem login
/// ou sem resposta no prazo. Bloqueia: chamar em segundo plano.
pub fn ler_uso(pasta: &Path) -> Option<UsoGemini> {
    if !instalado() || std::fs::create_dir_all(pasta).is_err() {
        return None;
    }
    let mut comando = processos::comando_leve(binario(), "uso-gemini");
    comando
        .args(["-p", "/usage", "--output-format", "json"])
        .current_dir(pasta)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (mut filho, escopo) = processos::abrir(comando).ok()?;
    let mut saida = filho.stdout.take()?;
    let (envia, recebe) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut tudo = String::new();
        let _ = saida.read_to_string(&mut tudo);
        let _ = envia.send(tudo);
    });
    let tudo = recebe.recv_timeout(PRAZO_DO_USO).ok();
    escopo.matar();
    let _ = filho.wait();
    let valor: Value = serde_json::from_str(tudo?.trim()).ok()?;
    uso_da_resposta(&valor, crate::modelo::agora() as i64)
}

/// Os grupos da resposta do `/usage` (`command.data.groups[].buckets[]`, com
/// `remaining_fraction` de 0 a 1 e `reset_time` ISO). `None` se não veio `SUCCESS` ou grupo algum.
pub fn uso_da_resposta(valor: &Value, agora: i64) -> Option<UsoGemini> {
    if valor["status"] != "SUCCESS" {
        return None;
    }
    let mut grupos = Vec::new();
    for grupo in valor["command"]["data"]["groups"].as_array()? {
        let nome = nome_do_grupo(grupo["name"].as_str().unwrap_or_default());
        let Some(balde) = grupo["buckets"].as_array().and_then(|b| b.iter().find(|b| b["disabled"] != true)) else {
            continue;
        };
        let renova = balde["reset_time"].as_str().and_then(crate::contas::segundos_iso);
        let mut fracao = balde["remaining_fraction"].as_f64().map(|resta| (1.0 - resta).clamp(0.0, 1.0));
        // A janela que já virou só recomeça no próximo uso.
        if renova.is_some_and(|r| r <= agora) {
            fracao = fracao.map(|_| 0.0);
        }
        grupos.push(GrupoDoGemini {
            nome,
            janela: balde["window"].as_str().unwrap_or_default().to_string(),
            fracao,
            renova,
        });
    }
    (!grupos.is_empty()).then_some(UsoGemini { grupos, medido_em: Some(agora) })
}

/// "Gemini Models" vira "Gemini"; "Claude and GPT models", "Claude e GPT".
fn nome_do_grupo(nome: &str) -> String {
    let nome = nome.trim();
    let sem_models = nome
        .strip_suffix(" Models")
        .or_else(|| nome.strip_suffix(" models"))
        .unwrap_or(nome);
    sem_models.replace(" and ", " e ")
}

#[cfg(test)]
mod testes {
    use super::*;

    #[derive(Clone)]
    struct Gravador(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Gravador {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn pedidos(g: &Gravador) -> Vec<Value> {
        String::from_utf8(g.0.lock().unwrap().clone()).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    fn opcoes(retomar: bool) -> Opcoes {
        Opcoes {
            agente: crate::modelo::Agente::Gemini,
            cwd: PathBuf::from("/casa/loja"),
            sessao: if retomar { "sessao-velha".into() } else { String::new() },
            retomar,
            bifurcar: None,
            cortar_em: None,
            modelo: Some("claude-sonnet-4-6".into()),
            permissao: "acceptEdits".into(),
            prompt_sistema: "Regras do Vitral.".into(),
            prompt_arquivo: None,
            ferramentas: None,
            url_mcp: String::new(),
            bloqueadas: Vec::new(),
            so_mcp_vitral: true,
            esforco: Some("xhigh".into()),
            ultracode: false,
            pasta_conta: None,
            memoria_nativa: false,
            sugestoes: false,
            mcp_da_equipe: None,
            eventos_de_hook: false,
            raciocinio_resumido: false,
        }
    }

    /// As opções de sessão como o agy 1.5.15 devolve (encurtadas).
    fn configuracoes() -> Value {
        json!([
            {"id": "model", "category": "model", "type": "select", "currentValue": "gemini-3.8-flash",
             "options": [{"value": "gemini-3.8-flash", "name": "Gemini 3.8 Flash"}, {"value": "claude-sonnet-4-6", "name": "Claude Sonnet 4.6 (Thinking)"}]},
            {"id": "thought_level", "type": "select", "currentValue": "high",
             "options": [{"value": "low", "name": "Low"}, {"value": "medium", "name": "Medium"}, {"value": "high", "name": "High"}]},
            {"id": "mode", "type": "select", "currentValue": "default",
             "options": [{"value": "default"}, {"value": "accept-edits"}, {"value": "auto"}, {"value": "plan"}]}
        ])
    }

    fn aberto(retomar: bool) -> (Protocolo, Arc<Mutex<Canal>>, Gravador, Vec<Evento>) {
        let g = Gravador(Arc::default());
        let canal = Arc::new(Mutex::new(Canal::novo(Box::new(g.clone()))));
        let mut p = Protocolo::novo(canal.clone(), &opcoes(retomar), "pedido", &[]);
        p.iniciar().unwrap();
        let mut eventos = p.linha(&json!({"jsonrpc": "2.0", "id": "vitral-init", "result": {"protocolVersion": 1}}));
        eventos.extend(p.linha(&json!({"jsonrpc": "2.0", "id": "vitral-sessao", "result": {"sessionId": "s1", "configOptions": configuracoes()}})));
        for opcao in ["model", "mode"] {
            eventos.extend(p.linha(&json!({"jsonrpc": "2.0", "id": format!("vitral-cfg-{opcao}"), "result": {}})));
        }
        (p, canal, g, eventos)
    }

    #[test]
    fn abre_a_sessao_com_o_prompt_de_sistema_e_configura_antes_do_pedido() {
        let (_, _, g, eventos) = aberto(false);
        let enviados = pedidos(&g);
        assert_eq!(enviados[0]["method"], "initialize");
        assert_eq!(enviados[0]["params"]["clientCapabilities"]["terminal"], false);
        assert_eq!(enviados[1]["method"], "session/new");
        assert_eq!(enviados[1]["params"]["cwd"], "/casa/loja");
        assert_eq!(enviados[1]["params"]["_meta"]["antigravity"]["systemPrompt"]["append"], "Regras do Vitral.");
        // O modelo e o modo mudam; o nível já é "high" (o xhigh do Vitral) e não vai.
        let configs: Vec<(&str, &str)> = enviados
            .iter()
            .filter(|v| v["method"] == "session/set_config_option")
            .map(|v| (v["params"]["configId"].as_str().unwrap(), v["params"]["value"].as_str().unwrap()))
            .collect();
        assert_eq!(configs, [("model", "claude-sonnet-4-6"), ("mode", "accept-edits")]);
        let prompt = enviados.last().unwrap();
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(prompt["params"], json!({"sessionId": "s1", "prompt": [{"type": "text", "text": "pedido"}]}));
        assert!(matches!(&eventos[0], Evento::Sessao(s) if s == "s1"));
        assert!(eventos.iter().any(|e| matches!(e, Evento::Inicio { modelo, .. } if modelo == "Claude Sonnet 4.6 (Thinking)")));
    }

    #[test]
    fn retomada_usa_session_resume_com_o_id_guardado() {
        let (_, _, g, _) = aberto(true);
        let enviados = pedidos(&g);
        assert_eq!(enviados[1]["method"], "session/resume");
        assert_eq!(enviados[1]["params"]["sessionId"], "sessao-velha");
    }

    #[test]
    fn turno_traduz_fala_raciocinio_ferramentas_e_termina_com_os_tokens() {
        let (mut p, canal, _, _) = aberto(false);
        let atualizar = |p: &mut Protocolo, u: Value| p.linha(&json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "s1", "update": u}}));
        let mut eventos = atualizar(&mut p, json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "pensando"}}));
        eventos.extend(atualizar(&mut p, json!({"sessionUpdate": "tool_call", "toolCallId": "c1", "name": "edit_file", "status": "in_progress", "kind": "edit",
            "args": {"Path": "/casa/loja/c.txt", "Target": "dois", "Replacement": "2"}})));
        eventos.extend(atualizar(&mut p, json!({"sessionUpdate": "tool_call_update", "toolCallId": "c1", "name": "edit_file", "status": "completed",
            "content": [{"type": "diff", "path": "/casa/loja/c.txt", "oldText": "dois", "newText": "2"}]})));
        eventos.extend(atualizar(&mut p, json!({"sessionUpdate": "tool_call", "toolCallId": "c2", "name": "run_command", "status": "completed",
            "args": {"CommandLine": "false"}, "rawOutput": {"output": "", "exit_code": 1}})));
        eventos.extend(atualizar(&mut p, json!({"sessionUpdate": "usage_update", "used": 9437, "size": 1048576})));
        eventos.extend(atualizar(&mut p, json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Feito"}})));
        eventos.extend(atualizar(&mut p, json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "."}})));
        eventos.extend(p.linha(&json!({"jsonrpc": "2.0", "id": "vitral-prompt-0", "result": {"stopReason": "end_turn",
            "usage": {"inputTokens": 5641, "outputTokens": 811, "cachedReadTokens": 14303, "cachedWriteTokens": 0}}})));
        assert!(matches!(eventos[0], Evento::Pensando));
        assert!(matches!(&eventos[1], Evento::Pensamento(t) if t == "pensando"));
        assert!(matches!(&eventos[2], Evento::Ferramenta { id, nome, entrada } if id == "c1" && nome == "Edit" && entrada["old_string"] == "dois" && entrada["new_string"] == "2"));
        assert!(matches!(&eventos[3], Evento::ResultadoFerramenta { id, conteudo, erro: false, .. } if id == "c1" && conteudo == "mudou /casa/loja/c.txt"));
        assert!(matches!(&eventos[4], Evento::Ferramenta { nome, entrada, .. } if nome == "Bash" && entrada["command"] == "false"));
        assert!(matches!(&eventos[5], Evento::ResultadoFerramenta { erro: true, .. }));
        assert!(matches!(eventos[6], Evento::Contexto(9437)));
        assert!(matches!(&eventos[7], Evento::TextoParcial(t) if t == "Feito"));
        assert!(matches!(&eventos[9], Evento::Gasto(t) if t.entrada == 5641 && t.cache_lido == 14303 && t.saida == 811));
        assert!(matches!(&eventos[10], Evento::Texto(t) if t == "Feito."));
        assert!(matches!(&eventos[11], Evento::Fim { erro: false, texto, janela: Some(1048576), .. } if texto == "Feito."));
        assert!(!canal.lock().unwrap().aberta());
    }

    #[test]
    fn mensagem_no_meio_do_turno_vai_como_outro_prompt_e_o_fim_espera_a_ultima_resposta() {
        let (mut p, canal, g, _) = aberto(false);
        canal.lock().unwrap().dirigir("e diga BANANA", &[]).unwrap();
        let ultimo = pedidos(&g).last().unwrap().clone();
        assert_eq!(ultimo["method"], "session/prompt");
        assert_eq!(ultimo["params"]["prompt"][0]["text"], "e diga BANANA");
        // O primeiro pedido responde na hora em que o segundo entra: o turno ainda não acabou.
        let eventos = p.linha(&json!({"jsonrpc": "2.0", "id": "vitral-prompt-0", "result": {"stopReason": "end_turn"}}));
        assert!(!eventos.iter().any(|e| matches!(e, Evento::Fim { .. })));
        assert!(canal.lock().unwrap().aberta());
        let eventos = p.linha(&json!({"jsonrpc": "2.0", "id": ultimo["id"], "result": {"stopReason": "end_turn"}}));
        assert!(matches!(&eventos[0], Evento::DirecaoConfirmada { texto, aceita: true } if texto == "e diga BANANA"));
        assert!(matches!(eventos.last(), Some(Evento::Fim { erro: false, .. })));
    }

    #[test]
    fn mensagem_antes_da_sessao_espera_e_vai_depois_do_pedido_inicial() {
        let g = Gravador(Arc::default());
        let canal = Arc::new(Mutex::new(Canal::novo(Box::new(g.clone()))));
        canal.lock().unwrap().dirigir("cedo", &[]).unwrap();
        let mut p = Protocolo::novo(canal, &Opcoes { modelo: None, esforco: None, permissao: "manual".into(), ..opcoes(false) }, "primeiro", &[]);
        p.iniciar().unwrap();
        p.linha(&json!({"jsonrpc": "2.0", "id": "vitral-init", "result": {}}));
        p.linha(&json!({"jsonrpc": "2.0", "id": "vitral-sessao", "result": {"sessionId": "s2", "configOptions": configuracoes()}}));
        let prompts: Vec<String> = pedidos(&g)
            .iter()
            .filter(|v| v["method"] == "session/prompt")
            .map(|v| v["params"]["prompt"][0]["text"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(prompts, ["primeiro", "cedo"]);
    }

    #[test]
    fn parar_manda_cancel_e_o_fim_diz_que_foi_interrompido() {
        let (mut p, canal, g, _) = aberto(false);
        canal.lock().unwrap().interromper().unwrap();
        assert_eq!(pedidos(&g).last().unwrap(), &json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": "s1"}}));
        assert!(!canal.lock().unwrap().aberta());
        let eventos = p.linha(&json!({"jsonrpc": "2.0", "id": "vitral-prompt-0", "result": {"stopReason": "cancelled"}}));
        assert!(matches!(eventos.last(), Some(Evento::Fim { erro: true, texto, .. }) if texto == "O turno foi interrompido."));
    }

    #[test]
    fn permissao_vira_cartao_e_a_resposta_volta_ao_agy_com_a_opcao_certa() {
        let (mut p, _, g, _) = aberto(false);
        let pedido = json!({"jsonrpc": "2.0", "id": "1", "method": "session/request_permission", "params": {"sessionId": "s1",
            "toolCall": {"toolCallId": "c3", "name": "run_command", "args": {"CommandLine": "ls -la"}, "kind": "execute"},
            "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}, {"optionId": "deny", "name": "Deny", "kind": "reject_once"}]}});
        let eventos = p.linha(&pedido);
        let [Evento::Permissao { ferramenta, entrada, resposta }] = eventos.as_slice() else { panic!("{eventos:?}") };
        assert_eq!(ferramenta, "Bash");
        assert_eq!(entrada["command"], "ls -la");
        resposta.try_send(Resposta::ok(json!({"behavior": "deny", "message": "não"}).to_string())).unwrap();
        let fim = std::time::Instant::now() + Duration::from_secs(2);
        while !pedidos(&g).iter().any(|v| v["id"] == "1") && std::time::Instant::now() < fim {
            std::thread::sleep(Duration::from_millis(10));
        }
        let resposta = pedidos(&g).into_iter().find(|v| v["id"] == "1").unwrap();
        assert_eq!(resposta["result"], json!({"outcome": {"outcome": "selected", "optionId": "deny"}}));
    }

    #[test]
    fn permissao_sem_resposta_volta_cancelada() {
        let (mut p, _, g, _) = aberto(false);
        let eventos = p.linha(&json!({"jsonrpc": "2.0", "id": "7", "method": "session/request_permission", "params": {"sessionId": "s1",
            "toolCall": {"toolCallId": "c4", "name": "write_file", "args": {"Path": "/casa/loja/b.txt", "Content": "oi"}},
            "options": [{"optionId": "allow", "kind": "allow_once"}, {"optionId": "deny", "kind": "reject_once"}]}}));
        drop(eventos);
        let fim = std::time::Instant::now() + Duration::from_secs(2);
        while !pedidos(&g).iter().any(|v| v["id"] == "7") && std::time::Instant::now() < fim {
            std::thread::sleep(Duration::from_millis(10));
        }
        let resposta = pedidos(&g).into_iter().find(|v| v["id"] == "7").unwrap();
        assert_eq!(resposta["result"]["outcome"]["outcome"], "cancelled");
    }

    #[test]
    fn conversa_que_o_agy_nao_tem_mais_abre_uma_nova() {
        let g = Gravador(Arc::default());
        let canal = Arc::new(Mutex::new(Canal::novo(Box::new(g.clone()))));
        let mut p = Protocolo::novo(canal, &opcoes(true), "oi", &[]);
        p.iniciar().unwrap();
        p.linha(&json!({"jsonrpc": "2.0", "id": "vitral-init", "result": {}}));
        let eventos = p.linha(&json!({"jsonrpc": "2.0", "id": "vitral-sessao", "error": {"code": -32002, "message": "session not found"}}));
        assert!(eventos.is_empty());
        let enviados = pedidos(&g);
        assert_eq!(enviados[1]["method"], "session/resume");
        assert_eq!(enviados[2]["method"], "session/new");
    }

    #[test]
    fn login_pedido_vira_erro_que_diz_como_entrar() {
        let g = Gravador(Arc::default());
        let canal = Arc::new(Mutex::new(Canal::novo(Box::new(g))));
        let mut p = Protocolo::novo(canal, &opcoes(false), "oi", &[]);
        p.iniciar().unwrap();
        p.linha(&json!({"jsonrpc": "2.0", "id": "vitral-init", "result": {}}));
        let eventos = p.linha(&json!({"jsonrpc": "2.0", "id": "vitral-sessao", "error": {"code": -32000, "message": "Authentication required"}}));
        assert!(matches!(eventos.as_slice(), [Evento::Fim { erro: true, texto, .. }] if texto.contains("Authentication required") && texto.contains("agy acp --login")));
    }

    #[test]
    fn pedido_do_agente_que_o_vitral_nao_oferece_volta_erro() {
        let (mut p, _, g, _) = aberto(false);
        p.linha(&json!({"jsonrpc": "2.0", "id": 9, "method": "fs/read_text_file", "params": {"path": "/x"}}));
        assert_eq!(pedidos(&g).last().unwrap()["error"]["code"], -32601);
    }

    #[test]
    fn modelos_juntam_as_variantes_de_nivel_e_deixam_o_sufixo_sozinho() {
        let lista = "Fetching available models...\n\
            gemini-3.8-flash-high\tGemini 3.8 Flash (High)\n\
            gemini-3.8-flash-medium\tGemini 3.8 Flash (Medium)\n\
            gemini-3.8-flash-low\tGemini 3.8 Flash (Low)\n\
            claude-sonnet-4-6\tClaude Sonnet 4.6 (Thinking)\n\
            gpt-oss-120b-medium\tGPT-OSS 120B (Medium)\n";
        let modelos = modelos_da_lista(lista);
        let pares: Vec<(&str, &str)> = modelos.iter().map(|m| (m.valor.as_str(), m.nome.as_str())).collect();
        assert_eq!(
            pares,
            [
                ("gemini-3.8-flash", "Gemini 3.8 Flash"),
                ("claude-sonnet-4-6", "Claude Sonnet 4.6 (Thinking)"),
                ("gpt-oss-120b-medium", "GPT-OSS 120B (Medium)"),
            ]
        );
    }

    #[test]
    fn modos_e_niveis_do_vitral_viram_os_do_agy() {
        assert_eq!(argumentos(&Opcoes { permissao: "bypassPermissions".into(), ..opcoes(false) }), ["acp", "--disable-telemetry", "--dangerously-skip-permissions"]);
        assert_eq!(argumentos(&opcoes(false)), ["acp", "--disable-telemetry"]);
        assert_eq!(modo_de("manual"), "default");
        assert_eq!(modo_de("acceptEdits"), "accept-edits");
        assert_eq!(nivel_de("max"), Some("high"));
        assert_eq!(nivel_de(""), None);
    }
}

#[cfg(test)]
mod testes_do_uso {
    use super::uso_da_resposta;
    use serde_json::json;

    /// A resposta de verdade do agy 24/11/2026, com a cota da Gemini em 30% usada.
    #[test]
    fn le_a_cota_de_cada_grupo() {
        let resposta = json!({
            "status": "SUCCESS",
            "command": { "name": "usage", "data": { "groups": [
                { "name": "Gemini Models", "buckets": [
                    { "id": "gemini-weekly", "name": "Weekly Limit Remaining", "window": "weekly",
                      "remaining_fraction": 0.7, "reset_time": "2026-12-01T11:32:05Z" } ] },
                { "name": "Claude and GPT models", "buckets": [
                    { "id": "3p-weekly", "name": "Weekly Limit Remaining", "window": "weekly",
                      "remaining_fraction": 1, "reset_time": "2026-12-01T11:32:05Z" } ] }
            ] } }
        });
        let uso = uso_da_resposta(&resposta, 1_795_000_000).expect("uso");
        let gemini = uso.do_gemini().unwrap();
        assert_eq!(gemini.nome, "Gemini");
        assert!((gemini.fracao.unwrap() - 0.3).abs() < 1e-9);
        assert!(gemini.renova.is_some());
        let outros = uso.dos_outros().unwrap();
        assert_eq!(outros.nome, "Claude e GPT");
        assert_eq!(outros.fracao, Some(0.0));
        assert!(!uso.bloqueado());
    }

    #[test]
    fn sem_sucesso_ou_sem_grupo_nao_ha_uso() {
        assert!(uso_da_resposta(&json!({ "status": "ERROR", "error": "not signed in" }), 0).is_none());
        assert!(uso_da_resposta(&json!({ "status": "SUCCESS", "command": { "data": { "groups": [] } } }), 0).is_none());
    }

    #[test]
    fn janela_que_ja_virou_volta_a_zero() {
        let resposta = json!({ "status": "SUCCESS", "command": { "data": { "groups": [
            { "name": "Gemini Models", "buckets": [
                { "window": "weekly", "remaining_fraction": 0.0, "reset_time": "2026-11-01T00:00:00Z" } ] } ] } } });
        let uso = uso_da_resposta(&resposta, 1_795_000_000).unwrap();
        assert_eq!(uso.do_gemini().unwrap().fracao, Some(0.0));
    }
}

#[cfg(test)]
mod testes_do_uso_de_verdade {
    /// Com o agy de verdade (rodar à mão: `cargo test -- --ignored ler_uso_de_verdade --nocapture`).
    #[test]
    #[ignore]
    fn ler_uso_de_verdade() {
        let pasta = std::env::temp_dir().join("vitral-uso-gemini");
        let uso = super::ler_uso(&pasta);
        eprintln!("{uso:?}");
        assert!(uso.is_some());
    }
}

#[cfg(test)]
mod testes_das_pastas_no_windows {
    use super::pastas_do_agy;
    use std::path::{Path, PathBuf};

    #[test]
    fn no_windows_o_agy_mora_no_localappdata() {
        assert!(pastas_do_agy(true, Path::new("/casa"), Some(PathBuf::from("/local"))).contains(&PathBuf::from("/local/agy/bin")));
        assert_eq!(pastas_do_agy(false, Path::new("/casa"), Some(PathBuf::from("/local"))), vec![PathBuf::from("/casa/.local/bin")]);
    }
}
