//! Uso do plano do Codex (OpenAI), lido do que o próprio Codex grava: cada resposta do modelo
//! deixa uma linha `token_count` com `rate_limits` no arquivo da sessão
//! (`~/.codex/sessions/AAAA/MM/DD/rollout-*.jsonl`). Sem rodar o Codex, sem rede e sem tokens.

use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::Value;

use crate::{contas::segundos_iso, modelo::agora, motor::Uso};

/// Pastas de dia (as mais novas que existem) e arquivos mais recentes que entram na busca.
const DIAS: usize = 7;
const ARQUIVOS: usize = 4;
/// Pedaço lido de cada vez, do fim para o começo, e o máximo lido de um arquivo.
const BLOCO: u64 = 64 * 1024;
const MAXIMO: u64 = 4 * 1024 * 1024;
/// Depois da primeira resposta sem janelas, quanto mais se lê atrás de uma com janelas.
const DEPOIS_DA_VAZIA: u64 = 256 * 1024;
/// Fim do `history.jsonl` lido para achar as sessões da semana.
const HISTORICO: u64 = 64 * 1024;
const SEMANA: i64 = 7 * 86_400;

/// Linha aproveitável de cada arquivo já lido, com o tamanho e o mtime da leitura: a releitura
/// a cada 30 s só abre o arquivo que mudou.
static LIDOS: Mutex<Vec<(PathBuf, u64, SystemTime, Option<Linha>)>> = Mutex::new(Vec::new());
/// Arquivo de cada sessão do histórico achado fora das pastas recentes, com a hora da busca.
static LOCALIZADOS: Mutex<Vec<(String, Option<PathBuf>, i64)>> = Mutex::new(Vec::new());

/// O que se sabe do plano do Codex neste computador.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Codex {
    /// Existe `auth.json` (só se confere que o arquivo existe; o conteúdo nunca é lido).
    pub logado: bool,
    pub uso: Uso,
    /// Nome do plano como o Codex mostra ("Plus", "Pro 200").
    pub plano: Option<String>,
    /// Janelas que não são de 5 h nem semanais (diária, mensal): rótulo, fração e renovação.
    pub outras: Vec<(String, f64, Option<i64>)>,
    /// A sessão manda o snapshot, mas sem janelas nem plano (login com chave de API).
    pub sem_limites: bool,
}

/// Uma linha `token_count` aproveitável: quando foi gravada e o `rate_limits`.
#[derive(Clone, Debug)]
struct Linha {
    instante: i64,
    limites: Value,
    /// Snapshot sem nenhuma janela (`primary` e `secondary` nulos).
    vazia: bool,
}

/// Pasta do Codex: `$CODEX_HOME` ou `~/.codex`.
fn raiz() -> Option<PathBuf> {
    match std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()) {
        Some(pasta) => Some(PathBuf::from(pasta)),
        None => Some(dirs::home_dir()?.join(".codex")),
    }
}

/// O uso mais recente do Codex (lê arquivos: rodar fora da thread da interface). `None` quando
/// a pasta do Codex não existe (não instalado ou nunca usado neste computador).
pub fn ler() -> Option<Codex> {
    ler_em(&raiz()?, agora() as i64)
}

