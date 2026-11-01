//! GitHub e Linear no motor, a parte das conexões e da configuração: quem está conectado (o login
//! do gh, a conta do Linear e a chave na memória), o isolamento dos chats, o repositório de cada
//! pasta ligada e os times do Linear. A interface só lê o que está guardado aqui; a leitura que
//! falta (ou que passou da validade) pede a busca, que o motor começa no sinal seguinte, em segundo
//! plano e numa fila por provedor. As ações do usuário devolvem um canal com o resultado.
//!
//! A chave do Linear fica só no chaveiro e em `chave_linear` (`Zeroizing`). Na abertura, com a
//! janela em foco, ela é lida sem destravar; o resto do Vitral nunca destrava o chaveiro sozinho.

use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

use smol::channel::Receiver;

use super::*;
use crate::{
    github::{EstadoLogin, Gh, InfoRepo, LoginGh},
    isolamento, issues,
    issues::{EstadoIssue, TipoEstado},
    linear::{Curl, DadosDoTime, Linear, Time, Transporte},
    modelo::{ConfigGithub, ContaLinear, LinearDoProjeto},
    remoto::{self, ErroRemoto, EstadoRede, Executor, Rede},
    segredos::{self, Chave, ErroChaveiro},
};

/// Quanto vale o que veio do GitHub e do Linear antes de buscar de novo (repositório, ramos,
/// times e os estados de cada time).
const VALIDADE: Duration = Duration::from_secs(3600);
/// Depois de um erro, a leitura só pede a busca de novo depois disso (sem laço de redesenho).
const ESPERA_DO_ERRO: Duration = Duration::from_secs(60);
/// O comando que "Entrar no GitHub" abre no terminal.
pub const ENTRAR_NO_GITHUB: &str = "gh auth login --web --git-protocol https";
/// O que o "Pedir escopos" roda: só o `repo` (o Vitral nunca pede o `workflow`).
pub const PEDIR_ESCOPOS: &str = "gh auth refresh -h github.com -s repo";

/// Uma leitura que vem da rede, como a interface desenha.
#[derive(Clone, Debug, PartialEq)]
pub enum Carregando<T> {
    Nada,
    /// Buscando; com o valor de antes, se havia (a interface desenha ele enquanto isso).
    Carregando(Option<T>),
    Pronto(T, Instant),
    /// O texto já sem segredos, com o valor de antes, se havia.
    Erro(String, Option<T>),
}

impl<T> Carregando<T> {
    pub fn valor(&self) -> Option<&T> {
        match self {
            Carregando::Pronto(v, _) | Carregando::Carregando(Some(v)) | Carregando::Erro(_, Some(v)) => Some(v),
            _ => None,
        }
    }

    #[allow(dead_code)] // Provisório: o popover das issues e a aba PR (G3b e G5).
    pub fn carregando(&self) -> bool {
        matches!(self, Carregando::Carregando(_))
    }

    pub fn erro(&self) -> Option<&str> {
        match self {
            Carregando::Erro(e, _) => Some(e),
            _ => None,
        }
    }

    /// A busca começou: fica o valor que havia.
    pub(super) fn buscando(self) -> Self {
        match self {
            Carregando::Pronto(v, _) | Carregando::Carregando(Some(v)) | Carregando::Erro(_, Some(v)) => Carregando::Carregando(Some(v)),
            _ => Carregando::Carregando(None),
        }
    }

    /// O resultado de uma busca: o erro guarda o valor de antes.
    pub(super) fn com(self, resultado: Result<T, String>, agora: Instant) -> Self {
        match resultado {
            Ok(v) => Carregando::Pronto(v, agora),
            Err(e) => Carregando::Erro(e, self.tirar()),
        }
    }

    pub(super) fn tirar(self) -> Option<T> {
        match self {
            Carregando::Pronto(v, _) | Carregando::Carregando(Some(v)) | Carregando::Erro(_, Some(v)) => Some(v),
            _ => None,
        }
    }
}

/// Uma leitura guardada e quando ela mudou de estado.
type Guardado<T> = (Instant, Carregando<T>);

/// Precisa buscar: nunca buscou, o que tem passou da validade, ou o erro já esperou o bastante.
/// Uma busca em andamento nunca é pedida de novo.
pub fn precisa_buscar<T>(guardado: Option<&Guardado<T>>, agora: Instant) -> bool {
    match guardado {
        None | Some((_, Carregando::Nada)) => true,
        Some((_, Carregando::Carregando(_))) => false,
        Some((_, Carregando::Pronto(_, quando))) => agora.saturating_duration_since(*quando) >= VALIDADE,
        Some((desde, Carregando::Erro(..))) => agora.saturating_duration_since(*desde) >= ESPERA_DO_ERRO,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provedor {
    Github,
    Linear,
}

/// O GitHub, como a tela de Conexões mostra.
#[derive(Clone, Debug, PartialEq)]
pub enum EstadoGithub {
    /// A primeira consulta ainda não voltou.
    Conferindo,
    /// O gh não está instalado: a parte de GitHub some do resto do app.
    SemGh,
    /// Sem conta ativa no github.com, ou com o token recusado (`state == error`, com o motivo).
    SemLogin(Option<String>),
    /// `state == timeout`: o gh não alcançou o GitHub agora.
    SemConexao,
    Conectado(LoginGh),
    /// O gh falhou de outro jeito (o texto já sem segredos).
    Erro(String),
}

/// A chave do Linear, como a tela de Conexões mostra.
#[derive(Clone, Debug, PartialEq)]
pub enum EstadoConexao {
    /// Nenhuma conta conectada.
    Desconectada,
    /// Conectada, e a chave ainda não foi lida (a janela abriu sem foco).
    NaoLida,
    Lendo,
    /// A chave está na memória.
    Pronta,
    /// O chaveiro está trancado: só "Destravar" (ação do usuário) lê a chave.
    ChaveiroTrancado,
    ChaveiroIndisponivel(String),
    /// A conta está na configuração, mas a chave não está mais no chaveiro.
    SemChave,
}

/// O que a tela de Conexões desenha.
#[derive(Clone, Debug, PartialEq)]
pub struct EstadoConexoes {
    pub github: EstadoGithub,
    pub linear: Option<ContaLinear>,
    pub chave: EstadoConexao,
    /// `None` enquanto o teste do bwrap roda.
    pub isolamento: Option<bool>,
}

/// Um remote do GitHub de uma pasta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemotoGithub {
    pub nome: String,
    /// "dono/nome".
    pub repositorio: String,
}

/// O que o Vitral usa de um repositório do GitHub (cache de 1 h).
#[derive(Clone, Debug, PartialEq)]
pub struct RepoGithub {
    pub info: InfoRepo,
    /// Quantos workflows do Actions ele tem (0 = sem CI); `None` se a consulta falhou.
    pub workflows: Option<u32>,
    /// O prefixo dos ramos dos PRs que você já abriu nele ("feature/vitor-").
    pub prefixo: Option<String>,
}

/// O que uma leitura `&self` pediu para buscar.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pedido {
    Remotos(PathBuf),
    Repo(String),
    Ramos(String),
    Times,
    DoTime(String),
}

