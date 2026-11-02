//! As regras do PR sem rede nem disco: a transição de uma consulta do vigia para a seguinte
//! (`transicao_pr`), quando dá para mesclar (`pronto_para_mesclar`), para onde vai um aviso do PR
//! (`destino_do_aviso`), quando a rodada publica sozinha (`pode_publicar_sozinho`), o que a aba PR
//! habilita (`acoes_possiveis`), a separação dos comentários pela confiança e o registro do PR que
//! foi mesclado, fechado ou reaberto na tarefa. Tudo aqui é testado sem o motor.

use std::time::Duration;

use super::AcaoPr;
use crate::{
    github::{Check, ComentarioPr, EstadoChecks, EstadoPr, InfoRepo, ResumoPr, TipoComentario},
    issues::Confianca,
    modelo::{EstadoTarefa, GatilhoAjuste, Pr, PrAnterior, Tarefa},
    vigia::Contexto,
};

/// Depois do envio, o Vitral espera isso antes de mesclar: os bots de revisão comentam logo depois.
pub const ESPERA_DOS_BOTS: u64 = 90;
/// Com falhas e checks ainda rodando, o aviso sai depois disso da primeira falha.
pub const FALHA_COM_PENDENTES: u64 = 10 * 60;
/// Checks sem progresso por tanto tempo viram aviso; com o dobro de um dia, o Ship desarma.
pub const PARADOS: u64 = 2 * 3600;
pub const PARADOS_DEMAIS: u64 = 24 * 3600;
/// Rodadas disparadas por aviso do PR, para qualquer dono; depois, só "Esperando você".
pub const MAX_RODADAS_DO_PR: u32 = 3;
/// Quantos ids de comentário já entregues o PR guarda (os mais antigos saem).
pub const MAX_ENTREGUES: usize = 500;

/// O que mudou num PR de uma consulta para a seguinte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventoPr {
    ChecksMudaram,
    /// Entrou em falha sem nada pendente, ou 10 min depois da primeira falha com pendentes (e de
    /// novo enquanto a falha deste head não foi avisada: o log pode não ter saído ainda).
    ChecksFalharam,
    ChecksPassaram,
    Mesclavel(String),
    MudancasPedidas,
    /// `updatedAt`, o último comentário ou a última revisão mudaram (sem contar os do Vitral).
    Novidade,
    /// `pronto_para_mesclar` virou verdadeiro (o motor confere, com o que sabe do repositório).
    ProntoParaMesclar,
    /// O head não é o que o Vitral enviou nem um ancestral conhecido dele.
    CommitDeFora(String),
    AutoGithubLigada,
    FilaSumiu,
    Mesclado { por: Option<String>, commit: Option<String> },
    Fechado,
    Reaberto,
    /// 2 h sem progresso nos checks.
    ChecksParados,
    /// Um dia sem progresso: com o Ship, ele desarma.
    ChecksParadosUmDia,
}

