//! O PR de uma tarefa no motor (seção 6): publicar, acompanhar pelo vigia, levar ao chat a falha do
//! CI e a revisão, mesclar (agora ou quando ficar pronto), o que vem depois de mesclar e a API que a
//! aba PR usa (seção 10.1). As regras ficam em `regras` (puras), o que bloqueia em `trabalho` (com o
//! `Executor`) e o laço do vigia em `laco`; aqui fica só a cola com o estado do motor.
//!
//! O vigia tem um laço só, criado em `Motor::new`: a cada volta, `volta_do_vigia` diz quando
//! acordar, e o fim de toda consulta manda o sinal. Sem PR para acompanhar, ele só espera o sinal.

mod laco;
pub mod regras;
mod trabalho;

use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

use gpui_kit::App;
use smol::channel::Receiver;

use super::conexoes::{Carregando, Guardado, Provedor, guardar, marcar_buscando, precisa_buscar, remoto_escolhido};
use super::*;
use crate::{
    git::Achado,
    github::{Check, ComentarioPr, DetalhePr, EstadoPr, Gh, InfoRepo, Mescla, TipoComentario},
    issues::{self as comum, Confianca, FonteIssue},
    linear::{Curl, Linear},
    modelo::{
        AcaoExterna, Confirmacao, EfeitoDoPr, EfeitosDaIssue, EsperaPr, EstadoCartao, GatilhoAjuste, ModoEntrega, Pr,
        PrAnterior,
    },
    remoto::{self, ErroRemoto, sem_segredos},
    ui::painel_pr::Ocupado,
};
pub use laco::esperar as esperar_vigia;
use regras::{Condicoes, Destino, EventoPr};
use trabalho::{Acompanhado, DepoisDaMescla, FalhaLida, FalhaPublicacao, Publicacao, Publicado};

/// De onde vem o diff que o diálogo mostra.
#[allow(dead_code)] // Provisório: o diálogo do diff no modo PR (G5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FonteDiff {
    /// A cópia da tarefa contra `base_da_tarefa` (sem rede).
    Tarefa(u64),
    /// O `gh pr diff`, quando a cópia já foi limpa.
    Pr { repositorio: String, numero: u64 },
}

/// O que a aba PR oferece; `acoes_do_pr` diz quais estão habilitadas e por quê não.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcaoPr {
    Mesclar { metodo: String },
    /// Liga o "Mesclar quando ficar pronto" (o Ship).
    MesclarQuandoPronto,
    AtualizarComBase,
    /// "Pedir ao chat" para resolver o conflito com a base.
    PedirAoChat,
    ProntoParaRevisao,
    VoltarParaRascunho,
    Fechar,
    Reabrir,
}

/// "Revisar e abrir PR" e "Atualizar PR".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PedidoPr {
    /// `None` no "Atualizar PR" sem "Atualizar a descrição".
    pub titulo: Option<String>,
    pub descricao: Option<String>,
    pub rascunho: bool,
    /// `Some(método)`: mesclar quando ficar pronto.
    pub quando_pronto: Option<String>,
    /// O usuário marcou "Confirmo que não são segredos".
    pub segredos_confirmados: bool,
}

/// O que o usuário confirmou antes de mesclar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirmado {
    /// Os commits de fora até este head.
    CommitsDeFora(String),
    /// Checks não obrigatórios falharam (`UNSTABLE`).
    Instavel,
}

#[allow(dead_code)] // Provisório: "Rodar falhas de novo" da sub-aba Checks (G5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RodarDeNovo {
    Falhas,
    Job(u64),
}

/// Uma linha da linha do tempo da aba PR, do mais recente para o mais antigo.
#[derive(Clone, Debug, PartialEq)]
pub enum EventoLinha {
    Commit { sha: String, titulo: String, autor: String, de_fora: bool, em: String },
    Revisao { id: u64, autor: String, estado: String, corpo: String, em: String, sem_acesso: bool },
    Comentario { id: u64, autor: String, corpo: String, em: String, sem_acesso: bool },
    ComentarioEmLinha { id: u64, autor: String, arquivo: String, linha: Option<u64>, trecho: String, corpo: String, em: String },
    /// Os bots informativos, numa linha só ("Bot cloudflare-workers-and-pages · 2 comentários").
    Bots { login: String, quantos: u32 },
}

/// O que o Vitral está fazendo no PR agora (a publicação inclui "Atualizar com main" e "Trazer as
/// mudanças do GitHub").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AndamentoPr {
    Publicando,
    Mesclando,
}

/// Quem pediu a ação no PR (a guarda e os textos mudam com isso).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrigemAcao {
    /// Um botão da interface (ou a confirmação de um cartão).
    Usuario,
    /// A ferramenta do próprio chat (`abrir_pr`), que a guarda deixou executar: o usuário pediu no
    /// turno. O chat está parado esperando a resposta, então o processo dele vivo não recusa.
    Ferramenta,
    /// O `aprovar_tarefa` do Mestre (a atualização de um PR que já existe).
    Mestre,
    /// O Ship: a rodada pelo PR publicou e mescla sozinha.
    Automatico,
}

/// Quem pediu o ajuste (`pedir_ajuste_de`): o limite de rodadas do projeto só vale para o dono.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrigemAjuste {
    Dono,
    Usuario,
    /// O aviso do PR (CI, revisão confiável) numa rodada automática.
    VigiaPr,
}

/// Uma leitura da aba PR que pediu busca.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PedidoPrLeitura {
    Detalhe(String, u64),
    Checks(String, u64),
    Linha(String, u64),
}

type ChavePr = (String, u64);

/// O estado do PR que fica só na memória.
pub struct EstadoPrs {
    vigia: laco::Vigia,
    publicando: HashSet<u64>,
    mesclando: HashSet<u64>,
    detalhes: HashMap<ChavePr, Guardado<DetalhePr>>,
    checks: HashMap<ChavePr, Guardado<Vec<Check>>>,
    linhas: HashMap<ChavePr, Guardado<Vec<EventoLinha>>>,
    /// Os comentários e revisões que já vieram de cada PR (o "Mandar ao chat" de um deles).
    comentarios: HashMap<ChavePr, Vec<ComentarioPr>>,
    /// A simulação da mescla da cópia com a base, por tarefa.
    simulacoes: HashMap<u64, (Instant, Result<Vec<String>, String>)>,
    /// As tarefas com a aba PR à vista.
    abas: HashSet<u64>,
    /// O create que estourou o tempo, por tarefa: o repositório, o ramo, o dono do head e o sha
    /// enviado, para o `pr_do_ramo` conferir antes de qualquer outra tentativa.
    criacoes_incertas: HashMap<u64, (String, String, String, String)>,
    /// Repositórios em que algum PR já teve check (o "sem CI" não vale neles).
    com_checks: HashSet<String>,
    /// Tarefas lendo a falha do CI ou os comentários agora (uma leitura por vez).
    lendo_falha: HashSet<u64>,
    lendo_comentarios: HashSet<u64>,
    pedidos: RefCell<Vec<PedidoPrLeitura>>,
}

impl EstadoPrs {
    pub fn novo(acordar: smol::channel::Sender<()>) -> Self {
        Self {
            vigia: laco::Vigia::novo(acordar),
            publicando: HashSet::new(),
            mesclando: HashSet::new(),
            detalhes: HashMap::new(),
            checks: HashMap::new(),
            linhas: HashMap::new(),
            comentarios: HashMap::new(),
            simulacoes: HashMap::new(),
            abas: HashSet::new(),
            criacoes_incertas: HashMap::new(),
            com_checks: HashSet::new(),
            lendo_falha: HashSet::new(),
            lendo_comentarios: HashSet::new(),
            pedidos: RefCell::new(Vec::new()),
        }
    }
}

/// Um canal que já tem a resposta.
fn respondido<T>(valor: T) -> Receiver<T> {
    let (envia, recebe) = smol::channel::bounded(1);
    let _ = envia.try_send(valor);
    recebe
}

/// O ramo do PR sai do GitHub depois de mesclar só quando foi o Vitral que mesclou, o repositório não
/// apaga sozinho e o head é do próprio repositório (não um fork para o original).
pub fn apagar_ramo_depois(anterior: &PrAnterior, pr: &Pr, info: Option<&InfoRepo>) -> bool {
    anterior.metodo.is_some() && info.is_some_and(|i| !i.apaga_ramo_ao_mesclar) && !pr.cabeca.contains(':') && !pr.ramo.is_empty()
}

/// "Refs ENG-123" ou "Refs #45" no fim da mensagem do commit.
fn mensagem_do_commit(titulo: &str, t: &Tarefa) -> String {
    match &t.issue {
        Some(i) => format!("{titulo}\n\nRefs {}", i.chave),
        None => titulo.to_string(),
    }
}

/// O corpo do PR sem gastar turno: a entrega, ou o objetivo, ou as mensagens do usuário desde a
/// última junção.
pub fn corpo_da_tarefa(t: &Tarefa, mensagens: &str) -> String {
    let resumo = t
        .entrega
        .as_ref()
        .map(|e| e.resumo.clone())
        .filter(|r| !r.trim().is_empty())
        .or_else(|| Some(t.objetivo.clone()).filter(|o| !o.trim().is_empty()))
        .unwrap_or_else(|| mensagens.to_string());
    let como = t.entrega.as_ref().map(|e| e.como_verificou.clone()).unwrap_or_default();
    let verificacao = t.ultima_verificacao.as_ref().map(|v| (v.comando.as_str(), v.passou));
    comum::corpo_do_pr(&comum::DadosDoCorpo { resumo: &resumo, como_verificou: &como, verificacao, issue: t.issue.as_ref() })
}

impl Motor {
    // ---------- onde e como ----------

    /// A pasta da tarefa entrega por PR.
    pub(super) fn entrega_por_pr(&self, t: &Tarefa) -> bool {
        t.tem_copia() && self.config_da_pasta(&t.projeto, &self.origem_da_tarefa(t)).github.entrega == ModoEntrega::Pr
    }

    /// O `<remoto>/<base>` com que a cópia de um chat do modo PR se compara (`base_da_tarefa`):
    /// o ponto de partida gravado, ou o da pasta. Fora do modo PR (e sem PR), `None`.
    pub(super) fn remoto_base(&self, t: &Tarefa) -> Option<String> {
        if !self.entrega_por_pr(t) && t.pr.is_none() && t.prs_anteriores.is_empty() {
            return None;
        }
        let origem = self.origem_da_tarefa(t);
        t.partir_de.clone().or_else(|| self.ponto_de_partida(&origem, &self.config_da_pasta(&t.projeto, &origem).github))
    }

    /// O remote e a base da pasta da tarefa (o PR aberto manda na base).
    fn remoto_e_base(&self, t: &Tarefa) -> Option<(String, String)> {
        if let Some(ponto) = t.partir_de.clone().or_else(|| self.remoto_base(t))
            && let Some((remoto, base)) = ponto.split_once('/')
        {
            let base = t.pr.as_ref().map(|p| p.base.clone()).filter(|b| !b.is_empty()).unwrap_or_else(|| base.to_string());
            return Some((remoto.to_string(), base));
        }
        None
    }

    /// O que falta para o chat estar livre para o PR (publicar, mesclar): o motivo, ou nada. Pela
    /// ferramenta do próprio chat, o processo dele está vivo (parado na ferramenta) e não conta.
    fn ocupado_para_o_pr(&self, t: &Tarefa, origem: OrigemAcao) -> Option<String> {
        if self.prs.publicando.contains(&t.id) {
            return Some("publicando".into());
        }
        if self.prs.mesclando.contains(&t.id) {
            return Some("mesclando".into());
        }
        let rodando = self.processos.contains_key(&t.trabalhador) || self.na_fila(&t.trabalhador) > 0;
        if (rodando && origem != OrigemAcao::Ferramenta) || self.juntando.contains(&t.id) {
            return Some("Esperando o chat terminar".into());
        }
        None
    }

    /// O "sem CI" de um repositório: nenhum workflow e nenhum check visto (neste PR ou antes).
    fn sem_ci(&self, pr: &Pr) -> bool {
        let workflows = self.repo_github(&pr.repositorio).tirar().and_then(|r| r.workflows);
        workflows == Some(0) && !self.prs.com_checks.contains(&pr.repositorio) && pr.checks.total == 0 && pr.checks_progresso_em == 0
    }

    fn condicoes(&self, t: &Tarefa, pr: &Pr, automatico: bool, instavel: bool) -> Condicoes {
        let conflito_local = self.prs.simulacoes.get(&t.id).is_some_and(|(_, s)| s.as_ref().is_ok_and(|a| !a.is_empty()));
        Condicoes { automatico, instavel_confirmado: instavel, sem_ci: self.sem_ci(pr), conflito_local }
    }

    // ---------- o vigia ----------

    /// Uma volta do laço do vigia: começa a consulta ou a sonda da vez e diz quando acordar de novo.
    pub(super) fn volta_do_vigia(&mut self, cx: &mut Context<Self>) -> Option<Instant> {
        if self.somente_leitura.is_some() || self.encerrado.load(Ordering::SeqCst) || self.sem_gh() {
            return None;
        }
        let agora = Instant::now();
        let repos = self.repos_do_vigia();
        let rede = self.estado_rede(Provedor::Github);
        let perdidas = self.prs.vigia.perdidas;
        let passo = self.prs.vigia.passo(&rede, &repos, agora);
        if self.prs.vigia.perdidas > perdidas {
            self.anotar_rede(Provedor::Github, Some(&ErroRemoto::TempoEsgotado));
        }
        match passo {
            laco::Passo::Consultar(repo) => {
                self.consultar_repo(repo, cx);
                Some(agora + self.prs.vigia.seguranca)
            }
            laco::Passo::Sondar => {
                self.sondar_github(cx);
                Some(agora + self.prs.vigia.seguranca)
            }
            laco::Passo::Esperar(quando) => quando,
        }
    }

    /// Os repositórios com PR aberto (ou com um create a conferir) e o contexto do PR mais quente de
    /// cada um.
    fn repos_do_vigia(&self) -> Vec<(String, crate::vigia::Contexto)> {
        let agora_s = agora();
        let mut por_repo: HashMap<String, (Vec<&Pr>, bool)> = HashMap::new();
        for t in &self.tarefas {
            if let Some(pr) = t.pr_aberto() {
                let entrada = por_repo.entry(pr.repositorio.clone()).or_default();
                entrada.0.push(pr);
                entrada.1 |= self.prs.abas.contains(&t.id);
            }
        }
        let mut repos: Vec<(String, crate::vigia::Contexto)> = por_repo
            .into_iter()
            .map(|(repo, (prs, aba))| {
                let c = regras::contexto_do_vigia(&prs, self.janela_ativa, aba, 0, agora_s);
                (repo, c)
            })
            .collect();
        for (repo, ..) in self.prs.criacoes_incertas.values() {
            if !repos.iter().any(|(r, _)| r == repo) {
                let c = crate::vigia::Contexto { algum_acompanhado: true, janela_ativa: self.janela_ativa, desde_push: Some(Duration::ZERO), ..Default::default() };
                repos.push((repo.clone(), c));
            }
        }
        repos.sort_by(|a, b| a.0.cmp(&b.0));
        repos
    }

    /// Consulta na hora todos os repositórios com PR (ao abrir e ao voltar o foco): também os sem
    /// acompanhar, e o último PR fechado de cada chat (para perceber quem reabriu).
    pub(super) fn vigiar_na_hora(&mut self) {
        let mut repos: Vec<String> = self.tarefas.iter().filter_map(|t| t.pr_aberto().map(|p| p.repositorio.clone())).collect();
        repos.extend(self.tarefas.iter().filter(|t| t.pr.is_none()).filter_map(|t| {
            t.prs_anteriores.last().filter(|a| a.estado == EstadoPr::Fechado && !a.repositorio.is_empty()).map(|a| a.repositorio.clone())
        }));
        repos.extend(self.prs.criacoes_incertas.values().map(|(r, ..)| r.clone()));
        repos.sort();
        repos.dedup();
        for repo in repos {
            self.prs.vigia.na_hora(&repo);
        }
    }

