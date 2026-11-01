//! Conversas do Claude Code (a extensão do VS Code e o terminal) trazidas para o Vitral. Cada sessão
//! é um `<id>.jsonl` em `projects/<cwd codificado>/` da pasta do Claude Code, que aqui só é lido,
//! nunca escrito. A listagem tira de cada sessão o id, a pasta (`cwd`), o título, as datas, quantas
//! mensagens tem e se ela está aberta agora; o histórico que o chat mostra é montado sob demanda, em
//! segundo plano, e não vai para o estado do Vitral.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::modelo::{Forma, Item, PastaLigada, Projeto};

/// Arquivo mudado há menos que isso conta como conversa aberta no Claude Code.
pub const EM_USO_SEGUNDOS: u64 = 300;
/// Quantos itens do histórico aparecem de cada vez ("Carregar mais" traz outros tantos).
pub const ITENS_POR_VEZ: usize = 300;
/// O histórico guarda no máximo isso de itens, os mais novos.
const MAX_ITENS: usize = 6000;
/// O resultado de uma ferramenta no histórico fica até aqui.
const MAX_RESULTADO: usize = 4000;
/// O `criado_por` dos chats que vieram do Claude Code.
pub const CRIADO_POR: &str = "importacao";
/// Onde o registro fica no `estado.json` (entre os campos que o `Estado` não conhece).
const CHAVE_REGISTRO: &str = "claude_code";
/// Os marcos da conversa (o começo de cada um, que a interface reconhece).
pub const MARCO: &str = "Trazida do Claude Code";

/// A sessão do Claude Code que um chat continua, gravada no chat.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Importada {
    /// O id da sessão original.
    pub sessao: String,
    pub arquivo: PathBuf,
    /// "VS Code" ou "terminal" (vazio quando não se sabe).
    pub origem: String,
    pub primeira: u64,
    pub ultima: u64,
    pub mensagens: usize,
    /// A última coisa dita lá, para a prévia do cartão.
    pub ultima_fala: String,
    /// Estava aberta no Claude Code quando foi trazida.
    pub em_uso: bool,
    pub importada_em: u64,
    /// A primeira vez no Vitral já saiu, numa cópia da sessão (`--fork-session`): as próximas
    /// retomam a sessão nova, e o histórico para em `ate` (o tamanho do arquivo original nessa hora).
    pub continuada: bool,
    pub ate: u64,
}

impl Importada {
    /// Daqui em diante a conversa é a do Vitral: o histórico para no tamanho de agora.
    pub fn marcar_continuada(&mut self) {
        if !self.continuada {
            self.ate = fs::metadata(&self.arquivo).map(|m| m.len()).unwrap_or(0);
            self.continuada = true;
        }
    }

    /// Até onde o histórico lê o arquivo original (`None`: inteiro, ainda acompanhando o de lá).
    pub fn limite(&self) -> Option<u64> {
        (self.continuada && self.ate > 0).then_some(self.ate)
    }
}

/// A sessão de onde a primeira vez no Vitral continua (`--resume <id> --fork-session`): enquanto a
/// conversa ainda não saiu do Claude Code e o arquivo dela existe.
pub fn bifurcar_de(importada: Option<&Importada>, sessao_criada: bool) -> Option<String> {
    let i = importada?;
    (!i.continuada && !sessao_criada && i.arquivo.is_file()).then(|| i.sessao.clone())
}

/// Uma sessão nova para trazer.
#[derive(Clone, Debug, PartialEq)]
pub struct Sessao {
    pub id: String,
    pub arquivo: PathBuf,
    pub cwd: PathBuf,
    pub titulo: String,
    pub origem: String,
    pub primeira: u64,
    pub ultima: u64,
    pub mensagens: usize,
    pub ultima_fala: String,
    pub em_uso: bool,
}

impl Sessao {
    pub fn importada(&self, agora: u64) -> Importada {
        Importada {
            sessao: self.id.clone(),
            arquivo: self.arquivo.clone(),
            origem: self.origem.clone(),
            primeira: self.primeira,
            ultima: self.ultima,
            mensagens: self.mensagens,
            ultima_fala: self.ultima_fala.clone(),
            em_uso: self.em_uso,
            importada_em: agora,
            continuada: false,
            ate: 0,
        }
    }
}

/// O que a leitura de uma sessão achou, antes dos filtros.
#[derive(Debug, Default, PartialEq)]
pub struct Lida {
    pub cwd: Option<PathBuf>,
    pub titulo: String,
    /// O `entrypoint` (claude-vscode, cli, sdk-cli…).
    pub entrada: String,
    pub primeira: u64,
    pub ultima: u64,
    pub mensagens: usize,
    pub ultima_fala: String,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Linha {
    #[serde(rename = "type", deserialize_with = "ou_padrao")]
    tipo: String,
    #[serde(deserialize_with = "ou_padrao")]
    is_sidechain: bool,
    #[serde(deserialize_with = "ou_padrao")]
    is_meta: bool,
    #[serde(deserialize_with = "ou_padrao")]
    is_compact_summary: bool,
    #[serde(deserialize_with = "ou_padrao")]
    is_api_error_message: bool,
    #[serde(deserialize_with = "ou_padrao")]
    cwd: Option<PathBuf>,
    timestamp: Value,
    #[serde(deserialize_with = "ou_padrao")]
    entrypoint: Option<String>,
    #[serde(deserialize_with = "ou_padrao")]
    message: Option<Mensagem>,
    #[serde(deserialize_with = "ou_padrao")]
    summary: Option<String>,
    #[serde(deserialize_with = "ou_padrao")]
    ai_title: Option<String>,
    #[serde(deserialize_with = "ou_padrao")]
    custom_title: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Mensagem {
    #[serde(deserialize_with = "ou_padrao")]
    id: Option<String>,
    content: Value,
}

/// Um campo com o tipo que esta versão não espera (um `null`, um formato mais novo) fica no padrão,
/// sem perder a linha.
fn ou_padrao<'de, D: serde::Deserializer<'de>, T: Default + serde::de::DeserializeOwned>(d: D) -> Result<T, D::Error> {
    Ok(serde_json::from_value(Value::deserialize(d)?).unwrap_or_default())
}

/// As linhas que dá para ler (a estragada, a de outro formato e a cortada no meio ficam de fora). As
/// gravações das chamadas à API (`api-request*`), que são as maiores, nem passam pelo JSON.
fn linhas(leitor: impl BufRead) -> impl Iterator<Item = Linha> {
    let mut leitor = leitor;
    let mut buffer = Vec::new();
    std::iter::from_fn(move || {
        loop {
            buffer.clear();
            match leitor.read_until(b'\n', &mut buffer) {
                Ok(0) | Err(_) => return None,
                Ok(_) => {}
            }
            if buffer.starts_with(br#"{"type":"api-request"#) || buffer.starts_with(br#"{"type":"file-history"#) {
                continue;
            }
            if let Ok(linha) = serde_json::from_slice::<Linha>(&buffer) {
                return Some(linha);
            }
        }
    })
}

/// O que uma linha do usuário traz.
#[derive(Debug, PartialEq)]
enum Fala {
    Mensagem(String),
    /// Um comando de barra ("/clear") ou do terminal ("!ls").
    Comando(String),
    Interrompido,
    Nada,
}

struct DoUsuario {
    fala: Fala,
    /// (id da chamada, texto, erro).
    resultados: Vec<(String, String, bool)>,
}

/// Os marcadores que o Claude Code põe nas mensagens do usuário e que não são fala dele.
const MARCADORES_FORA: [&str; 11] = [
    "<local-command-",
    "<task-notification",
    "<system-reminder",
    "<bash-stdout",
    "<bash-stderr",
    "<ide_opened_file",
    "<ide_selection",
    "<ide_diagnostics",
    "<user-memory-input",
    "<command-message",
    "<command-args",
];

fn entre<'a>(texto: &'a str, abre: &str, fecha: &str) -> Option<&'a str> {
    let inicio = texto.find(abre)? + abre.len();
    let fim = texto[inicio..].find(fecha)? + inicio;
    Some(texto[inicio..fim].trim())
}

