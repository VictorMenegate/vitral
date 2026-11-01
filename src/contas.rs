//! Contas do Claude. A principal é o login normal do Claude Code (`~/.claude`). Cada conta extra
//! tem uma pasta própria usada como `CLAUDE_CONFIG_DIR`, onde só o login é dela: CLAUDE.md,
//! configurações, skills, plugins e as conversas (`projects`) são links para `~/.claude`, então
//! uma conversa pode continuar em qualquer conta e tudo se comporta igual ao Claude Code normal.
//!
//! O cadastro fica em `~/.config/vitral/contas.json`; as pastas em `~/.config/vitral/contas/<id>`.

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{claude::binario_claude, motor::Uso};

/// Id reservado da conta principal (o login normal do Claude Code).
pub const PRINCIPAL: &str = "principal";
/// Escolha que deixa o Vitral usar a conta com mais folga no plano.
pub const AUTOMATICA: &str = "auto";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conta {
    pub id: String,
    pub nome: String,
    pub email: String,
    pub plano: String,
}

impl Conta {
    /// Pasta de configuração da conta (`None` na principal).
    pub fn pasta(&self) -> Option<PathBuf> {
        (self.id != PRINCIPAL).then(|| pasta_contas().map(|p| p.join(&self.id))).flatten()
    }
}

static CONFIG: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// A pasta da configuração (`Dados::config`): `contas.json` e `contas/<id>/` ficam nela.
pub fn usar_pasta(config: PathBuf) {
    let _ = CONFIG.set(config);
}

/// Sem a pasta da configuração não há contas extras: nunca cai em ".".
fn pasta_contas() -> Option<PathBuf> {
    Some(CONFIG.get()?.join("contas"))
}

fn arquivo_cadastro() -> Option<PathBuf> {
    Some(CONFIG.get()?.join("contas.json"))
}

