//! Os tipos do que vai para o disco: projetos, chats, tarefas, cartões e configuração. Onde e
//! como cada um é gravado fica em `dados.rs`.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

// Os tipos da integração nascem nos módulos dela; o modelo os reexporta (o G3 e o G4 usam todos).
#[allow(unused_imports)]
pub use crate::github::{EstadoChecks, EstadoPr, ResumoChecks};
#[allow(unused_imports)]
pub use crate::issues::{FonteIssue, IssueLigada};

/// O formato dos dados no disco. Sobe quando uma migração muda o formato; o atualizador só volta
/// para uma versão que leia o formato que está no disco (`dados::formato_em`). O 1 é o Vitral antigo
/// (o `.vitral/` em cada pasta de trabalho, que a migração não muda) e o 2, os dados globais. O
/// `pacote/manifesto.py` lê esta linha.
pub const FORMATO_DADOS: u32 = 2;

pub fn agora() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn novo_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..10].to_string()
}

/// Id do projeto dos chats sem projeto (Avulsos) e do Mestre.
pub const SEM_PROJETO: &str = "";
/// O chat único do Mestre.
pub const ID_MESTRE: &str = "mestre";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Papel {
    /// Conversa com o usuário, distribui tarefas e revisa (a do Vitral antigo).
    #[default]
    Coordenador,
    /// Executa uma tarefa do coordenador; o ajudante de um chat de projeto.
    Trabalhador,
    /// Único, global, id "mestre".
    Mestre,
    /// Chat de projeto ou avulso (projeto ""); sempre com tarefa.
    Projeto,
}

impl Papel {
    /// O pedaço da URL do MCP de cada papel.
    pub fn rota(self) -> &'static str {
        match self {
            Papel::Coordenador => "coordenador",
            Papel::Trabalhador => "trabalhador",
            Papel::Mestre => "mestre",
            Papel::Projeto => "projeto",
        }
    }

    /// Rota desconhecida dá `None` (o MCP responde 404).
    pub fn da_rota(rota: &str) -> Option<Papel> {
        [Papel::Coordenador, Papel::Trabalhador, Papel::Mestre, Papel::Projeto]
            .into_iter()
            .find(|p| p.rota() == rota)
    }

    pub fn rotulo(self) -> &'static str {
        match self {
            Papel::Coordenador => "conversa antiga",
            Papel::Trabalhador => "ajudante",
            Papel::Mestre => "Mestre",
            Papel::Projeto => "chat",
        }
    }

    /// Ocupa uma das vagas de `max_trabalhadores`.
    pub fn conta_vaga(self) -> bool {
        matches!(self, Papel::Projeto | Papel::Trabalhador)
    }

    /// Aparece sozinho na lista (o ajudante vai embaixo do chat dele).
    pub fn de_topo(self) -> bool {
        self != Papel::Trabalhador
    }

    /// Conversa com o usuário e aparece como cartão de topo na lista de hoje: a conversa antiga e
    /// o chat de projeto. O Mestre fica de fora até a interface dele (L6).
    pub fn na_lista(self) -> bool {
        matches!(self, Papel::Coordenador | Papel::Projeto)
    }
}

/// Estado de execução de um chat. Não vai para o disco: ao abrir, todos estão parados.
#[derive(Clone, Debug, PartialEq)]
pub enum Execucao {
    Parado,
    Rodando {
        desde: u64,
    },
    /// Pedido de permissão esperando o usuário.
    EsperandoVoce,
    Erro(String),
}

impl Execucao {
    pub fn rodando(&self) -> bool {
        matches!(self, Execucao::Rodando { .. } | Execucao::EsperandoVoce)
    }
}

