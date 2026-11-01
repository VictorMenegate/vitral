//! As issues no motor: as listas e os detalhes em cache, o chat de uma issue ("Iniciar agente",
//! "Só a cópia", o lote de até 3), anexar e ligar, e o estado de uma issue. Por enquanto, só a API
//! combinada com a interface (seção 10.1 da especificação): as leituras não têm nada e as ações
//! respondem que ainda não estão disponíveis. Este módulo é o `motor::issues`; o dos tipos comuns
//! e das funções puras é o `crate::issues`.
// Provisório: o G3a preenche estes corpos (e a interface do G3b passa a chamá-los).
#![allow(dead_code)]

use std::time::Instant;

use smol::channel::Receiver;

use super::*;
use super::conexoes::Carregando;
use crate::{
    isolamento,
    issues::{FiltroIssues, FonteIssue, IssueDetalhe, IssueLigada, IssueResumo},
    modelo::ModoEntrega,
    prompts::PorqueIsolado,
};

/// A resposta provisória das ações que ainda não existem.
pub const AINDA_NAO: &str = "Ainda não disponível nesta versão do Vitral.";

/// De onde vêm as issues do popover: o projeto (no Mestre, o do chip "Projeto") e a fonte.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EscopoIssues {
    pub projeto: String,
    pub fonte: FonteIssue,
    /// No GitHub, o "dono/nome" (um projeto pode ter mais de uma pasta com GitHub).
    pub repositorio: Option<String>,
}

/// O que o popover desenha de uma lista: o que está em cache, como está, e o estado da busca.
#[derive(Clone, Debug, Default)]
pub struct VisaoLista {
    pub itens: Vec<IssueResumo>,
    pub carregando: bool,
    pub erro: Option<String>,
    /// Para o "atualizado há 2 min".
    pub atualizada: Option<Instant>,
    /// Há mais páginas ("Carregar mais").
    pub tem_mais: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModoIssue {
    /// O chat nasce com o pedido da issue e o agente já trabalhando.
    Agente,
    /// Só a cópia: o composer nasce com a issue anexada.
    SoCopia,
}

/// O que a interface já perguntou antes de criar o chat.
#[derive(Clone, Debug, Default)]
pub struct OpcoesDaIssue {
    /// "Em qual pasta?", num projeto com mais de uma.
    pub pasta: Option<PathBuf>,
}

/// O resultado de "Iniciar agentes (n)": os chats criados e as issues que falharam, com o motivo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResultadoLote {
    pub criados: Vec<String>,
    pub falhas: Vec<(String, String)>,
}

/// O lote que entra num turno liga a marca de isolado: alguma entrada traz texto de terceiros
/// (aviso de PR, pedido da issue) ou uma issue anexada.
pub fn marca_isolado(lote: &[Entrada]) -> bool {
    lote.iter().any(|e| e.terceiros || !e.referencias.is_empty())
}

/// Quem roda isolado (seção 3.6): o chat de projeto marcado ou de uma pasta que entrega por PR, e o
/// ajudante marcado ou de um chat isolado. O Mestre não tem Bash; a conversa antiga não roda.
pub fn isolamento_do_chat(papel: Papel, marcado: bool, pasta_em_pr: bool, pai_isolado: bool) -> bool {
    match papel {
        Papel::Projeto => marcado || pasta_em_pr,
        Papel::Trabalhador => marcado || pai_isolado,
        Papel::Mestre | Papel::Coordenador => false,
    }
}

/// Um canal que já tem a resposta.
fn respondido<T>(valor: T) -> Receiver<T> {
    let (envia, recebe) = smol::channel::bounded(1);
    let _ = envia.try_send(valor);
    recebe
}

impl Motor {
    /// A lista em cache (dispara a busca se faltar).
    pub fn issues(&self, _escopo: &EscopoIssues, _filtro: &FiltroIssues) -> VisaoLista {
        VisaoLista { erro: Some(AINDA_NAO.into()), ..Default::default() }
    }

    pub fn atualizar_issues(&mut self, _escopo: &EscopoIssues, _filtro: &FiltroIssues, _cx: &mut Context<Self>) {}

    pub fn mais_issues(&mut self, _escopo: &EscopoIssues, _filtro: &FiltroIssues, _cx: &mut Context<Self>) {}

    pub fn buscar_issues(&mut self, _escopo: &EscopoIssues, _termo: String, _cx: &mut Context<Self>) {}

    pub fn detalhe_issue(&self, _fonte: FonteIssue, _id: &str) -> Carregando<IssueDetalhe> {
        Carregando::Erro(AINDA_NAO.into(), None)
    }

    pub fn atualizar_detalhe_issue(&mut self, _fonte: FonteIssue, _id: &str, _cx: &mut Context<Self>) {}

    /// A tarefa do chat que já trabalha na issue.
    pub fn tarefa_da_issue(&self, fonte: FonteIssue, id: &str) -> Option<u64> {
        self.tarefas
            .iter()
            .filter(|t| !t.estado.encerrada())
            .find(|t| t.issue.as_ref().is_some_and(|i| i.fonte == fonte && i.id == id))
            .map(|t| t.id)
    }

    pub fn chat_da_issue(
        &mut self,
        _projeto: &str,
        _issue: IssueResumo,
        _modo: ModoIssue,
        _o: OpcoesDaIssue,
        _cx: &mut Context<Self>,
    ) -> Result<String, String> {
        Err(AINDA_NAO.into())
    }