/// Fica com a linha de maior horário entre os arquivos mais recentes: com sessões em paralelo,
/// a última resposta é a que vale.
fn ler_em(raiz: &Path, agora: i64) -> Option<Codex> {
    if !raiz.is_dir() {
        return None;
    }
    let mut codex = Codex {
        logado: raiz.join("auth.json").exists(),
        ..Codex::default()
    };
    let linhas = linhas_de(&candidatos(raiz, agora));
    // Sem `plan_type` na mais nova, vale o da linha mais nova que tiver (como o Codex faz).
    codex.plano = linhas
        .iter()
        .filter(|l| l.limites["plan_type"].is_string())
        .max_by_key(|l| l.instante)
        .and_then(|l| l.limites["plan_type"].as_str())
        .map(nome_do_plano);
    let Some(melhor) = linhas.iter().filter(|l| !l.vazia).max_by_key(|l| l.instante) else {
        // Só snapshots sem janelas: sem plano é chave de API; com plano (conta do ChatGPT), as
        // respostas só vieram sem os números.
        codex.sem_limites = !linhas.is_empty() && codex.plano.is_none();
        return Some(codex);
    };
    codex.uso.medido_em = Some(melhor.instante);
    for (posicao, chave) in ["primary", "secondary"].into_iter().enumerate() {
        let janela = &melhor.limites[chave];
        let Some(usado) = janela["used_percent"].as_f64() else {
            continue;
        };
        let mut fracao = usado.clamp(0.0, 100.0) / 100.0;
        let renova = numero(&janela["resets_at"])
            .or_else(|| melhor.instante.checked_add(numero(&janela["resets_in_seconds"])?));
        // A janela que já virou só recomeça no próximo uso.
        if renova.is_some_and(|r| r <= agora) {
            fracao = 0.0;
        }
        let tipo = match numero(&janela["window_minutes"]) {
            Some(minutos) => Tipo::de_minutos(minutos),
            None if posicao == 0 => Tipo::CincoHoras,
            None => Tipo::Semana,
        };
        match tipo {
            Tipo::CincoHoras if codex.uso.cinco_horas.is_none() => {
                codex.uso.cinco_horas = Some(fracao);
                codex.uso.renova_5h = renova;
            }
            Tipo::Semana if codex.uso.semana.is_none() => {
                codex.uso.semana = Some(fracao);
                codex.uso.renova_semana = renova;
            }
            outro => codex.outras.push((outro.rotulo(), fracao, renova)),
        }
        codex.uso.bloqueado |= fracao >= 1.0;
    }
    Some(codex)
}

/// Inteiro de um campo JSON que pode vir como inteiro ou número com casas.
fn numero(valor: &Value) -> Option<i64> {
    valor.as_i64().or_else(|| valor.as_f64().map(|v| v as i64))
}

/// Tamanho da janela pelo `window_minutes`, com folga de 5% como no próprio Codex (os antigos
/// 299 e 10079 caem dentro); nunca pela posição.
#[derive(Debug, PartialEq)]
enum Tipo {
    CincoHoras,
    Dia,
    Semana,
    Mes,
    Outra(i64),
}

impl Tipo {
    fn de_minutos(minutos: i64) -> Tipo {
        // Saturando: `window_minutes` absurdo não pode derrubar a leitura (build de depuração).
        let perto = |alvo: i64| {
            minutos.saturating_sub(alvo).saturating_abs().saturating_mul(100) <= alvo * 5
        };
        if perto(300) {
            Tipo::CincoHoras
        } else if perto(1440) {
            Tipo::Dia
        } else if perto(10_080) {
            Tipo::Semana
        } else if perto(43_200) {
            Tipo::Mes
        } else {
            Tipo::Outra(minutos)
        }
    }

    fn rotulo(&self) -> String {
        match self {
            Tipo::CincoHoras => "Sessão de 5 h".into(),
            Tipo::Dia => "Dia".into(),
            Tipo::Semana => "Semana".into(),
            Tipo::Mes => "Mês".into(),
            Tipo::Outra(m) if *m >= 2880 => format!("Janela de {} dias", m / 1440),
            Tipo::Outra(m) if *m >= 60 => format!("Janela de {} h", m / 60),
            Tipo::Outra(m) => format!("Janela de {m} min"),
        }
    }
}

/// Nome do plano como o `/status` do Codex mostra.
fn nome_do_plano(bruto: &str) -> String {
    match bruto.to_ascii_lowercase().as_str() {
        "free" => "Free".into(),
        "go" => "Go".into(),
        "plus" => "Plus".into(),
        "pro" => "Pro 200".into(),
        "prolite" => "Pro 100".into(),
        "promax" => "Pro 500".into(),
        "team" | "self_serve_business_usage_based" => "Business".into(),
        "business" | "enterprise" | "hc" | "ent26" => "Enterprise".into(),
        "edu" | "education" => "Edu".into(),
        outro => {
            let texto = outro.replace('_', " ");
            let mut letras = texto.chars();
            letras
                .next()
                .map(|c| c.to_uppercase().chain(letras).collect())
                .unwrap_or_default()
        }
    }
}