/// Todas as contas, com a principal primeiro.
pub fn carregar() -> Vec<Conta> {
    let mut extras: Vec<Conta> = arquivo_cadastro()
        .and_then(|a| fs::read_to_string(a).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    extras.retain(|c| c.id != PRINCIPAL && c.pasta().is_some_and(|p| p.is_dir()));
    // O email da principal vem depois, pelo `status` (é lento para a abertura da janela).
    let principal = Conta {
        id: PRINCIPAL.into(),
        nome: "Principal".into(),
        email: String::new(),
        plano: String::new(),
    };
    let mut todas = vec![principal];
    todas.extend(extras);
    todas
}

pub fn salvar(contas: &[Conta]) -> Result<()> {
    let extras: Vec<&Conta> = contas.iter().filter(|c| c.id != PRINCIPAL).collect();
    let arquivo = arquivo_cadastro().context("sem a pasta da configuração")?;
    crate::dados::gravar_json_seguro(&arquivo, &extras, true)
}

/// Email e plano de uma conta, pelo `claude auth status`.
pub fn status(pasta: Option<&Path>) -> Result<(String, String)> {
    let mut comando = Command::new(binario_claude());
    comando.args(["auth", "status", "--json"]);
    if let Some(pasta) = pasta {
        comando.env("CLAUDE_CONFIG_DIR", pasta);
    }
    let saida = comando.output()?;
    let valor: Value = serde_json::from_slice(&saida.stdout).context("status ilegível")?;
    if valor["loggedIn"] != true {
        bail!("sem login");
    }
    let texto = |chave: &str| valor[chave].as_str().unwrap_or_default().to_string();
    Ok((texto("email"), texto("subscriptionType")))
}

/// Prepara a pasta de uma conta nova: links para o que é compartilhado com `~/.claude` e um
/// `.claude.json` com os servidores MCP do usuário.
pub fn preparar_pasta(id: &str) -> Result<PathBuf> {
    let pasta = pasta_contas().context("sem a pasta da configuração")?.join(id);
    fs::create_dir_all(&pasta)?;
    let home = dirs::home_dir().context("sem pasta pessoal")?;
    let original = home.join(".claude");
    for item in [
        "CLAUDE.md",
        "settings.json",
        "settings.local.json",
        "skills",
        "plugins",
        "agents",
        "commands",
        "output-styles",
        "projects",
    ] {
        let fonte = original.join(item);
        let destino = pasta.join(item);
        if fonte.exists() && fs::symlink_metadata(&destino).is_err() {
            std::os::unix::fs::symlink(&fonte, &destino)?;
        }
    }
    let config = pasta.join(".claude.json");
    if !config.exists() {
        let mcp = fs::read_to_string(home.join(".claude.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .map(|v| v["mcpServers"].clone())
            .filter(|v| v.is_object())
            .unwrap_or_else(|| serde_json::json!({}));
        let conteudo = serde_json::json!({ "hasCompletedOnboarding": true, "mcpServers": mcp });
        crate::dados::gravar_json_seguro(&config, &conteudo, false)?;
    }
    Ok(pasta)
}

pub fn apagar_pasta(id: &str) {
    if id != PRINCIPAL && !id.is_empty() && !id.contains('/') && !id.starts_with('.')
        && let Some(pasta) = pasta_contas()
    {
        let _ = fs::remove_dir_all(pasta.join(id));
    }
}

/// Um `claude auth login` em andamento: mostra o endereço de login e espera o código.
pub struct Login {
    pub id: String,
    pub pasta: PathBuf,
    filho: Child,
    entrada: Option<ChildStdin>,
}

impl Login {
    /// Começa o login e devolve o endereço que o usuário precisa abrir (o próprio Claude Code
    /// também tenta abrir o navegador).
    pub fn iniciar(id: &str) -> Result<(Self, String)> {
        let pasta = preparar_pasta(id)?;
        let mut filho = Command::new(binario_claude())
            .args(["auth", "login", "--claudeai"])
            .env("CLAUDE_CONFIG_DIR", &pasta)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("não abriu o claude")?;
        let saida = filho.stdout.take().unwrap();
        let (envia, recebe) = mpsc::channel();
        std::thread::spawn(move || {
            for linha in BufReader::new(saida).lines().map_while(Result::ok) {
                if let Some(inicio) = linha.find("https://") {
                    let _ = envia.send(linha[inicio..].trim().to_string());
                }
            }
        });
        let endereco = recebe
            .recv_timeout(Duration::from_secs(20))
            .context("o Claude Code não mostrou o endereço de login")?;
        let entrada = filho.stdin.take();
        Ok((
            Self {
                id: id.to_string(),
                pasta,
                filho,
                entrada,
            },
            endereco,
        ))
    }

    /// Manda o código copiado da página de login e espera o Claude Code terminar.
    pub fn concluir(mut self, codigo: &str) -> Result<(String, String)> {
        let mut entrada = self.entrada.take().context("login já encerrado")?;
        writeln!(entrada, "{}", codigo.trim())?;
        entrada.flush()?;
        drop(entrada);
        let fim = std::time::Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(estado) = self.filho.try_wait()? {
                if !estado.success() {
                    let mut erro = String::new();
                    if let Some(mut stderr) = self.filho.stderr.take() {
                        let _ = std::io::Read::read_to_string(&mut stderr, &mut erro);
                    }
                    bail!("o login falhou: {}", erro.trim());
                }
                break;
            }
            if std::time::Instant::now() > fim {
                let _ = self.filho.kill();
                bail!("o login não respondeu");
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        status(Some(&self.pasta))
    }
}

impl Drop for Login {
    fn drop(&mut self) {
        if self.filho.try_wait().ok().flatten().is_none() {
            let _ = self.filho.kill();
            let _ = self.filho.wait();
        }
    }
}

/// O que o `.claude.json` de uma conta diz do plano além das duas janelas.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plano {
    /// Nome para mostrar, como "Ultra · Max 20x".
    pub nome: Option<String>,
    /// Limites semanais por modelo já em uso: nome do modelo, fração e renovação.
    pub por_modelo: Vec<(String, f64, Option<i64>)>,
    /// Créditos extras, quando estão ligados ou pararam sem o usuário desligar.
    pub creditos: Option<Creditos>,
    /// Quando os limites por modelo e os créditos foram medidos (segundos Unix): o `fetchedAtMs`
    /// do cache ou a hora da busca na Anthropic.
    pub medido_em: Option<i64>,
}

impl Plano {
    /// Junta uma leitura nova ao que já se sabia: os limites por modelo e os créditos ficam os da
    /// medição mais recente (o cache do Claude Code costuma ser mais velho que a busca, e uma
    /// releitura dele não desfaz o que ela trouxe); o nome do plano vem sempre da leitura nova.
    pub fn juntar(self, lido: Plano) -> Plano {
        if lido.medido_em < self.medido_em {
            Plano { nome: lido.nome, ..self }
        } else {
            lido
        }
    }
}

/// Créditos extras (`extra_usage`), em unidades mínimas da moeda (centavos).
#[derive(Clone, Debug, PartialEq)]
pub struct Creditos {
    pub usados: i64,
    /// Limite do mês (`None`: sem limite).
    pub limite: Option<i64>,
    pub moeda: String,
    pub casas: u32,
    /// Por que não estão cobrindo envios agora (`None`: estão cobrindo).
    pub parada: Option<Parada>,
}

/// Por que os créditos extras pararam de cobrir envios.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Parada {
    /// O limite do mês (da conta ou da organização) foi atingido.
    LimiteDoMes,
    SemCreditos,
    /// Outro motivo, como a organização ter desligado.
    Desligados,
}

/// Uso do plano que o próprio Claude Code guardou da última vez (`cachedUsageUtilization`) e os
/// detalhes do plano (`oauthAccount`); o `.claude.json` não tem tokens.
fn guardado(conta: &Conta) -> Option<(Uso, Plano)> {
    let valor: Value = serde_json::from_str(&fs::read_to_string(arquivo_config(conta)?).ok()?).ok()?;
    let uso = ler_uso(&valor["cachedUsageUtilization"]).unwrap_or_default();
    Some((uso, ler_plano(&valor)))
}

/// O `.claude.json` da conta: o da pasta dela ou, na principal, o `~/.claude.json`.
fn arquivo_config(conta: &Conta) -> Option<PathBuf> {
    match conta.pasta() {
        Some(pasta) => Some(pasta.join(".claude.json")),
        None => Some(dirs::home_dir()?.join(".claude.json")),
    }
}

/// O uso guardado e o plano de cada conta (lê arquivos: rodar fora da thread da interface).
pub fn usos_guardados(contas: &[Conta]) -> Vec<(String, Uso, Plano)> {
    contas
        .iter()
        .filter_map(|c| {
            let (uso, plano) = guardado(c)?;
            Some((c.id.clone(), uso, plano))
        })
        .collect()
}

/// Nome do plano, limites semanais por modelo com uso (`limits`) e créditos extras.
fn ler_plano(valor: &Value) -> Plano {
    let conta = &valor["oauthAccount"];
    let exibido = conta["planDisplayName"].as_str().filter(|s| !s.is_empty());
    let nivel = conta["organizationRateLimitTier"].as_str().and_then(nome_do_nivel);
    // "Max" com "Max 5x" fica só "Max 5x"; nomes diferentes de fato vão juntos.
    let nome = match (exibido, nivel) {
        (Some(e), Some(n)) if n.to_lowercase().starts_with(&e.to_lowercase()) => Some(n),
        (Some(e), Some(n)) if e.to_lowercase().starts_with(&n.to_lowercase()) => {
            Some(e.to_string())
        }
        (Some(e), Some(n)) => Some(format!("{e} · {n}")),
        (Some(e), None) => Some(e.to_string()),
        (None, n) => n,
    };
    let uso = &valor["cachedUsageUtilization"]["utilization"];
    let por_modelo = uso["limits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| l["group"] == "weekly" && l["kind"] != "weekly_all")
        .filter_map(|l| {
            let porcento = l["percent"].as_f64().filter(|p| *p > 0.0)?;
            let escopo = &l["scope"];
            let nome = escopo["model"]["display_name"]
                .as_str()
                .or(escopo["surface"]["display_name"].as_str())?;
            let renova = l["resets_at"].as_str().and_then(segundos_iso);
            Some((nome.to_string(), porcento / 100.0, renova))
        })
        .collect();
    let extra = &uso["extra_usage"];
    let inteiro = |v: &Value| v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64));
    // O `is_enabled` fica falso enquanto os créditos não cobrem envios, inclusive no limite do
    // mês, que é quando a linha mais importa; só some se o usuário desligou ou nunca ligou.
    let ligados = extra["is_enabled"] == true;
    let no_limite = extra["spend_limit_reached"] == true;
    let parados_sozinhos = extra["credits_ever_enabled"] == true && extra["user_disabled"] != true;
    let parada = match extra["disabled_reason"].as_str() {
        _ if no_limite => Some(Parada::LimiteDoMes),
        _ if ligados => None,
        Some("out_of_credits") => Some(Parada::SemCreditos),
        Some("org_spend_cap_reached") => Some(Parada::LimiteDoMes),
        _ => Some(Parada::Desligados),
    };
    let creditos = (ligados || no_limite || parados_sozinhos).then(|| Creditos {
        usados: inteiro(&extra["used_credits"]).unwrap_or(0),
        limite: inteiro(&extra["monthly_limit"]),
        moeda: extra["currency"].as_str().unwrap_or("USD").to_string(),
        casas: extra["decimal_places"].as_u64().unwrap_or(2).min(6) as u32,
        parada,
    });
    let medido_em = valor["cachedUsageUtilization"]["fetchedAtMs"].as_f64().map(|ms| (ms / 1000.0) as i64);
    Plano {
        nome,
        por_modelo,
        creditos,
        medido_em,
    }
}

/// "default_claude_max_20x" vira "Max 20x"; o nível sem plano pago (`claude_ai`) não tem nome.
fn nome_do_nivel(nivel: &str) -> Option<String> {
    let resto = nivel.strip_prefix("default_").unwrap_or(nivel);
    let resto = resto.strip_prefix("claude_").unwrap_or(resto);
    let mut partes = resto.split('_').filter(|p| !p.is_empty());
    let primeira = partes.next().filter(|p| *p != "ai" && *p != "claude")?;
    let mut letras = primeira.chars();
    let mut nome: String = letras.next()?.to_uppercase().chain(letras).collect();
    for parte in partes {
        nome.push(' ');
        nome.push_str(parte);
    }
    Some(nome)
}

/// Lê o `cachedUsageUtilization`: porcentagens de 0 a 100, horários ISO 8601 e `fetchedAtMs`.
fn ler_uso(guardado: &Value) -> Option<Uso> {
    let uso = &guardado["utilization"];
    if !uso.is_object() {
        return None;
    }
    let fracao = |chave: &str| uso[chave]["utilization"].as_f64().map(|v| v / 100.0);
    let renova = |chave: &str| uso[chave]["resets_at"].as_str().and_then(segundos_iso);
    Some(Uso {
        cinco_horas: fracao("five_hour"),
        semana: fracao("seven_day"),
        bloqueado: false,
        renova_5h: renova("five_hour"),
        renova_semana: renova("seven_day"),
        medido_em: guardado["fetchedAtMs"].as_f64().map(|ms| (ms / 1000.0) as i64),
    })
}

/// Segundos Unix de um horário ISO 8601 como `2026-10-31T15:59:59.624047+00:00` (ou com `Z`).
/// Cada parte é lida num inteiro pequeno: um número absurdo vira `None`, não estouro.
pub(crate) fn segundos_iso(texto: &str) -> Option<i64> {
    let (data, resto) = texto.split_once('T')?;
    let mut partes = data.splitn(3, '-');
    let ano = i64::from(partes.next()?.parse::<u16>().ok()?);
    let mut partes = partes.map(|p| p.parse::<u8>().ok().map(i64::from));
    let (mes, dia) = (partes.next()??, partes.next()??);
    let fim = resto.find(['Z', '+', '-']).unwrap_or(resto.len());
    let (hora, fuso) = resto.split_at(fim);
    let mut partes = hora
        .split(':')
        .map(|p| p.split('.').next()?.parse::<u8>().ok().map(i64::from));
    let (h, m) = (partes.next()??, partes.next()??);
    let s = partes.next().flatten().unwrap_or(0);
    let deslocamento = match fuso {
        "" | "Z" => 0,
        _ => {
            let (fh, fm) = fuso[1..].split_once(':').unwrap_or((&fuso[1..], "0"));
            let (fh, fm) = (i64::from(fh.parse::<u8>().ok()?), i64::from(fm.parse::<u8>().ok()?));
            let segundos = fh * 3600 + fm * 60;
            if fuso.starts_with('-') { -segundos } else { segundos }
        }
    };
    Some(dias_desde_1970(ano, mes, dia) * 86_400 + h * 3600 + m * 60 + s - deslocamento)
}

/// Dias de 1970-01-01 até a data (algoritmo de Howard Hinnant, sem depender de biblioteca).
fn dias_desde_1970(ano: i64, mes: i64, dia: i64) -> i64 {
    let ano = if mes <= 2 { ano - 1 } else { ano };
    let era = ano.div_euclid(400);
    let do_ciclo = ano - era * 400;
    let do_ano = (153 * ((mes + 9) % 12) + 2) / 5 + dia - 1;
    era * 146_097 + do_ciclo * 365 + do_ciclo / 4 - do_ciclo / 100 + do_ano - 719_468
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    #[test]
    fn le_porcentagens_renovacao_e_hora_da_medicao() {
        let guardado = json!({
            "fetchedAtMs": 1793461962706_i64,
            "utilization": {
                "five_hour": { "utilization": 7, "resets_at": "2026-10-31T15:59:59.624047+00:00" },
                "seven_day": { "utilization": 38.5, "resets_at": "2026-11-02T03:59:59-03:00" },
                "seven_day_opus": null
            }
        });
        let uso = ler_uso(&guardado).unwrap();
        assert_eq!(uso.cinco_horas, Some(0.07));
        assert_eq!(uso.semana, Some(0.385));
        assert_eq!(uso.renova_5h, Some(1793462399));
        assert_eq!(uso.renova_semana, Some(1793602799));
        assert_eq!(uso.medido_em, Some(1793461962));
        assert!(!uso.bloqueado);
    }

    #[test]
    fn campos_que_faltam_ficam_sem_dado() {
        let guardado = json!({ "utilization": { "five_hour": null, "seven_day": { "utilization": 0 } } });
        let uso = ler_uso(&guardado).unwrap();
        assert_eq!(uso.cinco_horas, None);
        assert_eq!(uso.renova_5h, None);
        assert_eq!(uso.semana, Some(0.0));
        assert_eq!(uso.renova_semana, None);
        assert_eq!(uso.medido_em, None);
        assert!(ler_uso(&json!(null)).is_none());
        assert!(ler_uso(&json!({ "fetchedAtMs": 1 })).is_none());
    }

    #[test]
    fn plano_limites_por_modelo_e_creditos() {
        let valor = json!({
            "oauthAccount": {
                "planDisplayName": "Ultra",
                "organizationRateLimitTier": "default_claude_max_20x"
            },
            "cachedUsageUtilization": { "utilization": {
                "extra_usage": {
                    "is_enabled": true, "monthly_limit": null, "used_credits": 4232,
                    "currency": "BRL", "decimal_places": 2
                },
                "limits": [
                    { "kind": "session", "group": "session", "percent": 7, "scope": null },
                    { "kind": "weekly_all", "group": "weekly", "percent": 38, "scope": null },
                    { "kind": "weekly_scoped", "group": "weekly", "percent": 0,
                      "scope": { "model": { "display_name": "Fable" } } },
                    { "kind": "weekly_scoped", "group": "weekly", "percent": 12.5,
                      "resets_at": "2026-11-02T07:00:00+00:00",
                      "scope": { "model": { "id": null, "display_name": "Opus" }, "surface": null } }
                ]
            } }
        });
        let plano = ler_plano(&valor);
        assert_eq!(plano.nome.as_deref(), Some("Ultra · Max 20x"));
        assert_eq!(
            plano.por_modelo,
            vec![("Opus".to_string(), 0.125, segundos_iso("2026-11-02T07:00:00Z"))]
        );
        let creditos = plano.creditos.unwrap();
        assert_eq!((creditos.usados, creditos.limite, creditos.casas), (4232, None, 2));
        assert_eq!(creditos.moeda, "BRL");
        assert_eq!(creditos.parada, None);
        // Sem nada no arquivo, sem detalhes; créditos desligados não aparecem.
        assert_eq!(ler_plano(&json!({})), Plano::default());
        let desligado = json!({ "cachedUsageUtilization": { "utilization": {
            "extra_usage": { "is_enabled": false, "used_credits": 10 } } } });
        assert_eq!(ler_plano(&desligado).creditos, None);
        assert_eq!(nome_do_nivel("default_claude_max_5x").as_deref(), Some("Max 5x"));
        assert_eq!(nome_do_nivel("default_claude_ai"), None);
        let so_nivel = json!({ "oauthAccount": { "organizationRateLimitTier": "default_claude_pro" } });
        assert_eq!(ler_plano(&so_nivel).nome.as_deref(), Some("Pro"));
        let max = json!({ "oauthAccount": {
            "planDisplayName": "Max", "organizationRateLimitTier": "default_claude_max_5x" } });
        assert_eq!(ler_plano(&max).nome.as_deref(), Some("Max 5x"));
    }

    #[test]
    fn plano_fica_com_os_limites_da_medicao_mais_nova() {
        let lido = |nome: &str, fracao: f64, medido_em: Option<i64>| Plano {
            nome: Some(nome.into()),
            por_modelo: vec![("Opus".into(), fracao, None)],
            medido_em,
            ..Plano::default()
        };
        let busca = lido("Max 20x", 0.3, Some(2000));
        // O cache mais velho traz o nome, mas não desfaz os limites da busca.
        let junto = busca.clone().juntar(lido("Max 5x", 0.1, Some(1000)));
        assert_eq!(junto.nome.as_deref(), Some("Max 5x"));
        assert_eq!((junto.por_modelo[0].1, junto.medido_em), (0.3, Some(2000)));
        assert_eq!(busca.clone().juntar(lido("Max 20x", 0.1, None)).por_modelo[0].1, 0.3);
        // Uma medição mais nova (ou a do mesmo instante) vale inteira.
        assert_eq!(busca.clone().juntar(lido("Max 20x", 0.5, Some(3000))), lido("Max 20x", 0.5, Some(3000)));
        assert_eq!(Plano::default().juntar(lido("Pro", 0.1, None)), lido("Pro", 0.1, None));
    }

    #[test]
    fn creditos_parados_continuam_aparecendo() {
        let plano = |extra: Value| {
            ler_plano(&json!({ "cachedUsageUtilization": { "utilization": { "extra_usage": extra } } }))
                .creditos
        };
        // No limite do mês o servidor desliga o `is_enabled`; é aí que a linha mais importa.
        let no_limite = plano(json!({
            "is_enabled": false, "spend_limit_reached": true, "monthly_limit": 10000,
            "used_credits": 10000, "currency": "BRL", "decimal_places": 2,
            "disabled_reason": "org_level_disabled_until", "user_disabled": false,
            "credits_ever_enabled": true
        }))
        .unwrap();
        assert_eq!(no_limite.parada, Some(Parada::LimiteDoMes));
        assert_eq!((no_limite.usados, no_limite.limite), (10000, Some(10000)));
        let sem_creditos = plano(json!({
            "is_enabled": false, "spend_limit_reached": false, "used_credits": 500,
            "disabled_reason": "out_of_credits", "user_disabled": false, "credits_ever_enabled": true
        }));
        assert_eq!(sem_creditos.unwrap().parada, Some(Parada::SemCreditos));
        let outro = plano(json!({ "is_enabled": false, "credits_ever_enabled": true }));
        assert_eq!(outro.unwrap().parada, Some(Parada::Desligados));
        // Desligados pelo usuário somem.
        let do_usuario = plano(json!({
            "is_enabled": false, "spend_limit_reached": false, "user_disabled": true,
            "credits_ever_enabled": true, "used_credits": 500
        }));
        assert_eq!(do_usuario, None);
    }

    #[test]
    fn horario_iso_em_varios_formatos() {
        assert_eq!(segundos_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(segundos_iso("2024-02-29T12:00:00Z"), Some(1709208000));
        assert_eq!(segundos_iso("2026-10-31T12:59:59.9-03:00"), Some(1793462399));
        assert_eq!(segundos_iso("2026-10-31T15:59:59"), Some(1793462399));
        assert_eq!(segundos_iso("ontem"), None);
        assert_eq!(segundos_iso("2026-10-31Tdez:00:00Z"), None);
        // Número absurdo não estoura.
        assert_eq!(segundos_iso("99999999999999999-01-01T00:00:00Z"), None);
        assert_eq!(segundos_iso("2026-10-31T99999999999:00:00Z"), None);
        assert_eq!(segundos_iso("2026-10-31T00:00:00+99999999999:00"), None);
    }
}
