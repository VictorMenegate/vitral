//! Um processo do Claude Code em modo `--print` com entrada e saída `stream-json`.
//!
//! Cada vez (turno) de um chat roda num processo: o Vitral escreve a mensagem na entrada,
//! lê os eventos da saída numa thread e fecha a entrada quando o turno acaba, o que encerra o
//! processo. A próxima mensagem abre outro com `--resume`, então chats parados não gastam memória.

use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use serde_json::{Value, json};

use crate::processos::{self, Escopo};

#[derive(Clone, Debug)]
pub struct Opcoes {
    /// Quem roda o turno: o `claude` ou o `codex` (`agente_codex`).
    pub agente: crate::modelo::Agente,
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
    /// Só o servidor do Vitral, sem os MCP do usuário (a opção das Configurações desligada).
    pub so_mcp_vitral: bool,
    /// `--effort` (low, medium, high, xhigh, max); vazio usa o padrão.
    pub esforco: Option<String>,
    /// Ultracode neste turno: `--effort ultracode` (Extra mais fluxos de agentes), com os fluxos
    /// ligados nas configurações e o limite de agentes ao mesmo tempo.
    pub ultracode: bool,
    /// `CLAUDE_CONFIG_DIR` de uma conta extra; `None` é o login normal.
    pub pasta_conta: Option<PathBuf>,
    /// O Vitral pede a revisão da memória ao fim da sessão (`Config.memoria_ao_terminar`): o
    /// processo leva `VITRAL_MEMORIA_NATIVA=1`, para um hook `Stop` que faça o mesmo pedido saber
    /// que não precisa (senão o agente recebe os dois).
    pub memoria_nativa: bool,
    /// O arquivo com o servidor MCP da memória da equipe (`memoria_equipe::arquivo_do_mcp`): vai
    /// como mais um `--mcp-config`.
    pub mcp_da_equipe: Option<PathBuf>,
}

/// Versão do Claude Code (2.1.278 vira `(2, 1, 278)`).
pub type Versao = (u32, u32, u32);

/// A versão em que o Ultracode foi conferido: `--effort ultracode`, o teto da espera
/// (`CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS`) e o limite de agentes, que só existe desde a 2.1.269
/// (changelog embutido: "Added CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS"). Antes dela, o
/// limite de 3 seria ignorado calado; num valor de esforço que não conhece, o Claude Code avisa
/// e roda no esforço padrão.
pub const VERSAO_ULTRACODE: Versao = (2, 1, 278);