/// O item não tem hora: o `em` de uma conversa de antes dela. Não vai para o disco (os arquivos
/// das conversas antigas continuam iguais).
fn sem_hora(em: &u64) -> bool {
    *em == 0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Item {
    Usuario {
        texto: String,
        #[serde(default)]
        anexos: Vec<PathBuf>,
        /// Foto dos arquivos antes desta mensagem (para voltar o código a este ponto).
        #[serde(default)]
        ponto: Option<String>,
        /// Foi com Ultrathink: o histórico mostra o selo, não a palavra.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        ultrathink: bool,
        /// Foi com Ultracode (fluxo de agentes): o histórico mostra o selo.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        ultracode: bool,
        /// Issues anexadas ("ENG-123", "#45"), para o chip na bolha.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        referencias: Vec<String>,
        /// Quando entrou na conversa (Unix, segundos); 0 nas conversas de antes da hora nas
        /// mensagens, que não mostram nada.
        #[serde(default, skip_serializing_if = "sem_hora")]
        em: u64,
    },
    /// Mensagem que o Vitral (ou outro chat) entregou a este chat, no lugar do usuário.
    Entrega {
        de: String,
        texto: String,
        #[serde(default)]
        ponto: Option<String>,
        #[serde(default, skip_serializing_if = "sem_hora")]
        em: u64,
    },
    Assistente {
        texto: String,
        #[serde(default, skip_serializing_if = "sem_hora")]
        em: u64,
    },
    Ferramenta {
        id: String,
        nome: String,
        resumo: String,
        resultado: Option<String>,
        erro: bool,
        #[serde(default, skip_serializing_if = "sem_hora")]
        em: u64,
    },
    Aviso {
        texto: String,
        erro: bool,
        #[serde(default, skip_serializing_if = "sem_hora")]
        em: u64,
    },
    /// Cartão interativo; os dados ficam em `Estado.cartoes`.
    Cartao {
        id: u64,
        #[serde(default, skip_serializing_if = "sem_hora")]
        em: u64,
    },
    /// Item de uma versão mais nova, guardado e regravado como veio. Sempre a última variante.
    #[serde(untagged)]
    Desconhecido(serde_json::Value),
}

impl Item {
    /// O instante do item, ou 0 quando ele não tem hora (conversa de antes dela, ou item de uma
    /// versão mais nova).
    pub fn em(&self) -> u64 {
        match self {
            Item::Usuario { em, .. }
            | Item::Entrega { em, .. }
            | Item::Assistente { em, .. }
            | Item::Ferramenta { em, .. }
            | Item::Aviso { em, .. }
            | Item::Cartao { em, .. } => *em,
            Item::Desconhecido(_) => 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    /// Id do projeto; "" é Avulsos (e o Mestre).
    #[serde(default)]
    pub projeto: String,
    pub titulo: String,
    pub papel: Papel,
    /// Coordenador de quem este trabalhador recebe tarefas.
    pub pai: Option<String>,
    pub tarefa: Option<u64>,
    pub sessao: String,
    /// A sessão do Claude Code já existe (próximas vezes usam `--resume`).
    pub sessao_criada: bool,
    pub cwd: PathBuf,
    /// Modelo escolhido para a conversa (apelido como "opus"); `None` segue a configuração.
    pub modelo: Option<String>,
    /// Modelo que o Claude Code disse estar usando no último turno.
    #[serde(default)]
    pub modelo_atual: Option<String>,
    /// Modo de permissão escolhido para a conversa; `None` segue a configuração.
    #[serde(default)]
    pub permissao: Option<String>,
    /// Regras de "permitir sempre" desta conversa (ver `regra_de_permissao`).
    #[serde(default)]
    pub permitidas: Vec<String>,
    #[serde(default)]
    pub esforco: Option<String>,
    /// Conta do Claude (id, ou "auto"); `None` segue a configuração.
    #[serde(default)]
    pub conta: Option<String>,
    /// Tokens no contexto e tamanho da janela, do último turno.
    #[serde(default)]
    pub contexto: Option<u64>,
    #[serde(default)]
    pub janela: Option<u64>,
    /// Comandos de barra que o Claude Code aceita nesta conversa.
    #[serde(default)]
    pub comandos: Vec<String>,
    pub criado_em: u64,
    pub atualizado_em: u64,
    pub custo_usd: f64,
    /// O último `total_cost_usd` que o Claude Code deu, com a sessão a que ele se refere.
    #[serde(default)]
    pub custo_da_sessao: Option<(String, f64)>,
    #[serde(default)]
    pub fixado: bool,
    #[serde(default)]
    pub arquivado: bool,
    /// Quando o usuário viu a conversa pela última vez (para marcar o que é novo).
    #[serde(default)]
    pub lido_em: u64,
    /// Há um turno aberto (o processo começou e ainda não terminou). Ao reabrir, marca o turno
    /// que o Vitral fechado (ou uma queda de energia) cortou no meio.
    #[serde(default)]
    pub turno_aberto: bool,
    /// Modo em que a conversa estava antes de entrar em Planejar: aprovar o plano volta a ele.
    #[serde(default)]
    pub modo_antes_do_plano: Option<String>,
    /// Quem criou: "" o usuário, "mestre", "rotina:<id>", "migracao", "continuacao:<chat>",
    /// "importacao".
    #[serde(default)]
    pub criado_por: String,
    /// A conversa do Claude Code que este chat continua (o histórico vem do arquivo dela).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importada: Option<crate::importar::Importada>,
    /// Roda isolado (no bwrap, sem o chaveiro, o ssh e o login do gh): recebeu texto de terceiros
    /// (issue, aviso de PR). Pegajoso: uma vez ligado, não desliga mais.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub isolado: bool,
    /// As ações com efeito fora do Vitral que cada turno já fez (no máximo `MAX_ACOES_EXTERNAS`):
    /// "Tentar de novo" e a troca de conta reenviam o mesmo turno, e a ação não se repete.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acoes_externas: Vec<RegistroAcao>,
    /// Campos de versões mais novas, regravados como vieram.
    #[serde(flatten)]
    pub resto: serde_json::Map<String, serde_json::Value>,
    #[serde(skip)]
    pub itens: Vec<Item>,
}

/// Até quantos registros de ação externa um chat guarda (os mais antigos saem).
pub const MAX_ACOES_EXTERNAS: usize = 50;

/// Uma ação com efeito fora do Vitral (abrir, atualizar ou mesclar um PR) feita num turno.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegistroAcao {
    /// O uuid do turno (o mesmo numa repetição).
    pub turno: String,
    /// A `AcaoExterna` em JSON canônico.
    pub acao: String,
    pub resultado: String,
    pub em: u64,
}

impl Chat {
    /// O construtor único dos chats novos.
    pub fn novo(id: String, papel: Papel, projeto: &str, titulo: &str, cwd: PathBuf) -> Chat {
        let agora = agora();
        Chat {
            id,
            projeto: projeto.to_string(),
            titulo: titulo.to_string(),
            papel,
            pai: None,
            tarefa: None,
            sessao: uuid::Uuid::new_v4().to_string(),
            sessao_criada: false,
            cwd,
            modelo: None,
            modelo_atual: None,
            permissao: None,
            permitidas: Vec::new(),
            esforco: None,
            conta: None,
            contexto: None,
            janela: None,
            comandos: Vec::new(),
            criado_em: agora,
            atualizado_em: agora,
            custo_usd: 0.0,
            custo_da_sessao: None,
            fixado: false,
            arquivado: false,
            lido_em: agora,
            turno_aberto: false,
            modo_antes_do_plano: None,
            criado_por: String::new(),
            importada: None,
            isolado: false,
            acoes_externas: Vec::new(),
            resto: Default::default(),
            itens: Vec::new(),
        }
    }

    /// Soma o custo de um `result`. O `total_cost_usd` dele é o total da sessão, não o do turno:
    /// cresce a cada `result` do mesmo processo e, numa retomada (`--resume`), continua do total
    /// que a sessão gravou. Entra só o que cresceu desde o último visto. Um total menor (a sessão
    /// sem o total gravado, depois de uma queda) conta inteiro; o zerado de um processo que caiu ao
    /// abrir não muda nada.
    pub fn somar_custo(&mut self, total: f64) {
        if !total.is_finite() || total <= 0.0 {
            return;
        }
        let visto = match &self.custo_da_sessao {
            Some((sessao, visto)) if *sessao == self.sessao => *visto,
            _ => 0.0,
        };
        self.custo_usd += if total >= visto { total - visto } else { total };
        self.custo_da_sessao = Some((self.sessao.clone(), total));
    }

    /// O pedido de permissão cabe numa regra de "permitir sempre" desta conversa.
    pub fn permite(&self, ferramenta: &str, entrada: &serde_json::Value) -> bool {
        regra_de_permissao(ferramenta, entrada, &self.cwd).is_some_and(|r| self.permitidas.contains(&r))
    }
}

/// A regra de "permitir sempre" que cobre um pedido: "edicoes" para mudar arquivos dentro da
/// pasta do chat, "bash:<comando>" para um comando exato e "ferramenta:<nome>" para as outras. O
/// plano e as edições fora da pasta não têm regra: perguntam sempre.
pub fn regra_de_permissao(
    ferramenta: &str,
    entrada: &serde_json::Value,
    pasta: &Path,
) -> Option<String> {
    match ferramenta {
        "ExitPlanMode" => None,
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => {
            let caminho = entrada["file_path"].as_str().or(entrada["notebook_path"].as_str());
            dentro_da_pasta(Path::new(caminho?), pasta).then(|| "edicoes".to_string())
        }
        "Bash" => {
            let comando = entrada["command"].as_str()?.trim();
            (!comando.is_empty()).then(|| format!("bash:{comando}"))
        }
        _ => Some(format!("ferramenta:{ferramenta}")),
    }
}

/// O caminho fica na pasta do chat: relativo ou abaixo dela, e sem `..` (que pode sair dela).
pub fn dentro_da_pasta(caminho: &Path, pasta: &Path) -> bool {
    !caminho.components().any(|c| c == std::path::Component::ParentDir)
        && (caminho.is_relative() || caminho.starts_with(pasta))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EstadoTarefa {
    #[default]
    Rodando,
    EmRevisao,
    Ajustando,
    Aprovada,
    Falhou,
    Cancelada,
    /// Chat de projeto pronto para conversar, sem pedido pendente (nem ativa nem encerrada).
    Conversa,
    /// O PR da tarefa está aberto no GitHub (nem ativa nem encerrada): o Vitral acompanha os
    /// checks e a revisão. `estado == NoPr` implica `pr` aberto ou rascunho.
    NoPr,
}

impl EstadoTarefa {
    pub fn nome(self) -> &'static str {
        match self {
            EstadoTarefa::Rodando => "rodando",
            EstadoTarefa::EmRevisao => "em revisão",
            EstadoTarefa::Ajustando => "ajustando",
            EstadoTarefa::Aprovada => "aprovada",
            EstadoTarefa::Falhou => "falhou",
            EstadoTarefa::Cancelada => "cancelada",
            EstadoTarefa::Conversa => "conversa",
            EstadoTarefa::NoPr => "PR aberto",
        }
    }

    pub fn ativa(self) -> bool {
        matches!(self, EstadoTarefa::Rodando | EstadoTarefa::Ajustando)
    }

    pub fn encerrada(self) -> bool {
        matches!(
            self,
            EstadoTarefa::Aprovada | EstadoTarefa::Falhou | EstadoTarefa::Cancelada
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Verificacao {
    pub comando: String,
    pub passou: bool,
    pub saida: String,
    /// Quanto levou; None nas gravadas antes de o Vitral medir.
    #[serde(default)]
    pub duracao_s: Option<u64>,
}

impl Verificacao {
    /// Quanto levou, no formato curto da interface ("12 s", "3 min 05 s").
    pub fn tempo(&self) -> Option<String> {
        self.duracao_s.map(crate::ui::componentes::duracao_curta)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tarefa {
    pub id: u64,
    pub projeto: String,
    pub coordenador: String,
    pub trabalhador: String,
    pub titulo: String,
    pub objetivo: String,
    pub criterios: Vec<String>,
    pub verificacao: Option<String>,
    pub estado: EstadoTarefa,
    pub rodada: u32,
    pub worktree: Option<PathBuf>,
    pub branch: Option<String>,
    pub base: Option<String>,
    /// A cópia isolada (pasta, branch e pontos de volta) já foi apagada: o trabalhador não roda mais.
    #[serde(default)]
    pub copia_limpa: bool,
    pub progresso: Option<String>,
    pub relatorio: Option<String>,
    pub ultima_verificacao: Option<Verificacao>,
    pub criada_em: u64,
    /// Parecer do revisor independente sobre a última entrega.
    #[serde(default)]
    pub revisao: Option<Revisao>,
    /// Outras tarefas em andamento que mexem nos mesmos arquivos.
    #[serde(default)]
    pub sobreposicao: Vec<String>,
    /// Resultado do script de preparação da cópia (instalar dependências etc.).
    #[serde(default)]
    pub preparacao: Option<Verificacao>,
    /// Por que a tarefa parou no meio (Vitral fechado, Parar, verificação cortada); só anda de
    /// novo quando alguém manda retomar. Na verificação refeita sozinha ao abrir, fica marcada
    /// enquanto ela roda, para uma segunda queda não repetir sem ninguém pedir.
    #[serde(default)]
    pub parada: Option<String>,
    /// O relatório da última rodada ainda não chegou ao coordenador (a verificação ou o revisor
    /// ficaram no meio); guarda se o turno do trabalhador terminou com erro.
    #[serde(default)]
    pub entrega_pendente: Option<bool>,
    /// A cópia isolada está pronta e o trabalhador já pode rodar. Enquanto não está, as mensagens
    /// para ele esperam na fila (gravadas antes desta marca, as tarefas contam como prontas).
    #[serde(default = "sim")]
    pub preparada: bool,
    /// O coordenador aprovou com a verificação falhando ou problema grave do revisor: o que foi
    /// ignorado e por quê (vai também no commit).
    #[serde(default)]
    pub ressalva: Option<String>,
    #[serde(default)]
    pub forma: Forma,
    /// A pasta de onde a cópia saiu e onde ela é juntada; na forma direta, a pasta de trabalho.
    #[serde(default)]
    pub origem: PathBuf,
    /// Branch da origem quando a cópia nasceu.
    #[serde(default)]
    pub ramo_alvo: Option<String>,
    /// Início da faixa de 10 portas (`VITRAL_PORT`); 0 é sem faixa (vale `porta_da_tarefa`).
    #[serde(default)]
    pub porta: u64,
    #[serde(default)]
    pub atualizada_em: u64,
    /// Entrega registrada por `entregar_tarefa` na rodada atual.
    #[serde(default)]
    pub entrega: Option<Entrega>,
    /// Cartão de divisão do pedido atual.
    #[serde(default)]
    pub divisao: Option<u64>,
    /// Rodada em que o lembrete de chamar `entregar_tarefa` já foi mandado.
    #[serde(default)]
    pub lembrete_rodada: u32,
    /// Números do que a cópia tem para juntar.
    #[serde(default)]
    pub mudancas: Option<Mudancas>,
    /// Junção em andamento desde (gravado antes do merge; 0 é nenhuma).
    #[serde(default)]
    pub juntando_desde: u64,
    /// Geração da preparação da cópia: o resultado de uma geração antiga é descartado.
    #[serde(default)]
    pub geracao_preparo: u32,
    /// Por que a cópia não pôde ser preparada; não muda o estado.
    #[serde(default)]
    pub falha_copia: Option<String>,
    // ---------- GitHub e Linear ----------
    /// O PR aberto a partir da cópia (um por vez).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr: Option<Pr>,
    /// Os PRs mesclados ou fechados deste chat, do mais antigo para o mais novo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prs_anteriores: Vec<PrAnterior>,
    /// A issue que originou o chat ou que foi ligada a ele.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<IssueLigada>,
    /// O ramo escolhido antes da cópia existir (o `branchName` do Linear, o prefixo da pasta):
    /// `preparar_copia` usa este, e nunca recalcula ao refazer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramo_planejado: Option<String>,
    /// De onde a cópia parte: "origin/main" no modo PR; `None` é o HEAD da origem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partir_de: Option<String>,
    /// O que `copiar_arquivos_locais` pôs na cópia (o .env): nunca entra no commit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arquivos_copiados: Vec<String>,
    /// A rodada em que o ciclo atual começou (o limite de rodadas do dono conta daqui).
    #[serde(default)]
    pub rodada_do_ciclo: u32,
    /// O PR foi mesclado com o chat rodando: o recomeço da cópia fica para o fim do turno.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recomeco_pendente: bool,
    /// O pedido atual veio de "Iniciar agente" numa issue (conta como pedido para a entrega).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pedido_da_issue: bool,
    /// Uma rodada verificada espera o usuário apertar "Atualizar PR".
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pronto_para_atualizar: bool,
    /// Os efeitos de iniciar a issue (atribuir, mover, o comentário do Linear): só no chat nascido
    /// de "Iniciar agente" ou "Só a cópia" (no "Só a cópia", com a primeira mensagem).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efeitos_issue: Option<EfeitosDaIssue>,
    /// Campos de versões mais novas, regravados como vieram.
    #[serde(flatten)]
    pub resto: serde_json::Map<String, serde_json::Value>,
}

/// O que o Vitral já fez na issue ao iniciar. Nada aqui se repete sozinho.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EfeitosDaIssue {
    /// O id (UUID v4) do comentário "Em andamento no Vitral" no Linear: escolhido antes da primeira
    /// tentativa e o mesmo em todas (o Linear não cria outro com o mesmo id).
    pub id_comentario: String,
    /// Já rodaram (com sucesso ou com o aviso da falha).
    pub feitos: bool,
    /// Não rodaram por falta da chave (o chaveiro trancado): o motivo, para "Destravar e tentar".
    pub pendente: Option<String>,
    /// Os efeitos do PR no Linear (ligar o PR à issue, concluir ao mesclar) que esperam a chave:
    /// "Destravar e tentar" roda estes também.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub do_pr: Vec<EfeitoDoPr>,
}

/// Um efeito do PR na issue do Linear que ficou para depois (o chaveiro trancado).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EfeitoDoPr {
    /// Ligar o PR aberto à issue.
    Ligar { url: String, titulo: String },
    /// Mover a issue para o estado de mesclar (se ela ainda não terminou).
    Concluir,
}

/// O PR de uma tarefa, como o Vitral o viu por último. Nada aqui é segredo: `erro` já passou
/// por `remoto::sem_segredos`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Pr {
    /// "dono/nome" do destino.
    pub repositorio: String,
    pub numero: u64,
    pub url: String,
    pub titulo: String,
    pub base: String,
    pub ramo: String,
    /// O `--head` do gh: o ramo, ou "dono:ramo" num PR do fork para o original.
    pub cabeca: String,
    pub dono_do_head: String,
    pub estado: EstadoPr,
    /// O `headRefOid` visto por último.
    pub sha: String,
    /// O HEAD que o Vitral verificou e enviou: só ele é mesclado sozinho.
    pub sha_publicado: String,
    /// Shas curtos no head que não são do Vitral.
    pub commits_de_fora: Vec<String>,
    pub checks: ResumoChecks,
    /// A última vez que as contagens dos checks mudaram.
    pub checks_progresso_em: u64,
    /// O `mergeStateStatus` (CLEAN, BLOCKED, BEHIND, DIRTY…).
    pub mesclavel: String,
    /// O `reviewDecision`.
    pub decisao: String,
    pub conversas_abertas: u32,
    /// Para perceber novidade: o último comentário, a última revisão e o `updatedAt`.
    pub ultimo_comentario: u64,
    pub ultima_revisao: u64,
    pub atualizado: String,
    /// Os ids já levados ao chat (no máximo 500, os mais antigos saem).
    pub comentarios_entregues: Vec<u64>,
    /// Os ids dos comentários que o próprio Vitral fez.
    pub comentarios_nossos: Vec<u64>,
    /// `Some(método)`: mesclar quando ficar pronto (o "Ship").
    pub ao_ficar_pronto: Option<String>,
    pub acompanhando: bool,
    /// O sha e os links dos checks falhos já avisados, em ordem.
    pub falha_avisada: Option<String>,
    pub primeira_falha_em: Option<u64>,
    pub tentativas_log: u32,
    /// Rodadas disparadas por aviso do PR, para qualquer dono (no máximo 3).
    pub rodadas_automaticas: u32,
    /// "sha|mergeStateStatus" que o Vitral já tentou mesclar sozinho.
    pub tentativa_mescla: Option<String>,
    pub empurrado_em: u64,
    pub conferido_em: u64,
    pub fila_de_merge: bool,
    /// A mesclagem automática do GitHub está ligada (pelo site ou pela fila de merge).
    pub auto_github: bool,
    pub mesclado_por: Option<String>,
    /// O método com que o Vitral mesclou (`None` quando foi outra pessoa, pelo site ou pelo gh).
    pub metodo_mesclado: Option<String>,
    pub commit_mesclado: Option<String>,
    pub erro: Option<String>,
    /// O que espera o usuário neste PR ("Esperando você"): a falha do CI no Watch, a revisão, o
    /// comentário de quem não tem acesso, o pronto para mesclar…
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub esperando: Vec<EsperaPr>,
    /// A rodada de ajuste que um aviso do PR abriu (CI, revisão, ou o "Mandar ao chat" do usuário):
    /// a entrega dela conta como pedido, e só a de CI ou de revisão confiável publica sozinha.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ajuste: Option<AjustePr>,
}

impl Pr {
    /// Aberto ou rascunho: o estado em que a tarefa fica em `NoPr`.
    pub fn aberto(&self) -> bool {
        matches!(self.estado, EstadoPr::Aberto | EstadoPr::Rascunho)
    }

    /// Mesclar quando ficar pronto está ligado (o "Ship").
    pub fn armado(&self) -> bool {
        self.ao_ficar_pronto.is_some()
    }

    /// O head do PR não é o que o Vitral enviou.
    pub fn tem_commit_de_fora(&self) -> bool {
        !self.commits_de_fora.is_empty() || (!self.sha.is_empty() && !self.sha_publicado.is_empty() && self.sha != self.sha_publicado)
    }
}

/// Uma coisa do PR que espera o usuário, em "Esperando você" e na linha de acompanhamento.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EsperaPr {
    /// "CI falhou no PR #12" [Mandar ao chat]: no Watch, ou depois das 3 rodadas automáticas.
    CiFalhou {
        /// O sha e os links dos checks falhos (a `falha_avisada`).
        chave: String,
        /// Os nomes dos checks que falharam.
        falhos: Vec<String>,
    },
    /// "Revisão no PR #12 (2 de @x)" [Mandar ao chat]: comentários de quem tem acesso.
    Revisao { ids: Vec<u64>, autores: Vec<String> },
    /// "Comentário de @y (sem acesso ao repositório) no PR #12" [Ver] [Mandar ao chat]: nunca vai
    /// sozinho a ninguém.
    Terceiro { id: u64, autor: String, url: String },
    /// "PR #12 pronto para mesclar" [Mesclar], no Watch.
    ProntoParaMesclar,
    /// "Chegou commit de fora no PR #12" [Ver].
    CommitDeFora { shas: Vec<String> },
    /// "Checks parados há 2 h no PR #12".
    ChecksParados,
    /// "O GitHub não aceitou mesclar o PR #12: <motivo>" (o Ship foi desarmado).
    MesclaRecusada { motivo: String },
    /// Um item de uma versão mais nova, regravado como veio.
    #[serde(untagged)]
    Desconhecido(serde_json::Value),
}

/// O que abriu uma rodada de ajuste pelo PR.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatilhoAjuste {
    /// O aviso do CI que falhou.
    Ci,
    /// Comentários de revisão de quem tem acesso (ou de um bot de revisão).
    Revisao,
    /// Um comentário de quem não tem acesso, mandado ao chat pelo usuário: nunca publica sozinho.
    Terceiro,
}

/// A rodada de ajuste aberta por um aviso do PR.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AjustePr {
    pub rodada: u32,
    pub gatilho: GatilhoAjuste,
}

/// Um PR da tarefa que já foi mesclado ou fechado.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PrAnterior {
    pub numero: u64,
    pub url: String,
    pub titulo: String,
    pub estado: EstadoPr,
    pub mesclado_por: Option<String>,
    pub em: u64,
    /// "dono/nome" do destino (para reabrir e fechar a issue depois).
    pub repositorio: String,
    /// O head final do PR (o recomeço da cópia parte dele).
    pub sha: String,
    /// O método com que o Vitral mesclou; `None` quando foi outra pessoa.
    pub metodo: Option<String>,
    /// O Vitral apagou o ramo no GitHub depois de mesclar.
    pub ramo_apagado: bool,
    /// A issue ligada foi para Concluída (Linear) ou fechada pelo `Closes` (GitHub).
    pub issue_concluida: bool,
    /// Mesclado sem fechar a issue do GitHub (o `closingIssuesReferences` veio vazio): a linha do PR
    /// oferece "Fechar a issue #45" (o número).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fechar_issue: Option<u64>,
}