    pub fn chats_das_issues(&mut self, _projeto: &str, lote: Vec<IssueResumo>, _modo: ModoIssue, _cx: &mut Context<Self>) -> Receiver<ResultadoLote> {
        let falhas = lote.into_iter().map(|i| (i.chave, AINDA_NAO.to_string())).collect();
        respondido(ResultadoLote { criados: Vec::new(), falhas })
    }

    /// O bloco da issue que vai anexado na mensagem.
    pub fn bloco_da_issue(&mut self, _fonte: FonteIssue, _id: &str, _cx: &mut Context<Self>) -> Receiver<Result<String, String>> {
        respondido(Err(AINDA_NAO.into()))
    }

    pub fn ligar_issue(&mut self, _chat: &str, _issue: IssueLigada, _cx: &mut Context<Self>) -> Result<(), String> {
        Err(AINDA_NAO.into())
    }

    pub fn mudar_estado_issue(&mut self, _fonte: FonteIssue, _id: &str, _estado: String, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        respondido(Resposta::erro(AINDA_NAO))
    }

    /// "Destravar e tentar" no efeito que encontrou o chaveiro trancado.
    pub fn tentar_efeito_pendente(&mut self, _tarefa: u64, _cx: &mut Context<Self>) -> Receiver<Resposta> {
        respondido(Resposta::erro(AINDA_NAO))
    }
}

// ---------- quem roda isolado ----------

impl Motor {
    /// O chat roda isolado: marcado (recebeu texto de terceiros), de uma pasta que entrega por PR,
    /// ou ajudante de um chat isolado.
    pub fn roda_isolado(&self, chat: &Chat) -> bool {
        let pai_isolado = chat.papel == Papel::Trabalhador
            && chat.pai.as_deref().and_then(|p| self.chat(p)).is_some_and(|p| p.papel == Papel::Projeto && self.roda_isolado(p));
        let em_pr = chat.papel == Papel::Projeto && self.pasta_em_pr(chat);
        isolamento_do_chat(chat.papel, chat.isolado, em_pr, pai_isolado)
    }

    /// A pasta do chat entrega por PR.
    fn pasta_em_pr(&self, chat: &Chat) -> bool {
        !chat.projeto.is_empty()
            && self.pasta_do_chat(chat).is_some_and(|p| self.config_da_pasta(&chat.projeto, &p).github.entrega == ModoEntrega::Pr)
    }

    /// Por que o chat roda isolado (o prompt diz).
    pub(super) fn motivo_do_isolamento(&self, chat: &Chat) -> PorqueIsolado {
        let marcado = chat.isolado
            || chat.pai.as_deref().and_then(|p| self.chat(p)).is_some_and(|p| p.isolado);
        if marcado || !self.pasta_em_pr(chat) { PorqueIsolado::Terceiros } else { PorqueIsolado::EntregaPorPr }
    }

    /// Como o chat roda isolado, se roda: no bwrap, ou só com o ambiente se ele não funciona aqui.
    pub(super) fn modo_isolado(&self, chat: &Chat) -> Option<isolamento::Modo> {
        self.roda_isolado(chat).then(|| isolamento::Modo::com_bwrap(self.isolamento_disponivel()))
    }

    /// O modo do que o Vitral roda na cópia da tarefa (verificação, preparação, revisor): o do chat
    /// dela.
    pub(super) fn modo_isolado_da_tarefa(&self, t: &Tarefa) -> Option<isolamento::Modo> {
        self.chat(&t.trabalhador).and_then(|c| self.modo_isolado(c))
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn marca_isolado_com_texto_de_terceiros_ou_issue_anexada() {
        let usuario = Entrada { texto: "oi".into(), ..Default::default() };
        let vitral = Entrada { de: Some("Vitral".into()), texto: "aviso".into(), ..Default::default() };
        // O extra do Mestre (as referências do pedido) não é texto de terceiros.
        let mestre = Entrada { extra: "Referências: #ninee".into(), ..Default::default() };
        assert!(!marca_isolado(&[usuario.clone(), vitral.clone(), mestre]));
        assert!(!marca_isolado(&[]));
        let anexada = Entrada { extra: "<issue …>".into(), terceiros: true, referencias: vec!["ENG-5".into()], ..usuario.clone() };
        assert!(marca_isolado(&[usuario.clone(), anexada]));
        let aviso_de_pr = Entrada { terceiros: true, ..vitral };
        assert!(marca_isolado(&[aviso_de_pr]));
        assert!(marca_isolado(&[Entrada { referencias: vec!["#45".into()], ..usuario }]));
    }

    #[test]
    fn quem_roda_isolado() {
        assert!(!isolamento_do_chat(Papel::Projeto, false, false, false), "o chat comum em Juntar fica como hoje");
        assert!(isolamento_do_chat(Papel::Projeto, true, false, false));
        assert!(isolamento_do_chat(Papel::Projeto, false, true, false), "a pasta que entrega por PR");
        assert!(isolamento_do_chat(Papel::Trabalhador, false, false, true), "o ajudante de um chat isolado");
        assert!(isolamento_do_chat(Papel::Trabalhador, true, false, false));
        assert!(!isolamento_do_chat(Papel::Trabalhador, false, true, false));
        for papel in [Papel::Mestre, Papel::Coordenador] {
            assert!(!isolamento_do_chat(papel, true, true, true), "{papel:?}");
        }
    }
}
