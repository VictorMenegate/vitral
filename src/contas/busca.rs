//! O uso do plano buscado direto na Anthropic, com o mesmo pedido que o Claude Code faz
//! (`GET /api/oauth/usage`) e o token OAuth da própria conta: o `cachedUsageUtilization` do
//! `.claude.json` só muda quando o Claude Code busca, e fica horas parado se o uso foi no VS Code,
//! no terminal ou no site.
//!
//! O token sai do `.credentials.json` da conta a cada busca e só vai, num cabeçalho, para
//! `https://api.anthropic.com`, sem seguir redirecionamento; não fica guardado nem vai para log,
//! erro ou `Debug`. Vencido ou recusado, a conta só fica de fora da rodada: quem renova é o Claude
//! Code (renovar aqui trocaria o refresh token e poderia derrubar o login dele). A `Agenda` decide
//! quando buscar e quanto recuar depois de uma falha, que nunca vira aviso.

use std::{
    collections::HashMap,
    fmt, fs,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use super::{Conta, Plano, arquivo_config, ler_plano, ler_uso, pasta_do_login};
use crate::motor::Uso;

/// O servidor do pedido; os testes passam um servidor local no lugar (`buscar_em`).
const SERVIDOR: &str = "https://api.anthropic.com";
const CAMINHO: &str = "/api/oauth/usage";
/// O cabeçalho que libera o token OAuth na API (o mesmo do Claude Code).
const BETA_OAUTH: &str = "oauth-2025-04-20";
/// A permissão que o Claude Code confere antes de buscar o uso.
const PERMISSAO: &str = "user:profile";
/// O prazo do Claude Code para a mesma busca.
const PRAZO: Duration = Duration::from_secs(5);
/// A resposta tem poucos KB.
const LIMITE_RESPOSTA: u64 = 256 * 1024;
/// O token que vence no próximo minuto já conta como vencido.
const MARGEM_MS: i64 = 60_000;

/// O ritmo das rodadas (segundos): com o popover do uso aberto, a cada minuto; fechado, a cada
/// 5 min (os anéis do trilho também mostram o uso); com a janela sem foco há mais de 15 min, a
/// cada 15 min, e há mais de 2 h, a cada 30 min.
pub const COM_POPOVER: u64 = 60;
pub const NORMAL: u64 = 5 * 60;
const FORA: u64 = 15 * 60;
const FORA_HA_MUITO: u64 = 2 * 3600;
const LONGE: u64 = 30 * 60;
/// Abrir o popover ou clicar em "Atualizar" logo depois de uma rodada não manda outra.
const ESPACO_MINIMO: u64 = 15;

/// O que deu a busca de uma conta.
#[derive(Debug, PartialEq)]
pub enum Busca {
    /// O uso agora e o plano (o nome vem do `.claude.json`), medidos neste instante.
    Lida(Uso, Plano),
    /// Sem login do Claude Code na pasta da conta, ou login sem a permissão de perfil: não há o
    /// que buscar.
    SemLogin,
    /// O token venceu e o Claude Code ainda não renovou; nada foi pedido.
    Vencido,
    /// 401 ou 403. Leva o vencimento do token recusado, que o identifica sem guardar o token.
    Recusado { marca: Option<i64> },
    /// O mesmo token que o servidor já recusou: nem foi mandado.
    Repetido,
    /// 429, com o `Retry-After` em segundos quando veio.
    Limite(Option<u64>),
    /// Sem rede, prazo estourado, servidor fora, redirecionamento ou resposta ilegível.
    Falhou,
}

/// O token de acesso. O `Debug` não mostra nada, para ele não escapar num `{:?}`.
struct Token(String);

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(oculto)")
    }
}

/// O login OAuth que o Claude Code guardou numa pasta (`.credentials.json` → `claudeAiOauth`).
#[derive(Debug)]
struct Credencial {
    token: Token,
    /// Quando o token vence (ms Unix).
    vence_em: Option<i64>,
    /// Tem a permissão de ler o perfil (e o uso).
    perfil: bool,
}

