//! O que as Configurações fazem com o Codex e o Gemini (Antigravity CLI, `agy`) além de rodar
//! turnos: ler a versão, perguntar quem está logado, listar os servidores MCP, abrir o terminal
//! com o login e rodar o `update` deles, fora de qualquer turno. Tudo que bloqueia (processo
//! filho) fica aqui, para quem chama pôr em segundo plano; a leitura do que eles imprimem é pura e
//! tem testes. Nunca roda `login`, `logout` nem `update` por conta própria: só o botão.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};

use crate::{claude::{Atualizacao, Versao}, modelo::Agente};

/// Quanto se espera cada leitura (`--version`, `login status`, `mcp list`).
const PRAZO_DA_LEITURA: Duration = Duration::from_secs(8);

/// Como o Codex diz que a pessoa entrou (`codex login status`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContaDoCodex {
    ChatGpt,
    ChaveDeApi,
    Fora,
    /// Resposta que o Vitral não conhece: a primeira linha, como veio.
    Outra(String),
}

/// Um servidor MCP de uma lista só de leitura.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServidorFora {
    pub nome: String,
    /// O comando ou o endereço (sem variáveis de ambiente, cabeçalhos nem parâmetros de URL).
    pub detalhe: String,
    pub ligado: bool,
}

/// O que se sabe de um agente de fora, vivo no motor para sobreviver a fechar as Configurações.
#[derive(Debug, Clone, Default)]
pub struct CliFora {
    /// A versão instalada: `None` até a primeira leitura, `Some(None)` quando não deu para ler.
    pub versao: Option<Option<Versao>>,
    /// Só o Codex: o que o `codex login status` respondeu (`None` até a primeira leitura).
    pub conta: Option<ContaDoCodex>,
    /// O `update` está rodando.
    pub atualizando: bool,
    /// O que o último `update` respondeu.
    pub resposta: Option<Atualizacao>,
}

impl ContaDoCodex {
    pub fn entrou(&self) -> bool {
        matches!(self, ContaDoCodex::ChatGpt | ContaDoCodex::ChaveDeApi)
    }

    pub fn frase(&self) -> String {
        match self {
            ContaDoCodex::ChatGpt => "Entrou com a conta do ChatGPT".into(),
            ContaDoCodex::ChaveDeApi => "Entrou com uma chave de API".into(),
            ContaDoCodex::Fora => "Não entrou".into(),
            ContaDoCodex::Outra(linha) => linha.clone(),
        }
    }
}

/// Traduz o que o `codex login status` imprimiu.
pub fn conta_do_codex(saida: &str, saiu_bem: bool) -> ContaDoCodex {
    let primeira = saida.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    let minuscula = primeira.to_lowercase();
    if minuscula.starts_with("not logged in") || primeira.is_empty() {
        ContaDoCodex::Fora
    } else if minuscula.starts_with("logged in") && minuscula.contains("api key") {
        ContaDoCodex::ChaveDeApi
    } else if minuscula.starts_with("logged in") && minuscula.contains("chatgpt") {
        ContaDoCodex::ChatGpt
    } else if !saiu_bem && minuscula.contains("not") {
        ContaDoCodex::Fora
    } else {
        ContaDoCodex::Outra(primeira.to_string())
    }
}

/// O endereço sem o que vem depois de `?` (parâmetros de URL podem levar uma chave).
fn sem_parametros(endereco: &str) -> String {
    endereco.split(['?', '#']).next().unwrap_or_default().to_string()
}

/// Os servidores do `codex mcp list --json`.
pub fn servidores_do_codex(json: &str) -> Option<Vec<ServidorFora>> {
    let valor: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    let lista = valor.as_array()?;
    Some(
        lista
            .iter()
            .filter_map(|s| {
                let nome = s["name"].as_str()?.trim();
                let transporte = &s["transport"];
                let detalhe = transporte["command"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| transporte["url"].as_str().map(sem_parametros))
                    .unwrap_or_default();
                (!nome.is_empty()).then(|| ServidorFora { nome: nome.into(), detalhe, ligado: s["enabled"].as_bool().unwrap_or(true) })
            })
            .collect(),
    )
}