    /// Os PRs do repositório numa consulta, em segundo plano, com a guarda do laço.
    fn consultar_repo(&mut self, repo: String, cx: &mut Context<Self>) {
        let mut prs: Vec<Acompanhado> = self
            .tarefas
            .iter()
            .filter_map(|t| {
                let pr = t.pr_aberto().filter(|p| p.repositorio == repo)?;
                Some(Acompanhado { tarefa: t.id, pr: pr.clone(), copia: t.worktree.clone() })
            })
            .collect();
        // O último PR fechado de um chat sem PR: só para ver se alguém o reabriu.
        prs.extend(self.tarefas.iter().filter(|t| t.pr.is_none() && t.tem_copia()).filter_map(|t| {
            let a = t.prs_anteriores.last().filter(|a| a.estado == EstadoPr::Fechado && a.repositorio == repo)?;
            let pr = Pr {
                repositorio: a.repositorio.clone(),
                numero: a.numero,
                url: a.url.clone(),
                titulo: a.titulo.clone(),
                estado: EstadoPr::Fechado,
                sha: a.sha.clone(),
                sha_publicado: a.sha.clone(),
                ..Default::default()
            };
            Some(Acompanhado { tarefa: t.id, pr, copia: t.worktree.clone() })
        }));
        let incertas: Vec<(u64, String, String, String)> = self
            .prs
            .criacoes_incertas
            .iter()
            .filter(|(_, (r, ..))| *r == repo)
            .map(|(t, (_, ramo, dono, sha))| (*t, ramo.clone(), dono.clone(), sha.clone()))
            .collect();
        let agora_i = Instant::now();
        if prs.is_empty() && incertas.is_empty() {
            self.prs.vigia.terminou(&repo, false, false, agora_i);
            return;
        }
        let guarda = self.prs.vigia.comecou(agora_i);
        let fila = self.fila_do_provedor(Provedor::Github, false);
        let e = self.executor();
        let pasta = self.dados.pasta.clone();
        let lido = repo.clone();
        cx.spawn(async move |motor, cx| {
            let vez = fila.acquire_arc().await;
            let resultado = cx
                .background_spawn(async move {
                    let _vez = vez;
                    let conferidos = trabalho::conferir_prs(&*e, &pasta, &lido, &prs, agora());
                    let incertas: Vec<_> = incertas
                        .into_iter()
                        .map(|(t, ramo, dono, sha)| (t, trabalho::conferir_criacao(&*e, &pasta, &lido, &ramo, &dono, &sha)))
                        .collect();
                    (conferidos, incertas)
                })
                .await;
            let _ = motor.update(cx, |m, cx| {
                m.resumo_chegou(&repo, resultado, cx);
                cx.notify();
            });
            drop(guarda);
        })
        .detach();
    }

    /// A sonda da rede do GitHub (`gh api rate_limit`, 10 s), com a guarda do laço.
    fn sondar_github(&mut self, cx: &mut Context<Self>) {
        let guarda = self.prs.vigia.comecou(Instant::now());
        let e = self.executor();
        let pasta = self.dados.pasta.clone();
        cx.spawn(async move |motor, cx| {
            let r = cx.background_spawn(async move { Gh { e: &*e, pasta: &pasta }.limite() }).await;
            let _ = motor.update(cx, |m, cx| {
                match r {
                    Ok(_) => {
                        m.anotar_rede(Provedor::Github, None);
                        m.vigiar_na_hora();
                        // O login que caiu junto volta a ser conferido.
                        m.login_github(cx);
                    }
                    Err(_) => m.integracao_sonda_falhou(Provedor::Github),
                }
                cx.notify();
            });
            drop(guarda);
        })
        .detach();
    }

    #[allow(clippy::type_complexity)]
    fn resumo_chegou(
        &mut self,
        repo: &str,
        (conferidos, incertas): (
            Result<(Vec<trabalho::Conferido>, crate::github::ResumoDoRepo), ErroRemoto>,
            Vec<(u64, Result<Option<crate::github::PrAchado>, ErroRemoto>)>,
        ),
        cx: &mut Context<Self>,
    ) {
        let agora_i = Instant::now();
        match conferidos {
            Err(erro) => {
                self.anotar_rede(Provedor::Github, Some(&erro));
                self.prs.vigia.terminou(repo, false, erro.transitorio(), agora_i);
            }
            Ok((lista, _)) => {
                self.anotar_rede(Provedor::Github, None);
                let mut mudou = false;
                for (tarefa, r) in lista {
                    match r {
                        Ok((novo, eventos)) => {
                            mudou |= !eventos.is_empty();
                            self.aplicar_pr(tarefa, novo, eventos, cx);
                        }
                        Err(erro) => {
                            let texto = match &erro {
                                ErroRemoto::NaoEncontrado(_) => {
                                    let n = self.pr_da_tarefa(tarefa).map_or(0, |p| p.numero);
                                    format!("Não encontrei o PR #{n} em {repo}")
                                }
                                outro => outro.para_usuario("GitHub"),
                            };
                            if let Some(pr) = self.tarefa_mut(tarefa).and_then(|t| t.pr.as_mut()) {
                                pr.erro = Some(sem_segredos(&texto));
                            }
                        }
                    }
                }
                self.prs.vigia.terminou(repo, mudou, false, agora_i);
            }
        }
        for (tarefa, r) in incertas {
            match r {
                Ok(Some(achado)) => {
                    let Some((repo, ramo, dono, sha)) = self.prs.criacoes_incertas.remove(&tarefa) else {
                        continue;
                    };
                    let publicado = Publicado {
                        numero: achado.numero,
                        url: achado.url,
                        titulo: achado.titulo,
                        sha: sha.clone(),
                        criado: true,
                        rascunho: achado.rascunho,
                        aviso: None,
                    };
                    let plano = Publicacao { repositorio: repo, ramo: ramo.clone(), cabeca: ramo, dono_do_head: dono, ..Default::default() };
                    self.publicado(tarefa, &plano, publicado, None, cx);
                }
                Ok(None) => {
                    self.prs.criacoes_incertas.remove(&tarefa);
                    if let Some(chat) = self.tarefa(tarefa).map(|t| t.trabalhador.clone()) {
                        self.avisar(&chat, "O GitHub não criou o PR (o pedido tinha estourado o tempo). As mudanças já estão no ramo: publique de novo.", true);
                    }
                }
                Err(erro) => self.anotar_rede(Provedor::Github, Some(&erro)),
            }
        }
        self.salvar();
    }

    /// O PR novo de uma consulta entra na tarefa, e cada mudança vira o que precisa (aviso, rodada,
    /// mescla, o depois de mesclar).
    fn aplicar_pr(&mut self, id: u64, novo: Pr, mut eventos: Vec<EventoPr>, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        // O último fechado de um chat sem PR: só vale se alguém o reabriu.
        if t.pr.is_none() {
            if eventos.contains(&EventoPr::Reaberto)
                && let Some(t) = self.tarefa_mut(id)
            {
                regras::registrar_reabertura(t, novo, agora());
                let (chat, n) = (t.trabalhador.clone(), t.pr.as_ref().map_or(0, |p| p.numero));
                self.avisar(&chat, &format!("O PR #{n} foi reaberto; o Vitral volta a acompanhar."), false);
                self.cartao_do_pr(id, cx);
            }
            return;
        }
        let antes = t.pr.clone().unwrap_or_default();
        if antes.numero != novo.numero {
            return;
        }
        if novo.checks.total > 0 {
            self.prs.com_checks.insert(novo.repositorio.clone());
        }
        let condicoes = self.condicoes(&t, &novo, novo.armado(), false);
        if regras::virou_pronto(&antes, &novo, agora(), &condicoes) {
            eventos.push(EventoPr::ProntoParaMesclar);
        }
        if let Some(t) = self.tarefa_mut(id) {
            t.pr = Some(novo.clone());
        }
        let chave = (novo.repositorio.clone(), novo.numero);
        if eventos.contains(&EventoPr::ChecksMudaram) {
            self.prs.checks.remove(&chave);
        }
        let chat = t.trabalhador.clone();
        let n = novo.numero;
        for evento in eventos {
            match evento {
                EventoPr::ChecksFalharam if novo.acompanhando => self.falha_do_ci(id, cx),
                EventoPr::ChecksPassaram => {
                    self.registrar("checks_passaram", "github", &chat, &t.projeto, Some(id), &format!("checks do PR #{n} passaram"));
                    self.tirar_espera(id, |e| matches!(e, EsperaPr::CiFalhou { .. } | EsperaPr::ChecksParados));
                }
                EventoPr::Novidade | EventoPr::MudancasPedidas => {
                    self.prs.linhas.remove(&chave);
                    let desde = (!antes.atualizado.is_empty()).then(|| regras::iso_menos(&antes.atualizado, 60)).flatten();
                    self.ler_novidades(id, desde, cx);
                }
                EventoPr::ProntoParaMesclar => self.pronto_para_mesclar(id, cx),
                EventoPr::CommitDeFora(sha) => self.commit_de_fora(id, &sha, cx),
                EventoPr::FilaSumiu => {
                    self.desarmar(id, cx);
                    self.avisar(&chat, &format!("O PR #{n} saiu da fila de merge sem ser mesclado; o Vitral não mescla sozinho."), true);
                }
                EventoPr::Mesclado { .. } => self.depois_de_mesclar(id, cx),
                EventoPr::Fechado => self.pr_fechado(id, cx),
                EventoPr::ChecksParados if novo.acompanhando => {
                    self.esperar_no_pr(id, EsperaPr::ChecksParados);
                    self.avisar(&chat, &format!("Checks parados há 2 h no PR #{n}."), false);
                }
                EventoPr::ChecksParadosUmDia if novo.armado() => {
                    self.desarmar(id, cx);
                    self.avisar(&chat, &format!("Checks parados há um dia no PR #{n}: o Vitral não mescla mais sozinho. Confira e mescle você."), true);
                }
                EventoPr::Mesclavel(estado) if estado == "BEHIND" && novo.armado() => {
                    self.avisar(&chat, &format!("O PR #{n} está atrás de {}; atualize para mesclar (Atualizar com {}).", novo.base, novo.base), false);
                }
                _ => {}
            }
        }
        cx.notify();
    }