/// Lê o login de novo a cada busca: o Claude Code renova o token sozinho e o daqui ficaria velho.
fn ler_credencial(pasta: &Path) -> Option<Credencial> {
    let texto = fs::read_to_string(pasta.join(".credentials.json")).ok()?;
    let mut valor: Value = serde_json::from_str(&texto).ok()?;
    let oauth = valor.get_mut("claudeAiOauth")?;
    let token = match oauth.get_mut("accessToken")?.take() {
        Value::String(token) if !token.is_empty() => token,
        _ => return None,
    };
    let vence = &oauth["expiresAt"];
    let vence_em = vence.as_i64().or_else(|| vence.as_f64().map(|ms| ms as i64));
    // Sem a lista de permissões, vale o que o servidor responder.
    let perfil = oauth["scopes"].as_array().is_none_or(|lista| lista.iter().any(|p| p == PERMISSAO));
    Some(Credencial { token: Token(token), vence_em, perfil })
}

/// Busca o uso de uma conta na Anthropic. `recusado` é a marca do token que o servidor já
/// recusou (esse não vai de novo). Lê arquivos e espera a rede (até 5 s): rodar fora da thread
/// da interface.
pub fn buscar(conta: &Conta, recusado: Option<i64>) -> Busca {
    // Nos testes, nunca o login de verdade nem a rede.
    if cfg!(test) {
        return Busca::SemLogin;
    }
    let Some(login) = pasta_do_login(conta) else {
        return Busca::SemLogin;
    };
    buscar_em(SERVIDOR, &login, arquivo_config(conta).as_deref(), recusado, agora_ms())
}

fn buscar_em(servidor: &str, login: &Path, config: Option<&Path>, recusado: Option<i64>, agora_ms: i64) -> Busca {
    let Some(credencial) = ler_credencial(login).filter(|c| c.perfil) else {
        return Busca::SemLogin;
    };
    let marca = credencial.vence_em;
    if marca.is_some_and(|v| v <= agora_ms.saturating_add(MARGEM_MS)) {
        return Busca::Vencido;
    }
    if marca.is_some() && marca == recusado {
        return Busca::Repetido;
    }
    match pedir(servidor, credencial.token) {
        Ok(utilizacao) => {
            let (uso, plano) = com_o_plano(utilizacao, config, agora_ms);
            Busca::Lida(uso, plano)
        }
        Err(Recusa::Token) => Busca::Recusado { marca },
        Err(Recusa::Limite(depois)) => Busca::Limite(depois),
        Err(Recusa::Falhou) => Busca::Falhou,
    }
}

/// Por que o pedido não trouxe o uso. Sem texto de erro: nada do pedido sai daqui.
enum Recusa {
    Token,
    Limite(Option<u64>),
    Falhou,
}

/// O GET em si. O token entra só no cabeçalho `Authorization` e some junto com o pedido (o ureq
/// só despeja os bytes do pedido no nível `trace` do `log`, e o Vitral não instala logger).
fn pedir(servidor: &str, token: Token) -> Result<Value, Recusa> {
    let de_verdade = servidor == SERVIDOR;
    let config = ureq::Agent::config_builder()
        .user_agent(&format!("Vitral/{}", crate::atualizacao::VERSAO))
        .timeout_global(Some(PRAZO))
        .http_status_as_error(false)
        // Um redirecionamento não leva o token a lugar nenhum: vira falha.
        .max_redirects(0)
        // O servidor de verdade só por HTTPS; o falso dos testes é HTTP local, sem proxy no meio.
        .https_only(de_verdade);
    let config = if de_verdade { config } else { config.proxy(None) };
    let agente = config.build().new_agent();
    let autorizacao = format!("Bearer {}", token.0);
    drop(token);
    let resposta = agente
        .get(&format!("{servidor}{CAMINHO}"))
        .header("Authorization", &autorizacao)
        .header("anthropic-beta", BETA_OAUTH)
        .header("Accept", "application/json")
        .call();
    drop(autorizacao);
    let mut resposta = resposta.map_err(|_| Recusa::Falhou)?;
    match resposta.status().as_u16() {
        200 => {}
        401 | 403 => return Err(Recusa::Token),
        429 => {
            let depois = resposta
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok()?.trim().parse::<u64>().ok());
            return Err(Recusa::Limite(depois));
        }
        _ => return Err(Recusa::Falhou),
    }
    let bytes = resposta
        .body_mut()
        .with_config()
        .limit(LIMITE_RESPOSTA)
        .read_to_vec()
        .map_err(|_| Recusa::Falhou)?;
    // Sem nenhuma das duas janelas não é a resposta do uso (nem que venham nulas).
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .filter(|v| v.get("five_hour").is_some() || v.get("seven_day").is_some())
        .ok_or(Recusa::Falhou)
}