/// O estado das conexões e o que veio do GitHub e do Linear (só na memória).
pub struct Integracao {
    executor: Arc<dyn Executor>,
    github: EstadoGithub,
    login_em_voo: bool,
    /// "Entrar no GitHub" abriu o terminal: ao voltar o foco, o login é conferido de novo.
    esperando_login: bool,
    chave_linear: Option<Arc<Chave>>,
    estado_chave: EstadoConexao,
    isolamento_disponivel: Option<bool>,
    /// Os remotes do GitHub de cada pasta ligada.
    remotos: HashMap<PathBuf, Guardado<Vec<RemotoGithub>>>,
    info_repos: HashMap<String, Guardado<RepoGithub>>,
    ramos: HashMap<String, Guardado<Vec<String>>>,
    times: Option<Guardado<Vec<Time>>>,
    do_time: HashMap<String, Guardado<DadosDoTime>>,
    rede: HashMap<Provedor, Rede>,
    filas_github: remoto::Filas,
    filas_linear: remoto::Filas,
    /// Buscas pedidas por leituras (`&self`): começam no próximo sinal.
    pedidos: RefCell<Vec<Pedido>>,
}

impl Integracao {
    pub fn new(executor: Arc<dyn Executor>) -> Self {
        Self {
            executor,
            github: EstadoGithub::Conferindo,
            login_em_voo: false,
            esperando_login: false,
            chave_linear: None,
            estado_chave: EstadoConexao::Desconectada,
            isolamento_disponivel: None,
            remotos: HashMap::new(),
            info_repos: HashMap::new(),
            ramos: HashMap::new(),
            times: None,
            do_time: HashMap::new(),
            rede: HashMap::new(),
            filas_github: remoto::Filas::default(),
            filas_linear: remoto::Filas::default(),
            pedidos: RefCell::new(Vec::new()),
        }
    }

    fn fila(&self, provedor: Provedor, do_usuario: bool) -> Arc<smol::lock::Semaphore> {
        let filas = match provedor {
            Provedor::Github => &self.filas_github,
            Provedor::Linear => &self.filas_linear,
        };
        if do_usuario { filas.usuario.clone() } else { filas.fundo.clone() }
    }

    fn rede_mut(&mut self, provedor: Provedor) -> &mut Rede {
        self.rede.entry(provedor).or_default()
    }
}

// ---------- funções puras ----------

/// O estado do GitHub a partir do `github::login()`.
pub fn estado_do_login(resultado: Result<Option<LoginGh>, ErroRemoto>) -> EstadoGithub {
    match resultado {
        Err(ErroRemoto::SemPrograma(_)) => EstadoGithub::SemGh,
        Err(ErroRemoto::SemLogin) => EstadoGithub::SemLogin(None),
        Err(erro) if erro.transitorio() => EstadoGithub::SemConexao,
        Err(erro) => EstadoGithub::Erro(erro.para_usuario("GitHub")),
        Ok(None) => EstadoGithub::SemLogin(None),
        Ok(Some(login)) => match &login.estado {
            EstadoLogin::Ok => EstadoGithub::Conectado(login),
            EstadoLogin::TempoEsgotado => EstadoGithub::SemConexao,
            EstadoLogin::Erro(motivo) => EstadoGithub::SemLogin(Some(motivo.clone()).filter(|m| !m.is_empty())),
        },
    }
}

/// Os métodos de mesclar que o repositório aceita, com o padrão da conta primeiro.
#[allow(dead_code)] // Provisório: mesclar (G4) e o menu da aba PR (G5).
pub fn metodos_permitidos(info: &InfoRepo) -> Vec<&'static str> {
    let mut metodos: Vec<&'static str> = [("squash", info.squash), ("merge", info.merge), ("rebase", info.rebase)]
        .into_iter()
        .filter(|(_, permitido)| *permitido)
        .map(|(metodo, _)| metodo)
        .collect();
    if let Some(i) = metodos.iter().position(|m| *m == info.metodo_padrao) {
        let padrao = metodos.remove(i);
        metodos.insert(0, padrao);
    }
    metodos
}

/// Se "Abrir PR" pode ser escolhido para a pasta, ou por que não. `info` é o repositório de
/// destino; `do_fork` é o PR do fork para o original, que só pede leitura nele (mesclar lá pede
/// escrita, conferida na hora de mesclar).
pub fn modo_pr_disponivel(info: &InfoRepo, login: Option<&LoginGh>, do_fork: bool) -> Result<(), String> {
    if !login.is_some_and(|l| l.estado == EstadoLogin::Ok) {
        return Err("Entre no GitHub em Configurações › Conexões".into());
    }
    if do_fork || info.pode_escrever() { Ok(()) } else { Err("Sua conta só lê este repositório".into()) }
}

/// Os estados do "Automático" de um time: o `started` de menor posição (ao iniciar um agente) e o
/// `completed` de menor posição (ao mesclar).
pub fn estados_padrao(d: &DadosDoTime) -> (Option<&EstadoIssue>, Option<&EstadoIssue>) {
    let menor = |tipo: TipoEstado| {
        d.estados.iter().filter(|e| e.estado.tipo == tipo).min_by(|a, b| a.posicao.total_cmp(&b.posicao)).map(|e| &e.estado)
    };
    (menor(TipoEstado::EmAndamento), menor(TipoEstado::Concluida))
}

/// O remote que vale para a pasta: o escolhido na configuração, senão o origin, senão o primeiro
/// do GitHub.
pub fn remoto_escolhido<'a>(remotos: &'a [RemotoGithub], config: &ConfigGithub) -> Option<&'a RemotoGithub> {
    let por_nome = |nome: &str| remotos.iter().find(|r| r.nome == nome);
    por_nome(config.remoto.trim()).or_else(|| por_nome("origin")).or_else(|| remotos.first())
}

/// Os remotes da pasta que apontam para o GitHub, com o "dono/nome" de cada um.
pub fn remotos_do_github(remotos: &[(String, String)]) -> Vec<RemotoGithub> {
    remotos
        .iter()
        .filter_map(|(nome, url)| {
            let (dono, repo) = issues::parse_remoto(url)?;
            Some(RemotoGithub { nome: nome.clone(), repositorio: format!("{dono}/{repo}") })
        })
        .collect()
}