/// A versão a partir da qual as versões exatas de modelo (`ui::composer::VERSOES_DE_MODELO`)
/// podem ser pedidas. Não é o Vitral que recusa: quem recusa é o servidor, e o Claude Code velho
/// deixa o turno falhar. Medido no 2.1.278 com `--model claude-opus-5-5`: o `init` sai com o
/// modelo pedido e o turno morre com "API Error: 400 Claude Code 2.1.278 does not support this
/// model; version 2.1.280 or newer is required. Run 'claude update'", código 1. É só o modelo
/// mais novo que cobra isso (o `claude-sonnet-5-5` e o `claude-haiku-5-5` rodam no 2.1.278), mas
/// o 5.5 é justamente o que se quer escolher, então o seletor pede a 2.1.280 para todas.
///
/// Um nome que o Claude Code nem conhece (`--model bananinha-9`) termina igual: `init` com o nome
/// cru e o `result` "There's an issue with the selected model (…). It may not exist or you may not
/// have access to it", com `[claude-code:unrecognized_model]` no stderr.
pub const VERSAO_MODELOS_EXATOS: Versao = (2, 1, 280);

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
    /// A conversa do lado do agente ganhou este id (o `thread_id` do Codex): é ele que retoma.
    Sessao(String),
    /// Tokens que a conversa ocupa no contexto agora.
    Contexto(u64),
    TextoParcial(String),
    Texto(String),
    /// O modelo começou um bloco de raciocínio (o motor mede quanto ele dura).
    Pensando,
    /// O raciocínio do bloco que terminou. O texto vem vazio quando o Claude Code não o entrega.
    Pensamento(String),
    /// Um subagente (`pai` é o id da chamada Task/Agent que o abriu) usou uma ferramenta.
    SubFerramenta {
        pai: String,
        nome: String,
        entrada: Value,
    },
    /// Um subagente disse o que está fazendo.
    SubTexto {
        pai: String,
        texto: String,
    },
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
        let codex = opcoes.agente == crate::modelo::Agente::Codex;
        let mut comando = if codex {
            let mut comando = processos::comando(crate::agente_codex::binario(), "codex");
            comando.current_dir(&opcoes.cwd).args(crate::agente_codex::argumentos(opcoes, mensagem.imagens));
            if let Some((_, chave)) = opcoes.mcp_da_equipe.as_deref().and_then(crate::memoria_equipe::conexao_do_arquivo) {
                comando.env("VITRAL_MEMORIA_EQUIPE_TOKEN", chave);
            }
            comando
        } else {
            let mut comando = processos::comando(binario_claude(), "claude");
            comando.current_dir(&opcoes.cwd).args(argumentos(opcoes));
            if let Some(pasta) = &opcoes.pasta_conta {
                comando.env("CLAUDE_CONFIG_DIR", pasta);
            }
            for (chave, valor) in ambiente(opcoes) {
                comando.env(chave, valor);
            }
            comando
        };
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

        let (mut filho, escopo) =
            processos::abrir(comando).with_context(|| if codex { "não abriu o codex" } else { "não abriu o claude" })?;
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
            // O Codex fala outro formato, traduzido para os mesmos eventos.
            let mut do_codex = codex.then(crate::agente_codex::Leitor::default);
            for linha in BufReader::new(saida).lines() {
                let Ok(linha) = linha else { break };
                let Ok(valor) = serde_json::from_str::<Value>(&linha) else {
                    continue;
                };
                let eventos = match &mut do_codex {
                    Some(leitor) => leitor.linha(&valor),
                    None => interpretar(&valor),
                };
                for evento in eventos {
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

        if codex {
            // O pedido inteiro vai pela entrada, que fecha em seguida: o `codex exec` é um turno
            // por processo, e parar é pelo sinal (ver `interromper`).
            if let Some(mut entrada) = entrada.take() {
                entrada.write_all(mensagem.texto.as_bytes()).and_then(|()| entrada.flush())?;
            }
        } else if let Some(entrada) = entrada.as_mut() {
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

    /// A entrada ainda está aberta: dá para dirigir o turno (`dirigir`).
    pub fn entrada_aberta(&self) -> bool {
        self.entrada.is_some()
    }

    /// Dirige o turno que está rodando: escreve outra mensagem do usuário na entrada, que fica
    /// aberta durante o turno. `false` quando não há mais entrada (o turno já acabou, ou o Parar a
    /// levou) e a mensagem não foi escrita.
    ///
    /// O que o Claude Code 2.1.278 faz com ela, medido aqui (ver `ESPERA_DO_DIRIGIDO`):
    /// - ela entra numa fila interna dele (o `queued_command` que ele grava na sessão, então o
    ///   `--resume` do turno seguinte não a perde);
    /// - na primeira volta de uma ferramenta, ele a absorve no turno que já roda (a sessão anota
    ///   `queue-operation remove absorbed_mid_turn`) e o modelo muda de rumo na hora, com um
    ///   `result` só no fim — é o steering;
    /// - sem nenhuma volta de ferramenta até o fim (um turno que é só texto), ele fecha aquele
    ///   turno e abre outro na hora, no mesmo processo (uns 30 ms depois do `result`), mesmo com a
    ///   entrada já fechada;
    /// - o `interrupt` não a descarta: o turno fecha com erro e ela roda em seguida, como turno
    ///   novo (por isso o Parar mata o processo quando há mensagem dirigida esperando).
    ///
    /// Nada dela aparece na saída: quem põe a mensagem na conversa é o Vitral.
    pub fn dirigir(&mut self, mensagem: Mensagem) -> bool {
        let Some(entrada) = self.entrada.as_mut() else {
            return false;
        };
        escrever_mensagem(entrada, mensagem).is_ok()
    }

    /// Para o turno: pede pela entrada o `interrupt` do protocolo do Agent SDK (o mesmo do botão de
    /// parar da extensão; conferido no 2.1.278) e fecha ela. O Claude Code responde, fecha o turno
    /// com um `result` de erro, grava a conversa e sai, com tempo de limpar o que abriu. É o mesmo
    /// nos dois sistemas: no Windows, sem console, não há Ctrl+C para mandar. Sem a entrada (ou se a
    /// escrita falhar), o SIGINT no Linux e, no Windows, para tudo. Quem chama confere depois se ele
    /// saiu (`Motor::conferir_parada`).
    pub fn interromper(&mut self) {
        if let Some(mut entrada) = self.entrada.take() {
            let pedido = json!({
                "type": "control_request",
                "request_id": format!("vitral-parar-{}", uuid::Uuid::new_v4()),
                "request": { "subtype": "interrupt" },
            });
            if writeln!(entrada, "{pedido}").and_then(|()| entrada.flush()).is_ok() {
                return;
            }
        }
        // SAFETY: o processo ainda não foi colhido (é do `Child`), então o número é dele.
        #[cfg(unix)]
        unsafe {
            libc::kill(self.filho.id() as i32, libc::SIGINT);
        }
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
    ]
    .map(String::from)
    .to_vec();
    // A memória da equipe entra como outro valor do mesmo `--mcp-config` (ele aceita vários).
    if let Some(arquivo) = &opcoes.mcp_da_equipe {
        args.push(arquivo.display().to_string());
    }
    args.extend(["--allowedTools".to_string(), "mcp__vitral".to_string()]);
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
    args
}

/// O JSON de `--settings`: só no Ultracode, com os fluxos de agentes ligados (no plano Pro eles
/// começam desligados) e o próprio Ultracode.
fn configuracoes_do_turno(opcoes: &Opcoes) -> Option<String> {
    opcoes.ultracode.then(|| json!({ "enableWorkflows": true, "ultracode": true }).to_string())
}

/// Variáveis de ambiente do turno, além da pasta da conta.
fn ambiente(opcoes: &Opcoes) -> Vec<(String, String)> {
    let mut ambiente = Vec::new();
    let mut por = |chave: &str, valor: String| ambiente.push((chave.to_string(), valor));
    if opcoes.so_mcp_vitral {
        por("ENABLE_TOOL_SEARCH", "false".into());
    }
    // Pedidos de permissão podem esperar o usuário por muito tempo.
    por("MCP_TOOL_TIMEOUT", "86400000".into());
    // Em todo turno: um fluxo que escape da opção (num Claude Code que o deixe passar) também
    // fica no limite.
    por("CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS", AGENTES_DO_ULTRACODE.to_string());
    // Com a entrada fechada, o Claude Code espera os agentes que ficaram em segundo plano e, no
    // padrão, corta em 10 min: "Background tasks still running after 600s; terminating". Quem
    // deixa agente em segundo plano não é só o Ultracode — a ferramenta de agente de um turno
    // comum também —, e um lote de uma hora morria no meio, com o turno marcado como falhado.
    // Esperar sem teto é o certo aqui: o Vitral mostra "Os agentes seguem trabalhando em segundo
    // plano", o chat continua contando como rodando e o Parar derruba tudo quando o dono quiser.
    por("CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS", "0".into());
    // Para os hooks do usuário saberem que a sessão é de um chat do Vitral.
    por("VITRAL_CHAT", "1".into());
    if opcoes.memoria_nativa {
        por("VITRAL_MEMORIA_NATIVA", "1".into());
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

/// Um modelo que o Claude Code instalado oferece no seletor dele.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeloOferecido {
    /// O que vai no `--model`: o apelido de uma família (`opus`) ou o id completo.
    pub valor: String,
    /// O id que o valor resolve hoje (`opus` é o `claude-opus-5-5`).
    pub resolvido: String,
    /// O nome que o Claude Code mostra ("Fable 5.2").
    pub nome: String,
}

/// Os modelos que o Claude Code instalado ofereceu da última vez que respondeu (vazio até lá, e
/// para sempre se ele não respondeu: aí vale a lista fixa do seletor).
static MODELOS_OFERECIDOS: Mutex<Vec<ModeloOferecido>> = Mutex::new(Vec::new());

/// Os modelos oferecidos pelo Claude Code instalado, como ele respondeu da última vez.
pub fn modelos_oferecidos() -> Vec<ModeloOferecido> {
    MODELOS_OFERECIDOS.lock().map(|m| m.clone()).unwrap_or_default()
}

/// A família e os números da versão de um id completo: `claude-fable-5-2` é ("fable", [5, 2]).
/// A data no fim (`-20251001`) e o sufixo de janela (`[1m]`) não contam.
fn familia_e_versao(id: &str) -> Option<(&str, Vec<u32>)> {
    let base = id.split('[').next().unwrap_or(id);
    let mut partes = base.strip_prefix("claude-")?.split('-');
    let familia = partes.next().filter(|f| !f.is_empty() && !f.chars().all(|c| c.is_ascii_digit()))?;
    let versao = partes.filter(|p| p.len() < 6).filter_map(|p| p.parse().ok()).collect();
    Some((familia, versao))
}

/// O id da versão mais nova de uma família entre os modelos oferecidos; `apelido` é o da família
/// (`fable`). Olha o que o apelido resolve e cada modelo da mesma família que o Claude Code
/// lista, e fica com o de número maior (empatados, o que o apelido resolve). `None` se ele não
/// ofereceu nada dessa família.
pub fn mais_nova_da_familia(oferecidos: &[ModeloOferecido], apelido: &str) -> Option<String> {
    let do_apelido = oferecidos.iter().find(|m| m.valor == apelido).map(|m| m.resolvido.as_str());
    // A família é a do id que o apelido resolve (o `default` resolve num Opus); sem ele, o nome.
    let familia = do_apelido.and_then(familia_e_versao).map_or(apelido, |(f, _)| f);
    let mut melhor: Option<(&str, Vec<u32>)> = None;
    let candidatos = do_apelido.into_iter().chain(oferecidos.iter().flat_map(|m| [m.valor.as_str(), m.resolvido.as_str()]));
    for id in candidatos {
        let Some((f, versao)) = familia_e_versao(id) else {
            continue;
        };
        if f == familia && melhor.as_ref().is_none_or(|(_, v)| versao > *v) {
            melhor = Some((id, versao));
        }
    }
    melhor.map(|(id, _)| id.to_string())
}

/// O que vai no `--model` para a escolha do seletor. O apelido de uma família quer dizer "a
/// versão mais nova dela", e o Claude Code às vezes lista a versão nova à parte antes de mover o
/// apelido: o 2.1.314 oferece o Fable 5.2 e ainda resolve `fable` no 5.1. Quando ele oferece uma
/// versão mais nova do que a que o apelido resolve, vai o id dela; senão (e para uma versão
/// exata, ou sem resposta dele), a escolha como está. É o que faz a família acompanhar as
/// atualizações sozinha (dono, 13/11/2026: "tem que ser adaptável tudo para as atualizações").
pub fn modelo_para_o_turno(oferecidos: &[ModeloOferecido], escolhido: &str) -> String {
    let resolvido = oferecidos.iter().find(|m| m.valor == escolhido).map(|m| m.resolvido.as_str());
    let (Some(resolvido), Some(nova)) = (resolvido, mais_nova_da_familia(oferecidos, escolhido)) else {
        return escolhido.to_string();
    };
    // Só troca quando a escolha é mesmo um apelido (resolve em outro id) e há versão maior.
    let maior = familia_e_versao(&nova).map(|(_, v)| v) > familia_e_versao(resolvido).map(|(_, v)| v);
    if escolhido != resolvido && maior { nova } else { escolhido.to_string() }
}

/// Quanto esperar pela resposta do `initialize` (ela costuma vir em menos de 1 s).
const PRAZO_DOS_MODELOS: Duration = Duration::from_secs(12);

/// Pergunta ao Claude Code instalado quais modelos ele oferece e guarda a resposta: é o pedido
/// `initialize` do protocolo do Agent SDK (o que a extensão do VS Code faz ao abrir), sem mensagem
/// nenhuma, então não gasta uso. É assim que o seletor de modelo acompanha o Claude Code sozinho:
/// modelo novo aparece quando o Claude Code se atualiza, sem versão nova do Vitral. Roda sem os
/// hooks do usuário (`disableAllHooks`), sem servidor MCP nenhum (`--strict-mcp-config`), só com
/// as configurações do usuário, e não cria conversa. `pasta` é onde o processo roda: uma pasta
/// vazia e só do Vitral (a temporária do sistema é de todo mundo, e um `.claude` ou `.mcp.json`
/// plantado lá seria lido). Bloqueia até a resposta ou o prazo: chamar em segundo plano. Devolve
/// `true` se a lista mudou.
pub fn atualizar_modelos_oferecidos(pasta: &Path) -> bool {
    // Com o claude de mentira da bancada não há a quem perguntar.
    if std::env::var_os("CLAUDE_FALSO").is_some() {
        return false;
    }
    let Some(novos) = perguntar_modelos(pasta) else {
        return false;
    };
    let Ok(mut guardados) = MODELOS_OFERECIDOS.lock() else {
        return false;
    };
    if *guardados == novos {
        return false;
    }
    *guardados = novos;
    true
}

/// Cria a pasta (e as de cima) só para o dono.
fn pasta_so_do_dono(pasta: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(pasta)?;
        // Já existindo, fica só para o dono também.
        std::fs::set_permissions(pasta, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(pasta)
}

fn perguntar_modelos(pasta: &Path) -> Option<Vec<ModeloOferecido>> {
    pasta_so_do_dono(pasta).ok()?;
    let resposta = perguntar_ao_claude(pasta, true)?;
    modelos_de(&resposta).filter(|m| !m.is_empty())
}

/// Faz o pedido `initialize` ao Claude Code em `pasta` e devolve o que ele respondeu (o
/// `response.response`): os modelos, os comandos e skills daquela pasta, a conta. Sem mensagem
/// nenhuma, sem hooks e sem servidor MCP; `so_do_usuario` deixa de fora as configurações da
/// pasta (para a pergunta que não é de pasta nenhuma). Bloqueia até a resposta ou o prazo.
fn perguntar_ao_claude(pasta: &Path, so_do_usuario: bool) -> Option<Value> {
    pedir_ao_claude(pasta, so_do_usuario, None)
}

/// Um segundo pedido de controle, depois do `initialize`: o subtipo dele, o modelo com que o
/// processo abre (vazio é o padrão) e a conta (`CLAUDE_CONFIG_DIR`).
struct SegundoPedido<'a> {
    subtipo: &'a str,
    modelo: &'a str,
    pasta_conta: Option<&'a Path>,
}

/// O `initialize` e, com `segundo`, mais um pedido de controle; devolve a resposta do último.
fn pedir_ao_claude(pasta: &Path, so_do_usuario: bool, segundo: Option<SegundoPedido>) -> Option<Value> {
    // Com grupo próprio, para o fim derrubar também o que o claude tiver aberto.
    let mut comando = processos::comando_leve(binario_claude(), "modelos");
    comando
        .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"])
        .args(["--settings", r#"{"disableAllHooks":true}"#])
        .arg("--strict-mcp-config")
        .current_dir(pasta)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if so_do_usuario {
        comando.args(["--setting-sources", "user"]);
    }
    if let Some(segundo) = &segundo {
        if !segundo.modelo.is_empty() {
            comando.args(["--model", segundo.modelo]);
        }
        if let Some(conta) = segundo.pasta_conta {
            comando.env("CLAUDE_CONFIG_DIR", conta);
        }
    }
    // Aberto de dentro de outra sessão do Claude Code, o filho herdaria a marca dela.
    for variavel in ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SSE_PORT"] {
        comando.env_remove(variavel);
    }
    let (mut filho, escopo) = processos::abrir(comando).ok()?;
    let (Some(mut entrada), Some(saida)) = (filho.stdin.take(), filho.stdout.take()) else {
        escopo.matar();
        let _ = filho.wait();
        return None;
    };
    let pedido_de = |subtipo: &str| {
        let id = format!("vitral-{subtipo}-{}", uuid::Uuid::new_v4());
        let pedido = json!({ "type": "control_request", "request_id": id, "request": { "subtype": subtipo } });
        (id, pedido)
    };
    let (mut esperado, pedido) = pedido_de("initialize");
    let mut escrito = writeln!(entrada, "{pedido}").and_then(|()| entrada.flush()).is_ok();
    if let Some(segundo) = &segundo {
        let (id, pedido) = pedido_de(segundo.subtipo);
        esperado = id;
        escrito = escrito && writeln!(entrada, "{pedido}").and_then(|()| entrada.flush()).is_ok();
    }
    // A leitura vai noutra thread para o prazo valer mesmo se o processo não disser nada.
    let (envia, recebe) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // Em bytes, e não com `lines()`: uma linha que não é UTF-8 (um aviso com o nome de uma
        // pasta, por exemplo) encerraria a leitura antes da resposta.
        let mut leitor = BufReader::new(saida);
        let mut linha = Vec::new();
        loop {
            linha.clear();
            match leitor.read_until(b'\n', &mut linha) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let texto = String::from_utf8_lossy(&linha);
            let resposta = serde_json::from_str::<Value>(texto.trim()).ok().and_then(|v| resposta_ao_pedido(&v, &esperado).cloned());
            if let Some(resposta) = resposta {
                let _ = envia.send(resposta);
                return;
            }
        }
    });
    let resposta = if escrito { recebe.recv_timeout(PRAZO_DOS_MODELOS).ok() } else { None };
    // A entrada fica aberta até aqui: fechada antes, o Claude Code sairia sem responder.
    drop(entrada);
    // O grupo inteiro, antes de colher: só o filho deixaria órfão o que ele abriu.
    escopo.matar();
    let _ = filho.wait();
    resposta
}

/// O miolo da resposta ao pedido de controle `id`, se a linha da saída é ela.
fn resposta_ao_pedido<'a>(valor: &'a Value, id: &str) -> Option<&'a Value> {
    (valor["response"]["request_id"] == id).then(|| resposta_do_initialize(valor)).flatten()
}

