//! Um processo do Claude Code em modo `--print` com entrada e saída `stream-json`.
//!
//! Cada vez (turno) de um chat roda num processo: o Vitral escreve a mensagem na entrada,
//! lê os eventos da saída numa thread e fecha a entrada quando o turno acaba, o que encerra o
//! processo. A próxima mensagem abre outro com `--resume`, então chats parados não gastam memória.

use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use serde_json::{Value, json};

use crate::processos::{self, Escopo};

#[derive(Clone, Debug)]
pub struct Opcoes {
    pub cwd: PathBuf,
    pub sessao: String,
    pub retomar: bool,
    /// A conversa do Claude Code de onde a primeira vez continua, numa cópia dela (`--resume <id>
    /// --fork-session`, com a sessão nova em `sessao`): a original fica como está.
    pub bifurcar: Option<String>,
    pub modelo: Option<String>,
    pub permissao: String,
    pub prompt_sistema: String,
    /// O mesmo prompt já gravado num arquivo: vai por `--append-system-prompt-file`, com
    /// `--system-prompt-snapshot off` (vale já no turno seguinte de uma sessão retomada).
    pub prompt_arquivo: Option<PathBuf>,
    /// `--tools`: a lista do que o chat pode usar (o resto não existe para ele).
    pub ferramentas: Option<String>,
    pub url_mcp: String,
    pub bloqueadas: Vec<String>,
    pub pastas_extras: Vec<PathBuf>,
    /// Só o servidor do Vitral, sem os MCP do usuário (o coordenador não precisa deles).
    pub so_mcp_vitral: bool,
    /// `--effort` (low, medium, high, xhigh, max); vazio usa o padrão.
    pub esforco: Option<String>,
    /// Ultracode neste turno: `--effort ultracode` (Extra mais fluxos de agentes), com os fluxos
    /// ligados nas configurações e o limite de agentes ao mesmo tempo.
    pub ultracode: bool,
    /// `CLAUDE_CONFIG_DIR` de uma conta extra; `None` é o login normal.
    pub pasta_conta: Option<PathBuf>,
    /// Variáveis de ambiente a mais (ex.: VITRAL_PORT).
    pub ambiente: Vec<(String, String)>,
    /// JSON passado em `--settings` (ex.: hook que roda os testes ao parar).
    pub configuracoes: Option<Value>,
}

/// Versão do Claude Code (2.1.278 vira `(2, 1, 278)`).
pub type Versao = (u32, u32, u32);

/// A versão em que o Ultracode foi conferido: `--effort ultracode`, o teto da espera
/// (`CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS`) e o limite de agentes, que só existe desde a 2.1.269
/// (changelog embutido: "Added CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS"). Antes dela, o
/// limite de 3 seria ignorado calado; num valor de esforço que não conhece, o Claude Code avisa
/// e roda no esforço padrão.
pub const VERSAO_ULTRACODE: Versao = (2, 1, 278);

/// Agentes de um fluxo rodando ao mesmo tempo, em todo turno (o padrão do Claude Code seria 6
/// neste PC, e a fonte não segura a carga). O limite vale por fluxo.
pub const AGENTES_DO_ULTRACODE: u32 = 3;

/// Uma mensagem para o Claude: texto e imagens anexadas.
pub struct Mensagem<'a> {
    pub texto: &'a str,
    pub imagens: &'a [PathBuf],
}

#[derive(Debug)]
pub enum Evento {
    Inicio {
        modelo: String,
        comandos: Vec<String>,
        /// `claude_code_version` do init.
        versao: Option<Versao>,
        /// A ferramenta `Workflow` veio na lista (fluxos de agentes ligados); `None` sem lista.
        fluxos: Option<bool>,
    },
    /// Agentes e fluxos ainda rodando em segundo plano (o Claude Code abre outro turno quando
    /// eles terminam).
    Fundo(usize),
    /// Tokens que a conversa ocupa no contexto agora.
    Contexto(u64),
    TextoParcial(String),
    Texto(String),
    Ferramenta {
        id: String,
        nome: String,
        entrada: Value,
    },
    ResultadoFerramenta {
        id: String,
        conteudo: String,
        erro: bool,
    },
    Fim {
        erro: bool,
        texto: String,
        custo_usd: f64,
        janela: Option<u64>,
    },
    Limite {
        uso_5h: Option<f64>,
        uso_semana: Option<f64>,
        /// Quando cada janela renova (segundos Unix).
        renova_5h: Option<i64>,
        renova_semana: Option<i64>,
        bloqueado: bool,
        /// Quando o plano volta a aceitar pedidos (segundos Unix).
        bloqueado_ate: Option<u64>,
    },
    Saiu {
        stderr: String,
        /// Código de saída (-1 se morreu por sinal); `None` se não deu para ler.
        codigo: Option<i32>,
    },
}

pub struct Processo {
    filho: Child,
    entrada: Option<ChildStdin>,
    escopo: Escopo,
    parado: bool,
}