    /// O item de "Esperando você" do PR, sem repetir o mesmo tipo (o novo substitui).
    fn esperar_no_pr(&mut self, id: u64, item: EsperaPr) {
        if let Some(pr) = self.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
            let tipo = std::mem::discriminant(&item);
            let mesmo_terceiro = |e: &EsperaPr| matches!((e, &item), (EsperaPr::Terceiro { id: a, .. }, EsperaPr::Terceiro { id: b, .. }) if a == b);
            pr.esperando.retain(|e| if matches!(item, EsperaPr::Terceiro { .. }) { !mesmo_terceiro(e) } else { std::mem::discriminant(e) != tipo });
            pr.esperando.push(item);
        }
    }

    fn tirar_espera(&mut self, id: u64, f: impl Fn(&EsperaPr) -> bool) {
        if let Some(pr) = self.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
            pr.esperando.retain(|e| !f(e));
        }
    }

    // ---------- CI e revisão ----------

    /// O CI falhou: lê os checks e os logs; a chave já avisada não avisa de novo.
    fn falha_do_ci(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(pr) = self.pr_da_tarefa(id).cloned() else {
            return;
        };
        if !self.prs.lendo_falha.insert(id) {
            return;
        }
        let sem_log = pr.tentativas_log >= 3;
        let ship = pr.armado();
        let pasta = self.dados.pasta.clone();
        self.em_segundo_plano(
            Provedor::Github,
            false,
            move |e| trabalho::ler_falha(e, &pasta, &pr, None, sem_log, ship),
            move |m, r, cx| m.falha_lida(id, r, cx),
            cx,
        );
    }

    fn falha_lida(&mut self, id: u64, r: Result<FalhaLida, ErroRemoto>, cx: &mut Context<Self>) {
        self.prs.lendo_falha.remove(&id);
        let lida = match r {
            Ok(lida) => {
                self.anotar_rede(Provedor::Github, None);
                lida
            }
            Err(erro) => {
                self.anotar_rede(Provedor::Github, Some(&erro));
                return;
            }
        };
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let Some(pr) = t.pr_aberto().cloned() else {
            return;
        };
        if regras::falha_ja_avisada(&pr, &lida.chave) {
            return;
        }
        let Some(texto) = lida.texto else {
            // A execução ainda roda: tenta na próxima consulta (até 3 vezes; depois, sem o log).
            if let Some(pr) = self.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
                pr.tentativas_log = pr.tentativas_log.saturating_add(1);
            }
            return;
        };
        let nomes: Vec<String> = lida.falhos.iter().map(|c| c.nome.clone()).collect();
        if let Some(pr) = self.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
            pr.falha_avisada = Some(lida.chave.clone());
            pr.tentativas_log = 0;
        }
        let chat = t.trabalhador.clone();
        self.registrar("checks_falharam", "github", &chat, &t.projeto, Some(id), &format!("CI do PR #{} falhou: {}", pr.numero, nomes.join(", ")));
        if !self.janela_ativa
            && let Some(c) = self.chat(&chat).cloned()
        {
            self.notificar(&c, &format!("CI falhou no PR #{}", pr.numero));
        }
        match regras::destino_do_aviso(&t, Confianca::Confiavel) {
            Destino::Dono(dono) => {
                self.somar_rodada_do_pr(id);
                self.aviso_do_pr_ao_dono(&dono, &t, &texto, cx);
            }
            Destino::Automatico => {
                self.somar_rodada_do_pr(id);
                let _ = self.pedir_ajuste_de(id, texto, "CI do GitHub", "", "", OrigemAjuste::VigiaPr, Some(GatilhoAjuste::Ci), cx);
            }
            Destino::EsperandoVoce => self.esperar_no_pr(id, EsperaPr::CiFalhou { chave: lida.chave, falhos: nomes }),
        }
        self.salvar();
    }

    fn somar_rodada_do_pr(&mut self, id: u64) {
        if let Some(pr) = self.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
            pr.rodadas_automaticas = pr.rodadas_automaticas.saturating_add(1);
        }
    }

    /// O aviso do PR (texto de terceiros) ao dono do pedido: o Mestre decide com `pedir_ajuste`.
    fn aviso_do_pr_ao_dono(&mut self, dono: &str, t: &Tarefa, texto: &str, cx: &mut Context<Self>) {
        let titulo = self.chat(&t.trabalhador).map_or_else(|| t.titulo.clone(), |c| c.titulo.clone());
        let texto = format!(
            "Chat {} \"{titulo}\" ({}): {texto}\n\nDecida com pedir_ajuste {} (mande o que importa) ou espere o próximo aviso, sem laço de consulta.",
            t.id,
            self.nome_do_projeto(&t.projeto),
            t.id
        );
        let entrada = Entrada { de: Some("Vitral".into()), texto: sem_segredos(&texto), terceiros: true, ..Default::default() };
        self.entregar_entrada(dono, entrada, cx);
    }

    /// Novidade no PR: os comentários desde a última vez (o `since` é o `atualizado` de antes menos
    /// 60 s; os repetidos saem pelo id).
    fn ler_novidades(&mut self, id: u64, desde: Option<String>, cx: &mut Context<Self>) {
        let Some(pr) = self.pr_da_tarefa(id).cloned() else {
            return;
        };
        if !self.prs.lendo_comentarios.insert(id) {
            return;
        }
        let pasta = self.dados.pasta.clone();
        self.em_segundo_plano(
            Provedor::Github,
            false,
            move |e| trabalho::ler_comentarios(e, &pasta, &pr, desde.as_deref()),
            move |m, r, cx| m.comentarios_chegaram(id, r, cx),
            cx,
        );
    }

    fn guardar_comentarios(&mut self, chave: ChavePr, lista: &[ComentarioPr]) {
        let guardados = self.prs.comentarios.entry(chave).or_default();
        for c in lista {
            match guardados.iter_mut().find(|g| g.id == c.id) {
                Some(g) => *g = c.clone(),
                None => guardados.push(c.clone()),
            }
        }
    }

    fn comentarios_chegaram(&mut self, id: u64, r: Result<Vec<ComentarioPr>, ErroRemoto>, cx: &mut Context<Self>) {
        self.prs.lendo_comentarios.remove(&id);
        let lista = match r {
            Ok(lista) => lista,
            Err(erro) => {
                self.anotar_rede(Provedor::Github, Some(&erro));
                return;
            }
        };
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let Some(pr) = t.pr_aberto().cloned() else {
            return;
        };
        self.guardar_comentarios((pr.repositorio.clone(), pr.numero), &lista);
        let s = regras::separar_comentarios(&pr, lista);
        if let Some(pr) = self.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
            regras::marcar_entregues(pr, &s.vistos);
        }
        if !pr.acompanhando {
            self.salvar();
            return;
        }
        for c in &s.terceiros {
            self.esperar_no_pr(id, EsperaPr::Terceiro { id: c.id, autor: c.autor.clone(), url: c.url.clone() });
        }
        if s.confiaveis.is_empty() && s.mudancas_pedidas_por.is_empty() {
            self.salvar();
            return;
        }
        let chat = t.trabalhador.clone();
        let autores: Vec<String> = {
            let mut a: Vec<String> = s.confiaveis.iter().map(|c| c.autor.clone()).chain(s.mudancas_pedidas_por.iter().cloned()).collect();
            a.dedup();
            a
        };
        self.registrar("revisao_pr", "github", &chat, &t.projeto, Some(id), &format!("revisão no PR #{} de {}", pr.numero, autores.join(", ")));
        if !s.mudancas_pedidas_por.is_empty()
            && !self.janela_ativa
            && let Some(c) = self.chat(&chat).cloned()
        {
            self.notificar(&c, &format!("Mudanças pedidas no PR #{}", pr.numero));
        }
        let texto = sem_segredos(&prompts::revisao_do_pr(pr.numero, &s.confiaveis, &s.mudancas_pedidas_por));
        match regras::destino_do_aviso(&t, Confianca::Confiavel) {
            Destino::Dono(dono) => {
                self.somar_rodada_do_pr(id);
                self.aviso_do_pr_ao_dono(&dono, &t, &texto, cx);
            }
            Destino::Automatico => {
                self.somar_rodada_do_pr(id);
                let _ = self.pedir_ajuste_de(id, texto, "revisão do PR", "", "", OrigemAjuste::VigiaPr, Some(GatilhoAjuste::Revisao), cx);
            }
            Destino::EsperandoVoce => {
                let ids = s.confiaveis.iter().map(|c| c.id).collect();
                self.esperar_no_pr(id, EsperaPr::Revisao { ids, autores });
            }
        }
        self.salvar();
    }

    /// Chegou commit de fora: desarma o Ship (e a mesclagem automática do GitHub) e avisa.
    fn commit_de_fora(&mut self, id: u64, sha: &str, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let Some(pr) = t.pr.clone() else {
            return;
        };
        let curto: String = sha.chars().take(7).collect();
        let chat = t.trabalhador.clone();
        self.registrar("commit_de_fora", "github", &chat, &t.projeto, Some(id), &format!("commit de fora no PR #{}: {curto}", pr.numero));
        if pr.armado() {
            self.desarmar(id, cx);
            self.avisar(
                &chat,
                &format!("Chegou commit de fora do Vitral no PR #{} ({curto}); o Vitral não mescla sozinho. Revise e mescle você.", pr.numero),
                true,
            );
        }
        if pr.acompanhando {
            self.esperar_no_pr(id, EsperaPr::CommitDeFora { shas: pr.commits_de_fora.clone() });
            if !self.janela_ativa
                && let Some(c) = self.chat(&chat).cloned()
            {
                self.notificar(&c, &format!("Chegou commit de fora no PR #{}", pr.numero));
            }
        }
    }

    /// Ficou pronto: com o Ship, mescla sozinho; no Watch, o aviso; ao Mestre dono, a mensagem.
    fn pronto_para_mesclar(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let Some(pr) = t.pr_aberto().cloned() else {
            return;
        };
        if pr.armado() {
            let metodo = pr.ao_ficar_pronto.clone();
            let _ = self.mesclar_pr(id, metodo, false, OrigemAcao::Automatico, Vec::new(), cx);
            return;
        }
        if !pr.acompanhando {
            return;
        }
        if t.dono() == Some(crate::modelo::ID_MESTRE) {
            let texto = prompts::pronto_para_mesclar_ao_mestre(pr.numero, id, &pr.checks);
            self.entregar(crate::modelo::ID_MESTRE, Some("Vitral".into()), texto, cx);
        }
        self.esperar_no_pr(id, EsperaPr::ProntoParaMesclar);
        if !self.janela_ativa
            && let Some(c) = self.chat(&t.trabalhador).cloned()
        {
            self.notificar(&c, &format!("PR #{} pronto para mesclar", pr.numero));
        }
    }

    /// Desliga o Ship; com a mesclagem automática do GitHub ligada, desliga ela também.
    fn desarmar(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(pr) = self.tarefa_mut(id).and_then(|t| t.pr.as_mut()) else {
            return;
        };
        pr.ao_ficar_pronto = None;
        if pr.auto_github && pr.aberto() {
            let (repo, n) = (pr.repositorio.clone(), pr.numero);
            let pasta = self.dados.pasta.clone();
            self.em_segundo_plano(
                Provedor::Github,
                false,
                move |e| Gh { e, pasta: &pasta }.desligar_auto(&repo, n),
                move |m, r, _| {
                    m.anotar_rede(Provedor::Github, r.as_ref().err());
                    if let Err(erro) = r
                        && let Some(pr) = m.tarefa_mut(id).and_then(|t| t.pr.as_mut())
                    {
                        pr.erro = Some(format!("Não consegui desligar a mesclagem automática do GitHub ({}).", erro.para_usuario("GitHub")));
                    }
                    m.salvar();
                },
                cx,
            );
            if let Some(repo) = self.pr_da_tarefa(id).map(|p| p.repositorio.clone()) {
                self.prs.vigia.depois_do_envio(&repo, Instant::now());
            }
        }
        self.salvar();
    }

    // ---------- publicar (6.2) ----------

    /// Publica o PR da tarefa: o commit, o push e o PR aberto, atualizado ou recuperado.
    pub(super) fn publicar_pr(&mut self, id: u64, pedido: PedidoPr, origem: OrigemAcao, cx: &mut Context<Self>) -> Receiver<Resposta> {
        match self.plano_de_publicacao(id, &pedido, origem) {
            Err(motivo) => respondido(Resposta::erro(motivo)),
            Ok(plano) => {
                let (envia, recebe) = smol::channel::bounded(1);
                self.prs.publicando.insert(id);
                if let Some(t) = self.tarefa_mut(id) {
                    t.progresso = Some("publicando o PR".into());
                }
                cx.notify();
                let e = self.executor();
                let fila = self.fila_do_provedor(Provedor::Github, matches!(origem, OrigemAcao::Usuario | OrigemAcao::Ferramenta));
                let quando_pronto = pedido.quando_pronto.clone();
                cx.spawn(async move |motor, cx| {
                    let plano_bg = plano.clone();
                    let r = cx.background_spawn(async move { trabalho::publicar(&*e, &plano_bg, Some(fila)).await }).await;
                    let _ = motor.update(cx, |m, cx| {
                        let resposta = m.publicacao_chegou(id, &plano, r, quando_pronto, origem, cx);
                        let _ = envia.try_send(resposta);
                        m.salvar();
                        cx.notify();
                    });
                })
                .detach();
                recebe
            }
        }
    }

    /// As guardas e o que publicar.
    fn plano_de_publicacao(&self, id: u64, pedido: &PedidoPr, origem: OrigemAcao) -> Result<Publicacao, String> {
        if self.somente_leitura.is_some() {
            return Err("o Vitral está aberto só para leitura".into());
        }
        let t = self.tarefa(id).ok_or("Tarefa não encontrada.")?;
        if !self.de_projeto(&t.trabalhador) || !t.tem_copia() {
            return Err("Só um chat de projeto com cópia abre PR.".into());
        }
        if let Some(motivo) = self.ocupado_para_o_pr(t, origem) {
            return Err(if motivo == "publicando" { "O PR já está sendo publicado.".into() } else { motivo });
        }
        if self.prs.criacoes_incertas.contains_key(&id) {
            return Err("Resultado incerto: o Vitral está conferindo no GitHub se o PR foi criado.".into());
        }
        let origem_pasta = self.origem_da_tarefa(t);
        if !origem_pasta.is_dir() {
            return Err(format!("A pasta {} não foi encontrada: use Localizar pasta…", origem_pasta.display()));
        }
        let (Some(copia), true) = (t.worktree.clone(), t.preparada) else {
            return Err("A cópia deste chat ainda não está pronta.".into());
        };
        let config = self.config_da_pasta(&t.projeto, &origem_pasta).github;
        if config.entrega != ModoEntrega::Pr && t.pr.is_none() && origem != OrigemAcao::Mestre {
            return Err("A entrega desta pasta é Juntar aqui.".into());
        }
        if !self.github_conectado() {
            return Err("Entre no GitHub em Configurações › Conexões.".into());
        }
        let remotos = self.remotos_github(&origem_pasta).tirar().unwrap_or_default();
        let remoto = remoto_escolhido(&remotos, &config).ok_or("Esta pasta não tem remote no GitHub. A entrega é Juntar aqui.")?;
        let destino = self.destino_do_pr(&origem_pasta, &config).ok_or("Não sei para que repositório vai o PR.")?;
        let info = self.repo_github(&destino).tirar().map(|r| r.info);
        let do_fork = config.pr_no_original && destino != remoto.repositorio;
        if let Some(info) = &info
            && !info.pode_escrever()
            && !do_fork
        {
            return Err(format!("Sua conta só lê {destino}."));
        }
        let pr = t.pr_aberto();
        let base = pr
            .map(|p| p.base.clone())
            .filter(|b| !b.is_empty())
            .or_else(|| Some(config.base.trim().to_string()).filter(|b| !b.is_empty()))
            .or_else(|| info.as_ref().map(|i| i.ramo_padrao.clone()).filter(|b| !b.is_empty()))
            .or_else(|| t.partir_de.as_deref().and_then(|p| p.split_once('/')).map(|(_, b)| b.to_string()))
            .or_else(|| git::ramo_padrao_do_remoto(&origem_pasta, &remoto.nome))
            .ok_or("Não sei qual é o ramo de destino do PR (escolha em Mais opções).")?;
        let ramo = t.branch.clone().ok_or("A cópia não tem ramo.")?;
        let dono_remoto = remoto.repositorio.split_once('/').map_or("", |(d, _)| d).to_string();
        let (cabeca, dono_do_head) = if do_fork { (format!("{dono_remoto}:{ramo}"), dono_remoto) } else {
            (ramo.clone(), destino.split_once('/').map_or("", |(d, _)| d).to_string())
        };
        let titulo_chat = self.chat(&t.trabalhador).map_or_else(|| t.titulo.clone(), |c| c.titulo.clone());
        let titulo = pedido
            .titulo
            .clone()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .or_else(|| pr.map(|p| p.titulo.clone()))
            .unwrap_or_else(|| comum::titulo_do_pr(&titulo_chat, t.issue.as_ref()));
        let mensagens = self.chat(&t.trabalhador).map(|c| super::mensagens_desde_a_juncao(&c.itens)).unwrap_or_default();
        let corpo = match (&pedido.descricao, pr) {
            (Some(d), _) => Some(d.clone()),
            (None, Some(_)) => None,
            (None, None) => Some(corpo_da_tarefa(t, &mensagens)),
        };
        Ok(Publicacao {
            pasta_dados: self.dados.pasta.clone(),
            copia,
            origem: origem_pasta,
            remoto: remoto.nome.clone(),
            base,
            repositorio: destino,
            ramo,
            cabeca,
            dono_do_head,
            mensagem: mensagem_do_commit(&titulo, t),
            excluir: t.arquivos_copiados.clone(),
            segredos_confirmados: pedido.segredos_confirmados,
            // Sem o usuário olhando o diff (a ferramenta, o Mestre, o Ship): segredo e .github/ param.
            automatico: origem != OrigemAcao::Usuario,
            sha_publicado: pr.map(|p| p.sha_publicado.clone()).filter(|s| !s.is_empty()),
            pr_aberto: pr.map(|p| p.numero),
            url_aberto: pr.map(|p| p.url.clone()).unwrap_or_default(),
            novo_titulo: pr.is_some_and(|p| pedido.titulo.as_ref().is_some_and(|n| n.trim() != p.titulo)),
            titulo,
            corpo,
            rascunho: pedido.rascunho || (pr.is_none() && config.rascunho),
        })
    }

    fn publicacao_chegou(
        &mut self,
        id: u64,
        plano: &Publicacao,
        r: Result<Publicado, FalhaPublicacao>,
        quando_pronto: Option<String>,
        origem: OrigemAcao,
        cx: &mut Context<Self>,
    ) -> Resposta {
        self.prs.publicando.remove(&id);
        if let Some(t) = self.tarefa_mut(id) {
            t.progresso = None;
        }
        let Some(t) = self.tarefa(id).cloned() else {
            return Resposta::erro("Tarefa não encontrada.");
        };
        let chat = t.trabalhador.clone();
        match r {
            Ok(publicado) => {
                self.anotar_rede(Provedor::Github, None);
                self.publicado(id, plano, publicado, quando_pronto, cx)
            }
            Err(falha) => {
                if let FalhaPublicacao::Remoto(erro) = &falha {
                    self.anotar_rede(Provedor::Github, Some(erro));
                }
                let texto = falha.texto();
                match &falha {
                    FalhaPublicacao::Incerto { sha } => {
                        self.prs.criacoes_incertas.insert(id, (plano.repositorio.clone(), plano.ramo.clone(), plano.dono_do_head.clone(), sha.clone()));
                        self.prs.vigia.depois_do_envio(&plano.repositorio, Instant::now());
                        self.avisar(&chat, &texto, false);
                    }
                    FalhaPublicacao::Conflito { alvo, arquivos } => {
                        self.registrar("conflito", "vitral", &chat, &t.projeto, Some(id), &format!("conflito com {alvo}: {}", arquivos.join(", ")));
                        match t.dono() {
                            Some(dono) => {
                                let aviso = format!(
                                    "O PR do chat {id} não foi publicado: deu conflito com {alvo} (arquivos {}). Peça ao chat (pedir_ajuste {id}) para rodar `git merge {alvo}` na cópia dele, resolver mantendo as duas mudanças, fazer commit e rodar os testes; depois aprove de novo.",
                                    arquivos.join(", ")
                                );
                                self.entregar(dono, Some("Vitral".into()), aviso, cx);
                            }
                            None => self.entregar(&chat, Some("Vitral".into()), prompts::conflito_com_a_base(alvo, arquivos), cx),
                        }
                    }
                    FalhaPublicacao::Rejeitado(_) => {
                        if let Some(pr) = self.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
                            pr.erro = Some("O ramo do PR mudou no GitHub (commits de fora)".into());
                        }
                    }
                    _ => {
                        if let Some(pr) = self.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
                            pr.erro = Some(remoto::sem_segredos(&texto));
                        }
                    }
                }
                // A rodada pelo PR que não publicou sozinha espera o usuário.
                if matches!(origem, OrigemAcao::Automatico | OrigemAcao::Mestre)
                    && t.pr_aberto().is_some()
                    && !matches!(falha, FalhaPublicacao::Incerto { .. })
                {
                    if let Some(t) = self.tarefa_mut(id) {
                        t.pronto_para_atualizar = true;
                    }
                    let n = t.pr.as_ref().map_or(0, |p| p.numero);
                    self.avisar(&chat, &format!("Pronto para atualizar o PR #{n}: o Vitral não publicou sozinho ({texto})"), false);
                }
                Resposta::erro(texto)
            }
        }
    }

    /// O PR foi publicado: o estado novo na tarefa, o cartão, o aviso, o vigia e o Linear.
    fn publicado(&mut self, id: u64, plano: &Publicacao, p: Publicado, quando_pronto: Option<String>, cx: &mut Context<Self>) -> Resposta {
        let Some(t) = self.tarefa(id).cloned() else {
            return Resposta::erro("Tarefa não encontrada.");
        };
        let primeira = t.pr_aberto().is_none();
        let agora_s = agora();
        let mut pr = t.pr_aberto().cloned().unwrap_or_else(|| Pr {
            repositorio: plano.repositorio.clone(),
            base: plano.base.clone(),
            ramo: plano.ramo.clone(),
            cabeca: plano.cabeca.clone(),
            dono_do_head: plano.dono_do_head.clone(),
            estado: if p.rascunho { EstadoPr::Rascunho } else { EstadoPr::Aberto },
            acompanhando: true,
            ..Default::default()
        });
        pr.numero = p.numero;
        if !p.url.is_empty() {
            pr.url = p.url.clone();
        }
        pr.titulo = p.titulo.clone();
        pr.sha = p.sha.clone();
        pr.sha_publicado = p.sha.clone();
        pr.commits_de_fora.clear();
        pr.empurrado_em = agora_s;
        // O updatedAt de agora vem na próxima consulta, sem contar como novidade.
        pr.atualizado.clear();
        pr.erro = p.aviso.clone();
        pr.ajuste = None;
        pr.tentativa_mescla = None;
        pr.esperando.retain(|e| matches!(e, EsperaPr::Terceiro { .. } | EsperaPr::Revisao { .. }));
        if quando_pronto.is_some() {
            pr.ao_ficar_pronto = quando_pronto;
        }
        let (n, url, repo) = (pr.numero, pr.url.clone(), pr.repositorio.clone());
        if let Some(t) = self.tarefa_mut(id) {
            t.pr = Some(pr);
            t.estado = EstadoTarefa::NoPr;
            (t.revisao, t.entrega, t.entrega_pendente) = (None, None, None);
            t.pronto_para_atualizar = false;
            t.pedido_da_issue = false;
            if primeira {
                t.rodada_do_ciclo = t.rodada;
            }
            t.atualizada_em = agora_s;
        }
        let chat = t.trabalhador.clone();
        let (tipo, texto) = if primeira { ("pr_aberto", format!("PR #{n} aberto: {url}")) } else { ("pr_atualizado", format!("PR #{n} atualizado.")) };
        self.registrar(tipo, "vitral", &chat, &t.projeto, Some(id), &texto);
        self.avisar(&chat, &texto, false);
        if let Some(aviso) = &p.aviso {
            self.avisar(&chat, aviso, true);
        }
        // O Claude fica sabendo: os avisos do CI e da revisão chegam sozinhos.
        let nota = Entrada {
            de: Some("Vitral".into()),
            texto: format!("{texto} As mudanças desta cópia foram enviadas; avisos do CI e da revisão chegam como [Vitral]."),
            nota: true,
            registrada: true,
            ..Default::default()
        };
        self.filas.entry(chat.clone()).or_default().push(nota);
        self.cartao_do_pr(id, cx);
        self.prs.vigia.depois_do_envio(&repo, Instant::now());
        if primeira {
            self.ligar_pr_no_linear(id, url.clone(), p.titulo.clone(), cx);
        }
        // O Mestre dono fica sabendo (sem acordar): a parte não acabou, o PR está aberto.
        if t.dono() == Some(crate::modelo::ID_MESTRE) {
            self.nota_ao_mestre(format!("Chat {id}: {texto}"), cx);
        }
        self.salvar();
        Resposta::ok(texto)
    }

    /// A linha de acompanhamento do PR no chat (e no Mestre, quando ele é o dono): uma por PR,
    /// atualizada no lugar (os dados vêm ao vivo da tarefa).
    fn cartao_do_pr(&mut self, id: u64, _cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let mut onde = vec![t.trabalhador.clone()];
        if t.dono() == Some(crate::modelo::ID_MESTRE) {
            onde.push(crate::modelo::ID_MESTRE.to_string());
        }
        for chat in onde {
            let ja = self.cartoes.iter().any(|c| c.aberto() && c.chat == chat && matches!(c.tipo, TipoCartao::Pr { tarefa } if tarefa == id));
            if ja {
                continue;
            }
            let cartao = self.proximo_cartao.max(self.cartoes.iter().map(|c| c.id + 1).max().unwrap_or(1)).max(1);
            self.proximo_cartao = cartao + 1;
            self.cartoes.push(crate::modelo::Cartao {
                id: cartao,
                chat: chat.clone(),
                criado_em: agora(),
                decidido_em: 0,
                estado: EstadoCartao::Aberto,
                tipo: TipoCartao::Pr { tarefa: id },
                resto: Default::default(),
            });
            self.chats_sujos.insert(chat.clone());
            if let Some(c) = self.chat_mut(&chat) {
                c.itens.push(Item::Cartao { id: cartao });
            }
        }
    }

    /// O PR mesclou ou fechou: as linhas de acompanhamento dele ficam recolhidas.
    fn recolher_cartoes_do_pr(&mut self, id: u64, mesclado: bool) {
        for c in self.cartoes.iter_mut().filter(|c| c.aberto() && matches!(c.tipo, TipoCartao::Pr { tarefa } if tarefa == id)) {
            c.estado = if mesclado { EstadoCartao::Aceito } else { EstadoCartao::Recusado };
            c.decidido_em = agora();
        }
    }

    /// A rodada de ajuste aberta pelo PR terminou e passou pela verificação e pelo revisor: publica
    /// sozinha (Ship, gatilho de CI ou revisão confiável e as condições) ou espera "Atualizar PR".
    pub(super) fn entrega_do_pr_conferida(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let Some(pr) = t.pr_aberto().cloned() else {
            return;
        };
        // Segredo e `.github/` são conferidos pela publicação (sozinha, qualquer um deles para tudo).
        match regras::pode_publicar_sozinho(&t, false, false) {
            Ok(()) => {
                let _ = self.publicar_pr(id, PedidoPr::default(), OrigemAcao::Automatico, cx);
            }
            Err(motivo) => {
                if let Some(t) = self.tarefa_mut(id) {
                    t.pronto_para_atualizar = true;
                }
                let texto = format!("Pronto para atualizar o PR #{}: a rodada foi verificada ({motivo}).", pr.numero);
                self.avisar(&t.trabalhador, &texto, false);
                if let Some(c) = self.chat_mut(&t.trabalhador) {
                    c.atualizado_em = agora();
                }
                self.salvar();
            }
        }
    }

    /// O Mestre aprovou a entrega de um chat que entrega por PR: atualiza o PR que existe (sem
    /// `.github/`, segredo, commit de fora e até 3 rodadas) ou, no primeiro, pede a confirmação do
    /// usuário (a menos que o turno do Mestre tenha mensagem dele sem texto de terceiros).
    pub(super) fn aprovar_pelo_pr(&mut self, id: u64, autor: &str, resposta: Sender<Resposta>, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            let _ = resposta.try_send(Resposta::erro("Tarefa não encontrada."));
            return;
        };
        // O usuário pelo painel (o "Revisar e juntar" de uma pasta do modo PR): publica já.
        if autor == "usuario" {
            let r = self.publicar_pr(id, PedidoPr::default(), OrigemAcao::Usuario, cx);
            self.encaminhar(r, resposta, cx);
            return;
        }
        if let Some(pr) = t.pr_aberto() {
            if let Err(motivo) = regras::condicoes_sem_usuario(pr, false, false) {
                let _ = resposta.try_send(Resposta::erro(format!(
                    "Não atualizei o PR #{} sozinho: {motivo}. O usuário atualiza pelo Revisar e atualizar PR.",
                    pr.numero
                )));
                if let Some(t) = self.tarefa_mut(id) {
                    t.pronto_para_atualizar = true;
                }
                return;
            }
            let r = self.publicar_pr(id, PedidoPr::default(), OrigemAcao::Mestre, cx);
            self.encaminhar(r, resposta, cx);
            return;
        }
        self.primeiro_pr_pelo_mestre(id, autor, resposta, cx);
    }

    /// O primeiro PR pedido pelo Mestre (seção 8.1): executa se o turno dele tem mensagem do usuário
    /// sem texto de terceiros; senão, o cartão "Confirmar" para o usuário.
    // Na junção com o G6: este corpo vira a guarda dele, guarda_do_chat(ID_MESTRE,
    // &turno_de(ID_MESTRE), &acao, fatos) com o pedir_confirmacao (que grava o RegistroAcao e vê o
    // texto de terceiros lido no turno); o Executar chama publicar_pr(id, pedido, OrigemAcao::Mestre).
    fn primeiro_pr_pelo_mestre(&mut self, id: u64, autor: &str, resposta: Sender<Resposta>, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            let _ = resposta.try_send(Resposta::erro("Tarefa não encontrada."));
            return;
        };
        let turno = self.ultima_entrada.get(crate::modelo::ID_MESTRE).map(|(_, l)| l.clone()).unwrap_or_default();
        let do_usuario = autor == crate::modelo::ID_MESTRE && turno.iter().any(Entrada::do_usuario) && !turno.iter().any(|e| e.terceiros);
        if do_usuario {
            let r = self.publicar_pr(id, PedidoPr::default(), OrigemAcao::Mestre, cx);
            self.encaminhar(r, resposta, cx);
            return;
        }
        let titulo_chat = self.chat(&t.trabalhador).map_or_else(|| t.titulo.clone(), |c| c.titulo.clone());
        let titulo = comum::titulo_do_pr(&titulo_chat, t.issue.as_ref());
        let mensagens = self.chat(&t.trabalhador).map(|c| super::mensagens_desde_a_juncao(&c.itens)).unwrap_or_default();
        let origem = self.origem_da_tarefa(&t);
        let rascunho = self.config_da_pasta(&t.projeto, &origem).github.rascunho;
        let acao = AcaoExterna::AbrirPr { tarefa: id, titulo: titulo.clone(), descricao: corpo_da_tarefa(&t, &mensagens), rascunho, quando_pronto: None };
        let ja = self.cartoes.iter().any(|c| c.aberto() && matches!(&c.tipo, TipoCartao::Confirmar(k) if k.acao == acao));
        if !ja {
            let resumo = format!("O Mestre quer abrir o PR do chat {id} \"{titulo_chat}\": {titulo}");
            self.novo_cartao(TipoCartao::Confirmar(Confirmacao { acao, pedido_por: crate::modelo::ID_MESTRE.into(), resumo, resultado: None }));
            self.gravar_cartoes();
        }
        let _ = resposta.try_send(Resposta::ok("O usuário vai confirmar no Vitral; continue sem isso ou encerre sua vez."));
        cx.notify();
    }

    /// Passa a resposta de uma ação (o canal dela) para outro canal.
    fn encaminhar(&self, de: Receiver<Resposta>, para: Sender<Resposta>, cx: &mut Context<Self>) {
        cx.background_spawn(async move {
            if let Ok(r) = de.recv().await {
                let _ = para.send(r).await;
            }
        })
        .detach();
    }

    /// O efeito "PR aberto" na issue do Linear: liga o PR (ou um link comum); sem a chave na memória,
    /// fica para "Destravar e tentar".
    fn ligar_pr_no_linear(&mut self, id: u64, url: String, titulo: String, cx: &mut Context<Self>) {
        let Some(issue) = self.tarefa(id).and_then(|t| t.issue.clone()).filter(|i| i.fonte == FonteIssue::Linear) else {
            return;
        };
        let Some(chave) = self.chave_na_memoria() else {
            self.efeito_do_pr_pendente(id, EfeitoDoPr::Ligar { url, titulo });
            return;
        };
        let issue_id = issue.id.clone();
        self.em_segundo_plano(
            Provedor::Linear,
            false,
            move |e| Linear { t: &Curl(e), chave: &chave }.ligar_pr(&issue_id, &url, &titulo),
            move |m, r, _| {
                m.anotar_rede(Provedor::Linear, r.as_ref().err());
                if let Err(erro) = r
                    && let Some(chat) = m.tarefa(id).map(|t| t.trabalhador.clone())
                {
                    m.avisar(&chat, &sem_segredos(&format!("Não consegui ligar o PR à {} no Linear: {}.", issue.chave, erro.para_usuario("Linear"))), true);
                }
            },
            cx,
        );
    }

    fn efeito_do_pr_pendente(&mut self, id: u64, efeito: EfeitoDoPr) {
        let motivo = self.motivo_sem_chave();
        let Some(t) = self.tarefa_mut(id) else {
            return;
        };
        let chave = t.issue.as_ref().map_or_else(String::new, |i| i.chave.clone());
        let efeitos = t.efeitos_issue.get_or_insert_with(|| EfeitosDaIssue { feitos: true, ..Default::default() });
        if !efeitos.do_pr.contains(&efeito) {
            efeitos.do_pr.push(efeito);
        }
        efeitos.pendente = Some(motivo.clone());
        let chat = t.trabalhador.clone();
        self.avisar(&chat, &format!("Não consegui mexer na {chave} no Linear: {motivo}"), true);
    }

    /// "Destravar e tentar" dos efeitos do PR que esperavam a chave.
    pub(super) fn rodar_efeitos_do_pr(&mut self, id: u64, cx: &mut Context<Self>) {
        let pendentes = self.tarefa_mut(id).and_then(|t| t.efeitos_issue.as_mut()).map(|e| std::mem::take(&mut e.do_pr)).unwrap_or_default();
        for efeito in pendentes {
            match efeito {
                EfeitoDoPr::Ligar { url, titulo } => self.ligar_pr_no_linear(id, url, titulo, cx),
                EfeitoDoPr::Concluir => self.concluir_issue(id, cx),
            }
        }
    }

    /// O efeito "PR mesclado" na issue: no Linear, o estado de mesclar (só se ainda não terminou).
    fn concluir_issue(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let Some(issue) = t.issue.clone().filter(|i| i.fonte == FonteIssue::Linear) else {
            return;
        };
        let Some(chave) = self.chave_na_memoria() else {
            self.efeito_do_pr_pendente(id, EfeitoDoPr::Concluir);
            return;
        };
        let escolhido = self.projeto(&t.projeto).and_then(|p| p.linear.as_ref()).and_then(|l| l.estado_mesclar.clone());
        if escolhido.as_deref() == Some("") {
            return;
        }
        let issue_id = issue.id.clone();
        self.em_segundo_plano(
            Provedor::Linear,
            false,
            move |e| {
                let linear = Linear { t: &Curl(e), chave: &chave };
                // Confere antes: a integração GitHub do Linear pode já ter movido.
                let agora = linear.ao_iniciar(&issue_id)?;
                if matches!(agora.estado.tipo, comum::TipoEstado::Concluida | comum::TipoEstado::Cancelada) {
                    return Ok(Some(agora.estado));
                }
                let destino = escolhido.or_else(|| {
                    agora
                        .estados
                        .iter()
                        .filter(|e| e.estado.tipo == comum::TipoEstado::Concluida)
                        .min_by(|a, b| a.posicao.total_cmp(&b.posicao))
                        .map(|e| e.estado.id.clone())
                });
                match destino {
                    Some(estado) => linear.mudar_estado(&issue_id, &estado).map(Some),
                    None => Ok(None),
                }
            },
            move |m, r, _| {
                m.anotar_rede(Provedor::Linear, r.as_ref().err());
                match r {
                    Ok(Some(estado)) => {
                        if let Some(t) = m.tarefa_mut(id) {
                            if let Some(i) = t.issue.as_mut() {
                                i.movida_para = Some(estado.id.clone());
                            }
                            if let Some(a) = t.prs_anteriores.last_mut() {
                                a.issue_concluida = true;
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(erro) => {
                        if let Some(chat) = m.tarefa(id).map(|t| t.trabalhador.clone()) {
                            m.avisar(&chat, &sem_segredos(&format!("Não consegui concluir a {} no Linear: {}.", issue.chave, erro.para_usuario("Linear"))), true);
                        }
                    }
                }
                m.salvar();
            },
            cx,
        );
    }

    // ---------- mesclar (6.7) ----------

    /// Mescla agora (o usuário, ou o Ship quando ficou pronto) ou arma o "quando ficar pronto".
    pub(super) fn mesclar_pr(
        &mut self,
        id: u64,
        metodo: Option<String>,
        quando_pronto: bool,
        origem: OrigemAcao,
        confirmado: Vec<Confirmado>,
        cx: &mut Context<Self>,
    ) -> Receiver<Resposta> {
        let Some(t) = self.tarefa(id).cloned() else {
            return respondido(Resposta::erro("Tarefa não encontrada."));
        };
        let Some(pr) = t.pr_aberto().cloned() else {
            return respondido(Resposta::erro("Este chat não tem PR aberto."));
        };
        let info = self.repo_github(&pr.repositorio).tirar().map(|r| r.info);
        let permitidos: Vec<&str> = info.as_ref().map(super::conexoes::metodos_permitidos).unwrap_or_else(|| vec!["squash", "merge", "rebase"]);
        let metodo = metodo
            .map(|m| m.trim().to_ascii_lowercase())
            .filter(|m| !m.is_empty())
            .or_else(|| info.as_ref().map(|i| i.metodo_padrao.clone()).filter(|m| !m.is_empty()))
            .unwrap_or_else(|| permitidos.first().map_or("squash", |m| *m).to_string());
        if !permitidos.contains(&metodo.as_str()) {
            return respondido(Resposta::erro(format!("{} não aceita mesclar com {metodo}; dá para: {}.", pr.repositorio, permitidos.join(", "))));
        }
        if quando_pronto {
            if let Some(pr) = self.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
                pr.ao_ficar_pronto = Some(metodo.clone());
                pr.acompanhando = true;
            }
            self.tirar_espera(id, |e| matches!(e, EsperaPr::MesclaRecusada { .. }));
            self.prs.vigia.na_hora(&pr.repositorio);
            self.salvar();
            cx.notify();
            return respondido(Resposta::ok(format!(
                "Vou mesclar o PR #{} ({metodo}) quando os checks terminarem e não houver pedido de mudança.",
                pr.numero
            )));
        }
        if info.as_ref().is_some_and(|i| !i.pode_escrever()) {
            return respondido(Resposta::erro(format!("Sua conta não pode mesclar em {}", pr.repositorio)));
        }
        if let Some(motivo) = self.ocupado_para_o_pr(&t, origem) {
            return respondido(Resposta::erro(motivo));
        }
        let automatico = origem == OrigemAcao::Automatico;
        let instavel = confirmado.contains(&Confirmado::Instavel);
        let confirmado_ate = confirmado.iter().find_map(|c| match c {
            Confirmado::CommitsDeFora(sha) => Some(sha.clone()),
            Confirmado::Instavel => None,
        });
        let condicoes = self.condicoes(&t, &pr, automatico, instavel);
        self.prs.mesclando.insert(id);
        cx.notify();
        let (envia, recebe) = smol::channel::bounded(1);
        let e = self.executor();
        let pasta = self.dados.pasta.clone();
        let fila = self.fila_do_provedor(Provedor::Github, !automatico);
        let copia = t.worktree.clone();
        cx.spawn(async move |motor, cx| {
            let vez = fila.acquire_arc().await;
            let metodo_bg = metodo.clone();
            let r = cx
                .background_spawn(async move {
                    let _vez = vez;
                    // Confere de novo: o head e o mergeStateStatus de agora.
                    let acompanhado = [Acompanhado { tarefa: id, pr: pr.clone(), copia }];
                    let (mut conferidos, _) = trabalho::conferir_prs(&*e, &pasta, &pr.repositorio, &acompanhado, agora())?;
                    let (novo, _) = match conferidos.pop() {
                        Some((_, r)) => r?,
                        None => return Err(ErroRemoto::NaoEncontrado(format!("o PR #{}", pr.numero))),
                    };
                    let sha = if automatico {
                        novo.sha_publicado.clone()
                    } else if novo.sha != novo.sha_publicado {
                        match confirmado_ate {
                            Some(visto) if visto == novo.sha => visto,
                            _ => return Err(ErroRemoto::Validacao("Chegou commit de fora no PR: confirme o head que você viu antes de mesclar.".into())),
                        }
                    } else {
                        novo.sha.clone()
                    };
                    let pronto = regras::pronto_para_mesclar(&Pr { sha: sha.clone(), ..novo.clone() }, agora(), &condicoes);
                    if let Err(motivo) = pronto {
                        return Ok((novo, Err(ErroRemoto::Validacao(motivo))));
                    }
                    Ok((novo.clone(), trabalho::mesclar_conferindo(&*e, &pasta, &novo, &metodo_bg, &sha)))
                })
                .await;
            let _ = motor.update(cx, |m, cx| {
                let resposta = m.mescla_chegou(id, &metodo, origem, r, cx);
                let _ = envia.try_send(resposta);
                m.salvar();
                cx.notify();
            });
        })
        .detach();
        recebe
    }

    #[allow(clippy::type_complexity)]
    fn mescla_chegou(
        &mut self,
        id: u64,
        metodo: &str,
        origem: OrigemAcao,
        r: Result<(Pr, Result<Mescla, ErroRemoto>), ErroRemoto>,
        cx: &mut Context<Self>,
    ) -> Resposta {
        self.prs.mesclando.remove(&id);
        let (novo, mescla) = match r {
            Ok((novo, mescla)) => (Some(novo), mescla),
            Err(erro) => (None, Err(erro)),
        };
        if let Some(novo) = novo
            && let Some(t) = self.tarefa_mut(id)
            && t.pr.as_ref().is_some_and(|p| p.numero == novo.numero && p.aberto())
        {
            t.pr = Some(novo);
        }
        let Some(t) = self.tarefa(id).cloned() else {
            return Resposta::erro("Tarefa não encontrada.");
        };
        let Some(pr) = t.pr.clone() else {
            return Resposta::erro("O PR não está mais na tarefa.");
        };
        let (n, repo, chat) = (pr.numero, pr.repositorio.clone(), t.trabalhador.clone());
        let anotar = |m: &mut Motor, f: &dyn Fn(&mut Pr)| {
            if let Some(pr) = m.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
                f(pr);
            }
        };
        match mescla {
            Ok(Mescla::Mesclado) => {
                self.anotar_rede(Provedor::Github, None);
                anotar(self, &|pr| {
                    pr.metodo_mesclado = Some(metodo.to_string());
                    pr.erro = None;
                    pr.empurrado_em = agora();
                });
                self.prs.vigia.depois_do_envio(&repo, Instant::now());
                Resposta::ok(format!("PR #{n} mesclado ({metodo})."))
            }
            Ok(Mescla::NaFila) => {
                anotar(self, &|pr| {
                    pr.metodo_mesclado = Some(metodo.to_string());
                    pr.fila_de_merge = true;
                });
                self.prs.vigia.depois_do_envio(&repo, Instant::now());
                Resposta::ok(format!("O PR #{n} entrou na fila de merge; o Vitral acompanha até mesclar."))
            }
            Ok(Mescla::AutoLigada) => {
                anotar(self, &|pr| {
                    pr.metodo_mesclado = Some(metodo.to_string());
                    pr.auto_github = true;
                });
                self.prs.vigia.depois_do_envio(&repo, Instant::now());
                Resposta::ok(format!("Faltam checks obrigatórios: o GitHub mescla o PR #{n} quando eles passarem."))
            }
            Err(ErroRemoto::TempoEsgotado) => {
                anotar(self, &|pr| pr.erro = Some("Resultado incerto: o Vitral está conferindo no GitHub.".into()));
                self.prs.vigia.depois_do_envio(&repo, Instant::now());
                Resposta::erro("Resultado incerto: o Vitral está conferindo no GitHub.")
            }
            Err(ErroRemoto::Conflito(motivo)) if motivo == "base mudou" && origem != OrigemAcao::Automatico => {
                // Uma nova tentativa só, depois de 30 s.
                let metodo = metodo.to_string();
                cx.spawn(async move |motor, cx| {
                    cx.background_executor().timer(Duration::from_secs(30)).await;
                    let _ = motor.update(cx, |m, cx| {
                        let _ = m.mesclar_pr(id, Some(metodo), false, OrigemAcao::Usuario, Vec::new(), cx);
                    });
                })
                .detach();
                Resposta::ok(format!("A base do PR #{n} mudou no meio; o Vitral tenta mesclar de novo em 30 s."))
            }
            Err(erro) => {
                self.anotar_rede(Provedor::Github, Some(&erro));
                let motivo = match &erro {
                    ErroRemoto::Validacao(m) => sem_segredos(m),
                    outro => outro.para_usuario("GitHub"),
                };
                if origem == OrigemAcao::Automatico {
                    let chave = regras::chave_da_tentativa(&pr);
                    anotar(self, &|pr| pr.tentativa_mescla = Some(chave.clone()));
                    self.desarmar(id, cx);
                    let texto = format!("O GitHub não aceitou mesclar o PR #{n}: {motivo}");
                    self.esperar_no_pr(id, EsperaPr::MesclaRecusada { motivo: motivo.clone() });
                    self.avisar(&chat, &texto, true);
                    if let Some(c) = self.chat(&chat).cloned() {
                        self.notificar(&c, &texto);
                    }
                } else {
                    anotar(self, &|pr| pr.erro = Some(motivo.clone()));
                }
                Resposta::erro(format!("Não mesclei o PR #{n}: {motivo}"))
            }
        }
    }

    // ---------- depois de mesclar e fechar (6.8 e 6.9) ----------

    /// O PR foi mesclado (por qualquer pessoa): registra, apaga o ramo remoto se foi o Vitral,
    /// recomeça a cópia (ou deixa pendente com o chat rodando), anda com a origem, fecha o ciclo e
    /// mexe na issue. Idempotente: sem o PR mesclado na tarefa, só o recomeço pendente pode rodar.
    pub(super) fn depois_de_mesclar(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        if !t.pr.as_ref().is_some_and(|p| p.estado == EstadoPr::Mesclado) {
            return;
        }
        let info = t.pr.as_ref().and_then(|p| self.repo_github(&p.repositorio).tirar()).map(|r| r.info);
        let parte = t.divisao.is_some() && t.dono().is_some();
        let Some(pr) = self.tarefa_mut(id).and_then(|t| regras::registrar_mescla(t, agora())) else {
            return;
        };
        self.recolher_cartoes_do_pr(id, true);
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let anterior = t.prs_anteriores.last().cloned().unwrap_or_default();
        let apagar = apagar_ramo_depois(&anterior, &pr, info.as_ref());
        let chat = t.trabalhador.clone();
        let livre = !self.processos.contains_key(&chat) && self.na_fila(&chat) == 0;
        let n = pr.numero;
        self.registrar(
            "pr_mesclado",
            pr.mesclado_por.as_deref().unwrap_or("github"),
            &chat,
            &t.projeto,
            Some(id),
            &format!("PR #{n} mesclado{}", pr.mesclado_por.as_deref().map(|p| format!(" por {p}")).unwrap_or_default()),
        );
        if !self.janela_ativa
            && let Some(c) = self.chat(&chat).cloned()
        {
            self.notificar(&c, &format!("PR #{n} mesclado"));
        }
        if parte {
            self.parte_terminou(id, "mesclada", Some(format!("PR #{n}")), cx);
        }
        // A issue: no Linear, o estado de mesclar; no GitHub, o `Closes` fecha (sem ele, a linha do
        // PR oferece fechar).
        match t.issue.as_ref().map(|i| i.fonte) {
            Some(FonteIssue::Linear) => self.concluir_issue(id, cx),
            Some(FonteIssue::Github) => self.conferir_issue_fechada(id, &pr, cx),
            None => {}
        }
        self.recomecar_depois(id, &pr, apagar, livre, cx);
        self.salvar();
        cx.notify();
    }

    /// O recomeço da cópia depois do PR mesclado (ou só o ramo remoto e a origem, com o chat rodando:
    /// aí a cópia fica para o fim do turno).
    fn recomecar_depois(&mut self, id: u64, pr: &Pr, apagar: bool, recomecar: bool, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let origem = self.origem_da_tarefa(&t);
        let Some((remoto, base)) = self.remoto_e_base(&t).or_else(|| Some(("origin".into(), pr.base.clone()))) else {
            return;
        };
        let ramo_atual = t.branch.clone().unwrap_or_default();
        let config = self.config_da_pasta(&t.projeto, &origem).github;
        let sugerido = comum::ramo_da_tarefa(id, t.issue.as_ref(), self.ramo_sugerido_guardado_da(&t).as_deref(), &config.prefixo_ramo, &t.titulo, &|_| false);
        let existentes = git::nomes_de_ramos(&origem);
        let ramo_novo = regras::ramo_depois_do_pr(&sugerido, &ramo_atual, t.rodada, &|r| existentes.contains(r));
        let recomecar = recomecar && t.tem_copia() && t.worktree.as_ref().is_some_and(|w| w.is_dir());
        if !recomecar
            && t.tem_copia()
            && let Some(t) = self.tarefa_mut(id)
        {
            t.recomeco_pendente = true;
        }
        let d = DepoisDaMescla {
            copia: t.worktree.clone(),
            origem,
            remoto,
            base,
            numero: pr.numero,
            head_final: pr.sha.clone(),
            ramo: pr.ramo.clone(),
            ramo_novo,
            excluir: t.arquivos_copiados.clone(),
            grupo: t.grupo(),
            apagar_ramo: apagar,
            recomecar,
        };
        let e = self.executor();
        let (n, metodo, por) = (pr.numero, pr.metodo_mesclado.clone(), pr.mesclado_por.clone());
        cx.spawn(async move |motor, cx| {
            let d_bg = d.clone();
            let feito = cx.background_spawn(async move { trabalho::depois_da_mescla(&*e, &d_bg).await }).await;
            let _ = motor.update(cx, |m, cx| {
                m.recomeco_chegou(id, n, &d, metodo.as_deref(), por.as_deref(), feito);
                m.salvar();
                cx.notify();
            });
        })
        .detach();
    }

    fn ramo_sugerido_guardado_da(&self, t: &Tarefa) -> Option<String> {
        t.issue.as_ref().and_then(|i| self.ramo_sugerido_guardado(i))
    }

    fn recomeco_chegou(&mut self, id: u64, n: u64, d: &DepoisDaMescla, metodo: Option<&str>, por: Option<&str>, feito: trabalho::Recomecado) {
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let chat = t.trabalhador.clone();
        let alvo = format!("{}/{}", d.remoto, d.base);
        if let Some(Ok(())) = feito.ramo_apagado
            && let Some(a) = self.tarefa_mut(id).and_then(|t| t.prs_anteriores.iter_mut().rev().find(|a| a.numero == n))
        {
            a.ramo_apagado = true;
        }
        if let Some(Err(erro)) = &feito.ramo_apagado {
            self.avisar(&chat, &format!("O ramo {} não foi apagado no GitHub ({erro}).", d.ramo), false);
        }
        let mut ramo_novo = None;
        match feito.recomeco {
            None => {}
            Some(Ok(git::Recomeco::Feito { ramo, base, voltaram_mudancas, .. })) => {
                if let Some(t) = self.tarefa_mut(id) {
                    t.branch = Some(ramo.clone());
                    t.ramo_planejado = Some(ramo.clone());
                    t.base = Some(base);
                    t.recomeco_pendente = false;
                    t.mudancas = None;
                }
                let nota = Entrada {
                    de: Some("Vitral".into()),
                    texto: format!(
                        "O PR #{n} foi mesclado. A sua cópia agora está no ramo `{ramo}`, a partir de {alvo}{}.",
                        if voltaram_mudancas { ", com as mudanças sem commit de volta" } else { "" }
                    ),
                    nota: true,
                    registrada: true,
                    ..Default::default()
                };
                self.filas.entry(chat.clone()).or_default().push(nota);
                ramo_novo = Some(ramo);
            }
            Some(Ok(git::Recomeco::FaltamCommits)) => {
                if let Some(t) = self.tarefa_mut(id) {
                    t.recomeco_pendente = false;
                }
                self.avisar(
                    &chat,
                    &format!("O PR #{n} foi mesclado com commits que esta cópia não tem; a cópia ficou como estava. Use Recomeçar a partir de {alvo}."),
                    true,
                );
            }
            Some(Ok(git::Recomeco::Conflito { arquivos })) => {
                if let Some(t) = self.tarefa_mut(id) {
                    t.recomeco_pendente = false;
                }
                self.avisar(
                    &chat,
                    &format!(
                        "Não consegui levar suas mudanças depois do PR para cima de {alvo} (conflito em {}); a cópia ficou no ramo antigo.",
                        arquivos.join(", ")
                    ),
                    true,
                );
            }
            Some(Err(erro)) => self.avisar(&chat, &format!("A cópia não recomeçou depois do PR #{n} ({erro}); ela ficou como estava."), true),
        }
        let texto = prompts::pr_mesclado(n, metodo, por, ramo_novo.as_deref(), &alvo);
        self.avisar(&chat, &texto, false);
        if let Some(c) = self.chat_mut(&chat) {
            c.atualizado_em = agora();
        }
    }

    /// O recomeço que ficou para o fim do turno (o PR mesclou com o chat rodando).
    pub(super) fn recomeco_pendente(&mut self, chat: &str, cx: &mut Context<Self>) {
        let Some(t) = self.chat(chat).and_then(|c| c.tarefa).and_then(|n| self.tarefa(n)).cloned() else {
            return;
        };
        if !t.recomeco_pendente || self.processos.contains_key(chat) || self.na_fila(chat) > 0 {
            return;
        }
        let Some(a) = t.prs_anteriores.iter().rev().find(|a| a.estado == EstadoPr::Mesclado).cloned() else {
            if let Some(t) = self.tarefa_mut(t.id) {
                t.recomeco_pendente = false;
            }
            return;
        };
        let pr = Pr {
            repositorio: a.repositorio.clone(),
            numero: a.numero,
            sha: a.sha.clone(),
            metodo_mesclado: a.metodo.clone(),
            mesclado_por: a.mesclado_por.clone(),
            ..Default::default()
        };
        // O ramo remoto já foi tratado na hora do merge.
        self.recomecar_depois(t.id, &pr, false, true, cx);
    }

    /// O PR mesclado sem `closingIssuesReferences`: a linha do PR oferece fechar a issue do GitHub.
    fn conferir_issue_fechada(&mut self, id: u64, pr: &Pr, cx: &mut Context<Self>) {
        let Some(numero) = self.tarefa(id).and_then(|t| t.issue.as_ref()).and_then(|i| i.id.trim_start_matches('#').parse::<u64>().ok()) else {
            return;
        };
        let (repo, n) = (pr.repositorio.clone(), pr.numero);
        let pasta = self.dados.pasta.clone();
        self.em_segundo_plano(
            Provedor::Github,
            false,
            move |e| Gh { e, pasta: &pasta }.detalhe_pr(&repo, n),
            move |m, r: Result<DetalhePr, ErroRemoto>, _| {
                let Ok(d) = r else {
                    return;
                };
                if let Some(a) = m.tarefa_mut(id).and_then(|t| t.prs_anteriores.iter_mut().rev().find(|a| a.numero == n)) {
                    if d.issues_fechadas.contains(&numero) {
                        a.issue_concluida = true;
                    } else {
                        a.fechar_issue = Some(numero);
                    }
                }
                m.salvar();
            },
            cx,
        );
    }

    /// O PR foi fechado sem mesclar: vai para os anteriores; a cópia e o ramo continuam.
    fn pr_fechado(&mut self, id: u64, _cx: &mut Context<Self>) {
        let Some(pr) = self.tarefa_mut(id).and_then(|t| regras::registrar_fechamento(t, agora())) else {
            return;
        };
        self.recolher_cartoes_do_pr(id, false);
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let chat = t.trabalhador.clone();
        self.registrar("pr_fechado", "github", &chat, &t.projeto, Some(id), &format!("PR #{} fechado sem mesclar", pr.numero));
        self.avisar(
            &chat,
            &format!("PR #{} foi fechado sem mesclar; a cópia e o ramo continuam. Para tentar de novo, use Revisar e abrir PR.", pr.numero),
            false,
        );
        self.salvar();
    }

    /// Arquivar ou apagar o chat desarma o Ship do PR dele (o PR continua aberto; o diálogo da
    /// interface oferece fechar).
    pub(super) fn desarmar_ao_arquivar(&mut self, chat: &str, cx: &mut Context<Self>) {
        let Some(id) = self.chat(chat).and_then(|c| c.tarefa) else {
            return;
        };
        if self.pr_da_tarefa(id).is_some_and(Pr::armado) {
            self.desarmar(id, cx);
        }
    }

    // ---------- leituras da aba PR ----------

    #[allow(dead_code)] // Provisório: as leituras da aba PR (G5).
    fn pedir_pr(&self, pedido: PedidoPrLeitura) {
        let mut pedidos = self.prs.pedidos.borrow_mut();
        if pedidos.contains(&pedido) {
            return;
        }
        let acordar = pedidos.is_empty();
        pedidos.push(pedido);
        if acordar {
            let _ = self.sinais.try_send(Sinal::Integracao);
        }
    }

    pub(super) fn atender_pedidos_de_prs(&mut self, cx: &mut Context<Self>) {
        let pedidos = std::mem::take(&mut *self.prs.pedidos.borrow_mut());
        let agora_i = Instant::now();
        for pedido in pedidos {
            let pasta = self.dados.pasta.clone();
            match pedido {
                PedidoPrLeitura::Detalhe(repo, n) if precisa_buscar(self.prs.detalhes.get(&(repo.clone(), n)), agora_i) => {
                    marcar_buscando(&mut self.prs.detalhes, (repo.clone(), n), agora_i);
                    let lido = repo.clone();
                    self.em_segundo_plano(
                        Provedor::Github,
                        true,
                        move |e| Gh { e, pasta: &pasta }.detalhe_pr(&lido, n),
                        move |m, r, _| {
                            m.anotar_rede(Provedor::Github, r.as_ref().err());
                            guardar(&mut m.prs.detalhes, (repo, n), r.map_err(|e| e.para_usuario("GitHub")), Instant::now());
                        },
                        cx,
                    );
                }
                PedidoPrLeitura::Checks(repo, n) if precisa_buscar(self.prs.checks.get(&(repo.clone(), n)), agora_i) => {
                    marcar_buscando(&mut self.prs.checks, (repo.clone(), n), agora_i);
                    let lido = repo.clone();
                    self.em_segundo_plano(
                        Provedor::Github,
                        true,
                        move |e| Gh { e, pasta: &pasta }.checks(&lido, n),
                        move |m, r, _| {
                            m.anotar_rede(Provedor::Github, r.as_ref().err());
                            guardar(&mut m.prs.checks, (repo, n), r.map_err(|e| e.para_usuario("GitHub")), Instant::now());
                        },
                        cx,
                    );
                }
                PedidoPrLeitura::Linha(repo, n) if precisa_buscar(self.prs.linhas.get(&(repo.clone(), n)), agora_i) => {
                    marcar_buscando(&mut self.prs.linhas, (repo.clone(), n), agora_i);
                    let lido = repo.clone();
                    let de_fora: Vec<String> = self
                        .tarefas
                        .iter()
                        .filter_map(|t| t.pr.as_ref())
                        .filter(|p| p.repositorio == repo && p.numero == n)
                        .flat_map(|p| p.commits_de_fora.clone())
                        .collect();
                    let nossos: Vec<u64> = self
                        .tarefas
                        .iter()
                        .filter_map(|t| t.pr.as_ref())
                        .filter(|p| p.repositorio == repo && p.numero == n)
                        .flat_map(|p| p.comentarios_nossos.clone())
                        .collect();
                    self.em_segundo_plano(
                        Provedor::Github,
                        true,
                        move |e| {
                            let gh = Gh { e, pasta: &pasta };
                            let detalhe = gh.detalhe_pr(&lido, n)?;
                            let comentarios = gh.comentarios_novos(&lido, n, None)?;
                            Ok((detalhe, comentarios))
                        },
                        move |m, r: Result<(DetalhePr, Vec<ComentarioPr>), ErroRemoto>, _| {
                            m.anotar_rede(Provedor::Github, r.as_ref().err());
                            let r = r.map(|(detalhe, comentarios)| {
                                m.guardar_comentarios((repo.clone(), n), &comentarios);
                                let linha = linha_do_tempo_de(&detalhe, &comentarios, &de_fora, &nossos);
                                guardar(&mut m.prs.detalhes, (repo.clone(), n), Ok(detalhe), Instant::now());
                                linha
                            });
                            guardar(&mut m.prs.linhas, (repo, n), r.map_err(|e| e.para_usuario("GitHub")), Instant::now());
                        },
                        cx,
                    );
                }
                _ => {}
            }
        }
    }

    /// A simulação da mescla da cópia com a base (depois de um fetch), em segundo plano.
    #[allow(dead_code)] // Provisório: a aba PR (G5).
    fn simular(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let (Some(copia), Some((remoto, base))) = (t.worktree.clone().filter(|c| c.is_dir()), self.remoto_e_base(&t)) else {
            return;
        };
        if self.prs.simulacoes.get(&id).is_some_and(|(quando, _)| quando.elapsed() < Duration::from_secs(60)) {
            return;
        }
        self.prs.simulacoes.insert(id, (Instant::now(), self.prs.simulacoes.get(&id).map(|(_, s)| s.clone()).unwrap_or(Ok(Vec::new()))));
        let origem = self.origem_da_tarefa(&t);
        let e = self.executor();
        cx.spawn(async move |motor, cx| {
            let r = cx
                .background_spawn(async move {
                    let _rede = git::na_vez_da_rede(&origem).await;
                    // Sem rede, simula com a base que já estava baixada.
                    let _ = git::buscar(&*e, &copia, &remoto, &[&base]);
                    git::simular_mescla(&copia, &format!("{remoto}/{base}")).map_err(|e| sem_segredos(&format!("{e:#}")))
                })
                .await;
            let _ = motor.update(cx, |m, cx| {
                m.prs.simulacoes.insert(id, (Instant::now(), r));
                cx.notify();
            });
        })
        .detach();
    }
}