/// O uso buscado e o plano, lidos como se o Claude Code tivesse acabado de guardar a resposta
/// no `.claude.json` da conta (de onde vem o nome do plano).
fn com_o_plano(utilizacao: Value, config: Option<&Path>, agora_ms: i64) -> (Uso, Plano) {
    let conta = config
        .and_then(|arquivo| fs::read_to_string(arquivo).ok())
        .and_then(|texto| serde_json::from_str::<Value>(&texto).ok())
        .and_then(|mut valor| valor.get_mut("oauthAccount").map(Value::take))
        .unwrap_or(Value::Null);
    let valor = json!({
        "oauthAccount": conta,
        "cachedUsageUtilization": { "utilization": utilizacao, "fetchedAtMs": agora_ms },
    });
    let uso = ler_uso(&valor["cachedUsageUtilization"]).unwrap_or_default();
    (uso, ler_plano(&valor))
}

fn agora_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Por que uma conta está segurando as buscas.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Motivo {
    Rede,
    Recusado,
    Limite,
}

#[derive(Clone, Copy, Debug)]
struct Recuo {
    /// Até quando (segundos Unix).
    ate: u64,
    /// Falhas seguidas pelo mesmo motivo: o recuo dobra a cada uma.
    falhas: u32,
    motivo: Motivo,
    /// A marca do token recusado: um token novo (o Claude Code renovou) vai antes do fim.
    marca: Option<i64>,
}

/// Quando buscar o uso e quanto recuar depois de cada falha, por conta.
#[derive(Debug, Default)]
pub struct Agenda {
    /// Quando começou a última rodada (segundos Unix).
    ultima: Option<u64>,
    /// Uma rodada ainda no ar: não começa outra.
    rodando: bool,
    recuos: HashMap<String, Recuo>,
    /// Quando a última tentativa de cada conta não trouxe número.
    falhou_em: HashMap<String, u64>,
}

impl Agenda {
    /// De quanto em quanto tempo buscar (segundos). `fora_ha`: há quanto tempo a janela está sem
    /// foco. Longe da janela o ritmo espaça, mesmo com o popover aberto.
    pub fn intervalo(em_vista: bool, fora_ha: Option<u64>) -> u64 {
        match fora_ha.unwrap_or(0) {
            fora if fora >= FORA_HA_MUITO => LONGE,
            fora if fora >= FORA => FORA,
            _ if em_vista => COM_POPOVER,
            _ => NORMAL,
        }
    }

    /// Hora da rodada automática (a primeira sai logo na abertura).
    pub fn na_hora(&self, agora: u64, em_vista: bool, fora_ha: Option<u64>) -> bool {
        !self.rodando && self.ultima.is_none_or(|u| agora >= u.saturating_add(Self::intervalo(em_vista, fora_ha)))
    }

    /// Uma rodada pedida (o popover abriu, "Atualizar"): só não repete a que acabou de sair.
    pub fn pode_agora(&self, agora: u64) -> bool {
        !self.rodando && self.ultima.is_none_or(|u| agora >= u.saturating_add(ESPACO_MINIMO))
    }

    /// Começa uma rodada e diz quem entra, cada conta com a marca do token já recusado. No
    /// recuo de uma recusa a conta entra com a marca (só um token novo vai); no de rede, só a
    /// pedido ("Atualizar": a rede pode ter voltado); no de limite, não entra.
    pub fn comecar(&mut self, contas: &[Conta], agora: u64, a_pedido: bool) -> Vec<(Conta, Option<i64>)> {
        self.rodando = true;
        self.ultima = Some(agora);
        contas
            .iter()
            .filter_map(|conta| {
                let recusado = match self.recuos.get(&conta.id) {
                    None => None,
                    Some(recuo) if recuo.ate <= agora => None,
                    Some(recuo) => match recuo.motivo {
                        Motivo::Recusado => Some(recuo.marca?),
                        Motivo::Rede if a_pedido => None,
                        Motivo::Rede | Motivo::Limite => return None,
                    },
                };
                Some((conta.clone(), recusado))
            })
            .collect()
    }

