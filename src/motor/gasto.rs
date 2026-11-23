//! Onde o uso foi: o livro dos turnos terminados, com a hora, o chat, a pasta, o modelo, a conta,
//! o custo e os tokens de cada um.
//!
//! Cada `result` do Claude Code traz os tokens do turno (`Evento::Gasto`, de
//! `tokens_do_resultado`); o custo é o que o `Chat::somar_custo` somou no fim do turno. O livro
//! fica fora do estado, em `gastos/AAAA-MM.jsonl` na pasta de dados, uma linha por turno, gravado
//! pelo gravador do `dados.rs`. Na abertura entram o mês de agora e o anterior: é o que o "7 dias"
//! do diálogo das contas e a linha "Trabalhou" das conversas recentes precisam, e a memória não
//! cresce com os anos.

use std::{collections::HashMap, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

/// Os tokens de um turno: a entrada nova (sem desconto), o que foi lido do cache, o que foi
/// gravado nele e a saída.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    #[serde(default, skip_serializing_if = "zero")]
    pub entrada: u64,
    #[serde(default, skip_serializing_if = "zero")]
    pub cache_lido: u64,
    #[serde(default, skip_serializing_if = "zero")]
    pub cache_criado: u64,
    #[serde(default, skip_serializing_if = "zero")]
    pub saida: u64,
}

fn zero(n: &u64) -> bool {
    *n == 0
}

impl Tokens {
    pub fn total(&self) -> u64 {
        self.entrada + self.cache_lido + self.cache_criado + self.saida
    }

    pub fn somar(&mut self, outro: Tokens) {
        self.entrada += outro.entrada;
        self.cache_lido += outro.cache_lido;
        self.cache_criado += outro.cache_criado;
        self.saida += outro.saida;
    }
}

/// Os tokens que um `result` do Claude Code declara. O `model_usage_since_last_result` conta tudo
/// o que rodou desde o `result` anterior, com os subagentes, por modelo; sem ele (versão mais
/// velha), vale o `usage`, que é só o laço principal do turno. O `modelUsage` não serve: é o
/// total da sessão. `None` quando o `result` não traz número (o de um processo que caiu ao abrir).
pub fn tokens_do_resultado(valor: &Value) -> Option<Tokens> {
    let n = |v: &Value| v.as_u64().unwrap_or(0);
    let tokens = match valor["model_usage_since_last_result"].as_object() {
        Some(modelos) => modelos.values().fold(Tokens::default(), |mut soma, m| {
            soma.somar(Tokens {
                entrada: n(&m["inputTokens"]),
                cache_lido: n(&m["cacheReadInputTokens"]),
                cache_criado: n(&m["cacheCreationInputTokens"]),
                saida: n(&m["outputTokens"]),
            });
            soma
        }),
        None => {
            let uso = &valor["usage"];
            Tokens {
                entrada: n(&uso["input_tokens"]),
                cache_lido: n(&uso["cache_read_input_tokens"]),
                cache_criado: n(&uso["cache_creation_input_tokens"]),
                saida: n(&uso["output_tokens"]),
            }
        }
    };
    (tokens.total() > 0).then_some(tokens)
}

/// Os tokens do `turn.completed` do Codex: o `input_tokens` dele já inclui o que veio do cache.
pub fn tokens_do_codex(valor: &Value) -> Option<Tokens> {
    let n = |chave: &str| valor["usage"][chave].as_u64().unwrap_or(0);
    let cache_lido = n("cached_input_tokens");
    let tokens = Tokens {
        entrada: n("input_tokens").saturating_sub(cache_lido),
        cache_lido,
        cache_criado: 0,
        saida: n("output_tokens"),
    };
    (tokens.total() > 0).then_some(tokens)
}

/// Um turno terminado, uma linha do livro.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Turno {
    /// Quando terminou (segundos Unix).
    pub em: u64,
    pub chat: String,
    pub pasta: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub modelo: String,
    /// A conta do Claude em que o turno rodou (vazio no Codex).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub conta: String,
    /// O custo estimado pelo Claude Code (US$); 0 no Codex.
    #[serde(default)]
    pub custo: f64,
    #[serde(flatten)]
    pub tokens: Tokens,
    /// A mensagem que abriu o turno: o índice dela na conversa e a hora. É por ela que a linha
    /// "Trabalhou" acha o gasto do turno (os dois precisam bater: depois de um reenvio, o mesmo
    /// índice é de outra mensagem).
    #[serde(default)]
    pub item: usize,
    #[serde(default)]
    pub item_em: u64,
}