impl Processo {
    pub fn iniciar(
        opcoes: &Opcoes,
        mensagem: Mensagem,
        mut enviar: impl FnMut(Evento) + Send + 'static,
    ) -> Result<Self> {
        let mut comando = processos::comando(binario_claude(), "claude");
        comando.current_dir(&opcoes.cwd).args(argumentos(opcoes));
        if let Some(pasta) = &opcoes.pasta_conta {
            comando.env("CLAUDE_CONFIG_DIR", pasta);
        }
        for (chave, valor) in ambiente(opcoes) {
            comando.env(chave, valor);
        }
        // Aberto de dentro de outra sessão do Claude Code, o filho herdaria a marca dela.
        for variavel in [
            "CLAUDECODE",
            "CLAUDE_CODE_ENTRYPOINT",
            "CLAUDE_CODE_SSE_PORT",
        ] {
            comando.env_remove(variavel);
        }
        comando
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let (mut filho, escopo) = processos::abrir(comando).context("não abriu o claude")?;
        let pid = filho.id();
        let saida = filho.stdout.take().unwrap();
        let erros = filho.stderr.take().unwrap();
        let mut entrada = filho.stdin.take();

        // O stderr é lido junto: cheio (64 KiB) sem ninguém ler, ele travaria o claude.
        let ultimas = Arc::new(Mutex::new(VecDeque::new()));
        let leitura_erros = std::thread::spawn({
            let ultimas = ultimas.clone();
            move || {
                for linha in BufReader::new(erros).lines().map_while(Result::ok) {
                    let mut ultimas = ultimas.lock().unwrap_or_else(|e| e.into_inner());
                    if ultimas.len() == 20 {
                        ultimas.pop_front();
                    }
                    ultimas.push_back(linha);
                }
            }
        });
        std::thread::spawn(move || {
            for linha in BufReader::new(saida).lines() {
                let Ok(linha) = linha else { break };
                let Ok(valor) = serde_json::from_str::<Value>(&linha) else {
                    continue;
                };
                for evento in interpretar(&valor) {
                    enviar(evento);
                }
            }
            // Um neto que herdou o stderr pode segurá-lo aberto: não espera mais que 2 s. O código
            // de saída é lido sem colher o processo (quem colhe é o `Processo`).
            let limite = Instant::now() + Duration::from_secs(2);
            let mut codigo = processos::codigo_de_saida(pid);
            while (!leitura_erros.is_finished() || codigo.is_none()) && Instant::now() < limite {
                std::thread::sleep(Duration::from_millis(20));
                codigo = codigo.or_else(|| processos::codigo_de_saida(pid));
            }
            let stderr = Vec::from(ultimas.lock().unwrap_or_else(|e| e.into_inner()).clone()).join("\n");
            enviar(Evento::Saiu { stderr, codigo });
        });

        if let Some(entrada) = entrada.as_mut() {
            escrever_mensagem(entrada, mensagem)?;
        }
        Ok(Self {
            filho,
            entrada,
            escopo,
            parado: false,
        })
    }

    /// Encerra a entrada: o processo termina depois do turno atual.
    pub fn fechar_entrada(&mut self) {
        self.entrada = None;
    }

    pub fn interromper(&mut self) {
        self.entrada = None;
        // SAFETY: o processo ainda não foi colhido (é do `Child`), então o número é dele.
        #[cfg(unix)]
        unsafe {
            libc::kill(self.filho.id() as i32, libc::SIGINT);
        }
        // Sem console, no Windows não há Ctrl+C para mandar: por enquanto, para tudo.
        #[cfg(windows)]
        self.escopo.parar();
    }

    /// SIGTERM no claude e em tudo que ele abriu (com SIGKILL ele não limpa nada e os shells,
    /// servidores e hooks ficam rodando sozinhos).
    pub fn matar(&mut self) {
        self.entrada = None;
        if !self.parado && self.esperar().is_none() {
            self.parado = true;
            self.escopo.parar();
        }
    }

    /// Código de saída, ou -1 se morreu por sinal; None enquanto roda.
    pub fn esperar(&mut self) -> Option<i32> {
        self.filho.try_wait().ok().flatten().map(|s| s.code().unwrap_or(-1))
    }
}

impl Drop for Processo {
    fn drop(&mut self) {
        if self.esperar().is_some() {
            return;
        }
        self.matar();
        // A limpeza do claude leva alguns segundos: a espera fica numa thread para não travar a tela.
        processos::colher(self.filho.id(), Duration::from_secs(8));
    }
}