/// O esforço que o Claude Code aplica quando nada é pedido, por conta e por modelo (o `--model`
/// do turno; vazio é o padrão dele): `Some(None)` é "já perguntei e não soube" (ou a pergunta
/// ainda está no ar).
type EsforcosPadrao = Vec<((Option<PathBuf>, String), Option<String>)>;
static ESFORCOS_PADRAO: Mutex<EsforcosPadrao> = Mutex::new(Vec::new());

/// O esforço que vale no automático para este modelo nesta conta ("xhigh"), se o Claude Code já
/// disse.
pub fn esforco_padrao(pasta_conta: Option<&Path>, modelo: &str) -> Option<String> {
    let guardados = ESFORCOS_PADRAO.lock().ok()?;
    guardados.iter().find(|((c, m), _)| c.as_deref() == pasta_conta && m == modelo).and_then(|(_, e)| e.clone())
}

/// Ainda não foi perguntado: devolve `true` uma vez só por conta e modelo (e marca a pergunta
/// como feita, para o desenho seguinte não abrir outra).
pub fn falta_perguntar_esforco(pasta_conta: Option<&Path>, modelo: &str) -> bool {
    if std::env::var_os("CLAUDE_FALSO").is_some() {
        return false;
    }
    let Ok(mut guardados) = ESFORCOS_PADRAO.lock() else {
        return false;
    };
    if guardados.iter().any(|((c, m), _)| c.as_deref() == pasta_conta && m == modelo) {
        return false;
    }
    guardados.push(((pasta_conta.map(Path::to_path_buf), modelo.to_string()), None));
    true
}

/// Pergunta ao Claude Code qual esforço ele aplica a este modelo sem ninguém pedir (o
/// `get_settings`, conferido no 2.1.322: `applied.effort` é o esforço da próxima mensagem, depois
/// das configurações, da organização e do que o modelo aceita; o Opus 5.5 respondeu `xhigh`, o
/// Fable 5.2 `high`, o Sonnet e o Haiku 5.5 `medium`). Não manda mensagem nem gasta do plano.
/// Bloqueia até a resposta ou o prazo: chamar em segundo plano. `true` se soube.
pub fn atualizar_esforco_padrao(pasta: &Path, pasta_conta: Option<&Path>, modelo: &str) -> bool {
    if pasta_so_do_dono(pasta).is_err() {
        return false;
    }
    let segundo = SegundoPedido { subtipo: "get_settings", modelo, pasta_conta };
    let Some(esforco) = pedir_ao_claude(pasta, true, Some(segundo)).and_then(|r| r["applied"]["effort"].as_str().map(str::to_string))
    else {
        return false;
    };
    let Ok(mut guardados) = ESFORCOS_PADRAO.lock() else {
        return false;
    };
    match guardados.iter_mut().find(|((c, m), _)| c.as_deref() == pasta_conta && m == modelo) {
        Some((_, guardado)) => *guardado = Some(esforco),
        None => guardados.push(((pasta_conta.map(Path::to_path_buf), modelo.to_string()), Some(esforco))),
    }
    true
}

/// Um servidor MCP como o Claude Code o vê numa sessão (o `/mcp` do terminal).
#[derive(Clone, Debug, PartialEq)]
pub struct ServidorMcp {
    pub nome: String,
    /// `connected`, `failed`, `needs-auth`, `pending` ou `disabled`.
    pub estado: String,
    pub versao: Option<String>,
    /// De onde vem: `user`, `project`, `local`, `claudeai` (os conectores da conta)…
    pub escopo: String,
    pub ferramentas: Vec<String>,
    pub erro: Option<String>,
}