fn sim() -> bool {
    true
}

impl Default for Tarefa {
    fn default() -> Self {
        Tarefa {
            id: 0,
            projeto: String::new(),
            coordenador: String::new(),
            trabalhador: String::new(),
            titulo: String::new(),
            objetivo: String::new(),
            criterios: Vec::new(),
            verificacao: None,
            estado: EstadoTarefa::Rodando,
            rodada: 1,
            worktree: None,
            branch: None,
            base: None,
            copia_limpa: false,
            progresso: None,
            relatorio: None,
            ultima_verificacao: None,
            criada_em: 0,
            revisao: None,
            sobreposicao: Vec::new(),
            preparacao: None,
            parada: None,
            entrega_pendente: None,
            preparada: true,
            ressalva: None,
            forma: Forma::Copia,
            origem: PathBuf::new(),
            ramo_alvo: None,
            porta: 0,
            atualizada_em: 0,
            entrega: None,
            divisao: None,
            lembrete_rodada: 0,
            mudancas: None,
            juntando_desde: 0,
            geracao_preparo: 0,
            falha_copia: None,
            pr: None,
            prs_anteriores: Vec::new(),
            issue: None,
            ramo_planejado: None,
            partir_de: None,
            arquivos_copiados: Vec::new(),
            rodada_do_ciclo: 0,
            recomeco_pendente: false,
            pedido_da_issue: false,
            pronto_para_atualizar: false,
            efeitos_issue: None,
            resto: Default::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Mudancas {
    pub arquivos: u32,
    pub mais: u32,
    pub menos: u32,
    pub em: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entrega {
    pub resumo: String,
    pub como_verificou: String,
    pub criterios: Vec<CriterioEntregue>,
    pub pendencias: String,
    pub rodada: u32,
    pub em: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CriterioEntregue {
    pub indice: usize,
    /// "feito", "parcial" ou "nao".
    pub estado: String,
    pub evidencia: String,
}

impl Tarefa {
    /// O grupo dos pontos de volta da tarefa ("t12"): as fotos, a poda e a limpeza da cópia.
    pub fn grupo(&self) -> String {
        format!("t{}", self.id)
    }

    /// Quem fez o pedido aberto: "mestre", o chat pai (ajudante); `None` é o usuário.
    pub fn dono(&self) -> Option<&str> {
        Some(self.coordenador.as_str()).filter(|d| !d.is_empty())
    }

    pub fn tem_copia(&self) -> bool {
        self.forma == Forma::Copia
    }

    pub fn tem_git(&self) -> bool {
        matches!(self.forma, Forma::Copia | Forma::DiretaGit)
    }

    /// A cópia foi apagada: limpa, ou removida numa aprovação (que antes não marcava `copia_limpa`).
    pub fn copia_apagada(&self) -> bool {
        self.copia_limpa || (self.worktree.is_none() && self.branch.is_some())
    }

    /// A verificação e o revisor estão conferindo a entrega agora.
    pub fn conferindo(&self) -> bool {
        self.estado == EstadoTarefa::EmRevisao && self.progresso.is_some()
    }

    /// A conferência da entrega ficou no meio (Vitral fechado): refaz ao retomar.
    pub fn conferencia_cortada(&self) -> bool {
        self.estado == EstadoTarefa::EmRevisao && self.entrega_pendente.is_some() && !self.conferindo()
    }

    /// A rodada atual foi aberta por um aviso do PR (CI, revisão, "Mandar ao chat").
    pub fn ajuste_do_pr(&self) -> Option<AjustePr> {
        self.pr.as_ref().and_then(|p| p.ajuste).filter(|a| a.rodada == self.rodada)
    }

    /// O pedido aberto é do usuário, sem dono, e mesmo assim conta como pedido (a entrega passa pela
    /// verificação e pelo revisor): o de uma issue ("Iniciar agente") e o ajuste aberto pelo PR.
    pub fn pedido_sem_dono(&self) -> bool {
        self.dono().is_none() && (self.pedido_da_issue || self.ajuste_do_pr().is_some())
    }

    /// O PR aberto da tarefa (aberto ou rascunho).
    pub fn pr_aberto(&self) -> Option<&Pr> {
        self.pr.as_ref().filter(|p| p.aberto())
    }

    /// A verificação e o parecer guardados são desta rodada: nem o trabalhador está ajustando nem
    /// a conferência está rodando ou ficou no meio.
    pub fn resultados_valem(&self) -> bool {
        !self.estado.ativa() && !self.conferindo() && !self.conferencia_cortada()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Problema {
    pub arquivo: String,
    pub linha: Option<u64>,
    /// "alta", "media" ou "baixa".
    pub gravidade: String,
    pub descricao: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Revisao {
    /// "aprovar" ou "ajustar".
    pub veredito: String,
    pub resumo: String,
    pub problemas: Vec<Problema>,
}

/// Configuração de uma pasta de projeto (o ícone e a cor ficam no `Projeto`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigProjeto {
    /// Comando rodado em cada cópia nova antes do trabalhador começar (ex.: npm install).
    pub preparacao: String,
    /// Comando que sobe o projeto para testar (botão "Rodar").
    pub rodar: String,
    /// Arquivos fora do git copiados para cada cópia (padrões; vazio vale `.env*` e
    /// `CLAUDE.local.md`).
    pub copiar: Vec<String>,
    /// Como as mudanças dos chats desta pasta chegam ao GitHub.
    #[serde(skip_serializing_if = "ConfigGithub::padrao")]
    pub github: ConfigGithub,
}

/// "Ao aprovar": juntar na pasta, ou abrir um PR no GitHub.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum ModoEntrega {
    #[default]
    Juntar,
    Pr,
}

/// Um modo desconhecido (o "PrEMesclar" de uma versão antiga da especificação, ou um de uma
/// versão mais nova) lê como Juntar: o seguro, e o arquivo continua abrindo.
impl<'de> Deserialize<'de> for ModoEntrega {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match serde_json::Value::deserialize(d)?.as_str() {
            Some("Pr") => ModoEntrega::Pr,
            _ => ModoEntrega::Juntar,
        })
    }
}

/// O GitHub de uma pasta ligada. Vazio vale o padrão de cada campo.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigGithub {
    pub entrega: ModoEntrega,
    /// "" = origin, senão o primeiro remote do GitHub.
    pub remoto: String,
    /// O ramo de destino; "" = o padrão do repositório.
    pub base: String,
    /// "" = "vitral/".
    pub prefixo_ramo: String,
    pub rascunho: bool,
    /// Num fork: os PRs vão para o original.
    pub pr_no_original: bool,
    /// "dono/nome" do destino dos PRs, reescrito a cada detecção (só cache).
    pub repositorio: String,
}

impl ConfigGithub {
    pub fn padrao(&self) -> bool {
        *self == ConfigGithub::default()
    }

    /// O prefixo dos ramos, com o padrão.
    #[allow(dead_code)] // Provisório: o ramo do chat de uma issue (G3a).
    pub fn prefixo(&self) -> &str {
        match self.prefixo_ramo.trim() {
            "" => crate::issues::PREFIXO_PADRAO,
            p => p,
        }
    }
}

impl ConfigProjeto {
    pub fn padroes_de_copia(&self) -> Vec<String> {
        if self.copiar.is_empty() {
            vec![".env*".into(), "CLAUDE.local.md".into()]
        } else {
            self.copiar.clone()
        }
    }
}

/// Formato do `estado.json` dos dados globais. 0 (ou ausente) é o do `.vitral/` de cada pasta de
/// trabalho; um maior que este abre o app só para leitura.
pub const VERSAO_ESTADO: u32 = 2;

/// Chaves que nenhum projeto pode ter: são o Mestre e o grupo dos chats sem projeto.
pub const CHAVES_RESERVADAS: [&str; 2] = ["mestre", "avulsos"];

/// Como os chats de um projeto trabalham. Decidida quando o chat nasce, com a pasta presente, e
/// gravada: nunca é deduzida de a tarefa ter ou não uma cópia.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Forma {
    /// Numa cópia (git worktree) da origem, que é uma pasta ligada com git.
    #[default]
    Copia,
    /// Direto na origem, que tem git (a pasta própria): commit no fim de cada vez, sem cópia.
    DiretaGit,
    /// Direto na origem, sem git (pasta ligada sem git, pasta dentro de outro repositório, avulso).
    Direta,
}

/// Projeto dos dados globais: um nome, a aparência e as pastas onde os chats dele trabalham.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Projeto {
    /// Chave estável: o nome da subpasta num projeto migrado, `novo_id()` num criado no app.
    pub id: String,
    /// Único pela chave (sem acento e sem diferenciar maiúsculas); as reservadas são proibidas.
    pub nome: String,
    /// Id em `ICONES`; vazio é palpite pela primeira pasta.
    pub icone: String,
    /// "#rrggbb"; vazio é a cor automática do id.
    pub cor: String,
    /// O que vai neste projeto (o Mestre encaminha por aqui); vazio usa o README.
    pub descricao: String,
    /// Vão no prompt de todo chat do projeto.
    pub instrucoes: String,
    /// A primeira é a principal. Vazio só num provisório.
    pub pastas: Vec<PastaLigada>,
    pub arquivado: bool,
    pub ordem: u32,
    pub criado_em: u64,
    /// Sobrepõem a configuração para os chats novos do projeto.
    pub permissao: Option<String>,
    pub modelo: Option<String>,
    pub conta: Option<String>,
    /// Pasta de trabalho do Vitral antigo de onde ele veio.
    pub importado_de: Option<PathBuf>,
    /// Recriado na carga para um chat órfão não sumir.
    pub provisorio: bool,
    /// O time do Linear do projeto (só ids e nomes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linear: Option<LinearDoProjeto>,
    /// Campos de versões mais novas, regravados como vieram.
    #[serde(flatten)]
    pub resto: serde_json::Map<String, serde_json::Value>,
}

/// O Linear de um projeto: o time e para onde as issues vão ao iniciar e ao mesclar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinearDoProjeto {
    pub time_id: String,
    /// "ENG".
    pub time_chave: String,
    pub time_nome: String,
    /// `None` é o automático (o `started` de menor posição); `Some("")` é não mudar; senão, o id.
    pub estado_iniciar: Option<String>,
    /// `None` é o automático (o `completed` de menor posição).
    pub estado_mesclar: Option<String>,
    /// A última pasta escolhida para as issues do Linear (num projeto com mais de uma).
    pub pasta: Option<PathBuf>,
}

