//! Os agentes que um chat roda em segundo plano (a chamada Agent com `run_in_background`): quem
//! são, o que cada um faz agora e o aviso de quando terminam. O Claude Code diz quem são no
//! `background_tasks_changed` (o id e a descrição de cada um; a lista vem sempre inteira) e qual
//! chamada abriu cada um no `task_started`. O que cada um faz mora no cartão da chamada dele
//! (`Subagente::atividade`), que é de onde a linha da conversa e a lista da pílula leem: chega
//! pelo stream (cada chamada do agente) e pelo arquivo que o Claude Code grava para ele,
//! `<sessão>/subagents/agent-<id>.jsonl`, do qual só o fim é lido, e só o pedaço novo.
//!
//! Medido com a CLI de verdade (2.1.328, Haiku, 23/11/2026, um agente e depois dois): o arquivo
//! nasce com uns 230 KB (os anexos e o pedido à API) e cresce aos poucos; a última linha é a
//! chamada do agente enquanto ela roda, mas entre uma chamada e outra o fim são o resultado, os
//! anexos e as linhas do pedido à API (`api-request-blob`, `api-request`). O `.meta.json` ao lado
//! traz `description`, `toolUseId` e `model`, e ganha `finished` no fim.

use std::{
    collections::HashMap,
    io::{Read as _, Seek as _, SeekFrom},
    path::{Path, PathBuf},
};

use gpui_kit::{AppContext as _, Context};

use super::{Motor, TextoChegando};
use crate::{
    claude::{self, TarefaDeFundo},
    importar::subagente::RELATORIO,
    modelo::Item,
};

/// Quanto do fim do arquivo é lido de uma vez. O arquivo começa com os anexos do pedido, de
/// dezenas de KB cada; o que interessa é o fim.
pub const JANELA_DO_FIM: u64 = 256 * 1024;

/// Quantas vezes o arquivo de um agente é procurado antes de desistir (uma por segundo).
const TENTATIVAS: u32 = 30;

/// Um agente (ou fluxo de agentes) em segundo plano de um chat.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgenteNoFundo {
    /// O id do agente (o `task_id`), o mesmo do arquivo dele.
    pub id: String,
    pub descricao: String,
    /// Um fluxo de agentes, e não um subagente só: não tem cartão de subagente nem arquivo.
    pub fluxo: bool,
    /// A chamada (Agent) que o abriu, quando já se sabe: o `task_started` diz, e o `.meta.json`.
    pub chamada: Option<String>,
    /// Quando ele entrou na lista (Unix, segundos).
    pub desde: u64,
    /// O modelo em que ele roda, lido do `.meta.json`.
    pub modelo: Option<String>,
    /// De quando é a última ação anotada, em milissegundos: a que chega pelo stream leva a hora
    /// da chegada, e a lida do arquivo só entra se for mais nova.
    acao_em: u64,
    /// O arquivo dele, depois de achado, até onde foi lido e o tamanho que tinha.
    arquivo: Option<PathBuf>,
    lido_ate: u64,
    tamanho: u64,
    lendo: bool,
    tentativas: u32,
}

/// O segundo plano de um chat: os agentes que rodam agora e as chamadas que foram para lá.
#[derive(Clone, Debug, Default)]
pub struct NoFundo {
    agentes: Vec<AgenteNoFundo>,
    /// A chamada que abriu cada agente, pelo id dele (o `task_started` pode chegar antes da lista).
    chamadas: HashMap<String, String>,
    /// As chamadas que voltaram com "Async agent launched": o fim delas vira uma linha na conversa.
    lancadas: Vec<String>,
}

impl NoFundo {
    /// A lista nova do `background_tasks_changed`: quem continua nela fica como estava, quem
    /// chegou entra com a hora de agora, e quem saiu terminou.
    pub fn listar(&mut self, tarefas: Vec<TarefaDeFundo>, agora: u64) {
        let mut antes = std::mem::take(&mut self.agentes);
        self.agentes = tarefas
            .into_iter()
            .map(|tarefa| match antes.iter().position(|a| a.id == tarefa.id) {
                Some(posicao) => antes.swap_remove(posicao),
                None => AgenteNoFundo {
                    chamada: self.chamadas.get(&tarefa.id).cloned(),
                    id: tarefa.id,
                    descricao: tarefa.descricao,
                    fluxo: tarefa.fluxo,
                    desde: agora,
                    ..Default::default()
                },
            })
            .collect();
    }

    /// O Claude Code abriu o agente `id` pela chamada `chamada`.
    pub fn abriu(&mut self, id: &str, chamada: &str) {
        self.chamadas.insert(id.to_string(), chamada.to_string());
        if let Some(agente) = self.agentes.iter_mut().find(|a| a.id == id) {
            agente.chamada = Some(chamada.to_string());
        }
    }

    /// A chamada voltou na hora, com o agente indo para o segundo plano.
    pub fn lancada(&mut self, chamada: &str) {
        if !self.foi_lancada(chamada) {
            self.lancadas.push(chamada.to_string());
        }
    }