/// O Linear do projeto com outro time: os estados escolhidos eram ids do time anterior e voltam
/// ao automático; a pasta lembrada fica.
pub fn linear_com_time(anterior: Option<LinearDoProjeto>, time: &Time) -> LinearDoProjeto {
    let anterior = anterior.unwrap_or_default();
    let mesmo = anterior.time_id == time.id;
    LinearDoProjeto {
        time_id: time.id.clone(),
        time_chave: time.chave.clone(),
        time_nome: time.nome.clone(),
        estado_iniciar: anterior.estado_iniciar.filter(|_| mesmo),
        estado_mesclar: anterior.estado_mesclar.filter(|_| mesmo),
        pasta: anterior.pasta,
    }
}

/// A frase do diálogo "Conectar o Linear" para cada erro do `eu()`.
pub fn erro_ao_conectar(erro: &ErroRemoto) -> String {
    match erro {
        ErroRemoto::SemLogin => "O Linear não aceitou esta chave. Confira se copiou inteira e se ela não foi revogada.".into(),
        ErroRemoto::SemPermissao(_) => "A chave não tem permissão de leitura.".into(),
        ErroRemoto::SemRede(_) | ErroRemoto::TempoEsgotado => "Sem conexão com o Linear. Tente de novo.".into(),
        outro => outro.para_usuario("Linear"),
    }
}

/// O motivo de um erro do chaveiro, para entrar no meio de uma frase.
fn motivo_do_chaveiro(erro: &ErroChaveiro) -> String {
    match erro {
        ErroChaveiro::Trancado => "ele continuou trancado".into(),
        ErroChaveiro::Indisponivel(motivo) => remoto::sem_segredos(motivo),
        ErroChaveiro::TempoEsgotado => "ele não respondeu a tempo".into(),
    }
}

/// A conta que o Vitral guarda (sem a chave: só os 4 últimos caracteres).
fn conta_de(eu: &crate::linear::Eu, chave: &Chave) -> ContaLinear {
    let nome = if eu.nome.trim().is_empty() { &eu.nome_exibicao } else { &eu.nome };
    ContaLinear {
        workspace: eu.workspace.clone(),
        url_key: eu.url_key.clone(),
        nome: nome.clone(),
        id_usuario: eu.id.clone(),
        final_da_chave: chave.final_(),
    }
}

/// O "Testar e salvar" sem o motor: valida a chave, confere com o Linear e só então grava no
/// chaveiro (`gravar`). Recusada, sem rede ou malformada, nada é gravado; e nunca em arquivo.
pub fn conectar_com(
    texto: &str,
    t: &dyn Transporte,
    gravar: &mut dyn FnMut(&Chave) -> Result<(), ErroChaveiro>,
) -> Result<(Chave, ContaLinear), String> {
    let chave = Chave::linear(texto).map_err(str::to_string)?;
    let eu = Linear { t, chave: &chave }.eu().map_err(|e| erro_ao_conectar(&e))?;
    gravar(&chave).map_err(|e| format!("Não consegui guardar no chaveiro do sistema ({}). A chave não foi salva.", motivo_do_chaveiro(&e)))?;
    let conta = conta_de(&eu, &chave);
    Ok((chave, conta))
}

/// Um canal que já tem a resposta.
fn respondido<T>(valor: T) -> Receiver<T> {
    let (envia, recebe) = smol::channel::bounded(1);
    let _ = envia.try_send(valor);
    recebe
}

// ---------- o motor ----------

impl Motor {
    /// Na abertura: o teste do bwrap, o login do gh e, com a janela em foco, a chave do Linear sem
    /// destravar o chaveiro.
    pub(super) fn iniciar_conexoes(&mut self, cx: &mut Context<Self>) {
        let e = self.integracao.executor.clone();
        cx.spawn(async move |motor, cx| {
            let ativo = cx.background_spawn(async move { isolamento::testar(&*e) }).await;
            let _ = motor.update(cx, |m, cx| {
                m.integracao.isolamento_disponivel = Some(ativo);
                cx.notify();
            });
        })
        .detach();
        self.login_github(cx);
        if self.config.conexoes.linear.is_some() {
            self.integracao.estado_chave = EstadoConexao::NaoLida;
            cx.spawn(async move |motor, cx| {
                let _ = motor.update(cx, |m, cx| {
                    if m.janela_ativa {
                        m.ler_chave(false, None, cx);
                    }
                });
            })
            .detach();
        }
    }