    /// A rodada voltou.
    pub fn terminar(&mut self) {
        self.rodando = false;
    }

    /// Anota o que deu a busca de uma conta: sucesso limpa o recuo; recusa, limite e falha de
    /// rede recuam (sem insistir), e o token vencido só espera a próxima rodada, sem pedido.
    pub fn registrar(&mut self, conta: &str, busca: &Busca, agora: u64) {
        let (motivo, marca, depois) = match busca {
            Busca::Lida(..) | Busca::SemLogin => {
                self.recuos.remove(conta);
                self.falhou_em.remove(conta);
                return;
            }
            Busca::Vencido | Busca::Repetido => {
                self.falhou_em.insert(conta.to_string(), agora);
                return;
            }
            Busca::Recusado { marca } => (Motivo::Recusado, *marca, None),
            Busca::Limite(depois) => (Motivo::Limite, None, *depois),
            Busca::Falhou => (Motivo::Rede, None, None),
        };
        self.falhou_em.insert(conta.to_string(), agora);
        let falhas = match self.recuos.get(conta) {
            Some(recuo) if recuo.motivo == motivo => recuo.falhas.saturating_add(1),
            _ => 1,
        };
        let espera = recuo(motivo, falhas, depois);
        self.recuos.insert(conta.to_string(), Recuo { ate: agora.saturating_add(espera), falhas, motivo, marca });
    }

    /// A última tentativa não trouxe número e nada mais novo chegou depois (um evento do turno,
    /// o cache do Claude Code): o rodapé diz, discreto, que não deu para atualizar.
    pub fn sem_atualizar(&self, conta: &str, medido_em: Option<i64>) -> bool {
        self.falhou_em
            .get(conta)
            .is_some_and(|&falha| medido_em.is_none_or(|medido| falha as i64 > medido))
    }
}