/// Os argumentos do `claude` para um turno (sem o binário).
fn argumentos(opcoes: &Opcoes) -> Vec<String> {
    let mcp = json!({
        "mcpServers": { "vitral": { "type": "http", "url": opcoes.url_mcp } }
    })
    .to_string();
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-mode",
        &opcoes.permissao,
        "--mcp-config",
        &mcp,
        "--allowedTools",
        "mcp__vitral",
    ]
    .map(String::from)
    .to_vec();
    match &opcoes.prompt_arquivo {
        Some(arquivo) => args.extend([
            "--append-system-prompt-file".to_string(),
            arquivo.display().to_string(),
            "--system-prompt-snapshot".to_string(),
            "off".to_string(),
        ]),
        None => args.extend(["--append-system-prompt".to_string(), opcoes.prompt_sistema.clone()]),
    }
    if let Some(ferramentas) = opcoes.ferramentas.as_deref().filter(|f| !f.is_empty()) {
        args.extend(["--tools".to_string(), ferramentas.to_string()]);
    }
    if opcoes.permissao != "bypassPermissions" {
        args.extend(["--permission-prompt-tool", "mcp__vitral__permissao"].map(String::from));
    }
    // Aprovar o plano pode passar para "Sem perguntar" já neste turno.
    if opcoes.permissao == "plan" {
        args.push("--allow-dangerously-skip-permissions".into());
    }
    match (opcoes.retomar, &opcoes.bifurcar) {
        (true, _) => args.extend(["--resume", &opcoes.sessao].map(String::from)),
        (false, Some(origem)) => {
            args.extend(["--resume", origem, "--fork-session", "--session-id", &opcoes.sessao].map(String::from))
        }
        (false, None) => args.extend(["--session-id", &opcoes.sessao].map(String::from)),
    }
    if let Some(modelo) = opcoes.modelo.as_deref().filter(|m| !m.is_empty()) {
        args.extend(["--model", modelo].map(String::from));
    }
    // O Ultracode vai como nível de esforço: o único jeito que liga tanto antes da 2.1.284 (lá
    // ele só vale em Extra) quanto depois.
    let esforco = if opcoes.ultracode { Some("ultracode") } else { opcoes.esforco.as_deref() };
    if let Some(esforco) = esforco.filter(|e| !e.is_empty()) {
        args.extend(["--effort", esforco].map(String::from));
    }
    if let Some(configuracoes) = configuracoes_do_turno(opcoes) {
        args.extend(["--settings".to_string(), configuracoes]);
    }
    if opcoes.so_mcp_vitral {
        // Poucas ferramentas: carregadas de uma vez, sem a etapa de ToolSearch.
        args.push("--strict-mcp-config".into());
    }
    // Fluxos de agentes só pela opção do Ultracode: com os fluxos ligados na conta, o Claude
    // poderia abrir um sozinho (por exemplo, relançar o que caiu junto com o processo anterior).
    let mut bloqueadas = opcoes.bloqueadas.clone();
    if !opcoes.ultracode && !bloqueadas.iter().any(|b| b == "Workflow") {
        bloqueadas.push("Workflow".into());
    }
    if !bloqueadas.is_empty() {
        args.push("--disallowedTools".into());
        args.extend(bloqueadas);
    }
    for pasta in &opcoes.pastas_extras {
        args.extend(["--add-dir".to_string(), pasta.display().to_string()]);
    }
    args
}

/// O JSON de `--settings`: o que o motor mandou e, no Ultracode, os fluxos de agentes ligados
/// (no plano Pro eles começam desligados) e o próprio Ultracode. Tudo num `--settings` só.
fn configuracoes_do_turno(opcoes: &Opcoes) -> Option<String> {
    let mut configuracoes = opcoes.configuracoes.clone();
    if opcoes.ultracode {
        let objeto = configuracoes.get_or_insert_with(|| json!({}));
        objeto["enableWorkflows"] = json!(true);
        objeto["ultracode"] = json!(true);
    }
    configuracoes.map(|c| c.to_string())
}

/// Variáveis de ambiente do turno, além da pasta da conta.
fn ambiente(opcoes: &Opcoes) -> Vec<(String, String)> {
    let mut ambiente = opcoes.ambiente.clone();
    let mut por = |chave: &str, valor: String| ambiente.push((chave.to_string(), valor));
    if opcoes.so_mcp_vitral {
        por("ENABLE_TOOL_SEARCH", "false".into());
    }
    // Pedidos de permissão podem esperar o usuário por muito tempo.
    por("MCP_TOOL_TIMEOUT", "86400000".into());
    // Em todo turno: um fluxo que escape da opção (num Claude Code que o deixe passar) também
    // fica no limite.
    por("CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS", AGENTES_DO_ULTRACODE.to_string());
    if opcoes.ultracode {
        // Com a entrada fechada, o Claude Code espera os agentes do fluxo (o padrão corta em 10
        // min); o Parar do Vitral continua valendo.
        por("CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS", "0".into());
    }
    ambiente
}

/// "2.1.278 (Claude Code)", ou o nome da pasta da versão, vira `(2, 1, 278)`.
pub fn versao_de(texto: &str) -> Option<Versao> {
    let primeira = texto.split_whitespace().next()?;
    let mut partes = primeira.split('.').map(|p| p.parse::<u32>().ok());
    let versao = (partes.next()??, partes.next()??, partes.next()??);
    partes.next().is_none().then_some(versao)
}

/// A versão do `claude` que o Vitral abre: o instalador nativo deixa um link para
/// `versions/<versão>`; sem ele, pergunta ao próprio `claude --version`.
pub fn versao_instalada() -> Option<Versao> {
    let binario = binario_claude();
    if let Some(versao) = dunce::canonicalize(&binario)
        .ok()
        .and_then(|c| c.file_name().and_then(|n| versao_de(&n.to_string_lossy())))
    {
        return Some(versao);
    }
    let saida = processos::novo_comando(&binario)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    versao_de(&String::from_utf8_lossy(&saida.stdout))
}

fn escrever_mensagem(entrada: &mut ChildStdin, mensagem: Mensagem) -> Result<()> {
    let mut conteudo = vec![json!({ "type": "text", "text": mensagem.texto })];
    for caminho in mensagem.imagens {
        let Some(tipo) = tipo_imagem(caminho) else {
            continue;
        };
        let bytes = std::fs::read(caminho).with_context(|| caminho.display().to_string())?;
        conteudo.push(json!({
            "type": "image",
            "source": { "type": "base64", "media_type": tipo, "data": base64(&bytes) },
        }));
    }
    let linha = json!({
        "type": "user",
        "message": { "role": "user", "content": conteudo },
    });
    writeln!(entrada, "{linha}")?;
    entrada.flush()?;
    Ok(())
}