    /// A chamada foi para o segundo plano (e não é um subagente que a conversa espera).
    pub fn foi_lancada(&self, chamada: &str) -> bool {
        self.lancadas.iter().any(|c| c == chamada)
    }

    /// O agente da chamada `chamada` fez algo que o stream entregou agora (`agora_ms`).
    pub fn andou(&mut self, chamada: &str, agora_ms: u64) {
        if let Some(agente) = self.agentes.iter_mut().find(|a| a.chamada.as_deref() == Some(chamada)) {
            agente.acao_em = agente.acao_em.max(agora_ms);
        }
    }

    pub fn agentes(&self) -> &[AgenteNoFundo] {
        &self.agentes
    }
}

/// A linha de um agente em segundo plano, com o que a tela precisa.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Linha {
    pub id: String,
    /// A chamada dele, quando o cartão está na conversa: é o que o clique abre.
    pub chamada: Option<String>,
    pub descricao: String,
    /// Há quantos segundos ele foi lançado.
    pub segundos: u64,
    /// A última ação, como o cartão a guarda ("Bash · cargo test", ou o que ele disse).
    pub atividade: String,
    pub modelo: Option<String>,
    pub fluxo: bool,
}

/// Uma linha por agente que ainda roda em segundo plano, na ordem em que foram lançados. O tempo
/// conta do cartão da chamada (quando ela saiu); sem cartão, de quando ele entrou na lista. Quem
/// já tem o resultado no cartão terminou, e não aparece.
pub fn linhas(agentes: &[AgenteNoFundo], itens: &[Item], agora: u64) -> Vec<Linha> {
    agentes
        .iter()
        .filter_map(|agente| {
            let cartao = agente.chamada.as_deref().and_then(|chamada| cartao_do_agente(itens, chamada));
            let (desde, atividade) = match cartao {
                Some(Item::Ferramenta { resultado: Some(_), .. }) => return None,
                Some(Item::Ferramenta { em, sub, .. }) => {
                    (if *em > 0 { *em } else { agente.desde }, sub.as_ref().map(|s| s.atividade.clone()).unwrap_or_default())
                }
                _ => (agente.desde, String::new()),
            };
            Some(Linha {
                id: agente.id.clone(),
                chamada: cartao.and(agente.chamada.clone()),
                descricao: agente.descricao.clone(),
                segundos: agora.saturating_sub(desde),
                atividade,
                modelo: agente.modelo.clone(),
                fluxo: agente.fluxo,
            })
        })
        .collect()
}

/// O cartão (a chamada Task ou Agent) com este id, o mais novo primeiro.
fn cartao_do_agente<'i>(itens: &'i [Item], chamada: &str) -> Option<&'i Item> {
    itens.iter().rev().find(|item| matches!(item, Item::Ferramenta { id, nome, .. } if id == chamada && matches!(nome.as_str(), "Task" | "Agent")))
}

/// O que um agente fez por último, lido do arquivo dele.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Acao {
    /// Como o cartão guarda: o resumo da chamada ("Bash · cargo test") ou a primeira linha do
    /// que ele disse.
    pub texto: String,
    /// A hora da linha, em milissegundos (0: a linha não diz).
    pub quando: u64,
}

/// O que uma leitura do fim do arquivo devolve.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fim {
    /// Até onde as linhas inteiras foram lidas: a próxima leitura começa aqui.
    pub ate: u64,
    /// O tamanho do arquivo nesta leitura (enquanto for o mesmo, não há o que ler).
    pub tamanho: u64,
    /// Quantos bytes esta leitura leu do disco.
    pub lidos: u64,
    /// A última ação entre as linhas novas, se houve alguma.
    pub acao: Option<Acao>,
}

/// Lê o fim do arquivo de um subagente a partir de `desde` (onde a leitura anterior parou; 0 na
/// primeira) e diz a última ação dele entre as linhas novas. Nunca lê mais que `JANELA_DO_FIM`:
/// na primeira leitura, e quando o arquivo cresceu mais que isso de uma vez, o começo fica para
/// trás. A linha ainda sendo gravada (sem a quebra no fim) fica para a próxima leitura.
pub fn ler_o_fim(arquivo: &Path, desde: u64) -> std::io::Result<Fim> {
    let mut aberto = std::fs::File::open(arquivo)?;
    let tamanho = aberto.metadata()?.len();
    // O arquivo encolheu (não é mais o mesmo): recomeça.
    let desde = if desde > tamanho { 0 } else { desde };
    let inicio = desde.max(tamanho.saturating_sub(JANELA_DO_FIM));
    let mut bytes = Vec::with_capacity((tamanho - inicio) as usize);
    if tamanho > inicio {
        aberto.seek(SeekFrom::Start(inicio))?;
        aberto.take(tamanho - inicio).read_to_end(&mut bytes)?;
    }
    // Só linhas inteiras: a que ainda está sendo gravada fica para a próxima leitura.
    let fim = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |p| p + 1);
    // O começo ficou para trás: a primeira linha deste pedaço veio cortada.
    let comeco = if inicio > desde { bytes.iter().position(|b| *b == b'\n').map_or(fim, |p| p + 1) } else { 0 };
    let acao = bytes[comeco.min(fim)..fim].split(|b| *b == b'\n').rev().find_map(acao_da_linha);
    Ok(Fim { ate: inicio + fim as u64, tamanho, lidos: bytes.len() as u64, acao })
}