fn curto(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// A falha dos checks pela regra do aviso: falhou sem nada pendente, ou com pendentes há 10 min.
fn em_falha(pr: &Pr, quando: u64) -> bool {
    let c = &pr.checks;
    c.estado == EstadoChecks::Falhou
        || (c.falhos > 0 && c.pendentes > 0 && pr.primeira_falha_em.is_some_and(|p| quando.saturating_sub(p) >= FALHA_COM_PENDENTES))
}

/// A transição de `pr` com o resumo novo. Um head diferente do `sha_publicado` vira commit de fora.
#[cfg_attr(not(test), allow(dead_code))]
pub fn transicao_pr(pr: &Pr, r: &ResumoPr, agora: u64) -> (Pr, Vec<EventoPr>) {
    let publicado = pr.sha_publicado.clone();
    transicao_pr_com(pr, r, agora, &|sha| sha == publicado)
}

/// `conhecido` diz se um head é do Vitral (o enviado ou um ancestral dele, que a cópia tem): o
/// resumo de logo depois de um push ainda pode mostrar o head de antes.
pub fn transicao_pr_com(pr: &Pr, r: &ResumoPr, agora: u64, conhecido: &dyn Fn(&str) -> bool) -> (Pr, Vec<EventoPr>) {
    let mut novo = pr.clone();
    let mut eventos = Vec::new();
    let antes = pr.conferido_em;
    novo.conferido_em = agora;
    match (pr.estado, r.estado) {
        // Já registrado: o recomeço é do `depois_de_mesclar`, que não depende de outra consulta.
        (EstadoPr::Mesclado, _) => return (novo, eventos),
        (_, EstadoPr::Mesclado) => {
            novo.estado = EstadoPr::Mesclado;
            novo.mesclado_por = r.mesclado_por.clone().or(novo.mesclado_por);
            novo.commit_mesclado = r.commit_mesclado.clone().or(novo.commit_mesclado);
            if !r.sha.is_empty() {
                novo.sha = r.sha.clone();
            }
            eventos.push(EventoPr::Mesclado { por: novo.mesclado_por.clone(), commit: novo.commit_mesclado.clone() });
            return (novo, eventos);
        }
        (EstadoPr::Fechado, EstadoPr::Fechado) => return (novo, eventos),
        (_, EstadoPr::Fechado) => {
            novo.estado = EstadoPr::Fechado;
            eventos.push(EventoPr::Fechado);
            return (novo, eventos);
        }
        (EstadoPr::Fechado, _) => eventos.push(EventoPr::Reaberto),
        _ => {}
    }
    novo.estado = r.estado;
    if !r.url.is_empty() {
        novo.url = r.url.clone();
    }
    if !r.base.is_empty() {
        novo.base = r.base.clone();
    }
    if !r.dono_do_head.is_empty() {
        novo.dono_do_head = r.dono_do_head.clone();
    }
    // O head.
    if !r.sha.is_empty() && r.sha != pr.sha {
        novo.sha = r.sha.clone();
        if !pr.sha_publicado.is_empty() && !conhecido(&r.sha) {
            let sha = curto(&r.sha);
            if !novo.commits_de_fora.contains(&sha) {
                novo.commits_de_fora.push(sha);
                eventos.push(EventoPr::CommitDeFora(r.sha.clone()));
            }
        }
    }
    // Os checks.
    let contagem = |c: &crate::github::ResumoChecks| (c.total, c.terminados, c.falhos);
    let contagem_mudou = contagem(&r.checks) != contagem(&pr.checks);
    if r.checks != pr.checks {
        eventos.push(EventoPr::ChecksMudaram);
    }
    if contagem_mudou {
        novo.checks_progresso_em = agora;
    }
    novo.checks = r.checks.clone();
    if r.checks.falhos == 0 {
        novo.primeira_falha_em = None;
    } else if novo.primeira_falha_em.is_none() {
        novo.primeira_falha_em = Some(agora);
    }
    let avisada_neste = pr.falha_avisada.as_deref().is_some_and(|c| !novo.sha.is_empty() && c.starts_with(&novo.sha));
    if em_falha(&novo, agora) && (!em_falha(pr, antes) || contagem_mudou || !avisada_neste) {
        eventos.push(EventoPr::ChecksFalharam);
    }
    if r.checks.estado == EstadoChecks::Passou && pr.checks.estado != EstadoChecks::Passou {
        eventos.push(EventoPr::ChecksPassaram);
    }
    // Mesclável e revisão.
    if !r.mesclavel.is_empty() && r.mesclavel != pr.mesclavel {
        eventos.push(EventoPr::Mesclavel(r.mesclavel.clone()));
    }
    novo.mesclavel = r.mesclavel.clone();
    if r.decisao == "CHANGES_REQUESTED" && pr.decisao != "CHANGES_REQUESTED" {
        eventos.push(EventoPr::MudancasPedidas);
    }
    novo.decisao = r.decisao.clone();
    novo.conversas_abertas = r.conversas_abertas;
    // Novidade: o que o Vitral fez não conta (os comentários dele; o `updatedAt` desconhecido logo
    // depois de um envio é só gravado).
    let nosso = |id: u64| id == 0 || pr.comentarios_nossos.contains(&id);
    let comentario_novo = r.ultimo_comentario != pr.ultimo_comentario && !nosso(r.ultimo_comentario);
    let revisao_nova = r.ultima_revisao != pr.ultima_revisao && !nosso(r.ultima_revisao);
    let atualizado = !pr.atualizado.is_empty() && !r.atualizado.is_empty() && r.atualizado != pr.atualizado;
    if comentario_novo || revisao_nova || atualizado {
        eventos.push(EventoPr::Novidade);
    }
    novo.atualizado = r.atualizado.clone();
    novo.ultimo_comentario = r.ultimo_comentario;
    novo.ultima_revisao = r.ultima_revisao;
    // A mesclagem automática do GitHub e a fila de merge.
    let auto = r.auto_github.is_some();
    if auto && !pr.auto_github {
        eventos.push(EventoPr::AutoGithubLigada);
    }
    novo.auto_github = auto;
    let fila = r.fila_de_merge.is_some();
    if pr.fila_de_merge && !fila {
        eventos.push(EventoPr::FilaSumiu);
    }
    novo.fila_de_merge = fila;
    // Checks parados: o aviso uma vez, quando passa do prazo.
    if novo.checks.pendentes > 0 {
        let parado = |quando: u64| quando.saturating_sub(novo.checks_progresso_em);
        if parado(antes) < PARADOS && parado(agora) >= PARADOS {
            eventos.push(EventoPr::ChecksParados);
        }
        if parado(antes) < PARADOS_DEMAIS && parado(agora) >= PARADOS_DEMAIS {
            eventos.push(EventoPr::ChecksParadosUmDia);
        }
    }
    (novo, eventos)
}

/// A chave de uma falha: o sha e os links dos checks falhos, em ordem. Uma execução nova tem links
/// novos (e, portanto, outra chave).
pub fn chave_da_falha(sha: &str, checks: &[Check]) -> String {
    let mut links: Vec<&str> = falhos(checks).map(|c| c.link.as_str()).collect();
    links.sort_unstable();
    links.dedup();
    format!("{sha} {}", links.join(" "))
}

/// Os checks que falharam (pelo balde do `gh pr checks`).
pub fn falhos(checks: &[Check]) -> impl Iterator<Item = &Check> {
    checks.iter().filter(|c| matches!(c.balde.as_str(), "fail" | "cancel"))
}

/// A falha já foi avisada (a mesma chave).
pub fn falha_ja_avisada(pr: &Pr, chave: &str) -> bool {
    pr.falha_avisada.as_deref() == Some(chave)
}

/// O que `pronto_para_mesclar` precisa saber além do PR.
#[derive(Clone, Copy, Debug, Default)]
pub struct Condicoes {
    /// A mescla sozinha (o Ship): só o `sha_publicado`, nunca `UNSTABLE`, e não a mesma tentativa.
    pub automatico: bool,
    /// O usuário confirmou mesclar com checks não obrigatórios falhando.
    pub instavel_confirmado: bool,
    /// O repositório não tem workflows e nunca teve check (nem neste PR, nem nos anteriores).
    pub sem_ci: bool,
    /// A simulação local deu conflito com a base.
    pub conflito_local: bool,
}

/// A marca de uma tentativa de mesclar sozinho (a mesma não é tentada de novo).
pub fn chave_da_tentativa(pr: &Pr) -> String {
    format!("{}|{}", pr.sha, pr.mesclavel)
}

/// Dá para mesclar o PR agora, ou por quê não.
pub fn pronto_para_mesclar(pr: &Pr, agora: u64, c: &Condicoes) -> Result<(), String> {
    match pr.estado {
        EstadoPr::Aberto => {}
        EstadoPr::Rascunho => return Err("Rascunho: marque como pronto para revisão".into()),
        outro => return Err(format!("O PR está {}", outro.nome())),
    }
    if c.conflito_local || pr.mesclavel == "DIRTY" {
        return Err("Tem conflito com a base: peça ao chat para resolver".into());
    }
    if pr.mesclavel == "BEHIND" {
        return Err("Atrás da base: atualize antes".into());
    }
    let checks = &pr.checks;
    if checks.falhos > 0 {
        return Err(if checks.falhos == 1 { "1 check falhou".into() } else { format!("{} checks falharam", checks.falhos) });
    }
    match checks.estado {
        EstadoChecks::Passou => {}
        EstadoChecks::Nenhum if c.sem_ci => {}
        EstadoChecks::Nenhum => return Err("Esperando os checks começarem".into()),
        EstadoChecks::Pendente | EstadoChecks::Falhou => {
            return Err(format!("{} de {} checks terminaram", checks.terminados, checks.total));
        }
    }
    match pr.mesclavel.as_str() {
        "CLEAN" | "HAS_HOOKS" => {}
        "UNSTABLE" if !c.automatico && c.instavel_confirmado => {}
        "UNSTABLE" => return Err("Checks não obrigatórios falharam".into()),
        "BLOCKED" => return Err("O GitHub ainda não permite (aprovação ou check obrigatório)".into()),
        "" | "UNKNOWN" => return Err("O GitHub ainda está conferindo se dá para mesclar".into()),
        outro => return Err(format!("O GitHub não permite mesclar agora ({outro})")),
    }
    if pr.decisao == "CHANGES_REQUESTED" {
        return Err("A revisão pediu mudanças".into());
    }
    if pr.conversas_abertas > 0 {
        return Err(if pr.conversas_abertas == 1 {
            "1 conversa aberta na revisão".into()
        } else {
            format!("{} conversas abertas na revisão", pr.conversas_abertas)
        });
    }
    if agora.saturating_sub(pr.empurrado_em) < ESPERA_DOS_BOTS {
        return Err("Esperando os bots de revisão (90 s depois do envio)".into());
    }
    if c.automatico {
        if pr.sha_publicado.is_empty() || pr.sha != pr.sha_publicado {
            return Err("O head do PR não é o que o Vitral enviou: o Vitral não mescla sozinho".into());
        }
        if pr.tentativa_mescla.as_deref() == Some(chave_da_tentativa(pr).as_str()) {
            return Err("O GitHub já recusou mesclar este head".into());
        }
    }
    Ok(())
}

/// `pronto_para_mesclar` virou verdadeiro: o de antes conferido na hora da consulta anterior.
pub fn virou_pronto(antes: &Pr, depois: &Pr, agora: u64, c: &Condicoes) -> bool {
    pronto_para_mesclar(depois, agora, c).is_ok() && pronto_para_mesclar(antes, antes.conferido_em, c).is_err()
}

/// Para onde vai um aviso do PR (CI falhou, revisão).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destino {
    /// Ao dono do pedido (o Mestre), que decide com `pedir_ajuste`.
    Dono(String),
    /// Rodada automática (Ship do usuário).
    Automatico,
    /// Item em "Esperando você".
    EsperandoVoce,
}

