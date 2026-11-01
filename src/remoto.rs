//! Execução dos programas que falam com a rede (gh, curl, git fetch/push) e o que vem junto: limite
//! de tempo, erros classificados, recuo, estado da rede por provedor, filas e a varredura de
//! segredos. Tudo bloqueia: rode fora da thread da interface.
// Provisório: o motor passa a usar estes módulos no G4.
#![allow(dead_code)]

use std::{
    io::{Read, Write},
    ops::Range,
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::processos;

/// Todo subprocesso de rede passa por aqui: limite de tempo, SIGTERM e depois SIGKILL no grupo.
/// Os testes usam um executor falso com as amostras reais.
pub trait Executor: Send + Sync {
    /// `programa` é sempre um nome fixo do código ("gh", "curl", "git", "bwrap"), nunca texto de
    /// fora. `entrada` vai inteira para o stdin, que então fecha.
    fn rodar(
        &self,
        programa: &'static str,
        args: &[String],
        ambiente: &Ambiente,
        pasta: &Path,
        entrada: Option<&[u8]>,
        limite: Duration,
    ) -> Result<SaidaBruta, ErroRemoto>;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SaidaBruta {
    /// `None` quando morreu por sinal.
    pub codigo: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl SaidaBruta {
    pub fn texto(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn erros(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// O ambiente do processo: com `limpar`, só ficam as variáveis de `manter` (as que existirem).
/// Depois saem as de `remover` e entram as de `definir`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ambiente {
    pub limpar: bool,
    pub manter: Vec<&'static str>,
    pub definir: Vec<(String, String)>,
    pub remover: Vec<&'static str>,
}

/// O executor de verdade.
pub struct Real;

/// O que o processo escreve além disso é lido e jogado fora (parado no pipe, ele travaria).
const TETO_STDOUT: usize = 32 * 1024 * 1024;
const TETO_STDERR: usize = 1024 * 1024;

impl Executor for Real {
    fn rodar(
        &self,
        programa: &'static str,
        args: &[String],
        ambiente: &Ambiente,
        pasta: &Path,
        entrada: Option<&[u8]>,
        limite: Duration,
    ) -> Result<SaidaBruta, ErroRemoto> {
        // O único lugar em que a integração com GitHub e Linear cria processos: num grupo próprio
        // (Unix) ou num job (Windows), para o limite derrubar também o que ele abriu (o git que o
        // gh chama); no Linux, morre junto se o Vitral cair.
        let mut comando = processos::comando_leve(programa, "remoto");
        comando
            .args(args)
            .current_dir(pasta)
            .stdin(if entrada.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        aplicar_ambiente(&mut comando, ambiente);
        let (mut filho, escopo) = processos::abrir(comando).map_err(|erro| match erro.kind() {
            std::io::ErrorKind::NotFound => ErroRemoto::SemPrograma(programa),
            _ => ErroRemoto::Outro(format!("não abriu o {programa}: {erro}")),
        })?;
        // O stdin vai numa thread: com uma entrada grande e o filho já escrevendo no stdout, os
        // dois pipes cheios travariam. A cópia (pode ter a chave do Linear) é apagada no fim.
        let escrita = filho.stdin.take().zip(entrada).map(|(mut stdin, dados)| {
            let dados = zeroize::Zeroizing::new(dados.to_vec());
            std::thread::spawn(move || {
                let _ = stdin.write_all(&dados);
            })
        });
        let ler = |fonte: Option<Box<dyn Read + Send>>, teto: usize| {
            let guardado = Arc::new(Mutex::new(Vec::new()));
            let leitura = fonte.map(|mut fonte| {
                let guardado = guardado.clone();
                std::thread::spawn(move || {
                    let mut bloco = [0u8; 8192];
                    loop {
                        match fonte.read(&mut bloco) {
                            Ok(0) => break,
                            Ok(n) => {
                                let mut texto = guardado.lock().unwrap_or_else(|e| e.into_inner());
                                let cabe = teto.saturating_sub(texto.len()).min(n);
                                texto.extend_from_slice(&bloco[..cabe]);
                            }
                            Err(erro) if erro.kind() == std::io::ErrorKind::Interrupted => {}
                            Err(_) => break,
                        }
                    }
                })
            });
            (guardado, leitura)
        };
        let (stdout, leitura_saida) = ler(filho.stdout.take().map(|s| Box::new(s) as _), TETO_STDOUT);
        let (stderr, leitura_erros) = ler(filho.stderr.take().map(|s| Box::new(s) as _), TETO_STDERR);
        let status = processos::esperar(&mut filho, &escopo, limite);
        // Quem escapou do grupo e ainda segura o pipe não prende a resposta.
        let folga = Instant::now() + Duration::from_secs(2);
        let pendentes = [leitura_saida, leitura_erros, escrita];
        while pendentes.iter().flatten().any(|t| !t.is_finished()) && Instant::now() < folga {
            std::thread::sleep(Duration::from_millis(10));
        }
        let tirar = |guardado: &Arc<Mutex<Vec<u8>>>| std::mem::take(&mut *guardado.lock().unwrap_or_else(|e| e.into_inner()));
        let status = status.ok_or(ErroRemoto::TempoEsgotado)?;
        Ok(SaidaBruta {
            codigo: status.code(),
            stdout: tirar(&stdout),
            stderr: tirar(&stderr),
        })
    }
}

fn aplicar_ambiente(comando: &mut Command, ambiente: &Ambiente) {
    if ambiente.limpar {
        comando.env_clear();
        for nome in &ambiente.manter {
            if let Some(valor) = std::env::var_os(nome) {
                comando.env(nome, valor);
            }
        }
    }
    for nome in &ambiente.remover {
        comando.env_remove(nome);
    }
    for (nome, valor) in &ambiente.definir {
        comando.env(nome, valor);
    }
}

// ---------- erros ----------

#[derive(Clone, Debug, PartialEq)]
pub enum ErroRemoto {
    SemPrograma(&'static str),
    SemRede(String),
    SemLogin,
    SemPermissao(String),
    NaoEncontrado(String),
    Limite { ate: Option<SystemTime> },
    Validacao(String),
    Conflito(String),
    TempoEsgotado,
    Outro(String),
}

/// O texto do `NaoEncontrado` de um log que passou da retenção (HTTP 410).
pub const LOG_VENCIDO: &str = "log vencido";
/// O de um log que o GitHub não tem mais (apagado, ou de uma execução sem log guardado).
pub const LOG_SUMIU: &str = "log sumiu";

impl ErroRemoto {
    /// A frase para aviso, notificação ou `Pr.erro`, já sem segredos. `provedor` é "GitHub" ou
    /// "Linear".
    pub fn para_usuario(&self, provedor: &str) -> String {
        let texto = match self {
            Self::SemPrograma("gh") => "O gh não está instalado. Instale com `sudo dnf install gh`.".to_string(),
            Self::SemPrograma(programa) => format!("O {programa} não está instalado."),
            Self::SemRede(_) => format!("Sem conexão com o {provedor}."),
            Self::SemLogin if provedor == "Linear" => "O Linear recusou a chave (revogada?).".to_string(),
            Self::SemLogin => format!("Entre no {provedor} para ver PRs e issues."),
            Self::SemPermissao(motivo) => format!("Sem permissão no {provedor}: {motivo}"),
            Self::NaoEncontrado(motivo) if motivo == LOG_VENCIDO => {
                "O log desta execução já expirou no GitHub (retenção de 90 dias).".to_string()
            }
            Self::NaoEncontrado(motivo) if motivo == LOG_SUMIU => {
                "O GitHub não tem mais o log desta execução (apagado ou expirado).".to_string()
            }
            Self::NaoEncontrado(motivo) => format!("Não encontrado no {provedor}: {motivo}"),
            Self::Limite { ate: Some(ate) } => {
                let hora = chrono::DateTime::<chrono::Local>::from(*ate).format("%H:%M");
                format!("Limite de pedidos do {provedor} até {hora}; o Vitral espera sozinho.")
            }
            Self::Limite { ate: None } => format!("Limite de pedidos do {provedor}; o Vitral espera sozinho."),
            Self::Validacao(texto) | Self::Conflito(texto) | Self::Outro(texto) => texto.clone(),
            Self::TempoEsgotado => format!("O {provedor} não respondeu a tempo."),
        };
        cortar(&sem_segredos(&texto), 300)
    }

    /// Passa sozinho com o tempo: vale o recuo (ou a sonda), não o aviso.
    pub fn transitorio(&self) -> bool {
        matches!(self, Self::SemRede(_) | Self::Limite { .. } | Self::TempoEsgotado)
    }
}

/// Corta no limite (em bytes, sem partir caractere) e marca com "…".
fn cortar(texto: &str, limite: usize) -> String {
    if texto.len() <= limite {
        return texto.to_string();
    }
    let mut fim = limite;
    while !texto.is_char_boundary(fim) {
        fim -= 1;
    }
    format!("{}…", &texto[..fim])
}

/// Mensagens de rede do gh e do git (o Go e o libcurl escrevem em inglês).
const SEM_REDE: [&str; 9] = [
    "error connecting to",
    "dial tcp",
    "no such host",
    "could not resolve host",
    "i/o timeout",
    "tls handshake",
    "connection refused",
    "proxyconnect",
    "network is unreachable",
];

/// O erro de um `gh` que não deu certo. Um stdout com JSON e `data` (o erro parcial do GraphQL)
/// é tratado por quem chamou, antes daqui.
pub fn classificar_gh(codigo: Option<i32>, stdout: &str, stderr: &str) -> ErroRemoto {
    let texto = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
    let minusculo = texto.to_lowercase();
    let tem = |trecho: &str| minusculo.contains(&trecho.to_lowercase());
    let resumo = || cortar(&sem_segredos(texto), 300);
    if codigo == Some(4) {
        return ErroRemoto::SemLogin;
    }
    if tem("Could not resolve to a") && !stdout.contains("\"data\"") {
        return ErroRemoto::NaoEncontrado(resumo());
    }
    if tem("HTTP 410") {
        return ErroRemoto::NaoEncontrado(LOG_VENCIDO.into());
    }
    if tem("log not found") {
        return ErroRemoto::NaoEncontrado(LOG_SUMIU.into());
    }
    if tem("HTTP 404") {
        return ErroRemoto::NaoEncontrado(resumo());
    }
    if tem("HTTP 401") {
        return ErroRemoto::SemLogin;
    }
    // Antes do 403: o limite também chega como "HTTP 403: API rate limit exceeded".
    if tem("rate limit") || tem("HTTP 429") {
        let ate = fim_do_limite(&format!("{stdout}\n{stderr}"), SystemTime::now());
        return ErroRemoto::Limite { ate };
    }
    if tem("SAML") {
        return ErroRemoto::SemPermissao("autorize o acesso à organização: gh auth refresh -h github.com".into());
    }
    if tem("HTTP 403") {
        return ErroRemoto::SemPermissao(resumo());
    }
    if SEM_REDE.iter().any(|t| tem(t)) {
        return ErroRemoto::SemRede(resumo());
    }
    if tem("Base branch was modified") {
        return ErroRemoto::Conflito("base mudou".into());
    }
    let validacao = [
        "HTTP 422",
        "is not mergeable",
        "Pull request is in clean status",
        "Pull request is in unstable status",
        "already exists",
        "No commits between",
        "Validation Failed",
        "Head branch was modified",
    ];
    if validacao.iter().any(|t| tem(t)) {
        return ErroRemoto::Validacao(resumo());
    }
    ErroRemoto::Outro(resumo())
}

/// O erro de um `curl` (pelo código de saída) ou da resposta HTTP do Linear (pelo status, pelos
/// cabeçalhos e pelo `extensions.code` do primeiro erro do GraphQL).
pub fn classificar_curl(
    codigo: Option<i32>,
    http: Option<u16>,
    cabecalhos: &[(String, String)],
    corpo: &str,
) -> ErroRemoto {
    match codigo {
        Some(5 | 6 | 7 | 35 | 52 | 56) => return ErroRemoto::SemRede(cortar(&sem_segredos(corpo.trim()), 300)),
        Some(28) => return ErroRemoto::TempoEsgotado,
        _ => {}
    }
    let codigo_graphql = serde_json::from_str::<serde_json::Value>(corpo)
        .ok()
        .and_then(|v| v["errors"][0]["extensions"]["code"].as_str().map(str::to_string))
        .unwrap_or_default();
    let mensagem = serde_json::from_str::<serde_json::Value>(corpo)
        .ok()
        .and_then(|v| v["errors"][0]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| corpo.trim().to_string());
    let resumo = cortar(&sem_segredos(&mensagem), 300);
    let tem = |trecho: &str| codigo_graphql == trecho || corpo.contains(trecho);
    if http == Some(401) || tem("AUTHENTICATION_ERROR") {
        return ErroRemoto::SemLogin;
    }
    if http == Some(429) || tem("RATELIMITED") {
        let segundos = cabecalhos
            .iter()
            .find(|(nome, _)| nome.eq_ignore_ascii_case("retry-after"))
            .and_then(|(_, valor)| valor.trim().parse::<u64>().ok());
        let ate = segundos.and_then(|s| SystemTime::now().checked_add(Duration::from_secs(s)));
        return ErroRemoto::Limite { ate };
    }
    if tem("FORBIDDEN") {
        return ErroRemoto::SemPermissao(resumo);
    }
    if tem("INVALID_INPUT") {
        return ErroRemoto::Validacao(resumo);
    }
    if codigo.is_some_and(|c| c != 0) && http.is_none() {
        return ErroRemoto::Outro(format!("o curl saiu com {}: {resumo}", codigo.unwrap_or_default()));
    }
    ErroRemoto::Outro(resumo)
}

/// O erro de um git de rede (fetch, push, ls-remote), que roda com `LC_ALL=C`.
pub fn classificar_git(codigo: Option<i32>, stderr: &str) -> ErroRemoto {
    let minusculo = stderr.to_lowercase();
    let tem = |trecho: &str| minusculo.contains(trecho);
    let resumo = cortar(&sem_segredos(stderr.trim()), 300);
    if SEM_REDE.iter().any(|t| tem(t)) || (tem("could not read from remote") && tem("connection")) {
        return ErroRemoto::SemRede(resumo);
    }
    if tem("operation too slow") || tem("timed out") {
        return ErroRemoto::TempoEsgotado;
    }
    if tem("authentication failed") || tem("could not read username") || tem("terminal prompts disabled") {
        return ErroRemoto::SemLogin;
    }
    if tem("repository not found") || tem("couldn't find remote ref") || tem("does not appear to be a git repository") {
        return ErroRemoto::NaoEncontrado(resumo);
    }
    if tem("permission denied") || tem("403") {
        return ErroRemoto::SemPermissao(resumo);
    }
    if codigo.is_none() {
        return ErroRemoto::Outro(format!("o git foi interrompido: {resumo}"));
    }
    ErroRemoto::Outro(resumo)
}

/// Quando o limite acaba, pelos cabeçalhos que vierem no texto: `Retry-After` (segundos a partir
/// de agora) ou `X-RateLimit-Reset` (segundos Unix).
fn fim_do_limite(texto: &str, agora: SystemTime) -> Option<SystemTime> {
    let valor = |nome: &str| {
        texto.lines().find_map(|linha| {
            let (chave, valor) = linha.split_once(':')?;
            chave.trim().eq_ignore_ascii_case(nome).then(|| valor.trim().parse::<u64>().ok())?
        })
    };
    if let Some(segundos) = valor("retry-after") {
        return agora.checked_add(Duration::from_secs(segundos));
    }
    valor("x-ratelimit-reset").and_then(|s| UNIX_EPOCH.checked_add(Duration::from_secs(s)))
}

// ---------- recuo, rede e filas ----------

/// Espera entre tentativas depois de `falhas` erros transitórios seguidos.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Recuo {
    pub falhas: u32,
}

const TETO_DO_RECUO: Duration = Duration::from_secs(15 * 60);

impl Recuo {
    /// `base` dobrando a cada falha (até 64 vezes), ±10% para os provedores não acordarem juntos,
    /// e nunca mais que 15 min. Não entra em pânico com nenhum número de falhas.
    pub fn proxima(&self, base: Duration) -> Duration {
        let dobrado = base.checked_mul(1 << self.falhas.min(6)).unwrap_or(TETO_DO_RECUO).min(TETO_DO_RECUO);
        let sorte = (uuid::Uuid::new_v4().as_u128() % 2001) as f64; // 0..=2000
        dobrado.mul_f64(0.9 + sorte / 10_000.0).min(TETO_DO_RECUO)
    }
}

/// Como está a conversa com um provedor.
#[derive(Clone, Debug, PartialEq)]
pub enum EstadoRede {
    Ok,
    /// Sem conexão: as consultas param e só a sonda roda, uma por vez.
    Fora { desde: Instant, proxima_sonda: Instant, tentativas: u32 },
    /// O provedor pediu para esperar.
    Limite { ate: Instant },
}

/// As esperas da sonda, uma depois da outra (a última se repete).
const SONDAS: [u64; 4] = [30, 60, 120, 300];
/// Um limite sem hora para acabar.
const LIMITE_SEM_HORA: Duration = Duration::from_secs(60);

/// O estado da rede de um provedor e a conta dos tempos esgotados seguidos.
#[derive(Clone, Debug)]
pub struct Rede {
    pub estado: EstadoRede,
    esgotados: u32,
}

impl Default for Rede {
    fn default() -> Self {
        Self { estado: EstadoRede::Ok, esgotados: 0 }
    }
}

impl Rede {
    pub fn deu_certo(&mut self) {
        *self = Self::default();
    }

    /// Vai para `Fora` com um `SemRede` ou com dois `TempoEsgotado` seguidos; para `Limite` com
    /// um `Limite`. Os outros erros não dizem nada da rede.
    pub fn falhou(&mut self, erro: &ErroRemoto, agora: Instant) {
        let fora = |agora: Instant| EstadoRede::Fora {
            desde: agora,
            proxima_sonda: agora + Duration::from_secs(SONDAS[0]),
            tentativas: 0,
        };
        match erro {
            ErroRemoto::SemRede(_) => {
                if !matches!(self.estado, EstadoRede::Fora { .. }) {
                    self.estado = fora(agora);
                }
            }
            ErroRemoto::TempoEsgotado => {
                self.esgotados += 1;
                if self.esgotados >= 2 && !matches!(self.estado, EstadoRede::Fora { .. }) {
                    self.estado = fora(agora);
                }
            }
            ErroRemoto::Limite { ate } => {
                let falta = ate
                    .and_then(|ate| ate.duration_since(SystemTime::now()).ok())
                    .unwrap_or(LIMITE_SEM_HORA);
                self.estado = EstadoRede::Limite { ate: agora + falta };
            }
            _ => self.esgotados = 0,
        }
    }

    /// A sonda falhou: a próxima espera mais (30, 60, 120 e depois 300 s).
    pub fn sonda_falhou(&mut self, agora: Instant) {
        if let EstadoRede::Fora { proxima_sonda, tentativas, .. } = &mut self.estado {
            *tentativas = tentativas.saturating_add(1);
            let espera = SONDAS[(*tentativas as usize).min(SONDAS.len() - 1)];
            *proxima_sonda = agora + Duration::from_secs(espera);
        }
    }
}

/// Filas por provedor: uma para o fundo (vigia), outra para as ações do usuário. A permissão é
/// pega com `.acquire_arc().await` antes do `background_spawn` e a guarda vai para dentro da
/// tarefa; nunca se segura uma delas esperando `na_vez_do_projeto`.
pub struct Filas {
    pub fundo: Arc<smol::lock::Semaphore>,
    pub usuario: Arc<smol::lock::Semaphore>,
}

impl Default for Filas {
    fn default() -> Self {
        Self {
            fundo: Arc::new(smol::lock::Semaphore::new(1)),
            usuario: Arc::new(smol::lock::Semaphore::new(1)),
        }
    }
}

// ---------- segredos ----------

/// Prefixos de tokens que nunca podem ir para aviso, erro, notificação, `Pr.erro`, log ou disco.
const PREFIXOS: [&str; 7] = ["lin_api_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"];
/// Menos que isso depois do prefixo não é token (é o próprio prefixo citado no código).
const MINIMO_DO_TOKEN: usize = 16;

fn do_token(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Onde há segredo no texto: tokens do GitHub e do Linear (inteiros, com o prefixo), o valor de
/// um cabeçalho `Authorization:` até o fim da linha e blocos `-----BEGIN … PRIVATE KEY-----`
/// (até o END, ou até o fim do texto). Faixas em bytes, em ordem e sem sobreposição.
pub fn achar_segredos(texto: &str) -> Vec<Range<usize>> {
    let bytes = texto.as_bytes();
    let minusculo = texto.to_ascii_lowercase();
    let mut faixas = Vec::new();
    for prefixo in PREFIXOS {
        let mut desde = 0;
        while let Some(achou) = texto[desde..].find(prefixo) {
            let inicio = desde + achou;
            let mut fim = inicio + prefixo.len();
            while fim < bytes.len() && do_token(bytes[fim]) {
                fim += 1;
            }
            let solto = inicio == 0 || !do_token(bytes[inicio - 1]);
            if solto && fim - inicio - prefixo.len() >= MINIMO_DO_TOKEN {
                faixas.push(inicio..fim);
            }
            desde = fim.max(inicio + 1);
        }
    }
    let mut desde = 0;
    while let Some(achou) = minusculo[desde..].find("authorization:") {
        let mut inicio = desde + achou + "authorization:".len();
        while inicio < bytes.len() && (bytes[inicio] == b' ' || bytes[inicio] == b'\t') {
            inicio += 1;
        }
        let fim = texto[inicio..].find(['\n', '\r']).map_or(texto.len(), |n| inicio + n);
        if fim > inicio {
            faixas.push(inicio..fim);
        }
        desde = fim.max(inicio);
    }
    let mut desde = 0;
    while let Some(achou) = texto[desde..].find("-----BEGIN ") {
        let inicio = desde + achou;
        let cabecalho_fim = texto[inicio + 11..].find("-----").map(|n| inicio + 11 + n);
        let Some(cabecalho_fim) = cabecalho_fim else { break };
        if !texto[inicio..cabecalho_fim].contains("PRIVATE KEY") {
            desde = cabecalho_fim;
            continue;
        }
        let fim = texto[cabecalho_fim..]
            .find("-----END ")
            .and_then(|n| {
                let depois = cabecalho_fim + n + 9;
                texto[depois..].find("-----").map(|m| depois + m + 5)
            })
            .unwrap_or(texto.len());
        faixas.push(inicio..fim);
        desde = fim;
    }
    faixas.sort_by_key(|f| (f.start, std::cmp::Reverse(f.end)));
    let mut juntas: Vec<Range<usize>> = Vec::new();
    for faixa in faixas {
        match juntas.last_mut() {
            Some(ultima) if faixa.start <= ultima.end => ultima.end = ultima.end.max(faixa.end),
            _ => juntas.push(faixa),
        }
    }
    juntas
}

/// O texto com cada segredo trocado por "…". Vale para todo texto que vira aviso, erro,
/// notificação, `Pr.erro` ou log.
pub fn sem_segredos(texto: &str) -> String {
    let mut limpo = String::with_capacity(texto.len());
    let mut desde = 0;
    for faixa in achar_segredos(texto) {
        limpo.push_str(&texto[desde..faixa.start]);
        limpo.push('…');
        desde = faixa.end;
    }
    limpo.push_str(&texto[desde..]);
    limpo
}

/// Que tipo de segredo é o trecho que `achar_segredos` achou (para o aviso, sem o segredo).
pub fn tipo_do_segredo(trecho: &str) -> &'static str {
    if trecho.starts_with("lin_api_") {
        "chave do Linear"
    } else if trecho.starts_with("github_pat_") || trecho.starts_with("gh") {
        "token do GitHub"
    } else if trecho.starts_with("-----BEGIN") {
        "chave privada"
    } else {
        "cabeçalho Authorization"
    }
}

#[cfg(test)]
pub mod falso {
    //! Um `Executor` que devolve respostas prontas (amostras reais) e guarda as chamadas.

    use super::*;

    #[derive(Clone, Debug)]
    pub struct Chamada {
        pub programa: &'static str,
        pub args: Vec<String>,
        pub ambiente: Ambiente,
        pub pasta: std::path::PathBuf,
        pub entrada: Option<Vec<u8>>,
        pub limite: Duration,
    }

    /// Responde na ordem das respostas dadas; sem resposta, `Outro("sem resposta")`.
    #[derive(Default)]
    pub struct Falso {
        pub respostas: Mutex<std::collections::VecDeque<Result<SaidaBruta, ErroRemoto>>>,
        pub chamadas: Mutex<Vec<Chamada>>,
    }

    impl Falso {
        pub fn com(respostas: Vec<Result<SaidaBruta, ErroRemoto>>) -> Self {
            Self { respostas: Mutex::new(respostas.into()), chamadas: Mutex::default() }
        }

        pub fn chamadas(&self) -> Vec<Chamada> {
            self.chamadas.lock().unwrap().clone()
        }
    }

    pub fn saida(codigo: i32, stdout: &str, stderr: &str) -> Result<SaidaBruta, ErroRemoto> {
        Ok(SaidaBruta { codigo: Some(codigo), stdout: stdout.as_bytes().to_vec(), stderr: stderr.as_bytes().to_vec() })
    }

    impl Executor for Falso {
        fn rodar(
            &self,
            programa: &'static str,
            args: &[String],
            ambiente: &Ambiente,
            pasta: &Path,
            entrada: Option<&[u8]>,
            limite: Duration,
        ) -> Result<SaidaBruta, ErroRemoto> {
            self.chamadas.lock().unwrap().push(Chamada {
                programa,
                args: args.to_vec(),
                ambiente: ambiente.clone(),
                pasta: pasta.to_path_buf(),
                entrada: entrada.map(<[u8]>::to_vec),
                limite,
            });
            self.respostas
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(ErroRemoto::Outro("sem resposta".into())))
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn classificar_gh_cobre_cada_linha_da_tabela() {
        use ErroRemoto::*;
        let gh = |codigo, stderr: &str| classificar_gh(Some(codigo), "", stderr);
        assert_eq!(gh(4, "To get started with GitHub CLI, please run:  gh auth login"), SemLogin);
        assert!(matches!(
            gh(1, "GraphQL: Could not resolve to a PullRequest with the number of 999. (repository.pullRequest)"),
            NaoEncontrado(_)
        ));
        // Com `data` no stdout é o erro parcial do GraphQL, que o chamador trata; aqui não vira
        // NaoEncontrado.
        assert!(!matches!(
            classificar_gh(Some(1), r#"{"data":{"repository":{"p1":null}}}"#, "gh: Could not resolve to a PullRequest"),
            NaoEncontrado(_)
        ));
        assert!(matches!(gh(1, "HTTP 404: Not Found (https://api.github.com/repos/o/r)"), NaoEncontrado(_)));
        assert_eq!(gh(1, "failed to get run log: HTTP 410: Server Error"), NaoEncontrado(LOG_VENCIDO.into()));
        assert_eq!(gh(1, "log not found: 121180596056"), NaoEncontrado(LOG_SUMIU.into()));
        assert_eq!(gh(1, "HTTP 401: Bad credentials (https://api.github.com/graphql)"), SemLogin);
        assert!(matches!(gh(1, "HTTP 403: API rate limit exceeded for user ID 1."), Limite { .. }));
        assert!(matches!(gh(1, "You have exceeded a secondary rate limit"), Limite { .. }));
        assert!(matches!(gh(1, "HTTP 429: Too Many Requests"), Limite { .. }));
        assert_eq!(
            gh(1, "HTTP 403: Resource protected by organization SAML enforcement."),
            SemPermissao("autorize o acesso à organização: gh auth refresh -h github.com".into())
        );
        assert!(matches!(gh(1, "HTTP 403: Must have admin rights to Repository."), SemPermissao(_)));
        for rede in [
            "error connecting to api.github.com",
            "Post \"https://api.github.com/graphql\": dial tcp: lookup api.github.com: no such host",
            "could not resolve host: github.com",
            "read tcp 10.0.0.2:5000->140.82.112.6:443: i/o timeout",
            "net/http: TLS handshake timeout",
            "dial tcp 127.0.0.1:9: connect: connection refused",
            "proxyconnect tcp: dial tcp 127.0.0.1:9: connect: connection refused",
        ] {
            assert!(matches!(gh(1, rede), SemRede(_)), "{rede}");
        }
        assert!(matches!(gh(1, "HTTP 422: Validation Failed"), Validacao(_)));
        assert!(matches!(gh(1, "GraphQL: Pull Request is not mergeable (mergePullRequest)"), Validacao(_)));
        assert!(matches!(gh(1, "Pull request is in clean status"), Validacao(_)));
        assert_eq!(gh(1, "GraphQL: Base branch was modified. Review and try the merge again."), Conflito("base mudou".into()));
        assert!(matches!(gh(1, "algo que ninguém previu"), Outro(_)));
    }

    #[test]
    fn o_limite_do_gh_le_retry_after_e_reset() {
        let agora = UNIX_EPOCH + Duration::from_secs(1_000);
        assert_eq!(fim_do_limite("HTTP/2.0 429\nRetry-After: 60\n", agora), Some(agora + Duration::from_secs(60)));
        assert_eq!(
            fim_do_limite("X-Ratelimit-Reset: 1793476692", agora),
            Some(UNIX_EPOCH + Duration::from_secs(1_793_476_692))
        );
        assert_eq!(fim_do_limite("nada aqui", agora), None);
    }

    #[test]
    fn classificar_curl_cobre_cada_linha_da_tabela() {
        use ErroRemoto::*;
        for codigo in [5, 6, 7, 35, 52, 56] {
            assert!(matches!(classificar_curl(Some(codigo), None, &[], "curl: (6) Could not resolve host"), SemRede(_)));
        }
        assert_eq!(classificar_curl(Some(28), None, &[], ""), TempoEsgotado);
        assert_eq!(classificar_curl(Some(0), Some(401), &[], ""), SemLogin);
        let erro = |code: &str| format!(r#"{{"errors":[{{"message":"x","extensions":{{"code":"{code}"}}}}]}}"#);
        assert_eq!(classificar_curl(Some(0), Some(400), &[], &erro("AUTHENTICATION_ERROR")), SemLogin);
        let espera = vec![("Retry-After".to_string(), "30".to_string())];
        let Limite { ate: Some(ate) } = classificar_curl(Some(0), Some(429), &espera, "") else {
            panic!("devia ser Limite com hora");
        };
        let falta = ate.duration_since(SystemTime::now()).unwrap();
        assert!(falta > Duration::from_secs(25) && falta <= Duration::from_secs(30));
        assert!(matches!(classificar_curl(Some(0), Some(400), &[], &erro("RATELIMITED")), Limite { .. }));
        assert!(matches!(classificar_curl(Some(0), Some(200), &[], &erro("FORBIDDEN")), SemPermissao(_)));
        assert!(matches!(classificar_curl(Some(0), Some(200), &[], &erro("INVALID_INPUT")), Validacao(_)));
    }

    #[test]
    fn classificar_git_separa_rede_login_e_o_resto() {
        use ErroRemoto::*;
        assert!(matches!(classificar_git(Some(128), "fatal: unable to access 'https://github.com/o/r/': Could not resolve host: github.com"), SemRede(_)));
        assert_eq!(classificar_git(Some(128), "error: RPC failed; curl 28 Operation too slow."), TempoEsgotado);
        assert_eq!(classificar_git(Some(128), "fatal: could not read Username for 'https://github.com': terminal prompts disabled"), SemLogin);
        assert!(matches!(classificar_git(Some(128), "remote: Repository not found."), NaoEncontrado(_)));
        assert!(matches!(classificar_git(Some(1), "outra coisa"), Outro(_)));
    }

    #[test]
    fn para_usuario_nunca_mostra_segredo() {
        let erro = ErroRemoto::Outro(concat!("falhou com Authorization: lin_", "api_abcdefghijklmnopqrstuvwxyz0123").into());
        let texto = erro.para_usuario("Linear");
        assert!(!texto.contains("lin_api_"), "{texto}");
        assert_eq!(ErroRemoto::SemRede("x".into()).para_usuario("GitHub"), "Sem conexão com o GitHub.");
        assert!(ErroRemoto::NaoEncontrado(LOG_VENCIDO.into()).para_usuario("GitHub").contains("90 dias"));
        assert!(ErroRemoto::SemRede(String::new()).transitorio());
        assert!(!ErroRemoto::SemLogin.transitorio());
    }

    #[test]
    fn recuo_dobra_tem_teto_e_nao_entra_em_panico() {
        let base = Duration::from_secs(20);
        for falhas in [0, 1, 2, 6, 7, 40, u32::MAX] {
            let espera = Recuo { falhas }.proxima(base);
            let esperado = base.checked_mul(1 << falhas.min(6)).unwrap().min(TETO_DO_RECUO);
            assert!(espera <= TETO_DO_RECUO, "{falhas}: {espera:?}");
            assert!(espera >= esperado.mul_f64(0.9).min(TETO_DO_RECUO), "{falhas}: {espera:?}");
            assert!(espera <= esperado.mul_f64(1.1), "{falhas}: {espera:?}");
        }
        assert!(Recuo { falhas: 40 }.proxima(Duration::MAX) <= TETO_DO_RECUO);
    }

    #[test]
    fn rede_cai_com_sem_rede_ou_dois_tempos_esgotados_e_a_sonda_espaca() {
        let agora = Instant::now();
        let mut rede = Rede::default();
        rede.falhou(&ErroRemoto::TempoEsgotado, agora);
        assert_eq!(rede.estado, EstadoRede::Ok);
        rede.falhou(&ErroRemoto::TempoEsgotado, agora);
        let EstadoRede::Fora { proxima_sonda, .. } = rede.estado else { panic!("devia cair") };
        assert_eq!(proxima_sonda, agora + Duration::from_secs(30));
        let esperas: Vec<u64> = (0..5)
            .map(|_| {
                rede.sonda_falhou(agora);
                let EstadoRede::Fora { proxima_sonda, .. } = rede.estado else { unreachable!() };
                (proxima_sonda - agora).as_secs()
            })
            .collect();
        assert_eq!(esperas, [60, 120, 300, 300, 300]);
        rede.deu_certo();
        assert_eq!(rede.estado, EstadoRede::Ok);
        // Um erro que não é de rede zera a conta dos tempos esgotados.
        rede.falhou(&ErroRemoto::TempoEsgotado, agora);
        rede.falhou(&ErroRemoto::Validacao("x".into()), agora);
        rede.falhou(&ErroRemoto::TempoEsgotado, agora);
        assert_eq!(rede.estado, EstadoRede::Ok);
        rede.falhou(&ErroRemoto::SemRede("x".into()), agora);
        assert!(matches!(rede.estado, EstadoRede::Fora { .. }));
        let mut limitada = Rede::default();
        limitada.falhou(&ErroRemoto::Limite { ate: None }, agora);
        assert_eq!(limitada.estado, EstadoRede::Limite { ate: agora + LIMITE_SEM_HORA });
    }

    #[test]
    fn as_filas_sao_independentes() {
        let filas = Filas::default();
        smol::block_on(async {
            let _fundo = filas.fundo.acquire_arc().await;
            assert!(filas.fundo.try_acquire().is_none());
            assert!(filas.usuario.try_acquire().is_some());
        });
    }

    #[test]
    fn acha_e_tira_os_segredos() {
        let token = concat!("gh", "p_0123456789abcdefghijABCDEFGHIJ012345");
        let linear = concat!("lin_", "api_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcd");
        let pat = concat!("github_", "pat_11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz");
        let texto = format!(
            "token={token}\nAuthorization: {linear}\nchave {pat} fim\n-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY-----\ndepois"
        );
        let limpo = sem_segredos(&texto);
        for segredo in [token, linear, pat, "AAAA", "ghp_", "lin_api_", "github_pat_"] {
            assert!(!limpo.contains(segredo), "{segredo} ficou em {limpo}");
        }
        assert!(limpo.contains("token=…"));
        assert!(limpo.contains("Authorization: …"));
        assert!(limpo.contains("chave … fim"));
        assert!(limpo.ends_with("…\ndepois"));
        let faixas = achar_segredos(&texto);
        assert_eq!(faixas.len(), 4);
        assert_eq!(&texto[faixas[0].clone()], token);
        assert_eq!(tipo_do_segredo(&texto[faixas[0].clone()]), "token do GitHub");
        // O prefixo citado no código, um trecho curto e um nome qualquer não são segredo.
        for inocente in [r#"PREFIXOS = ["ghp_", "lin_api_"]"#, "ghp_curto", "xghp_0123456789abcdefghijABCDEF", "-----BEGIN PUBLIC KEY-----"] {
            assert!(achar_segredos(inocente).is_empty(), "{inocente}");
        }
        // Chave privada sem o fim: vai até o fim do texto.
        let cortada = "a\n-----BEGIN RSA PRIVATE KEY-----\nMIIE";
        assert_eq!(sem_segredos(cortada), "a\n…");
        assert_eq!(sem_segredos("sem nada"), "sem nada");
        assert_eq!(sem_segredos("ação ghp_0123456789abcdefghij çé"), "ação … çé");
    }

    #[test]
    fn o_executor_de_verdade_le_tudo_e_respeita_o_limite() {
        let pasta = std::env::temp_dir();
        let ambiente = Ambiente {
            limpar: true,
            manter: vec!["PATH"],
            definir: vec![("VITRAL_TESTE".into(), "sim".into())],
            remover: vec![],
        };
        // Uma variável que este processo tem (o `cargo test` põe) e que não passa adiante. O HOME não
        // serve: no Windows, o sh do Git inventa um a partir da pasta do usuário.
        assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
        let args: Vec<String> = ["-c", "cat; echo \"$VITRAL_TESTE ${CARGO_MANIFEST_DIR:-limpo}\"; echo erro >&2; exit 3"]
            .map(String::from)
            .to_vec();
        let saida = Real.rodar("sh", &args, &ambiente, &pasta, Some(b"entrada\n"), Duration::from_secs(10)).unwrap();
        assert_eq!(saida.codigo, Some(3));
        assert_eq!(saida.texto(), "entrada\nsim limpo\n");
        assert_eq!(saida.erros(), "erro\n");
        // Saída maior que o pipe não trava.
        let grande: Vec<String> = ["-c", "head -c 300000 /dev/zero"].map(String::from).to_vec();
        let saida = Real.rodar("sh", &grande, &Ambiente::default(), &pasta, None, Duration::from_secs(10)).unwrap();
        assert_eq!(saida.stdout.len(), 300_000);
        let inicio = Instant::now();
        let lento: Vec<String> = ["-c", "sleep 30 & sleep 30"].map(String::from).to_vec();
        let erro = Real.rodar("sh", &lento, &Ambiente::default(), &pasta, None, Duration::from_millis(300));
        assert_eq!(erro, Err(ErroRemoto::TempoEsgotado));
        assert!(inicio.elapsed() < Duration::from_secs(5));
        let ausente = Real.rodar("programa-que-nao-existe-vitral", &[], &Ambiente::default(), &pasta, None, Duration::from_secs(1));
        assert_eq!(ausente, Err(ErroRemoto::SemPrograma("programa-que-nao-existe-vitral")));
    }
}