/// A linha do tempo da aba PR: os commits, as revisões e os comentários, do mais recente para o
/// mais antigo, com os bots informativos agrupados numa linha por bot.
fn linha_do_tempo_de(d: &DetalhePr, comentarios: &[ComentarioPr], de_fora: &[String], nossos: &[u64]) -> Vec<EventoLinha> {
    let mut itens: Vec<(String, EventoLinha)> = Vec::new();
    for c in &d.commits {
        let curto: String = c.sha.chars().take(7).collect();
        itens.push((
            c.data.clone(),
            EventoLinha::Commit {
                sha: curto.clone(),
                titulo: c.titulo.clone(),
                autor: c.autores.first().cloned().unwrap_or_default(),
                de_fora: de_fora.contains(&curto),
                em: c.data.clone(),
            },
        ));
    }
    let mut bots: Vec<(String, u32, String)> = Vec::new();
    for c in comentarios {
        let sem_acesso = c.confianca() == Confianca::Terceiro && !nossos.contains(&c.id);
        if c.confianca() == Confianca::Informativo {
            match bots.iter_mut().find(|(l, ..)| *l == c.autor) {
                Some((_, n, quando)) => {
                    *n += 1;
                    if c.criado > *quando {
                        *quando = c.criado.clone();
                    }
                }
                None => bots.push((c.autor.clone(), 1, c.criado.clone())),
            }
            continue;
        }
        let evento = match c.tipo {
            TipoComentario::Revisao => {
                if c.corpo.trim().is_empty() && c.estado_revisao.as_deref() == Some("COMMENTED") {
                    continue;
                }
                EventoLinha::Revisao {
                    id: c.id,
                    autor: c.autor.clone(),
                    estado: c.estado_revisao.clone().unwrap_or_default(),
                    corpo: c.corpo.clone(),
                    em: c.criado.clone(),
                    sem_acesso,
                }
            }
            TipoComentario::EmLinha => EventoLinha::ComentarioEmLinha {
                id: c.id,
                autor: c.autor.clone(),
                arquivo: c.caminho.clone().unwrap_or_default(),
                linha: c.linha,
                trecho: c.trecho.clone().unwrap_or_default(),
                corpo: c.corpo.clone(),
                em: c.criado.clone(),
            },
            TipoComentario::Geral => EventoLinha::Comentario { id: c.id, autor: c.autor.clone(), corpo: c.corpo.clone(), em: c.criado.clone(), sem_acesso },
        };
        itens.push((c.criado.clone(), evento));
    }
    for (login, quantos, quando) in bots {
        itens.push((quando, EventoLinha::Bots { login, quantos }));
    }
    itens.sort_by(|a, b| b.0.cmp(&a.0));
    itens.into_iter().map(|(_, e)| e).collect()
}