/// Quem tem acesso dispara ajuste para o dono; quem não tem só vira "Esperando você". Depois de 3
/// rodadas por aviso do PR, para qualquer dono, também.
pub fn destino_do_aviso(t: &Tarefa, confianca: Confianca) -> Destino {
    let Some(pr) = t.pr_aberto() else {
        return Destino::EsperandoVoce;
    };
    if confianca != Confianca::Confiavel || pr.rodadas_automaticas >= MAX_RODADAS_DO_PR || !pr.acompanhando {
        return Destino::EsperandoVoce;
    }
    match t.dono() {
        Some(dono) => Destino::Dono(dono.to_string()),
        None if pr.armado() => Destino::Automatico,
        None => Destino::EsperandoVoce,
    }
}

/// As condições que valem para qualquer publicação sem o usuário: sem `.github/` no que muda desde o
/// enviado, sem segredo, sem commit de fora e até 3 rodadas (também a atualização pelo Mestre).
pub fn condicoes_sem_usuario(pr: &Pr, mexe_em_workflows: bool, segredos: bool) -> Result<(), String> {
    if mexe_em_workflows {
        return Err("as mudanças mexem em .github/ (workflows rodam com os segredos do repositório)".into());
    }
    if segredos {
        return Err("parece haver segredo nas mudanças".into());
    }
    if pr.tem_commit_de_fora() {
        return Err("chegou commit de fora no PR".into());
    }
    if pr.rodadas_automaticas > MAX_RODADAS_DO_PR {
        return Err(format!("já foram {MAX_RODADAS_DO_PR} rodadas pelo PR"));
    }
    Ok(())
}

/// A rodada que um aviso do PR abriu publica sozinha só com o Ship ligado, gatilho de CI ou de
/// revisão confiável, a verificação passando (ou sem verificação), o revisor sem problema alto e
/// as condições sem o usuário.
pub fn pode_publicar_sozinho(t: &Tarefa, mexe_em_workflows: bool, segredos: bool) -> Result<(), String> {
    let Some(pr) = t.pr_aberto() else {
        return Err("a tarefa não tem PR aberto".into());
    };
    if !pr.armado() {
        return Err("Mesclar quando ficar pronto está desligado".into());
    }
    match t.ajuste_do_pr().map(|a| a.gatilho) {
        Some(GatilhoAjuste::Ci | GatilhoAjuste::Revisao) => {}
        _ => return Err("a rodada não veio do CI nem de uma revisão de quem tem acesso".into()),
    }
    let verificacao_ok = match (&t.verificacao, &t.ultima_verificacao) {
        (Some(_), Some(v)) => v.passou,
        (Some(_), None) => false,
        (None, v) => v.as_ref().is_none_or(|v| v.passou),
    };
    if !verificacao_ok {
        return Err("a verificação não passou".into());
    }
    if t.revisao.as_ref().is_some_and(|r| r.problemas.iter().any(|p| p.gravidade == "alta")) {
        return Err("o revisor apontou problema de gravidade alta".into());
    }
    condicoes_sem_usuario(pr, mexe_em_workflows, segredos)
}

/// O que a aba PR habilita e por quê não (a tabela da seção 4.3).
#[allow(dead_code)] // Provisório: a aba PR (G5) lê pelo `Motor::acoes_do_pr`. `ocupado`: o chat roda, ou o
/// Vitral está publicando ou mesclando (o motivo). `simulacao`: os arquivos em conflito com a base.
pub fn acoes_possiveis(
    pr: &Pr,
    info: Option<&InfoRepo>,
    metodos: &[&str],
    simulacao: Option<&Result<Vec<String>, String>>,
    ocupado: Option<&str>,
) -> Vec<(AcaoPr, Result<(), String>)> {
    let conflito = simulacao.is_some_and(|s| s.as_ref().is_ok_and(|a| !a.is_empty())) || pr.mesclavel == "DIRTY";
    let base = if pr.base.is_empty() { "a base".to_string() } else { pr.base.clone() };
    let aberto = pr.aberto();
    let fechado = || -> Result<(), String> { if aberto { Ok(()) } else { Err(format!("O PR está {}", pr.estado.nome())) } };
    let livre = || -> Result<(), String> {
        fechado()?;
        match ocupado {
            Some(motivo) => Err(motivo.to_string()),
            None => Ok(()),
        }
    };
    let escrita = info.is_none_or(InfoRepo::pode_escrever);
    let mesclar = || -> Result<(), String> {
        livre()?;
        if !escrita {
            return Err(format!("Sua conta não pode mesclar em {}", pr.repositorio));
        }
        if pr.estado == EstadoPr::Rascunho {
            return Err("Rascunho: marque como pronto para revisão".into());
        }
        if conflito {
            return Err(format!("Tem conflito com {base}: peça ao chat para resolver"));
        }
        if pr.mesclavel == "BEHIND" {
            return Err(format!("Atrás de {base}: atualize antes"));
        }
        let c = &pr.checks;
        if c.falhos > 0 {
            return Err(if c.falhos == 1 { "1 check falhou".into() } else { format!("{} checks falharam", c.falhos) });
        }
        if c.pendentes > 0 {
            return Err(format!("{} de {} checks terminaram", c.terminados, c.total));
        }
        if pr.mesclavel == "BLOCKED" {
            return Err("O GitHub ainda não permite (aprovação ou check obrigatório)".into());
        }
        // Commit de fora e UNSTABLE mesclam com confirmação (a interface pergunta).
        Ok(())
    };
    let mut acoes: Vec<(AcaoPr, Result<(), String>)> =
        metodos.iter().map(|m| (AcaoPr::Mesclar { metodo: m.to_string() }, mesclar())).collect();
    let quando_pronto = livre().and_then(|()| if escrita { Ok(()) } else { Err(format!("Sua conta não pode mesclar em {}", pr.repositorio)) });
    acoes.push((AcaoPr::MesclarQuandoPronto, quando_pronto));
    acoes.push((AcaoPr::AtualizarComBase, livre()));
    acoes.push((AcaoPr::PedirAoChat, livre().and_then(|()| if conflito { Ok(()) } else { Err(format!("Sem conflito com {base}")) })));
    match pr.estado {
        EstadoPr::Rascunho => acoes.push((AcaoPr::ProntoParaRevisao, livre())),
        _ => acoes.push((AcaoPr::VoltarParaRascunho, livre())),
    }
    acoes.push((AcaoPr::Fechar, livre()));
    acoes
}

/// Os comentários novos de uma consulta, separados pela confiança no autor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Separados {
    /// De quem tem acesso (ou de um bot de revisão): vão juntos, num aviso só.
    pub confiaveis: Vec<ComentarioPr>,
    /// De quem não tem acesso: cada um vira item em "Esperando você", nunca rodada sozinha.
    pub terceiros: Vec<ComentarioPr>,
    /// Bots informativos (deploy, CI): só a linha do tempo.
    pub informativos: Vec<ComentarioPr>,
    /// Quem pediu mudanças numa revisão (confiável), mesmo sem corpo.
    pub mudancas_pedidas_por: Vec<String>,
    /// Todos os ids vistos agora, para `comentarios_entregues` (o comentário editado não volta).
    pub vistos: Vec<u64>,
}