fn interpretar(valor: &Value) -> Vec<Evento> {
    // Mensagens de subagentes (ferramenta Task) ficam de fora: só o fio principal aparece.
    if !valor["parent_tool_use_id"].is_null() {
        return Vec::new();
    }
    let texto = |v: &Value| v.as_str().unwrap_or_default().to_string();
    match valor["type"].as_str().unwrap_or_default() {
        "system" if valor["subtype"] == "init" => vec![Evento::Inicio {
            modelo: texto(&valor["model"]),
            comandos: valor["slash_commands"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c.as_str().map(str::to_string))
                .filter(|c| !c.starts_with("__"))
                .collect(),
            versao: valor["claude_code_version"].as_str().and_then(versao_de),
            fluxos: valor["tools"]
                .as_array()
                .filter(|f| !f.is_empty())
                .map(|f| f.iter().any(|n| n == "Workflow")),
        }],
        // Só o que o Claude Code espera antes de sair: fluxos e subagentes em segundo plano (os
        // comandos de shell ele encerra sozinho).
        "system" if valor["subtype"] == "background_tasks_changed" => vec![Evento::Fundo(
            valor["tasks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|t| matches!(t["task_type"].as_str(), Some("local_workflow" | "local_agent")))
                .filter(|t| t["ambient"] != true)
                .count(),
        )],
        "stream_event" => {
            let evento = &valor["event"];
            if evento["type"] == "content_block_delta" && evento["delta"]["type"] == "text_delta" {
                vec![Evento::TextoParcial(texto(&evento["delta"]["text"]))]
            } else {
                Vec::new()
            }
        }
        "assistant" => {
            let uso = &valor["message"]["usage"];
            let contexto = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"]
                .iter()
                .filter_map(|chave| uso[chave].as_u64())
                .sum::<u64>();
            let mut eventos: Vec<Evento> = valor["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|bloco| match bloco["type"].as_str()? {
                    "text" => Some(Evento::Texto(texto(&bloco["text"]))),
                    "tool_use" => Some(Evento::Ferramenta {
                        id: texto(&bloco["id"]),
                        nome: texto(&bloco["name"]),
                        entrada: bloco["input"].clone(),
                    }),
                    _ => None,
                })
                .collect();
            if contexto > 0 {
                eventos.push(Evento::Contexto(contexto));
            }
            eventos
        }
        "user" => valor["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|bloco| bloco["type"] == "tool_result")
            .map(|bloco| Evento::ResultadoFerramenta {
                id: texto(&bloco["tool_use_id"]),
                conteudo: conteudo_resultado(&bloco["content"]),
                erro: bloco["is_error"].as_bool().unwrap_or(false),
            })
            .collect(),
        "result" => vec![Evento::Fim {
            erro: valor["is_error"].as_bool().unwrap_or(false),
            texto: valor["result"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| texto(&valor["subtype"])),
            custo_usd: valor["total_cost_usd"].as_f64().unwrap_or(0.0),
            janela: valor["modelUsage"]
                .as_object()
                .into_iter()
                .flat_map(|modelos| modelos.values())
                .filter_map(|m| m["contextWindow"].as_u64())
                .max(),
        }],
        "rate_limit_event" => {
            let info = &valor["rate_limit_info"];
            // Frações de 0 a 1; sem `unifiedWindows`, vale ao menos a janela do aviso.
            let janela = |nome: &str| {
                let j = &info["unifiedWindows"][nome];
                if j.is_object() {
                    (j["utilization"].as_f64(), j["resetsAt"].as_i64())
                } else if info["rateLimitType"] == nome {
                    (info["utilization"].as_f64(), info["resetsAt"].as_i64())
                } else {
                    (None, None)
                }
            };
            let (uso_5h, renova_5h) = janela("five_hour");
            let (uso_semana, renova_semana) = janela("seven_day");
            vec![Evento::Limite {
                uso_5h,
                uso_semana,
                renova_5h,
                renova_semana,
                bloqueado: info["status"] == "rejected",
                bloqueado_ate: info["resetsAt"].as_f64().map(|s| s as u64),
            }]
        }
        _ => Vec::new(),
    }
}

fn conteudo_resultado(conteudo: &Value) -> String {
    match conteudo {
        Value::String(s) => s.clone(),
        Value::Array(partes) => partes
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        outro => outro.to_string(),
    }
}

/// O `claude` do PATH ou o de `~/.local/bin` (apps abertos pela dock nem sempre têm esse PATH).
pub fn binario_claude() -> PathBuf {
    if let Some(caminho) = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|p| p.join("claude"))
            .find(|p| p.is_file())
    }) {
        return caminho;
    }
    let home = dirs::home_dir().unwrap_or_default();
    [".local/bin/claude", ".claude/local/claude"]
        .iter()
        .map(|p| home.join(p))
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("claude"))
}

/// A versão a partir da qual o prompt vai por arquivo, sem a foto da sessão
/// (`--system-prompt-snapshot off`, conferido no 2.1.278).
pub const VERSAO_PROMPT_POR_ARQUIVO: Versao = (2, 1, 278);