/// Os servidores do `agy mcp list`.
///
/// O formato com servidores não foi visto (o agy desta máquina não tem nenhum; sem eles ele diz
/// "No MCP servers configured."), então a leitura é tolerante: um JSON de lista, se vier, ou uma
/// linha por servidor, com o nome na primeira palavra (sem marcador de lista nem `:`) e o resto
/// como detalhe; "(disabled)" ou "desligado" na linha marca o servidor como desligado.
pub fn servidores_do_gemini(texto: &str) -> Option<Vec<ServidorFora>> {
    let texto = texto.trim();
    if texto.is_empty() || texto.to_lowercase().starts_with("no mcp servers") {
        return Some(Vec::new());
    }
    if texto.starts_with('[') || texto.starts_with('{') {
        let valor: serde_json::Value = serde_json::from_str(texto).ok()?;
        let lista = valor.as_array().cloned().or_else(|| valor["servers"].as_array().cloned())?;
        return Some(
            lista
                .iter()
                .filter_map(|s| {
                    let nome = s["name"].as_str()?.trim();
                    let detalhe = s["command"].as_str().map(str::to_string).or_else(|| s["url"].as_str().map(sem_parametros)).unwrap_or_default();
                    let ligado = s["enabled"].as_bool().unwrap_or(true) && s["disabled"].as_bool() != Some(true);
                    (!nome.is_empty()).then(|| ServidorFora { nome: nome.into(), detalhe, ligado })
                })
                .collect(),
        );
    }
    let servidores: Vec<ServidorFora> = texto
        .lines()
        .filter_map(|linha| {
            let linha = linha.trim().trim_start_matches(['-', '*', '•', '·', '✓', '✔', '✗', '✘']).trim();
            let (nome, resto) = linha.split_once(char::is_whitespace).unwrap_or((linha, ""));
            let nome = nome.trim_end_matches(':');
            if nome.is_empty() {
                return None;
            }
            let minuscula = resto.to_lowercase();
            let ligado = !(minuscula.contains("(disabled)") || minuscula.contains("desligado") || minuscula.contains("[disabled]"));
            let detalhe = resto
                .replace("(disabled)", "")
                .replace("[disabled]", "")
                .split_whitespace()
                .map(|p| if p.starts_with("http://") || p.starts_with("https://") { sem_parametros(p) } else { p.to_string() })
                .collect::<Vec<_>>()
                .join(" ");
            Some(ServidorFora { nome: nome.into(), detalhe, ligado })
        })
        .collect();
    Some(servidores)
}

/// O comando que o terminal roda para `binario args` (o login): com o caminho inteiro entre aspas,
/// porque o PATH do Vitral aberto pela dock nem sempre tem a pasta do programa.
pub fn comando_de_terminal(binario: &Path, args: &str, powershell: bool) -> String {
    let caminho = binario.display().to_string();
    if powershell {
        format!("& '{}' {args}", caminho.replace('\'', "''"))
    } else {
        format!("'{}' {args}", caminho.replace('\'', "'\\''"))
    }
}

/// Deixa a resposta do `update` do agy legível: ele responde um JSON de uma linha
/// (`{"success":true,"message":"Already on the latest version."}`).
pub fn limpar_resposta(resposta: Atualizacao) -> Atualizacao {
    let [linha] = resposta.linhas.as_slice() else {
        return resposta;
    };
    let Ok(valor) = serde_json::from_str::<serde_json::Value>(linha) else {
        return resposta;
    };
    let Some(mensagem) = valor["message"].as_str().map(str::trim).filter(|m| !m.is_empty()) else {
        return resposta;
    };
    Atualizacao { ok: resposta.ok && valor["success"].as_bool() != Some(false), linhas: vec![mensagem.to_string()] }
}

pub fn binario(agente: Agente) -> PathBuf {
    match agente {
        Agente::Codex => crate::agente_codex::binario(),
        Agente::Gemini => crate::agente_gemini::binario(),
        Agente::Claude => crate::claude::binario_claude(),
    }
}

/// Roda `binario args` em `pasta`, espera até o prazo e devolve (saiu com 0, o que imprimiu pela
/// saída e pelo erro). `None` sem o programa ou sem resposta no prazo. Bloqueia.
fn saida_com_prazo(binario: PathBuf, args: &[&str], pasta: Option<PathBuf>) -> Option<(bool, String)> {
    let mut comando = crate::processos::novo_comando(binario);
    comando.args(args).env("NO_COLOR", "1").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(pasta) = pasta {
        comando.current_dir(pasta);
    }
    let mut filho = comando.spawn().ok()?;
    let inicio = Instant::now();
    loop {
        match filho.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if inicio.elapsed() < PRAZO_DA_LEITURA => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = filho.kill();
                let _ = filho.wait();
                return None;
            }
        }
    }
    let saida = filho.wait_with_output().ok()?;
    let mut texto = String::from_utf8_lossy(&saida.stdout).into_owned();
    texto.push_str(&String::from_utf8_lossy(&saida.stderr));
    Some((saida.status.success(), texto))
}