/// O gasto somado de um trecho (um turno, um chat, uma pasta).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gasto {
    pub custo: f64,
    pub tokens: Tokens,
    pub turnos: usize,
}

impl Gasto {
    fn somar(&mut self, turno: &Turno) {
        self.custo += turno.custo;
        self.tokens.somar(turno.tokens);
        self.turnos += 1;
    }

    /// O número que ordena e mede as barras: o custo quando o período tem custo (o Claude), os
    /// tokens quando não tem (só Codex).
    pub fn peso(&self, por_custo: bool) -> f64 {
        if por_custo { self.custo } else { self.tokens.total() as f64 }
    }
}

/// O gasto de um chat dentro de uma pasta.
#[derive(Clone, Debug, PartialEq)]
pub struct GastoDoChat {
    pub chat: String,
    pub gasto: Gasto,
}

/// O gasto de uma pasta, com os chats que mais gastaram primeiro.
#[derive(Clone, Debug, PartialEq)]
pub struct GastoDaPasta {
    pub pasta: String,
    pub gasto: Gasto,
    pub chats: Vec<GastoDoChat>,
}

/// O resumo de um período: as pastas, da que mais gastou para a que menos, e o total.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resumo {
    pub pastas: Vec<GastoDaPasta>,
    pub total: Gasto,
    /// As barras medem o custo (há custo no período) ou os tokens.
    pub por_custo: bool,
}

/// Soma os turnos de `desde` em diante por pasta e, dentro de cada uma, por chat.
pub fn resumir(turnos: &[Turno], desde: u64) -> Resumo {
    let mut pastas: Vec<GastoDaPasta> = Vec::new();
    let mut total = Gasto::default();
    for turno in turnos.iter().filter(|t| t.em >= desde) {
        total.somar(turno);
        let pasta = match pastas.iter_mut().find(|p| p.pasta == turno.pasta) {
            Some(pasta) => pasta,
            None => {
                pastas.push(GastoDaPasta { pasta: turno.pasta.clone(), gasto: Gasto::default(), chats: Vec::new() });
                pastas.last_mut().expect("acabou de entrar")
            }
        };
        pasta.gasto.somar(turno);
        match pasta.chats.iter_mut().find(|c| c.chat == turno.chat) {
            Some(chat) => chat.gasto.somar(turno),
            None => {
                let mut gasto = Gasto::default();
                gasto.somar(turno);
                pasta.chats.push(GastoDoChat { chat: turno.chat.clone(), gasto });
            }
        }
    }
    let por_custo = total.custo > 0.0;
    let ordem = |a: &Gasto, b: &Gasto| b.peso(por_custo).total_cmp(&a.peso(por_custo));
    pastas.sort_by(|a, b| ordem(&a.gasto, &b.gasto));
    for pasta in &mut pastas {
        pasta.chats.sort_by(|a, b| ordem(&a.gasto, &b.gasto));
    }
    Resumo { pastas, total, por_custo }
}

/// O começo do dia local de `agora` (segundos Unix): o "Hoje" do diálogo.
pub fn comeco_do_dia(agora: u64) -> u64 {
    use chrono::{TimeZone as _, Timelike as _};
    let local = crate::modelo::no_fuso_local(agora as i64);
    let meia_noite = local.date_naive().and_hms_opt(0, 0, 0).and_then(|d| chrono::Local.from_local_datetime(&d).earliest());
    match meia_noite {
        Some(inicio) => inicio.timestamp().max(0) as u64,
        // Dia sem meia-noite (virada de horário de verão): a hora de agora menos o que já passou.
        None => agora.saturating_sub(u64::from(local.num_seconds_from_midnight())),
    }
}

/// "2026-11": o mês local do instante, que dá nome ao arquivo.
pub fn mes_de(em: u64) -> String {
    crate::modelo::no_fuso_local(em as i64).format("%Y-%m").to_string()
}

/// O mês anterior ao de "2026-11" ("2026-10"; de janeiro, o dezembro do ano anterior).
fn mes_anterior(mes: &str) -> Option<String> {
    let (ano, numero) = mes.split_once('-')?;
    let (ano, numero): (i32, u32) = (ano.parse().ok()?, numero.parse().ok()?);
    Some(if numero <= 1 { format!("{:04}-12", ano - 1) } else { format!("{ano:04}-{:02}", numero - 1) })
}