/// A sessão gravada sumiu da pasta do projeto que ainda existe: o Claude Code a apagou (o
/// `cleanupPeriodDays`, 30 dias por padrão). Sem a pasta do projeto não dá para saber, e quem
/// responde é o erro do `--resume`.
pub fn sessao_expirou(pasta_conta: Option<&std::path::Path>, cwd: &std::path::Path, sessao: &str) -> bool {
    let Some(config) = pasta_de_config(pasta_conta) else {
        return false;
    };
    let projetos: Vec<PathBuf> = [Some(cwd.to_path_buf()), dunce::canonicalize(cwd).ok()]
        .into_iter()
        .flatten()
        .map(|pasta| config.join("projects").join(nome_do_projeto(&pasta.to_string_lossy())))
        .collect();
    projetos.iter().any(|p| p.is_dir()) && !sessao_existe(pasta_conta, cwd, sessao)
}

fn pasta_de_config(pasta_conta: Option<&std::path::Path>) -> Option<PathBuf> {
    pasta_conta
        .map(|p| p.to_path_buf())
        .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from))
        .or_else(|| dirs::home_dir().map(|h| h.join(".claude")))
}

/// O Claude Code já gravou conversa nesta sessão: com `--session-id` ele recusaria ("already in
/// use"), então o turno tem de ir com `--resume`. As contas extras têm `projects` ligado ao normal.
pub fn sessao_existe(pasta_conta: Option<&std::path::Path>, cwd: &std::path::Path, sessao: &str) -> bool {
    let Some(config) = pasta_de_config(pasta_conta) else {
        return false;
    };
    [Some(cwd.to_path_buf()), dunce::canonicalize(cwd).ok()]
        .into_iter()
        .flatten()
        .any(|pasta| {
            config
                .join("projects")
                .join(nome_do_projeto(&pasta.to_string_lossy()))
                .join(format!("{sessao}.jsonl"))
                .is_file()
        })
}

/// Nome da pasta do projeto em `projects`, como o Claude Code monta: o que não é letra ou número
/// vira `-`; acima de 200 caracteres, corta e acrescenta um hash do caminho em base 36.
fn nome_do_projeto(caminho: &str) -> String {
    let unidades: Vec<u16> = caminho.encode_utf16().collect();
    let nome: String = unidades
        .iter()
        .map(|&u| match char::from_u32(u as u32) {
            Some(c) if c.is_ascii_alphanumeric() => c,
            _ => '-',
        })
        .collect();
    if nome.len() <= 200 {
        return nome;
    }
    let hash = unidades
        .iter()
        .fold(0i32, |h, &u| (h << 5).wrapping_sub(h).wrapping_add(u as i32));
    let mut resto = (hash as i64).unsigned_abs();
    let mut base36 = Vec::new();
    loop {
        base36.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(resto % 36) as usize]);
        resto /= 36;
        if resto == 0 {
            break;
        }
    }
    base36.reverse();
    format!("{}-{}", &nome[..200], String::from_utf8_lossy(&base36))
}

/// Uma linha curta que descreve a chamada de ferramenta, no estilo do painel do Claude Code.
pub fn resumo_ferramenta(nome: &str, entrada: &Value) -> String {
    let campo = |chave: &str| entrada[chave].as_str().map(str::to_string);
    let curto = |s: String| {
        let linha = s.lines().next().unwrap_or_default().to_string();
        if linha.chars().count() > 90 {
            format!("{}…", linha.chars().take(90).collect::<String>())
        } else {
            linha
        }
    };
    let detalhe = match nome {
        "Bash" => campo("description").or_else(|| campo("command")),
        "Read" | "Edit" | "Write" | "NotebookEdit" => campo("file_path").map(|p| {
            PathBuf::from(&p)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or(p)
        }),
        "Grep" | "Glob" => campo("pattern"),
        "WebFetch" => campo("url"),
        "WebSearch" => campo("query"),
        "Task" | "Agent" => campo("description"),
        "Workflow" => campo("name").or_else(|| campo("description")),
        "TodoWrite" => Some("lista de afazeres".into()),
        _ if nome.starts_with("mcp__vitral__") => resumo_do_vitral(nome.trim_start_matches("mcp__vitral__"), entrada),
        _ => None,
    };
    let nome = nome.strip_prefix("mcp__vitral__").unwrap_or(nome);
    match detalhe {
        Some(d) => format!("{nome} · {}", curto(d)),
        None => nome.to_string(),
    }
}