/// Um arquivo de sessão candidato; o tamanho e o mtime são a chave do que já foi lido.
struct Arquivo {
    caminho: PathBuf,
    tamanho: u64,
    mtime: SystemTime,
}

impl Arquivo {
    fn de(caminho: PathBuf) -> Option<Arquivo> {
        let dados = fs::metadata(&caminho).ok()?;
        Some(Arquivo {
            tamanho: dados.len(),
            mtime: dados.modified().ok()?,
            caminho,
        })
    }
}

/// Os arquivos de sessão mais recentes por mtime: os das pastas dos últimos dias, os arquivados
/// e os das sessões com pedido na semana, que podem ser de uma data antiga (sessão retomada
/// continua gravando no arquivo em que começou).
fn candidatos(raiz: &Path, agora: i64) -> Vec<Arquivo> {
    let mut pastas = Vec::new();
    'busca: for ano in subpastas(&raiz.join("sessions")) {
        for mes in subpastas(&ano) {
            for dia in subpastas(&mes) {
                pastas.push(dia);
                if pastas.len() >= DIAS {
                    break 'busca;
                }
            }
        }
    }
    pastas.push(raiz.join("archived_sessions"));
    let mut arquivos: Vec<Arquivo> = pastas
        .iter()
        .filter_map(|p| fs::read_dir(p).ok())
        .flatten()
        .filter_map(Result::ok)
        // Os `.jsonl.zst` (parados há mais de 7 dias) ficam de fora: a semana já virou.
        .filter(|e| e_sessao(&e.file_name().to_string_lossy()))
        .filter_map(|e| Arquivo::de(e.path()))
        .collect();
    for id in sessoes_recentes(raiz, agora.saturating_sub(SEMANA)) {
        if !arquivos.iter().any(|a| da_sessao(&a.caminho, &id))
            && let Some(arquivo) = localizar(raiz, &id, agora).and_then(Arquivo::de)
        {
            arquivos.push(arquivo);
        }
    }
    arquivos.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| b.caminho.cmp(&a.caminho)));
    arquivos.truncate(ARQUIVOS);
    arquivos
}

fn e_sessao(nome: &str) -> bool {
    nome.starts_with("rollout-") && nome.ends_with(".jsonl")
}

/// `rollout-<data>-<id>.jsonl` ou, com mais de um arquivo na sessão, `...-<id>_<outro>.jsonl`.
fn da_sessao(caminho: &Path, id: &str) -> bool {
    let Some(nome) = caminho.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let base = nome.strip_suffix(".jsonl").unwrap_or_default();
    e_sessao(nome) && (base.ends_with(&format!("-{id}")) || base.contains(&format!("-{id}_")))
}

/// Sessões com pedido desde `desde`, da mais recente para a mais velha, pelo fim do
/// `history.jsonl` (o Codex grava ali cada pedido com o id da sessão e o horário).
fn sessoes_recentes(raiz: &Path, desde: i64) -> Vec<String> {
    let mut fim = Vec::new();
    let lido = fs::File::open(raiz.join("history.jsonl")).and_then(|mut arquivo| {
        let tamanho = arquivo.metadata()?.len();
        arquivo.seek(SeekFrom::Start(tamanho.saturating_sub(HISTORICO)))?;
        arquivo.take(HISTORICO).read_to_end(&mut fim)
    });
    let mut ids: Vec<String> = Vec::new();
    if lido.is_err() {
        return ids;
    }
    // A primeira linha pode ter vindo pela metade: não se lê como JSON e fica de fora.
    for linha in fim.split(|&b| b == b'\n').rev() {
        let Ok(valor) = serde_json::from_slice::<Value>(linha) else {
            continue;
        };
        if numero(&valor["ts"]).is_some_and(|ts| ts < desde) {
            break;
        }
        let Some(id) = valor["session_id"].as_str() else {
            continue;
        };
        let valido = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if valido && !ids.iter().any(|i| i == id) {
            ids.push(id.to_string());
            if ids.len() >= ARQUIVOS {
                break;
            }
        }
    }
    ids
}