    /// A janela voltou: confere o login depois de "Entrar no GitHub" e lê a chave que ainda não
    /// foi lida (sem destravar).
    pub(super) fn conexoes_ao_voltar(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.integracao.esperando_login) || self.integracao.github == EstadoGithub::SemConexao {
            self.login_github(cx);
        }
        if self.integracao.estado_chave == EstadoConexao::NaoLida {
            self.ler_chave(false, None, cx);
        }
    }

    pub fn conexoes(&self) -> EstadoConexoes {
        EstadoConexoes {
            github: self.integracao.github.clone(),
            linear: self.config.conexoes.linear.clone(),
            chave: self.integracao.estado_chave.clone(),
            isolamento: self.integracao.isolamento_disponivel,
        }
    }

    /// O bwrap funciona aqui (falso também enquanto o teste roda).
    pub fn isolamento_disponivel(&self) -> bool {
        self.integracao.isolamento_disponivel == Some(true)
    }

    /// O login do gh agora; sem efeito com outra consulta em andamento.
    pub fn login_github(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.integracao.login_em_voo, true) {
            return;
        }
        let pasta = self.dados.pasta.clone();
        self.em_segundo_plano(
            Provedor::Github,
            false,
            move |e| Gh { e, pasta: &pasta }.login(),
            |m, resultado, _| {
                m.integracao.login_em_voo = false;
                m.anotar_rede(Provedor::Github, resultado.as_ref().err());
                m.integracao.github = estado_do_login(resultado);
            },
            cx,
        );
    }

    /// "Entrar no GitHub": o login do gh num terminal; ao voltar o foco, o Vitral confere.
    pub fn entrar_no_github(&mut self) {
        self.integracao.esperando_login = true;
        let casa = dirs::home_dir().unwrap_or_else(|| self.dados.pasta.clone());
        crate::sistema::abrir_terminal(&casa, None, Some(ENTRAR_NO_GITHUB));
    }

    /// "Pedir escopos": o `repo` que faltou, num terminal (o gh pede a confirmação no navegador).
    pub fn pedir_escopos_github(&mut self) {
        self.integracao.esperando_login = true;
        let casa = dirs::home_dir().unwrap_or_else(|| self.dados.pasta.clone());
        crate::sistema::abrir_terminal(&casa, None, Some(PEDIR_ESCOPOS));
    }

    /// "Testar e salvar" do diálogo: confere a chave com o Linear, grava no chaveiro e guarda a
    /// conta. Malformada, recusada ou sem rede, nada é gravado.
    pub fn conectar_linear(&mut self, chave: String, cx: &mut Context<Self>) -> Receiver<Result<ContaLinear, String>> {
        let texto = zeroize::Zeroizing::new(chave);
        // A validação local responde na hora, sem rede.
        if let Err(motivo) = Chave::linear(&texto) {
            return respondido(Err(motivo.to_string()));
        }
        let (envia, recebe) = smol::channel::bounded(1);
        self.em_segundo_plano(
            Provedor::Linear,
            true,
            move |e| {
                let mut gravar = |chave: &Chave| smol::block_on(segredos::gravar_chave_linear(chave));
                conectar_com(&texto, &Curl(e), &mut gravar)
            },
            move |m, resultado, _| {
                let resposta = resultado.map(|(chave, conta)| {
                    m.anotar_rede(Provedor::Linear, None);
                    m.integracao.chave_linear = Some(Arc::new(chave));
                    m.integracao.estado_chave = EstadoConexao::Pronta;
                    m.integracao.times = None;
                    m.integracao.do_time.clear();
                    m.config.conexoes.linear = Some(conta.clone());
                    m.gravar_config();
                    conta
                });
                let _ = envia.try_send(resposta);
            },
            cx,
        );
        recebe
    }

    /// "Testar": a chave da memória (ou do chaveiro, destravando: é ação do usuário) contra o
    /// Linear. Atualiza o nome e o workspace guardados.
    pub fn testar_linear(&mut self, cx: &mut Context<Self>) -> Receiver<Result<ContaLinear, String>> {
        let (envia, recebe) = smol::channel::bounded(1);
        match self.integracao.chave_linear.clone() {
            Some(chave) => self.testar_com(chave, envia, cx),
            None => self.ler_chave(
                true,
                Some(Box::new(move |m, cx| match m.integracao.chave_linear.clone() {
                    Some(chave) => m.testar_com(chave, envia, cx),
                    None => {
                        let _ = envia.try_send(Err(m.motivo_sem_chave()));
                    }
                })),
                cx,
            ),
        }
        recebe
    }

    fn testar_com(&mut self, chave: Arc<Chave>, envia: smol::channel::Sender<Result<ContaLinear, String>>, cx: &mut Context<Self>) {
        self.em_segundo_plano(
            Provedor::Linear,
            true,
            move |e| Linear { t: &Curl(e), chave: &chave }.eu().map(|eu| (eu, chave)),
            move |m, resultado, _| {
                m.anotar_rede(Provedor::Linear, resultado.as_ref().err());
                let resposta = match resultado {
                    Ok((eu, chave)) => {
                        let conta = conta_de(&eu, &chave);
                        m.config.conexoes.linear = Some(conta.clone());
                        m.gravar_config();
                        Ok(conta)
                    }
                    Err(erro) => Err(erro_ao_conectar(&erro)),
                };
                let _ = envia.try_send(resposta);
            },
            cx,
        );
    }

    /// "Desconectar" (já confirmado): a chave sai do chaveiro e da memória, e a conta da
    /// configuração. Sem conseguir tirar do chaveiro, nada muda (a chave continuaria lá).
    pub fn desconectar_linear(&mut self, cx: &mut Context<Self>) -> Receiver<Result<(), String>> {
        let (envia, recebe) = smol::channel::bounded(1);
        cx.spawn(async move |motor, cx| {
            let apagou = cx.background_spawn(async move { segredos::apagar_chave_linear().await }).await;
            let _ = motor.update(cx, |m, cx| {
                let resposta = match apagou {
                    Ok(()) => {
                        m.integracao.chave_linear = None;
                        m.integracao.estado_chave = EstadoConexao::Desconectada;
                        m.integracao.times = None;
                        m.integracao.do_time.clear();
                        m.config.conexoes.linear = None;
                        m.gravar_config();
                        Ok(())
                    }
                    Err(erro) => Err(format!("Não consegui tirar a chave do chaveiro ({}).", motivo_do_chaveiro(&erro))),
                };
                let _ = envia.try_send(resposta);
                cx.notify();
            });
        })
        .detach();
        recebe
    }

    /// "Destravar": lê a chave destravando o chaveiro (o sistema pede a senha).
    pub fn destravar_chaveiro(&mut self, cx: &mut Context<Self>) -> Receiver<Result<(), String>> {
        let (envia, recebe) = smol::channel::bounded(1);
        self.ler_chave(
            true,
            Some(Box::new(move |m, _| {
                let resposta = if m.integracao.chave_linear.is_some() { Ok(()) } else { Err(m.motivo_sem_chave()) };
                let _ = envia.try_send(resposta);
            })),
            cx,
        );
        recebe
    }

    /// Por que a chave não está na memória, para a interface.
    pub(super) fn motivo_sem_chave(&self) -> String {
        match &self.integracao.estado_chave {
            EstadoConexao::ChaveiroTrancado => "O chaveiro está trancado.".into(),
            EstadoConexao::ChaveiroIndisponivel(motivo) => format!("O chaveiro do sistema não respondeu ({motivo})."),
            EstadoConexao::SemChave => "A chave não está mais no chaveiro; conecte o Linear de novo.".into(),
            EstadoConexao::Desconectada => "Conecte o Linear em Configurações › Conexões.".into(),
            EstadoConexao::NaoLida | EstadoConexao::Lendo | EstadoConexao::Pronta => "O chaveiro ainda está sendo lido.".into(),
        }
    }

    /// Lê a chave do chaveiro. `destravar` só por ação do usuário; `depois` roda com o estado novo.
    #[allow(clippy::type_complexity)]
    pub(super) fn ler_chave(&mut self, destravar: bool, depois: Option<Box<dyn FnOnce(&mut Motor, &mut Context<Motor>)>>, cx: &mut Context<Self>) {
        if self.config.conexoes.linear.is_none() {
            self.integracao.estado_chave = EstadoConexao::Desconectada;
            if let Some(depois) = depois {
                depois(self, cx);
            }
            return;
        }
        self.integracao.estado_chave = EstadoConexao::Lendo;
        cx.spawn(async move |motor, cx| {
            let lida = cx.background_spawn(async move { segredos::ler_chave_linear(destravar).await }).await;
            let _ = motor.update(cx, |m, cx| {
                m.integracao.estado_chave = match lida {
                    Ok(Some(chave)) => {
                        m.integracao.chave_linear = Some(Arc::new(chave));
                        EstadoConexao::Pronta
                    }
                    Ok(None) => EstadoConexao::SemChave,
                    Err(ErroChaveiro::Trancado) => EstadoConexao::ChaveiroTrancado,
                    Err(erro) => EstadoConexao::ChaveiroIndisponivel(motivo_do_chaveiro(&erro)),
                };
                if let Some(depois) = depois {
                    depois(m, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ---------- leituras do GitHub e do Linear ----------

    /// Os remotes do GitHub da pasta (`Pronto` com a lista vazia: a pasta não tem GitHub).
    pub fn remotos_github(&self, pasta: &Path) -> Carregando<Vec<RemotoGithub>> {
        let guardado = self.integracao.remotos.get(pasta);
        if precisa_buscar(guardado, Instant::now()) {
            self.pedir(Pedido::Remotos(pasta.to_path_buf()));
        }
        guardado.map_or(Carregando::Nada, |(_, c)| c.clone())
    }

    /// O repositório do GitHub (informação, workflows e o prefixo dos seus PRs), com cache de 1 h.
    pub fn repo_github(&self, repo: &str) -> Carregando<RepoGithub> {
        let guardado = self.integracao.info_repos.get(repo);
        if issues::repositorio_valido(repo) && precisa_buscar(guardado, Instant::now()) {
            self.pedir(Pedido::Repo(repo.to_string()));
        }
        guardado.map_or(Carregando::Nada, |(_, c)| c.clone())
    }

    /// O repositório do GitHub da pasta, pelo remote que vale na configuração dela. `Nada` também
    /// numa pasta sem GitHub (ver `remotos_github`).
    #[allow(dead_code)] // API da seção 10.1; quem chama é o popover das issues (G3b).
    pub fn info_github(&self, pasta: &Path) -> Carregando<InfoRepo> {
        let Some(remotos) = self.remotos_github(pasta).tirar() else {
            return Carregando::Nada;
        };
        let projeto = self.projeto_da_pasta(pasta).unwrap_or_default();
        let config = self.config_da_pasta(&projeto, pasta).github;
        let Some(remoto) = remoto_escolhido(&remotos, &config) else {
            return Carregando::Nada;
        };
        match self.repo_github(&remoto.repositorio) {
            Carregando::Nada => Carregando::Nada,
            Carregando::Carregando(r) => Carregando::Carregando(r.map(|r| r.info)),
            Carregando::Pronto(r, quando) => Carregando::Pronto(r.info, quando),
            Carregando::Erro(e, r) => Carregando::Erro(e, r.map(|r| r.info)),
        }
    }

    /// O "dono/nome" para onde vão os PRs da pasta: o próprio repositório, ou o original de um
    /// fork com "Abrir os PRs em …" ligado.
    pub fn destino_do_pr(&self, pasta: &Path, config: &ConfigGithub) -> Option<String> {
        let remotos = self.remotos_github(pasta).tirar()?;
        let remoto = remoto_escolhido(&remotos, config)?;
        let info = self.repo_github(&remoto.repositorio).tirar().map(|r| r.info);
        match info {
            Some(info) if config.pr_no_original && info.fork => info.original.clone().or(Some(info.repositorio)),
            _ => Some(remoto.repositorio.clone()),
        }
    }

    /// O prefixo que os seus PRs anteriores no repositório da pasta sugerem ("feature/vitor-").
    #[allow(dead_code)] // API da seção 10.1; quem chama é o ramo do chat de uma issue (G3a).
    pub fn prefixo_sugerido(&self, pasta: &Path) -> Option<String> {
        let remotos = self.remotos_github(pasta).tirar()?;
        let projeto = self.projeto_da_pasta(pasta).unwrap_or_default();
        let remoto = remoto_escolhido(&remotos, &self.config_da_pasta(&projeto, pasta).github)?;
        self.repo_github(&remoto.repositorio).tirar()?.prefixo
    }

    /// Os ramos do repositório ("Ramo de destino" em Mais opções).
    pub fn ramos_do_repo(&self, repo: &str) -> Carregando<Vec<String>> {
        let guardado = self.integracao.ramos.get(repo);
        if issues::repositorio_valido(repo) && precisa_buscar(guardado, Instant::now()) {
            self.pedir(Pedido::Ramos(repo.to_string()));
        }
        guardado.map_or(Carregando::Nada, |(_, c)| c.clone())
    }

    /// Os times do workspace do Linear. Precisa da chave na memória: com o chaveiro trancado, o
    /// erro diz isso (a leitura nunca destrava).
    pub fn times_linear(&self) -> Carregando<Vec<Time>> {
        if self.integracao.chave_linear.is_none() {
            return match self.config.conexoes.linear {
                None => Carregando::Nada,
                Some(_) => Carregando::Erro(self.motivo_sem_chave(), None),
            };
        }
        let guardado = self.integracao.times.as_ref();
        if precisa_buscar(guardado, Instant::now()) {
            self.pedir(Pedido::Times);
        }
        guardado.map_or(Carregando::Nada, |(_, c)| c.clone())
    }

    /// Os estados do fluxo de um time (cache de 1 h).
    pub fn dados_do_time(&self, time: &str) -> Carregando<DadosDoTime> {
        if self.integracao.chave_linear.is_none() || time.is_empty() {
            return Carregando::Nada;
        }
        let guardado = self.integracao.do_time.get(time);
        if precisa_buscar(guardado, Instant::now()) {
            self.pedir(Pedido::DoTime(time.to_string()));
        }
        guardado.map_or(Carregando::Nada, |(_, c)| c.clone())
    }

    /// O time do Linear do projeto, gravado na hora (o chip "Time" do popover das issues).
    #[allow(dead_code)] // API da seção 10.1; quem chama é o chip "Time" do popover das issues (G3b).
    pub fn escolher_time(&mut self, projeto: &str, time: &Time, cx: &mut Context<Self>) {
        if self.somente_leitura.is_some() {
            return;
        }
        let Some(p) = self.projetos.iter_mut().find(|p| p.id == projeto) else {
            return;
        };
        p.linear = Some(linear_com_time(p.linear.take(), time));
        self.salvar();
        cx.notify();
    }

    #[allow(dead_code)] // API da seção 10.1; quem chama é a faixa "sem conexão" (G3b e G5).
    pub fn estado_rede(&self, p: Provedor) -> EstadoRede {
        self.integracao.rede.get(&p).map_or(EstadoRede::Ok, |r| r.estado.clone())
    }

    // ---------- buscas em segundo plano ----------

    /// Guarda o pedido de uma leitura (`&self`) e acorda o motor, que começa a busca no sinal.
    fn pedir(&self, pedido: Pedido) {
        let mut pedidos = self.integracao.pedidos.borrow_mut();
        if pedidos.contains(&pedido) {
            return;
        }
        let acordar = pedidos.is_empty();
        pedidos.push(pedido);
        if acordar {
            let _ = self.sinais.try_send(Sinal::Integracao);
        }
    }

    /// Começa as buscas pedidas desde o último sinal (as que ainda fazem falta).
    pub(super) fn atender_pedidos(&mut self, cx: &mut Context<Self>) {
        let pedidos = std::mem::take(&mut *self.integracao.pedidos.borrow_mut());
        let agora = Instant::now();
        for pedido in pedidos {
            match pedido {
                Pedido::Remotos(pasta) if precisa_buscar(self.integracao.remotos.get(&pasta), agora) => {
                    marcar_buscando(&mut self.integracao.remotos, pasta.clone(), agora);
                    let lida = pasta.clone();
                    cx.spawn(async move |motor, cx| {
                        let remotos = cx.background_spawn(async move { remotos_do_github(&git::remotos(&lida)) }).await;
                        let _ = motor.update(cx, |m, cx| {
                            guardar(&mut m.integracao.remotos, pasta, Ok(remotos), Instant::now());
                            cx.notify();
                        });
                    })
                    .detach();
                }
                Pedido::Repo(repo) if precisa_buscar(self.integracao.info_repos.get(&repo), agora) => {
                    marcar_buscando(&mut self.integracao.info_repos, repo.clone(), agora);
                    let pasta = self.dados.pasta.clone();
                    let lido = repo.clone();
                    self.em_segundo_plano(
                        Provedor::Github,
                        true,
                        move |e| {
                            let gh = Gh { e, pasta: &pasta };
                            let info = gh.info_repo(&lido)?;
                            // Sem CI e o prefixo sugerido não impedem nada: um erro neles fica de fora.
                            let workflows = gh.workflows(&lido).ok();
                            let prefixo = gh.meus_ramos_de_pr(&lido).ok().and_then(|ramos| issues::prefixo_sugerido(&ramos));
                            Ok(RepoGithub { info, workflows, prefixo })
                        },
                        move |m, resultado: Result<RepoGithub, ErroRemoto>, _| {
                            m.anotar_rede(Provedor::Github, resultado.as_ref().err());
                            let resultado = resultado.map_err(|e| e.para_usuario("GitHub"));
                            guardar(&mut m.integracao.info_repos, repo, resultado, Instant::now());
                        },
                        cx,
                    );
                }
                Pedido::Ramos(repo) if precisa_buscar(self.integracao.ramos.get(&repo), agora) => {
                    marcar_buscando(&mut self.integracao.ramos, repo.clone(), agora);
                    let pasta = self.dados.pasta.clone();
                    let lido = repo.clone();
                    self.em_segundo_plano(
                        Provedor::Github,
                        true,
                        move |e| Gh { e, pasta: &pasta }.ramos(&lido),
                        move |m, resultado, _| {
                            m.anotar_rede(Provedor::Github, resultado.as_ref().err());
                            let resultado = resultado.map_err(|e| e.para_usuario("GitHub"));
                            guardar(&mut m.integracao.ramos, repo, resultado, Instant::now());
                        },
                        cx,
                    );
                }
                Pedido::Times if precisa_buscar(self.integracao.times.as_ref(), agora) => {
                    let Some(chave) = self.integracao.chave_linear.clone() else {
                        continue;
                    };
                    let anterior = self.integracao.times.take().map_or(Carregando::Nada, |(_, c)| c);
                    self.integracao.times = Some((agora, anterior.buscando()));
                    self.em_segundo_plano(
                        Provedor::Linear,
                        true,
                        move |e| Linear { t: &Curl(e), chave: &chave }.times(),
                        move |m, resultado, _| {
                            m.anotar_rede(Provedor::Linear, resultado.as_ref().err());
                            let resultado = resultado.map_err(|e| e.para_usuario("Linear"));
                            let anterior = m.integracao.times.take().map_or(Carregando::Nada, |(_, c)| c);
                            let agora = Instant::now();
                            m.integracao.times = Some((agora, anterior.com(resultado, agora)));
                        },
                        cx,
                    );
                }
                Pedido::DoTime(time) if precisa_buscar(self.integracao.do_time.get(&time), agora) => {
                    let Some(chave) = self.integracao.chave_linear.clone() else {
                        continue;
                    };
                    marcar_buscando(&mut self.integracao.do_time, time.clone(), agora);
                    let lido = time.clone();
                    self.em_segundo_plano(
                        Provedor::Linear,
                        true,
                        move |e| Linear { t: &Curl(e), chave: &chave }.do_time(&lido),
                        move |m, resultado, _| {
                            m.anotar_rede(Provedor::Linear, resultado.as_ref().err());
                            let resultado = resultado.map_err(|e| e.para_usuario("Linear"));
                            guardar(&mut m.integracao.do_time, time, resultado, Instant::now());
                        },
                        cx,
                    );
                }
                _ => {}
            }
        }
    }

    /// A chave do Linear, se está na memória (nunca lê o chaveiro).
    pub(super) fn chave_na_memoria(&self) -> Option<Arc<Chave>> {
        self.integracao.chave_linear.clone()
    }

    /// Quem roda os programas de rede (o falso, nos testes).
    pub(super) fn executor(&self) -> Arc<dyn Executor> {
        self.integracao.executor.clone()
    }

    /// Um estado do Linear pelo id, nos times que já vieram (para a mudança otimista).
    pub(super) fn estado_conhecido(&self, id: &str) -> Option<EstadoIssue> {
        self.integracao
            .do_time
            .values()
            .filter_map(|(_, c)| c.valor())
            .flat_map(|d| d.estados.iter())
            .find(|e| e.estado.id == id)
            .map(|e| e.estado.clone())
    }

    /// O estado da rede do provedor depois de uma chamada.
    pub(super) fn anotar_rede(&mut self, provedor: Provedor, erro: Option<&ErroRemoto>) {
        let rede = self.integracao.rede_mut(provedor);
        match erro {
            None => rede.deu_certo(),
            Some(erro) => rede.falhou(erro, Instant::now()),
        }
    }

    /// Roda `trabalho` (que bloqueia: gh, curl, o chaveiro) fora da thread da interface, na vez
    /// do provedor, e entrega o resultado a `depois` no motor, com um redesenho. A permissão da
    /// fila é pega antes e vai para dentro da tarefa de fundo.
    pub(super) fn em_segundo_plano<T: Send + 'static>(
        &mut self,
        provedor: Provedor,
        do_usuario: bool,
        trabalho: impl FnOnce(&dyn Executor) -> T + Send + 'static,
        depois: impl FnOnce(&mut Motor, T, &mut Context<Motor>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let fila = self.integracao.fila(provedor, do_usuario);
        let e = self.integracao.executor.clone();
        cx.spawn(async move |motor, cx| {
            let vez = fila.acquire_arc().await;
            let resultado = cx
                .background_spawn(async move {
                    let _vez = vez;
                    trabalho(&*e)
                })
                .await;
            let _ = motor.update(cx, |m, cx| {
                depois(m, resultado, cx);
                cx.notify();
            });
        })
        .detach();
    }
}

/// A busca de uma leitura guardada começou: fica o valor que havia.
fn marcar_buscando<K: Eq + std::hash::Hash, T>(mapa: &mut HashMap<K, Guardado<T>>, chave: K, agora: Instant) {
    let anterior = mapa.remove(&chave).map_or(Carregando::Nada, |(_, c)| c);
    mapa.insert(chave, (agora, anterior.buscando()));
}

/// O resultado de uma busca no lugar da leitura (o erro guarda o valor de antes).
fn guardar<K: Eq + std::hash::Hash, T>(mapa: &mut HashMap<K, Guardado<T>>, chave: K, resultado: Result<T, String>, agora: Instant) {
    let anterior = mapa.remove(&chave).map_or(Carregando::Nada, |(_, c)| c);
    mapa.insert(chave, (agora, anterior.com(resultado, agora)));
}

#[cfg(test)]
mod testes {
    use std::sync::Mutex;

    use super::*;
    use crate::{
        linear::{EstadoDoTime, RespostaHttp},
        segredos::CHAVE_INVALIDA,
    };

    const BOA: &str = concat!("lin_", "api_ABCdef0123456789xyzWa1b2");

    /// Um Linear que responde o que foi dado e conta os pedidos.
    struct LinearFalso {
        respostas: Mutex<Vec<Result<RespostaHttp, ErroRemoto>>>,
        pedidos: Mutex<usize>,
    }

    impl LinearFalso {
        fn com(resposta: Result<RespostaHttp, ErroRemoto>) -> Self {
            Self { respostas: Mutex::new(vec![resposta]), pedidos: Mutex::new(0) }
        }

        fn pedidos(&self) -> usize {
            *self.pedidos.lock().unwrap()
        }
    }

    impl Transporte for LinearFalso {
        fn postar(&self, _chave: &Chave, _corpo: &str) -> Result<RespostaHttp, ErroRemoto> {
            *self.pedidos.lock().unwrap() += 1;
            self.respostas.lock().unwrap().pop().unwrap_or_else(|| Err(ErroRemoto::Outro("sem resposta".into())))
        }
    }

    fn http(status: u16, corpo: &str) -> Result<RespostaHttp, ErroRemoto> {
        Ok(RespostaHttp { status, cabecalhos: Vec::new(), corpo: corpo.into() })
    }

    const EU_OK: &str = r#"{"data":{"viewer":{"id":"u1","name":"Victor","displayName":"victor"},"organization":{"id":"o1","name":"Eita","urlKey":"eita"}}}"#;

    #[test]
    fn chave_invalida_nao_grava_nada() {
        let mut gravadas = 0;
        let mut gravar = |_: &Chave| -> Result<(), ErroChaveiro> {
            gravadas += 1;
            Ok(())
        };
        // Malformada: nem chega ao Linear.
        let linear = LinearFalso::com(http(200, EU_OK));
        for ruim in ["", "lin_api_curta", concat!("\"lin_", "api_ABCdef0123456789xyzWa1b2\""), concat!("gh", "p_ABCdef0123456789xyzWa1b2")] {
            assert_eq!(conectar_com(ruim, &linear, &mut gravar).unwrap_err(), CHAVE_INVALIDA);
        }
        assert_eq!(linear.pedidos(), 0);
        // Recusada pelo Linear (o 401, e o 400 com AUTHENTICATION_ERROR que o Linear também manda).
        let recusa = r#"{"errors":[{"message":"Authentication required, not authenticated","extensions":{"code":"AUTHENTICATION_ERROR"}}]}"#;
        for resposta in [http(401, recusa), http(400, recusa)] {
            let erro = conectar_com(BOA, &LinearFalso::com(resposta), &mut gravar).unwrap_err();
            assert_eq!(erro, "O Linear não aceitou esta chave. Confira se copiou inteira e se ela não foi revogada.");
        }
        let proibida = r#"{"errors":[{"message":"Forbidden","extensions":{"code":"FORBIDDEN"}}]}"#;
        assert_eq!(conectar_com(BOA, &LinearFalso::com(http(200, proibida)), &mut gravar).unwrap_err(), "A chave não tem permissão de leitura.");
        let sem_rede = LinearFalso::com(Err(ErroRemoto::SemRede("Could not resolve host: api.linear.app".into())));
        assert_eq!(conectar_com(BOA, &sem_rede, &mut gravar).unwrap_err(), "Sem conexão com o Linear. Tente de novo.");
        assert_eq!(gravadas, 0, "nada vai para o chaveiro sem o Linear aceitar");
    }

    #[test]
    fn chave_aceita_vai_para_o_chaveiro_e_a_conta_so_leva_o_final() {
        let mut gravadas = Vec::new();
        let (chave, conta) = conectar_com(&format!("  {BOA}\n"), &LinearFalso::com(http(200, EU_OK)), &mut |c: &Chave| {
            gravadas.push(c.final_());
            Ok(())
        })
        .unwrap();
        assert_eq!(chave.texto(), BOA);
        assert_eq!(gravadas, ["a1b2"]);
        assert_eq!(
            conta,
            ContaLinear { workspace: "Eita".into(), url_key: "eita".into(), nome: "Victor".into(), id_usuario: "u1".into(), final_da_chave: "a1b2".into() }
        );
        // O chaveiro recusou: a conexão não acontece, e nunca há outro lugar para a chave.
        let erro = conectar_com(BOA, &LinearFalso::com(http(200, EU_OK)), &mut |_: &Chave| Err(ErroChaveiro::Indisponivel("org.freedesktop.secrets não existe".into())))
            .unwrap_err();
        assert_eq!(erro, "Não consegui guardar no chaveiro do sistema (org.freedesktop.secrets não existe). A chave não foi salva.");
    }

    fn info(permissao: &str) -> InfoRepo {
        InfoRepo {
            repositorio: "VictorMenegate/top-bar-app-color".into(),
            permissao: permissao.into(),
            metodo_padrao: "merge".into(),
            squash: true,
            merge: true,
            rebase: false,
            ..Default::default()
        }
    }

    fn login(estado: EstadoLogin) -> LoginGh {
        LoginGh { login: "VictorMenegate".into(), estado, escopos: vec!["repo".into()] }
    }

    #[test]
    fn metodos_permitidos_com_o_padrao_primeiro() {
        assert_eq!(metodos_permitidos(&info("ADMIN")), ["merge", "squash"]);
        let so_squash = InfoRepo { merge: false, ..info("ADMIN") };
        assert_eq!(metodos_permitidos(&so_squash), ["squash"]);
        let padrao_proibido = InfoRepo { metodo_padrao: "rebase".into(), rebase: false, ..info("ADMIN") };
        assert_eq!(metodos_permitidos(&padrao_proibido), ["squash", "merge"]);
        assert!(metodos_permitidos(&InfoRepo::default()).is_empty());
    }

    #[test]
    fn abrir_pr_pede_login_e_escrita_menos_do_fork() {
        let ok = login(EstadoLogin::Ok);
        assert_eq!(modo_pr_disponivel(&info("ADMIN"), Some(&ok), false), Ok(()));
        assert_eq!(modo_pr_disponivel(&info("WRITE"), Some(&ok), false), Ok(()));
        assert_eq!(modo_pr_disponivel(&info("READ"), Some(&ok), false).unwrap_err(), "Sua conta só lê este repositório");
        // Do fork para o original, ler basta.
        assert_eq!(modo_pr_disponivel(&info("READ"), Some(&ok), true), Ok(()));
        let entrar = "Entre no GitHub em Configurações › Conexões";
        assert_eq!(modo_pr_disponivel(&info("ADMIN"), None, false).unwrap_err(), entrar);
        assert_eq!(modo_pr_disponivel(&info("ADMIN"), Some(&login(EstadoLogin::Erro("token".into()))), false).unwrap_err(), entrar);
        assert_eq!(modo_pr_disponivel(&info("ADMIN"), Some(&login(EstadoLogin::TempoEsgotado)), true).unwrap_err(), entrar);
    }

    #[test]
    fn estados_padrao_sao_os_de_menor_posicao() {
        let estado = |id: &str, tipo, posicao| EstadoDoTime { estado: EstadoIssue { id: id.into(), nome: id.into(), tipo, cor: None }, posicao };
        let dados = DadosDoTime {
            estados: vec![
                estado("revisao", TipoEstado::EmAndamento, 3.0),
                estado("feito", TipoEstado::Concluida, 5.0),
                estado("fazendo", TipoEstado::EmAndamento, 2.0),
                estado("a-fazer", TipoEstado::AFazer, 1.0),
                estado("lancado", TipoEstado::Concluida, 6.0),
            ],
            ..Default::default()
        };
        let (iniciar, mesclar) = estados_padrao(&dados);
        assert_eq!((iniciar.map(|e| e.id.as_str()), mesclar.map(|e| e.id.as_str())), (Some("fazendo"), Some("feito")));
        assert_eq!(estados_padrao(&DadosDoTime::default()), (None, None));
    }

    #[test]
    fn estado_do_github_pelo_login() {
        assert_eq!(estado_do_login(Err(ErroRemoto::SemPrograma("gh"))), EstadoGithub::SemGh);
        assert_eq!(estado_do_login(Ok(None)), EstadoGithub::SemLogin(None));
        assert_eq!(estado_do_login(Err(ErroRemoto::TempoEsgotado)), EstadoGithub::SemConexao);
        assert_eq!(estado_do_login(Ok(Some(login(EstadoLogin::TempoEsgotado)))), EstadoGithub::SemConexao);
        assert_eq!(
            estado_do_login(Ok(Some(login(EstadoLogin::Erro("The token in keyring is invalid.".into()))))),
            EstadoGithub::SemLogin(Some("The token in keyring is invalid.".into()))
        );
        assert!(matches!(estado_do_login(Ok(Some(login(EstadoLogin::Ok)))), EstadoGithub::Conectado(l) if l.login == "VictorMenegate"));
    }

    #[test]
    fn remote_da_pasta_e_o_time_do_linear() {
        let remotos = remotos_do_github(&[
            ("upstream".into(), "https://github.com/cli/cli.git".into()),
            ("gitlab".into(), "git@gitlab.com:x/y.git".into()),
            ("origin".into(), "git@github.com:VictorMenegate/top-bar-app-color.git".into()),
        ]);
        assert_eq!(remotos.iter().map(|r| r.repositorio.as_str()).collect::<Vec<_>>(), ["cli/cli", "VictorMenegate/top-bar-app-color"]);
        let config = |remoto: &str| ConfigGithub { remoto: remoto.into(), ..Default::default() };
        assert_eq!(remoto_escolhido(&remotos, &config("")).unwrap().nome, "origin");
        assert_eq!(remoto_escolhido(&remotos, &config("upstream")).unwrap().nome, "upstream");
        assert_eq!(remoto_escolhido(&remotos, &config("sumiu")).unwrap().nome, "origin");
        assert_eq!(remoto_escolhido(&remotos[..1], &config("")).unwrap().nome, "upstream");
        assert!(remoto_escolhido(&[], &config("")).is_none());

        let eng = Time { id: "t1".into(), chave: "ENG".into(), nome: "Engenharia".into(), ..Default::default() };
        let antes = LinearDoProjeto {
            time_id: "t1".into(),
            estado_iniciar: Some("fazendo".into()),
            estado_mesclar: Some(String::new()),
            pasta: Some("/p/loja".into()),
            ..Default::default()
        };
        let mesmo = linear_com_time(Some(antes.clone()), &eng);
        assert_eq!((mesmo.time_chave.as_str(), mesmo.estado_iniciar.as_deref()), ("ENG", Some("fazendo")));
        let ops = Time { id: "t2".into(), chave: "OPS".into(), ..Default::default() };
        let outro = linear_com_time(Some(antes), &ops);
        assert_eq!((outro.estado_iniciar, outro.estado_mesclar, outro.pasta), (None, None, Some("/p/loja".into())));
    }

    /// Manual, só leitura e com rede (`cargo test -- --ignored integracao_`): o aceite do G2 com
    /// o gh logado do Victor.
    #[test]
    #[ignore]
    fn integracao_conexoes_do_victor() {
        let pasta = std::env::temp_dir();
        let gh = Gh { e: &remoto::Real, pasta: &pasta };
        let EstadoGithub::Conectado(login) = estado_do_login(gh.login()) else {
            panic!("o gh não está conectado");
        };
        println!("Conectado como {} · {}", login.login, login.escopos.join(", "));
        assert_eq!(login.login, "VictorMenegate");
        let info = gh.info_repo("VictorMenegate/top-bar-app-color").unwrap();
        println!("{} ({})", info.repositorio, info.permissao.to_lowercase());
        assert_eq!(info.permissao, "ADMIN");
        assert_eq!(modo_pr_disponivel(&info, Some(&login), false), Ok(()));
        println!("métodos: {:?}, workflows: {:?}", metodos_permitidos(&info), gh.workflows(&info.repositorio));
        assert!(isolamento::testar(&remoto::Real), "o bwrap não funcionou aqui");
    }

    #[test]
    fn leitura_so_busca_de_novo_quando_precisa() {
        let agora = Instant::now();
        assert!(precisa_buscar::<u32>(None, agora));
        assert!(!precisa_buscar(Some(&(agora, Carregando::<u32>::Carregando(None))), agora));
        assert!(!precisa_buscar(Some(&(agora, Carregando::Pronto(1u32, agora))), agora));
        assert!(precisa_buscar(Some(&(agora, Carregando::Pronto(1u32, agora))), agora + VALIDADE));
        // O erro não vira laço: só depois de um minuto.
        let erro = (agora, Carregando::<u32>::Erro("sem rede".into(), Some(1)));
        assert!(!precisa_buscar(Some(&erro), agora + Duration::from_secs(5)));
        assert!(precisa_buscar(Some(&erro), agora + ESPERA_DO_ERRO));
        // Buscando e voltando com erro, o valor de antes continua desenhado.
        let de_novo = Carregando::Pronto(7u32, agora).buscando();
        assert_eq!(de_novo, Carregando::Carregando(Some(7)));
        assert_eq!(de_novo.com(Err("caiu".into()), agora), Carregando::Erro("caiu".into(), Some(7)));
    }
}
