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
    /// Com `bifurcar`: a cópia leva a sessão só até a entrada com este uuid, inclusive
    /// (`--resume-session-at`, que o Claude Code 2.1.325 tem fora do `--help`).
    pub cortar_em: Option<String>,
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
    /// `--prompt-suggestions`: depois do turno, o Claude Code manda um `prompt_suggestion` com a
    /// próxima mensagem provável (`Evento::Sugestao`). Ele só sai com a entrada ainda aberta.
    pub sugestoes: bool,
    /// O arquivo com o servidor MCP da memória da equipe (`memoria_equipe::arquivo_do_mcp`): vai
    /// como mais um `--mcp-config`.
    pub mcp_da_equipe: Option<PathBuf>,
    /// `--include-hook-events`: os eventos dos hooks do usuário na saída. É por eles que chega o
    /// motivo de um hook que mandou perguntar (`Evento::HookPediu`); o pedido de permissão vem
    /// sem ele. Só num Claude Code que conhece a opção (`VERSAO_EVENTOS_DE_HOOK`): sem ela, o
    /// cartão aparece igual, sem o motivo.
    pub eventos_de_hook: bool,
    /// `--thinking-display summarized`: o texto do raciocínio vem na saída (`Evento::Pensamento`).
    /// Sem a opção, o Claude Code 2.1.328 manda o bloco vazio. Só num Claude Code que a conhece
    /// (`VERSAO_RACIOCINIO_RESUMIDO`).
    pub raciocinio_resumido: bool,
}

/// O que dá para trocar num turno que já está rodando, pela entrada do processo: o modo de
/// permissão, o modelo e o esforço. Medido no Claude Code 2.1.328 em 23/11/2026, com o Haiku, no
/// meio de um `sleep` do Bash:
/// - `set_permission_mode` responde `{"mode": "acceptEdits"}` e manda um `system`/`status` com o
///   `permissionMode`; o Write que o modelo pediu em seguida, no mesmo turno, passou sem perguntar
///   (sem o pedido, voltou negado). `manual` é aceito e respondido como `default`; um nome que não
///   existe volta erro (`invalid_mode`); o `bypassPermissions` volta erro (`bypass_not_launched`)
///   num processo aberto sem `--allow-dangerously-skip-permissions`, e com ela entra;
/// - `set_model` responde `{"model": "<id resolvido>"}` e a chamada seguinte ao modelo, no mesmo
///   turno, já sai no modelo novo (o `model` da mensagem); um nome que não existe volta erro
///   (`not_found`), e sem `model` ele volta ao padrão;
/// - `apply_flag_settings` com `{"effortLevel": "low"}` responde sucesso sem corpo, e o
///   `get_settings` passa a dizer `applied.effort: "low"`; `null` volta ao automático; um nível
///   que não existe também responde sucesso e não muda nada.
///
/// Na 2.1.323 os três pedidos respondem igual (conferido sem mandar mensagem). Uma recusa não vira
/// nada na tela: a troca fica gravada na conversa e vale no turno seguinte, como sempre valeu.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AoVivo {
    /// O `--permission-mode` (`manual`, `acceptEdits`, `plan`, `auto`, `bypassPermissions`).
    pub modo: String,
    /// O `--model`; vazio é o padrão do Claude Code.
    pub modelo: String,
    /// O `--effort`; `None` é o automático.
    pub esforco: Option<String>,
}

impl AoVivo {
    /// O que o processo tem ao abrir. O turno com Ultracode fica marcado: o esforço dele não se
    /// troca no meio (ver `pedidos_ao_vivo`).
    fn das_opcoes(opcoes: &Opcoes) -> Self {
        Self {
            modo: opcoes.permissao.clone(),
            modelo: opcoes.modelo.clone().unwrap_or_default(),
            esforco: if opcoes.ultracode { Some("ultracode".into()) } else { opcoes.esforco.clone().filter(|e| !e.is_empty()) },
        }
    }
}

/// Os pedidos de controle (o `request` de cada um) que levam o processo do que ele tem para o que
/// a conversa pede agora; vazio quando nada mudou. O Ultracode é do turno inteiro (os fluxos de
/// agentes entram pelos argumentos do processo): com ele de um lado ou do outro, o esforço não
/// vai, e vale no turno seguinte.
fn pedidos_ao_vivo(tem: &AoVivo, quer: &AoVivo) -> Vec<Value> {
    let mut pedidos = Vec::new();
    if quer.modo != tem.modo {
        // O Manual do Vitral é o `default` do pedido: o 2.1.328 aceita `manual` e responde
        // `default`, e a lista de modos do erro dele só tem `default`.
        let modo = if quer.modo == "manual" { "default" } else { quer.modo.as_str() };
        pedidos.push(json!({ "subtype": "set_permission_mode", "mode": modo }));
    }
    if quer.modelo != tem.modelo {
        // Sem `model`, o Claude Code volta ao padrão dele.
        pedidos.push(match quer.modelo.as_str() {
            "" => json!({ "subtype": "set_model" }),
            modelo => json!({ "subtype": "set_model", "model": modelo }),
        });
    }
    let ultracode = |esforco: &Option<String>| esforco.as_deref() == Some("ultracode");
    if quer.esforco != tem.esforco && !ultracode(&tem.esforco) && !ultracode(&quer.esforco) {
        pedidos.push(json!({ "subtype": "apply_flag_settings", "settings": { "effortLevel": quer.esforco } }));
    }
    pedidos
}

/// O pedido que tira da fila interna do Claude Code a mensagem dirigida de `uuid` (o `uuid` com
/// que ela foi escrita na entrada). Não está no tipo público do SDK; medido no 2.1.328: com a
/// mensagem ainda na fila, responde `{"cancelled": true, "message_uuid": …}` e manda um
/// `command_lifecycle` com `state: "cancelled"`, e a sessão gravada fica sem ela; com a mensagem
/// já absorvida no turno (ou um uuid que ele não conhece), `{"cancelled": false}`. A 2.1.323
/// responde igual.
fn pedido_de_retirar(uuid: &str) -> Value {
    json!({ "subtype": "cancel_async_message", "message_uuid": uuid })
}

/// O pedido do título da sessão: o Claude Code gera um nome curto para a conversa a partir da
/// `description` (a primeira fala) e, com o `persist`, grava a linha `ai-title` no arquivo da
/// sessão, de onde o motor a lê depois do turno. É o `generateSessionTitle` do SDK, e o que a
/// extensão do VS Code usa. Medido no 2.1.328 em 23/11/2026, com o Haiku, numa pasta de `/tmp`:
/// - num `-p` o Claude Code não gera o título sozinho: num turno inteiro o arquivo da sessão ficou
///   com 65 linhas e nenhuma `ai-title`. O título automático dele fica desligado quando o
///   `CLAUDE_CODE_ENTRYPOINT` é `sdk-cli` e não há `CLAUDE_AGENT_SDK_VERSION` no ambiente (o SDK a
///   define; o Vitral não finge ser o SDK), e também em toda sessão retomada;
/// - com o pedido na entrada logo depois da mensagem, a resposta `{"title": "Erro na função soma
///   em soma.rs"}` chegou 0,3 s depois do `init`, e a linha estava no arquivo 0,1 s depois dela,
///   com o turno ainda rodando; daí em diante o Claude Code a repete a cada gravação;
/// - numa sessão que já tem título (o segundo turno, retomado), volta o que ela tem, sem gerar
///   outro; e, pelo código dele (não medido), a descrição com menos de 10 letras ou que é só um
///   comando de barra volta `{"title": null}` sem chamar o modelo.
///
/// O tratamento do pedido é o mesmo no 2.1.323 (conferido no código dele, sem rodar).
fn pedido_do_titulo(descricao: &str) -> Value {
    json!({ "subtype": "generate_session_title", "description": descricao, "persist": true })
}

/// A linha de um pedido de controle; `nome` entra no id (`vitral-<nome>-<uuid>`), que é como a
/// resposta é reconhecida na saída (`resposta_de_controle`).
fn linha_do_pedido(nome: &str, corpo: Value) -> Value {
    json!({
        "type": "control_request",
        "request_id": format!("vitral-{nome}-{}", uuid::Uuid::new_v4()),
        "request": corpo,
    })
}

/// O nome que entra no id do pedido, pelo subtipo dele.
fn nome_do_pedido(corpo: &Value) -> &'static str {
    match corpo["subtype"].as_str() {
        Some("set_permission_mode") => "modo",
        Some("set_model") => "modelo",
        Some("apply_flag_settings") => "esforco",
        Some("cancel_async_message") => "retirar",
        Some("generate_session_title") => "titulo",
        _ => "pedido",
    }
}

/// A `--allow-dangerously-skip-permissions` vai em todo turno, para o "Sem perguntar" poder
/// entrar no meio dele: sem ela, o `set_permission_mode` para `bypassPermissions` volta
/// `bypass_not_launched` (medido no 2.1.328). Menos como root: aí o Claude Code nem abre com a
/// opção ("cannot be used with root/sudo privileges"), e só o Planejar a leva, como já levava
/// (aprovar o plano pode passar para "Sem perguntar").
fn libera_o_bypass(modo: &str, root: bool) -> bool {
    modo == "plan" || !root
}

/// O Vitral roda como root (no Windows não há o que conferir).
fn sou_root() -> bool {
    #[cfg(unix)]
    // SAFETY: `geteuid` não tem pré-condição nem falha.
    return unsafe { libc::geteuid() } == 0;
    #[cfg(not(unix))]
    false
}

/// O relatório que o subagente devolve, sem a moldura que o Claude Code põe em volta dele
/// (2.1.325): o `<tools-used-before-summary>`, o aviso "[Subagent hand-back] … The report
/// follows:", o recuo de cada linha e o `agentId` para continuar pelo SendMessage. Sem a moldura,
/// o texto volta como veio.
pub fn relatorio_do_subagente(texto: &str) -> String {
    let mut resto = texto.to_string();
    while let (Some(a), Some(b)) = (resto.find("<tools-used-before-summary>"), resto.find("</tools-used-before-summary>")) {
        if b < a {
            break;
        }
        resto.replace_range(a..b + "</tools-used-before-summary>".len(), "");
    }
    if resto.contains("[Subagent hand-back]")
        && let Some(i) = resto.find("The report follows:")
    {
        resto = resto[i + "The report follows:".len()..].to_string();
    }
    let mut linhas: Vec<&str> = resto.lines().collect();
    if let Some(i) = linhas.iter().rposition(|l| l.trim_start().starts_with("agentId:")) {
        linhas.truncate(i);
    }
    let recuo = linhas.iter().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
    let limpo = linhas.iter().map(|l| l.get(recuo..).unwrap_or(l.trim_start())).collect::<Vec<_>>().join("\n");
    let limpo = limpo.trim();
    if limpo.is_empty() { texto.trim().to_string() } else { limpo.to_string() }
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
    /// O `uuid` com que a mensagem vai ao Claude Code: o ponto para voltar os arquivos a antes dela.
    pub marca: Option<&'a str>,
}

/// Um agente (ou fluxo de agentes) que o Claude Code roda em segundo plano, como o
/// `background_tasks_changed` o lista (medido na 2.1.328, 23/11/2026): o `task_id` é o id do
/// agente, o mesmo do arquivo dele (`<sessão>/subagents/agent-<id>.jsonl`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TarefaDeFundo {
    pub id: String,
    pub descricao: String,
    /// Um fluxo de agentes (`local_workflow`), e não um subagente só.
    pub fluxo: bool,
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
    /// Os mesmos agentes e fluxos, um por um: o id e a descrição de cada (a lista vem sempre
    /// inteira, e quem saiu dela terminou).
    AgentesNoFundo(Vec<TarefaDeFundo>),
    /// O Claude Code abriu o agente `id` pela chamada `pai` (`task_started`): é o que liga um
    /// agente da lista do segundo plano ao cartão dele na conversa.
    AgenteAbriu { id: String, pai: String },
    /// Os comandos de shell que o agente deixou rodando em segundo plano (`run_in_background`):
    /// o id da tarefa (para o `stop_task`) e a descrição. Vazio quando não há mais nenhum.
    Comandos(Vec<(String, String)>),
    /// A conversa do lado do agente ganhou este id (o `thread_id` do Codex): é ele que retoma.
    Sessao(String),
    /// Tokens que a conversa ocupa no contexto agora.
    Contexto(u64),
    TextoParcial(String),
    Texto(String),
    /// O modelo começou um bloco de raciocínio (o motor mede quanto ele dura).
    Pensando,
    /// O raciocínio do bloco que terminou. O texto vem vazio quando o Claude Code não o entrega
    /// (sem `--thinking-display summarized`; ver `Opcoes::raciocinio_resumido`).
    Pensamento(String),
    /// O processo está vivo e não há nada para mostrar: a batida que o Claude Code manda a cada
    /// 30 s enquanto uma ferramenta roda (`tool_progress` com `heartbeat`). Só conta como sinal de
    /// vida, para um build longo não ganhar o aviso "sem sinal" (`Motor::silencio`).
    Vivo,
    /// A resposta ao `interrupt` disse que ainda há mensagens dirigidas na fila interna do Claude
    /// Code (`still_queued`): ele abriria outro turno com elas depois do Parar.
    FicouNaFila(usize),
    /// O `set_model` no meio do turno valeu: o id do modelo que o turno passa a usar.
    ModeloEmUso(String),
    /// O Claude Code está resumindo a conversa para liberar contexto (`status: "compacting"`, que
    /// ele repete a cada 30 s enquanto dura), ou parou de resumir (o `status` com o
    /// `compact_result`).
    Compactando(bool),
    /// A conversa foi compactada aqui (`compact_boundary`): pelo `/compact` (`manual`) ou sozinha,
    /// e quantos tokens ela ocupava antes (`None` se o Claude Code não disse).
    Compactou { manual: bool, antes: Option<u64> },
    /// Um subagente (`pai` é o id da chamada Task/Agent que o abriu) usou uma ferramenta; `id` é
    /// o da chamada dele (é o que um pedido de permissão dele traz).
    SubFerramenta {
        pai: String,
        id: String,
        nome: String,
        entrada: Value,
    },
    /// O subagente entregou o relatório a quem o abriu pela chamada `SubagentHandback` (2.1.271
    /// em diante, no modo automático): o resultado da chamada `pai` só diz que ele foi entregue.
    SubRelatorio {
        pai: String,
        texto: String,
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
        /// O comando foi para o segundo plano e segue rodando lá (pedido assim, ou porque passou
        /// do tempo dele): o resultado chegou, mas não é o fim do comando.
        no_fundo: bool,
    },
    /// As imagens que o resultado da chamada `id` trouxe (o print do navegador por MCP, a imagem
    /// gerada, o Read de uma imagem), já fora do base64. Vem logo depois do `ResultadoFerramenta`
    /// dela; o motor as grava na pasta do chat e a conversa fica só com os caminhos.
    ImagensDoResultado {
        id: String,
        imagens: Vec<crate::imagens_recebidas::ImagemRecebida>,
    },
    /// Um hook `PreToolUse` do usuário mandou perguntar antes de uma chamada dessa ferramenta
    /// (`permissionDecision: "ask"`), com o motivo que ele deu (pode vir vazio). Vem do
    /// `hook_response` (`--include-hook-events`), logo antes do pedido de permissão da chamada,
    /// que não traz o motivo (`motor/pedidos_de_hook.rs`).
    HookPediu { ferramenta: String, motivo: String },
    /// O subagente da chamada `pai` foi para o segundo plano: o resultado da chamada é só o aviso
    /// de que ele começou, e o de verdade chega no `SubFim`.
    SubLancado { pai: String },
    /// O Claude Code abriu o subagente da chamada `pai` (`task_started`): ele já conta, com 0
    /// ferramentas e o relógio andando, antes da primeira ferramenta dele.
    SubInicio { pai: String },
    /// O Claude Code contou quantas ferramentas o subagente em segundo plano já usou.
    SubProgresso { pai: String, ferramentas: u32, tokens: u64 },
    /// O subagente em segundo plano terminou (ou falhou), com o resumo do que fez.
    SubFim { pai: String, erro: bool, resumo: String, ferramentas: u32, tokens: u64 },
    /// O Codex ou o Gemini confirmou (ou recusou) uma mensagem enviada durante o turno.
    DirecaoConfirmada { texto: String, aceita: bool },
    /// O Gemini pede permissão para uma chamada (`session/request_permission` do ACP): vai para o
    /// mesmo cartão dos pedidos do Claude Code, e a resposta (no formato do `permissao` do MCP,
    /// `behavior: allow|deny`) volta por `resposta`.
    Permissao {
        ferramenta: String,
        entrada: Value,
        resposta: smol::channel::Sender<crate::mcp::Resposta>,
    },
    /// A próxima mensagem provável do usuário (`prompt_suggestion`, com `--prompt-suggestions`),
    /// depois do `result`.
    Sugestao(String),
    /// Os tokens que um `result` declara (os do turno; ver `motor::tokens_do_resultado`). Vem
    /// antes do `Fim` do mesmo `result`.
    Gasto(crate::motor::Tokens),
    /// Os tempos que um `result` declara (ver `motor::medida_do_resultado`), também antes do `Fim`.
    Medida(crate::motor::Medida),
    /// A primeira palavra do modelo chegou, tantos milissegundos depois de o processo abrir (uma
    /// vez por processo). Menos o `ttft_ms` do `result`, é quanto custou subir o Claude Code.
    PrimeiraPalavra(u64),
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

/// O canal de um agente que fala JSON-RPC pela entrada (o Codex e o Gemini): ele fica com a
/// entrada e sabe dirigir e parar o turno no protocolo de cada um.
pub trait CanalDoAgente: Send {
    fn aberta(&self) -> bool;
    fn fechar(&mut self);
    fn dirigir(&mut self, texto: &str, imagens: &[PathBuf]) -> std::io::Result<()>;
    fn interromper(&mut self) -> std::io::Result<()>;
}

impl CanalDoAgente for crate::agente_codex::servidor::Canal {
    fn aberta(&self) -> bool {
        Self::aberta(self)
    }
    fn fechar(&mut self) {
        Self::fechar(self)
    }
    fn dirigir(&mut self, texto: &str, imagens: &[PathBuf]) -> std::io::Result<()> {
        Self::dirigir(self, texto, imagens)
    }
    fn interromper(&mut self) -> std::io::Result<()> {
        Self::interromper(self)
    }
}

impl CanalDoAgente for crate::agente_gemini::Canal {
    fn aberta(&self) -> bool {
        Self::aberta(self)
    }
    fn fechar(&mut self) {
        Self::fechar(self)
    }
    fn dirigir(&mut self, texto: &str, imagens: &[PathBuf]) -> std::io::Result<()> {
        Self::dirigir(self, texto, imagens)
    }
    fn interromper(&mut self) -> std::io::Result<()> {
        Self::interromper(self)
    }
}

/// O tradutor da saída de um agente JSON-RPC: uma linha lida, os eventos dela.
type Tradutor = Box<dyn FnMut(&Value) -> Vec<Evento> + Send>;