/// Os servidores da resposta do `mcp_status`.
pub fn servidores_mcp_de(resposta: &Value) -> Vec<ServidorMcp> {
    let texto = |v: &Value| v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    resposta["mcpServers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            Some(ServidorMcp {
                nome: texto(&s["name"])?,
                estado: texto(&s["status"]).unwrap_or_default(),
                versao: texto(&s["serverInfo"]["version"]),
                escopo: texto(&s["scope"]).unwrap_or_default(),
                ferramentas: s["tools"].as_array().into_iter().flatten().filter_map(|f| texto(&f["name"])).collect(),
                erro: texto(&s["error"]),
            })
        })
        .collect()
}

/// Quanto se espera os servidores MCP ligarem antes de mostrar o que há.
const PRAZO_DO_MCP: Duration = Duration::from_secs(20);

/// Um Claude Code aberto só para conversar sobre os servidores MCP de uma pasta (o `/mcp` do
/// terminal, que no `-p` só responde "use /mcp in the terminal"): sem mensagem nenhuma e sem hooks,
/// na conta dada, não gasta do plano. Fica vivo enquanto a tela dos servidores está aberta, para
/// o reconectar valer na mesma sessão que mostra o estado; ao sair de cena, leva junto o que
/// abriu. Os pedidos são os do protocolo de controle, conferidos no 2.1.322 (`mcp_status` e
/// `mcp_reconnect`). Todos bloqueiam: chamar em segundo plano.
pub struct SessaoMcp {
    filho: Child,
    entrada: Option<ChildStdin>,
    escopo: Escopo,
    /// As respostas de controle, com o id do pedido: (id, deu certo, miolo).
    respostas: std::sync::mpsc::Receiver<(String, bool, Value)>,
}

impl SessaoMcp {
    /// `mcp_extra` é um arquivo de `--mcp-config` a mais (a memória da equipe do Vitral), para a
    /// lista ser a que os chats têm de verdade.
    pub fn abrir(pasta: &Path, pasta_conta: Option<&Path>, mcp_extra: Option<&Path>) -> Option<Self> {
        let mut comando = processos::comando_leve(binario_claude(), "mcp");
        comando
            .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"])
            .args(["--settings", r#"{"disableAllHooks":true}"#])
            .current_dir(pasta)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(conta) = pasta_conta {
            comando.env("CLAUDE_CONFIG_DIR", conta);
        }
        if let Some(arquivo) = mcp_extra {
            comando.arg("--mcp-config").arg(arquivo);
        }
        for variavel in ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SSE_PORT"] {
            comando.env_remove(variavel);
        }
        let (mut filho, escopo) = processos::abrir(comando).ok()?;
        let (Some(entrada), Some(saida)) = (filho.stdin.take(), filho.stdout.take()) else {
            escopo.matar();
            let _ = filho.wait();
            return None;
        };
        let (envia, respostas) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut leitor = BufReader::new(saida);
            let mut linha = Vec::new();
            loop {
                linha.clear();
                match leitor.read_until(b'\n', &mut linha) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
                let Ok(valor) = serde_json::from_str::<Value>(String::from_utf8_lossy(&linha).trim()) else {
                    continue;
                };
                if valor["type"] != "control_response" {
                    continue;
                }
                let r = &valor["response"];
                let id = r["request_id"].as_str().unwrap_or_default().to_string();
                if envia.send((id, r["subtype"] == "success", r["response"].clone())).is_err() {
                    return;
                }
            }
        });
        let mut sessao = SessaoMcp { filho, entrada: Some(entrada), escopo, respostas };
        sessao.pedir(json!({ "subtype": "initialize" }), PRAZO_DOS_MODELOS)?;
        Some(sessao)
    }

    /// Manda um pedido de controle e espera a resposta dele (as de outros pedidos, atrasadas,
    /// são descartadas). `None` se a escrita falhou, o prazo passou ou o Claude Code recusou.
    fn pedir(&mut self, pedido: Value, prazo: Duration) -> Option<Value> {
        let id = format!("vitral-mcp-{}", uuid::Uuid::new_v4());
        let linha = json!({ "type": "control_request", "request_id": id, "request": pedido });
        let entrada = self.entrada.as_mut()?;
        writeln!(entrada, "{linha}").and_then(|()| entrada.flush()).ok()?;
        let limite = Instant::now() + prazo;
        loop {
            let (de, certo, miolo) = self.respostas.recv_timeout(limite.saturating_duration_since(Instant::now())).ok()?;
            if de == id {
                return certo.then_some(miolo);
            }
        }
    }

    /// Os servidores e como estão agora, esperando os que ainda estão ligando (`pending`) até o
    /// prazo.
    pub fn servidores(&mut self) -> Option<Vec<ServidorMcp>> {
        let limite = Instant::now() + PRAZO_DO_MCP;
        let mut ultimos = None;
        loop {
            let Some(resposta) = self.pedir(json!({ "subtype": "mcp_status" }), Duration::from_secs(8)) else {
                return ultimos;
            };
            let lidos = servidores_mcp_de(&resposta);
            let ligando = lidos.iter().any(|s| s.estado == "pending");
            ultimos = Some(lidos);
            if !ligando || Instant::now() >= limite {
                return ultimos;
            }
            std::thread::sleep(Duration::from_millis(700));
        }
    }

    /// Liga de novo o servidor `nome` nesta sessão. `false` se o Claude Code recusou ou não
    /// respondeu; o estado novo sai do `servidores`.
    pub fn reconectar(&mut self, nome: &str) -> bool {
        self.pedir(json!({ "subtype": "mcp_reconnect", "serverName": nome }), Duration::from_secs(40)).is_some()
    }
}

impl Drop for SessaoMcp {
    fn drop(&mut self) {
        self.entrada = None;
        // O grupo inteiro: os servidores MCP são filhos do Claude Code.
        self.escopo.matar();
        let _ = self.filho.wait();
    }
}

/// O miolo da resposta do `initialize`, se a linha da saída é ela.
fn resposta_do_initialize(valor: &Value) -> Option<&Value> {
    (valor["type"] == "control_response").then(|| &valor["response"]["response"]).filter(|r| r.is_object())
}

/// Um comando de barra ou skill que o Claude Code oferece numa pasta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComandoOferecido {
    pub nome: String,
    pub descricao: String,
}

/// Os comandos e skills que o Claude Code ofereceu em cada pasta já perguntada.
static COMANDOS_POR_PASTA: Mutex<Vec<(PathBuf, Vec<ComandoOferecido>)>> = Mutex::new(Vec::new());
/// As pastas já perguntadas (ou sendo perguntadas) nesta abertura do app.
static PASTAS_PERGUNTADAS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// A pasta ainda não foi perguntada: marca e devolve `true` (quem chama faz a pergunta, uma vez).
pub fn falta_perguntar_comandos(pasta: &Path) -> bool {
    let Ok(mut pastas) = PASTAS_PERGUNTADAS.lock() else {
        return false;
    };
    if pastas.iter().any(|p| p == pasta) {
        return false;
    }
    pastas.push(pasta.to_path_buf());
    true
}

/// Os nomes dos comandos e skills que o Claude Code oferece nesta pasta, se ele já respondeu.
pub fn comandos_da_pasta(pasta: &Path) -> Option<Vec<String>> {
    let guardados = COMANDOS_POR_PASTA.lock().ok()?;
    let (_, comandos) = guardados.iter().find(|(p, _)| p == pasta)?;
    Some(comandos.iter().map(|c| c.nome.clone()).collect())
}

/// A descrição que o Claude Code deu a um comando ou skill, em qualquer pasta já perguntada.
pub fn descricao_do_comando(nome: &str) -> Option<String> {
    let guardados = COMANDOS_POR_PASTA.lock().ok()?;
    guardados
        .iter()
        .flat_map(|(_, comandos)| comandos.iter())
        .find(|c| c.nome == nome && !c.descricao.is_empty())
        .map(|c| c.descricao.clone())
}

/// Pergunta ao Claude Code instalado quais comandos de barra e skills ele oferece em `pasta` (os
/// dele, os do usuário, os de plugins e os da própria pasta) e guarda. É o mesmo pedido
/// `initialize` dos modelos, sem mensagem: não gasta uso nem cria conversa. Sem isso, o "/" de um
/// chat que ainda não rodou nenhum turno no Vitral só mostrava oito comandos fixos, e as skills
/// só apareciam depois da primeira mensagem (dono, 13/11/2026: "tá sem as skills"). Bloqueia:
/// chamar em segundo plano. Devolve `true` se guardou uma lista.
pub fn atualizar_comandos_da_pasta(pasta: &Path) -> bool {
    if std::env::var_os("CLAUDE_FALSO").is_some() || !pasta.is_dir() {
        return false;
    }
    let Some(comandos) = perguntar_ao_claude(pasta, false).as_ref().and_then(comandos_de).filter(|c| !c.is_empty()) else {
        return false;
    };
    let Ok(mut guardados) = COMANDOS_POR_PASTA.lock() else {
        return false;
    };
    guardados.retain(|(p, _)| p != pasta);
    guardados.push((pasta.to_path_buf(), comandos));
    true
}