/// Um pedaço de texto do usuário: fala, comando, interrupção ou marcador do Claude Code.
fn classificar(texto: &str) -> Fala {
    let t = texto.trim();
    if t.is_empty() {
        return Fala::Nada;
    }
    if t.starts_with("<command-name>") {
        let nome = entre(t, "<command-name>", "</command-name>").unwrap_or_default();
        let nome = if nome.starts_with('/') { nome.to_string() } else { format!("/{nome}") };
        return if nome.len() > 1 { Fala::Comando(nome) } else { Fala::Nada };
    }
    if t.starts_with("<bash-input>") {
        let comando = entre(t, "<bash-input>", "</bash-input>").unwrap_or_default();
        return if comando.is_empty() { Fala::Nada } else { Fala::Comando(format!("!{comando}")) };
    }
    if MARCADORES_FORA.iter().any(|m| t.starts_with(m)) {
        return Fala::Nada;
    }
    if t.starts_with("[Request interrupted") {
        return Fala::Interrompido;
    }
    Fala::Mensagem(t.to_string())
}

fn do_usuario(conteudo: &Value) -> DoUsuario {
    let mut resultados = Vec::new();
    let mut textos: Vec<String> = Vec::new();
    let mut fala = Fala::Nada;
    let mut imagens = 0;
    let mut juntar = |f: Fala, fala: &mut Fala| match f {
        Fala::Mensagem(t) => textos.push(t),
        Fala::Nada => {}
        outra if *fala == Fala::Nada => *fala = outra,
        _ => {}
    };
    match conteudo {
        Value::String(texto) => juntar(classificar(texto), &mut fala),
        Value::Array(blocos) => {
            for bloco in blocos {
                match bloco["type"].as_str().unwrap_or_default() {
                    "text" => juntar(classificar(bloco["text"].as_str().unwrap_or_default()), &mut fala),
                    "image" => imagens += 1,
                    "tool_result" => resultados.push((
                        bloco["tool_use_id"].as_str().unwrap_or_default().to_string(),
                        texto_do_resultado(&bloco["content"]),
                        bloco["is_error"].as_bool().unwrap_or(false),
                    )),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    if !textos.is_empty() || imagens > 0 {
        let mut texto = textos.join("\n\n");
        if imagens > 0 {
            let marca = if imagens == 1 { "(1 imagem)".to_string() } else { format!("({imagens} imagens)") };
            texto = if texto.is_empty() { marca } else { format!("{texto}\n\n{marca}") };
        }
        fala = Fala::Mensagem(texto);
    }
    DoUsuario { fala, resultados }
}

fn texto_do_resultado(conteudo: &Value) -> String {
    match conteudo {
        Value::String(s) => s.clone(),
        Value::Array(partes) => partes.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        Value::Null => String::new(),
        outro => outro.to_string(),
    }
}

/// Segundos Unix de um `timestamp` (ISO 8601, ou milissegundos).
fn segundos(valor: &Value) -> Option<u64> {
    match valor {
        Value::String(s) => chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp().max(0) as u64),
        Value::Number(n) => n.as_u64().map(|ms| if ms > 100_000_000_000 { ms / 1000 } else { ms }),
        _ => None,
    }
}

fn primeira_linha(texto: &str) -> String {
    texto.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string()
}

/// O texto sem as marcas `<...>`, numa linha, cortado numa palavra.
fn titulo_de(texto: &str, limite: usize) -> String {
    let mut limpo = String::new();
    let mut resto = texto;
    while let Some(abre) = resto.find('<') {
        limpo.push_str(&resto[..abre]);
        resto = &resto[abre..];
        let marca = resto[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/');
        match resto.find('>') {
            Some(fecha) if marca && fecha <= 120 && !resto[1..fecha].contains('<') => {
                limpo.push(' ');
                resto = &resto[fecha + 1..];
            }
            _ => {
                limpo.push('<');
                resto = &resto[1..];
            }
        }
    }
    limpo.push_str(resto);
    let palavras: Vec<&str> = limpo.split_whitespace().collect();
    let mut titulo = String::new();
    for palavra in palavras {
        let tamanho = titulo.chars().count() + palavra.chars().count() + usize::from(!titulo.is_empty());
        if tamanho > limite {
            if titulo.is_empty() {
                titulo = palavra.chars().take(limite).collect();
            }
            titulo.push('…');
            return titulo;
        }
        if !titulo.is_empty() {
            titulo.push(' ');
        }
        titulo.push_str(palavra);
    }
    titulo
}

fn origem_de(entrada: &str) -> String {
    match entrada {
        "claude-vscode" => "VS Code".into(),
        "cli" => "terminal".into(),
        _ => String::new(),
    }
}

/// Lê uma sessão inteira (sem guardar as mensagens): pasta, título, datas e quantas mensagens.
/// Título: o que o usuário deu (`/rename`), depois o resumo do Claude Code (o `summary` antigo, depois
/// o `ai-title`), depois o começo da primeira mensagem.
pub fn ler_sessao(arquivo: &Path) -> std::io::Result<Lida> {
    let leitor = BufReader::with_capacity(1 << 16, fs::File::open(arquivo)?);
    let mut lida = Lida::default();
    let (mut proprio, mut resumo, mut gerado, mut primeira_fala) = (None, None, None, None);
    let mut respostas: HashSet<String> = HashSet::new();
    let mut sem_id = 0;
    let mut usuario = 0;
    let marcar = |l: &Linha, lida: &mut Lida| {
        if let Some(s) = segundos(&l.timestamp) {
            if lida.primeira == 0 || s < lida.primeira {
                lida.primeira = s;
            }
            lida.ultima = lida.ultima.max(s);
        }
    };
    for linha in linhas(leitor) {
        let limpo = |t: &Option<String>| t.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
        match linha.tipo.as_str() {
            "custom-title" => proprio = limpo(&linha.custom_title).or(proprio),
            "summary" => resumo = limpo(&linha.summary).or(resumo),
            "ai-title" => gerado = limpo(&linha.ai_title).or(gerado),
            _ => {}
        }
        if linha.is_sidechain || !matches!(linha.tipo.as_str(), "user" | "assistant") {
            continue;
        }
        if lida.cwd.is_none() {
            lida.cwd = linha.cwd.clone().filter(|c| !c.as_os_str().is_empty());
        }
        if lida.entrada.is_empty()
            && let Some(e) = &linha.entrypoint
        {
            lida.entrada = e.clone();
        }
        let Some(mensagem) = &linha.message else {
            continue;
        };
        if linha.tipo == "user" {
            if linha.is_meta || linha.is_compact_summary {
                continue;
            }
            if let Fala::Mensagem(texto) = do_usuario(&mensagem.content).fala {
                usuario += 1;
                primeira_fala.get_or_insert_with(|| texto.clone());
                lida.ultima_fala = format!("Você: {}", primeira_linha(&texto));
                marcar(&linha, &mut lida);
            }
            continue;
        }
        let texto: Vec<&str> = mensagem
            .content
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .filter(|t| !t.trim().is_empty())
            .collect();
        if texto.is_empty() || linha.is_api_error_message {
            continue;
        }
        match &mensagem.id {
            Some(id) => {
                respostas.insert(id.clone());
            }
            None => sem_id += 1,
        }
        lida.ultima_fala = primeira_linha(texto.last().unwrap_or(&""));
        marcar(&linha, &mut lida);
    }
    lida.mensagens = usuario + respostas.len() + sem_id;
    lida.titulo = proprio
        .or(resumo)
        .or(gerado)
        .map(|t| titulo_de(&t, 80))
        .or_else(|| primeira_fala.map(|t| titulo_de(&t, 60)))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Conversa do Claude Code".into());
    lida.ultima_fala = titulo_de(&lida.ultima_fala, 120);
    Ok(lida)
}

/// O histórico de uma sessão como itens da conversa (fala do usuário, texto do assistente,
/// ferramentas resumidas com o resultado cortado, comandos e interrupções como avisos), até `ate`
/// bytes do arquivo. Guarda os `MAX_ITENS` mais novos; `cortados` diz quantos ficaram de fora.
#[derive(Debug, Default)]
pub struct Historico {
    pub itens: Vec<Item>,
    pub cortados: usize,
}

pub fn historico(arquivo: &Path, ate: Option<u64>) -> std::io::Result<Historico> {
    let arquivo = fs::File::open(arquivo)?;
    let leitor: Box<dyn Read> = match ate {
        Some(limite) => Box::new(arquivo.take(limite)),
        None => Box::new(arquivo),
    };
    Ok(historico_de(BufReader::with_capacity(1 << 16, leitor)))
}

fn historico_de(leitor: impl BufRead) -> Historico {
    let mut itens: Vec<Item> = Vec::new();
    // As chamadas pelo id, com o índice contado desde o começo (os cortados saem da frente).
    let mut chamadas: HashMap<String, usize> = HashMap::new();
    let mut cortados = 0;
    let mut resposta: Option<String> = None;
    let aviso = |texto: &str, erro: bool| Item::Aviso { texto: texto.to_string(), erro };
    for linha in linhas(leitor) {
        if linha.is_sidechain {
            continue;
        }
        let Some(mensagem) = linha.message else {
            continue;
        };
        match linha.tipo.as_str() {
            "user" if linha.is_compact_summary => {
                itens.push(aviso("A conversa foi resumida aqui (o Claude Code compactou o contexto).", false));
                resposta = None;
            }
            "user" if !linha.is_meta => {
                let u = do_usuario(&mensagem.content);
                for (id, texto, erro) in u.resultados {
                    if let Some(Item::Ferramenta { resultado, erro: e, .. }) =
                        chamadas.get(&id).and_then(|&i| i.checked_sub(cortados)).and_then(|i| itens.get_mut(i))
                    {
                        *resultado = Some(crate::git::inicio_do_texto(&texto, MAX_RESULTADO));
                        *e = erro;
                    }
                }
                match u.fala {
                    Fala::Mensagem(texto) => itens.push(Item::Usuario {
                        texto,
                        anexos: Vec::new(),
                        ponto: None,
                        ultrathink: false,
                        ultracode: false,
                    }),
                    Fala::Comando(comando) => itens.push(aviso(&format!("Comando {comando}"), false)),
                    Fala::Interrompido => itens.push(aviso("Interrompido pelo usuário.", false)),
                    Fala::Nada => continue,
                }
                resposta = None;
            }
            "assistant" if linha.is_api_error_message => {
                let texto = mensagem.content.as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n");
                itens.push(aviso(&format!("Erro do Claude Code: {}", primeira_linha(&texto)), true));
                resposta = None;
            }
            "assistant" => {
                for bloco in mensagem.content.as_array().into_iter().flatten() {
                    match bloco["type"].as_str().unwrap_or_default() {
                        "text" => {
                            let texto = bloco["text"].as_str().unwrap_or_default().trim();
                            if texto.is_empty() {
                                continue;
                            }
                            // Pedaços seguidos da mesma resposta viram um texto só.
                            if let (Some(atual), Some(id)) = (&resposta, &mensagem.id)
                                && atual == id
                                && let Some(Item::Assistente { texto: anterior }) = itens.last_mut()
                            {
                                anterior.push_str("\n\n");
                                anterior.push_str(texto);
                                continue;
                            }
                            itens.push(Item::Assistente { texto: texto.to_string() });
                            resposta = mensagem.id.clone();
                        }
                        "tool_use" => {
                            let nome = bloco["name"].as_str().unwrap_or_default();
                            if nome.is_empty() || nome == "ToolSearch" || nome == "mcp__vitral__permissao" {
                                continue;
                            }
                            let id = bloco["id"].as_str().unwrap_or_default().to_string();
                            chamadas.insert(id.clone(), cortados + itens.len());
                            itens.push(Item::Ferramenta {
                                id,
                                nome: nome.to_string(),
                                resumo: crate::claude::resumo_ferramenta(nome, &bloco["input"]),
                                resultado: None,
                                erro: false,
                            });
                            resposta = None;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        // Sem deixar crescer sem fim: os mais antigos saem aos blocos.
        if itens.len() >= 2 * MAX_ITENS {
            let sobra = itens.len() - MAX_ITENS;
            itens.drain(..sobra);
            cortados += sobra;
        }
    }
    if itens.len() > MAX_ITENS {
        let sobra = itens.len() - MAX_ITENS;
        itens.drain(..sobra);
        cortados += sobra;
    }
    Historico { itens, cortados }
}

/// A pasta do Claude Code: `CLAUDE_CONFIG_DIR` ou `~/.claude`. Na bancada, só com
/// `CLAUDE_CONFIG_DIR`: as conversas de verdade nunca entram nela.
pub fn pasta_do_claude() -> Option<PathBuf> {
    if let Some(pasta) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(pasta));
    }
    if na_bancada() {
        return None;
    }
    dirs::home_dir().map(|h| h.join(".claude"))
}

fn na_bancada() -> bool {
    std::env::var_os("DENTRO_DA_BANCADA").is_some_and(|v| v == "1")
}

/// De onde ler: a pasta do Claude Code e a de cada conta extra (que ligam o mesmo `projects`; a
/// listagem não repete). Na bancada, só a primeira.
pub fn raizes(contas: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let Some(principal) = pasta_do_claude() else {
        return Vec::new();
    };
    let mut raizes = vec![principal];
    if !na_bancada() {
        raizes.extend(contas);
    }
    raizes
}

/// As pastas `projects` das raízes, cada uma uma vez só (um link simbólico conta como a pasta dele).
pub fn pastas_de_projects(raizes: &[PathBuf]) -> Vec<PathBuf> {
    let mut vistas = HashSet::new();
    raizes
        .iter()
        .filter_map(|r| dunce::canonicalize(r.join("projects")).ok())
        .filter(|p| p.is_dir() && vistas.insert(p.clone()))
        .collect()
}

/// As pastas onde o que roda não é conversa do usuário: as temporárias (testes) e a de dados do
/// Vitral (os chats dele).
pub fn pastas_de_fora(dados: &Path) -> Vec<PathBuf> {
    let mut pastas = vec![PathBuf::from("/tmp"), PathBuf::from("/var/tmp"), std::env::temp_dir(), dados.to_path_buf()];
    let canonicas: Vec<PathBuf> = pastas.iter().filter_map(|p| dunce::canonicalize(p).ok()).collect();
    pastas.extend(canonicas);
    pastas.sort();
    pastas.dedup();
    pastas
}

/// Os ids de sessão que algum `claude` aberto agora retoma (`--resume`, `-r`, `--session-id`).
pub fn sessoes_abertas(proc: &Path) -> HashSet<String> {
    let Ok(entradas) = fs::read_dir(proc) else {
        return HashSet::new();
    };
    entradas
        .flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| n.parse::<u32>().is_ok()))
        .filter_map(|e| fs::read(e.path().join("cmdline")).ok())
        .flat_map(|bytes| {
            let args: Vec<String> = bytes
                .split(|&b| b == 0)
                .filter(|a| !a.is_empty())
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect();
            sessoes_do_comando(&args)
        })
        .collect()
}

/// As sessões de uma linha de comando, se ela é a de um `claude`.
pub fn sessoes_do_comando(args: &[String]) -> Vec<String> {
    let e_claude = args.iter().take(2).any(|a| {
        Path::new(a).file_name().is_some_and(|n| n.to_string_lossy().starts_with("claude")) || a.contains("claude-code")
    });
    if !e_claude {
        return Vec::new();
    }
    let mut ids = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let valor = match a.split_once('=') {
            Some(("--resume" | "--session-id", v)) => Some(v),
            _ if matches!(a.as_str(), "--resume" | "-r" | "--session-id") => args.get(i + 1).map(String::as_str),
            _ => None,
        };
        if let Some(v) = valor.filter(|v| uuid::Uuid::parse_str(v).is_ok()) {
            ids.push(v.to_string());
        }
    }
    ids
}

/// O que a listagem não traz.
pub struct Filtro<'a> {
    /// Já trazidas (mesmo as de chats apagados depois).
    pub trazidas: &'a HashSet<String>,
    /// As sessões dos chats do próprio Vitral (inclusive a cópia de cada conversa que continuou nele).
    pub do_vitral: &'a HashSet<String>,
    pub de_fora: &'a [PathBuf],
    pub abertas: &'a HashSet<String>,
    pub agora: u64,
}

#[derive(Debug, Default, PartialEq)]
pub struct Listagem {
    /// Da mais antiga para a mais nova.
    pub novas: Vec<Sessao>,
    pub ja_trazidas: usize,
    /// Sem mensagem nenhuma (só um `/clear`), ou ilegíveis.
    pub vazias: usize,
    /// Numa pasta temporária ou na do Vitral, de um chat do Vitral ou de um programa (`claude -p`).
    pub de_teste: usize,
    /// A pasta da conversa não existe mais (ou o disco não está montado).
    pub sem_pasta: usize,
}

/// As sessões de todas as pastas `projects` (cada id uma vez), com os filtros.
pub fn listar(pastas: &[PathBuf], filtro: &Filtro) -> Listagem {
    let mut l = Listagem::default();
    let mut vistas: HashSet<String> = HashSet::new();
    let filhos = |p: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).collect();
        v.sort();
        v
    };
    for pasta in pastas {
        for projeto in filhos(pasta).into_iter().filter(|p| p.is_dir()) {
            for arquivo in filhos(&projeto) {
                let id = match (arquivo.extension(), arquivo.file_stem().and_then(|s| s.to_str())) {
                    (Some(e), Some(id)) if e == "jsonl" && uuid::Uuid::parse_str(id).is_ok() && arquivo.is_file() => id.to_string(),
                    _ => continue,
                };
                if !vistas.insert(id.clone()) {
                    continue;
                }
                if filtro.trazidas.contains(&id) {
                    l.ja_trazidas += 1;
                    continue;
                }
                if filtro.do_vitral.contains(&id) {
                    l.de_teste += 1;
                    continue;
                }
                let Ok(lida) = ler_sessao(&arquivo) else {
                    l.vazias += 1;
                    continue;
                };
                let Some(cwd) = lida.cwd.clone().filter(|_| lida.mensagens > 0) else {
                    l.vazias += 1;
                    continue;
                };
                let canonica = dunce::canonicalize(&cwd).ok();
                let de_fora = |c: &Path| filtro.de_fora.iter().any(|f| c.starts_with(f));
                if lida.entrada.starts_with("sdk") || de_fora(&cwd) || canonica.as_deref().is_some_and(de_fora) {
                    l.de_teste += 1;
                    continue;
                }
                if !cwd.is_dir() {
                    l.sem_pasta += 1;
                    continue;
                }
                let mudou = fs::metadata(&arquivo)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs());
                let em_uso = filtro.abertas.contains(&id) || filtro.agora.saturating_sub(mudou) < EM_USO_SEGUNDOS;
                l.novas.push(Sessao {
                    id,
                    arquivo,
                    cwd,
                    titulo: lida.titulo,
                    origem: origem_de(&lida.entrada),
                    primeira: lida.primeira,
                    ultima: lida.ultima.max(lida.primeira),
                    mensagens: lida.mensagens,
                    ultima_fala: lida.ultima_fala,
                    em_uso,
                });
            }
        }
    }
    l.novas.sort_by(|a, b| (a.primeira, &a.id).cmp(&(b.primeira, &b.id)));
    l
}