/// A vez da mensagem com que o processo abriu, pelo que o Claude Code diz dela na saída
/// (`command_lifecycle`: `queued`, `started`, `completed`). Serve para uma coisa só: reconhecer o
/// `result` que chega ANTES de ela começar, que não é o fim do turno dela.
///
/// Medido no 2.1.328 em 23/11/2026 (Haiku, pasta em /tmp): ao retomar uma sessão cujo processo
/// anterior foi derrubado com um agente em segundo plano (o Parar de quem esperava os agentes), o
/// Claude Code manda o aviso do agente cortado (`task_notification` com `stopped`), põe a mensagem
/// na fila (`queued`), manda um `init` e um `result` de sucesso vazio, com `num_turns` 0, e só
/// depois começa a mensagem (`started`, outro `init`, a resposta, o `result` dela). O motor tomava
/// o primeiro `result` pelo fim do turno: o chat ficava parado com o processo no meio do turno, e
/// a mensagem seguinte (ou a revisão da memória) abria OUTRO processo da mesma sessão.
pub(crate) struct VezDaMensagem {
    uuid: Option<String>,
    na_fila: bool,
}

impl VezDaMensagem {
    pub(crate) fn nova(uuid: Option<&str>) -> Self {
        Self { uuid: uuid.map(str::to_string), na_fila: false }
    }

    /// Lê uma linha da saída e diz se ela é um `result` de antes da vez da mensagem (a tirar).
    pub(crate) fn result_de_antes(&mut self, valor: &Value) -> bool {
        let Some(uuid) = self.uuid.as_deref() else {
            return false;
        };
        match valor["type"].as_str() {
            Some("command_lifecycle") if valor["command_uuid"] == uuid => {
                self.na_fila = valor["state"] == "queued";
                false
            }
            // Só o `result` vazio que foi medido: sem turno do modelo e sem erro. Um Claude Code
            // que nunca dissesse o `started` não prende o turno: o `result` de verdade passa.
            Some("result") => self.na_fila && valor["num_turns"] == 0 && valor["is_error"] != true,
            _ => false,
        }
    }
}

pub struct Processo {
    filho: Child,
    /// Escrita numa thread própria (`EntradaDeFundo`): o processo pode demorar a ler.
    entrada: Option<processos::EntradaDeFundo>,
    /// O canal do Codex ou do Gemini, que fica com a entrada (`CanalDoAgente`).
    canal: Option<Arc<Mutex<dyn CanalDoAgente>>>,
    escopo: Escopo,
    parado: bool,
    /// O modo, o modelo e o esforço que o processo tem agora: os dos argumentos e o que foi
    /// trocado depois pela entrada (`acertar`).
    ao_vivo: AoVivo,
    /// Os `uuid` das mensagens dirigidas a este processo (`dirigir`): é por eles que o Parar as
    /// tira da fila interna do Claude Code (`retirar_dirigidas`).
    dirigidas: Vec<String>,
}

