//! O PR de uma tarefa no motor: publicar, acompanhar (o vigia), mesclar e o que vem depois. Por
//! enquanto, só a API combinada com a interface (seção 10.1 da especificação): as leituras do que
//! vem do GitHub não têm nada e as ações respondem que ainda não estão disponíveis; o que já está
//! na tarefa (o PR e os anteriores) é lido de verdade.
// Provisório: o G4 preenche estes corpos (e a interface do G5 passa a chamá-los).
#![allow(dead_code)]

use gpui_kit::App;
use smol::channel::Receiver;

use super::*;
use super::conexoes::Carregando;
use super::issues::AINDA_NAO;
use crate::{
    git::Achado,
    github::{Check, DetalhePr},
    modelo::{Pr, PrAnterior},
};

/// De onde vem o diff que o diálogo mostra.
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

/// Um canal que já tem a resposta.
fn respondido<T>(valor: T) -> Receiver<T> {
    let (envia, recebe) = smol::channel::bounded(1);
    let _ = envia.try_send(valor);
    recebe
}

fn ainda_nao() -> Receiver<Resposta> {
    respondido(Resposta::erro(AINDA_NAO))
}

impl Motor {
    // ---------- leitura ----------

    pub fn pr_da_tarefa(&self, id: u64) -> Option<&Pr> {
        self.tarefa(id)?.pr.as_ref()
    }

    pub fn prs_anteriores(&self, id: u64) -> &[PrAnterior] {
        self.tarefa(id).map_or(&[], |t| t.prs_anteriores.as_slice())
    }

    pub fn detalhe_pr(&self, _repo: &str, _n: u64) -> Carregando<DetalhePr> {
        Carregando::Erro(AINDA_NAO.into(), None)
    }

    pub fn linha_do_tempo(&self, _repo: &str, _n: u64) -> Carregando<Vec<EventoLinha>> {
        Carregando::Erro(AINDA_NAO.into(), None)
    }

    pub fn checks_do_pr(&self, _repo: &str, _n: u64) -> Carregando<Vec<Check>> {
        Carregando::Erro(AINDA_NAO.into(), None)
    }

    pub fn log_do_check(&mut self, _repo: &str, _link: String, _cx: &mut Context<Self>) -> Receiver<Result<String, String>> {
        respondido(Err(AINDA_NAO.into()))
    }

    pub fn diff_do_pr(&self, _fonte: FonteDiff, _cx: &App) -> Task<Option<String>> {
        Task::ready(None)
    }

    /// A simulação da mescla com a base: `Ok(arquivos em conflito)`, vazia quando junta limpo.
    pub fn simulacao(&self, _id: u64) -> Option<Result<Vec<String>, String>> {
        None
    }

    /// As ações da aba PR e por que cada uma não está habilitada.
    pub fn acoes_do_pr(&self, id: u64) -> Vec<(AcaoPr, Result<(), String>)> {
        if self.pr_da_tarefa(id).is_none() {
            return Vec::new();
        }
        vec![(AcaoPr::MesclarQuandoPronto, Err(AINDA_NAO.into()))]
    }

    /// Provisório: nenhum achado. Nada publica até o G4, que confere de verdade antes do push.
    pub fn segredos_da_tarefa(&self, _id: u64, _cx: &App) -> Task<Vec<Achado>> {
        Task::ready(Vec::new())
    }

    pub fn arquivos_novos(&self, _id: u64, _cx: &App) -> Task<Vec<String>> {
        Task::ready(Vec::new())
    }

    /// A aba PR ficou visível ou sumiu (o vigia consulta mais com ela aberta).
    pub fn vigiar_aba_pr(&mut self, _id: u64, _visivel: bool, _cx: &mut Context<Self>) {}

    // ---------- ações ----------

    pub fn publicar_pr_pelo_usuario(&mut self, _id: u64, _pedido: PedidoPr, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        ainda_nao()
    }

    pub fn mesclar_pelo_usuario(&mut self, _id: u64, _metodo: Option<String>, _confirmado: Vec<Confirmado>, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        ainda_nao()
    }

    pub fn armar_mescla(&mut self, _id: u64, _ligar: bool, _metodo: Option<String>, _cx: &mut Context<Self>) {}

    pub fn acompanhar(&mut self, _id: u64, _ligar: bool, _cx: &mut Context<Self>) {}

    pub fn rascunho(&mut self, _id: u64, _rascunho: bool, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        ainda_nao()
    }

    pub fn reabrir_pr(&mut self, _id: u64, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        ainda_nao()
    }

    pub fn fechar_pr(&mut self, _id: u64, _comentario: Option<String>, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        ainda_nao()
    }

    pub fn editar_titulo_pr(&mut self, _id: u64, _titulo: String, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        ainda_nao()
    }

    pub fn mandar_falha_ao_chat(&mut self, _id: u64, _link: Option<String>, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        ainda_nao()
    }

    pub fn mandar_comentarios_ao_chat(&mut self, _id: u64, _ids: Vec<u64>, _cx: &mut Context<Self>) -> Resposta {
        Resposta::erro(AINDA_NAO)
    }

    pub fn rodar_checks_de_novo(&mut self, _id: u64, _alvo: RodarDeNovo, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        ainda_nao()
    }

    pub fn atualizar_com_base(&mut self, _id: u64, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        ainda_nao()
    }

    pub fn trazer_do_github(&mut self, _id: u64, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        ainda_nao()
    }

    pub fn recomecar_do_main(&mut self, _id: u64, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        ainda_nao()
    }

    pub fn fechar_issue_do_pr(&mut self, _id: u64, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        ainda_nao()
    }

    // ---------- confirmações ----------

    /// "Confirmar" ou "Agora não" num cartão `Confirmar`.
    pub fn decidir_confirmacao(&mut self, _cartao: u64, _aceitar: bool, _cx: &mut Context<Self>) {}
}