impl Projeto {
    pub fn chave(&self) -> String {
        crate::dados::chave(&self.nome)
    }

    pub fn principal(&self) -> Option<&PastaLigada> {
        self.pastas.first()
    }
}

/// Uma pasta de um projeto, com a configuração dela (preparação, rodar e o que copiar).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PastaLigada {
    /// Canônico ao ligar; só muda por "Localizar pasta…".
    pub caminho: PathBuf,
    /// A pasta própria do projeto (`projetos/<slug>` nos dados), criada com git.
    pub propria: bool,
    #[serde(flatten)]
    pub config: ConfigProjeto,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub versao: u32,
    /// `bypassPermissions`, `auto`, `acceptEdits` ou `manual`.
    pub permissao: String,
    /// Modelo do Mestre (e das conversas antigas); vazio usa o padrão do Claude Code.
    #[serde(alias = "modelo_coordenador")]
    pub modelo_mestre: String,
    /// Modelo dos chats de projeto e dos ajudantes.
    #[serde(alias = "modelo_trabalhador")]
    pub modelo_chats: String,
    /// Chats de projeto e ajudantes rodando ao mesmo tempo.
    pub max_trabalhadores: usize,
    pub max_rodadas: u32,
    /// Apaga sozinho, ao abrir, as cópias das tarefas encerradas paradas há mais dias que isso; 0 é nunca.
    pub limpar_copias_dias: u32,
    /// Conta padrão das conversas desta pasta: id, "auto" ou vazio (principal).
    pub conta: String,
    /// Um revisor novo, só de leitura, revisa cada entrega antes do coordenador.
    pub revisor: bool,
    /// Modelo do revisor; vazio usa o dos trabalhadores.
    pub modelo_revisor: String,
    /// Trabalhadores também carregam os servidores MCP do Claude Code do usuário.
    pub mcp_trabalhadores: bool,
    /// Arquiva sozinho as conversas paradas há tantos dias (0 = nunca).
    pub arquivar_apos_dias: u32,
    /// O Mestre começa a divisão sem mostrar o cartão.
    pub comecar_sem_perguntar: bool,
    /// Entrega verde é juntada pelo motor sozinho.
    pub aprovar_sozinho: bool,
    pub max_ajudantes: usize,
    /// Fora da janela por mais que isso, a volta mostra o resumo.
    pub minutos_para_resumo: u64,
    /// As contas ligadas (a do Linear sem a chave, que fica no chaveiro).
    pub conexoes: Conexoes,
    /// "Agora não" no popover das issues: o botão do Linear some das conversas.
    pub esconder_botao_linear: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Conexoes {
    pub linear: Option<ContaLinear>,
}