/// A versão instalada do Codex ou do Gemini (`--version`); `None` sem o programa ou sem versão.
pub fn versao_de(agente: Agente) -> Option<Versao> {
    if !agente.instalado() {
        return None;
    }
    let (_, texto) = saida_com_prazo(binario(agente), &["--version"], None)?;
    crate::versoes_agentes::versao_no_texto(&texto)
}

/// O que o `codex login status` diz; `None` sem o Codex ou sem resposta.
pub fn ler_conta_do_codex() -> Option<ContaDoCodex> {
    if !crate::agente_codex::instalado() {
        return None;
    }
    let (ok, texto) = saida_com_prazo(binario(Agente::Codex), &["login", "status"], None)?;
    Some(conta_do_codex(&texto, ok))
}

/// A lista de servidores MCP do agente (só leitura); `None` sem o programa, sem resposta ou com
/// uma saída que não se entende.
pub fn ler_servidores(agente: Agente) -> Option<Vec<ServidorFora>> {
    let casa = dirs::home_dir();
    match agente {
        Agente::Codex if crate::agente_codex::instalado() => {
            let (ok, texto) = saida_com_prazo(binario(agente), &["mcp", "list", "--json"], casa)?;
            ok.then(|| servidores_do_codex(&texto)).flatten()
        }
        Agente::Gemini if crate::agente_gemini::instalado() => {
            let (ok, texto) = saida_com_prazo(binario(agente), &["mcp", "list"], casa)?;
            ok.then(|| servidores_do_gemini(&texto)).flatten()
        }
        _ => None,
    }
}

/// Roda `codex update` ou `agy update` (fora de qualquer turno) e devolve o que ele disse.
/// Bloqueia: chamar em segundo plano.
pub fn atualizar(agente: Agente) -> Atualizacao {
    let nome = if agente == Agente::Gemini { "agy" } else { "codex" };
    limpar_resposta(crate::claude::atualizar_programa(&binario(agente), nome, "update"))
}

/// O `codex logout`; devolve se saiu com 0. Bloqueia.
pub fn sair_do_codex() -> bool {
    crate::claude::atualizar_programa(&binario(Agente::Codex), "codex", "logout").ok
}