/// O livro na memória: os turnos em ordem de hora, onde estão os de cada chat (a conversa
/// pergunta por turno a cada desenho) e, por chat, os tokens dos `result` do turno que ainda não
/// terminou.
#[derive(Debug, Default)]
pub struct Livro {
    turnos: Vec<Turno>,
    por_chat: HashMap<String, Vec<usize>>,
    pendentes: HashMap<String, Tokens>,
}

impl Livro {
    /// Lê o mês de agora e o anterior. Linha estragada (uma queda no meio não acontece, o arquivo
    /// troca inteiro; mas uma versão mais nova pode gravar outra coisa) é pulada.
    pub fn ler(pasta: &Path, agora: u64) -> Livro {
        let atual = mes_de(agora);
        let mut turnos = Vec::new();
        for mes in mes_anterior(&atual).into_iter().chain([atual]) {
            if let Ok(texto) = std::fs::read_to_string(pasta.join(format!("{mes}.jsonl"))) {
                turnos.extend(texto.lines().filter_map(|linha| serde_json::from_str::<Turno>(linha).ok()));
            }
        }
        turnos.sort_by_key(|t| t.em);
        let mut por_chat: HashMap<String, Vec<usize>> = HashMap::new();
        for (indice, turno) in turnos.iter().enumerate() {
            por_chat.entry(turno.chat.clone()).or_default().push(indice);
        }
        Livro { turnos, por_chat, pendentes: HashMap::new() }
    }

    pub fn turnos(&self) -> &[Turno] {
        &self.turnos
    }

    /// Os tokens de mais um `result` do turno em andamento do chat.
    pub fn somar_pendente(&mut self, chat: &str, tokens: Tokens) {
        self.pendentes.entry(chat.to_string()).or_default().somar(tokens);
    }

    /// Fecha o turno: os tokens pendentes do chat viram a linha. Sem custo e sem token (o turno
    /// que falhou ao abrir), nada entra. Devolve o mês e o conteúdo do arquivo dele, para gravar.
    pub fn fechar(&mut self, mut turno: Turno) -> Option<(String, Vec<u8>)> {
        turno.tokens = self.pendentes.remove(&turno.chat).unwrap_or_default();
        if turno.custo <= 0.0 && turno.tokens.total() == 0 {
            return None;
        }
        let mes = mes_de(turno.em);
        self.por_chat.entry(turno.chat.clone()).or_default().push(self.turnos.len());
        self.turnos.push(turno);
        let mut bytes = Vec::new();
        for linha in self.turnos.iter().filter(|t| mes_de(t.em) == mes) {
            if serde_json::to_writer(&mut bytes, linha).is_ok() {
                bytes.push(b'\n');
            }
        }
        Some((mes, bytes))
    }

    fn do_chat<'a>(&'a self, chat: &str) -> impl DoubleEndedIterator<Item = &'a Turno> + 'a {
        self.por_chat.get(chat).into_iter().flatten().map(|&indice| &self.turnos[indice])
    }

    /// O gasto do turno que a mensagem `item` (com a hora `item_em`) abriu, se foi anotado.
    pub fn do_turno(&self, chat: &str, item: usize, item_em: u64) -> Option<Gasto> {
        let mut gasto = Gasto::default();
        for turno in self.do_chat(chat).filter(|t| t.item == item && t.item_em == item_em) {
            gasto.somar(turno);
        }
        (gasto.turnos > 0).then_some(gasto)
    }

    /// O último turno anotado do chat.
    pub fn ultimo(&self, chat: &str) -> Option<&Turno> {
        self.do_chat(chat).next_back()
    }
}

impl Motor {
    /// O livro dos turnos terminados (o diálogo das contas e a conversa leem daqui).
    pub fn gastos(&self) -> &Livro {
        &self.gastos
    }

    /// Mais um `result` do turno em andamento: os tokens ficam à espera do fim de verdade (com
    /// agentes em segundo plano, o turno tem vários `result`).
    pub(super) fn somar_tokens(&mut self, chat_id: &str, tokens: Tokens) {
        self.gastos.somar_pendente(chat_id, tokens);
    }