/// O projeto (e a pasta ligada) que contém a pasta da conversa: a ligada mais funda; empatadas, a
/// de um projeto que não está arquivado. `None` é Avulsos.
pub fn destino<'a>(cwd: &Path, projetos: &'a [Projeto]) -> Option<(&'a Projeto, &'a PastaLigada)> {
    projetos
        .iter()
        .flat_map(|p| p.pastas.iter().map(move |l| (p, l)))
        .filter(|(_, l)| !l.caminho.as_os_str().is_empty() && cwd.starts_with(&l.caminho))
        .max_by_key(|(p, l)| (l.caminho.components().count(), !p.arquivado))
}

/// O chat trazido trabalha direto na pasta da conversa (o Claude Code só retoma a sessão dela): com
/// um commit por vez quando ela é a pasta própria do projeto; nas outras, sem mexer no git.
pub fn forma(cwd: &Path, pasta: Option<&PastaLigada>) -> Forma {
    match pasta {
        Some(l) if l.propria && l.caminho == cwd => Forma::DiretaGit,
        _ => Forma::Direta,
    }
}

/// O que já foi trazido, guardado no `estado.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Registro {
    pub importadas: Vec<String>,
    /// A importação da primeira abertura ainda não terminou (uma queda no meio a refaz).
    pub pendente: bool,
    pub ultima_vez: u64,
}