/// O arquivo de uma sessão em qualquer pasta de dia. Guarda o resultado: o achado vale enquanto
/// o arquivo existir e o não achado é procurado de novo só depois de 10 min.
fn localizar(raiz: &Path, id: &str, agora: i64) -> Option<PathBuf> {
    let mut localizados = LOCALIZADOS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, caminho, quando)) = localizados.iter().find(|(i, ..)| i == id) {
        match caminho {
            Some(c) if c.exists() => return Some(c.clone()),
            None if agora.saturating_sub(*quando) < 600 => return None,
            _ => {}
        }
    }
    let achado = subpastas(&raiz.join("sessions"))
        .into_iter()
        .flat_map(|ano| subpastas(&ano))
        .flat_map(|mes| subpastas(&mes))
        .find_map(|dia| {
            fs::read_dir(dia)
                .ok()?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .find(|p| da_sessao(p, id))
        });
    localizados.retain(|(i, ..)| i != id);
    localizados.push((id.to_string(), achado.clone(), agora));
    achado
}

/// A linha aproveitável de cada arquivo, lendo de novo só os que mudaram desde a última vez.
fn linhas_de(arquivos: &[Arquivo]) -> Vec<Linha> {
    let mut lidos = LIDOS.lock().unwrap_or_else(|e| e.into_inner());
    let mut linhas = Vec::new();
    for arquivo in arquivos {
        let guardada = lidos
            .iter()
            .find(|(caminho, tamanho, mtime, _)| {
                (caminho, *tamanho, *mtime) == (&arquivo.caminho, arquivo.tamanho, arquivo.mtime)
            })
            .map(|(.., linha)| linha.clone());
        let linha = guardada.unwrap_or_else(|| {
            let mtime = arquivo.mtime.duration_since(UNIX_EPOCH);
            let linha = ultima_linha(&arquivo.caminho, mtime.map_or(0, |d| d.as_secs() as i64));
            lidos.retain(|(caminho, ..)| *caminho != arquivo.caminho);
            lidos.push((arquivo.caminho.clone(), arquivo.tamanho, arquivo.mtime, linha.clone()));
            linha
        });
        linhas.extend(linha);
    }
    lidos.retain(|(caminho, ..)| arquivos.iter().any(|a| a.caminho == *caminho));
    linhas
}

/// Subpastas com nome numérico (AAAA, MM, DD), da mais nova para a mais velha.
fn subpastas(pasta: &Path) -> Vec<PathBuf> {
    let mut nomes: Vec<(u32, PathBuf)> = fs::read_dir(pasta)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Some((e.file_name().to_str()?.parse().ok()?, e.path())))
        .collect();
    nomes.sort_by(|a, b| b.0.cmp(&a.0));
    nomes.into_iter().map(|(_, p)| p).collect()
}