/// Os comandos da resposta do `initialize` (os internos, de nome começado em `__`, ficam de fora).
fn comandos_de(resposta: &Value) -> Option<Vec<ComandoOferecido>> {
    let comandos = resposta["commands"].as_array()?;
    Some(
        comandos
            .iter()
            .filter_map(|c| {
                let nome = c["name"].as_str()?.trim().to_string();
                let descricao = c["description"].as_str().unwrap_or_default().trim().to_string();
                (!nome.is_empty() && !nome.starts_with("__")).then_some(ComandoOferecido { nome, descricao })
            })
            .collect(),
    )
}

/// Os modelos de uma linha da saída, se ela é a resposta do `initialize`. Só os modelos: a
/// resposta traz também a conta, que não interessa aqui e não é guardada.
#[cfg(test)]
fn modelos_da_resposta(valor: &Value) -> Option<Vec<ModeloOferecido>> {
    modelos_de(resposta_do_initialize(valor)?)
}

/// Os modelos do miolo da resposta do `initialize`.
fn modelos_de(resposta: &Value) -> Option<Vec<ModeloOferecido>> {
    let modelos = resposta["models"].as_array()?;
    Some(
        modelos
            .iter()
            .filter_map(|m| {
                let valor = m["value"].as_str()?.trim().to_string();
                let resolvido = m["resolvedModel"].as_str().unwrap_or(&valor).trim().to_string();
                let nome = m["displayName"].as_str().unwrap_or_default().trim().to_string();
                (!valor.is_empty()).then_some(ModeloOferecido { valor, resolvido, nome })
            })
            .collect(),
    )
}

/// O que o `claude update` respondeu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Atualizacao {
    /// O comando saiu com 0.
    pub ok: bool,
    /// O que ele imprimiu, linha por linha, sem as vazias.
    pub linhas: Vec<String>,
}

/// No máximo tantas linhas da saída, as últimas (uma instalação por npm imprime algumas); a linha
/// das Configurações não é um console.
const LINHAS_DA_ATUALIZACAO: usize = 8;

/// Roda `claude update` no `claude` que os turnos abrem e devolve o que ele disse. Bloqueia
/// enquanto ele baixa e instala, então quem chama põe em segundo plano; e sempre fora de um
/// turno — no Windows a atualização nem roda com o `claude.exe` em uso ("claude.exe in use
/// (close other Claude Code sessions, including VS Code)").
///
/// O Vitral não decide o caminho da atualização: o próprio comando sabe de cada instalação e
/// imprime o que fez ("Current version: X", "Claude Code is up to date (X)", "Successfully
/// updated from X to version Y"). O que ele não resolve também sai no texto dele, que é o que a
/// interface mostra: a instalação por npm manda rodar o `npm install -g`, a pasta sem permissão
/// de escrita dá "Error: Failed to install native update" (com "Try running \"claude doctor\""),
/// sem rede ele avisa que não deu para procurar, e a instalação nativa sem o `~/.local/bin` no
/// PATH lembra de acertar o PATH. `NO_COLOR` porque o texto vai para a interface, não para um
/// terminal.
pub fn atualizar() -> Atualizacao {
    atualizar_com(&binario_claude())
}