/// A ação que uma linha do arquivo do subagente diz: a chamada que ele fez ou o que ele disse.
/// O resto (o pedido, os resultados, os anexos, as linhas do pedido à API, o raciocínio) não é
/// ação.
fn acao_da_linha(linha: &[u8]) -> Option<Acao> {
    // Barato primeiro: os anexos e os pedidos à API passam de dezenas de KB, e não são do agente.
    const MARCA: &[u8] = b"\"assistant\"";
    if !linha.windows(MARCA.len()).any(|janela| janela == MARCA) {
        return None;
    }
    let valor: serde_json::Value = serde_json::from_slice(linha).ok()?;
    if valor["type"] != "assistant" {
        return None;
    }
    let texto = valor["message"]["content"].as_array()?.iter().rev().find_map(|bloco| match bloco["type"].as_str()? {
        "tool_use" => {
            let nome = bloco["name"].as_str()?;
            // A entrega do relatório e os bastidores não são o que ele está fazendo.
            (nome != RELATORIO && nome != "ToolSearch" && nome != "mcp__vitral__permissao").then(|| claude::resumo_ferramenta(nome, &bloco["input"]))
        }
        "text" => bloco["text"].as_str().and_then(|t| t.trim().lines().next()).filter(|l| !l.is_empty()).map(|l| l.chars().take(160).collect()),
        _ => None,
    })?;
    let quando = valor["timestamp"].as_str().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map_or(0, |d| d.timestamp_millis().max(0) as u64);
    Some(Acao { texto, quando })
}

/// O que o `agent-<id>.meta.json` diz do agente.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    pub descricao: String,
    pub chamada: Option<String>,
    pub modelo: Option<String>,
}

/// Lê o `.meta.json` que mora ao lado do arquivo do agente.
pub fn ler_meta(arquivo: &Path) -> Option<Meta> {
    let dados: serde_json::Value = serde_json::from_slice(&std::fs::read(arquivo.with_extension("meta.json")).ok()?).ok()?;
    let campo = |chave: &str| dados[chave].as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
    Some(Meta { descricao: campo("description").unwrap_or_default(), chamada: campo("toolUseId"), modelo: campo("model") })
}

/// O arquivo do agente `id` na sessão `sessao`, na primeira das pastas de projeto que o tem.
pub fn arquivo_do_agente(projetos: &[PathBuf], sessao: &str, id: &str) -> Option<PathBuf> {
    // O id vem do stream: só o que cabe num nome de arquivo.
    if sessao.is_empty() || id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    projetos.iter().map(|projeto| projeto.join(sessao).join("subagents").join(format!("agent-{id}.jsonl"))).find(|arquivo| arquivo.is_file())
}

/// A ação lida do arquivo entra no cartão do agente, se for mais nova que a última que o stream
/// entregou (a mesma chamada chega pelos dois, e o stream chega antes). Devolve se o cartão mudou.
pub fn anotar_acao(agente: &mut AgenteNoFundo, itens: &mut [Item], acao: Acao) -> bool {
    if agente.acao_em > 0 && acao.quando <= agente.acao_em {
        return false;
    }
    agente.acao_em = acao.quando;
    let Some(chamada) = agente.chamada.as_deref() else {
        return false;
    };
    let sub = itens.iter_mut().rev().find_map(|item| match item {
        Item::Ferramenta { id, nome, sub, .. } if id == chamada && matches!(nome.as_str(), "Task" | "Agent") => Some(sub),
        _ => None,
    });
    match sub.map(|sub| sub.get_or_insert_with(Default::default)) {
        Some(sub) if sub.atividade != acao.texto => {
            sub.atividade = acao.texto;
            true
        }
        _ => false,
    }
}

/// O agente em segundo plano da chamada `chamada` terminou (ou falhou): entra no fim da conversa
/// o item que marca isso, com a descrição dele e quanto levou. Devolve se entrou (a chamada que
/// não é um subagente, ou que não está na conversa, não ganha a linha).
pub fn anotar_fim(itens: &mut Vec<Item>, chamada: &str, erro: bool, agora: u64) -> bool {
    let Some(Item::Ferramenta { resumo, em, .. }) = cartao_do_agente(itens, chamada) else {
        return false;
    };
    // O resumo do cartão é "Agent · <descrição>".
    let descricao = resumo.split_once(" · ").map_or("", |(_, descricao)| descricao).trim().to_string();
    let durou = if *em > 0 { agora.saturating_sub(*em) } else { 0 };
    itens.push(Item::FimDoAgente { chamada: chamada.to_string(), descricao, erro, durou, em: agora });
    true
}