/// A última linha com janelas do arquivo (ou, se não houver, a última sem janelas), lendo de
/// trás para frente em blocos; a última pode estar pela metade, sendo gravada, e é ignorada.
/// Depois da primeira resposta sem janelas (chave de API, Codex antigo) só se lê mais um
/// trecho curto, em vez de varrer o arquivo atrás de uma que talvez não exista.
fn ultima_linha(arquivo: &Path, mtime: i64) -> Option<Linha> {
    let mut arquivo = fs::File::open(arquivo).ok()?;
    let tamanho = arquivo.metadata().ok()?.len();
    let mut fim = tamanho;
    // Começo de linha cortado no bloco anterior (que fica depois deste no arquivo).
    let mut resto: Vec<u8> = Vec::new();
    let mut vazia = None;
    let mut limite = MAXIMO;
    while fim > 0 && tamanho - fim < limite {
        let inicio = fim.saturating_sub(BLOCO);
        let mut bloco = vec![0; (fim - inicio) as usize];
        arquivo.seek(SeekFrom::Start(inicio)).ok()?;
        arquivo.read_exact(&mut bloco).ok()?;
        bloco.extend_from_slice(&resto);
        let corte = if inicio == 0 {
            0
        } else {
            match bloco.iter().position(|&b| b == b'\n') {
                Some(p) => p + 1,
                None => {
                    resto = bloco;
                    fim = inicio;
                    continue;
                }
            }
        };
        for linha in bloco[corte..].split(|&b| b == b'\n').rev() {
            let Ok(texto) = std::str::from_utf8(linha) else {
                continue;
            };
            match ler_linha(texto, mtime) {
                Some(achou) if !achou.vazia => return Some(achou),
                Some(achou) => vazia = vazia.or(Some(achou)),
                None if texto.contains("\"rate_limits\":null") => {}
                None => continue,
            }
            limite = limite.min(tamanho - inicio + DEPOIS_DA_VAZIA);
        }
        resto = bloco[..corte].to_vec();
        fim = inicio;
    }
    vazia
}

/// Só serve `event_msg` → `token_count` com `rate_limits` da cota normal do Codex: o
/// `codex_bengalfox` (Spark) e o `premium` (vem todo nulo nos erros de limite) têm cota à parte.
fn ler_linha(linha: &str, mtime: i64) -> Option<Linha> {
    if !linha.contains("\"rate_limits\"") {
        return None;
    }
    let mut valor: Value = serde_json::from_str(linha).ok()?;
    if valor["type"] != "event_msg" || valor["payload"]["type"] != "token_count" {
        return None;
    }
    let limites = valor["payload"]["rate_limits"].take();
    if !limites.is_object() || !matches!(limites["limit_id"].as_str(), None | Some("codex")) {
        return None;
    }
    let instante = valor["timestamp"]
        .as_str()
        .and_then(segundos_iso)
        .unwrap_or(mtime);
    let vazia = !limites["primary"].is_object() && !limites["secondary"].is_object();
    Some(Linha {
        instante,
        limites,
        vazia,
    })
}

#[cfg(test)]
mod testes {
    use super::*;

    const FIXTURE: &str = r#"{"timestamp":"2025-01-03T12:00:00.000Z","ordinal":7,"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"limit_id":null,"limit_name":null,"primary":{"used_percent":0.0,"window_minutes":60,"resets_at":1800000000},"secondary":{"used_percent":12.5,"window_minutes":10080,"resets_at":1800100000},"credits":null,"individual_limit":null,"spend_control_reached":null,"plan_type":null,"rate_limit_reached_type":null}}}"#;
    const PLUS: &str = r#"{"timestamp":"2026-09-04T10:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"limit_id":"codex","limit_name":null,"primary":{"used_percent":20.0,"window_minutes":300,"resets_at":1788544770},"secondary":{"used_percent":3.0,"window_minutes":10080,"resets_at":1789131570},"credits":{"has_credits":false,"unlimited":false,"balance":"0"},"individual_limit":null,"spend_control_reached":null,"plan_type":"plus","rate_limit_reached_type":null}}}"#;
    const PRO: &str = r#"{"type":"event_msg","timestamp":"2026-09-07T12:00:00.000Z","payload":{"rate_limits":{"limit_id":"codex","primary":{"used_percent":75.0,"window_minutes":10080,"resets_at":1789435329},"secondary":null,"plan_type":"pro"},"type":"token_count","info":null}}"#;
    const ANTIGO: &str = r#"{"timestamp":"2025-10-10T12:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":0.0,"window_minutes":299,"resets_in_seconds":17940},"secondary":{"used_percent":6.0,"window_minutes":10079,"resets_in_seconds":275281}}}}"#;
    const NULO: &str = r#"{"timestamp":"2026-03-01T12:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":null}}"#;
    const SPARK: &str = r#"{"timestamp":"2026-09-04T11:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"limit_id":"codex_bengalfox","primary":{"used_percent":90.0,"window_minutes":300,"resets_at":1788544770},"secondary":null}}}"#;
    const CHAVE_API: &str = r#"{"timestamp":"2026-09-04T11:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"limit_id":"codex","limit_name":null,"primary":null,"secondary":null,"credits":null,"plan_type":null}}}"#;