/// O caminho do arquivo de configuração do agente, se ele existir neste computador: o
/// `config.toml` do Codex (em `$CODEX_HOME` ou `~/.codex`) e o `settings.json` do agy
/// (`~/.gemini/antigravity-cli/settings.json`, o caminho que o próprio agy usa; ele só nasce
/// quando a pessoa muda um ajuste, então pode não existir).
pub fn arquivo_de_configuracao(agente: Agente) -> Option<PathBuf> {
    let caminho = match agente {
        Agente::Codex => crate::codex::raiz()?.join("config.toml"),
        Agente::Gemini => dirs::home_dir()?.join(".gemini").join("antigravity-cli").join("settings.json"),
        Agente::Claude => return None,
    };
    caminho.is_file().then_some(caminho)
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::path::Path;

    #[test]
    fn o_login_do_codex_vira_frase_em_portugues() {
        let chatgpt = conta_do_codex("Logged in using ChatGPT\n", true);
        assert_eq!(chatgpt, ContaDoCodex::ChatGpt);
        assert_eq!(chatgpt.frase(), "Entrou com a conta do ChatGPT");
        assert!(chatgpt.entrou());
        // A chave vem mascarada pelo próprio Codex, mas nunca vai para a tela.
        let chave = conta_do_codex("Logged in using an API key - sk-proj-***abcd\n", true);
        assert_eq!(chave, ContaDoCodex::ChaveDeApi);
        assert_eq!(chave.frase(), "Entrou com uma chave de API");
        assert!(!chave.frase().contains("sk-"));
        let fora = conta_do_codex("Not logged in\n", false);
        assert_eq!(fora, ContaDoCodex::Fora);
        assert_eq!(fora.frase(), "Não entrou");
        assert!(!fora.entrou());
        // O que não se conhece sai como veio (a primeira linha), sem fingir que entrou.
        let outra = conta_do_codex("Algo novo aqui\nsegunda linha", true);
        assert_eq!(outra, ContaDoCodex::Outra("Algo novo aqui".into()));
        assert_eq!(outra.frase(), "Algo novo aqui");
        assert!(!outra.entrou());
        assert_eq!(conta_do_codex("", false), ContaDoCodex::Fora);
    }

    #[test]
    fn a_lista_de_mcp_do_codex_vem_do_json() {
        let json = r#"[
          {"name":"docs","enabled":true,"disabled_reason":null,
           "transport":{"type":"stdio","command":"docs-server","args":["--porta","4000"],"env":{"CHAVE":"segredo"},"env_vars":[],"cwd":null},
           "startup_timeout_sec":null,"tool_timeout_sec":null,"auth_status":"unsupported"},
          {"name":"figma","enabled":false,"disabled_reason":null,
           "transport":{"type":"streamable_http","url":"https://mcp.figma.com/mcp?token=segredo","bearer_token_env_var":null,"http_headers":null,"env_http_headers":null},
           "startup_timeout_sec":null,"tool_timeout_sec":null,"auth_status":"not_logged_in"}
        ]"#;
        let lista = servidores_do_codex(json).unwrap();
        assert_eq!(
            lista,
            [
                ServidorFora { nome: "docs".into(), detalhe: "docs-server".into(), ligado: true },
                ServidorFora { nome: "figma".into(), detalhe: "https://mcp.figma.com/mcp".into(), ligado: false },
            ]
        );
        // Nenhum servidor é uma lista vazia; o que não é JSON de lista não é lista.
        assert_eq!(servidores_do_codex("[]\n"), Some(vec![]));
        assert_eq!(servidores_do_codex("error: nope"), None);
        assert_eq!(servidores_do_codex("{}"), None);
    }

    #[test]
    fn a_lista_de_mcp_do_gemini_vem_do_texto_do_agy() {
        assert_eq!(servidores_do_gemini("No MCP servers configured.\n"), Some(vec![]));
        assert_eq!(servidores_do_gemini(""), Some(vec![]));
        let texto = "- fs  npx -y @modelcontextprotocol/server-filesystem /work\n- api  https://example.com/mcp?token=segredo (disabled)\n";
        let lista = servidores_do_gemini(texto).unwrap();
        assert_eq!(lista.len(), 2);
        assert_eq!((lista[0].nome.as_str(), lista[0].ligado), ("fs", true));
        assert_eq!(lista[0].detalhe, "npx -y @modelcontextprotocol/server-filesystem /work");
        assert_eq!((lista[1].nome.as_str(), lista[1].ligado), ("api", false));
        assert_eq!(lista[1].detalhe, "https://example.com/mcp");
        // Se um dia o agy imprimir JSON, vale também.
        let json = r#"[{"name":"gh","command":"docker","enabled":true}]"#;
        assert_eq!(servidores_do_gemini(json).unwrap()[0].nome, "gh");
    }

    #[test]
    fn o_comando_do_terminal_leva_o_caminho_entre_aspas() {
        let agy = Path::new("/home/ana/.local/bin/agy");
        assert_eq!(comando_de_terminal(agy, "acp --login", false), "'/home/ana/.local/bin/agy' acp --login");
        let com_aspa = Path::new("/home/o'neil/bin/codex");
        assert_eq!(comando_de_terminal(com_aspa, "login", false), "'/home/o'\\''neil/bin/codex' login");
        let exe = Path::new("C:\\Users\\Ana\\AppData\\Local\\agy\\bin\\agy.exe");
        assert_eq!(comando_de_terminal(exe, "acp --login", true), "& 'C:\\Users\\Ana\\AppData\\Local\\agy\\bin\\agy.exe' acp --login");
        assert_eq!(comando_de_terminal(Path::new("C:\\o'n\\codex.exe"), "login", true), "& 'C:\\o''n\\codex.exe' login");
    }

    #[test]
    fn a_resposta_json_do_agy_update_vira_a_mensagem() {
        let json = |ok: bool, msg: &str| Atualizacao { ok, linhas: vec![format!(r#"{{"success":{ok},"message":"{msg}"}}"#)] };
        let em_dia = limpar_resposta(json(true, "Already on the latest version."));
        assert_eq!((em_dia.ok, em_dia.linhas), (true, vec!["Already on the latest version.".to_string()]));
        // `success: false` falha mesmo com o processo saindo com 0.
        let erro = limpar_resposta(Atualizacao { ok: true, linhas: vec![r#"{"success":false,"message":"no network"}"#.into()] });
        assert_eq!((erro.ok, erro.linhas), (false, vec!["no network".to_string()]));
        // Texto comum passa como veio.
        let texto = Atualizacao { ok: true, linhas: vec!["Updated to 1.5.17".into()] };
        assert_eq!(limpar_resposta(texto.clone()), texto);
    }
}