/// O que uma leitura em segundo plano devolve: o arquivo (achado agora ou já sabido), o meta
/// (só quando o arquivo acabou de ser achado) e o fim dele.
struct Lido {
    arquivo: Option<PathBuf>,
    meta: Option<Meta>,
    fim: Option<Fim>,
}

/// Acha o arquivo do agente (ou usa o já achado) e lê o fim dele. Bloqueia: roda em segundo plano.
fn ler(projetos: Vec<PathBuf>, sessao: String, id: String, conhecido: Option<PathBuf>, desde: u64) -> Lido {
    let (arquivo, meta) = match conhecido {
        Some(arquivo) => (Some(arquivo), None),
        None => {
            let arquivo = arquivo_do_agente(&projetos, &sessao, &id);
            let meta = arquivo.as_deref().and_then(ler_meta);
            (arquivo, meta)
        }
    };
    let fim = arquivo.as_deref().and_then(|a| ler_o_fim(a, desde).ok());
    Lido { arquivo, meta, fim }
}

fn agora_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

impl Motor {
    /// Uma linha por agente que este chat roda em segundo plano agora.
    pub fn linhas_do_segundo_plano(&self, chat: &str) -> Vec<Linha> {
        match (self.segundo_plano.get(chat), self.chat_gravado(chat)) {
            (Some(fundo), Some(c)) => linhas(fundo.agentes(), &c.itens, crate::modelo::agora()),
            _ => Vec::new(),
        }
    }

    /// O subagente da chamada `chamada` fez algo que o stream entregou: a leitura do arquivo dele
    /// não desfaz isso.
    pub(super) fn segundo_plano_andou(&mut self, chat: &str, chamada: &str) {
        if let Some(fundo) = self.segundo_plano.get_mut(chat) {
            fundo.andou(chamada, agora_ms());
        }
    }

    /// Uma vez por segundo, enquanto algum chat trabalha: o fim do arquivo de cada agente em
    /// segundo plano é lido, em segundo plano, só se o arquivo cresceu e só o pedaço novo. É o
    /// que traz o que o agente diz entre uma chamada e outra, que o stream não entrega, e o
    /// modelo em que ele roda.
    pub fn conferir_segundo_plano(&mut self, cx: &mut Context<Self>) {
        if self.segundo_plano.is_empty() {
            return;
        }
        let chats: Vec<String> = self.segundo_plano.keys().filter(|chat| self.execucao_real(chat).rodando()).cloned().collect();
        for chat in chats {
            // Onde o Claude Code grava: a pasta do projeto na conta do chat e na normal. Só é
            // conferido enquanto algum agente ainda não tem o arquivo achado.
            let falta_achar = self.segundo_plano.get(&chat).is_some_and(|f| f.agentes.iter().any(|a| !a.fluxo && a.arquivo.is_none() && a.tentativas < TENTATIVAS));
            let Some(c) = self.chat_gravado(&chat) else {
                continue;
            };
            let sessao = c.sessao.clone();
            let projetos = if falta_achar { claude::pastas_do_projeto(self.conta_para(c).pasta().as_deref(), &c.cwd) } else { Vec::new() };
            let Some(fundo) = self.segundo_plano.get_mut(&chat) else {
                continue;
            };
            for agente in fundo.agentes.iter_mut().filter(|a| !a.fluxo && !a.lendo) {
                match &agente.arquivo {
                    // O arquivo não cresceu: nada a ler.
                    Some(arquivo) if std::fs::metadata(arquivo).map_or(0, |m| m.len()) == agente.tamanho => continue,
                    Some(_) => {}
                    None if agente.tentativas >= TENTATIVAS => continue,
                    None => agente.tentativas += 1,
                }
                agente.lendo = true;
                let (chat, id, conhecido, desde) = (chat.clone(), agente.id.clone(), agente.arquivo.clone(), agente.lido_ate);
                let (projetos, sessao) = (projetos.clone(), sessao.clone());
                cx.spawn(async move |motor, cx| {
                    let lido = cx.background_spawn({
                        let id = id.clone();
                        async move { ler(projetos, sessao, id, conhecido, desde) }
                    })
                    .await;
                    let _ = motor.update(cx, |motor, cx| motor.segundo_plano_lido(&chat, &id, lido, cx));
                })
                .detach();
            }
        }
    }

    /// A leitura voltou: a última ação entra no cartão do agente, e só os painéis que mostram
    /// este chat redesenham (como o texto chegando), não a janela.
    fn segundo_plano_lido(&mut self, chat: &str, id: &str, lido: Lido, cx: &mut Context<Self>) {
        let Some(agente) = self.segundo_plano.get_mut(chat).and_then(|f| f.agentes.iter_mut().find(|a| a.id == id)) else {
            return;
        };
        agente.lendo = false;
        if lido.arquivo.is_some() {
            agente.arquivo = lido.arquivo;
        }
        if let Some(meta) = lido.meta {
            agente.modelo = meta.modelo;
            if agente.chamada.is_none() {
                agente.chamada = meta.chamada;
            }
        }
        let Some(fim) = lido.fim else {
            return;
        };
        (agente.lido_ate, agente.tamanho) = (fim.ate, fim.tamanho);
        let (Some(acao), Some(c)) = (fim.acao, self.chats.iter_mut().find(|c| c.id == chat)) else {
            return;
        };
        if anotar_acao(agente, &mut c.itens, acao) {
            self.chats_sujos.insert(chat.to_string());
            cx.emit(TextoChegando { chats: vec![chat.to_string()] });
        }
    }
}