fn atualizar_com(binario: &std::path::Path) -> Atualizacao {
    // Um binário recém-trocado pode dar "Text file busy" por um instante (no Windows, o antivírus
    // segurando o .exe): tenta de novo, como o atualizador do próprio Vitral.
    let mut tentativas = 0;
    let saida = loop {
        let saida = processos::novo_comando(binario)
            .arg("update")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .output();
        match saida {
            Err(erro) if crate::atualizacao::pacote::texto_ocupado(&erro) && tentativas < 20 => {
                tentativas += 1;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            saida => break saida,
        }
    };
    let saida = match saida {
        Ok(saida) => saida,
        Err(erro) => {
            return Atualizacao {
                ok: false,
                linhas: vec![format!("Não deu para abrir o {}: {erro}", binario.display())],
            };
        }
    };
    let mut texto = String::from_utf8_lossy(&saida.stdout).into_owned();
    texto.push_str(&String::from_utf8_lossy(&saida.stderr));
    let mut linhas: Vec<String> = texto
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    // O fim é o que importa: o resultado ("Successfully updated…") e o erro, que vem pelo stderr,
    // ficam depois do progresso.
    if linhas.len() > LINHAS_DA_ATUALIZACAO {
        linhas.drain(..linhas.len() - LINHAS_DA_ATUALIZACAO);
    }
    if linhas.is_empty() {
        linhas.push("O claude não disse nada.".into());
    }
    Atualizacao { ok: saida.status.success(), linhas }
}

/// A atualização instalou uma versão nova (não era só "up to date"): é quando vale avisar que ela
/// vale no turno seguinte.
pub fn instalou_versao_nova(a: &Atualizacao) -> bool {
    a.ok && a.linhas.iter().any(|l| l.contains("Successfully updated") || l.contains("Successfully downgraded"))
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
    // Mensagens de subagentes (ferramenta Task/Agent): só o que ele faz (as ferramentas que usa e
    // o que diz) sobe, preso à chamada que o abriu; o resto do fio dele fica de fora.
    if let Some(pai) = valor["parent_tool_use_id"].as_str() {
        if valor["type"] != "assistant" {
            return Vec::new();
        }
        return valor["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|bloco| match bloco["type"].as_str()? {
                "tool_use" => Some(Evento::SubFerramenta {
                    pai: pai.to_string(),
                    nome: bloco["name"].as_str().unwrap_or_default().to_string(),
                    entrada: bloco["input"].clone(),
                }),
                "text" => bloco["text"]
                    .as_str()
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| Evento::SubTexto { pai: pai.to_string(), texto: t.to_string() }),
                _ => None,
            })
            .collect();
    }
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
            } else if evento["type"] == "content_block_start"
                && matches!(evento["content_block"]["type"].as_str(), Some("thinking" | "redacted_thinking"))
            {
                vec![Evento::Pensando]
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
                    "thinking" => Some(Evento::Pensamento(texto(&bloco["thinking"]))),
                    "redacted_thinking" => Some(Evento::Pensamento(String::new())),
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
        // Um Read de imagem devolve só o bloco "image" (o base64 dela, que não vai para o
        // histórico): sem a marca, a chamada aberta ficava numa caixa vazia, como se tivesse
        // sido interrompida.
        Value::Array(partes) => partes
            .iter()
            .filter_map(|p| match p["type"].as_str() {
                Some("image") => Some(format!("({})", p["source"]["media_type"].as_str().unwrap_or("imagem"))),
                _ => p["text"].as_str().map(str::to_string),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        outro => outro.to_string(),
    }
}

/// O `claude` do PATH ou o de `~/.local/bin` (apps abertos pela dock nem sempre têm esse PATH).
pub fn binario_claude() -> PathBuf {
    binario_claude_em(std::env::var_os("PATH"), dirs::home_dir().unwrap_or_default())
}

/// No Windows, só o `claude.exe` do instalador nativo: o `claude.cmd` do npm precisa de um shell.
const NOME_DO_CLAUDE: &str = if cfg!(windows) { "claude.exe" } else { "claude" };

fn binario_claude_em(path: Option<std::ffi::OsString>, home: PathBuf) -> PathBuf {
    if let Some(caminho) = path.and_then(|path| {
        std::env::split_paths(&path)
            .map(|p| p.join(NOME_DO_CLAUDE))
            .find(|p| p.is_file())
    }) {
        return caminho;
    }
    let locais: &[[&str; 2]] = if cfg!(windows) { &[[".local", "bin"]] } else { &[[".local", "bin"], [".claude", "local"]] };
    locais
        .iter()
        .map(|[pasta, dentro]| home.join(pasta).join(dentro).join(NOME_DO_CLAUDE))
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(NOME_DO_CLAUDE))
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
pub(crate) fn nome_do_projeto(caminho: &str) -> String {
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

/// Quantas linhas de cada lado de uma troca (e de um arquivo gravado) ficam guardadas na conversa.
const LINHAS_DO_DETALHE: usize = 60;
/// O teto do detalhe inteiro: um Write de um arquivo enorme não engorda o chat no disco.
const MAX_DETALHE: usize = 8_000;

/// O que a chamada pediu, para a conversa mostrar ao abrir a linha, e o arquivo dela: o diff de um
/// Edit (o que saiu com "- ", o que entrou com "+ "), o começo do que um Write gravou e o comando
/// inteiro de um Bash, quando a linha da conversa só mostra a descrição ou o começo dele.
pub fn detalhe_ferramenta(nome: &str, entrada: &Value) -> (Option<String>, Option<String>) {
    let campo = |chave: &str| entrada[chave].as_str().filter(|t| !t.is_empty());
    let arquivo = campo("file_path").or_else(|| campo("notebook_path")).map(str::to_string);
    let detalhe = match nome {
        "Edit" => Some(troca(campo("old_string").unwrap_or_default(), campo("new_string").unwrap_or_default())),
        "MultiEdit" => entrada["edits"].as_array().map(|trocas| {
            trocas
                .iter()
                .map(|t| troca(t["old_string"].as_str().unwrap_or_default(), t["new_string"].as_str().unwrap_or_default()))
                .collect::<Vec<_>>()
                .join("\n…\n")
        }),
        "Write" => campo("content").map(|c| com_prefixo(c, "+ ")),
        "NotebookEdit" => campo("new_source").map(|c| com_prefixo(c, "+ ")),
        "Bash" => campo("command")
            .filter(|c| campo("description").is_some() || c.lines().count() > 1 || c.chars().count() > 90)
            .map(|c| format!("$ {c}")),
        _ => None,
    };
    let detalhe = detalhe.filter(|d| !d.trim().is_empty()).map(|d| {
        if d.len() <= MAX_DETALHE {
            return d;
        }
        let mut fim = MAX_DETALHE;
        while !d.is_char_boundary(fim) {
            fim -= 1;
        }
        format!("{}\n… (cortado)", &d[..fim])
    });
    (detalhe, arquivo)
}

/// O que saiu e o que entrou numa troca, cada linha com o seu sinal.
fn troca(antes: &str, depois: &str) -> String {
    match (antes.is_empty(), depois.is_empty()) {
        (true, _) => com_prefixo(depois, "+ "),
        (_, true) => com_prefixo(antes, "- "),
        _ => format!("{}\n{}", com_prefixo(antes, "- "), com_prefixo(depois, "+ ")),
    }
}

fn com_prefixo(texto: &str, prefixo: &str) -> String {
    let total = texto.lines().count();
    let mut linhas: Vec<String> = texto.lines().take(LINHAS_DO_DETALHE).map(|l| format!("{prefixo}{l}")).collect();
    if total > LINHAS_DO_DETALHE {
        linhas.push(format!("… mais {} linhas", total - LINHAS_DO_DETALHE));
    }
    linhas.join("\n")
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

/// O detalhe de uma ferramenta do Vitral: o chat de destino e o título do chat criado e, nas
/// conversas das versões de antes, as partes da divisão ("3 partes: Ninee, Vitral"), o projeto
/// sugerido e o texto lembrado.
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
        "ver_issue" => campo("issue"),
        // "listar_issues · Vitral · abertas" (o projeto só aparece no Mestre).
        "listar_issues" => {
            let estado = if campo("estado").is_some_and(|e| e.eq_ignore_ascii_case("fechadas")) { "fechadas" } else { "abertas" };
            let mut partes: Vec<String> = campo("projeto").map(|p| p.trim_start_matches('#').to_string()).into_iter().collect();
            partes.push(estado.to_string());
            if let Some(busca) = campo("busca") {
                partes.push(format!("\"{busca}\""));
            }
            Some(partes.join(" · "))
        }
        // "abrir_pr · Corrige o login", "mesclar_pr · #12 · squash · quando pronto",
        // "ver_pr · #12 · checks, log_falhas" (o PR, ou o chat no Mestre).
        "abrir_pr" | "mesclar_pr" | "ver_pr" => {
            let sim = |chave: &str| entrada[chave].as_bool().unwrap_or(false) || campo(chave).is_some_and(|v| v == "true");
            let mut partes: Vec<String> = Vec::new();
            if nome == "abrir_pr" {
                partes.extend(campo("titulo"));
                if sim("rascunho") {
                    partes.push("rascunho".into());
                }
                if sim("mesclar_quando_pronto") {
                    partes.push("quando pronto".into());
                }
            } else {
                let pr = match &entrada["pr"] {
                    Value::Number(n) => Some(format!("#{n}")),
                    _ => campo("pr").map(|p| pr_curto(&p)),
                };
                partes.extend(pr.or_else(|| chat("tarefa")));
                partes.extend(campo("metodo").filter(|_| nome == "mesclar_pr"));
                if nome == "mesclar_pr" && sim("quando_pronto") {
                    partes.push("quando pronto".into());
                }
                let incluir: Vec<&str> = entrada["incluir"].as_array().map(|l| l.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                if nome == "ver_pr" && !incluir.is_empty() {
                    partes.push(incluir.join(", "));
                }
            }
            (!partes.is_empty()).then(|| partes.join(" · "))
        }
        _ => campo("titulo")
            .or_else(|| campo("pergunta"))
            .or_else(|| campo("texto"))
            .or_else(|| entrada["tarefa"].as_u64().map(|n| format!("chat {n}"))),
    }
}

/// "12" e "#12" viram "#12"; "dono/nome#12" e a URL do PR, "dono/nome#12".
fn pr_curto(texto: &str) -> String {
    let texto = texto.trim();
    if let Some(resto) = texto.strip_prefix("https://github.com/") {
        let partes: Vec<&str> = resto.split(['#', '?']).next().unwrap_or_default().split('/').collect();
        if let [dono, nome, "pull", n, ..] = partes[..] {
            return format!("{dono}/{nome}#{n}");
        }
        return texto.to_string();
    }
    if texto.chars().all(|c| c.is_ascii_digit()) { format!("#{texto}") } else { texto.to_string() }
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
    fn acha_o_claude_no_path_e_na_pasta_do_instalador() {
        let base = std::env::temp_dir().join(format!("vitral-claude-{}", crate::modelo::novo_id()));
        let (bin, casa) = (base.join("bin"), base.join("casa"));
        let instalado = casa.join(".local").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&instalado).unwrap();
        let path = || Some(std::env::join_paths([&bin]).unwrap());
        // Em lugar nenhum: o nome solto, para o sistema procurar.
        assert_eq!(binario_claude_em(path(), casa.clone()), PathBuf::from(NOME_DO_CLAUDE));
        // O claude.cmd do npm (Windows) não serve: precisa de um shell.
        std::fs::write(bin.join("claude.cmd"), "").unwrap();
        std::fs::write(instalado.join(NOME_DO_CLAUDE), "").unwrap();
        assert_eq!(binario_claude_em(path(), casa.clone()), instalado.join(NOME_DO_CLAUDE));
        std::fs::write(bin.join(NOME_DO_CLAUDE), "").unwrap();
        assert_eq!(binario_claude_em(path(), casa.clone()), bin.join(NOME_DO_CLAUDE));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// O `claude update` com um claude de mentira: a saída dele é o que a interface mostra, e o
    /// código de saída é o que diz se deu certo. Nunca com o claude de verdade, que mexeria na
    /// instalação de quem está rodando o teste.
    #[cfg(unix)]
    #[test]
    fn a_atualizacao_devolve_o_que_o_claude_disse() {
        use std::os::unix::fs::PermissionsExt as _;
        let pasta = std::env::temp_dir().join(format!("vitral-update-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        let falso = pasta.join("claude");
        let escrever = |corpo: &str| {
            std::fs::write(&falso, format!("#!/bin/sh\n[ \"$1\" = update ] || exit 9\n{corpo}\n")).unwrap();
            std::fs::set_permissions(&falso, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        escrever("echo 'Current version: 2.1.318'\necho\necho 'Claude Code is up to date (2.1.318)'");
        let em_dia = atualizar_com(&falso);
        // As linhas vazias saem, e o resto vai inteiro.
        assert_eq!(em_dia, Atualizacao {
            ok: true,
            linhas: vec!["Current version: 2.1.318".into(), "Claude Code is up to date (2.1.318)".into()],
        });
        assert!(!instalou_versao_nova(&em_dia));
        escrever("echo 'Successfully updated from 2.1.318 to version 2.1.320'");
        assert!(instalou_versao_nova(&atualizar_com(&falso)));
        // O que falhou: o texto dele, pelo stderr também, e `ok` falso pelo código de saída.
        escrever("echo 'Error: Failed to install native update'\necho 'claude doctor' >&2\nexit 1");
        let erro = atualizar_com(&falso);
        assert!(!erro.ok && !instalou_versao_nova(&erro));
        assert_eq!(erro.linhas, ["Error: Failed to install native update", "claude doctor"]);
        // Muito texto: ficam as últimas linhas, que é onde estão o resultado e o erro.
        escrever("for i in 1 2 3 4 5 6 7 8 9 10; do echo \"Checking $i\"; done\necho 'Successfully updated from 2.1.318 to version 2.1.320'");
        let longa = atualizar_com(&falso);
        assert_eq!(longa.linhas.len(), LINHAS_DA_ATUALIZACAO);
        assert_eq!(longa.linhas.last().map(String::as_str), Some("Successfully updated from 2.1.318 to version 2.1.320"));
        assert!(instalou_versao_nova(&longa));
        // Calado (um exec que não imprime nada) não deixa a linha vazia.
        escrever("exit 0");
        assert_eq!(atualizar_com(&falso).linhas, ["O claude não disse nada."]);
        // Sem o binário, o erro de abrir é o texto.
        let faltando = pasta.join("nao-existe");
        let sem = atualizar_com(&faltando);
        assert!(!sem.ok && sem.linhas[0].starts_with(&format!("Não deu para abrir o {}", faltando.display())));
        let _ = std::fs::remove_dir_all(&pasta);
    }

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
            agente: crate::modelo::Agente::Claude,
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
            so_mcp_vitral: false,
            esforco: Some("max".into()),
            ultracode: false,
            pasta_conta: None,
            memoria_nativa: false,
            mcp_da_equipe: None,
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
        // Fora do Ultracode: sem fluxos de agentes, mas com o limite deles e com a espera sem
        // teto — um turno comum também deixa agente em segundo plano.
        assert_eq!(args.last().map(String::as_str), Some("Workflow"));
        let env = ambiente(&normal);
        assert!(env.contains(&("CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS".into(), "3".into())));
        assert!(env.contains(&("CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS".into(), "0".into())));

        let ultracode = Opcoes { ultracode: true, ..opcoes() };
        let args = argumentos(&ultracode);
        assert!(!args.iter().any(|a| a == "Workflow"));
        // O Máximo da conversa não vale: o Ultracode roda em Extra.
        assert_eq!(valor_de(&args, "--effort"), Some("ultracode"));
        assert_eq!(args.iter().filter(|a| *a == "--effort" || *a == "--settings").count(), 2);
        let env = ambiente(&ultracode);
        let de = |chave: &str| env.iter().find(|(k, _)| k == chave).map(|(_, v)| v.as_str());
        assert_eq!(de("CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS"), Some("3"));
        assert_eq!(de("CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS"), Some("0"));

        // O --settings leva só o Ultracode.
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
        // Quem já o bloqueia: vai uma vez só, e continua bloqueado até com o Ultracode.
        let bloqueando = Opcoes { bloqueadas: vec!["Task".into(), "Workflow".into()], ..opcoes() };
        let args = argumentos(&bloqueando);
        assert_eq!(args.iter().filter(|a| *a == "Workflow").count(), 1);
        let args = argumentos(&Opcoes { ultracode: true, ..bloqueando });
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
    fn resumo_das_ferramentas_das_conversas_antigas() {
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
    fn resumo_das_ferramentas_do_pr() {
        let r = |nome: &str, v: Value| resumo_ferramenta(&format!("mcp__vitral__{nome}"), &v);
        assert_eq!(r("abrir_pr", json!({ "titulo": "Corrige o login", "descricao": "## O que muda" })), "abrir_pr · Corrige o login");
        assert_eq!(
            r("abrir_pr", json!({ "titulo": "Corrige o login", "mesclar_quando_pronto": true, "rascunho": "true" })),
            "abrir_pr · Corrige o login · rascunho · quando pronto"
        );
        assert_eq!(r("abrir_pr", json!({})), "abrir_pr");
        assert_eq!(r("mesclar_pr", json!({ "pr": "#12", "metodo": "squash" })), "mesclar_pr · #12 · squash");
        assert_eq!(r("mesclar_pr", json!({ "pr": 12, "metodo": "squash", "quando_pronto": true })), "mesclar_pr · #12 · squash · quando pronto");
        assert_eq!(r("mesclar_pr", json!({ "tarefa": 13, "quando_pronto": true })), "mesclar_pr · chat 13 · quando pronto");
        assert_eq!(r("mesclar_pr", json!({})), "mesclar_pr");
        assert_eq!(r("ver_pr", json!({ "pr": "12", "incluir": ["checks", "log_falhas"] })), "ver_pr · #12 · checks, log_falhas");
        assert_eq!(r("ver_pr", json!({ "pr": "https://github.com/o/r/pull/12/checks" })), "ver_pr · o/r#12");
        assert_eq!(r("ver_pr", json!({ "pr": "o/r#12", "metodo": "squash" })), "ver_pr · o/r#12", "o método não é do ver_pr");
        assert_eq!(r("ver_pr", json!({})), "ver_pr");
        assert_eq!(r("ver_issue", json!({ "issue": "ENG-123" })), "ver_issue · ENG-123");
        assert_eq!(r("listar_issues", json!({ "projeto": "#vitral" })), "listar_issues · vitral · abertas");
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
    #[test]
    fn detalhe_do_que_a_chamada_pediu() {
        let edit = json!({ "file_path": "/p/a.rs", "old_string": "a\nb", "new_string": "c" });
        assert_eq!(detalhe_ferramenta("Edit", &edit), (Some("- a\n- b\n+ c".into()), Some("/p/a.rs".into())));
        // Só entrou, só saiu.
        assert_eq!(detalhe_ferramenta("Edit", &json!({ "file_path": "a", "old_string": "", "new_string": "x" })).0.as_deref(), Some("+ x"));
        assert_eq!(detalhe_ferramenta("Edit", &json!({ "file_path": "a", "old_string": "x", "new_string": "" })).0.as_deref(), Some("- x"));
        let varias = json!({ "file_path": "a", "edits": [{ "old_string": "a", "new_string": "b" }, { "old_string": "c", "new_string": "d" }] });
        assert_eq!(detalhe_ferramenta("MultiEdit", &varias).0.as_deref(), Some("- a\n+ b\n…\n- c\n+ d"));
        assert_eq!(detalhe_ferramenta("Write", &json!({ "file_path": "n.txt", "content": "um\ndois\n" })), (Some("+ um\n+ dois".into()), Some("n.txt".into())));
        // O Read só diz o arquivo; o Bash curto e sem descrição já está inteiro na linha.
        assert_eq!(detalhe_ferramenta("Read", &json!({ "file_path": "a.rs" })), (None, Some("a.rs".into())));
        assert_eq!(detalhe_ferramenta("Bash", &json!({ "command": "ls" })), (None, None));
        assert_eq!(detalhe_ferramenta("Bash", &json!({ "command": "ls -la", "description": "Lista" })).0.as_deref(), Some("$ ls -la"));
        assert_eq!(detalhe_ferramenta("Bash", &json!({ "command": "a\nb" })).0.as_deref(), Some("$ a\nb"));
        assert_eq!(detalhe_ferramenta("Grep", &json!({ "pattern": "x" })), (None, None));
        // Um arquivo enorme fica com o começo e a conta do que faltou; o total tem teto.
        let longo = (0..200).map(|i| format!("linha {i}")).collect::<Vec<_>>().join("\n");
        let gravado = detalhe_ferramenta("Write", &json!({ "file_path": "a", "content": longo })).0.unwrap();
        assert_eq!(gravado.lines().count(), 61);
        assert!(gravado.ends_with("… mais 140 linhas"));
        let largo = "ç".repeat(20_000);
        let cortado = detalhe_ferramenta("Write", &json!({ "file_path": "a", "content": largo })).0.unwrap();
        assert!(cortado.len() < 8_100 && cortado.ends_with("… (cortado)"));
    }

}

#[cfg(test)]
mod testes_dos_modelos {
    use super::{ModeloOferecido, modelos_da_resposta};
    use serde_json::json;

    /// Da resposta do `initialize` saem só os modelos (a conta que vem junto não é lida), com o
    /// que cada apelido resolve; qualquer outra linha da saída não é resposta.
    #[test]
    fn modelos_saem_da_resposta_do_initialize() {
        let resposta = json!({
            "type": "control_response",
            "response": { "subtype": "success", "request_id": "x", "response": {
                "account": { "email": "alguem@exemplo.com" },
                "models": [
                    { "value": "opus", "resolvedModel": "claude-opus-5-5", "displayName": "Opus 5.5" },
                    { "value": "claude-fable-5-2", "resolvedModel": "claude-fable-5-2", "displayName": "Fable 5.2" },
                    { "value": "", "displayName": "sem valor" },
                    { "value": "claude-opus-5" },
                ],
            } },
        });
        assert_eq!(
            modelos_da_resposta(&resposta),
            Some(vec![
                ModeloOferecido { valor: "opus".into(), resolvido: "claude-opus-5-5".into(), nome: "Opus 5.5".into() },
                ModeloOferecido {
                    valor: "claude-fable-5-2".into(),
                    resolvido: "claude-fable-5-2".into(),
                    nome: "Fable 5.2".into()
                },
                ModeloOferecido { valor: "claude-opus-5".into(), resolvido: "claude-opus-5".into(), nome: String::new() },
            ])
        );
        assert_eq!(modelos_da_resposta(&json!({ "type": "system", "subtype": "init" })), None);
        // Uma resposta de controle sem a lista (um Claude Code antigo) não vale.
        assert_eq!(modelos_da_resposta(&json!({ "type": "control_response", "response": { "response": {} } })), None);
    }

    /// O raciocínio e o que os subagentes fazem sobem como eventos próprios; o resto do fio de um
    /// subagente (os resultados dele, os pedaços parciais) fica de fora.
    #[test]
    fn raciocinio_e_subagente_viram_eventos() {
        use super::{Evento, interpretar};
        let comeco = json!({ "type": "stream_event", "event": { "type": "content_block_start", "content_block": { "type": "thinking" } } });
        assert!(matches!(interpretar(&comeco).as_slice(), [Evento::Pensando]));
        let pensou = json!({ "type": "assistant", "message": { "content": [{ "type": "thinking", "thinking": "vou olhar o admin" }] } });
        assert!(matches!(interpretar(&pensou).as_slice(), [Evento::Pensamento(t)] if t == "vou olhar o admin"));
        let do_sub = json!({ "type": "assistant", "parent_tool_use_id": "toolu_pai", "message": { "content": [
            { "type": "tool_use", "id": "x", "name": "Read", "input": { "file_path": "a.rs" } },
            { "type": "text", "text": "Lendo o módulo." },
            { "type": "text", "text": "   " },
        ] } });
        match interpretar(&do_sub).as_slice() {
            [Evento::SubFerramenta { pai, nome, .. }, Evento::SubTexto { pai: p2, texto }] => {
                assert_eq!((pai.as_str(), nome.as_str(), p2.as_str(), texto.as_str()), ("toolu_pai", "Read", "toolu_pai", "Lendo o módulo."));
            }
            outro => panic!("{}", outro.len()),
        }
        let resultado_do_sub = json!({ "type": "user", "parent_tool_use_id": "toolu_pai", "message": { "content": [
            { "type": "tool_result", "tool_use_id": "x", "content": "ok" } ] } });
        assert!(interpretar(&resultado_do_sub).is_empty());
    }

    /// A resposta do `mcp_status` como o Claude Code 2.1.322 a deu.
    #[test]
    fn servidores_mcp_da_resposta() {
        use super::servidores_mcp_de;
        let resposta = json!({ "mcpServers": [
            { "name": "central", "status": "connected", "serverInfo": { "name": "central", "version": "1.0.0" }, "transport": "stdio",
              "scope": "user", "tools": [{ "name": "central_painel", "annotations": {} }, { "name": "central_config" }] },
            { "name": "claude.ai Gmail", "status": "needs-auth", "transport": "claudeai-proxy", "scope": "claudeai" },
            { "status": "connected" },
        ] });
        let lidos = servidores_mcp_de(&resposta);
        assert_eq!(lidos.len(), 2);
        assert_eq!((lidos[0].nome.as_str(), lidos[0].estado.as_str(), lidos[0].versao.as_deref()), ("central", "connected", Some("1.0.0")));
        assert_eq!(lidos[0].ferramentas, ["central_painel", "central_config"]);
        assert_eq!((lidos[1].estado.as_str(), lidos[1].escopo.as_str(), lidos[1].ferramentas.len()), ("needs-auth", "claudeai", 0));
        assert!(servidores_mcp_de(&json!({})).is_empty());
    }

    /// Da mesma resposta saem os comandos e skills da pasta, com a descrição, sem os internos.
    #[test]
    fn comandos_saem_da_resposta_do_initialize() {
        use super::{ComandoOferecido, comandos_de, resposta_do_initialize};
        let linha = json!({
            "type": "control_response",
            "response": { "response": { "commands": [
                { "name": "easypanel", "description": "Operar o EasyPanel", "argumentHint": "" },
                { "name": "__interno", "description": "x" },
                { "name": "clear" },
                { "description": "sem nome" },
            ] } },
        });
        let resposta = resposta_do_initialize(&linha).unwrap();
        assert_eq!(
            comandos_de(resposta),
            Some(vec![
                ComandoOferecido { nome: "easypanel".into(), descricao: "Operar o EasyPanel".into() },
                ComandoOferecido { nome: "clear".into(), descricao: String::new() },
            ])
        );
        assert_eq!(resposta_do_initialize(&json!({ "type": "system" })), None);
        assert_eq!(comandos_de(&json!({})), None);
    }

    fn oferecido(valor: &str, resolvido: &str) -> ModeloOferecido {
        ModeloOferecido { valor: valor.into(), resolvido: resolvido.into(), nome: String::new() }
    }

    /// A família segue a versão mais nova que o Claude Code oferece, mesmo quando o apelido dele
    /// ainda aponta para a anterior (a lista é a do 2.1.314, que tem o Fable 5.2 à parte).
    #[test]
    fn a_familia_segue_a_versao_mais_nova_oferecida() {
        use super::{mais_nova_da_familia, modelo_para_o_turno};
        let oferecidos = [
            oferecido("default", "claude-opus-5-5"),
            oferecido("opus", "claude-opus-5-5"),
            oferecido("claude-fable-5-2", "claude-fable-5-2"),
            oferecido("haiku", "claude-haiku-5-5"),
            oferecido("fable", "claude-fable-5-1"),
            oferecido("claude-haiku-4-5-20251001", "claude-haiku-4-5-20251001"),
            oferecido("claude-opus-5", "claude-opus-5"),
            oferecido("claude-fable-5", "claude-fable-5"),
        ];
        assert_eq!(mais_nova_da_familia(&oferecidos, "fable").as_deref(), Some("claude-fable-5-2"));
        assert_eq!(mais_nova_da_familia(&oferecidos, "opus").as_deref(), Some("claude-opus-5-5"));
        assert_eq!(mais_nova_da_familia(&oferecidos, "haiku").as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(mais_nova_da_familia(&oferecidos, "sonnet"), None, "nada do Sonnet foi oferecido");
        // O apelido atrasado vira o id da versão nova; o que já está em dia vai como apelido.
        assert_eq!(modelo_para_o_turno(&oferecidos, "fable"), "claude-fable-5-2");
        assert_eq!(modelo_para_o_turno(&oferecidos, "opus"), "opus");
        assert_eq!(modelo_para_o_turno(&oferecidos, "haiku"), "haiku");
        // Uma versão exata escolhida fica: quem a fixou quer aquela.
        assert_eq!(modelo_para_o_turno(&oferecidos, "claude-fable-5-1"), "claude-fable-5-1");
        assert_eq!(modelo_para_o_turno(&oferecidos, "claude-fable-5"), "claude-fable-5");
        // Sem resposta do Claude Code, ou com um valor que ele não listou, nada muda.
        assert_eq!(modelo_para_o_turno(&[], "fable"), "fable");
        assert_eq!(modelo_para_o_turno(&oferecidos, "sonnet"), "sonnet");
        assert_eq!(modelo_para_o_turno(&oferecidos, ""), "");
    }

    /// Manual, com o Claude Code instalado (`cargo test -- --ignored integracao_modelos`): a
    /// pergunta de verdade, numa pasta vazia. Não manda mensagem, então não gasta uso; a pasta
    /// continua vazia e só do dono depois dela.
    #[test]
    #[ignore]
    #[cfg(unix)]
    fn integracao_modelos_do_claude_de_verdade() {
        use std::os::unix::fs::PermissionsExt as _;
        let pasta = std::env::temp_dir().join(format!("vitral-modelos-{}", crate::modelo::novo_id())).join("perguntas");
        let modelos = super::perguntar_modelos(&pasta).expect("o Claude Code instalado não respondeu");
        assert!(modelos.iter().any(|m| m.resolvido.starts_with("claude-")), "{modelos:?}");
        assert_eq!(std::fs::metadata(&pasta).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::read_dir(&pasta).unwrap().count(), 0, "a pergunta deixou arquivo na pasta");
        let _ = std::fs::remove_dir_all(pasta.parent().unwrap());
    }
}