/// Quem está conectado no Linear. A chave nunca vem para cá: só os 4 últimos caracteres.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContaLinear {
    pub workspace: String,
    pub url_key: String,
    pub nome: String,
    pub id_usuario: String,
    pub final_da_chave: String,
}

/// Pedido que o Vitral manda sozinho para um coordenador novo, num horário (com o Vitral aberto).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Rotina {
    pub id: String,
    pub projeto: String,
    pub texto: String,
    /// Minutos desde a meia-noite.
    pub hora: u32,
    /// Dias da semana (0 = domingo).
    pub dias: Vec<u8>,
    pub ativa: bool,
    /// Data (AAAA-MM-DD) da última vez que rodou.
    pub ultima: String,
}

/// Um instante Unix no fuso local (o do sistema, como o `localtime_r`); fora do alcance do chrono,
/// o próprio 1970.
pub fn no_fuso_local(segundos: i64) -> chrono::DateTime<chrono::Local> {
    chrono::DateTime::from_timestamp(segundos, 0).unwrap_or_default().with_timezone(&chrono::Local)
}

/// Dia da semana (0 = domingo), minutos desde a meia-noite e a data AAAA-MM-DD, no fuso local.
pub fn agora_local() -> (u8, u32, String) {
    use chrono::{Datelike as _, Timelike as _};
    let local = no_fuso_local(agora() as i64);
    (
        local.weekday().num_days_from_sunday() as u8,
        local.hour() * 60 + local.minute(),
        format!("{:04}-{:02}-{:02}", local.year(), local.month(), local.day()),
    )
}

/// "12/10", no fuso local.
pub fn data_curta(segundos: u64) -> String {
    use chrono::Datelike as _;
    let local = no_fuso_local(segundos as i64);
    format!("{:02}/{:02}", local.day(), local.month())
}

/// "14:32", no fuso local: a hora de um item da conversa.
pub fn hora_local(segundos: u64) -> String {
    use chrono::Timelike as _;
    let local = no_fuso_local(segundos as i64);
    format!("{:02}:{:02}", local.hour(), local.minute())
}

/// O dia de um instante, no fuso local (ano e dia do ano): dois itens com o mesmo par ficam no
/// mesmo grupo da conversa.
pub fn dia_local(segundos: u64) -> (i32, u32) {
    use chrono::Datelike as _;
    let local = no_fuso_local(segundos as i64);
    (local.year(), local.ordinal())
}

const MESES: [&str; 12] = [
    "janeiro",
    "fevereiro",
    "março",
    "abril",
    "maio",
    "junho",
    "julho",
    "agosto",
    "setembro",
    "outubro",
    "novembro",
    "dezembro",
];

/// O nome do dia no separador de data da conversa: "Hoje", "Ontem", "12 de março" no ano de
/// `agora` e "12 de março de 2025" nos outros.
pub fn nome_do_dia(segundos: u64, agora: u64) -> String {
    use chrono::Datelike as _;
    let (dia, hoje) = (no_fuso_local(segundos as i64), no_fuso_local(agora as i64));
    let ontem = hoje.date_naive().pred_opt().unwrap_or(hoje.date_naive());
    if dia.date_naive() == hoje.date_naive() {
        "Hoje".to_string()
    } else if dia.date_naive() == ontem {
        "Ontem".to_string()
    } else {
        let mes = MESES[(dia.month() as usize - 1).min(11)];
        let texto = format!("{} de {mes}", dia.day());
        if dia.year() == hoje.year() { texto } else { format!("{texto} de {}", dia.year()) }
    }
}

/// "às 15:42", ou "em 03/11 às 15:42" quando não é hoje, no fuso local.
pub fn quando_local(segundos: u64) -> String {
    use chrono::{Datelike as _, Timelike as _};
    let (local, hoje) = (no_fuso_local(segundos as i64), no_fuso_local(agora() as i64));
    let hora = format!("às {:02}:{:02}", local.hour(), local.minute());
    if (local.year(), local.ordinal()) == (hoje.year(), hoje.ordinal()) {
        hora
    } else {
        format!("em {:02}/{:02} {hora}", local.day(), local.month())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            versao: 0,
            permissao: "bypassPermissions".into(),
            modelo_mestre: String::new(),
            modelo_chats: String::new(),
            max_trabalhadores: 4,
            max_rodadas: 3,
            limpar_copias_dias: 0,
            conta: String::new(),
            revisor: true,
            modelo_revisor: String::new(),
            mcp_trabalhadores: false,
            arquivar_apos_dias: 0,
            comecar_sem_perguntar: false,
            aprovar_sozinho: true,
            max_ajudantes: 3,
            minutos_para_resumo: 15,
            conexoes: Conexoes::default(),
            esconder_botao_linear: false,
        }
    }
}

/// Uma mensagem a caminho de um chat. `de` vazio é o usuário; anexos são arquivos dele.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entrada {
    pub de: Option<String>,
    pub texto: String,
    #[serde(default)]
    pub anexos: Vec<PathBuf>,
    /// Já está na conversa (nova tentativa que esperou vaga): não entra de novo como item.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub registrada: bool,
    /// Pensar mais neste turno: a palavra vai no fim do texto e o esforço sobe (ver
    /// `motor::esforco_do_ultrathink`). Fica na fila e sobrevive a um reinício.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ultrathink: bool,
    /// Turno com Ultracode (`--effort ultracode`): o Claude orquestra vários agentes por um
    /// fluxo. Só o usuário liga, só para esta mensagem.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ultracode: bool,
    /// Chat que mandou; `None` é o usuário ou o Vitral.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remetente: Option<String>,
    /// Não abre turno sozinha: entra junto com a próxima entrada que abre.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nota: bool,
    /// Rotina que mandou.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotina: Option<String>,
    /// Vai só ao Claude, sem aparecer na conversa (o bloco de uma issue anexada).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub extra: String,
    /// Traz texto de terceiros (issue, comentário de PR, log de CI): o turno nunca faz ação
    /// externa sem confirmação, e o chat passa a rodar isolado.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub terceiros: bool,
    /// As issues anexadas ("ENG-123"), para o chip na bolha.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub referencias: Vec<String>,
}

impl Entrada {
    /// Escrita pelo usuário (nem o Vitral, nem outro chat, nem uma rotina).
    pub fn do_usuario(&self) -> bool {
        self.de.is_none() && self.rotina.is_none() && self.remetente.is_none()
    }
}

/// O que a caixa ligou só para a próxima mensagem.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reforco {
    pub ultrathink: bool,
    pub ultracode: bool,
}

/// Mensagens esperando o chat ficar livre.
pub type Filas = HashMap<String, Vec<Entrada>>;