    /// Uma pasta do Codex de mentira com um arquivo de sessão por dia: (AAAA/MM/DD, linhas).
    fn pasta_codex(nome: &str, sessoes: &[(&str, &[&str])]) -> PathBuf {
        let raiz = std::env::temp_dir().join(format!("vitral-codex-{nome}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&raiz);
        for (dia, linhas) in sessoes {
            let pasta = raiz.join("sessions").join(dia);
            fs::create_dir_all(&pasta).unwrap();
            let nome = format!("rollout-{}T10-00-00-0000.jsonl", dia.replace('/', "-"));
            fs::write(pasta.join(nome), linhas.join("\n") + "\n").unwrap();
        }
        raiz
    }

    #[test]
    fn formato_novo_do_plano_plus() {
        let linha = ler_linha(PLUS, 0).unwrap();
        assert_eq!(linha.instante, segundos_iso("2026-09-04T10:00:00Z").unwrap());
        let raiz = pasta_codex("plus", &[("2026/09/04", &[PLUS, r#"{"type":"event_msg","payload":{"type":"agent_mess"#])]);
        let codex = ler_em(&raiz, 1788540000).unwrap();
        assert_eq!(codex.uso.cinco_horas, Some(0.2));
        assert_eq!(codex.uso.renova_5h, Some(1788544770));
        assert_eq!(codex.uso.semana, Some(0.03));
        assert_eq!(codex.uso.renova_semana, Some(1789131570));
        assert_eq!(codex.uso.medido_em, Some(linha.instante));
        assert_eq!(codex.plano.as_deref(), Some("Plus"));
        assert!(!codex.logado && !codex.sem_limites && !codex.uso.bloqueado);
        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn plano_pro_so_tem_a_semana_e_janela_vai_pelo_tamanho() {
        let raiz = pasta_codex("pro", &[("2026/09/07", &[PRO])]);
        let codex = ler_em(&raiz, 1789000000).unwrap();
        assert_eq!(codex.uso.cinco_horas, None);
        assert_eq!(codex.uso.semana, Some(0.75));
        assert_eq!(codex.plano.as_deref(), Some("Pro 200"));
        // A janela de 60 min da fixture do Codex não é de 5 h: vai para as outras.
        let raiz_fixture = pasta_codex("fixture", &[("2025/01/03", &[FIXTURE])]);
        let codex = ler_em(&raiz_fixture, 1700000000).unwrap();
        assert_eq!(codex.uso.cinco_horas, None);
        assert_eq!(codex.uso.semana, Some(0.125));
        assert_eq!(codex.outras, vec![("Janela de 1 h".to_string(), 0.0, Some(1800000000))]);
        let _ = fs::remove_dir_all(raiz);
        let _ = fs::remove_dir_all(raiz_fixture);
    }

    #[test]
    fn formato_antigo_com_segundos_para_renovar() {
        let raiz = pasta_codex("antigo", &[("2025/10/10", &[ANTIGO, NULO])]);
        let medido = segundos_iso("2025-10-10T12:00:00Z").unwrap();
        let codex = ler_em(&raiz, medido + 60).unwrap();
        assert_eq!(codex.uso.cinco_horas, Some(0.0));
        assert_eq!(codex.uso.renova_5h, Some(medido + 17940));
        assert_eq!(codex.uso.semana, Some(0.06));
        assert_eq!(codex.uso.renova_semana, Some(medido + 275281));
        assert_eq!(codex.plano, None);
        // Passou a renovação da semana: volta a zero.
        let codex = ler_em(&raiz, medido + 300_000).unwrap();
        assert_eq!(codex.uso.semana, Some(0.0));
        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn a_linha_mais_nova_entre_arquivos_vale_e_spark_fica_de_fora() {
        let raiz = pasta_codex("varios", &[("2026/09/04", &[PLUS, SPARK]), ("2025/10/10", &[ANTIGO])]);
        let codex = ler_em(&raiz, 1788540000).unwrap();
        assert_eq!(codex.uso.cinco_horas, Some(0.2));
        assert_eq!(codex.plano.as_deref(), Some("Plus"));
        let _ = fs::remove_dir_all(raiz);
        // Um snapshot sem janelas depois de um com janelas não apaga o que se sabia.
        let raiz = pasta_codex("vazia-depois", &[("2026/09/04", &[PLUS, CHAVE_API])]);
        let codex = ler_em(&raiz, 1788540000).unwrap();
        assert_eq!(codex.uso.semana, Some(0.03));
        assert!(!codex.sem_limites);
        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn sem_pasta_sem_sessao_e_chave_de_api() {
        let inexistente = std::env::temp_dir().join("vitral-codex-que-nao-existe");
        assert_eq!(ler_em(&inexistente, 0), None);
        let raiz = pasta_codex("vazia", &[]);
        fs::create_dir_all(&raiz).unwrap();
        fs::write(raiz.join("auth.json"), "{}").unwrap();
        let codex = ler_em(&raiz, 0).unwrap();
        assert!(codex.logado && !codex.sem_limites);
        assert_eq!(codex.uso, Uso::default());
        let raiz_api = pasta_codex("api", &[("2026/09/04", &[CHAVE_API, NULO])]);
        assert!(ler_em(&raiz_api, 0).unwrap().sem_limites);
        let _ = fs::remove_dir_all(raiz);
        let _ = fs::remove_dir_all(raiz_api);
    }

    #[test]
    fn linha_grande_e_cortada_entre_blocos() {
        let grande = format!(r#"{{"type":"response_item","payload":{{"texto":"{}"}}}}"#, "x".repeat(200_000));
        let raiz = pasta_codex("grande", &[("2026/09/04", &[PLUS, &grande, "{\"type\":"])]);
        let codex = ler_em(&raiz, 1788540000).unwrap();
        assert_eq!(codex.uso.cinco_horas, Some(0.2));
        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn tamanho_da_janela_com_folga() {
        assert_eq!(Tipo::de_minutos(299), Tipo::CincoHoras);
        assert_eq!(Tipo::de_minutos(10_079), Tipo::Semana);
        assert_eq!(Tipo::de_minutos(1440), Tipo::Dia);
        assert_eq!(Tipo::de_minutos(43_200), Tipo::Mes);
        assert_eq!(Tipo::de_minutos(60), Tipo::Outra(60));
        assert_eq!(nome_do_plano("prolite"), "Pro 100");
        assert_eq!(nome_do_plano("edu_plus"), "Edu plus");
    }

    #[test]
    fn numeros_absurdos_nao_derrubam_a_leitura() {
        assert_eq!(Tipo::de_minutos(i64::MAX), Tipo::Outra(i64::MAX));
        assert_eq!(Tipo::de_minutos(i64::MIN), Tipo::Outra(i64::MIN));
        let absurdo = r#"{"timestamp":"2026-09-04T10:00:00.000Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":10.0,"window_minutes":1e300,"resets_in_seconds":1e300},"secondary":null}}}"#;
        let raiz = pasta_codex("absurdo", &[("2026/09/04", &[absurdo])]);
        let codex = ler_em(&raiz, 0).unwrap();
        assert_eq!(codex.outras.len(), 1);
        assert_eq!(codex.outras[0].2, None);
        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn snapshot_vazio_com_plano_nao_e_chave_de_api() {
        let vazia_plus = CHAVE_API.replace(r#""plan_type":null"#, r#""plan_type":"plus""#);
        let raiz = pasta_codex("vazia-plus", &[("2026/09/04", &[&vazia_plus])]);
        let codex = ler_em(&raiz, 0).unwrap();
        assert!(!codex.sem_limites);
        assert_eq!(codex.plano.as_deref(), Some("Plus"));
        assert_eq!(codex.uso, Uso::default());
        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn vazia_no_fim_procura_so_um_trecho_curto() {
        let enchimento = format!(r#"{{"type":"response_item","payload":{{"texto":"{}"}}}}"#, "x".repeat(1000));
        let muitas: Vec<&str> = std::iter::repeat_n(enchimento.as_str(), 600).collect();
        let mut linhas = vec![PLUS];
        linhas.extend(&muitas);
        linhas.push(CHAVE_API);
        // A linha com janelas fica a uns 600 KB da vazia: além do trecho, então não se acha.
        let raiz = pasta_codex("longe", &[("2026/09/04", &linhas[..])]);
        assert!(ler_em(&raiz, 1788540000).unwrap().sem_limites);
        let _ = fs::remove_dir_all(raiz);
        // Perto, acha.
        let raiz = pasta_codex("perto", &[("2026/09/04", &[PLUS, &enchimento, CHAVE_API])]);
        assert_eq!(ler_em(&raiz, 1788540000).unwrap().uso.cinco_horas, Some(0.2));
        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn rele_so_o_arquivo_que_mudou() {
        let raiz = pasta_codex("mudou", &[("2026/09/07", &[PLUS])]);
        assert_eq!(ler_em(&raiz, 1788540000).unwrap().uso.semana, Some(0.03));
        let arquivo = raiz.join("sessions/2026/09/07/rollout-2026-09-07T10-00-00-0000.jsonl");
        fs::write(&arquivo, format!("{PLUS}\n{PRO}\n")).unwrap();
        assert_eq!(ler_em(&raiz, 1788540000).unwrap().uso.semana, Some(0.75));
        let _ = fs::remove_dir_all(raiz);
    }

    #[test]
    fn sessao_retomada_de_data_antiga_entra_pelo_historico() {
        let agora = 1789000000;
        let dias: Vec<String> = (1..=8).map(|d| format!("2026/09/{d:02}")).collect();
        let sessoes: Vec<(&str, &[&str])> = dias.iter().map(|d| (d.as_str(), &[PLUS][..])).collect();
        let raiz = pasta_codex("retomada", &sessoes);
        let id = "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
        let pasta = raiz.join("sessions/2026/08/01");
        fs::create_dir_all(&pasta).unwrap();
        let antigo = pasta.join(format!("rollout-2026-08-01T10-00-00-{id}.jsonl"));
        fs::write(&antigo, format!("{PRO}\n")).unwrap();
        // Retomada agora: o arquivo da data antiga é o que mudou por último.
        let depois = SystemTime::now() + std::time::Duration::from_secs(100);
        fs::File::options().write(true).open(&antigo).unwrap().set_modified(depois).unwrap();
        assert_eq!(ler_em(&raiz, agora).unwrap().uso.semana, Some(0.03));
        let historico = format!(
            "{{\"session_id\":\"outra\",\"ts\":{},\"text\":\"velho\"}}\n\
             {{\"session_id\":\"{id}\",\"ts\":{},\"text\":\"continua\"}}\n",
            agora - SEMANA - 10,
            agora - 100
        );
        fs::write(raiz.join("history.jsonl"), historico).unwrap();
        let codex = ler_em(&raiz, agora).unwrap();
        assert_eq!(codex.uso.semana, Some(0.75));
        assert_eq!(codex.plano.as_deref(), Some("Pro 200"));
        assert_eq!(sessoes_recentes(&raiz, agora - SEMANA), vec![id.to_string()]);
        assert!(da_sessao(Path::new("rollout-2026-08-01T10-00-00-abc_def.jsonl"), "abc"));
        assert!(!da_sessao(Path::new("rollout-2026-08-01T10-00-00-xabc.jsonl"), "abc"));
        assert!(!da_sessao(Path::new("rollout-2026-08-01T10-00-00-abc.jsonl.zst"), "abc"));
        let _ = fs::remove_dir_all(raiz);
    }
}