// Provisório: quem chama a API da aba PR é a interface do G5 (feita ao mesmo tempo, contra estas
// assinaturas).
#[allow(dead_code)]
impl Motor {
    // ---------- leitura (seção 10.1) ----------

    pub fn pr_da_tarefa(&self, id: u64) -> Option<&Pr> {
        self.tarefa(id)?.pr.as_ref()
    }

    pub fn prs_anteriores(&self, id: u64) -> &[PrAnterior] {
        self.tarefa(id).map_or(&[], |t| t.prs_anteriores.as_slice())
    }

    pub fn detalhe_pr(&self, repo: &str, n: u64) -> Carregando<DetalhePr> {
        let guardado = self.prs.detalhes.get(&(repo.to_string(), n));
        if comum::repositorio_valido(repo) && precisa_buscar(guardado, Instant::now()) {
            self.pedir_pr(PedidoPrLeitura::Detalhe(repo.to_string(), n));
        }
        guardado.map_or(Carregando::Carregando(None), |(_, c)| c.clone())
    }

    pub fn linha_do_tempo(&self, repo: &str, n: u64) -> Carregando<Vec<EventoLinha>> {
        let guardado = self.prs.linhas.get(&(repo.to_string(), n));
        if comum::repositorio_valido(repo) && precisa_buscar(guardado, Instant::now()) {
            self.pedir_pr(PedidoPrLeitura::Linha(repo.to_string(), n));
        }
        guardado.map_or(Carregando::Carregando(None), |(_, c)| c.clone())
    }

