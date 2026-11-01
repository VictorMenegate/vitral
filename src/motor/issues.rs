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
use crate::issues::{FiltroIssues, FonteIssue, IssueDetalhe, IssueLigada, IssueResumo};

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

// ---------- ponte do G3b (provisória) ----------
// O G3a (o motor das issues) define estes mesmos nomes, com estas assinaturas e o mesmo
// comportamento; aqui só o mínimo para a interface do G3b compilar e ser testada antes da junção.
// Na junção vale o motor/issues.rs do G3a inteiro (que já tem tudo isto).

impl ResultadoLote {
    /// "3 chats criados, 1 falhou: ENG-158 (motivo)".
    pub fn texto(&self) -> String {
        let criados = crate::ui::componentes::contagem(self.criados.len(), "chat criado", "chats criados");
        if self.falhas.is_empty() {
            return criados;
        }
        let falhas: Vec<String> = self.falhas.iter().map(|(chave, motivo)| format!("{chave} ({motivo})")).collect();
        let falhou = if self.falhas.len() == 1 { "1 falhou".to_string() } else { format!("{} falharam", self.falhas.len()) };
        format!("{criados}, {falhou}: {}", falhas.join("; "))
    }
}

/// Uma issue anexada no composer, com o bloco que `bloco_da_issue` devolveu, pronta para ir na
/// mensagem (`enviar_com_issues`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnexoIssue {
    pub issue: IssueLigada,
    pub bloco: String,
}

/// A issue ligada a partir da linha da lista (só ids e nomes). No GitHub, o repositório sai da URL.
pub fn ligada_de(issue: &IssueResumo) -> IssueLigada {
    let repositorio = match (issue.fonte, crate::issues::parse_referencia(&issue.url)) {
        (FonteIssue::Github, Some(crate::issues::Referencia::Github { repositorio, .. })) => repositorio,
        _ => None,
    };
    IssueLigada {
        fonte: issue.fonte,
        id: issue.id.clone(),
        chave: issue.chave.clone(),
        titulo: issue.titulo.clone(),
        url: issue.url.clone(),
        repositorio,
        time_id: None,
        movida_para: None,
    }
}

impl Motor {
    /// "Em qual pasta?": a interface pergunta antes de "Iniciar agente" (só no Linear, com mais de
    /// uma pasta e nenhuma lembrada).
    pub fn precisa_escolher_pasta(&self, projeto: &str, issue: &IssueResumo) -> bool {
        let pastas = self.pastas_do(projeto);
        let lembrada = self.projeto(projeto).and_then(|p| p.linear.as_ref()).and_then(|l| l.pasta.clone());
        issue.fonte == FonteIssue::Linear && pastas.len() > 1 && !lembrada.is_some_and(|l| pastas.iter().any(|p| p.caminho == l))
    }

    /// A mensagem do usuário com issues anexadas: o bloco de cada uma vai só ao Claude (`extra`), a
    /// mensagem traz texto de terceiros e a bolha mostra as chaves. (No G3a, a primeira mensagem
    /// com a issue do chat do "Só a cópia" também faz os efeitos de iniciar.)
    pub fn enviar_com_issues(
        &mut self,
        chat: &str,
        texto: String,
        anexos: Vec<PathBuf>,
        reforco: Reforco,
        issues: Vec<AnexoIssue>,
        cx: &mut Context<Self>,
    ) {
        self.enviar_do_usuario_com(chat, texto, anexos, reforco, &issues, cx);
    }
}