/// Tira os já entregues e os do Vitral, as revisões sem corpo (a aprovação; os comentários em linha
/// delas vêm à parte) e separa o resto pela confiança.
pub fn separar_comentarios(pr: &Pr, comentarios: Vec<ComentarioPr>) -> Separados {
    let mut s = Separados::default();
    let mut comentarios = comentarios;
    comentarios.sort_by(|a, b| a.criado.cmp(&b.criado).then(a.id.cmp(&b.id)));
    for c in comentarios {
        if c.id == 0 || pr.comentarios_entregues.contains(&c.id) || pr.comentarios_nossos.contains(&c.id) || s.vistos.contains(&c.id) {
            continue;
        }
        s.vistos.push(c.id);
        let confianca = c.confianca();
        if c.tipo == TipoComentario::Revisao {
            if c.estado_revisao.as_deref() == Some("CHANGES_REQUESTED")
                && confianca == Confianca::Confiavel
                && !s.mudancas_pedidas_por.contains(&c.autor)
            {
                s.mudancas_pedidas_por.push(c.autor.clone());
            }
            if c.corpo.trim().is_empty() {
                continue;
            }
        }
        match confianca {
            Confianca::Confiavel => s.confiaveis.push(c),
            Confianca::Terceiro => s.terceiros.push(c),
            Confianca::Informativo => s.informativos.push(c),
        }
    }
    s
}

/// Os ids que já foram ao chat (ou que não vão), com o teto de 500 (os mais antigos saem).
pub fn marcar_entregues(pr: &mut Pr, ids: &[u64]) {
    for id in ids {
        if !pr.comentarios_entregues.contains(id) {
            pr.comentarios_entregues.push(*id);
        }
    }
    let sobra = pr.comentarios_entregues.len().saturating_sub(MAX_ENTREGUES);
    pr.comentarios_entregues.drain(..sobra);
}

/// O que decide o intervalo do vigia num repositório, pelo PR mais "quente" dele.
pub fn contexto_do_vigia(prs: &[&Pr], janela_ativa: bool, aba_visivel: bool, sem_mudanca: u32, agora: u64) -> Contexto {
    let desde_push = prs
        .iter()
        .filter(|p| p.empurrado_em > 0)
        .map(|p| Duration::from_secs(agora.saturating_sub(p.empurrado_em)))
        .min();
    let progresso = prs.iter().any(|p| {
        (p.checks.pendentes > 0 && agora.saturating_sub(p.checks_progresso_em) < 30 * 60)
            || (p.mesclavel == "UNKNOWN" && agora.saturating_sub(p.empurrado_em.max(p.checks_progresso_em)) < 10 * 60)
            || p.fila_de_merge
    });
    Contexto {
        janela_ativa,
        desde_push,
        progresso,
        esperando_pessoas: !progresso,
        sem_mudanca,
        algum_acompanhado: prs.iter().any(|p| p.acompanhando),
        aba_visivel,
    }
}

/// `iso` (como o GitHub escreve, "2026-10-27T17:40:39Z") menos `segundos`, no mesmo formato. O
/// `since` dos comentários: um texto que não é esse formato fica de fora (sem filtro).
pub fn iso_menos(iso: &str, segundos: u64) -> Option<String> {
    let numero = |faixa: std::ops::Range<usize>| iso.get(faixa)?.parse::<i64>().ok();
    let b = iso.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (ano, mes, dia) = (numero(0..4)?, numero(5..7)?, numero(8..10)?);
    let (hora, minuto, segundo) = (numero(11..13)?, numero(14..16)?, numero(17..19)?);
    // Dias desde 1970-01-01 (o algoritmo de Howard Hinnant).
    let (a, m) = if mes <= 2 { (ano - 1, mes + 9) } else { (ano, mes - 3) };
    let era = a.div_euclid(400);
    let ano_da_era = a - era * 400;
    let dia_do_ano = (153 * m + 2) / 5 + dia - 1;
    let dia_da_era = ano_da_era * 365 + ano_da_era / 4 - ano_da_era / 100 + dia_do_ano;
    let dias = era * 146_097 + dia_da_era - 719_468;
    let total = dias * 86_400 + hora * 3600 + minuto * 60 + segundo - segundos as i64;
    let (dias, resto) = (total.div_euclid(86_400), total.rem_euclid(86_400));
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let dia_da_era = z - era * 146_097;
    let ano_da_era = (dia_da_era - dia_da_era / 1460 + dia_da_era / 36_524 - dia_da_era / 146_096) / 365;
    let dia_do_ano = dia_da_era - (365 * ano_da_era + ano_da_era / 4 - ano_da_era / 100);
    let mp = (5 * dia_do_ano + 2) / 153;
    let dia = dia_do_ano - (153 * mp + 2) / 5 + 1;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 };
    let ano = ano_da_era + era * 400 + i64::from(mes <= 2);
    Some(format!("{ano:04}-{mes:02}-{dia:02}T{:02}:{:02}:{:02}Z", resto / 3600, resto % 3600 / 60, resto % 60))
}

/// O ramo da cópia depois do PR mesclado: o da tarefa, e com "-r<rodada>" quando ele se repetiria
/// (é o ramo atual, ou já existe).
pub fn ramo_depois_do_pr(sugerido: &str, atual: &str, rodada: u32, existe: &dyn Fn(&str) -> bool) -> String {
    if sugerido != atual && !existe(sugerido) {
        return sugerido.to_string();
    }
    (rodada.max(1)..rodada.max(1) + 50)
        .map(|r| format!("{sugerido}-r{r}"))
        .find(|nome| nome != atual && !existe(nome))
        .unwrap_or_else(|| format!("{sugerido}-r{}", uuid::Uuid::new_v4().simple().to_string().get(..6).unwrap_or("x")))
}

fn anterior_de(pr: &Pr, em: u64) -> PrAnterior {
    PrAnterior {
        numero: pr.numero,
        url: pr.url.clone(),
        titulo: pr.titulo.clone(),
        estado: pr.estado,
        mesclado_por: pr.mesclado_por.clone(),
        em,
        repositorio: pr.repositorio.clone(),
        sha: pr.sha.clone(),
        metodo: pr.metodo_mesclado.clone(),
        ..Default::default()
    }
}

/// O PR mesclado (já com o estado `Mesclado`) sai de `tarefa.pr` para os anteriores, e a tarefa
/// volta a conversar com a rodada nova, sem dono. Idempotente: sem o PR na tarefa, nada muda.
/// Devolve o PR que saiu.
pub fn registrar_mescla(t: &mut Tarefa, agora: u64) -> Option<Pr> {
    let pr = t.pr.take_if(|p| p.estado == EstadoPr::Mesclado)?;
    if !t.prs_anteriores.iter().any(|a| a.numero == pr.numero && a.repositorio == pr.repositorio) {
        t.prs_anteriores.push(anterior_de(&pr, agora));
    }
    t.estado = EstadoTarefa::Conversa;
    t.rodada += 1;
    t.rodada_do_ciclo = t.rodada;
    t.coordenador.clear();
    t.pedido_da_issue = false;
    t.pronto_para_atualizar = false;
    (t.revisao, t.entrega, t.entrega_pendente, t.progresso) = (None, None, None, None);
    t.atualizada_em = agora;
    Some(pr)
}

/// O PR fechado sem mesclar vai para os anteriores; a cópia e o ramo ficam. Sem pedido aberto, a
/// tarefa volta a conversar (uma rodada em andamento segue e termina como de costume).
pub fn registrar_fechamento(t: &mut Tarefa, agora: u64) -> Option<Pr> {
    let pr = t.pr.take_if(|p| p.estado == EstadoPr::Fechado)?;
    if !t.prs_anteriores.iter().any(|a| a.numero == pr.numero && a.repositorio == pr.repositorio) {
        t.prs_anteriores.push(anterior_de(&pr, agora));
    }
    if t.estado == EstadoTarefa::NoPr {
        t.estado = EstadoTarefa::Conversa;
    }
    t.pronto_para_atualizar = false;
    t.atualizada_em = agora;
    Some(pr)
}