/// O detalhe de uma ferramenta do Vitral: as partes e os projetos da divisão ("3 partes: Ninee,
/// Vitral"), o projeto sugerido, o chat de destino, o texto lembrado, o título do chat criado.
fn resumo_do_vitral(nome: &str, entrada: &Value) -> Option<String> {
    let campo = |chave: &str| entrada[chave].as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
    let chat = |chave: &str| match &entrada[chave] {
        Value::Number(n) => Some(format!("chat {n}")),
        Value::String(t) if t.trim().eq_ignore_ascii_case("mestre") => Some("Mestre".into()),
        Value::String(t) if !t.trim().is_empty() => {
            let t = t.trim().trim_start_matches(['#', '@']);
            Some(if t.chars().all(|c| c.is_ascii_digit()) { format!("chat {t}") } else { t.to_string() })
        }
        _ => None,
    };
    match nome {
        "propor_divisao" => {
            let partes = entrada["partes"].as_array()?;
            let mut projetos: Vec<String> = Vec::new();
            for p in partes {
                let projeto = match p["projeto"].as_str().map(str::trim) {
                    Some("") | None => "Avulsos".to_string(),
                    Some(nome) => nome.trim_start_matches('#').to_string(),
                };
                if !projetos.contains(&projeto) {
                    projetos.push(projeto);
                }
            }
            let quantas = if partes.len() == 1 { "1 parte".to_string() } else { format!("{} partes", partes.len()) };
            Some(if projetos.is_empty() { quantas } else { format!("{quantas}: {}", projetos.join(", ")) })
        }
        "sugerir_projeto" => campo("nome"),
        "mensagem_para_chat" | "ver_chat" => chat("chat"),
        "aprovar_tarefa" | "pedir_ajuste" | "cancelar_tarefa" | "ver_diff" => {
            let alvo = chat("tarefa")?;
            Some(match campo("arquivo").filter(|_| nome == "ver_diff") {
                Some(arquivo) => format!("{alvo} · {arquivo}"),
                None => alvo,
            })
        }
        "historico" => {
            let horas = entrada["horas"].as_u64().unwrap_or(24);
            Some(match campo("projeto") {
                Some(p) => format!("{horas} h · {p}"),
                None => format!("{horas} h"),
            })
        }
        "listar_chats" | "anotar_decisao" if campo("texto").is_none() => campo("projeto"),
        "entregar_tarefa" => campo("resumo"),
        _ => campo("titulo")
            .or_else(|| campo("pergunta"))
            .or_else(|| campo("texto"))
            .or_else(|| entrada["tarefa"].as_u64().map(|n| format!("chat {n}"))),
    }
}