    pub fn checks_do_pr(&self, repo: &str, n: u64) -> Carregando<Vec<Check>> {
        let guardado = self.prs.checks.get(&(repo.to_string(), n));
        if comum::repositorio_valido(repo) && precisa_buscar(guardado, Instant::now()) {
            self.pedir_pr(PedidoPrLeitura::Checks(repo.to_string(), n));
        }
        guardado.map_or(Carregando::Carregando(None), |(_, c)| c.clone())
    }

    /// O trecho do log que explica a falha ("Ver log").
    pub fn log_do_check(&mut self, repo: &str, link: String, cx: &mut Context<Self>) -> Receiver<Result<String, String>> {
        let (envia, recebe) = smol::channel::bounded(1);
        let (repo, pasta) = (repo.to_string(), self.dados.pasta.clone());
        self.em_segundo_plano(
            Provedor::Github,
            true,
            move |e| Gh { e, pasta: &pasta }.log_da_falha(&repo, &link),
            move |m, r, _| {
                m.anotar_rede(Provedor::Github, r.as_ref().err());
                let resposta = match r {
                    Ok(Some(log)) => Ok(sem_segredos(&log)),
                    Ok(None) => Err("A execução ainda está rodando; o log sai quando ela terminar.".to_string()),
                    Err(ErroRemoto::NaoEncontrado(motivo)) if motivo == remoto::LOG_VENCIDO => {
                        Err("O log desta execução já expirou no GitHub (retenção de 90 dias).".to_string())
                    }
                    Err(erro) => Err(erro.para_usuario("GitHub")),
                };
                let _ = envia.try_send(resposta);
            },
            cx,
        );
        recebe
    }