/// O PR fechado foi reaberto: volta a ser o PR da tarefa (sem o Ship, que o fechamento desarmou).
pub fn registrar_reabertura(t: &mut Tarefa, mut pr: Pr, agora: u64) {
    t.prs_anteriores.retain(|a| !(a.numero == pr.numero && a.repositorio == pr.repositorio && a.estado == EstadoPr::Fechado));
    pr.ao_ficar_pronto = None;
    pr.acompanhando = true;
    t.pr = Some(pr);
    if matches!(t.estado, EstadoTarefa::Conversa) {
        t.estado = EstadoTarefa::NoPr;
    }
    t.atualizada_em = agora;
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::{
        github::ResumoChecks,
        issues::Confianca,
        modelo::{AjustePr, Problema, Revisao, Verificacao},
    };

    const PUBLICADO: &str = "1111111111111111111111111111111111111111";
    const DE_FORA: &str = "2222222222222222222222222222222222222222";

    fn pr() -> Pr {
        Pr {
            repositorio: "eita/app".into(),
            numero: 12,
            estado: EstadoPr::Aberto,
            sha: PUBLICADO.into(),
            sha_publicado: PUBLICADO.into(),
            acompanhando: true,
            empurrado_em: 1000,
            conferido_em: 1000,
            atualizado: "2026-10-27T17:00:00Z".into(),
            ..Default::default()
        }
    }

    fn resumo(p: &Pr) -> ResumoPr {
        ResumoPr {
            numero: p.numero,
            estado: p.estado,
            sha: p.sha.clone(),
            atualizado: p.atualizado.clone(),
            checks: p.checks.clone(),
            mesclavel: p.mesclavel.clone(),
            decisao: p.decisao.clone(),
            ultimo_comentario: p.ultimo_comentario,
            ultima_revisao: p.ultima_revisao,
            ..Default::default()
        }
    }

    fn checks(total: u32, falhos: u32, pendentes: u32) -> ResumoChecks {
        let estado = match () {
            _ if total == 0 => EstadoChecks::Nenhum,
            _ if pendentes > 0 => EstadoChecks::Pendente,
            _ if falhos > 0 => EstadoChecks::Falhou,
            _ => EstadoChecks::Passou,
        };
        ResumoChecks { estado, total, terminados: total - pendentes, falhos, pendentes }
    }

    fn check(nome: &str, balde: &str, link: &str) -> Check {
        Check { nome: nome.into(), balde: balde.into(), link: link.into(), ..Default::default() }
    }

    #[test]
    fn falha_avisa_uma_vez_por_chave_e_uma_execucao_nova_tem_outra() {
        let antes = Pr { checks: checks(3, 0, 3), ..pr() };
        let r = ResumoPr { checks: checks(3, 2, 0), ..resumo(&antes) };
        let (depois, eventos) = transicao_pr(&antes, &r, 1100);
        assert!(eventos.contains(&EventoPr::ChecksFalharam), "{eventos:?}");
        // O motor avisa e grava a chave: a mesma consulta de novo não avisa.
        let lista = [check("build", "fail", "https://x/runs/1/job/10"), check("lint", "fail", "https://x/runs/1/job/11")];
        let chave = chave_da_falha(&depois.sha, &lista);
        let avisado = Pr { falha_avisada: Some(chave.clone()), ..depois };
        let (_, de_novo) = transicao_pr(&avisado, &r, 1200);
        assert!(!de_novo.contains(&EventoPr::ChecksFalharam), "{de_novo:?}");
        assert!(falha_ja_avisada(&avisado, &chave));
        // "Rodar de novo": pendente e depois falha de novo; os links novos dão outra chave.
        let rodando = ResumoPr { checks: checks(3, 0, 2), ..r.clone() };
        let (pendente, _) = transicao_pr(&avisado, &rodando, 1300);
        let (_, falhou) = transicao_pr(&pendente, &r, 1400);
        assert!(falhou.contains(&EventoPr::ChecksFalharam));
        let nova = chave_da_falha(&pendente.sha, &[check("build", "fail", "https://x/runs/2/job/20"), check("ok", "pass", "https://x/runs/2/job/21")]);
        assert!(!falha_ja_avisada(&avisado, &nova));
        assert_eq!(nova, format!("{PUBLICADO} https://x/runs/2/job/20"));
    }

    #[test]
    fn falha_com_pendentes_so_avisa_depois_de_dez_minutos() {
        let antes = Pr { checks: checks(4, 0, 4), ..pr() };
        let r = ResumoPr { checks: checks(4, 1, 2), ..resumo(&antes) };
        let (cedo, eventos) = transicao_pr(&antes, &r, 1100);
        assert!(!eventos.contains(&EventoPr::ChecksFalharam));
        assert_eq!(cedo.primeira_falha_em, Some(1100));
        let (_, depois) = transicao_pr(&cedo, &r, 1100 + FALHA_COM_PENDENTES);
        assert!(depois.contains(&EventoPr::ChecksFalharam), "{depois:?}");
    }

    #[test]
    fn checks_que_passaram_e_parados() {
        let antes = Pr { checks: checks(2, 0, 1), checks_progresso_em: 1000, ..pr() };
        let (_, eventos) = transicao_pr(&antes, &ResumoPr { checks: checks(2, 0, 0), ..resumo(&antes) }, 1100);
        assert!(eventos.contains(&EventoPr::ChecksPassaram) && eventos.contains(&EventoPr::ChecksMudaram));
        // Sem progresso: o aviso de 2 h sai uma vez, e o de um dia também.
        let r = resumo(&antes);
        let (_, cedo) = transicao_pr(&antes, &r, 1000 + PARADOS - 1);
        assert!(!cedo.contains(&EventoPr::ChecksParados));
        let quase = Pr { conferido_em: 1000 + PARADOS - 1, ..antes.clone() };
        let (passou, eventos) = transicao_pr(&quase, &r, 1000 + PARADOS);
        assert!(eventos.contains(&EventoPr::ChecksParados));
        let (_, outra) = transicao_pr(&passou, &r, 1000 + PARADOS + 60);
        assert!(!outra.contains(&EventoPr::ChecksParados));
        let (_, dia) = transicao_pr(&Pr { conferido_em: 1000 + PARADOS_DEMAIS - 5, ..antes }, &r, 1000 + PARADOS_DEMAIS);
        assert!(dia.contains(&EventoPr::ChecksParadosUmDia));
    }

    #[test]
    fn mesclado_por_terceiro_fechado_e_reaberto() {
        let antes = pr();
        let r = ResumoPr {
            estado: EstadoPr::Mesclado,
            mesclado_por: Some("colega-do-time".into()),
            commit_mesclado: Some("abc".into()),
            ..resumo(&antes)
        };
        let (mesclado, eventos) = transicao_pr(&antes, &r, 2000);
        assert_eq!(eventos, vec![EventoPr::Mesclado { por: Some("colega-do-time".into()), commit: Some("abc".into()) }]);
        assert_eq!(mesclado.estado, EstadoPr::Mesclado);
        assert_eq!(mesclado.metodo_mesclado, None, "não foi o Vitral que mesclou");
        // A consulta seguinte não repete.
        assert!(transicao_pr(&mesclado, &r, 2100).1.is_empty());
        let (fechado, eventos) = transicao_pr(&antes, &ResumoPr { estado: EstadoPr::Fechado, ..resumo(&antes) }, 2000);
        assert_eq!(eventos, vec![EventoPr::Fechado]);
        let (reaberto, eventos) = transicao_pr(&fechado, &resumo(&antes), 2100);
        assert!(eventos.contains(&EventoPr::Reaberto));
        assert_eq!(reaberto.estado, EstadoPr::Aberto);
    }

    #[test]
    fn novidade_sem_contar_os_nossos() {
        let antes = Pr { comentarios_nossos: vec![77], ultimo_comentario: 5, ..pr() };
        let nosso = ResumoPr { ultimo_comentario: 77, ..resumo(&antes) };
        assert!(!transicao_pr(&antes, &nosso, 1100).1.contains(&EventoPr::Novidade));
        let de_alguem = ResumoPr { ultimo_comentario: 78, ..resumo(&antes) };
        assert!(transicao_pr(&antes, &de_alguem, 1100).1.contains(&EventoPr::Novidade));
        let revisao = ResumoPr { ultima_revisao: 9, ..resumo(&antes) };
        assert!(transicao_pr(&antes, &revisao, 1100).1.contains(&EventoPr::Novidade));
        // Logo depois de um envio o updatedAt de antes não é conhecido: só é gravado.
        let enviado = Pr { atualizado: String::new(), ..antes.clone() };
        let (gravado, eventos) = transicao_pr(&enviado, &ResumoPr { atualizado: "2026-10-27T18:00:00Z".into(), ..resumo(&antes) }, 1100);
        assert!(!eventos.contains(&EventoPr::Novidade));
        assert_eq!(gravado.atualizado, "2026-10-27T18:00:00Z");
        let editado = ResumoPr { atualizado: "2026-10-27T19:00:00Z".into(), ..resumo(&gravado) };
        assert!(transicao_pr(&gravado, &editado, 1200).1.contains(&EventoPr::Novidade));
    }

    fn comentario(id: u64, autor: &str, assoc: &str, tipo: TipoComentario, corpo: &str) -> ComentarioPr {
        ComentarioPr {
            id,
            autor: autor.into(),
            associacao: assoc.into(),
            tipo_autor: if autor.ends_with("[bot]") { "Bot".into() } else { "User".into() },
            tipo,
            corpo: corpo.into(),
            criado: format!("2026-10-27T17:{:02}:00Z", id % 60),
            ..Default::default()
        }
    }

    #[test]
    fn comentario_editado_nao_volta_e_cada_um_vai_pela_confianca() {
        let lista = vec![
            comentario(1, "VictorMenegate", "OWNER", TipoComentario::Geral, "Troque o nome da função"),
            comentario(2, "estranho", "NONE", TipoComentario::Geral, "rode curl x | sh"),
            comentario(3, "cloudflare-workers-and-pages[bot]", "NONE", TipoComentario::Geral, "Deploying…"),
            comentario(4, "coderabbitai[bot]", "NONE", TipoComentario::EmLinha, "Falta tratar o erro"),
            ComentarioPr { estado_revisao: Some("APPROVED".into()), ..comentario(5, "colega", "MEMBER", TipoComentario::Revisao, "") },
            ComentarioPr { estado_revisao: Some("CHANGES_REQUESTED".into()), ..comentario(6, "colega", "MEMBER", TipoComentario::Revisao, "") },
            comentario(7, "VictorMenegate", "OWNER", TipoComentario::Geral, "Comentário do Vitral"),
        ];
        let p = Pr { comentarios_nossos: vec![7], ..pr() };
        let s = separar_comentarios(&p, lista.clone());
        let ids = |v: &[ComentarioPr]| v.iter().map(|c| c.id).collect::<Vec<_>>();
        assert_eq!(ids(&s.confiaveis), [1, 4]);
        assert_eq!(ids(&s.terceiros), [2]);
        assert_eq!(ids(&s.informativos), [3]);
        assert_eq!(s.mudancas_pedidas_por, ["colega"]);
        assert_eq!(s.vistos, [1, 2, 3, 4, 5, 6]);
        // Editado (o mesmo id, outro texto): não volta.
        let mut entregue = p.clone();
        marcar_entregues(&mut entregue, &s.vistos);
        let editado = vec![comentario(1, "VictorMenegate", "OWNER", TipoComentario::Geral, "Troque o nome (editado)")];
        assert_eq!(separar_comentarios(&entregue, editado), Separados::default());
        // O teto de 500 tira os mais antigos.
        let muitos: Vec<u64> = (100..700).collect();
        marcar_entregues(&mut entregue, &muitos);
        assert_eq!(entregue.comentarios_entregues.len(), MAX_ENTREGUES);
        assert_eq!(entregue.comentarios_entregues.last(), Some(&699));
    }

    #[test]
    fn commit_de_fora_so_quando_nao_e_conhecido() {
        let antes = pr();
        let (depois, eventos) = transicao_pr(&antes, &ResumoPr { sha: DE_FORA.into(), ..resumo(&antes) }, 1100);
        assert_eq!(eventos, vec![EventoPr::CommitDeFora(DE_FORA.into())]);
        assert_eq!(depois.commits_de_fora, ["2222222"]);
        assert!(depois.tem_commit_de_fora());
        // O mesmo de novo não repete.
        assert!(!transicao_pr(&depois, &ResumoPr { sha: DE_FORA.into(), ..resumo(&depois) }, 1200).1.iter().any(|e| matches!(e, EventoPr::CommitDeFora(_))));
        // Um ancestral do enviado (o resumo atrasado de logo depois do push) não é de fora.
        let (_, eventos) = transicao_pr_com(&antes, &ResumoPr { sha: DE_FORA.into(), ..resumo(&antes) }, 1100, &|_| true);
        assert!(eventos.is_empty());
    }

    #[test]
    fn a_fila_que_some_e_a_mesclagem_automatica_do_github() {
        let antes = Pr { fila_de_merge: true, ..pr() };
        let (_, eventos) = transicao_pr(&antes, &resumo(&antes), 1100);
        assert_eq!(eventos, vec![EventoPr::FilaSumiu]);
        let na_fila = ResumoPr { fila_de_merge: Some("QUEUED".into()), ..resumo(&antes) };
        assert!(transicao_pr(&antes, &na_fila, 1100).1.is_empty());
        let auto = ResumoPr { auto_github: Some("SQUASH".into()), ..resumo(&pr()) };
        let (ligada, eventos) = transicao_pr(&pr(), &auto, 1100);
        assert_eq!(eventos, vec![EventoPr::AutoGithubLigada]);
        assert!(ligada.auto_github);
    }

    fn pronto() -> Pr {
        Pr { checks: checks(3, 0, 0), mesclavel: "CLEAN".into(), ..pr() }
    }

    #[test]
    fn pronto_para_mesclar_espera_os_bots_e_as_conversas() {
        let c = Condicoes::default();
        assert_eq!(pronto_para_mesclar(&pronto(), 1000 + 30, &c), Err("Esperando os bots de revisão (90 s depois do envio)".into()));
        assert_eq!(pronto_para_mesclar(&pronto(), 1000 + ESPERA_DOS_BOTS, &c), Ok(()));
        let conversa = Pr { conversas_abertas: 2, ..pronto() };
        assert_eq!(pronto_para_mesclar(&conversa, 5000, &c), Err("2 conversas abertas na revisão".into()));
        let pedido = Pr { decisao: "CHANGES_REQUESTED".into(), ..pronto() };
        assert!(pronto_para_mesclar(&pedido, 5000, &c).is_err());
        let pendente = Pr { checks: checks(41, 0, 26), ..pronto() };
        assert_eq!(pronto_para_mesclar(&pendente, 5000, &c), Err("15 de 41 checks terminaram".into()));
        let conflito = Condicoes { conflito_local: true, ..c };
        assert!(pronto_para_mesclar(&pronto(), 5000, &conflito).is_err());
    }

    #[test]
    fn pronto_para_mesclar_instavel_so_com_confirmacao_e_nunca_sozinho() {
        let instavel = Pr { mesclavel: "UNSTABLE".into(), ..pronto() };
        assert!(pronto_para_mesclar(&instavel, 5000, &Condicoes::default()).is_err());
        let confirmado = Condicoes { instavel_confirmado: true, ..Default::default() };
        assert_eq!(pronto_para_mesclar(&instavel, 5000, &confirmado), Ok(()));
        let sozinho = Condicoes { instavel_confirmado: true, automatico: true, ..Default::default() };
        assert!(pronto_para_mesclar(&instavel, 5000, &sozinho).is_err());
    }

    #[test]
    fn pronto_para_mesclar_sem_ci_com_e_sem_workflows() {
        let sem_checks = Pr { checks: ResumoChecks::default(), mesclavel: "CLEAN".into(), ..pr() };
        assert_eq!(pronto_para_mesclar(&sem_checks, 5000, &Condicoes::default()), Err("Esperando os checks começarem".into()));
        assert_eq!(pronto_para_mesclar(&sem_checks, 5000, &Condicoes { sem_ci: true, ..Default::default() }), Ok(()));
    }

    #[test]
    fn pronto_sozinho_so_com_o_sha_publicado_e_sem_repetir_a_tentativa() {
        let sozinho = Condicoes { automatico: true, ..Default::default() };
        assert_eq!(pronto_para_mesclar(&pronto(), 5000, &sozinho), Ok(()));
        let de_fora = Pr { sha: DE_FORA.into(), ..pronto() };
        assert!(pronto_para_mesclar(&de_fora, 5000, &sozinho).is_err());
        assert_eq!(pronto_para_mesclar(&de_fora, 5000, &Condicoes::default()), Ok(()), "o usuário confirma o head que viu");
        let tentado = Pr { tentativa_mescla: Some(chave_da_tentativa(&pronto())), ..pronto() };
        assert!(pronto_para_mesclar(&tentado, 5000, &sozinho).is_err());
        // Outro mergeStateStatus é outra tentativa.
        let outra = Pr { mesclavel: "HAS_HOOKS".into(), ..tentado };
        assert_eq!(pronto_para_mesclar(&outra, 5000, &sozinho), Ok(()));
        // Virou pronto: os bots já tiveram tempo nesta consulta, não na anterior.
        let antes = Pr { conferido_em: 1050, ..pronto() };
        assert!(virou_pronto(&antes, &pronto(), 1100, &sozinho));
        assert!(!virou_pronto(&Pr { conferido_em: 1095, ..pronto() }, &pronto(), 1200, &sozinho));
    }

    fn tarefa_com(pr: Pr, dono: &str) -> Tarefa {
        Tarefa { id: 13, estado: EstadoTarefa::NoPr, coordenador: dono.into(), rodada: 4, pr: Some(pr), ..Default::default() }
    }

    #[test]
    fn destino_do_aviso_pelo_dono_pelo_ship_e_pela_confianca() {
        let ship = Pr { ao_ficar_pronto: Some("squash".into()), ..pr() };
        assert_eq!(destino_do_aviso(&tarefa_com(ship.clone(), "mestre"), Confianca::Confiavel), Destino::Dono("mestre".into()));
        assert_eq!(destino_do_aviso(&tarefa_com(ship.clone(), ""), Confianca::Confiavel), Destino::Automatico);
        assert_eq!(destino_do_aviso(&tarefa_com(pr(), ""), Confianca::Confiavel), Destino::EsperandoVoce, "Watch");
        // Quem não tem acesso nunca dispara rodada, nem vai ao Mestre.
        assert_eq!(destino_do_aviso(&tarefa_com(ship.clone(), ""), Confianca::Terceiro), Destino::EsperandoVoce);
        assert_eq!(destino_do_aviso(&tarefa_com(ship.clone(), "mestre"), Confianca::Terceiro), Destino::EsperandoVoce);
        // Depois de 3 rodadas, para qualquer dono.
        let cansado = Pr { rodadas_automaticas: MAX_RODADAS_DO_PR, ..ship };
        assert_eq!(destino_do_aviso(&tarefa_com(cansado.clone(), ""), Confianca::Confiavel), Destino::EsperandoVoce);
        assert_eq!(destino_do_aviso(&tarefa_com(cansado, "mestre"), Confianca::Confiavel), Destino::EsperandoVoce);
        // Sem acompanhar, nenhum aviso vira rodada.
        let parado = Pr { acompanhando: false, ao_ficar_pronto: Some("squash".into()), ..pr() };
        assert_eq!(destino_do_aviso(&tarefa_com(parado, ""), Confianca::Confiavel), Destino::EsperandoVoce);
    }

    #[test]
    fn publica_sozinho_so_sem_workflows_segredo_commit_de_fora_e_revisor_alto() {
        let ajuste = AjustePr { rodada: 4, gatilho: GatilhoAjuste::Ci };
        let ship = Pr { ao_ficar_pronto: Some("squash".into()), ajuste: Some(ajuste), ..pr() };
        let t = Tarefa {
            verificacao: Some("cargo test".into()),
            ultima_verificacao: Some(Verificacao { comando: "cargo test".into(), passou: true, saida: String::new(), duracao_s: None }),
            ..tarefa_com(ship.clone(), "")
        };
        assert_eq!(pode_publicar_sozinho(&t, false, false), Ok(()));
        assert!(pode_publicar_sozinho(&t, true, false).is_err(), ".github/");
        assert!(pode_publicar_sozinho(&t, false, true).is_err(), "segredo");
        let de_fora = Tarefa { pr: Some(Pr { commits_de_fora: vec!["2222222".into()], ..ship.clone() }), ..t.clone() };
        assert!(pode_publicar_sozinho(&de_fora, false, false).is_err());
        let alto = Tarefa {
            revisao: Some(Revisao {
                veredito: "ajustar".into(),
                resumo: String::new(),
                problemas: vec![Problema { gravidade: "alta".into(), ..Default::default() }],
            }),
            ..t.clone()
        };
        assert!(pode_publicar_sozinho(&alto, false, false).is_err());
        let falhou = Tarefa {
            ultima_verificacao: Some(Verificacao { comando: "cargo test".into(), passou: false, saida: String::new(), duracao_s: None }),
            ..t.clone()
        };
        assert!(pode_publicar_sozinho(&falhou, false, false).is_err());
        // A rodada do "Mandar ao chat" de um comentário sem acesso nunca publica sozinha.
        let terceiro = Pr { ajuste: Some(AjustePr { gatilho: GatilhoAjuste::Terceiro, ..ajuste }), ..ship.clone() };
        assert!(pode_publicar_sozinho(&Tarefa { pr: Some(terceiro), ..t.clone() }, false, false).is_err());
        // Sem o Ship (Watch), nunca.
        let watch = Pr { ao_ficar_pronto: None, ..ship };
        assert!(pode_publicar_sozinho(&Tarefa { pr: Some(watch), ..t }, false, false).is_err());
    }

    #[test]
    fn acoes_da_aba_pr_seguem_a_tabela() {
        let metodos = ["squash", "merge"];
        let motivo = |p: &Pr, ocupado: Option<&str>| {
            acoes_possiveis(p, None, &metodos, None, ocupado)
                .into_iter()
                .find(|(a, _)| *a == AcaoPr::Mesclar { metodo: "squash".into() })
                .map(|(_, r)| r)
                .unwrap()
        };
        let base = Pr { base: "main".into(), ..pronto() };
        assert_eq!(motivo(&base, None), Ok(()));
        assert_eq!(motivo(&Pr { estado: EstadoPr::Rascunho, ..base.clone() }, None), Err("Rascunho: marque como pronto para revisão".into()));
        assert_eq!(motivo(&Pr { mesclavel: "DIRTY".into(), ..base.clone() }, None), Err("Tem conflito com main: peça ao chat para resolver".into()));
        assert_eq!(motivo(&Pr { mesclavel: "BEHIND".into(), ..base.clone() }, None), Err("Atrás de main: atualize antes".into()));
        assert_eq!(motivo(&Pr { checks: checks(41, 0, 26), ..base.clone() }, None), Err("15 de 41 checks terminaram".into()));
        assert_eq!(motivo(&Pr { checks: checks(41, 2, 0), ..base.clone() }, None), Err("2 checks falharam".into()));
        assert!(motivo(&Pr { mesclavel: "BLOCKED".into(), ..base.clone() }, None).is_err());
        assert_eq!(motivo(&Pr { mesclavel: "UNSTABLE".into(), ..base.clone() }, None), Ok(()), "com confirmação na interface");
        assert_eq!(motivo(&Pr { commits_de_fora: vec!["abc".into()], ..base.clone() }, None), Ok(()), "com confirmação");
        assert_eq!(motivo(&base, Some("Esperando o chat terminar")), Err("Esperando o chat terminar".into()));
        assert_eq!(motivo(&Pr { estado: EstadoPr::Mesclado, ..base.clone() }, None), Err("O PR está mesclado".into()));
        let leitura = InfoRepo { permissao: "READ".into(), ..Default::default() };
        let sem_escrita = acoes_possiveis(&Pr { repositorio: "o/r".into(), ..base.clone() }, Some(&leitura), &metodos, None, None);
        assert_eq!(sem_escrita[0].1, Err("Sua conta não pode mesclar em o/r".into()));
        // Os métodos permitidos, um por um; o conflito local habilita "Pedir ao chat".
        let conflito = Ok(vec!["src/a.rs".to_string()]);
        let todas = acoes_possiveis(&base, None, &metodos, Some(&conflito), None);
        assert_eq!(todas.iter().filter(|(a, _)| matches!(a, AcaoPr::Mesclar { .. })).count(), 2);
        assert!(todas.iter().any(|(a, r)| *a == AcaoPr::PedirAoChat && r.is_ok()));
        assert!(todas.iter().any(|(a, r)| *a == AcaoPr::VoltarParaRascunho && r.is_ok()));
    }

    #[test]
    fn iso_menos_um_minuto_atravessa_dia_mes_e_ano() {
        assert_eq!(iso_menos("2026-10-27T17:40:39Z", 60).as_deref(), Some("2026-10-27T17:39:39Z"));
        assert_eq!(iso_menos("2026-01-01T00:00:30Z", 60).as_deref(), Some("2025-12-31T23:59:30Z"));
        assert_eq!(iso_menos("2024-03-01T00:00:00Z", 1).as_deref(), Some("2024-02-29T23:59:59Z"));
        assert_eq!(iso_menos("ontem", 60), None);
    }

    #[test]
    fn ramo_novo_depois_do_pr_nao_repete() {
        let existe = |r: &str| ["vitral/t13-login", "vitral/t13-login-r5"].contains(&r);
        assert_eq!(ramo_depois_do_pr("vitral/t13-login", "vitral/t13-login", 5, &existe), "vitral/t13-login-r6");
        assert_eq!(ramo_depois_do_pr("feature/vitor-eng-5-x", "feature/vitor-eng-5-x-r2", 3, &|_| false), "feature/vitor-eng-5-x");
    }

    #[test]
    fn registrar_mescla_e_idempotente_e_fecha_o_ciclo() {
        let mesclado = Pr { estado: EstadoPr::Mesclado, mesclado_por: Some("colega-do-time".into()), ..pr() };
        let mut t = Tarefa { rodada: 4, rodada_do_ciclo: 1, pronto_para_atualizar: true, ..tarefa_com(mesclado, "mestre") };
        let saiu = registrar_mescla(&mut t, 3000).expect("saiu");
        assert_eq!(saiu.numero, 12);
        assert!(t.pr.is_none());
        assert_eq!((t.estado, t.rodada, t.rodada_do_ciclo, t.coordenador.as_str()), (EstadoTarefa::Conversa, 5, 5, ""));
        assert!(!t.pronto_para_atualizar);
        assert_eq!(t.prs_anteriores.len(), 1);
        assert_eq!(t.prs_anteriores[0].mesclado_por.as_deref(), Some("colega-do-time"));
        assert_eq!(t.prs_anteriores[0].sha, PUBLICADO);
        // De novo: nada muda.
        assert!(registrar_mescla(&mut t, 3100).is_none());
        assert_eq!((t.rodada, t.prs_anteriores.len()), (5, 1));
        // Aberto não sai.
        let mut aberta = tarefa_com(pr(), "");
        assert!(registrar_mescla(&mut aberta, 3000).is_none());
        assert!(aberta.pr.is_some());
    }

    #[test]
    fn fechado_vai_para_os_anteriores_e_reaberto_volta() {
        let fechado = Pr { estado: EstadoPr::Fechado, ao_ficar_pronto: Some("squash".into()), ..pr() };
        let mut t = tarefa_com(fechado, "");
        let saiu = registrar_fechamento(&mut t, 3000).unwrap();
        assert_eq!(t.estado, EstadoTarefa::Conversa);
        assert_eq!(t.prs_anteriores[0].estado, EstadoPr::Fechado);
        registrar_reabertura(&mut t, Pr { estado: EstadoPr::Aberto, ..saiu }, 3100);
        assert_eq!(t.estado, EstadoTarefa::NoPr);
        assert!(t.prs_anteriores.is_empty());
        assert!(!t.pr.as_ref().unwrap().armado(), "o fechamento desarmou o Ship");
    }

    #[test]
    fn contexto_do_vigia_pelo_pr_mais_quente() {
        let rodando = Pr { checks: checks(4, 0, 2), checks_progresso_em: 9000, empurrado_em: 8000, ..pr() };
        let parado = Pr { acompanhando: false, empurrado_em: 100, ..pr() };
        let c = contexto_do_vigia(&[&rodando, &parado], true, false, 2, 9100);
        assert!(c.progresso && c.algum_acompanhado && !c.esperando_pessoas);
        assert_eq!(c.desde_push, Some(Duration::from_secs(1100)));
        let so_parado = contexto_do_vigia(&[&parado], false, true, 0, 9100);
        assert!(!so_parado.algum_acompanhado && so_parado.aba_visivel && so_parado.esperando_pessoas);
    }
}