#[derive(Default, Serialize, Deserialize)]
pub struct Estado {
    /// 0 no `.vitral/` de uma pasta de trabalho; `VERSAO_ESTADO` nos dados globais.
    #[serde(default)]
    pub versao: u32,
    #[serde(default)]
    pub projetos: Vec<Projeto>,
    pub chats: Vec<Chat>,
    pub tarefas: Vec<Tarefa>,
    pub proxima_tarefa: u64,
    #[serde(default)]
    pub filas: Filas,
    #[serde(default)]
    pub rotinas: Vec<Rotina>,
    #[serde(default)]
    pub cartoes: Vec<Cartao>,
    #[serde(default)]
    pub proximo_cartao: u64,
    #[serde(default)]
    pub ocorrencias: Vec<Ocorrencia>,
    /// Quando o usuário saiu da janela.
    #[serde(default)]
    pub visto_em: u64,
    /// Campos de versões mais novas, regravados como vieram.
    #[serde(flatten)]
    pub resto: serde_json::Map<String, serde_json::Value>,
}

/// O mesmo `Estado` emprestado, para gravar sem clonar os chats com todas as mensagens.
#[derive(Serialize)]
pub struct EstadoRef<'a> {
    pub versao: u32,
    pub projetos: &'a [Projeto],
    pub chats: &'a [Chat],
    pub tarefas: &'a [Tarefa],
    pub proxima_tarefa: u64,
    pub filas: &'a Filas,
    pub rotinas: &'a [Rotina],
    pub cartoes: &'a [Cartao],
    pub proximo_cartao: u64,
    pub ocorrencias: &'a [Ocorrencia],
    pub visto_em: u64,
    #[serde(flatten)]
    pub resto: &'a serde_json::Map<String, serde_json::Value>,
}

impl Estado {
    #[cfg(test)]
    pub fn por_referencia(&self) -> EstadoRef<'_> {
        EstadoRef {
            versao: self.versao,
            projetos: &self.projetos,
            chats: &self.chats,
            tarefas: &self.tarefas,
            proxima_tarefa: self.proxima_tarefa,
            filas: &self.filas,
            rotinas: &self.rotinas,
            cartoes: &self.cartoes,
            proximo_cartao: self.proximo_cartao,
            ocorrencias: &self.ocorrencias,
            visto_em: self.visto_em,
            resto: &self.resto,
        }
    }
}

/// Até quantas ocorrências o estado guarda.
pub const MAX_OCORRENCIAS: usize = 500;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cartao {
    pub id: u64,
    /// Onde aparece (sempre "mestre" por enquanto).
    pub chat: String,
    pub criado_em: u64,
    #[serde(default)]
    pub decidido_em: u64,
    pub estado: EstadoCartao,
    pub tipo: TipoCartao,
    #[serde(flatten)]
    pub resto: serde_json::Map<String, serde_json::Value>,
}

impl Cartao {
    pub fn aberto(&self) -> bool {
        self.estado == EstadoCartao::Aberto
    }

    pub fn divisao(&self) -> Option<&Divisao> {
        match &self.tipo {
            TipoCartao::Divisao(d) => Some(d),
            _ => None,
        }
    }