    /// O diff do diálogo: o da cópia contra `base_da_tarefa` (sem rede), ou o do GitHub.
    pub fn diff_do_pr(&self, fonte: FonteDiff, cx: &App) -> Task<Option<String>> {
        match fonte {
            FonteDiff::Tarefa(id) => {
                let Some(t) = self.tarefa(id).cloned() else {
                    return Task::ready(None);
                };
                let (origem, remoto_base) = (self.origem_da_tarefa(&t), self.remoto_base(&t));
                cx.background_spawn(async move {
                    let (copia, base) = (t.worktree.filter(|c| c.is_dir())?, t.base?);
                    let base = git::base_da_tarefa(&origem, &copia, &base, remoto_base.as_deref());
                    git::diff_completo(&copia, &base).ok()
                })
            }
            FonteDiff::Pr { repositorio, numero } => {
                let (e, pasta) = (self.executor(), self.dados.pasta.clone());
                cx.background_spawn(async move { Gh { e: &*e, pasta: &pasta }.diff_pr(&repositorio, numero).ok() })
            }
        }
    }

    /// A simulação da mescla com a base: `Ok(arquivos em conflito)`, vazia quando junta limpo.
    pub fn simulacao(&self, id: u64) -> Option<Result<Vec<String>, String>> {
        self.prs.simulacoes.get(&id).map(|(_, s)| s.clone())
    }

    /// As ações da aba PR e por que cada uma não está habilitada: a tabela 4.3, pura, em
    /// `ui::painel_pr::acoes_do_pr`. Aqui entram o repositório, a simulação e quem ocupa o PR (a
    /// publicação, a mesclagem ou o chat).
    pub fn acoes_do_pr(&self, id: u64) -> Vec<(AcaoPr, Result<(), String>)> {
        let Some(t) = self.tarefa(id) else {
            return Vec::new();
        };
        let Some(pr) = t.pr.as_ref() else {
            // O último PR fechado reabre.
            return match t.prs_anteriores.last() {
                Some(a) if a.estado == EstadoPr::Fechado => vec![(AcaoPr::Reabrir, Ok(()))],
                _ => Vec::new(),
            };
        };
        let info = self.repo_github(&pr.repositorio).tirar().map(|r| r.info);
        // Sem publicação nem mesclagem em andamento, o que sobra do `ocupado_para_o_pr` é o chat
        // (rodando, com fila ou juntando), que as ações do usuário esperam terminar.
        let rodando = self.ocupado_para_o_pr(t, OrigemAcao::Usuario).is_some();
        let ocupado = match self.andamento_do_pr(id) {
            Some(AndamentoPr::Publicando) => Some(Ocupado::Publicando),
            Some(AndamentoPr::Mesclando) => Some(Ocupado::Mesclando),
            None => rodando.then_some(Ocupado::Chat),
        };
        crate::ui::painel_pr::acoes_do_pr(pr, info.as_ref(), self.simulacao(id).as_ref(), ocupado)
    }

    /// O que parece segredo no que a cópia acrescentaria ao PR.
    pub fn segredos_da_tarefa(&self, id: u64, cx: &App) -> Task<Vec<Achado>> {
        let Some(t) = self.tarefa(id).cloned() else {
            return Task::ready(Vec::new());
        };
        let (origem, remoto_base) = (self.origem_da_tarefa(&t), self.remoto_base(&t));
        cx.background_spawn(async move {
            let (Some(copia), Some(base)) = (t.worktree.filter(|c| c.is_dir()), t.base) else {
                return Vec::new();
            };
            let base = git::base_da_tarefa(&origem, &copia, &base, remoto_base.as_deref());
            git::segredos_no_diff(&copia, &base, &t.arquivos_copiados)
        })
    }

    /// Os arquivos que o PR acrescenta (sem os copiados da origem, que nunca entram).
    pub fn arquivos_novos(&self, id: u64, cx: &App) -> Task<Vec<String>> {
        let Some(t) = self.tarefa(id).cloned() else {
            return Task::ready(Vec::new());
        };
        let (origem, remoto_base) = (self.origem_da_tarefa(&t), self.remoto_base(&t));
        cx.background_spawn(async move {
            let (Some(copia), Some(base)) = (t.worktree.filter(|c| c.is_dir()), t.base) else {
                return Vec::new();
            };
            let base = git::base_da_tarefa(&origem, &copia, &base, remoto_base.as_deref());
            git::arquivos_acrescentados(&copia, &base, &t.arquivos_copiados)
        })
    }

    /// A aba PR ficou visível ou sumiu: com ela à vista, o vigia consulta mesmo o PR sem acompanhar, e
    /// a simulação com a base é refeita.
    pub fn vigiar_aba_pr(&mut self, id: u64, visivel: bool, cx: &mut Context<Self>) {
        if !visivel {
            self.prs.abas.remove(&id);
            return;
        }
        if self.prs.abas.insert(id) {
            if let Some(repo) = self.pr_da_tarefa(id).map(|p| p.repositorio.clone()) {
                self.prs.vigia.na_hora(&repo);
            }
            self.simular(id, cx);
        }
    }

    /// O ⟳ da aba PR (acréscimo do G5 à 10.1, com a assinatura dele): o detalhe, os checks e a linha
    /// do tempo do PR que a aba mostra (o atual ou um anterior) de novo, a consulta do vigia na hora
    /// e, com o PR atual, a simulação com a base.
    pub fn atualizar_pr(&mut self, id: u64, repo: &str, n: u64, cx: &mut Context<Self>) {
        let chave = (repo.to_string(), n);
        self.prs.detalhes.remove(&chave);
        self.prs.checks.remove(&chave);
        self.prs.linhas.remove(&chave);
        if comum::repositorio_valido(repo) {
            self.prs.vigia.na_hora(repo);
        }
        if self.pr_da_tarefa(id).is_some_and(|p| p.repositorio == repo && p.numero == n) {
            self.prs.simulacoes.remove(&id);
            self.simular(id, cx);
        }
        cx.notify();
    }

    /// O Vitral está publicando ou mesclando o PR da tarefa agora (o `Ocupado` da tabela da aba PR).
    pub fn andamento_do_pr(&self, id: u64) -> Option<AndamentoPr> {
        if self.prs.publicando.contains(&id) {
            Some(AndamentoPr::Publicando)
        } else if self.prs.mesclando.contains(&id) {
            Some(AndamentoPr::Mesclando)
        } else {
            None
        }
    }

    /// Tira um item de "Esperando você" do PR (acréscimo à API da 10.1: o × do item).
    pub fn dispensar_espera_do_pr(&mut self, id: u64, item: &EsperaPr, cx: &mut Context<Self>) {
        self.tirar_espera(id, |e| e == item);
        self.salvar();
        cx.notify();
    }

    // ---------- ações (seção 10.1) ----------

    pub fn publicar_pr_pelo_usuario(&mut self, id: u64, pedido: PedidoPr, cx: &mut Context<Self>) -> Receiver<Resposta> {
        self.publicar_pr(id, pedido, OrigemAcao::Usuario, cx)
    }

    pub fn mesclar_pelo_usuario(&mut self, id: u64, metodo: Option<String>, confirmado: Vec<Confirmado>, cx: &mut Context<Self>) -> Receiver<Resposta> {
        self.tirar_espera(id, |e| matches!(e, EsperaPr::ProntoParaMesclar));
        self.mesclar_pr(id, metodo, false, OrigemAcao::Usuario, confirmado, cx)
    }

    pub fn armar_mescla(&mut self, id: u64, ligar: bool, metodo: Option<String>, cx: &mut Context<Self>) {
        if ligar {
            let _ = self.mesclar_pr(id, metodo, true, OrigemAcao::Usuario, Vec::new(), cx);
        } else {
            self.desarmar(id, cx);
            cx.notify();
        }
    }