pub fn tipo_imagem(caminho: &std::path::Path) -> Option<&'static str> {
    let extensao = caminho.extension()?.to_str()?.to_lowercase();
    Some(match extensao.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

fn base64(bytes: &[u8]) -> String {
    const ALFABETO: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut saida = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for bloco in bytes.chunks(3) {
        let b = [bloco[0], *bloco.get(1).unwrap_or(&0), *bloco.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= bloco.len() {
                saida.push(ALFABETO[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                saida.push('=');
            }
        }
    }
    saida
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn pasta_do_projeto_igual_a_do_claude_code() {
        assert_eq!(nome_do_projeto("/home/victor/Projetos/x.y_z"), "-home-victor-Projetos-x-y-z");
        // Conferido com a função do Claude Code 2.1.278 rodando no node.
        let longo = format!("/home/victor/{}ação", "pasta-com-nome-bem-comprido/".repeat(8));
        let nome = nome_do_projeto(&longo);
        assert_eq!(nome.len(), 207);
        assert!(nome.ends_with("-bem--lwloyn"), "{nome}");
    }

    fn opcoes() -> Opcoes {
        Opcoes {
            cwd: PathBuf::from("/tmp/x"),
            sessao: "s1".into(),
            retomar: true,
            bifurcar: None,
            modelo: Some("opus".into()),
            permissao: "acceptEdits".into(),
            prompt_sistema: "p".into(),
            prompt_arquivo: None,
            ferramentas: None,
            url_mcp: "http://127.0.0.1:1/mcp".into(),
            bloqueadas: vec!["Edit(//a/**)".into()],
            pastas_extras: Vec::new(),
            so_mcp_vitral: false,
            esforco: Some("max".into()),
            ultracode: false,
            pasta_conta: None,
            ambiente: Vec::new(),
            configuracoes: None,
        }
    }

    fn valor_de<'a>(args: &'a [String], chave: &str) -> Option<&'a str> {
        let i = args.iter().position(|a| a == chave)?;
        args.get(i + 1).map(String::as_str)
    }

    #[test]
    fn ultracode_vai_como_esforco_e_nas_configuracoes() {
        let normal = opcoes();
        let args = argumentos(&normal);
        assert_eq!(valor_de(&args, "--effort"), Some("max"));
        assert!(!args.iter().any(|a| a == "--settings"));
        // Fora do Ultracode: sem fluxos de agentes, com o limite deles e com o teto padrão da espera.
        assert_eq!(args.last().map(String::as_str), Some("Workflow"));
        let env = ambiente(&normal);
        assert!(env.contains(&("CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS".into(), "3".into())));
        assert!(!env.iter().any(|(k, _)| k == "CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS"));

        let com_hook = Opcoes {
            ultracode: true,
            configuracoes: Some(json!({ "hooks": { "Stop": [] } })),
            ..opcoes()
        };
        let args = argumentos(&com_hook);
        assert!(!args.iter().any(|a| a == "Workflow"));
        // O Máximo da conversa não vale: o Ultracode roda em Extra.
        assert_eq!(valor_de(&args, "--effort"), Some("ultracode"));
        assert_eq!(args.iter().filter(|a| *a == "--effort" || *a == "--settings").count(), 2);
        let configuracoes: Value = serde_json::from_str(valor_de(&args, "--settings").unwrap()).unwrap();
        assert!(configuracoes["hooks"]["Stop"].is_array(), "o hook continua no mesmo --settings");
        assert_eq!(configuracoes["enableWorkflows"], true);
        assert_eq!(configuracoes["ultracode"], true);
        let env = ambiente(&com_hook);
        let de = |chave: &str| env.iter().find(|(k, _)| k == chave).map(|(_, v)| v.as_str());
        assert_eq!(de("CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS"), Some("3"));
        assert_eq!(de("CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS"), Some("0"));

        // Sem hook, o --settings leva só o Ultracode.
        let sozinho = Opcoes { ultracode: true, esforco: None, ..opcoes() };
        let args = argumentos(&Opcoes { bloqueadas: Vec::new(), ..sozinho.clone() });
        assert!(!args.iter().any(|a| a == "--disallowedTools"), "a lista não vai vazia");
        let args = argumentos(&sozinho);
        assert_eq!(valor_de(&args, "--effort"), Some("ultracode"));
        let configuracoes: Value = serde_json::from_str(valor_de(&args, "--settings").unwrap()).unwrap();
        assert_eq!(configuracoes, json!({ "enableWorkflows": true, "ultracode": true }));
    }

    #[test]
    fn argumentos_do_turno_comum() {
        let args = argumentos(&Opcoes { esforco: Some(String::new()), ..opcoes() });
        assert_eq!(&args[..3], ["-p", "--input-format", "stream-json"]);
        assert_eq!(valor_de(&args, "--resume"), Some("s1"));
        assert_eq!(valor_de(&args, "--model"), Some("opus"));
        assert_eq!(valor_de(&args, "--permission-prompt-tool"), Some("mcp__vitral__permissao"));
        assert_eq!(valor_de(&args, "--disallowedTools"), Some("Edit(//a/**)"));
        assert!(!args.iter().any(|a| a == "--effort" || a == "--strict-mcp-config"));
        // Sem outras bloqueadas, o Workflow vai sozinho.
        let args = argumentos(&Opcoes { bloqueadas: Vec::new(), ..opcoes() });
        assert_eq!(valor_de(&args, "--disallowedTools"), Some("Workflow"));
        // O coordenador já o bloqueia: vai uma vez só, e continua bloqueado até com o Ultracode.
        let coordenador = Opcoes { bloqueadas: vec!["Task".into(), "Workflow".into()], ..opcoes() };
        let args = argumentos(&coordenador);
        assert_eq!(args.iter().filter(|a| *a == "Workflow").count(), 1);
        let args = argumentos(&Opcoes { ultracode: true, ..coordenador });
        assert_eq!(args.last().map(String::as_str), Some("Workflow"));
        let env = ambiente(&Opcoes { so_mcp_vitral: true, ..opcoes() });
        assert!(env.contains(&("ENABLE_TOOL_SEARCH".into(), "false".into())));
    }

    #[test]
    fn conversa_do_claude_code_continua_numa_copia() {
        let bifurcar = Some("orig".to_string());
        let args = argumentos(&Opcoes { retomar: false, bifurcar: bifurcar.clone(), ..opcoes() });
        let i = args.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(&args[i..i + 5], ["--resume", "orig", "--fork-session", "--session-id", "s1"]);
        // Depois da primeira vez, a sessão nova.
        let args = argumentos(&Opcoes { bifurcar, ..opcoes() });
        assert_eq!(valor_de(&args, "--resume"), Some("s1"));
        assert!(!args.iter().any(|a| a == "--fork-session" || a == "--session-id"));
        let args = argumentos(&Opcoes { retomar: false, ..opcoes() });
        assert_eq!(valor_de(&args, "--session-id"), Some("s1"));
        assert!(!args.iter().any(|a| a == "--resume" || a == "--fork-session"));
    }

    #[test]
    fn prompt_por_arquivo_sem_a_foto_e_lista_de_ferramentas() {
        let args = argumentos(&opcoes());
        assert_eq!(valor_de(&args, "--append-system-prompt"), Some("p"));
        assert!(!args.iter().any(|a| a == "--system-prompt-snapshot" || a == "--tools"));
        let por_arquivo = Opcoes {
            prompt_arquivo: Some(PathBuf::from("/c/prompts/c1.md")),
            ferramentas: Some("Read,Grep,Glob".into()),
            ..opcoes()
        };
        let args = argumentos(&por_arquivo);
        assert_eq!(valor_de(&args, "--append-system-prompt-file"), Some("/c/prompts/c1.md"));
        assert_eq!(valor_de(&args, "--system-prompt-snapshot"), Some("off"));
        assert_eq!(valor_de(&args, "--tools"), Some("Read,Grep,Glob"));
        // Os dois jeitos juntos o Claude Code recusa.
        assert!(!args.iter().any(|a| a == "--append-system-prompt"));
    }

    #[test]
    fn sessao_que_sumiu_da_pasta_do_projeto_expirou() {
        let base = std::env::temp_dir().join(format!("vitral-sessao-{}", crate::modelo::novo_id()));
        let cwd = base.join("loja");
        std::fs::create_dir_all(&cwd).unwrap();
        let conta = base.join("conta");
        // Sem a pasta do projeto: não dá para saber.
        assert!(!sessao_expirou(Some(&conta), &cwd, "s1"));
        let projeto = conta.join("projects").join(nome_do_projeto(&dunce::canonicalize(&cwd).unwrap().to_string_lossy()));
        std::fs::create_dir_all(&projeto).unwrap();
        assert!(sessao_expirou(Some(&conta), &cwd, "s1"));
        std::fs::write(projeto.join("s1.jsonl"), "{}").unwrap();
        assert!(!sessao_expirou(Some(&conta), &cwd, "s1") && sessao_existe(Some(&conta), &cwd, "s1"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn versao_do_claude_code() {
        assert_eq!(versao_de("2.1.278 (Claude Code)"), Some((2, 1, 278)));
        assert_eq!(versao_de("2.1.314"), Some((2, 1, 314)));
        assert_eq!(versao_de("cli.js"), None);
        assert_eq!(versao_de("2.1"), None);
        assert!(versao_de("2.1.268").unwrap() < VERSAO_ULTRACODE, "sem o limite de agentes");
        assert!(versao_de("2.1.278").unwrap() >= VERSAO_ULTRACODE);
        assert!(versao_de("3.0.0").unwrap() >= VERSAO_ULTRACODE);
    }

    #[test]
    fn init_diz_a_versao_e_se_ha_fluxos() {
        let init = |ferramentas: Value| {
            json!({ "type": "system", "subtype": "init", "model": "claude-opus-5",
                    "claude_code_version": "2.1.278", "tools": ferramentas })
        };
        let eventos = interpretar(&init(json!(["Bash", "Workflow"])));
        let [Evento::Inicio { versao, fluxos, .. }] = eventos.as_slice() else {
            panic!("sem início");
        };
        assert_eq!((*versao, *fluxos), (Some((2, 1, 278)), Some(true)));
        let eventos = interpretar(&init(json!(["Bash"])));
        assert!(matches!(eventos.as_slice(), [Evento::Inicio { fluxos: Some(false), .. }]));
        let eventos = interpretar(&init(json!([])));
        assert!(matches!(eventos.as_slice(), [Evento::Inicio { fluxos: None, .. }]));
    }

    #[test]
    fn agentes_em_segundo_plano_contam_so_fluxos_e_subagentes() {
        let evento = json!({
            "type": "system", "subtype": "background_tasks_changed",
            "tasks": [
                { "task_id": "a", "task_type": "local_workflow", "description": "fluxo" },
                { "task_id": "b", "task_type": "local_agent", "description": "subagente" },
                { "task_id": "c", "task_type": "local_bash", "description": "servidor" },
                { "task_id": "d", "task_type": "local_agent", "description": "vigia", "ambient": true }
            ]
        });
        assert!(matches!(interpretar(&evento).as_slice(), [Evento::Fundo(2)]));
        let vazio = json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [] });
        assert!(matches!(interpretar(&vazio).as_slice(), [Evento::Fundo(0)]));
    }

    #[test]
    fn resumo_das_ferramentas_do_mestre() {
        let r = |nome: &str, v: Value| resumo_ferramenta(&format!("mcp__vitral__{nome}"), &v);
        let partes = json!({ "resumo": "x", "partes": [
            { "projeto": "Ninee", "titulo": "Checkout" },
            { "projeto": "vitral", "titulo": "Docs" },
            { "projeto": "Ninee", "titulo": "Pix" }
        ]});
        assert_eq!(r("propor_divisao", partes), "propor_divisao · 3 partes: Ninee, vitral");
        assert_eq!(r("propor_divisao", json!({ "partes": [{ "projeto": "" }] })), "propor_divisao · 1 parte: Avulsos");
        assert_eq!(r("sugerir_projeto", json!({ "nome": "Receitas", "motivo": "m" })), "sugerir_projeto · Receitas");
        assert_eq!(r("mensagem_para_chat", json!({ "chat": "12", "texto": "oi" })), "mensagem_para_chat · chat 12");
        assert_eq!(r("mensagem_para_chat", json!({ "chat": "mestre", "texto": "oi" })), "mensagem_para_chat · Mestre");
        assert_eq!(r("ver_chat", json!({ "chat": 7 })), "ver_chat · chat 7");
        assert_eq!(r("pedir_ajuste", json!({ "tarefa": 12, "feedback": "f" })), "pedir_ajuste · chat 12");
        assert_eq!(r("ver_diff", json!({ "tarefa": 12, "arquivo": "a.rs" })), "ver_diff · chat 12 · a.rs");
        assert_eq!(r("lembrar", json!({ "texto": "pagamentos vão para a Ninee" })), "lembrar · pagamentos vão para a Ninee");
        assert_eq!(r("historico", json!({})), "historico · 24 h");
        assert_eq!(r("entregar_tarefa", json!({ "resumo": "feito" })), "entregar_tarefa · feito");
        assert_eq!(r("criar_tarefa", json!({ "titulo": "Cupons" })), "criar_tarefa · Cupons");
        assert_eq!(r("estado", json!({})), "estado");
    }

    #[test]
    fn limite_recusado_traz_a_hora_em_que_volta() {
        let evento = json!({
            "type": "rate_limit_event",
            "rate_limit_info": {
                "status": "rejected",
                "resetsAt": 1_790_000_000u64,
                "unifiedWindows": { "five_hour": { "utilization": 1.0, "resetsAt": 1_790_000_000u64 } }
            }
        });
        let eventos = interpretar(&evento);
        let [Evento::Limite { bloqueado, bloqueado_ate, uso_5h, .. }] = eventos.as_slice() else {
            panic!("sem evento de limite");
        };
        assert!(*bloqueado);
        assert_eq!(*bloqueado_ate, Some(1_790_000_000));
        assert_eq!(*uso_5h, Some(1.0));
    }
}