    /// O turno terminou: anota no livro o que ele custou (`custo_antes` é o custo do chat antes
    /// de somar o `result`) e manda o arquivo do mês para o gravador.
    pub(super) fn registrar_gasto(&mut self, chat_id: &str, custo_antes: f64) {
        let Some(c) = self.chat(chat_id) else {
            return;
        };
        let marco = c.itens.iter().rposition(|i| matches!(i, Item::Usuario { .. } | Item::Entrega { .. } | Item::Vez { .. }));
        let conta = match c.agente {
            Agente::Claude => self.conta_do_turno.get(chat_id).cloned().unwrap_or_default(),
            Agente::Codex | Agente::Gemini => String::new(),
        };
        let turno = Turno {
            em: agora(),
            chat: chat_id.to_string(),
            pasta: c.cwd.to_string_lossy().into_owned(),
            modelo: c.modelo_atual.clone().or_else(|| c.modelo.clone()).unwrap_or_default(),
            conta,
            custo: (c.custo_usd - custo_antes).max(0.0),
            tokens: Tokens::default(),
            item: marco.unwrap_or(0),
            item_em: marco.map_or(0, |k| c.itens[k].em()),
        };
        let Some((mes, bytes)) = self.gastos.fechar(turno) else {
            return;
        };
        if self.somente_leitura.is_none() {
            self.dados.salvar_gastos(&mes, bytes);
        }
    }
}

#[cfg(test)]
mod testes {
    use super::{Livro, Tokens, Turno, comeco_do_dia, mes_anterior, mes_de, resumir, tokens_do_codex, tokens_do_resultado};
    use serde_json::json;

    fn turno(em: u64, chat: &str, pasta: &str, custo: f64, saida: u64) -> Turno {
        Turno {
            em,
            chat: chat.into(),
            pasta: pasta.into(),
            custo,
            tokens: Tokens { saida, ..Tokens::default() },
            ..Turno::default()
        }
    }

    #[test]
    fn tokens_do_result_com_e_sem_o_campo_por_turno() {
        // O campo por turno soma os modelos (o principal e o do subagente).
        let novo = json!({
            "type": "result",
            "usage": { "input_tokens": 1, "output_tokens": 1 },
            "model_usage_since_last_result": {
                "claude-opus": { "inputTokens": 100, "outputTokens": 40, "cacheReadInputTokens": 9000, "cacheCreationInputTokens": 500 },
                "claude-haiku": { "inputTokens": 20, "outputTokens": 5, "cacheReadInputTokens": 0, "cacheCreationInputTokens": 0 }
            }
        });
        assert_eq!(tokens_do_resultado(&novo), Some(Tokens { entrada: 120, cache_lido: 9000, cache_criado: 500, saida: 45 }));
        // Sem ele, o `usage` do turno.
        let velho = json!({ "type": "result", "usage": { "input_tokens": 7, "cache_read_input_tokens": 300, "cache_creation_input_tokens": 20, "output_tokens": 11 } });
        assert_eq!(tokens_do_resultado(&velho), Some(Tokens { entrada: 7, cache_lido: 300, cache_criado: 20, saida: 11 }));
        assert_eq!(velho_total(&velho), 338);
        // O `result` sem número (o claude falso, o processo que caiu) não vira evento.
        assert_eq!(tokens_do_resultado(&json!({ "type": "result", "total_cost_usd": 0.0 })), None);
        assert_eq!(tokens_do_resultado(&json!({ "type": "result", "usage": { "input_tokens": 0 } })), None);
        // Codex: a entrada já inclui o cache.
        let codex = json!({ "type": "turn.completed", "usage": { "input_tokens": 51894, "cached_input_tokens": 50000, "output_tokens": 92 } });
        assert_eq!(tokens_do_codex(&codex), Some(Tokens { entrada: 1894, cache_lido: 50000, cache_criado: 0, saida: 92 }));
        assert_eq!(tokens_do_codex(&json!({ "type": "turn.completed" })), None);
    }

    fn velho_total(valor: &serde_json::Value) -> u64 {
        tokens_do_resultado(valor).map_or(0, |t| t.total())
    }