impl Processo {
    pub fn iniciar(
        opcoes: &Opcoes,
        mensagem: Mensagem,
        enviar: impl FnMut(Evento) + Send + 'static,
    ) -> Result<Self> {
        let agente = opcoes.agente;
        let comando = if agente == crate::modelo::Agente::Codex {
            let mut comando = processos::comando(crate::agente_codex::binario(), "codex");
            comando.current_dir(&opcoes.cwd).args(crate::agente_codex::argumentos_servidor(opcoes));
            if let Some((_, chave)) = opcoes.mcp_da_equipe.as_deref().and_then(crate::memoria_equipe::conexao_do_arquivo) {
                comando.env("VITRAL_MEMORIA_EQUIPE_TOKEN", chave);
            }
            comando
        } else if agente == crate::modelo::Agente::Gemini {
            let mut comando = processos::comando(crate::agente_gemini::binario(), "gemini");
            comando.current_dir(&opcoes.cwd).args(crate::agente_gemini::argumentos(opcoes));
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
        Self::com_o_comando(comando, opcoes, mensagem, enviar)
    }

    /// Abre o processo de `comando` (o `claude` ou o `codex`, já com os argumentos e o ambiente do
    /// turno), escreve a mensagem e põe a leitura da saída para andar. À parte do `iniciar` para
    /// os testes abrirem o claude de mentira no lugar do instalado.
    fn com_o_comando(
        mut comando: processos::Comando,
        opcoes: &Opcoes,
        mensagem: Mensagem,
        mut enviar: impl FnMut(Evento) + Send + 'static,
    ) -> Result<Self> {
        let agente = opcoes.agente;
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

        // O relógio da primeira palavra começa aqui, antes de o processo existir.
        let abriu = Instant::now();
        let (mut filho, escopo) =
            processos::abrir(comando).with_context(|| format!("não abriu o {}", agente.programa()))?;
        let pid = filho.id();
        let saida = filho.stdout.take().unwrap();
        let erros = filho.stderr.take().unwrap();
        let mut entrada = filho.stdin.take().map(processos::EntradaDeFundo::nova);
        // O Codex e o Gemini falam JSON-RPC: a entrada vai para o canal do protocolo de cada um, e
        // a saída é traduzida para os mesmos eventos do Claude Code.
        let (canal, mut tradutor): (Option<Arc<Mutex<dyn CanalDoAgente>>>, Option<Tradutor>) = match agente {
            crate::modelo::Agente::Codex => match entrada.take() {
                Some(stdin) => {
                    let canal = Arc::new(Mutex::new(crate::agente_codex::servidor::Canal::novo(Box::new(stdin))));
                    let mut protocolo = crate::agente_codex::servidor::Protocolo::novo(
                        canal.clone(),
                        crate::agente_codex::parametros_servidor(opcoes),
                        crate::agente_codex::servidor::entrada(mensagem.texto, mensagem.imagens),
                        opcoes.esforco.clone(),
                    );
                    protocolo.iniciar()?;
                    (Some(canal), Some(Box::new(move |v: &Value| protocolo.linha(v))))
                }
                None => (None, None),
            },
            crate::modelo::Agente::Gemini => match entrada.take() {
                Some(stdin) => {
                    let canal = Arc::new(Mutex::new(crate::agente_gemini::Canal::novo(Box::new(stdin))));
                    let mut protocolo =
                        crate::agente_gemini::Protocolo::novo(canal.clone(), opcoes, mensagem.texto, mensagem.imagens);
                    protocolo.iniciar()?;
                    (Some(canal), Some(Box::new(move |v: &Value| protocolo.linha(v))))
                }
                None => (None, None),
            },
            crate::modelo::Agente::Claude => (None, None),
        };

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
        let mut vez = VezDaMensagem::nova(mensagem.marca.filter(|_| agente == crate::modelo::Agente::Claude));
        std::thread::spawn(move || {
            // O Codex e o Gemini falam outro formato, traduzido para os mesmos eventos.
            // A primeira palavra do modelo, contada de quando o processo abriu: uma vez só.
            let mut falta_a_primeira = true;
            for linha in BufReader::new(saida).lines() {
                let Ok(linha) = linha else { break };
                let Ok(valor) = serde_json::from_str::<Value>(&linha) else {
                    continue;
                };
                // O `result` de antes da vez da mensagem não é o fim do turno dela.
                if vez.result_de_antes(&valor) {
                    continue;
                }
                let eventos = match &mut tradutor {
                    Some(traduzir) => traduzir(&valor),
                    None => {
                        let mut eventos = interpretar(&valor);
                        eventos.extend(agentes_do_fundo(&valor));
                        eventos
                    }
                };
                let palavra = falta_a_primeira
                    && match tradutor.is_some() {
                        true => eventos.iter().any(|e| {
                            matches!(e, Evento::TextoParcial(_) | Evento::Texto(_) | Evento::Pensando | Evento::Pensamento(_) | Evento::Ferramenta { .. })
                        }),
                        false => primeira_palavra(&valor),
                    };
                if palavra {
                    falta_a_primeira = false;
                    enviar(Evento::PrimeiraPalavra(abriu.elapsed().as_millis() as u64));
                }
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

        if let Some(entrada) = entrada.as_mut() {
            escrever_mensagem(entrada, mensagem)?;
        }
        Ok(Self {
            filho,
            entrada,
            canal,
            escopo,
            parado: false,
            ao_vivo: AoVivo::das_opcoes(opcoes),
            dirigidas: Vec::new(),
        })
    }

    /// O modo, o modelo e o esforço que o processo tem agora.
    pub fn ao_vivo(&self) -> &AoVivo {
        &self.ao_vivo
    }

    /// Leva o turno que roda para o que a conversa pede agora (ver `AoVivo`): escreve na entrada os
    /// pedidos do que mudou e devolve quantos foram. Zero sem entrada (o turno já acabou, ou o
    /// Parar a levou), no Codex ou sem nada a trocar: aí a escolha vale no turno seguinte, que
    /// abre com ela nos argumentos. A resposta de cada pedido chega depois, pela saída; uma recusa
    /// não vira nada na tela.
    pub fn acertar(&mut self, quer: &AoVivo) -> usize {
        let mut foram = 0;
        for pedido in pedidos_ao_vivo(&self.ao_vivo, quer) {
            let subtipo = pedido["subtype"].as_str().unwrap_or_default().to_string();
            if !self.pedir(pedido) {
                break;
            }
            foram += 1;
            match subtipo.as_str() {
                "set_permission_mode" => self.ao_vivo.modo.clone_from(&quer.modo),
                "set_model" => self.ao_vivo.modelo.clone_from(&quer.modelo),
                _ => self.ao_vivo.esforco.clone_from(&quer.esforco),
            }
        }
        foram
    }

    /// O modo do processo mudou por outro caminho (aprovar o plano o troca na resposta do pedido
    /// de permissão): o próximo `acertar` não o pede de novo.
    pub fn anotar_modo(&mut self, modo: &str) {
        self.ao_vivo.modo = modo.to_string();
    }

    /// Pede ao Claude Code o título da sessão, gravado no arquivo dela (`pedido_do_titulo`). O
    /// turno segue enquanto ele responde. `false` sem entrada, no Codex ou se a escrita falhou.
    pub fn pedir_titulo(&mut self, descricao: &str) -> bool {
        self.pedir(pedido_do_titulo(descricao))
    }

    /// Tira da fila interna do Claude Code as mensagens dirigidas a este processo que ele ainda não
    /// começou (`pedido_de_retirar`), para o `interrupt` que vem em seguida parar o turno sem ele
    /// abrir outro com elas. `false` sem entrada ou se a escrita falhou: aí só matando o processo.
    pub fn retirar_dirigidas(&mut self) -> bool {
        if self.canal.is_some() || self.entrada.is_none() {
            return false;
        }
        let dirigidas = std::mem::take(&mut self.dirigidas);
        dirigidas.iter().all(|uuid| self.pedir(pedido_de_retirar(uuid)))
    }

    /// Escreve um pedido de controle na entrada. `false` sem ela, no Codex e no Gemini (a entrada
    /// deles é do canal do protocolo) ou se a escrita falhou.
    fn pedir(&mut self, corpo: Value) -> bool {
        if self.canal.is_some() {
            return false;
        }
        let Some(entrada) = self.entrada.as_mut() else {
            return false;
        };
        let pedido = linha_do_pedido(nome_do_pedido(&corpo), corpo);
        writeln!(entrada, "{pedido}").and_then(|()| entrada.flush()).is_ok()
    }

    /// Para uma tarefa de fundo do Claude Code (o comando deixado rodando): o `stop_task` do
    /// protocolo, pela entrada. `false` sem entrada (o processo já está saindo e leva as dele).
    pub fn parar_tarefa(&mut self, tarefa: &str) -> bool {
        let Some(entrada) = self.entrada.as_mut() else {
            return false;
        };
        let pedido = json!({
            "type": "control_request",
            "request_id": format!("vitral-parar-tarefa-{}", uuid::Uuid::new_v4()),
            "request": { "subtype": "stop_task", "task_id": tarefa },
        });
        writeln!(entrada, "{pedido}").and_then(|()| entrada.flush()).is_ok()
    }

    /// Encerra a entrada: o processo termina depois do turno atual.
    pub fn fechar_entrada(&mut self) {
        self.entrada = None;
        if let Some(canal) = &self.canal { canal.lock().unwrap_or_else(|e| e.into_inner()).fechar(); }
    }

    /// A memória que o agente e tudo o que ele abriu ocupam agora (`Escopo::memoria`).
    pub fn memoria(&self) -> Option<u64> {
        self.escopo.memoria()
    }

    /// A entrada ainda está aberta: dá para dirigir o turno (`dirigir`).
    pub fn entrada_aberta(&self) -> bool {
        self.entrada.is_some() || self.canal.as_ref().is_some_and(|c| c.lock().unwrap_or_else(|e| e.into_inner()).aberta())
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
    ///   novo. Por isso o Parar a tira da fila antes, pelo `uuid` com que ela foi escrita
    ///   (`retirar_dirigidas`; medido no 2.1.328: a resposta do `interrupt` passa a dizer
    ///   `still_queued: []`, o turno fecha com o `result` de erro, o processo sai sozinho menos de
    ///   1 s depois de a entrada fechar e a sessão retomada não tem a mensagem). Num Claude Code
    ///   anterior ao 2.1.323 o Parar ainda mata o processo (`VERSAO_RETIRAR_DIRIGIDA`).
    ///
    /// Nada dela aparece na saída: quem põe a mensagem na conversa é o Vitral.
    pub fn dirigir(&mut self, mensagem: Mensagem) -> bool {
        if let Some(canal) = &self.canal {
            return canal.lock().unwrap_or_else(|e| e.into_inner()).dirigir(mensagem.texto, mensagem.imagens).is_ok();
        }
        let Some(entrada) = self.entrada.as_mut() else {
            return false;
        };
        // Com `uuid`, para o Parar poder tirá-la da fila dele (`retirar_dirigidas`).
        let marca = mensagem.marca.map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_string);
        if escrever_mensagem(entrada, Mensagem { marca: Some(&marca), ..mensagem }).is_err() {
            return false;
        }
        self.dirigidas.push(marca);
        true
    }

    /// Os `uuid` das mensagens dirigidas a este processo, na ordem em que foram.
    #[cfg(test)]
    fn uuids_das_dirigidas(&self) -> &[String] {
        &self.dirigidas
    }

    /// Para o turno: pede pela entrada o `interrupt` do protocolo do Agent SDK (o mesmo do botão de
    /// parar da extensão; conferido no 2.1.278) e fecha ela. O Claude Code responde, fecha o turno
    /// com um `result` de erro, grava a conversa e sai, com tempo de limpar o que abriu. É o mesmo
    /// nos dois sistemas: no Windows, sem console, não há Ctrl+C para mandar. Sem a entrada (ou se a
    /// escrita falhar), o SIGINT no Linux e, no Windows, para tudo. Quem chama confere depois se ele
    /// saiu (`Motor::conferir_parada`) e, com mensagem dirigida esperando, a retira antes
    /// (`retirar_dirigidas`).
    pub fn interromper(&mut self) {
        if let Some(canal) = &self.canal
            && canal.lock().unwrap_or_else(|e| e.into_inner()).interromper().is_ok() { return; }
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
        self.fechar_entrada();
        if !self.parado && self.esperar().is_none() {
            self.parado = true;
            self.escopo.parar();
        }
    }

    /// SIGKILL nele e no que abriu: para o processo que não saiu nem com o SIGTERM do `matar`.
    pub fn matar_de_vez(&mut self) {
        self.fechar_entrada();
        if self.esperar().is_none() {
            self.parado = true;
            self.escopo.matar();
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
    if opcoes.sugestoes {
        args.push("--prompt-suggestions".into());
    }
    if opcoes.eventos_de_hook {
        args.push("--include-hook-events".into());
    }
    if opcoes.raciocinio_resumido {
        args.extend(["--thinking-display", "summarized"].map(String::from));
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
    // Em todos os modos, até no "Sem perguntar": lá o Claude Code só pergunta o que o próprio
    // usuário mandou perguntar (um hook `PreToolUse` com "ask", uma regra `ask` das configurações
    // dele), e sem ter a quem perguntar ele recusa a chamada (medido no 2.1.328, 23/11/2026).
    args.extend(["--permission-prompt-tool", "mcp__vitral__permissao"].map(String::from));
    // Aprovar o plano pode passar para "Sem perguntar" já neste turno, e o seletor de modo
    // também, no meio de qualquer turno (`libera_o_bypass`).
    if libera_o_bypass(&opcoes.permissao, sou_root()) {
        args.push("--allow-dangerously-skip-permissions".into());
    }
    match (opcoes.retomar, &opcoes.bifurcar) {
        (true, _) => args.extend(["--resume", &opcoes.sessao].map(String::from)),
        (false, Some(origem)) => {
            args.extend(["--resume", origem, "--fork-session", "--session-id", &opcoes.sessao].map(String::from));
            if let Some(uuid) = &opcoes.cortar_em {
                args.extend(["--resume-session-at", uuid].map(String::from));
            }
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
    // O Claude Code guarda o antes de cada arquivo que edita, por mensagem: é o que deixa voltar os
    // arquivos a antes de uma mensagem (`voltar_arquivos`).
    por(CHECKPOINTS, "1".into());
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
    // Chamadas ao modelo que o Vitral não mostra em lugar nenhum: o resumo de quando você volta e
    // o título do terminal. Desligadas, não mudam o que o turno responde. A sugestão da próxima
    // mensagem só com a opção ligada (`Opcoes::sugestoes`), que o Vitral mostra na caixa.
    por("CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION", if opcoes.sugestoes { "true" } else { "false" }.into());
    por("CLAUDE_CODE_ENABLE_AWAY_SUMMARY", "0".into());
    por("CLAUDE_CODE_DISABLE_TERMINAL_TITLE", "1".into());
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

/// A versão pelo link que o instalador nativo deixa para `versions/<versão>`: sem abrir processo
/// nenhum, serve para a thread da interface (o turno que começa antes de a leitura de fundo
/// voltar). `None` sem o link (o `claude` do npm, o de mentira da bancada).
pub fn versao_pelo_link(binario: &Path) -> Option<Versao> {
    dunce::canonicalize(binario).ok().and_then(|c| c.file_name().and_then(|n| versao_de(&n.to_string_lossy())))
}

/// A versão do `claude` que o Vitral abre: o instalador nativo deixa um link para
/// `versions/<versão>`; sem ele, pergunta ao próprio `claude --version`.
pub fn versao_instalada() -> Option<Versao> {
    let binario = binario_claude();
    if let Some(versao) = versao_pelo_link(&binario) {
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
    /// Os campos do pedido além do subtipo.
    campos: Value,
    /// A sessão a retomar (`--resume`): o pedido é sobre ela (o `rewind_files`).
    sessao: Option<&'a str>,
    /// A resposta inteira (`response`, com o `subtype` "success" ou "error"), e não só o miolo.
    resposta_crua: bool,
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
        if let Some(sessao) = segundo.sessao {
            comando.args(["--resume", sessao]).env(CHECKPOINTS, "1");
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
    let pedido_de = |subtipo: &str, campos: &Value| {
        let id = format!("vitral-{subtipo}-{}", uuid::Uuid::new_v4());
        let mut corpo = json!({ "subtype": subtipo });
        if let (Some(corpo), Some(campos)) = (corpo.as_object_mut(), campos.as_object()) {
            corpo.extend(campos.clone());
        }
        let pedido = json!({ "type": "control_request", "request_id": id, "request": corpo });
        (id, pedido)
    };
    let (mut esperado, pedido) = pedido_de("initialize", &Value::Null);
    let mut escrito = writeln!(entrada, "{pedido}").and_then(|()| entrada.flush()).is_ok();
    let crua = segundo.as_ref().is_some_and(|s| s.resposta_crua);
    if let Some(segundo) = &segundo {
        let (id, pedido) = pedido_de(segundo.subtipo, &segundo.campos);
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
            let resposta = serde_json::from_str::<Value>(texto.trim()).ok().and_then(|v| {
                if crua {
                    (v["type"] == "control_response" && v["response"]["request_id"] == esperado.as_str()).then(|| v["response"].clone())
                } else {
                    resposta_ao_pedido(&v, &esperado).cloned()
                }
            });
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

/// O que voltar os arquivos a antes de uma mensagem faz (ou fez): os arquivos e as linhas.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Volta {
    pub arquivos: Vec<String>,
    pub entram: u64,
    pub saem: u64,
}

/// Volta os arquivos que o Claude Code mudou desde a mensagem `marca` (o `uuid` com que ela foi
/// mandada) da sessão `sessao`: o `rewind_files` do protocolo, com o checkpoint de arquivos ligado.
/// `seco`: só diz o que mudaria. Abre um processo na sessão sem mandar mensagem nenhuma (não gasta
/// o plano; medido na 2.1.325). Bloqueia até a resposta ou o prazo.
pub fn voltar_arquivos(pasta: &Path, pasta_conta: Option<&Path>, sessao: &str, marca: &str, seco: bool) -> Result<Volta, String> {
    pasta_so_do_dono(pasta).map_err(|e| e.to_string())?;
    let segundo = SegundoPedido {
        subtipo: "rewind_files",
        modelo: "",
        pasta_conta,
        campos: json!({ "user_message_id": marca, "dry_run": seco }),
        sessao: Some(sessao),
        resposta_crua: true,
    };
    let resposta = pedir_ao_claude(pasta, false, Some(segundo)).ok_or("o Claude Code não respondeu")?;
    volta_da_resposta(&resposta)
}

fn volta_da_resposta(resposta: &Value) -> Result<Volta, String> {
    let miolo = &resposta["response"];
    if resposta["subtype"] != "success" || miolo["canRewind"] != true {
        let erro = miolo["error"].as_str().or(resposta["error"].as_str()).unwrap_or("o Claude Code recusou");
        return Err(erro.to_string());
    }
    Ok(Volta {
        arquivos: miolo["filesChanged"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(str::to_string)).collect(),
        entram: miolo["insertions"].as_u64().unwrap_or(0),
        saem: miolo["deletions"].as_u64().unwrap_or(0),
    })
}

/// Liga o checkpoint de arquivos do Claude Code no modo de SDK: sem ela, o `rewind_files` não tem o
/// que voltar.
const CHECKPOINTS: &str = "CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING";

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
    let segundo = SegundoPedido { subtipo: "get_settings", modelo, pasta_conta, campos: Value::Null, sessao: None, resposta_crua: false };
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

fn escrever_mensagem(entrada: &mut processos::EntradaDeFundo, mensagem: Mensagem) -> Result<()> {
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
    let mut linha = json!({
        "type": "user",
        "message": { "role": "user", "content": conteudo },
    });
    if let Some(marca) = mensagem.marca {
        linha["uuid"] = json!(marca);
    }
    writeln!(entrada, "{linha}")?;
    entrada.flush()?;
    Ok(())
}

/// A linha é a primeira palavra do modelo: o começo de um bloco da resposta (texto, raciocínio ou
/// chamada) ou um pedaço dele, com `--include-partial-messages`; sem os pedaços, a mensagem
/// inteira. As linhas dos subagentes não contam.
fn primeira_palavra(valor: &Value) -> bool {
    if !valor["parent_tool_use_id"].is_null() {
        return false;
    }
    match valor["type"].as_str() {
        Some("stream_event") => matches!(valor["event"]["type"].as_str(), Some("content_block_start" | "content_block_delta")),
        Some("assistant") => true,
        _ => false,
    }
}

/// O que a linha diz dos agentes em segundo plano, um por um, além do que o `interpretar` já tira
/// dela (quantos são, e o começo de cada subagente).
fn agentes_do_fundo(valor: &Value) -> Vec<Evento> {
    if valor["type"] != "system" {
        return Vec::new();
    }
    let de_agente = |tipo: &Value| matches!(tipo.as_str(), Some("local_agent" | "local_workflow"));
    let texto = |v: &Value| v.as_str().unwrap_or_default().to_string();
    match valor["subtype"].as_str() {
        // Os mesmos que o `Fundo` conta: os que o Claude Code espera antes de sair.
        Some("background_tasks_changed") => vec![Evento::AgentesNoFundo(
            valor["tasks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|t| t["ambient"] != true && de_agente(&t["task_type"]))
                .filter_map(|t| {
                    Some(TarefaDeFundo { id: t["task_id"].as_str()?.to_string(), descricao: texto(&t["description"]), fluxo: t["task_type"] == "local_workflow" })
                })
                .collect(),
        )],
        // O comando que um subagente roda também tem o `task_started` dele (`local_bash`).
        Some("task_started") if de_agente(&valor["task_type"]) => match (valor["task_id"].as_str(), valor["tool_use_id"].as_str()) {
            (Some(id), Some(pai)) => vec![Evento::AgenteAbriu { id: id.to_string(), pai: pai.to_string() }],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn interpretar(valor: &Value) -> Vec<Evento> {
    // A batida de uma ferramenta que demora (a cada 30 s, com `heartbeat: true`; medido no 2.1.328
    // com um comando de 75 s). Vem presa à chamada pelo `parent_tool_use_id`, como o que um
    // subagente faz, e não é dele: só diz que o processo está vivo.
    if valor["type"] == "tool_progress" {
        return vec![Evento::Vivo];
    }
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
                // O relatório que o subagente entrega a quem o abriu (2.1.271 em diante, no modo
                // automático) não é uma ferramenta dele: é a resposta, e o resultado da chamada
                // `pai` só vai dizer que ela foi entregue (medido na 2.1.328).
                "tool_use" if bloco["name"] == crate::importar::subagente::RELATORIO => bloco["input"]["message"]
                    .as_str()
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| Evento::SubRelatorio { pai: pai.to_string(), texto: t.to_string() }),
                "tool_use" => Some(Evento::SubFerramenta {
                    pai: pai.to_string(),
                    id: bloco["id"].as_str().unwrap_or_default().to_string(),
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
        "system" if valor["subtype"] == "background_tasks_changed" => {
            let tarefas = || valor["tasks"].as_array().into_iter().flatten().filter(|t| t["ambient"] != true);
            vec![
                Evento::Fundo(tarefas().filter(|t| matches!(t["task_type"].as_str(), Some("local_workflow" | "local_agent"))).count()),
                Evento::Comandos(
                    tarefas()
                        .filter(|t| t["task_type"] == "local_bash")
                        .filter_map(|t| Some((t["task_id"].as_str()?.to_string(), texto(&t["description"]))))
                        .collect(),
                ),
            ]
        }
        "system" if valor["subtype"] == "task_started" && valor["tool_use_id"].is_string() => {
            vec![Evento::SubInicio { pai: texto(&valor["tool_use_id"]) }]
        }
        "system" if valor["subtype"] == "task_progress" && valor["tool_use_id"].is_string() => vec![Evento::SubProgresso {
            pai: texto(&valor["tool_use_id"]),
            ferramentas: valor["usage"]["tool_uses"].as_u64().unwrap_or(0) as u32,
            tokens: valor["usage"]["total_tokens"].as_u64().unwrap_or(0),
        }],
        "system" if valor["subtype"] == "task_notification" && valor["tool_use_id"].is_string() => vec![Evento::SubFim {
            pai: texto(&valor["tool_use_id"]),
            erro: valor["status"] != "completed",
            resumo: texto(&valor["summary"]),
            ferramentas: valor["usage"]["tool_uses"].as_u64().unwrap_or(0) as u32,
            tokens: valor["usage"]["total_tokens"].as_u64().unwrap_or(0),
        }],
        "system" if valor["subtype"] == "hook_response" && valor["hook_event"] == "PreToolUse" => {
            hook_que_pediu(valor).into_iter().collect()
        }
        // A compactação (medido no 2.1.328 com um `/compact`): `status: "compacting"` enquanto
        // resume, `status: null` com o `compact_result` quando acaba e, em seguida, o
        // `compact_boundary` com o gatilho e os tokens de antes e de depois. O `status` também sai
        // na troca de modo (só com o `permissionMode`), e aí não diz nada.
        "system" if valor["subtype"] == "status" && valor["status"] == "compacting" => vec![Evento::Compactando(true)],
        "system" if valor["subtype"] == "status" && !valor["compact_result"].is_null() => vec![Evento::Compactando(false)],
        "system" if valor["subtype"] == "compact_boundary" => {
            let dados = &valor["compact_metadata"];
            let mut eventos = vec![Evento::Compactou {
                manual: dados["trigger"] == "manual",
                antes: dados["pre_tokens"].as_u64(),
            }];
            // Quanto a conversa ocupa depois do resumo: o anel do contexto desce na hora.
            eventos.extend(dados["post_tokens"].as_u64().filter(|t| *t > 0).map(Evento::Contexto));
            eventos
        }
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
        "user" => {
            // Desde o 2.1.3xx a chamada Agent devolve na hora "Async agent launched": o subagente
            // segue trabalhando, e não terminou (medido em 16/11/2026).
            let lancado = valor["tool_use_result"]["status"] == "async_launched";
            // O comando pedido em segundo plano, ou o que passou do tempo dele e foi mandado para
            // lá: o resultado traz o `backgroundTaskId` (medido na 2.1.328) e o comando segue.
            let no_fundo = valor["tool_use_result"]["backgroundTaskId"].is_string();
            valor["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|bloco| bloco["type"] == "tool_result")
                .flat_map(|bloco| {
                    let id = texto(&bloco["tool_use_id"]);
                    if lancado {
                        return vec![Evento::SubLancado { pai: id }];
                    }
                    // O texto vira o resultado; as imagens (o print do navegador por MCP, o Read
                    // de um .png) saem num evento à parte, já fora do base64, para o motor
                    // guardar em disco: o base64 não vai para o histórico.
                    let (conteudo, imagens) = crate::imagens_recebidas::partes_do_resultado(&bloco["content"]);
                    let erro = bloco["is_error"].as_bool().unwrap_or(false);
                    let mut eventos = vec![Evento::ResultadoFerramenta { id: id.clone(), conteudo, erro, no_fundo }];
                    if !imagens.is_empty() {
                        eventos.push(Evento::ImagensDoResultado { id, imagens });
                    }
                    eventos
                })
                .collect()
        }
        "result" => crate::motor::tokens_do_resultado(valor)
            .map(Evento::Gasto)
            .into_iter()
            .chain(crate::motor::medida_do_resultado(valor).map(Evento::Medida))
            .chain([Evento::Fim {
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
        }]).collect(),
        "control_response" => resposta_de_controle(valor).into_iter().collect(),
        "prompt_suggestion" => valor["suggestion"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| vec![Evento::Sugestao(s.to_string())])
            .unwrap_or_default(),
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

/// O que a resposta a um pedido de controle do turno muda: o modelo que passou a valer (a do
/// `set_model`, reconhecida pelo id do pedido) e a mensagem dirigida que o `interrupt` deixou na
/// fila (`still_queued`). As outras, e toda recusa, não viram nada: o que foi pedido e não valeu
/// fica para o turno seguinte, sem erro na tela.
fn resposta_de_controle(valor: &Value) -> Option<Evento> {
    let resposta = &valor["response"];
    if resposta["subtype"] != "success" {
        return None;
    }
    let miolo = &resposta["response"];
    if let Some(na_fila) = miolo["still_queued"].as_array().filter(|f| !f.is_empty()) {
        return Some(Evento::FicouNaFila(na_fila.len()));
    }
    let do_modelo = resposta["request_id"].as_str().is_some_and(|id| id.starts_with("vitral-modelo-"));
    miolo["model"].as_str().filter(|m| do_modelo && !m.is_empty()).map(|m| Evento::ModeloEmUso(m.to_string()))
}

/// O `hook_response` de um hook `PreToolUse` que mandou perguntar (`permissionDecision: "ask"` na
/// saída dele): a ferramenta (o `hook_name` é "PreToolUse:<ferramenta>") e o motivo. O evento não
/// traz o id da chamada (2.1.328).
fn hook_que_pediu(valor: &Value) -> Option<Evento> {
    let ferramenta = valor["hook_name"].as_str()?.strip_prefix("PreToolUse:").filter(|f| !f.is_empty())?;
    let saida = valor["stdout"].as_str().or(valor["output"].as_str())?;
    let decisao = serde_json::from_str::<Value>(saida.trim()).ok()?;
    let decisao = &decisao["hookSpecificOutput"];
    (decisao["permissionDecision"] == "ask").then(|| Evento::HookPediu {
        ferramenta: ferramenta.to_string(),
        motivo: decisao["permissionDecisionReason"].as_str().unwrap_or_default().trim().to_string(),
    })
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

/// A versão a partir da qual o Vitral pede os eventos de hook (`--include-hook-events`): a mais
/// antiga em que a opção foi conferida (23/11/2026; o changelog não diz quando ela entrou). Num
/// Claude Code que não a conhecesse, a opção derrubaria todo turno.
pub const VERSAO_EVENTOS_DE_HOOK: Versao = (2, 1, 323);

/// A versão a partir da qual o Parar tira da fila as mensagens dirigidas em vez de matar o
/// processo (`Processo::retirar_dirigidas`): a mais antiga em que o `cancel_async_message` e o
/// `still_queued` da resposta do `interrupt` foram conferidos (23/11/2026; o turno inteiro foi
/// medido no 2.1.328). Num Claude Code que não conhecesse o pedido, a mensagem ficaria na fila e
/// rodaria como turno novo depois do Parar.
pub const VERSAO_RETIRAR_DIRIGIDA: Versao = (2, 1, 323);

/// A versão a partir da qual o Vitral pede o título gerado da sessão (`pedido_do_titulo`): a mais
/// antiga em que o pedido foi conferido (23/11/2026, no código dela; o turno foi medido no
/// 2.1.328). Um Claude Code que não o conhecesse responderia com erro, sem título.
pub const VERSAO_TITULO_GERADO: Versao = (2, 1, 323);

/// A versão a partir da qual o Vitral pede o texto do raciocínio (`--thinking-display
/// summarized`, que não aparece no `--help`): a mais antiga em que a opção foi conferida
/// (23/11/2026). Num Claude Code que não a conhecesse, ela derrubaria todo turno.
pub const VERSAO_RACIOCINIO_RESUMIDO: Versao = (2, 1, 323);

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
    arquivo_da_sessao(pasta_conta, cwd, sessao).is_some()
}

/// O arquivo em que o Claude Code gravou a sessão (uma entrada por linha), se ele existe.
pub fn arquivo_da_sessao(pasta_conta: Option<&std::path::Path>, cwd: &std::path::Path, sessao: &str) -> Option<PathBuf> {
    let config = pasta_de_config(pasta_conta)?;
    [Some(cwd.to_path_buf()), dunce::canonicalize(cwd).ok()]
        .into_iter()
        .flatten()
        .map(|pasta| config.join("projects").join(nome_do_projeto(&pasta.to_string_lossy())).join(format!("{sessao}.jsonl")))
        .find(|arquivo| arquivo.is_file())
}

/// As pastas em que o Claude Code grava as sessões de quem trabalha em `cwd`: a da conta do chat
/// e a normal (as contas extras ligam o mesmo `projects`, mas uma conta montada à mão pode não
/// ligar), com o caminho como veio e com o resolvido. Na bancada, a normal só com um
/// `CLAUDE_CONFIG_DIR` de mentira (`importar::pasta_do_claude`). Só as que existem.
pub fn pastas_do_projeto(pasta_conta: Option<&std::path::Path>, cwd: &std::path::Path) -> Vec<PathBuf> {
    let mut pastas: Vec<PathBuf> = Vec::new();
    for config in [pasta_conta.map(|p| p.to_path_buf()), crate::importar::pasta_do_claude()].into_iter().flatten() {
        for caminho in [Some(cwd.to_path_buf()), dunce::canonicalize(cwd).ok()].into_iter().flatten() {
            let pasta = config.join("projects").join(nome_do_projeto(&caminho.to_string_lossy()));
            if pasta.is_dir() && !pastas.contains(&pasta) {
                pastas.push(pasta);
            }
        }
    }
    pastas
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
        "Edit" | "Write" if campo("diff").is_some() => campo("diff").map(str::to_string),
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
        // O plano inteiro: é o que o cartão da conversa abre depois de respondido. No 2.1.328 a
        // chamada vem sem ele (o motor o guarda quando o pedido de permissão chega).
        "ExitPlanMode" => return (campo("plan").and_then(plano_guardado), None),
        "AskUserQuestion" => Some(perguntas_e_opcoes(entrada)),
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

/// O maior plano guardado inteiro na chamada (os outros detalhes param em `MAX_DETALHE`; um plano
/// de verdade passa disso, e é para ler inteiro).
pub const MAX_PLANO: usize = 60_000;

/// O texto de um plano como fica guardado na chamada: sem as sobras das pontas e até
/// `MAX_PLANO`. `None` para o plano vazio.
pub fn plano_guardado(plano: &str) -> Option<String> {
    let plano = plano.trim();
    if plano.is_empty() {
        return None;
    }
    if plano.len() <= MAX_PLANO {
        return Some(plano.to_string());
    }
    let mut fim = MAX_PLANO;
    while !plano.is_char_boundary(fim) {
        fim -= 1;
    }
    Some(format!("{}\n\n… (cortado)", &plano[..fim]))
}

/// O plano que o resultado da chamada ExitPlanMode aprovada traz, depois de "## Approved Plan:"
/// (ou "## Approved Plan (edited by user):"), como o Claude Code 2.1.328 o escreve. É de onde o
/// plano sai quando a chamada veio sem ele e o pedido de permissão não passou pelo Vitral (a
/// conversa trazida do Claude Code). `None` no plano recusado, que só traz a recusa.
pub fn plano_do_resultado(resultado: &str) -> Option<&str> {
    let (_, depois) = resultado.split_once("## Approved Plan")?;
    let (_, plano) = depois.split_once('\n')?;
    Some(plano.trim()).filter(|p| !p.is_empty())
}

/// As perguntas de uma AskUserQuestion com as opções de cada uma, para ler ao abrir a linha da
/// chamada depois de respondida.
fn perguntas_e_opcoes(entrada: &Value) -> String {
    let texto = |v: &Value| v.as_str().map(str::trim).unwrap_or_default().to_string();
    entrada["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|pergunta| {
            let mut linhas = vec![texto(&pergunta["question"])];
            for opcao in pergunta["options"].as_array().into_iter().flatten() {
                let (rotulo, descricao) = (texto(&opcao["label"]), texto(&opcao["description"]));
                linhas.push(if descricao.is_empty() { format!("· {rotulo}") } else { format!("· {rotulo}: {descricao}") });
            }
            linhas.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Quanto uma edição muda, com o texto inteiro da chamada (rodada de uso, item 5.1): numa troca,
/// as linhas que sobram depois de tirar o começo e o fim iguais; num Write, as linhas gravadas; num
/// diff pronto, as linhas com + e −. Outras ferramentas, nada.
pub fn mudanca_da_ferramenta(nome: &str, entrada: &Value) -> Option<crate::modelo::Mudanca> {
    let campo = |chave: &str| entrada[chave].as_str();
    let (entram, saem) = match nome {
        "Edit" | "Write" if campo("diff").is_some_and(|d| !d.is_empty()) => linhas_do_diff(campo("diff").unwrap_or_default()),
        "Edit" => linhas_da_troca(campo("old_string").unwrap_or_default(), campo("new_string").unwrap_or_default()),
        "MultiEdit" => entrada["edits"].as_array()?.iter().fold((0, 0), |(e, s), t| {
            let (e2, s2) = linhas_da_troca(t["old_string"].as_str().unwrap_or_default(), t["new_string"].as_str().unwrap_or_default());
            (e + e2, s + s2)
        }),
        "Write" => (campo("content")?.lines().count() as u32, 0),
        "NotebookEdit" => (campo("new_source").unwrap_or_default().lines().count() as u32, 0),
        _ => return None,
    };
    Some(crate::modelo::Mudanca { entram, saem, novo: false })
}

/// As linhas que entram e as que saem numa troca, sem o começo e o fim que ficaram iguais.
fn linhas_da_troca(antes: &str, depois: &str) -> (u32, u32) {
    let (a, d): (Vec<&str>, Vec<&str>) = (antes.lines().collect(), depois.lines().collect());
    let comeco = a.iter().zip(&d).take_while(|(x, y)| x == y).count();
    let fim = a[comeco..].iter().rev().zip(d[comeco..].iter().rev()).take_while(|(x, y)| x == y).count();
    ((d.len() - comeco - fim) as u32, (a.len() - comeco - fim) as u32)
}

/// As linhas com + e − de um diff unificado (sem os cabeçalhos +++ e ---).
fn linhas_do_diff(diff: &str) -> (u32, u32) {
    diff.lines().filter(|l| !l.starts_with("+++") && !l.starts_with("---")).fold((0, 0), |(e, s), l| match l.as_bytes().first() {
        Some(b'+') => (e + 1, s),
        Some(b'-') => (e, s + 1),
        _ => (e, s),
    })
}

/// O resultado do Write diz que o arquivo foi criado agora (e não regravado): o selo "novo". No
/// Codex, o resultado de um arquivo novo é "criou <caminho>" (`agente_codex::resumo_das_mudancas`).
pub fn write_criou(resultado: &str) -> bool {
    resultado.to_ascii_lowercase().contains("created successfully") || resultado.starts_with("criou ")
}

/// Quantas linhas iguais ficam antes da mudança quando o começo igual de uma troca longa sai.
const CONTEXTO_DA_TROCA: usize = 3;

/// O que saiu e o que entrou numa troca, cada linha com o seu sinal. Numa troca que passa das
/// linhas guardadas, o começo igual dos dois lados fica de fora (menos 3 linhas de contexto): sem
/// isso, uma palavra trocada na linha 150 de um trecho de 200 ficava depois do corte, e o diff
/// aberto na conversa não mostrava mudança nenhuma.
fn troca(antes: &str, depois: &str) -> String {
    let (antes, depois) = sem_o_comeco_igual(antes, depois);
    match (antes.is_empty(), depois.is_empty()) {
        (true, _) => com_prefixo(depois, "+ "),
        (_, true) => com_prefixo(antes, "- "),
        _ => format!("{}\n{}", com_prefixo(antes, "- "), com_prefixo(depois, "+ ")),
    }
}

/// Os dois lados de uma troca a partir de 3 linhas antes da primeira que difere, quando algum
/// deles passa das linhas guardadas; a troca curta fica como veio.
fn sem_o_comeco_igual<'a>(antes: &'a str, depois: &'a str) -> (&'a str, &'a str) {
    if antes.lines().count() <= LINHAS_DO_DETALHE && depois.lines().count() <= LINHAS_DO_DETALHE {
        return (antes, depois);
    }
    let iguais = antes.lines().zip(depois.lines()).take_while(|(a, d)| a == d).count();
    let pular = iguais.saturating_sub(CONTEXTO_DA_TROCA);
    // Do byte em que começa a linha `pular` em diante.
    let a_partir = |texto: &'a str| &texto[texto.split_inclusive('\n').take(pular).map(str::len).sum::<usize>()..];
    (a_partir(antes), a_partir(depois))
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
        // A primeira pergunta (as outras aparecem ao abrir a linha).
        "AskUserQuestion" => entrada["questions"][0]["question"].as_str().map(str::trim).filter(|p| !p.is_empty()).map(str::to_string),
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

/// O que o visor e o painel do arquivo desenham como imagem: os formatos do anexo e o SVG, que o
/// `img` do GPUI também desenha. O SVG fica fora do `tipo_imagem` porque a API não o aceita como
/// imagem: anexado, ele segue como arquivo de texto.
pub fn imagem_de_ver(caminho: &std::path::Path) -> bool {
    tipo_imagem(caminho).is_some() || caminho.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("svg"))
}

pub(crate) fn base64(bytes: &[u8]) -> String {
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
    #[test]
    fn o_svg_se_ve_como_imagem_mas_nao_vai_como_anexo_de_imagem() {
        use std::path::Path;
        assert!(super::imagem_de_ver(Path::new("assets/logo.SVG")));
        assert!(super::imagem_de_ver(Path::new("print.png")));
        assert!(!super::imagem_de_ver(Path::new("relatorio.pdf")));
        assert_eq!(super::tipo_imagem(Path::new("assets/logo.svg")), None);
    }

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
            cortar_em: None,
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
            sugestoes: false,
            mcp_da_equipe: None,
            eventos_de_hook: false,
            raciocinio_resumido: false,
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
        assert!(!args.iter().any(|a| a == "--resume-session-at"));
        // O chat bifurcado no meio da conversa leva a cópia só até o ponto.
        let cortada = argumentos(&Opcoes { retomar: false, bifurcar: bifurcar.clone(), cortar_em: Some("u1".into()), ..opcoes() });
        let i = cortada.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(&cortada[i..i + 7], ["--resume", "orig", "--fork-session", "--session-id", "s1", "--resume-session-at", "u1"]);
        // Sem a cópia (a sessão do chat novo já nasceu), o corte não vai.
        let depois = argumentos(&Opcoes { bifurcar: bifurcar.clone(), cortar_em: Some("u1".into()), ..opcoes() });
        assert!(!depois.iter().any(|a| a == "--resume-session-at"));
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
        match interpretar(&evento).as_slice() {
            [Evento::Fundo(2), Evento::Comandos(comandos)] => assert_eq!(comandos, &[("c".to_string(), "servidor".to_string())]),
            outro => panic!("{outro:?}"),
        }
        let vazio = json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [] });
        assert!(matches!(interpretar(&vazio).as_slice(), [Evento::Fundo(0), Evento::Comandos(c)] if c.is_empty()));
    }

    /// Os agentes em segundo plano, um por um, como o Claude Code 2.1.328 os mostrou com a CLI de
    /// verdade (Haiku, 23/11/2026): a lista traz o `task_id` e a descrição de cada `local_agent`, e
    /// o `task_started` diz qual chamada abriu cada um.
    #[test]
    fn os_agentes_em_segundo_plano_vem_um_por_um_com_o_id_e_a_descricao() {
        let lista = json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [
            { "task_id": "a21a9faa5262cf9ab", "run_id": "0mwtn4idn-1562673f", "task_type": "local_agent", "subagent_type": "general-purpose", "description": "Contar rápido" },
            { "task_id": "a1d9cb59ea430972c", "run_id": "0mwtn4iqt-6fbbbd5b", "task_type": "local_agent", "subagent_type": "general-purpose", "description": "Contar devagar" },
            { "task_id": "w1", "task_type": "local_workflow", "description": "Revisão em paralelo" },
            { "task_id": "b9", "task_type": "local_bash", "description": "npm run dev" },
            { "task_id": "d", "task_type": "local_agent", "description": "vigia", "ambient": true }
        ], "uuid": "e686b32d", "session_id": "s" });
        match agentes_do_fundo(&lista).as_slice() {
            [Evento::AgentesNoFundo(agentes)] => assert_eq!(
                agentes,
                &[
                    TarefaDeFundo { id: "a21a9faa5262cf9ab".into(), descricao: "Contar rápido".into(), fluxo: false },
                    TarefaDeFundo { id: "a1d9cb59ea430972c".into(), descricao: "Contar devagar".into(), fluxo: false },
                    TarefaDeFundo { id: "w1".into(), descricao: "Revisão em paralelo".into(), fluxo: true },
                ]
            ),
            outro => panic!("{outro:?}"),
        }
        // A lista vazia também chega: é ela que diz que o último terminou.
        let vazia = json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [] });
        assert!(matches!(agentes_do_fundo(&vazia).as_slice(), [Evento::AgentesNoFundo(agentes)] if agentes.is_empty()));
        // O começo de um agente diz a chamada que o abriu.
        let comecou = json!({ "type": "system", "subtype": "task_started", "task_id": "a21a9faa5262cf9ab", "run_id": "0mwtn4idn-1562673f",
            "tool_use_id": "toolu_01NMXF6gw8HjAGm1YojE6g2g", "description": "Contar rápido", "subagent_type": "general-purpose",
            "is_backgrounded": true, "spawn_depth": 1, "task_type": "local_agent", "prompt": "Rode sleep 6" });
        match agentes_do_fundo(&comecou).as_slice() {
            [Evento::AgenteAbriu { id, pai }] => assert_eq!((id.as_str(), pai.as_str()), ("a21a9faa5262cf9ab", "toolu_01NMXF6gw8HjAGm1YojE6g2g")),
            outro => panic!("{outro:?}"),
        }
        // O comando que o próprio subagente roda também tem um `task_started`: não é um agente.
        let comando = json!({ "type": "system", "subtype": "task_started", "task_id": "br3aylza9", "owned_by_subagent": true,
            "tool_use_id": "toolu_01LBsHTNtUz4L2ePxB3pvNqK", "description": "Run sleep 6 then echo um", "is_backgrounded": false,
            "parent_task_id": "a21a9faa5262cf9ab", "task_type": "local_bash" });
        assert!(agentes_do_fundo(&comando).is_empty());
        // E o resto do stream não diz nada dos agentes.
        assert!(agentes_do_fundo(&json!({ "type": "assistant", "message": { "content": [] } })).is_empty());
    }

    #[test]
    fn relatorio_do_subagente_sem_a_moldura() {
        let cru = "  <tools-used-before-summary>Bash</tools-used-before-summary>\n\n[Subagent hand-back] The text below is the final report of a subagent. The report follows:\n  a.txt tem 3 linhas.\n  Conferi com wc.\nagentId: a329c (use SendMessage with to: 'a329c' to continue this agent)";
        assert_eq!(super::relatorio_do_subagente(cru), "a.txt tem 3 linhas.\nConferi com wc.");
        // Sem a moldura, como veio.
        assert_eq!(super::relatorio_do_subagente("Achei 3 arquivos."), "Achei 3 arquivos.");
    }

    #[test]
    fn sugestao_da_proxima_mensagem() {
        // O formato do 2.1.325 (`prompt_suggestion`, com `--prompt-suggestions`).
        let sugestao = json!({ "type": "prompt_suggestion", "suggestion": "  roda os testes\n", "uuid": "u", "session_id": "s" });
        assert!(matches!(interpretar(&sugestao).as_slice(), [Evento::Sugestao(t)] if t == "roda os testes"));
        // Vazia (ele ficou em silêncio) não vira nada.
        let vazia = json!({ "type": "prompt_suggestion", "suggestion": "  " });
        assert!(interpretar(&vazia).is_empty());
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

    /// Rodada do Paseo, item 1: a mudança lá embaixo de uma troca longa tem de caber no que fica
    /// guardado, senão o diff aberto na conversa não tem o que mostrar.
    #[test]
    fn a_troca_longa_guarda_a_mudanca_e_nao_o_comeco_igual() {
        let antes: String = (1..=200).map(|i| format!("linha {i}\n")).collect();
        let depois = antes.replace("linha 150\n", "linha cento e cinquenta\n");
        let (detalhe, _) = detalhe_ferramenta("Edit", &json!({ "file_path": "a.rs", "old_string": antes, "new_string": depois }));
        let detalhe = detalhe.unwrap();
        assert!(detalhe.starts_with("- linha 147\n- linha 148\n- linha 149\n- linha 150\n- linha 151\n"), "{detalhe}");
        assert!(detalhe.contains("\n+ linha 147\n+ linha 148\n+ linha 149\n+ linha cento e cinquenta\n+ linha 151\n"), "{detalhe}");
        // 54 linhas de cada lado, da 147 à 200: nada passou do corte.
        assert!(!detalhe.contains("… mais"), "{detalhe}");
        // A troca curta fica inteira, com o começo igual (é por ele que o painel acha o lugar no arquivo).
        let curta = json!({ "file_path": "a.rs", "old_string": "a\nb\nc\nd\ne\nx", "new_string": "a\nb\nc\nd\ne\ny" });
        assert_eq!(detalhe_ferramenta("Edit", &curta).0.as_deref(), Some("- a\n- b\n- c\n- d\n- e\n- x\n+ a\n+ b\n+ c\n+ d\n+ e\n+ y"));
        // O MultiEdit também, troca por troca.
        let varias = json!({ "file_path": "a.rs", "edits": [{ "old_string": antes, "new_string": depois }, { "old_string": "c", "new_string": "d" }] });
        let detalhe = detalhe_ferramenta("MultiEdit", &varias).0.unwrap();
        assert!(detalhe.starts_with("- linha 147\n") && detalhe.ends_with("\n…\n- c\n+ d"), "{detalhe}");
        // Uma troca longa que muda desde a primeira linha não perde nada do começo.
        let do_topo = antes.replace("linha 1\n", "primeira\n");
        let detalhe = detalhe_ferramenta("Edit", &json!({ "file_path": "a.rs", "old_string": antes, "new_string": do_topo })).0.unwrap();
        assert!(detalhe.starts_with("- linha 1\n- linha 2\n"), "{detalhe}");
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
mod testes_do_ask_de_hook {
    use super::*;

    fn opcoes(modo: &str, url_mcp: &str) -> Opcoes {
        Opcoes {
            agente: crate::modelo::Agente::Claude,
            cwd: PathBuf::from("/tmp/x"),
            sessao: "11111111-1111-4111-8111-111111111111".into(),
            retomar: false,
            bifurcar: None,
            cortar_em: None,
            modelo: None,
            permissao: modo.into(),
            prompt_sistema: "p".into(),
            prompt_arquivo: None,
            ferramentas: None,
            url_mcp: url_mcp.into(),
            bloqueadas: Vec::new(),
            so_mcp_vitral: true,
            esforco: None,
            ultracode: false,
            pasta_conta: None,
            memoria_nativa: false,
            sugestoes: false,
            mcp_da_equipe: None,
            eventos_de_hook: true,
            raciocinio_resumido: true,
        }
    }

    fn valor_de<'a>(args: &'a [String], chave: &str) -> Option<&'a str> {
        let i = args.iter().position(|a| a == chave)?;
        args.get(i + 1).map(String::as_str)
    }

    /// Medido no Claude Code 2.1.328 (23/11/2026): com o "ask" de um hook `PreToolUse`, ele chama
    /// a ferramenta do `--permission-prompt-tool` em todos os modos, até em `bypassPermissions`;
    /// sem ela (era o caso do "Sem perguntar"), a chamada volta recusada com o motivo do hook como
    /// erro. O motivo só sai nos eventos de hook (`--include-hook-events`).
    #[test]
    fn todo_modo_tem_a_quem_perguntar_e_ouve_os_hooks() {
        for modo in ["manual", "acceptEdits", "plan", "auto", "bypassPermissions"] {
            let args = argumentos(&opcoes(modo, "http://127.0.0.1:1/mcp"));
            assert_eq!(valor_de(&args, "--permission-mode"), Some(modo));
            assert_eq!(valor_de(&args, "--permission-prompt-tool"), Some("mcp__vitral__permissao"), "{modo}");
            assert!(args.iter().any(|a| a == "--include-hook-events"), "{modo}");
            // Num Claude Code que não conhece a opção dos eventos, ela não vai (derrubaria o
            // turno), e a ferramenta de permissão continua lá: o cartão aparece, sem o motivo.
            let args = argumentos(&Opcoes { eventos_de_hook: false, ..opcoes(modo, "http://127.0.0.1:1/mcp") });
            assert_eq!(valor_de(&args, "--permission-prompt-tool"), Some("mcp__vitral__permissao"), "{modo}");
            assert!(!args.iter().any(|a| a == "--include-hook-events"), "{modo}");
        }
    }

    /// O turno que começa antes de a leitura de fundo da versão voltar ainda sabe a versão do
    /// instalador nativo, pelo link.
    #[cfg(unix)]
    #[test]
    fn a_versao_sai_do_link_do_instalador_sem_abrir_processo() {
        let pasta = std::env::temp_dir().join(format!("vitral-link-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(pasta.join("versions")).unwrap();
        std::fs::write(pasta.join("versions/2.1.328"), "").unwrap();
        std::os::unix::fs::symlink(pasta.join("versions/2.1.328"), pasta.join("claude")).unwrap();
        assert_eq!(versao_pelo_link(&pasta.join("claude")), Some((2, 1, 328)));
        // Sem o link (o do npm, o de mentira): só o `--version` diz.
        std::fs::write(pasta.join("solto"), "").unwrap();
        assert_eq!(versao_pelo_link(&pasta.join("solto")), None);
        assert_eq!(versao_pelo_link(&pasta.join("nao-existe")), None);
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn os_eventos_de_hook_so_num_claude_code_que_conhece_a_opcao() {
        assert!(versao_de("2.1.328 (Claude Code)").is_some_and(|v| v >= VERSAO_EVENTOS_DE_HOOK));
        assert!(versao_de("2.1.323 (Claude Code)").is_some_and(|v| v >= VERSAO_EVENTOS_DE_HOOK));
        assert!(versao_de("2.1.300 (Claude Code)").is_some_and(|v| v < VERSAO_EVENTOS_DE_HOOK));
    }

    /// O `hook_response` como o 2.1.328 o manda (copiado da saída dele).
    fn resposta_do_hook(nome: &str, saida: &str) -> Value {
        json!({
            "type": "system", "subtype": "hook_response", "hook_id": "0defe090-dfd7-4bd4-b337-b6578af9b068",
            "hook_name": nome, "hook_event": nome.split(':').next().unwrap_or_default(),
            "output": saida, "stdout": saida, "stderr": "", "exit_code": 0, "outcome": "success",
            "uuid": "52e37abc-4230-4400-aed9-c3a9a9b25f9d", "session_id": "c94b6ba9-8047-48c1-b9eb-f6c7e588a68a",
        })
    }

    fn decisao(decisao: &str, motivo: &str) -> String {
        json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": decisao,
                                        "permissionDecisionReason": motivo } })
        .to_string()
            + "\n"
    }

    #[test]
    fn hook_que_manda_perguntar_vira_evento_com_o_motivo() {
        let motivo = "MOTIVO-DO-HOOK: este comando precisa do sim do dono.";
        let eventos = interpretar(&resposta_do_hook("PreToolUse:Bash", &decisao("ask", motivo)));
        match eventos.as_slice() {
            [Evento::HookPediu { ferramenta, motivo: m }] => assert_eq!((ferramenta.as_str(), m.as_str()), ("Bash", motivo)),
            outro => panic!("{outro:?}"),
        }
        // A ferramenta de um servidor MCP vem com o nome inteiro.
        let eventos = interpretar(&resposta_do_hook("PreToolUse:mcp__easypanel__deploy", &decisao("ask", "produção")));
        assert!(
            matches!(eventos.as_slice(), [Evento::HookPediu { ferramenta, motivo }] if ferramenta == "mcp__easypanel__deploy" && motivo == "produção"),
            "{eventos:?}"
        );
        // Sem motivo, o pedido ainda é de um hook.
        let sem = json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "ask" } }).to_string();
        let eventos = interpretar(&resposta_do_hook("PreToolUse:Bash", &sem));
        assert!(matches!(eventos.as_slice(), [Evento::HookPediu { motivo, .. }] if motivo.is_empty()), "{eventos:?}");
        // Só o `stdout` (um Claude Code que não repita a saída em `output`).
        let mut so_stdout = resposta_do_hook("PreToolUse:Bash", &decisao("ask", motivo));
        so_stdout.as_object_mut().unwrap().remove("output");
        assert!(matches!(interpretar(&so_stdout).as_slice(), [Evento::HookPediu { .. }]));
    }

    #[test]
    fn os_outros_eventos_de_hook_nao_viram_nada() {
        let nada = |valor: Value| assert!(interpretar(&valor).is_empty(), "{valor}");
        // Os hooks que não decidem, os que permitem e os que negam (a recusa vem no tool_result).
        nada(resposta_do_hook("PreToolUse:Bash", "{}\n"));
        nada(resposta_do_hook("PreToolUse:Bash", ""));
        nada(resposta_do_hook("PreToolUse:Bash", "texto solto de um hook antigo\n"));
        nada(resposta_do_hook("PreToolUse:Bash", &decisao("allow", "pode")));
        nada(resposta_do_hook("PreToolUse:Bash", &decisao("deny", "não pode")));
        // Os outros eventos: só o PreToolUse pergunta antes da chamada.
        nada(resposta_do_hook("PostToolUse:Bash", &decisao("ask", "tarde demais")));
        nada(resposta_do_hook("SessionStart:startup", &decisao("ask", "x")));
        nada(resposta_do_hook("UserPromptSubmit", "{}\n"));
        nada(json!({ "type": "system", "subtype": "hook_started", "hook_id": "1", "hook_name": "PreToolUse:Bash",
                     "hook_event": "PreToolUse" }));
    }

    /// De ponta a ponta com o claude de mentira da bancada, que encena o que o 2.1.328 faz com o
    /// "ask" de um hook (`[hook ...]`): com os argumentos de um turno em "Sem perguntar", a
    /// chamada passa pela ferramenta de permissão do servidor MCP do Vitral, e o motivo chega
    /// antes dela, pelo evento do hook. Sem a ferramenta nos argumentos (como era), ninguém é
    /// perguntado e a chamada volta recusada.
    #[cfg(unix)]
    #[test]
    fn o_ask_de_hook_chega_a_ferramenta_de_permissao_em_sem_perguntar() {
        use std::sync::mpsc;

        let (chegou, chamadas) = mpsc::channel::<(String, Value)>();
        let servidor = crate::mcp::iniciar(move |chamada: crate::mcp::Chamada| {
            let resposta = json!({ "behavior": "allow", "updatedInput": chamada.argumentos["input"] });
            let _ = chegou.send((chamada.ferramenta.clone(), chamada.argumentos.clone()));
            let _ = chamada.resposta.try_send(crate::mcp::Resposta::ok(resposta.to_string()));
        })
        .unwrap();
        let falso = Path::new(env!("CARGO_MANIFEST_DIR")).join("ferramentas/claude-falso/claude");
        let pasta = std::env::temp_dir().join(format!("vitral-ask-de-hook-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        let motivo = "Deploy no expediente do galpão.";
        let mensagem = json!({ "type": "user", "message": { "role": "user", "content": [
            { "type": "text", "text": format!("[hook Bash echo teste-ask | {motivo}]\nroda o comando") }] } });
        let turno = |args: Vec<String>| -> Vec<Evento> {
            let mut filho = processos::novo_comando("python3")
                .arg("-I")
                .arg(&falso)
                .args(args)
                .current_dir(&pasta)
                .env("XDG_STATE_HOME", &pasta)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("python3 para o claude de mentira");
            // A entrada fecha logo depois da mensagem: é o fim do turno para ele.
            writeln!(filho.stdin.take().unwrap(), "{mensagem}").unwrap();
            let saida = filho.wait_with_output().unwrap();
            String::from_utf8_lossy(&saida.stdout)
                .lines()
                .filter_map(|linha| serde_json::from_str::<Value>(linha).ok())
                .flat_map(|valor| interpretar(&valor))
                .collect()
        };

        let args = argumentos(&opcoes("bypassPermissions", &servidor.url("c1")));
        let eventos = turno(args.clone());
        let posicao = |achar: &dyn Fn(&Evento) -> bool| eventos.iter().position(achar);
        let chamada = posicao(&|e| matches!(e, Evento::Ferramenta { nome, .. } if nome == "Bash")).expect("a chamada");
        let pediu = posicao(&|e| matches!(e, Evento::HookPediu { ferramenta, motivo: m } if ferramenta == "Bash" && m == motivo))
            .unwrap_or_else(|| panic!("sem o evento do hook: {eventos:?}"));
        let resultado = posicao(&|e| matches!(e, Evento::ResultadoFerramenta { erro: false, .. }))
            .unwrap_or_else(|| panic!("a chamada permitida não rodou: {eventos:?}"));
        assert!(chamada < pediu && pediu < resultado, "{eventos:?}");
        // O pedido chegou à ferramenta de permissão, sem o motivo (ele só vem no evento).
        let (ferramenta, argumentos_do_pedido) = chamadas.try_recv().expect("o pedido de permissão");
        assert_eq!(ferramenta, "permissao");
        assert_eq!(argumentos_do_pedido["tool_name"], "Bash");
        assert_eq!(argumentos_do_pedido["input"]["command"], "echo teste-ask");
        assert!(!argumentos_do_pedido.to_string().contains(motivo));
        assert!(chamadas.try_recv().is_err(), "um pedido só");

        // Como era em "Sem perguntar": sem a ferramenta de permissão, a recusa com o motivo.
        let i = args.iter().position(|a| a == "--permission-prompt-tool").expect("a ferramenta de permissão");
        let mut como_era = args;
        como_era.drain(i..i + 2);
        let eventos = turno(como_era);
        assert!(
            eventos.iter().any(|e| matches!(e, Evento::ResultadoFerramenta { erro: true, conteudo, .. } if conteudo == motivo)),
            "{eventos:?}"
        );
        assert!(chamadas.try_recv().is_err(), "ninguém foi perguntado");
        let _ = std::fs::remove_dir_all(&pasta);
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

    /// Item 2: cada chamada do subagente traz o id dela (é por ele que se sabe qual subagente
    /// espera uma permissão), e o relatório que ele entrega pela chamada `SubagentHandback` sobe
    /// como relatório, e não como mais uma ferramenta (medido na 2.1.328, modo automático).
    #[test]
    fn a_chamada_do_subagente_traz_o_id_e_o_relatorio_entregue_nao_e_ferramenta() {
        use super::{Evento, interpretar};
        let chamada = json!({ "type": "assistant", "parent_tool_use_id": "toolu_pai", "agent_id": "a1ef", "message": { "content": [
            { "type": "tool_use", "id": "toolu_ler", "name": "Read", "input": { "file_path": "notas.txt" } }] } });
        match interpretar(&chamada).as_slice() {
            [Evento::SubFerramenta { pai, id, nome, .. }] => assert_eq!((pai.as_str(), id.as_str(), nome.as_str()), ("toolu_pai", "toolu_ler", "Read")),
            outro => panic!("{outro:?}"),
        }
        let entrega = json!({ "type": "assistant", "parent_tool_use_id": "toolu_pai", "agent_id": "a1ef", "message": { "content": [
            { "type": "tool_use", "id": "toolu_entrega", "name": "SubagentHandback", "input": { "message": "A última palavra é banana." } }] } });
        match interpretar(&entrega).as_slice() {
            [Evento::SubRelatorio { pai, texto }] => assert_eq!((pai.as_str(), texto.as_str()), ("toolu_pai", "A última palavra é banana.")),
            outro => panic!("{outro:?}"),
        }
        // A entrega ainda sem o texto (a chamada chegando aos pedaços) não vira nada.
        let vazia = json!({ "type": "assistant", "parent_tool_use_id": "toolu_pai", "message": { "content": [
            { "type": "tool_use", "id": "toolu_entrega", "name": "SubagentHandback", "input": {} }] } });
        assert!(interpretar(&vazia).is_empty());
    }

    /// O subagente em segundo plano, como o Claude Code 2.1.322 o mostrou (16/11/2026): a chamada
    /// volta na hora com "Async agent launched", o progresso conta as ferramentas e o fim vem
    /// num aviso à parte, com o resumo.
    #[test]
    fn subagente_em_segundo_plano_so_termina_no_aviso_de_fim() {
        use super::{Evento, interpretar};
        let lancado = json!({ "type": "user", "message": { "content": [{ "tool_use_id": "toolu_pai", "type": "tool_result",
            "content": [{ "type": "text", "text": "Async agent launched successfully." }] }] },
            "tool_use_result": { "isAsync": true, "status": "async_launched", "agentId": "a5b" } });
        assert!(matches!(interpretar(&lancado).as_slice(), [Evento::SubLancado { pai }] if pai == "toolu_pai"));
        let progresso = json!({ "type": "system", "subtype": "task_progress", "task_id": "a5b", "tool_use_id": "toolu_pai",
            "description": "Reading um.txt", "usage": { "total_tokens": 57402, "tool_uses": 2, "duration_ms": 6893 } });
        assert!(matches!(interpretar(&progresso).as_slice(), [Evento::SubProgresso { pai, ferramentas: 2, tokens: 57402 }] if pai == "toolu_pai"));
        let fim = json!({ "type": "system", "subtype": "task_notification", "task_id": "a5b", "tool_use_id": "toolu_pai",
            "status": "completed", "summary": "Encontrei três arquivos.",
            "usage": { "total_tokens": 58704, "tool_uses": 3, "duration_ms": 9438 } });
        assert!(matches!(interpretar(&fim).as_slice(),
            [Evento::SubFim { pai, erro: false, resumo, ferramentas: 3, tokens: 58704 }] if pai == "toolu_pai" && resumo == "Encontrei três arquivos."));
        // O começo (2.1.325): o subagente já conta antes da primeira ferramenta.
        let comecou = json!({ "type": "system", "subtype": "task_started", "task_id": "a5b", "tool_use_id": "toolu_pai",
            "description": "Contar linhas de a.txt" });
        assert!(matches!(interpretar(&comecou).as_slice(), [Evento::SubInicio { pai }] if pai == "toolu_pai"));
        let falhou = json!({ "type": "system", "subtype": "task_notification", "tool_use_id": "toolu_pai", "status": "failed" });
        assert!(matches!(interpretar(&falhou).as_slice(), [Evento::SubFim { erro: true, .. }]));
    }

    /// O `result` e os resultados de comando como o Claude Code 2.1.328 os mostrou (23/11/2026).
    #[test]
    fn o_result_traz_os_tempos_e_o_comando_diz_se_foi_para_o_segundo_plano() {
        use super::{Evento, interpretar};
        let fim = json!({ "type": "result", "subtype": "success", "is_error": false, "result": "feito",
            "duration_ms": 7004, "duration_api_ms": 2699, "ttft_ms": 1421, "num_turns": 2, "total_cost_usd": 0.01,
            "usage": { "input_tokens": 4, "output_tokens": 72, "cache_read_input_tokens": 31050, "cache_creation_input_tokens": 7470,
                "cache_creation": { "ephemeral_1h_input_tokens": 7470, "ephemeral_5m_input_tokens": 0 } } });
        match interpretar(&fim).as_slice() {
            [Evento::Gasto(tokens), Evento::Medida(medida), Evento::Fim { erro: false, texto, .. }] => {
                assert_eq!(tokens.cache_lido, 31050);
                assert_eq!((medida.duracao_ms, medida.modelo_acumulado_ms, medida.ttft_ms, medida.passos, medida.cache_ttl), (7004, 2699, 1421, 2, 3600));
                assert_eq!(texto, "feito");
            }
            outro => panic!("{outro:?}"),
        }
        // O `result` sem tempo nenhum (o de uma versão velha) segue só com o fim.
        let velho = json!({ "type": "result", "is_error": false, "result": "ok", "total_cost_usd": 0.0 });
        assert!(matches!(interpretar(&velho).as_slice(), [Evento::Fim { .. }]));
        // O comando que terminou.
        let terminou = json!({ "type": "user", "message": { "content": [{ "tool_use_id": "toolu_a", "type": "tool_result", "content": "lento" }] },
            "tool_use_result": { "stdout": "lento", "stderr": "", "interrupted": false } });
        assert!(matches!(interpretar(&terminou).as_slice(), [Evento::ResultadoFerramenta { id, no_fundo: false, .. }] if id == "toolu_a"));
        // O que foi pedido em segundo plano volta na hora, e o que passou do tempo volta quando
        // passa: nos dois o `backgroundTaskId` diz que o comando segue rodando.
        let pedido = json!({ "type": "user", "message": { "content": [{ "tool_use_id": "toolu_b", "type": "tool_result",
            "content": "Command running in background with ID: bdotu4uvf." }] },
            "tool_use_result": { "stdout": "", "stderr": "", "interrupted": false, "backgroundTaskId": "bdotu4uvf" } });
        assert!(matches!(interpretar(&pedido).as_slice(), [Evento::ResultadoFerramenta { id, no_fundo: true, .. }] if id == "toolu_b"));
        let estourou = json!({ "type": "user", "message": { "content": [{ "tool_use_id": "toolu_c", "type": "tool_result",
            "content": "Command did not complete within its 3s timeout and was moved to the background (ID: bakp5fylo)." }] },
            "tool_use_result": { "stdout": "", "stderr": "", "interrupted": false, "backgroundTaskId": "bakp5fylo", "timedOutAfterMs": 3000 } });
        assert!(matches!(interpretar(&estourou).as_slice(), [Evento::ResultadoFerramenta { id, no_fundo: true, .. }] if id == "toolu_c"));
        // O resultado de erro vem com o `tool_use_result` em texto, não em objeto.
        let erro = json!({ "type": "user", "message": { "content": [{ "tool_use_id": "toolu_d", "type": "tool_result", "content": "x", "is_error": true }] },
            "tool_use_result": "Error: Blocked" });
        assert!(matches!(interpretar(&erro).as_slice(), [Evento::ResultadoFerramenta { erro: true, no_fundo: false, .. }]));
    }

    #[test]
    fn a_primeira_palavra_e_o_primeiro_bloco_da_resposta() {
        use super::primeira_palavra;
        // Com os pedaços: o começo de um bloco (até o de uma chamada, que não vira evento) e o pedaço.
        assert!(primeira_palavra(&json!({ "type": "stream_event", "event": { "type": "content_block_start", "content_block": { "type": "tool_use" } } })));
        assert!(primeira_palavra(&json!({ "type": "stream_event", "event": { "type": "content_block_start", "content_block": { "type": "thinking" } } })));
        assert!(primeira_palavra(&json!({ "type": "stream_event", "event": { "type": "content_block_delta", "delta": { "type": "text_delta", "text": "O" } } })));
        // Sem eles (o claude falso), a mensagem inteira.
        assert!(primeira_palavra(&json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "oi" }] } })));
        // O que vem antes da palavra não conta, nem o que é de um subagente.
        assert!(!primeira_palavra(&json!({ "type": "system", "subtype": "init", "model": "claude-opus-5-5" })));
        assert!(!primeira_palavra(&json!({ "type": "stream_event", "event": { "type": "message_start" } })));
        assert!(!primeira_palavra(&json!({ "type": "rate_limit_event", "rate_limit_info": {} })));
        assert!(!primeira_palavra(&json!({ "type": "assistant", "parent_tool_use_id": "toolu_pai", "message": { "content": [{ "type": "text", "text": "oi" }] } })));
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

#[cfg(test)]
mod testes_da_mudanca {
    use super::{mudanca_da_ferramenta, write_criou};
    use serde_json::json;

    #[test]
    fn mudanca_contada_com_o_texto_inteiro() {
        // Na troca, o começo e o fim iguais não contam.
        let edit = json!({"file_path": "a.rs", "old_string": "fn a() {\n    1\n}\n", "new_string": "fn a() {\n    2\n    3\n}\n"});
        let m = mudanca_da_ferramenta("Edit", &edit).unwrap();
        assert_eq!((m.entram, m.saem, m.novo), (2, 1, false));
        let multi = json!({"edits": [{"old_string": "x", "new_string": "y"}, {"old_string": "", "new_string": "z\nw"}]});
        let m = mudanca_da_ferramenta("MultiEdit", &multi).unwrap();
        assert_eq!((m.entram, m.saem), (3, 1));
        // O Write conta tudo o que gravou, mesmo acima do corte do detalhe.
        let longo: String = (0..500).map(|i| format!("linha {i}\n")).collect();
        let m = mudanca_da_ferramenta("Write", &json!({"file_path": "b.md", "content": longo})).unwrap();
        assert_eq!((m.entram, m.saem), (500, 0));
        assert_eq!(mudanca_da_ferramenta("Bash", &json!({"command": "ls"})), None);
        let diff = json!({"diff": "--- a\n+++ b\n@@\n-x\n+y\n+z\n"});
        let m = mudanca_da_ferramenta("Edit", &diff).unwrap();
        assert_eq!((m.entram, m.saem), (2, 1));
        assert!(write_criou("File created successfully at: /x/b.md"));
        assert!(!write_criou("The file /x/b.md has been updated successfully."));
    }
}

/// O turno que roda recebe pela entrada o que antes só valia no seguinte (o modo, o modelo e o
/// esforço); o Parar tira da fila a mensagem dirigida em vez de matar o processo; a batida de uma
/// ferramenta que demora conta como sinal de vida; e o texto do raciocínio vem na saída. Os
/// pedidos e as respostas são os medidos no Claude Code 2.1.328 em 23/11/2026, e o claude de
/// mentira da bancada os encena.
#[cfg(test)]
mod testes_do_turno_ao_vivo {
    use super::*;
    #[cfg(unix)]
    use std::sync::mpsc;

    fn ao_vivo(modo: &str, modelo: &str, esforco: Option<&str>) -> AoVivo {
        AoVivo { modo: modo.into(), modelo: modelo.into(), esforco: esforco.map(str::to_string) }
    }

    fn opcoes(pasta: &Path) -> Opcoes {
        Opcoes {
            agente: crate::modelo::Agente::Claude,
            cwd: pasta.to_path_buf(),
            sessao: "11111111-1111-4111-8111-111111111111".into(),
            retomar: false,
            bifurcar: None,
            cortar_em: None,
            modelo: Some("haiku".into()),
            permissao: "manual".into(),
            prompt_sistema: "p".into(),
            prompt_arquivo: None,
            ferramentas: None,
            // Sem "/mcp/" no endereço: o claude de mentira não chama servidor nenhum.
            url_mcp: "http://127.0.0.1:1/nada".into(),
            bloqueadas: Vec::new(),
            so_mcp_vitral: true,
            esforco: None,
            ultracode: false,
            pasta_conta: None,
            memoria_nativa: false,
            sugestoes: false,
            mcp_da_equipe: None,
            eventos_de_hook: true,
            raciocinio_resumido: true,
        }
    }

    fn valor_de<'a>(args: &'a [String], chave: &str) -> Option<&'a str> {
        let i = args.iter().position(|a| a == chave)?;
        args.get(i + 1).map(String::as_str)
    }

    #[test]
    fn cada_troca_vira_o_pedido_de_controle_dela() {
        let tem = ao_vivo("manual", "opus", Some("high"));
        assert!(pedidos_ao_vivo(&tem, &tem).is_empty(), "sem troca, nenhum pedido");
        // O modo, com o nome que o Claude Code dá a cada um.
        for modo in ["acceptEdits", "plan", "auto", "bypassPermissions"] {
            assert_eq!(
                pedidos_ao_vivo(&tem, &AoVivo { modo: modo.into(), ..tem.clone() }),
                [json!({ "subtype": "set_permission_mode", "mode": modo })]
            );
        }
        // O Manual do Vitral é o `default` do pedido (o 2.1.328 aceita `manual` e responde
        // `default`; a lista de modos do erro dele só tem `default`).
        let em_edicoes = ao_vivo("acceptEdits", "opus", Some("high"));
        assert_eq!(pedidos_ao_vivo(&em_edicoes, &tem), [json!({ "subtype": "set_permission_mode", "mode": "default" })]);
        // O modelo; vazio é o padrão do Claude Code, que o pedido sem `model` devolve.
        assert_eq!(
            pedidos_ao_vivo(&tem, &AoVivo { modelo: "claude-haiku-4-5-20251001".into(), ..tem.clone() }),
            [json!({ "subtype": "set_model", "model": "claude-haiku-4-5-20251001" })]
        );
        assert_eq!(pedidos_ao_vivo(&tem, &AoVivo { modelo: String::new(), ..tem.clone() }), [json!({ "subtype": "set_model" })]);
        // O esforço; o automático é o `null`.
        assert_eq!(
            pedidos_ao_vivo(&tem, &AoVivo { esforco: Some("low".into()), ..tem.clone() }),
            [json!({ "subtype": "apply_flag_settings", "settings": { "effortLevel": "low" } })]
        );
        assert_eq!(
            pedidos_ao_vivo(&tem, &AoVivo { esforco: None, ..tem.clone() }),
            [json!({ "subtype": "apply_flag_settings", "settings": { "effortLevel": null } })]
        );
        // Tudo de uma vez: um pedido por troca.
        let subtipos: Vec<String> = pedidos_ao_vivo(&tem, &ao_vivo("plan", "haiku", None))
            .iter()
            .map(|p| p["subtype"].as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(subtipos, ["set_permission_mode", "set_model", "apply_flag_settings"]);
    }

    /// O Ultracode entra pelos argumentos do processo (os fluxos de agentes, o `--settings`): com
    /// ele de um lado ou do outro, o esforço fica para o turno seguinte. O resto troca igual.
    #[test]
    fn o_esforco_do_ultracode_nao_troca_no_meio() {
        let no_ultracode = ao_vivo("manual", "opus", Some("ultracode"));
        assert!(pedidos_ao_vivo(&no_ultracode, &ao_vivo("manual", "opus", Some("high"))).is_empty());
        assert!(pedidos_ao_vivo(&no_ultracode, &ao_vivo("manual", "opus", None)).is_empty());
        assert!(pedidos_ao_vivo(&ao_vivo("manual", "opus", Some("high")), &no_ultracode).is_empty());
        assert_eq!(
            pedidos_ao_vivo(&no_ultracode, &ao_vivo("plan", "opus", Some("high"))),
            [json!({ "subtype": "set_permission_mode", "mode": "plan" })]
        );
        // O processo que abre com Ultracode já nasce marcado, e o comum com o esforço dele.
        let pasta = PathBuf::from("/tmp/x");
        assert_eq!(AoVivo::das_opcoes(&Opcoes { ultracode: true, esforco: Some("max".into()), ..opcoes(&pasta) }).esforco.as_deref(), Some("ultracode"));
        assert_eq!(AoVivo::das_opcoes(&Opcoes { esforco: Some("max".into()), ..opcoes(&pasta) }), ao_vivo("manual", "haiku", Some("max")));
        assert_eq!(AoVivo::das_opcoes(&Opcoes { esforco: Some(String::new()), modelo: None, ..opcoes(&pasta) }), ao_vivo("manual", "", None));
    }

    #[test]
    fn o_pedido_vai_como_control_request_com_o_id_do_vitral() {
        let corpo = json!({ "subtype": "set_permission_mode", "mode": "plan" });
        let linha = linha_do_pedido(nome_do_pedido(&corpo), corpo.clone());
        assert_eq!(linha["type"], "control_request");
        assert_eq!(linha["request"], corpo);
        assert!(linha["request_id"].as_str().is_some_and(|id| id.starts_with("vitral-modo-")), "{linha}");
        assert_eq!(nome_do_pedido(&json!({ "subtype": "set_model" })), "modelo");
        assert_eq!(nome_do_pedido(&json!({ "subtype": "apply_flag_settings" })), "esforco");
        // O que tira da fila a mensagem dirigida, pelo uuid com que ela foi escrita.
        let retirar = pedido_de_retirar("8b82d03c-d4b3-4238-9531-17dc4a1df43e");
        assert_eq!(retirar, json!({ "subtype": "cancel_async_message", "message_uuid": "8b82d03c-d4b3-4238-9531-17dc4a1df43e" }));
        assert_eq!(nome_do_pedido(&retirar), "retirar");
    }

    /// O pedido do título da sessão, como medido no 2.1.328 em 23/11/2026: a primeira fala na
    /// `description` e o `persist`, que manda gravar a linha `ai-title` na sessão. A resposta (o
    /// título, ou `null` quando ele não soube titular) não vira evento: o motor lê o título do
    /// arquivo da sessão, depois do turno.
    #[test]
    fn o_pedido_do_titulo_leva_a_primeira_fala_e_manda_gravar() {
        let corpo = pedido_do_titulo("olha esse erro aqui que deu no teste");
        assert_eq!(
            corpo,
            json!({ "subtype": "generate_session_title", "description": "olha esse erro aqui que deu no teste", "persist": true })
        );
        let linha = linha_do_pedido(nome_do_pedido(&corpo), corpo.clone());
        assert_eq!(linha["type"], "control_request");
        assert_eq!(linha["request"], corpo);
        assert!(linha["request_id"].as_str().is_some_and(|id| id.starts_with("vitral-titulo-")), "{linha}");
        for miolo in [json!({ "title": "Erro na função soma" }), json!({ "title": null })] {
            let resposta = json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "vitral-titulo-1", "response": miolo } });
            assert!(interpretar(&resposta).is_empty(), "{resposta}");
        }
        // O pedido existe igual desde a 2.1.323 (o claude de mentira da bancada diz 2.1.300 por padrão).
        assert!(versao_de("2.1.328 (Claude Code)").is_some_and(|v| v >= VERSAO_TITULO_GERADO));
        assert!(versao_de("2.1.323 (Claude Code)").is_some_and(|v| v >= VERSAO_TITULO_GERADO));
        assert!(versao_de("2.1.300 (Claude Code)").is_some_and(|v| v < VERSAO_TITULO_GERADO));
    }

    /// De ponta a ponta com o claude de mentira, que encena o pedido como o 2.1.328: o título pedido
    /// no meio do turno fica gravado na sessão (só numa pasta de configuração de mentira), e é de
    /// lá que o motor o lê.
    #[cfg(unix)]
    #[test]
    fn o_titulo_pedido_no_turno_fica_gravado_na_sessao() {
        let config = std::env::temp_dir().join(format!("vitral-titulo-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&config).unwrap();
        let ambiente = [("CLAUDE_CONFIG_DIR", config.to_str().unwrap()), ("CLAUDE_FALSO_TITULO", "Erro na função soma")];
        let mut turno = Turno::abrir("[ferramenta Bash cargo test | 1]\nolha esse erro aqui que deu no teste", |o| o, &ambiente);
        assert!(turno.processo.pedir_titulo("olha esse erro aqui que deu no teste"));
        turno.ate("o fim do turno", |e| matches!(e, Evento::Fim { .. }));
        let sessao = "11111111-1111-4111-8111-111111111111";
        let arquivo = arquivo_da_sessao(Some(&config), &turno.pasta, sessao).expect("a sessão gravada pelo claude de mentira");
        assert_eq!(crate::importar::titulos_da_sessao(&arquivo).gerado.as_deref(), Some("Erro na função soma"));
        // Com a entrada fechada (o turno acabou) não há a quem pedir.
        turno.processo.fechar_entrada();
        assert!(!turno.processo.pedir_titulo("olha esse erro aqui que deu no teste"));
        drop(turno);
        let _ = std::fs::remove_dir_all(&config);
    }

    /// Medido no 2.1.328: aberto sem a `--allow-dangerously-skip-permissions`, o processo recusa
    /// o `set_permission_mode` para `bypassPermissions` ("bypass_not_launched"); com ela, entra.
    /// Como root ele nem abre com a opção, então ela só vai onde já ia (o Planejar).
    #[test]
    fn o_sem_perguntar_pode_entrar_no_meio_de_qualquer_turno() {
        for modo in ["manual", "acceptEdits", "plan", "auto", "bypassPermissions"] {
            assert!(libera_o_bypass(modo, false), "{modo}");
            assert_eq!(libera_o_bypass(modo, true), modo == "plan", "{modo} como root");
            let args = argumentos(&Opcoes { permissao: modo.into(), ..opcoes(Path::new("/tmp/x")) });
            assert_eq!(args.iter().any(|a| a == "--allow-dangerously-skip-permissions"), libera_o_bypass(modo, sou_root()), "{modo}");
        }
    }

    /// Medido no 2.1.328: sem a opção, o bloco de raciocínio chega com o texto vazio; com
    /// `--thinking-display summarized` (fora do `--help`), chega com ele. A 2.1.323 aceita a opção.
    #[test]
    fn o_texto_do_raciocinio_e_pedido_a_quem_conhece_a_opcao() {
        let pasta = Path::new("/tmp/x");
        let args = argumentos(&opcoes(pasta));
        assert_eq!(valor_de(&args, "--thinking-display"), Some("summarized"));
        let args = argumentos(&Opcoes { raciocinio_resumido: false, ..opcoes(pasta) });
        assert!(!args.iter().any(|a| a == "--thinking-display"));
        assert!(versao_de("2.1.328 (Claude Code)").is_some_and(|v| v >= VERSAO_RACIOCINIO_RESUMIDO));
        assert!(versao_de("2.1.323 (Claude Code)").is_some_and(|v| v >= VERSAO_RACIOCINIO_RESUMIDO));
        assert!(versao_de("2.1.300 (Claude Code)").is_some_and(|v| v < VERSAO_RACIOCINIO_RESUMIDO));
        assert!(versao_de("2.1.323 (Claude Code)").is_some_and(|v| v >= VERSAO_RETIRAR_DIRIGIDA));
        assert!(versao_de("2.1.300 (Claude Code)").is_some_and(|v| v < VERSAO_RETIRAR_DIRIGIDA));
        // O bloco como o 2.1.328 o manda, com a opção e sem ela.
        let bloco = |texto: &str| {
            json!({ "type": "assistant", "parent_tool_use_id": null, "message": { "model": "claude-haiku-5-5", "content": [
                { "type": "thinking", "thinking": texto, "signature": "CAQS3AYKEAgUGAI4AUIIdGhpbmtpbmc" }] } })
        };
        let com = "I'm working out the timing: train A arrives 16:55 and train B arrives 16:45.";
        assert!(matches!(interpretar(&bloco(com)).as_slice(), [Evento::Pensamento(t)] if t == com));
        assert!(matches!(interpretar(&bloco("")).as_slice(), [Evento::Pensamento(t)] if t.is_empty()));
    }

    /// A linha é a que o 2.1.328 mandou aos 30 e aos 60 s de um comando de 75 s. Ela vem presa à
    /// chamada (`parent_tool_use_id`), como os passos de um subagente, e não é um.
    #[test]
    fn a_batida_da_ferramenta_e_sinal_de_vida() {
        let batida = json!({
            "type": "tool_progress", "tool_use_id": "toolu_01FjUGWS8amto8qs5E2ZFmEn-heartbeat-0", "tool_name": "Bash",
            "parent_tool_use_id": "toolu_01FjUGWS8amto8qs5E2ZFmEn", "elapsed_time_seconds": 30, "heartbeat": true,
            "session_id": "a5102f2d-119f-4862-9d1a-ba981ad4fe47", "uuid": "382ed80b-e3a3-4bca-b4d3-247416d53245",
        });
        assert!(matches!(interpretar(&batida).as_slice(), [Evento::Vivo]), "{:?}", interpretar(&batida));
    }

    /// As respostas aos pedidos de controle e os avisos que os acompanham, copiados da saída do
    /// 2.1.328. Uma recusa não vira nada: a troca vale no turno seguinte, sem erro na tela.
    #[test]
    fn as_respostas_de_controle_nao_viram_erro_na_tela() {
        let resposta = |id: &str, corpo: Value| {
            let mut r = json!({ "subtype": "success", "request_id": id });
            if !corpo.is_null() {
                r["response"] = corpo;
            }
            json!({ "type": "control_response", "response": r })
        };
        let recusa = |id: &str, erro: &str, codigo: &str| {
            json!({ "type": "control_response", "response": { "subtype": "error", "request_id": id, "error": erro, "error_code": codigo } })
        };
        let nada = |valor: Value| assert!(interpretar(&valor).is_empty(), "{valor} virou {:?}", interpretar(&valor));
        nada(resposta("vitral-modo-1", json!({ "mode": "acceptEdits" })));
        nada(json!({ "type": "system", "subtype": "status", "status": null, "permissionMode": "acceptEdits",
                     "uuid": "6e4618d7-111d-4d3e-b049-2172d5ef3f9e", "session_id": "b8610580-4229-4347-b29e-9e1b7eac4bca" }));
        nada(recusa(
            "vitral-modo-2",
            "Cannot set permission mode to bypassPermissions because the session was not launched with --dangerously-skip-permissions",
            "bypass_not_launched",
        ));
        nada(recusa("vitral-modo-3", "Cannot set permission mode: must be one of acceptEdits, auto, bypassPermissions, default, dontAsk, plan", "invalid_mode"));
        nada(json!({ "type": "control_response", "response": { "subtype": "error", "request_id": "vitral-modelo-1",
                     "error": "Model 'bananinha-9' not found", "error_code": "not_found", "model": "claude-haiku-5-5" } }));
        nada(resposta("vitral-esforco-1", Value::Null));
        nada(resposta("vitral-retirar-1", json!({ "cancelled": true, "message_uuid": "8b82d03c-d4b3-4238-9531-17dc4a1df43e" })));
        nada(resposta("vitral-retirar-2", json!({ "cancelled": false })));
        nada(json!({ "type": "command_lifecycle", "command_uuid": "8b82d03c-d4b3-4238-9531-17dc4a1df43e", "state": "cancelled",
                     "uuid": "7a6c96cf-b3ca-462e-9cb4-839d723d8da2", "session_id": "13c823ee-17a5-48fd-b566-9fae508dd5d5" }));
        // O aviso que o `set_model` deixa na conversa do Claude Code não é mensagem de ninguém.
        nada(json!({ "type": "user", "message": { "role": "user", "content": "<local-command-stdout>Set model to `claude-haiku-4-5-20251001`</local-command-stdout>" },
                     "session_id": "b8610580-4229-4347-b29e-9e1b7eac4bca", "parent_tool_use_id": null, "isReplay": true }));
        // O Parar que não deixou nada na fila.
        nada(resposta("vitral-parar-1", json!({ "still_queued": [] })));
        nada(resposta("vitral-parar-tarefa-1", json!({})));

        // O que o motor precisa saber: o modelo que passou a valer, e a mensagem que o Parar não
        // tirou da fila (o Claude Code abriria outro turno com ela).
        let trocou = interpretar(&resposta("vitral-modelo-2", json!({ "model": "claude-haiku-4-5-20251001" })));
        assert!(matches!(trocou.as_slice(), [Evento::ModeloEmUso(m)] if m == "claude-haiku-4-5-20251001"), "{trocou:?}");
        let ficou = interpretar(&resposta("vitral-parar-2", json!({ "still_queued": ["8b82d03c-d4b3-4238-9531-17dc4a1df43e"] })));
        assert!(matches!(ficou.as_slice(), [Evento::FicouNaFila(1)]), "{ficou:?}");
    }

    /// Um turno no claude de mentira, aberto pelo mesmo caminho dos turnos de verdade
    /// (`Processo::com_o_comando`), com os argumentos de um turno em Manual.
    #[cfg(unix)]
    struct Turno {
        processo: Processo,
        eventos: mpsc::Receiver<Evento>,
        vistos: Vec<Evento>,
        pasta: PathBuf,
    }

    #[cfg(unix)]
    impl Turno {
        fn abrir(texto: &str, opcoes: impl FnOnce(Opcoes) -> Opcoes, ambiente: &[(&str, &str)]) -> Self {
            Self::abrir_marcado(texto, None, opcoes, ambiente)
        }

        /// Como o motor abre: a mensagem vai com o `uuid` dela (a `marca`).
        fn abrir_marcado(texto: &str, marca: Option<&str>, opcoes: impl FnOnce(Opcoes) -> Opcoes, ambiente: &[(&str, &str)]) -> Self {
            let falso = Path::new(env!("CARGO_MANIFEST_DIR")).join("ferramentas/claude-falso/claude");
            let pasta = std::env::temp_dir().join(format!("vitral-ao-vivo-{}", crate::modelo::novo_id()));
            std::fs::create_dir_all(&pasta).unwrap();
            let opcoes = opcoes(self::opcoes(&pasta));
            let mut comando = processos::comando_leve("python3", "teste");
            comando.arg("-I").arg(&falso).args(argumentos(&opcoes)).current_dir(&pasta).env("XDG_STATE_HOME", &pasta);
            for (chave, valor) in ambiente {
                comando.env(chave, valor);
            }
            let (manda, eventos) = mpsc::channel();
            let mensagem = Mensagem { texto, imagens: &[], marca };
            let processo = Processo::com_o_comando(comando, &opcoes, mensagem, move |evento| {
                let _ = manda.send(evento);
            })
            .expect("python3 para o claude de mentira");
            Self { processo, eventos, vistos: Vec::new(), pasta }
        }

        /// Lê a saída até o evento que casa (ele fica por último em `vistos`).
        fn ate(&mut self, o_que: &str, casa: impl Fn(&Evento) -> bool) {
            let limite = Instant::now() + Duration::from_secs(30);
            loop {
                let Ok(evento) = self.eventos.recv_timeout(limite.saturating_duration_since(Instant::now())) else {
                    panic!("não chegou {o_que}; chegou: {:?}", self.vistos);
                };
                let achou = casa(&evento);
                let saiu = matches!(evento, Evento::Saiu { .. });
                self.vistos.push(evento);
                if achou {
                    return;
                }
                assert!(!saiu, "o processo saiu antes de {o_que}: {:?}", self.vistos);
            }
        }

        /// Lê até o processo sair e devolve o código de saída dele (-1 se morreu por sinal).
        fn ate_sair(&mut self) -> Option<i32> {
            self.ate("a saída do processo", |e| matches!(e, Evento::Saiu { .. }));
            match self.vistos.last() {
                Some(Evento::Saiu { codigo, .. }) => *codigo,
                _ => None,
            }
        }

        fn quantos(&self, casa: impl Fn(&Evento) -> bool) -> usize {
            self.vistos.iter().filter(|e| casa(e)).count()
        }

        fn disse(&self, trecho: &str) -> bool {
            self.vistos.iter().any(|e| matches!(e, Evento::Texto(t) if t.contains(trecho)))
        }
    }

    #[cfg(unix)]
    impl Drop for Turno {
        fn drop(&mut self) {
            self.processo.matar();
            let _ = std::fs::remove_dir_all(&self.pasta);
        }
    }

    /// O critério do pedido: a troca feita com o turno rodando chega a ele (o claude de mentira
    /// diz no fim da resposta o que recebeu no meio do turno), sem erro; com a entrada já
    /// fechada, nada vai e nada falha (a escolha vale no turno seguinte, como antes).
    #[cfg(unix)]
    #[test]
    fn a_troca_no_meio_do_turno_chega_ao_processo_que_roda() {
        let mut turno = Turno::abrir("[ferramenta Bash cargo build | 3]\nfaz o build", |o| o, &[]);
        turno.ate("a chamada", |e| matches!(e, Evento::Ferramenta { .. }));
        assert_eq!(turno.processo.ao_vivo(), &ao_vivo("manual", "haiku", None));
        let quer = ao_vivo("acceptEdits", "claude-haiku-4-5-20251001", Some("low"));
        assert_eq!(turno.processo.acertar(&quer), 3, "um pedido por troca");
        assert_eq!(turno.processo.ao_vivo(), &quer);
        assert_eq!(turno.processo.acertar(&quer), 0, "o que já foi não vai de novo");
        turno.ate("o fim do turno", |e| matches!(e, Evento::Fim { .. }));
        assert!(matches!(turno.vistos.last(), Some(Evento::Fim { erro: false, .. })), "{:?}", turno.vistos);
        assert!(
            turno.disse("no meio do turno: modo acceptEdits, modelo claude-haiku-4-5-20251001, esforço low"),
            "as trocas não chegaram ao turno: {:?}",
            turno.vistos
        );
        assert_eq!(turno.quantos(|e| matches!(e, Evento::ModeloEmUso(m) if m == "claude-haiku-4-5-20251001")), 1, "{:?}", turno.vistos);
        // O turno acabou e a entrada fechou: a troca seguinte não vai a lugar nenhum, sem erro.
        turno.processo.fechar_entrada();
        assert_eq!(turno.processo.acertar(&ao_vivo("plan", "opus", None)), 0);
        assert_eq!(turno.processo.ao_vivo(), &quer, "o que não foi não fica anotado");
        turno.ate_sair();
    }

    /// O "Sem perguntar" entra no meio de um turno aberto em Manual: o claude de mentira, como o
    /// de verdade, só aceita num processo aberto com a `--allow-dangerously-skip-permissions`.
    #[cfg(unix)]
    #[test]
    fn o_sem_perguntar_entra_num_turno_aberto_em_manual() {
        if sou_root() {
            return;
        }
        let mut turno = Turno::abrir("[ferramenta Bash cargo build | 2]\nfaz o build", |o| o, &[]);
        turno.ate("a chamada", |e| matches!(e, Evento::Ferramenta { .. }));
        assert_eq!(turno.processo.acertar(&ao_vivo("bypassPermissions", "haiku", None)), 1);
        turno.ate("o fim do turno", |e| matches!(e, Evento::Fim { .. }));
        assert!(turno.disse("no meio do turno: modo bypassPermissions"), "{:?}", turno.vistos);
    }

    /// O critério do Parar: com uma mensagem dirigida ainda na fila do Claude Code, o Parar a
    /// retira e interrompe; o turno fecha com o `result` de erro e o processo sai sozinho (código
    /// 1, e não morto por sinal), sem abrir outro turno com ela.
    #[cfg(unix)]
    #[test]
    fn o_parar_retira_a_mensagem_dirigida_e_o_processo_sai_sozinho() {
        let mut turno = Turno::abrir("[ferramenta Bash cargo build | 20]\nfaz o build", |o| o, &[]);
        turno.ate("a chamada", |e| matches!(e, Evento::Ferramenta { .. }));
        assert!(turno.processo.dirigir(Mensagem { texto: "BANANA", imagens: &[], marca: None }));
        let uuids = turno.processo.uuids_das_dirigidas().to_vec();
        assert_eq!(uuids.len(), 1);
        assert!(uuid::Uuid::parse_str(&uuids[0]).is_ok(), "{uuids:?}");
        assert!(turno.processo.retirar_dirigidas());
        turno.processo.interromper();
        assert_eq!(turno.ate_sair(), Some(1), "saiu sozinho, depois de fechar o turno: {:?}", turno.vistos);
        assert_eq!(turno.quantos(|e| matches!(e, Evento::Fim { erro: true, .. })), 1, "{:?}", turno.vistos);
        assert_eq!(turno.quantos(|e| matches!(e, Evento::Fim { .. })), 1, "{:?}", turno.vistos);
        assert_eq!(turno.quantos(|e| matches!(e, Evento::Inicio { .. })), 1, "{:?}", turno.vistos);
        assert_eq!(turno.quantos(|e| matches!(e, Evento::FicouNaFila(_))), 0, "{:?}", turno.vistos);
        assert!(!turno.disse("BANANA"), "a mensagem retirada rodou: {:?}", turno.vistos);
        // Parado, não há mais o que retirar nem a quem pedir.
        assert!(!turno.processo.retirar_dirigidas());
    }

    /// O que acontece sem retirar (por isso o Parar matava o processo): o `interrupt` fecha o
    /// turno, avisa que a mensagem ficou na fila, e o Claude Code abre outro turno com ela.
    #[cfg(unix)]
    #[test]
    fn sem_retirar_a_mensagem_dirigida_roda_depois_do_parar() {
        let mut turno = Turno::abrir("[ferramenta Bash cargo build | 20]\nfaz o build", |o| o, &[]);
        turno.ate("a chamada", |e| matches!(e, Evento::Ferramenta { .. }));
        assert!(turno.processo.dirigir(Mensagem { texto: "BANANA", imagens: &[], marca: None }));
        turno.processo.interromper();
        turno.ate_sair();
        assert_eq!(turno.quantos(|e| matches!(e, Evento::FicouNaFila(1))), 1, "{:?}", turno.vistos);
        assert_eq!(turno.quantos(|e| matches!(e, Evento::Inicio { .. })), 2, "{:?}", turno.vistos);
        assert!(turno.disse("BANANA"), "{:?}", turno.vistos);
    }

    /// A batida chega enquanto a ferramenta roda (aqui de segundo em segundo; no de verdade, a
    /// cada 30 s), entre a chamada e o resultado dela.
    #[cfg(unix)]
    #[test]
    fn a_batida_chega_enquanto_a_ferramenta_roda() {
        let mut turno = Turno::abrir("[ferramenta Bash cargo build | 3]\nfaz o build", |o| o, &[("CLAUDE_FALSO_BATIDA", "1")]);
        turno.ate("o resultado da chamada", |e| matches!(e, Evento::ResultadoFerramenta { .. }));
        let chamada = turno.vistos.iter().position(|e| matches!(e, Evento::Ferramenta { .. })).expect("a chamada");
        let batidas = turno.vistos[chamada..].iter().filter(|e| matches!(e, Evento::Vivo)).count();
        assert!(batidas >= 1, "sem batida entre a chamada e o resultado: {:?}", turno.vistos);
        assert_eq!(turno.quantos(|e| matches!(e, Evento::SubFerramenta { .. } | Evento::SubTexto { .. })), 0);
    }

    /// O texto do raciocínio vem com a opção e não vem sem ela, como no Claude Code 2.1.328.
    #[cfg(unix)]
    #[test]
    fn o_texto_do_raciocinio_vem_com_a_opcao() {
        let pedido = "[ferramenta Pensa primeiro o build e depois os testes | 1]\nfaz o build";
        let mut com = Turno::abrir(pedido, |o| o, &[]);
        com.ate("o raciocínio", |e| matches!(e, Evento::Pensamento(_)));
        assert!(matches!(com.vistos.last(), Some(Evento::Pensamento(t)) if t == "primeiro o build e depois os testes"), "{:?}", com.vistos);
        let mut sem = Turno::abrir(pedido, |o| Opcoes { raciocinio_resumido: false, ..o }, &[]);
        sem.ate("o raciocínio", |e| matches!(e, Evento::Pensamento(_)));
        assert!(matches!(sem.vistos.last(), Some(Evento::Pensamento(t)) if t.is_empty()), "{:?}", sem.vistos);
    }

    // ---------- um chat, um processo (23/11/2026) ----------

    /// O que o Claude Code 2.1.328 mandou ao retomar uma sessão cujo processo anterior foi
    /// derrubado com um agente em segundo plano (medido em 23/11/2026, Haiku, pasta em /tmp): o
    /// aviso do agente cortado, a mensagem na fila, um `init` e um `result` vazio com `num_turns`
    /// 0, e só então a mensagem começa. Esse primeiro `result` não é o fim do turno dela.
    #[test]
    fn o_result_de_antes_da_vez_da_mensagem_nao_e_o_fim_do_turno() {
        let uuid = "d22bde2c-4f94-40cd-92c9-de002c9d90f2";
        let vez = |estado: &str| json!({ "type": "command_lifecycle", "command_uuid": uuid, "state": estado });
        let aviso = json!({ "type": "system", "subtype": "task_notification", "task_id": "a3eaeeaf23ca00f7c", "status": "stopped",
            "summary": "Background agent \"x\" didn't finish before the previous session ended" });
        let init = json!({ "type": "system", "subtype": "init", "model": "claude-haiku-4-5" });
        let vazio = json!({ "type": "result", "subtype": "success", "is_error": false, "result": "", "num_turns": 0 });
        let resposta = json!({ "type": "result", "subtype": "success", "is_error": false, "result": "15", "num_turns": 1 });
        let mut da_mensagem = VezDaMensagem::nova(Some(uuid));
        let saida = [&aviso, &vez("queued"), &init, &vazio, &vez("started"), &init, &resposta, &vez("completed")];
        let de_antes: Vec<bool> = saida.iter().map(|linha| da_mensagem.result_de_antes(linha)).collect();
        assert_eq!(de_antes, [false, false, false, true, false, false, false, false], "só o `result` vazio é de antes");
        // Depois que a mensagem começou, todo `result` é de um turno do processo (o dela, o de uma
        // mensagem dirigida, o que um agente abriu ao terminar).
        assert!(!da_mensagem.result_de_antes(&vazio));
        // A vez de OUTRA mensagem (uma dirigida, com o uuid dela) não mexe na da primeira.
        let mut outra = VezDaMensagem::nova(Some(uuid));
        let da_outra = json!({ "type": "command_lifecycle", "command_uuid": "outro", "state": "queued" });
        assert!(!outra.result_de_antes(&da_outra) && !outra.result_de_antes(&vazio), "sem a fila da mensagem, vale o `result`");
        // Num Claude Code que não diz a vez (ou no turno sem uuid), nada é tirado: como antes.
        let mut sem = VezDaMensagem::nova(None);
        assert!(!sem.result_de_antes(&vez("queued")) && !sem.result_de_antes(&vazio));
        // O turno interrompido antes de começar fecha com erro DEPOIS do `started` (medido): vale.
        let mut parado = VezDaMensagem::nova(Some(uuid));
        let erro = json!({ "type": "result", "subtype": "error_during_execution", "is_error": true, "num_turns": 0 });
        assert!(!parado.result_de_antes(&vez("queued")) && !parado.result_de_antes(&vez("started")) && !parado.result_de_antes(&erro));
    }

    /// O critério do defeito de 23/11: a retomada depois do corte dá UM fim de turno, e ele vem
    /// depois da resposta. Antes, o `result` vazio chegava ao motor como o fim: o chat parecia
    /// parado com o processo ainda no meio do turno, e a mensagem seguinte abria outro processo
    /// da mesma sessão.
    #[cfg(unix)]
    #[test]
    fn a_retomada_depois_do_corte_termina_uma_vez_so_depois_da_resposta() {
        let marca = uuid::Uuid::new_v4().to_string();
        let ambiente = [("CLAUDE_FALSO_AGENTE_CORTADO", "1")];
        let mut turno = Turno::abrir_marcado("[ferramenta Bash cargo build | 1]\nfaz o build", Some(&marca), |o| o, &ambiente);
        turno.ate("o fim do turno", |e| matches!(e, Evento::Fim { .. }));
        assert!(turno.disse("Anotei"), "o fim chegou antes da resposta: {:?}", turno.vistos);
        assert!(matches!(turno.vistos.last(), Some(Evento::Fim { erro: false, texto, .. }) if texto.contains("Anotei")), "{:?}", turno.vistos);
        turno.processo.fechar_entrada();
        turno.ate_sair();
        assert_eq!(turno.quantos(|e| matches!(e, Evento::Fim { .. })), 1, "{:?}", turno.vistos);
    }

    /// A mensagem nova entra no processo que espera os agentes (medido no 2.1.328: com a entrada
    /// aberta, o Claude Code abre outro turno com ela na hora, no mesmo processo), o agente que
    /// termina abre o turno dele, e só com a entrada fechada o processo sai.
    #[cfg(unix)]
    #[test]
    fn a_mensagem_nova_entra_no_processo_que_espera_os_agentes() {
        let marca = uuid::Uuid::new_v4().to_string();
        let mut turno = Turno::abrir_marcado("[ferramenta AgenteFundo confere os testes | 6]\nfaz o build", Some(&marca), |o| o, &[]);
        turno.ate("o fim do turno", |e| matches!(e, Evento::Fim { .. }));
        assert_eq!(turno.quantos(|e| matches!(e, Evento::Fundo(1))), 1, "o agente segue em segundo plano: {:?}", turno.vistos);
        assert!(turno.processo.entrada_aberta());
        assert!(turno.processo.dirigir(Mensagem { texto: "e mais essa", imagens: &[], marca: None }));
        turno.ate("a resposta da mensagem nova", |e| matches!(e, Evento::Texto(t) if t.contains("turno novo no mesmo processo: e mais essa")));
        turno.ate("o fim do turno dela", |e| matches!(e, Evento::Fim { erro: false, .. }));
        assert_eq!(turno.quantos(|e| matches!(e, Evento::Fundo(0))), 0, "o agente ainda roda: {:?}", turno.vistos);
        turno.ate("o agente terminar", |e| matches!(e, Evento::Fundo(0)));
        turno.ate("o turno que o agente abre", |e| matches!(e, Evento::Texto(t) if t.contains("o agente terminou")));
        turno.ate("o fim dele", |e| matches!(e, Evento::Fim { erro: false, .. }));
        turno.processo.fechar_entrada();
        assert_eq!(turno.ate_sair(), Some(0), "{:?}", turno.vistos);
        assert_eq!(turno.quantos(|e| matches!(e, Evento::Fim { .. })), 3, "três turnos, um processo: {:?}", turno.vistos);
        assert_eq!(turno.quantos(|e| matches!(e, Evento::Inicio { .. })), 3, "{:?}", turno.vistos);
    }

    /// Com a entrada fechada, o processo espera o agente, abre o turno dele e sai (o que o Vitral
    /// fazia antes; continua valendo para o que não pode ser dirigido).
    #[cfg(unix)]
    #[test]
    fn com_a_entrada_fechada_o_processo_espera_o_agente_e_sai() {
        let mut turno = Turno::abrir("[ferramenta AgenteFundo confere os testes | 3]\nfaz o build", |o| o, &[]);
        turno.ate("o fim do turno", |e| matches!(e, Evento::Fim { .. }));
        turno.processo.fechar_entrada();
        assert!(!turno.processo.dirigir(Mensagem { texto: "tarde demais", imagens: &[], marca: None }));
        assert_eq!(turno.ate_sair(), Some(0), "{:?}", turno.vistos);
        assert_eq!(turno.quantos(|e| matches!(e, Evento::Fim { .. })), 2, "o turno e o do agente: {:?}", turno.vistos);
    }

    /// O Parar (e fechar o Vitral) em quem espera os agentes derruba o processo com eles: nada
    /// continua rodando por fora.
    #[cfg(unix)]
    #[test]
    fn parar_derruba_o_processo_que_espera_os_agentes() {
        let mut turno = Turno::abrir("[ferramenta AgenteFundo confere os testes | 120]\nfaz o build", |o| o, &[]);
        turno.ate("o fim do turno", |e| matches!(e, Evento::Fim { .. }));
        turno.processo.matar();
        assert_eq!(turno.ate_sair(), Some(143), "derrubado pelo SIGTERM: {:?}", turno.vistos);
        assert!(turno.processo.esperar().is_some(), "o processo ainda roda");
        assert_eq!(turno.quantos(|e| matches!(e, Evento::Fim { .. })), 1, "{:?}", turno.vistos);
    }
}

/// Testes de aceite da pergunta com opções (AskUserQuestion), do plano que fica na conversa e do
/// marco de compactação. Os formatos foram medidos no Claude Code 2.1.328 em 23/11/2026 (Haiku,
/// numa pasta em /tmp): ver o `LEIA-ME.md`.
#[cfg(test)]
mod testes_da_pergunta_do_plano_e_da_compactacao {
    use super::*;

    fn opcoes(modo: &str, url_mcp: &str) -> Opcoes {
        Opcoes {
            agente: crate::modelo::Agente::Claude,
            cwd: PathBuf::from("/tmp/x"),
            sessao: "11111111-1111-4111-8111-111111111111".into(),
            retomar: false,
            bifurcar: None,
            cortar_em: None,
            modelo: None,
            permissao: modo.into(),
            prompt_sistema: "p".into(),
            prompt_arquivo: None,
            ferramentas: None,
            url_mcp: url_mcp.into(),
            bloqueadas: Vec::new(),
            so_mcp_vitral: true,
            esforco: None,
            ultracode: false,
            pasta_conta: None,
            memoria_nativa: false,
            sugestoes: false,
            mcp_da_equipe: None,
            eventos_de_hook: true,
            raciocinio_resumido: true,
        }
    }

    /// As linhas que o 2.1.328 escreve num `/compact` (copiadas da saída dele, sem os ids).
    #[test]
    fn a_compactacao_vira_o_estado_e_o_marco() {
        let comecou = json!({ "type": "system", "subtype": "status", "status": "compacting", "session_id": "s" });
        assert!(matches!(interpretar(&comecou).as_slice(), [Evento::Compactando(true)]));
        let acabou = json!({ "type": "system", "subtype": "status", "status": null, "compact_result": "success", "session_id": "s" });
        assert!(matches!(interpretar(&acabou).as_slice(), [Evento::Compactando(false)]));
        // O `status` que não fala de compactação (a troca de modo no meio do turno) não diz nada.
        let modo = json!({ "type": "system", "subtype": "status", "status": null, "permissionMode": "acceptEdits" });
        assert!(interpretar(&modo).is_empty());
        let marco = json!({ "type": "system", "subtype": "compact_boundary", "session_id": "s",
            "compact_metadata": { "trigger": "manual", "pre_tokens": 42987, "post_tokens": 8404,
                                  "cumulative_dropped_tokens": 34583, "duration_ms": 5213 } });
        let eventos = interpretar(&marco);
        // O marco e, depois dele, quanto a conversa ocupa agora (o anel do contexto desce).
        assert!(
            matches!(eventos.as_slice(), [Evento::Compactou { manual: true, antes: Some(42987) }, Evento::Contexto(8404)]),
            "{eventos:?}"
        );
        let automatico = json!({ "type": "system", "subtype": "compact_boundary",
            "compact_metadata": { "trigger": "auto", "pre_tokens": 168432 } });
        let eventos = interpretar(&automatico);
        assert!(matches!(eventos.as_slice(), [Evento::Compactou { manual: false, antes: Some(168432) }]), "{eventos:?}");
        // Sem os números (um Claude Code que não os mande), o marco ainda entra.
        let sem = json!({ "type": "system", "subtype": "compact_boundary" });
        assert!(matches!(interpretar(&sem).as_slice(), [Evento::Compactou { manual: false, antes: None }]));
    }

    #[test]
    fn a_pergunta_e_o_plano_tem_resumo_e_detalhe_proprios() {
        let pergunta = json!({ "questions": [
            { "question": "Qual cor você prefere?", "header": "Cor", "multiSelect": false,
              "options": [{ "label": "Azul", "description": "A do céu." }, { "label": "Verde" }] },
            { "question": "Quais frutas?", "header": "Frutas", "multiSelect": true,
              "options": [{ "label": "Maçã", "description": "" }, { "label": "Uva", "description": "Roxa." }] },
        ] });
        assert_eq!(resumo_ferramenta("AskUserQuestion", &pergunta), "AskUserQuestion · Qual cor você prefere?");
        // Ao abrir a linha: as perguntas com as opções de cada uma.
        assert_eq!(
            detalhe_ferramenta("AskUserQuestion", &pergunta).0.as_deref(),
            Some("Qual cor você prefere?\n· Azul: A do céu.\n· Verde\n\nQuais frutas?\n· Maçã\n· Uva: Roxa.")
        );
        // O plano inteiro fica guardado na chamada (o cartão da conversa o abre); um plano passa
        // do limite das outras ferramentas.
        let plano = format!("# Plano\n\n{}", "1. um passo\n".repeat(900));
        assert!(plano.len() > MAX_DETALHE);
        assert_eq!(detalhe_ferramenta("ExitPlanMode", &json!({ "plan": plano })).0.as_deref(), Some(plano.trim()));
        // No 2.1.328 a chamada vem sem o plano (ele só vem no pedido de permissão e no resultado).
        assert_eq!(detalhe_ferramenta("ExitPlanMode", &json!({})), (None, None));
        assert_eq!(resumo_ferramenta("ExitPlanMode", &json!({})), "ExitPlanMode");
    }

    /// O resultado da chamada aprovada traz o plano (2.1.328): é de onde ele sai quando a chamada
    /// veio sem ele e o pedido de permissão não passou pelo Vitral (a conversa trazida).
    #[test]
    fn o_plano_aprovado_sai_do_resultado() {
        let resultado = "User has approved your plan. You can now start coding. Start with updating your todo list if applicable\n\n\
            Your plan has been saved to: /home/x/.claude/plans/a.md\nYou can refer back to it if needed during implementation.\n\n\
            ## Approved Plan:\n# Plano: criar ola.txt\n\n1. Criar o arquivo.\n2. Conferir.\n";
        assert_eq!(plano_do_resultado(resultado), Some("# Plano: criar ola.txt\n\n1. Criar o arquivo.\n2. Conferir."));
        // Editado antes de aprovar, o título muda.
        assert_eq!(plano_do_resultado("ok\n\n## Approved Plan (edited by user):\nNovo plano"), Some("Novo plano"));
        assert_eq!(plano_do_resultado("O usuário pediu mudanças no plano: menos passos"), None);
        assert_eq!(plano_do_resultado("## Approved Plan:\n  \n"), None);
    }

    /// Um turno no claude de mentira com os argumentos de um turno de verdade; devolve os eventos.
    #[cfg(unix)]
    fn turno(texto: &str, args: Vec<String>) -> Vec<Evento> {
        let falso = Path::new(env!("CARGO_MANIFEST_DIR")).join("ferramentas/claude-falso/claude");
        let pasta = std::env::temp_dir().join(format!("vitral-pergunta-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        let mensagem = json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "text", "text": texto }] } });
        let mut filho = processos::novo_comando("python3")
            .arg("-I")
            .arg(&falso)
            .args(args)
            .current_dir(&pasta)
            .env("XDG_STATE_HOME", &pasta)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("python3 para o claude de mentira");
        writeln!(filho.stdin.take().unwrap(), "{mensagem}").unwrap();
        let saida = filho.wait_with_output().unwrap();
        let _ = std::fs::remove_dir_all(&pasta);
        String::from_utf8_lossy(&saida.stdout)
            .lines()
            .filter_map(|linha| serde_json::from_str::<Value>(linha).ok())
            .flat_map(|valor| interpretar(&valor))
            .collect()
    }

    /// O servidor MCP do Vitral respondendo aos pedidos com `decidir`; devolve os pedidos vistos.
    #[cfg(unix)]
    fn servidor_que_responde(
        decidir: impl Fn(&Value) -> Value + Send + Sync + 'static,
    ) -> (crate::mcp::Servidor, std::sync::mpsc::Receiver<Value>) {
        let (chegou, pedidos) = std::sync::mpsc::channel::<Value>();
        let chegou = Mutex::new(chegou);
        let servidor = crate::mcp::iniciar(move |chamada: crate::mcp::Chamada| {
            let resposta = decidir(&chamada.argumentos);
            let _ = chegou.lock().unwrap().send(chamada.argumentos.clone());
            let _ = chamada.resposta.try_send(crate::mcp::Resposta::ok(resposta.to_string()));
        })
        .unwrap();
        (servidor, pedidos)
    }

    /// De ponta a ponta com o claude de mentira, que encena a AskUserQuestion como o 2.1.328: o
    /// pedido chega à ferramenta de permissão (em qualquer modo, até em "Sem perguntar") com as
    /// perguntas, e a resposta do cartão volta como `allow` com o `updatedInput` levando as
    /// perguntas e as `answers`, com a pergunta inteira como chave.
    #[cfg(unix)]
    #[test]
    fn a_resposta_da_pergunta_volta_no_formato_do_claude_code() {
        let (servidor, pedidos) = servidor_que_responde(|argumentos| {
            let respostas: Vec<(String, String)> = argumentos["input"]["questions"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|q| {
                    let pergunta = q["question"].as_str().unwrap_or_default().to_string();
                    let resposta = if q["multiSelect"] == true { "Maçã, Uva" } else { "Verde" };
                    (pergunta, resposta.to_string())
                })
                .collect();
            crate::motor::resposta_da_pergunta(&argumentos["input"], &respostas)
        });
        let texto = "[pergunta Cor | Qual cor você prefere? | Azul: A do céu.; Verde; Vermelho]\n\
                     [pergunta+ Frutas | Quais frutas você gosta? | Maçã; Banana; Uva]\npergunta";
        let eventos = turno(texto, argumentos(&opcoes("bypassPermissions", &servidor.url("c1"))));
        let chamadas = eventos.iter().filter(|e| matches!(e, Evento::Ferramenta { nome, .. } if nome == "AskUserQuestion")).count();
        assert_eq!(chamadas, 1, "as duas perguntas numa chamada só: {eventos:?}");
        let pedido = pedidos.try_recv().expect("o pedido de permissão");
        assert_eq!(pedido["tool_name"], "AskUserQuestion");
        let perguntas = pedido["input"]["questions"].as_array().expect("as perguntas");
        assert_eq!(perguntas.len(), 2);
        assert_eq!(perguntas[0]["question"], "Qual cor você prefere?");
        assert_eq!(perguntas[0]["header"], "Cor");
        assert_eq!(perguntas[0]["multiSelect"], false);
        assert_eq!(perguntas[0]["options"][0], json!({ "label": "Azul", "description": "A do céu." }));
        assert_eq!(perguntas[1]["multiSelect"], true);
        assert!(pedidos.try_recv().is_err(), "um pedido só");
        // O que o Claude Code devolve ao modelo com as respostas certas.
        let resultado = eventos
            .iter()
            .find_map(|e| match e {
                Evento::ResultadoFerramenta { conteudo, erro: false, .. } => Some(conteudo.as_str()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("a pergunta não foi respondida: {eventos:?}"));
        assert!(resultado.contains("\"Qual cor você prefere?\"=\"Verde\""), "{resultado}");
        assert!(resultado.contains("\"Quais frutas você gosta?\"=\"Maçã, Uva\""), "{resultado}");

        // Como era (o Permitir do cartão genérico devolvia a entrada como veio): ninguém respondeu.
        let (servidor, _pedidos) = servidor_que_responde(|argumentos| json!({ "behavior": "allow", "updatedInput": argumentos["input"] }));
        let eventos = turno(texto, argumentos(&opcoes("manual", &servidor.url("c1"))));
        assert!(
            eventos.iter().any(|e| matches!(e, Evento::ResultadoFerramenta { conteudo, .. } if conteudo == "The user did not answer the questions.")),
            "{eventos:?}"
        );
        // Só as respostas, sem as perguntas: o Claude Code recusa o `updatedInput`.
        let (servidor, _pedidos) = servidor_que_responde(|_| json!({ "behavior": "allow", "updatedInput": { "answers": { "x": "y" } } }));
        let eventos = turno(texto, argumentos(&opcoes("manual", &servidor.url("c1"))));
        assert!(
            eventos.iter().any(|e| matches!(e, Evento::ResultadoFerramenta { conteudo, erro: true, .. } if conteudo.contains("failed schema validation"))),
            "{eventos:?}"
        );
    }

    /// O plano no claude de mentira, como no 2.1.328: a chamada vem vazia, o plano vem no pedido
    /// de permissão e, aprovado, no resultado; recusado, o resultado é a mensagem, como erro.
    #[cfg(unix)]
    #[test]
    fn o_plano_vem_no_pedido_e_no_resultado_aprovado() {
        let texto = "[plano # Trocar o cupom\\n\\n1. Ler o carrinho.\\n2. Aplicar o desconto.]\nplaneja";
        let (servidor, pedidos) = servidor_que_responde(|argumentos| json!({ "behavior": "allow", "updatedInput": argumentos["input"] }));
        let eventos = turno(texto, argumentos(&opcoes("plan", &servidor.url("c1"))));
        let entrada = eventos
            .iter()
            .find_map(|e| match e {
                Evento::Ferramenta { nome, entrada, .. } if nome == "ExitPlanMode" => Some(entrada.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("sem a chamada do plano: {eventos:?}"));
        assert_eq!(entrada, json!({}), "a chamada vem sem o plano");
        let pedido = pedidos.try_recv().expect("o pedido de permissão");
        assert_eq!(pedido["tool_name"], "ExitPlanMode");
        let plano = "# Trocar o cupom\n\n1. Ler o carrinho.\n2. Aplicar o desconto.";
        assert_eq!(pedido["input"]["plan"], plano);
        assert_eq!(pedido["tool_use_id"], "toolu_falso_p1");
        let aprovado = eventos
            .iter()
            .find_map(|e| match e {
                Evento::ResultadoFerramenta { id, conteudo, erro: false, .. } if id == "toolu_falso_p1" => Some(conteudo.as_str()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("o plano aprovado não voltou: {eventos:?}"));
        assert_eq!(plano_do_resultado(aprovado), Some(plano));

        let recusa = "O usuário pediu mudanças no plano: começa pelos testes";
        let (servidor, _pedidos) = servidor_que_responde(move |_| json!({ "behavior": "deny", "message": recusa }));
        let eventos = turno(texto, argumentos(&opcoes("plan", &servidor.url("c1"))));
        assert!(
            eventos.iter().any(|e| matches!(e, Evento::ResultadoFerramenta { conteudo, erro: true, .. } if conteudo == recusa)),
            "{eventos:?}"
        );
    }

    /// A compactação no claude de mentira: o `[compacta ...]` no meio de um turno (automática) e o
    /// "/compact" como mensagem (manual), com as mesmas linhas do 2.1.328.
    #[cfg(unix)]
    #[test]
    fn a_compactacao_encenada_tem_o_estado_e_o_marco() {
        let args = argumentos(&opcoes("manual", "http://127.0.0.1:1/mcp/projeto/c1"));
        let eventos = turno("[compacta auto 168432 | 1]\ncontinua", args.clone());
        let posicao = |achar: &dyn Fn(&Evento) -> bool| eventos.iter().position(achar);
        let comecou = posicao(&|e| matches!(e, Evento::Compactando(true))).unwrap_or_else(|| panic!("{eventos:?}"));
        let acabou = posicao(&|e| matches!(e, Evento::Compactando(false))).unwrap_or_else(|| panic!("{eventos:?}"));
        let marco = posicao(&|e| matches!(e, Evento::Compactou { manual: false, antes: Some(168432) })).unwrap_or_else(|| panic!("{eventos:?}"));
        let fim = posicao(&|e| matches!(e, Evento::Fim { erro: false, .. })).unwrap_or_else(|| panic!("{eventos:?}"));
        assert!(comecou < acabou && acabou < marco && marco < fim, "{eventos:?}");
        assert!(eventos[marco..fim].iter().any(|e| matches!(e, Evento::Texto(_))), "o turno segue depois do marco: {eventos:?}");

        let eventos = turno("/compact", args);
        assert!(eventos.iter().any(|e| matches!(e, Evento::Compactando(true))), "{eventos:?}");
        assert!(eventos.iter().any(|e| matches!(e, Evento::Compactou { manual: true, antes: Some(_) })), "{eventos:?}");
        // Como no de verdade: o /compact termina sem texto.
        assert!(!eventos.iter().any(|e| matches!(e, Evento::Texto(_))), "{eventos:?}");
        assert!(matches!(eventos.last(), Some(Evento::Fim { erro: false, texto, .. }) if texto.is_empty()), "{eventos:?}");
    }
}

#[cfg(test)]
mod testes_das_imagens_do_resultado {
    use super::*;
    use crate::{imagens_recebidas, modelo::Item};

    /// Um PNG de mentira: a assinatura, o IHDR com as medidas e um recheio.
    fn png(largura: u32, altura: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend([0, 0, 0, 13]);
        bytes.extend(b"IHDR");
        bytes.extend(largura.to_be_bytes());
        bytes.extend(altura.to_be_bytes());
        bytes.extend((0..6000u32).map(|i| (i * 37 % 253) as u8));
        bytes
    }

    fn resultado(id: &str, conteudo: Value) -> Value {
        json!({"type": "user", "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": id, "content": conteudo},
        ]}})
    }

    fn bloco_de_imagem(bytes: &[u8]) -> Value {
        json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": base64(bytes)}})
    }

    /// O print de um MCP de navegador: o texto vira o resultado e a imagem sai num evento à
    /// parte, já fora do base64.
    #[test]
    fn o_resultado_com_imagem_entrega_o_texto_e_a_imagem_separados() {
        let imagem = png(1280, 800);
        let linha = resultado("toolu_1", json!([{"type": "text", "text": "Took a screenshot of the page."}, bloco_de_imagem(&imagem)]));
        let eventos = interpretar(&linha);
        match eventos.as_slice() {
            [Evento::ResultadoFerramenta { id, conteudo, erro: false, .. }, Evento::ImagensDoResultado { id: da_imagem, imagens }] => {
                assert_eq!((id.as_str(), da_imagem.as_str()), ("toolu_1", "toolu_1"));
                assert_eq!(conteudo, "Took a screenshot of the page.");
                assert_eq!(imagens.len(), 1);
                assert_eq!((imagens[0].extensao, &imagens[0].bytes), ("png", &imagem));
            }
            outros => panic!("eventos inesperados: {outros:?}"),
        }
        // Nem o texto de depuração dos eventos carrega a imagem.
        assert!(format!("{eventos:?}").len() < 600);
    }

    /// O Read de uma imagem devolve só o bloco dela: o resultado fica sem texto (a miniatura é o
    /// que se mostra), e o resultado sem imagem continua um evento só.
    #[test]
    fn o_resultado_so_com_a_imagem_fica_sem_texto() {
        let eventos = interpretar(&resultado("toolu_2", json!([bloco_de_imagem(&png(40, 20))])));
        assert!(matches!(eventos.as_slice(), [
            Evento::ResultadoFerramenta { conteudo, .. },
            Evento::ImagensDoResultado { imagens, .. },
        ] if conteudo.is_empty() && imagens.len() == 1));
        let eventos = interpretar(&resultado("toolu_3", json!("saída do comando")));
        assert!(matches!(eventos.as_slice(), [Evento::ResultadoFerramenta { conteudo, .. }] if conteudo == "saída do comando"));
        let eventos = interpretar(&resultado("toolu_4", json!([{"type": "text", "text": "um"}, {"type": "text", "text": "dois"}])));
        assert!(matches!(eventos.as_slice(), [Evento::ResultadoFerramenta { conteudo, .. }] if conteudo == "um\ndois"));
    }

    /// A imagem que não dá para guardar deixa a marca dela no texto, como antes.
    #[test]
    fn a_imagem_que_nao_da_para_guardar_vira_a_marca_no_texto() {
        let linha = resultado("toolu_5", json!([
            {"type": "text", "text": "Diagrama:"},
            {"type": "image", "source": {"type": "base64", "media_type": "image/svg+xml", "data": base64(b"<svg></svg>")}},
        ]));
        let eventos = interpretar(&linha);
        assert!(matches!(eventos.as_slice(), [Evento::ResultadoFerramenta { conteudo, .. }] if conteudo == "Diagrama:\n(image/svg+xml)"));
    }

    /// Da linha do Claude Code à conversa gravada: a imagem vai para a pasta do chat e o que fica
    /// na conversa (o que o `salvar_chat` grava) é o caminho. O base64 não aparece em lugar nenhum.
    #[test]
    fn o_base64_da_imagem_nao_chega_a_conversa_gravada() {
        let raiz = std::env::temp_dir().join(format!("vitral-claude-imagens-{}", crate::modelo::novo_id()));
        let dados = crate::dados::Dados::em(&raiz);
        let imagem = png(1920, 1080);
        let em_base64 = base64(&imagem);
        let linha = resultado("toolu_print", json!([{"type": "text", "text": "Print tirado."}, bloco_de_imagem(&imagem)]));

        // A chamada já está na conversa; os eventos do resultado chegam como o motor os trata.
        let mut itens = vec![Item::Ferramenta {
            id: "toolu_print".into(),
            nome: "mcp__chrome-devtools__take_screenshot".into(),
            resumo: "mcp__chrome-devtools__take_screenshot".into(),
            resultado: None,
            erro: false,
            em: 0,
            detalhe: None,
            arquivo: None,
            sub: None,
            mudanca: None,
            imagens: Vec::new(),
            durou: 0,
        }];
        for evento in interpretar(&linha) {
            match evento {
                Evento::ResultadoFerramenta { conteudo, .. } => {
                    if let Item::Ferramenta { resultado, .. } = &mut itens[0] {
                        *resultado = Some(conteudo);
                    }
                }
                Evento::ImagensDoResultado { id, imagens } => {
                    assert!(imagens_recebidas::guardar_na_chamada(&mut itens, &id, &dados.pasta_imagens("chat-1"), &imagens));
                }
                outro => panic!("evento inesperado: {outro:?}"),
            }
        }

        let Item::Ferramenta { resultado, imagens, .. } = &itens[0] else { unreachable!() };
        assert_eq!(resultado.as_deref(), Some("Print tirado."));
        assert_eq!(imagens.len(), 1);
        assert!(imagens[0].starts_with(dados.pasta_imagens("chat-1")));
        assert_eq!(std::fs::read(&imagens[0]).unwrap(), imagem);

        let gravado = serde_json::to_string_pretty(&itens).unwrap();
        assert!(gravado.contains(&imagens[0].display().to_string()));
        for pedaco in [&em_base64[..64], &em_base64[em_base64.len() / 2..em_base64.len() / 2 + 64]] {
            assert!(!gravado.contains(pedaco), "o base64 foi para a conversa");
        }
        assert!(!gravado.contains("(image/png)"));
        assert!(gravado.len() < 1500, "{} bytes", gravado.len());
        let _ = std::fs::remove_dir_all(&raiz);
    }
}