impl Registro {
    pub fn de(resto: &Map<String, Value>) -> Option<Registro> {
        resto.get(CHAVE_REGISTRO).and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn gravar(&self, resto: &mut Map<String, Value>) {
        if let Ok(valor) = serde_json::to_value(self) {
            resto.insert(CHAVE_REGISTRO.into(), valor);
        }
    }
}

/// O resultado de uma importação, para o aviso.
#[derive(Debug, Default, PartialEq)]
pub struct Relatorio {
    pub trazidas: usize,
    /// (nome do destino, quantas), na ordem em que apareceram.
    pub por_destino: Vec<(String, usize)>,
    pub abertas: usize,
    pub ja_trazidas: usize,
}

impl Relatorio {
    pub fn contar(&mut self, destino: &str, aberta: bool) {
        self.trazidas += 1;
        self.abertas += usize::from(aberta);
        match self.por_destino.iter_mut().find(|(d, _)| d == destino) {
            Some((_, n)) => *n += 1,
            None => self.por_destino.push((destino.to_string(), 1)),
        }
    }

    /// "42 conversas do Claude Code importadas · 39 em Avulsos · 3 em loja". Sem nada novo, a
    /// importação da abertura fica calada e a do menu diz que não havia.
    pub fn texto(&self, automatica: bool) -> Option<String> {
        if self.trazidas == 0 {
            return (!automatica).then(|| match self.ja_trazidas {
                0 => "Nenhuma conversa do Claude Code para trazer.".to_string(),
                1 => "Nenhuma conversa nova do Claude Code (1 já está no Vitral).".to_string(),
                n => format!("Nenhuma conversa nova do Claude Code ({n} já estão no Vitral)."),
            });
        }
        let mut partes = vec![match self.trazidas {
            1 => "1 conversa do Claude Code importada".to_string(),
            n => format!("{n} conversas do Claude Code importadas"),
        }];
        let mut destinos = self.por_destino.clone();
        destinos.sort_by(|a, b| b.1.cmp(&a.1));
        partes.extend(destinos.iter().map(|(nome, n)| format!("{n} em {nome}")));
        match self.abertas {
            0 => {}
            1 => partes.push("1 aberta agora no Claude Code".into()),
            n => partes.push(format!("{n} abertas agora no Claude Code")),
        }
        Some(partes.join(" · "))
    }
}

/// O marco que fica na conversa, entre o histórico e o que vem no Vitral.
pub fn marco(i: &Importada) -> String {
    let quando = crate::modelo::data_curta(i.importada_em);
    match i.origem.as_str() {
        "" => format!("{MARCO} em {quando}"),
        origem => format!("{MARCO} ({origem}) em {quando}"),
    }
}

pub const AVISO_EM_USO: &str = "Esta conversa estava aberta no Claude Code quando foi trazida. A primeira \
mensagem daqui continua numa cópia dela; a de lá segue como está.";

/// O que "Este chat" diz da conversa trazida.
pub fn descricao(i: &Importada, pasta: &str) -> String {
    let de = match i.origem.as_str() {
        "" => "do Claude Code".to_string(),
        origem => format!("do Claude Code ({origem})"),
    };
    let datas = match (crate::modelo::data_curta(i.primeira), crate::modelo::data_curta(i.ultima)) {
        (a, b) if a == b => format!("de {a}"),
        (a, b) => format!("de {a} a {b}"),
    };
    let mensagens = if i.mensagens == 1 { "1 mensagem".to_string() } else { format!("{} mensagens", i.mensagens) };
    let mut texto = format!(
        "Continua uma conversa {de} {datas}, com {mensagens}. Mexe direto em {pasta}, como lá: sem cópia nem Juntar."
    );
    if !i.continuada {
        texto.push_str(" A primeira mensagem daqui continua numa cópia da sessão; a original segue no Claude Code como está.");
    }
    texto
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::modelo::novo_id;

    fn pasta_de_teste(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-importar-{nome}-{}", novo_id()));
        fs::create_dir_all(&pasta).unwrap();
        dunce::canonicalize(pasta).unwrap()
    }

    const S1: &str = "11111111-1111-4111-8111-111111111111";
    const S2: &str = "22222222-2222-4222-8222-222222222222";
    const S3: &str = "33333333-3333-4333-8333-333333333333";
    const S4: &str = "44444444-4444-4444-8444-444444444444";
    const S5: &str = "55555555-5555-4555-8555-555555555555";
    #[cfg_attr(not(unix), allow(dead_code, reason = "só o teste do link da conta usa"))]
    const S6: &str = "66666666-6666-4666-8666-666666666666";

    fn usuario(cwd: &Path, quando: &str, conteudo: Value) -> String {
        serde_json::json!({
            "parentUuid": null, "isSidechain": false, "type": "user", "entrypoint": "claude-vscode",
            "cwd": cwd, "timestamp": quando, "message": {"role": "user", "content": conteudo}
        })
        .to_string()
    }

    fn assistente(cwd: &Path, quando: &str, id: &str, blocos: Value) -> String {
        serde_json::json!({
            "isSidechain": false, "type": "assistant", "entrypoint": "claude-vscode", "cwd": cwd,
            "timestamp": quando, "message": {"id": id, "role": "assistant", "content": blocos}
        })
        .to_string()
    }

    /// Uma sessão do formato de hoje: título gerado, comando, imagem, ferramenta com resultado,
    /// resposta em dois pedaços, subagente e uma linha estragada no meio.
    fn sessao_de_hoje(cwd: &Path) -> String {
        [
            r#"{"type":"mode","mode":"normal","sessionId":"x"}"#.to_string(),
            usuario(cwd, "2026-10-19T03:46:09.840Z", serde_json::json!("<local-command-caveat>Caveat</local-command-caveat>")),
            usuario(cwd, "2026-10-19T03:46:09.841Z", serde_json::json!("<command-name>/clear</command-name>\n<command-message>clear</command-message>")),
            usuario(cwd, "2026-10-19T03:47:00.000Z", serde_json::json!([
                {"type": "text", "text": "<ide_opened_file>/home/x/a.rs</ide_opened_file>"},
                {"type": "text", "text": "Arruma o <b>botão</b> da barra\nque está torto"},
                {"type": "image", "source": {"type": "base64", "data": "AAAA"}}
            ])),
            r#"{"type":"ai-title","aiTitle":"Botão torto","sessionId":"x"}"#.to_string(),
            assistente(cwd, "2026-10-19T03:47:05.000Z", "m1", serde_json::json!([{"type": "thinking", "thinking": "hm"}])),
            assistente(cwd, "2026-10-19T03:47:06.000Z", "m1", serde_json::json!([{"type": "text", "text": "Vou olhar."}])),
            assistente(cwd, "2026-10-19T03:47:07.000Z", "m1", serde_json::json!([{"type": "tool_use", "id": "t1", "name": "Read", "input": {"file_path": "/home/x/a.rs"}}])),
            "{\"type\":\"user\",\"message\":{\"content\":[estragada".to_string(),
            usuario(cwd, "2026-10-19T03:47:08.000Z", serde_json::json!([{"type": "tool_result", "tool_use_id": "t1", "content": "fn main() {}"}])),
            serde_json::json!({"isSidechain": true, "type": "assistant", "cwd": cwd, "timestamp": "2026-10-19T03:47:09.000Z",
                "message": {"id": "sub", "content": [{"type": "text", "text": "do subagente"}]}}).to_string(),
            assistente(cwd, "2026-10-19T03:47:10.000Z", "m2", serde_json::json!([{"type": "text", "text": "Pronto, está reto."}])),
            assistente(cwd, "2026-10-19T03:47:11.000Z", "m2", serde_json::json!([{"type": "text", "text": "Mais alguma coisa?"}])),
            usuario(cwd, "2026-10-19T03:48:00.000Z", serde_json::json!([{"type": "text", "text": "[Request interrupted by user]"}])),
            r#"{"type":"api-request-blob","message":{"role":"user","content":"enorme"}}"#.to_string(),
            r#"{"type":"last-prompt","lastPrompt":"x"}"#.to_string(),
        ]
        .join("\n")
            + "\n"
    }

    /// Uma sessão do formato antigo: a linha de resumo (`summary`) e o texto como string.
    fn sessao_antiga(cwd: &Path) -> String {
        [
            r#"{"type":"summary","summary":"Configurar o DNS da Cloudflare","leafUuid":"l1"}"#.to_string(),
            usuario(cwd, "2026-09-20T22:41:22.851Z", serde_json::json!("Troca o dns para o da cloudflare")),
            assistente(cwd, "2026-09-20T22:42:00.000Z", "a1", serde_json::json!([{"type": "text", "text": "Feito."}])),
        ]
        .join("\n")
    }

    fn so_clear(cwd: &Path) -> String {
        [
            usuario(cwd, "2026-10-19T15:01:46.051Z", serde_json::json!("<local-command-caveat>Caveat</local-command-caveat>")),
            usuario(cwd, "2026-10-19T15:01:46.040Z", serde_json::json!("<command-name>/clear</command-name>")),
        ]
        .join("\n")
    }

    fn gravar(projects: &Path, cwd: &Path, id: &str, conteudo: &str) -> PathBuf {
        let pasta = projects.join(cwd.to_string_lossy().replace(['/', '\\', ':', '.'], "-"));
        fs::create_dir_all(&pasta).unwrap();
        let arquivo = pasta.join(format!("{id}.jsonl"));
        fs::write(&arquivo, conteudo).unwrap();
        arquivo
    }

    #[test]
    fn le_o_formato_de_hoje() {
        let raiz = pasta_de_teste("hoje");
        let cwd = raiz.join("casa");
        let arquivo = raiz.join("s.jsonl");
        fs::write(&arquivo, sessao_de_hoje(&cwd)).unwrap();
        let lida = ler_sessao(&arquivo).unwrap();
        assert_eq!(lida.cwd.as_deref(), Some(cwd.as_path()));
        assert_eq!(lida.titulo, "Botão torto");
        assert_eq!(lida.entrada, "claude-vscode");
        // Uma fala do usuário (o resultado, o comando e a interrupção não contam) e duas respostas.
        assert_eq!(lida.mensagens, 3);
        assert_eq!(lida.primeira, segundos(&Value::from("2026-10-19T03:47:00.000Z")).unwrap());
        assert_eq!(lida.ultima, segundos(&Value::from("2026-10-19T03:47:11.000Z")).unwrap());
        assert_eq!(lida.ultima_fala, "Mais alguma coisa?");
        let h = historico(&arquivo, None).unwrap();
        let resumo: Vec<String> = h
            .itens
            .iter()
            .map(|i| match i {
                Item::Usuario { texto, .. } => format!("U:{texto}"),
                Item::Assistente { texto } => format!("A:{texto}"),
                Item::Ferramenta { resumo, resultado, .. } => format!("F:{resumo}={}", resultado.clone().unwrap_or_default()),
                Item::Aviso { texto, .. } => format!("V:{texto}"),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(
            resumo,
            [
                "V:Comando /clear",
                "U:Arruma o <b>botão</b> da barra\nque está torto\n\n(1 imagem)",
                "A:Vou olhar.",
                "F:Read · a.rs=fn main() {}",
                "A:Pronto, está reto.\n\nMais alguma coisa?",
                "V:Interrompido pelo usuário.",
            ]
        );
        assert_eq!(h.cortados, 0);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn formato_antigo_com_summary_e_titulo_pela_primeira_mensagem() {
        let raiz = pasta_de_teste("antigo");
        let cwd = raiz.join("casa");
        let arquivo = raiz.join("s.jsonl");
        fs::write(&arquivo, sessao_antiga(&cwd)).unwrap();
        let lida = ler_sessao(&arquivo).unwrap();
        assert_eq!(lida.titulo, "Configurar o DNS da Cloudflare");
        assert_eq!(lida.mensagens, 2);
        // Sem resumo nem título gerado, vale o começo da primeira mensagem, sem as marcas.
        let sem_resumo = sessao_antiga(&cwd).lines().skip(1).collect::<Vec<_>>().join("\n");
        fs::write(&arquivo, sem_resumo).unwrap();
        assert_eq!(ler_sessao(&arquivo).unwrap().titulo, "Troca o dns para o da cloudflare");
        // O título dado pelo usuário vence os outros.
        let com_nome = format!("{}\n{{\"type\":\"custom-title\",\"customTitle\":\"Projeto Facul\"}}", sessao_antiga(&cwd));
        fs::write(&arquivo, com_nome).unwrap();
        assert_eq!(ler_sessao(&arquivo).unwrap().titulo, "Projeto Facul");
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn campo_com_tipo_estranho_nao_perde_a_linha() {
        let raiz = pasta_de_teste("estranho");
        let arquivo = raiz.join("s.jsonl");
        let linha = r#"{"type":"user","isMeta":null,"isSidechain":"nao","cwd":"/c","entrypoint":7,"timestamp":1792381569836,"message":{"id":[1],"content":"oi"}}"#;
        fs::write(&arquivo, format!("{linha}\n{{\"type\":\"summary\",\"summary\":{{\"novo\":1}}}}\n")).unwrap();
        let lida = ler_sessao(&arquivo).unwrap();
        assert_eq!((lida.mensagens, lida.cwd.as_deref(), lida.titulo.as_str()), (1, Some(Path::new("/c")), "oi"));
        assert_eq!(lida.primeira, 1_792_381_569);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn titulo_tira_as_marcas_e_corta_numa_palavra() {
        assert_eq!(titulo_de("<pasted_content id=\"a4e4\">\nIr para o conteúdo", 60), "Ir para o conteúdo");
        assert_eq!(titulo_de("uma frase bem comprida que passa do limite", 20), "uma frase bem…");
        assert_eq!(titulo_de("a < b e c > d", 60), "a < b e c > d");
        assert_eq!(titulo_de("supercalifragilistico", 5), "super…");
    }

    // O link simbólico da conta extra é do Unix (as contas extras ainda não existem no Windows).
    #[cfg(unix)]
    #[test]
    fn listagem_filtra_e_nao_repete_pelo_link_da_conta() {
        let raiz = pasta_de_teste("listar");
        let casa = raiz.join("casa");
        let loja = casa.join("Projetos").join("loja");
        let teste = raiz.join("tmp");
        let dados = raiz.join("vitral");
        for p in [&loja, &teste, &dados] {
            fs::create_dir_all(p).unwrap();
        }
        let principal = raiz.join("claude");
        let projects = principal.join("projects");
        let a = gravar(&projects, &casa, S1, &sessao_de_hoje(&casa));
        gravar(&projects, &loja, S2, &sessao_antiga(&loja));
        gravar(&projects, &casa, S3, &so_clear(&casa));
        gravar(&projects, &raiz.join("sumiu"), S4, &sessao_antiga(&raiz.join("sumiu")));
        gravar(&projects, &teste, S5, &sessao_antiga(&teste));
        gravar(&projects, &dados.join("avulsos").join("c1"), S6, &sessao_antiga(&dados));
        // Lixo que não é sessão: a memória, um subagente antigo, um arquivo solto.
        fs::create_dir_all(projects.join("x").join("memory")).unwrap();
        fs::write(projects.join("x").join("agent-1234.jsonl"), sessao_antiga(&casa)).unwrap();
        fs::write(projects.join("solto.jsonl"), sessao_antiga(&casa)).unwrap();
        // A conta extra liga o mesmo `projects` por link simbólico.
        let conta = raiz.join("conta");
        fs::create_dir_all(&conta).unwrap();
        std::os::unix::fs::symlink(&projects, conta.join("projects")).unwrap();
        let pastas = pastas_de_projects(&[principal.clone(), conta.clone(), raiz.join("sem-claude")]);
        assert_eq!(pastas, vec![projects.clone()]);
        let nenhuma = HashSet::new();
        let abertas: HashSet<String> = [S2.to_string()].into();
        let de_fora = [teste.clone(), dados.clone()];
        let agora = crate::modelo::agora();
        let filtro = Filtro { trazidas: &nenhuma, do_vitral: &nenhuma, de_fora: &de_fora, abertas: &abertas, agora: agora + 3600 };
        let l = listar(&pastas, &filtro);
        let ids: Vec<&str> = l.novas.iter().map(|s| s.id.as_str()).collect();
        // A antiga (setembro) antes da de hoje (outubro).
        assert_eq!(ids, [S2, S1]);
        assert_eq!((l.vazias, l.de_teste, l.sem_pasta, l.ja_trazidas), (1, 2, 1, 0));
        let hoje = &l.novas[1];
        assert_eq!((hoje.arquivo.as_path(), hoje.cwd.as_path(), hoje.origem.as_str()), (a.as_path(), casa.as_path(), "VS Code"));
        // Aberta por um processo, ou mudada há pouco.
        assert!(l.novas[0].em_uso && !hoje.em_uso);
        let agora_mesmo = Filtro { agora, ..filtro };
        assert!(listar(&pastas, &agora_mesmo).novas.iter().all(|s| s.em_uso));
        // A segunda vez não traz de novo o que já veio, nem a sessão de um chat do Vitral.
        let trazidas: HashSet<String> = [S1.to_string()].into();
        let do_vitral: HashSet<String> = [S2.to_string()].into();
        let de_novo = listar(&pastas, &Filtro { trazidas: &trazidas, do_vitral: &do_vitral, ..filtro });
        assert!(de_novo.novas.is_empty());
        assert_eq!((de_novo.ja_trazidas, de_novo.de_teste), (1, 3));
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn sessao_vazia_e_arquivo_vazio_ficam_de_fora() {
        let raiz = pasta_de_teste("vazia");
        let projects = raiz.join("claude").join("projects");
        let cwd = raiz.join("casa");
        fs::create_dir_all(&cwd).unwrap();
        gravar(&projects, &cwd, S1, "");
        gravar(&projects, &cwd, S2, "não é json\n{}\n");
        gravar(&projects, &cwd, S3, &so_clear(&cwd));
        let nenhuma = HashSet::new();
        let filtro = Filtro { trazidas: &nenhuma, do_vitral: &nenhuma, de_fora: &[], abertas: &HashSet::new(), agora: 0 };
        let l = listar(&[projects], &filtro);
        assert!(l.novas.is_empty());
        assert_eq!(l.vazias, 3);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn sessoes_abertas_pela_linha_de_comando() {
        let s = |v: &[&str]| sessoes_do_comando(&v.iter().map(|a| a.to_string()).collect::<Vec<_>>());
        let vscode = "/home/v/.vscode/extensions/anthropic.claude-code-2.1.314-linux-x64/resources/native-binary/claude";
        assert_eq!(s(&[vscode, "--output-format", "stream-json", format!("--resume={S1}").as_str()]), [S1]);
        assert_eq!(s(&["claude", "-r", S2]), [S2]);
        assert_eq!(s(&["node", "/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js", "--resume", S3]), [S3]);
        assert_eq!(s(&["claude", "--session-id", S4, "--resume", "nao-e-uuid"]), [S4]);
        assert!(s(&["vim", "--resume", S1]).is_empty());
        // Um /proc falso.
        let raiz = pasta_de_teste("proc");
        for (pid, args) in [("10", vec![vscode, "--resume", S5]), ("11", vec!["bash"])] {
            fs::create_dir_all(raiz.join(pid)).unwrap();
            fs::write(raiz.join(pid).join("cmdline"), args.join("\0") + "\0").unwrap();
        }
        fs::create_dir_all(raiz.join("self")).unwrap();
        assert_eq!(sessoes_abertas(&raiz), [S5.to_string()].into());
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn destino_e_a_pasta_ligada_mais_funda() {
        let pasta = |c: &str, propria: bool| PastaLigada { caminho: PathBuf::from(c), propria, ..Default::default() };
        let projeto = |id: &str, pastas: Vec<PastaLigada>, arquivado: bool| Projeto {
            id: id.into(),
            nome: id.into(),
            pastas,
            arquivado,
            ..Default::default()
        };
        let projetos = vec![
            projeto("tudo", vec![pasta("/home/v/Projetos", false)], false),
            projeto("loja", vec![pasta("/home/v/Projetos/loja", false), pasta("/home/v/docs", false)], false),
            projeto("velho", vec![pasta("/home/v/Projetos/loja", false)], true),
            projeto("propria", vec![pasta("/dados/projetos/receitas", true)], false),
        ];
        let id = |cwd: &str| destino(Path::new(cwd), &projetos).map(|(p, _)| p.id.as_str());
        assert_eq!(id("/home/v/Projetos/loja/backend"), Some("loja"));
        assert_eq!(id("/home/v/Projetos/loja"), Some("loja"));
        assert_eq!(id("/home/v/Projetos/outro"), Some("tudo"));
        assert_eq!(id("/home/v/docs"), Some("loja"));
        assert_eq!(id("/home/v/Projetos-2"), None);
        assert_eq!(id("/home/v"), None);
        let (_, l) = destino(Path::new("/dados/projetos/receitas"), &projetos).unwrap();
        assert_eq!(forma(Path::new("/dados/projetos/receitas"), Some(l)), Forma::DiretaGit);
        assert_eq!(forma(Path::new("/dados/projetos/receitas/sub"), Some(l)), Forma::Direta);
        assert_eq!(forma(Path::new("/home/v"), None), Forma::Direta);
    }

    #[test]
    fn registro_no_resto_do_estado() {
        let mut resto = Map::new();
        resto.insert("outro".into(), Value::from(1));
        assert_eq!(Registro::de(&resto), None);
        let r = Registro { importadas: vec![S1.into()], pendente: true, ultima_vez: 9 };
        r.gravar(&mut resto);
        assert_eq!(Registro::de(&resto), Some(r));
        assert_eq!(resto["outro"], 1);
    }

    #[test]
    fn texto_do_relatorio() {
        let mut r = Relatorio::default();
        assert_eq!(r.texto(true), None);
        assert_eq!(r.texto(false).as_deref(), Some("Nenhuma conversa do Claude Code para trazer."));
        r.ja_trazidas = 40;
        assert_eq!(r.texto(false).as_deref(), Some("Nenhuma conversa nova do Claude Code (40 já estão no Vitral)."));
        r.contar("loja", false);
        for _ in 0..39 {
            r.contar("Avulsos", false);
        }
        r.contar("Avulsos", true);
        r.contar("Avulsos", true);
        assert_eq!(
            r.texto(true).as_deref(),
            Some("42 conversas do Claude Code importadas · 41 em Avulsos · 1 em loja · 2 abertas agora no Claude Code")
        );
        let mut um = Relatorio::default();
        um.contar("Avulsos", false);
        assert_eq!(um.texto(false).as_deref(), Some("1 conversa do Claude Code importada · 1 em Avulsos"));
    }

    #[test]
    fn historico_para_onde_a_conversa_saiu_e_guarda_os_mais_novos() {
        let raiz = pasta_de_teste("ate");
        let cwd = raiz.join("casa");
        let arquivo = raiz.join("s.jsonl");
        let antes = sessao_antiga(&cwd) + "\n";
        let depois = usuario(&cwd, "2026-09-21T10:00:00.000Z", serde_json::json!("continuei no VS Code"));
        fs::write(&arquivo, format!("{antes}{depois}\n")).unwrap();
        let mut i = Importada { sessao: S1.into(), arquivo: arquivo.clone(), ..Default::default() };
        assert_eq!(historico(&arquivo, i.limite()).unwrap().itens.len(), 3);
        assert_eq!(bifurcar_de(Some(&i), false).as_deref(), Some(S1));
        assert_eq!(bifurcar_de(Some(&i), true), None);
        fs::write(&arquivo, &antes).unwrap();
        i.marcar_continuada();
        assert_eq!(i.ate, antes.len() as u64);
        assert_eq!(bifurcar_de(Some(&i), false), None);
        // O VS Code continuou a original depois: o histórico daqui para onde ela saiu.
        fs::write(&arquivo, format!("{antes}{depois}\n")).unwrap();
        assert_eq!(historico(&arquivo, i.limite()).unwrap().itens.len(), 2);
        // Sem o arquivo, não há de onde continuar.
        let sumiu = Importada { arquivo: raiz.join("nao.jsonl"), ..i.clone() };
        assert_eq!(bifurcar_de(Some(&Importada { continuada: false, ..sumiu }), false), None);
        // Muitas mensagens: ficam as mais novas, e o resultado da ferramenta acha a chamada.
        let mut grande = String::new();
        for n in 0..(MAX_ITENS + 10) {
            grande.push_str(&usuario(&cwd, "2026-09-21T10:00:00.000Z", serde_json::json!(format!("m{n}"))));
            grande.push('\n');
        }
        grande.push_str(&assistente(&cwd, "2026-09-21T10:00:00.000Z", "z", serde_json::json!([{"type": "tool_use", "id": "tz", "name": "Bash", "input": {"command": "ls"}}])));
        grande.push('\n');
        grande.push_str(&usuario(&cwd, "2026-09-21T10:00:00.000Z", serde_json::json!([{"type": "tool_result", "tool_use_id": "tz", "content": "a\nb", "is_error": true}])));
        fs::write(&arquivo, grande).unwrap();
        let h = historico(&arquivo, None).unwrap();
        assert_eq!(h.itens.len(), MAX_ITENS);
        assert_eq!(h.cortados, 11);
        assert!(matches!(h.itens.last(), Some(Item::Ferramenta { resultado: Some(r), erro: true, .. }) if r == "a\nb"));
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn textos_do_marco_e_de_este_chat() {
        let i = Importada {
            origem: "VS Code".into(),
            primeira: 1_790_000_000,
            ultima: 1_790_500_000,
            mensagens: 42,
            importada_em: 1_790_600_000,
            ..Default::default()
        };
        assert!(marco(&i).starts_with("Trazida do Claude Code (VS Code) em "));
        let texto = descricao(&i, "~");
        assert!(texto.starts_with("Continua uma conversa do Claude Code (VS Code) de "));
        assert!(texto.contains("com 42 mensagens. Mexe direto em ~"));
        assert!(texto.ends_with("segue no Claude Code como está."));
        assert!(!descricao(&Importada { continuada: true, ..i }, "~").contains("cópia da sessão"));
    }
}
