//! O motor do Vitral: guarda chats e tarefas, roda os processos do Claude Code, atende as
//! ferramentas MCP e faz o ciclo coordenador → trabalhador → revisão.
//!
//! Vive numa única entidade da interface (`Entity<Motor>`). Os processos e o servidor MCP
//! rodam em threads e mandam `Sinal`s por um canal; o motor trata cada um na thread da
//! interface e as telas redesenham observando a entidade.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use gpui_kit::{AnyWeakEntity, AppContext as _, Context, Task};
use serde_json::{Value, json};
use smol::channel::{Sender, unbounded};

mod conexoes;
mod importacao;
mod issues;
mod mestre;
mod prs;

pub use conexoes::{
    Carregando, EstadoConexao, EstadoConexoes, EstadoGithub, RemotoGithub, estados_padrao, linear_com_time,
    modo_pr_disponivel, remoto_escolhido,
};
// Provisório: o PR (G4 e G5) usa o estado da rede, o repositório inteiro e os métodos permitidos.
#[allow(unused_imports)]
pub use conexoes::{Provedor, RepoGithub, metodos_permitidos};
// Provisório: a interface das issues (G3b) e a do PR (G5) passam a usar estes tipos.
#[allow(unused_imports)]
pub use issues::{AnexoIssue, EscopoIssues, ModoIssue, OpcoesDaIssue, ResultadoLote, VisaoLista, ligada_de};
pub use mestre::ResumoVolta;
#[allow(unused_imports)]
pub use prs::{AcaoPr, AndamentoPr, Confirmado, EventoLinha, FonteDiff, OrigemAcao, OrigemAjuste, PedidoPr, RodarDeNovo};
use mestre::{Ciclos, MotivoPassagem, Readmes};

use crate::{
    claude::{self, Evento, Mensagem, Opcoes, Processo, VERSAO_ULTRACODE, Versao},
    codex::{self, Codex},
    contas::{
        self, Conta, Plano,
        busca::{self, Agenda, Busca},
    },
    dados::{self, Carregado, Dados, ErroCarga, slug},
    git,
    mcp::{self, Chamada, Resposta},
    modelo::{
        Cartao, Chat, Config, ConfigProjeto, Entrada, Estado, EstadoRef, EstadoTarefa, Execucao, Filas,
        Forma, Item, Ocorrencia, Papel, PastaLigada, Projeto, Reforco, Rotina, TipoCartao,
        Tarefa, Verificacao, agora, novo_id,
    },
    prompts,
};

/// Motivo de parada quando foi o usuário que parou (não sobe para "Esperando você").
const PARADA_VOCE: &str = "você interrompeu";
const PARADA_FECHOU: &str = "o Vitral foi fechado no meio";
const PARADA_VERIFICACAO: &str = "a verificação foi interrompida";
const PARADA_PREPARACAO: &str = "a preparação da cópia foi interrompida";
const CORTADO: &str = "o Vitral foi fechado no meio do turno";
const VERIFICANDO: &str = "verificando e revisando a entrega";
const NA_FILA_CONFERIR: &str = "na fila para conferir";
const NA_FILA_PREPARAR: &str = "na fila para preparar a cópia";
/// O começo da recusa de juntar com a origem em outro branch: a interface confirma e chama
/// `juntar_no_ramo_atual`.
pub const RAMO_TROCADO: &str = "Você trocou de branch em";
/// Até onde vão as instruções do projeto (o prompt de cada chat as leva inteiras).
pub const MAX_INSTRUCOES: usize = 8000;
const TENTANDO: &str = "o processo caiu; tentando de novo";
/// Ajudante que acabou sem a cópia: ele nunca roda.
const SEM_COPIA_CRIADA: &str = "A cópia deste ajudante não foi criada, então ele não roda. Peça ao chat \
                                dele um ajudante novo.";
/// A conversa do Vitral antigo (o coordenador) não roda mais turno.
pub const CONVERSA_ANTIGA: &str = "Conversa do Vitral antigo, só para leitura: para continuar, abra um chat novo no projeto.";
/// O primeiro aviso na conversa do Mestre.
const BOAS_VINDAS: &str = "Sou o Mestre: me diga o que quer fazer. Eu entendo o pedido, mando para o projeto certo \
(cite um com #nome, ou um chat com @12) e mostro como vou dividir antes de começar.";
/// O aviso de quando o chat de projeto juntou: o começo dele marca de onde contam as mensagens que
/// o revisor confere ("desde a última junção").
const JUNTOU: &str = "As mudanças entraram em";

/// Trabalho pesado: preparar uma cópia ou conferir uma entrega (verificação e revisor). Um de cada
/// vez no Vitral inteiro: a fonte do PC não segura dois `npm install` ou dois revisores juntos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pesado {
    Preparar(u64),
    /// A entrega de uma tarefa (o fim de turno de um trabalhador, ou a refeita ao abrir).
    Conferir(u64),
    /// O parecer do revisor que o chat de projeto (ou o usuário) pediu, sem mudar o estado.
    Parecer(u64),
}

impl Pesado {
    fn tarefa(self) -> u64 {
        match self {
            Pesado::Preparar(id) | Pesado::Conferir(id) | Pesado::Parecer(id) => id,
        }
    }
}

enum Sinal {
    /// Evento do processo de um chat; o número separa um processo do seguinte do mesmo chat.
    Claude(String, u64, Evento),
    Mcp(Chamada),
    /// O clique numa notificação do sistema: abrir a conversa.
    Abrir(String),
    /// Um aviso do Windows (toast) a mostrar: o GPUI só mostra pela thread da janela.
    #[cfg(windows)]
    Aviso(gpui_kit::SystemNotification),
    /// Uma leitura do GitHub ou do Linear pediu uma busca (`conexoes::Motor::pedir`).
    Integracao,
}

/// A notificação de um chat que ainda está na tela: a próxima do mesmo chat a substitui. (Só no
/// Linux: no Windows quem guarda é o GPUI.)
#[derive(Default)]
#[cfg_attr(windows, allow(dead_code))]
struct Aviso {
    /// O id que o servidor de notificações deu (`notify-send -p`).
    id: Option<u32>,
    /// O `notify-send` esperando o clique. Substituído, ele sai: a notificação nova responde.
    filho: Option<std::process::Child>,
    /// Qual `notify-send` é o atual (o que sai depois de substituído não mexe no aviso).
    marca: u64,
}

pub struct PedidoPermissao {
    pub id: String,
    pub chat: String,
    pub ferramenta: String,
    pub entrada: Value,
    resposta: Sender<Resposta>,
}

/// O que uma leitura do uso mudou, do menos ao mais visível.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Mudanca {
    Nada,
    /// Só a hora da medição ou o "não deu para atualizar": aparece só no popover.
    Hora,
    /// As porcentagens, as renovações, o limite ou o plano.
    Numeros,
}

impl Mudanca {
    /// Do uso e do plano antes e depois de juntar uma leitura.
    fn entre((antes, depois): (Uso, Uso), (velho, novo): (Option<&Plano>, &Plano)) -> Mudanca {
        let uso_sem_hora = |uso: Uso| Uso { medido_em: None, ..uso };
        let plano_sem_hora = |plano: &Plano| Plano { medido_em: None, ..plano.clone() };
        let do_uso = match () {
            _ if antes == depois => Mudanca::Nada,
            _ if uso_sem_hora(antes) == uso_sem_hora(depois) => Mudanca::Hora,
            _ => Mudanca::Numeros,
        };
        let do_plano = match velho {
            Some(velho) if velho == novo => Mudanca::Nada,
            Some(velho) if plano_sem_hora(velho) == plano_sem_hora(novo) => Mudanca::Hora,
            _ => Mudanca::Numeros,
        };
        do_uso.max(do_plano)
    }
}

#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub struct Uso {
    pub cinco_horas: Option<f64>,
    pub semana: Option<f64>,
    /// O plano recusou pedidos (limite atingido).
    pub bloqueado: bool,
    /// Quando cada janela renova e quando esses números foram medidos (segundos Unix).
    pub renova_5h: Option<i64>,
    pub renova_semana: Option<i64>,
    pub medido_em: Option<i64>,
}

impl Uso {
    /// Junta duas medições ficando, em cada janela, com a mais recente que tem número; a janela
    /// que já renovou volta a zero. O `bloqueado` continua o desta.
    pub fn juntar(self, outra: Uso, agora: i64) -> Uso {
        let (nova, velha) = if outra.medido_em >= self.medido_em {
            (outra, self)
        } else {
            (self, outra)
        };
        let janela = |n: (Option<f64>, Option<i64>), v: (Option<f64>, Option<i64>)| {
            let (fracao, renova) = if n.0.is_some() { n } else { v };
            match renova {
                Some(r) if r <= agora => (fracao.map(|_| 0.0), renova),
                _ => (fracao, renova),
            }
        };
        let (cinco_horas, renova_5h) =
            janela((nova.cinco_horas, nova.renova_5h), (velha.cinco_horas, velha.renova_5h));
        let (semana, renova_semana) =
            janela((nova.semana, nova.renova_semana), (velha.semana, velha.renova_semana));
        Uso {
            cinco_horas,
            semana,
            bloqueado: self.bloqueado,
            renova_5h,
            renova_semana,
            medido_em: nova.medido_em.or(velha.medido_em),
        }
    }
}

/// Por que um chat espera você. A regra é uma só: lista, sino, trilho e modo recolhido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Espera {
    /// Pedido de permissão ou plano para aprovar.
    Pedido,
    /// Parou no meio sem ter sido você (Vitral fechado, verificação cortada): pede Retomar.
    Parada,
    Erro,
    /// Resposta do coordenador que você ainda não viu.
    Nova,
}

/// A espera de um chat pelos fatos dele: o pedido vem antes da parada, do erro e da resposta
/// nova; resposta nova só conta no coordenador.
pub fn espera(execucao: &Execucao, coordenador: bool, nao_lido: bool, parada: bool) -> Option<Espera> {
    match execucao {
        Execucao::EsperandoVoce => Some(Espera::Pedido),
        _ if parada => Some(Espera::Parada),
        Execucao::Erro(_) => Some(Espera::Erro),
        _ if coordenador && nao_lido => Some(Espera::Nova),
        _ => None,
    }
}

/// Quem da família atender primeiro (o coordenador vem na posição 0): pedidos, depois o que
/// parou no meio, os erros e as respostas novas; o coordenador antes dos trabalhadores.
pub fn alvo_da_familia(esperas: &[Option<Espera>]) -> Option<usize> {
    [Espera::Pedido, Espera::Parada, Espera::Erro, Espera::Nova]
        .into_iter()
        .find_map(|m| esperas.iter().position(|x| *x == Some(m)))
}

/// O que precisa de você na pasta (ou num projeto), contando famílias como os cartões de
/// "Esperando você". O número é o mesmo no sino, na seção, no "+1" do escopo e no modo recolhido.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrecisaDeVoce {
    pub pedidos: usize,
    pub paradas: usize,
    pub erros: usize,
    /// Respostas novas viram ponto, nunca número.
    pub novas: usize,
    /// Alguém falhou (mesmo que a família tenha antes um pedido para responder).
    pub erro: bool,
}

impl PrecisaDeVoce {
    /// Pedidos, erros e paradas que precisam de Retomar.
    pub fn numero(&self) -> usize {
        self.pedidos + self.erros + self.paradas
    }

    pub fn somar(&mut self, espera: Espera, erro: bool) {
        match espera {
            Espera::Pedido => self.pedidos += 1,
            Espera::Parada => self.paradas += 1,
            Espera::Erro => self.erros += 1,
            Espera::Nova => self.novas += 1,
        }
        self.erro |= erro;
    }
}

/// Uma coisa que espera você, para a caixa do sino e o Ctrl+G.
#[derive(Clone, Debug, PartialEq)]
pub enum Pendencia {
    Permissao { pedido: String, chat: String, ferramenta: String, resumo: String },
    Plano { pedido: String, chat: String },
    Erro { chat: String, texto: String, pode_tentar: bool },
    Parada { chat: String, motivo: String },
    Nova { chat: String },
}

impl Pendencia {
    pub fn chat(&self) -> &str {
        match self {
            Pendencia::Permissao { chat, .. }
            | Pendencia::Plano { chat, .. }
            | Pendencia::Erro { chat, .. }
            | Pendencia::Parada { chat, .. }
            | Pendencia::Nova { chat } => chat,
        }
    }
}

/// Por que o diff de uma tarefa não saiu.
#[derive(Clone, Debug, PartialEq)]
pub enum LeituraDiff {
    /// A tarefa não tem (ou não tem mais) a cópia isolada.
    SemCopia,
    Git(String),
}

/// Por que o app abriu só para leitura: nada é gravado e nenhum turno começa.
#[derive(Clone, Debug, PartialEq)]
pub enum SomenteLeitura {
    Ilegivel { arquivo: PathBuf, copia: Option<PathBuf> },
    VersaoNova { arquivo: PathBuf, versao: u32 },
    MigracaoAdiada { raizes: Vec<PathBuf>, pids: Vec<u32> },
    MigracaoFalhou { raiz: PathBuf, erro: String },
}

impl From<ErroCarga> for SomenteLeitura {
    fn from(erro: ErroCarga) -> Self {
        match erro {
            ErroCarga::Ilegivel { arquivo, copia } => SomenteLeitura::Ilegivel { arquivo, copia },
            ErroCarga::VersaoNova { arquivo, versao } => SomenteLeitura::VersaoNova { arquivo, versao },
        }
    }
}

/// O texto digitado achou um projeto, nenhum ou mais de um.
#[derive(Clone, Debug, PartialEq)]
pub enum Resolucao {
    Um(String),
    Nenhum,
    Varios(Vec<String>),
}

/// Um projeto novo: com pastas, liga as pastas; sem nenhuma, cria a pasta própria (com git).
#[derive(Clone, Debug, Default)]
pub struct NovoProjeto {
    pub nome: String,
    pub pastas: Vec<PathBuf>,
    pub icone: String,
    pub cor: String,
    pub descricao: String,
    pub instrucoes: String,
    /// A configuração de cada pasta, na ordem de `pastas` (sem pastas, a primeira vai para a própria).
    pub configs: Vec<ConfigProjeto>,
    pub permissao: Option<String>,
    pub modelo: Option<String>,
    pub conta: Option<String>,
    /// O time do Linear escolhido no formulário.
    pub linear: Option<crate::modelo::LinearDoProjeto>,
}

/// O que muda num projeto: só o que vem preenchido (é um patch: a ordem e o arquivamento mexidos com
/// o formulário aberto ficam).
#[derive(Clone, Debug, Default)]
pub struct MudancaProjeto {
    pub nome: Option<String>,
    pub icone: Option<String>,
    pub cor: Option<String>,
    pub descricao: Option<String>,
    pub instrucoes: Option<String>,
    /// A configuração de uma pasta, pelo índice.
    pub pasta: Option<(usize, ConfigProjeto)>,
    /// A lista inteira das pastas, a primeira é a principal; vazia, o projeto fica com a pasta própria.
    pub pastas: Option<Vec<PastaLigada>>,
    /// `Some(None)` volta à configuração geral.
    pub permissao: Option<Option<String>>,
    pub modelo: Option<Option<String>>,
    pub conta: Option<Option<String>>,
    /// O time do Linear e os estados automáticos; `Some(None)` desliga o Linear do projeto.
    pub linear: Option<Option<crate::modelo::LinearDoProjeto>>,
}

pub struct Motor {
    pub dados: Dados,
    pub config: Config,
    pub projetos: Vec<Projeto>,
    pub chats: Vec<Chat>,
    pub tarefas: Vec<Tarefa>,
    pub rotinas: Vec<Rotina>,
    proxima_tarefa: u64,
    cartoes: Vec<Cartao>,
    proximo_cartao: u64,
    ocorrencias: Vec<Ocorrencia>,
    visto_em: u64,
    /// Campos do estado de versões mais novas, regravados como vieram.
    resto_estado: serde_json::Map<String, Value>,
    somente_leitura: Option<SomenteLeitura>,
    /// Uma gravação falhou ("caminho: erro") e desde quando: nada novo começa até voltar.
    pub falha_gravacao: Option<(String, u64)>,
    /// O que avisar ao abrir (estado restaurado, projetos recriados, a migração).
    pub avisos_ao_abrir: Vec<String>,
    /// Pedidos de abertura (`vitral <pasta>`, `--nova`, os de outra abertura pelo socket) que a
    /// janela ainda não atendeu.
    pub aberturas: Vec<crate::instancia::Abertura>,
    execucao: HashMap<String, Execucao>,
    processos: HashMap<String, Processo>,
    geracao: u64,
    geracao_atual: HashMap<String, u64>,
    /// O que entrou no último turno de cada chat, para "Tentar de novo", com o uuid do turno: a
    /// repetição (tentar de novo, a troca de conta) é o mesmo turno, e a guarda das ações externas
    /// não repete o que ele já fez.
    ultima_entrada: HashMap<String, (String, Vec<Entrada>)>,
    /// Contas do Claude (a principal primeiro) e o uso do plano de cada uma.
    pub contas: Vec<Conta>,
    uso_por_conta: HashMap<String, Uso>,
    /// Até quando cada conta que bateu no limite fica de fora (segundos Unix).
    limite_ate: HashMap<String, u64>,
    /// Nome do plano, limites por modelo e créditos extras de cada conta (do `.claude.json`).
    plano_por_conta: HashMap<String, Plano>,
    /// Quando buscar o uso dos planos na Anthropic e o recuo de cada conta (`contas::busca`).
    agenda_uso: Agenda,
    /// A marca do popover do uso do Claude aberto: enquanto ela vive, a busca é de minuto em
    /// minuto (a interface solta a marca quando o popover some da tela).
    uso_em_vista: Option<AnyWeakEntity>,
    /// Uso do plano do Codex lido do disco (`None`: sem pasta do Codex neste computador).
    pub codex: Option<Codex>,
    /// A primeira leitura do Codex terminou (antes dela, `None` não quer dizer "não instalado").
    pub codex_lido: bool,
    /// Conta usada no turno atual (ou no último) de cada chat.
    conta_do_turno: HashMap<String, String>,
    /// Processos que já entregaram o resultado e estão terminando de gravar a sessão.
    encerrando: Vec<Processo>,
    parcial: HashMap<String, String>,
    /// Mensagens que chegaram com o chat ocupado: entram juntas no próximo turno.
    filas: Filas,
    /// Trabalhadores esperando vaga (limite de `max_trabalhadores`).
    espera: VecDeque<String>,
    interrompidos: Vec<String>,
    /// Trabalhadores que perguntaram algo ao coordenador e esperam a resposta.
    aguardando_resposta: Vec<String>,
    /// Nova tentativa automática agendada depois de uma queda (chat → marca do agendamento e
    /// quando ela roda, em segundos Unix).
    retentativas: HashMap<String, (u64, u64)>,
    /// Chats que já gastaram a tentativa automática desde o último turno que deu certo.
    tentativas_auto: HashSet<String>,
    /// Tarefas com a verificação e o revisor rodando agora, com a rodada que está sendo conferida
    /// (o fim de uma verificação velha não tira a marca da que começou depois).
    verificando: HashMap<u64, u32>,
    /// Tarefas sendo juntadas no projeto agora.
    juntando: HashSet<u64>,
    /// Entregas cortadas na verificação quando o Vitral fechou: refeitas uma de cada vez.
    entregas_a_retomar: VecDeque<u64>,
    /// Tarefas com a cópia isolada sendo criada e preparada agora.
    preparando: HashSet<u64>,
    /// Preparações de cópia cortadas quando o Vitral fechou: refeitas uma de cada vez.
    preparos_a_retomar: VecDeque<u64>,
    /// A fila do trabalho pesado e o que roda dela agora (um por vez).
    pesado: VecDeque<Pesado>,
    pesado_atual: Option<Pesado>,
    /// Quem espera a preparação (o `criar_tarefa` do pai do ajudante) e o parecer (o
    /// `revisar_minhas_mudancas`) para responder.
    respostas_preparo: HashMap<u64, Sender<Resposta>>,
    respostas_parecer: HashMap<u64, Sender<Resposta>>,
    /// Chats diretos esperando outro chat sair da mesma pasta (um escritor por pasta).
    esperando_pasta: Vec<String>,
    /// Cópias andando com a origem agora (depois de uma junção, ou antes do turno): o turno espera.
    sincronizando: HashSet<u64>,
    /// Chats cuja cópia já foi conferida com a origem para o turno que vai começar.
    sincronizados: HashSet<String>,
    /// Chats com mensagens ainda não gravadas: vão para o disco a cada poucos segundos.
    chats_sujos: HashSet<String>,
    /// Chats cujo turno já foi repetido depois de um erro de sessão (uma vez só).
    repetidos_sessao: HashSet<String>,
    /// Chats cuja sessão do Claude Code é nova (a antiga expirou ou, no Mestre, o contexto encheu):
    /// o próximo turno começa com o bloco de passagem.
    passagens: HashMap<String, MotivoPassagem>,
    /// Mensagens automáticas seguidas entre pares de chats (o freio de ciclo).
    ciclos: Ciclos,
    /// "Enquanto você esteve fora": só na memória.
    pub resumo_volta: Option<ResumoVolta>,
    /// O primeiro parágrafo do README de cada pasta, pelo mtime (o catálogo do Mestre).
    readmes: Readmes,
    /// Sugestões de projeto do Mestre cujo projeto está sendo criado agora.
    criando_projeto_de: HashSet<u64>,
    pub permissoes: Vec<PedidoPermissao>,
    /// Versão do Claude Code que os turnos abrem (lida ao abrir e a cada início de turno).
    pub versao_claude: Option<Versao>,
    /// Se a conta tem os fluxos de agentes (a ferramenta `Workflow`): `false` só depois de um
    /// turno com Ultracode em que eles não vieram. Não trava a opção (a pessoa pode ter mudado a
    /// configuração da conta): só a avisa em âmbar.
    fluxos_por_conta: HashMap<String, bool>,
    /// Fluxos e subagentes que o processo atual de cada chat ainda roda em segundo plano.
    fundo: HashMap<String, usize>,
    /// O `result` que chegou com agentes ainda em segundo plano (texto e custo): o turno só
    /// termina quando eles acabam, ou quando o processo sai.
    fim_adiado: HashMap<String, (String, f64)>,
    pub janela_ativa: bool,
    /// A conversa que o clique numa notificação pediu para abrir; a Janela consome.
    pub pedido_de_abrir: Option<String>,
    /// As notificações na tela, por chat.
    #[cfg_attr(windows, allow(dead_code))]
    avisos: Arc<std::sync::Mutex<HashMap<String, Aviso>>>,
    /// Chats que algum painel aberto mostra (a Janela atualiza a cada quadro): o texto parcial
    /// dos outros não redesenha a janela.
    pub visiveis: HashSet<String>,
    /// Marcado ao fechar ou trocar de pasta: a etapa seguinte de uma entrega ou preparação em
    /// segundo plano (revisor, script de preparação) não abre processo depois da parada.
    encerrado: Arc<AtomicBool>,
    /// GitHub e Linear: as conexões, a chave do Linear na memória e o que veio da rede.
    integracao: conexoes::Integracao,
    /// As listas e os detalhes das issues (só na memória).
    cache_issues: issues::Issues,
    /// O PR das tarefas: o vigia, o que está sendo publicado ou mesclado e o que veio do GitHub
    /// para a aba PR (só na memória).
    prs: prs::EstadoPrs,
    servidor: Option<mcp::Servidor>,
    sinais: Sender<Sinal>,
    _recebedor: Task<()>,
}

impl Motor {
    pub fn new(dados: Dados, aberto: Result<Carregado, SomenteLeitura>, cx: &mut Context<Self>) -> Self {
        let (mut estado, config, mut avisos_ao_abrir, somente_leitura) = match aberto {
            Ok(c) => (c.estado, c.config, c.avisos, None),
            Err(leitura) => (Estado::default(), Config::default(), Vec::new(), Some(leitura)),
        };
        let normalizado = if somente_leitura.is_none() {
            dados::normalizar(&mut estado, &dados, &dados::NoDisco, agora())
        } else {
            Default::default()
        };
        avisos_ao_abrir.extend(normalizado.avisos);
        let Estado {
            projetos,
            mut chats,
            mut tarefas,
            proxima_tarefa,
            filas,
            rotinas,
            cartoes,
            proximo_cartao,
            ocorrencias,
            visto_em,
            resto: resto_estado,
            ..
        } = estado;
        let antes: Vec<usize> = chats.iter().map(|c| c.itens.len()).collect();
        let paradas_antes: HashSet<u64> = tarefas.iter().filter(|t| t.parada.is_some()).map(|t| t.id).collect();
        let (entregas_a_retomar, preparos_a_retomar) =
            marcar_interrompidas(&mut chats, &mut tarefas, &filas);
        let paradas_novas: Vec<(String, String, u64, String)> = tarefas
            .iter()
            .filter(|t| !paradas_antes.contains(&t.id))
            .filter_map(|t| Some((t.trabalhador.clone(), t.projeto.clone(), t.id, t.parada.clone()?)))
            .collect();
        // Avisos, marcas de parada e o que a normalização mexeu vão para o disco já: se o PC cair
        // de novo antes de qualquer ação, a próxima abertura não marca as mesmas tarefas nem
        // repete o aviso.
        if somente_leitura.is_none() {
            for (c, n) in chats.iter().zip(antes) {
                if c.itens.len() != n || normalizado.chats_mexidos.contains(&c.id) {
                    let _ = dados.salvar_chat(c);
                }
            }
            for id in &normalizado.chats_a_apagar {
                dados.apagar_chat(id);
            }
            let _ = std::fs::create_dir_all(dados.pasta_mestre());
            // A pasta de um avulso (um coordenador vazio convertido, ou apagada à mão) volta vazia.
            let avulsos = dados.pasta_avulso("");
            for c in chats.iter().filter(|c| c.papel == Papel::Projeto && c.cwd.starts_with(&avulsos)) {
                let _ = std::fs::create_dir_all(&c.cwd);
            }
        }
        let (sinais, recebe) = unbounded::<Sinal>();
        // O clique num aviso do Windows abre a conversa dele (a marca do aviso é o id do chat).
        #[cfg(windows)]
        {
            let envia = sinais.clone();
            cx.on_system_notification_response(move |resposta, _| {
                let _ = envia.try_send(Sinal::Abrir(resposta.tag.to_string()));
            });
        }

        let envia_mcp = sinais.clone();
        let servidor = match mcp::iniciar(move |chamada| {
            let _ = envia_mcp.send_blocking(Sinal::Mcp(chamada));
        }) {
            Ok(servidor) => Some(servidor),
            Err(erro) => {
                eprintln!("vitral: servidor MCP não abriu: {erro:#}");
                None
            }
        };

        // Trata de uma vez o que chegou junto, com um redesenho só. Pedaço de texto vem a cada
        // token: espera um pouco pelos próximos (no máximo 20 redesenhos por segundo).
        let _recebedor = cx.spawn(async move |motor, cx| {
            while let Ok(sinal) = recebe.recv().await {
                if matches!(sinal, Sinal::Claude(_, _, Evento::TextoParcial(_))) {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(50))
                        .await;
                }
                let mut sinais = vec![sinal];
                while sinais.len() < 512
                    && let Ok(outro) = recebe.try_recv()
                {
                    sinais.push(outro);
                }
                // Texto chegando numa conversa que não está aberta não redesenha a janela.
                if motor
                    .update(cx, |motor, cx| {
                        let mut redesenhar = false;
                        for sinal in sinais {
                            redesenhar |= motor.tratar(sinal, cx);
                        }
                        if redesenhar {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        cx.spawn(async move |motor, cx| {
            let _ = motor.update(cx, |m, cx| m.limpar_ao_abrir(cx));
        })
        .detach();
        // Rotinas: confere a cada meio minuto se alguma está na hora, se há conversa para arquivar
        // e relê o uso dos planos.
        cx.spawn(async move |motor, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(30))
                    .await;
                let vez = motor.update(cx, |m, cx| {
                    m.rodar_rotinas(cx);
                    m.arquivar_paradas(cx);
                    // E relê o uso do plano guardado de cada conta e o do Codex.
                    m.reler_uso(cx);
                });
                if vez.is_err() {
                    break;
                }
            }
        })
        .detach();
        // O uso dos planos direto da Anthropic: a cada 15 s confere se é hora (o ritmo muda com o
        // popover aberto e com a janela sem foco); a primeira rodada sai logo na abertura.
        cx.spawn(async move |motor, cx| {
            loop {
                if motor.update(cx, |m, cx| m.rotina_uso(cx)).is_err() {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(15))
                    .await;
            }
        })
        .detach();
        // As mensagens do turno em andamento vão para o disco a cada poucos segundos (o PC cai).
        cx.spawn(async move |motor, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(5))
                    .await;
                if motor.update(cx, |m, cx| m.gravar_sujos(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        // Email e plano da conta principal: o `claude auth status` demora um pouco, por isso o
        // uso guardado e o do Codex (só arquivos) são lidos à parte, logo abaixo.
        cx.spawn(async move |motor, cx| {
            let status = cx.background_spawn(async move { contas::status(None) }).await;
            let _ = motor.update(cx, |m, cx| {
                if let Ok((email, plano)) = status
                    && let Some(c) = m.contas.iter_mut().find(|c| c.id == contas::PRINCIPAL)
                {
                    c.email = email;
                    c.plano = plano;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.spawn(async move |motor, cx| {
            let _ = motor.update(cx, |m, cx| m.retomar_entregas(cx));
        })
        .detach();
        // A versão do Claude Code decide se o Ultracode aparece (o `--version` leva um instante).
        cx.spawn(async move |motor, cx| {
            let versao = cx.background_spawn(async move { claude::versao_instalada() }).await;
            let _ = motor.update(cx, |m, cx| {
                if m.versao_claude.is_none() && versao.is_some() {
                    m.versao_claude = versao;
                    cx.notify();
                }
            });
        })
        .detach();
        // O vigia dos PRs: um laço só, que espera o próximo prazo ou o sinal (o fim de toda consulta e
        // as ações que mudam alguma coisa). Sem PR para acompanhar, ele só espera o sinal.
        let (acordar_vigia, acordado_vigia) = smol::channel::unbounded::<()>();
        cx.spawn(async move |motor, cx| {
            loop {
                let Ok(quando) = motor.update(cx, |m, cx| m.volta_do_vigia(cx)) else {
                    break;
                };
                let relogio = cx.background_executor().clone();
                prs::esperar_vigia(quando, &acordado_vigia, move |espera| relogio.timer(espera)).await;
            }
        })
        .detach();
        let mut motor = Self {
            dados,
            config,
            projetos,
            chats,
            tarefas,
            rotinas,
            proxima_tarefa,
            cartoes,
            proximo_cartao,
            ocorrencias,
            visto_em,
            resto_estado,
            somente_leitura,
            falha_gravacao: None,
            avisos_ao_abrir,
            aberturas: Vec::new(),
            execucao: HashMap::new(),
            processos: HashMap::new(),
            geracao: 0,
            geracao_atual: HashMap::new(),
            ultima_entrada: HashMap::new(),
            contas: contas::carregar(),
            uso_por_conta: HashMap::new(),
            limite_ate: HashMap::new(),
            plano_por_conta: HashMap::new(),
            agenda_uso: Agenda::default(),
            uso_em_vista: None,
            codex: None,
            codex_lido: false,
            conta_do_turno: HashMap::new(),
            encerrando: Vec::new(),
            parcial: HashMap::new(),
            filas,
            espera: VecDeque::new(),
            interrompidos: Vec::new(),
            aguardando_resposta: Vec::new(),
            retentativas: HashMap::new(),
            tentativas_auto: HashSet::new(),
            verificando: HashMap::new(),
            juntando: HashSet::new(),
            entregas_a_retomar,
            preparando: HashSet::new(),
            preparos_a_retomar,
            pesado: VecDeque::new(),
            pesado_atual: None,
            respostas_preparo: HashMap::new(),
            respostas_parecer: HashMap::new(),
            esperando_pasta: Vec::new(),
            sincronizando: HashSet::new(),
            sincronizados: HashSet::new(),
            chats_sujos: HashSet::new(),
            repetidos_sessao: HashSet::new(),
            passagens: HashMap::new(),
            ciclos: Ciclos::default(),
            resumo_volta: None,
            readmes: Default::default(),
            criando_projeto_de: HashSet::new(),
            permissoes: Vec::new(),
            versao_claude: None,
            fluxos_por_conta: HashMap::new(),
            fundo: HashMap::new(),
            fim_adiado: HashMap::new(),
            janela_ativa: true,
            pedido_de_abrir: None,
            avisos: Arc::default(),
            visiveis: HashSet::new(),
            encerrado: Arc::default(),
            integracao: conexoes::Integracao::new(Arc::new(crate::remoto::Real)),
            cache_issues: issues::Issues::default(),
            prs: prs::EstadoPrs::novo(acordar_vigia),
            servidor,
            sinais,
            _recebedor,
        };
        if motor.somente_leitura.is_none() {
            for (chat, projeto, id, motivo) in paradas_novas {
                motor.registrar("parou", "vitral", &chat, &projeto, Some(id), &format!("chat {id} parou: {motivo}"));
            }
            if motor.chat(crate::modelo::ID_MESTRE).is_some_and(|c| c.itens.is_empty()) {
                motor.avisar_no_mestre(BOAS_VINDAS);
            }
            motor.conferir_vitral_antigo();
            motor.conferir_instalacao();
            if motor.chats_sujos.remove(crate::modelo::ID_MESTRE)
                && let Some(c) = motor.chat(crate::modelo::ID_MESTRE)
            {
                let _ = motor.dados.salvar_chat(c);
            }
        }
        motor.salvar();
        motor.dados.esperar_gravacoes();
        // O último uso do plano que o Claude Code guardou de cada conta e o do Codex.
        motor.reler_uso(cx);
        // O bwrap, o login do gh e a chave do Linear (sem destravar o chaveiro).
        motor.iniciar_conexoes(cx);
        // O pedido de uma issue que não chegou a sair antes de o Vitral fechar.
        if motor.somente_leitura.is_none() {
            motor.retomar_pedidos_de_issue(cx);
            // O vigia retoma os PRs abertos (e o último fechado de cada chat) na primeira volta.
            motor.vigiar_na_hora();
        }
        motor
    }

    // ---------- leitura para a interface ----------

    /// A migração acabou de trazer o Vitral antigo: o aviso na conversa do Mestre e a ocorrência no
    /// registro.
    pub fn migracao_concluida(&mut self, relatorio: &crate::migracao::Relatorio, cx: &mut Context<Self>) {
        let mut texto = relatorio.texto(dirs::home_dir().as_deref());
        let sem_descricao: Vec<String> = self
            .projetos_visiveis()
            .iter()
            .filter(|p| p.descricao.trim().is_empty() && p.principal().is_none_or(|l| self.readme_de(&l.caminho).is_none()))
            .map(|p| p.nome.clone())
            .collect();
        if !sem_descricao.is_empty() {
            texto.push_str(&format!(
                " Escreva uma descrição em {} (botão direito no ícone > Editar projeto) para eu saber o que mandar para cada um.",
                sem_descricao.join(", ")
            ));
        }
        self.registrar("migracao", "vitral", "", "", None, &texto);
        self.avisar_no_mestre(&texto);
        self.salvar();
        cx.notify();
    }

    /// Um pedido de abertura para a janela atender (ela observa o motor).
    pub fn pedir_abertura(&mut self, abertura: crate::instancia::Abertura, cx: &mut Context<Self>) {
        if !abertura.vazia() {
            self.aberturas.push(abertura);
            cx.notify();
        }
    }

    /// Uma linha no registro de ocorrências (as mais antigas saem depois de `MAX_OCORRENCIAS`).
    fn registrar(&mut self, tipo: &str, autor: &str, chat: &str, projeto: &str, tarefa: Option<u64>, texto: &str) {
        self.ocorrencias.push(Ocorrencia {
            quando: agora(),
            tipo: tipo.into(),
            autor: autor.into(),
            chat: chat.into(),
            projeto: projeto.into(),
            tarefa,
            texto: texto.into(),
        });
        let sobra = self.ocorrencias.len().saturating_sub(crate::modelo::MAX_OCORRENCIAS);
        self.ocorrencias.drain(..sobra);
    }

    pub fn somente_leitura(&self) -> Option<&SomenteLeitura> {
        self.somente_leitura.as_ref()
    }

    /// O chat do Mestre (a normalização garante que ele existe).
    pub fn mestre_id(&self) -> Option<&str> {
        self.chat(crate::modelo::ID_MESTRE).map(|c| c.id.as_str())
    }

    pub fn chat(&self, id: &str) -> Option<&Chat> {
        self.chats.iter().find(|c| c.id == id)
    }

    fn chat_mut(&mut self, id: &str) -> Option<&mut Chat> {
        self.chats.iter_mut().find(|c| c.id == id)
    }

    pub fn tarefa(&self, id: u64) -> Option<&Tarefa> {
        self.tarefas.iter().find(|t| t.id == id)
    }

    fn tarefa_mut(&mut self, id: u64) -> Option<&mut Tarefa> {
        self.tarefas.iter_mut().find(|t| t.id == id)
    }

    pub fn execucao(&self, chat: &str) -> Execucao {
        self.execucao.get(chat).cloned().unwrap_or(Execucao::Parado)
    }

    pub fn parcial(&self, chat: &str) -> Option<&str> {
        self.parcial
            .get(chat)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }

    /// Mensagens esperando o chat ficar livre (as notas, que só vão junto com a próxima, não contam).
    pub fn na_fila(&self, chat: &str) -> usize {
        self.filas.get(chat).map_or(0, |f| f.iter().filter(|e| !e.nota).count())
    }

    pub fn esperando_vaga(&self, chat: &str) -> bool {
        self.espera.iter().any(|c| c == chat)
    }

    pub fn rodando(&self) -> impl Iterator<Item = &Chat> {
        self.chats
            .iter()
            .filter(|c| self.execucao(&c.id).rodando() || self.esperando_vaga(&c.id))
    }

    /// Quantos chats fechar o Vitral agora cortaria no meio: turno rodando ou esperando vaga, nova
    /// tentativa marcada, ou a tarefa deles em preparação, verificação ou junção.
    pub fn trabalhando(&self) -> usize {
        let mut chats: HashSet<&str> = self.rodando().map(|c| c.id.as_str()).collect();
        chats.extend(self.retentativas.keys().map(String::as_str));
        chats.extend(
            self.tarefas
                .iter()
                .filter(|t| {
                    self.verificando.contains_key(&t.id)
                        || self.preparando.contains(&t.id)
                        || self.juntando.contains(&t.id)
                        || self.sincronizando.contains(&t.id)
                        || self.na_fila_pesada(t.id)
                })
                .map(|t| t.trabalhador.as_str()),
        );
        chats.len()
    }

    /// A tarefa espera a vez no trabalho pesado (preparar ou conferir), ou roda nele agora.
    pub fn na_fila_pesada(&self, id: u64) -> bool {
        self.pesado.iter().any(|p| p.tarefa() == id) || self.pesado_atual.is_some_and(|p| p.tarefa() == id)
    }

    /// As tarefas que a direita mostra com um chat: as do coordenador ou, num chat de projeto, a
    /// dele e as dos ajudantes dele.
    pub fn tarefas_da_equipe<'a>(&'a self, chat: &'a str) -> impl Iterator<Item = &'a Tarefa> + 'a {
        self.tarefas
            .iter()
            .filter(move |t| t.coordenador == chat || (t.trabalhador == chat && !chat.is_empty()))
    }

    /// O chat é de projeto (ou avulso): conversa com o usuário, com uma tarefa só dele.
    fn de_projeto(&self, chat: &str) -> bool {
        self.chat(chat).is_some_and(|c| c.papel == Papel::Projeto)
    }

    pub fn tarefas_do(&self, coordenador: &str) -> impl Iterator<Item = &Tarefa> {
        self.tarefas
            .iter()
            .filter(move |t| t.coordenador == coordenador)
    }

    /// A tarefa parou no meio e nada a faz andar até alguém mandar retomar.
    pub fn tarefa_parada(&self, t: &Tarefa) -> bool {
        t.parada.is_some()
            && match t.estado {
                EstadoTarefa::Rodando | EstadoTarefa::Ajustando => {
                    !self.processos.contains_key(&t.trabalhador)
                        && !self.esperando_vaga(&t.trabalhador)
                        && !self.retentativas.contains_key(&t.trabalhador)
                        && !self.preparando.contains(&t.id)
                        && !self.preparos_a_retomar.contains(&t.id)
                        && !self.na_fila_pesada(t.id)
                }
                EstadoTarefa::EmRevisao => {
                    t.entrega_pendente.is_some()
                        && !self.verificando.contains_key(&t.id)
                        && !self.entregas_a_retomar.contains(&t.id)
                        && !self.na_fila_pesada(t.id)
                }
                _ => false,
            }
    }

    /// Por que o chat (ou a tarefa dele) parou no meio, quando dá para retomar.
    pub fn parada(&self, chat_id: &str) -> Option<String> {
        let chat = self.chat(chat_id)?;
        if let Some(t) = chat.tarefa.and_then(|t| self.tarefa(t))
            && self.tarefa_parada(t)
        {
            return t.parada.clone();
        }
        let livre = !self.processos.contains_key(chat_id)
            && !self.esperando_vaga(chat_id)
            && !self.retentativas.contains_key(chat_id);
        (chat.turno_aberto && livre).then(|| CORTADO.to_string())
    }

    /// Parou sem ter sido o usuário (Vitral fechado, verificação cortada): pede a atenção dele.
    pub fn precisa_retomar(&self, chat_id: &str) -> bool {
        self.parada(chat_id).is_some_and(|m| m != PARADA_VOCE)
    }

    /// Quando roda a nova tentativa automática agendada depois de uma queda (segundos Unix).
    pub fn nova_tentativa_em(&self, chat: &str) -> Option<u64> {
        self.retentativas.get(chat).map(|r| r.1)
    }

    /// O modo em que a conversa fica ao aprovar o plano dela.
    pub fn modo_ao_aprovar_plano(&self, chat: &Chat) -> String {
        modo_depois_do_plano(
            chat.modo_antes_do_plano.as_deref(),
            Some(self.permissao_de(chat).as_str()),
            &self.config.permissao,
        )
    }

    /// A espera de um chat só (sem a família), fora os arquivados.
    pub fn espera_do_chat(&self, c: &Chat) -> Option<Espera> {
        if self.arquivada(c) {
            return None;
        }
        espera(&self.execucao(&c.id), c.papel.na_lista(), self.nao_lido(c), self.precisa_retomar(&c.id))
    }

    /// O coordenador e os trabalhadores não arquivados dele, na ordem das tarefas.
    pub fn familia<'a>(&'a self, coordenador: &'a Chat) -> Vec<&'a Chat> {
        let mut trabalhadores: Vec<&Chat> = self
            .chats
            .iter()
            .filter(|t| t.pai.as_deref() == Some(coordenador.id.as_str()) && !t.arquivado)
            .collect();
        trabalhadores.sort_by_key(|t| t.tarefa);
        std::iter::once(coordenador).chain(trabalhadores).collect()
    }

    /// A espera da família de um coordenador: quem atender, por quê e se alguém falhou.
    pub fn espera_da_familia<'a>(&'a self, coordenador: &'a Chat) -> Option<(&'a Chat, Espera, bool)> {
        if self.arquivada(coordenador) {
            return None;
        }
        let familia = self.familia(coordenador);
        let esperas: Vec<Option<Espera>> = familia.iter().map(|c| self.espera_do_chat(c)).collect();
        let ix = alvo_da_familia(&esperas)?;
        Some((familia[ix], esperas[ix]?, esperas.contains(&Some(Espera::Erro))))
    }

    /// O que precisa de você em toda a pasta (`None`) ou num projeto.
    pub fn precisa_de_voce(&self, projeto: Option<&str>) -> PrecisaDeVoce {
        let mut conta = PrecisaDeVoce::default();
        for c in self.chats.iter().filter(|c| {
            c.papel.na_lista() && !self.arquivada(c) && projeto.is_none_or(|p| c.projeto == p)
        }) {
            if let Some((_, espera, erro)) = self.espera_da_familia(c) {
                conta.somar(espera, erro);
            }
        }
        conta
    }

    /// Tudo o que espera você, um por linha: pedidos e planos na ordem em que chegaram, depois os
    /// erros, as paradas e as respostas novas (a mais recente primeiro). Sem os arquivados.
    pub fn pendencias(&self) -> Vec<Pendencia> {
        let ativo = |id: &str| self.chat(id).is_some_and(|c| !self.arquivada(c));
        let mut lista: Vec<Pendencia> = self
            .permissoes
            .iter()
            .filter(|p| ativo(&p.chat))
            .map(|p| match p.ferramenta.as_str() {
                "ExitPlanMode" => Pendencia::Plano { pedido: p.id.clone(), chat: p.chat.clone() },
                ferramenta => Pendencia::Permissao {
                    pedido: p.id.clone(),
                    chat: p.chat.clone(),
                    ferramenta: ferramenta.to_string(),
                    resumo: claude::resumo_ferramenta(ferramenta, &p.entrada),
                },
            })
            .collect();
        let ativos: Vec<&Chat> = self.chats.iter().filter(|c| !self.arquivada(c)).collect();
        for c in &ativos {
            if let Execucao::Erro(texto) = self.execucao(&c.id) {
                lista.push(Pendencia::Erro {
                    chat: c.id.clone(),
                    texto,
                    pode_tentar: self.pode_tentar_de_novo(&c.id),
                });
            }
        }
        for c in &ativos {
            if self.precisa_retomar(&c.id) {
                let motivo = self.parada(&c.id).unwrap_or_default();
                lista.push(Pendencia::Parada { chat: c.id.clone(), motivo });
            }
        }
        let mut novas: Vec<&&Chat> =
            ativos.iter().filter(|c| c.papel.na_lista() && self.nao_lido(c)).collect();
        novas.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
        lista.extend(novas.into_iter().map(|c| Pendencia::Nova { chat: c.id.clone() }));
        lista
    }

    /// "Marcar tudo como lido" da caixa do sino.
    pub fn marcar_lidos(&mut self, ids: &[String], cx: &mut Context<Self>) {
        for id in ids {
            self.marcar_lido(id);
        }
        self.salvar();
        cx.notify();
    }

    /// Por que a tarefa não pode ser aprovada agora, por quem a pediu ou pelo usuário. Tarefa que
    /// parou no meio (Vitral fechado, Parar) pode: o usuário decide pelo que vê no diff.
    pub fn recusa_aprovar(&self, id: u64) -> Option<String> {
        let Some(t) = self.tarefa(id) else {
            return Some("Este chat não existe mais.".into());
        };
        let ocupado = |chat: &str| {
            self.processos.contains_key(chat)
                || self.esperando_vaga(chat)
                || self.retentativas.contains_key(chat)
        };
        if self.de_projeto(&t.trabalhador) {
            let recusa = if self.juntando.contains(&id) || t.juntando_desde > 0 {
                "As mudanças deste chat já estão sendo juntadas; o resultado chega sozinho.".to_string()
            } else if self.sincronizando.contains(&id) {
                "A cópia deste chat está andando com a origem; tente de novo em instantes.".into()
            } else if self.verificando.contains_key(&id) || self.na_fila_pesada(id) {
                "A cópia deste chat está sendo preparada ou conferida; espere terminar.".into()
            } else if t.tem_copia() && (!t.preparada || t.worktree.is_none()) {
                "Este chat ainda não tem cópia: não há o que juntar.".into()
            } else if ocupado(&t.trabalhador) {
                "O chat ainda está trabalhando; espere a vez dele terminar para juntar.".into()
            } else if self.na_fila(&t.trabalhador) > 0 {
                "O chat tem mensagem esperando para entrar; espere ela terminar para juntar.".into()
            } else {
                return None;
            };
            return Some(recusa);
        }
        let recusa = match t.estado {
            EstadoTarefa::Aprovada => format!("O ajudante #{id} já foi aprovado e juntado."),
            EstadoTarefa::Cancelada => {
                format!("O ajudante #{id} foi cancelado: a cópia dele ficou guardada, mas não vai ser juntada.")
            }
            EstadoTarefa::Falhou => format!("O ajudante #{id} falhou sem chegar a ter cópia; não há o que juntar."),
            _ if self.juntando.contains(&id) => format!("O ajudante #{id} já está sendo juntado; o resultado chega sozinho."),
            _ if self.verificando.contains_key(&id) => {
                format!("A verificação e o revisor ainda estão conferindo o ajudante #{id}; espere o relatório.")
            }
            _ if !t.preparada => "A cópia deste ajudante ainda está sendo preparada e ele nem começou.".into(),
            _ if ocupado(&t.trabalhador) => {
                format!("O ajudante #{id} ainda está rodando; espere o relatório antes de aprovar.")
            }
            _ if self.na_fila(&t.trabalhador) > 0 => format!(
                "O ajudante #{id} tem mensagem esperando para entrar (um ajuste ou uma resposta); espere o \
                 relatório antes de aprovar."
            ),
            _ => return None,
        };
        Some(recusa)
    }

    /// Texto de fundo da caixa de mensagem de um trabalhador, conforme a tarefa dele: curto, para
    /// caber no painel estreito. Que a mensagem a uma tarefa em revisão abre uma rodada nova, quem
    /// diz é a dica do enviar.
    pub fn dica_do_trabalhador(&self, chat_id: &str) -> &'static str {
        let chat = self.chat(chat_id);
        match chat.and_then(|c| c.tarefa).and_then(|t| self.tarefa(t)) {
            Some(t) if !t.preparada && !t.estado.ativa() => "Sem cópia: fale com o chat dele",
            Some(t) if t.estado.encerrada() && chat.is_some_and(|c| !c.cwd.is_dir()) => {
                "Cópia removida: fale com o chat dele"
            }
            _ => "Mensagem direta para este ajudante…",
        }
    }

    // ---------- projetos ----------

    /// Os projetos à vista, na ordem: no trilho, nos filtros, no "Nova conversa" e nas rotinas.
    pub fn projetos_visiveis(&self) -> Vec<&Projeto> {
        let mut lista: Vec<&Projeto> = self.projetos.iter().filter(|p| !p.arquivado).collect();
        lista.sort_by_key(|p| p.ordem);
        lista
    }

    /// Os arquivados, que voltam pelo menu do Vitral ou por "Todos os projetos".
    pub fn projetos_arquivados(&self) -> Vec<&Projeto> {
        let mut lista: Vec<&Projeto> = self.projetos.iter().filter(|p| p.arquivado).collect();
        lista.sort_by_key(|p| p.ordem);
        lista
    }

    pub fn projeto(&self, id: &str) -> Option<&Projeto> {
        self.projetos.iter().find(|p| p.id == id)
    }

    /// O nome que a interface mostra: o do projeto, "Avulsos" sem projeto, o id se ele sumiu.
    pub fn nome_do_projeto(&self, id: &str) -> String {
        match self.projeto(id) {
            Some(p) => p.nome.clone(),
            None if id.is_empty() => "Avulsos".into(),
            None => id.to_string(),
        }
    }

    /// O projeto pelo id, pelo nome ou pela chave (sem acento, sem diferenciar maiúsculas).
    pub fn projeto_por_nome(&self, texto: &str) -> Resolucao {
        resolver_projeto(&self.projetos, texto)
    }

    pub fn pastas_do(&self, id: &str) -> &[PastaLigada] {
        self.projeto(id).map_or(&[], |p| p.pastas.as_slice())
    }

    /// A primeira pasta do projeto, onde as conversas dele nascem.
    pub fn pasta_principal(&self, id: &str) -> Option<PathBuf> {
        self.projeto(id).and_then(|p| p.principal()).map(|p| p.caminho.clone())
    }

    /// O projeto que tem essa pasta ligada (pelo caminho canônico).
    pub fn projeto_da_pasta(&self, caminho: &Path) -> Option<String> {
        let canonico = dunce::canonicalize(caminho).unwrap_or_else(|_| caminho.to_path_buf());
        self.projetos
            .iter()
            .find(|p| p.pastas.iter().any(|l| l.caminho == canonico || l.caminho == caminho))
            .map(|p| p.id.clone())
    }

    /// A configuração da pasta onde a tarefa trabalha (a origem; sem ela, a principal).
    pub fn config_da_pasta(&self, projeto: &str, pasta: &Path) -> ConfigProjeto {
        let pastas = self.pastas_do(projeto);
        pastas
            .iter()
            .find(|l| l.caminho == pasta)
            .or(pastas.first())
            .map(|l| l.config.clone())
            .unwrap_or_default()
    }

    /// A origem da tarefa: a gravada ou, nas antigas sem ela, a pasta principal do projeto.
    pub fn origem_da_tarefa(&self, t: &Tarefa) -> PathBuf {
        if !t.origem.as_os_str().is_empty() {
            return t.origem.clone();
        }
        self.pasta_principal(&t.projeto).unwrap_or_default()
    }

    /// A pasta dos arquivos do projeto onde o chat está: a origem da tarefa, senão a principal.
    pub fn pasta_do_chat(&self, c: &Chat) -> Option<PathBuf> {
        match c.tarefa.and_then(|t| self.tarefa(t)) {
            Some(t) => Some(self.origem_da_tarefa(t)).filter(|o| !o.as_os_str().is_empty()),
            None => self.pasta_principal(&c.projeto),
        }
    }

    /// O nome de um projeto novo ou renomeado, ou por que ele não serve.
    pub fn validar_nome(&self, nome: &str, ignorar: Option<&str>) -> Result<String, String> {
        dados::chave_valida(nome, &self.projetos, ignorar).map(|_| nome.trim().to_string())
    }

    /// Cria o projeto. Com pastas, liga-as (canônicas); sem nenhuma, cria a pasta própria com git,
    /// em segundo plano. Devolve o id.
    pub fn criar_projeto(&mut self, novo: NovoProjeto, cx: &mut Context<Self>) -> Task<anyhow::Result<String>> {
        self.criar_projeto_com(novo, None, cx)
    }

    /// `cartao`: a sugestão do Mestre que este projeto atende; ela é aceita na mesma gravação.
    fn criar_projeto_com(&mut self, novo: NovoProjeto, cartao: Option<u64>, cx: &mut Context<Self>) -> Task<anyhow::Result<String>> {
        let nome = match self.validar_nome(&novo.nome, None) {
            Ok(nome) => nome,
            Err(erro) => return Task::ready(Err(anyhow::anyhow!(erro))),
        };
        if self.somente_leitura.is_some() {
            return Task::ready(Err(anyhow::anyhow!("o Vitral está aberto só para leitura")));
        }
        let dados = self.dados.clone();
        let (pastas, configs) = (novo.pastas.clone(), novo.configs.clone());
        cx.spawn(async move |motor, cx| {
            let nome_bg = nome.clone();
            let pastas = cx
                .background_spawn(async move {
                    let config = |i: usize| configs.get(i).cloned().unwrap_or_default();
                    if pastas.is_empty() {
                        let pasta = dados.criar_pasta_propria(&nome_bg)?;
                        return Ok::<_, anyhow::Error>(vec![PastaLigada { caminho: pasta, propria: true, config: config(0) }]);
                    }
                    pastas
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let caminho = dunce::canonicalize(&p).map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?;
                            Ok(PastaLigada { caminho, propria: false, config: config(i) })
                        })
                        .collect()
                })
                .await?;
            motor
                .update(cx, |m, cx| {
                    // Outro projeto pode ter tomado o nome enquanto a pasta era criada.
                    let nome = m.validar_nome(&nome, None).map_err(|e| anyhow::anyhow!(e))?;
                    let id = novo_id();
                    let ordem = m.projetos.iter().map(|p| p.ordem + 1).max().unwrap_or(0);
                    let vazio = |v: Option<String>| v.filter(|v| !v.trim().is_empty());
                    m.projetos.push(Projeto {
                        id: id.clone(),
                        nome,
                        icone: novo.icone,
                        cor: novo.cor,
                        descricao: novo.descricao,
                        instrucoes: novo.instrucoes.chars().take(MAX_INSTRUCOES).collect(),
                        pastas,
                        ordem,
                        criado_em: agora(),
                        permissao: vazio(novo.permissao),
                        modelo: vazio(novo.modelo),
                        conta: vazio(novo.conta),
                        linear: novo.linear,
                        ..Default::default()
                    });
                    if let Some(cartao) = cartao {
                        m.aceitar_sugestao(cartao, &id, cx);
                        if let Some(c) = m.chat(crate::modelo::ID_MESTRE) {
                            let _ = m.dados.salvar_chat(c);
                        }
                    }
                    m.salvar();
                    cx.notify();
                    Ok(id)
                })
                .map_err(|_| anyhow::anyhow!("o Vitral fechou no meio"))?
        })
    }

    /// Muda só o que veio na mudança. Recusa nome repetido ou reservado. As pastas novas ficam
    /// canônicas; sem nenhuma, o projeto fica com a pasta própria (a que ele já tinha, ou uma nova).
    pub fn mudar_projeto(&mut self, id: &str, mut mudanca: MudancaProjeto, cx: &mut Context<Self>) -> Result<(), String> {
        if self.somente_leitura.is_some() {
            return Err("o Vitral está aberto só para leitura".into());
        }
        if let Some(nome) = &mudanca.nome {
            self.validar_nome(nome, Some(id))?;
        }
        let projeto = self.projeto(id).ok_or("Projeto não encontrado.")?;
        if let Some(pastas) = mudanca.pastas.as_mut() {
            for p in pastas.iter_mut().filter(|p| !projeto.pastas.iter().any(|l| l.caminho == p.caminho)) {
                if let Ok(canonico) = dunce::canonicalize(&p.caminho) {
                    p.caminho = canonico;
                }
            }
            if pastas.is_empty() {
                let propria = match projeto.pastas.iter().find(|l| l.propria) {
                    Some(l) => l.clone(),
                    None => {
                        let nome = mudanca.nome.as_deref().unwrap_or(&projeto.nome);
                        let caminho = self.dados.criar_pasta_propria(nome).map_err(|e| format!("não criou a pasta própria: {e:#}"))?;
                        PastaLigada { caminho, propria: true, ..Default::default() }
                    }
                };
                pastas.push(propria);
            }
        }
        let projeto = self.projetos.iter_mut().find(|p| p.id == id).ok_or("Projeto não encontrado.")?;
        aplicar_mudanca(projeto, mudanca);
        self.salvar();
        cx.notify();
        Ok(())
    }

    pub fn projeto_arquivado(&self, projeto: &str) -> bool {
        self.projeto(projeto).is_some_and(|p| p.arquivado)
    }

    /// A conversa está em Arquivadas: arquivada ela mesma ou com o projeto.
    pub fn arquivada(&self, c: &Chat) -> bool {
        conversa_arquivada(&self.projetos, c)
    }

    /// Arquiva ou desarquiva o projeto: só a marca, e as conversas ficam como estavam. Arquivar
    /// solta as portas das tarefas dele; na volta, elas ganham faixas de novo e as conversas já
    /// lidas renovam o `lido_em`, como ao desarquivar uma conversa (senão o arquivamento
    /// automático as guardaria de novo na passada seguinte).
    pub fn arquivar_projeto(&mut self, projeto: &str, arquivar: bool, cx: &mut Context<Self>) {
        let Some(p) = self.projetos.iter_mut().find(|p| p.id == projeto) else {
            return;
        };
        p.arquivado = arquivar;
        if arquivar {
            for t in self.tarefas.iter_mut().filter(|t| t.projeto == projeto) {
                t.porta = 0;
            }
        } else {
            let agora = agora();
            for c in self.chats.iter_mut().filter(|c| c.projeto == projeto && !c.arquivado) {
                if c.lido_em >= c.atualizado_em || c.itens.is_empty() {
                    c.lido_em = c.lido_em.max(agora);
                }
            }
            let chats: Vec<String> = self.chats.iter().filter(|c| c.projeto == projeto).map(|c| c.id.clone()).collect();
            for c in chats {
                self.dar_porta(&c);
                if self.chat(&c).is_some_and(|c| !c.arquivado) {
                    self.entregar_fila(&c, cx);
                }
            }
        }
        self.salvar();
        cx.notify();
    }

    /// Põe o projeto antes ou depois de outro, no trilho.
    pub fn reordenar_projeto(&mut self, id: &str, alvo: &str, antes: bool, cx: &mut Context<Self>) {
        if reordenar(&mut self.projetos, id, alvo, antes) {
            self.salvar();
            cx.notify();
        }
    }

    /// "Localizar pasta…": a pasta que sumiu (renomeada, disco em outro lugar) passa a ser essa.
    /// As tarefas que tinham a antiga como origem vão junto, e as cópias delas voltam a apontar para
    /// a nova (`git worktree repair`, conferido pela `raiz_comum`). Com cópias, recusa uma pasta sem
    /// git ou de outro repositório (sem o commit de onde elas saíram). Devolve o aviso das cópias que
    /// não voltaram.
    pub fn localizar_pasta(&mut self, id: &str, indice: usize, nova: &Path, cx: &mut Context<Self>) -> Task<Result<Option<String>, String>> {
        let nova = match dunce::canonicalize(&nova) {
            Ok(nova) if nova.is_dir() => nova,
            Ok(nova) => return Task::ready(Err(format!("{} não é uma pasta", nova.display()))),
            Err(e) => return Task::ready(Err(format!("{}: {e}", nova.display()))),
        };
        if let Some(outro) = self.projeto_da_pasta(&nova).filter(|o| o != id) {
            return Task::ready(Err(format!("{} já é de {}", nova.display(), self.nome_do_projeto(&outro))));
        }
        let Some(projeto) = self.projeto(id) else {
            return Task::ready(Err("Projeto não encontrado.".into()));
        };
        let Some(antiga) = projeto.pastas.get(indice).map(|l| l.caminho.clone()) else {
            return Task::ready(Err("Pasta não encontrada.".into()));
        };
        let copias: Vec<(u64, PathBuf, Option<String>)> = self
            .tarefas
            .iter()
            .filter(|t| t.projeto == id && t.origem == antiga && t.tem_copia() && !t.copia_limpa)
            .filter_map(|t| Some((t.id, t.worktree.clone()?, t.base.clone())))
            .collect();
        let id = id.to_string();
        cx.spawn(async move |motor, cx| {
            let pasta = nova.clone();
            let fora: Vec<u64> = cx
                .background_spawn(async move {
                    let existem: Vec<&(u64, PathBuf, Option<String>)> = copias.iter().filter(|(_, w, _)| w.is_dir()).collect();
                    if existem.is_empty() {
                        return Ok(Vec::new());
                    }
                    let onde = mestre::com_til(&pasta);
                    if !git::usa_git(&pasta) {
                        return Err(format!("{onde} não usa git, e as cópias dos chats vêm de um repositório"));
                    }
                    if existem.iter().filter_map(|(_, _, base)| base.as_deref()).any(|base| !git::tem_commit(&pasta, base)) {
                        return Err(format!("{onde} não é o repositório de onde as cópias dos chats saíram"));
                    }
                    let caminhos: Vec<PathBuf> = existem.iter().map(|(_, w, _)| w.clone()).collect();
                    let fora = git::na_vez_do_projeto(&pasta, || git::reparar_copias(&pasta, &caminhos));
                    Ok(existem.iter().filter(|(_, w, _)| fora.contains(w)).map(|(t, _, _)| *t).collect())
                })
                .await?;
            motor
                .update(cx, |m, cx| {
                    let projeto = m.projetos.iter_mut().find(|p| p.id == id).ok_or("Projeto não encontrado.")?;
                    let pasta = projeto
                        .pastas
                        .iter_mut()
                        .find(|l| l.caminho == antiga)
                        .ok_or("a pasta saiu do projeto enquanto era localizada")?;
                    pasta.caminho = nova.clone();
                    projeto.provisorio = false;
                    for t in m.tarefas.iter_mut().filter(|t| t.projeto == id && t.origem == antiga) {
                        t.origem = nova.clone();
                        if t.falha_copia.as_deref().is_some_and(|f| f.starts_with("origem desconhecida")) {
                            t.falha_copia = None;
                        }
                    }
                    m.salvar();
                    cx.notify();
                    let chats = fora.iter().map(|t| format!("chat {t}")).collect::<Vec<_>>();
                    Ok(match chats.as_slice() {
                        [] => None,
                        [um] => Some(format!("A cópia do {um} não voltou a se ligar a {}; juntar vai recusar.", mestre::com_til(&nova))),
                        _ => Some(format!(
                            "As cópias de {} não voltaram a se ligar a {}; juntar vai recusar.",
                            chats.join(", "),
                            mestre::com_til(&nova)
                        )),
                    })
                })
                .map_err(|_| "o Vitral fechou no meio".to_string())?
        })
    }

    /// Se o chat pode ir para o projeto `destino` ("" é Avulsos), com o aviso a mostrar antes.
    /// Vai com os descendentes (os ajudantes dele).
    pub fn pode_mover(&self, chat: &str, destino: &str) -> Result<Option<String>, String> {
        let c = self.chat(chat).ok_or("Conversa não encontrada.")?;
        if c.papel == Papel::Mestre {
            return Err("O Mestre não pertence a projeto.".into());
        }
        if c.papel == Papel::Trabalhador && c.pai.is_some() {
            return Err("O ajudante vai junto com o chat dele.".into());
        }
        if c.projeto == destino {
            return Err("A conversa já está nesse projeto.".into());
        }
        if !destino.is_empty() {
            let p = self.projeto(destino).ok_or("Projeto não encontrado.")?;
            if p.arquivado {
                return Err(format!("{} está arquivado.", p.nome));
            }
        }
        let familia = descendentes(&self.chats, chat);
        if familia.iter().any(|id| !self.chat_livre(id)) {
            return Err("O chat (ou um ajudante dele) está trabalhando; espere terminar.".into());
        }
        let pastas: Vec<&Path> = self.pastas_do(destino).iter().map(|p| p.caminho.as_path()).collect();
        // Os ajudantes juntam na pasta do chat, que vai junto: só a cópia dele conta.
        let fora = self
            .tarefas
            .iter()
            .filter(|t| familia.contains(&t.trabalhador) && !t.estado.encerrada() && self.de_projeto(&t.trabalhador))
            .map(|t| self.origem_da_tarefa(t))
            .find(|o| !o.as_os_str().is_empty() && !pastas.contains(&o.as_path()));
        Ok(fora.map(|o| format!("As cópias continuam ligadas a {}; juntar junta lá.", mestre::com_til(&o))))
    }

    /// Muda só o projeto do chat, dos descendentes e das tarefas deles, numa gravação: a pasta, a
    /// origem e a forma ficam.
    pub fn mover_chat(&mut self, chat: &str, destino: &str, cx: &mut Context<Self>) -> Result<(), String> {
        self.pode_mover(chat, destino)?;
        let familia = descendentes(&self.chats, chat);
        for c in self.chats.iter_mut().filter(|c| familia.contains(&c.id)) {
            c.projeto = destino.to_string();
        }
        for t in self.tarefas.iter_mut().filter(|t| familia.contains(&t.trabalhador) || t.coordenador == chat) {
            t.projeto = destino.to_string();
        }
        let nome = self.nome_do_projeto(destino);
        self.avisar(chat, &format!("Esta conversa foi movida para {nome}."), false);
        self.salvar();
        cx.notify();
        Ok(())
    }

    /// Nada roda no chat: nem turno, nem vaga, nem nova tentativa.
    fn chat_livre(&self, id: &str) -> bool {
        !self.execucao(id).rodando()
            && !self.processos.contains_key(id)
            && !self.esperando_vaga(id)
            && !self.retentativas.contains_key(id)
    }

    /// O que o projeto tem em andamento: conversas trabalhando (turno, vaga, nova tentativa) e
    /// tarefas que o Vitral confere, junta ou prepara sozinho, que não dá para parar.
    pub fn ocupacao_do_projeto(&self, projeto: &str) -> Ocupacao {
        let conversas = self
            .chats
            .iter()
            .filter(|c| c.projeto == projeto)
            .filter(|c| {
                self.execucao(&c.id).rodando()
                    || self.processos.contains_key(&c.id)
                    || self.esperando_vaga(&c.id)
                    || self.retentativas.contains_key(&c.id)
            })
            .map(|c| c.id.clone())
            .collect();
        let tarefas = self
            .tarefas
            .iter()
            .filter(|t| t.projeto == projeto)
            .filter(|t| {
                self.verificando.contains_key(&t.id)
                    || self.juntando.contains(&t.id)
                    || self.preparando.contains(&t.id)
                    || self.entregas_a_retomar.contains(&t.id)
                    || self.preparos_a_retomar.contains(&t.id)
                    || self.sincronizando.contains(&t.id)
                    || t.juntando_desde > 0
                    || self.na_fila_pesada(t.id)
            })
            .map(|t| t.id)
            .collect();
        Ocupacao { conversas, tarefas }
    }

    /// O que vai junto quando o projeto sai: conversas, cópias das tarefas, rotinas e o que fica.
    pub fn resumo_do_projeto(&self, projeto: &str) -> ResumoDoProjeto {
        let (conversas, tarefas) = do_projeto(&self.chats, &self.tarefas, projeto);
        let papel = |p: Papel| self.chats.iter().filter(|c| conversas.contains(&c.id) && c.papel == p).count();
        let deles: Vec<&Tarefa> = self.tarefas.iter().filter(|t| tarefas.contains(&t.id)).collect();
        let pastas = self.pastas_do(projeto);
        ResumoDoProjeto {
            conversas: conversas.len() - papel(Papel::Trabalhador),
            trabalhadores: papel(Papel::Trabalhador),
            copias: deles.iter().filter_map(|t| t.worktree.clone()).filter(|c| c.exists()).collect(),
            rotinas: self.rotinas.iter().filter(|r| r.projeto == projeto).count(),
            ligadas: pastas.iter().filter(|p| !p.propria).map(|p| p.caminho.clone()).collect(),
            propria: pastas.iter().find(|p| p.propria).map(|p| p.caminho.clone()),
            branches: deles.iter().filter(|t| t.branch.is_some() && !t.copia_apagada()).count(),
            cartoes: self.cartoes.iter().filter(|c| cartao_com_parte_em(c, projeto)).count(),
        }
    }

    /// "Apagar projeto": o projeto e o que o Vitral guarda dele saem; as pastas ligadas ficam como
    /// estão (só o `git worktree prune` e os pontos de volta das tarefas apagadas). A pasta própria
    /// vai para a Lixeira só quando o projeto não tem pasta ligada; se ela não for, nada sai. As
    /// cópias vão para a Lixeira (guardam o que não foi juntado). Recusa com algo rodando.
    pub fn apagar_projeto(&mut self, projeto: &str, cx: &mut Context<Self>) -> Task<Result<Apagado, String>> {
        let Some(p) = self.projeto(projeto) else {
            return Task::ready(Err("o projeto não existe mais".into()));
        };
        if !self.ocupacao_do_projeto(projeto).livre() {
            return Task::ready(Err(format!("{} tem chats trabalhando agora", p.nome)));
        }
        let propria = match pasta_propria_para_lixeira(p, &self.dados.pasta_projetos()) {
            Ok(propria) => propria,
            Err(erro) => return Task::ready(Err(erro)),
        };
        let resumo = self.resumo_do_projeto(projeto);
        let (_, ids) = do_projeto(&self.chats, &self.tarefas, projeto);
        let mut origens: Vec<(PathBuf, Vec<String>)> = Vec::new();
        for t in self.tarefas.iter().filter(|t| ids.contains(&t.id)) {
            let origem = self.origem_da_tarefa(t);
            if origem.as_os_str().is_empty() || propria.as_ref() == Some(&origem) {
                continue;
            }
            let grupo = format!("t{}", t.id);
            match origens.iter_mut().find(|(o, _)| *o == origem) {
                Some((_, grupos)) => grupos.push(grupo),
                None => origens.push((origem, vec![grupo])),
            }
        }
        let projeto = projeto.to_string();
        let area = self.dados.pasta_copias(&projeto);
        cx.spawn(async move |motor, cx| {
            let feito = cx
                .background_spawn(async move {
                    let mut na_lixeira = 0;
                    if let Some(pasta) = propria.as_ref().filter(|p| p.symlink_metadata().is_ok()) {
                        git::na_vez_do_projeto(pasta, || crate::sistema::para_a_lixeira(pasta))
                            .map_err(|erro| format!("{erro:#}"))?;
                        na_lixeira += 1;
                    }
                    let mut erros = Vec::new();
                    for copia in &resumo.copias {
                        match crate::sistema::para_a_lixeira(copia) {
                            Ok(()) => na_lixeira += 1,
                            Err(erro) => erros.push(format!("{}: {erro:#}", copia.display())),
                        }
                    }
                    let _ = std::fs::remove_dir(&area);
                    for (origem, grupos) in origens.iter().filter(|(o, _)| o.is_dir()) {
                        git::na_vez_do_projeto(origem, || {
                            git::podar(origem);
                            git::apagar_pontos(origem, grupos);
                        });
                    }
                    Ok::<_, String>((na_lixeira, resumo.conversas, erros))
                })
                .await?;
            let (na_lixeira, conversas, erros) = feito;
            motor
                .update(cx, |m, cx| {
                    let retirado = tirar_do_estado(
                        &mut m.chats,
                        &mut m.tarefas,
                        &mut m.filas,
                        &mut m.rotinas,
                        &mut m.cartoes,
                        &projeto,
                    );
                    m.projetos.retain(|p| p.id != projeto);
                    m.esquecer(&retirado);
                    m.salvar();
                    for id in &retirado.chats {
                        m.dados.apagar_chat(id);
                    }
                    m.dados.apagar_notas(&projeto);
                    cx.notify();
                    Apagado { na_lixeira, conversas, erros }
                })
                .map_err(|_| "o Vitral fechou no meio".to_string())
        })
    }

    /// Esquece o que o motor guarda em memória das conversas e tarefas que saíram do estado. Um
    /// processo que tenha aberto depois da conferência sai junto; um pedido de permissão é negado.
    fn esquecer(&mut self, r: &Retirado) {
        let saiu = |id: &String| r.chats.contains(id);
        for id in &r.chats {
            if let Some(mut processo) = self.processos.remove(id) {
                processo.matar();
            }
            self.execucao.remove(id);
            self.geracao_atual.remove(id);
            self.ultima_entrada.remove(id);
            self.conta_do_turno.remove(id);
            self.parcial.remove(id);
            self.retentativas.remove(id);
            self.tentativas_auto.remove(id);
            self.chats_sujos.remove(id);
            self.repetidos_sessao.remove(id);
            self.passagens.remove(id);
            self.fundo.remove(id);
            self.fim_adiado.remove(id);
            self.visiveis.remove(id);
        }
        self.espera.retain(|c| !saiu(c));
        self.esperando_pasta.retain(|c| !saiu(c));
        self.sincronizados.retain(|c| !saiu(c));
        self.interrompidos.retain(|c| !saiu(c));
        self.aguardando_resposta.retain(|c| !saiu(c));
        let (deles, outros): (Vec<_>, Vec<_>) = self.permissoes.drain(..).partition(|p| saiu(&p.chat));
        self.permissoes = outros;
        for pedido in deles {
            let _ = pedido.resposta.try_send(Resposta::ok(
                json!({"behavior": "deny", "message": "O projeto foi apagado."}).to_string(),
            ));
        }
        for id in &r.tarefas {
            self.verificando.remove(id);
            self.juntando.remove(id);
            self.preparando.remove(id);
            self.sincronizando.remove(id);
            self.respostas_preparo.remove(id);
            self.respostas_parecer.remove(id);
        }
        self.pesado.retain(|p| !r.tarefas.contains(&p.tarefa()));
        self.entregas_a_retomar.retain(|t| !r.tarefas.contains(t));
        self.preparos_a_retomar.retain(|t| !r.tarefas.contains(t));
        if self.pedido_de_abrir.as_ref().is_some_and(saiu) {
            self.pedido_de_abrir = None;
        }
    }

    /// "Ativar git" numa pasta de projeto.
    pub fn ativar_git(&self, pasta: PathBuf, cx: &mut Context<Self>) -> Task<anyhow::Result<git::RepositorioNovo>> {
        cx.background_spawn(async move { git::iniciar_repositorio(&pasta) })
    }

    // ---------- ações da interface ----------

    /// Um chat novo no projeto ("" é Avulsos), com a tarefa dele em Conversa. A forma (cópia, direto
    /// com git, direto sem git) é decidida aqui, com a pasta presente, e fica gravada; a cópia só nasce
    /// na primeira mensagem. `pasta` escolhe uma das pastas ligadas (sem ela, a principal);
    /// `criado_por` é "" (o usuário), "rotina:<id>"…
    pub fn novo_chat(
        &mut self,
        projeto: &str,
        pasta: Option<&Path>,
        criado_por: &str,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let id = self.criar_chat(projeto, pasta, criado_por, cx)?;
        self.salvar();
        cx.notify();
        Ok(id)
    }

    /// O chat novo e a tarefa dele em conversa, sem gravar (o Começar da divisão grava tudo junto).
    fn criar_chat(
        &mut self,
        projeto: &str,
        pasta: Option<&Path>,
        criado_por: &str,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        if self.somente_leitura.is_some() {
            return Err("o Vitral está aberto só para leitura".into());
        }
        let id = novo_id();
        let (forma, origem, aviso) = if projeto == crate::modelo::SEM_PROJETO {
            let pasta = self.dados.pasta_avulso(&id);
            std::fs::create_dir_all(&pasta).map_err(|e| format!("{}: {e}", pasta.display()))?;
            (Forma::Direta, pasta, None)
        } else {
            let p = self.projeto(projeto).ok_or("Projeto não encontrado.")?;
            let ligada = dados::escolher_origem(p, pasta)?;
            let origem = ligada.caminho.clone();
            if !origem.is_dir() {
                return Err(format!(
                    "A pasta {} não foi encontrada (disco desmontado ou pasta movida).",
                    origem.display()
                ));
            }
            let mut com_git = git::usa_git(&origem);
            // Pasta própria sem commit (uma queda no meio da criação): o repositório é refeito.
            if ligada.propria && !com_git {
                git::iniciar_repositorio(&origem).map_err(|e| format!("não ativou o git em {}: {e:#}", origem.display()))?;
                com_git = true;
            }
            let topo = if com_git { Some(origem.clone()) } else { git::topo_do_repositorio(&origem) };
            let (forma, aviso) = dados::escolher_forma(ligada.propria, com_git, topo.as_deref(), &origem);
            (forma, origem, aviso)
        };
        if self.projeto_arquivado(projeto) {
            self.arquivar_projeto(projeto, false, cx);
        }
        let n = self.proxima_tarefa;
        self.proxima_tarefa += 1;
        let cwd = dados::cwd_inicial(forma, &self.dados.pasta_copias(projeto), n, &origem);
        let porta = dados::porta_livre(&self.tarefas).unwrap_or(0);
        self.tarefas.push(Tarefa {
            id: n,
            projeto: projeto.to_string(),
            trabalhador: id.clone(),
            titulo: "Nova conversa".into(),
            estado: EstadoTarefa::Conversa,
            forma,
            origem,
            porta,
            preparada: forma != Forma::Copia,
            criada_em: agora(),
            atualizada_em: agora(),
            ..Default::default()
        });
        let mut chat = Chat::novo(id.clone(), Papel::Projeto, projeto, "Nova conversa", cwd);
        chat.tarefa = Some(n);
        chat.criado_por = criado_por.to_string();
        if let Some(p) = self.projeto(projeto) {
            chat.permissao = p.permissao.clone();
            chat.modelo = p.modelo.clone();
            chat.conta = p.conta.clone();
        }
        self.chats.push(chat);
        if let Some(aviso) = aviso {
            self.avisar(&id, &aviso, false);
        }
        Ok(id)
    }

    /// O chat pode trocar de pasta: nunca rodou (sem sessão, sem mensagens, sem fila) e a cópia não
    /// está sendo preparada. Depois disso, a troca é um chat novo na outra pasta.
    pub fn pode_trocar_pasta(&self, chat: &str) -> bool {
        self.chat(chat).is_some_and(|c| {
            c.papel == Papel::Projeto
                && !c.projeto.is_empty()
                && !c.sessao_criada
                && c.itens.is_empty()
                && self.filas.get(chat).is_none_or(Vec::is_empty)
                && c.tarefa.is_some_and(|t| !self.preparando.contains(&t) && !self.na_fila_pesada(t))
        })
    }

    /// O chip "Pasta: …": refaz a tarefa do chat com a origem nova, ainda sem cópia (a forma é decidida
    /// de novo, com a pasta presente).
    pub fn trocar_pasta(&mut self, chat: &str, pasta: &Path, cx: &mut Context<Self>) -> Result<(), String> {
        if !self.pode_trocar_pasta(chat) {
            return Err("Este chat já rodou: para trabalhar em outra pasta, abra um chat novo nela.".into());
        }
        let c = self.chat(chat).ok_or("Chat não encontrado.")?;
        let (projeto, id) = (c.projeto.clone(), c.tarefa.ok_or("Este chat não trabalha numa pasta.")?);
        let p = self.projeto(&projeto).ok_or("Projeto não encontrado.")?;
        let ligada = dados::escolher_origem(p, Some(pasta))?;
        let origem = ligada.caminho.clone();
        if !origem.is_dir() {
            return Err(format!("A pasta {} não foi encontrada (disco desmontado ou pasta movida).", mestre::com_til(&origem)));
        }
        let com_git = git::usa_git(&origem);
        let topo = if com_git { Some(origem.clone()) } else { git::topo_do_repositorio(&origem) };
        let (forma, aviso) = dados::escolher_forma(ligada.propria, com_git, topo.as_deref(), &origem);
        let cwd = dados::cwd_inicial(forma, &self.dados.pasta_copias(&projeto), id, &origem);
        if let Some(t) = self.tarefa_mut(id) {
            (t.forma, t.origem, t.preparada) = (forma, origem, forma != Forma::Copia);
            (t.worktree, t.branch, t.base, t.ramo_alvo, t.falha_copia) = (None, None, None, None, None);
            // Uma preparação que ainda volte da pasta antiga é descartada.
            t.geracao_preparo += 1;
        }
        if let Some(c) = self.chat_mut(chat) {
            c.cwd = cwd;
        }
        if let Some(aviso) = aviso {
            self.avisar(chat, &aviso, false);
        }
        self.salvar();
        cx.notify();
        Ok(())
    }

    /// "Tentar preparar de novo": a cópia que falhou (ou a pasta que voltou) é preparada já, na
    /// frente da fila.
    pub fn tentar_preparar_de_novo(&mut self, chat: &str, cx: &mut Context<Self>) {
        if let Some(id) = self.chat(chat).and_then(|c| c.tarefa) {
            self.preparar_copia(id, None, true, cx);
        }
    }

    /// `reforco`: o Ultrathink e o Ultracode que estavam ligados na caixa (comando de barra não
    /// leva nenhum dos dois).
    pub fn enviar_do_usuario(
        &mut self,
        chat: &str,
        texto: String,
        anexos: Vec<PathBuf>,
        reforco: Reforco,
        cx: &mut Context<Self>,
    ) {
        self.enviar_do_usuario_com(chat, texto, anexos, reforco, &[], cx);
    }

    /// A mensagem do usuário, com as issues anexadas (`issues::enviar_com_issues`): o bloco de cada
    /// uma vai só ao Claude, e a mensagem traz texto de terceiros.
    fn enviar_do_usuario_com(
        &mut self,
        chat: &str,
        texto: String,
        anexos: Vec<PathBuf>,
        reforco: Reforco,
        issues: &[issues::AnexoIssue],
        cx: &mut Context<Self>,
    ) {
        let texto = texto.trim().to_string();
        if texto.is_empty() && anexos.is_empty() && issues.is_empty() {
            return;
        }
        // Comandos que o Vitral resolve sozinho.
        if matches!(texto.as_str(), "/clear" | "/nova" | "/new") {
            self.nova_sessao(chat, cx);
            return;
        }
        // O título sai da primeira mensagem; o de uma rotina ("Rotina: …") fica.
        let mut titulo = None;
        if let Some(c) = self.chat_mut(chat)
            && c.itens.is_empty()
            && c.papel.na_lista()
            && c.titulo == "Nova conversa"
            && !texto.starts_with('/')
        {
            c.titulo = titulo_de(&texto);
            titulo = Some((c.titulo.clone(), c.tarefa));
        }
        // No chat de projeto, a tarefa leva o mesmo título (é ele que dá o nome do branch da cópia).
        if let Some((titulo, Some(id))) = titulo
            && self.de_projeto(chat)
            && let Some(t) = self.tarefa_mut(id)
        {
            t.titulo = titulo;
        }
        if let Err(recusa) = self.mensagem_direta(chat, &texto, cx) {
            self.avisar(chat, &recusa, true);
            self.salvar();
            cx.notify();
            return;
        }
        self.tentativas_auto.remove(chat);
        // O usuário falou: os ciclos de mensagens automáticas desse chat recomeçam.
        self.ciclos.zerar(chat);
        let mut entrada = entrada_do_usuario(texto, anexos, reforco);
        if self.chat(chat).is_some_and(|c| c.papel == Papel::Mestre) {
            entrada.extra = self.extra_para_o_mestre(&entrada.texto);
        }
        if !issues.is_empty() {
            let blocos: Vec<&str> = issues.iter().map(|a| a.bloco.trim()).filter(|b| !b.is_empty()).collect();
            if !blocos.is_empty() {
                let antes = std::mem::take(&mut entrada.extra);
                entrada.extra = std::iter::once(antes.as_str()).filter(|e| !e.is_empty()).chain(blocos).collect::<Vec<_>>().join("\n\n");
            }
            entrada.terceiros = true;
            entrada.referencias = issues.iter().map(|a| a.issue.chave.clone()).collect();
        }
        self.entregar_entrada(chat, entrada, cx);
    }

    /// Mensagem do usuário direto a um ajudante. Com ele em revisão, vira uma nova rodada (com
    /// verificação, revisor e relatório) e o chat dele fica sabendo; sem a cópia do ajudante
    /// encerrado, não há onde rodar. A conversa do Vitral antigo não recebe nada.
    fn mensagem_direta(&mut self, chat_id: &str, texto: &str, cx: &mut Context<Self>) -> Result<(), String> {
        if self.chat(chat_id).is_some_and(|c| c.papel == Papel::Coordenador) {
            return Err(CONVERSA_ANTIGA.into());
        }
        // O pedido do Mestre em revisão num chat de projeto: a mensagem abre uma rodada nova e o
        // Mestre fica sabendo sem acordar.
        if let Some(id) = self.chat(chat_id).filter(|c| c.papel == Papel::Projeto).and_then(|c| c.tarefa)
            && !texto.starts_with('/')
            && let Some(t) = self.tarefa_mut(id)
            && t.estado == EstadoTarefa::EmRevisao
            && (t.dono() == Some(crate::modelo::ID_MESTRE) || (t.dono().is_none() && t.pedido_da_issue))
        {
            t.rodada += 1;
            t.estado = EstadoTarefa::Ajustando;
            (t.parada, t.entrega_pendente, t.entrega, t.progresso) = (None, None, None, None);
            // O pedido de uma issue é do próprio usuário: a mensagem dele abre a rodada nova.
            if t.dono().is_some() {
                self.nota_ao_mestre(format!("O usuário falou direto com o chat {id}; o novo relatório chega quando ele entregar."), cx);
            }
            return Ok(());
        }
        let Some(chat) = self.chat(chat_id).filter(|c| c.papel == Papel::Trabalhador) else {
            return Ok(());
        };
        let Some(id) = chat.tarefa else {
            return Ok(());
        };
        let sem_copia = !chat.cwd.is_dir();
        let juntando = self.juntando.contains(&id);
        let Some(t) = self.tarefa_mut(id) else {
            return Ok(());
        };
        if juntando {
            return Err(format!(
                "O ajudante #{id} está sendo juntado na pasta do chat dele agora; a mensagem não foi enviada."
            ));
        }
        if !t.preparada && !t.estado.ativa() {
            return Err(SEM_COPIA_CRIADA.into());
        }
        if t.estado.encerrada() && sem_copia {
            return Err(format!(
                "O ajudante #{id} está {} e a cópia dele foi removida; a mensagem não foi enviada. \
                 Fale com o chat dele.",
                t.estado.nome()
            ));
        }
        // Comandos do Claude Code (/compact, /cost) não mexem no código.
        if t.estado != EstadoTarefa::EmRevisao || texto.starts_with('/') {
            return Ok(());
        }
        t.rodada += 1;
        t.estado = EstadoTarefa::Ajustando;
        t.parada = None;
        t.entrega_pendente = None;
        t.progresso = None;
        let (rodada, coordenador) = (t.rodada, t.coordenador.clone());
        let aviso = format!(
            "O usuário falou direto com o ajudante #{id}, que estava em revisão, e isso abriu a rodada \
             {rodada}:\n{}\nO novo relatório chega sozinho quando ele terminar.",
            git::inicio_do_texto(texto, 600)
        );
        // Só fica sabendo: quem acorda o chat é o relatório.
        let nota = Entrada { de: Some("Vitral".into()), texto: aviso, nota: true, ..Default::default() };
        self.entregar_entrada(&coordenador, nota, cx);
        Ok(())
    }

    pub fn parar(&mut self, chat: &str, cx: &mut Context<Self>) {
        // A fila fica guardada: entra junto com a próxima mensagem (ou em "Entregar agora").
        let retentava = self.retentativas.remove(chat).is_some();
        let esperava = self.esperando_vaga(chat);
        self.espera.retain(|c| c != chat);
        if esperava || retentava {
            self.marcar_parada(chat, PARADA_VOCE);
        }
        if let Some(processo) = self.processos.get_mut(chat) {
            // Esperando os agentes em segundo plano, não há turno para interromper: o processo
            // sai com eles (SIGTERM em tudo que ele abriu).
            if self.fim_adiado.contains_key(chat) {
                processo.matar();
            } else {
                processo.interromper();
                self.conferir_parada(chat, cx);
            }
            self.interrompidos.push(chat.to_string());
        }
        // Permissões pendentes deste chat são negadas.
        let (deste, outros): (Vec<_>, Vec<_>) =
            self.permissoes.drain(..).partition(|p| p.chat == chat);
        self.permissoes = outros;
        for pedido in deste {
            let _ = pedido.resposta.try_send(Resposta::ok(
                json!({"behavior": "deny", "message": "O usuário interrompeu."}).to_string(),
            ));
        }
        cx.notify();
    }

    /// O Claude Code interrompido pela entrada sai em menos de um segundo. Se o mesmo processo ainda
    /// roda 5 s depois (um Claude Code que não leu o pedido, ou travado), ele e o que abriu param.
    fn conferir_parada(&mut self, chat: &str, cx: &mut Context<Self>) {
        let Some(geracao) = self.geracao_atual.get(chat).copied() else {
            return;
        };
        let id = chat.to_string();
        cx.spawn(async move |motor, cx| {
            cx.background_executor().timer(std::time::Duration::from_secs(5)).await;
            let _ = motor.update(cx, |m, _| {
                if m.geracao_atual.get(&id) == Some(&geracao)
                    && let Some(processo) = m.processos.get_mut(&id)
                {
                    processo.matar();
                }
            });
        })
        .detach();
    }

    /// Responde a um pedido de permissão. Negar com mensagem manda a mensagem ao Claude (é como
    /// se pede mudanças num plano).
    pub fn responder_permissao(
        &mut self,
        id: &str,
        permitir: bool,
        mensagem: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(posicao) = self.permissoes.iter().position(|p| p.id == id) else {
            return;
        };
        let pedido = self.permissoes.remove(posicao);
        // Aprovar o plano sai do Planejar, como na extensão: o resto do turno e os próximos rodam
        // no modo de antes (sem isso o Claude Code voltaria ao Manual e o turno seguinte, ao plano).
        // O plano que o próprio Claude abriu, ou com o modo trocado no meio, volta ao da conversa.
        let modo = (permitir && pedido.ferramenta == "ExitPlanMode").then(|| match self.chat(&pedido.chat) {
            Some(chat) => self.modo_ao_aprovar_plano(chat),
            None => modo_depois_do_plano(None, None, &self.config.permissao),
        });
        let resposta = if permitir {
            let mut resposta = json!({"behavior": "allow", "updatedInput": pedido.entrada});
            if let Some(modo) = &modo {
                resposta["updatedPermissions"] =
                    json!([{"type": "setMode", "mode": modo, "destination": "session"}]);
            }
            resposta
        } else {
            let mensagem = mensagem
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| "O usuário negou esta ação.".into());
            json!({"behavior": "deny", "message": mensagem})
        };
        let _ = pedido.resposta.try_send(Resposta::ok(resposta.to_string()));
        if let Some(modo) = modo
            && let Some(c) = self.chat_mut(&pedido.chat)
        {
            c.permissao = Some(modo);
            c.modo_antes_do_plano = None;
            self.salvar();
        }
        if !self.permissoes.iter().any(|p| p.chat == pedido.chat)
            && self.execucao(&pedido.chat) == Execucao::EsperandoVoce
        {
            self.execucao
                .insert(pedido.chat.clone(), Execucao::Rodando { desde: agora() });
        }
        cx.notify();
    }

    /// Permite o pedido e guarda a regra na conversa: os pedidos iguais passam sem perguntar, já
    /// neste turno (os que estavam esperando também). Edições no modo Manual também mudam a
    /// conversa para Aceitar edições nos próximos turnos.
    pub fn permitir_sempre(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(pedido) = self.permissoes.iter().find(|p| p.id == id) else {
            return;
        };
        let chat_id = pedido.chat.clone();
        let regra = self.chat(&chat_id).and_then(|c| {
            crate::modelo::regra_de_permissao(&pedido.ferramenta, &pedido.entrada, &c.cwd)
        });
        let manual = self.chat(&chat_id).is_some_and(|c| self.permissao_de(c) == "manual");
        if let Some(regra) = regra
            && let Some(c) = self.chat_mut(&chat_id)
        {
            if regra == "edicoes" && manual {
                c.permissao = Some("acceptEdits".into());
            }
            if !c.permitidas.contains(&regra) {
                c.permitidas.push(regra);
            }
            self.salvar();
        }
        let chat = self.chat(&chat_id);
        let cobertos: Vec<String> = self
            .permissoes
            .iter()
            .filter(|p| p.chat == chat_id)
            .filter(|p| p.id == id || chat.is_some_and(|c| c.permite(&p.ferramenta, &p.entrada)))
            .map(|p| p.id.clone())
            .collect();
        for pedido in cobertos {
            self.responder_permissao(&pedido, true, None, cx);
        }
    }

    pub fn renomear(&mut self, chat: &str, titulo: String, cx: &mut Context<Self>) {
        if let Some(c) = self.chat_mut(chat).filter(|c| c.papel != Papel::Mestre) {
            c.titulo = titulo;
        }
        self.salvar();
        cx.notify();
    }

    pub fn alternar_fixado(&mut self, chat: &str, cx: &mut Context<Self>) {
        if let Some(c) = self.chat_mut(chat).filter(|c| c.papel != Papel::Mestre) {
            c.fixado = !c.fixado;
        }
        self.salvar();
        cx.notify();
    }

    pub fn alternar_arquivado(&mut self, chat: &str, cx: &mut Context<Self>) {
        if self.chat(chat).is_some_and(|c| c.papel == Papel::Mestre) {
            return;
        }
        if let Some(c) = self.chat_mut(chat) {
            trocar_arquivado(c, agora());
        }
        // Arquivado, o Ship do PR dele desarma (o PR continua aberto; a interface oferece fechar).
        if self.chat(chat).is_some_and(|c| c.arquivado) {
            self.desarmar_ao_arquivar(chat, cx);
        }
        // A tarefa do chat arquivado solta as portas; a do desarquivado ganha de novo (e o que esperou
        // por ele entra).
        if self.chat(chat).is_some_and(|c| c.arquivado) {
            self.soltar_porta(chat);
        } else {
            self.dar_porta(chat);
            self.entregar_fila(chat, cx);
        }
        self.salvar();
        cx.notify();
    }

    /// O chat de projeto que nunca rodou: sem sessão, só com os avisos do Vitral, sem fila, sem
    /// ajudantes, sem nada trabalhando e com a tarefa em conversa, ainda sem cópia.
    pub fn chat_vazio(&self, chat: &str) -> bool {
        let Some(c) = self.chat(chat) else {
            return false;
        };
        let tarefas: Vec<&Tarefa> = self.tarefas.iter().filter(|t| t.trabalhador == chat).collect();
        nunca_rodou(c, &tarefas)
            && self.filas.get(chat).is_none_or(Vec::is_empty)
            && self.chat_livre(chat)
            && !self.chats.iter().any(|f| f.pai.as_deref() == Some(chat))
            && tarefas.iter().all(|t| !self.preparando.contains(&t.id) && !self.na_fila_pesada(t.id))
    }

    /// "Arquivar" no menu do chat: o chat de projeto vazio sai do Vitral em vez de ir para
    /// Arquivadas (com a pasta do avulso, se ela continua vazia). Devolve se apagou.
    pub fn arquivar_chat(&mut self, chat: &str, cx: &mut Context<Self>) -> bool {
        if !self.chat_vazio(chat) {
            if self.chat(chat).is_some_and(|c| !c.arquivado) {
                self.alternar_arquivado(chat, cx);
            }
            return false;
        }
        let tarefas: Vec<u64> = self.tarefas.iter().filter(|t| t.trabalhador == chat).map(|t| t.id).collect();
        let cwd = self.chat(chat).map(|c| c.cwd.clone());
        self.chats.retain(|c| c.id != chat);
        self.tarefas.retain(|t| !tarefas.contains(&t.id));
        self.filas.remove(chat);
        self.esquecer(&Retirado { chats: vec![chat.to_string()], tarefas, copias: Vec::new(), rotinas: 0 });
        self.salvar();
        self.dados.apagar_chat(chat);
        let avulsos = self.dados.pasta_avulso("");
        if let Some(cwd) = cwd.filter(|c| c.starts_with(&avulsos) && *c != avulsos) {
            let _ = std::fs::remove_dir(cwd);
        }
        cx.notify();
        true
    }

    /// Arquiva várias conversas de uma vez (o "Arquivar estas" da lista e o automático).
    pub fn arquivar_varios(&mut self, ids: &[String], cx: &mut Context<Self>) {
        for c in self.chats.iter_mut().filter(|c| ids.contains(&c.id) && c.papel != Papel::Mestre) {
            c.arquivado = true;
        }
        for id in ids {
            self.soltar_porta(id);
        }
        self.salvar();
        cx.notify();
    }

    /// Arquiva sozinho os coordenadores parados há mais de `arquivar_apos_dias` (0 = nunca): já
    /// lidos, não fixados, sem fila, sem ninguém da família rodando ou pedindo e com as tarefas
    /// encerradas. É só a marca: desarquivar devolve tudo como estava.
    fn arquivar_paradas(&mut self, cx: &mut Context<Self>) {
        let dias = self.config.arquivar_apos_dias;
        if dias == 0 {
            return;
        }
        let limite = agora().saturating_sub(u64::from(dias) * 86_400);
        let parados: Vec<String> = self
            .chats
            .iter()
            .filter(|c| candidata_a_arquivar(c, limite) && !self.projeto_arquivado(&c.projeto))
            .filter(|c| !self.nao_lido(c) && self.na_fila(&c.id) == 0)
            .filter(|c| {
                self.chats
                    .iter()
                    .filter(|t| t.id == c.id || t.pai.as_deref() == Some(c.id.as_str()))
                    .all(|t| {
                        self.execucao(&t.id) == Execucao::Parado
                            && !self.esperando_vaga(&t.id)
                            // Parou no meio (espera o Retomar) ou tem nova tentativa marcada.
                            && self.parada(&t.id).is_none()
                            && !self.retentativas.contains_key(&t.id)
                    })
            })
            .filter(|c| self.tarefas_do(&c.id).all(|t| t.estado.encerrada()))
            // O chat de projeto só com a tarefa em conversa e nada para juntar.
            .filter(|c| {
                c.papel != Papel::Projeto
                    || c.tarefa.and_then(|t| self.tarefa(t)).is_some_and(|t| {
                        t.estado == EstadoTarefa::Conversa
                            && t.mudancas.as_ref().is_none_or(|m| m.arquivos == 0)
                            && !self.na_fila_pesada(t.id)
                            && !self.sincronizando.contains(&t.id)
                    })
            })
            .map(|c| c.id.clone())
            .collect();
        if !parados.is_empty() {
            self.arquivar_varios(&parados, cx);
        }
    }

    pub fn mudar_config(&mut self, f: impl FnOnce(&mut Config), cx: &mut Context<Self>) {
        f(&mut self.config);
        self.gravar_config();
        cx.notify();
    }

    /// As rotinas ficam no estado.
    pub fn mudar_rotinas(&mut self, f: impl FnOnce(&mut Vec<Rotina>), cx: &mut Context<Self>) {
        f(&mut self.rotinas);
        self.salvar();
        cx.notify();
    }

    /// Para todos os processos (ao fechar a janela).
    pub fn encerrar_tudo(&mut self) {
        self.encerrado.store(true, Ordering::SeqCst);
        for (_, mut processo) in self.processos.drain() {
            processo.matar();
        }
        // Verificações e revisões em andamento, e o que os processos deixaram em segundo plano.
        crate::processos::encerrar_todos();
        if self.somente_leitura.is_none() {
            for chat in &self.chats {
                let _ = self.dados.salvar_chat(chat);
            }
        }
        self.salvar();
        self.dados.esperar_gravacoes();
    }

    // ---------- turnos ----------

    /// Entrega uma mensagem a um chat: começa um turno, ou espera na fila se ele estiver ocupado.
    fn entregar(&mut self, chat: &str, de: Option<String>, texto: String, cx: &mut Context<Self>) {
        self.entregar_entrada(
            chat,
            Entrada {
                de,
                texto,
                anexos: Vec::new(),
                ..Default::default()
            },
            cx,
        );
    }

    fn entregar_entrada(&mut self, chat: &str, entrada: Entrada, cx: &mut Context<Self>) {
        // Qualquer mensagem do coordenador (ou do usuário) responde a uma pergunta pendente.
        let do_dono = entrada.remetente.as_deref().is_some_and(|r| {
            self.chat(chat).and_then(|c| c.tarefa).and_then(|t| self.tarefa(t)).is_some_and(|t| t.coordenador == r)
        });
        let do_coordenador = do_dono
            || match &entrada.de {
                None => true,
                Some(origem) => self
                    .chat(chat)
                    .and_then(|c| c.pai.as_ref())
                    .and_then(|pai| self.chat(pai))
                    .is_some_and(|pai| origem == "coordenador" || origem.ends_with(&format!("({})", pai.id))),
            };
        if do_coordenador {
            self.aguardando_resposta.retain(|c| c != chat);
        }
        let nota = entrada.nota;
        self.filas.entry(chat.to_string()).or_default().push(entrada);
        let ocupado = self.processos.contains_key(chat) || self.esperando_vaga(chat);
        if ocupado || nota {
            self.salvar();
            cx.notify();
            return;
        }
        // Livre: entra tudo o que estava esperando, junto com a mensagem nova.
        self.entregar_fila(chat, cx);
    }

    /// Começa um turno com as mensagens que estavam na fila de um chat parado.
    pub fn entregar_fila(&mut self, chat: &str, cx: &mut Context<Self>) {
        if self.processos.contains_key(chat) || self.esperando_vaga(chat) || self.na_fila(chat) == 0 {
            return;
        }
        if let Some(fila) = self.filas.remove(chat) {
            self.comecar_turno(chat, fila, cx);
        }
    }

    /// Repete o último turno de um chat que falhou.
    pub fn tentar_de_novo(&mut self, chat: &str, cx: &mut Context<Self>) {
        if self.processos.contains_key(chat) {
            return;
        }
        if let Some((_, mensagens)) = self.ultima_entrada.get(chat).cloned() {
            self.avisar(chat, "Tentando de novo.", false);
            self.comecar_turno_com(chat, mensagens, false, cx);
        }
    }

    pub fn pode_tentar_de_novo(&self, chat: &str) -> bool {
        matches!(self.execucao(chat), Execucao::Erro(_)) && self.ultima_entrada.contains_key(chat)
    }

    /// Faz andar de novo um chat que parou no meio: refaz a entrega cortada, entrega a fila ou
    /// pede para o Claude continuar de onde parou.
    pub fn retomar(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let Some(motivo) = self.parada(chat_id) else {
            return;
        };
        let Some(chat) = self.chat(chat_id).cloned() else {
            return;
        };
        let tarefa = chat.tarefa.and_then(|t| self.tarefa(t)).cloned();
        if let Some(t) = &tarefa
            && t.estado == EstadoTarefa::EmRevisao
            && t.entrega_pendente.is_some()
            && self.tarefa_parada(t)
        {
            // Pela fila da abertura, uma verificação por vez (a fonte do PC não aguenta duas), e
            // a marca de parada fica enquanto roda.
            if !self.entregas_a_retomar.contains(&t.id) {
                self.entregas_a_retomar.push_back(t.id);
            }
            self.enfileirar_pesado(Pesado::Conferir(t.id), true, cx);
        } else if let Some(t) = &tarefa
            && precisa_preparar(t, &chat, &self.filas)
        {
            // O Vitral fechou durante a preparação da cópia: ela é refeita e o pedido vai depois.
            self.preparar_copia(t.id, None, true, cx);
        } else if !recebeu_mensagem(&chat) && self.na_fila(chat_id) > 0 {
            // Esperava vaga: o pedido da tarefa está na fila.
            self.entregar_fila(chat_id, cx);
        } else {
            // Vai junto com o que estiver na fila (relatórios, respostas).
            let texto = format!(
                "O turno anterior parou antes de terminar ({}). Continue de onde parou.",
                motivo_para_o_claude(&motivo)
            );
            self.entregar(chat_id, Some("Vitral".into()), texto, cx);
        }
        self.salvar();
        cx.notify();
    }

    /// Marca a tarefa ativa do trabalhador como parada no meio, com o motivo.
    fn marcar_parada(&mut self, chat_id: &str, motivo: &str) {
        let Some(id) = self.chat(chat_id).and_then(|c| c.tarefa) else {
            return;
        };
        if let Some(t) = self.tarefa_mut(id)
            && t.estado.ativa()
        {
            t.parada = Some(motivo.to_string());
            t.progresso = None;
        }
    }

    /// Depois de uma queda, agenda uma única nova tentativa do turno de um trabalhador com tarefa
    /// em andamento (o Claude Code já refaz sozinho as falhas curtas de rede). Diz se agendou.
    fn tentar_sozinho(&mut self, chat_id: &str, motivo: &str, segundos: u64, cx: &mut Context<Self>) -> bool {
        let Some(tarefa) = self
            .chat(chat_id)
            .filter(|c| c.papel == Papel::Trabalhador || c.papel == Papel::Projeto)
            .and_then(|c| c.tarefa)
            .filter(|t| self.tarefa(*t).is_some_and(|t| t.estado.ativa() && (t.dono().is_some() || !self.de_projeto(&t.trabalhador))))
        else {
            return false;
        };
        if !self.ultima_entrada.contains_key(chat_id) || !self.tentativas_auto.insert(chat_id.to_string()) {
            return false;
        }
        self.avisar(chat_id, &format!("{motivo}; tentando de novo em {segundos} s."), false);
        if let Some(t) = self.tarefa_mut(tarefa) {
            t.progresso = Some(TENTANDO.into());
        }
        self.execucao.insert(chat_id.to_string(), Execucao::Parado);
        let marca = self.geracao;
        self.retentativas.insert(chat_id.to_string(), (marca, agora() + segundos));
        let id = chat_id.to_string();
        cx.spawn(async move |motor, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(segundos))
                .await;
            let _ = motor.update(cx, |m, cx| m.retentar(&id, marca, cx));
        })
        .detach();
        true
    }

    /// Repete o turno que caiu, se ninguém mexeu no chat durante a espera.
    fn retentar(&mut self, chat_id: &str, marca: u64, cx: &mut Context<Self>) {
        if self.retentativas.get(chat_id).map(|r| r.0) != Some(marca) {
            return;
        }
        let ativa = self
            .chat(chat_id)
            .and_then(|c| c.tarefa)
            .and_then(|t| self.tarefa(t))
            .is_some_and(|t| t.estado.ativa());
        if ativa
            && !self.processos.contains_key(chat_id)
            && let Some(mensagens) = self.entrada_para_repetir(chat_id)
        {
            self.comecar_turno_com(chat_id, mensagens, false, cx);
        } else {
            self.retentativas.remove(chat_id);
        }
        cx.notify();
    }

    /// O último turno para uma repetição automática (queda, erro da API, troca de conta): sem o
    /// Ultracode, que rodaria um fluxo inteiro de novo sem o usuário pedir. Só o "Tentar de novo"
    /// o repete.
    fn entrada_para_repetir(&mut self, chat_id: &str) -> Option<Vec<Entrada>> {
        let (_, mut mensagens) = self.ultima_entrada.get(chat_id).cloned()?;
        if mensagens.iter().any(|e| e.ultracode) {
            for entrada in &mut mensagens {
                entrada.ultracode = false;
            }
            self.avisar(chat_id, "A nova tentativa vai sem o Ultracode; ligue de novo se quiser.", false);
        }
        Some(mensagens)
    }

    /// Falha sem nova tentativa: a tarefa do trabalhador volta ao coordenador como entrega com
    /// erro, e o usuário é avisado se a janela estiver em segundo plano.
    fn desistir(&mut self, chat_id: &str, detalhe: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chat(chat_id).cloned() else {
            return;
        };
        self.tentativas_auto.remove(chat_id);
        let ativa = chat
            .tarefa
            .filter(|t| self.tarefa(*t).is_some_and(|t| t.estado.ativa()));
        let com_dono = ativa.and_then(|t| self.tarefa(t)).is_some_and(|t| t.dono().is_some());
        if (chat.papel == Papel::Trabalhador || (chat.papel == Papel::Projeto && com_dono))
            && let Some(id) = ativa
        {
            let relatorio = format!(
                "O processo do Claude Code caiu e o pedido não terminou: {detalhe}\nO que já estava feito \
                 continua na cópia."
            );
            self.registrar("falhou", "vitral", chat_id, &chat.projeto, Some(id), &format!("chat {id} caiu: {}", git::inicio_do_texto(detalhe, 200)));
            self.tarefa_terminou(id, true, relatorio, cx);
            if !self.janela_ativa {
                self.notificar(&chat, "Parou com erro: o processo caiu e o pedido voltou a quem pediu");
            }
        } else if !self.janela_ativa {
            self.notificar(&chat, "Parou com erro: o processo do Claude caiu");
        }
    }

    fn comecar_turno(&mut self, chat_id: &str, mensagens: Vec<Entrada>, cx: &mut Context<Self>) {
        self.comecar_turno_com(chat_id, mensagens, true, cx);
    }

    fn comecar_turno_com(
        &mut self,
        chat_id: &str,
        mut mensagens: Vec<Entrada>,
        registrar: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(mut chat) = self.chat_mut(chat_id).map(Chat::copia_sem_itens) else {
            return;
        };
        if self.somente_leitura.is_some() {
            return;
        }
        // Repetição do turno: se for esperar na fila, não entra de novo na conversa depois.
        if !registrar {
            for entrada in &mut mensagens {
                entrada.registrada = true;
            }
        }
        // Sem conseguir gravar, nada novo começa: a mensagem espera na fila até voltar.
        if self.falha_gravacao.is_some() {
            self.esperar_na_fila(chat_id, mensagens, "Não estou conseguindo gravar; a mensagem entra quando voltar a gravar.");
            cx.notify();
            return;
        }
        // A conversa do Vitral antigo fica só para leitura: o que chegar para ela não roda.
        if chat.papel == Papel::Coordenador {
            let ja = self
                .chat(chat_id)
                .and_then(|c| c.itens.last())
                .is_some_and(|i| matches!(i, Item::Aviso { texto, .. } if texto == CONVERSA_ANTIGA));
            if !ja {
                self.avisar(chat_id, CONVERSA_ANTIGA, false);
            }
            cx.notify();
            return;
        }
        // No Mestre, o que o usuário escreveu vem antes dos avisos e relatórios, no texto e na conversa.
        if chat.papel == Papel::Mestre {
            mestre::usuario_primeiro(&mut mensagens);
        }
        // Um turno novo toma o lugar da nova tentativa agendada.
        self.retentativas.remove(chat_id);
        let tarefa = chat.tarefa.and_then(|t| self.tarefa(t)).cloned();
        if chat.papel == Papel::Projeto
            && let Some(t) = &tarefa
        {
            if self.arquivada(&chat) {
                self.esperar_na_fila(chat_id, mensagens, "Este chat está arquivado; a mensagem entra quando ele for desarquivado.");
                self.salvar();
                cx.notify();
                return;
            }
            // A cópia nasce (ou renasce, depois de uma limpeza) na primeira mensagem, que espera por ela.
            let preparando = self.preparando.contains(&t.id)
                || self.preparos_a_retomar.contains(&t.id)
                || self.pesado.contains(&Pesado::Preparar(t.id));
            match guarda_da_copia(t, preparando) {
                GuardaCopia::Segue => {}
                GuardaCopia::Esperar | GuardaCopia::Preparar => {
                    let aviso = if t.branch.is_some() && t.worktree.is_none() {
                        format!("Refazendo a cópia de {}; a mensagem entra quando ela ficar pronta.", mestre::com_til(&t.origem))
                    } else {
                        format!("Preparando a cópia de {}; a mensagem entra quando ela ficar pronta.", mestre::com_til(&t.origem))
                    };
                    self.esperar_na_fila(chat_id, mensagens, &aviso);
                    if guarda_da_copia(t, preparando) == GuardaCopia::Preparar {
                        self.preparar_copia(t.id, None, true, cx);
                    }
                    self.salvar();
                    cx.notify();
                    return;
                }
            }
            if self.juntando.contains(&t.id) || self.sincronizando.contains(&t.id) || t.juntando_desde > 0 {
                self.esperar_na_fila(chat_id, mensagens, "Estou juntando as mudanças deste chat; a mensagem entra em seguida.");
                cx.notify();
                return;
            }
            // A cópia sem nada só dela anda com a origem antes do turno (o que outro chat juntou).
            let sem_trabalho = t.mudancas.as_ref().is_none_or(|m| m.arquivos == 0);
            if t.tem_copia() && sem_trabalho && t.worktree.is_some() && !self.sincronizados.remove(chat_id) {
                self.filas.entry(chat_id.to_string()).or_default().extend(mensagens);
                self.sincronizar_antes_do_turno(t.id, cx);
                return;
            }
            if let Some(pasta) = self.escritor_na_pasta(&chat, t) {
                let aviso = format!(
                    "Outro chat está mexendo em {}; esta mensagem entra quando ele terminar.",
                    mestre::com_til(&pasta)
                );
                self.esperar_na_fila(chat_id, mensagens, &aviso);
                if !self.esperando_pasta.iter().any(|c| c == chat_id) {
                    self.esperando_pasta.push(chat_id.to_string());
                }
                cx.notify();
                return;
            }
        }
        // Sem a cópia pronta, o trabalhador não roda: rodaria na pasta original, sem o pedido.
        if let Some(t) = tarefa
            .as_ref()
            .filter(|t| !t.preparada && chat.papel == Papel::Trabalhador)
            .cloned()
        {
            // Tarefa encerrada sem cópia nunca vai rodar: a fila ficaria parada para sempre.
            if !t.estado.ativa() {
                self.avisar(chat_id, SEM_COPIA_CRIADA, true);
                self.salvar();
                cx.notify();
                return;
            }
            self.filas
                .entry(chat_id.to_string())
                .or_default()
                .extend(mensagens);
            if self.preparando.contains(&t.id) || self.preparos_a_retomar.contains(&t.id) || self.na_fila_pesada(t.id) {
                self.avisar(
                    chat_id,
                    "A cópia deste ajudante ainda está sendo preparada; a mensagem entra quando ela \
                     ficar pronta.",
                    false,
                );
            } else {
                self.preparar_copia(t.id, None, false, cx);
            }
            self.salvar();
            cx.notify();
            return;
        }
        if self.copia_apagada(&chat) {
            self.avisar(chat_id, "A cópia deste chat já foi apagada; ele não roda mais.", true);
            cx.notify();
            return;
        }
        // A pasta sumiu (cópia de tarefa já juntada, projeto apagado): o Claude Code nem abriria.
        if !chat.cwd.is_dir() {
            self.avisar(
                chat_id,
                &format!(
                    "A pasta deste chat não existe mais ({}); a mensagem não foi enviada.",
                    mestre::com_til(&chat.cwd)
                ),
                true,
            );
            self.salvar();
            cx.notify();
            return;
        }
        // Ultracode só com o que ele precisa: sem isso, o Claude Code o ignora calado.
        let pediu_ultracode = mensagens.iter().any(|e| e.ultracode);
        let sem_ultracode = pediu_ultracode.then(|| self.motivo_sem_ultracode(&chat)).flatten();
        let ultracode = pediu_ultracode && sem_ultracode.is_none();
        // Um Ultracode por vez no Vitral inteiro: a fonte do PC não segura dois fluxos de agentes.
        let outro_ultracode = ultracode && self.ultracode_rodando().is_some_and(|c| c != chat_id);
        // Sem vaga, espera com as mensagens guardadas. O turno que o usuário pediu tem uma vaga a
        // mais e passa na frente da fila.
        // "Iniciar agente" numa issue (um só, não o lote) conta como turno do usuário para a vaga.
        let do_usuario = mensagens.iter().any(Entrada::do_usuario) || self.cache_issues.vez_do_usuario.contains(chat_id);
        let ativos = self.trabalhadores_ativos();
        if chat.papel.conta_vaga()
            && (!pode_comecar(chat.papel, do_usuario, ativos, self.config.max_trabalhadores) || outro_ultracode)
        {
            self.filas
                .entry(chat_id.to_string())
                .or_default()
                .extend(mensagens);
            if !self.esperando_vaga(chat_id) {
                if do_usuario {
                    self.espera.push_front(chat_id.to_string());
                } else {
                    self.espera.push_back(chat_id.to_string());
                }
                if outro_ultracode {
                    self.avisar(chat_id, "O Ultracode espera o da outra conversa terminar.", false);
                }
            }
            cx.notify();
            return;
        }
        self.cache_issues.vez_do_usuario.remove(chat_id);
        if sem_ultracode.is_some() {
            for entrada in &mut mensagens {
                entrada.ultracode = false;
            }
        }

        // A sessão gravada expirou (o Claude Code apaga as antigas): uma nova, com as últimas
        // mensagens no começo, em vez de um `--resume` que falharia.
        let pasta_conta = self.conta_para(&chat).pasta();
        let expirou = chat.sessao_criada && claude::sessao_expirou(pasta_conta.as_deref(), &chat.cwd, &chat.sessao);
        if expirou {
            self.passagens.insert(chat_id.to_string(), MotivoPassagem::Expirou);
            chat.sessao = uuid::Uuid::new_v4().to_string();
            chat.sessao_criada = false;
            if let Some(c) = self.chat_mut(chat_id) {
                (c.sessao, c.sessao_criada, c.contexto) = (chat.sessao.clone(), false, None);
            }
            self.avisar(chat_id, "A sessão antiga do Claude Code expirou; continuei numa nova com um resumo.", false);
        }
        let passagem = self.passagens.remove(chat_id).map(|motivo| {
            if chat.papel == Papel::Mestre {
                return self.bloco_de_passagem_do_mestre(motivo);
            }
            let ja_na_conversa = mensagens.iter().filter(|e| e.registrada).count();
            let tarefa = chat.tarefa.and_then(|t| self.tarefa(t));
            let itens = self.chat(chat_id).map_or(&[][..], |c| c.itens.as_slice());
            bloco_de_passagem(itens, tarefa, ja_na_conversa)
        });

        // A marca de isolado é avaliada com o lote que vai entrar: o turno que traz texto de terceiros
        // (issue anexada, aviso de PR) já roda isolado, e daí em diante o chat não sai mais.
        if matches!(chat.papel, Papel::Projeto | Papel::Trabalhador) && !chat.isolado && issues::marca_isolado(&mensagens) {
            chat.isolado = true;
            if let Some(c) = self.chat_mut(chat_id) {
                c.isolado = true;
            }
        }
        let (texto, imagens) = texto_do_turno(&mensagens);
        let texto = match passagem {
            Some(bloco) => format!("{bloco}\n\n---\n\n{texto}"),
            None => texto,
        };
        let ultrathink = mensagens.iter().any(|e| e.ultrathink);
        let turno = turno_do_lote(self.ultima_entrada.get(chat_id).map(|(t, _)| t.as_str()), &mensagens);
        self.ultima_entrada.insert(chat_id.to_string(), (turno, mensagens.clone()));
        // Ponto de volta: foto dos arquivos antes de quem edita começar o turno, no grupo da tarefa
        // (o ramo da cópia pode ter qualquer nome). Sem git, não há foto.
        let novas = mensagens.iter().any(|e| !e.registrada);
        let grupo = tarefa.as_ref().map(Tarefa::grupo);
        let ponto = match (&tarefa, chat.papel) {
            _ if !novas => None,
            (_, Papel::Trabalhador) if git::e_repositorio(&chat.cwd) => git::foto(&chat.cwd, grupo.as_deref()).ok(),
            (Some(t), Papel::Projeto) if t.tem_copia() => git::foto(&chat.cwd, grupo.as_deref()).ok(),
            (Some(t), Papel::Projeto) if t.forma == Forma::DiretaGit && git::e_repositorio(&chat.cwd) => {
                git::foto(&chat.cwd, grupo.as_deref()).ok()
            }
            _ => None,
        };
        {
            let c = self.chat_mut(chat_id).unwrap();
            c.itens.extend(itens_das_entradas(mensagens, ponto));
            c.atualizado_em = agora();
        }
        if let Some(motivo) = sem_ultracode {
            self.avisar(chat_id, &format!("Esta mensagem vai sem o Ultracode: {motivo}."), false);
        }

        let Some(servidor) = &self.servidor else {
            self.avisar(
                chat_id,
                "O servidor interno do Vitral não abriu; não dá para rodar conversas.",
                true,
            );
            cx.notify();
            return;
        };
        let conta = self.conta_para(&chat);
        let mut opcoes = self.opcoes(&chat, servidor.url(chat.papel, &chat.id, self.roda_isolado(&chat)));
        opcoes.pasta_conta = conta.pasta();
        // No `-p` a palavra só injeta o lembrete; o esforço do turno sobe aqui, como no terminal.
        if ultrathink {
            opcoes.esforco = Some(esforco_do_ultrathink(opcoes.esforco.as_deref()).into());
        }
        // Vence o esforço acima: o Ultracode roda sempre em Extra.
        opcoes.ultracode = ultracode;
        // O hook de verificação rodaria já no primeiro `result`, com os agentes do fluxo ainda
        // mexendo nos arquivos (e o trabalhador iria consertar junto com eles). A verificação do
        // Vitral, quando o turno termina de verdade, continua.
        if ultracode {
            opcoes.configuracoes = None;
        }
        // A sessão foi criada, mas a marca não chegou ao disco (queda no primeiro turno).
        if !opcoes.retomar
            && claude::sessao_existe(opcoes.pasta_conta.as_deref(), &chat.cwd, &chat.sessao)
        {
            opcoes.retomar = true;
        }
        self.conta_do_turno
            .insert(chat_id.to_string(), conta.id.clone());
        let sinais = self.sinais.clone();
        let id = chat_id.to_string();
        self.geracao += 1;
        let geracao = self.geracao;
        let mensagem = Mensagem {
            texto: &texto,
            imagens: &imagens,
        };
        match Processo::iniciar(&opcoes, mensagem, move |evento| {
            let _ = sinais.send_blocking(Sinal::Claude(id.clone(), geracao, evento));
        }) {
            Ok(processo) => {
                self.processos.insert(chat_id.to_string(), processo);
                self.geracao_atual.insert(chat_id.to_string(), geracao);
                self.fundo.remove(chat_id);
                self.fim_adiado.remove(chat_id);
                self.execucao
                    .insert(chat_id.to_string(), Execucao::Rodando { desde: agora() });
                if let Some(c) = self.chat_mut(chat_id) {
                    c.turno_aberto = true;
                }
                if let Some(t) = chat.tarefa.and_then(|t| self.tarefa_mut(t))
                    && t.estado.ativa()
                {
                    t.parada = None;
                    if t.progresso.as_deref() == Some(TENTANDO) {
                        t.progresso = None;
                    }
                }
            }
            Err(erro) => {
                let mensagem = format!("Não consegui abrir o Claude Code: {erro:#}");
                if let Some(c) = self.chat_mut(chat_id) {
                    c.turno_aberto = false;
                }
                if !self.tentar_sozinho(chat_id, &mensagem, 10, cx) {
                    self.avisar(chat_id, &mensagem, true);
                    self.execucao
                        .insert(chat_id.to_string(), Execucao::Erro(mensagem.clone()));
                    self.desistir(chat_id, &mensagem, cx);
                }
            }
        }
        if let Some(c) = self.chat(chat_id) {
            let _ = self.dados.salvar_chat(c);
        }
        self.salvar();
        cx.notify();
    }

    fn opcoes(&self, chat: &Chat, url_mcp: String) -> Opcoes {
        let notas = self.dados.notas(&chat.projeto);
        let tarefa = chat.tarefa.and_then(|t| self.tarefa(t));
        // A conversa do Vitral antigo não roda (a guarda do turno a recusa antes).
        let (mut prompt_sistema, mut bloqueadas, pastas_extras, modelo_padrao) = match chat.papel {
            Papel::Mestre => self.opcoes_do_mestre(),
            Papel::Trabalhador => self.opcoes_do_ajudante(chat, tarefa, &notas),
            Papel::Projeto | Papel::Coordenador => self.opcoes_de_projeto(chat, tarefa, &notas),
        };
        // Chat isolado (seção 3.6): o claude no bwrap (ou só com o ambiente, sem ele) e, como
        // reforço, os redutores (o `bash -c` passa por eles); o prompt diz por quê.
        let isolado = self.modo_isolado(chat);
        if isolado.is_some() {
            bloqueadas.extend(crate::isolamento::BLOQUEADAS.map(String::from));
            let motivo = self.motivo_do_isolamento(chat);
            prompt_sistema.push_str(prompts::nota_de_isolamento(motivo, chat.papel == Papel::Trabalhador));
        }
        if let Some(pasta) = pastas_extras.first() {
            let _ = std::fs::create_dir_all(pasta);
        }
        // Trabalhador: portas próprias (servidores de teste não brigam entre cópias) e, se a
        // tarefa tem verificação, um hook que roda os testes quando ele para.
        // O `git status` dos agentes não segura o index.lock (derrubaria uma junção do Vitral).
        let mut ambiente = vec![("GIT_OPTIONAL_LOCKS".to_string(), "0".to_string())];
        let mut configuracoes = None;
        // Uma memória só, a do Vitral (o `mestre/memoria.md`).
        if chat.papel == Papel::Mestre {
            ambiente.push(("CLAUDE_CODE_DISABLE_AUTO_MEMORY".into(), "1".into()));
        }
        if chat.papel.conta_vaga()
            && let Some(tarefa) = tarefa
        {
            ambiente.push(("VITRAL_PORT".into(), porta_de(tarefa).to_string()));
            // Cada cópia criaria uma memória própria no Claude Code: a do projeto são as notas e as
            // instruções.
            ambiente.push(("CLAUDE_CODE_DISABLE_AUTO_MEMORY".into(), "1".into()));
            // O hook só com um pedido aberto: na conversa direta com o usuário, nunca.
            let pedido = chat.papel == Papel::Trabalhador || tarefa.estado.ativa();
            if let Some(verificacao) = tarefa.verificacao.as_ref().filter(|_| pedido) {
                ambiente.push(("VITRAL_VERIFICACAO".into(), verificacao.clone()));
                configuracoes = crate::revisor::configuracao_hook_verificacao();
            }
        }
        // Por arquivo, o prompt novo vale já no turno de uma sessão retomada; num Claude Code mais
        // velho (ou antes de saber a versão), vai na linha de comando.
        let prompt_arquivo = self.versao_claude.filter(|v| *v >= claude::VERSAO_PROMPT_POR_ARQUIVO).and_then(|_| {
            let arquivo = self.dados.arquivo_prompt(&chat.id);
            let gravou = arquivo.parent().is_some_and(|p| std::fs::create_dir_all(p).is_ok())
                && std::fs::write(&arquivo, &prompt_sistema).is_ok();
            gravou.then_some(arquivo)
        });
        Opcoes {
            cwd: chat.cwd.clone(),
            sessao: chat.sessao.clone(),
            retomar: chat.sessao_criada,
            bifurcar: crate::importar::bifurcar_de(chat.importada.as_ref(), chat.sessao_criada),
            modelo: chat.modelo.clone().or(Some(modelo_padrao)),
            permissao: self.permissao_de(chat),
            prompt_sistema,
            prompt_arquivo,
            ferramentas: (chat.papel == Papel::Mestre).then(|| mestre::FERRAMENTAS_DO_MESTRE.to_string()),
            url_mcp,
            bloqueadas,
            pastas_extras,
            // O MCP do usuário pode ter centenas de ferramentas: fora por padrão (no Haiku, que não
            // adia a carga delas, o pedido estoura o contexto).
            so_mcp_vitral: matches!(chat.papel, Papel::Coordenador | Papel::Mestre) || !self.config.mcp_trabalhadores,
            esforco: chat.esforco.clone(),
            ultracode: false,
            pasta_conta: None,
            ambiente,
            configuracoes,
            isolado,
        }
    }

    /// O prompt, as regras de bloqueio, as outras pastas (só para leitura) e o modelo padrão de um
    /// chat de projeto ou avulso.
    fn opcoes_de_projeto(&self, chat: &Chat, tarefa: Option<&Tarefa>, notas: &str) -> (String, Vec<String>, Vec<PathBuf>, String) {
        let projeto = self.projeto(&chat.projeto);
        let pastas: Vec<PathBuf> = self.pastas_do(&chat.projeto).iter().map(|l| l.caminho.clone()).collect();
        let origem = tarefa.map(|t| self.origem_da_tarefa(t)).filter(|o| !o.as_os_str().is_empty()).unwrap_or_else(|| chat.cwd.clone());
        let forma = tarefa.map_or(Forma::Direta, |t| t.forma);
        let avulso = forma == Forma::Direta && chat.cwd.starts_with(self.dados.pasta_avulso(""));
        // No modo PR, o prompt fala do PR (o repositório e a base), e não do Juntar.
        let destino_pr = tarefa.filter(|t| self.entrega_por_pr(t)).map(|t| {
            let config = self.config_da_pasta(&t.projeto, &origem).github;
            let repositorio = self.destino_do_pr(&origem, &config).unwrap_or_else(|| "o GitHub".into());
            let base = self.remoto_base(t).and_then(|p| p.split_once('/').map(|(_, b)| b.to_string())).unwrap_or_else(|| "a base".into());
            (repositorio, base)
        });
        let onde = match (forma, &destino_pr) {
            (Forma::Copia, Some((repositorio, base))) => prompts::Onde::CopiaPr { cwd: &chat.cwd, origem: &origem, repositorio, base },
            _ => match forma {
            Forma::Copia => prompts::Onde::Copia { cwd: &chat.cwd, origem: &origem },
            Forma::DiretaGit => prompts::Onde::DiretaGit { origem: &origem },
            Forma::Direta if chat.importada.is_some() => prompts::Onde::Continuada { cwd: &chat.cwd },
            Forma::Direta if avulso => prompts::Onde::Avulso { cwd: &chat.cwd },
            Forma::Direta => prompts::Onde::Direta { origem: &origem },
            },
        };
        let outras: Vec<PathBuf> = pastas
            .iter()
            .filter(|p| (avulso || **p != origem) && p.is_dir())
            .cloned()
            .collect();
        let info = projeto.map(|p| prompts::ProjetoInfo {
            nome: &p.nome,
            descricao: &p.descricao,
            instrucoes: &p.instrucoes,
            outras: outras.clone(),
        });
        // Ajudantes só com git: cada um trabalha numa cópia da pasta deste chat.
        let ajudantes = (chat.papel == Papel::Projeto && forma != Forma::Direta && self.config.max_ajudantes > 0)
            .then_some(self.config.max_ajudantes);
        let prompt = prompts::chat_de_projeto(info.as_ref(), &onde, notas, ajudantes);
        let mut bloqueadas = regras_de_bloqueio(forma, &origem, &chat.cwd, &pastas, None);
        bloqueadas.extend(SEM_TROCAR_DE_PASTA.map(String::from));
        let modelo = projeto.and_then(|p| p.modelo.clone()).unwrap_or_else(|| self.config.modelo_chats.clone());
        (prompt, bloqueadas, outras, modelo)
    }

    /// O prompt, as regras de bloqueio, as outras pastas e o modelo padrão de um ajudante: ele mexe
    /// só na cópia dele; a pasta do pai, a origem do pai e as pastas do projeto ficam só para leitura.
    fn opcoes_do_ajudante(&self, chat: &Chat, tarefa: Option<&Tarefa>, notas: &str) -> (String, Vec<String>, Vec<PathBuf>, String) {
        let pasta_do_pai = tarefa.map(|t| self.origem_da_tarefa(t)).unwrap_or_default();
        let origem_do_pai = chat
            .pai
            .as_deref()
            .and_then(|p| self.chat(p))
            .and_then(|p| p.tarefa)
            .and_then(|n| self.tarefa(n))
            .map(|t| self.origem_da_tarefa(t))
            .unwrap_or_default();
        let pastas: Vec<PathBuf> = self.pastas_do(&chat.projeto).iter().map(|l| l.caminho.clone()).collect();
        let mut bloqueadas =
            regras_de_bloqueio(Forma::Copia, &pasta_do_pai, &chat.cwd, &pastas, Some((&pasta_do_pai, &origem_do_pai)));
        bloqueadas.extend(SEM_TROCAR_DE_PASTA.map(String::from));
        let outras: Vec<PathBuf> =
            pastas.iter().filter(|p| **p != origem_do_pai && **p != pasta_do_pai && p.is_dir()).cloned().collect();
        let dono = chat.pai.as_deref().map_or_else(|| "o chat que criou você".to_string(), |p| self.rotulo_do_pai(p));
        let prompt = prompts::trabalhador(&self.nome_do_projeto(&chat.projeto), &pasta_do_pai, &chat.cwd, notas, &dono);
        let modelo = self
            .projeto(&chat.projeto)
            .and_then(|p| p.modelo.clone())
            .unwrap_or_else(|| self.config.modelo_chats.clone());
        (prompt, bloqueadas, outras, modelo)
    }

    /// Como o ajudante (e o prompt dele) chama o pai: o chat 12 "Checkout com Pix" do projeto Ninee.
    fn rotulo_do_pai(&self, pai: &str) -> String {
        match self.chat(pai) {
            Some(c) => format!(
                "o chat {} \"{}\" do projeto {}",
                mestre::numero_do_chat(c),
                c.titulo,
                self.nome_do_projeto(&c.projeto)
            ),
            None => "o chat que criou você".into(),
        }
    }

    /// Os ajudantes do chat que ainda não terminaram (nem aprovados, nem cancelados, nem falhos).
    pub fn ajudantes_abertos(&self, chat: &str) -> Vec<u64> {
        self.tarefas
            .iter()
            .filter(|t| t.coordenador == chat && !chat.is_empty() && !t.estado.encerrada())
            .filter(|t| self.chat(&t.trabalhador).is_some_and(|c| c.papel == Papel::Trabalhador))
            .map(|t| t.id)
            .collect()
    }

    /// Por que o Ultracode não vale nesta conversa agora (`None`: vale). O de outra conversa
    /// rodando não entra: esse só faz esperar a vez.
    pub fn motivo_sem_ultracode(&self, chat: &Chat) -> Option<String> {
        let padrao = match chat.papel {
            Papel::Coordenador | Papel::Mestre => &self.config.modelo_mestre,
            Papel::Trabalhador | Papel::Projeto => &self.config.modelo_chats,
        };
        let escolhido = chat.modelo.as_deref().unwrap_or(padrao);
        let modelo = if escolhido.is_empty() { chat.modelo_atual.as_deref().unwrap_or("") } else { escolhido };
        motivo_sem_ultracode(chat.papel, modelo, self.versao_claude)
    }

    /// Um turno com Ultracode nesta conta mostrou os fluxos de agentes desligados.
    pub fn fluxos_desligados(&self, chat: &Chat) -> bool {
        self.fluxos_por_conta.get(&self.conta_para(chat).id) == Some(&false)
    }

    /// O init mostrou que o Ultracode não ligou: as mensagens dele perdem o selo e o turno deixa
    /// de contar como Ultracode (o de outra conversa não espera por ele).
    fn ultracode_nao_ligou(&mut self, chat_id: &str) {
        let Some((_, mensagens)) = self.ultima_entrada.get_mut(chat_id) else {
            return;
        };
        let mut faltam = mensagens.iter().filter(|e| e.ultracode).count();
        for entrada in mensagens.iter_mut() {
            entrada.ultracode = false;
        }
        if let Some(c) = self.chat_mut(chat_id) {
            for item in c.itens.iter_mut().rev() {
                if faltam == 0 {
                    break;
                }
                if let Item::Usuario { ultracode, .. } = item
                    && *ultracode
                {
                    *ultracode = false;
                    faltam -= 1;
                }
            }
        }
        self.chats_sujos.insert(chat_id.to_string());
    }

    /// O trabalhador espera na fila o Ultracode de outra conversa terminar.
    fn espera_outro_ultracode(&self, chat_id: &str) -> bool {
        self.filas.get(chat_id).is_some_and(|f| f.iter().any(|e| e.ultracode))
            && self.ultracode_rodando().is_some_and(|c| c != chat_id)
            && self.chat(chat_id).is_some_and(|c| self.motivo_sem_ultracode(c).is_none())
    }

    /// O turno em andamento (ou o último) do chat foi pedido com Ultracode.
    fn turno_com_ultracode(&self, chat: &str) -> bool {
        self.ultima_entrada
            .get(chat)
            .is_some_and(|(_, m)| m.iter().any(|e| e.ultracode))
    }

    /// A conversa que roda um turno com Ultracode agora.
    pub fn ultracode_rodando(&self) -> Option<&str> {
        self.processos
            .keys()
            .find(|id| self.turno_com_ultracode(id))
            .map(String::as_str)
    }

    /// O turno já deu a resposta, mas os agentes dele seguem trabalhando em segundo plano.
    pub fn esperando_agentes(&self, chat: &str) -> bool {
        self.fim_adiado.contains_key(chat)
    }

    /// O Mestre roda sempre sem perguntar: nada do que ele tem escreve arquivo ou roda comando.
    pub fn permissao_de(&self, chat: &Chat) -> String {
        if chat.papel == Papel::Mestre {
            return "bypassPermissions".into();
        }
        chat.permissao
            .clone()
            .unwrap_or_else(|| self.config.permissao.clone())
    }

    // ---------- contas ----------

    /// Escolha de conta que vale para o chat: a dele, senão a padrão da pasta.
    pub fn escolha_de_conta(&self, chat: &Chat) -> String {
        chat.conta.clone().unwrap_or_else(|| {
            if self.config.conta.is_empty() {
                contas::PRINCIPAL.to_string()
            } else {
                self.config.conta.clone()
            }
        })
    }

    /// A conta que roda o próximo turno do chat. Na automática, a que tem mais folga no plano.
    fn conta_para(&self, chat: &Chat) -> Conta {
        self.conta_da_escolha(&self.escolha_de_conta(chat))
    }

    /// A conta que um chat sem escolha própria usaria agora (a que o trilho mostra).
    pub fn conta_padrao(&self) -> Conta {
        let escolha = if self.config.conta.is_empty() {
            contas::PRINCIPAL
        } else {
            &self.config.conta
        };
        self.conta_da_escolha(escolha)
    }

    fn conta_da_escolha(&self, escolha: &str) -> Conta {
        if escolha != contas::AUTOMATICA
            && let Some(conta) = self.contas.iter().find(|c| c.id == escolha)
        {
            return conta.clone();
        }
        let folga = |c: &&Conta| {
            let uso = self.uso_da_conta(&c.id);
            // Com todas no limite, vai a que volta primeiro.
            let volta = if uso.bloqueado {
                self.limite_ate.get(&c.id).copied().unwrap_or(u64::MAX)
            } else {
                0
            };
            (uso.bloqueado, volta, (uso.cinco_horas.unwrap_or(0.0) * 1000.0) as i64)
        };
        self.contas
            .iter()
            .min_by_key(folga)
            .cloned()
            .unwrap_or_else(|| self.contas[0].clone())
    }

    /// Depois de um erro de limite na conta do turno, repete o turno em outra conta se a escolha
    /// do chat for automática e houver conta com folga.
    fn trocar_de_conta(&mut self, chat: &str, erro: &str, cx: &mut Context<Self>) -> bool {
        let Some(c) = self.chat_mut(chat).map(Chat::copia_sem_itens) else {
            return false;
        };
        let Some(atual) = self.conta_do_turno.get(chat).cloned() else {
            return false;
        };
        let limite = self.uso_da_conta(&atual).bloqueado || erro.to_lowercase().contains("limit");
        if !limite || self.escolha_de_conta(&c) != contas::AUTOMATICA {
            return false;
        }
        let mut uso = self.uso_da_conta(&atual);
        uso.bloqueado = true;
        self.uso_por_conta.insert(atual.clone(), uso);
        let fim = fim_do_limite(None, self.limite_ate.get(&atual).copied(), agora());
        self.limite_ate.insert(atual.clone(), fim);
        let proxima = self.conta_para(&c);
        if proxima.id == atual || self.uso_da_conta(&proxima.id).bloqueado {
            if self.contas.len() > 1
                && let Some(volta) = self.contas.iter().filter_map(|x| self.limite_ate.get(&x.id)).min()
            {
                let aviso = format!(
                    "Todas as contas chegaram ao limite do plano; a primeira volta {}.",
                    crate::modelo::quando_local(*volta)
                );
                self.avisar(chat, &aviso, false);
            }
            return false;
        }
        self.execucao.insert(chat.to_string(), Execucao::Parado);
        let nome = self
            .contas
            .iter()
            .find(|x| x.id == atual)
            .map(|x| x.nome.clone())
            .unwrap_or(atual);
        self.avisar(
            chat,
            &format!(
                "A conta {nome} chegou ao limite do plano; continuando na conta {}.",
                proxima.nome
            ),
            false,
        );
        if let Some(mensagens) = self.entrada_para_repetir(chat) {
            self.comecar_turno_com(chat, mensagens, false, cx);
        }
        true
    }

    /// Erro de sessão logo no começo do turno: ela existe e o Vitral achava que não (queda logo
    /// depois de criá-la), ou sumiu. Acerta a marca e repete o turno uma vez. Diz se repetiu.
    fn consertar_sessao(&mut self, chat_id: &str, erro: &str, cx: &mut Context<Self>) -> bool {
        let Some(chat) = self.chat(chat_id) else {
            return false;
        };
        let da_sessao = erro.contains(&chat.sessao);
        let existe = if da_sessao && erro.contains("is already in use") {
            true
        } else if da_sessao && erro.contains("No conversation found with session ID") {
            false
        } else {
            return false;
        };
        let Some((_, mensagens)) = self.ultima_entrada.get(chat_id).cloned() else {
            return false;
        };
        if chat.sessao_criada == existe || !self.repetidos_sessao.insert(chat_id.to_string()) {
            return false;
        }
        if let Some(c) = self.chat_mut(chat_id) {
            c.sessao_criada = existe;
            if !existe {
                c.sessao = uuid::Uuid::new_v4().to_string();
                c.contexto = None;
            }
        }
        if !existe {
            self.passagens.insert(chat_id.to_string(), MotivoPassagem::Expirou);
        }
        let aviso = if existe {
            "Retomando a sessão anterior."
        } else {
            "A sessão antiga do Claude Code sumiu; continuei numa nova com um resumo."
        };
        self.avisar(chat_id, aviso, !existe);
        self.execucao.insert(chat_id.to_string(), Execucao::Parado);
        self.salvar();
        self.comecar_turno_com(chat_id, mensagens, false, cx);
        true
    }

    /// Guarda até quando a conta do turno fica de fora quando o plano recusa pedidos.
    fn marcar_limite(&mut self, chat_id: &str, bloqueado: bool, ate: Option<u64>) {
        let conta = self
            .conta_do_turno
            .get(chat_id)
            .cloned()
            .unwrap_or_else(|| contas::PRINCIPAL.to_string());
        if bloqueado {
            let fim = fim_do_limite(ate, self.limite_ate.get(&conta).copied(), agora());
            self.limite_ate.insert(conta, fim);
        } else {
            self.limite_ate.remove(&conta);
        }
    }

    pub fn uso_da_conta(&self, conta: &str) -> Uso {
        let mut uso = self.uso_por_conta.get(conta).copied().unwrap_or_default();
        // A conta no limite não recebe turno, então nenhum evento viria tirar a marca: ela vence
        // sozinha na hora em que o plano renova.
        if uso.bloqueado && self.limite_ate.get(conta).is_none_or(|&fim| fim <= agora()) {
            uso.bloqueado = false;
        }
        uso
    }

    /// Quando a conta no limite volta a receber turnos (None se ela não está no limite).
    pub fn limite_vence(&self, conta: &str) -> Option<u64> {
        self.uso_da_conta(conta).bloqueado.then(|| self.limite_ate.get(conta).copied()).flatten()
    }

    pub fn plano_da_conta(&self, conta: &str) -> Option<&Plano> {
        self.plano_por_conta.get(conta)
    }

    /// Relê fora da thread da interface o uso guardado de cada conta e o do Codex; só
    /// redesenha se algo mudou.
    pub fn reler_uso(&self, cx: &mut Context<Self>) {
        let todas = self.contas.clone();
        cx.spawn(async move |motor, cx| {
            let (usos, codex) = cx
                .background_spawn(async move { (contas::usos_guardados(&todas), codex::ler()) })
                .await;
            let _ = motor.update(cx, |m, cx| {
                if m.guardar_usos(usos, codex) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Junta o uso lido do `.claude.json` de cada conta ao que já se sabia (zerando as janelas
    /// que renovaram), guarda os detalhes do plano e o Codex, e diz se algo mudou.
    fn guardar_usos(&mut self, lidos: Vec<(String, Uso, Plano)>, codex: Option<Codex>) -> bool {
        let agora = agora() as i64;
        let mut mudou = self.codex != codex || !self.codex_lido;
        self.codex = codex;
        self.codex_lido = true;
        let ids: Vec<String> = self.contas.iter().map(|c| c.id.clone()).collect();
        for id in ids {
            let (lido, plano) = lidos
                .iter()
                .find(|(conta, ..)| *conta == id)
                .map(|(_, uso, plano)| (*uso, plano.clone()))
                .unwrap_or_default();
            mudou |= self.juntar_uso_da_conta(&id, lido, plano, agora) != Mudanca::Nada;
        }
        mudou
    }

    /// Junta uma leitura do uso de uma conta (do arquivo ou da busca) ao que já se sabia e diz o
    /// que mudou.
    fn juntar_uso_da_conta(&mut self, id: &str, lido: Uso, plano: Plano, agora: i64) -> Mudanca {
        let antes = self.uso_por_conta.get(id).copied().unwrap_or_default();
        let depois = antes.juntar(lido, agora);
        let velho = self.plano_por_conta.get(id);
        let novo = velho.cloned().unwrap_or_default().juntar(plano);
        let mudou = Mudanca::entre((antes, depois), (velho, &novo));
        self.uso_por_conta.insert(id.to_string(), depois);
        self.plano_por_conta.insert(id.to_string(), novo);
        mudou
    }

    /// O popover do uso do Claude abriu: busca agora e, enquanto `vista` viver, de minuto em
    /// minuto.
    pub fn uso_em_vista(&mut self, vista: AnyWeakEntity, cx: &mut Context<Self>) {
        self.uso_em_vista = Some(vista);
        self.buscar_uso(false, cx);
    }

    fn vendo_uso(&self) -> bool {
        self.uso_em_vista.as_ref().is_some_and(AnyWeakEntity::is_upgradable)
    }

    /// "Atualizar" do Claude: relê o cache e busca na Anthropic agora.
    pub fn atualizar_uso(&mut self, cx: &mut Context<Self>) {
        self.reler_uso(cx);
        self.buscar_uso(true, cx);
    }

    /// A última busca do uso da conta não trouxe número e nada mais novo chegou depois.
    pub fn uso_sem_atualizar(&self, conta: &str) -> bool {
        self.agenda_uso.sem_atualizar(conta, self.uso_da_conta(conta).medido_em)
    }

    /// A rodada automática, no ritmo da `Agenda` (popover aberto, janela sem foco).
    fn rotina_uso(&mut self, cx: &mut Context<Self>) {
        let agora = agora();
        let fora_ha = (!self.janela_ativa).then(|| agora.saturating_sub(self.visto_em));
        if self.agenda_uso.na_hora(agora, self.vendo_uso(), fora_ha) {
            self.rodada_uso(false, cx);
        }
    }

    /// Uma rodada pedida (popover aberto, "Atualizar"), se a última não acabou de sair.
    fn buscar_uso(&mut self, a_pedido: bool, cx: &mut Context<Self>) {
        if self.agenda_uso.pode_agora(agora()) {
            self.rodada_uso(a_pedido, cx);
        }
    }

    /// Busca o uso de cada conta fora do recuo, uma depois da outra, fora da thread da
    /// interface; só redesenha se algo mudou.
    fn rodada_uso(&mut self, a_pedido: bool, cx: &mut Context<Self>) {
        let da_vez = self.agenda_uso.comecar(&self.contas, agora(), a_pedido);
        cx.spawn(async move |motor, cx| {
            let buscas = cx
                .background_spawn(async move {
                    da_vez
                        .into_iter()
                        .map(|(conta, recusado)| {
                            let busca = busca::buscar(&conta, recusado);
                            (conta.id, busca)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = motor.update(cx, |m, cx| {
                if m.guardar_buscas(buscas) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Junta o que a rodada trouxe e diz se é para redesenhar: números novos sempre; a hora da
    /// medição e o "não deu para atualizar" só com o popover aberto (o trilho não mostra).
    fn guardar_buscas(&mut self, buscas: Vec<(String, Busca)>) -> bool {
        self.agenda_uso.terminar();
        let agora = agora();
        let mut mudou = Mudanca::Nada;
        for (id, busca) in buscas {
            if !self.contas.iter().any(|c| c.id == id) {
                continue;
            }
            let antes = self.uso_sem_atualizar(&id);
            self.agenda_uso.registrar(&id, &busca, agora);
            if let Busca::Lida(uso, plano) = busca {
                mudou = mudou.max(self.juntar_uso_da_conta(&id, uso, plano, agora as i64));
            }
            if self.uso_sem_atualizar(&id) != antes {
                mudou = mudou.max(Mudanca::Hora);
            }
        }
        mudou == Mudanca::Numeros || (mudou == Mudanca::Hora && self.vendo_uso())
    }

    pub fn adicionar_conta(&mut self, conta: Conta, cx: &mut Context<Self>) {
        self.contas.retain(|c| c.id != conta.id);
        self.contas.push(conta);
        let _ = contas::salvar(&self.contas);
        cx.notify();
    }

    pub fn remover_conta(&mut self, id: &str, cx: &mut Context<Self>) {
        if id == contas::PRINCIPAL {
            return;
        }
        self.contas.retain(|c| c.id != id);
        let _ = contas::salvar(&self.contas);
        contas::apagar_pasta(id);
        for chat in &mut self.chats {
            if chat.conta.as_deref() == Some(id) {
                chat.conta = None;
            }
        }
        if self.config.conta == id {
            self.config.conta.clear();
            self.gravar_config();
        }
        self.salvar();
        cx.notify();
    }

    /// Muda uma opção da conversa (modelo, modo, esforço, conta); vale a partir do próximo turno.
    pub fn mudar_chat(&mut self, chat: &str, f: impl FnOnce(&mut Chat), cx: &mut Context<Self>) {
        let antes = self.chat(chat).map(|c| self.permissao_de(c));
        let padrao = self.config.permissao.clone();
        if let Some(c) = self.chat_mut(chat) {
            f(c);
            // Quem entra em Planejar guarda o modo de antes, para onde aprovar o plano volta.
            if c.permissao.as_deref().unwrap_or(&padrao) != "plan" {
                c.modo_antes_do_plano = None;
            } else if let Some(antes) = antes.filter(|m| m != "plan") {
                c.modo_antes_do_plano = Some(antes);
            }
        }
        self.salvar();
        cx.notify();
    }

    /// `/clear`: a conversa continua na tela, mas o Claude começa uma sessão nova, sem memória.
    pub fn nova_sessao(&mut self, chat: &str, cx: &mut Context<Self>) {
        if self.processos.contains_key(chat) {
            return;
        }
        if let Some(c) = self.chat_mut(chat) {
            c.sessao = uuid::Uuid::new_v4().to_string();
            c.sessao_criada = false;
            c.contexto = None;
            if let Some(i) = c.importada.as_mut() {
                i.marcar_continuada();
            }
        }
        self.avisar(
            chat,
            "Sessão nova: o Claude não lembra das mensagens acima.",
            false,
        );
        self.salvar();
        cx.notify();
    }

    fn trabalhadores_ativos(&self) -> usize {
        self.processos
            .keys()
            .filter(|id| self.chat(id).is_some_and(|c| c.papel.conta_vaga()))
            .count()
    }

    /// Trata um sinal e diz se a janela precisa redesenhar: o pedaço de texto de uma conversa
    /// que nenhum painel mostra só conta quando é o primeiro (a lista e a Equipe não leem o texto
    /// parcial, mas "escrevendo" muda o tamanho da conversa).
    fn tratar(&mut self, sinal: Sinal, cx: &mut Context<Self>) -> bool {
        self.encerrando.retain_mut(|p| p.esperar().is_none());
        match sinal {
            // Eventos atrasados de um processo anterior (ex.: a saída dele depois que o turno
            // seguinte já começou) não podem mexer no processo novo.
            Sinal::Claude(chat, geracao, evento) => {
                if self.geracao_atual.get(&chat) == Some(&geracao) {
                    if let Evento::TextoParcial(pedaco) = &evento {
                        return juntar_parcial(&mut self.parcial, &self.visiveis, &chat, pedaco);
                    }
                    self.tratar_evento(&chat, evento, cx)
                }
            }
            Sinal::Mcp(chamada) => self.tratar_ferramenta(chamada, cx),
            Sinal::Abrir(chat) => {
                if self.chat(&chat).is_none() {
                    return false;
                }
                self.pedido_de_abrir = Some(chat);
            }
            #[cfg(windows)]
            Sinal::Aviso(aviso) => {
                cx.show_system_notification(aviso);
                return false;
            }
            Sinal::Integracao => {
                self.atender_pedidos(cx);
                self.atender_pedidos_de_issues(cx);
                self.atender_pedidos_de_prs(cx);
            }
        }
        true
    }

    fn tratar_evento(&mut self, chat_id: &str, evento: Evento, cx: &mut Context<Self>) {
        match evento {
            Evento::Inicio {
                modelo,
                comandos,
                versao,
                fluxos,
            } => {
                if versao.is_some() {
                    self.versao_claude = versao;
                }
                // Os fluxos só contam como desligados num turno que os pediu (no plano Pro eles
                // começam desligados e só o Ultracode os liga).
                let ultracode = self.turno_com_ultracode(chat_id);
                if let (Some(conta), Some(tem)) = (self.conta_do_turno.get(chat_id).cloned(), fluxos)
                    && (tem || ultracode)
                {
                    self.fluxos_por_conta.insert(conta, tem);
                }
                // A versão só se sabe aqui quando a leitura ao abrir falhou. Antiga, o Ultracode
                // pode ter ligado sem o limite de agentes (antes da 2.1.269) ou nem ligado: o turno
                // continua contando como um, e a opção fica desabilitada daqui em diante.
                if ultracode && let Some(antiga) = versao.filter(|v| *v < VERSAO_ULTRACODE) {
                    self.avisar(
                        chat_id,
                        &format!(
                            "Este Claude Code ({}) é anterior ao {}, em que o Vitral conferiu o Ultracode: \
                             ele pode rodar sem o limite de agentes ao mesmo tempo, ou nem ligar. Atualize o \
                             Claude Code; Parar encerra o turno.",
                            texto_da_versao(antiga),
                            texto_da_versao(VERSAO_ULTRACODE)
                        ),
                        true,
                    );
                }
                let antiga = versao.is_some_and(|v| v < VERSAO_ULTRACODE);
                let nao_ligou = if !ultracode || antiga {
                    None
                } else if fluxos == Some(false) {
                    Some(format!(
                        "O Claude Code não ligou o Ultracode: {FLUXOS_DESLIGADOS}. O turno segue em Extra, \
                         sem eles."
                    ))
                } else if !modelo.is_empty() && !aceita_ultracode(&modelo) {
                    Some(format!("O Claude Code não ligou o Ultracode: o modelo {modelo} não tem o esforço Extra."))
                } else {
                    None
                };
                // O histórico não diz que a mensagem foi com ele, e o turno não conta como um.
                if let Some(aviso) = nao_ligou {
                    self.avisar(chat_id, &aviso, true);
                    self.ultracode_nao_ligou(chat_id);
                }
                let mut nova = false;
                if let Some(c) = self.chat_mut(chat_id) {
                    nova = !c.sessao_criada;
                    c.sessao_criada = true;
                    if !modelo.is_empty() {
                        c.modelo_atual = Some(modelo);
                    }
                    if !comandos.is_empty() {
                        c.comandos = comandos;
                    }
                }
                // Gravado na hora: com a marca perdida numa queda, o próximo turno tentaria criar
                // de novo a sessão, e o Claude Code recusa.
                if nova {
                    self.conversa_continuada(chat_id);
                    self.salvar();
                }
            }
            Evento::Contexto(tokens) => {
                if let Some(c) = self.chat_mut(chat_id) {
                    c.contexto = Some(tokens);
                }
            }
            Evento::Fundo(n) => {
                self.fundo.insert(chat_id.to_string(), n);
            }
            Evento::TextoParcial(pedaco) => {
                juntar_parcial(&mut self.parcial, &self.visiveis, chat_id, &pedaco);
            }
            Evento::Texto(texto) => {
                self.parcial.remove(chat_id);
                self.chats_sujos.insert(chat_id.to_string());
                if let Some(c) = self.chat_mut(chat_id)
                    && !texto.trim().is_empty()
                {
                    c.itens.push(Item::Assistente {
                        texto: texto.trim().to_string(),
                    });
                }
            }
            Evento::Ferramenta { id, nome, entrada } => {
                self.parcial.remove(chat_id);
                // Busca de ferramentas e pedidos de permissão são bastidores: não aparecem.
                if nome == "mcp__vitral__permissao" || nome == "ToolSearch" {
                    return;
                }
                let resumo = claude::resumo_ferramenta(&nome, &entrada);
                self.chats_sujos.insert(chat_id.to_string());
                if let Some(c) = self.chat_mut(chat_id) {
                    c.itens.push(Item::Ferramenta {
                        id,
                        nome,
                        resumo,
                        resultado: None,
                        erro: false,
                    });
                }
            }
            Evento::ResultadoFerramenta { id, conteudo, erro } => {
                self.chats_sujos.insert(chat_id.to_string());
                if let Some(c) = self.chat_mut(chat_id) {
                    for item in c.itens.iter_mut().rev() {
                        if let Item::Ferramenta {
                            id: id_item,
                            resultado,
                            erro: erro_item,
                            ..
                        } = item
                            && *id_item == id
                        {
                            *resultado = Some(git::inicio_do_texto(&conteudo, 20_000));
                            *erro_item = erro;
                            break;
                        }
                    }
                }
            }
            Evento::Limite {
                uso_5h,
                uso_semana,
                renova_5h,
                renova_semana,
                bloqueado,
                bloqueado_ate,
            } => {
                self.marcar_limite(chat_id, bloqueado, bloqueado_ate);
                let conta = self
                    .conta_do_turno
                    .get(chat_id)
                    .cloned()
                    .unwrap_or_else(|| contas::PRINCIPAL.to_string());
                let medido = Uso {
                    cinco_horas: uso_5h,
                    semana: uso_semana,
                    bloqueado,
                    renova_5h,
                    renova_semana,
                    medido_em: Some(agora() as i64),
                };
                let junto = self.uso_da_conta(&conta).juntar(medido, agora() as i64);
                self.uso_por_conta.insert(conta, Uso { bloqueado, ..junto });
            }
            Evento::Fim {
                erro,
                texto,
                custo_usd,
                janela,
            } => {
                let interrompido = self.interrompidos.iter().any(|c| c == chat_id);
                if adiar_o_fim(self.fundo.get(chat_id).copied().unwrap_or(0), erro, interrompido)
                    && let Some(processo) = self.processos.get_mut(chat_id)
                {
                    // O fluxo (ou subagente) segue em segundo plano e o Claude Code abre outro
                    // turno quando ele acaba. Com a entrada fechada, ele espera os agentes e sai;
                    // a tarefa só vai para a revisão no último `result`.
                    processo.fechar_entrada();
                    let primeiro = self
                        .fim_adiado
                        .insert(chat_id.to_string(), (texto, custo_usd))
                        .is_none();
                    if let Some(c) = self.chat_mut(chat_id)
                        && janela.is_some()
                    {
                        c.janela = janela;
                    }
                    if primeiro {
                        self.avisar(
                            chat_id,
                            "Os agentes seguem trabalhando em segundo plano; o turno termina quando eles acabarem.",
                            false,
                        );
                    }
                    return;
                }
                // O custo do `result` é o total da sessão até ali: vale o último.
                let custo_usd = match self.fim_adiado.remove(chat_id) {
                    Some((_, antes)) => custo_usd.max(antes),
                    None => custo_usd,
                };
                if let Some(mut processo) = self.processos.remove(chat_id) {
                    processo.fechar_entrada();
                    // Turno com erro ou parado e agentes ainda em segundo plano: com a entrada
                    // fechada, o Claude Code seguiria esperando por eles (sem teto no Ultracode),
                    // fora da tela, do Parar e da conta de um Ultracode por vez. Saem juntos.
                    if self.fundo.remove(chat_id).unwrap_or(0) > 0 {
                        processo.matar();
                    }
                    self.encerrando.push(processo);
                }
                if let Some(c) = self.chat_mut(chat_id) {
                    c.somar_custo(custo_usd);
                    c.turno_aberto = false;
                    c.atualizado_em = agora();
                    if janela.is_some() {
                        c.janela = janela;
                    }
                }
                // Parado pelo usuário: o Claude Code fecha o turno com erro, mas não é falha, e a
                // tarefa não vai para revisão (o usuário pode continuar a conversa).
                if self.interrompidos.iter().any(|c| c == chat_id) {
                    self.interrompidos.retain(|c| c != chat_id);
                    self.parcial.remove(chat_id);
                    self.avisar(chat_id, "Interrompido.", false);
                    self.execucao.insert(chat_id.to_string(), Execucao::Parado);
                    self.marcar_parada(chat_id, PARADA_VOCE);
                    self.depois_da_vez(chat_id, cx);
                    if let Some(c) = self.chat(chat_id) {
                        let _ = self.dados.salvar_chat(c);
                    }
                    self.liberar_vaga(cx);
                    self.salvar();
                    return;
                }
                if erro && self.consertar_sessao(chat_id, &texto, cx) {
                    self.parcial.remove(chat_id);
                    return;
                }
                // Limite do plano numa conta, com outra disponível na escolha automática: o
                // mesmo turno roda de novo na outra conta.
                if erro && self.trocar_de_conta(chat_id, &texto, cx) {
                    return;
                }
                // Erro passageiro da API, que o Claude Code já tentou refazer: mais uma vez daqui
                // a um minuto, antes de gastar uma rodada do coordenador com ele.
                if erro
                    && erro_passageiro(&texto)
                    && self.tentar_sozinho(
                        chat_id,
                        &format!("O turno terminou com erro da API ({})", texto.trim()),
                        60,
                        cx,
                    )
                {
                    self.parcial.remove(chat_id);
                    if let Some(c) = self.chat(chat_id) {
                        let _ = self.dados.salvar_chat(c);
                    }
                    self.liberar_vaga(cx);
                    self.salvar();
                    return;
                }
                if erro {
                    self.avisar(
                        chat_id,
                        &format!("O turno terminou com erro: {texto}"),
                        true,
                    );
                    self.execucao
                        .insert(chat_id.to_string(), Execucao::Erro(texto.clone()));
                    // A nova tentativa automática também falhou: a tarefa volta ao coordenador
                    // (em fim_de_turno), como em desistir.
                    if self.tentativas_auto.contains(chat_id)
                        && !self.janela_ativa
                        && let Some(c) = self.chat(chat_id)
                        && c.papel == Papel::Trabalhador
                        && c.tarefa.and_then(|t| self.tarefa(t)).is_some_and(|t| t.estado.ativa())
                    {
                        self.notificar(c, "Parou com erro: o ajudante caiu e o pedido voltou ao chat dele");
                    }
                } else {
                    self.execucao.insert(chat_id.to_string(), Execucao::Parado);
                }
                self.fim_de_turno(chat_id, erro, texto, cx);
            }
            Evento::Saiu { stderr, codigo } => {
                // Saiu depois de um `result` adiado, sem outro: os agentes acabaram sem outro turno
                // (o fim normal, com o texto daquele `result`) ou foram cortados junto com o
                // processo (teto da espera, queda), e aí o turno termina com erro.
                if !self.interrompidos.iter().any(|c| c == chat_id)
                    && self.processos.contains_key(chat_id)
                    && let Some((texto, custo_usd)) = self.fim_adiado.remove(chat_id)
                {
                    self.processos.remove(chat_id);
                    let restantes = self.fundo.remove(chat_id).unwrap_or(0);
                    if let Some(c) = self.chat_mut(chat_id) {
                        c.somar_custo(custo_usd);
                        c.turno_aberto = false;
                        c.atualizado_em = agora();
                    }
                    match agentes_cortados(restantes, codigo, &stderr) {
                        Some(motivo) => {
                            let aviso = format!("Os agentes em segundo plano não terminaram: {motivo}.");
                            self.avisar(chat_id, &aviso, true);
                            self.execucao
                                .insert(chat_id.to_string(), Execucao::Erro(aviso.clone()));
                            self.fim_de_turno(chat_id, true, format!("{texto}\n\n{aviso}"), cx);
                        }
                        None => {
                            self.execucao.insert(chat_id.to_string(), Execucao::Parado);
                            self.fim_de_turno(chat_id, false, texto, cx);
                        }
                    }
                    return;
                }
                // Parado esperando os agentes: o custo do `result` adiado também conta.
                if let Some((_, custo_usd)) = self.fim_adiado.remove(chat_id)
                    && let Some(c) = self.chat_mut(chat_id)
                {
                    c.somar_custo(custo_usd);
                }
                // Sem `Fim`: o processo caiu ou foi interrompido no meio do turno.
                if self.processos.remove(chat_id).is_some() {
                    self.parcial.remove(chat_id);
                    let interrompido = self.interrompidos.contains(&chat_id.to_string());
                    self.interrompidos.retain(|c| c != chat_id);
                    if let Some(c) = self.chat_mut(chat_id) {
                        c.turno_aberto = false;
                    }
                    self.depois_da_vez(chat_id, cx);
                    if interrompido {
                        self.avisar(chat_id, "Interrompido.", false);
                        self.execucao.insert(chat_id.to_string(), Execucao::Parado);
                        self.marcar_parada(chat_id, PARADA_VOCE);
                    } else if self.consertar_sessao(chat_id, &stderr, cx) {
                        // Repetido com a sessão certa.
                    } else {
                        let detalhe = if stderr.trim().is_empty() {
                            "o processo do Claude Code fechou sem resposta".to_string()
                        } else {
                            stderr
                        };
                        let motivo = format!("O processo do Claude Code caiu ({})", detalhe.trim());
                        if !self.tentar_sozinho(chat_id, &motivo, 10, cx) {
                            self.avisar(chat_id, &format!("Falhou: {detalhe}"), true);
                            self.execucao
                                .insert(chat_id.to_string(), Execucao::Erro(detalhe.clone()));
                            self.desistir(chat_id, &detalhe, cx);
                        }
                    }
                    if let Some(c) = self.chat(chat_id) {
                        let _ = self.dados.salvar_chat(c);
                    }
                    self.liberar_vaga(cx);
                    self.salvar();
                }
            }
        }
    }

    fn fim_de_turno(&mut self, chat_id: &str, erro: bool, texto: String, cx: &mut Context<Self>) {
        self.depois_da_vez(chat_id, cx);
        self.parcial.remove(chat_id);
        if !erro {
            self.tentativas_auto.remove(chat_id);
            self.repetidos_sessao.remove(chat_id);
        }
        if let Some(c) = self.chat(chat_id) {
            let _ = self.dados.salvar_chat(c);
        }
        let Some(chat) = self.chat_mut(chat_id).map(Chat::copia_sem_itens) else {
            return;
        };

        if chat.papel == Papel::Projeto
            && let Some(t) = chat.tarefa.and_then(|n| self.tarefa(n)).cloned()
        {
            self.fim_do_pedido(&chat, &t, erro, &texto, cx);
        }
        if chat.papel == Papel::Mestre {
            self.conferir_contexto_do_mestre();
        }
        if chat.papel == Papel::Trabalhador
            && let Some(id) = chat.tarefa
            && self.tarefa(id).is_some_and(|t| t.estado.ativa())
        {
            // Parou para perguntar algo, ou já tem resposta/ajuste esperando: ainda não é a
            // entrega; o relatório sai no fim do próximo turno.
            let continua = self.filas.get(chat_id).is_some_and(|f| !f.is_empty());
            if (self.aguardando_resposta.contains(&chat.id) || continua) && !erro {
                if let Some(t) = self.tarefa_mut(id) {
                    t.progresso = Some("esperando resposta do chat dele".into());
                }
            } else {
                self.tarefa_terminou(id, erro, texto.clone(), cx);
            }
        }

        // Mensagens que chegaram durante o turno viram o próximo (as notas esperam uma de verdade).
        if self.na_fila(chat_id) > 0
            && let Some(fila) = self.filas.remove(chat_id)
        {
            self.comecar_turno(chat_id, fila, cx);
        } else if chat.papel == Papel::Projeto && !self.janela_ativa && !erro {
            self.notificar(&chat, &texto_do_fim(&texto));
        }
        // O PR mesclou com o chat rodando: a cópia recomeça agora que ele parou.
        if chat.papel == Papel::Projeto {
            self.recomeco_pendente(chat_id, cx);
        }
        self.liberar_vaga(cx);
        self.salvar();
    }

    fn liberar_vaga(&mut self, cx: &mut Context<Self>) {
        // Uma volta na fila só. Quem ainda não cabe (ou o Ultracode que espera o de outra conversa)
        // fica no lugar dele, sem passar de novo por comecar_turno (que o avisaria mais uma vez). O
        // turno do usuário cabe numa vaga a mais.
        let mut ficam = Vec::new();
        for _ in 0..self.espera.len() {
            let Some(proximo) = self.espera.pop_front() else {
                break;
            };
            let papel = self.chat(&proximo).map_or(Papel::Trabalhador, |c| c.papel);
            let do_usuario = self.filas.get(&proximo).is_some_and(|f| f.iter().any(Entrada::do_usuario));
            let cabe = pode_comecar(papel, do_usuario, self.trabalhadores_ativos(), self.config.max_trabalhadores);
            if !cabe || self.espera_outro_ultracode(&proximo) {
                ficam.push(proximo);
                continue;
            }
            let fila = self.filas.remove(&proximo).unwrap_or_default();
            if !fila.is_empty() {
                self.comecar_turno(&proximo, fila, cx);
            }
        }
        for chat in ficam.into_iter().rev() {
            self.espera.push_front(chat);
        }
        // Os que esperavam outro chat sair da mesma pasta.
        for chat in std::mem::take(&mut self.esperando_pasta) {
            let livre = self
                .chat(&chat)
                .zip(self.chat(&chat).and_then(|c| c.tarefa).and_then(|t| self.tarefa(t)))
                .is_some_and(|(c, t)| self.escritor_na_pasta(c, t).is_none());
            if livre {
                self.entregar_fila(&chat, cx);
            } else if self.chat(&chat).is_some() {
                self.esperando_pasta.push(chat);
            }
        }
    }

    /// A mensagem espera na fila, com o aviso de por quê uma vez só (na primeira que espera).
    fn esperar_na_fila(&mut self, chat: &str, mensagens: Vec<Entrada>, aviso: &str) {
        let ja_esperava = self.na_fila(chat) > 0;
        // O "Entregar agora" de uma fila que ainda espera não repete o mesmo aviso.
        let repetido = self
            .chat(chat)
            .and_then(|c| c.itens.last())
            .is_some_and(|i| matches!(i, Item::Aviso { texto, .. } if texto == aviso));
        self.filas.entry(chat.to_string()).or_default().extend(mensagens);
        if !ja_esperava && !repetido {
            self.avisar(chat, aviso, false);
        }
    }

    /// Outro chat direto (sem cópia) com processo vivo na mesma pasta, numa de dentro ou numa de
    /// fora: devolve a pasta dele. A cópia nunca espera (cada uma é só de um chat).
    fn escritor_na_pasta(&self, chat: &Chat, t: &Tarefa) -> Option<PathBuf> {
        if t.tem_copia() {
            return None;
        }
        let ativos: Vec<PathBuf> = self
            .processos
            .keys()
            .filter(|id| **id != chat.id)
            .filter_map(|id| self.chat(id))
            .filter(|c| c.papel.conta_vaga())
            .filter(|c| c.tarefa.and_then(|n| self.tarefa(n)).is_some_and(|t| !t.tem_copia()))
            .map(|c| c.cwd.clone())
            .collect();
        escritor_ocupado(&chat.cwd, &ativos)
    }

    /// Antes do turno de uma cópia sem trabalho próprio: ela anda até o que entrou na origem (o que
    /// outro chat juntou), na vez do repositório. A mensagem espera na fila e entra em seguida.
    fn sincronizar_antes_do_turno(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let chat = t.trabalhador.clone();
        if !self.sincronizando.insert(id) {
            return;
        }
        let (copia, origem) = (t.worktree.clone().unwrap_or_default(), self.origem_da_tarefa(&t));
        cx.spawn(async move |motor, cx| {
            cx.background_spawn(async move {
                // Sem a origem (disco desmontado), o turno roda na cópia como ela está.
                if origem.is_dir() && copia.is_dir() {
                    let _ = git::na_vez_do_projeto(&origem, || git::sincronizar_copia(&copia, &origem));
                }
            })
            .await;
            let _ = motor.update(cx, |m, cx| {
                m.sincronizando.remove(&id);
                m.sincronizados.insert(chat.clone());
                m.entregar_fila(&chat, cx);
                if !m.processos.contains_key(&chat) {
                    m.sincronizados.remove(&chat);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// O trabalhador terminou o turno: a entrega fica gravada (se o Vitral cair, ela é refeita ao
    /// abrir) e vai para a fila do trabalho pesado, onde a verificação e o revisor a conferem antes
    /// de o relatório ir ao dono.
    fn tarefa_terminou(&mut self, id: u64, erro: bool, relatorio: String, cx: &mut Context<Self>) {
        let Some(tarefa) = self.tarefa_mut(id) else {
            return;
        };
        tarefa.estado = EstadoTarefa::EmRevisao;
        tarefa.relatorio = Some(relatorio);
        tarefa.progresso = Some(NA_FILA_CONFERIR.into());
        tarefa.parada = None;
        tarefa.entrega_pendente = Some(erro);
        self.salvar();
        self.enfileirar_pesado(Pesado::Conferir(id), false, cx);
    }

    /// Roda a verificação e o revisor da entrega e manda o relatório ao dono. Diz se começou.
    fn conferir_entrega(&mut self, id: u64, cx: &mut Context<Self>) -> bool {
        let retomada = self.entregas_a_retomar.iter().position(|t| *t == id).map(|i| self.entregas_a_retomar.remove(i));
        let Some(t) = self.tarefa_mut(id) else {
            return false;
        };
        let (EstadoTarefa::EmRevisao, Some(erro)) = (t.estado, t.entrega_pendente) else {
            return false;
        };
        let relatorio = t.relatorio.clone().unwrap_or_default();
        t.progresso = Some(VERIFICANDO.into());
        // A refeita ao abrir fica marcada enquanto roda: se cair de novo, espera o Retomar.
        if retomada.is_some() {
            t.parada = Some(PARADA_VERIFICACAO.into());
        }
        let tarefa = t.clone();
        self.verificando.insert(id, tarefa.rodada);
        let projeto = self.origem_da_tarefa(&tarefa);
        // Onde está o que o pedido mudou: a cópia, ou a pasta própria (da base do pedido até o HEAD,
        // com os commits de cada vez); sem git, não há diff.
        let pasta_diff = match tarefa.forma {
            Forma::DiretaGit if self.de_projeto(&tarefa.trabalhador) => Some(projeto.clone()).filter(|p| p.is_dir()),
            _ => tarefa.worktree.clone(),
        };
        let direta = pasta_diff.as_ref() != tarefa.worktree.as_ref();
        // O revisor usa a mesma conta do trabalhador e o modelo configurado.
        let revisar = self.config.revisor && !erro && pasta_diff.is_some();
        let modelo_revisor = Some(self.config.modelo_revisor.clone())
            .filter(|m| !m.is_empty())
            .or_else(|| self.chat(&tarefa.trabalhador).and_then(|c| c.modelo.clone()))
            .or_else(|| Some(self.config.modelo_chats.clone()).filter(|m| !m.is_empty()));
        let pasta_conta = self
            .chat(&tarefa.trabalhador)
            .map(|c| self.conta_para(c))
            .and_then(|c| c.pasta());
        // Arquivos que as outras cópias abertas do mesmo repositório estão mexendo (as dos chats em
        // conversa também): o repositório é conferido em segundo plano. A do pai de um ajudante não
        // conta: a cópia dele partiu dela.
        let do_pai = (!self.de_projeto(&tarefa.trabalhador))
            .then(|| self.chat(&tarefa.coordenador).and_then(|c| c.tarefa))
            .flatten();
        let outras: Vec<(u64, PathBuf, String)> = self
            .tarefas
            .iter()
            .filter(|t| t.id != id && !t.estado.encerrada() && Some(t.id) != do_pai)
            .filter_map(|t| Some((t.id, t.worktree.clone()?, t.base.clone()?)))
            .collect();
        let encerrado = self.encerrado.clone();
        // Na cópia de um chat isolado, a verificação e o revisor rodam isolados como ele.
        let isolado = self.modo_isolado_da_tarefa(&tarefa);
        // No modo PR, a base é o ponto comum com `<remoto>/<base>`: o que entrou no GitHub não conta.
        let remoto_base = self.remoto_base(&tarefa);

        cx.spawn(async move |motor, cx| {
            let tarefa_bg = tarefa.clone();
            let (verificacao, estatistica, revisao, sobreposicao) = cx
                .background_spawn(async move {
                    let tarefa = tarefa_bg;
                    let parou = || encerrado.load(Ordering::SeqCst);
                    let onde = tarefa.worktree.clone().unwrap_or(projeto.clone());
                    let verificacao = tarefa
                        .verificacao
                        .clone()
                        .filter(|_| !parou())
                        .map(|c| git::verificar(&onde, &c, isolado));
                    // Depois de um `git merge` pedido num conflito, a base certa é o que veio do projeto.
                    let base = match (&pasta_diff, &tarefa.base) {
                        (Some(_), Some(b)) if direta => Some(b.clone()),
                        (Some(p), Some(b)) => Some(git::base_da_tarefa(&projeto, p, b, remoto_base.as_deref())),
                        _ => None,
                    };
                    let (estatistica, meus) = match (&pasta_diff, &base) {
                        (Some(p), Some(b)) => (
                            git::diff_stat(p, b).ok(),
                            git::arquivos_mudados(p, b),
                        ),
                        _ => (None, Vec::new()),
                    };
                    let mut sobreposicao = Vec::new();
                    let repositorio = git::raiz_comum(&projeto);
                    for (outra, pasta, base) in outras {
                        if repositorio.is_none() || git::raiz_comum(&pasta) != repositorio {
                            continue;
                        }
                        let base = git::base_efetiva(&projeto, &pasta, &base);
                        let comuns: Vec<String> = git::arquivos_mudados(&pasta, &base)
                            .into_iter()
                            .filter(|a| meus.contains(a))
                            .collect();
                        if !comuns.is_empty() {
                            sobreposicao.push(format!("#{outra}: {}", comuns.join(", ")));
                        }
                    }
                    // Com o Vitral fechando ou trocando de pasta, o revisor nem abre: o resultado
                    // seria jogado fora, e a entrega é refeita ao abrir a pasta de novo.
                    let revisao = match (&pasta_diff, &base) {
                        (Some(p), Some(b)) if revisar && !meus.is_empty() && !parou() => Some(
                            crate::revisor::revisar(p, b, &tarefa, modelo_revisor.as_deref(), pasta_conta, isolado)
                                .map_err(|e| format!("{e:#}")),
                        ),
                        _ => None,
                    };
                    (verificacao, estatistica, revisao, sobreposicao)
                })
                .await;
            let _ = motor.update(cx, |motor, cx| {
                // A entrega pendente fica gravada como estava e é refeita ao abrir de novo.
                if motor.encerrado.load(Ordering::SeqCst) {
                    return;
                }
                let rodada = tarefa.rodada;
                let (revisao, falha_revisor) = match revisao {
                    Some(Ok(r)) => (Some(r), None),
                    Some(Err(erro)) => (None, Some(erro)),
                    None => (None, None),
                };
                if motor.verificando.get(&id) == Some(&rodada) {
                    motor.verificando.remove(&id);
                }
                // Se a tarefa foi cancelada ou ganhou outra rodada no meio, este parecer e este
                // relatório não valem mais (e o progresso da rodada nova fica como está).
                let mut vale = false;
                if let Some(t) = motor.tarefa_mut(id)
                    && t.rodada == rodada
                {
                    if t.estado == EstadoTarefa::EmRevisao {
                        t.ultima_verificacao = verificacao;
                        t.revisao = revisao;
                        t.sobreposicao = sobreposicao;
                        t.progresso = None;
                        t.parada = None;
                        vale = t.entrega_pendente.take().is_some();
                    } else {
                        t.entrega_pendente = None;
                    }
                }
                // A conferência sai da fila antes: a junção sozinha confere que nada pesado roda na tarefa.
                motor.salvar();
                motor.pesado_terminou(Pesado::Conferir(id), cx);
                if vale && let Some(dono) = tarefa.dono() {
                    // Ao Mestre vai o relatório compacto (ou nada, quando junta sozinho); ao pai do
                    // ajudante, o dele.
                    if dono == crate::modelo::ID_MESTRE && motor.de_projeto(&tarefa.trabalhador) {
                        motor.entrega_ao_mestre(id, relatorio.clone(), estatistica.clone(), erro, cx);
                    } else if let Some(t) = motor.tarefa(id) {
                        let texto = mestre::relatorio_ao_pai(t, &relatorio, estatistica.as_deref(), erro, falha_revisor.as_deref());
                        motor.entregar(dono, Some("Vitral".into()), texto, cx);
                    }
                } else if vale && motor.tarefa(id).is_some_and(|t| t.pr_aberto().is_some()) {
                    // A rodada de um chat com PR aberto: publica sozinha (Ship) ou espera "Atualizar PR".
                    motor.entrega_do_pr_conferida(id, cx);
                } else if vale && tarefa.pedido_da_issue {
                    // O pedido de uma issue é do usuário: "Pronto para revisar ENG-123".
                    motor.pronta_para_revisar(id);
                }
                motor.salvar();
                cx.notify();
            });
        })
        .detach();
        true
    }


    /// Põe o trabalho na fila do pesado (o que o usuário pediu vai na frente) e anda com ela.
    fn enfileirar_pesado(&mut self, trabalho: Pesado, frente: bool, cx: &mut Context<Self>) {
        if self.pesado_atual == Some(trabalho) {
            return;
        }
        if let Some(i) = self.pesado.iter().position(|p| *p == trabalho) {
            if !frente {
                return;
            }
            self.pesado.remove(i);
        }
        if frente {
            self.pesado.push_front(trabalho);
        } else {
            self.pesado.push_back(trabalho);
        }
        self.andar_pesado(cx);
    }

    /// Começa o próximo trabalho pesado, se nenhum roda. O que não tem mais o que fazer sai da fila.
    fn andar_pesado(&mut self, cx: &mut Context<Self>) {
        while self.pesado_atual.is_none() && !self.encerrado.load(Ordering::SeqCst) {
            let Some(trabalho) = self.pesado.pop_front() else {
                return;
            };
            let comecou = match trabalho {
                Pesado::Preparar(id) => self.rodar_preparo(id, cx),
                Pesado::Conferir(id) => self.conferir_entrega(id, cx),
                Pesado::Parecer(id) => self.rodar_parecer(id, cx),
            };
            if comecou {
                self.pesado_atual = Some(trabalho);
            }
        }
        cx.notify();
    }

    /// O trabalho pesado que rodava terminou: o próximo da fila começa.
    fn pesado_terminou(&mut self, trabalho: Pesado, cx: &mut Context<Self>) {
        if self.pesado_atual == Some(trabalho) {
            self.pesado_atual = None;
        }
        self.andar_pesado(cx);
    }

    /// Ao abrir: as entregas cortadas na verificação e as preparações cortadas voltam para a fila
    /// do trabalho pesado, uma de cada vez. A marca de parada entra quando cada uma começa: se cair
    /// de novo, espera o Retomar.
    fn retomar_entregas(&mut self, cx: &mut Context<Self>) {
        let entregas: Vec<u64> = self.entregas_a_retomar.iter().copied().collect();
        let preparos: Vec<u64> = self.preparos_a_retomar.iter().copied().collect();
        for id in entregas {
            self.enfileirar_pesado(Pesado::Conferir(id), false, cx);
        }
        for id in preparos {
            self.enfileirar_pesado(Pesado::Preparar(id), false, cx);
        }
        self.salvar();
    }

    /// "Pedir parecer do revisor" (o usuário, num chat de projeto): o revisor confere o que a cópia
    /// tem para juntar, pela fila do trabalho pesado, sem mudar o chat.
    pub fn pedir_parecer(&mut self, chat: &str, cx: &mut Context<Self>) {
        if let Some(id) = self.chat(chat).filter(|c| c.papel == Papel::Projeto).and_then(|c| c.tarefa) {
            self.enfileirar_pesado(Pesado::Parecer(id), true, cx);
        }
    }

    /// O parecer de um chat de projeto: a verificação do pedido (se houver) e o revisor sobre o que a
    /// cópia mudou desde a base. Sem pedido aberto, o revisor confere as mensagens do usuário desde a
    /// última junção. Diz se começou.
    fn rodar_parecer(&mut self, id: u64, cx: &mut Context<Self>) -> bool {
        let responder = move |m: &mut Self, texto: String, erro: bool| {
            if let Some(r) = m.respostas_parecer.remove(&id) {
                let _ = r.try_send(if erro { Resposta::erro(texto) } else { Resposta::ok(texto) });
            }
        };
        let Some(t) = self.tarefa(id).cloned() else {
            responder(self, "Este chat não tem mais tarefa.".into(), true);
            return false;
        };
        let origem = self.origem_da_tarefa(&t);
        // Na pasta própria, o diff é o da base até o HEAD (os commits de cada vez).
        let pasta = match t.forma {
            Forma::Copia => t.worktree.clone(),
            Forma::DiretaGit => Some(origem.clone()),
            Forma::Direta => None,
        };
        let (Some(pasta), Some(base)) = (pasta, t.base.clone()) else {
            responder(self, "Ainda não há o que o revisor conferir: este chat não tem cópia com mudanças (ou mexe numa pasta sem git).".into(), true);
            return false;
        };
        let com_pedido = t.estado.ativa() || t.estado == EstadoTarefa::EmRevisao;
        let mut conferida = t.clone();
        if !com_pedido {
            let mensagens = self.chat(&t.trabalhador).map(|c| mensagens_desde_a_juncao(&c.itens)).unwrap_or_default();
            conferida.objetivo = format!("Mensagens do usuário desde a última junção:\n{mensagens}");
            conferida.criterios = vec![
                "Faz o que o usuário pediu nessas mensagens".into(),
                "Não quebra o que já funcionava".into(),
            ];
            conferida.verificacao = None;
        }
        let modelo = Some(self.config.modelo_revisor.clone())
            .filter(|m| !m.is_empty())
            .or_else(|| Some(self.config.modelo_chats.clone()).filter(|m| !m.is_empty()));
        let pasta_conta = self.chat(&t.trabalhador).map(|c| self.conta_para(c)).and_then(|c| c.pasta());
        let encerrado = self.encerrado.clone();
        let isolado = self.modo_isolado_da_tarefa(&t);
        if let Some(t) = self.tarefa_mut(id) {
            t.progresso = Some("o revisor está conferindo as mudanças".into());
        }
        cx.spawn(async move |motor, cx| {
            let (verificacao, revisao) = cx
                .background_spawn(async move {
                    let base = if origem.is_dir() { git::base_efetiva(&origem, &pasta, &base) } else { base };
                    let verificacao = conferida.verificacao.clone().map(|c| git::verificar(&pasta, &c, isolado));
                    let revisao = if git::arquivos_mudados(&pasta, &base).is_empty() {
                        Err("nada mudou na cópia desde a última junção".to_string())
                    } else if encerrado.load(Ordering::SeqCst) {
                        Err("o Vitral fechou".to_string())
                    } else {
                        crate::revisor::revisar(&pasta, &base, &conferida, modelo.as_deref(), pasta_conta, isolado)
                            .map_err(|e| format!("{e:#}"))
                    };
                    (verificacao, revisao)
                })
                .await;
            let _ = motor.update(cx, |m, cx| {
                let mut texto = String::new();
                if let Some(v) = &verificacao {
                    texto.push_str(&format!(
                        "Verificação `{}`: {}\n```\n{}\n```\n",
                        v.comando,
                        if v.passou { "PASSOU" } else { "FALHOU" },
                        git::final_do_texto(v.saida.trim(), 3000)
                    ));
                }
                let erro = revisao.is_err();
                match &revisao {
                    Ok(r) => texto.push_str(&crate::revisor::texto(r)),
                    Err(e) => texto.push_str(&format!("O revisor não conferiu: {e}.")),
                }
                let chat = m.tarefa(id).map(|t| t.trabalhador.clone());
                if let Some(t) = m.tarefa_mut(id) {
                    t.progresso = None;
                    if t.estado == EstadoTarefa::Conversa {
                        if let Ok(r) = &revisao {
                            t.revisao = Some(r.clone());
                        }
                        if verificacao.is_some() {
                            t.ultima_verificacao = verificacao.clone();
                        }
                    }
                }
                if !m.respostas_parecer.contains_key(&id)
                    && let (Some(chat), Ok(r)) = (chat, &revisao)
                {
                    let veredito = if r.veredito == "aprovar" { "aprovaria" } else { "pede ajustes" };
                    m.avisar(&chat, &format!("Parecer do revisor: {veredito}. {}", r.resumo.trim()), false);
                }
                responder(m, texto, erro);
                m.salvar();
                m.pesado_terminou(Pesado::Parecer(id), cx);
                cx.notify();
            });
        })
        .detach();
        true
    }

    // ---------- ferramentas MCP ----------

    fn tratar_ferramenta(&mut self, chamada: Chamada, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_mut(&chamada.chat).map(Chat::copia_sem_itens) else {
            let _ = chamada
                .resposta
                .try_send(Resposta::erro("Chat desconhecido para o Vitral."));
            return;
        };
        // A rota diz o papel com que o processo abriu; a tabela, o que esse papel pode chamar.
        if chat.papel != chamada.papel {
            let _ = chamada.resposta.try_send(Resposta::erro("o papel deste chat mudou; encerre a vez"));
            return;
        }
        if !mcp::pode(chat.papel, &chamada.ferramenta, self.roda_isolado(&chat)) {
            let erro = format!("Ferramenta \"{}\" não existe para este chat.", chamada.ferramenta);
            let _ = chamada.resposta.try_send(Resposta::erro(erro));
            return;
        }
        let chamada = match self.ferramenta_de_issue(&chat, chamada, cx) {
            Ok(()) => {
                self.salvar();
                cx.notify();
                return;
            }
            Err(chamada) => chamada,
        };
        let chamada = match self.ferramenta_nova(&chat, chamada, cx) {
            Ok(()) => {
                self.salvar();
                cx.notify();
                return;
            }
            Err(chamada) => chamada,
        };
        let a = &chamada.argumentos;
        let texto = |chave: &str| a[chave].as_str().unwrap_or_default().trim().to_string();
        let numero = |chave: &str| {
            a[chave]
                .as_u64()
                .or_else(|| a[chave].as_str()?.trim_start_matches('#').parse().ok())
        };

        let resposta = match chamada.ferramenta.as_str() {
            "permissao" => {
                // "Permitir sempre" desta conversa: vale já no turno em andamento.
                if chat.permite(a["tool_name"].as_str().unwrap_or("?"), &a["input"]) {
                    let resposta = json!({"behavior": "allow", "updatedInput": a["input"]});
                    let _ = chamada.resposta.try_send(Resposta::ok(resposta.to_string()));
                    return;
                }
                self.permissoes.push(PedidoPermissao {
                    id: novo_id(),
                    chat: chat.id.clone(),
                    ferramenta: a["tool_name"].as_str().unwrap_or("?").to_string(),
                    entrada: a["input"].clone(),
                    resposta: chamada.resposta,
                });
                self.execucao
                    .insert(chat.id.clone(), Execucao::EsperandoVoce);
                let pedido = texto_do_pedido(a["tool_name"].as_str().unwrap_or("?"), &a["input"]);
                self.registrar("permissao", &chat.id, &chat.id, &chat.projeto, chat.tarefa, &pedido);
                // Como os outros avisos, só com a janela sem foco: com ela à frente, o pedido já
                // aparece no app (e se responde lá, nunca na notificação).
                if !self.janela_ativa {
                    self.notificar(&chat, &texto_do_pedido(a["tool_name"].as_str().unwrap_or("?"), &a["input"]));
                }
                return;
            }
            "perguntar_coordenador" => match (chat.pai.clone(), chat.tarefa) {
                (Some(pai), Some(id)) => self.pergunta_ao_pai(&chat, &pai, id, &texto("pergunta"), cx),
                _ => Resposta::erro("Este chat não tem quem pediu o trabalho."),
            },
            "informar_progresso" => {
                if let Some(id) = chat.tarefa
                    && let Some(t) = self.tarefa_mut(id)
                {
                    t.progresso = Some(texto("texto"));
                }
                Resposta::ok("ok")
            }
            "revisar_minhas_mudancas" if chat.papel == Papel::Projeto => {
                let Some(id) = chat.tarefa else {
                    let _ = chamada.resposta.try_send(Resposta::erro("Este chat não tem tarefa."));
                    return;
                };
                self.respostas_parecer.insert(id, chamada.resposta);
                self.enfileirar_pesado(Pesado::Parecer(id), false, cx);
                return;
            }
            "anotar_decisao" => match self.dados.anotar(&chat.projeto, &texto("texto")) {
                Ok(()) => Resposta::ok("Anotado. Vale já no próximo turno dos chats deste projeto."),
                Err(erro) => Resposta::erro(format!("Não anotou: {erro}")),
            },
            "criar_tarefa" if chat.papel == Papel::Projeto => {
                self.criar_ajudante(&chat, a.clone(), chamada.resposta, cx);
                return;
            }
            "ver_diff" | "pedir_ajuste" | "aprovar_tarefa" | "cancelar_tarefa" if chat.papel == Papel::Projeto => {
                let Some(id) = numero("tarefa") else {
                    let _ = chamada.resposta.try_send(Resposta::erro("Informe o número do ajudante."));
                    return;
                };
                // A regra do dono: só os ajudantes deste chat.
                let dono = match self.tarefa(id) {
                    Some(t) if self.chat(&t.trabalhador).is_some_and(|c| c.papel == Papel::Trabalhador) => mestre::pedido_de(t, &chat.id),
                    Some(_) => Err(format!("O chat {id} não é um ajudante seu.")),
                    None => Err(format!("Não há ajudante #{id}.")),
                };
                if let Err(recusa) = dono {
                    let _ = chamada.resposta.try_send(Resposta::erro(recusa));
                    return;
                }
                match chamada.ferramenta.as_str() {
                    "ver_diff" => {
                        self.ver_diff(id, lista_de_textos(&a["arquivos"]), chamada.resposta, cx);
                        return;
                    }
                    "aprovar_tarefa" => {
                        let forcar = a["forcar"].as_bool().unwrap_or(false)
                            || a["forcar"].as_str().is_some_and(|f| f.trim() == "true");
                        let portao = match (self.recusa_aprovar(id), self.tarefa(id)) {
                            (None, Some(t)) => portao_aprovacao(t, forcar, &texto("motivo")),
                            (recusa, _) => Err(recusa.unwrap_or_default()),
                        };
                        match portao {
                            Ok(ressalva) => self.aprovar_tarefa(id, texto("nota"), ressalva, chamada.resposta, &chat.id, None, cx),
                            Err(recusa) => {
                                let _ = chamada.resposta.try_send(Resposta::erro(recusa));
                            }
                        }
                        return;
                    }
                    "pedir_ajuste" => {
                        let incluir = lista_de(&a["incluir"]).map(|l| {
                            l.iter()
                                .filter_map(|v| v.as_str().map(|s| s.trim().to_lowercase()))
                                .collect::<Vec<_>>()
                        });
                        let mut ignorar: Vec<usize> = lista_de(&a["ignorar_problemas"])
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|v| {
                                v.as_u64()
                                    .or_else(|| v.as_str()?.trim().trim_start_matches('#').parse().ok())
                            })
                            .map(|n| n as usize)
                            .collect();
                        // Como ao Mestre, só vão os problemas altos e médios do revisor.
                        if let Some(r) = self.tarefa(id).and_then(|t| t.revisao.as_ref()) {
                            ignorar.extend(r.problemas.iter().enumerate().filter(|(_, p)| p.gravidade == "baixa").map(|(i, _)| i + 1));
                        }
                        let quem = format!("chat {} \"{}\"", mestre::numero_do_chat(&chat), chat.titulo);
                        self.pedir_ajuste_como(id, texto("feedback"), incluir, ignorar, &quem, cx)
                    }
                    _ => self.cancelar_tarefa(id, texto("motivo"), cx),
                }
            }
            outra => Resposta::erro(format!("Ferramenta \"{outra}\" não existe para este chat.")),
        };
        let _ = chamada.resposta.try_send(resposta);
        self.salvar();
    }

    /// `criar_tarefa` de um chat de projeto: um ajudante, num chat próprio, numa cópia da pasta do
    /// pai feita a partir do que ela tem agora (o ponto antes do ajudante é na preparação, na vez do
    /// repositório). O pai recebe a resposta quando a cópia fica pronta e o relatório no fim de cada
    /// rodada.
    fn criar_ajudante(&mut self, pai: &Chat, argumentos: Value, resposta: Sender<Resposta>, cx: &mut Context<Self>) {
        let texto = |chave: &str| argumentos[chave].as_str().unwrap_or_default().trim().to_string();
        let titulo = texto("titulo");
        let objetivo = texto("objetivo");
        let criterios: Vec<String> = lista_de_textos(&argumentos["criterios"]);
        if titulo.is_empty() || objetivo.is_empty() || criterios.is_empty() {
            let _ = resposta.try_send(Resposta::erro(
                "criar_tarefa precisa de titulo, objetivo e pelo menos um critério.",
            ));
            return;
        }
        let tarefa_do_pai = pai.tarefa.and_then(|n| self.tarefa(n));
        if let Err(recusa) = pode_criar_ajudante(pai.papel, tarefa_do_pai, self.ajudantes_abertos(&pai.id).len(), self.config.max_ajudantes) {
            let _ = resposta.try_send(Resposta::erro(recusa));
            return;
        }
        // A pasta em que o pai mexe (a cópia dele, ou a pasta própria): é dela que a cópia sai e
        // nela que o ajudante é juntado.
        let pasta_do_pai = pai.cwd.clone();
        if !pasta_do_pai.is_dir() {
            let _ = resposta.try_send(Resposta::erro(format!(
                "A sua pasta ({}) não foi encontrada: não dá para criar o ajudante.",
                pasta_do_pai.display()
            )));
            return;
        }
        let verificacao = Some(texto("verificacao")).filter(|v| !v.is_empty());
        let modelo = Some(texto("modelo")).filter(|v| !v.is_empty());
        let id = self.proxima_tarefa;
        self.proxima_tarefa += 1;
        let projeto = pai.projeto.clone();
        let ajudante = novo_id();
        let porta = dados::porta_livre(&self.tarefas).unwrap_or(0);
        self.tarefas.push(Tarefa {
            id,
            projeto: projeto.clone(),
            coordenador: pai.id.clone(),
            trabalhador: ajudante.clone(),
            titulo: titulo.clone(),
            objetivo,
            criterios,
            verificacao,
            estado: EstadoTarefa::Rodando,
            rodada: 1,
            progresso: Some(NA_FILA_PREPARAR.into()),
            criada_em: agora(),
            preparada: false,
            forma: Forma::Copia,
            origem: pasta_do_pai.clone(),
            porta,
            atualizada_em: agora(),
            ..Default::default()
        });
        let mut chat = Chat::novo(ajudante.clone(), Papel::Trabalhador, &projeto, &format!("#{id} {titulo}"), pasta_do_pai);
        chat.pai = Some(pai.id.clone());
        chat.tarefa = Some(id);
        chat.modelo = modelo;
        // Com a conta e a permissão do pai; o ajudante de um chat isolado nasce isolado.
        (chat.conta, chat.permissao) = (pai.conta.clone(), pai.permissao.clone());
        chat.isolado = self.roda_isolado(pai);
        self.chats.push(chat);
        self.preparar_copia(id, Some(resposta), false, cx);
    }

    /// `perguntar_coordenador` de um ajudante: vai para o chat que o criou, com o freio de ciclo.
    fn pergunta_ao_pai(&mut self, ajudante: &Chat, pai: &str, id: u64, pergunta: &str, cx: &mut Context<Self>) -> Resposta {
        if pergunta.is_empty() {
            return Resposta::erro("Escreva a pergunta.");
        }
        let titulo = self.tarefa(id).map_or(ajudante.titulo.clone(), |t| t.titulo.clone());
        let texto = format!(
            "Pergunta do ajudante #{id} \"{titulo}\": {pergunta}\n\nResponda com mensagem_para_chat para o chat {id}."
        );
        let nota = self.contar_ciclo(&ajudante.id, pai);
        if !self.aguardando_resposta.contains(&ajudante.id) {
            self.aguardando_resposta.push(ajudante.id.clone());
        }
        self.registrar("pergunta", &ajudante.id, &ajudante.id, &ajudante.projeto, Some(id), &git::inicio_do_texto(pergunta, 200));
        let entrada = Entrada { de: Some(ajudante.titulo.clone()), texto, remetente: Some(ajudante.id.clone()), nota, ..Default::default() };
        self.entregar_entrada(pai, entrada, cx);
        if nota {
            return Resposta::ok("Muitas mensagens seguidas: a pergunta ficou guardada até o usuário falar. Encerre sua vez.");
        }
        Resposta::ok(
            "Pergunta enviada. A resposta chega como nova mensagem; se puder continuar sem ela, continue, senão \
             encerre sua vez.",
        )
    }

    /// A cópia da tarefa (git worktree) entra na fila do trabalho pesado: criar a cópia, copiar os
    /// arquivos locais e rodar a preparação do projeto. `resposta` é o `criar_tarefa` do pai do ajudante,
    /// que espera por ela; `frente`, o que o usuário pediu (a primeira mensagem de um chat).
    fn preparar_copia(&mut self, id: u64, resposta: Option<Sender<Resposta>>, frente: bool, cx: &mut Context<Self>) {
        if let Some(resposta) = resposta {
            self.respostas_preparo.insert(id, resposta);
        }
        if self.preparando.contains(&id) {
            return;
        }
        if let Some(t) = self.tarefa_mut(id)
            && t.progresso.is_none()
        {
            t.progresso = Some(NA_FILA_PREPARAR.into());
        }
        self.enfileirar_pesado(Pesado::Preparar(id), frente, cx);
        self.salvar();
    }

    /// Cria (ou, num chat de projeto que já rodou, recria no mesmo caminho) a cópia e a prepara; só
    /// então as mensagens que esperavam entram. Diz se começou.
    fn rodar_preparo(&mut self, id: u64, cx: &mut Context<Self>) -> bool {
        let Some(tarefa) = self.tarefa(id).cloned() else {
            return false;
        };
        // A conferência olha as mensagens do chat (o trabalhador que já recebeu o pedido).
        let Some(chat) = self.chat(&tarefa.trabalhador) else {
            return false;
        };
        let (de_projeto, sessao_criada, cwd) = (chat.papel == Papel::Projeto, chat.sessao_criada, chat.cwd.clone());
        let pendente = precisa_preparar(&tarefa, chat, &self.filas);
        let retomada = self.preparos_a_retomar.iter().position(|t| *t == id).map(|i| self.preparos_a_retomar.remove(i));
        if retomada.is_some() || de_projeto {
            if !pendente {
                if let Some(t) = self.tarefa_mut(id) {
                    t.progresso = None;
                }
                // Pronta, ou sem nada esperando por ela: as mensagens que houver entram.
                self.entregar_fila(&tarefa.trabalhador, cx);
                return false;
            }
            if retomada.is_some()
                && let Some(t) = self.tarefa_mut(id)
            {
                t.parada = Some(PARADA_PREPARACAO.into());
            }
        }
        if !self.preparando.insert(id) {
            return false;
        }
        let caminho_projeto = self.origem_da_tarefa(&tarefa);
        let area = self.dados.pasta_copias(&tarefa.projeto);
        // O chat de uma pasta que entrega por PR (sem issue, que já planejou) planeja agora, antes da
        // cópia nascer: o ramo pelo prefixo da pasta e o ponto de partida no GitHub. Fica gravado.
        let tarefa = if de_projeto && tarefa.branch.is_none() && tarefa.ramo_planejado.is_none() && self.entrega_por_pr(&tarefa) {
            let config = self.config_da_pasta(&tarefa.projeto, &caminho_projeto).github;
            let existentes = git::nomes_de_ramos(&caminho_projeto);
            let ramo = crate::issues::ramo_da_tarefa(id, None, None, &config.prefixo_ramo, &tarefa.titulo, &|nome| existentes.contains(nome));
            let partir_de = self.ponto_de_partida(&caminho_projeto, &config);
            if let Some(t) = self.tarefa_mut(id) {
                t.ramo_planejado = Some(ramo);
                t.partir_de = partir_de;
            }
            self.tarefa(id).cloned().unwrap_or(tarefa)
        } else {
            tarefa
        };
        // O ramo e o ponto de partida escolhidos antes (o do Linear, o prefixo da pasta, o
        // `<remoto>/<base>` do modo PR) valem sempre, também ao refazer: nunca são recalculados.
        let novo_branch = tarefa.ramo_planejado.clone().unwrap_or_else(|| format!("vitral/t{id}-{}", slug(&tarefa.titulo)));
        let partir_de = tarefa.partir_de.clone();
        // O chat de projeto tem a pasta gravada desde que nasceu, e a cópia que já rodou volta
        // nela (o `--resume` vale pela pasta), partindo do branch dela.
        let (destino, branch, recriar) = if de_projeto {
            let recriar = sessao_criada || tarefa.branch.is_some();
            (cwd, tarefa.branch.clone().unwrap_or(novo_branch), recriar)
        } else {
            (area.join(format!("t{id}")), novo_branch, false)
        };
        // A cópia que ficou pela metade é desta tarefa, e o trabalhador nunca rodou nela.
        let antiga = (!tarefa.preparada && !recriar)
            .then(|| tarefa.worktree.clone().zip(tarefa.branch.clone()))
            .flatten();
        // A preparação e os arquivos locais do ajudante são os da pasta ligada de onde o pai saiu.
        let pasta_ligada = match de_projeto {
            true => caminho_projeto.clone(),
            false => self
                .chat(&tarefa.coordenador)
                .and_then(|c| c.tarefa)
                .and_then(|n| self.tarefa(n))
                .map_or_else(|| caminho_projeto.clone(), |t| self.origem_da_tarefa(t)),
        };
        let config_projeto = self.config_da_pasta(&tarefa.projeto, &pasta_ligada);
        // O ajudante parte do que a pasta do pai tem agora: antes da cópia, um ponto nela.
        let ponto = (!de_projeto).then(|| format!("Vitral: ponto antes do ajudante #{id}"));
        // A preparação na cópia de um chat isolado roda isolada como ele.
        let isolado = self.modo_isolado_da_tarefa(&tarefa);
        // No modo PR a cópia parte de `<remoto>/<base>`: antes, o fetch (rede, fora da trava do projeto).
        let executor = self.executor();
        let geracao = match self.tarefa_mut(id) {
            Some(t) => {
                t.geracao_preparo += 1;
                t.progresso = Some(
                    if recriar {
                        "refazendo a cópia"
                    } else if retomada.is_some() {
                        "preparando a cópia de novo"
                    } else {
                        "preparando a cópia do projeto"
                    }
                    .into(),
                );
                t.geracao_preparo
            }
            None => return false,
        };
        cx.spawn(async move |motor, cx| {
            let projeto = caminho_projeto.clone();
            let (copia, avisos_de_rede) = cx
                .background_spawn(async move {
                    if !projeto.is_dir() {
                        let erro = anyhow::anyhow!("a pasta {} não foi encontrada (disco desmontado ou pasta movida)", projeto.display());
                        return (Some(Err(erro)), Vec::new());
                    }
                    // O chat de projeto e o ajudante decidiram ter cópia com o git presente.
                    if !git::usa_git(&projeto) {
                        return (Some(Err(anyhow::anyhow!("{} não usa mais git (ou não tem commit)", projeto.display()))), Vec::new());
                    }
                    let avisos = match &partir_de {
                        Some(ponto) => buscar_ponto_de_partida(&*executor, &projeto, ponto).await,
                        None => Vec::new(),
                    };
                    let _ = std::fs::create_dir_all(&area);
                    // Na vez do repositório: uma junção no meio mudaria o HEAD e disputaria o índice.
                    // O ponto do pai e a cópia nova ficam na mesma vez.
                    let copia = git::na_vez_do_projeto(&projeto, || {
                        if let Some((pasta, branch)) = antiga {
                            let _ = git::limpar_copia(&projeto, &pasta, &area, &branch, Some(&format!("t{id}")), &[], false);
                        }
                        if let Some(mensagem) = &ponto
                            && let Err(erro) = git::checkpoint(&projeto, mensagem)
                        {
                            return Some(Err(erro.context(format!("não deu para guardar num commit o que {} tem", projeto.display()))));
                        }
                        Some(if recriar {
                            git::recriar_worktree_de(&projeto, &destino, &branch, partir_de.as_deref())
                                .map(|(c, existia)| (c, Some(existia)))
                        } else {
                            git::criar_worktree_de(&projeto, &destino, &branch, partir_de.as_deref()).map(|c| (c, None))
                        })
                    });
                    (copia, avisos)
                })
                .await;
            // Gravada antes da preparação, que demora: se o Vitral fechar, ela é apagada ao refazer.
            let pasta = match &copia {
                Some(Ok((c, _))) => {
                    let segue = motor.update(cx, |m, cx| {
                        if let Some(t) = m.tarefa_mut(id)
                            && t.geracao_preparo == geracao
                        {
                            t.worktree = Some(c.caminho.clone());
                            t.branch = Some(c.branch.clone());
                            t.base = Some(c.base.clone());
                        }
                        m.salvar();
                        cx.notify();
                        // Fechou enquanto a cópia era criada: a preparação (npm install e afins) não
                        // roda, e é refeita ao abrir de novo.
                        !m.encerrado.load(Ordering::SeqCst)
                    });
                    if !matches!(segue, Ok(true)) {
                        return;
                    }
                    Some(c.caminho.clone())
                }
                _ => None,
            };
            let (copiados, preparacao) = match pasta {
                Some(pasta) => {
                    cx.background_spawn(async move {
                        // Arquivos locais fora do git (.env) e o script de preparação do projeto.
                        let padroes = config_projeto.padroes_de_copia();
                        let copiados =
                            git::copiar_arquivos_locais(&caminho_projeto, &pasta, &padroes);
                        let preparacao = Some(config_projeto.preparacao.trim().to_string())
                            .filter(|p| !p.is_empty())
                            .map(|comando| git::verificar(&pasta, &comando, isolado));
                        (copiados, preparacao)
                    })
                    .await
                }
                None => (Vec::new(), None),
            };
            let _ = motor.update(cx, |motor, cx| {
                if !motor.encerrado.load(Ordering::SeqCst) {
                    motor.copia_pronta(id, geracao, copia, copiados, preparacao, avisos_de_rede, cx)
                }
            });
        })
        .detach();
        self.salvar();
        cx.notify();
        true
    }

    /// Fim da preparação. Num chat de projeto, as mensagens que esperavam entram (ou, se falhou, ficam
    /// guardadas com o motivo, e o estado não muda). No ajudante, ele recebe o pedido (com o que chegou
    /// para ele enquanto isso) e o pai, a resposta de criar_tarefa.
    #[allow(clippy::too_many_arguments)]
    fn copia_pronta(
        &mut self,
        id: u64,
        geracao: u32,
        copia: Option<anyhow::Result<(git::CopiaIsolada, Option<bool>)>>,
        copiados: Vec<String>,
        preparacao: Option<Verificacao>,
        avisos_de_rede: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        self.preparando.remove(&id);
        let Some(tarefa) = self.tarefa(id).cloned() else {
            self.pesado_terminou(Pesado::Preparar(id), cx);
            return;
        };
        // Outra preparação tomou o lugar desta (a pasta trocou, a tarefa foi refeita).
        if tarefa.geracao_preparo != geracao {
            self.pesado_terminou(Pesado::Preparar(id), cx);
            return;
        }
        if self.de_projeto(&tarefa.trabalhador) {
            for aviso in &avisos_de_rede {
                self.avisar(&tarefa.trabalhador, aviso, false);
            }
            self.copia_do_chat_pronta(&tarefa, copia, copiados, preparacao, cx);
            self.salvar();
            self.pesado_terminou(Pesado::Preparar(id), cx);
            self.entregar_fila(&tarefa.trabalhador, cx);
            cx.notify();
            return;
        }
        let resposta = self.respostas_preparo.remove(&id);
        let ajudante = tarefa.trabalhador.clone();
        let pai = tarefa.coordenador.clone();
        let mut aviso = String::new();
        let copia = copia.unwrap_or_else(|| Err(anyhow::anyhow!("a pasta do chat não usa git")));
        match copia.map(|(copia, _)| copia) {
            Ok(copia) => {
                if let Some(c) = self.chat_mut(&ajudante) {
                    c.cwd = copia.caminho.clone();
                }
                if let Some(t) = self.tarefa_mut(id) {
                    t.worktree = Some(copia.caminho);
                    t.branch = Some(copia.branch);
                    t.base = Some(copia.base);
                    t.ramo_alvo = copia.ramo;
                    t.arquivos_copiados = copiados.clone();
                }
            }
            Err(erro) => {
                // O ajudante nunca roda na pasta do pai.
                if let Some(t) = self.tarefa_mut(id) {
                    if t.estado.ativa() {
                        t.estado = EstadoTarefa::Falhou;
                    }
                    t.progresso = None;
                    t.parada = None;
                }
                self.avisar(&ajudante, &format!("Não deu para criar a cópia deste ajudante: {erro:#}"), true);
                let texto = format!(
                    "O ajudante #{id} \"{}\" não começou: não deu para criar a cópia dele ({erro:#}). Ele ficou como \
                     falhou e não rodou; crie de novo se ainda fizer sentido.",
                    tarefa.titulo
                );
                match resposta {
                    Some(resposta) => {
                        let _ = resposta.try_send(Resposta::erro(texto));
                    }
                    None => self.entregar(&pai, Some("Vitral".into()), texto, cx),
                }
                self.salvar();
                self.pesado_terminou(Pesado::Preparar(id), cx);
                cx.notify();
                return;
            }
        }
        if !copiados.is_empty() {
            aviso.push_str(&format!(" Copiei para a cópia: {}.", copiados.join(", ")));
        }
        let mut primeira = prompts::tarefa(&tarefa);
        if let Some(p) = &preparacao {
            let tempo = p.tempo().map(|t| format!(" ({t})")).unwrap_or_default();
            if p.passou {
                aviso.push_str(&format!(" Preparação `{}` ok{tempo}.", p.comando));
            } else {
                aviso.push_str(&format!(" A preparação `{}` FALHOU{tempo}.", p.comando));
                primeira.push_str(&format!(
                    "\nAviso: a preparação da sua pasta (`{}`) falhou:\n```\n{}\n```\n",
                    p.comando,
                    p.saida.trim()
                ));
            }
        }
        if let Some(t) = self.tarefa_mut(id) {
            t.progresso = None;
            t.preparacao = preparacao;
            t.preparada = true;
            t.parada = None;
        }
        let ativa = tarefa.estado.ativa();
        if ativa {
            // O pedido é do pai; o que chegou durante a preparação vai junto.
            let de = self.chat(&pai).map_or_else(|| "chat".to_string(), |c| c.titulo.clone());
            let mut mensagens = vec![Entrada { de: Some(de), texto: primeira, remetente: Some(pai.clone()), ..Default::default() }];
            mensagens.extend(self.filas.remove(&ajudante).unwrap_or_default());
            self.comecar_turno(&ajudante, mensagens, cx);
        }
        if let Some(resposta) = resposta {
            let texto = if ativa {
                let fila = if self.esperando_vaga(&ajudante) {
                    " Ele está na fila e começa quando houver vaga."
                } else {
                    ""
                };
                format!(
                    "Tarefa #{id} criada: o ajudante #{id} começou numa cópia da sua pasta, com o que ela tinha agora.{fila}{aviso} \
                     O relatório chega para você sozinho quando ele terminar; não mexa nos arquivos dele e encerre sua vez \
                     quando não houver mais o que fazer."
                )
            } else {
                format!(
                    "Tarefa #{id} criada, mas o ajudante ficou {} antes de começar.",
                    tarefa.estado.nome()
                )
            };
            let _ = resposta.try_send(Resposta::ok(texto));
        }
        self.salvar();
        self.pesado_terminou(Pesado::Preparar(id), cx);
        cx.notify();
    }

    /// A cópia de um chat de projeto ficou pronta (ou não): grava, avisa no chat e, com a preparação
    /// falhando, deixa a saída dela junto da próxima mensagem.
    fn copia_do_chat_pronta(
        &mut self,
        tarefa: &Tarefa,
        copia: Option<anyhow::Result<(git::CopiaIsolada, Option<bool>)>>,
        copiados: Vec<String>,
        preparacao: Option<Verificacao>,
        cx: &mut Context<Self>,
    ) {
        let (id, chat) = (tarefa.id, tarefa.trabalhador.clone());
        let origem = self.origem_da_tarefa(tarefa);
        let sessao_criada = self.chat(&chat).is_some_and(|c| c.sessao_criada);
        let (copia, recriada) = match copia {
            Some(Ok(pronta)) => pronta,
            erro => {
                let motivo = match erro {
                    Some(Err(erro)) => format!("{erro:#}"),
                    _ => "a pasta não usa git".into(),
                };
                if let Some(t) = self.tarefa_mut(id) {
                    t.falha_copia = Some(motivo.clone());
                    (t.progresso, t.parada) = (None, None);
                }
                self.avisar(
                    &chat,
                    &format!(
                        "Não deu para preparar a cópia ({motivo}). A mensagem ficou guardada: mande outra para \
                         tentar de novo."
                    ),
                    true,
                );
                if let Some(dono) = tarefa.dono() {
                    let texto = format!("A cópia do chat {id} \"{}\" não foi preparada: {motivo}.", tarefa.titulo);
                    self.entregar(dono, Some("Vitral".into()), texto, cx);
                }
                return;
            }
        };
        if let Some(c) = self.chat_mut(&chat)
            && !sessao_criada
        {
            c.cwd = copia.caminho.clone();
        }
        let mut aviso = match recriada {
            Some(true) => format!("A cópia foi recriada a partir de {}.", mestre::com_til(&origem)),
            Some(false) => format!(
                "A cópia foi recriada a partir de {}; mudanças que não tinham sido juntadas se perderam na limpeza.",
                mestre::com_til(&origem)
            ),
            None => format!("A cópia de {} está pronta em {}.", mestre::com_til(&origem), mestre::com_til(&copia.caminho)),
        };
        if copia.sujo {
            aviso.push_str(" A pasta original tem mudanças sem commit, que a cópia não vê (ela parte do último commit).");
        }
        if !copiados.is_empty() {
            aviso.push_str(&format!(" Copiei para a cópia: {}.", copiados.join(", ")));
        }
        let mut falhou = None;
        if let Some(p) = &preparacao {
            let tempo = p.tempo().map(|t| format!(" ({t})")).unwrap_or_default();
            if p.passou {
                aviso.push_str(&format!(" Preparação `{}` ok{tempo}.", p.comando));
            } else {
                aviso.push_str(&format!(" A preparação `{}` FALHOU{tempo}.", p.comando));
                falhou = Some(format!(
                    "A preparação da sua cópia (`{}`) falhou:\n```\n{}\n```",
                    p.comando,
                    p.saida.trim()
                ));
            }
        }
        if let Some(t) = self.tarefa_mut(id) {
            t.worktree = Some(copia.caminho);
            t.branch = Some(copia.branch);
            t.base = Some(copia.base);
            t.ramo_alvo = copia.ramo;
            t.preparada = true;
            t.copia_limpa = false;
            // O que veio da origem (o .env) nunca entra no commit: nem no Juntar, nem no PR.
            for arquivo in &copiados {
                if !t.arquivos_copiados.contains(arquivo) {
                    t.arquivos_copiados.push(arquivo.clone());
                }
            }
            t.falha_copia = None;
            t.preparacao = preparacao;
            (t.progresso, t.parada) = (None, None);
        }
        self.avisar(&chat, &aviso, false);
        // Acabou de sair da origem: o primeiro turno não precisa conferir se ela andou.
        self.sincronizados.insert(chat.clone());
        // A saída da preparação que falhou vai ao Claude junto com a mensagem que esperava.
        if let Some(texto) = falhou {
            let nota = Entrada { de: Some("Vitral".into()), texto, nota: true, registrada: true, ..Default::default() };
            self.filas.entry(chat).or_default().insert(0, nota);
        }
    }

    fn ver_diff(&mut self, id: u64, arquivos: Vec<String>, resposta: Sender<Resposta>, cx: &mut Context<Self>) {
        let tarefa = self.tarefa(id).cloned().unwrap();
        let projeto = self.origem_da_tarefa(&tarefa);
        // No resumo de um diff grande, os arquivos que o revisor apontou vêm primeiro.
        let apontados: Vec<String> = tarefa.revisao.iter().flat_map(|r| &r.problemas).map(|p| p.arquivo.clone()).collect();
        if tarefa.copia_apagada() {
            let _ = resposta.try_send(Resposta::ok(format!(
                "A tarefa #{id} está {} e a cópia dela já foi apagada: não há diff para mostrar.",
                tarefa.estado.nome()
            )));
            return;
        }
        if !tarefa.preparada {
            let _ = resposta.try_send(Resposta::ok(
                "A cópia desta tarefa ainda não ficou pronta e o trabalhador não começou.",
            ));
            return;
        }
        let remoto_base = self.remoto_base(&tarefa);
        let (Some(pasta), Some(base)) = (tarefa.worktree, tarefa.base) else {
            let _ = resposta.try_send(Resposta::ok(
                "Esta tarefa roda direto na pasta do projeto (sem git): leia os arquivos ou rode `git \
                 diff` se houver repositório.",
            ));
            return;
        };
        cx.background_spawn(async move {
            let base = git::base_da_tarefa(&projeto, &pasta, &base, remoto_base.as_deref());
            let texto = git::diff_para_coordenador(&pasta, &projeto, &base, &arquivos, &apontados, 20_000)
                .unwrap_or_else(|erro| format!("Não deu para ler o diff: {erro}"));
            let _ = resposta.try_send(Resposta::ok(texto));
        })
        .detach();
    }

    pub fn diff_da_tarefa(&self, id: u64, cx: &mut Context<Self>) -> Task<Result<String, LeituraDiff>> {
        let tarefa = self.tarefa(id).cloned();
        let projeto = tarefa.as_ref().map(|t| self.origem_da_tarefa(t));
        let remoto_base = tarefa.as_ref().and_then(|t| self.remoto_base(t));
        cx.background_spawn(async move {
            let (Some(tarefa), Some(projeto)) = (tarefa, projeto) else {
                return Err(LeituraDiff::SemCopia);
            };
            let (Some(pasta), Some(base)) = (tarefa.worktree, tarefa.base) else {
                return Err(LeituraDiff::SemCopia);
            };
            if !pasta.is_dir() {
                return Err(LeituraDiff::SemCopia);
            }
            git::diff_completo(&pasta, &git::base_da_tarefa(&projeto, &pasta, &base, remoto_base.as_deref()))
                .map_err(|erro| LeituraDiff::Git(format!("{erro:#}")))
        })
    }

    /// O usuário aprova pelo painel: junta como o coordenador faria e avisa o coordenador. A
    /// `ressalva` (aprovar com pendência vermelha) vai para o commit, como a do coordenador.
    pub fn aprovar_pelo_usuario(
        &mut self,
        id: u64,
        ressalva: Option<String>,
        cx: &mut Context<Self>,
    ) -> smol::channel::Receiver<Resposta> {
        let (envia, recebe) = smol::channel::bounded(1);
        let (avisa, aviso) = smol::channel::bounded::<Resposta>(1);
        // Só há a quem avisar com um pedido aberto (no chat de projeto, a junção tira o dono).
        let coordenador = self.tarefa(id).and_then(|t| t.dono()).map(str::to_string);
        self.aprovar_tarefa(id, "Aprovada pelo usuário no Vitral.".into(), ressalva, avisa, "usuario", None, cx);
        cx.spawn(async move |motor, cx| {
            let Ok(resposta) = aviso.recv().await else {
                return;
            };
            if let Some(coordenador) = coordenador
                && !resposta.erro
            {
                let texto = if coordenador == crate::modelo::ID_MESTRE {
                    format!("O usuário aprovou a tarefa #{id} pelo painel. {}", resposta.texto)
                } else {
                    format!("O usuário aprovou o ajudante #{id} pelo painel. {}", resposta.texto)
                };
                // O Mestre só fica sabendo: a conclusão da divisão, se for o caso, é que o acorda.
                let _ = motor.update(cx, |m, cx| {
                    if coordenador == crate::modelo::ID_MESTRE {
                        m.nota_ao_mestre(texto, cx);
                    } else {
                        m.entregar(&coordenador, Some("Vitral".into()), texto, cx);
                    }
                });
            }
            let _ = envia.send(resposta).await;
        })
        .detach();
        recebe
    }

    /// O usuário pede ajuste direto ao trabalhador (ex.: comentários na revisão das mudanças).
    pub fn ajuste_do_usuario(&mut self, id: u64, feedback: String, cx: &mut Context<Self>) -> Resposta {
        // Num chat de projeto em conversa, os comentários são uma mensagem comum, sem rodada nova.
        if let Some(chat) = self
            .tarefa(id)
            .filter(|t| t.estado == EstadoTarefa::Conversa && self.de_projeto(&t.trabalhador))
            .map(|t| t.trabalhador.clone())
        {
            let texto = format!("Comentários da revisão das mudanças:\n{feedback}");
            self.enviar_do_usuario(&chat, texto, Vec::new(), Reforco::default(), cx);
            return Resposta::ok("Os comentários foram para o chat.");
        }
        // Nada vai anexado: no Revisar, o usuário escolhe as sugestões do revisor com "Usar".
        let resposta = self.pedir_ajuste_de(id, feedback.clone(), "usuário", "", "", OrigemAjuste::Usuario, None, cx);
        if !resposta.erro
            && let Some(coordenador) = self.tarefa(id).and_then(|t| t.dono()).map(str::to_string)
        {
            if coordenador == crate::modelo::ID_MESTRE {
                let texto = format!(
                    "O usuário pediu ajustes direto ao chat {id}:\n{feedback}\nO novo relatório chega para você como \
                     sempre."
                );
                self.nota_ao_mestre(texto, cx);
            } else {
                // Só fica sabendo: quem acorda o pai é o relatório.
                let texto = format!(
                    "O usuário pediu ajustes direto ao ajudante #{id}:\n{feedback}\nO novo relatório chega para você como \
                     sempre."
                );
                let nota = Entrada { de: Some("Vitral".into()), texto, nota: true, ..Default::default() };
                self.entregar_entrada(&coordenador, nota, cx);
            }
        }
        self.salvar();
        resposta
    }

    pub fn descartar_arquivo(&self, id: u64, arquivo: String, cx: &mut Context<Self>) -> Task<anyhow::Result<Option<String>>> {
        let tarefa = self.tarefa(id).cloned();
        let projeto = tarefa.as_ref().map(|t| self.origem_da_tarefa(t));
        let remoto_base = tarefa.as_ref().and_then(|t| self.remoto_base(t));
        cx.background_spawn(async move {
            let tarefa = tarefa.ok_or_else(|| anyhow::anyhow!("o chat não existe mais"))?;
            let (Some(pasta), Some(base), Some(projeto)) = (tarefa.worktree, tarefa.base, projeto) else {
                anyhow::bail!("este chat não tem cópia isolada");
            };
            // Volta ao que está no projeto (no modo PR, ao ponto comum com a base do GitHub), não à
            // base antiga (que desfaria o que outra tarefa juntou).
            let base = git::base_da_tarefa(&projeto, &pasta, &base, remoto_base.as_deref());
            git::descartar_arquivo(&pasta, &base, &arquivo)
        })
    }

    /// Volta os arquivos do chat ao ponto anterior a uma mensagem.
    pub fn voltar_ao_ponto(&self, chat: &str, ponto: String, cx: &mut Context<Self>) -> Task<anyhow::Result<usize>> {
        let pasta = self.chat(chat).map(|c| c.cwd.clone());
        let rodando = self.processos.contains_key(chat);
        let apagada = self.chat(chat).is_some_and(|c| self.copia_apagada(c));
        cx.background_spawn(async move {
            if rodando {
                anyhow::bail!("pare a conversa antes de voltar os arquivos");
            }
            if apagada {
                anyhow::bail!("a cópia deste chat já foi apagada");
            }
            let pasta = pasta.ok_or_else(|| anyhow::anyhow!("conversa não encontrada"))?;
            git::voltar_para(&pasta, &ponto)
        })
    }

    // ---------- cópias das tarefas ----------

    /// A cópia isolada do trabalhador já foi apagada (na aprovação ou na limpeza): o chat não roda
    /// mais, e voltar para a pasta do projeto o deixaria escrever no original.
    pub fn copia_apagada(&self, chat: &Chat) -> bool {
        chat.papel == Papel::Trabalhador
            && chat.tarefa.and_then(|t| self.tarefa(t)).is_some_and(|t| {
                t.copia_apagada() || (t.worktree.is_none() && chat.cwd != self.origem_da_tarefa(t))
            })
    }

    /// Tarefas cuja cópia ainda existe e já não vai ser juntada: as encerradas e, com `revisao`,
    /// também as em revisão (que viram canceladas ao limpar).
    /// A cópia parada da tarefa já não vai ser juntada (a tarefa encerrou ou o coordenador foi
    /// arquivado): dá para apagar.
    pub fn pode_apagar_copia(&self, id: u64) -> bool {
        self.tarefa(id).is_some_and(|t| {
            self.copias_paradas(true).any(|p| p.id == id)
                && (t.estado.encerrada()
                    || self.chat(&t.coordenador).is_some_and(|c| self.arquivada(c))
                    || self.chat(&t.trabalhador).is_some_and(|c| c.papel == Papel::Projeto && self.arquivada(c)))
        })
    }

    /// O chat de projeto arquivado em conversa, sem processo e sem nada para juntar: a cópia dele pode
    /// sair (a próxima mensagem, depois de desarquivar, cria outra).
    fn copia_de_chat_parado(&self, t: &Tarefa) -> bool {
        t.estado == EstadoTarefa::Conversa
            && t.mudancas.as_ref().is_none_or(|m| m.arquivos == 0)
            && t.juntando_desde == 0
            && !self.sincronizando.contains(&t.id)
            && self.chat(&t.trabalhador).is_some_and(|c| c.papel == Papel::Projeto && self.arquivada(c))
    }

    pub fn copias_paradas<'a>(&'a self, revisao: bool) -> impl Iterator<Item = &'a Tarefa> + 'a {
        self.tarefas.iter().filter(move |t| {
            t.worktree.is_some()
                && !t.copia_limpa
                && (t.estado.encerrada()
                    || (revisao && t.estado == EstadoTarefa::EmRevisao)
                    || self.copia_de_chat_parado(t))
                // Em revisão com progresso: a verificação e o revisor ainda rodam na cópia.
                && !(t.estado == EstadoTarefa::EmRevisao && t.progresso.is_some())
                && !self.processos.contains_key(&t.trabalhador)
                && !self.esperando_vaga(&t.trabalhador)
                && !self.retentativas.contains_key(&t.trabalhador)
                && !self.juntando.contains(&t.id)
                && !self.verificando.contains_key(&t.id)
                && !self.preparando.contains(&t.id)
                && !self.na_fila_pesada(t.id)
                // Um ajudante aberto parte desta cópia e junta nela.
                && self.ajudantes_abertos(&t.trabalhador).is_empty()
        })
    }

    /// Apaga a pasta, o branch e os pontos de volta das cópias das tarefas paradas. As em revisão
    /// viram canceladas: sem cópia não há o que juntar. Devolve quantas apagou e os erros.
    pub fn limpar_copias(&mut self, ids: &[u64], cx: &mut Context<Self>) -> Task<(usize, Vec<String>)> {
        let lote: Vec<(u64, PathBuf, PathBuf, PathBuf, String, Vec<String>, bool)> = self
            .copias_paradas(true)
            .filter(|t| ids.contains(&t.id))
            .filter_map(|t| {
                let (pasta, branch) = (t.worktree.clone()?, t.branch.clone()?);
                let pontos = self.pontos_so_do_chat(&t.trabalhador);
                let area = self.area_da_copia(t);
                let de_projeto = self.de_projeto(&t.trabalhador);
                Some((t.id, self.origem_da_tarefa(t), pasta, area, branch, pontos, de_projeto))
            })
            .collect();
        // Marca antes: nenhum turno começa na pasta enquanto ela some. Vai para o disco já: se o
        // PC cair no meio, a abertura acha a marca e termina a limpeza em vez de deixar aprovar a
        // cópia pela metade.
        for (id, ..) in &lote {
            if let Some(t) = self.tarefa_mut(*id) {
                t.copia_limpa = true;
            }
        }
        if !lote.is_empty() {
            self.salvar();
        }
        cx.spawn(async move |motor, cx| {
            let resultados: Vec<(u64, anyhow::Result<bool>)> = cx
                .background_spawn(async move {
                    lote.into_iter()
                        .map(|(id, projeto, pasta, area, branch, pontos, de_projeto)| {
                            // Na vez do projeto: uma junção ou cópia nova no meio disputaria o índice.
                            // O ajudante cuja pasta do pai já saiu limpa pelo repositório de onde a cópia veio.
                            let projeto = if !de_projeto && !projeto.is_dir() {
                                git::repositorio_da_copia(&pasta).unwrap_or(projeto)
                            } else {
                                projeto
                            };
                            let limpou = git::na_vez_do_projeto(&projeto, || {
                                let grupo = format!("t{id}");
                                if de_projeto {
                                    limpar_copia_do_chat(&projeto, &pasta, &area, &branch, &grupo, &pontos)
                                } else {
                                    git::limpar_copia(&projeto, &pasta, &area, &branch, Some(&grupo), &pontos, false).map(|_| false)
                                }
                            });
                            (id, limpou)
                        })
                        .collect()
                })
                .await;
            motor
                .update(cx, |m, cx| {
                    let (mut apagadas, mut erros) = (0, Vec::new());
                    for (id, resultado) in resultados {
                        let de_projeto = m.tarefa(id).is_some_and(|t| m.de_projeto(&t.trabalhador));
                        let Some(t) = m.tarefa_mut(id) else {
                            continue;
                        };
                        let guardou = match resultado {
                            Ok(guardou) => guardou,
                            Err(erro) => {
                                t.copia_limpa = false;
                                erros.push(format!("#{id}: {erro:#}"));
                                continue;
                            }
                        };
                        apagadas += 1;
                        t.worktree = None;
                        t.progresso = None;
                        let trabalhador = t.trabalhador.clone();
                        if de_projeto {
                            // O chat continua: o branch fica (com o que não foi juntado, num commit) e a
                            // próxima mensagem cria a cópia de novo, no mesmo lugar.
                            t.preparada = false;
                            if !matches!(t.estado, EstadoTarefa::Conversa) && !t.estado.encerrada() {
                                (t.estado, t.entrega_pendente) = (EstadoTarefa::Conversa, None);
                                t.coordenador.clear();
                            }
                            let branch = t.branch.clone().unwrap_or_default();
                            let origem = mestre::com_til(&t.origem);
                            let mut aviso = format!(
                                "A cópia deste chat foi apagada para liberar espaço; a próxima mensagem cria outra a \
                                 partir de {origem}."
                            );
                            if guardou {
                                aviso.push_str(&format!(" O que não tinha sido juntado ficou guardado no branch {branch}."));
                            }
                            m.avisar(&trabalhador, &aviso, false);
                            if !m.chat(&trabalhador).is_some_and(|c| m.arquivada(c)) {
                                m.entregar_fila(&trabalhador, cx);
                            }
                        } else {
                            t.branch = None;
                            if !t.estado.encerrada() {
                                t.estado = EstadoTarefa::Cancelada;
                            }
                            m.avisar(&trabalhador, "A cópia deste ajudante foi apagada; ele não roda mais.", false);
                        }
                        if let Some(c) = m.chat(&trabalhador) {
                            let _ = m.dados.salvar_chat(c);
                        }
                    }
                    m.salvar();
                    cx.notify();
                    (apagadas, erros)
                })
                .unwrap_or_default()
        })
    }

    /// Ao abrir: esquece as cópias apagadas à mão e apaga as das tarefas encerradas sem atividade
    /// há mais dias que o prazo das Configurações.
    fn limpar_ao_abrir(&mut self, cx: &mut Context<Self>) {
        let mut projetos: Vec<PathBuf> = self
            .tarefas
            .iter()
            .map(|t| self.origem_da_tarefa(t))
            .filter(|o| o.is_dir())
            .collect();
        projetos.sort();
        projetos.dedup();
        let dias = u64::from(self.config.limpar_copias_dias);
        let limite = agora().saturating_sub(dias * 86_400);
        let mut antigas: Vec<u64> = self
            .copias_paradas(false)
            .filter(|t| {
                let ultima = self.chat(&t.trabalhador).map_or(0, |c| c.atualizado_em);
                dias > 0 && ultima.max(t.criada_em) < limite
            })
            .map(|t| t.id)
            .collect();
        // Limpeza que o fechamento do Vitral interrompeu: a marca ficou, a pasta não sumiu.
        for t in self.tarefas.iter_mut().filter(|t| t.copia_limpa && t.worktree.is_some()) {
            t.copia_limpa = false;
            antigas.push(t.id);
        }
        // Os pontos de volta dos chats de projeto (cada vez tira um): ficam os mais novos.
        let podas: Vec<(PathBuf, String)> = self
            .tarefas
            .iter()
            .filter(|t| t.tem_git() && self.de_projeto(&t.trabalhador))
            .map(|t| (self.origem_da_tarefa(t), format!("t{}", t.id)))
            .filter(|(o, _)| o.is_dir())
            .collect();
        self.recuperar_juncoes(cx);
        // A cópia de um chat que ainda não contou o que tem para juntar (o trabalhador do Vitral
        // antigo que virou chat de projeto): a barra "Para juntar" aparece já.
        let sem_conta: Vec<u64> = self
            .tarefas
            .iter()
            .filter(|t| t.estado == EstadoTarefa::Conversa && t.tem_copia() && t.worktree.is_some() && t.mudancas.is_none())
            .filter(|t| self.chat(&t.trabalhador).is_some_and(|c| c.papel == Papel::Projeto && !self.arquivada(c)))
            .map(|t| t.id)
            .collect();
        for id in sem_conta {
            self.contar_mudancas(id, cx);
        }
        cx.spawn(async move |motor, cx| {
            cx.background_spawn(async move {
                for projeto in projetos {
                    git::na_vez_do_projeto(&projeto, || git::podar(&projeto));
                }
                for (origem, grupo) in podas {
                    git::na_vez_do_projeto(&origem, || git::podar_pontos(&origem, &grupo, git::PONTOS_GUARDADOS));
                }
            })
            .await;
            if !antigas.is_empty() {
                let _ = motor.update(cx, |m, cx| m.limpar_copias(&antigas, cx).detach());
            }
        })
        .detach();
    }

    /// Pontos de volta do chat que nenhum outro usa (duas fotos iguais no mesmo segundo dão o
    /// mesmo commit).
    fn pontos_so_do_chat(&self, chat: &str) -> Vec<String> {
        let pontos = |c: &Chat| -> Vec<String> {
            c.itens
                .iter()
                .filter_map(|i| match i {
                    Item::Usuario { ponto, .. } | Item::Entrega { ponto, .. } => ponto.clone(),
                    _ => None,
                })
                .collect()
        };
        let Some(este) = self.chat(chat) else {
            return Vec::new();
        };
        let dos_outros: std::collections::HashSet<String> =
            self.chats.iter().filter(|c| c.id != chat).flat_map(pontos).collect();
        pontos(este).into_iter().filter(|p| !dos_outros.contains(p)).collect()
    }

    /// "Continuar num chat novo" de uma conversa do Vitral antigo: um chat de projeto no mesmo projeto
    /// (`criado_por = "continuacao:<id>"`) e a conversa antiga em markdown, para ir anexada à primeira
    /// mensagem dele. Devolve o chat novo e o arquivo.
    pub fn continuar_conversa_antiga(&mut self, id: &str, cx: &mut Context<Self>) -> Result<(String, PathBuf), String> {
        let antiga = self.chat(id).filter(|c| c.papel == Papel::Coordenador).ok_or("Conversa antiga não encontrada.")?;
        let projeto = antiga.projeto.clone();
        let arquivo = self.exportar_conversa(id).map_err(|e| format!("Não deu para guardar a conversa antiga: {e:#}"))?;
        let novo = self.novo_chat(&projeto, None, &format!("continuacao:{id}"), cx)?;
        Ok((novo, arquivo))
    }

    /// Grava a conversa em markdown (em `.vitral/exportadas`) para outro chat ler como contexto.
    pub fn exportar_conversa(&self, id: &str) -> anyhow::Result<PathBuf> {
        let chat = self.chat(id).ok_or_else(|| anyhow::anyhow!("conversa não encontrada"))?;
        let mut texto = format!("# Conversa \"{}\" (projeto {})\n\n", chat.titulo, self.nome_do_projeto(&chat.projeto));
        for item in &chat.itens {
            match item {
                Item::Usuario { texto: t, .. } => texto.push_str(&format!("## Usuário\n\n{t}\n\n")),
                Item::Assistente { texto: t } => texto.push_str(&format!("## Claude\n\n{t}\n\n")),
                Item::Entrega { de, texto: t, .. } => {
                    texto.push_str(&format!("## Mensagem de {de}\n\n{t}\n\n"))
                }
                Item::Ferramenta { resumo, .. } => texto.push_str(&format!("- (ferramenta) {resumo}\n")),
                Item::Aviso { .. } | Item::Cartao { .. } | Item::Desconhecido(_) => {}
            }
        }
        let pasta = self.dados.pasta_exportadas();
        std::fs::create_dir_all(&pasta)?;
        let arquivo = pasta.join(format!("{}-{}.md", slug(&chat.titulo), chat.id));
        std::fs::write(&arquivo, texto)?;
        Ok(arquivo)
    }

    fn rodar_rotinas(&mut self, cx: &mut Context<Self>) {
        let (dia, minutos, hoje) = crate::modelo::agora_local();
        let na_hora: Vec<usize> = self
            .rotinas
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.ativa && r.ultima != hoje && minutos >= r.hora && r.dias.contains(&dia)
            })
            .map(|(i, _)| i)
            .collect();
        // Só grava quando alguma rodou (a marca do dia): antes, regravava a cada meio minuto.
        if na_hora.is_empty() {
            return;
        }
        for i in na_hora {
            // A do projeto arquivado ganha a marca sem rodar: desarquivar à tarde não a dispara.
            self.rotinas[i].ultima = hoje.clone();
            let rotina = self.rotinas[i].clone();
            if self.projeto_arquivado(&rotina.projeto) {
                let texto = format!("A rotina \"{}\" não rodou: o projeto está arquivado", titulo_de(&rotina.texto));
                self.registrar("rotina", "vitral", "", &rotina.projeto, None, &texto);
            } else {
                let _ = self.rodar_rotina(&rotina, cx);
            }
        }
        self.salvar();
    }

    /// A rotina sem projeto vai ao Mestre (e a divisão dela começa sem cartão); a de um projeto abre
    /// um chat novo nele com o pedido.
    pub fn rodar_rotina(&mut self, rotina: &Rotina, cx: &mut Context<Self>) -> Result<String, String> {
        if rotina.projeto.is_empty() {
            let entrada = Entrada {
                de: Some("Rotina".into()),
                rotina: Some(rotina.id.clone()),
                texto: rotina.texto.clone(),
                ..Default::default()
            };
            self.entregar_entrada(crate::modelo::ID_MESTRE, entrada, cx);
            return Ok(crate::modelo::ID_MESTRE.into());
        }
        let id = match self.novo_chat(&rotina.projeto, None, &format!("rotina:{}", rotina.id), cx) {
            Ok(id) => id,
            Err(erro) => {
                eprintln!("vitral: a rotina {} não rodou: {erro}", rotina.id);
                return Err(erro);
            }
        };
        let titulo = format!("Rotina: {}", titulo_de(&rotina.texto));
        if let Some(c) = self.chat_mut(&id) {
            c.titulo = titulo.clone();
        }
        if let Some(t) = self.chat(&id).and_then(|c| c.tarefa).and_then(|t| self.tarefa_mut(t)) {
            t.titulo = titulo;
        }
        self.enviar_do_usuario(&id, rotina.texto.clone(), Vec::new(), Reforco::default(), cx);
        Ok(id)
    }

    pub fn marcar_lido(&mut self, chat: &str) {
        if let Some(c) = self.chat_mut(chat) {
            c.lido_em = agora();
        }
    }

    /// Algo novo desde a última vez que o usuário abriu a conversa.
    pub fn nao_lido(&self, chat: &Chat) -> bool {
        chat.atualizado_em > chat.lido_em && !chat.itens.is_empty() && !self.execucao(&chat.id).rodando()
    }

    /// Ajuste do dono (o coordenador ou o Mestre): vão junto os problemas do revisor e a saída da
    /// verificação desta entrega, para ele não ter de copiar (e resumir) tudo no feedback.
    fn pedir_ajuste_como(
        &mut self,
        id: u64,
        feedback: String,
        incluir: Option<Vec<String>>,
        ignorar: Vec<usize>,
        quem: &str,
        cx: &mut Context<Self>,
    ) -> Resposta {
        // No meio da verificação, o parecer guardado ainda é o da rodada anterior.
        let (anexo, junto) = self
            .tarefa(id)
            .filter(|_| !self.verificando.contains_key(&id))
            .map(|t| anexo_do_ajuste(t, incluir.as_deref(), &ignorar))
            .unwrap_or_default();
        let junto = if junto.is_empty() {
            String::new()
        } else {
            format!(" com {}", junto.join(" e "))
        };
        self.pedir_ajuste_de(id, feedback, quem, &anexo, &junto, OrigemAjuste::Dono, None, cx)
    }

    /// Uma rodada de ajuste. `origem`: o limite de rodadas do projeto (`max_rodadas`, contado desde o
    /// começo do ciclo) só vale para o dono; o do PR é `rodadas_automaticas`. `gatilho`: a rodada veio
    /// de um aviso do PR (CI, revisão), e o texto dele (de terceiros) vai como está, delimitado.
    #[allow(clippy::too_many_arguments)]
    fn pedir_ajuste_de(
        &mut self,
        id: u64,
        feedback: String,
        quem: &str,
        anexo: &str,
        junto: &str,
        origem: OrigemAjuste,
        gatilho: Option<crate::modelo::GatilhoAjuste>,
        cx: &mut Context<Self>,
    ) -> Resposta {
        let maximo = self.config.max_rodadas;
        let juntando = self.juntando.contains(&id);
        let de_projeto = self.tarefa(id).is_some_and(|t| self.de_projeto(&t.trabalhador));
        let Some(tarefa) = self.tarefa_mut(id) else {
            return Resposta::erro("Tarefa não encontrada.");
        };
        if tarefa.estado.encerrada() {
            return Resposta::erro(format!("A tarefa #{id} já está {}.", tarefa.estado.nome()));
        }
        if tarefa.estado == EstadoTarefa::Conversa {
            return Resposta::erro(format!("O chat {id} não tem pedido aberto: converse com ele por mensagem."));
        }
        if juntando {
            return Resposta::erro(format!(
                "A tarefa #{id} está sendo juntada no projeto agora; espere o resultado."
            ));
        }
        tarefa.rodada += 1;
        tarefa.estado = EstadoTarefa::Ajustando;
        tarefa.parada = None;
        tarefa.entrega_pendente = None;
        tarefa.entrega = None;
        tarefa.pronto_para_atualizar = false;
        let rodada = tarefa.rodada;
        let passou = passou_do_limite(origem, rodada, tarefa.rodada_do_ciclo, maximo);
        if let Some(pr) = tarefa.pr.as_mut() {
            pr.ajuste = gatilho.map(|gatilho| crate::modelo::AjustePr { rodada, gatilho });
        }
        let trabalhador = tarefa.trabalhador.clone();
        let dono = tarefa.coordenador.clone();
        self.tentativas_auto.remove(&trabalhador);
        let anexo = if anexo.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", anexo.trim_end())
        };
        // O ajuste do dono vem dele (e sai da fila se ele cancelar); o do usuário é uma mensagem
        // dele. O chat de projeto entrega por entregar_tarefa; o ajudante, no fim da vez.
        let fecho = if de_projeto {
            "Quando terminar, verifique e chame entregar_tarefa."
        } else {
            "Quando terminar, verifique e termine a vez com o novo relatório."
        };
        let (de, remetente) = match (de_projeto, quem) {
            (_, "usuário") => (None, None),
            (true, _) => (Some(quem.to_string()), Some(dono.clone())),
            (false, _) => (Some(self.chat(&dono).map_or_else(|| quem.to_string(), |c| c.titulo.clone())), Some(dono.clone())),
        };
        // O aviso do PR (o CI, a revisão) vai como está: ele já é o pedido, e é texto de terceiros.
        if gatilho.is_some() {
            let entrada = Entrada { de: Some("Vitral".into()), texto: feedback, terceiros: true, ..Default::default() };
            self.entregar_entrada(&trabalhador, entrada, cx);
            return Resposta::ok(format!("Ajuste enviado ao chat {id} (rodada {rodada})."));
        }
        let mensagem = format!("Ajuste pedido pelo {quem} (rodada {rodada}):\n{feedback}{anexo}\n\n{fecho}");
        self.entregar_entrada(&trabalhador, Entrada { de, texto: mensagem, remetente, ..Default::default() }, cx);
        if passou {
            Resposta::ok(format!(
                "Ajuste enviado{junto}, mas esta já é a rodada {rodada} (limite combinado: {maximo}). \
                 Se não resolver, explique o impasse ao usuário."
            ))
        } else {
            Resposta::ok(format!(
                "Ajuste enviado (rodada {rodada}){junto}. O novo relatório chega sozinho."
            ))
        }
    }

    /// O pai cancela o ajudante: o que ele mandou sai da fila, o ajudante para e fica cancelado. A
    /// cópia fica até o usuário limpar.
    fn cancelar_tarefa(&mut self, id: u64, motivo: String, cx: &mut Context<Self>) -> Resposta {
        if self.juntando.contains(&id) {
            return Resposta::erro(format!("O ajudante #{id} está sendo juntado agora; espere o resultado."));
        }
        let Some(tarefa) = self.tarefa_mut(id) else {
            return Resposta::erro(format!("Não há ajudante #{id}."));
        };
        if tarefa.estado == EstadoTarefa::Aprovada {
            return Resposta::erro(format!("O ajudante #{id} já foi aprovado e juntado."));
        }
        tarefa.estado = EstadoTarefa::Cancelada;
        (tarefa.progresso, tarefa.entrega_pendente) = (None, None);
        let (ajudante, dono) = (tarefa.trabalhador.clone(), tarefa.coordenador.clone());
        if let Some(fila) = self.filas.get_mut(&ajudante) {
            mestre::tirar_do_dono(fila, &dono);
            if fila.is_empty() {
                self.filas.remove(&ajudante);
            }
        }
        self.parar(&ajudante, cx);
        let motivo = if motivo.is_empty() { "sem motivo informado".into() } else { motivo };
        let de = self.chat(&dono).map_or_else(|| "o chat dele".to_string(), |c| format!("o chat \"{}\"", c.titulo));
        self.avisar(&ajudante, &format!("Este ajudante foi cancelado por {de}: {motivo}"), false);
        Resposta::ok(format!(
            "Ajudante #{id} cancelado. A cópia dele fica guardada até o usuário limpar (Configurações, cópias paradas)."
        ))
    }

    /// Aprova a entrega. No chat de projeto, junta a cópia na origem (`juntar_chat`). No ajudante,
    /// junta a cópia dele na pasta do pai, num único `na_vez`: antes, um ponto com o que o pai tem
    /// sem commit; depois, a cópia do ajudante sai e o chat dele vai para as arquivadas. `ressalva`
    /// é o que se aprovou mesmo assim (verificação falhando, problema grave): vai no commit e aparece
    /// para o usuário. `autor`: quem aprovou ("usuario", "mestre", "vitral" ou o id do pai).
    /// `entrega`: o `aprovar_tarefa{entrega}` do Mestre; `None` vale o modo da pasta.
    #[allow(clippy::too_many_arguments)]
    fn aprovar_tarefa(
        &mut self,
        id: u64,
        nota: String,
        ressalva: Option<String>,
        resposta: Sender<Resposta>,
        autor: &str,
        entrega: Option<crate::modelo::ModoEntrega>,
        cx: &mut Context<Self>,
    ) {
        if self.tarefa(id).is_some_and(|t| self.de_projeto(&t.trabalhador)) {
            // No modo PR, aprovar é publicar: a junção na pasta só acontece quando o PR mesclar.
            let por_pr = self.tarefa(id).is_some_and(|t| t.tem_copia() && entrega.map_or(self.entrega_por_pr(t), |e| e == crate::modelo::ModoEntrega::Pr));
            if por_pr {
                if let Some(recusa) = self.recusa_aprovar(id) {
                    let _ = resposta.try_send(Resposta::erro(recusa));
                } else if self.tarefa(id).is_some_and(|t| !self.entrega_por_pr(t) && t.pr_aberto().is_none()) {
                    let _ = resposta.try_send(Resposta::erro("A entrega desta pasta é Juntar aqui: aprove sem entrega=\"pr\"."));
                } else {
                    self.aprovar_pelo_pr(id, autor, resposta, cx);
                }
                return;
            }
            self.juntar_chat(id, nota, ressalva, false, autor, resposta, cx);
            return;
        }
        if let Some(recusa) = self.recusa_aprovar(id) {
            let _ = resposta.try_send(Resposta::erro(recusa));
            return;
        }
        let Some(tarefa) = self.tarefa(id).cloned() else {
            return;
        };
        if tarefa.copia_apagada() {
            let _ = resposta.try_send(Resposta::erro(format!(
                "O ajudante #{id} está {} e a cópia dele já foi apagada: não há o que juntar.",
                tarefa.estado.nome()
            )));
            return;
        }
        let (Some(pasta), Some(branch), Some(base)) = (
            tarefa.worktree.clone(),
            tarefa.branch.clone(),
            tarefa.base.clone(),
        ) else {
            let _ = resposta.try_send(Resposta::erro(format!("O ajudante #{id} não tem cópia: não há o que juntar.")));
            return;
        };
        let pasta_do_pai = self.origem_da_tarefa(&tarefa);
        if !pasta_do_pai.is_dir() {
            let _ = resposta.try_send(Resposta::erro(format!(
                "A pasta em que o ajudante #{id} junta ({}) não foi encontrada.",
                pasta_do_pai.display()
            )));
            return;
        }
        let area = self.area_da_copia(&tarefa);
        let mut mensagem = if nota.is_empty() {
            format!("{} (ajudante #{id})", tarefa.titulo)
        } else {
            format!("{}\n\n{nota}\n\nAjudante #{id} do Vitral", tarefa.titulo)
        };
        if let Some(ressalva) = &ressalva {
            mensagem.push_str(&format!("\n\nAprovado apesar de: {ressalva}"));
        }
        let lembrete = if ressalva.is_some() {
            " A ressalva ficou no commit e o usuário foi avisado; cite-a quando entregar."
        } else {
            ""
        };
        let ponto = format!("Vitral: ponto antes de juntar o ajudante #{id}");
        self.juntando.insert(id);
        cx.notify();
        let pontos = self.pontos_so_do_chat(&tarefa.trabalhador);
        let grupo = tarefa.grupo();
        let excluir = tarefa.arquivos_copiados.clone();
        let (ajudante, pai) = (tarefa.trabalhador.clone(), tarefa.coordenador.clone());
        cx.spawn(async move |motor, cx| {
            let alvo = pasta_do_pai.clone();
            let resultado = cx
                .background_spawn(async move {
                    // Uma junção por vez no repositório. O ponto, a junção e a limpeza ficam na mesma
                    // vez, sem pedi-la de novo (ela não é reentrante).
                    git::na_vez_do_projeto(&alvo, || {
                        git::checkpoint(&alvo, &ponto)
                            .map_err(|e| e.context(format!("não deu para guardar num commit o que {} tem", alvo.display())))?;
                        let juncao = git::aprovar(&alvo, &pasta, &branch, &base, &mensagem, &excluir);
                        if matches!(juncao, Ok(git::Juncao::Juntou { .. } | git::Juncao::SemMudancas)) {
                            let _ = git::limpar_copia(&alvo, &pasta, &area, &branch, Some(&grupo), &pontos, false);
                        }
                        juncao
                    })
                })
                .await;
            let _ = motor.update(cx, |motor, cx| {
                motor.juntando.remove(&id);
                let onde = pasta_do_pai.display();
                let juntou = |motor: &mut Motor, cx: &mut Context<Motor>| {
                    motor.marcar_aprovada(id, ressalva.clone());
                    motor.arquivar_varios(std::slice::from_ref(&ajudante), cx);
                    if let Some(n) = motor.chat(&pai).and_then(|c| c.tarefa) {
                        motor.contar_mudancas(n, cx);
                    }
                };
                let texto = match resultado {
                    Ok(git::Juncao::Juntou { commits }) => {
                        juntou(motor, cx);
                        Resposta::ok(format!(
                            "Ajudante #{id} aprovado e juntado na sua pasta ({onde}):\n{commits}\nA cópia dele saiu e o chat \
                             dele foi para as arquivadas.{lembrete}"
                        ))
                    }
                    Ok(git::Juncao::SemMudancas) => {
                        juntou(motor, cx);
                        Resposta::ok(format!("Ajudante #{id} aprovado; ele não tinha mudanças para juntar.{lembrete}"))
                    }
                    Ok(git::Juncao::ProjetoSujo { arquivos }) => Resposta::erro(format!(
                        "Não juntei o ajudante #{id}: a sua pasta ({onde}) ainda tem mudanças sem commit em: {}. Tente de \
                         novo; se continuar, avise o usuário.",
                        arquivos.join(", ")
                    )),
                    Ok(git::Juncao::SoltosNoProjeto { arquivos }) => Resposta::erro(format!(
                        "Não juntei o ajudante #{id}: ele cria arquivos que já existem soltos (fora do git) na sua pasta \
                         ({onde}): {}. Apague ou mova esses arquivos (ou peça ao ajudante outro nome) e aprove de novo.",
                        arquivos.join(", ")
                    )),
                    Ok(git::Juncao::Conflito { arquivos, detalhe, alvo }) => Resposta::erro(format!(
                        "Conflito com o que já está na sua pasta; nada foi juntado. Arquivos: {}. Detalhe: {detalhe}\n\
                         Peça ao ajudante (pedir_ajuste {id}) para rodar `git merge {alvo}` na cópia dele, resolver mantendo \
                         as duas mudanças, fazer commit e rodar os testes. Depois aprove de novo.",
                        if arquivos.is_empty() { "?".into() } else { arquivos.join(", ") }
                    )),
                    Ok(git::Juncao::Ocupado { detalhe }) => Resposta::erro(format!(
                        "Nada foi juntado: outro git estava usando a pasta ({}). Não é conflito: aprove de novo em \
                         instantes. Se continuar, pode ser um .git/index.lock que sobrou de um git que caiu; avise o usuário.",
                        detalhe.trim()
                    )),
                    Err(erro) => Resposta::erro(format!("Não deu para aprovar: {erro:#}")),
                };
                let _ = resposta.try_send(texto);
                motor.salvar();
                cx.notify();
            });
        })
        .detach();
    }

    /// "Juntar" de um chat de projeto: a cópia entra na origem e continua, andando até o HEAD novo,
    /// que vira a base (a próxima junção leva só o que é novo). Nas formas diretas, as mudanças já
    /// estão na pasta e só a rodada fecha. `aceitar_ramo` junta mesmo com a origem em outro branch.
    fn juntar_chat(
        &mut self,
        id: u64,
        nota: String,
        ressalva: Option<String>,
        aceitar_ramo: bool,
        autor: &str,
        resposta: Sender<Resposta>,
        cx: &mut Context<Self>,
    ) {
        if let Some(recusa) = self.recusa_aprovar(id).filter(|_| self.tarefa(id).is_some_and(|t| t.tem_copia())) {
            let _ = resposta.try_send(Resposta::erro(recusa));
            return;
        }
        let Some(t) = self.tarefa(id).cloned() else {
            let _ = resposta.try_send(Resposta::erro("Tarefa não encontrada."));
            return;
        };
        let origem = self.origem_da_tarefa(&t);
        if !t.tem_copia() {
            self.fechar_rodada(id, None, ressalva);
            self.pedido_juntado(&t, &origem.display().to_string(), autor, cx);
            let _ = resposta.try_send(Resposta::ok(format!(
                "As mudanças deste chat já estão em {}; a rodada foi fechada.",
                mestre::com_til(&origem)
            )));
            self.salvar();
            cx.notify();
            return;
        }
        if !origem.is_dir() {
            let _ = resposta.try_send(Resposta::erro(format!(
                "A pasta {} não foi encontrada: use Localizar pasta…",
                mestre::com_til(&origem)
            )));
            return;
        }
        let (Some(copia), Some(branch), Some(base)) = (t.worktree.clone(), t.branch.clone(), t.base.clone()) else {
            let _ = resposta.try_send(Resposta::erro("Este chat ainda não tem cópia: não há o que juntar."));
            return;
        };
        let chat = t.trabalhador.clone();
        let titulo = self.chat(&chat).map_or_else(|| t.titulo.clone(), |c| c.titulo.clone());
        let mut mensagem = match t.dono() {
            None => format!("{titulo} (junção pelo usuário)"),
            Some(_) if nota.trim().is_empty() => titulo.clone(),
            Some(_) => format!("{titulo}\n\n{}", nota.trim()),
        };
        mensagem.push_str(&format!("\n\nChat {id} do Vitral"));
        if let Some(ressalva) = &ressalva {
            mensagem.push_str(&format!("\n\nAprovada apesar de: {ressalva}"));
        }
        let ramo = t.ramo_alvo.clone().filter(|_| !aceitar_ramo);
        // Gravado antes do merge: uma queda no meio é conferida ao abrir.
        if let Some(t) = self.tarefa_mut(id) {
            t.juntando_desde = agora();
        }
        self.juntando.insert(id);
        self.salvar();
        cx.notify();
        let grupo = format!("t{id}");
        let autor = autor.to_string();
        // Os arquivos copiados da origem (o .env) nunca entram no commit.
        let excluir = t.arquivos_copiados.clone();
        cx.spawn(async move |motor, cx| {
            let origem_bg = origem.clone();
            let resultado = cx
                .background_spawn(async move {
                    git::na_vez_do_projeto(&origem_bg, || {
                        git::juntar(&origem_bg, &copia, &branch, &base, &mensagem, ramo.as_deref(), &grupo, &excluir)
                    })
                })
                .await;
            let _ = motor.update(cx, |m, cx| {
                m.juntando.remove(&id);
                let lugar = mestre::com_til(&origem);
                let texto = match resultado {
                    Ok(git::Juntada::Feita { commits, base, andou }) => {
                        m.fechar_rodada(id, Some(base), ressalva);
                        m.pedido_juntado(&t, &lugar, &autor, cx);
                        m.avisar(&chat, &format!("{JUNTOU} {lugar}."), false);
                        if !andou {
                            m.avisar(
                                &chat,
                                &format!("A cópia não pôde ser atualizada com {lugar}; o próximo diff pode mostrar o que já entrou."),
                                false,
                            );
                        }
                        // O Claude fica sabendo na próxima mensagem: a cópia está em dia com a origem.
                        let nota = Entrada {
                            de: Some("Vitral".into()),
                            texto: format!("{JUNTOU} {lugar}: a cópia está em dia com ela, e o próximo diff conta daqui."),
                            nota: true,
                            registrada: true,
                            ..Default::default()
                        };
                        m.filas.entry(chat.clone()).or_default().push(nota);
                        m.sincronizar_outras(id, origem.clone(), cx);
                        match commits {
                            Some(commits) => Resposta::ok(format!("As mudanças do chat {id} entraram em {lugar}:\n{commits}")),
                            None => Resposta::ok(format!("O chat {id} não tinha mudanças para juntar; a cópia está em dia com {lugar}.")),
                        }
                    }
                    Ok(git::Juntada::RamoTrocado { alvo, atual }) => Resposta::erro(format!(
                        "{RAMO_TROCADO} {lugar}: a cópia saiu de {alvo} e a pasta está em {atual}. Juntar em {atual}?"
                    )),
                    Ok(git::Juntada::OutroRepositorio) => Resposta::erro(format!(
                        "A cópia não pertence mais ao repositório de {lugar} (pasta movida?): use Localizar pasta…"
                    )),
                    Ok(git::Juntada::Falhou(git::Juncao::ProjetoSujo { arquivos })) => Resposta::erro(format!(
                        "A pasta {lugar} tem mudanças sem commit em: {}; faça commit ou stash e junte de novo.",
                        arquivos.join(", ")
                    )),
                    Ok(git::Juntada::Falhou(git::Juncao::SoltosNoProjeto { arquivos })) => Resposta::erro(format!(
                        "A junção traria arquivos que já existem soltos (fora do git) em {lugar}: {}. Mova ou apague \
                         esses arquivos (ou faça commit deles) e junte de novo.",
                        arquivos.join(", ")
                    )),
                    Ok(git::Juntada::Falhou(git::Juncao::Conflito { arquivos, alvo, .. })) => {
                        let arquivos = if arquivos.is_empty() { "?".to_string() } else { arquivos.join(", ") };
                        m.registrar("conflito", "vitral", &chat, &t.projeto, Some(id), &format!("conflito ao juntar em {lugar}: {arquivos}"));
                        if t.dono().is_some() {
                            // Com dono, ele decide (pedir_ajuste com o merge).
                            Resposta::erro(format!(
                                "Conflito com o que já entrou em {lugar} (arquivos {arquivos}): nada foi juntado. Peça ao chat \
                                 (pedir_ajuste {id}) para rodar `git merge {alvo}` na cópia dele, resolver mantendo as duas \
                                 mudanças, fazer commit e rodar os testes; depois aprove de novo."
                            ))
                        } else {
                            // Sem dono, o próprio chat resolve na cópia dele.
                            let pedido = format!(
                                "Deu conflito ao juntar com o que já entrou em {lugar} (arquivos {arquivos}). Rode `git \
                                 merge {alvo}` na sua cópia, resolva mantendo as duas mudanças, faça commit e rode os \
                                 testes; depois avise."
                            );
                            m.entregar(&chat, Some("Vitral".into()), pedido, cx);
                            Resposta::erro(format!(
                                "Conflito com o que já entrou em {lugar} (arquivos {arquivos}): nada foi juntado. O chat foi \
                                 avisado para resolver na cópia; depois junte de novo."
                            ))
                        }
                    }
                    Ok(git::Juntada::Falhou(git::Juncao::Ocupado { detalhe })) => Resposta::erro(format!(
                        "Nada foi juntado: outro git estava usando {lugar} ({}). Tente de novo em instantes.",
                        detalhe.trim()
                    )),
                    Ok(git::Juntada::Falhou(_)) => Resposta::erro("Nada foi juntado.".to_string()),
                    Err(erro) => Resposta::erro(format!("Não deu para juntar: {erro:#}")),
                };
                if let Some(t) = m.tarefa_mut(id) {
                    t.juntando_desde = 0;
                }
                let _ = resposta.try_send(texto);
                m.salvar();
                m.entregar_fila(&chat, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Junta mesmo com a origem em outro branch que o de quando a cópia nasceu (a confirmação do
    /// "Você trocou de branch").
    pub fn juntar_no_ramo_atual(&mut self, id: u64, cx: &mut Context<Self>) -> smol::channel::Receiver<Resposta> {
        let (envia, recebe) = smol::channel::bounded(1);
        self.juntar_chat(id, String::new(), None, true, "usuario", envia, cx);
        recebe
    }

    /// Fecha a rodada de um chat de projeto (juntou, ou nas formas diretas): volta a conversar, sem
    /// dono, com a base nova da cópia.
    fn fechar_rodada(&mut self, id: u64, base: Option<String>, ressalva: Option<String>) {
        let Some(t) = self.tarefa_mut(id) else {
            return;
        };
        t.estado = EstadoTarefa::Conversa;
        t.rodada += 1;
        t.coordenador.clear();
        t.pedido_da_issue = false;
        (t.revisao, t.entrega, t.mudancas, t.entrega_pendente) = (None, None, None, None);
        (t.parada, t.progresso, t.juntando_desde) = (None, None, 0);
        t.ressalva = ressalva;
        t.atualizada_em = agora();
        if let Some(base) = base {
            t.base = Some(base);
        }
    }

    /// Depois de uma junção, já fora da vez do repositório: as outras cópias do mesmo repositório
    /// que não têm nada só delas andam com a origem, uma de cada vez (as outras andam no próximo
    /// turno). Os turnos delas esperam enquanto isso.
    fn sincronizar_outras(&mut self, juntada: u64, origem: PathBuf, cx: &mut Context<Self>) {
        let candidatas: Vec<(u64, String, PathBuf, PathBuf)> = self
            .tarefas
            .iter()
            .filter(|t| t.id != juntada && t.tem_copia() && t.preparada && !t.estado.encerrada())
            .filter(|t| t.juntando_desde == 0 && !self.juntando.contains(&t.id) && !self.sincronizando.contains(&t.id))
            .filter(|t| self.de_projeto(&t.trabalhador) && self.chat_livre(&t.trabalhador) && self.na_fila(&t.trabalhador) == 0)
            .filter_map(|t| Some((t.id, t.trabalhador.clone(), t.worktree.clone()?, self.origem_da_tarefa(t))))
            .collect();
        if candidatas.is_empty() {
            return;
        }
        self.sincronizando.extend(candidatas.iter().map(|c| c.0));
        cx.spawn(async move |motor, cx| {
            let candidatas = cx
                .background_spawn(async move {
                    let repositorio = git::raiz_comum(&origem);
                    for (_, _, copia, sua) in &candidatas {
                        if repositorio.is_some() && git::raiz_comum(sua) == repositorio && copia.is_dir() {
                            let _ = git::na_vez_do_projeto(sua, || git::sincronizar_copia(copia, sua));
                        }
                    }
                    candidatas
                })
                .await;
            let _ = motor.update(cx, |m, cx| {
                for (id, chat, ..) in candidatas {
                    m.sincronizando.remove(&id);
                    m.entregar_fila(&chat, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Ao abrir: as junções que o Vitral fechado (ou a queda) cortou no meio. O que a junção deixou
    /// na origem é desfeito; se as mudanças já tinham entrado, a rodada fecha como se tivesse
    /// terminado, e senão o chat fica como estava, com o aviso para juntar de novo.
    fn recuperar_juncoes(&mut self, cx: &mut Context<Self>) {
        let cortadas: Vec<(u64, String, PathBuf, PathBuf, String)> = self
            .tarefas
            .iter()
            .filter(|t| t.juntando_desde > 0)
            .filter_map(|t| Some((t.id, t.trabalhador.clone(), self.origem_da_tarefa(t), t.worktree.clone()?, t.branch.clone()?)))
            .collect();
        for t in self.tarefas.iter_mut().filter(|t| t.juntando_desde > 0 && (t.worktree.is_none() || t.branch.is_none())) {
            t.juntando_desde = 0;
        }
        for (id, chat, origem, copia, branch) in cortadas {
            cx.spawn(async move |motor, cx| {
                let origem_bg = origem.clone();
                let achado = cx
                    .background_spawn(async move {
                        git::na_vez_do_projeto(&origem_bg, || {
                            match git::recuperar_juncao(&origem_bg, &copia, &branch)? {
                                git::JuncaoCortada::Entrou => Ok(Some(git::depois_de_juntar(&copia, &origem_bg)?.0)),
                                git::JuncaoCortada::NaoEntrou => Ok::<_, anyhow::Error>(None),
                            }
                        })
                    })
                    .await;
                let _ = motor.update(cx, |m, cx| {
                    let lugar = mestre::com_til(&origem);
                    match achado {
                        Ok(Some(base)) => {
                            m.fechar_rodada(id, Some(base), None);
                            m.avisar(&chat, &format!("{JUNTOU} {lugar} (a junção foi interrompida, mas já tinha terminado)."), false);
                        }
                        Ok(None) => m.avisar(
                            &chat,
                            &format!("A junção do chat {id} foi interrompida; nada entrou em {lugar}. Junte de novo."),
                            true,
                        ),
                        Err(erro) => m.avisar(
                            &chat,
                            &format!("A junção do chat {id} foi interrompida e não deu para conferir {lugar} ({erro:#}). Junte de novo."),
                            true,
                        ),
                    }
                    if let Some(t) = m.tarefa_mut(id) {
                        t.juntando_desde = 0;
                    }
                    m.salvar();
                    m.entregar_fila(&chat, cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// O fim da vez de um chat de projeto, em segundo plano: na cópia, conta o que falta juntar; na
    /// pasta própria, faz o commit da vez (e poda os pontos de volta); sem git, nada.
    fn depois_da_vez(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let Some(t) = self.chat(chat_id).filter(|c| c.papel == Papel::Projeto).and_then(|c| c.tarefa).and_then(|t| self.tarefa(t)).cloned() else {
            return;
        };
        let origem = self.origem_da_tarefa(&t);
        let id = t.id;
        match t.forma {
            Forma::Copia => self.contar_mudancas(id, cx),
            Forma::DiretaGit => {
                let mensagem = format!("Vitral: {}", self.chat(chat_id).map_or(t.titulo.as_str(), |c| c.titulo.as_str()));
                cx.background_spawn(async move {
                    if !origem.is_dir() {
                        return;
                    }
                    git::na_vez_do_projeto(&origem, || {
                        // A pasta própria sem commit (uma queda no meio da criação) ganha o repositório.
                        if !git::usa_git(&origem) {
                            let _ = git::iniciar_repositorio(&origem);
                        }
                        if let Err(erro) = git::commit_de_turno(&origem, &mensagem) {
                            eprintln!("vitral: o commit da vez em {} falhou: {erro:#}", origem.display());
                        }
                        git::podar_pontos(&origem, &format!("t{id}"), git::PONTOS_GUARDADOS);
                    });
                })
                .detach();
            }
            Forma::Direta => {}
        }
    }

    /// O que a cópia do chat tem para juntar (`Tarefa.mudancas`), contado em segundo plano: alimenta
    /// a barra "Para juntar".
    fn contar_mudancas(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).filter(|t| t.tem_copia()) else {
            return;
        };
        let origem = self.origem_da_tarefa(t);
        let (Some(copia), Some(base)) = (t.worktree.clone(), t.base.clone()) else {
            return;
        };
        cx.spawn(async move |motor, cx| {
            let contagem = cx
                .background_spawn(async move {
                    if !copia.is_dir() {
                        return None;
                    }
                    let base = if origem.is_dir() { git::base_efetiva(&origem, &copia, &base) } else { base };
                    git::contar_mudancas(&copia, &base).ok()
                })
                .await;
            let _ = motor.update(cx, |m, cx| {
                if let (Some((arquivos, mais, menos)), Some(t)) = (contagem, m.tarefa_mut(id)) {
                    t.mudancas = Some(crate::modelo::Mudancas { arquivos, mais, menos, em: agora() });
                    m.salvar();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// A tarefa entrou na pasta: com ressalva, quem a pediu vê na conversa o que foi ignorado.
    fn marcar_aprovada(&mut self, id: u64, ressalva: Option<String>) {
        let Some(t) = self.tarefa_mut(id) else {
            return;
        };
        t.estado = EstadoTarefa::Aprovada;
        // A junção apaga a cópia; sem git, o trabalhador rodou no próprio projeto.
        t.copia_limpa = t.worktree.is_some();
        t.worktree = None;
        t.progresso = None;
        t.parada = None;
        t.ressalva = ressalva.clone();
        let coordenador = t.coordenador.clone();
        if let Some(ressalva) = ressalva {
            self.avisar(&coordenador, &format!("O ajudante #{id} foi aprovado apesar de: {ressalva}"), true);
        }
    }

    // ---------- textos ----------

    fn avisar(&mut self, chat: &str, texto: &str, erro: bool) {
        self.chats_sujos.insert(chat.to_string());
        if let Some(c) = self.chat_mut(chat) {
            c.itens.push(Item::Aviso {
                texto: texto.to_string(),
                erro,
            });
        }
    }

    /// Notificação do sistema, numa thread que espera o clique: a primeira linha do
    /// `notify-send` é o id (para a próxima do mesmo chat substituir esta), a seguinte é a ação.
    fn notificar(&self, chat: &Chat, texto: &str) {
        let titulo = if chat.papel == Papel::Mestre {
            chat.titulo.clone()
        } else {
            format!("{} · {}", self.nome_do_projeto(&chat.projeto), chat.titulo)
        };
        let corpo = texto.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().to_string();
        // No Windows, o aviso do sistema (toast) com o AppUserModelID do Vitral: o do mesmo chat
        // substitui o anterior, e o clique volta como `Sinal::Abrir`.
        #[cfg(windows)]
        {
            let aviso = gpui_kit::SystemNotification {
                tag: chat.id.clone().into(),
                title: titulo.into(),
                body: corpo.into(),
                actions: Vec::new(),
            };
            let _ = self.sinais.try_send(Sinal::Aviso(aviso));
        }
        #[cfg(not(windows))]
        let (avisos, sinais, id) = (self.avisos.clone(), self.sinais.clone(), chat.id.clone());
        #[cfg(not(windows))]
        std::thread::spawn(move || {
            use std::io::BufRead as _;
            let travar = || avisos.lock().unwrap_or_else(|e| e.into_inner());
            let (saida, marca) = {
                let mut todos = travar();
                let aviso = todos.entry(id.clone()).or_default();
                if let Some(mut velho) = aviso.filho.take() {
                    let _ = velho.kill();
                    let _ = velho.wait();
                }
                let filho = crate::processos::novo_comando("notify-send")
                    .args(argumentos_do_aviso(&titulo, &corpo, aviso.id))
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                let Ok(mut filho) = filho else {
                    return;
                };
                aviso.marca += 1;
                let saida = filho.stdout.take();
                aviso.filho = Some(filho);
                (saida, aviso.marca)
            };
            let Some(saida) = saida else {
                return;
            };
            let mut linhas = std::io::BufReader::new(saida).lines().map_while(Result::ok);
            if let Some(numero) = linhas.next().and_then(|l| l.trim().parse().ok())
                && let Some(aviso) = travar().get_mut(&id).filter(|a| a.marca == marca)
            {
                aviso.id = Some(numero);
            }
            if linhas.next().is_some_and(|acao| acao.trim() == "default") {
                let _ = sinais.send_blocking(Sinal::Abrir(id.clone()));
            }
            if let Some(aviso) = travar().get_mut(&id).filter(|a| a.marca == marca)
                && let Some(mut filho) = aviso.filho.take()
            {
                let _ = filho.wait();
            }
        });
    }

    fn salvar(&self) {
        if self.somente_leitura.is_some() {
            return;
        }
        let estado = EstadoRef {
            versao: crate::modelo::VERSAO_ESTADO,
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
            resto: &self.resto_estado,
        };
        if let Err(erro) = self.dados.salvar_estado(&estado) {
            eprintln!("vitral: não salvou o estado: {erro:#}");
        }
    }

    fn gravar_config(&self) {
        if self.somente_leitura.is_some() {
            return;
        }
        if let Err(erro) = self.dados.salvar_config(&self.config) {
            eprintln!("vitral: não salvou a configuração: {erro:#}");
        }
    }

    /// Grava as mensagens dos chats que mudaram desde a última passada e confere se as gravações
    /// estão dando certo: com uma falha, nada novo começa e ela é tentada de novo a cada passada;
    /// quando volta, o que esperou na fila entra.
    fn gravar_sujos(&mut self, cx: &mut Context<Self>) {
        if self.somente_leitura.is_some() {
            return;
        }
        for id in std::mem::take(&mut self.chats_sujos) {
            if let Some(c) = self.chat(&id) {
                let _ = self.dados.salvar_chat(c);
            }
        }
        match (self.dados.falha_de_gravacao(), self.falha_gravacao.is_some()) {
            (Some(erro), false) => {
                self.falha_gravacao = Some((erro, agora()));
                cx.notify();
            }
            (Some(erro), true) => {
                if let Some(falha) = &mut self.falha_gravacao {
                    falha.0 = erro;
                }
                self.dados.gravar_de_novo();
            }
            (None, true) => {
                self.falha_gravacao = None;
                self.entregar_filas_pendentes(cx);
                cx.notify();
            }
            (None, false) => {}
        }
    }

    /// Depois de voltar a gravar: os chats parados com mensagem na fila começam.
    fn entregar_filas_pendentes(&mut self, cx: &mut Context<Self>) {
        let parados: Vec<String> = self
            .filas
            .iter()
            .filter(|(id, fila)| fila.iter().any(|e| !e.nota) && self.chat_livre(id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in parados {
            self.entregar_fila(&id, cx);
        }
    }

    /// A pasta onde as cópias do projeto da tarefa ficam: a nova (`dados/copias/<projeto>`) ou, numa
    /// cópia do Vitral antigo, a `.vitral/worktrees/<projeto>` dela. A limpeza só apaga à mão uma
    /// pasta que está nela.
    fn area_da_copia(&self, t: &Tarefa) -> PathBuf {
        area_da_copia(&self.dados.pasta.join("copias"), t)
    }

    /// A tarefa do chat ganha uma faixa de portas livre, se não tem.
    fn dar_porta(&mut self, chat: &str) {
        let Some(id) = self.chat(chat).and_then(|c| c.tarefa) else {
            return;
        };
        let livre = dados::porta_livre(&self.tarefas).unwrap_or(0);
        if let Some(t) = self.tarefa_mut(id)
            && t.porta == 0
            && !t.estado.encerrada()
        {
            t.porta = livre;
        }
    }

    fn soltar_porta(&mut self, chat: &str) {
        if let Some(id) = self.chat(chat).and_then(|c| c.tarefa)
            && let Some(t) = self.tarefa_mut(id)
        {
            t.porta = 0;
        }
    }

}

/// Lista de textos de um argumento MCP; aceita também um texto só, ou a lista escrita como JSON.
/// O fetch de `<remoto>/<base>` antes da cópia do modo PR, na vez da rede do repositório (fora da
/// trava do projeto). Os avisos vão para o chat: commits de `<base>` que só existem nesta máquina, ou
/// sem rede (a cópia parte do que já estava baixado).
async fn buscar_ponto_de_partida(e: &dyn crate::remoto::Executor, projeto: &Path, ponto: &str) -> Vec<String> {
    let Some((remoto, base)) = ponto.split_once('/') else {
        return Vec::new();
    };
    let resultado = {
        let _vez = git::na_vez_da_rede(projeto).await;
        git::buscar(e, projeto, remoto, &[base])
    };
    let mut avisos = Vec::new();
    match resultado {
        Ok(()) => {
            let fora = git::commits_locais_fora(projeto, remoto, base);
            if fora > 0 {
                avisos.push(format!(
                    "{} de {base} nesta máquina ainda não {} no GitHub; a cópia partiu do GitHub.",
                    crate::ui::componentes::contagem(fora as usize, "commit", "commits"),
                    if fora == 1 { "está" } else { "estão" }
                ));
            }
        }
        Err(_) => avisos.push(format!("Não consegui atualizar {base} do GitHub; parti do que já estava baixado.")),
    }
    avisos
}

fn lista_de_textos(valor: &Value) -> Vec<String> {
    match valor {
        Value::Array(lista) => lista.iter().filter_map(|v| v.as_str()).map(str::to_string).collect(),
        Value::String(texto) => match serde_json::from_str::<Vec<String>>(texto) {
            Ok(lista) => lista,
            Err(_) => vec![texto.clone()],
        },
        _ => Vec::new(),
    }
}

fn titulo_de(texto: &str) -> String {
    let linha = texto
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("Conversa");
    let palavras: Vec<&str> = linha.split_whitespace().take(7).collect();
    let mut titulo = palavras.join(" ");
    if titulo.chars().count() > 48 {
        titulo = format!("{}…", titulo.chars().take(47).collect::<String>());
    }
    titulo
}

/// Ao abrir: marca o que o Vitral fechado (ou uma queda de energia) deixou no meio. Tarefas e
/// turnos cortados ficam parados até alguém mandar retomar, com um aviso só; as entregas cortadas
/// na verificação e as preparações de cópia cortadas voltam nas listas, para refazer sozinho uma
/// vez.
fn marcar_interrompidas(
    chats: &mut [Chat],
    tarefas: &mut [Tarefa],
    filas: &Filas,
) -> (VecDeque<u64>, VecDeque<u64>) {
    let mut avisos: Vec<(String, &str)> = Vec::new();
    let mut entregas = VecDeque::new();
    let mut preparos = VecDeque::new();
    for tarefa in tarefas.iter_mut() {
        let trabalhador = chats.iter().find(|c| c.id == tarefa.trabalhador);
        let refazer = trabalhador.map_or(!tarefa.preparada, |c| precisa_preparar(tarefa, c, filas));
        if tarefa.estado.ativa() && tarefa.parada.is_none() && refazer {
            tarefa.progresso = Some("esperando para preparar a cópia de novo".into());
            preparos.push_back(tarefa.id);
            avisos.push((
                tarefa.trabalhador.clone(),
                "O Vitral fechou enquanto a cópia deste ajudante era preparada. Ela é preparada de \
                 novo agora, e o pedido vai para ele quando ficar pronta.",
            ));
        } else if tarefa.estado == EstadoTarefa::Conversa {
            // O chat de projeto: a mensagem esperava a cópia. Uma vez só; caindo de novo na
            // refeita, ela fica guardada e a próxima mensagem prepara.
            tarefa.progresso = None;
            if refazer && tarefa.parada.is_none() {
                tarefa.progresso = Some("esperando para preparar a cópia de novo".into());
                preparos.push_back(tarefa.id);
                avisos.push((
                    tarefa.trabalhador.clone(),
                    "O Vitral fechou enquanto a cópia deste chat era preparada. Ela é preparada de novo \
                     agora, e a mensagem entra quando ficar pronta.",
                ));
            }
        } else if tarefa.estado.ativa() && tarefa.parada.is_none() {
            let comecou = trabalhador.is_some_and(recebeu_mensagem);
            let na_fila = filas.get(&tarefa.trabalhador).is_some_and(|f| !f.is_empty());
            let (motivo, aviso) = if comecou {
                (
                    PARADA_FECHOU,
                    "O Vitral foi fechado no meio deste pedido. Use Retomar (ou mande uma mensagem) \
                     para continuar; o relatório vai para quem pediu quando terminar.",
                )
            } else if na_fila {
                (
                    "esperava vaga quando o Vitral fechou",
                    "Este pedido esperava vaga quando o Vitral fechou e ainda não começou. Use \
                     Retomar para começar.",
                )
            } else {
                (
                    "o Vitral fechou durante a preparação da cópia",
                    "O Vitral fechou enquanto a cópia era preparada, e o pedido não chegou a este \
                     chat. Use Retomar para mandar.",
                )
            };
            tarefa.parada = Some(motivo.into());
            tarefa.progresso = None;
            avisos.push((tarefa.trabalhador.clone(), aviso));
        } else if tarefa.estado.ativa() {
            // Já estava parada (ou caiu de novo na preparação refeita): nada dela roda ao abrir.
            tarefa.progresso = None;
        }
        // Versões antigas não gravavam a entrega pendente, só o progresso.
        let antiga = tarefa.progresso.as_deref() == Some(VERIFICANDO);
        if tarefa.estado == EstadoTarefa::EmRevisao && (tarefa.entrega_pendente.is_some() || antiga) {
            tarefa.entrega_pendente.get_or_insert(false);
            if tarefa.parada.is_none() {
                tarefa.progresso = Some("retomando a verificação (o Vitral fechou no meio)".into());
                entregas.push_back(tarefa.id);
            } else {
                tarefa.progresso = None;
            }
        }
    }
    let ativas: HashSet<u64> = tarefas
        .iter()
        .filter(|t| t.estado.ativa())
        .map(|t| t.id)
        .collect();
    let cortado = "O Vitral foi fechado no meio deste turno. Use Retomar para o Claude continuar de \
                   onde parou.";
    for chat in chats.iter_mut() {
        for (_, aviso) in avisos.iter().filter(|(id, _)| *id == chat.id) {
            chat.itens.push(Item::Aviso {
                texto: aviso.to_string(),
                erro: false,
            });
        }
        let ja_avisado = matches!(chat.itens.last(), Some(Item::Aviso { texto, .. }) if texto == cortado);
        // A parada da tarefa já cobre o turno cortado do ajudante; a conversa do Vitral antigo não
        // volta a rodar.
        if chat.tarefa.is_some_and(|t| ativas.contains(&t)) || chat.papel == Papel::Coordenador {
            chat.turno_aberto = false;
        } else if chat.turno_aberto && !ja_avisado {
            chat.itens.push(Item::Aviso {
                texto: cortado.into(),
                erro: false,
            });
        }
    }
    (entregas, preparos)
}

/// O trabalhador ainda não pode rodar: a cópia da tarefa não ficou pronta, ou (estado gravado por
/// versões antigas) o pedido nunca foi nem está na fila. No chat de projeto: a cópia não está pronta
/// e alguma mensagem espera por ela (ou há pedido aberto), seja qual for o estado.
fn precisa_preparar(tarefa: &Tarefa, trabalhador: &Chat, filas: &Filas) -> bool {
    let com_fila = filas.get(&trabalhador.id).is_some_and(|f| !f.is_empty());
    if trabalhador.papel == Papel::Projeto {
        // O chat nascido de uma issue ("Só a cópia" também) prepara a cópia sem esperar mensagem.
        return tarefa.tem_copia()
            && !tarefa.preparada
            && !tarefa.estado.encerrada()
            && (com_fila || tarefa.estado.ativa() || tarefa.efeitos_issue.is_some());
    }
    tarefa.estado.ativa()
        && (!tarefa.preparada
            || (!recebeu_mensagem(trabalhador)
                && !filas.get(&trabalhador.id).is_some_and(|f| !f.is_empty())))
}

/// Arquiva ou desarquiva. Desarquivar uma conversa já lida renova o `lido_em`: senão o
/// arquivamento automático, que conta do último uso, a guardaria de novo na volta seguinte.
/// A não lida fica como está (o automático não mexe nela, e abrir já renova).
fn trocar_arquivado(c: &mut Chat, agora: u64) {
    c.arquivado = !c.arquivado;
    if !c.arquivado && (c.lido_em >= c.atualizado_em || c.itens.is_empty()) {
        c.lido_em = c.lido_em.max(agora);
    }
}

/// O que um projeto tem em andamento (ver `Motor::ocupacao_do_projeto`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ocupacao {
    pub conversas: Vec<String>,
    pub tarefas: Vec<u64>,
}

impl Ocupacao {
    pub fn livre(&self) -> bool {
        self.conversas.is_empty() && self.tarefas.is_empty()
    }
}

/// O que sai com o projeto, para a pergunta antes de apagar.
#[derive(Clone, Debug, Default)]
pub struct ResumoDoProjeto {
    /// Conversas de topo (sem os trabalhadores).
    pub conversas: usize,
    pub trabalhadores: usize,
    /// As cópias das tarefas que ainda estão no disco.
    pub copias: Vec<PathBuf>,
    pub rotinas: usize,
    /// As pastas ligadas, que ficam como estão.
    pub ligadas: Vec<PathBuf>,
    pub propria: Option<PathBuf>,
    /// Branches das tarefas que ficam nos repositórios.
    pub branches: usize,
    /// Cartões do Mestre com parte no projeto.
    pub cartoes: usize,
}

/// O resultado de apagar: quantas pastas foram para a Lixeira (a própria e as cópias), quantas
/// conversas saíram e as cópias que não foram ("caminho: motivo").
#[derive(Clone, Debug, PartialEq)]
pub struct Apagado {
    pub na_lixeira: usize,
    pub conversas: usize,
    pub erros: Vec<String>,
}

/// O que `tirar_do_estado` tirou.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Retirado {
    pub chats: Vec<String>,
    pub tarefas: Vec<u64>,
    /// As pastas das cópias que as tarefas tinham (existindo ou não).
    pub copias: Vec<PathBuf>,
    pub rotinas: usize,
}

/// As conversas e as tarefas de um projeto: as conversas dele, as tarefas dele (ou de um
/// coordenador dele), os trabalhadores dessas tarefas e os ajudantes (pelo `pai`).
pub fn do_projeto(chats: &[Chat], tarefas: &[Tarefa], projeto: &str) -> (Vec<String>, Vec<u64>) {
    let coordenadores: HashSet<&str> =
        chats.iter().filter(|c| c.projeto == projeto).map(|c| c.id.as_str()).collect();
    let saem: Vec<&Tarefa> = tarefas
        .iter()
        .filter(|t| t.projeto == projeto || coordenadores.contains(t.coordenador.as_str()))
        .collect();
    let trabalhadores: HashSet<&str> = saem.iter().map(|t| t.trabalhador.as_str()).collect();
    let conversas = chats
        .iter()
        .filter(|c| {
            c.projeto == projeto
                || trabalhadores.contains(c.id.as_str())
                || c.pai.as_deref().is_some_and(|p| coordenadores.contains(p))
        })
        .map(|c| c.id.clone())
        .collect();
    (conversas, saem.iter().map(|t| t.id).collect())
}

/// O cartão tem uma parte (não removida) no projeto.
/// O chat de projeto nunca rodou: sem sessão, só os avisos do Vitral e a tarefa em conversa, sem
/// cópia (é o que "Arquivar" apaga em vez de guardar). A conversa trazida do Claude Code já rodou lá.
pub fn nunca_rodou(c: &Chat, tarefas: &[&Tarefa]) -> bool {
    c.papel == Papel::Projeto
        && c.importada.is_none()
        && !c.sessao_criada
        && c.itens.iter().all(|i| matches!(i, Item::Aviso { .. }))
        && tarefas.iter().all(|t| t.estado == EstadoTarefa::Conversa && t.worktree.is_none())
}

/// O cartão é uma divisão com uma parte (não tirada) no projeto.
pub fn cartao_com_parte_em(c: &Cartao, projeto: &str) -> bool {
    matches!(&c.tipo, TipoCartao::Divisao(d) if d.partes.iter().any(|p| !p.removida && p.projeto == projeto))
}

/// Tira do estado tudo o que é do projeto (`do_projeto`), as filas dessas conversas e as rotinas
/// dele. Nos cartões do Mestre, a parte do projeto fica, marcada como falha.
pub fn tirar_do_estado(
    chats: &mut Vec<Chat>,
    tarefas: &mut Vec<Tarefa>,
    filas: &mut Filas,
    rotinas: &mut Vec<Rotina>,
    cartoes: &mut [Cartao],
    projeto: &str,
) -> Retirado {
    let (conversas, ids) = do_projeto(chats, tarefas, projeto);
    let copias = tarefas.iter().filter(|t| ids.contains(&t.id)).filter_map(|t| t.worktree.clone()).collect();
    chats.retain(|c| !conversas.contains(&c.id));
    tarefas.retain(|t| !ids.contains(&t.id));
    filas.retain(|id, _| !conversas.contains(id));
    let antes = rotinas.len();
    rotinas.retain(|r| r.projeto != projeto);
    for cartao in cartoes.iter_mut() {
        if let TipoCartao::Divisao(d) = &mut cartao.tipo {
            for parte in d.partes.iter_mut().filter(|p| p.projeto == projeto && !p.removida) {
                parte.falha.get_or_insert_with(|| "projeto apagado".into());
            }
        }
    }
    Retirado { chats: conversas, tarefas: ids, copias, rotinas: antes - rotinas.len() }
}

/// A pasta que "Apagar projeto" manda para a Lixeira: a própria, e só quando o projeto não tem
/// nenhuma pasta ligada (com as duas, a própria fica: a pessoa pode estar usando as duas). Ela
/// precisa estar direto em `dados/projetos`. As ligadas nunca.
pub fn pasta_propria_para_lixeira(projeto: &Projeto, pasta_projetos: &Path) -> Result<Option<PathBuf>, String> {
    if projeto.pastas.iter().any(|p| !p.propria) {
        return Ok(None);
    }
    let Some(propria) = projeto.pastas.iter().find(|p| p.propria) else {
        return Ok(None);
    };
    let caminho = &propria.caminho;
    let dentro = caminho.parent().is_some_and(|mae| {
        mae == pasta_projetos
            || matches!((dunce::canonicalize(mae), dunce::canonicalize(&pasta_projetos)), (Ok(a), Ok(b)) if a == b)
    });
    let nome_ok = caminho.file_name().is_some_and(|n| !n.to_string_lossy().starts_with('.'));
    if !dentro || !nome_ok {
        return Err(format!("{} não fica na pasta dos projetos do Vitral", caminho.display()));
    }
    Ok(Some(caminho.clone()))
}

/// A conversa fica fora da lista principal: arquivada ela mesma ou com o projeto.
pub fn conversa_arquivada(projetos: &[Projeto], c: &Chat) -> bool {
    c.arquivado || projetos.iter().any(|p| p.id == c.projeto && p.arquivado)
}

/// O chat e todos os que descendem dele pelo `pai` (os trabalhadores das tarefas dele).
pub fn descendentes(chats: &[Chat], chat: &str) -> Vec<String> {
    let mut familia = vec![chat.to_string()];
    let mut i = 0;
    while i < familia.len() {
        let pai = familia[i].clone();
        for c in chats.iter().filter(|c| c.pai.as_deref() == Some(pai.as_str())) {
            if !familia.contains(&c.id) {
                familia.push(c.id.clone());
            }
        }
        i += 1;
    }
    familia
}

/// O projeto pelo id, pelo nome ou pela chave, nessa ordem: o id ganha de um nome igual.
pub fn resolver_projeto(projetos: &[Projeto], texto: &str) -> Resolucao {
    let texto = texto.trim().trim_start_matches('#');
    if let Some(p) = projetos.iter().find(|p| p.id == texto) {
        return Resolucao::Um(p.id.clone());
    }
    let pelo = |casa: &dyn Fn(&Projeto) -> bool| -> Vec<String> {
        projetos.iter().filter(|p| casa(p)).map(|p| p.id.clone()).collect()
    };
    let chave = dados::chave(texto);
    for achados in [pelo(&|p| p.nome == texto), pelo(&|p| !chave.is_empty() && p.chave() == chave)] {
        match achados.len() {
            0 => continue,
            1 => return Resolucao::Um(achados[0].clone()),
            _ => return Resolucao::Varios(achados),
        }
    }
    Resolucao::Nenhum
}

/// Põe `id` antes (ou depois) de `alvo` e renumera a ordem de todos. Diz se mudou.
pub fn reordenar(projetos: &mut [Projeto], id: &str, alvo: &str, antes: bool) -> bool {
    if id == alvo {
        return false;
    }
    let mut ordem: Vec<String> = {
        let mut todos: Vec<&Projeto> = projetos.iter().collect();
        todos.sort_by_key(|p| p.ordem);
        todos.into_iter().map(|p| p.id.clone()).collect()
    };
    let (Some(de), true) = (ordem.iter().position(|p| p == id), ordem.iter().any(|p| p == alvo)) else {
        return false;
    };
    let movido = ordem.remove(de);
    let para = ordem.iter().position(|p| p == alvo).expect("o alvo está na lista") + usize::from(!antes);
    ordem.insert(para, movido);
    let mut mudou = false;
    for p in projetos.iter_mut() {
        let nova = ordem.iter().position(|o| *o == p.id).unwrap_or(0) as u32;
        mudou |= p.ordem != nova;
        p.ordem = nova;
    }
    mudou
}

/// Aplica no projeto só o que veio na mudança.
pub fn aplicar_mudanca(p: &mut Projeto, m: MudancaProjeto) {
    if let Some(nome) = m.nome {
        p.nome = nome.trim().to_string();
    }
    if let Some(icone) = m.icone {
        p.icone = icone;
    }
    if let Some(cor) = m.cor {
        p.cor = cor;
    }
    if let Some(descricao) = m.descricao {
        p.descricao = descricao;
    }
    if let Some(instrucoes) = m.instrucoes {
        p.instrucoes = instrucoes.chars().take(MAX_INSTRUCOES).collect();
    }
    if let Some(pastas) = m.pastas {
        p.provisorio &= pastas.is_empty();
        p.pastas = pastas;
    }
    if let Some((i, config)) = m.pasta
        && let Some(pasta) = p.pastas.get_mut(i)
    {
        pasta.config = config;
    }
    let vazio = |v: Option<String>| v.filter(|v| !v.trim().is_empty());
    if let Some(permissao) = m.permissao {
        p.permissao = vazio(permissao);
    }
    if let Some(modelo) = m.modelo {
        p.modelo = vazio(modelo);
    }
    if let Some(conta) = m.conta {
        p.conta = vazio(conta);
    }
    if let Some(linear) = m.linear {
        p.linear = linear;
    }
}

/// Onde ficam as cópias como a da tarefa: a pasta dela em `copias` (a área nova), ou a
/// `.vitral/worktrees/<projeto>` de uma cópia do Vitral antigo.
fn area_da_copia(copias: &Path, t: &Tarefa) -> PathBuf {
    let nova = copias.join(if t.projeto.is_empty() { "avulsos" } else { &t.projeto });
    let Some(mae) = t.worktree.as_deref().and_then(Path::parent) else {
        return nova;
    };
    let antiga = mae.parent().is_some_and(|w| {
        w.file_name().is_some_and(|n| n == "worktrees")
            && w.parent().and_then(Path::file_name).is_some_and(|n| n == ".vitral")
    });
    if mae.starts_with(copias) || antiga { mae.to_path_buf() } else { nova }
}

/// As portas da tarefa: a faixa dela ou, sem faixa, a fórmula antiga.
pub fn porta_de(t: &Tarefa) -> u64 {
    if t.porta > 0 { t.porta } else { porta_da_tarefa(t.id) }
}

/// O que o arquivamento automático olha no próprio chat: coordenador à vista, não fixado e
/// sem uso desde `limite`.
fn candidata_a_arquivar(c: &Chat, limite: u64) -> bool {
    c.papel.na_lista() && !c.arquivado && !c.fixado && c.atualizado_em.max(c.lido_em) < limite
}

/// O chat já recebeu alguma mensagem (do usuário ou entregue pelo Vitral).
fn recebeu_mensagem(chat: &Chat) -> bool {
    chat.itens
        .iter()
        .any(|i| matches!(i, Item::Usuario { .. } | Item::Entrega { .. }))
}

/// Erros da API que costumam passar sozinhos (sobrecarga, 5xx, rede).
fn erro_passageiro(texto: &str) -> bool {
    let texto = texto.to_lowercase();
    [
        "overloaded",
        "api error: 5",
        "internal server error",
        "econnreset",
        "timed out",
        "connection error",
        "fetch failed",
        "socket hang up",
    ]
    .iter()
    .any(|p| texto.contains(p))
}

/// Até quando a conta que bateu no limite fica de fora: a hora que o Claude Code mandou, senão a
/// marca que ainda vale, senão meia hora (depois disso a escolha automática tenta a conta de novo).
fn fim_do_limite(informado: Option<u64>, anterior: Option<u64>, agora: u64) -> u64 {
    informado
        .filter(|&fim| fim > agora)
        .or(anterior.filter(|&fim| fim > agora))
        .unwrap_or(agora + 30 * 60)
}

/// Para onde a conversa vai quando o plano é aprovado: o modo de antes do Planejar, senão o modo
/// atual da conversa, senão o padrão da pasta, senão Aceitar edições.
fn modo_depois_do_plano(antes: Option<&str>, atual: Option<&str>, padrao: &str) -> String {
    antes
        .into_iter()
        .chain(atual)
        .chain([padrao])
        .find(|m| *m != "plan")
        .unwrap_or("acceptEdits")
        .to_string()
}

/// As mensagens do turno como itens da conversa, menos as que já estão nela (nova tentativa que
/// esperou vaga); o ponto de volta fica na primeira que entra.
fn itens_das_entradas(mensagens: Vec<Entrada>, mut ponto: Option<String>) -> Vec<Item> {
    mensagens
        .into_iter()
        .filter(|e| !e.registrada)
        .map(|entrada| {
            let ponto = ponto.take();
            match entrada.de {
                Some(de) => Item::Entrega {
                    de,
                    texto: entrada.texto,
                    ponto,
                },
                None => Item::Usuario {
                    texto: entrada.texto,
                    anexos: entrada.anexos,
                    ponto,
                    ultrathink: entrada.ultrathink,
                    ultracode: entrada.ultracode,
                    referencias: entrada.referencias,
                },
            }
        })
        .collect()
}

/// O uuid do turno que vai abrir: o mesmo do anterior quando o lote é uma repetição dele (todas as
/// entradas já estão na conversa: "Tentar de novo", a troca de conta, a nova tentativa depois de
/// uma queda, mesmo que tenha esperado na fila); senão, um novo.
fn turno_do_lote(anterior: Option<&str>, mensagens: &[Entrada]) -> String {
    match anterior {
        Some(turno) if !mensagens.is_empty() && mensagens.iter().all(|e| e.registrada) => turno.to_string(),
        _ => uuid::Uuid::new_v4().to_string(),
    }
}

/// A mensagem que o usuário escreveu, pronta para entrar no turno ou na fila.
fn entrada_do_usuario(texto: String, anexos: Vec<PathBuf>, reforco: Reforco) -> Entrada {
    let comando = texto.starts_with('/');
    Entrada {
        de: None,
        ultrathink: reforco.ultrathink && !comando,
        ultracode: reforco.ultracode && !comando,
        texto,
        anexos,
        ..Default::default()
    }
}

/// No turno com Ultracode: o limite de agentes vale por fluxo, e com ele ligado o Claude Code não
/// limita os subagentes; nada impede dois fluxos, ou um fluxo e vários subagentes, juntos.
const UM_FLUXO_POR_VEZ: &str = "[Vitral] Rode um fluxo de agentes por vez e não abra outros agentes \
enquanto ele roda: este computador não aguenta muita carga ao mesmo tempo.";

/// O texto que vai ao Claude Code num turno (as entradas separadas por traço, com quem mandou e
/// os anexos que não são imagem) e as imagens. Com Ultracode, o pedido de um fluxo por vez; com
/// Ultrathink em alguma, a palavra entra uma vez no fim.
fn texto_do_turno(mensagens: &[Entrada]) -> (String, Vec<PathBuf>) {
    let mut texto = String::new();
    let mut imagens = Vec::new();
    for entrada in mensagens {
        if !texto.is_empty() {
            texto.push_str("\n\n---\n\n");
        }
        match &entrada.de {
            Some(origem) if origem == "Vitral" => texto.push_str(&format!("[Vitral] {}", entrada.texto)),
            Some(origem) => texto.push_str(&format!("[de: {origem}] {}", entrada.texto)),
            None => texto.push_str(&entrada.texto),
        }
        if !entrada.extra.is_empty() {
            texto.push_str("\n\n");
            texto.push_str(&entrada.extra);
        }
        // Imagens vão junto na mensagem; outros arquivos o Claude lê pelo caminho.
        let (fotos, arquivos): (Vec<_>, Vec<_>) = entrada
            .anexos
            .iter()
            .cloned()
            .partition(|a| claude::tipo_imagem(a).is_some());
        if !arquivos.is_empty() {
            texto.push_str("\n\nArquivos anexados pelo usuário (leia se precisar):");
            for arquivo in &arquivos {
                texto.push_str(&format!("\n- {}", arquivo.display()));
            }
        }
        imagens.extend(fotos);
    }
    if mensagens.iter().any(|e| e.ultracode) {
        texto.push_str("\n\n");
        texto.push_str(UM_FLUXO_POR_VEZ);
    }
    if mensagens.iter().any(|e| e.ultrathink) {
        texto.push_str("\n\nultrathink");
    }
    (texto, imagens)
}

/// O ajuste passou do limite combinado de rodadas: só vale para o dono, e conta desde o começo do
/// ciclo (o PR aberto, o merge), com a primeira rodada dele como 1. Sem ciclo marcado, desde a
/// primeira. As rodadas do usuário e do vigia do PR não esbarram nele (o do PR são as 3 automáticas).
fn passou_do_limite(origem: OrigemAjuste, rodada: u32, rodada_do_ciclo: u32, maximo: u32) -> bool {
    origem == OrigemAjuste::Dono && rodada.saturating_sub(rodada_do_ciclo.saturating_sub(1)) > maximo
}

/// Até quantos caracteres das últimas mensagens vão na passagem de sessão.
const PASSAGEM_CARACTERES: usize = 6000;

/// O começo do primeiro turno de uma sessão nova que tomou o lugar de uma que expirou: as últimas
/// 10 mensagens (até 6000 caracteres, as mais recentes ficam), o pedido em aberto e a cópia. As
/// `pular` últimas mensagens já vão no próprio turno.
fn bloco_de_passagem(itens: &[Item], tarefa: Option<&Tarefa>, pular: usize) -> String {
    let mut falas: Vec<String> = itens
        .iter()
        .rev()
        .filter_map(|i| match i {
            Item::Usuario { texto, .. } => Some(format!("Usuário: {}", texto.trim())),
            Item::Entrega { de, texto, .. } => Some(format!("[de: {de}] {}", texto.trim())),
            Item::Assistente { texto } => Some(format!("Você: {}", texto.trim())),
            _ => None,
        })
        .skip(pular)
        .take(10)
        .collect();
    let mut sobra = PASSAGEM_CARACTERES;
    let mut cabem = Vec::new();
    for fala in falas.drain(..) {
        if sobra == 0 {
            break;
        }
        let n = fala.chars().count();
        if n <= sobra {
            sobra -= n;
            cabem.push(fala);
        } else {
            cabem.push(format!("{}…", fala.chars().take(sobra).collect::<String>()));
            sobra = 0;
        }
    }
    cabem.reverse();
    let mut bloco = String::from(
        "[Vitral] Passagem de sessão: a sessão anterior do Claude Code desta conversa expirou, e esta é \
         uma nova. As últimas mensagens, para continuar de onde parou (a conversa inteira continua na \
         tela do usuário):",
    );
    for fala in cabem {
        bloco.push_str("\n\n");
        bloco.push_str(&fala);
    }
    if let Some(t) = tarefa.filter(|t| !t.estado.encerrada()) {
        // Num chat de projeto em conversa não há pedido aberto: só a cópia e o que falta juntar.
        let mut linha = Vec::new();
        if t.estado != EstadoTarefa::Conversa {
            linha.push(format!("Tarefa em aberto: #{} {} ({}).", t.id, t.titulo, t.estado.nome()));
        }
        if let (Some(branch), Some(copia)) = (&t.branch, &t.worktree) {
            linha.push(format!("A cópia é {}, no branch {branch}.", copia.display()));
            if let Some(m) = t.mudancas.as_ref().filter(|m| m.arquivos > 0) {
                let arquivos = if m.arquivos == 1 { "1 arquivo mudado".to_string() } else { format!("{} arquivos mudados", m.arquivos) };
                linha.push(format!("Ela tem {arquivos} que ainda não entraram na origem (+{} −{}).", m.mais, m.menos));
            }
        }
        if !linha.is_empty() {
            bloco.push_str("\n\n");
            bloco.push_str(&linha.join(" "));
        }
    }
    bloco
}

/// As ferramentas do Claude Code que mudam a pasta do chat no meio do turno: o chat de projeto e o
/// ajudante ficam na cópia (ou na pasta) que o Vitral deu.
const SEM_TROCAR_DE_PASTA: [&str; 2] = ["EnterWorktree", "ExitWorktree"];

/// Edit, Write e NotebookEdit bloqueados (`//` marca caminho absoluto; vale até sem permissões):
/// a origem sempre que o chat trabalha numa cópia (mesmo movido para um projeto sem ela, ou com ela
/// desligada do projeto), as pastas ligadas do projeto (menos a própria pasta, e as que a contêm, na
/// forma direta) e, no ajudante, a cópia e a origem do pai.
pub fn regras_de_bloqueio(
    forma: Forma,
    origem: &Path,
    cwd: &Path,
    pastas: &[PathBuf],
    pai: Option<(&Path, &Path)>,
) -> Vec<String> {
    let mut bloquear: Vec<&Path> = Vec::new();
    if forma == Forma::Copia && !origem.as_os_str().is_empty() {
        bloquear.push(origem);
    }
    for pasta in pastas {
        let minha = forma != Forma::Copia && (pasta == origem || cwd.starts_with(pasta));
        if !minha {
            bloquear.push(pasta);
        }
    }
    if let Some((copia_do_pai, origem_do_pai)) = pai {
        bloquear.extend([copia_do_pai, origem_do_pai]);
    }
    let mut regras = Vec::new();
    for pasta in bloquear.into_iter().filter(|p| !p.as_os_str().is_empty()) {
        for ferramenta in ["Edit", "Write", "NotebookEdit"] {
            let regra = format!("{ferramenta}({}/**)", claude::caminho_de_regra(pasta));
            if !regras.contains(&regra) {
                regras.push(regra);
            }
        }
    }
    regras
}

/// Se o chat pode criar mais um ajudante: só o chat de projeto com git (cada ajudante trabalha numa
/// cópia da pasta dele), com a cópia pronta e abaixo do limite. O ajudante não cria ajudante.
pub fn pode_criar_ajudante(papel: Papel, tarefa: Option<&Tarefa>, abertos: usize, limite: usize) -> Result<(), String> {
    if papel != Papel::Projeto {
        return Err("Só um chat de projeto cria ajudantes; um ajudante faz o trabalho dele sozinho.".into());
    }
    let Some(t) = tarefa else {
        return Err("Este chat não tem pasta para dar aos ajudantes.".into());
    };
    if !t.tem_git() {
        return Err(
            "Este chat trabalha numa pasta sem git: cada ajudante precisa de uma cópia dela, então aqui não dá para \
             criar ajudantes. Faça o trabalho você mesmo."
                .into(),
        );
    }
    if t.tem_copia() && !t.preparada {
        return Err("A sua cópia ainda não está pronta: não dá para criar o ajudante agora.".into());
    }
    if limite == 0 {
        return Err("Os ajudantes estão desligados nas Configurações: faça o trabalho você mesmo.".into());
    }
    if abertos >= limite {
        return Err(format!(
            "Você já tem {abertos} ajudantes abertos (o limite é {limite}): espere um terminar (aprovar_tarefa ou \
             cancelar_tarefa) antes de criar outro."
        ));
    }
    Ok(())
}

/// O turno pode começar com `ativos` processos de chat de projeto e ajudante rodando: o Mestre e a
/// conversa antiga não contam; o turno que o usuário pediu cabe numa vaga a mais.
pub fn pode_comecar(papel: Papel, do_usuario: bool, ativos: usize, limite: usize) -> bool {
    !papel.conta_vaga() || ativos < limite + usize::from(do_usuario)
}

/// A pasta de outro chat direto vivo que é a mesma, está dentro ou contém `cwd` (pelos caminhos
/// canônicos), ou nada.
pub fn escritor_ocupado(cwd: &Path, ativos: &[PathBuf]) -> Option<PathBuf> {
    let canonico = |p: &Path| dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let meu = canonico(cwd);
    // A pasta pessoal (a das conversas trazidas do Claude Code) não é pasta de projeto: lá, como no
    // VS Code, várias conversas rodam juntas, e ela não trava as pastas de dentro.
    let casa = dirs::home_dir().map(|h| canonico(&h));
    if casa.as_ref() == Some(&meu) {
        return None;
    }
    ativos
        .iter()
        .map(|a| canonico(a))
        .filter(|outro| casa.as_ref() != Some(outro))
        .find(|outro| meu.starts_with(outro) || outro.starts_with(&meu))
}

/// O que a mensagem a um chat de projeto faz com a cópia dele.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardaCopia {
    /// Pronta (ou o chat não usa cópia): o turno segue.
    Segue,
    /// Sem cópia (nunca preparada, apagada na limpeza ou que falhou): prepara agora; a mensagem espera.
    Preparar,
    /// Já está sendo preparada (ou na fila para isso): a mensagem espera.
    Esperar,
}

/// A guarda da cópia no começo do turno, seja qual for o estado da tarefa (a conversa também). A
/// cópia sendo apagada (a limpeza marca antes) também faz esperar: depois ela é refeita.
pub fn guarda_da_copia(t: &Tarefa, preparando: bool) -> GuardaCopia {
    let apagando = t.copia_limpa && t.worktree.is_some();
    if !t.tem_copia() || (t.preparada && !apagando) {
        GuardaCopia::Segue
    } else if preparando || apagando {
        GuardaCopia::Esperar
    } else {
        GuardaCopia::Preparar
    }
}

/// O que a limpeza faz com a cópia de um chat de projeto, na vez do repositório: nada sem a origem,
/// nada se a cópia é de outro repositório; com trabalho não juntado, um commit e o branch fica. Diz
/// se guardou trabalho.
fn limpar_copia_do_chat(origem: &Path, copia: &Path, area: &Path, branch: &str, grupo: &str, pontos: &[String]) -> anyhow::Result<bool> {
    if !origem.is_dir() {
        anyhow::bail!("a pasta {} não foi encontrada; a cópia fica até ela voltar", origem.display());
    }
    if git::raiz_comum(copia).is_none() || git::raiz_comum(copia) != git::raiz_comum(origem) {
        anyhow::bail!("a cópia não é mais do repositório de {}; ficou como está", origem.display());
    }
    let estado = git::estado_copia(copia, origem)?;
    let guardar = estado.a_frente > 0 || estado.suja;
    if guardar {
        git::commit_de_turno(copia, "Vitral: guardado antes de apagar a cópia")?;
    }
    git::limpar_copia(origem, copia, area, branch, Some(grupo), pontos, guardar)?;
    Ok(guardar)
}

/// As mensagens do usuário desde a última junção do chat (o que o revisor confere sem um pedido),
/// as mais novas primeiro se não couberem.
fn mensagens_desde_a_juncao(itens: &[Item]) -> String {
    let desde = itens
        .iter()
        .rposition(|i| matches!(i, Item::Aviso { texto, .. } if texto.starts_with(JUNTOU)))
        .map_or(0, |i| i + 1);
    let textos: Vec<String> = itens[desde..]
        .iter()
        .filter_map(|i| match i {
            Item::Usuario { texto, .. } if !texto.starts_with('/') => Some(format!("- {}", texto.trim())),
            _ => None,
        })
        .collect();
    if textos.is_empty() {
        return "(nenhuma mensagem do usuário desde a última junção)".into();
    }
    git::final_do_texto(&textos.join("\n"), 6000)
}

/// Esforço do turno com Ultrathink: `max` continua `max`; o resto sobe para `xhigh`.
pub fn esforco_do_ultrathink(atual: Option<&str>) -> &'static str {
    if atual == Some("max") { "max" } else { "xhigh" }
}

/// O `result` chegou com agentes em segundo plano: o turno espera por eles, menos com erro ou
/// parado pelo usuário.
fn adiar_o_fim(agentes_em_fundo: usize, erro: bool, interrompido: bool) -> bool {
    agentes_em_fundo > 0 && !erro && !interrompido
}

/// O modelo tem o esforço Extra (xhigh), sem o qual o Claude Code desliga o Ultracode calado.
/// Vazio (o padrão) e os apelidos das famílias novas contam como sim; o Haiku é o 4.5.
pub fn aceita_ultracode(modelo: &str) -> bool {
    let base = modelo.split('[').next().unwrap_or_default().trim().to_lowercase();
    if base == "haiku" {
        return false;
    }
    let Some(resto) = base.strip_prefix("claude-") else {
        return true;
    };
    // A data no fim (claude-opus-4-20250514) não conta.
    let nome = resto
        .split('-')
        .filter(|p| !(p.len() >= 6 && p.chars().all(|c| c.is_ascii_digit())))
        .collect::<Vec<_>>()
        .join("-");
    const SEM_EXTRA: [&str; 10] = [
        "opus-4", "opus-4-0", "opus-4-1", "opus-4-5", "opus-4-6", "sonnet-4", "sonnet-4-0", "sonnet-4-5",
        "sonnet-4-6", "haiku-4-5",
    ];
    let antigo = ["2", "3-", "instant"].iter().any(|p| nome.starts_with(p));
    !antigo && !SEM_EXTRA.contains(&nome.as_str())
}

fn texto_da_versao((a, b, c): Versao) -> String {
    format!("{a}.{b}.{c}")
}

/// Por que os fluxos de agentes não vieram num turno com Ultracode, se o próprio Vitral os liga
/// no `--settings`: um `false` em outra configuração da conta vence.
pub const FLUXOS_DESLIGADOS: &str = "os fluxos de agentes estão desligados nesta conta (em Dynamic workflows \
no /config, por disableWorkflows ou pela organização)";

/// Por que o Ultracode não vale (`None`: vale). Os fluxos desligados na conta não entram: a
/// pessoa pode ter mudado a configuração, então a opção só avisa.
pub fn motivo_sem_ultracode(papel: Papel, modelo: &str, versao: Option<Versao>) -> Option<String> {
    if papel == Papel::Coordenador {
        return Some("a conversa do Vitral antigo é só para leitura".into());
    }
    if papel == Papel::Mestre {
        return Some("só nos chats de projeto".into());
    }
    if let Some(versao) = versao.filter(|v| *v < VERSAO_ULTRACODE) {
        return Some(format!(
            "precisa do Claude Code {} ou mais novo (este é o {})",
            texto_da_versao(VERSAO_ULTRACODE),
            texto_da_versao(versao)
        ));
    }
    if !aceita_ultracode(modelo) {
        return Some("este modelo não tem o esforço Extra, que o Ultracode usa".into());
    }
    None
}

/// Por que os agentes em segundo plano de um turno adiado não terminaram, quando o processo saiu
/// sem outro `result` (`None`: acabaram e o processo saiu normalmente).
fn agentes_cortados(restantes: usize, codigo: Option<i32>, stderr: &str) -> Option<String> {
    if stderr.contains("Background tasks still running") {
        return Some("o Claude Code parou de esperar por eles e os encerrou (passaram do tempo máximo de espera)".into());
    }
    let detalhe = stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| format!(" ({})", git::inicio_do_texto(l, 300)))
        .unwrap_or_default();
    match codigo {
        Some(-1) => Some(format!("o processo do Claude Code foi derrubado{detalhe}")),
        Some(c) if c != 0 => Some(format!("o processo do Claude Code saiu com o código {c}{detalhe}")),
        _ if restantes > 0 => Some(format!("o processo do Claude Code saiu com {restantes} ainda rodando{detalhe}")),
        _ => None,
    }
}

/// Janela de contexto do modelo antes do primeiro `result` dizer qual é.
pub fn janela_padrao(modelo: Option<&str>) -> u64 {
    match modelo {
        Some(m) if m.ends_with("[1m]") => 1_000_000,
        Some(m)
            if ["claude-opus-5", "claude-sonnet-5", "claude-haiku-5", "claude-fable"]
                .iter()
                .any(|p| m.starts_with(p)) =>
        {
            1_000_000
        }
        _ => 200_000,
    }
}

/// O motivo da parada como o Claude deve ler: o "você" da interface é o usuário.
fn motivo_para_o_claude(motivo: &str) -> &str {
    if motivo == PARADA_VOCE {
        "o usuário interrompeu"
    } else {
        motivo
    }
}

/// Portão das aprovações do coordenador: só depois do relatório desta rodada e, com a verificação
/// falhando ou problema de gravidade alta do revisor, só com `forcar` e um motivo. Devolve a
/// ressalva quando aprovou mesmo assim.
fn portao_aprovacao(t: &Tarefa, forcar: bool, motivo: &str) -> Result<Option<String>, String> {
    let id = t.id;
    if t.estado != EstadoTarefa::EmRevisao {
        return Err(format!(
            "A tarefa #{id} está {}; só dá para aprovar depois do relatório desta rodada (o usuário \
             pode aprovar pelo painel se quiser).",
            t.estado.nome()
        ));
    }
    if t.entrega_pendente.is_some() {
        return Err(format!(
            "O relatório da tarefa #{id} ainda não saiu (a verificação ficou no meio); espere ele \
             chegar."
        ));
    }
    let mut vermelho = Vec::new();
    if let Some(v) = t.ultima_verificacao.as_ref().filter(|v| !v.passou) {
        vermelho.push(format!("verificação `{}` falhando", v.comando));
    }
    let graves: Vec<String> = t
        .revisao
        .iter()
        .flat_map(|r| &r.problemas)
        .filter(|p| p.gravidade == "alta")
        .map(crate::revisor::onde)
        .collect();
    if !graves.is_empty() {
        let quantos = if graves.len() == 1 {
            "1 problema".to_string()
        } else {
            format!("{} problemas", graves.len())
        };
        vermelho.push(format!("{quantos} de gravidade alta do revisor ({})", graves.join(", ")));
    }
    if vermelho.is_empty() {
        return Ok(None);
    }
    let lista = vermelho.join("; ");
    let motivo = motivo.trim();
    if !forcar || motivo.is_empty() {
        return Err(format!(
            "Não aprovei a tarefa #{id}: {lista}. Peça o ajuste (pedir_ajuste). Se a falha \
             comprovadamente não vem desta tarefa (já falhava antes, ou o problema apontado não \
             existe), aprove com forcar=true e um motivo: ele fica no commit e o usuário vê."
        ));
    }
    Ok(Some(format!("{lista}. Motivo: {motivo}")))
}

/// O que vai junto com um ajuste do coordenador: os problemas do revisor e o fim da saída da
/// verificação desta entrega. Sem `incluir`, só o que apontou problema. Devolve o texto e o que
/// foi junto, para a resposta.
fn anexo_do_ajuste(t: &Tarefa, incluir: Option<&[String]>, ignorar: &[usize]) -> (String, Vec<String>) {
    let mut texto = String::new();
    let mut junto = Vec::new();
    // Antes do relatório desta rodada, o que está guardado é da anterior.
    if t.estado != EstadoTarefa::EmRevisao || t.entrega_pendente.is_some() {
        return (texto, junto);
    }
    let quer = |nome: &str| incluir.map(|l| l.iter().any(|i| i.starts_with(nome)));
    if let Some(r) = &t.revisao {
        let restantes: Vec<_> = (1..=r.problemas.len())
            .zip(&r.problemas)
            .filter(|(n, _)| !ignorar.contains(n))
            .map(|(_, p)| p)
            .collect();
        let importante = restantes.iter().any(|p| p.gravidade != "baixa")
            || (r.veredito == "ajustar" && ignorar.is_empty());
        if !restantes.is_empty() && quer("revis").unwrap_or(importante) {
            texto.push_str("Problemas apontados pelo revisor independente:\n");
            texto.push_str(&crate::revisor::linhas_problemas(r, ignorar));
            junto.push(if restantes.len() == 1 {
                "1 problema do revisor".to_string()
            } else {
                format!("{} problemas do revisor", restantes.len())
            });
        }
    }
    if let Some(v) = &t.ultima_verificacao
        && quer("verific").unwrap_or(!v.passou)
    {
        let linhas: Vec<&str> = v.saida.trim_end().lines().collect();
        let fim = linhas[linhas.len().saturating_sub(40)..].join("\n");
        if !texto.is_empty() {
            texto.push('\n');
        }
        texto.push_str(&format!(
            "Verificação `{}` {} (últimas linhas):\n```\n{fim}\n```\n",
            v.comando,
            if v.passou { "passou" } else { "FALHOU" }
        ));
        junto.push("a saída da verificação".into());
    }
    (texto, junto)
}

/// Lista de um argumento das ferramentas; às vezes o Claude manda o array como texto ("[1, 2]"
/// ou "1, 2").
fn lista_de(valor: &Value) -> Option<Vec<Value>> {
    match valor {
        Value::Array(itens) => Some(itens.clone()),
        Value::String(texto) => Some(serde_json::from_str(texto).unwrap_or_else(|_| {
            texto
                .split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(|t| Value::String(t.into()))
                .collect()
        })),
        _ => None,
    }
}

/// Notificação do sistema: "projeto · título" e uma linha só de corpo, com o ícone do Vitral.
/// Os argumentos do `notify-send`: o clique (a ação padrão) abre a conversa, e a notificação
/// anterior do mesmo chat é substituída em vez de empilhar.
#[cfg_attr(windows, allow(dead_code))]
fn argumentos_do_aviso(titulo: &str, corpo: &str, substitui: Option<u32>) -> Vec<String> {
    let mut args: Vec<String> = ["-a", "Vitral", "-i", "vitral", "-h", "string:desktop-entry:vitral"]
        .into_iter()
        .chain(["-A", "default=Abrir", "-p"])
        .map(String::from)
        .collect();
    if let Some(id) = substitui {
        args.extend(["--replace-id".to_string(), id.to_string()]);
    }
    args.extend([titulo.to_string(), corpo.to_string()]);
    args
}

/// "Precisa da sua permissão: Bash, $ cargo test"; o plano pede aprovação.
fn texto_do_pedido(ferramenta: &str, entrada: &Value) -> String {
    let nome = ferramenta.rsplit("__").next().unwrap_or(ferramenta);
    match ferramenta {
        "ExitPlanMode" => "Precisa da sua aprovação: o plano está pronto".into(),
        "Bash" => {
            let comando = entrada["command"].as_str().unwrap_or_default();
            let linha = comando.lines().next().unwrap_or_default().trim();
            format!("Precisa da sua permissão: Bash, $ {linha}")
        }
        _ => {
            let resumo = claude::resumo_ferramenta(ferramenta, entrada);
            let detalhe = resumo.split_once(" · ").map(|(_, d)| d).unwrap_or_default();
            if detalhe.is_empty() {
                format!("Precisa da sua permissão: {nome}")
            } else {
                format!("Precisa da sua permissão: {nome}, {detalhe}")
            }
        }
    }
}

/// "Terminou: " e a primeira linha da resposta.
fn texto_do_fim(resposta: &str) -> String {
    let linha = resposta.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    if linha.is_empty() { "Terminou".into() } else { format!("Terminou: {linha}") }
}

/// Junta um pedaço ao texto parcial do chat e diz se a janela precisa redesenhar: o primeiro
/// pedaço sempre (a conversa passa a "escrevendo"), os outros só com o chat num painel aberto.
fn juntar_parcial(
    parcial: &mut HashMap<String, String>,
    visiveis: &HashSet<String>,
    chat: &str,
    pedaco: &str,
) -> bool {
    let primeiro = !parcial.contains_key(chat);
    parcial.entry(chat.to_string()).or_default().push_str(pedaco);
    primeiro || visiveis.contains(chat)
}

/// Faixa de 10 portas de cada tarefa, para servidores de teste não brigarem entre cópias.
pub fn porta_da_tarefa(id: u64) -> u64 {
    41000 + (id % 400) * 10
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::modelo::{Chat, Tarefa};

    fn chat(id: &str, papel: Papel, tarefa: Option<u64>) -> Chat {
        let mut c = Chat::novo(id.into(), papel, "p", id, PathBuf::from("/tmp"));
        (c.tarefa, c.sessao, c.sessao_criada) = (tarefa, "s".into(), true);
        (c.criado_em, c.atualizado_em, c.lido_em) = (0, 0, 0);
        c
    }

    fn tarefa(id: u64, trabalhador: &str, estado: EstadoTarefa) -> Tarefa {
        Tarefa {
            id,
            projeto: "p".into(),
            coordenador: "c".into(),
            trabalhador: trabalhador.into(),
            titulo: "t".into(),
            objetivo: "o".into(),
            criterios: vec!["c".into()],
            verificacao: None,
            estado,
            rodada: 1,
            worktree: None,
            branch: None,
            base: None,
            copia_limpa: false,
            progresso: Some("rodando os testes".into()),
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
            ..Default::default()
        }
    }

    fn avisos(chat: &Chat) -> usize {
        chat.itens.iter().filter(|i| matches!(i, Item::Aviso { .. })).count()
    }

    #[test]
    fn tarefa_cortada_vira_parada_com_um_aviso_so() {
        let mut trabalhador = chat("w", Papel::Trabalhador, Some(1));
        trabalhador.turno_aberto = true;
        trabalhador.itens.push(Item::Usuario {
            texto: "Tarefa #1".into(),
            anexos: Vec::new(),
            ponto: None,
            ultrathink: false,
            ultracode: false,
            referencias: Vec::new(),
        });
        let mut chats = vec![chat("c", Papel::Coordenador, None), trabalhador];
        let mut tarefas = vec![tarefa(1, "w", EstadoTarefa::Rodando)];
        let filas = Filas::new();

        marcar_interrompidas(&mut chats, &mut tarefas, &filas);
        assert_eq!(tarefas[0].parada.as_deref(), Some(PARADA_FECHOU));
        assert_eq!(tarefas[0].progresso, None);
        assert_eq!(avisos(&chats[1]), 1);
        assert_eq!(avisos(&chats[0]), 0);
        assert!(!chats[1].turno_aberto);

        // Reabrir de novo sem retomar não empilha outro aviso.
        marcar_interrompidas(&mut chats, &mut tarefas, &filas);
        assert_eq!(avisos(&chats[1]), 1);
    }

    #[test]
    fn tarefa_que_esperava_vaga_para_e_a_sem_pedido_volta_a_preparar() {
        let mut chats = vec![
            chat("w1", Papel::Trabalhador, Some(1)),
            chat("w2", Papel::Trabalhador, Some(2)),
        ];
        let mut tarefas = vec![
            tarefa(1, "w1", EstadoTarefa::Rodando),
            tarefa(2, "w2", EstadoTarefa::Rodando),
        ];
        let mut filas = Filas::new();
        filas.insert(
            "w1".into(),
            vec![Entrada {
                de: None,
                texto: "Tarefa #1".into(),
                anexos: Vec::new(),
                registrada: false,
                ..Default::default()
            }],
        );
        let (_, preparos) = marcar_interrompidas(&mut chats, &mut tarefas, &filas);
        assert!(tarefas[0].parada.as_deref().unwrap().contains("vaga"));
        // Gravada por uma versão antiga, sem o pedido nem fila: a preparação é refeita.
        assert_eq!(tarefas[1].parada, None);
        assert_eq!(preparos, VecDeque::from([2]));
    }

    #[test]
    fn preparacao_cortada_e_refeita_sozinha_so_uma_vez() {
        let mut chats = vec![chat("w", Papel::Trabalhador, Some(1))];
        let mut cortada = tarefa(1, "w", EstadoTarefa::Rodando);
        cortada.preparada = false;
        cortada.worktree = Some(PathBuf::from("/tmp/t1"));
        let mut tarefas = vec![cortada];
        // Mensagem que o usuário mandou durante a preparação: espera na fila e vai junto.
        let mut filas = Filas::new();
        filas.insert(
            "w".into(),
            vec![Entrada {
                de: None,
                texto: "use a versão nova".into(),
                anexos: Vec::new(),
                registrada: false,
                ..Default::default()
            }],
        );

        let (entregas, preparos) = marcar_interrompidas(&mut chats, &mut tarefas, &filas);
        assert!(entregas.is_empty());
        assert_eq!(preparos, VecDeque::from([1]));
        assert_eq!(tarefas[0].parada, None);
        assert_eq!(avisos(&chats[0]), 1);
        assert!(precisa_preparar(&tarefas[0], &chats[0], &filas));

        // Caiu de novo durante a preparação refeita: agora espera o Retomar.
        tarefas[0].parada = Some(PARADA_PREPARACAO.into());
        tarefas[0].progresso = Some("preparando a cópia de novo".into());
        let (_, preparos) = marcar_interrompidas(&mut chats, &mut tarefas, &filas);
        assert!(preparos.is_empty());
        assert_eq!(tarefas[0].progresso, None);
        assert_eq!(tarefas[0].parada.as_deref(), Some(PARADA_PREPARACAO));
        assert_eq!(avisos(&chats[0]), 1);

        // Pronta e já com o pedido: nada a preparar.
        tarefas[0].preparada = true;
        chats[0].itens.push(Item::Entrega {
            de: "coordenador".into(),
            texto: "Tarefa #1".into(),
            ponto: None,
        });
        assert!(!precisa_preparar(&tarefas[0], &chats[0], &Filas::new()));
    }

    #[test]
    fn turno_cortado_ganha_aviso_uma_vez_e_a_conversa_antiga_nao_volta() {
        let mut projeto = chat("p", Papel::Projeto, None);
        projeto.turno_aberto = true;
        let mut antiga = chat("c", Papel::Coordenador, None);
        antiga.turno_aberto = true;
        let mut chats = vec![projeto, chat("p2", Papel::Projeto, None), antiga];
        let mut tarefas = Vec::new();
        marcar_interrompidas(&mut chats, &mut tarefas, &Filas::new());
        marcar_interrompidas(&mut chats, &mut tarefas, &Filas::new());
        assert_eq!(avisos(&chats[0]), 1);
        assert_eq!(avisos(&chats[1]), 0);
        assert_eq!((avisos(&chats[2]), chats[2].turno_aberto), (0, false));
    }

    #[test]
    fn entrega_cortada_e_refeita_sozinha_so_uma_vez() {
        let mut chats = vec![chat("w", Papel::Trabalhador, Some(1))];
        let mut nova = tarefa(1, "w", EstadoTarefa::EmRevisao);
        nova.entrega_pendente = Some(true);
        // Gravada por uma versão antiga: só o progresso diz que a verificação ficou no meio.
        let mut antiga = tarefa(2, "w", EstadoTarefa::EmRevisao);
        antiga.progresso = Some(VERIFICANDO.into());
        let mut tarefas = vec![nova, antiga, tarefa(3, "w", EstadoTarefa::EmRevisao)];

        let (fila, _) = marcar_interrompidas(&mut chats, &mut tarefas, &Filas::new());
        assert_eq!(fila, VecDeque::from([1, 2]));
        assert_eq!(tarefas[0].entrega_pendente, Some(true));
        assert_eq!(tarefas[1].entrega_pendente, Some(false));
        assert_eq!(tarefas[2].entrega_pendente, None);

        // Caiu de novo durante a verificação refeita: agora espera o Retomar.
        tarefas[0].parada = Some(PARADA_VERIFICACAO.into());
        let (fila, _) = marcar_interrompidas(&mut chats, &mut tarefas, &Filas::new());
        assert_eq!(fila, VecDeque::from([2]));
        assert_eq!(tarefas[0].progresso, None);
    }

    #[test]
    fn limite_de_rodadas_so_para_o_dono_e_desde_o_ciclo() {
        // Sem ciclo (nunca abriu PR): como antes, a 4ª rodada passa do limite de 3.
        assert!(!passou_do_limite(OrigemAjuste::Dono, 3, 0, 3));
        assert!(passou_do_limite(OrigemAjuste::Dono, 4, 0, 3));
        // O ciclo começou na rodada 6 (o PR abriu ali): conta 6 como 1.
        assert!(!passou_do_limite(OrigemAjuste::Dono, 8, 6, 3));
        assert!(passou_do_limite(OrigemAjuste::Dono, 9, 6, 3));
        // O usuário e o vigia do PR nunca esbarram nele.
        assert!(!passou_do_limite(OrigemAjuste::Usuario, 40, 0, 3));
        assert!(!passou_do_limite(OrigemAjuste::VigiaPr, 40, 0, 3));
    }

    #[test]
    fn so_erros_passageiros_da_api_ganham_nova_tentativa() {
        assert!(erro_passageiro(
            r#"API Error: 529 {"type":"error","error":{"type":"overloaded_error"}}"#
        ));
        assert!(erro_passageiro("API Error: 500 Internal server error"));
        assert!(erro_passageiro("API Error: Request timed out."));
        assert!(erro_passageiro("API Error: Connection error."));
        assert!(!erro_passageiro("Prompt is too long"));
        assert!(!erro_passageiro("Claude AI usage limit reached|1700000000"));
        assert!(!erro_passageiro("error_max_turns"));
    }

    fn entrega_vermelha() -> Tarefa {
        let mut t = tarefa(4, "w", EstadoTarefa::EmRevisao);
        t.progresso = None;
        t.ultima_verificacao = Some(Verificacao {
            comando: "cargo test".into(),
            passou: false,
            saida: (1..=60).map(|n| format!("linha {n}\n")).collect(),
            duracao_s: None,
        });
        let problema = |arquivo: &str, linha, gravidade: &str| crate::modelo::Problema {
            arquivo: arquivo.into(),
            linha,
            gravidade: gravidade.into(),
            descricao: "errado".into(),
        };
        t.revisao = Some(crate::modelo::Revisao {
            veredito: "ajustar".into(),
            resumo: "falta um caso".into(),
            problemas: vec![
                problema("src/a.rs", Some(7), "alta"),
                problema("src/b.rs", None, "baixa"),
                problema("src/c.rs", Some(2), "media"),
            ],
        });
        t
    }

    #[test]
    fn portao_recusa_sem_relatorio_e_vermelho_sem_motivo() {
        let mut t = entrega_vermelha();
        let recusa = portao_aprovacao(&t, false, "").unwrap_err();
        assert!(recusa.contains("verificação `cargo test` falhando"));
        assert!(recusa.contains("1 problema de gravidade alta do revisor (src/a.rs:7)"));
        // Forçar sem motivo não basta; com motivo, a ressalva diz o que foi ignorado e por quê.
        assert!(portao_aprovacao(&t, true, "  ").is_err());
        let ressalva = portao_aprovacao(&t, true, "já falhava na base").unwrap().unwrap();
        assert!(ressalva.starts_with("verificação `cargo test` falhando;"));
        assert!(ressalva.ends_with("Motivo: já falhava na base"));
        // Sem nada vermelho passa direto, sem ressalva.
        t.ultima_verificacao.as_mut().unwrap().passou = true;
        t.revisao.as_mut().unwrap().problemas.remove(0);
        assert_eq!(portao_aprovacao(&t, false, ""), Ok(None));
        // Antes do relatório desta rodada, nem forçando.
        t.entrega_pendente = Some(false);
        assert!(portao_aprovacao(&t, true, "motivo").is_err());
        for estado in [EstadoTarefa::Rodando, EstadoTarefa::Ajustando, EstadoTarefa::Cancelada] {
            let t = tarefa(5, "w", estado);
            assert!(portao_aprovacao(&t, true, "motivo").unwrap_err().contains("só dá para aprovar"));
        }
    }

    #[test]
    fn ajuste_anexa_o_que_apontou_problema_numerado() {
        let t = entrega_vermelha();
        let (texto, junto) = anexo_do_ajuste(&t, None, &[]);
        assert!(texto.contains("1. [alta] src/a.rs:7: errado"));
        assert!(texto.contains("2. [baixa] src/b.rs: errado"));
        assert!(texto.contains("Verificação `cargo test` FALHOU"));
        // Só as últimas 40 linhas da saída.
        assert!(texto.contains("linha 60") && texto.contains("linha 21\n") && !texto.contains("linha 20\n"));
        assert_eq!(junto, vec!["3 problemas do revisor".to_string(), "a saída da verificação".to_string()]);
        // Falso positivo fora; os outros guardam o número do relatório.
        let (texto, junto) = anexo_do_ajuste(&t, None, &[1]);
        assert!(!texto.contains("src/a.rs") && texto.contains("3. [media] src/c.rs:2"));
        assert_eq!(junto[0], "2 problemas do revisor");
        // Só baixa sobrando: por padrão não vai; pedido, vai.
        assert!(!anexo_do_ajuste(&t, None, &[1, 3]).0.contains("revisor"));
        let so_revisor = ["revisor".to_string()];
        let (texto, _) = anexo_do_ajuste(&t, Some(&so_revisor[..]), &[1, 3]);
        assert!(texto.contains("src/b.rs") && !texto.contains("Verificação"));
        assert_eq!(anexo_do_ajuste(&t, Some(&[][..]), &[]), (String::new(), Vec::new()));
        // Com a rodada ainda sem relatório, o que está guardado é velho: nada vai.
        let mut ajustando = entrega_vermelha();
        ajustando.estado = EstadoTarefa::Ajustando;
        assert!(anexo_do_ajuste(&ajustando, None, &[]).0.is_empty());
    }

    #[test]
    fn lista_das_ferramentas_aceita_array_em_texto() {
        assert_eq!(lista_de(&json!([1, 2])), Some(vec![json!(1), json!(2)]));
        assert_eq!(lista_de(&json!("[3]")), Some(vec![json!(3)]));
        assert_eq!(lista_de(&json!("revisor, verificacao")), Some(vec![json!("revisor"), json!("verificacao")]));
        assert_eq!(lista_de(&Value::Null), None);
    }

    #[test]
    fn limite_vence_na_hora_informada_ou_em_meia_hora() {
        let t = 1_000_000;
        // A hora do Claude Code vale; sem ela, fica a marca que ainda vale, senão meia hora.
        assert_eq!(fim_do_limite(Some(t + 600), Some(t + 60), t), t + 600);
        assert_eq!(fim_do_limite(None, Some(t + 60), t), t + 60);
        assert_eq!(fim_do_limite(None, Some(t - 1), t), t + 1800);
        assert_eq!(fim_do_limite(Some(t), None, t), t + 1800);
        assert!(crate::modelo::quando_local(agora()).starts_with("às "));
        assert!(crate::modelo::quando_local(agora() + 3 * 86_400).starts_with("em "));
    }

    #[test]
    fn nova_tentativa_que_esperou_vaga_nao_repete_o_pedido() {
        let entrada = |texto: &str, registrada| Entrada {
            de: Some("coordenador".into()),
            texto: texto.into(),
            anexos: Vec::new(),
            registrada,
            ..Default::default()
        };
        let itens = itens_das_entradas(
            vec![entrada("Tarefa #1", true), entrada("resposta", false)],
            Some("abc".into()),
        );
        assert_eq!(itens.len(), 1);
        assert!(matches!(
            &itens[0],
            Item::Entrega { texto, ponto: Some(p), .. } if texto == "resposta" && p == "abc"
        ));
        assert!(itens_das_entradas(vec![entrada("Tarefa #1", true)], None).is_empty());
        // Na fila gravada, só a marca verdadeira vai para o disco.
        let gravada = serde_json::to_value(entrada("x", false)).unwrap();
        assert!(gravada.get("registrada").is_none());
        let lida: Entrada = serde_json::from_value(serde_json::to_value(entrada("x", true)).unwrap()).unwrap();
        assert!(lida.registrada);
    }

    #[test]
    fn plano_aprovado_volta_ao_modo_de_antes() {
        let bypass = "bypassPermissions";
        assert_eq!(modo_depois_do_plano(Some("manual"), Some("plan"), bypass), "manual");
        assert_eq!(modo_depois_do_plano(None, Some("plan"), bypass), bypass);
        assert_eq!(modo_depois_do_plano(Some("plan"), Some("plan"), "plan"), "acceptEdits");
        assert_eq!(modo_depois_do_plano(None, None, "plan"), "acceptEdits");
        // O Claude abriu o plano sozinho, ou o modo mudou no meio: fica o da conversa.
        assert_eq!(modo_depois_do_plano(None, Some("manual"), bypass), "manual");
        assert_eq!(modo_depois_do_plano(None, Some("acceptEdits"), "manual"), "acceptEdits");
    }

    #[test]
    fn desarquivar_conversa_velha_nao_volta_a_arquivar() {
        let dia = 86_400;
        let agora = 40 * dia;
        let limite = agora - 7 * dia;
        let mut velha = chat("c", Papel::Coordenador, None);
        velha.atualizado_em = dia;
        velha.lido_em = 2 * dia;
        velha.itens.push(Item::Assistente { texto: "pronto".into() });
        assert!(candidata_a_arquivar(&velha, limite));

        trocar_arquivado(&mut velha, agora);
        assert!(velha.arquivado);
        trocar_arquivado(&mut velha, agora);
        assert!(!velha.arquivado);
        assert!(!candidata_a_arquivar(&velha, limite));

        // A não lida continua não lida ao desarquivar.
        let mut nova = chat("n", Papel::Coordenador, None);
        nova.arquivado = true;
        nova.atualizado_em = 2 * dia;
        nova.lido_em = dia;
        nova.itens.push(Item::Assistente { texto: "resposta".into() });
        trocar_arquivado(&mut nova, agora);
        assert!(!nova.arquivado);
        assert_eq!(nova.lido_em, dia);

        let mut fixada = chat("f", Papel::Coordenador, None);
        fixada.fixado = true;
        assert!(!candidata_a_arquivar(&fixada, limite));
        assert!(!candidata_a_arquivar(&chat("w", Papel::Trabalhador, Some(1)), limite));
    }

    #[test]
    fn esforco_do_ultrathink_sobe_para_extra() {
        for atual in [None, Some(""), Some("low"), Some("medium"), Some("high"), Some("xhigh")] {
            assert_eq!(esforco_do_ultrathink(atual), "xhigh", "{atual:?}");
        }
        assert_eq!(esforco_do_ultrathink(Some("max")), "max");
    }

    const ULTRATHINK: Reforco = Reforco { ultrathink: true, ultracode: false };
    const ULTRACODE: Reforco = Reforco { ultrathink: false, ultracode: true };

    #[test]
    fn ultrathink_entra_uma_vez_no_fim_do_turno() {
        let mut primeira = entrada_do_usuario("arrume o teste".into(), Vec::new(), ULTRATHINK);
        let segunda = entrada_do_usuario("e o lint".into(), Vec::new(), Reforco::default());
        let (texto, _) = texto_do_turno(&[primeira.clone(), segunda.clone()]);
        assert!(texto.ends_with("\n\nultrathink"), "{texto}");
        assert_eq!(texto.matches("ultrathink").count(), 1);
        primeira.ultrathink = false;
        let (texto, _) = texto_do_turno(&[primeira, segunda]);
        assert!(!texto.contains("ultrathink"));
        // Comando de barra não consome a chave (a caixa a mantém ligada).
        assert!(!entrada_do_usuario("/compact".into(), Vec::new(), ULTRATHINK).ultrathink);
    }

    #[test]
    fn ultracode_vai_pelo_esforco_e_nunca_pela_palavra() {
        let ambos = Reforco { ultrathink: true, ultracode: true };
        let entrada = entrada_do_usuario("refatore o motor".into(), Vec::new(), ambos);
        assert!(entrada.ultracode && entrada.ultrathink);
        let (texto, _) = texto_do_turno(&[entrada.clone()]);
        // A palavra "ultracode" no texto só valeria com a mensagem marcada como digitada; o
        // Vitral liga pelo `--effort` e não a escreve.
        assert!(!texto.contains("ultracode"), "{texto}");
        assert!(texto.ends_with("\n\nultrathink"));
        // Um fluxo por vez, e só no turno com Ultracode.
        assert_eq!(texto.matches(UM_FLUXO_POR_VEZ).count(), 1);
        let (comum, _) = texto_do_turno(&[entrada_do_usuario("oi".into(), Vec::new(), ULTRATHINK)]);
        assert!(!comum.contains(UM_FLUXO_POR_VEZ));
        let itens = itens_das_entradas(vec![entrada], None);
        assert!(matches!(&itens[0], Item::Usuario { ultracode: true, ultrathink: true, .. }));
        // Comando de barra não leva (e a caixa continua com a chave ligada).
        assert!(!entrada_do_usuario("/compact".into(), Vec::new(), ULTRACODE).ultracode);
        // Entrega do coordenador ou do Vitral nunca leva Ultracode.
        let mut entrega = entrada_do_usuario("Tarefa #2".into(), Vec::new(), Reforco::default());
        entrega.de = Some("coordenador".into());
        assert!(!entrega.ultracode);
    }

    #[test]
    fn ultracode_na_fila_sobrevive_ao_reinicio() {
        let entrada = entrada_do_usuario("migre os testes".into(), Vec::new(), ULTRACODE);
        let gravada = serde_json::to_value(&entrada).unwrap();
        assert_eq!(gravada["ultracode"], true);
        assert!(gravada.get("ultrathink").is_none());
        let lida: Entrada = serde_json::from_value(gravada).unwrap();
        assert!(lida.ultracode && !lida.ultrathink);
        // Desligado, o campo nem vai para o disco; o histórico guarda o selo.
        let comum = serde_json::to_value(entrada_do_usuario("oi".into(), Vec::new(), Reforco::default())).unwrap();
        assert!(comum.get("ultracode").is_none());
        let item = serde_json::to_value(&itens_das_entradas(vec![lida], None)[0]).unwrap();
        assert_eq!(item["Usuario"]["ultracode"], true);
    }

    #[test]
    fn ultracode_so_onde_ele_liga_de_verdade() {
        let versao = Some((2, 1, 278));
        assert_eq!(motivo_sem_ultracode(Papel::Trabalhador, "opus", versao), None);
        assert_eq!(motivo_sem_ultracode(Papel::Trabalhador, "", None), None);
        assert_eq!(motivo_sem_ultracode(Papel::Coordenador, "opus", versao).as_deref(), Some("a conversa do Vitral antigo é só para leitura"));
        assert_eq!(motivo_sem_ultracode(Papel::Mestre, "opus", versao).as_deref(), Some("só nos chats de projeto"));
        // Antes da 2.1.269 o limite de agentes não existe.
        let antiga = motivo_sem_ultracode(Papel::Trabalhador, "opus", Some((2, 1, 268))).unwrap();
        assert!(antiga.contains("2.1.278") && antiga.contains("2.1.268"), "{antiga}");
        assert!(motivo_sem_ultracode(Papel::Trabalhador, "haiku", versao).unwrap().contains("Extra"));
    }

    #[test]
    fn agentes_cortados_quando_o_processo_sai_antes_deles() {
        // Acabaram e o processo saiu sem outro turno: o fim normal.
        assert_eq!(agentes_cortados(0, Some(0), ""), None);
        assert_eq!(agentes_cortados(0, None, "aviso qualquer"), None);
        let teto = "Background tasks still running after 600s; terminating. Set X=0 to wait indefinitely.";
        assert!(agentes_cortados(0, Some(0), teto).unwrap().contains("tempo máximo"));
        assert!(agentes_cortados(2, Some(0), "").unwrap().contains("2 ainda rodando"));
        let caiu = agentes_cortados(0, Some(1), "linha\nFATAL ERROR: heap out of memory\n").unwrap();
        assert!(caiu.contains("código 1") && caiu.contains("heap out of memory"), "{caiu}");
        assert!(agentes_cortados(1, Some(-1), "").unwrap().contains("derrubado"));
    }

    #[test]
    fn modelos_com_o_esforco_extra() {
        for sim in
            ["", "opus", "sonnet", "fable", "claude-opus-5-5", "claude-opus-4-7", "claude-sonnet-5[1m]", "claude-fable-5-1"]
        {
            assert!(aceita_ultracode(sim), "{sim}");
        }
        for nao in [
            "haiku",
            "claude-haiku-4-5-20251001",
            "claude-opus-4-6",
            "claude-opus-4-20250514",
            "claude-sonnet-4-5[1m]",
            "claude-3-5-sonnet-20241022",
        ] {
            assert!(!aceita_ultracode(nao), "{nao}");
        }
    }

    #[test]
    fn resultado_com_agentes_em_segundo_plano_espera_por_eles() {
        assert!(adiar_o_fim(1, false, false));
        assert!(!adiar_o_fim(0, false, false));
        assert!(!adiar_o_fim(2, true, false), "erro termina o turno");
        assert!(!adiar_o_fim(2, false, true), "parado pelo usuário termina o turno");
    }

    #[test]
    fn a_repeticao_do_turno_leva_o_mesmo_uuid() {
        let nova = Entrada { texto: "abre o PR".into(), ..Default::default() };
        let repetida = Entrada { registrada: true, ..nova.clone() };
        let primeiro = turno_do_lote(None, std::slice::from_ref(&nova));
        assert_eq!(primeiro.len(), 36);
        // "Tentar de novo" e a troca de conta mandam o lote já registrado: o mesmo turno.
        assert_eq!(turno_do_lote(Some(&primeiro), std::slice::from_ref(&repetida)), primeiro);
        // Uma mensagem nova (mesmo com uma nota já registrada junto) é outro turno.
        let nota = Entrada { nota: true, registrada: true, ..Default::default() };
        assert_ne!(turno_do_lote(Some(&primeiro), &[nota, nova.clone()]), primeiro);
        assert_ne!(turno_do_lote(Some(&primeiro), &[]), primeiro);
        assert_ne!(turno_do_lote(None, &[repetida]), primeiro);
    }

    #[test]
    fn entrada_antiga_sem_ultrathink_continua_abrindo() {
        let lida: Entrada = serde_json::from_value(serde_json::json!({"de": null, "texto": "oi"})).unwrap();
        assert!(!lida.ultrathink && !lida.registrada);
        let item: Item = serde_json::from_value(serde_json::json!({"Usuario": {"texto": "oi"}})).unwrap();
        assert!(matches!(item, Item::Usuario { ultrathink: false, .. }));
        // Sem a chave ligada, o campo nem vai para o disco.
        let gravada = serde_json::to_value(entrada_do_usuario("oi".into(), Vec::new(), Reforco::default())).unwrap();
        assert!(gravada.get("ultrathink").is_none());
    }

    #[test]
    fn janela_padrao_pelo_modelo() {
        assert_eq!(janela_padrao(Some("claude-opus-5-5")), 1_000_000);
        assert_eq!(janela_padrao(Some("claude-haiku-4-5")), 200_000);
        assert_eq!(janela_padrao(Some("claude-sonnet-4-5[1m]")), 1_000_000);
        assert_eq!(janela_padrao(None), 200_000);
    }

    #[test]
    fn notificacao_abre_a_conversa_e_substitui_a_anterior() {
        let args = argumentos_do_aviso("loja · Preços", "Terminou", None);
        assert_eq!(
            args.join(" "),
            "-a Vitral -i vitral -h string:desktop-entry:vitral -A default=Abrir -p loja · Preços Terminou"
        );
        let args = argumentos_do_aviso("t", "c", Some(42));
        assert!(args.windows(2).any(|w| w == ["--replace-id", "42"]));
        assert_eq!(&args[args.len() - 2..], ["t", "c"]);
    }

    #[test]
    fn textos_da_notificacao() {
        let bash = json!({"command": "cargo test\n--release", "description": "Roda os testes"});
        assert_eq!(texto_do_pedido("Bash", &bash), "Precisa da sua permissão: Bash, $ cargo test");
        assert_eq!(texto_do_pedido("ExitPlanMode", &json!({})), "Precisa da sua aprovação: o plano está pronto");
        assert_eq!(
            texto_do_pedido("Edit", &json!({"file_path": "/x/src/motor.rs"})),
            "Precisa da sua permissão: Edit, motor.rs"
        );
        assert_eq!(texto_do_fim(""), "Terminou");
        assert_eq!(texto_do_fim("\n  Feito: o bug sumiu.\nresto"), "Terminou: Feito: o bug sumiu.");
    }

    #[test]
    fn espera_e_quem_atender_na_familia() {
        let erro = Execucao::Erro("caiu".into());
        assert_eq!(espera(&Execucao::EsperandoVoce, false, false, true), Some(Espera::Pedido));
        assert_eq!(espera(&erro, false, false, true), Some(Espera::Parada));
        assert_eq!(espera(&erro, true, true, false), Some(Espera::Erro));
        assert_eq!(espera(&Execucao::Parado, false, true, false), None);
        assert_eq!(espera(&Execucao::Parado, true, true, false), Some(Espera::Nova));
        assert_eq!(alvo_da_familia(&[Some(Espera::Nova), Some(Espera::Erro), Some(Espera::Pedido)]), Some(2));
        assert_eq!(alvo_da_familia(&[None, None]), None);
    }

    #[test]
    fn parcial_so_redesenha_no_primeiro_pedaco_ou_com_o_chat_aberto() {
        let mut parcial = HashMap::new();
        let visiveis = HashSet::from(["aberto".to_string()]);
        assert!(juntar_parcial(&mut parcial, &visiveis, "fundo", "Ol"));
        assert!(!juntar_parcial(&mut parcial, &visiveis, "fundo", "á"));
        assert_eq!(parcial["fundo"], "Olá");
        assert!(juntar_parcial(&mut parcial, &visiveis, "aberto", "a"));
        assert!(juntar_parcial(&mut parcial, &visiveis, "aberto", "b"));
    }

    #[test]
    fn precisa_de_voce_conta_pedidos_erros_e_paradas_e_nao_as_novas() {
        // Demonstração: 2 pedidos, 1 erro e 1 parada dão 4, em vermelho; as novas são só ponto.
        let mut conta = PrecisaDeVoce::default();
        conta.somar(Espera::Pedido, false);
        conta.somar(Espera::Pedido, true);
        conta.somar(Espera::Erro, true);
        conta.somar(Espera::Parada, false);
        conta.somar(Espera::Nova, false);
        assert_eq!(conta.numero(), 4);
        assert!(conta.erro);
        assert_eq!(conta.novas, 1);
        let so_novas = { let mut c = PrecisaDeVoce::default(); c.somar(Espera::Nova, false); c };
        assert_eq!(so_novas.numero(), 0);
    }

    #[test]
    fn apagar_tira_do_estado_so_o_que_e_do_projeto() {
        let no = |id: &str, projeto: &str, papel: Papel, pai: Option<&str>| {
            let mut c = chat(id, papel, None);
            c.projeto = projeto.into();
            c.pai = pai.map(Into::into);
            c
        };
        let tarefa_de = |id: u64, projeto: &str, coordenador: &str, trabalhador: &str, copia: Option<&str>| {
            let mut t = tarefa(id, trabalhador, EstadoTarefa::EmRevisao);
            (t.projeto, t.coordenador, t.worktree) = (projeto.into(), coordenador.into(), copia.map(PathBuf::from));
            t
        };
        let mut chats = vec![
            no("c1", "loja", Papel::Coordenador, None),
            no("c2", "site", Papel::Coordenador, None),
            no("w1", "loja", Papel::Trabalhador, Some("c1")),
            // Um trabalhador gravado com outro projeto ainda é da tarefa da loja.
            no("w3", "outro", Papel::Trabalhador, Some("c1")),
            no("w2", "site", Papel::Trabalhador, Some("c2")),
            // Um ajudante de um chat da loja, sem tarefa: vai junto pelo pai.
            no("a1", "outro", Papel::Trabalhador, Some("c1")),
        ];
        let mut tarefas = vec![
            tarefa_de(1, "loja", "c1", "w1", Some("/d/copias/loja/t1")),
            tarefa_de(2, "site", "c2", "w2", Some("/d/copias/site/t2")),
            tarefa_de(3, "loja", "c1", "w3", None),
        ];
        let mut filas = Filas::new();
        for id in ["c1", "w1", "c2"] {
            filas.insert(id.into(), Vec::new());
        }
        let mut rotinas: Vec<Rotina> = [("r1", "loja"), ("r2", "site"), ("r3", "loja")]
            .into_iter()
            .map(|(id, projeto)| Rotina { id: id.into(), projeto: projeto.into(), ..Default::default() })
            .collect();
        let parte = |projeto: &str| crate::modelo::Parte { projeto: projeto.into(), ..Default::default() };
        let mut cartoes = vec![Cartao {
            id: 1,
            chat: crate::modelo::ID_MESTRE.into(),
            criado_em: 0,
            decidido_em: 0,
            estado: crate::modelo::EstadoCartao::Aberto,
            tipo: TipoCartao::Divisao(crate::modelo::Divisao { partes: vec![parte("loja"), parte("site")], ..Default::default() }),
            resto: Default::default(),
        }];

        let retirado = tirar_do_estado(&mut chats, &mut tarefas, &mut filas, &mut rotinas, &mut cartoes, "loja");
        assert_eq!(retirado.chats, ["c1", "w1", "w3", "a1"]);
        assert_eq!(retirado.tarefas, [1, 3]);
        assert_eq!(retirado.copias, [PathBuf::from("/d/copias/loja/t1")]);
        assert_eq!(retirado.rotinas, 2);
        let ids: Vec<&str> = chats.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["c2", "w2"]);
        assert_eq!(tarefas.iter().map(|t| t.id).collect::<Vec<_>>(), [2]);
        assert_eq!(filas.keys().collect::<Vec<_>>(), ["c2"]);
        assert_eq!(rotinas.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["r2"]);
        let TipoCartao::Divisao(d) = &cartoes[0].tipo else { unreachable!() };
        assert_eq!(d.partes[0].falha.as_deref(), Some("projeto apagado"));
        assert!(d.partes[1].falha.is_none());

        // Um projeto sem nada no estado não mexe em nada.
        let antes = (chats.len(), tarefas.len(), rotinas.len());
        let nada = tirar_do_estado(&mut chats, &mut tarefas, &mut filas, &mut rotinas, &mut cartoes, "nada");
        assert_eq!(nada, Retirado::default());
        assert_eq!((chats.len(), tarefas.len(), rotinas.len()), antes);
    }

    #[test]
    fn so_a_pasta_propria_sem_pasta_ligada_vai_para_a_lixeira() {
        let projetos = Path::new("/d/projetos");
        let pasta = |caminho: &str, propria: bool| PastaLigada { caminho: caminho.into(), propria, ..Default::default() };
        let com = |pastas: Vec<PastaLigada>| Projeto { id: "loja".into(), pastas, ..Default::default() };
        // Ligada e própria: nenhuma das duas.
        let as_duas = com(vec![pasta("/home/v/Projetos/loja", false), pasta("/d/projetos/loja", true)]);
        assert_eq!(pasta_propria_para_lixeira(&as_duas, projetos), Ok(None));
        // Só a ligada: nada.
        assert_eq!(pasta_propria_para_lixeira(&com(vec![pasta("/home/v/Projetos/loja", false)]), projetos), Ok(None));
        // Só a própria: ela.
        let so_propria = com(vec![pasta("/d/projetos/loja", true)]);
        assert_eq!(pasta_propria_para_lixeira(&so_propria, projetos), Ok(Some(PathBuf::from("/d/projetos/loja"))));
        // Sem pasta nenhuma (provisório): nada.
        assert_eq!(pasta_propria_para_lixeira(&com(Vec::new()), projetos), Ok(None));
        // Uma "própria" fora da pasta dos projetos, ou a própria pasta dos projetos, nunca.
        for ruim in ["/home/v/Projetos/loja", "/d/projetos", "/d/projetos/.loja.criando", "/d/projetos/a/b"] {
            assert!(pasta_propria_para_lixeira(&com(vec![pasta(ruim, true)]), projetos).is_err(), "{ruim}");
        }
    }

    fn projeto(id: &str, nome: &str, ordem: u32) -> Projeto {
        Projeto { id: id.into(), nome: nome.into(), ordem, ..Default::default() }
    }

    #[test]
    fn projeto_arquivado_leva_as_conversas() {
        let projetos = vec![Projeto { arquivado: true, ..projeto("site", "site", 0) }, projeto("loja", "loja", 1)];
        let mut c = chat("c", Papel::Coordenador, None);
        c.projeto = "loja".into();
        assert!(!conversa_arquivada(&projetos, &c));
        c.arquivado = true;
        assert!(conversa_arquivada(&projetos, &c));
        (c.arquivado, c.projeto) = (false, "site".into());
        assert!(conversa_arquivada(&projetos, &c));
        c.projeto = "sumiu".into();
        assert!(!conversa_arquivada(&projetos, &c));
    }

    #[test]
    fn projeto_pelo_id_pelo_nome_e_pela_chave() {
        let projetos = vec![
            projeto("a1b2c3", "Loja Nova", 0),
            projeto("site", "Site", 1),
            projeto("x9", "Ação", 2),
            projeto("y9", "acao", 3),
        ];
        assert_eq!(resolver_projeto(&projetos, "a1b2c3"), Resolucao::Um("a1b2c3".into()));
        assert_eq!(resolver_projeto(&projetos, "Loja Nova"), Resolucao::Um("a1b2c3".into()));
        assert_eq!(resolver_projeto(&projetos, "#loja-nova"), Resolucao::Um("a1b2c3".into()));
        assert_eq!(resolver_projeto(&projetos, "SITE"), Resolucao::Um("site".into()));
        // O nome exato desempata; só a chave, não.
        assert_eq!(resolver_projeto(&projetos, "Ação"), Resolucao::Um("x9".into()));
        assert_eq!(resolver_projeto(&projetos, "ACAO"), Resolucao::Varios(vec!["x9".into(), "y9".into()]));
        assert_eq!(resolver_projeto(&projetos, "blog"), Resolucao::Nenhum);
        assert_eq!(resolver_projeto(&projetos, "!!"), Resolucao::Nenhum);
    }

    #[test]
    fn reordenar_poe_antes_ou_depois_e_renumera() {
        let mut projetos = vec![projeto("a", "a", 0), projeto("b", "b", 1), projeto("c", "c", 2), projeto("d", "d", 7)];
        let ordem = |p: &[Projeto]| {
            let mut v: Vec<(u32, &str)> = p.iter().map(|p| (p.ordem, p.id.as_str())).collect();
            v.sort();
            v.into_iter().map(|(_, id)| id.to_string()).collect::<Vec<_>>()
        };
        assert!(reordenar(&mut projetos, "d", "a", true));
        assert_eq!(ordem(&projetos), ["d", "a", "b", "c"]);
        assert!(reordenar(&mut projetos, "d", "c", false));
        assert_eq!(ordem(&projetos), ["a", "b", "c", "d"]);
        assert_eq!(projetos.iter().map(|p| p.ordem).collect::<Vec<_>>(), [0, 1, 2, 3]);
        assert!(!reordenar(&mut projetos, "a", "a", true));
        assert!(!reordenar(&mut projetos, "a", "sumiu", true));
        assert!(!reordenar(&mut projetos, "b", "c", true), "já estava antes");
    }

    #[test]
    fn arquivar_apaga_so_o_chat_de_projeto_que_nunca_rodou() {
        let mut c = Chat::novo("c".into(), Papel::Projeto, "loja", "Nova conversa", "/p/loja".into());
        c.itens.push(Item::Aviso { texto: "A pasta não usa git: o chat trabalha direto nela.".into(), erro: false });
        let conversa = Tarefa { id: 1, trabalhador: "c".into(), estado: EstadoTarefa::Conversa, ..Default::default() };
        assert!(nunca_rodou(&c, &[&conversa]));
        let com_copia = Tarefa { worktree: Some("/d/copias/loja/t1".into()), ..conversa.clone() };
        assert!(!nunca_rodou(&c, &[&com_copia]), "a cópia já nasceu");
        let pedido = Tarefa { estado: EstadoTarefa::Rodando, coordenador: "mestre".into(), ..conversa.clone() };
        assert!(!nunca_rodou(&c, &[&pedido]), "pedido do Mestre aberto");
        let mut falou = c.clone();
        falou.itens.push(Item::Usuario { texto: "oi".into(), anexos: Vec::new(), ponto: None, ultrathink: false, ultracode: false, referencias: Vec::new() });
        assert!(!nunca_rodou(&falou, &[&conversa]));
        let mut sessao = c.clone();
        sessao.sessao_criada = true;
        assert!(!nunca_rodou(&sessao, &[&conversa]));
        let antigo = Chat { papel: Papel::Coordenador, ..c.clone() };
        assert!(!nunca_rodou(&antigo, &[]), "só o chat de projeto");
        let trazida = Chat { importada: Some(Default::default()), ..c.clone() };
        assert!(!nunca_rodou(&trazida, &[&conversa]), "a conversa do Claude Code arquiva, não some");
    }

    #[test]
    fn mudar_projeto_so_mexe_no_que_veio() {
        let mut p = Projeto {
            icone: "store".into(),
            descricao: "antiga".into(),
            pastas: vec![PastaLigada { caminho: "/p/loja".into(), ..Default::default() }],
            ..projeto("loja", "loja", 0)
        };
        aplicar_mudanca(&mut p, MudancaProjeto { nome: Some(" Loja ".into()), ..Default::default() });
        assert_eq!((p.nome.as_str(), p.icone.as_str(), p.descricao.as_str()), ("Loja", "store", "antiga"));
        let config = ConfigProjeto { preparacao: "npm i".into(), ..Default::default() };
        aplicar_mudanca(&mut p, MudancaProjeto { pasta: Some((0, config)), instrucoes: Some("x".repeat(9000)), ..Default::default() });
        assert_eq!(p.pastas[0].config.preparacao, "npm i");
        assert_eq!(p.pastas[0].caminho, PathBuf::from("/p/loja"));
        assert_eq!(p.instrucoes.chars().count(), 8000);
        aplicar_mudanca(&mut p, MudancaProjeto { pasta: Some((5, ConfigProjeto::default())), ..Default::default() });
        assert_eq!(p.pastas.len(), 1);
        let ligada = |c: &str| PastaLigada { caminho: c.into(), ..Default::default() };
        p.provisorio = true;
        p.ordem = 3;
        let mudanca = MudancaProjeto {
            pastas: Some(vec![ligada("/p/site"), ligada("/p/loja")]),
            permissao: Some(Some("auto".into())),
            modelo: Some(Some(" ".into())),
            ..Default::default()
        };
        aplicar_mudanca(&mut p, mudanca);
        assert_eq!(p.pastas.iter().map(|l| l.caminho.clone()).collect::<Vec<_>>(), [PathBuf::from("/p/site"), "/p/loja".into()]);
        assert!(!p.provisorio, "com pasta, deixa de ser provisório");
        assert_eq!((p.permissao.as_deref(), p.modelo.as_deref(), p.ordem), (Some("auto"), None, 3));
        aplicar_mudanca(&mut p, MudancaProjeto { permissao: Some(None), ..Default::default() });
        assert_eq!((p.permissao.as_deref(), p.pastas.len()), (None, 2));
    }

    #[test]
    fn mover_leva_os_descendentes() {
        let filho = |id: &str, pai: &str| {
            let mut c = chat(id, Papel::Trabalhador, None);
            c.pai = Some(pai.into());
            c
        };
        let chats = vec![chat("c", Papel::Coordenador, None), filho("w1", "c"), filho("a1", "w1"), filho("w9", "outro")];
        assert_eq!(descendentes(&chats, "c"), ["c", "w1", "a1"]);
        assert_eq!(descendentes(&chats, "w1"), ["w1", "a1"]);
        assert_eq!(descendentes(&chats, "sozinho"), ["sozinho"]);
    }

    #[test]
    fn area_das_copias_novas_e_antigas() {
        let copias = Path::new("/d/copias");
        let com = |w: Option<&str>| Tarefa { projeto: "loja".into(), worktree: w.map(PathBuf::from), ..Default::default() };
        assert_eq!(area_da_copia(copias, &com(Some("/d/copias/loja/t3"))), PathBuf::from("/d/copias/loja"));
        assert_eq!(
            area_da_copia(copias, &com(Some("/home/v/Projetos/.vitral/worktrees/loja/t3"))),
            PathBuf::from("/home/v/Projetos/.vitral/worktrees/loja")
        );
        // Uma pasta qualquer não vira área: a limpeza não apaga nada à mão nela.
        assert_eq!(area_da_copia(copias, &com(Some("/home/v/outra/t3"))), PathBuf::from("/d/copias/loja"));
        assert_eq!(area_da_copia(copias, &com(None)), PathBuf::from("/d/copias/loja"));
        assert_eq!(porta_de(&Tarefa { id: 2, porta: 41_010, ..Default::default() }), 41_010);
        assert_eq!(porta_de(&Tarefa { id: 2, ..Default::default() }), porta_da_tarefa(2));
    }

    #[test]
    fn passagem_de_sessao_leva_as_ultimas_mensagens_e_o_pedido() {
        let usuario = |t: &str| Item::Usuario { texto: t.into(), anexos: Vec::new(), ponto: None, ultrathink: false, ultracode: false, referencias: Vec::new() };
        let mut itens = vec![usuario("primeira")];
        for i in 0..12 {
            itens.push(Item::Assistente { texto: format!("resposta {i}") });
            itens.push(Item::Aviso { texto: "aviso".into(), erro: false });
        }
        itens.push(usuario("a de agora"));
        let mut t = tarefa(4, "w", EstadoTarefa::Ajustando);
        (t.titulo, t.branch, t.worktree) = ("Cupons".into(), Some("vitral/t4-cupons".into()), Some("/d/copias/loja/t4".into()));
        let bloco = bloco_de_passagem(&itens, Some(&t), 1);
        assert!(bloco.starts_with("[Vitral] Passagem de sessão"));
        // As 10 mais recentes, em ordem, sem os avisos e sem a do próprio turno.
        assert!(!bloco.contains("a de agora") && !bloco.contains("primeira") && !bloco.contains("aviso"));
        assert!(bloco.contains("Você: resposta 2") && !bloco.contains("resposta 1\n"));
        assert!(bloco.find("resposta 2").unwrap() < bloco.find("resposta 11").unwrap());
        assert!(bloco.ends_with("Tarefa em aberto: #4 Cupons (ajustando). A cópia é /d/copias/loja/t4, no branch vitral/t4-cupons."));
        // Mensagens longas: as mais recentes ficam, até 6000 caracteres.
        let longas = vec![usuario(&"a".repeat(5000)), Item::Assistente { texto: "b".repeat(5000) }];
        let bloco = bloco_de_passagem(&longas, None, 0);
        assert!(bloco.contains(&format!("Você: {}", "b".repeat(5000))));
        assert!(bloco.contains("…") && !bloco.contains(&"a".repeat(1000)));
    }

    #[test]
    fn ocupacao_livre_so_sem_conversa_nem_tarefa() {
        assert!(Ocupacao::default().livre());
        assert!(!Ocupacao { conversas: vec!["c1".into()], tarefas: Vec::new() }.livre());
        assert!(!Ocupacao { conversas: Vec::new(), tarefas: vec![4] }.livre());
    }

    fn regras(pastas: &[&str]) -> Vec<String> {
        pastas
            .iter()
            .flat_map(|p| ["Edit", "Write", "NotebookEdit"].map(|f| format!("{f}(/{p}/**)")))
            .collect()
    }

    #[test]
    fn bloqueios_do_chat_de_projeto() {
        let pastas: Vec<PathBuf> = ["/p/loja", "/p/docs"].map(PathBuf::from).to_vec();
        let (origem, copia) = (Path::new("/p/loja"), Path::new("/d/copias/loja/t3"));
        // Na cópia: a origem e as outras pastas ficam só para leitura; a cópia, não.
        assert_eq!(regras_de_bloqueio(Forma::Copia, origem, copia, &pastas, None), regras(&["/p/loja", "/p/docs"]));
        // Direto: a própria pasta fica livre.
        assert_eq!(regras_de_bloqueio(Forma::Direta, origem, origem, &pastas, None), regras(&["/p/docs"]));
        assert_eq!(regras_de_bloqueio(Forma::DiretaGit, origem, origem, &pastas, None), regras(&["/p/docs"]));
        // Movido para um projeto sem a origem (ou com ela desligada do projeto): a origem continua.
        let outras: Vec<PathBuf> = vec![PathBuf::from("/p/site")];
        assert_eq!(regras_de_bloqueio(Forma::Copia, origem, copia, &outras, None), regras(&["/p/loja", "/p/site"]));
        // Direto numa pasta que saiu do projeto: só as do projeto.
        assert_eq!(regras_de_bloqueio(Forma::Direta, origem, origem, &outras, None), regras(&["/p/site"]));
        // Uma pasta ligada que contém a do chat direto não o bloqueia nela.
        let aninhadas: Vec<PathBuf> = ["/p", "/p/loja"].map(PathBuf::from).to_vec();
        assert!(regras_de_bloqueio(Forma::Direta, origem, origem, &aninhadas, None).is_empty());
        // O avulso movido só de grupo: todas as pastas do projeto.
        let avulso = Path::new("/d/avulsos/x");
        assert_eq!(regras_de_bloqueio(Forma::Direta, avulso, avulso, &pastas, None), regras(&["/p/loja", "/p/docs"]));
        // O ajudante: também a cópia e a origem do pai.
        let pai = (Path::new("/d/copias/loja/t3"), Path::new("/p/loja"));
        let ajudante = regras_de_bloqueio(Forma::Copia, copia, Path::new("/d/copias/loja/t9"), &[], Some(pai));
        assert_eq!(ajudante, regras(&["/d/copias/loja/t3", "/p/loja"]));
    }

    #[test]
    fn ajudante_so_no_chat_de_projeto_com_git_e_abaixo_do_limite() {
        let com = |forma: Forma, preparada: bool| Tarefa { forma, preparada, ..Default::default() };
        let copia = com(Forma::Copia, true);
        assert_eq!(pode_criar_ajudante(Papel::Projeto, Some(&copia), 2, 3), Ok(()));
        assert_eq!(pode_criar_ajudante(Papel::Projeto, Some(&com(Forma::DiretaGit, true)), 0, 3), Ok(()));
        // Sem git (pasta sem git, avulso), o ajudante não teria cópia.
        assert!(pode_criar_ajudante(Papel::Projeto, Some(&com(Forma::Direta, true)), 0, 3).unwrap_err().contains("sem git"));
        assert!(pode_criar_ajudante(Papel::Projeto, Some(&com(Forma::Copia, false)), 0, 3).unwrap_err().contains("não está pronta"));
        assert!(pode_criar_ajudante(Papel::Projeto, Some(&copia), 3, 3).unwrap_err().contains("3 ajudantes abertos"));
        assert!(pode_criar_ajudante(Papel::Projeto, Some(&copia), 0, 0).unwrap_err().contains("desligados"));
        assert!(pode_criar_ajudante(Papel::Projeto, None, 0, 3).is_err());
        // O ajudante não cria ajudante, e o Mestre e a conversa antiga também não.
        for papel in [Papel::Trabalhador, Papel::Mestre, Papel::Coordenador] {
            assert!(pode_criar_ajudante(papel, Some(&copia), 0, 3).is_err(), "{papel:?}");
        }
    }

    #[test]
    fn vagas_com_uma_a_mais_para_o_usuario() {
        assert!(pode_comecar(Papel::Projeto, false, 1, 2));
        assert!(!pode_comecar(Papel::Projeto, false, 2, 2));
        assert!(pode_comecar(Papel::Projeto, true, 2, 2), "o usuário cabe na vaga a mais");
        assert!(!pode_comecar(Papel::Trabalhador, true, 3, 2));
        assert!(pode_comecar(Papel::Mestre, false, 9, 2), "o Mestre não conta");
        assert!(pode_comecar(Papel::Coordenador, false, 9, 2));
    }

    #[test]
    fn um_escritor_por_pasta_pelos_caminhos() {
        let ativos: Vec<PathBuf> = ["/p/loja", "/p/site/app"].map(PathBuf::from).to_vec();
        assert_eq!(escritor_ocupado(Path::new("/p/loja"), &ativos), Some(PathBuf::from("/p/loja")));
        assert_eq!(escritor_ocupado(Path::new("/p/loja/src"), &ativos), Some(PathBuf::from("/p/loja")), "dentro");
        assert_eq!(escritor_ocupado(Path::new("/p/site"), &ativos), Some(PathBuf::from("/p/site/app")), "por fora");
        assert_eq!(escritor_ocupado(Path::new("/p/lojinha"), &ativos), None, "prefixo do nome não conta");
        assert_eq!(escritor_ocupado(Path::new("/p/outra"), &ativos), None);
        // A pasta pessoal não trava nem é travada (as conversas do Claude Code rodam nela juntas).
        if let Some(casa) = dirs::home_dir() {
            let dentro = casa.join(".local/share/vitral/avulsos/c1");
            assert_eq!(escritor_ocupado(&casa, &[casa.clone(), dentro.clone()]), None);
            assert_eq!(escritor_ocupado(&dentro, &[casa.clone()]), None);
            assert_eq!(escritor_ocupado(&dentro, &[dentro.clone()]), Some(dentro));
        }
    }

    #[test]
    fn guarda_da_copia_preparando_depois_da_queda_e_com_falha() {
        let mut t = Tarefa { estado: EstadoTarefa::Conversa, preparada: false, ..tarefa(5, "w", EstadoTarefa::Conversa) };
        assert_eq!(guarda_da_copia(&t, false), GuardaCopia::Preparar, "a primeira mensagem prepara");
        assert_eq!(guarda_da_copia(&t, true), GuardaCopia::Esperar, "a segunda espera a que já está sendo preparada");
        t.falha_copia = Some("disco cheio".into());
        assert_eq!(guarda_da_copia(&t, false), GuardaCopia::Preparar, "depois de uma falha, a próxima tenta de novo");
        t.preparada = true;
        t.worktree = Some("/d/copias/p/t5".into());
        assert_eq!(guarda_da_copia(&t, false), GuardaCopia::Segue);
        t.copia_limpa = true;
        assert_eq!(guarda_da_copia(&t, false), GuardaCopia::Esperar, "a cópia está sendo apagada");
        let direta = Tarefa { forma: Forma::Direta, preparada: false, ..tarefa(6, "w", EstadoTarefa::Conversa) };
        assert_eq!(guarda_da_copia(&direta, false), GuardaCopia::Segue);

        // O Vitral fechou com a mensagem esperando a cópia: ela é preparada de novo, uma vez.
        let mut chats = vec![chat("w", Papel::Projeto, Some(5))];
        chats[0].sessao_criada = false;
        let mut tarefas = vec![Tarefa { preparada: false, progresso: Some(NA_FILA_PREPARAR.into()), ..tarefa(5, "w", EstadoTarefa::Conversa) }];
        let mut filas = Filas::new();
        filas.insert("w".into(), vec![Entrada { texto: "oi".into(), ..Default::default() }]);
        let (_, preparos) = marcar_interrompidas(&mut chats, &mut tarefas, &filas);
        assert_eq!(preparos, VecDeque::from([5]));
        assert_eq!(avisos(&chats[0]), 1);
        assert!(precisa_preparar(&tarefas[0], &chats[0], &filas));
        // Caiu de novo na refeita: a mensagem fica guardada e a próxima prepara.
        tarefas[0].parada = Some(PARADA_PREPARACAO.into());
        let (_, preparos) = marcar_interrompidas(&mut chats, &mut tarefas, &filas);
        assert!(preparos.is_empty());
        assert_eq!((tarefas[0].progresso.as_deref(), avisos(&chats[0])), (None, 1));
        // Sem mensagem esperando, não há o que preparar na abertura.
        assert!(!precisa_preparar(&tarefas[0], &chats[0], &Filas::new()));
    }

    #[test]
    fn revisor_sem_pedido_le_as_mensagens_desde_a_juncao() {
        let usuario = |t: &str| Item::Usuario { texto: t.into(), anexos: Vec::new(), ponto: None, ultrathink: false, ultracode: false, referencias: Vec::new() };
        let mut itens = vec![usuario("antes"), Item::Aviso { texto: format!("{JUNTOU} /p/loja."), erro: false }];
        assert_eq!(mensagens_desde_a_juncao(&itens), "(nenhuma mensagem do usuário desde a última junção)");
        itens.extend([usuario("troque o frete"), Item::Assistente { texto: "feito".into() }, usuario("/compact"), usuario("e o cupom")]);
        assert_eq!(mensagens_desde_a_juncao(&itens), "- troque o frete\n- e o cupom");
        assert_eq!(mensagens_desde_a_juncao(&itens[..1]), "- antes");
    }

    #[test]
    fn passagem_de_sessao_do_chat_em_conversa_fala_da_copia() {
        let mut t = tarefa(7, "w", EstadoTarefa::Conversa);
        (t.branch, t.worktree) = (Some("vitral/t7-frete".into()), Some("/d/copias/loja/t7".into()));
        t.mudancas = Some(crate::modelo::Mudancas { arquivos: 2, mais: 12, menos: 3, em: 0 });
        let bloco = bloco_de_passagem(&[], Some(&t), 0);
        assert!(!bloco.contains("Tarefa em aberto"), "{bloco}");
        assert!(bloco.ends_with(
            "\n\nA cópia é /d/copias/loja/t7, no branch vitral/t7-frete. Ela tem 2 arquivos mudados que ainda não \
             entraram na origem (+12 −3)."
        ));
    }

    #[test]
    fn so_o_usuario_conta_como_do_usuario() {
        assert!(Entrada { texto: "oi".into(), ..Default::default() }.do_usuario());
        assert!(!Entrada { de: Some("Vitral".into()), ..Default::default() }.do_usuario());
        assert!(!Entrada { rotina: Some("r".into()), ..Default::default() }.do_usuario());
        assert!(!Entrada { remetente: Some("mestre".into()), ..Default::default() }.do_usuario());
    }

    #[test]
    fn busca_nova_nao_e_desfeita_pelo_cache_velho() {
        let por_modelo = |f: f64| vec![("Opus".to_string(), f, None)];
        let cache = (
            Uso { cinco_horas: Some(0.07), semana: Some(0.38), medido_em: Some(1000), ..Uso::default() },
            Plano { nome: Some("Max 20x".into()), por_modelo: por_modelo(0.1), medido_em: Some(1000), ..Plano::default() },
        );
        let busca = (
            Uso { cinco_horas: Some(0.42), semana: Some(0.61), medido_em: Some(2000), ..Uso::default() },
            Plano { nome: Some("Max 20x".into()), por_modelo: por_modelo(0.2), medido_em: Some(2000), ..Plano::default() },
        );
        let uso = cache.0.juntar(busca.0, 2000);
        let plano = cache.1.clone().juntar(busca.1.clone());
        assert_eq!((uso.cinco_horas, uso.medido_em), (Some(0.42), Some(2000)));
        assert_eq!(plano.por_modelo, por_modelo(0.2));
        assert_eq!(Mudanca::entre((cache.0, uso), (Some(&cache.1), &plano)), Mudanca::Numeros);
        // A releitura do cache, mais velho, não desfaz a busca; o nome do plano vem dele.
        let releitura = Plano { nome: Some("Pro".into()), ..cache.1.clone() };
        let depois = plano.clone().juntar(releitura);
        assert_eq!((depois.por_modelo.clone(), depois.medido_em), (por_modelo(0.2), Some(2000)));
        assert_eq!(depois.nome.as_deref(), Some("Pro"));
        assert_eq!(uso.juntar(cache.0, 2100), uso);
        assert_eq!(Mudanca::entre((uso, uso), (Some(&plano), &plano)), Mudanca::Nada);
        // Os mesmos números um minuto depois: só a hora muda (o trilho não redesenha).
        let de_novo = Uso { medido_em: Some(2060), ..busca.0 };
        let plano_de_novo = Plano { medido_em: Some(2060), ..busca.1.clone() };
        let junto = uso.juntar(de_novo, 2060);
        let plano_junto = plano.clone().juntar(plano_de_novo);
        assert_eq!(Mudanca::entre((uso, junto), (Some(&plano), &plano_junto)), Mudanca::Hora);
        // A primeira leitura de uma conta é número novo.
        assert_eq!(Mudanca::entre((Uso::default(), uso), (None, &plano)), Mudanca::Numeros);
    }
}
