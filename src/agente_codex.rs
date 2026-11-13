//! O Codex (CLI da OpenAI) como o agente de um chat: um `codex exec --json` por turno, com o pedido
//! pela entrada padrão, e a saída traduzida para os mesmos eventos do Claude Code (`claude::Evento`),
//! de modo que a conversa, a trilha e o motor não precisem saber quem respondeu.
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
    processos,
};

/// No Windows o Codex do npm é um `codex.cmd`.
const NOME_DO_CODEX: &str = if cfg!(windows) { "codex.cmd" } else { "codex" };

/// Quanto se espera o `codex debug models` responder.
const PRAZO_DOS_MODELOS: Duration = Duration::from_secs(20);

/// O `codex` do PATH ou o de `~/.local/bin` (apps abertos pela dock nem sempre têm esse PATH).
pub fn binario() -> PathBuf {
    binario_em(std::env::var_os("PATH"), dirs::home_dir().unwrap_or_default())
}

fn binario_em(path: Option<std::ffi::OsString>, home: PathBuf) -> PathBuf {
    if let Some(caminho) = path.and_then(|path| std::env::split_paths(&path).map(|p| p.join(NOME_DO_CODEX)).find(|p| p.is_file())) {
        return caminho;
    }
    let local = home.join(".local").join("bin").join(NOME_DO_CODEX);
    if local.is_file() { local } else { PathBuf::from(NOME_DO_CODEX) }
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

/// O que cada modo de permissão quer dizer num chat do Codex (para a dica do seletor).
pub fn dica_da_permissao(permissao: &str) -> &'static str {
    match sandbox_de(permissao) {
        "read-only" => "O Codex só lê: não muda arquivo nem roda comando que escreve.",
        "danger-full-access" => "O Codex roda sem sandbox, com acesso ao computador todo e à rede.",
        _ => "O Codex escreve só na pasta do chat e roda sem rede. Ele não pede permissão no meio do turno.",
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
            "turn.completed" => vec![Evento::Fim {
                erro: false,
                texto: std::mem::take(&mut self.ultima_fala),
                custo_usd: 0.,
                janela: None,
            }],
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
        // A chamada (uma vez só, no começo ou já no fim) e o resultado quando o item termina.
        let mut ferramenta = |nome: &str, entrada: Value, resultado: String, erro: bool| {
            let mut eventos = Vec::new();
            if self.abertos.insert(id.clone()) {
                eventos.push(Evento::Ferramenta { id: id.clone(), nome: nome.to_string(), entrada });
            }
            if terminou {
                eventos.push(Evento::ResultadoFerramenta { id: id.clone(), conteudo: resultado, erro });
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
            "file_change" => {
                let mudancas = item["changes"].as_array().cloned().unwrap_or_default();
                let caminho = |m: &Value| m["path"].as_str().unwrap_or_default().to_string();
                let (nome, entrada) = match mudancas.as_slice() {
                    [uma] if uma["kind"] == "add" => ("Write", json!({ "file_path": caminho(uma) })),
                    [uma] => ("Edit", json!({ "file_path": caminho(uma) })),
                    _ => ("Edit", json!({ "file_path": mudancas.first().map(caminho).unwrap_or_default() })),
                };
                ferramenta(nome, entrada, resumo_das_mudancas(&mudancas), falhou)
            }
            "mcp_tool_call" => {
                let nome = format!("mcp__{}__{}", texto("server"), texto("tool"));
                let resultado = match &item["result"] {
                    Value::Null => item["error"]["message"].as_str().unwrap_or_default().to_string(),
                    Value::String(s) => s.clone(),
                    outro => outro.to_string(),
                };
                ferramenta(&nome, item["arguments"].clone(), resultado, falhou)
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

/// O que mudou, um arquivo por linha (o Codex não entrega o diff).
fn resumo_das_mudancas(mudancas: &[Value]) -> String {
    mudancas
        .iter()
        .map(|m| {
            let verbo = match m["kind"].as_str().unwrap_or_default() {
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
            mcp_da_equipe: None,
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
                Evento::ResultadoFerramenta { id, conteudo, erro } => format!("R:{id}:{}:{erro}", conteudo.trim()),
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
}