    pub fn acompanhar(&mut self, id: u64, ligar: bool, cx: &mut Context<Self>) {
        if !ligar {
            self.desarmar(id, cx);
        }
        if let Some(pr) = self.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
            pr.acompanhando = ligar;
            if !ligar {
                pr.esperando.clear();
            }
        }
        self.salvar();
        cx.notify();
    }

    /// Uma ação do gh no PR aberto da tarefa, com a consulta do vigia logo depois.
    fn acao_no_pr(
        &mut self,
        id: u64,
        feito: impl FnOnce(&Pr) -> String + 'static,
        acao: impl FnOnce(&Gh, &Pr) -> Result<(), ErroRemoto> + Send + 'static,
        cx: &mut Context<Self>,
    ) -> Receiver<Resposta> {
        let Some(pr) = self.pr_da_tarefa(id).cloned().or_else(|| {
            let a = self.prs_anteriores(id).last()?.clone();
            Some(Pr { repositorio: a.repositorio, numero: a.numero, url: a.url, titulo: a.titulo, estado: a.estado, ..Default::default() })
        }) else {
            return respondido(Resposta::erro("Este chat não tem PR."));
        };
        let (envia, recebe) = smol::channel::bounded(1);
        let pasta = self.dados.pasta.clone();
        let (pr_bg, repo) = (pr.clone(), pr.repositorio.clone());
        self.em_segundo_plano(
            Provedor::Github,
            true,
            move |e| acao(&Gh { e, pasta: &pasta }, &pr_bg),
            move |m, r, _| {
                m.anotar_rede(Provedor::Github, r.as_ref().err());
                let resposta = match r {
                    Ok(()) => Resposta::ok(feito(&pr)),
                    Err(erro) => Resposta::erro(erro.para_usuario("GitHub")),
                };
                m.prs.vigia.depois_do_envio(&repo, Instant::now());
                m.prs.vigia.na_hora(&repo);
                let _ = envia.try_send(resposta);
            },
            cx,
        );
        recebe
    }

    pub fn rascunho(&mut self, id: u64, rascunho: bool, cx: &mut Context<Self>) -> Receiver<Resposta> {
        self.acao_no_pr(
            id,
            move |pr| if rascunho { format!("O PR #{} voltou para rascunho.", pr.numero) } else { format!("O PR #{} está pronto para revisão.", pr.numero) },
            move |gh, pr| gh.pronto_para_revisao(&pr.repositorio, pr.numero, rascunho),
            cx,
        )
    }

    pub fn reabrir_pr(&mut self, id: u64, cx: &mut Context<Self>) -> Receiver<Resposta> {
        // O fechado volta na consulta seguinte (o vigia vê o último fechado de um chat sem PR).
        self.acao_no_pr(id, |pr| format!("O PR #{} foi reaberto.", pr.numero), |gh, pr| gh.reabrir_pr(&pr.repositorio, pr.numero), cx)
    }

    pub fn fechar_pr(&mut self, id: u64, comentario: Option<String>, cx: &mut Context<Self>) -> Receiver<Resposta> {
        self.desarmar(id, cx);
        self.acao_no_pr(
            id,
            |pr| format!("O PR #{} foi fechado.", pr.numero),
            move |gh, pr| gh.fechar_pr(&pr.repositorio, pr.numero, comentario.as_deref()),
            cx,
        )
    }

    pub fn editar_titulo_pr(&mut self, id: u64, titulo: String, cx: &mut Context<Self>) -> Receiver<Resposta> {
        let titulo = titulo.trim().to_string();
        if titulo.is_empty() {
            return respondido(Resposta::erro("O título não pode ficar vazio."));
        }
        if let Some(pr) = self.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
            pr.titulo = titulo.clone();
        }
        self.salvar();
        self.acao_no_pr(id, |pr| format!("Título do PR #{} mudado.", pr.numero), move |gh, pr| gh.editar_pr(&pr.repositorio, pr.numero, Some(&titulo), None), cx)
    }

    /// "Mandar ao chat" de uma falha do CI (`link`: só aquele check): a rodada é do usuário e, como
    /// o texto é do CI, de terceiros.
    pub fn mandar_falha_ao_chat(&mut self, id: u64, link: Option<String>, cx: &mut Context<Self>) -> Receiver<Resposta> {
        let Some(pr) = self.tarefa(id).and_then(|t| t.pr_aberto()).cloned() else {
            return respondido(Resposta::erro("Este chat não tem PR aberto."));
        };
        let (envia, recebe) = smol::channel::bounded(1);
        let pasta = self.dados.pasta.clone();
        let ship = pr.armado();
        self.em_segundo_plano(
            Provedor::Github,
            true,
            move |e| trabalho::ler_falha(e, &pasta, &pr, link.as_deref(), false, ship),
            move |m, r, cx| {
                m.anotar_rede(Provedor::Github, r.as_ref().err());
                let resposta = match r {
                    Err(erro) => Resposta::erro(erro.para_usuario("GitHub")),
                    Ok(FalhaLida { texto: None, .. }) => Resposta::erro("A execução ainda está rodando; mande quando ela terminar."),
                    Ok(FalhaLida { texto: Some(texto), chave, .. }) => {
                        m.tirar_espera(id, |e| matches!(e, EsperaPr::CiFalhou { .. }));
                        if let Some(pr) = m.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
                            pr.falha_avisada = Some(chave);
                        }
                        m.pedir_ajuste_de(id, texto, "usuário", "", "", OrigemAjuste::Usuario, Some(GatilhoAjuste::Ci), cx)
                    }
                };
                m.salvar();
                let _ = envia.try_send(resposta);
            },
            cx,
        );
        recebe
    }

    /// "Mandar ao chat" de comentários do PR: só os escolhidos. Um de quem não tem acesso deixa a
    /// rodada sem publicação sozinha.
    pub fn mandar_comentarios_ao_chat(&mut self, id: u64, ids: Vec<u64>, cx: &mut Context<Self>) -> Resposta {
        let Some(pr) = self.tarefa(id).and_then(|t| t.pr_aberto()).cloned() else {
            return Resposta::erro("Este chat não tem PR aberto.");
        };
        let guardados = self.prs.comentarios.get(&(pr.repositorio.clone(), pr.numero)).cloned().unwrap_or_default();
        let escolhidos: Vec<ComentarioPr> = guardados.into_iter().filter(|c| ids.contains(&c.id)).collect();
        if escolhidos.is_empty() {
            return Resposta::erro("Os comentários ainda não foram carregados: abra a aba PR e tente de novo.");
        }
        let terceiro = escolhidos.iter().any(|c| c.confianca() != Confianca::Confiavel);
        let texto = sem_segredos(&prompts::revisao_do_pr(pr.numero, &escolhidos, &[]));
        self.tirar_espera(id, |e| match e {
            EsperaPr::Terceiro { id: c, .. } => ids.contains(c),
            EsperaPr::Revisao { ids: lista, .. } => lista.iter().all(|c| ids.contains(c)),
            _ => false,
        });
        if let Some(pr) = self.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
            regras::marcar_entregues(pr, &ids);
        }
        let gatilho = if terceiro { GatilhoAjuste::Terceiro } else { GatilhoAjuste::Revisao };
        let resposta = self.pedir_ajuste_de(id, texto, "usuário", "", "", OrigemAjuste::Usuario, Some(gatilho), cx);
        self.salvar();
        resposta
    }

    pub fn rodar_checks_de_novo(&mut self, id: u64, alvo: RodarDeNovo, cx: &mut Context<Self>) -> Receiver<Resposta> {
        let Some(pr) = self.pr_da_tarefa(id).cloned() else {
            return respondido(Resposta::erro("Este chat não tem PR."));
        };
        let checks = self.prs.checks.get(&(pr.repositorio.clone(), pr.numero)).and_then(|(_, c)| c.valor().cloned()).unwrap_or_default();
        let execucoes: Vec<(u64, Option<u64>)> = match alvo {
            RodarDeNovo::Falhas => {
                let mut runs: Vec<u64> = regras::falhos(&checks)
                    .filter_map(|c| match crate::github::origem_do_link(&c.link) {
                        Some(crate::github::OrigemDoLog::Job { execucao, .. }) => Some(execucao),
                        _ => None,
                    })
                    .collect();
                runs.sort_unstable();
                runs.dedup();
                runs.into_iter().map(|r| (r, None)).collect()
            }
            RodarDeNovo::Job(job) => checks
                .iter()
                .filter_map(|c| match crate::github::origem_do_link(&c.link) {
                    Some(crate::github::OrigemDoLog::Job { execucao, job: j }) if j == job => Some((execucao, Some(job))),
                    _ => None,
                })
                .take(1)
                .collect(),
        };
        if execucoes.is_empty() {
            return respondido(Resposta::erro("Não achei execução do Actions para rodar de novo (abra os checks antes)."));
        }
        self.prs.checks.remove(&(pr.repositorio.clone(), pr.numero));
        self.acao_no_pr(
            id,
            |pr| format!("Os checks do PR #{} estão rodando de novo.", pr.numero),
            move |gh, pr| {
                for (execucao, job) in execucoes {
                    gh.rodar_de_novo(&pr.repositorio, execucao, job)?;
                }
                Ok(())
            },
            cx,
        )
    }

    /// "Atualizar com main": a base entra na cópia e vai para o PR (o `sha_publicado` é o HEAD novo).
    pub fn atualizar_com_base(&mut self, id: u64, cx: &mut Context<Self>) -> Receiver<Resposta> {
        self.git_no_pr(id, false, cx)
    }

    /// "Trazer as mudanças do GitHub": o ramo do PR entra na cópia (os commits viram "de fora").
    pub fn trazer_do_github(&mut self, id: u64, cx: &mut Context<Self>) -> Receiver<Resposta> {
        self.git_no_pr(id, true, cx)
    }

    fn git_no_pr(&mut self, id: u64, do_github: bool, cx: &mut Context<Self>) -> Receiver<Resposta> {
        let Some(t) = self.tarefa(id).cloned() else {
            return respondido(Resposta::erro("Tarefa não encontrada."));
        };
        let Some(pr) = t.pr_aberto().cloned() else {
            return respondido(Resposta::erro("Este chat não tem PR aberto."));
        };
        if let Some(motivo) = self.ocupado_para_o_pr(&t, OrigemAcao::Usuario) {
            return respondido(Resposta::erro(motivo));
        }
        let (Some(copia), Some((remoto, base))) = (t.worktree.clone().filter(|c| c.is_dir()), self.remoto_e_base(&t)) else {
            return respondido(Resposta::erro("A cópia deste chat não está disponível."));
        };
        let origem = self.origem_da_tarefa(&t);
        let e = self.executor();
        let (envia, recebe) = smol::channel::bounded(1);
        self.prs.publicando.insert(id);
        cx.notify();
        let ramo = pr.ramo.clone();
        cx.spawn(async move |motor, cx| {
            let (copia_bg, origem_bg, remoto_bg, base_bg, ramo_bg) = (copia.clone(), origem.clone(), remoto.clone(), base.clone(), ramo.clone());
            let r = cx
                .background_spawn(async move {
                    if do_github {
                        trabalho::trazer_do_github(&*e, &copia_bg, &origem_bg, &remoto_bg, &ramo_bg).await.map(Err)
                    } else {
                        trabalho::atualizar_com_base(&*e, &copia_bg, &origem_bg, &remoto_bg, &base_bg, &ramo_bg).await.map(Ok)
                    }
                })
                .await;
            let _ = motor.update(cx, |m, cx| {
                m.prs.publicando.remove(&id);
                m.prs.simulacoes.remove(&id);
                let chat = m.tarefa(id).map(|t| t.trabalhador.clone()).unwrap_or_default();
                let n = pr.numero;
                let resposta = match r {
                    Ok(Ok(Some(sha))) => {
                        if let Some(pr) = m.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
                            pr.sha = sha.clone();
                            pr.sha_publicado = sha;
                            pr.empurrado_em = agora();
                            pr.atualizado.clear();
                        }
                        m.prs.vigia.depois_do_envio(&pr.repositorio, Instant::now());
                        Resposta::ok(format!("O PR #{n} foi atualizado com {base}."))
                    }
                    Ok(Ok(None)) => Resposta::ok(format!("O PR #{n} já está em dia com {base}.")),
                    Ok(Err(commits)) => {
                        if let Some(pr) = m.tarefa_mut(id).and_then(|t| t.pr.as_mut()) {
                            for c in &commits {
                                if !pr.commits_de_fora.contains(c) {
                                    pr.commits_de_fora.push(c.clone());
                                }
                            }
                            pr.erro = None;
                        }
                        Resposta::ok(format!("As mudanças do GitHub entraram na cópia ({} commits de fora).", commits.len()))
                    }
                    Err(FalhaPublicacao::Conflito { alvo, arquivos }) => {
                        m.entregar(&chat, Some("Vitral".into()), prompts::conflito_com_a_base(&alvo, &arquivos), cx);
                        Resposta::erro(format!("Deu conflito com {alvo} ({}); o chat foi avisado para resolver.", arquivos.join(", ")))
                    }
                    Err(falha) => Resposta::erro(falha.texto()),
                };
                let _ = envia.try_send(resposta);
                m.salvar();
                cx.notify();
            });
        })
        .detach();
        recebe
    }

    /// "Recomeçar a partir de origin/main": a cópia passa a um ramo novo a partir da base, com as
    /// mudanças sem commit de volta (o histórico de antes fica no ref de backup).
    pub fn recomecar_do_main(&mut self, id: u64, cx: &mut Context<Self>) -> Receiver<Resposta> {
        let Some(t) = self.tarefa(id).cloned() else {
            return respondido(Resposta::erro("Tarefa não encontrada."));
        };
        if self.processos.contains_key(&t.trabalhador) || self.na_fila(&t.trabalhador) > 0 {
            return respondido(Resposta::erro("Esperando o chat terminar"));
        }
        let (Some(copia), Some((remoto, base))) = (t.worktree.clone().filter(|c| c.is_dir()), self.remoto_e_base(&t)) else {
            return respondido(Resposta::erro("A cópia deste chat não está disponível."));
        };
        let origem = self.origem_da_tarefa(&t);
        let config = self.config_da_pasta(&t.projeto, &origem).github;
        let sugerido = comum::ramo_da_tarefa(id, t.issue.as_ref(), None, &config.prefixo_ramo, &t.titulo, &|_| false);
        let existentes = git::nomes_de_ramos(&origem);
        let ramo_novo = regras::ramo_depois_do_pr(&sugerido, t.branch.as_deref().unwrap_or_default(), t.rodada, &|r| existentes.contains(r));
        let (excluir, grupo) = (t.arquivos_copiados.clone(), t.grupo());
        let e = self.executor();
        let (envia, recebe) = smol::channel::bounded(1);
        cx.spawn(async move |motor, cx| {
            let (copia_bg, ramo_bg, remoto_bg, base_bg) = (copia.clone(), ramo_novo.clone(), remoto.clone(), base.clone());
            let r = cx
                .background_spawn(async move {
                    {
                        let _rede = git::na_vez_da_rede(&origem).await;
                        git::buscar(&*e, &copia_bg, &remoto_bg, &[&base_bg]).map_err(|e| e.para_usuario("GitHub"))?;
                    }
                    let head = git::cabeca(&copia_bg).unwrap_or_default();
                    git::recomecar_da_base(&copia_bg, &remoto_bg, &base_bg, &head, &ramo_bg, &excluir, &grupo).map_err(|e| format!("{e:#}"))
                })
                .await;
            let _ = motor.update(cx, |m, cx| {
                let resposta = match r {
                    Ok(git::Recomeco::Feito { ramo, base: ponto, .. }) => {
                        if let Some(t) = m.tarefa_mut(id) {
                            t.branch = Some(ramo.clone());
                            t.ramo_planejado = Some(ramo.clone());
                            t.base = Some(ponto);
                            t.recomeco_pendente = false;
                        }
                        Resposta::ok(format!("A cópia agora está em `{ramo}`, a partir de {remoto}/{base}."))
                    }
                    Ok(git::Recomeco::Conflito { arquivos }) => Resposta::erro(format!(
                        "As mudanças sem commit não couberam em cima de {remoto}/{base} (conflito em {}); a cópia ficou como estava.",
                        arquivos.join(", ")
                    )),
                    Ok(git::Recomeco::FaltamCommits) => Resposta::erro("A cópia ficou como estava."),
                    Err(erro) => Resposta::erro(sem_segredos(&erro)),
                };
                let _ = envia.try_send(resposta);
                m.salvar();
                cx.notify();
            });
        })
        .detach();
        recebe
    }

    /// "Fechar a issue #45" do PR mesclado sem `Closes`.
    pub fn fechar_issue_do_pr(&mut self, id: u64, cx: &mut Context<Self>) -> Receiver<Resposta> {
        let Some(a) = self.prs_anteriores(id).iter().rev().find(|a| a.fechar_issue.is_some()).cloned() else {
            return respondido(Resposta::ok("Nenhuma issue para fechar."));
        };
        let numero = a.fechar_issue.unwrap_or_default();
        let (envia, recebe) = smol::channel::bounded(1);
        let (repo, pasta) = (a.repositorio.clone(), self.dados.pasta.clone());
        self.em_segundo_plano(
            Provedor::Github,
            true,
            move |e| Gh { e, pasta: &pasta }.fechar_issue(&repo, numero, false),
            move |m, r, _| {
                m.anotar_rede(Provedor::Github, r.as_ref().err());
                let resposta = match r {
                    Ok(()) => {
                        if let Some(a) = m.tarefa_mut(id).and_then(|t| t.prs_anteriores.iter_mut().rev().find(|x| x.numero == a.numero)) {
                            a.fechar_issue = None;
                            a.issue_concluida = true;
                        }
                        Resposta::ok(format!("A issue #{numero} foi fechada."))
                    }
                    Err(erro) => Resposta::erro(erro.para_usuario("GitHub")),
                };
                m.salvar();
                let _ = envia.try_send(resposta);
            },
            cx,
        );
        recebe
    }

    // ---------- confirmações ----------

    /// "Confirmar" ou "Agora não" num cartão `Confirmar`: a ação roda como do usuário.
    pub fn decidir_confirmacao(&mut self, cartao: u64, aceitar: bool, cx: &mut Context<Self>) {
        let Some(c) = self.cartoes.iter().find(|c| c.id == cartao && c.aberto()).cloned() else {
            return;
        };
        let TipoCartao::Confirmar(confirmacao) = c.tipo else {
            return;
        };
        if let Some(c) = self.cartoes.iter_mut().find(|c| c.id == cartao) {
            c.estado = if aceitar { EstadoCartao::Aceito } else { EstadoCartao::Recusado };
            c.decidido_em = agora();
        }
        if !aceitar {
            self.resultado_do_cartao(cartao, "Agora não.".into(), &confirmacao.pedido_por, cx);
            self.gravar_cartoes();
            cx.notify();
            return;
        }
        let recebe = match &confirmacao.acao {
            AcaoExterna::AbrirPr { tarefa, titulo, descricao, rascunho, quando_pronto } => {
                let pedido = PedidoPr {
                    titulo: Some(titulo.clone()),
                    descricao: Some(descricao.clone()),
                    rascunho: *rascunho,
                    quando_pronto: quando_pronto.clone(),
                    segredos_confirmados: false,
                };
                self.publicar_pr(*tarefa, pedido, OrigemAcao::Usuario, cx)
            }
            AcaoExterna::AtualizarPr { tarefa } => self.publicar_pr(*tarefa, PedidoPr::default(), OrigemAcao::Usuario, cx),
            AcaoExterna::Mesclar { tarefa, metodo, sha, quando_pronto } => {
                let confirmado = if sha.is_empty() { Vec::new() } else { vec![Confirmado::CommitsDeFora(sha.clone())] };
                self.mesclar_pr(*tarefa, Some(metodo.clone()), *quando_pronto, OrigemAcao::Usuario, confirmado, cx)
            }
        };
        let pedido_por = confirmacao.pedido_por.clone();
        cx.spawn(async move |motor, cx| {
            let Ok(resposta) = recebe.recv().await else {
                return;
            };
            let _ = motor.update(cx, |m, cx| {
                m.resultado_do_cartao(cartao, resposta.texto, &pedido_por, cx);
                m.gravar_cartoes();
                cx.notify();
            });
        })
        .detach();
        self.gravar_cartoes();
        cx.notify();
    }

    /// O que aconteceu com a confirmação: no cartão e, como nota, para quem pediu.
    fn resultado_do_cartao(&mut self, cartao: u64, texto: String, pedido_por: &str, cx: &mut Context<Self>) {
        if let Some(c) = self.cartoes.iter_mut().find(|c| c.id == cartao)
            && let TipoCartao::Confirmar(k) = &mut c.tipo
        {
            k.resultado = Some(texto.clone());
        }
        if self.chat(pedido_por).is_some() {
            let nota = Entrada { de: Some("Vitral".into()), texto: format!("O usuário decidiu o cartão {cartao}: {texto}"), nota: true, ..Default::default() };
            self.entregar_entrada(pedido_por, nota, cx);
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn apaga_o_ramo_so_quando_o_vitral_mesclou() {
        let info = InfoRepo { apaga_ramo_ao_mesclar: false, ..Default::default() };
        let pr = Pr { ramo: "vitral/t1-x".into(), cabeca: "vitral/t1-x".into(), ..Default::default() };
        let pelo_vitral = PrAnterior { metodo: Some("squash".into()), ..Default::default() };
        assert!(apagar_ramo_depois(&pelo_vitral, &pr, Some(&info)));
        assert!(!apagar_ramo_depois(&PrAnterior::default(), &pr, Some(&info)), "mesclado por outra pessoa");
        assert!(!apagar_ramo_depois(&pelo_vitral, &pr, Some(&InfoRepo { apaga_ramo_ao_mesclar: true, ..Default::default() })));
        assert!(!apagar_ramo_depois(&pelo_vitral, &Pr { cabeca: "eu:vitral/t1-x".into(), ..pr.clone() }, Some(&info)), "fork");
        assert!(!apagar_ramo_depois(&pelo_vitral, &pr, None), "sem saber do repositório, não");
    }

    #[test]
    fn corpo_e_commit_citam_a_issue() {
        use crate::issues::IssueLigada;
        let issue = IssueLigada { fonte: FonteIssue::Github, chave: "#45".into(), titulo: "Login".into(), ..Default::default() };
        let t = Tarefa { objetivo: "Corrigir o login".into(), issue: Some(issue), ..Default::default() };
        let corpo = corpo_da_tarefa(&t, "");
        assert!(corpo.starts_with("## O que muda\nCorrigir o login\n"), "{corpo}");
        assert!(corpo.ends_with("Closes #45\n"));
        assert_eq!(mensagem_do_commit("Corrige o login (#45)", &t), "Corrige o login (#45)\n\nRefs #45");
    }

    #[test]
    fn linha_do_tempo_agrupa_os_bots_e_marca_quem_nao_tem_acesso() {
        use crate::github::CommitPr;
        let d = DetalhePr {
            commits: vec![CommitPr { sha: "2222222abc".into(), titulo: "do site".into(), autores: vec!["x".into()], data: "2026-10-27T10:00:00Z".into() }],
            ..Default::default()
        };
        let c = |id: u64, autor: &str, assoc: &str, quando: &str| ComentarioPr {
            id,
            autor: autor.into(),
            associacao: assoc.into(),
            tipo_autor: if autor.ends_with("[bot]") { "Bot".into() } else { "User".into() },
            corpo: "oi".into(),
            criado: quando.into(),
            ..Default::default()
        };
        let comentarios = [
            c(1, "cloudflare-workers-and-pages[bot]", "NONE", "2026-10-27T11:00:00Z"),
            c(2, "cloudflare-workers-and-pages[bot]", "NONE", "2026-10-27T12:00:00Z"),
            c(3, "estranho", "NONE", "2026-10-27T13:00:00Z"),
        ];
        let linha = linha_do_tempo_de(&d, &comentarios, &["2222222".into()], &[]);
        assert_eq!(linha.len(), 3);
        assert!(matches!(&linha[0], EventoLinha::Comentario { id: 3, sem_acesso: true, .. }));
        assert_eq!(linha[1], EventoLinha::Bots { login: "cloudflare-workers-and-pages[bot]".into(), quantos: 2 });
        assert!(matches!(&linha[2], EventoLinha::Commit { de_fora: true, .. }));
    }
}