#[cfg(test)]
mod testes {
    use std::fs;

    use serde_json::json;

    use super::*;
    use crate::modelo::Subagente;

    fn pasta_de_teste(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-fundo-{nome}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&pasta);
        fs::create_dir_all(&pasta).unwrap();
        pasta
    }

    fn tarefa(id: &str, descricao: &str) -> TarefaDeFundo {
        TarefaDeFundo { id: id.into(), descricao: descricao.into(), fluxo: false }
    }

    fn cartao(id: &str, nome: &str, resumo: &str, resultado: Option<&str>, em: u64, atividade: &str) -> Item {
        Item::Ferramenta {
            id: id.into(),
            nome: nome.into(),
            resumo: resumo.into(),
            resultado: resultado.map(str::to_string),
            erro: false,
            em,
            detalhe: None,
            arquivo: None,
            sub: (!atividade.is_empty()).then(|| Subagente { atividade: atividade.into(), ..Default::default() }),
            mudanca: None,
            durou: 0,
            imagens: Vec::new(),
        }
    }

    /// Uma linha do arquivo do subagente, com os campos que o Claude Code 2.1.328 grava.
    fn linha(tipo: &str, quando: &str, mensagem: serde_json::Value) -> String {
        json!({
            "parentUuid": null, "isSidechain": true, "agentId": "a90a7d757b83e6f0e", "type": tipo,
            "timestamp": quando, "sessionId": "s", "version": "2.1.328", "cwd": "/tmp/p", "message": mensagem,
        })
        .to_string()
    }

    fn chamada_do_agente(quando: &str, nome: &str, entrada: serde_json::Value) -> String {
        linha("assistant", quando, json!({ "id": "m", "role": "assistant", "content": [{ "type": "tool_use", "id": "toolu_x", "name": nome, "input": entrada }] }))
    }

    /// O que o Claude Code grava entre o resultado de uma chamada e a chamada seguinte (medido):
    /// o resultado, um anexo e as duas linhas do pedido à API.
    fn entre_as_chamadas(quando: &str) -> Vec<String> {
        vec![
            linha("user", quando, json!({ "role": "user", "content": [{ "tool_use_id": "toolu_x", "type": "tool_result", "content": "um", "is_error": false }] })),
            json!({ "isSidechain": true, "type": "attachment", "attachment": { "type": "x", "texto": "y".repeat(600) }, "timestamp": quando }).to_string(),
            r#"{"type":"api-request-blob","sessionId":"s","agentId":"a90a7d757b83e6f0e","hash":"fa0e","content":[{"type":"text","text":"um"}]}"#.to_string(),
            format!(r#"{{"type":"api-request","sessionId":"s","agentId":"a90a7d757b83e6f0e","id":"fc9f","timestamp":"{quando}"}}"#),
        ]
    }

    fn acrescentar(arquivo: &Path, linhas: &[String]) {
        for linha in linhas {
            acrescentar_cru(arquivo, &format!("{linha}\n"));
        }
    }

    /// Grava no fim do arquivo exatamente este texto (sem a quebra de linha).
    fn acrescentar_cru(arquivo: &Path, texto: &str) {
        use std::io::Write as _;
        fs::OpenOptions::new().create(true).append(true).open(arquivo).unwrap().write_all(texto.as_bytes()).unwrap();
    }

    /// Item 1, de onde vêm os dados: a lista do `background_tasks_changed` diz quem roda, e o
    /// `task_started` liga cada um à chamada que o abriu, na ordem em que chegarem.
    #[test]
    fn a_lista_do_segundo_plano_guarda_quem_roda_e_a_chamada_de_cada_um() {
        let mut fundo = NoFundo::default();
        // Como no de verdade: a lista, e logo depois o começo do agente.
        fundo.listar(vec![tarefa("a1", "Contar rápido")], 100);
        fundo.abriu("a1", "toolu_1");
        // O começo pode chegar antes da lista: a chamada fica guardada.
        fundo.abriu("a2", "toolu_2");
        fundo.listar(vec![tarefa("a1", "Contar rápido"), tarefa("a2", "Contar devagar")], 130);
        let vistos: Vec<(&str, &str, Option<&str>, u64)> =
            fundo.agentes().iter().map(|a| (a.id.as_str(), a.descricao.as_str(), a.chamada.as_deref(), a.desde)).collect();
        // Quem já estava na lista continua com a hora em que entrou.
        assert_eq!(vistos, [("a1", "Contar rápido", Some("toolu_1"), 100), ("a2", "Contar devagar", Some("toolu_2"), 130)]);
        // Quem saiu da lista terminou; o outro segue.
        fundo.listar(vec![tarefa("a2", "Contar devagar")], 160);
        assert_eq!(fundo.agentes().iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["a2"]);
        assert_eq!(fundo.agentes()[0].desde, 130);
        fundo.listar(Vec::new(), 190);
        assert!(fundo.agentes().is_empty());
        // Só a chamada que voltou com "Async agent launched" foi para o segundo plano.
        fundo.lancada("toolu_1");
        assert!(fundo.foi_lancada("toolu_1"));
        assert!(!fundo.foi_lancada("toolu_3"));
    }

    /// Item 1: "uma linha por agente", com a descrição, o tempo desde o lançamento e a última
    /// ação, e a chamada que o clique abre.
    #[test]
    fn uma_linha_por_agente_com_a_descricao_o_tempo_e_a_ultima_acao() {
        let mut fundo = NoFundo::default();
        fundo.listar(vec![tarefa("a1", "Mapear o carrinho"), tarefa("a2", "Conferir os cupons"), tarefa("a3", "Sem cartão")], 1_000);
        fundo.abriu("a1", "toolu_1");
        fundo.abriu("a2", "toolu_2");
        fundo.listar(vec![tarefa("a1", "Mapear o carrinho"), tarefa("a2", "Conferir os cupons"), tarefa("a3", "Sem cartão")], 1_000);
        let itens = vec![
            cartao("toolu_1", "Agent", "Agent · Mapear o carrinho", None, 990, "Bash · cargo test"),
            cartao("toolu_2", "Agent", "Agent · Conferir os cupons", None, 995, ""),
            Item::Assistente { texto: "Lancei os dois.".into(), em: 996 },
        ];
        let vistas = linhas(fundo.agentes(), &itens, 1_500);
        let resumo: Vec<(&str, Option<&str>, &str, u64, &str)> =
            vistas.iter().map(|l| (l.id.as_str(), l.chamada.as_deref(), l.descricao.as_str(), l.segundos, l.atividade.as_str())).collect();
        assert_eq!(
            resumo,
            [
                // O tempo conta de quando a chamada saiu (o cartão), e a ação é a do cartão.
                ("a1", Some("toolu_1"), "Mapear o carrinho", 510, "Bash · cargo test"),
                // Ainda sem ação: a tela diz que ele está começando.
                ("a2", Some("toolu_2"), "Conferir os cupons", 505, ""),
                // Sem cartão na conversa não há o que abrir, e o tempo conta de quando entrou na lista.
                ("a3", None, "Sem cartão", 500, ""),
            ]
        );
        // O agente cujo cartão já tem o resultado terminou: sai antes mesmo de a lista nova chegar.
        let mut itens = itens;
        itens[0] = cartao("toolu_1", "Agent", "Agent · Mapear o carrinho", Some("pronto"), 990, "Bash · cargo test");
        assert_eq!(linhas(fundo.agentes(), &itens, 1_500).iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["a2", "a3"]);
        assert!(linhas(&[], &itens, 1_500).is_empty());
    }

    /// Item 1, o custo: só o fim do arquivo é lido, com o ponto em que a leitura anterior parou
    /// guardado; o que não cresceu não é relido. O arquivo é o que o Claude Code 2.1.328 gravou
    /// (medido com a CLI de verdade): enquanto uma chamada roda ela é a última linha; entre uma e
    /// outra, o fim são o resultado, os anexos e as linhas do pedido à API.
    #[test]
    fn le_so_o_fim_do_arquivo_e_so_o_pedaco_novo() {
        let arquivo = pasta_de_teste("fim").join("agent-a90a7d757b83e6f0e.jsonl");
        // O começo de verdade: o pedido e os anexos, bem maiores que a janela.
        let anexo = json!({ "isSidechain": true, "type": "attachment", "attachment": { "type": "x", "texto": "y".repeat(70_000) } }).to_string();
        let mut comeco = vec![linha("user", "2026-11-23T19:02:01.075Z", json!({ "role": "user", "content": "Rode sleep 8 três vezes." }))];
        comeco.extend(std::iter::repeat_n(anexo, 6));
        comeco.push(chamada_do_agente("2026-11-23T19:02:02.556Z", "Bash", json!({ "command": "sleep 8 && echo um", "description": "Sleep 8 seconds then echo um" })));
        acrescentar(&arquivo, &comeco);
        let tamanho = fs::metadata(&arquivo).unwrap().len();
        assert!(tamanho > JANELA_DO_FIM + 100_000, "o arquivo de teste precisa passar da janela: {tamanho}");

        // A primeira leitura só lê a janela do fim, e acha a chamada que roda.
        let primeira = ler_o_fim(&arquivo, 0).unwrap();
        assert_eq!(primeira.acao, Some(Acao { texto: "Bash · Sleep 8 seconds then echo um".into(), quando: 1_795_460_522_556 }));
        assert_eq!((primeira.ate, primeira.tamanho), (tamanho, tamanho));
        assert!(primeira.lidos <= JANELA_DO_FIM, "leu {} de {tamanho}", primeira.lidos);

        // Nada mudou: nada é lido, e não há ação nova.
        let parada = ler_o_fim(&arquivo, primeira.ate).unwrap();
        assert_eq!((parada.ate, parada.lidos, parada.acao), (tamanho, 0, None));

        // O resultado chegou e o pedido seguinte foi à API: o fim do arquivo não é uma chamada, e a
        // leitura só lê o pedaço novo.
        let depois = entre_as_chamadas("2026-11-23T19:02:10.974Z");
        acrescentar(&arquivo, &depois);
        let novo: u64 = depois.iter().map(|l| l.len() as u64 + 1).sum();
        let segunda = ler_o_fim(&arquivo, primeira.ate).unwrap();
        assert_eq!((segunda.ate, segunda.lidos, segunda.acao), (tamanho + novo, novo, None));

        // A chamada seguinte, e depois o que ele diz (que o stream não entrega): a última vale.
        acrescentar(&arquivo, &[chamada_do_agente("2026-11-23T19:02:11.973Z", "Read", json!({ "file_path": "/tmp/p/src/ui/painel_arquivo.rs" }))]);
        let terceira = ler_o_fim(&arquivo, segunda.ate).unwrap();
        assert_eq!(terceira.acao.map(|a| a.texto), Some("Read · painel_arquivo.rs".to_string()));
        acrescentar(
            &arquivo,
            &[
                linha("assistant", "2026-11-23T19:02:20.000Z", json!({ "id": "m", "role": "assistant", "content": [{ "type": "thinking", "thinking": "" }] })),
                linha("assistant", "2026-11-23T19:02:20.986Z", json!({ "id": "m", "role": "assistant", "content": [{ "type": "text", "text": "Conferindo os testes do carrinho.\n\nSão doze." }] })),
            ],
        );
        let quarta = ler_o_fim(&arquivo, terceira.ate).unwrap();
        assert_eq!(quarta.acao, Some(Acao { texto: "Conferindo os testes do carrinho.".into(), quando: 1_795_460_540_986 }));

        // A linha ainda sendo gravada (sem a quebra no fim) fica para a próxima leitura.
        let inteira = chamada_do_agente("2026-11-23T19:02:30.000Z", "Grep", json!({ "pattern": "cupom" }));
        let (metade, resto) = inteira.split_at(inteira.len() / 2);
        acrescentar_cru(&arquivo, metade);
        let cortada = ler_o_fim(&arquivo, quarta.ate).unwrap();
        assert_eq!((cortada.ate, cortada.acao), (quarta.ate, None));
        acrescentar_cru(&arquivo, &format!("{resto}\n"));
        let completa = ler_o_fim(&arquivo, cortada.ate).unwrap();
        assert_eq!(completa.acao.map(|a| a.texto), Some("Grep · cupom".to_string()));
        assert_eq!(completa.ate, fs::metadata(&arquivo).unwrap().len());

        // A entrega do relatório não é uma ação dele, e o arquivo que não existe é um erro.
        acrescentar(&arquivo, &[chamada_do_agente("2026-11-23T19:02:31.000Z", RELATORIO, json!({ "message": "Pronto." }))]);
        assert_eq!(ler_o_fim(&arquivo, completa.ate).unwrap().acao, None);
        assert!(ler_o_fim(&arquivo.with_file_name("agent-nao-existe.jsonl"), 0).is_err());
    }

    /// Item 1: o arquivo do agente mora em `<projeto>/<sessão>/subagents/agent-<id>.jsonl`, e o
    /// `.meta.json` ao lado traz a descrição, a chamada e o modelo (o de verdade, medido).
    #[test]
    fn acha_o_arquivo_do_agente_pelo_id_e_le_o_meta() {
        let raiz = pasta_de_teste("arquivo");
        let (vazio, projeto) = (raiz.join("-outra-conta"), raiz.join("-tmp-p"));
        let subagentes = projeto.join("s1").join("subagents");
        fs::create_dir_all(&subagentes).unwrap();
        fs::create_dir_all(&vazio).unwrap();
        let arquivo = subagentes.join("agent-a90a7d757b83e6f0e.jsonl");
        fs::write(&arquivo, "").unwrap();
        fs::write(
            subagentes.join("agent-a90a7d757b83e6f0e.meta.json"),
            r#"{"agentType":"general-purpose","isBuiltIn":true,"launchedWithDefaults":true,"description":"Contar devagar","toolUseId":"toolu_01DvtnNfPNDvZGhdNwqg9cbv","spawnDepth":1,"requestShape":"background","requestNonInteractive":true,"model":"claude-haiku-5-5","pickedEffortModel":"claude-haiku-5-5","finished":true}"#,
        )
        .unwrap();
        let projetos = [vazio, projeto];
        assert_eq!(arquivo_do_agente(&projetos, "s1", "a90a7d757b83e6f0e"), Some(arquivo.clone()));
        // Outra sessão, outro agente: não há arquivo.
        assert_eq!(arquivo_do_agente(&projetos, "s2", "a90a7d757b83e6f0e"), None);
        assert_eq!(arquivo_do_agente(&projetos, "s1", "aoutro"), None);
        assert_eq!(
            ler_meta(&arquivo),
            Some(Meta { descricao: "Contar devagar".into(), chamada: Some("toolu_01DvtnNfPNDvZGhdNwqg9cbv".into()), modelo: Some("claude-haiku-5-5".into()) })
        );
        assert_eq!(ler_meta(&arquivo.with_file_name("agent-aoutro.jsonl")), None);
    }

    /// Item 1 e 2: a última ação mora no cartão do agente, que é de onde a linha da conversa e a
    /// lista da pílula leem. O que o arquivo diz entra nele; o que o stream já entregou não volta
    /// atrás por causa de uma leitura atrasada.
    #[test]
    fn a_acao_lida_do_arquivo_entra_no_cartao_sem_desfazer_a_do_stream() {
        let mut fundo = NoFundo::default();
        fundo.listar(vec![tarefa("a1", "Mapear o carrinho")], 1_000);
        fundo.abriu("a1", "toolu_1");
        let mut itens = vec![cartao("toolu_1", "Agent", "Agent · Mapear o carrinho", None, 990, "")];
        let atividade = |itens: &[Item]| match &itens[0] {
            Item::Ferramenta { sub, .. } => sub.as_ref().map(|s| s.atividade.clone()).unwrap_or_default(),
            _ => unreachable!(),
        };
        // Sem nada do stream ainda, o arquivo vale.
        let mut agente = fundo.agentes()[0].clone();
        assert!(anotar_acao(&mut agente, &mut itens, Acao { texto: "Grep · cupom".into(), quando: 5_000 }));
        assert_eq!(atividade(&itens), "Grep · cupom");
        // A mesma ação lida de novo não muda nada (nada a redesenhar).
        assert!(!anotar_acao(&mut agente, &mut itens, Acao { texto: "Grep · cupom".into(), quando: 5_000 }));

        // O stream entregou a chamada seguinte às 9 000: a leitura atrasada da anterior não a desfaz.
        fundo.andou("toolu_1", 9_000);
        let mut agente = fundo.agentes()[0].clone();
        if let Item::Ferramenta { sub, .. } = &mut itens[0] {
            sub.get_or_insert_with(Default::default).atividade = "Bash · cargo test".into();
        }
        assert!(!anotar_acao(&mut agente, &mut itens, Acao { texto: "Read · views.py".into(), quando: 8_000 }));
        assert_eq!(atividade(&itens), "Bash · cargo test");
        // O que ele disse depois disso só está no arquivo: entra.
        assert!(anotar_acao(&mut agente, &mut itens, Acao { texto: "Conferindo os testes do carrinho.".into(), quando: 9_500 }));
        assert_eq!(atividade(&itens), "Conferindo os testes do carrinho.");
        // Sem a chamada dele (o fluxo, ou o começo que não chegou) não há cartão a mudar.
        let mut solto = AgenteNoFundo { id: "a9".into(), ..Default::default() };
        assert!(!anotar_acao(&mut solto, &mut itens, Acao { texto: "Glob · *.rs".into(), quando: 9_900 }));
        assert_eq!(atividade(&itens), "Conferindo os testes do carrinho.");
    }

    /// Item 3: quando um agente em segundo plano termina, entra na conversa, no ponto em que ele
    /// termina, o item do fim dele, com a descrição e quanto levou; o que falhou fica marcado.
    #[test]
    fn o_fim_do_agente_entra_na_conversa_no_ponto_em_que_ele_termina() {
        let mut itens = vec![
            cartao("toolu_1", "Agent", "Agent · Mapear o carrinho", Some("Dois lugares mexem no cupom."), 1_000, "Bash · cargo test"),
            cartao("toolu_2", "Agent", "Agent · Conferir os cupons", Some("A API não respondeu."), 1_010, ""),
            cartao("toolu_b", "Bash", "Bash · npm run dev", Some("ok"), 1_020, ""),
            Item::Assistente { texto: "Lancei os dois.".into(), em: 1_030 },
        ];
        assert!(anotar_fim(&mut itens, "toolu_1", false, 2_500));
        assert!(anotar_fim(&mut itens, "toolu_2", true, 2_600));
        let fins: Vec<(&str, &str, bool, u64, u64)> = itens[4..]
            .iter()
            .map(|item| match item {
                Item::FimDoAgente { chamada, descricao, erro, durou, em } => (chamada.as_str(), descricao.as_str(), *erro, *durou, *em),
                outro => panic!("{outro:?}"),
            })
            .collect();
        // No fim da conversa (depois da resposta que já veio), na ordem em que terminaram.
        assert_eq!(fins, [("toolu_1", "Mapear o carrinho", false, 1_500, 2_500), ("toolu_2", "Conferir os cupons", true, 1_590, 2_600)]);
        // O comando de fundo também avisa o fim, mas não é um agente; e a chamada que não está na
        // conversa não tem o que dizer.
        assert!(!anotar_fim(&mut itens, "toolu_b", false, 2_700));
        assert!(!anotar_fim(&mut itens, "toolu_9", false, 2_700));
        assert_eq!(itens.len(), 6);
    }
}