/// Quanto esperar (segundos) depois de `falhas` falhas seguidas: a rede, de 2 min dobrando até
/// 30 min; o limite, de 5 min dobrando até 1 h, ou o `Retry-After` se ele pedir mais (até 6 h);
/// a recusa, 1 h (um token novo vai antes).
fn recuo(motivo: Motivo, falhas: u32, depois: Option<u64>) -> u64 {
    let dobrado = |base: u64, teto: u64| base.saturating_mul(1 << falhas.clamp(1, 10).saturating_sub(1)).min(teto);
    match motivo {
        Motivo::Rede => dobrado(2 * 60, 30 * 60),
        Motivo::Limite => dobrado(5 * 60, 3600).max(depois.unwrap_or(0).min(6 * 3600)),
        Motivo::Recusado => 3600,
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    const TOKEN: &str = "sk-ant-oat-falso-0123456789";
    const RENOVACAO: &str = "sk-ant-ort-falso-9876543210";

    /// Um pedido como o servidor falso viu: caminho e os cabeçalhos que importam.
    #[derive(Clone, Debug, Default)]
    struct Visto {
        caminho: String,
        autorizacao: Option<String>,
        beta: Option<String>,
        agente: Option<String>,
        tudo: String,
    }

    /// Servidor local que responde sempre o mesmo e guarda o que recebeu.
    struct Falso {
        base: String,
        vistos: Arc<Mutex<Vec<Visto>>>,
        servidor: Arc<tiny_http::Server>,
    }

    impl Falso {
        fn novo(status: u16, cabecalhos: &[(&str, &str)], corpo: &str) -> Falso {
            let servidor = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
            let porta = servidor.server_addr().to_ip().unwrap().port();
            let vistos = Arc::new(Mutex::new(Vec::new()));
            let cabecalhos: Vec<tiny_http::Header> =
                cabecalhos.iter().map(|(k, v)| tiny_http::Header::from_bytes(*k, *v).unwrap()).collect();
            let (s, v, corpo) = (servidor.clone(), vistos.clone(), corpo.to_string());
            std::thread::spawn(move || {
                for pedido in s.incoming_requests() {
                    let cabecalho = |nome: &str| {
                        pedido.headers().iter().find(|h| h.field.to_string().eq_ignore_ascii_case(nome)).map(|h| h.value.to_string())
                    };
                    let tudo = pedido.headers().iter().map(|h| format!("{}: {}\n", h.field, h.value)).collect();
                    v.lock().unwrap().push(Visto {
                        caminho: pedido.url().to_string(),
                        autorizacao: cabecalho("Authorization"),
                        beta: cabecalho("anthropic-beta"),
                        agente: cabecalho("User-Agent"),
                        tudo,
                    });
                    let mut resposta = tiny_http::Response::from_string(corpo.clone()).with_status_code(status);
                    for c in &cabecalhos {
                        resposta.add_header(c.clone());
                    }
                    let _ = pedido.respond(resposta);
                }
            });
            Falso { base: format!("http://127.0.0.1:{porta}"), vistos, servidor }
        }

        fn vistos(&self) -> Vec<Visto> {
            self.vistos.lock().unwrap().clone()
        }
    }

    impl Drop for Falso {
        fn drop(&mut self) {
            self.servidor.unblock();
        }
    }

    /// Uma pasta de conta de mentira com o login do Claude Code (e, se pedido, o `.claude.json`).
    fn pasta(nome: &str, credenciais: Value) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-busca-{nome}-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&pasta).unwrap();
        fs::write(pasta.join(".credentials.json"), credenciais.to_string()).unwrap();
        pasta
    }

    fn login(vence_em: i64, permissoes: &[&str]) -> Value {
        json!({ "claudeAiOauth": {
            "accessToken": TOKEN, "refreshToken": RENOVACAO, "expiresAt": vence_em,
            "scopes": permissoes, "subscriptionType": "max", "rateLimitTier": "default_claude_max_20x"
        } })
    }

    const AGORA_MS: i64 = 1_793_560_000_000;
    const RESPOSTA: &str = r#"{
        "five_hour": { "utilization": 42.0, "resets_at": "2026-11-01T18:59:59.624047+00:00" },
        "seven_day": { "utilization": 61.5, "resets_at": "2026-11-06T03:59:59+00:00" },
        "seven_day_opus": null,
        "extra_usage": { "is_enabled": true, "monthly_limit": null, "used_credits": 150,
                         "currency": "BRL", "decimal_places": 2 },
        "limits": [ { "kind": "weekly_scoped", "group": "weekly", "percent": 20,
                      "scope": { "model": { "display_name": "Opus" } } } ]
    }"#;

    fn conta(id: &str) -> Conta {
        Conta { id: id.into(), nome: id.into(), email: String::new(), plano: String::new() }
    }

    #[test]
    fn busca_traz_o_uso_e_manda_so_o_token_de_acesso() {
        let falso = Falso::novo(200, &[("Content-Type", "application/json")], RESPOSTA);
        let pasta = pasta("ok", login(AGORA_MS + 3_600_000, &["user:inference", "user:profile"]));
        let config = pasta.join(".claude.json");
        fs::write(&config, r#"{ "oauthAccount": { "planDisplayName": "Max",
            "organizationRateLimitTier": "default_claude_max_20x" }, "projects": {} }"#).unwrap();
        let Busca::Lida(uso, plano) = buscar_em(&falso.base, &pasta, Some(&config), None, AGORA_MS) else {
            panic!("a busca não leu o uso");
        };
        assert_eq!(uso.cinco_horas, Some(0.42));
        assert_eq!(uso.semana, Some(0.615));
        assert_eq!(uso.renova_5h, crate::contas::segundos_iso("2026-11-01T18:59:59Z"));
        // "Atualizado há…" conta da hora da busca.
        assert_eq!(uso.medido_em, Some(AGORA_MS / 1000));
        assert_eq!(plano.nome.as_deref(), Some("Max 20x"));
        assert_eq!(plano.por_modelo, vec![("Opus".to_string(), 0.2, None)]);
        assert_eq!(plano.creditos.map(|c| c.usados), Some(150));
        assert_eq!(plano.medido_em, Some(AGORA_MS / 1000));
        let vistos = falso.vistos();
        assert_eq!(vistos.len(), 1);
        let visto = &vistos[0];
        assert_eq!(visto.caminho, "/api/oauth/usage");
        assert_eq!(visto.autorizacao.as_deref(), Some(format!("Bearer {TOKEN}").as_str()));
        assert_eq!(visto.beta.as_deref(), Some("oauth-2025-04-20"));
        assert_eq!(visto.agente.as_deref(), Some(format!("Vitral/{}", crate::atualizacao::VERSAO).as_str()));
        // O refresh token nunca sai do arquivo.
        assert!(!visto.tudo.contains(RENOVACAO), "{}", visto.tudo);
        // Sem `.claude.json`, o uso vem do mesmo jeito, só sem o nome do plano.
        let Busca::Lida(uso, plano) = buscar_em(&falso.base, &pasta, None, None, AGORA_MS) else {
            panic!("sem o .claude.json a busca falhou");
        };
        assert_eq!(uso.cinco_horas, Some(0.42));
        assert_eq!(plano.nome, None);
        // Nem com um `.claude.json` estranho.
        fs::write(&config, "[1, 2]").unwrap();
        assert!(matches!(buscar_em(&falso.base, &pasta, Some(&config), None, AGORA_MS), Busca::Lida(..)));
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn vencido_sem_login_ou_sem_permissao_nem_pede() {
        let falso = Falso::novo(200, &[], RESPOSTA);
        let vencido = pasta("vencido", login(AGORA_MS - 1, &["user:profile"]));
        assert_eq!(buscar_em(&falso.base, &vencido, None, None, AGORA_MS), Busca::Vencido);
        // Vencendo no próximo minuto já conta como vencido (o Claude Code está para renovar).
        let quase = pasta("quase", login(AGORA_MS + 30_000, &["user:profile"]));
        assert_eq!(buscar_em(&falso.base, &quase, None, None, AGORA_MS), Busca::Vencido);
        let sem_perfil = pasta("sem-perfil", login(AGORA_MS + 3_600_000, &["user:inference"]));
        assert_eq!(buscar_em(&falso.base, &sem_perfil, None, None, AGORA_MS), Busca::SemLogin);
        let vazia = std::env::temp_dir().join(format!("vitral-busca-vazia-{}", uuid::Uuid::new_v4().simple()));
        assert_eq!(buscar_em(&falso.base, &vazia, None, None, AGORA_MS), Busca::SemLogin);
        let sem_token = pasta("sem-token", json!({ "claudeAiOauth": { "accessToken": "" } }));
        assert_eq!(buscar_em(&falso.base, &sem_token, None, None, AGORA_MS), Busca::SemLogin);
        // O token que o servidor já recusou não vai de novo; um renovado vai.
        let recusado = pasta("recusado", login(AGORA_MS + 3_600_000, &["user:profile"]));
        let marca = Some(AGORA_MS + 3_600_000);
        assert_eq!(buscar_em(&falso.base, &recusado, None, marca, AGORA_MS), Busca::Repetido);
        assert!(falso.vistos().is_empty());
        let renovado = Some(AGORA_MS + 1);
        assert!(matches!(buscar_em(&falso.base, &recusado, None, renovado, AGORA_MS), Busca::Lida(..)));
        assert_eq!(falso.vistos().len(), 1);
        for p in [vencido, quase, sem_perfil, sem_token, recusado] {
            fs::remove_dir_all(p).unwrap();
        }
    }

    #[test]
    fn recusa_limite_e_falhas_viram_resultado_sem_o_token() {
        let vence_em = AGORA_MS + 3_600_000;
        let pasta = pasta("erros", login(vence_em, &["user:profile"]));
        let casos = [
            (401, vec![], "{}", Busca::Recusado { marca: Some(vence_em) }),
            (403, vec![], "{}", Busca::Recusado { marca: Some(vence_em) }),
            (429, vec![("Retry-After", "120")], "{}", Busca::Limite(Some(120))),
            (429, vec![], "{}", Busca::Limite(None)),
            (500, vec![], "{}", Busca::Falhou),
            (200, vec![], "não é json", Busca::Falhou),
            (200, vec![], "[1, 2]", Busca::Falhou),
            (200, vec![], "{}", Busca::Falhou),
        ];
        for (status, cabecalhos, corpo, esperado) in casos {
            let falso = Falso::novo(status, &cabecalhos, corpo);
            let busca = buscar_em(&falso.base, &pasta, None, None, AGORA_MS);
            assert_eq!(busca, esperado, "{status} {corpo}");
            assert!(!format!("{busca:?}").contains(TOKEN));
        }
        // Sem servidor nenhum na porta.
        let porta = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let fechado = format!("http://127.0.0.1:{porta}");
        assert_eq!(buscar_em(&fechado, &pasta, None, None, AGORA_MS), Busca::Falhou);
        // O `Debug` do login não mostra o token.
        let credencial = ler_credencial(&pasta).unwrap();
        let texto = format!("{credencial:?}");
        assert!(!texto.contains(TOKEN) && !texto.contains(RENOVACAO), "{texto}");
        assert!(texto.contains("Token(oculto)"));
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn redirecionamento_nao_leva_o_token() {
        let destino = Falso::novo(200, &[], RESPOSTA);
        let local = format!("{}/api/oauth/usage", destino.base);
        let desvio = Falso::novo(302, &[("Location", local.as_str())], "");
        let pasta = pasta("desvio", login(AGORA_MS + 3_600_000, &["user:profile"]));
        assert_eq!(buscar_em(&desvio.base, &pasta, None, None, AGORA_MS), Busca::Falhou);
        assert_eq!(desvio.vistos().len(), 1);
        assert!(destino.vistos().is_empty());
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn nos_testes_a_busca_de_verdade_nao_sai() {
        assert_eq!(buscar(&conta(crate::contas::PRINCIPAL), None), Busca::SemLogin);
    }

    #[test]
    fn ritmo_das_rodadas() {
        assert_eq!(Agenda::intervalo(true, None), 60);
        assert_eq!(Agenda::intervalo(false, None), 300);
        assert_eq!(Agenda::intervalo(true, Some(60)), 60);
        // Longe da janela espaça, mesmo com o popover aberto.
        assert_eq!(Agenda::intervalo(true, Some(15 * 60)), 15 * 60);
        assert_eq!(Agenda::intervalo(false, Some(3 * 3600)), 30 * 60);
        let mut agenda = Agenda::default();
        // A primeira rodada sai logo.
        assert!(agenda.na_hora(1000, false, None));
        let contas = [conta("principal"), conta("x1")];
        assert_eq!(agenda.comecar(&contas, 1000, false).len(), 2);
        // Com uma rodada no ar, nenhuma outra.
        assert!(!agenda.na_hora(5000, false, None));
        assert!(!agenda.pode_agora(5000));
        agenda.terminar();
        assert!(!agenda.na_hora(1000 + 299, false, None));
        assert!(agenda.na_hora(1000 + 300, false, None));
        assert!(agenda.na_hora(1000 + 60, true, None));
        // Abrir o popover logo depois de uma rodada não manda outra.
        assert!(!agenda.pode_agora(1000 + 14));
        assert!(agenda.pode_agora(1000 + 15));
    }

    #[test]
    fn recuo_depois_de_cada_falha() {
        let contas = [conta("a")];
        let mut agenda = Agenda::default();
        let entra = |agenda: &mut Agenda, agora: u64, a_pedido: bool| {
            let da_vez = agenda.comecar(&contas, agora, a_pedido);
            agenda.terminar();
            da_vez.into_iter().map(|(c, marca)| (c.id, marca)).collect::<Vec<_>>()
        };
        // Rede: 2 min, depois 4; "Atualizar" passa por cima (a rede pode ter voltado).
        agenda.registrar("a", &Busca::Falhou, 0);
        assert!(entra(&mut agenda, 119, false).is_empty());
        assert_eq!(entra(&mut agenda, 119, true), vec![("a".to_string(), None)]);
        assert_eq!(entra(&mut agenda, 120, false).len(), 1);
        agenda.registrar("a", &Busca::Falhou, 120);
        assert!(entra(&mut agenda, 120 + 239, false).is_empty());
        assert_eq!(entra(&mut agenda, 120 + 240, false).len(), 1);
        assert_eq!(recuo(Motivo::Rede, 40, None), 30 * 60);
        // O sucesso limpa o recuo.
        agenda.registrar("a", &Busca::Lida(Uso::default(), Plano::default()), 400);
        assert_eq!(entra(&mut agenda, 401, false).len(), 1);
        // Limite: 5 min dobrando, o `Retry-After` quando pede mais, e nem "Atualizar" passa.
        agenda.registrar("a", &Busca::Limite(None), 1000);
        assert!(entra(&mut agenda, 1000 + 299, true).is_empty());
        assert_eq!(entra(&mut agenda, 1000 + 300, false).len(), 1);
        agenda.registrar("a", &Busca::Limite(Some(7200)), 2000);
        assert!(entra(&mut agenda, 2000 + 7199, true).is_empty());
        assert_eq!(entra(&mut agenda, 2000 + 7200, false).len(), 1);
        assert_eq!(recuo(Motivo::Limite, 3, Some(10)), 20 * 60);
        assert_eq!(recuo(Motivo::Limite, 9, Some(u64::MAX)), 6 * 3600);
        // Recusa: 1 h, mas a conta entra com a marca do token recusado (um renovado vai antes).
        agenda.registrar("a", &Busca::Recusado { marca: Some(77) }, 20_000);
        assert_eq!(entra(&mut agenda, 20_001, false), vec![("a".to_string(), Some(77))]);
        agenda.registrar("a", &Busca::Repetido, 20_001);
        assert_eq!(entra(&mut agenda, 20_000 + 3599, false), vec![("a".to_string(), Some(77))]);
        // Passada a hora, o mesmo token tenta de novo uma vez.
        assert_eq!(entra(&mut agenda, 20_000 + 3600, false), vec![("a".to_string(), None)]);
        // Recusa sem como reconhecer o token: só depois da hora.
        agenda.registrar("a", &Busca::Recusado { marca: None }, 30_000);
        assert!(entra(&mut agenda, 30_001, true).is_empty());
        // Vencido não recua: a próxima rodada relê o arquivo.
        let mut outra = Agenda::default();
        outra.registrar("a", &Busca::Vencido, 0);
        assert_eq!(outra.comecar(&contas, 1, false).len(), 1);
    }

    #[test]
    fn rodape_diz_quando_nao_deu_para_atualizar() {
        let mut agenda = Agenda::default();
        assert!(!agenda.sem_atualizar("a", Some(100)));
        agenda.registrar("a", &Busca::Falhou, 200);
        assert!(agenda.sem_atualizar("a", Some(100)));
        assert!(agenda.sem_atualizar("a", None));
        // Um evento do turno depois da falha já atualizou.
        assert!(!agenda.sem_atualizar("a", Some(250)));
        agenda.registrar("a", &Busca::Vencido, 300);
        assert!(agenda.sem_atualizar("a", Some(250)));
        agenda.registrar("a", &Busca::Lida(Uso::default(), Plano::default()), 400);
        assert!(!agenda.sem_atualizar("a", Some(250)));
        // Sem login não há o que dizer.
        agenda.registrar("b", &Busca::SemLogin, 500);
        assert!(!agenda.sem_atualizar("b", None));
    }

    /// Uma busca de verdade com a conta principal: `cargo test -- --ignored busca_de_verdade
    /// --nocapture`. Imprime só as porcentagens; fica fora da suíte (usa o login real e a rede).
    #[test]
    #[ignore]
    fn busca_de_verdade() {
        let casa = dirs::home_dir().unwrap();
        match buscar_em(SERVIDOR, &casa.join(".claude"), Some(&casa.join(".claude.json")), None, agora_ms()) {
            Busca::Lida(uso, plano) => {
                let p = |f: Option<f64>| f.map_or("–".to_string(), |f| format!("{:.1}%", f * 100.0));
                println!("5 h {} · semana {}", p(uso.cinco_horas), p(uso.semana));
                for (modelo, fracao, _) in plano.por_modelo {
                    println!("semana · {modelo} {}", p(Some(fracao)));
                }
            }
            outra => println!("sem leitura: {outra:?}"),
        }
    }
}