    #[test]
    fn soma_por_dia_e_por_pasta() {
        let dia = 86_400;
        let hoje = 100 * dia;
        let turnos = vec![
            // Ontem e antes.
            turno(hoje - 3 * dia, "a", "/p/central", 1.0, 10),
            turno(hoje - dia / 2, "a", "/p/central", 2.0, 20),
            // Hoje: duas pastas, dois chats na Central.
            turno(hoje + 10, "a", "/p/central", 0.5, 5),
            turno(hoje + 20, "b", "/p/central", 1.5, 15),
            turno(hoje + 30, "c", "/p/layout", 0.25, 3),
            turno(hoje + 40, "b", "/p/central", 1.0, 10),
        ];
        let de_hoje = resumir(&turnos, hoje);
        assert!(de_hoje.por_custo);
        assert_eq!(de_hoje.total.turnos, 4);
        assert!((de_hoje.total.custo - 3.25).abs() < 1e-9);
        let pastas: Vec<(&str, f64, usize)> = de_hoje.pastas.iter().map(|p| (p.pasta.as_str(), p.gasto.custo, p.gasto.turnos)).collect();
        assert_eq!(pastas, [("/p/central", 3.0, 3), ("/p/layout", 0.25, 1)]);
        // Dentro da pasta, o chat que mais gastou primeiro.
        let chats: Vec<(&str, f64, u64)> =
            de_hoje.pastas[0].chats.iter().map(|c| (c.chat.as_str(), c.gasto.custo, c.gasto.tokens.saida)).collect();
        assert_eq!(chats, [("b", 2.5, 25), ("a", 0.5, 5)]);
        // 7 dias: entra o que veio antes de hoje.
        let da_semana = resumir(&turnos, hoje - 7 * dia);
        assert_eq!(da_semana.total.turnos, 6);
        assert!((da_semana.pastas[0].gasto.custo - 6.0).abs() < 1e-9);
        assert_eq!(da_semana.pastas[0].chats[0].chat, "a");
        // Período vazio.
        assert_eq!(resumir(&turnos, hoje + dia).pastas.len(), 0);
        // Só Codex (sem custo): a ordem e as barras seguem os tokens.
        let codex = vec![turno(hoje, "x", "/p/um", 0.0, 10), turno(hoje, "y", "/p/dois", 0.0, 90)];
        let resumo = resumir(&codex, hoje);
        assert!(!resumo.por_custo);
        assert_eq!(resumo.pastas[0].pasta, "/p/dois");
        assert_eq!(resumo.pastas[0].gasto.peso(false), 90.0);
    }

    #[test]
    fn o_livro_fecha_o_turno_com_os_tokens_dos_results_e_grava_o_mes() {
        let pasta = std::env::temp_dir().join(format!("vitral-gastos-{}", uuid::Uuid::new_v4()));
        let agora = 1_800_000_000;
        let mut livro = Livro::ler(&pasta, agora);
        assert!(livro.turnos().is_empty());
        // Dois `result` no mesmo turno (agentes em segundo plano): os tokens somam.
        livro.somar_pendente("a", Tokens { entrada: 10, saida: 1, ..Tokens::default() });
        livro.somar_pendente("a", Tokens { cache_lido: 100, saida: 2, ..Tokens::default() });
        let (mes, bytes) = livro.fechar(Turno { item: 4, item_em: 77, ..turno(agora, "a", "/p/central", 0.42, 0) }).unwrap();
        assert_eq!(mes, mes_de(agora));
        assert_eq!(livro.ultimo("a").unwrap().tokens, Tokens { entrada: 10, cache_lido: 100, cache_criado: 0, saida: 3 });
        // O turno é achado pela mensagem que o abriu (índice e hora); outro índice ou outra hora, não.
        assert_eq!(livro.do_turno("a", 4, 77).map(|g| g.tokens.total()), Some(113));
        assert_eq!(livro.do_turno("a", 4, 78), None);
        assert_eq!(livro.do_turno("b", 4, 77), None);
        assert!(livro.ultimo("b").is_none());
        // Os pendentes foram consumidos: o turno seguinte, sem custo e sem token, não entra.
        assert_eq!(livro.fechar(turno(agora + 5, "a", "/p/central", 0.0, 0)), None);
        // O que foi para o disco volta igual, e a linha estragada é pulada.
        std::fs::create_dir_all(&pasta).unwrap();
        let mut conteudo = bytes.clone();
        conteudo.extend_from_slice(b"isto nao e json\n");
        std::fs::write(pasta.join(format!("{mes}.jsonl")), conteudo).unwrap();
        let relido = Livro::ler(&pasta, agora);
        assert_eq!(relido.turnos(), livro.turnos());
        assert_eq!(relido.do_turno("a", 4, 77), livro.do_turno("a", 4, 77));
        assert_eq!(String::from_utf8(bytes).unwrap().lines().count(), 1);
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn mes_do_arquivo_e_comeco_do_dia() {
        assert_eq!(mes_anterior("2026-11").as_deref(), Some("2026-10"));
        assert_eq!(mes_anterior("2026-01").as_deref(), Some("2025-12"));
        assert_eq!(mes_anterior("torto"), None);
        let agora = 1_800_000_000;
        let inicio = comeco_do_dia(agora);
        assert!(inicio <= agora && agora - inicio < 86_400 + 3600);
        assert_eq!(mes_de(inicio), mes_de(agora));
        assert_eq!(comeco_do_dia(inicio), inicio);
    }
}