    pub fn divisao_mut(&mut self) -> Option<&mut Divisao> {
        match &mut self.tipo {
            TipoCartao::Divisao(d) => Some(d),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EstadoCartao {
    Aberto,
    Aceito,
    Recusado,
    Substituido,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TipoCartao {
    Divisao(Divisao),
    Projeto(SugestaoProjeto),
    Memoria {
        texto: String,
    },
    /// A linha de acompanhamento do PR de uma tarefa (os dados vêm ao vivo de `Tarefa.pr`).
    Pr {
        tarefa: u64,
    },
    /// Uma ação com efeito fora do Vitral que um chat pediu e o usuário confirma.
    Confirmar(Confirmacao),
    #[serde(untagged)]
    Desconhecido(serde_json::Value),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Confirmacao {
    pub acao: AcaoExterna,
    /// O chat que pediu.
    pub pedido_por: String,
    pub resumo: String,
    /// O que aconteceu depois de decidido.
    #[serde(default)]
    pub resultado: Option<String>,
}

/// O que tem efeito no GitHub e passa pela guarda (seção 8.1 da especificação).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AcaoExterna {
    AbrirPr { tarefa: u64, titulo: String, descricao: String, rascunho: bool, quando_pronto: Option<String> },
    AtualizarPr { tarefa: u64 },
    Mesclar { tarefa: u64, metodo: String, sha: String, quando_pronto: bool },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Divisao {
    pub resumo: String,
    pub partes: Vec<Parte>,
    pub perguntas: Vec<String>,
    /// O texto das entradas do usuário do turno (até 8000 caracteres).
    pub pedido_original: String,
    pub anexos: Vec<PathBuf>,
    /// A chave do turno que propôs.
    pub turno: String,
    /// Projeto, chat e título de cada parte, em ordem.
    pub assinatura: String,
    pub sem_perguntar: bool,
    pub por_referencia: bool,
    pub conclusao_avisada: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Parte {
    /// Id; "" é Avulsos.
    pub projeto: String,
    pub pasta: Option<PathBuf>,
    /// Chat que já existe e recebe a parte.
    pub chat: Option<String>,
    pub titulo: String,
    pub pedido: String,
    pub criterios: Vec<String>,
    pub verificacao: Option<String>,
    pub modelo: Option<String>,
    pub arquivos: Vec<String>,
    pub aviso: Option<String>,
    pub removida: bool,
    pub criada: Option<ParteCriada>,
    pub falha: Option<String>,
    /// "juntada" ou "cancelada".
    pub resultado: Option<String>,
    /// Onde foi juntada ("em ~/Projetos/loja") ou por que foi cancelada.
    pub detalhe: Option<String>,
    /// A issue que esta parte resolve (o `issue` do `propor_divisao`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<IssueLigada>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ParteCriada {
    pub chat: String,
    pub tarefa: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SugestaoProjeto {
    pub nome: String,
    pub descricao: String,
    pub instrucoes: String,
    pub icone: String,
    pub cor: String,
    pub pastas: Vec<PathBuf>,
    pub motivo: String,
    pub pedido_pendente: String,
    /// Id do projeto criado.
    pub criado: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Ocorrencia {
    pub quando: u64,
    /// "pedido", "entrega", "juntou", "conflito", "falhou", "parou", "permissao", "pergunta",
    /// "divisao", "projeto", "cancelada", "ciclo" ou "migracao" (texto: uma versão nova não quebra).
    pub tipo: String,
    /// "usuario", "mestre", "vitral" ou o id de um chat.
    pub autor: String,
    pub chat: String,
    pub projeto: String,
    pub tarefa: Option<u64>,
    pub texto: String,
}

impl Chat {
    /// Cópia dos campos do chat com `itens` vazio, sem passar pelo histórico (que pode ter MBs).
    pub fn copia_sem_itens(&mut self) -> Chat {
        let itens = std::mem::take(&mut self.itens);
        let copia = self.clone();
        self.itens = itens;
        copia
    }
}

#[cfg(test)]
mod testes {
    use std::fs;

    use super::*;

    /// 12/03/2026 ao meio-dia UTC: ao meio-dia a data local é a mesma em qualquer fuso de verdade,
    /// e o teste não depende do fuso de quem o roda.
    const MEIO_DIA: u64 = 1_773_316_800;
    const DIA: u64 = 86_400;

    #[test]
    fn nome_do_dia_de_cada_separador() {
        assert_eq!(nome_do_dia(MEIO_DIA, MEIO_DIA), "Hoje");
        assert_eq!(nome_do_dia(MEIO_DIA - DIA, MEIO_DIA), "Ontem");
        assert_eq!(nome_do_dia(MEIO_DIA - 2 * DIA, MEIO_DIA), "10 de março");
        // Outro ano leva o ano no rótulo (o 12/03/2025 cai 365 dias antes).
        assert_eq!(nome_do_dia(MEIO_DIA - 365 * DIA, MEIO_DIA), "12 de março de 2025");
        // Depois de hoje (o relógio do sistema voltou) não é "Ontem" nem "Hoje".
        assert_eq!(nome_do_dia(MEIO_DIA + DIA, MEIO_DIA), "13 de março");
    }

    #[test]
    fn hora_local_no_formato_do_relogio() {
        let minuto = MEIO_DIA - MEIO_DIA % 60;
        let hora = hora_local(minuto);
        assert_eq!(hora.len(), 5, "{hora}");
        assert_eq!(hora.as_bytes()[2], b':', "{hora}");
        assert!(hora.bytes().enumerate().all(|(i, c)| i == 2 || c.is_ascii_digit()), "{hora}");
        // Os segundos não aparecem; o minuto seguinte muda o texto.
        assert_eq!(hora_local(minuto + 59), hora);
        assert_ne!(hora_local(minuto + 60), hora);
    }

    #[test]
    fn dia_local_agrupa_o_mesmo_dia() {
        assert_eq!(dia_local(MEIO_DIA), dia_local(MEIO_DIA + 3600));
        assert_ne!(dia_local(MEIO_DIA), dia_local(MEIO_DIA + DIA));
        // Fora do alcance do chrono, a virada de 1970 (nunca um pânico).
        assert!(dia_local(u64::MAX).0 <= 1970, "{:?}", dia_local(u64::MAX));
    }

    #[test]
    fn item_sem_hora_nao_vai_para_o_disco() {
        let sem = Item::Assistente { texto: "oi".into(), em: 0 };
        assert_eq!(serde_json::to_value(&sem).unwrap(), serde_json::json!({ "Assistente": { "texto": "oi" } }));
        let com = Item::Assistente { texto: "oi".into(), em: MEIO_DIA };
        assert_eq!(
            serde_json::to_value(&com).unwrap(),
            serde_json::json!({ "Assistente": { "texto": "oi", "em": MEIO_DIA } })
        );
        // O item de uma conversa de antes da hora abre sem ela, e não como desconhecido.
        let lido: Item = serde_json::from_value(serde_json::json!({ "Assistente": { "texto": "oi" } })).unwrap();
        assert!(matches!(&lido, Item::Assistente { texto, em: 0 } if texto == "oi"));
        assert_eq!(lido.em(), 0);
        assert_eq!(Item::Desconhecido(serde_json::json!({ "Coisa": {} })).em(), 0);
    }

    fn chat_de_teste() -> Chat {
        let mut chat: Chat = serde_json::from_value(serde_json::json!({
            "id": "c1", "projeto": "loja", "titulo": "Conversa", "papel": "Trabalhador",
            "pai": "c0", "tarefa": 3, "sessao": "s", "sessao_criada": true, "cwd": "/tmp/loja",
            "modelo": "opus", "criado_em": 1, "atualizado_em": 2, "custo_usd": 0.5
        }))
        .unwrap();
        chat.itens.push(Item::Assistente { texto: "oi".into(), em: 0 });
        chat
    }

    #[test]
    fn custo_soma_so_o_que_a_sessao_cresceu() {
        let mut chat = chat_de_teste();
        let perto = |a: f64, b: f64| (a - b).abs() < 1e-9;
        chat.somar_custo(0.25);
        assert!(perto(chat.custo_usd, 0.75), "{}", chat.custo_usd);
        // A retomada continua do total gravado: 0,25 dos turnos anteriores e 0,15 deste.
        chat.somar_custo(0.4);
        assert!(perto(chat.custo_usd, 0.9), "{}", chat.custo_usd);
        // O `result` zerado de um processo que caiu ao abrir.
        chat.somar_custo(0.0);
        assert!(perto(chat.custo_usd, 0.9), "{}", chat.custo_usd);
        // Total menor: a sessão recomeçou do zero (o total não foi gravado na queda).
        chat.somar_custo(0.1);
        assert!(perto(chat.custo_usd, 1.0), "{}", chat.custo_usd);
        // Sessão nova conta do zero, mesmo com um total maior que o visto na anterior.
        chat.sessao = "s2".into();
        chat.somar_custo(0.3);
        assert!(perto(chat.custo_usd, 1.3), "{}", chat.custo_usd);
        let gravado = serde_json::to_value(&chat).unwrap();
        assert_eq!(gravado["custo_da_sessao"], serde_json::json!(["s2", 0.3]));
    }

    #[test]
    fn estado_por_referencia_grava_o_mesmo_json() {
        let chat = chat_de_teste();
        let tarefa: Tarefa = serde_json::from_value(serde_json::json!({
            "id": 3, "projeto": "loja", "coordenador": "c0", "trabalhador": "c1",
            "titulo": "t", "objetivo": "o", "criterios": ["a"], "estado": "Rodando",
            "rodada": 1, "criada_em": 5
        }))
        .unwrap();
        let mut filas = Filas::new();
        filas.insert("c1".into(), vec![Entrada { texto: "depois".into(), ..Default::default() }]);
        let mut resto = serde_json::Map::new();
        resto.insert("de_uma_versao_nova".into(), serde_json::json!({"x": 1}));
        let estado = Estado {
            versao: VERSAO_ESTADO,
            projetos: vec![Projeto { id: "loja".into(), nome: "Loja".into(), ..Default::default() }],
            chats: vec![chat],
            tarefas: vec![tarefa],
            proxima_tarefa: 4,
            filas,
            rotinas: vec![Rotina { id: "r1".into(), projeto: "loja".into(), ..Default::default() }],
            cartoes: vec![Cartao {
                id: 1,
                chat: ID_MESTRE.into(),
                criado_em: 2,
                decidido_em: 0,
                estado: EstadoCartao::Aberto,
                tipo: TipoCartao::Memoria { texto: "lembrar".into() },
                resto: Default::default(),
            }],
            proximo_cartao: 2,
            ocorrencias: vec![Ocorrencia { quando: 3, tipo: "pedido".into(), ..Default::default() }],
            visto_em: 9,
            resto,
        };
        let referencia = serde_json::to_value(estado.por_referencia()).unwrap();
        assert_eq!(referencia, serde_json::to_value(&estado).unwrap());
        let lido: Estado = serde_json::from_value(referencia.clone()).unwrap();
        assert_eq!(lido.chats[0].id, "c1");
        assert_eq!(lido.proxima_tarefa, 4);
        assert_eq!(lido.filas["c1"][0].texto, "depois");
        assert_eq!(lido.resto["de_uma_versao_nova"], serde_json::json!({"x": 1}));
        assert_eq!(serde_json::to_value(lido.por_referencia()).unwrap(), referencia);
    }

    #[test]
    fn desconhecido_e_resto_voltam_iguais() {
        let item = serde_json::json!({"Divisao": {"partes": [1, 2]}});
        let itens: Vec<Item> = serde_json::from_value(serde_json::json!([
            {"Assistente": {"texto": "oi"}}, item.clone(), {"Cartao": {"id": 4}}
        ]))
        .unwrap();
        assert!(matches!(&itens[1], Item::Desconhecido(v) if *v == item));
        assert!(matches!(itens[2], Item::Cartao { id: 4, .. }));
        assert_eq!(serde_json::to_value(&itens).unwrap()[1], item);

        let mut chat = serde_json::to_value(chat_de_teste()).unwrap();
        chat["campo_novo"] = serde_json::json!([1, 2]);
        let lido: Chat = serde_json::from_value(chat.clone()).unwrap();
        assert_eq!(lido.resto["campo_novo"], serde_json::json!([1, 2]));
        assert_eq!(serde_json::to_value(&lido).unwrap(), chat);

        let tarefa = serde_json::json!({
            "id": 3, "projeto": "loja", "coordenador": "", "trabalhador": "c1", "titulo": "t",
            "objetivo": "o", "criterios": [], "estado": "Conversa", "rodada": 1, "criada_em": 5,
            "forma": "DiretaGit", "origem": "/p/loja", "vindo_de_longe": true
        });
        let lida: Tarefa = serde_json::from_value(tarefa).unwrap();
        assert_eq!((lida.estado, lida.forma), (EstadoTarefa::Conversa, Forma::DiretaGit));
        assert!(!lida.estado.ativa() && !lida.estado.encerrada());
        assert_eq!(lida.origem, PathBuf::from("/p/loja"));
        assert_eq!(serde_json::to_value(&lida).unwrap()["vindo_de_longe"], true);
        assert_eq!(lida.dono(), None);

        let cartao = serde_json::json!({
            "id": 1, "chat": "mestre", "criado_em": 0, "estado": "Aberto",
            "tipo": {"Futuro": {"a": 1}}, "extra": "x"
        });
        let lido: Cartao = serde_json::from_value(cartao).unwrap();
        assert!(matches!(&lido.tipo, TipoCartao::Desconhecido(_)));
        let regravado = serde_json::to_value(&lido).unwrap();
        assert_eq!(regravado["tipo"], serde_json::json!({"Futuro": {"a": 1}}));
        assert_eq!(regravado["extra"], "x");
    }

    #[test]
    fn entrada_antiga_le_com_os_padroes_e_grava_igual() {
        let antiga = serde_json::json!({"de": null, "texto": "oi", "anexos": []});
        let lida: Entrada = serde_json::from_value(antiga.clone()).unwrap();
        assert!(lida.remetente.is_none() && !lida.nota && lida.rotina.is_none() && lida.extra.is_empty());
        assert_eq!(serde_json::to_value(&lida).unwrap(), antiga);
    }

    #[test]
    fn papel_pela_rota() {
        for papel in [Papel::Coordenador, Papel::Trabalhador, Papel::Mestre, Papel::Projeto] {
            assert_eq!(Papel::da_rota(papel.rota()), Some(papel));
        }
        assert_eq!(Papel::da_rota("intruso"), None);
        assert_eq!(Papel::da_rota(""), None);
        assert!(Papel::Projeto.conta_vaga() && Papel::Trabalhador.conta_vaga() && !Papel::Mestre.conta_vaga());
        assert!(!Papel::Trabalhador.de_topo() && Papel::Mestre.de_topo());
    }

    #[test]
    fn copia_sem_itens_deixa_o_historico_no_original() {
        let mut chat = chat_de_teste();
        let copia = chat.copia_sem_itens();
        assert!(copia.itens.is_empty());
        assert_eq!(chat.itens.len(), 1);
        assert_eq!(
            serde_json::to_value(&copia).unwrap(),
            serde_json::to_value(&chat).unwrap()
        );
    }

    #[test]
    fn chat_gravado_em_segundo_plano_volta_igual() {
        let raiz = std::env::temp_dir().join(format!("vitral-gravador-{}", novo_id()));
        let dados = crate::dados::Dados::em(&raiz);
        let mut chat = chat_de_teste();
        let estado = Estado { chats: vec![chat.clone()], proxima_tarefa: 1, ..Default::default() };
        dados.salvar_estado(&estado.por_referencia()).unwrap();
        dados.salvar_chat(&chat).unwrap();
        chat.itens.push(Item::Assistente { texto: "tchau".into(), em: 0 });
        dados.salvar_chat(&chat).unwrap();
        dados.esperar_gravacoes();
        let carregado = dados.carregar().unwrap();
        assert_eq!(carregado.estado.chats[0].itens.len(), 2);
        assert!(!crate::dados::com_sufixo(&dados.arquivo_chat("c1"), ".tmp").exists());
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn tarefa_antiga_conta_como_pronta() {
        let tarefa: Tarefa = serde_json::from_value(serde_json::json!({
            "id": 5, "projeto": "p", "coordenador": "c", "trabalhador": "w", "titulo": "t",
            "objetivo": "o", "criterios": ["c"], "estado": "Rodando", "rodada": 1, "criada_em": 0,
        }))
        .unwrap();
        assert!(tarefa.preparada);
        assert_eq!((tarefa.forma, tarefa.porta), (Forma::Copia, 0));
    }

    #[test]
    fn apagar_na_fila_tira_a_gravacao_pendente_e_o_arquivo() {
        let raiz = std::env::temp_dir().join(format!("vitral-apagar-{}", novo_id()));
        let dados = crate::dados::Dados::em(&raiz);
        let chat = chat_de_teste();
        dados.salvar_chat(&chat).unwrap();
        dados.esperar_gravacoes();
        assert!(dados.arquivo_chat("c1").exists());
        dados.salvar_chat(&chat).unwrap();
        dados.apagar_chat("c1");
        dados.anotar("loja", "nota").unwrap();
        dados.apagar_notas("loja");
        dados.apagar_chat("nunca-existiu");
        dados.esperar_gravacoes();
        assert!(!dados.arquivo_chat("c1").exists());
        assert!(!dados.arquivo_notas("loja").exists());
        assert_eq!(dados.notas("loja"), "");
        assert_eq!(dados.falha_de_gravacao(), None);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn projeto_antigo_abre_com_os_padroes_do_github_e_do_linear() {
        let antigo = serde_json::json!({
            "id": "a1", "nome": "Loja", "pastas": [{"caminho": "/p/loja", "propria": false, "preparacao": "npm i"}],
            "ordem": 2, "campo_novo": 1
        });
        let projeto: Projeto = serde_json::from_value(antigo.clone()).unwrap();
        assert!(projeto.linear.is_none());
        let github = &projeto.pastas[0].config.github;
        assert_eq!(github.entrega, ModoEntrega::Juntar);
        assert_eq!((github.prefixo(), github.base.as_str(), github.rascunho), ("vitral/", "", false));
        // Sem nada do GitHub e do Linear configurado, nada disso vai para o arquivo.
        let gravado = serde_json::to_value(&projeto).unwrap();
        assert!(gravado.get("linear").is_none() && gravado["pastas"][0].get("github").is_none(), "{gravado}");
        assert_eq!((&gravado["campo_novo"], &gravado["pastas"][0]["preparacao"]), (&antigo["campo_novo"], &antigo["pastas"][0]["preparacao"]));

        let tarefa: Tarefa = serde_json::from_value(serde_json::json!({
            "id": 5, "projeto": "a1", "coordenador": "", "trabalhador": "c", "titulo": "t", "objetivo": "o",
            "criterios": [], "estado": "Conversa", "rodada": 2, "criada_em": 0
        }))
        .unwrap();
        assert!(tarefa.pr.is_none() && tarefa.issue.is_none() && tarefa.ramo_planejado.is_none() && tarefa.partir_de.is_none());
        assert_eq!((tarefa.rodada_do_ciclo, tarefa.recomeco_pendente, tarefa.pronto_para_atualizar), (0, false, false));
        let gravada = serde_json::to_value(&tarefa).unwrap();
        for campo in ["pr", "prs_anteriores", "issue", "partir_de", "arquivos_copiados", "pedido_da_issue"] {
            assert!(gravada.get(campo).is_none(), "{campo}");
        }
        let chat: Chat = serde_json::from_value(serde_json::to_value(chat_de_teste()).unwrap()).unwrap();
        assert!(!chat.isolado && chat.acoes_externas.is_empty());
        let config: Config = serde_json::from_value(serde_json::json!({"permissao": "auto"})).unwrap();
        assert!(config.conexoes.linear.is_none() && !config.esconder_botao_linear);
        let parte: Parte = serde_json::from_value(serde_json::json!({"projeto": "a1", "titulo": "x"})).unwrap();
        assert!(parte.issue.is_none());
    }

    #[test]
    fn modo_de_entrega_ausente_ou_desconhecido_e_juntar() {
        let ler = |valor: serde_json::Value| serde_json::from_value::<ConfigGithub>(valor).unwrap().entrega;
        assert_eq!(ler(serde_json::json!({})), ModoEntrega::Juntar);
        assert_eq!(ler(serde_json::json!({"entrega": "Pr"})), ModoEntrega::Pr);
        // O "PR e mesclar" saiu do v1: não existe, e um arquivo com ele continua abrindo.
        assert_eq!(ler(serde_json::json!({"entrega": "PrEMesclar"})), ModoEntrega::Juntar);
        assert_eq!(ler(serde_json::json!({"entrega": 3})), ModoEntrega::Juntar);
        let pasta = PastaLigada {
            caminho: "/p/loja".into(),
            config: ConfigProjeto {
                github: ConfigGithub { entrega: ModoEntrega::Pr, prefixo_ramo: "feature/vitor-".into(), ..Default::default() },
                ..Default::default()
            },
            ..Default::default()
        };
        let gravada = serde_json::to_value(&pasta).unwrap();
        assert_eq!(gravada["github"]["entrega"], "Pr");
        let lida: PastaLigada = serde_json::from_value(gravada).unwrap();
        assert_eq!((lida.config.github.entrega, lida.config.github.prefixo()), (ModoEntrega::Pr, "feature/vitor-"));
    }

    #[test]
    fn estados_e_cartoes_novos_voltam_iguais() {
        assert_eq!(EstadoTarefa::NoPr.nome(), "PR aberto");
        assert!(!EstadoTarefa::NoPr.ativa() && !EstadoTarefa::NoPr.encerrada());
        let cartoes = [
            TipoCartao::Pr { tarefa: 12 },
            TipoCartao::Confirmar(Confirmacao {
                acao: AcaoExterna::Mesclar { tarefa: 12, metodo: "squash".into(), sha: "abc1234".into(), quando_pronto: false },
                pedido_por: "c12".into(),
                resumo: "mesclar o PR #12 (squash)".into(),
                resultado: None,
            }),
        ];
        for tipo in cartoes {
            let valor = serde_json::to_value(&tipo).unwrap();
            let lido: TipoCartao = serde_json::from_value(valor.clone()).unwrap();
            assert!(!matches!(lido, TipoCartao::Desconhecido(_)), "{valor}");
            assert_eq!(serde_json::to_value(&lido).unwrap(), valor);
        }
        let mut tarefa = Tarefa { estado: EstadoTarefa::NoPr, ..Default::default() };
        tarefa.pr = Some(Pr { numero: 12, estado: EstadoPr::Rascunho, acompanhando: true, ..Default::default() });
        let lida: Tarefa = serde_json::from_value(serde_json::to_value(&tarefa).unwrap()).unwrap();
        assert!(lida.pr.as_ref().is_some_and(|pr| pr.aberto() && pr.numero == 12 && pr.acompanhando));
    }

    /// Invariante 1 da especificação: com o Linear conectado e um PR com erro, nenhum arquivo
    /// gravado tem um token (a chave fica só no chaveiro e na memória).
    #[test]
    fn nenhum_arquivo_gravado_tem_token() {
        let raiz = std::env::temp_dir().join(format!("vitral-sem-token-{}", novo_id()));
        let dados = crate::dados::Dados::em(&raiz);
        let chave = crate::segredos::Chave::linear(concat!("lin_", "api_ABCdef0123456789xyzWa1b2")).unwrap();
        let config = Config {
            conexoes: Conexoes {
                linear: Some(ContaLinear {
                    workspace: "Eita".into(),
                    url_key: "eita".into(),
                    nome: "Victor".into(),
                    id_usuario: "u1".into(),
                    final_da_chave: chave.final_(),
                }),
            },
            ..Default::default()
        };
        let erro_do_gh = concat!("remote: token gh", "p_ABCdef0123456789xyzWa1b2 recusado (Authorization: Bearer x)");
        let tarefa = Tarefa {
            id: 3,
            estado: EstadoTarefa::NoPr,
            pr: Some(Pr { numero: 12, erro: Some(crate::remoto::sem_segredos(erro_do_gh)), ..Default::default() }),
            issue: Some(IssueLigada { chave: "ENG-5".into(), ..Default::default() }),
            ..Default::default()
        };
        let mut chat = chat_de_teste();
        chat.isolado = true;
        chat.acoes_externas.push(RegistroAcao { turno: "u".into(), acao: "{}".into(), resultado: "ok".into(), em: 1 });
        let estado = Estado { chats: vec![chat.clone()], tarefas: vec![tarefa], proxima_tarefa: 4, ..Default::default() };
        dados.salvar_estado(&estado.por_referencia()).unwrap();
        dados.salvar_config(&config).unwrap();
        dados.salvar_chat(&chat).unwrap();
        dados.esperar_gravacoes();
        let mut arquivos = vec![raiz.clone()];
        let mut lidos = 0;
        while let Some(caminho) = arquivos.pop() {
            if caminho.is_dir() {
                arquivos.extend(fs::read_dir(&caminho).unwrap().map(|e| e.unwrap().path()));
                continue;
            }
            let texto = fs::read_to_string(&caminho).unwrap_or_default();
            lidos += 1;
            for prefixo in ["lin_api_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"] {
                assert!(!texto.contains(prefixo), "{prefixo} em {}", caminho.display());
            }
        }
        assert!(lidos >= 3, "{lidos}");
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn configuracao_antiga_de_pasta_continua_abrindo() {
        let antiga = serde_json::json!({"preparacao": "pip install", "rodar": "", "icone": "store", "cor": "#fff", "arquivado": true});
        let config: ConfigProjeto = serde_json::from_value(antiga).unwrap();
        assert_eq!(config.preparacao, "pip install");
        assert_eq!(config.padroes_de_copia(), [".env*", "CLAUDE.local.md"]);
        let pasta: PastaLigada = serde_json::from_value(serde_json::json!({
            "caminho": "/p/loja", "propria": false, "preparacao": "npm i", "copiar": [".env"]
        }))
        .unwrap();
        assert_eq!((pasta.config.preparacao.as_str(), pasta.config.padroes_de_copia()), ("npm i", vec![".env".to_string()]));
    }
}
