//! O motor do Vitral: guarda os chats, roda os processos do Claude Code (um por turno, direto na
//! pasta de cada chat) e atende as ferramentas MCP.
//!
//! Vive numa única entidade da interface (`Entity<Motor>`). Os processos e o servidor MCP
//! rodam em threads e mandam `Sinal`s por um canal; o motor trata cada um na thread da
//! interface e as telas redesenham observando a entidade.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui_kit::{AnyWeakEntity, AppContext as _, Context, Task};
use serde_json::{Value, json};
use smol::channel::{Sender, unbounded};

mod importacao;
mod conversas;
mod ideias;
mod agendadas;
pub mod gasto;
mod memoria;
mod pedidos_de_hook;
pub mod revisao_do_par;
mod saidas;
pub use gasto::{Medida, Tokens, medida_do_resultado, tokens_do_codex, tokens_do_resultado};
mod bifurcar;
pub mod subconversa;
pub mod fundo;
pub mod passos;
mod ajustes;
mod cli_fora;
pub use ideias::SituacaoDoLinear;

pub use conversas::ResumoVolta;
use conversas::{Ciclos, MotivoPassagem};

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
        Agente, Chat, Config, Entrada, Estado, EstadoRef, Execucao, Filas, Item, Ocorrencia, Parceiro, Reforco, agora,
        novo_id,
    },
    prompts,
};

const CORTADO: &str = "o Vitral foi fechado no meio do turno";

/// Por quanto tempo, depois de um Esc, a faixa oferece "Continuar" (item 6.2).
const PARADA_POR_VOCE_VALE: u64 = 10 * 60;

enum Sinal {
    /// Evento do processo de um chat; o número separa um processo do seguinte do mesmo chat.
    Claude(String, u64, Evento),
    Mcp(Chamada),
    /// O clique numa notificação do sistema: abrir a conversa.
    Abrir(String),
    /// Um aviso do Windows (toast) a mostrar: o GPUI só mostra pela thread da janela.
    #[cfg(windows)]
    Aviso(gpui_kit::SystemNotification),
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
    /// Um hook `PreToolUse` do usuário mandou perguntar, com este motivo (vazio se ele não deu):
    /// ver `pedidos_de_hook`.
    pub motivo_do_hook: Option<String>,
    /// O id da chamada que espera a resposta (o `tool_use_id` que o Claude Code manda junto).
    chamada: Option<String>,
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
    /// Resposta que você ainda não viu.
    Nova,
}

/// A espera de um chat pelos fatos dele: o pedido vem antes da parada, do erro e da resposta
/// nova.
pub fn espera(execucao: &Execucao, nao_lido: bool, parada: bool) -> Option<Espera> {
    match execucao {
        Execucao::EsperandoVoce => Some(Espera::Pedido),
        _ if parada => Some(Espera::Parada),
        Execucao::Erro(_) => Some(Espera::Erro),
        _ if nao_lido => Some(Espera::Nova),
        _ => None,
    }
}

/// O que precisa de você, contando as conversas como os cartões de "Esperando você". O número é o
/// mesmo no sino, na seção e no modo recolhido.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrecisaDeVoce {
    pub pedidos: usize,
    pub paradas: usize,
    pub erros: usize,
    /// Respostas novas viram ponto, nunca número.
    pub novas: usize,
    /// Alguém falhou.
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
    /// A pergunta com opções (a ferramenta AskUserQuestion): o texto da primeira.
    Pergunta { pedido: String, chat: String, pergunta: String },
    Erro { chat: String, texto: String, pode_tentar: bool },
    Parada { chat: String, motivo: String },
    Nova { chat: String },
}

impl Pendencia {
    pub fn chat(&self) -> &str {
        match self {
            Pendencia::Permissao { chat, .. }
            | Pendencia::Plano { chat, .. }
            | Pendencia::Pergunta { chat, .. }
            | Pendencia::Erro { chat, .. }
            | Pendencia::Parada { chat, .. }
            | Pendencia::Nova { chat } => chat,
        }
    }
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

pub struct Motor {
    pub dados: Dados,
    pub config: Config,
    pub chats: Vec<Chat>,
    ocorrencias: Vec<Ocorrencia>,
    visto_em: u64,
    /// Campos do estado de versões mais novas, regravados como vieram.
    resto_estado: serde_json::Map<String, Value>,
    somente_leitura: Option<SomenteLeitura>,
    /// Uma gravação falhou ("caminho: erro") e desde quando: nada novo começa até voltar.
    pub falha_gravacao: Option<(String, u64)>,
    /// O que avisar ao abrir (estado restaurado, a migração, o que mudou nos chats).
    pub avisos_ao_abrir: Vec<String>,
    /// O quadro de ideias (`ideias.json`; ver `motor/ideias.rs`).
    quadro: crate::ideias::Quadro,
    /// O texto de um cartão mudou na edição: vai para o disco na próxima passada de `gravar_sujos`.
    quadro_sujo: bool,
    /// O Linear do quadro (a chave, o time e o que está a caminho).
    linear: ideias::EstadoDoLinear,
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
    /// As últimas leituras do uso de cada conta, para a previsão de quando o limite acaba.
    historico_uso: HashMap<String, contas::previsao::Historico>,
    /// O livro dos turnos terminados: custo e tokens por chat e por pasta (`motor/gasto.rs`).
    gastos: gasto::Livro,
    /// A memória de cada chat trabalhando, lida do escopo dele (`motor/memoria.rs`).
    memorias: memoria::Leituras,
    /// O que os hooks do usuário mandaram perguntar e ainda espera o pedido de permissão
    /// (`motor/pedidos_de_hook.rs`).
    motivos_de_hook: pedidos_de_hook::MotivosDeHook,
    /// Até quando cada conta que bateu no limite fica de fora (segundos Unix).
    limite_ate: HashMap<String, u64>,
    /// As contas em que o Claude Code disse que o login se perdeu (token revogado ou vencido sem
    /// renovar): ficam fora da escolha até um turno nelas dar certo ou a pessoa entrar de novo.
    /// Gravadas em `contas-sem-login.json`, para a marca sobreviver ao fechar o Vitral.
    sem_login: HashSet<String>,
    /// As contas que têm o login do Claude Code guardado (`busca::tem_login`): lido ao abrir e a
    /// cada busca do uso. Com `sem_login` e `recusadas`, diz se há conta conectada
    /// (`claude_conectado`), que é o que o anel de uso do Claude pede para aparecer.
    com_login: HashSet<String>,
    /// As contas cujo token a Anthropic recusou (401 ou 403) na última busca do uso. Só aparece
    /// nas Contas, como desconectada: não tira a conta da escolha (um 403 do uso não prova que o
    /// turno falharia).
    recusadas: HashSet<String>,
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
    /// Cota do Gemini no Antigravity (`agente_gemini::ler_uso`; `None`: sem agy, sem login ou sem
    /// resposta). Como o `agy` leva uns 3 s, ele é perguntado de 5 em 5 minutos, não a cada releitura.
    pub gemini: Option<crate::agente_gemini::UsoGemini>,
    /// A cota do Gemini já foi perguntada ao menos uma vez (antes, `gemini: None` não diz "sem login").
    pub gemini_lido: bool,
    /// Quando o Gemini foi perguntado pela última vez (0: nunca, ou pedido de atualizar).
    gemini_perguntado_em: u64,
    /// Conta usada no turno atual (ou no último) de cada chat.
    conta_do_turno: HashMap<String, String>,
    /// Processos que já entregaram o resultado e estão terminando de gravar a sessão, por chat:
    /// enquanto um deles vive, o chat não abre outro (`motor/saidas.rs`: um chat, um processo).
    encerrando: saidas::Saidas<Processo>,
    /// O turno que espera o processo anterior do chat sair para abrir: as mensagens dele, já como
    /// vão entrar. Abre sozinho quando o anterior sai (`abrir_quando_sair`); o Parar o desfaz.
    a_abrir: HashMap<String, Vec<Entrada>>,
    /// A trava que segura a suspensão automática enquanto algum chat trabalha (`acertar_trava`).
    trava_de_suspensao: Option<std::process::Child>,
    /// Quando a trava foi pedida pela última vez: sem `systemd-inhibit` (ou com ele saindo na hora),
    /// cada sinal do motor relançaria o comando; assim tenta de novo só depois de um tempo.
    trava_pedida_em: Option<std::time::Instant>,
    /// Quando chegou o último evento do processo de cada chat (texto, ferramenta, progresso): um
    /// turno que fica calado muito tempo pode ter travado (`silencio`).
    ultimo_sinal: HashMap<String, std::time::Instant>,
    /// Os chats parados no limite que continuam sozinhos quando ele volta: o instante (segundos)
    /// de tentar de novo. Some ao começar outro turno no chat.
    retomar_as: HashMap<String, u64>,
    /// Os avisos de "outro chat também editou" já dados nesta sessão: (chat, arquivo, outro chat).
    avisos_de_arquivo: HashSet<(String, PathBuf, String)>,
    /// As chamadas que esperaram uma permissão sua, por (chat, id da chamada): quando você
    /// permitiu. A duração da chamada conta daí, não do pedido (`duracao_da_chamada`).
    liberadas: HashMap<(String, String), u64>,
    /// A lista de passos que o agente mantém (o TodoWrite e as ferramentas de tarefa do Claude
    /// Code), a última de cada chat. Só na memória: some ao fechar o Vitral.
    passos: HashMap<String, passos::Passos>,
    /// Os comandos que o agente de cada chat deixou rodando no fundo: id da tarefa e descrição.
    comandos_no_fundo: HashMap<String, Vec<(String, String)>>,
    /// Processos do Claude Code que já entregaram o resultado e ficam com a entrada aberta à
    /// espera da sugestão da próxima mensagem (`prompt_suggestion`, que não sai com a entrada
    /// fechada), com a geração do turno: fecham quando ela chega, depois de
    /// `ESPERA_DA_SUGESTAO` ou quando o chat começa outro turno.
    sugerindo: HashMap<String, (Processo, u64)>,
    /// A próxima mensagem provável de cada chat, até o chat começar outro turno.
    sugestoes: HashMap<String, String>,
    parcial: HashMap<String, String>,
    /// Mensagens que chegaram com o chat ocupado: entram juntas no próximo turno.
    filas: Filas,
    interrompidos: Vec<String>,
    /// Quantas vezes cada chat já pediu o título gerado ao Claude Code nesta abertura do app
    /// (`titulo_a_pedir`).
    titulos_pedidos: HashMap<String, u8>,
    /// Chats com mensagens ainda não gravadas: vão para o disco a cada poucos segundos.
    chats_sujos: HashSet<String>,
    /// Chats cujo turno já foi repetido depois de um erro de sessão (uma vez só).
    repetidos_sessao: HashSet<String>,
    /// Chats cuja sessão do Claude Code é nova (a antiga expirou ou a última mensagem foi
    /// corrigida): o próximo turno começa com o bloco de passagem.
    passagens: HashMap<String, MotivoPassagem>,
    /// Mensagens automáticas seguidas entre pares de chats (o freio de ciclo).
    ciclos: Ciclos,
    /// "Enquanto você esteve fora": só na memória.
    pub resumo_volta: Option<ResumoVolta>,
    pub permissoes: Vec<PedidoPermissao>,
    /// Versão do Claude Code que os turnos abrem (lida ao abrir e a cada início de turno).
    pub versao_claude: Option<Versao>,
    /// O `claude update` das Configurações está rodando. Mora no motor, e não no diálogo, para
    /// sobreviver a fechar as Configurações no meio e para reabrir não deixar começar outro.
    pub atualizando_claude: bool,
    /// O que o último `claude update` respondeu, para a linha das Configurações.
    pub resposta_do_claude: Option<claude::Atualizacao>,
    /// O Codex e o Gemini como programas instalados: versão, conta, `update` (`motor/cli_fora.rs`).
    pub cli_codex: crate::cli_agentes::CliFora,
    pub cli_gemini: crate::cli_agentes::CliFora,
    /// Se a conta tem os fluxos de agentes (a ferramenta `Workflow`): `false` só depois de um
    /// turno com Ultracode em que eles não vieram. Não trava a opção (a pessoa pode ter mudado a
    /// configuração da conta): só a avisa em âmbar.
    fluxos_por_conta: HashMap<String, bool>,
    /// Fluxos e subagentes que o processo atual de cada chat ainda roda em segundo plano.
    fundo: HashMap<String, usize>,
    /// Os mesmos, um por um: quem são, a chamada que abriu cada um e até onde o arquivo de cada
    /// um foi lido (ver `motor::fundo`). Vale pelo processo do turno.
    segundo_plano: HashMap<String, fundo::NoFundo>,
    /// As conversas de subagente abertas em aba, só para ler (ver `motor::subconversa`): ficam
    /// fora do estado e da lista, e saem daqui quando a aba fecha.
    subconversas: Vec<subconversa::Subconversa>,
    /// De qual subagente é cada chamada feita por um subagente, por chat: o id da chamada e o id
    /// da chamada (Task/Agent) que o abriu. É como um pedido de permissão diz qual subagente
    /// espera você. Vale pelo processo do turno.
    chamadas_dos_subs: HashMap<String, HashMap<String, String>>,
    /// O relatório que um subagente entregou pela chamada de entrega (`SubRelatorio`), por chat
    /// e chamada que o abriu, até o resultado dessa chamada chegar.
    relatorios: HashMap<(String, String), String>,
    /// Quando você mandou parar um comando de fundo de cada chat pela última vez: o aviso de fim
    /// que chega logo depois ("killed") não marca o comando como falho.
    comandos_parados: HashMap<String, std::time::Instant>,
    /// Quando cada chat começou o bloco de raciocínio em curso (para dizer quanto durou).
    pensando: HashMap<String, std::time::Instant>,
    /// Os chats em que o Claude Code está resumindo a conversa agora (`Evento::Compactando`).
    compactando: HashSet<String>,
    /// O plano que chegou no pedido de permissão antes da chamada dele (os dois vêm por canais
    /// diferentes): o id da chamada, o chat e o texto, até a chamada chegar.
    planos_sem_chamada: HashMap<String, (String, String)>,
    /// O `result` que chegou com agentes ainda em segundo plano (texto e custo): o turno só
    /// termina quando eles acabam, ou quando o processo sai.
    fim_adiado: HashMap<String, (String, f64)>,
    /// As mensagens que o usuário dirigiu ao turno de cada chat e que o Claude Code ainda pode ter
    /// esperando na fila interna dele (ver `Processo::dirigir`): saem daqui quando ele as absorve
    /// (a primeira volta de ferramenta) ou quando abre outro turno com elas.
    dirigidas: HashMap<String, Vec<Entrada>>,
    /// Os chats em que o Parar pediu ao Claude Code para tirar da fila as mensagens dirigidas
    /// (`Parada::RetirarEInterromper`): no fim do turno parado elas voltam para a fila do Vitral e
    /// o processo sai sozinho, sem precisar morrer para não abrir outro turno com elas.
    retiradas: HashSet<String>,
    /// A vez de cada chat nas trocas de modo, modelo e esforço feitas com o turno rodando: só a
    /// última vai para o processo, depois de `ASSENTAR_A_TROCA` parada (`acertar_turno_depois`).
    trocas: HashMap<String, u64>,
    /// Quantas mensagens o usuário dirigiu ao turno que está rodando em cada chat, para a faixa
    /// dizer que ele foi dirigido. Zera a cada turno novo.
    dirigidos: HashMap<String, usize>,
    /// O estado mudou só na memória (o lido): a gravação agendada o leva ao disco.
    estado_sujo: bool,
    /// Os chats cujo turno você parou com o Esc (ou o Parar), e quando: a faixa oferece "Continuar"
    /// e o Enter com a caixa vazia continua, por alguns minutos (rodada de uso, item 6.2).
    parados_por_voce: HashMap<String, u64>,
    /// Os chats cujo turno de agora é a revisão da memória (rodada de uso, item 6.1): por fora eles
    /// contam como parados (o "Terminou" sai na hora), por dentro ainda trabalham.
    revisando: HashMap<String, Revisao>,
    /// Os chats cujo turno de agora é a revisão pelo outro agente (`motor/revisao_do_par.rs`): por
    /// fora contam como parados, como na revisão da memória, e a sua mensagem tem a vez.
    revisoes_do_par: HashMap<String, revisao_do_par::EmCurso>,
    /// A marca provisória da mensagem que abriu o turno do Codex de cada chat, até ele dizer o id
    /// do turno (`Evento::TurnoDoAgente`): aí ela vira `modelo::marca_do_codex`.
    marcas_do_codex: HashMap<String, String>,
    pub janela_ativa: bool,
    /// A conversa que o clique numa notificação pediu para abrir; a Janela consome.
    pub pedido_de_abrir: Option<String>,
    /// Chats que começaram a rodar por pedido de OUTRO chat (`mensagem_para_chat`, `abrir_chat`):
    /// a Janela os põe numa aba de fundo (`Config.chat_chamado_abre_aba`).
    pub chats_trazidos: Vec<String>,
    /// A conversa de subagente que o mapa dos agentes mostra no painel dele (sem aba).
    subconversa_do_mapa: Option<String>,
    /// As notificações na tela, por chat.
    #[cfg_attr(windows, allow(dead_code))]
    avisos: Arc<std::sync::Mutex<HashMap<String, Aviso>>>,
    /// Chats que algum painel aberto mostra (a Janela atualiza a cada quadro): o texto parcial
    /// dos outros não redesenha a janela.
    pub visiveis: HashSet<String>,
    /// Os chats abertos cujo texto cresceu no lote de sinais em curso (ver `aviso_do_lote`).
    texto_chegando: Vec<String>,
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
        let Estado { mut chats, filas, ocorrencias, visto_em, resto: resto_estado, .. } = estado;
        let antes: Vec<usize> = chats.iter().map(|c| c.itens.len()).collect();
        marcar_turnos_cortados(&mut chats);
        // Avisos e o que a normalização mexeu vão para o disco já: se o PC cair de novo antes de
        // qualquer ação, a próxima abertura não repete o aviso.
        if somente_leitura.is_none() {
            for (c, n) in chats.iter().zip(antes) {
                if c.itens.len() != n || normalizado.chats_mexidos.contains(&c.id) {
                    let _ = dados.salvar_chat(c);
                }
            }
            for id in &normalizado.chats_a_apagar {
                dados.apagar_chat(id);
            }
            // As imagens que as ferramentas devolveram (`imagens_recebidas`): saem as dos chats
            // que não existem mais e, acima do teto de todas juntas, as mais antigas. A conversa
            // que o estado não lista mas ainda tem o arquivo dela (um estado que voltou ao
            // `.anterior`) fica com as imagens.
            let vivos = chats.iter().map(|c| c.id.clone()).chain(dados.chats_gravados()).collect();
            crate::imagens_recebidas::varrer(&dados.pasta_imagens(""), &vivos, crate::imagens_recebidas::TETO_TOTAL);
            // As imagens coladas na caixa que nenhuma conversa, fila ou rascunho cita (nem as
            // arquivadas) saem de `anexos/`, em segundo plano e sem pressa (`limpeza_anexos`).
            let dados_da_limpeza = dados.clone();
            let _ = std::thread::Builder::new().name("limpeza-anexos".into()).spawn(move || {
                let apagados = crate::limpeza_anexos::limpar(&dados_da_limpeza, &Default::default());
                if apagados > 0 {
                    eprintln!("vitral: {apagados} imagem(ns) colada(s) sem conversa apagada(s) de anexos/");
                }
            });
            // A pasta de um chat sem pasta (apagada à mão) volta vazia.
            let avulsos = dados.pasta_avulso("");
            for c in chats.iter().filter(|c| c.cwd.starts_with(&avulsos)) {
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
        // token: espera um pouco pelos próximos (`JANELA_DO_TEXTO`: uns 16 redesenhos por segundo).
        let _recebedor = cx.spawn(async move |motor, cx| {
            while let Ok(sinal) = recebe.recv().await {
                if matches!(sinal, Sinal::Claude(_, _, Evento::TextoParcial(_))) {
                    cx.background_executor().timer(JANELA_DO_TEXTO).await;
                }
                let mut sinais = vec![sinal];
                while sinais.len() < 512
                    && let Ok(outro) = recebe.try_recv()
                {
                    sinais.push(outro);
                }
                // Texto chegando numa conversa que não está aberta não redesenha a janela, e o que
                // só cresce numa conversa aberta redesenha só os painéis dela.
                if motor
                    .update(cx, |motor, cx| {
                        let mut redesenhar = false;
                        for sinal in sinais {
                            redesenhar |= motor.tratar(sinal, cx);
                        }
                        match aviso_do_lote(redesenhar, std::mem::take(&mut motor.texto_chegando)) {
                            AvisoDoLote::Tudo => cx.notify(),
                            AvisoDoLote::Texto(chats) => cx.emit(TextoChegando { chats }),
                            AvisoDoLote::Nada => {}
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        // Confere a cada meio minuto se há conversa para arquivar e relê o uso dos planos.
        cx.spawn(async move |motor, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(30))
                    .await;
                let vez = motor.update(cx, |m, cx| {
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
                // Na mesma volta, as mensagens com hora marcada cuja hora chegou.
                let volta = motor.update(cx, |m, cx| {
                    m.rotina_uso(cx);
                    m.entregar_agendadas(cx);
                });
                if volta.is_err() {
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
                // Na mesma volta, quem terminou o turno e não saiu no prazo é derrubado.
                let volta = motor.update(cx, |m, cx| {
                    m.encerrando.vencer(std::time::Instant::now());
                    m.gravar_sujos(cx)
                });
                if volta.is_err() {
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
                    c.email = email.clone();
                    c.plano = plano;
                    // A conta extra que é a mesma da principal (o login do Claude Code passou a
                    // ser o dela) sai da escolha: eram duas linhas da mesma conta, e a Automática
                    // revezava entre ela e ela mesma (dono, 18/11/2026: "tá as duas contas aqui").
                    // Só na memória: o `contas.json` fica, e ela volta se a principal mudar.
                    if !email.is_empty() {
                        m.contas.retain(|c| c.id == contas::PRINCIPAL || !c.email.eq_ignore_ascii_case(&email));
                    }
                    cx.notify();
                }
            });
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
                // E os modelos que ele oferece: o seletor acompanha o Claude Code sozinho.
                m.reler_modelos_do_claude(cx);
                m.reler_modelos_do_codex(cx);
            });
        })
        .detach();
        // O quadro de ideias. Ilegível, o arquivo fica copiado ao lado e o quadro começa vazio.
        let mut quadro = match dados.ler_ideias() {
            Ok(quadro) => quadro,
            Err(erro) => {
                avisos_ao_abrir.push(format!("Não deu para ler o quadro de ideias ({erro:#})."));
                Default::default()
            }
        };
        quadro.acertar_ordens();
        let linear = ideias::EstadoDoLinear::da_config(&dados.config);
        let gastos = gasto::Livro::ler(&dados.pasta_gastos(), agora());
        let mut motor = Self {
            gastos,
            memorias: Default::default(),
            motivos_de_hook: Default::default(),
            quadro,
            quadro_sujo: false,
            linear,
            dados,
            config,
            chats,
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
            historico_uso: HashMap::new(),
            limite_ate: HashMap::new(),
            sem_login: contas::ler_sem_login(),
            com_login: HashSet::new(),
            recusadas: HashSet::new(),
            plano_por_conta: HashMap::new(),
            agenda_uso: Agenda::default(),
            uso_em_vista: None,
            codex: None,
            codex_lido: false,
            gemini: None,
            gemini_lido: false,
            gemini_perguntado_em: 0,
            conta_do_turno: HashMap::new(),
            encerrando: Default::default(),
            a_abrir: HashMap::new(),
            trava_de_suspensao: None,
            trava_pedida_em: None,
            ultimo_sinal: HashMap::new(),
            retomar_as: HashMap::new(),
            avisos_de_arquivo: HashSet::new(),
            passos: HashMap::new(),
            comandos_no_fundo: HashMap::new(),
            sugerindo: HashMap::new(),
            sugestoes: HashMap::new(),
            parcial: HashMap::new(),
            filas,
            interrompidos: Vec::new(),
            titulos_pedidos: HashMap::new(),
            chats_sujos: HashSet::new(),
            repetidos_sessao: HashSet::new(),
            passagens: HashMap::new(),
            ciclos: Ciclos::default(),
            resumo_volta: None,
            permissoes: Vec::new(),
            versao_claude: None,
            atualizando_claude: false,
            resposta_do_claude: None,
            cli_codex: Default::default(),
            cli_gemini: Default::default(),
            fluxos_por_conta: HashMap::new(),
            fundo: HashMap::new(),
            subconversas: Vec::new(),
            segundo_plano: HashMap::new(),
            chamadas_dos_subs: HashMap::new(),
            relatorios: HashMap::new(),
            comandos_parados: HashMap::new(),
            pensando: HashMap::new(),
            compactando: HashSet::new(),
            planos_sem_chamada: HashMap::new(),
            liberadas: HashMap::new(),
            fim_adiado: HashMap::new(),
            dirigidas: HashMap::new(),
            retiradas: HashSet::new(),
            trocas: HashMap::new(),
            dirigidos: HashMap::new(),
            estado_sujo: false,
            parados_por_voce: HashMap::new(),
            revisando: HashMap::new(),
            revisoes_do_par: HashMap::new(),
            marcas_do_codex: HashMap::new(),
            janela_ativa: true,
            pedido_de_abrir: None,
            chats_trazidos: Vec::new(),
            subconversa_do_mapa: None,
            avisos: Arc::default(),
            visiveis: HashSet::new(),
            texto_chegando: Vec::new(),
            servidor,
            sinais,
            _recebedor,
        };
        if motor.somente_leitura.is_none() {
            // O chat que saiu de uma cópia e começa uma sessão nova leva um resumo da conversa.
            for id in normalizado.sessoes_novas {
                motor.passagens.insert(id, MotivoPassagem::SaiuDaCopia);
            }
            // O que falta instalar (o git, o Claude Code, o Git Bash no Windows) e as pastas dos
            // chats no OneDrive.
            let mut pastas: Vec<PathBuf> = motor.chats.iter().filter(|c| !c.arquivado).map(|c| c.cwd.clone()).collect();
            pastas.sort();
            pastas.dedup();
            motor.avisos_ao_abrir.extend(crate::sistema::avisos_de_instalacao(&pastas));
        }
        // Quais contas têm o login guardado, já na abertura: o anel do Claude não pisca.
        motor.com_login = motor.contas.iter().filter(|c| busca::tem_login(c)).map(|c| c.id.clone()).collect();
        motor.salvar();
        motor.dados.esperar_gravacoes();
        // O último uso do plano que o Claude Code guardou de cada conta e o do Codex.
        motor.reler_uso(cx);
        motor
    }

    // ---------- leitura para a interface ----------

    /// A migração acabou de trazer o Vitral antigo: o aviso na janela e a ocorrência no registro.
    pub fn migracao_concluida(&mut self, relatorio: &crate::migracao::Relatorio, cx: &mut Context<Self>) {
        let texto = relatorio.texto(dirs::home_dir().as_deref());
        self.registrar("migracao", "vitral", "", &texto);
        self.avisos_ao_abrir.push(texto);
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
    fn registrar(&mut self, tipo: &str, autor: &str, chat: &str, texto: &str) {
        self.ocorrencias.push(Ocorrencia {
            quando: agora(),
            tipo: tipo.into(),
            autor: autor.into(),
            chat: chat.into(),
            texto: texto.into(),
        });
        let sobra = self.ocorrencias.len().saturating_sub(crate::modelo::MAX_OCORRENCIAS);
        self.ocorrencias.drain(..sobra);
    }

    pub fn somente_leitura(&self) -> Option<&SomenteLeitura> {
        self.somente_leitura.as_ref()
    }

    /// O chat de verdade com este id: o que mora no estado e na lista. Dentro do motor é sempre
    /// este que se procura: a conversa de um subagente aberta numa aba (`Motor::chat` a acha) é só
    /// para ler, e nada do motor age sobre ela.
    pub(crate) fn chat_gravado(&self, id: &str) -> Option<&Chat> {
        self.chats.iter().find(|c| c.id == id)
    }

    /// Mexe no registro do subagente aberto pela chamada `pai` (Task/Agent) do chat, criando-o
    /// na primeira vez. Chamada que não existe (ou que não é do chat) não faz nada.
    fn no_subagente(&mut self, chat: &str, pai: &str, f: impl FnOnce(&mut crate::modelo::Subagente)) {
        let Some(c) = self.chat_mut(chat) else {
            return;
        };
        // O comando de fundo (um Bash) também avisa quando termina, com o id da chamada dele: não
        // é subagente, e não ganha registro de um.
        let achado = c.itens.iter_mut().rev().find_map(|item| match item {
            Item::Ferramenta { id, nome, sub, .. } if id == pai && nome != "Bash" => Some(sub),
            _ => None,
        });
        if let Some(sub) = achado {
            f(sub.get_or_insert_with(Default::default));
            self.chats_sujos.insert(chat.to_string());
        }
    }

    fn chat_mut(&mut self, id: &str) -> Option<&mut Chat> {
        self.chats.iter_mut().find(|c| c.id == id)
    }

    /// A execução como a tela a vê: a revisão da memória conta como parada (o turno de trabalho já
    /// terminou, e o "Terminou" sai na hora; rodada de uso, item 6.1).
    pub fn execucao(&self, chat: &str) -> Execucao {
        // A conversa de um subagente aberta numa aba roda enquanto ele trabalha.
        if let Some((pai, chamada)) = subconversa::partes_do_id(chat) {
            return self.execucao_do_subagente(pai, chamada);
        }
        match self.execucao_real(chat) {
            Execucao::Rodando { .. } if self.revisando.contains_key(chat) || self.revisoes_do_par.contains_key(chat) => {
                Execucao::Parado
            }
            outra => outra,
        }
    }

    /// A execução de verdade: a revisão da memória ainda é trabalho (o fechar e o Reiniciar contam).
    fn execucao_real(&self, chat: &str) -> Execucao {
        self.execucao.get(chat).cloned().unwrap_or(Execucao::Parado)
    }

    /// Há quanto tempo o turno andando deste chat não manda nada, a partir de `SILENCIO_SUSPEITO`.
    /// Um comando longo (um build, um `sleep`) também fica calado: a tela só avisa, não decide.
    /// Com subagentes no fundo o processo principal espera por eles, e o silêncio é o normal.
    pub fn silencio(&self, chat: &str) -> Option<std::time::Duration> {
        if !matches!(self.execucao(chat), Execucao::Rodando { .. }) || self.esperando_agentes(chat) {
            return None;
        }
        let calado = self.ultimo_sinal.get(chat)?.elapsed();
        (calado >= SILENCIO_SUSPEITO).then_some(calado)
    }

    /// O chat está revendo a memória agora (a linha de estado diz, discreta).
    pub fn revendo_memoria(&self, chat: &str) -> bool {
        self.revisando.contains_key(chat) && self.execucao_real(chat).rodando()
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

    /// Quantos chats fechar o Vitral agora cortaria no meio: turno rodando ou nova tentativa marcada.
    pub fn trabalhando(&self) -> usize {
        self.ocupados(false)
    }

    /// Quantos chats uma troca do binário do claude pegaria no meio: os que estão trabalhando e
    /// também os que têm mensagem esperando a vez, que abre um turno a qualquer momento.
    pub fn turnos_a_caminho(&self) -> usize {
        self.ocupados(true)
    }

    fn ocupados(&self, com_fila: bool) -> usize {
        // A revisão da memória também conta: fechar agora a cortaria.
        let mut chats: HashSet<&str> =
            self.chats.iter().filter(|c| self.execucao_real(&c.id).rodando()).map(|c| c.id.as_str()).collect();
        if com_fila {
            chats.extend(self.chats.iter().map(|c| c.id.as_str()).filter(|id| self.na_fila(id) > 0));
        }
        chats.len()
    }

    /// Por que o chat parou no meio, quando dá para retomar.
    pub fn parada(&self, chat_id: &str) -> Option<String> {
        let chat = self.chat_gravado(chat_id)?;
        let livre = !self.processos.contains_key(chat_id);
        (chat.turno_aberto && livre).then(|| CORTADO.to_string())
    }

    /// Parou sem ter sido o usuário (o Vitral fechado no meio): pede a atenção dele.
    pub fn precisa_retomar(&self, chat_id: &str) -> bool {
        self.parada(chat_id).is_some()
    }

    /// O modo em que a conversa fica ao aprovar o plano dela.
    pub fn modo_ao_aprovar_plano(&self, chat: &Chat) -> String {
        modo_depois_do_plano(
            chat.modo_antes_do_plano.as_deref(),
            Some(self.permissao_de(chat).as_str()),
            &self.config.permissao,
        )
    }

    /// A espera de um chat, fora os arquivados.
    pub fn espera_do_chat(&self, c: &Chat) -> Option<Espera> {
        if c.arquivado {
            return None;
        }
        espera(&self.execucao(&c.id), self.nao_lido(c), self.precisa_retomar(&c.id))
    }

    /// O que precisa de você em todos os chats.
    pub fn precisa_de_voce(&self) -> PrecisaDeVoce {
        let mut conta = PrecisaDeVoce::default();
        for c in self.chats.iter().filter(|c| !c.arquivado) {
            if let Some(espera) = self.espera_do_chat(c) {
                conta.somar(espera, espera == Espera::Erro);
            }
        }
        conta
    }

    /// Tudo o que espera você, um por linha: pedidos e planos na ordem em que chegaram, depois os
    /// erros, as paradas e as respostas novas (a mais recente primeiro). Sem os arquivados.
    pub fn pendencias(&self) -> Vec<Pendencia> {
        let ativo = |id: &str| self.chat_gravado(id).is_some_and(|c| !c.arquivado);
        let mut lista: Vec<Pendencia> = self
            .permissoes
            .iter()
            .filter(|p| ativo(&p.chat))
            .map(|p| pendencia_do_pedido(&p.id, &p.chat, &p.ferramenta, &p.entrada))
            .collect();
        let ativos: Vec<&Chat> = self.chats.iter().filter(|c| !c.arquivado).collect();
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
            ativos.iter().filter(|c| self.nao_lido(c)).collect();
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

    /// Nada roda no chat: nem turno, nem nova tentativa.
    fn chat_livre(&self, id: &str) -> bool {
        !self.execucao_real(id).rodando() && !self.processos.contains_key(id)
    }

    /// Esquece o que o motor guarda em memória das conversas que saíram do estado. Um
    /// processo que tenha aberto depois da conferência sai junto; um pedido de permissão é negado.
    fn esquecer(&mut self, chats: &[String]) {
        let saiu = |id: &String| chats.contains(id);
        for id in chats {
            if let Some(mut processo) = self.processos.remove(id) {
                processo.matar();
            }
            if let Some((mut processo, _)) = self.sugerindo.remove(id) {
                processo.matar();
            }
            self.encerrando.derrubar_do_chat(id);
            self.a_abrir.remove(id);
            self.execucao.remove(id);
            self.geracao_atual.remove(id);
            self.ultima_entrada.remove(id);
            self.conta_do_turno.remove(id);
            self.parcial.remove(id);
            self.chats_sujos.remove(id);
            self.repetidos_sessao.remove(id);
            self.passagens.remove(id);
            self.fundo.remove(id);
            self.esquecer_subagentes(id);
            self.subconversas.retain(|s| s.pai != *id);
            self.fim_adiado.remove(id);
            self.dirigidas.remove(id);
            self.retiradas.remove(id);
            self.trocas.remove(id);
            self.dirigidos.remove(id);
            self.revisando.remove(id);
            self.revisoes_do_par.remove(id);
            self.marcas_do_codex.remove(id);
            self.parados_por_voce.remove(id);
            self.visiveis.remove(id);
        }
        self.interrompidos.retain(|c| !saiu(c));
        let (deles, outros): (Vec<_>, Vec<_>) = self.permissoes.drain(..).partition(|p| saiu(&p.chat));
        self.permissoes = outros;
        for pedido in deles {
            let _ = pedido.resposta.try_send(Resposta::ok(
                json!({"behavior": "deny", "message": "Esta conversa saiu do Vitral."}).to_string(),
            ));
        }
        if self.pedido_de_abrir.as_ref().is_some_and(saiu) {
            self.pedido_de_abrir = None;
        }
    }

    // ---------- ações da interface ----------

    /// Um chat novo que mexe direto em `pasta`; sem pasta, numa pasta vazia só dele
    /// (`avulsos/<chat>` nos dados). `criado_por` é "" (o usuário), "importacao", "chat:<id>"…
    pub fn novo_chat(&mut self, pasta: Option<&Path>, criado_por: &str, cx: &mut Context<Self>) -> Result<String, String> {
        if self.somente_leitura.is_some() {
            return Err("o Vitral está aberto só para leitura".into());
        }
        let id = novo_id();
        let cwd = match pasta {
            None => {
                let pasta = self.dados.pasta_avulso(&id);
                std::fs::create_dir_all(&pasta).map_err(|e| format!("{}: {e}", pasta.display()))?;
                pasta
            }
            Some(pasta) if pasta.is_dir() => dunce::canonicalize(pasta).unwrap_or_else(|_| pasta.to_path_buf()),
            Some(pasta) => {
                return Err(format!("A pasta {} não foi encontrada (disco desmontado ou pasta movida).", pasta.display()));
            }
        };
        let mut chat = Chat::novo(id.clone(), "Nova conversa", cwd);
        chat.criado_por = criado_por.to_string();
        chat.agente = self.config.agente_padrao;
        self.chats.push(chat);
        self.salvar();
        cx.notify();
        Ok(id)
    }

    /// As pastas onde já há chat (sem as pastas só de um chat), da usada há menos tempo para a mais
    /// antiga, sem repetir: o "Novo chat em outra pasta…" as oferece.
    pub fn pastas_dos_chats(&self) -> Vec<PathBuf> {
        let avulsos = self.dados.pasta_avulso("");
        let mut chats: Vec<&Chat> = self.chats.iter().filter(|c| !c.cwd.starts_with(&avulsos)).collect();
        chats.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
        let mut pastas: Vec<PathBuf> = Vec::new();
        for c in chats {
            if !pastas.contains(&c.cwd) {
                pastas.push(c.cwd.clone());
            }
        }
        pastas
    }

    /// Os chats que a lista mostra (as arquivadas inclusive; quem as separa é a lista). Tudo o que
    /// monta ou conta a lista passa por aqui: um filtro por pasta entra só neste lugar.
    pub fn chats_da_lista(&self) -> impl Iterator<Item = &Chat> {
        self.chats.iter()
    }

    /// O chat não tem pasta própria de verdade: a dele é uma pasta vazia em `avulsos/`.
    pub fn sem_pasta(&self, chat: &Chat) -> bool {
        chat.cwd.starts_with(self.dados.pasta_avulso(""))
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
        let texto = texto.trim().to_string();
        if texto.is_empty() && anexos.is_empty() {
            return;
        }
        // Escrever num chat arquivado é voltar a usar ele: ele sai das Arquivadas e o que esperava
        // por isso entra (dono, 19/11/2026: "mandei a mensagem aqui e não começou").
        if self.chat_gravado(chat).is_some_and(|c| c.arquivado) {
            self.alternar_arquivado(chat, cx);
        }
        // Comandos que o Vitral resolve sozinho.
        if matches!(texto.as_str(), "/clear" | "/nova" | "/new") {
            self.nova_sessao(chat, cx);
            return;
        }
        // O título sai da primeira mensagem.
        if let Some(c) = self.chat_mut(chat)
            && c.itens.is_empty()
            && c.titulo == "Nova conversa"
            && !texto.starts_with('/')
        {
            c.titulo = titulo_de(&texto);
        }
        // O usuário falou: os ciclos de mensagens automáticas desse chat recomeçam, e a oferta de
        // continuar depois do Esc some (item 6.2).
        self.ciclos.zerar(chat);
        self.parados_por_voce.remove(chat);
        let entrada = entrada_do_usuario(texto, anexos, reforco);
        // Mensagem durante a revisão da memória (item 6.1): ela nunca dirige a revisão. Se a
        // revisão ainda não escreveu nada, ela é cortada e a mensagem abre um turno normal (a
        // revisão volta a ser pedida no próximo fim); se já escreveu, a mensagem espera na fila,
        // para não cortar um arquivo pela metade.
        // A revisão pelo outro agente dá a vez à sua mensagem: rodando, ela para (a mensagem espera
        // na fila e entra quando o revisor sair); se ainda ia abrir, sai da espera. Nos dois casos
        // o que ele tinha feito some, e o chat volta ao agente dele.
        if self.revisoes_do_par.contains_key(chat) {
            if self.processos.contains_key(chat) {
                self.filas.entry(chat.to_string()).or_default().push(entrada);
                self.parar(chat, cx);
                self.salvar();
                cx.notify();
                return;
            }
            if let Some(esperando) = self.a_abrir.get_mut(chat) {
                esperando.retain(|e| e.de.as_deref() != Some("Vitral"));
            }
            self.desfazer_revisao_do_par(chat);
        }
        // A revisão que ainda esperava para abrir (o processo do turno anterior saindo) dá a vez à
        // sua mensagem: sai da espera e volta a ser pedida no próximo fim.
        if self.revisando.contains_key(chat) && self.a_abrir.contains_key(chat) {
            if let Some(esperando) = self.a_abrir.get_mut(chat) {
                esperando.retain(|e| e.de.as_deref() != Some("Vitral"));
            }
            self.revisando.remove(chat);
            if let Some(c) = self.chat_mut(chat) {
                c.memoria_da_sessao = None;
            }
        }
        if self.revisando.contains_key(chat) && self.processos.contains_key(chat) {
            let escreveu = self.chat_gravado(chat).is_some_and(|c| {
                let desde = self.revisando.get(chat).map_or(c.itens.len(), |r| r.desde_item);
                revisao_escreveu(&c.itens[desde.min(c.itens.len())..])
            });
            self.filas.entry(chat.to_string()).or_default().push(entrada);
            if !escreveu {
                self.parar(chat, cx);
            }
            self.salvar();
            cx.notify();
            return;
        }
        // Com o turno rodando, a mensagem do usuário entra nele (o "Mandar depois" é que escolhe a
        // fila). Só a dele: o que vem de outro chat continua esperando a vez. Com um pedido de
        // permissão aberto, ela é a resposta ao pedido.
        if !reforco.depois {
            if self.responder_pedido_escrevendo(chat, &entrada, cx) {
                return;
            }
            match self.dirigir_turno(chat, &entrada, cx) {
                Dirigido::Foi => return,
                Dirigido::NaFila(motivo) => {
                    if let Some(motivo) = motivo {
                        self.avisar(chat, motivo, false);
                    }
                }
            }
        }
        self.entregar_entrada(chat, entrada, cx);
    }

    pub fn parar(&mut self, chat: &str, cx: &mut Context<Self>) {
        // A fila fica guardada: entra junto com a próxima mensagem (ou em "Entregar agora").
        // Mensagem dirigida ainda engatilhada na fila interna do Claude Code: o `interrupt` não a
        // descarta (ele a rodaria como turno novo, depois do Parar). O Parar a tira de lá antes
        // (`Processo::retirar_dirigidas`) e ela volta para a fila do Vitral; o Claude Code fecha o
        // turno e grava a sessão, como em qualquer Parar. Só num Claude Code que não conhece o
        // pedido (ou no Codex) o processo ainda sai inteiro, com SIGTERM.
        // Um chat, um processo: o Parar alcança também o processo do turno anterior que ainda não
        // saiu (ou que ficou trabalhando por fora), e o turno que esperava por ele não abre mais.
        self.desistir_de_abrir(chat);
        if let Some((mut processo, geracao)) = self.sugerindo.remove(chat) {
            processo.matar();
            self.encerrando.guardar(chat, geracao, processo, true, std::time::Instant::now());
        }
        self.encerrando.derrubar_do_chat(chat);
        let dirigida_esperando = self.dirigidas.get(chat).is_some_and(|d| !d.is_empty());
        let pode_retirar = self.chat(chat).is_some_and(|c| retira_dirigidas(c.agente, self.versao_do_claude()));
        let parada = como_parar(self.fim_adiado.contains_key(chat), dirigida_esperando, pode_retirar);
        if let Some(processo) = self.processos.get_mut(chat) {
            match parada {
                // Esperando os agentes em segundo plano, não há turno para interromper: o processo
                // sai com eles (SIGTERM em tudo que ele abriu).
                Parada::Matar => processo.matar(),
                Parada::RetirarEInterromper if !processo.retirar_dirigidas() => processo.matar(),
                Parada::RetirarEInterromper => {
                    self.retiradas.insert(chat.to_string());
                    processo.interromper();
                    self.conferir_parada(chat, cx);
                }
                Parada::Interromper => {
                    processo.interromper();
                    self.conferir_parada(chat, cx);
                }
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
        // A chamada só começa a rodar agora: o tempo que ela esperou por você não é dela.
        if let (true, Some(chamada)) = (permitir, &pedido.chamada) {
            self.liberadas.insert((pedido.chat.clone(), chamada.clone()), agora());
        }
        // Aprovar o plano sai do Planejar, como na extensão: o resto do turno e os próximos rodam
        // no modo de antes (sem isso o Claude Code voltaria ao Manual e o turno seguinte, ao plano).
        // O plano que o próprio Claude abriu, ou com o modo trocado no meio, volta ao da conversa.
        let modo = (permitir && pedido.ferramenta == "ExitPlanMode").then(|| match self.chat_gravado(&pedido.chat) {
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
                .unwrap_or_else(|| recusa_sem_texto(&pedido.ferramenta).into());
            json!({"behavior": "deny", "message": mensagem})
        };
        let _ = pedido.resposta.try_send(Resposta::ok(resposta.to_string()));
        if let Some(modo) = modo {
            // O processo já sai do plano pela resposta: a próxima troca não pede o modo de novo.
            if let Some(processo) = self.processos.get_mut(&pedido.chat) {
                processo.anotar_modo(&modo);
            }
            if let Some(c) = self.chat_mut(&pedido.chat) {
                c.permissao = Some(modo);
                c.modo_antes_do_plano = None;
                self.salvar();
            }
        }
        self.pedido_respondido(&pedido.chat, cx);
    }

    /// Responde a uma pergunta com opções (a ferramenta AskUserQuestion) com o que foi escolhido
    /// no cartão: cada pergunta (o texto inteiro dela) com a resposta. Vai como `allow`, pelo
    /// mesmo caminho da permissão (`resposta_da_pergunta`). Sem resposta nenhuma é o "Não
    /// responder" do cartão: o modelo lê "The user did not answer the questions.", e a chamada
    /// não fica marcada como falha (negada, ficaria).
    pub fn responder_pergunta(&mut self, id: &str, respostas: &[(String, String)], cx: &mut Context<Self>) {
        let Some(posicao) = self.permissoes.iter().position(|p| p.id == id) else {
            return;
        };
        let pedido = self.permissoes.remove(posicao);
        let resposta = resposta_da_pergunta(&pedido.entrada, respostas);
        let _ = pedido.resposta.try_send(Resposta::ok(resposta.to_string()));
        self.pedido_respondido(&pedido.chat, cx);
    }

    /// Um pedido do chat foi respondido: sem outro esperando, o turno volta a rodar.
    fn pedido_respondido(&mut self, chat: &str, cx: &mut Context<Self>) {
        if !self.permissoes.iter().any(|p| p.chat == chat) && self.execucao(chat) == Execucao::EsperandoVoce {
            self.execucao.insert(chat.to_string(), Execucao::Rodando { desde: agora() });
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
        let regra = self.chat_gravado(&chat_id).and_then(|c| {
            crate::modelo::regra_de_permissao(&pedido.ferramenta, &pedido.entrada, &c.cwd)
        });
        let manual = self.chat_gravado(&chat_id).is_some_and(|c| self.permissao_de(c) == "manual");
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
        let chat = self.chat_gravado(&chat_id);
        let cobertos: Vec<String> = self
            .permissoes
            .iter()
            .filter(|p| p.chat == chat_id)
            .filter(|p| {
                p.id == id
                    || chat.is_some_and(|c| {
                        pedidos_de_hook::passa_sem_perguntar(c, &p.ferramenta, &p.entrada, p.motivo_do_hook.is_some())
                    })
            })
            .map(|p| p.id.clone())
            .collect();
        for pedido in cobertos {
            self.responder_permissao(&pedido, true, None, cx);
        }
    }

    pub fn renomear(&mut self, chat: &str, titulo: String, cx: &mut Context<Self>) {
        if let Some(c) = self.chat_mut(chat) {
            c.titulo = titulo;
        }
        self.salvar();
        cx.notify();
    }

    pub fn alternar_fixado(&mut self, chat: &str, cx: &mut Context<Self>) {
        if let Some(c) = self.chat_mut(chat) {
            c.fixado = !c.fixado;
        }
        self.salvar();
        cx.notify();
    }

    pub fn alternar_arquivado(&mut self, chat: &str, cx: &mut Context<Self>) {
        if let Some(c) = self.chat_mut(chat) {
            trocar_arquivado(c, agora());
        }
        // O que esperou pelo desarquivado entra.
        if self.chat_gravado(chat).is_some_and(|c| !c.arquivado) {
            self.entregar_fila(chat, cx);
        }
        self.salvar();
        cx.notify();
    }

    /// O chat que nunca rodou: sem sessão, só com os avisos do Vitral, sem fila e sem nada
    /// trabalhando.
    pub fn chat_vazio(&self, chat: &str) -> bool {
        self.chat_gravado(chat).is_some_and(nunca_rodou) && self.filas.get(chat).is_none_or(Vec::is_empty) && self.chat_livre(chat)
    }

    /// "Arquivar" no menu do chat: o chat vazio sai do Vitral em vez de ir para Arquivadas (com a
    /// pasta do chat sem pasta, se ela continua vazia). Devolve se apagou.
    pub fn arquivar_chat(&mut self, chat: &str, cx: &mut Context<Self>) -> bool {
        if !self.chat_vazio(chat) {
            if self.chat_gravado(chat).is_some_and(|c| !c.arquivado) {
                self.alternar_arquivado(chat, cx);
            }
            return false;
        }
        let cwd = self.chat_gravado(chat).map(|c| c.cwd.clone());
        self.chats.retain(|c| c.id != chat);
        self.filas.remove(chat);
        self.esquecer(&[chat.to_string()]);
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
        for c in self.chats.iter_mut().filter(|c| ids.contains(&c.id)) {
            c.arquivado = true;
        }
        self.salvar();
        cx.notify();
    }

    /// Arquiva sozinho as conversas paradas há mais de `arquivar_apos_dias` (0 = nunca): já lidas,
    /// não fixadas, sem fila e sem nada rodando ou pedindo. É só a marca: desarquivar devolve tudo
    /// como estava.
    fn arquivar_paradas(&mut self, cx: &mut Context<Self>) {
        let dias = self.config.arquivar_apos_dias;
        if dias == 0 {
            return;
        }
        let limite = agora().saturating_sub(u64::from(dias) * 86_400);
        let parados: Vec<String> = self
            .chats
            .iter()
            .filter(|c| candidata_a_arquivar(c, limite))
            .filter(|c| !self.nao_lido(c) && self.na_fila(&c.id) == 0)
            .filter(|c| {
                self.execucao(&c.id) == Execucao::Parado
                    // Parou no meio (espera o Retomar).
                    && self.parada(&c.id).is_none()
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

    /// Com algum chat trabalhando, o computador não suspende sozinho por inatividade: deixar um
    /// turno longo rodando e ir para outro aplicativo (ou sair de perto) não o congela no meio
    /// (dono, 17/11/2026: "se deixar rodando e for mexer em outro aplicativo ele não vai parar").
    /// É uma trava `sleep` do logind (`systemd-inhibit`): a tela ainda apaga e bloqueia, e fechar
    /// a tampa ainda suspende (o logind ignora a trava na tampa). Ela morre junto com o Vitral
    /// (`tail --pid`), mesmo numa queda. Só no Linux.
    fn acertar_trava(&mut self) {
        if !cfg!(target_os = "linux") {
            return;
        }
        // Só turno andando: o chat parado num pedido de permissão (ou com o processo vivo à toa)
        // não segura a suspensão de um notebook que ninguém está olhando.
        let trabalhando =
            self.processos.keys().any(|chat| matches!(self.execucao_real(chat), Execucao::Rodando { .. }));
        if let Some(trava) = self.trava_de_suspensao.as_mut()
            && trava.try_wait().ok().flatten().is_some()
        {
            self.trava_de_suspensao = None;
        }
        match (trabalhando, self.trava_de_suspensao.is_some()) {
            (true, false) => {
                if self.trava_pedida_em.is_some_and(|em| em.elapsed() < std::time::Duration::from_secs(30)) {
                    return;
                }
                self.trava_pedida_em = Some(std::time::Instant::now());
                let pid = std::process::id().to_string();
                let mut comando = crate::processos::novo_comando("systemd-inhibit");
                comando
                    .args(["--what=sleep", "--who=Vitral", "--why=Um chat está trabalhando", "--mode=block", "tail", "--pid", &pid, "-f", "/dev/null"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                // Num grupo próprio, para soltar a trava levando junto o `tail` de dentro dela.
                #[cfg(unix)]
                std::os::unix::process::CommandExt::process_group(&mut comando, 0);
                self.trava_de_suspensao = comando.spawn().ok();
            }
            (false, true) => {
                if let Some(mut trava) = self.trava_de_suspensao.take() {
                    // SAFETY: o grupo é o do filho que ainda não foi colhido (o id é dele).
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-(trava.id() as i32), libc::SIGTERM);
                    }
                    #[cfg(not(unix))]
                    let _ = trava.kill();
                    let _ = trava.wait();
                }
            }
            _ => {}
        }
    }

    /// A próxima mensagem provável do chat (`Config.sugerir_resposta`), até ele começar outro
    /// turno.
    pub fn sugestao(&self, chat: &str) -> Option<&str> {
        self.sugestoes.get(chat).map(String::as_str)
    }

    /// Guarda o processo que acabou o turno com a entrada aberta, à espera da sugestão, e marca
    /// quando desistir dela.
    fn esperar_sugestao(&mut self, chat_id: &str, processo: Processo, geracao: u64, cx: &mut Context<Self>) {
        self.parar_de_esperar_sugestao(chat_id);
        self.sugerindo.insert(chat_id.to_string(), (processo, geracao));
        let id = chat_id.to_string();
        cx.spawn(async move |motor, cx| {
            cx.background_executor().timer(ESPERA_DA_SUGESTAO).await;
            let _ = motor.update(cx, |m, _| {
                if m.sugerindo.get(&id).is_some_and(|(_, g)| *g == geracao) {
                    m.parar_de_esperar_sugestao(&id);
                }
            });
        })
        .detach();
    }

    /// Fecha a entrada do processo que esperava a sugestão do chat: ele sai sozinho.
    fn parar_de_esperar_sugestao(&mut self, chat_id: &str) {
        if let Some((mut processo, geracao)) = self.sugerindo.remove(chat_id) {
            processo.fechar_entrada();
            self.encerrando.guardar(chat_id, geracao, processo, false, std::time::Instant::now());
        }
    }

    // ---------- um chat, um processo ----------

    /// O turno novo só abre com o processo anterior do chat fora. Se ele ainda está saindo (o
    /// Claude Code leva de 0,5 a 1,6 s gravando a sessão e rodando os hooks de fim; uns 7 s com
    /// um comando de fundo), as mensagens ficam guardadas e o turno abre sozinho quando ele sair
    /// (`abrir_quando_sair`). Quem não sai no prazo é derrubado (`saidas::PRAZO_DE_SAIDA`).
    /// Devolve `true` quando o turno ficou esperando.
    fn esperar_o_anterior_sair(&mut self, chat_id: &str, mensagens: &[Entrada], cx: &mut Context<Self>) -> bool {
        // O chat já tem o processo dele, com turno rodando ou à espera dos agentes: nada abre por
        // cima (o `insert` trocaria o processo do chat por outro). O que ia abrir espera a vez na
        // fila, na frente, e entra quando ele sair.
        if self.processos.contains_key(chat_id) {
            let fila = self.filas.entry(chat_id.to_string()).or_default();
            fila.splice(0..0, mensagens.iter().cloned());
            self.salvar();
            cx.notify();
            return true;
        }
        // O que ainda esperava a sugestão fecha a entrada e passa a contar como saindo.
        self.parar_de_esperar_sugestao(chat_id);
        if !self.encerrando.vivo(chat_id) {
            return false;
        }
        let ja_esperava = self.a_abrir.contains_key(chat_id);
        self.a_abrir.entry(chat_id.to_string()).or_default().extend(mensagens.iter().cloned());
        // O anterior fica calado daqui em diante: o que ele ainda mandar (a sugestão, um turno
        // que abra sozinho) não mexe mais na conversa; se abrir turno, é derrubado (`tratar`).
        self.geracao += 1;
        self.geracao_atual.insert(chat_id.to_string(), self.geracao);
        self.sugestoes.remove(chat_id);
        self.execucao.insert(chat_id.to_string(), Execucao::Rodando { desde: agora() });
        if !ja_esperava {
            self.abrir_quando_sair(chat_id, cx);
        }
        cx.notify();
        true
    }

    /// Confere de pouco em pouco se o processo anterior do chat saiu e abre o turno que esperava.
    fn abrir_quando_sair(&self, chat_id: &str, cx: &mut Context<Self>) {
        let id = chat_id.to_string();
        cx.spawn(async move |motor, cx| {
            loop {
                cx.background_executor().timer(saidas::PASSO_DA_ESPERA).await;
                let segue = motor.update(cx, |m, cx| {
                    m.encerrando.vencer(std::time::Instant::now());
                    // O Parar (ou a conversa apagada) desfez a espera.
                    if !m.a_abrir.contains_key(&id) {
                        return false;
                    }
                    if m.encerrando.vivo(&id) {
                        return true;
                    }
                    let mensagens = m.a_abrir.remove(&id).unwrap_or_default();
                    m.execucao.insert(id.clone(), Execucao::Parado);
                    if mensagens.is_empty() {
                        cx.notify();
                    } else {
                        m.comecar_turno_com(&id, mensagens, true, cx);
                    }
                    false
                });
                if !matches!(segue, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// O turno que esperava o processo anterior sair não abre mais (o Parar, ou fechar o Vitral):
    /// o que você escreveu volta para a fila, na frente; o que era do próprio Vitral (a vez do
    /// outro agente, a revisão da memória, o "continue de onde parou") some.
    fn desistir_de_abrir(&mut self, chat: &str) {
        let Some(mensagens) = self.a_abrir.remove(chat) else {
            return;
        };
        let suas: Vec<Entrada> = mensagens.into_iter().filter(|e| e.de.as_deref() != Some("Vitral")).collect();
        if !suas.is_empty() {
            let fila = self.filas.entry(chat.to_string()).or_default();
            fila.splice(0..0, suas);
        }
        self.revisando.remove(chat);
        self.desfazer_revisao_do_par(chat);
        self.execucao.insert(chat.to_string(), Execucao::Parado);
        self.salvar();
    }

    /// O chat tem turno rodando ou esperando para abrir: as ações que abrem turno (tentar de
    /// novo, continuar) não entram por cima.
    fn com_turno(&self, chat: &str) -> bool {
        self.processos.contains_key(chat) || self.a_abrir.contains_key(chat)
    }

    /// Um processo que já tinha terminado o turno mandou o `init` de outro (um aviso de tarefa
    /// de fundo, por exemplo, abre um turno sozinho). Ele volta a ser o processo do chat, para o
    /// turno aparecer, receber mensagem e o Parar o alcançar; se foi parado por você, é derrubado
    /// (`saidas::ao_acordar`). Devolve se os eventos dele continuam valendo.
    fn acordou(&mut self, chat_id: &str, geracao: u64) -> bool {
        let agora_mesmo = std::time::Instant::now();
        let (mut processo, parado) = match self.sugerindo.remove(chat_id) {
            Some((processo, g)) if g == geracao => (processo, false),
            outro => {
                if let Some(volta) = outro {
                    self.sugerindo.insert(chat_id.to_string(), volta);
                }
                let Some(parado) = self.encerrando.parado(chat_id, geracao) else {
                    return true;
                };
                match self.encerrando.tirar(chat_id, geracao) {
                    Some(processo) => (processo, parado),
                    None => return true,
                }
            }
        };
        match saidas::ao_acordar(parado, self.a_abrir.contains_key(chat_id)) {
            saidas::Acordou::Adotar => {
                self.processos.insert(chat_id.to_string(), processo);
                self.sugestoes.remove(chat_id);
                self.interrompidos.retain(|c| c != chat_id);
                self.execucao.insert(chat_id.to_string(), Execucao::Rodando { desde: agora() });
                if let Some(c) = self.chat_mut(chat_id) {
                    c.turno_aberto = true;
                }
                self.acertar_trava();
                true
            }
            saidas::Acordou::Derrubar => {
                processo.matar();
                self.encerrando.guardar(chat_id, geracao, processo, true, agora_mesmo);
                // Calado: o que ele ainda mandar antes de morrer não entra na conversa.
                self.geracao += 1;
                self.geracao_atual.insert(chat_id.to_string(), self.geracao);
                false
            }
        }
    }

    /// Para todos os processos (ao fechar a janela).
    pub fn encerrar_tudo(&mut self) {
        for (_, mut processo) in self.processos.drain() {
            processo.matar();
        }
        self.acertar_trava();
        for (_, (mut processo, _)) in self.sugerindo.drain() {
            processo.matar();
        }
        // Os que ainda saíam (ou que ficaram trabalhando por fora) também, e o turno que esperava
        // por um deles volta para a fila, para não se perder.
        self.encerrando.derrubar_todos();
        for chat in self.a_abrir.keys().cloned().collect::<Vec<_>>() {
            self.desistir_de_abrir(&chat);
        }
        // O que os processos deixaram em segundo plano.
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
        let nota = entrada.nota;
        self.filas.entry(chat.to_string()).or_default().push(entrada);
        if self.processos.contains_key(chat) || nota {
            self.salvar();
            cx.notify();
            return;
        }
        // Livre: entra tudo o que estava esperando, junto com a mensagem nova.
        self.entregar_fila(chat, cx);
    }

    /// Começa um turno com as mensagens que estavam na fila de um chat parado.
    pub fn entregar_fila(&mut self, chat: &str, cx: &mut Context<Self>) {
        if self.processos.contains_key(chat) || self.na_fila(chat) == 0 {
            return;
        }
        if let Some(fila) = self.tirar_prontas(chat) {
            self.comecar_turno(chat, fila, cx);
        }
    }

    /// O Enter responde a um pedido de permissão aberto neste chat (ver
    /// `responder_pedido_escrevendo`), em vez de dirigir o turno ou esperar na fila.
    pub fn enviar_responde_pedido(&self, chat: &str) -> bool {
        self.pedidos_abertos(chat) > 0
    }

    /// Quantos pedidos de permissão deste chat esperam resposta agora, num turno que os pode
    /// receber (não na revisão da memória, nem com o Vitral só para leitura).
    pub fn pedidos_abertos(&self, chat: &str) -> usize {
        if self.somente_leitura.is_some() || self.revisando.contains_key(chat) || !self.processos.contains_key(chat) {
            return 0;
        }
        self.permissoes.iter().filter(|p| p.chat == chat).count()
    }

    /// Tudo o que este chat tem aberto é pergunta com opções (AskUserQuestion): escrever na caixa
    /// é responder a ela (a dica do Enviar diz isso, em vez de falar em negar).
    pub fn so_perguntas_abertas(&self, chat: &str) -> bool {
        let mut pedidos = self.permissoes.iter().filter(|p| p.chat == chat).peekable();
        pedidos.peek().is_some() && pedidos.all(|p| p.ferramenta == "AskUserQuestion")
    }

    /// Escrever com um pedido de permissão aberto é responder a ele: o Enter nega os pedidos
    /// abertos do chat e o texto vai ao modelo como a explicação da recusa, pelo mesmo caminho do
    /// "Negar e explicar…" do cartão (`responder_permissao` com a mensagem). A mensagem entra na
    /// conversa como a de quem escreveu. Com imagem anexada (que a recusa não leva), os pedidos são
    /// negados com `RESPONDEU_ESCREVENDO` e a mensagem segue para dentro do turno, pela entrada.
    /// Devolve se a mensagem já foi entregue; `false` também para o que vale para o turno inteiro
    /// (comando de barra, Ultrathink, Ultracode), que continua indo para a fila sem negar nada.
    fn responder_pedido_escrevendo(&mut self, chat_id: &str, entrada: &Entrada, cx: &mut Context<Self>) -> bool {
        if !self.enviar_responde_pedido(chat_id) {
            return false;
        }
        let resposta = resposta_ao_pedido(entrada);
        if resposta == RespostaAoPedido::Fila {
            return false;
        }
        let pedidos: Vec<(String, String)> =
            self.permissoes.iter().filter(|p| p.chat == chat_id).map(|p| (p.id.clone(), p.ferramenta.clone())).collect();
        let RespostaAoPedido::Explicacao(texto) = resposta else {
            for (id, _) in pedidos {
                self.responder_permissao(&id, false, Some(RESPONDEU_ESCREVENDO.to_string()), cx);
            }
            return false;
        };
        // Antes da recusa: o resultado negado da chamada vem depois da mensagem, na conversa.
        if let Some(c) = self.chat_mut(chat_id) {
            c.itens.extend(itens_das_entradas(vec![entrada.clone()], None));
            c.atualizado_em = agora();
        }
        *self.dirigidos.entry(chat_id.to_string()).or_default() += 1;
        for (id, ferramenta) in pedidos {
            // Na pergunta com opções, o que foi escrito é a resposta (a "Outra…" de cada
            // pergunta), e não uma recusa: a chamada negada apareceria como "falhou".
            let escritas = self
                .permissoes
                .iter()
                .find(|p| p.id == id && ferramenta == "AskUserQuestion")
                .map(|p| respostas_escritas(&p.entrada, &texto))
                .filter(|respostas| !respostas.is_empty());
            match escritas {
                Some(respostas) => self.responder_pergunta(&id, &respostas, cx),
                None => self.responder_permissao(&id, false, Some(recusa_explicada(&ferramenta, &texto)), cx),
            }
        }
        if let Some(c) = self.chat(chat_id) {
            let _ = self.dados.salvar_chat(c);
        }
        self.salvar();
        cx.notify();
        true
    }

    /// A versão do Claude Code que os turnos abrem; antes de a leitura de fundo voltar, a do link
    /// do instalador nativo.
    fn versao_do_claude(&self) -> Option<Versao> {
        self.versao_claude.or_else(|| claude::versao_pelo_link(&claude::binario_claude()))
    }

    /// O modo, o modelo ou o esforço da conversa mudou com o turno rodando: a escolha vai para o
    /// processo dele (`acertar_turno`) quando ficar `ASSENTAR_A_TROCA` parada. O Shift+Tab passa
    /// pelos modos do caminho (do Planejar ao Manual, pelo Automático e pelo "Sem perguntar"), e
    /// nenhum deles pode valer no turno nem por um instante: só o último vai.
    fn acertar_turno_depois(&mut self, chat: &str, cx: &mut Context<Self>) {
        if !self.processos.get(chat).is_some_and(Processo::entrada_aberta) {
            return;
        }
        let vez = self.trocas.entry(chat.to_string()).or_default();
        *vez += 1;
        let (id, vez) = (chat.to_string(), *vez);
        cx.spawn(async move |motor, cx| {
            cx.background_executor().timer(ASSENTAR_A_TROCA).await;
            let _ = motor.update(cx, |m, _| {
                if m.trocas.get(&id) == Some(&vez) {
                    m.trocas.remove(&id);
                    m.acertar_turno(&id);
                }
            });
        })
        .detach();
    }

    /// Leva ao turno que roda o modo, o modelo e o esforço que a conversa pede agora, pela entrada
    /// do processo (`Processo::acertar`; só no Claude Code). O turno com Ultrathink ou Ultracode
    /// fica com o esforço dele até o fim. Sem processo, com a entrada fechada ou com o pedido
    /// recusado, nada acontece: a escolha vale no turno seguinte, que abre com ela.
    fn acertar_turno(&mut self, chat_id: &str) {
        // A revisão pelo outro agente roda só lendo, seja qual for o modo da conversa.
        if self.revisoes_do_par.contains_key(chat_id) {
            return;
        }
        let Some(chat) = self.chat(chat_id).filter(|c| c.agente == Agente::Claude) else {
            return;
        };
        let pedido = claude::AoVivo {
            modo: self.permissao_de(chat),
            modelo: self.modelo_do_turno(chat).unwrap_or_default(),
            esforco: chat.esforco.clone().filter(|e| !e.is_empty()),
        };
        let reforco = self.ultima_entrada.get(chat_id).is_some_and(|(_, m)| m.iter().any(|e| e.ultrathink || e.ultracode));
        if let Some(processo) = self.processos.get_mut(chat_id) {
            let quer = quer_ao_vivo(pedido, processo.ao_vivo(), reforco);
            processo.acertar(&quer);
        }
    }

    /// Manda a mensagem do usuário para dentro do turno que já está rodando (o steering do Claude
    /// Code; ver `Processo::dirigir` para o que foi medido). Não para o turno, não encerra a rodada
    /// e não abre turno novo pelo Vitral: o texto entra na conversa como mensagem dele e o processo
    /// a recebe pela entrada. `NaFila` quando não dá para dirigir e ela tem de esperar a vez, com o
    /// motivo quando ele vale um aviso.
    fn dirigir_turno(&mut self, chat_id: &str, entrada: &Entrada, cx: &mut Context<Self>) -> Dirigido {
        if self.somente_leitura.is_some() || !self.processos.contains_key(chat_id) {
            return Dirigido::NaFila(None);
        }
        // Um turno sendo parado está indo embora. Esperando os agentes do fundo, a entrada fica
        // aberta e a mensagem entra (o Claude Code abre um turno com ela); sem agentes, o fim
        // adiado é o da mensagem dirigida, e a entrada pode já ter fechado.
        if self.interrompidos.iter().any(|c| c == chat_id) || (self.fim_adiado.contains_key(chat_id) && !self.agentes_no_fundo_com_entrada(chat_id)) {
            return Dirigido::NaFila(None);
        }
        if !matches!(self.execucao_real(chat_id), Execucao::Rodando { .. }) {
            return Dirigido::NaFila(None);
        }
        // O que vale para o turno inteiro não cabe no meio dele: o esforço do Ultrathink e do
        // Ultracode são argumentos do processo. Um comando de barra no meio do turno mexeria na
        // sessão por fora do Vitral.
        if entrada.ultrathink || entrada.ultracode {
            return Dirigido::NaFila(Some(
                "O Ultrathink e o Ultracode valem para o turno inteiro: esta mensagem espera na fila.",
            ));
        }
        if entrada.texto.starts_with('/') {
            return Dirigido::NaFila(Some("Um comando de barra espera o turno acabar."));
        }
        let (texto, imagens) = texto_do_turno(std::slice::from_ref(entrada));
        let mensagem = Mensagem { texto: &texto, imagens: &imagens, marca: None };
        let Some(processo) = self.processos.get_mut(chat_id) else {
            return Dirigido::NaFila(None);
        };
        if !processo.dirigir(mensagem) {
            return Dirigido::NaFila(None);
        }
        // O Claude Code pode absorver a mensagem neste turno ou abrir outro com ela: nos dois
        // casos ela já foi, e o fim do turno espera por isso (ver `adiar_o_fim`).
        self.dirigidas.entry(chat_id.to_string()).or_default().push(entrada.clone());
        *self.dirigidos.entry(chat_id.to_string()).or_default() += 1;
        // Ela não entra na repetição do turno ("Tentar de novo", a troca de conta): quando o turno
        // falha, ela volta para a fila (`devolver_dirigidas`) e iria duas vezes.
        if let Some(c) = self.chat_mut(chat_id) {
            c.itens.extend(itens_das_entradas(vec![entrada.clone()], None));
            c.atualizado_em = agora();
        }
        if let Some(c) = self.chat_gravado(chat_id) {
            let _ = self.dados.salvar_chat(c);
        }
        self.salvar();
        cx.notify();
        Dirigido::Foi
    }

    /// Depois de um `result` com mensagem dirigida engatilhada, o Claude Code abre o turno dela em
    /// milissegundos. Se em `ESPERA_DO_TURNO_DIRIGIDO` ele não abriu (o `Inicio` tira o fim
    /// adiado), a entrada fecha e o processo sai sozinho, como antes.
    fn vigiar_turno_dirigido(&self, chat_id: &str, cx: &mut Context<Self>) {
        let (chat, geracao) = (chat_id.to_string(), self.geracao_atual.get(chat_id).copied());
        cx.spawn(async move |motor, cx| {
            cx.background_executor().timer(ESPERA_DO_TURNO_DIRIGIDO).await;
            let _ = motor.update(cx, |m, _| {
                if m.geracao_atual.get(&chat).copied() == geracao
                    && m.fim_adiado.contains_key(&chat)
                    && let Some(processo) = m.processos.get_mut(&chat)
                {
                    processo.fechar_entrada();
                }
            });
        })
        .detach();
    }

    /// "Mandar agora": tira da fila a mensagem `indice` (o de `fila_do_chat`) e a manda para dentro
    /// do turno que está rodando. Quando o turno não recebe mensagem agora, ela fica na fila e
    /// volta o motivo.
    pub fn mandar_da_fila_agora(&mut self, chat: &str, indice: usize, cx: &mut Context<Self>) -> Result<(), &'static str> {
        const NAO_DEU: &str = "Este turno não recebe mensagem agora; ela continua na fila.";
        let Some(entrada) = self.filas.get(chat).and_then(|f| f.get(indice)).filter(|e| !e.nota).cloned() else {
            return Err("Esta mensagem já saiu da fila.");
        };
        match self.dirigir_turno(chat, &entrada, cx) {
            Dirigido::Foi => {
                if let Some(fila) = self.filas.get_mut(chat) {
                    if indice < fila.len() {
                        fila.remove(indice);
                    }
                    if fila.is_empty() {
                        self.filas.remove(chat);
                    }
                }
                self.salvar();
                cx.notify();
                Ok(())
            }
            Dirigido::NaFila(motivo) => Err(motivo.unwrap_or(NAO_DEU)),
        }
    }

    /// Os agentes do fundo ainda trabalham e a entrada do processo segue aberta: dá para mandar
    /// mensagem nesse tempo.
    fn agentes_no_fundo_com_entrada(&self, chat: &str) -> bool {
        self.fundo.get(chat).copied().unwrap_or(0) > 0 && self.processos.get(chat).is_some_and(Processo::entrada_aberta)
    }

    /// O turno que roda neste chat recebe mensagem agora (o Enviar e o "Mandar agora" entram nele).
    pub fn turno_recebe_mensagem(&self, chat: &str) -> bool {
        self.somente_leitura.is_none()
            && matches!(self.execucao(chat), Execucao::Rodando { .. })
            && (!self.fim_adiado.contains_key(chat) || self.agentes_no_fundo_com_entrada(chat))
            && !self.interrompidos.iter().any(|c| c == chat)
            && self.processos.get(chat).is_some_and(Processo::entrada_aberta)
    }

    /// Quantas mensagens o usuário dirigiu ao turno que roda agora neste chat.
    pub fn dirigidas_no_turno(&self, chat: &str) -> usize {
        self.dirigidos.get(chat).copied().unwrap_or(0)
    }

    /// Devolve à fila as mensagens dirigidas que o turno não chegou a receber (ele falhou, ou foi
    /// parado junto com o processo). Elas já estão na conversa, então voltam marcadas como
    /// registradas: entram no próximo turno sem aparecer duas vezes. Devolve quantas foram.
    fn devolver_dirigidas(&mut self, chat_id: &str) -> usize {
        let Some(dirigidas) = self.dirigidas.remove(chat_id) else {
            return 0;
        };
        let n = dirigidas.len();
        if n > 0 {
            self.filas
                .entry(chat_id.to_string())
                .or_default()
                .extend(dirigidas.into_iter().map(|e| Entrada { registrada: true, ..e }));
            self.salvar();
        }
        n
    }

    /// Repete o último turno de um chat que falhou.
    pub fn tentar_de_novo(&mut self, chat: &str, cx: &mut Context<Self>) {
        if self.com_turno(chat) {
            return;
        }
        if let Some((_, mensagens)) = self.ultima_entrada.get(chat).cloned() {
            self.avisar(chat, "Tentando de novo.", false);
            self.comecar_turno_com(chat, mensagens, false, cx);
        }
    }

    /// Os comandos que o agente deste chat deixou rodando no fundo (vazio com o processo fora).
    pub fn comandos_no_fundo(&self, chat: &str) -> &[(String, String)] {
        if !self.processos.contains_key(chat) {
            return &[];
        }
        self.comandos_no_fundo.get(chat).map_or(&[], Vec::as_slice)
    }

    /// Para um dos comandos de fundo do chat (o `stop_task` dele).
    pub fn parar_comando_no_fundo(&mut self, chat: &str, tarefa: &str, cx: &mut Context<Self>) {
        if let Some(processo) = self.processos.get_mut(chat) {
            processo.parar_tarefa(tarefa);
            self.comandos_parados.insert(chat.to_string(), std::time::Instant::now());
        }
        cx.notify();
    }

    /// Para os comandos de fundo do chat (o `stop_task` de cada um).
    pub fn parar_comandos_no_fundo(&mut self, chat: &str, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.comandos_no_fundo(chat).iter().map(|(id, _)| id.clone()).collect();
        self.comandos_parados.insert(chat.to_string(), std::time::Instant::now());
        if let Some(processo) = self.processos.get_mut(chat) {
            for id in ids {
                processo.parar_tarefa(&id);
            }
        }
        cx.notify();
    }

    /// Os passos que o agente deste chat anotou por último (vazio sem lista).
    pub fn passos(&self, chat: &str) -> &[Passo] {
        self.passos.get(chat).map_or(&[], passos::Passos::lista)
    }

    /// O que é preciso para voltar os arquivos de um chat: a pasta, a pasta da conta e a sessão.
    /// `None` com o chat rodando (o processo vivo ainda mexe nos arquivos) ou sem sessão.
    pub fn onde_voltar_arquivos(&self, chat: &str) -> Option<(PathBuf, Option<PathBuf>, String)> {
        let c = self.chat_gravado(chat)?;
        if self.processos.contains_key(chat) || self.execucao(chat).rodando() || !c.sessao_criada {
            return None;
        }
        Some((c.cwd.clone(), self.conta_para(c).pasta(), c.sessao.clone()))
    }

    /// Um aviso do Vitral na conversa (gravado), fora de um turno.
    pub fn avisar_na_conversa(&mut self, chat: &str, texto: &str, cx: &mut Context<Self>) {
        self.avisar(chat, texto, false);
        if let Some(c) = self.chat_gravado(chat) {
            let _ = self.dados.salvar_chat(c);
        }
        cx.notify();
    }

    /// Quando o chat parado no limite vai continuar sozinho (None se não foi pedido).
    pub fn retomada_agendada(&self, chat: &str) -> Option<u64> {
        self.retomar_as.get(chat).copied()
    }

    /// "Continuar às 14:20": na hora em que o limite volta (com meio minuto de folga, para o plano
    /// já ter renovado), o último turno é repetido, como no Tentar de novo.
    pub fn agendar_retomada(&mut self, chat: &str, quando: u64, cx: &mut Context<Self>) {
        self.retomar_as.insert(chat.to_string(), quando);
        let espera = std::time::Duration::from_secs(quando.saturating_sub(agora()) + 30);
        let id = chat.to_string();
        cx.spawn(async move |motor, cx| {
            cx.background_executor().timer(espera).await;
            let _ = motor.update(cx, |m, cx| {
                if m.retomar_as.get(&id) != Some(&quando) {
                    return;
                }
                m.retomar_as.remove(&id);
                if m.pode_tentar_de_novo(&id) {
                    m.tentar_de_novo(&id, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn cancelar_retomada(&mut self, chat: &str, cx: &mut Context<Self>) {
        if self.retomar_as.remove(chat).is_some() {
            cx.notify();
        }
    }

    pub fn pode_tentar_de_novo(&self, chat: &str) -> bool {
        matches!(self.execucao(chat), Execucao::Erro(_)) && self.ultima_entrada.contains_key(chat)
    }

    /// Tira da conversa uma mensagem do usuário e o que veio depois dela, e devolve o que estava
    /// nela para a caixa. Para o Claude não continuar lembrando do que saiu, o próximo turno roda
    /// numa sessão nova: uma cópia da de agora até o ponto antes da mensagem
    /// (`--resume-session-at`, ver `motor/bifurcar.rs`), que guarda o contexto exato; quando o
    /// ponto não é achado na sessão, uma sessão vazia com o bloco de passagem do que ficou na
    /// tela. Os arquivos da pasta não voltam.
    pub fn retirar_mensagem(&mut self, chat_id: &str, ix: usize, cx: &mut Context<Self>) -> Option<Retirada> {
        if self.somente_leitura.is_some() || self.processos.contains_key(chat_id) {
            return None;
        }
        if !editavel(&self.chat_gravado(chat_id)?.itens, ix) {
            return None;
        }
        // A sessão até o ponto antes da mensagem, lida antes de a mensagem sair da conversa.
        let volta = self.sessao_antes_da_mensagem(chat_id, ix);
        let c = self.chat_mut(chat_id)?;
        let mut saindo = c.itens.split_off(ix);
        let Item::Usuario { texto, anexos, ultrathink, ultracode, .. } = saindo.remove(0) else {
            // O `editavel` já garante que o primeiro é a mensagem; se não for, nada muda.
            c.itens.append(&mut saindo);
            return None;
        };
        c.atualizado_em = agora();
        c.turno_aberto = false;
        // Com o ponto achado na sessão, o próximo turno sai numa cópia dela até ali (o agente
        // fica com o contexto exato, sem resumo); sem ele, abre uma nova com a passagem. Sem
        // sessão, nada a trocar (`bifurcar::sessao_depois_da_correcao`).
        if bifurcar::sessao_depois_da_correcao(c, volta) {
            self.passagens.insert(chat_id.to_string(), MotivoPassagem::Reenvio);
        }
        // Na dupla, o outro agente também viu o que saiu: ele recomeça numa sessão nova e recebe a
        // conversa que ficou na vez dele.
        if let Some(c) = self.chat_mut(chat_id)
            && let Some(p) = c.parceiro.as_mut()
            && p.sessao_criada
        {
            (p.sessao, p.sessao_criada, p.contexto) = (uuid::Uuid::new_v4().to_string(), false, None);
        }
        // O turno que saiu não é mais repetível, e o que ele estava escrevendo não tem mais lugar.
        self.ultima_entrada.remove(chat_id);
        self.parcial.remove(chat_id);
        self.execucao.insert(chat_id.to_string(), Execucao::Parado);
        if let Some(c) = self.chat_gravado(chat_id) {
            let _ = self.dados.salvar_chat(c);
        }
        self.salvar();
        cx.notify();
        Some(Retirada {
            texto,
            anexos,
            reforco: Reforco { ultrathink, ultracode, depois: false },
            saiu_depois: saindo.len(),
        })
    }

    /// As mensagens esperando na fila deste chat, na ordem em que vão entrar (as notas, que não
    /// abrem turno sozinhas, ficam de fora como em `na_fila`).
    pub fn fila_do_chat(&self, chat: &str) -> Vec<(usize, &Entrada)> {
        self.filas
            .get(chat)
            .map(|f| f.iter().enumerate().filter(|(_, e)| !e.nota).collect())
            .unwrap_or_default()
    }

    /// Tira da fila a mensagem na posição `indice` (a mesma de `fila_do_chat`): ela não vai mais
    /// ao Claude. Devolve a mensagem inteira (o texto, os anexos e o reforço), para a caixa: nada
    /// do que foi escrito ou colado se perde num × (rodada de uso, item 1.3).
    pub fn tirar_da_fila(&mut self, chat: &str, indice: usize, cx: &mut Context<Self>) -> Option<Entrada> {
        let fila = self.filas.get_mut(chat)?;
        if indice >= fila.len() || fila[indice].nota {
            return None;
        }
        let entrada = fila.remove(indice);
        if fila.is_empty() {
            self.filas.remove(chat);
        }
        self.salvar();
        cx.notify();
        Some(entrada)
    }

    /// Troca o texto de uma mensagem da fila (o índice é o de `fila_do_chat`). Texto vazio não
    /// troca nada: quem tira é o `tirar_da_fila`. `antes` é o texto que o diálogo abriu: se a fila
    /// andou enquanto ele estava aberto (uma entrou, outra saiu), nada é trocado e volta `false`.
    pub fn editar_na_fila(&mut self, chat: &str, indice: usize, antes: &str, texto: &str, cx: &mut Context<Self>) -> bool {
        let Some(fila) = self.filas.get_mut(chat) else {
            return false;
        };
        if !editar_na_lista(fila, indice, antes, texto) {
            return false;
        }
        self.salvar();
        cx.notify();
        true
    }

    /// Troca uma mensagem da fila de lugar com a vizinha (`para_baixo`: a de depois), para mudar a
    /// ordem em que elas entram. `false` quando ela já está na ponta.
    pub fn mover_na_fila(&mut self, chat: &str, indice: usize, para_baixo: bool, cx: &mut Context<Self>) -> bool {
        let Some(fila) = self.filas.get_mut(chat) else {
            return false;
        };
        if !mover_na_lista(fila, indice, para_baixo) {
            return false;
        }
        self.salvar();
        cx.notify();
        true
    }

    /// Faz andar de novo um chat que parou no meio: entrega a fila ou pede para o Claude continuar
    /// de onde parou.
    pub fn retomar(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let Some(motivo) = self.parada(chat_id) else {
            return;
        };
        let Some(chat) = self.chat_gravado(chat_id).cloned() else {
            return;
        };
        if !recebeu_mensagem(&chat) && self.na_fila(chat_id) > 0 {
            // O que esperava na fila ainda não tinha entrado.
            self.entregar_fila(chat_id, cx);
        } else {
            // Vai junto com o que estiver na fila (relatórios, respostas).
            let texto = format!(
                "O turno anterior parou antes de terminar ({motivo}). Continue de onde parou."
            );
            self.entregar(chat_id, Some("Vitral".into()), texto, cx);
        }
        self.salvar();
        cx.notify();
    }

    /// O último turno para a repetição automática (a conta chegou ao limite e o turno continua na
    /// próxima): sem o Ultracode, que rodaria um fluxo inteiro de novo sem o usuário pedir. Só o
    /// "Tentar de novo" o repete.
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

    /// O processo caiu: fica no registro (para o resumo da volta) e o usuário é avisado se a janela
    /// estiver em segundo plano.
    fn desistir(&mut self, chat_id: &str) {
        let Some(chat) = self.chat_mut(chat_id).map(Chat::copia_sem_itens) else {
            return;
        };
        self.registrar("falhou", &chat.id, &chat.id, &format!("{}: o processo do Claude caiu", chat.titulo));
        if !self.janela_ativa {
            self.notificar(&chat, "Parou com erro: o processo do Claude caiu", true);
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
        if chat.arquivado {
            self.esperar_na_fila(chat_id, mensagens, "Este chat está arquivado; a mensagem entra quando ele for desarquivado.");
            self.salvar();
            cx.notify();
            return;
        }
        // A pasta sumiu (disco desmontado, pasta movida ou apagada): o Claude Code nem abriria.
        if !chat.cwd.is_dir() {
            self.avisar(
                chat_id,
                &format!(
                    "A pasta deste chat não existe mais ({}); a mensagem não foi enviada.",
                    conversas::com_til(&chat.cwd)
                ),
                true,
            );
            self.salvar();
            cx.notify();
            return;
        }
        // Um chat, um processo: com o processo do turno anterior ainda saindo, este turno espera
        // por ele e abre sozinho em seguida (nunca dois processos na mesma sessão).
        if self.esperar_o_anterior_sair(chat_id, &mensagens, cx) {
            return;
        }
        // O @claude ou @codex do começo chama aquele agente: num chat que ainda não tem o outro, a
        // dupla nasce aqui, sem precisar ligar nada (dono, 17/11/2026: "automático quando chamo
        // com o @"), e o chamado recebe a conversa inteira.
        let alvo = mensagens.iter().find(|e| e.do_usuario() && !e.registrada).map(|e| alvo_da_dupla(&e.texto));
        if let Some(AlvoDaDupla::So(chamado)) = alvo
            && chamado != chat.agente
            && chat.parceiro.is_none()
            && chamado.instalado()
            && let Some(c) = self.chat_mut(chat_id)
        {
            let antes = c.agente;
            iniciar_dupla(c, chamado);
            chat = c.copia_sem_itens();
            self.avisar(chat_id, &aviso_da_dupla(chamado, antes), false);
        }
        // Com o Claude e o Codex juntos: a mensagem do usuário escolhe quem trabalha, e quem
        // trabalha recebe o que o outro fez desde a vez dele. Sem @, responde quem está na vez (a
        // revisão de um pelo outro é a do fim do turno, `motor/revisao_do_par.rs`).
        let resumo_da_dupla = if let Some(parceiro) = chat.parceiro.as_ref().map(|p| p.agente) {
            if let Some(alvo) = alvo {
                let primeiro = match alvo {
                    // Um terceiro agente não entra numa dupla já formada: responde quem está na vez.
                    AlvoDaDupla::So(agente) if agente != chat.agente && agente != parceiro => {
                        self.avisar(
                            chat_id,
                            &format!(
                                "Este chat já tem o {} e o {} juntos; o {} não entra nele. Respondeu o {}.",
                                chat.agente.nome(),
                                parceiro.nome(),
                                agente.nome(),
                                chat.agente.nome()
                            ),
                            false,
                        );
                        chat.agente
                    }
                    AlvoDaDupla::So(agente) => agente,
                    AlvoDaDupla::OsDois => chat.agente,
                };
                if primeiro != chat.agente
                    && let Some(c) = self.chat_mut(chat_id)
                {
                    c.trocar_vez();
                    chat = c.copia_sem_itens();
                }
            }
            let itens = self.chat_gravado(chat_id).map_or(&[][..], |c| c.itens.as_slice());
            let outro = chat.parceiro.as_ref().map_or(chat.agente.outro(), |p| p.agente);
            o_que_o_outro_fez(itens, chat.agente, outro)
        } else {
            None
        };
        // Ultracode só com o que ele precisa: sem isso, o Claude Code o ignora calado. Um por vez no
        // Vitral inteiro: a fonte do PC não segura dois fluxos de agentes.
        let claude = chat.agente == Agente::Claude;
        if !chat.agente.instalado() {
            let aviso = format!(
                "O {} não está instalado neste computador (ou não está no PATH); a mensagem não foi enviada.",
                chat.agente.nome()
            );
            self.avisar(chat_id, &aviso, true);
            self.salvar();
            cx.notify();
            return;
        }
        let pediu_ultracode = mensagens.iter().any(|e| e.ultracode);
        let sem_ultracode = pediu_ultracode
            .then(|| {
                self.motivo_sem_ultracode(&chat).or_else(|| {
                    self.ultracode_rodando()
                        .is_some_and(|c| c != chat_id)
                        .then(|| "o Ultracode já roda em outra conversa, e este computador não aguenta dois fluxos de agentes juntos".to_string())
                })
            })
            .flatten();
        let ultracode = pediu_ultracode && sem_ultracode.is_none();
        if sem_ultracode.is_some() {
            for entrada in &mut mensagens {
                entrada.ultracode = false;
            }
        }
        // A sessão gravada expirou (o Claude Code apaga as antigas): uma nova, com as últimas
        // mensagens no começo, em vez de um `--resume` que falharia.
        let pasta_conta = self.conta_para(&chat).pasta();
        let expirou = claude && chat.sessao_criada && claude::sessao_expirou(pasta_conta.as_deref(), &chat.cwd, &chat.sessao);
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
            let ja_na_conversa = mensagens.iter().filter(|e| e.registrada).count();
            let itens = self.chat_gravado(chat_id).map_or(&[][..], |c| c.itens.as_slice());
            bloco_de_passagem(itens, ja_na_conversa, motivo)
        });

        let (texto, imagens) = texto_do_turno(&mensagens);
        // O uuid com que a mensagem vai ao Claude Code, novo a cada processo (a repetição de um turno
        // com o mesmo uuid poderia ser tomada por repetida); o item novo da conversa o guarda.
        let marca = uuid::Uuid::new_v4().to_string();
        // No Codex a marca é provisória: o id do turno chega quando ele começa.
        let codex = chat.agente == Agente::Codex;
        let texto = [passagem, resumo_da_dupla]
            .into_iter()
            .flatten()
            .rfold(texto, |texto, bloco| format!("{bloco}\n\n---\n\n{texto}"));
        let ultrathink = mensagens.iter().any(|e| e.ultrathink);
        let turno = turno_do_lote(self.ultima_entrada.get(chat_id).map(|(t, _)| t.as_str()), &mensagens);
        // O turno da revisão pelo outro agente não é o que o "Tentar de novo" repete.
        if !self.revisoes_do_par.contains_key(chat_id) {
            self.ultima_entrada.insert(chat_id.to_string(), (turno, mensagens.clone()));
        }
        {
            let repeticao = mensagens.iter().all(|e| e.registrada);
            let c = self.chat_mut(chat_id).unwrap();
            c.itens.extend(itens_das_entradas(mensagens, (claude || codex).then_some(marca.as_str())));
            // A marca de quem trabalha, na dupla; a repetição do mesmo turno não marca de novo.
            let ultima_vez = c.itens.iter().rev().find_map(|i| match i {
                Item::Vez { agente, .. } => Some(*agente),
                _ => None,
            });
            if c.parceiro.is_some() && !(repeticao && ultima_vez == Some(c.agente)) {
                c.itens.push(Item::Vez { agente: c.agente, em: agora() });
            }
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
        let mut opcoes = self.opcoes(&chat, servidor.url(&chat.id));
        opcoes.pasta_conta = conta.pasta();
        // A revisão pelo outro agente só lê: Planejar no Claude, que no Codex é o sandbox
        // `read-only` (`agente_codex::sandbox_de`).
        if self.revisoes_do_par.contains_key(chat_id) {
            opcoes.permissao = "plan".into();
        }
        // No `-p` a palavra só injeta o lembrete; o esforço do turno sobe aqui, como no terminal.
        if ultrathink {
            opcoes.esforco = Some(esforco_do_ultrathink(opcoes.esforco.as_deref()).into());
        }
        // Vence o esforço acima: o Ultracode roda sempre em Extra.
        opcoes.ultracode = ultracode;
        // A sessão foi criada, mas a marca não chegou ao disco (queda no primeiro turno).
        if claude
            && !opcoes.retomar
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
            marca: claude.then_some(marca.as_str()),
        };
        // A sugestão do turno anterior sai, e o processo que ainda esperava por ela fecha antes de
        // o novo abrir a mesma sessão.
        self.sugestoes.remove(chat_id);
        self.parar_de_esperar_sugestao(chat_id);
        match Processo::iniciar(&opcoes, mensagem, move |evento| {
            let _ = sinais.send_blocking(Sinal::Claude(id.clone(), geracao, evento));
        }) {
            Ok(mut processo) => {
                // O título gerado: o Claude Code só o cria sozinho no terminal e no VS Code, então
                // o turno o pede enquanto o título for o automático (`titulo_a_pedir`).
                if !self.revisoes_do_par.contains_key(chat_id)
                    && let Some(descricao) = self.titulo_a_pedir(chat_id)
                {
                    processo.pedir_titulo(&descricao);
                }
                self.processos.insert(chat_id.to_string(), processo);
                self.geracao_atual.insert(chat_id.to_string(), geracao);
                self.motivos_de_hook.esquecer(chat_id);
                self.gastos.abrir(chat_id);
                // A mensagem nova do Codex espera o id do turno (a repetição já tem o dela).
                let com_marca = codex
                    && self.chat_gravado(chat_id).is_some_and(|c| {
                        c.itens.iter().any(|i| matches!(i, Item::Usuario { marca: Some(m), .. } if *m == marca))
                    });
                if com_marca {
                    self.marcas_do_codex.insert(chat_id.to_string(), marca.clone());
                } else {
                    self.marcas_do_codex.remove(chat_id);
                }
                self.liberadas.retain(|(chat, _), _| chat != chat_id);
                self.ultimo_sinal.insert(chat_id.to_string(), std::time::Instant::now());
                // Um turno começou (mandado por você ou pela retomada): a volta agendada cai.
                self.retomar_as.remove(chat_id);
                self.acertar_trava();
                self.fundo.remove(chat_id);
                self.esquecer_subagentes(chat_id);
                self.fim_adiado.remove(chat_id);
                self.dirigidas.remove(chat_id);
                self.retiradas.remove(chat_id);
                // O processo novo já abre com o que a conversa pede: a troca que esperava não vai.
                self.trocas.remove(chat_id);
                self.dirigidos.remove(chat_id);
                self.execucao
                    .insert(chat_id.to_string(), Execucao::Rodando { desde: agora() });
                if let Some(c) = self.chat_mut(chat_id) {
                    c.turno_aberto = true;
                }
            }
            Err(erro) => {
                let mensagem = format!("Não consegui abrir o {}: {erro:#}", chat.agente.programa());
                if let Some(c) = self.chat_mut(chat_id) {
                    c.turno_aberto = false;
                }
                self.avisar(chat_id, &mensagem, true);
                self.execucao.insert(chat_id.to_string(), Execucao::Erro(mensagem));
                self.desistir(chat_id);
            }
        }
        if let Some(c) = self.chat_gravado(chat_id) {
            let _ = self.dados.salvar_chat(c);
        }
        self.salvar();
        cx.notify();
    }

    /// O modelo que vai no turno deste chat. No Codex e no Gemini, só o escolhido no chat (sem
    /// ele, vale o padrão do próprio agente). No Claude, a família vai como a versão mais nova que o Claude
    /// Code instalado oferece (ver `claude::modelo_para_o_turno`); num Claude Code sem as versões
    /// exatas, como está.
    fn modelo_do_turno(&self, chat: &Chat) -> Option<String> {
        if chat.agente != Agente::Claude {
            return chat.modelo.clone();
        }
        let modelo = chat.modelo.clone().unwrap_or_else(|| self.config.modelo_chats.clone());
        Some(if motivo_sem_versao_exata(self.versao_claude).is_none() {
            claude::modelo_para_o_turno(&claude::modelos_oferecidos(), &modelo)
        } else {
            modelo
        })
    }

    /// A conta e o modelo de que depende o esforço automático deste chat: a chave da pergunta ao
    /// Claude Code (`claude::atualizar_esforco_padrao`). `None` no Codex, que diz no catálogo.
    pub fn chave_do_esforco(&self, chat: &Chat) -> Option<(Option<PathBuf>, String)> {
        (chat.agente == Agente::Claude).then(|| (self.conta_para(chat).pasta(), self.modelo_do_turno(chat).unwrap_or_default()))
    }

    /// O esforço que o chat usa quando está no automático ("xhigh"), se já se sabe.
    pub fn esforco_automatico(&self, chat: &Chat) -> Option<String> {
        match self.chave_do_esforco(chat) {
            Some((conta, modelo)) => claude::esforco_padrao(conta.as_deref(), &modelo),
            None if chat.agente == Agente::Codex => chat.modelo.as_deref().and_then(crate::agente_codex::esforco_padrao),
            None => None,
        }
    }

    /// A pasta vazia, só do Vitral, onde rodam as perguntas ao Claude Code e ao Codex.
    pub fn pasta_das_perguntas(&self) -> PathBuf {
        self.dados.cache.join("perguntas")
    }

    fn opcoes(&self, chat: &Chat, url_mcp: String) -> Opcoes {
        let onde = if chat.importada.is_some() {
            prompts::Onde::Continuada
        } else if self.sem_pasta(chat) {
            prompts::Onde::SemPasta
        } else {
            prompts::Onde::Direta
        };
        let mcp_da_equipe = crate::memoria_equipe::arquivo_do_mcp(&self.dados.cache, &self.dados.config, &self.config.memoria_da_equipe);
        // As instruções de quem usa (Configurações > Agentes) somam ao que o Vitral manda.
        let prompt_sistema = prompts::com_instrucoes(prompts::do_chat(&chat.cwd, onde, mcp_da_equipe.is_some()), &self.config.instrucoes);
        // Por arquivo, o prompt novo vale já no turno de uma sessão retomada; num Claude Code mais
        // velho (ou antes de saber a versão), vai na linha de comando.
        let prompt_arquivo = self.versao_claude.filter(|v| *v >= claude::VERSAO_PROMPT_POR_ARQUIVO).and_then(|_| {
            let arquivo = self.dados.arquivo_prompt(&chat.id);
            let gravou = arquivo.parent().is_some_and(|p| std::fs::create_dir_all(p).is_ok())
                && std::fs::write(&arquivo, &prompt_sistema).is_ok();
            gravou.then_some(arquivo)
        });
        let claude = chat.agente == Agente::Claude;
        let copia = matches!(chat.agente, Agente::Claude | Agente::Codex);
        let versao = self.versao_do_claude();
        Opcoes {
            agente: chat.agente,
            cwd: chat.cwd.clone(),
            sessao: chat.sessao.clone(),
            retomar: chat.sessao_criada,
            // A conversa importada é do Claude Code: no Codex ela entra pelo bloco de passagem.
            // O chat bifurcado de outro também começa numa cópia: a da sessão do chat de origem.
            // No Codex, a cópia é o `thread/fork` da conversa de origem, até antes do turno do corte.
            bifurcar: crate::importar::bifurcar_de(chat.importada.as_ref(), chat.sessao_criada)
                .filter(|_| claude)
                .or_else(|| chat.bifurcado_de.as_ref().filter(|_| !chat.sessao_criada && copia).map(|b| b.sessao.clone())),
            cortar_em: chat.bifurcado_de.as_ref().filter(|_| !chat.sessao_criada && copia).and_then(|b| b.ate.clone()),
            // A família vai como a versão mais nova que o Claude Code instalado oferece (ver
            // `claude::modelo_para_o_turno`); num Claude Code sem as versões exatas, como está.
            modelo: self.modelo_do_turno(chat),
            permissao: self.permissao_de(chat),
            prompt_sistema,
            prompt_arquivo,
            ferramentas: None,
            url_mcp,
            bloqueadas: SEM_TROCAR_DE_PASTA.map(String::from).to_vec(),
            // O MCP do usuário pode ter centenas de ferramentas: fora por padrão (no Haiku, que não
            // adia a carga delas, o pedido estoura o contexto).
            so_mcp_vitral: !self.config.mcp_trabalhadores,
            esforco: chat.esforco.clone(),
            ultracode: false,
            pasta_conta: None,
            memoria_nativa: self.config.memoria_ao_terminar,
            sugestoes: self.config.sugerir_resposta && claude,
            mcp_da_equipe,
            // No turno que começa antes de a leitura de fundo da versão voltar, vale a do link
            // do instalador nativo.
            eventos_de_hook: versao.is_some_and(|v| v >= claude::VERSAO_EVENTOS_DE_HOOK),
            raciocinio_resumido: versao.is_some_and(|v| v >= claude::VERSAO_RACIOCINIO_RESUMIDO),
        }
    }

    /// Relê a versão do Claude Code instalada: ao abrir as Configurações, que a mostram, e depois
    /// de atualizar por lá — os chips que dependem dela (o Ultracode, as versões exatas de modelo)
    /// precisam acompanhar sem reiniciar o app. O `--version` leva um instante, então vai em
    /// segundo plano.
    pub fn reler_versao_claude(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |motor, cx| {
            let versao = cx.background_spawn(async move { claude::versao_instalada() }).await;
            let _ = motor.update(cx, |m, cx| {
                if versao.is_some() && m.versao_claude != versao {
                    m.versao_claude = versao;
                    // Versão nova do Claude Code pode trazer modelo novo.
                    m.reler_modelos_do_claude(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Troca quem responde no chat e o modelo dele (`None` é o padrão do agente). Mudando o agente,
    /// a conversa de um não abre no outro: o próximo turno começa numa sessão nova, com as últimas
    /// mensagens no bloco de passagem. Com um turno rodando não troca (devolve `false`).
    pub fn definir_agente(&mut self, chat_id: &str, agente: Agente, modelo: Option<String>, cx: &mut Context<Self>) -> bool {
        let Some(antes) = self.chat_gravado(chat_id).map(|c| c.agente) else {
            return false;
        };
        if antes == agente {
            self.mudar_chat(chat_id, |c| c.modelo = modelo, cx);
            return true;
        }
        if self.processos.contains_key(chat_id) {
            self.avisar(chat_id, "Troque de agente quando este turno terminar.", false);
            cx.notify();
            return false;
        }
        // Na dupla, escolher um modelo do outro agente passa a vez para ele, com esse modelo: a
        // próxima mensagem vai para ele, e os dois seguem no chat.
        if self.chat_gravado(chat_id).and_then(|c| c.parceiro.as_ref()).is_some_and(|p| p.agente == agente) {
            self.mudar_chat(
                chat_id,
                move |c| {
                    if let Some(p) = c.parceiro.as_mut() {
                        (p.modelo, p.modelo_atual) = (modelo, None);
                    }
                    c.trocar_vez();
                },
                cx,
            );
            return true;
        }
        // Num chat que já conversou, trocar de agente junta os dois em vez de começar do zero: o
        // de antes guarda a sessão dele (o @ o chama de volta) e o novo recebe a conversa inteira
        // na primeira vez (dono, 17/11/2026: "automático quando troco").
        let tem_conversa = self.chat_gravado(chat_id).is_some_and(|c| {
            c.sessao_criada || c.importada.is_some() || c.itens.iter().any(|i| matches!(i, Item::Usuario { .. }))
        });
        // A dupla é de dois: com outro parceiro já no chat, o terceiro não entra.
        if let Some(parceiro) = self.chat(chat_id).and_then(|c| c.parceiro.as_ref()).map(|p| p.agente) {
            let aviso = format!(
                "Este chat já tem o {} e o {} juntos; separe os dois para trocar para o {}.",
                antes.nome(),
                parceiro.nome(),
                agente.nome()
            );
            self.avisar(chat_id, &aviso, false);
            cx.notify();
            return false;
        }
        if tem_conversa && agente.instalado() {
            if let Some(c) = self.chat_mut(chat_id) {
                iniciar_dupla(c, agente);
                (c.modelo, c.modelo_atual) = (modelo, None);
            }
            self.avisar(chat_id, &aviso_da_dupla(agente, antes), false);
            self.salvar();
            cx.notify();
            return true;
        }
        let mut com_conversa = false;
        if let Some(c) = self.chat_mut(chat_id) {
            com_conversa = c.sessao_criada || c.importada.is_some() || c.itens.iter().any(|i| matches!(i, Item::Usuario { .. }));
            (c.agente, c.modelo, c.modelo_atual) = (agente, modelo, None);
            (c.sessao, c.sessao_criada) = (uuid::Uuid::new_v4().to_string(), false);
            (c.contexto, c.janela, c.custo_da_sessao) = (None, None, None);
            // Os comandos de barra e as skills são do Claude Code: a lista da sessão de antes não
            // vale para a nova, e o catálogo da pasta é perguntado de novo (ao voltar para o Claude
            // ele traz o que mudou no disco enquanto o outro agente trabalhava).
            c.comandos.clear();
            crate::claude::esquecer_comandos_da_pasta(&c.cwd);
            if agente != Agente::Claude {
                c.esforco = c.esforco.take().filter(|e| e != "ultracode");
            }
        }
        if com_conversa {
            self.passagens.insert(chat_id.to_string(), MotivoPassagem::TrocouDeAgente);
            self.avisar(chat_id, &format!("Daqui em diante quem responde é o {}, numa conversa nova com as últimas mensagens.", agente.programa()), false);
        }
        self.salvar();
        cx.notify();
        true
    }

    /// Abre a raia de `de`: um chat novo do outro agente (o parceiro padrão: o Codex para um chat
    /// do Claude, e o Claude para os outros), na mesma pasta, para receber o mesmo pedido. Nasce em
    /// Planejar, que só lê: os dois trabalham na mesma pasta, e dois agentes mexendo nos mesmos
    /// arquivos se atropelam. Devolve o id do chat novo.
    pub fn abrir_raia(&mut self, de: &str, pedido: &str, cx: &mut Context<Self>) -> Result<String, String> {
        let (pasta, agente) = self.chat_gravado(de).map(|c| (c.cwd.clone(), c.agente)).ok_or("o chat não existe mais")?;
        let outro = agente.outro();
        if !outro.instalado() {
            return Err(format!("o {} não está instalado neste computador", outro.nome()));
        }
        let id = self.novo_chat(Some(&pasta), &format!("raia:{de}"), cx)?;
        if let Some(c) = self.chat_mut(&id) {
            (c.agente, c.modelo) = (outro, None);
            c.permissao = Some("plan".into());
            // O mesmo pedido dá o mesmo título: o nome de quem responde na frente separa os dois.
            c.titulo = format!("{} · {}", nome_curto_do_agente(outro), titulo_de(pedido));
        }
        self.salvar();
        Ok(id)
    }

    /// Pergunta ao Codex e ao Gemini instalados quais modelos eles oferecem (em segundo plano) e
    /// redesenha se a lista mudou: ao abrir o app.
    pub fn reler_modelos_do_codex(&self, cx: &mut Context<Self>) {
        let pasta = self.dados.cache.join("perguntas");
        cx.spawn(async move |motor, cx| {
            let mudou = cx
                .background_spawn(async move {
                    let codex = crate::agente_codex::atualizar_modelos(&pasta);
                    crate::agente_gemini::atualizar_modelos(&pasta) || codex
                })
                .await;
            if mudou {
                let _ = motor.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
    }

    /// Pergunta de novo ao Claude Code quais modelos ele oferece (em segundo plano, cerca de 1 s) e
    /// redesenha se a lista mudou: ao abrir o app e quando a versão dele muda.
    pub fn reler_modelos_do_claude(&self, cx: &mut Context<Self>) {
        // A pergunta roda numa pasta vazia do cache do Vitral, e não na temporária do sistema.
        let pasta = self.dados.cache.join("perguntas");
        cx.spawn(async move |motor, cx| {
            let mudou = cx.background_spawn(async move { claude::atualizar_modelos_oferecidos(&pasta) }).await;
            if mudou {
                let _ = motor.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
    }

    /// Roda o `claude update` em segundo plano e, no fim, guarda a resposta e relê a versão: os
    /// chips que dependem dela (o Ultracode, as versões exatas de modelo) mudam sem reiniciar o
    /// app. A tarefa é do motor, então fechar as Configurações no meio não perde o resultado.
    pub fn atualizar_claude(&mut self, cx: &mut Context<Self>) {
        if self.atualizando_claude {
            return;
        }
        self.atualizando_claude = true;
        self.resposta_do_claude = None;
        cx.notify();
        cx.spawn(async move |motor, cx| {
            let resposta = cx.background_spawn(async move { claude::atualizar() }).await;
            let _ = motor.update(cx, |m, cx| {
                m.atualizando_claude = false;
                m.resposta_do_claude = Some(resposta);
                m.reler_versao_claude(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Por que o Ultracode não vale nesta conversa agora (`None`: vale). O de outra conversa
    /// rodando não entra: esse só faz esperar a vez.
    pub fn motivo_sem_ultracode(&self, chat: &Chat) -> Option<String> {
        // Vale com ou sem a dupla: "este chat responde pelo Codex" era falso no chat em que os dois
        // respondem juntos (relatório de design, 16/11/2026).
        if chat.agente != Agente::Claude {
            return Some("só no Claude Code".into());
        }
        let escolhido = chat.modelo.as_deref().unwrap_or(&self.config.modelo_chats);
        let modelo = if escolhido.is_empty() { chat.modelo_atual.as_deref().unwrap_or("") } else { escolhido };
        motivo_sem_ultracode(modelo, self.versao_claude)
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

    /// O Claude Code está resumindo a conversa deste chat agora: a linha do turno diz
    /// "Compactando…".
    pub fn compactando(&self, chat: &str) -> bool {
        self.compactando.contains(chat)
    }

    /// O turno já deu a resposta, mas os agentes dele seguem trabalhando em segundo plano.
    pub fn esperando_agentes(&self, chat: &str) -> bool {
        self.fim_adiado.contains_key(chat)
    }

    /// Quantos agentes o turno roda em segundo plano agora.
    pub fn agentes_no_fundo(&self, chat: &str) -> usize {
        self.fundo.get(chat).copied().unwrap_or(0)
    }

    /// Parar agora derrubaria o processo inteiro (agentes em segundo plano, a espera por eles, um
    /// turno com Ultracode): o Esc só arma, e um segundo Esc para (rodada de uso, item 6.2).
    pub fn parar_custa_caro(&self, chat: &str) -> bool {
        self.execucao_real(chat).rodando()
            && (self.agentes_no_fundo(chat) > 0 || self.fim_adiado.contains_key(chat) || self.turno_com_ultracode(chat))
    }

    /// O turno foi parado por você há pouco (o Esc ou o Parar) e nada mais aconteceu: a faixa
    /// oferece "Continuar", e o Enter com a caixa vazia continua.
    pub fn parou_por_voce(&self, chat: &str) -> bool {
        self.parados_por_voce
            .get(chat)
            .is_some_and(|quando| agora().saturating_sub(*quando) < PARADA_POR_VOCE_VALE && !self.execucao_real(chat).rodando())
    }

    /// Some a oferta de continuar (trocou de chat).
    pub fn esquecer_parada_por_voce(&mut self, chat: &str) {
        self.parados_por_voce.remove(chat);
    }

    /// "Continuar" depois de um Esc: manda a mesma frase do Retomar (item 6.2).
    pub fn continuar_depois_do_esc(&mut self, chat: &str, cx: &mut Context<Self>) {
        if self.parados_por_voce.remove(chat).is_none() || self.com_turno(chat) {
            return;
        }
        let texto = "O turno anterior parou antes de terminar (você o parou). Continue de onde parou.".to_string();
        self.entregar(chat, Some("Vitral".into()), texto, cx);
        self.salvar();
        cx.notify();
    }

    pub fn permissao_de(&self, chat: &Chat) -> String {
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
    pub fn conta_para(&self, chat: &Chat) -> Conta {
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
        // A conta escolhida vale enquanto tem login; sem ele, cai na escolha automática.
        if escolha != contas::AUTOMATICA
            && let Some(conta) = self.contas.iter().find(|c| c.id == escolha)
            && !self.sem_login.contains(&conta.id)
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
            // Conta sem login só entra se não houver outra.
            (self.sem_login.contains(&c.id), uso.bloqueado, volta, (uso.cinco_horas.unwrap_or(0.0) * 1000.0) as i64)
        };
        self.contas
            .iter()
            .min_by_key(folga)
            .cloned()
            .unwrap_or_else(|| self.contas[0].clone())
    }

    /// Há conta do Claude conectada: alguma com o login guardado e não perdido. Sem nenhuma, o
    /// anel de uso do Claude some do canto (dono, 23/11/2026).
    pub fn claude_conectado(&self) -> bool {
        self.contas
            .iter()
            .any(|c| conta_conectada(self.com_login.contains(&c.id), self.sem_login.contains(&c.id), self.recusadas.contains(&c.id)))
    }

    /// O Codex está com a conta dele (o `auth.json`).
    pub fn codex_conectado(&self) -> bool {
        self.codex.as_ref().is_some_and(|c| c.logado)
    }

    /// A conta perdeu o login num turno e ainda não voltou (`login_perdido`).
    pub fn conta_sem_login(&self, id: &str) -> bool {
        self.sem_login.contains(id)
    }

    /// A conta aparece desconectada nas Contas: perdeu o login num turno ou a Anthropic recusou o
    /// token dela na busca do uso (dono, 16/11/2026: "tem que ter como ver se a conta
    /// desconectou e dar pra conectar de novo").
    pub fn conta_desconectada(&self, id: &str) -> bool {
        self.sem_login.contains(id) || self.recusadas.contains(id)
    }

    /// Marca (ou desmarca) a conta sem login e grava a lista quando ela muda.
    fn marcar_sem_login(&mut self, id: &str, sem: bool) -> bool {
        let mudou = if sem { self.sem_login.insert(id.to_string()) } else { self.sem_login.remove(id) };
        if mudou {
            contas::salvar_sem_login(&self.sem_login);
        }
        mudou
    }

    /// A conta que rodou o último turno do chat (a que bateu no limite ou perdeu o login).
    pub fn conta_do_ultimo_turno(&self, chat: &str) -> Option<&Conta> {
        let id = self.conta_do_turno.get(chat)?;
        self.contas.iter().find(|c| &c.id == id)
    }

    /// No limite de uma conta escolhida à mão: outra conta com login e folga, a de menor uso
    /// (rodada de uso, item 3.2). Na Automática a troca já é sozinha (`trocar_de_conta`).
    pub fn outra_conta_com_folga(&self, chat: &Chat) -> Option<Conta> {
        if chat.agente != Agente::Claude {
            return None;
        }
        let atual = self.conta_do_turno.get(&chat.id)?;
        self.contas
            .iter()
            .filter(|c| &c.id != atual && !self.sem_login.contains(&c.id) && !self.uso_da_conta(&c.id).bloqueado)
            .min_by_key(|c| (self.uso_da_conta(&c.id).cinco_horas.unwrap_or(0.0) * 1000.0) as i64)
            .cloned()
    }

    /// "Continuar na <outra>" da faixa: a conta do chat passa a ser `conta` (o seletor de modelo
    /// mostra, e é por lá que se volta) e o mesmo turno roda de novo nela, sem o Ultracode, como a
    /// troca automática (rodada de uso, item 3.2).
    pub fn continuar_em_outra_conta(&mut self, chat_id: &str, conta: &str, cx: &mut Context<Self>) {
        if self.com_turno(chat_id) || !self.pode_tentar_de_novo(chat_id) {
            return;
        }
        let nome_de = |id: &str| self.contas.iter().find(|c| c.id == id).map_or_else(|| id.to_string(), |c| c.nome.clone());
        let (nova, antiga) = (nome_de(conta), self.conta_do_turno.get(chat_id).map(|id| nome_de(id)).unwrap_or_default());
        if let Some(c) = self.chat_mut(chat_id) {
            c.conta = Some(conta.to_string());
        }
        self.avisar(chat_id, &format!("A conta {antiga} chegou ao limite do plano; continuando na conta {nova}."), false);
        self.execucao.insert(chat_id.to_string(), Execucao::Parado);
        if let Some(mensagens) = self.entrada_para_repetir(chat_id) {
            self.comecar_turno_com(chat_id, mensagens, false, cx);
        }
        self.salvar();
        cx.notify();
    }

    /// O login da conta foi refeito no diálogo das Contas (item 8.3): ela volta à escolha, com o
    /// plano que o Claude Code disse.
    pub fn login_refeito(&mut self, id: &str, plano: String, cx: &mut Context<Self>) {
        self.marcar_sem_login(id, false);
        self.recusadas.remove(id);
        self.com_login.insert(id.to_string());
        if let Some(conta) = self.contas.iter_mut().find(|c| c.id == id)
            && !plano.is_empty()
        {
            conta.plano = plano;
        }
        if let Err(erro) = contas::salvar(&self.contas) {
            eprintln!("vitral: não salvou as contas: {erro:#}");
        }
        cx.notify();
    }

    /// O Claude Code disse que o login da conta do turno se perdeu ("OAuth token revoked", "OAuth
    /// session expired and could not be refreshed"). Tentar de novo na mesma conta falha igual, e
    /// era o que o chat fazia (dono, 16/11/2026). A conta sai da escolha e, havendo outra com
    /// login, o mesmo turno roda de novo nela, com o aviso de qual foi; sem outra, o aviso diz
    /// onde entrar de novo e o erro segue o caminho normal. Devolve `true` se repetiu o turno.
    fn login_perdido(&mut self, chat: &str, erro: &str, cx: &mut Context<Self>) -> bool {
        if !falha_de_login(erro) {
            return false;
        }
        let Some(c) = self.chat_mut(chat).map(Chat::copia_sem_itens) else {
            return false;
        };
        let Some(atual) = self.conta_do_turno.get(chat).cloned().filter(|_| c.agente == Agente::Claude) else {
            return false;
        };
        self.marcar_sem_login(&atual, true);
        let nome = self.contas.iter().find(|x| x.id == atual).map_or_else(|| atual.clone(), |x| x.nome.clone());
        let proxima = self.conta_para(&c);
        if proxima.id == atual || self.sem_login.contains(&proxima.id) {
            self.avisar(
                chat,
                &format!(
                    "A conta {nome} perdeu o login no Claude Code. Entre nela de novo em Contas do Claude (menu do logo) e \
                     use Tentar de novo."
                ),
                true,
            );
            return false;
        }
        self.execucao.insert(chat.to_string(), Execucao::Parado);
        self.avisar(
            chat,
            &format!(
                "A conta {nome} perdeu o login no Claude Code; continuando na conta {}. Para voltar a usar a {nome}, \
                 entre nela de novo em Contas do Claude (menu do logo).",
                proxima.nome
            ),
            false,
        );
        if let Some(mensagens) = self.entrada_para_repetir(chat) {
            self.comecar_turno_com(chat, mensagens, false, cx);
        }
        true
    }

    /// Depois de um erro de limite na conta do turno, repete o turno em outra conta se a escolha
    /// do chat for automática e houver conta com folga.
    fn trocar_de_conta(&mut self, chat: &str, erro: &str, cx: &mut Context<Self>) -> bool {
        let Some(c) = self.chat_mut(chat).map(Chat::copia_sem_itens) else {
            return false;
        };
        if c.agente != Agente::Claude {
            return false;
        }
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
        // O chat bifurcado cuja cópia da sessão o Claude Code recusou (`motor/bifurcar.rs`).
        if self.consertar_bifurcacao(chat_id, erro, cx) {
            return true;
        }
        let Some(chat) = self.chat_gravado(chat_id) else {
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

    /// No ritmo das últimas leituras, se cada janela da conta enche antes de renovar.
    pub fn previsao_da_conta(&self, conta: &str) -> contas::previsao::Previsoes {
        self.historico_uso.get(conta).map(|h| h.prever(agora() as i64)).unwrap_or_default()
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
    pub fn reler_uso(&mut self, cx: &mut Context<Self>) {
        let todas = self.contas.clone();
        // O Gemini só de 5 em 5 minutos (o `agy` leva uns 3 s e abre um processo).
        let pasta_gemini = (agora().saturating_sub(self.gemini_perguntado_em) >= GEMINI_A_CADA).then(|| self.pasta_das_perguntas());
        if pasta_gemini.is_some() {
            self.gemini_perguntado_em = agora();
        }
        cx.spawn(async move |motor, cx| {
            let (usos, codex, gemini) = cx
                .background_spawn(async move {
                    let gemini = pasta_gemini.map(|pasta| crate::agente_gemini::ler_uso(&pasta));
                    (contas::usos_guardados(&todas), codex::ler(), gemini)
                })
                .await;
            let _ = motor.update(cx, |m, cx| {
                let mut mudou = m.guardar_usos(usos, codex);
                if let Some(gemini) = gemini {
                    mudou |= m.gemini != gemini || !m.gemini_lido;
                    m.gemini = gemini;
                    m.gemini_lido = true;
                }
                if mudou {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// O "Atualizar" do popover do Gemini: pergunta já, sem esperar os 5 minutos.
    pub fn reler_uso_gemini(&mut self, cx: &mut Context<Self>) {
        self.gemini_perguntado_em = 0;
        self.reler_uso(cx);
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
        self.historico_uso.entry(id.to_string()).or_default().registrar(&lido);
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
                            let com_login = busca::tem_login(&conta);
                            (conta.id, busca, com_login)
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
    fn guardar_buscas(&mut self, buscas: Vec<(String, Busca, bool)>) -> bool {
        self.agenda_uso.terminar();
        let agora = agora();
        let mut mudou = Mudanca::Nada;
        for (id, busca, com_login) in buscas {
            if !self.contas.iter().any(|c| c.id == id) {
                continue;
            }
            // O login guardado apareceu ou sumiu (entrou ou saiu pelo terminal): o anel acompanha.
            let tinha = self.com_login.contains(&id);
            if com_login != tinha {
                if com_login { self.com_login.insert(id.clone()) } else { self.com_login.remove(&id) };
                mudou = Mudanca::Numeros;
            }
            let antes = self.uso_sem_atualizar(&id);
            self.agenda_uso.registrar(&id, &busca, agora);
            // O token lido e aceito prova o login (a marca de um turno que falhou sai); recusado,
            // a conta aparece desconectada nas Contas. Vencido, repetido ou sem rede não dizem nada.
            let desconectada = self.conta_desconectada(&id);
            match &busca {
                Busca::Lida(..) => {
                    self.recusadas.remove(&id);
                    self.marcar_sem_login(&id, false);
                }
                Busca::Recusado { .. } => {
                    self.recusadas.insert(id.clone());
                }
                _ => {}
            }
            if self.conta_desconectada(&id) != desconectada {
                mudou = Mudanca::Numeros;
            }
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
        // Entrar de novo numa conta devolve ela à escolha.
        self.marcar_sem_login(&conta.id, false);
        self.recusadas.remove(&conta.id);
        self.com_login.insert(conta.id.clone());
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

    /// Muda uma opção da conversa (modelo, modo, esforço, conta). O modo, o modelo e o esforço
    /// valem já no turno que está rodando (`acertar_turno_depois`); o resto, a partir do próximo.
    pub fn mudar_chat(&mut self, chat: &str, f: impl FnOnce(&mut Chat), cx: &mut Context<Self>) {
        let antes = self.chat_gravado(chat).map(|c| self.permissao_de(c));
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
        self.acertar_turno_depois(chat, cx);
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

    /// Trata um sinal e diz se a janela precisa redesenhar: o pedaço de texto de uma conversa
    /// que nenhum painel mostra só conta quando é o primeiro (a lista e a Equipe não leem o texto
    /// parcial, mas "escrevendo" muda o tamanho da conversa).
    fn tratar(&mut self, sinal: Sinal, cx: &mut Context<Self>) -> bool {
        self.encerrando.vencer(std::time::Instant::now());
        self.acertar_trava();
        match sinal {
            // Eventos atrasados de um processo anterior (ex.: a saída dele depois que o turno
            // seguinte já começou) não podem mexer no processo novo.
            Sinal::Claude(chat, geracao, evento) => {
                let abriu_turno = matches!(evento, Evento::Inicio { .. });
                if self.geracao_atual.get(&chat) != Some(&geracao) {
                    // Um processo antigo, já calado, abrindo outro turno por conta própria: com
                    // o chat em outro processo (ou esperando para abrir), ele não pode continuar.
                    if abriu_turno && let Some(mut processo) = self.encerrando.tirar(&chat, geracao) {
                        processo.matar();
                        self.encerrando.guardar(&chat, geracao, processo, true, std::time::Instant::now());
                    }
                } else if abriu_turno && !self.processos.contains_key(&chat) && !self.acordou(&chat, geracao) {
                    // O processo que tinha sido parado voltou a trabalhar: derrubado.
                } else {
                    self.ultimo_sinal.insert(chat.clone(), std::time::Instant::now());
                    // Só uma anotação para a estatística do turno: nada na tela muda.
                    if let Evento::PrimeiraPalavra(milis) = evento {
                        self.gastos.anotar_espera(&chat, milis);
                        return false;
                    }
                    if let Evento::TextoParcial(pedaco) = &evento {
                        return match pedaco_de_texto(&mut self.parcial, &self.visiveis, &chat, pedaco) {
                            Pedaco::Nada => false,
                            Pedaco::Cresceu => {
                                self.texto_chegando.push(chat);
                                false
                            }
                            Pedaco::Comecou => true,
                        };
                    }
                    self.tratar_evento(&chat, evento, cx)
                }
            }
            Sinal::Mcp(chamada) => self.tratar_ferramenta(chamada, cx),
            Sinal::Abrir(chat) => {
                if self.chat_gravado(&chat).is_none() {
                    return false;
                }
                self.pedido_de_abrir = Some(chat);
            }
            #[cfg(windows)]
            Sinal::Aviso(aviso) => {
                cx.show_system_notification(aviso);
                return false;
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
                // Turno começando no mesmo processo: a mensagem dirigida saiu da fila interna do
                // Claude Code e é ela que está rodando agora.
                self.dirigidas.remove(chat_id);
                // Com a entrada aberta e sem agentes no fundo, é um turno como outro qualquer: o
                // fim que estava adiado por causa dela deixa de valer, e dá para dirigir de novo.
                if self.fundo.get(chat_id).copied().unwrap_or(0) == 0
                    && self.processos.get(chat_id).is_some_and(Processo::entrada_aberta)
                {
                    self.fim_adiado.remove(chat_id);
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
            Evento::Sessao(id) => {
                // Gravado na hora, como no `Inicio`: é com este id que o próximo turno retoma.
                if let Some(c) = self.chat_mut(chat_id)
                    && (c.sessao != id || !c.sessao_criada)
                {
                    (c.sessao, c.sessao_criada) = (id, true);
                    self.salvar();
                }
            }
            Evento::Contexto(tokens) => {
                if let Some(c) = self.chat_mut(chat_id) {
                    c.contexto = Some(tokens);
                }
            }
            // A mensagem que abriu o turno do Codex guarda o id dele: é o ponto exato para
            // corrigi-la ou continuar dali em outro chat (`bifurcar::ponto_no_codex`).
            Evento::TurnoDoAgente(turno) => {
                if let Some(provisoria) = self.marcas_do_codex.remove(chat_id)
                    && let Some(c) = self.chat_mut(chat_id)
                    && bifurcar::anotar_turno_do_codex(&mut c.itens, &provisoria, &turno)
                {
                    self.chats_sujos.insert(chat_id.to_string());
                }
                return;
            }
            Evento::Fundo(n) => {
                self.fundo.insert(chat_id.to_string(), n);
                // O último agente do fundo terminou com o turno já adiado: a entrada, que ficou
                // aberta para as mensagens desse tempo, fecha (sem nenhuma esperando), e o Claude
                // Code fecha o turno e sai, como antes.
                if n == 0
                    && self.fim_adiado.contains_key(chat_id)
                    && !self.dirigidas.get(chat_id).is_some_and(|d| !d.is_empty())
                    && let Some(processo) = self.processos.get_mut(chat_id)
                {
                    processo.fechar_entrada();
                }
            }
            Evento::AgentesNoFundo(tarefas) => {
                self.segundo_plano.entry(chat_id.to_string()).or_default().listar(tarefas, agora());
            }
            Evento::AgenteAbriu { id, pai } => {
                self.segundo_plano.entry(chat_id.to_string()).or_default().abriu(&id, &pai);
            }
            Evento::Comandos(comandos) => {
                if comandos.is_empty() {
                    self.comandos_no_fundo.remove(chat_id);
                } else {
                    self.comandos_no_fundo.insert(chat_id.to_string(), comandos);
                }
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
                        em: agora(),
                    });
                }
            }
            Evento::Pensando => {
                self.pensando.insert(chat_id.to_string(), std::time::Instant::now());
            }
            Evento::Pensamento(texto) => {
                let milis = self.pensando.remove(chat_id).map_or(0, |inicio| inicio.elapsed().as_millis() as u64);
                let texto = texto.trim();
                // Sem texto e sem medida (ou um instante só) não há o que mostrar.
                if texto.is_empty() && milis < 1_000 {
                    return;
                }
                self.chats_sujos.insert(chat_id.to_string());
                if let Some(c) = self.chat_mut(chat_id) {
                    c.itens.push(Item::Raciocinio { texto: git::inicio_do_texto(texto, 20_000), milis, em: agora() });
                }
            }
            // Só o sinal de vida, que o `tratar` já anotou: nada muda na conversa.
            Evento::Vivo => return,
            // O Parar não tirou a mensagem dirigida da fila do Claude Code (ele não achou o uuid,
            // ou ela chegou depois): ele abriria outro turno com ela. O processo sai inteiro, como
            // saía antes de o Parar retirar, e ela volta para a fila do Vitral no fim do turno.
            Evento::FicouNaFila(quantas) => {
                if quantas > 0 && self.interrompidos.iter().any(|c| c == chat_id) {
                    self.retiradas.remove(chat_id);
                    if let Some(processo) = self.processos.get_mut(chat_id) {
                        processo.matar();
                    }
                }
                return;
            }
            // O modelo trocado no meio do turno passou a valer: é ele que a conversa mostra em uso.
            Evento::ModeloEmUso(modelo) => {
                if let Some(c) = self.chat_mut(chat_id).filter(|_| !modelo.is_empty()) {
                    c.modelo_atual = Some(modelo);
                }
            }
            // O Claude Code resume a conversa (o `/compact`, ou sozinho com o contexto cheio): a
            // linha do turno diz "Compactando…" enquanto dura. Ele repete o estado a cada 30 s.
            Evento::Compactando(sim) => {
                let mudou = if sim { self.compactando.insert(chat_id.to_string()) } else { self.compactando.remove(chat_id) };
                if !mudou {
                    return;
                }
            }
            // Terminou de resumir: o marco entra na conversa, no ponto em que o contexto foi
            // cortado (o que o modelo lembra dali para trás é o resumo).
            Evento::Compactou { manual, antes } => {
                self.compactando.remove(chat_id);
                self.avisar(chat_id, &marco_da_compactacao(manual, antes), false);
            }
            // Um hook do usuário mandou perguntar: o motivo espera o pedido de permissão dessa
            // chamada, que vem em seguida pelo MCP, sem ele (`tratar_ferramenta`).
            Evento::HookPediu { ferramenta, motivo } => {
                self.motivos_de_hook.guardar(chat_id, &ferramenta, &motivo, std::time::Instant::now());
                return;
            }
            Evento::SubRelatorio { pai, texto } => {
                // O resultado da chamada dele só vai dizer que o relatório foi entregue: o cartão
                // fica com este (ver `ResultadoFerramenta` e `SubFim`).
                self.relatorios.insert((chat_id.to_string(), pai.clone()), texto);
                self.subconversa_andou(chat_id, &pai, cx);
            }
            Evento::SubFerramenta { pai, id, nome, entrada } => {
                if nome == "mcp__vitral__permissao" || nome == "ToolSearch" {
                    return;
                }
                // Quantas chamadas este subagente já fez, pelas que chegaram com id: o
                // `task_progress` do Claude Code vem antes da chamada e já a conta (medido na
                // 2.1.328), e somar mais uma aqui dava uma ferramenta a mais no cartão.
                let vistas = (!id.is_empty()).then(|| {
                    let chamadas = self.chamadas_dos_subs.entry(chat_id.to_string()).or_default();
                    chamadas.insert(id, pai.clone());
                    chamadas.values().filter(|de| **de == pai).count() as u32
                });
                self.subconversa_andou(chat_id, &pai, cx);
                self.segundo_plano_andou(chat_id, &pai);
                let passo = claude::resumo_ferramenta(&nome, &entrada);
                self.no_subagente(chat_id, &pai, |sub| {
                    sub.ferramentas = match vistas {
                        Some(vistas) => sub.ferramentas.max(vistas),
                        None => sub.ferramentas + 1,
                    };
                    sub.atividade = passo.clone();
                    sub.passos.push(passo);
                    if sub.passos.len() > crate::modelo::PASSOS_DO_SUBAGENTE {
                        sub.passos.remove(0);
                    }
                });
            }
            Evento::SubTexto { pai, texto } => {
                let linha: String = texto.trim().lines().next().unwrap_or_default().chars().take(160).collect();
                self.subconversa_andou(chat_id, &pai, cx);
                self.segundo_plano_andou(chat_id, &pai);
                if !linha.is_empty() {
                    self.no_subagente(chat_id, &pai, |sub| sub.atividade = linha);
                }
            }
            Evento::Ferramenta { id, nome, entrada } => {
                self.parcial.remove(chat_id);
                // Busca de ferramentas e pedidos de permissão são bastidores: não aparecem.
                if nome == "mcp__vitral__permissao" || nome == "ToolSearch" {
                    return;
                }
                // As ferramentas de passos mudam a lista do chat e dizem na trilha o que mudou
                // ("Começou: rodar os testes" no lugar de "Atualizou os passos").
                let resumo = if passos::e_de_passos(&nome) {
                    self.passos.entry(chat_id.to_string()).or_default().resumo(&id, &nome, &entrada)
                } else {
                    claude::resumo_ferramenta(&nome, &entrada)
                };
                let (mut detalhe, arquivo) = claude::detalhe_ferramenta(&nome, &entrada);
                // O plano que o pedido de permissão trouxe antes de a chamada chegar.
                if nome == "ExitPlanMode"
                    && let Some((_, plano)) = self.planos_sem_chamada.remove(&id)
                    && detalhe.is_none()
                {
                    detalhe = claude::plano_guardado(&plano);
                }
                // O +N −M contado agora, com o texto inteiro (item 5.1).
                let mudanca = claude::mudanca_da_ferramenta(&nome, &entrada);
                // Outro chat mexeu no mesmo arquivo há pouco: sem cópias isoladas, os dois
                // escrevem na mesma pasta, e um pode desfazer o outro sem saber.
                let mexidos = match (&mudanca, &arquivo, self.chat_gravado(chat_id)) {
                    (Some(_), Some(arquivo), Some(c)) => {
                        let caminho = caminho_do_arquivo(&c.cwd, arquivo);
                        let desde = agora().saturating_sub(JANELA_DE_EDICAO_CRUZADA);
                        chats_que_editaram(&self.chats, chat_id, &caminho, desde)
                            .into_iter()
                            .filter(|(id, _)| self.avisos_de_arquivo.insert((chat_id.to_string(), caminho.clone(), id.clone())))
                            .map(|(_, titulo)| (caminho.clone(), titulo))
                            .collect()
                    }
                    _ => Vec::new(),
                };
                self.chats_sujos.insert(chat_id.to_string());
                if let Some(c) = self.chat_mut(chat_id) {
                    c.itens.push(Item::Ferramenta {
                        id,
                        nome,
                        resumo,
                        resultado: None,
                        erro: false,
                        em: agora(),
                        detalhe,
                        arquivo,
                        sub: None,
                        mudanca,
                        imagens: Vec::new(),
                        durou: 0,
                    });
                }
                for (caminho, outro) in mexidos {
                    let nome = caminho.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    self.avisar(chat_id, &format!("O chat «{outro}» também editou {nome} nas últimas horas, na mesma pasta: confira se um não desfaz o outro."), false);
                }
            }
            Evento::ResultadoFerramenta { id, conteudo, erro, no_fundo } => {
                self.chats_sujos.insert(chat_id.to_string());
                // É aqui que o Claude Code absorve no turno a mensagem dirigida (medido na
                // 2.1.278): daqui em diante ela não volta mais como turno à parte.
                self.dirigidas.remove(chat_id);
                let liberada = self.liberadas.remove(&(chat_id.to_string(), id.clone()));
                // O subagente que entregou o relatório pela chamada de entrega: é ele que fica no
                // cartão, e não o aviso de que foi entregue.
                let conteudo = subconversa::relatorio_do_cartao(conteudo, self.relatorios.remove(&(chat_id.to_string(), id.clone())));
                // As ferramentas de tarefa só mudam a lista de passos com o resultado (o id do
                // passo criado vem nele).
                if let Some(lista) = self.passos.get_mut(chat_id) {
                    lista.resultado(&id, &conteudo, erro);
                }
                if let Some(c) = self.chat_mut(chat_id) {
                    anotar_resultado(&mut c.itens, &id, conteudo, erro, no_fundo, liberada, agora());
                }
                // Se a chamada era de um subagente com a conversa aberta, ela leu o fim dele.
                self.subconversa_andou(chat_id, &id, cx);
            }
            // As imagens que o resultado trouxe vão para a pasta do chat nos dados do Vitral, e a
            // chamada guarda os caminhos: o base64 não entra no estado (`imagens_recebidas`).
            Evento::ImagensDoResultado { id, imagens } => {
                let pasta = self.dados.pasta_imagens(chat_id);
                let guardou = self
                    .chat_mut(chat_id)
                    .is_some_and(|c| crate::imagens_recebidas::guardar_na_chamada(&mut c.itens, &id, &pasta, &imagens));
                if guardou {
                    self.chats_sujos.insert(chat_id.to_string());
                }
            }
            Evento::SubLancado { pai } => {
                // Como no resultado de uma chamada: a mensagem dirigida já entrou no turno.
                self.dirigidas.remove(chat_id);
                // É por aqui que se sabe que o fim deste agente vira uma linha na conversa.
                self.segundo_plano.entry(chat_id.to_string()).or_default().lancada(&pai);
                // Só abre o registro dele: até a primeira ação, a linha diz que ele está começando.
                self.no_subagente(chat_id, &pai, |_| {});
            }
            Evento::SubInicio { pai } => {
                // Só abre o registro dele: a linha passa a mostrar 0 ferramentas e o relógio.
                self.no_subagente(chat_id, &pai, |_| {});
            }
            Evento::SubProgresso { pai, ferramentas, tokens } => {
                self.subconversa_andou(chat_id, &pai, cx);
                self.no_subagente(chat_id, &pai, |sub| {
                    sub.ferramentas = sub.ferramentas.max(ferramentas);
                    sub.tokens = sub.tokens.max(tokens);
                });
            }
            Evento::SubFim { pai, erro, resumo, ferramentas, tokens } => {
                // O relatório entregue pela chamada de entrega vale mais que o resumo do aviso,
                // que nesse caso só diz que ele foi entregue. Fica guardado para o resultado da
                // chamada, que no subagente esperado pela conversa ainda vem depois.
                let resumo = subconversa::relatorio_do_cartao(resumo, self.relatorios.get(&(chat_id.to_string(), pai.clone())).cloned());
                self.subconversa_andou(chat_id, &pai, cx);
                self.no_subagente(chat_id, &pai, |sub| {
                    sub.ferramentas = sub.ferramentas.max(ferramentas);
                    sub.tokens = sub.tokens.max(tokens);
                });
                self.chats_sujos.insert(chat_id.to_string());
                let parado_por_voce = self.comandos_parados.get(chat_id).is_some_and(|quando| quando.elapsed() < std::time::Duration::from_secs(10));
                if let Some(c) = self.chat_mut(chat_id) {
                    for item in c.itens.iter_mut().rev() {
                        if let Item::Ferramenta { id, nome, resultado, erro: erro_item, sub, .. } = item
                            && *id == pai
                        {
                            let resumo = if resumo.trim().is_empty() { if erro { "Falhou" } else { "Terminou" }.to_string() } else { resumo };
                            *resultado = Some(git::inicio_do_texto(&resumo, 20_000));
                            // O aviso de fim de um comando de fundo (um Bash) chega por aqui
                            // também: ele não é subagente (não ganha a conta de um), e o que
                            // você mesmo parou pela pílula não falhou.
                            if nome == "Bash" {
                                *erro_item = erro && !parado_por_voce;
                                break;
                            }
                            *erro_item = erro;
                            sub.get_or_insert_with(Default::default).fim = agora();
                            break;
                        }
                    }
                }
                // O agente que foi para o segundo plano avisa aqui, no ponto em que termina: a
                // linha "Terminou: …" entra no fim da conversa (o que a conversa esperou termina
                // no próprio cartão).
                if self.segundo_plano.get(chat_id).is_some_and(|f| f.foi_lancada(&pai))
                    && let Some(c) = self.chat_mut(chat_id)
                {
                    fundo::anotar_fim(&mut c.itens, &pai, erro, agora());
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
                self.historico_uso.entry(conta.clone()).or_default().registrar(&medido);
                let junto = self.uso_da_conta(&conta).juntar(medido, agora() as i64);
                self.uso_por_conta.insert(conta, Uso { bloqueado, ..junto });
            }
            Evento::Gasto(tokens) => self.somar_tokens(chat_id, tokens),
            Evento::Medida(medida) => self.somar_medida(chat_id, medida),
            Evento::PrimeiraPalavra(milis) => self.gastos.anotar_espera(chat_id, milis),
            Evento::Sugestao(texto) => {
                // Só vale com o chat parado: rodando de novo, ela já é do turno que passou.
                if !self.processos.contains_key(chat_id) {
                    self.sugestoes.insert(chat_id.to_string(), texto);
                }
                self.parar_de_esperar_sugestao(chat_id);
            }
            Evento::DirecaoConfirmada { texto, aceita } => {
                if let Some(dirigidas) = self.dirigidas.get_mut(chat_id)
                    && let Some(indice) = dirigidas.iter().position(|e| texto_do_turno(std::slice::from_ref(e)).0 == texto)
                {
                    let entrada = dirigidas.remove(indice);
                    if !aceita {
                        self.filas.entry(chat_id.to_string()).or_default().push(Entrada { registrada: true, ..entrada });
                        self.avisar(chat_id, "O turno já estava terminando; a mensagem ficou na fila para o próximo.", false);
                        self.salvar();
                    }
                }
            }
            // O pedido de permissão do Codex e do Gemini: o mesmo cartão dos do Claude Code, e o
            // "Permitir sempre" da conversa vale para eles também. O modo de agora também responde
            // (Aceitar edições, Sem perguntar: `modo_permite`), e a revisão pelo outro agente, que
            // só lê, tem tudo negado sem perguntar, como no Claude Code.
            Evento::Permissao { ferramenta, entrada, resposta, chamada } => {
                if self.revisoes_do_par.contains_key(chat_id) {
                    let negado = json!({"behavior": "deny", "message": "Esta é uma revisão só de leitura."});
                    let _ = resposta.try_send(Resposta::ok(negado.to_string()));
                    return;
                }
                let responde_sozinho = self.chat(chat_id).is_some_and(|c| {
                    c.permite(&ferramenta, &entrada) || crate::modelo::modo_permite(&self.permissao_de(c), &ferramenta, &entrada, &c.cwd)
                });
                if responde_sozinho {
                    let permitido = json!({"behavior": "allow", "updatedInput": entrada});
                    let _ = resposta.try_send(Resposta::ok(permitido.to_string()));
                    return;
                }
                let pedido = texto_do_pedido(&ferramenta, &entrada);
                self.permissoes.push(PedidoPermissao {
                    id: novo_id(),
                    chat: chat_id.to_string(),
                    ferramenta,
                    entrada,
                    resposta,
                    motivo_do_hook: None,
                    chamada,
                });
                self.execucao.insert(chat_id.to_string(), Execucao::EsperandoVoce);
                self.registrar("permissao", chat_id, chat_id, &pedido);
                if !self.janela_ativa
                    && let Some(chat) = self.chat(chat_id)
                {
                    self.notificar(chat, &pedido, true);
                }
                cx.notify();
            }
            // O Codex fechou o pedido sem esperar (o turno parou, ou foi respondido por outro
            // caminho): o cartão sai, e sem outro esperando o turno volta a rodar.
            Evento::PermissaoRetirada => {
                if tirar_pedidos_fechados(&mut self.permissoes, chat_id) {
                    self.pedido_respondido(chat_id, cx);
                }
            }
            Evento::Fim {
                erro,
                texto,
                custo_usd,
                janela,
            } => {
                // O turno acabou: nada mais compacta (uma compactação que falhou não manda marco).
                self.compactando.remove(chat_id);
                let interrompido = self.interrompidos.iter().any(|c| c == chat_id);
                let agentes = self.fundo.get(chat_id).copied().unwrap_or(0);
                let dirigida_esperando = self.dirigidas.get(chat_id).is_some_and(|d| !d.is_empty());
                if adiar_o_fim(agentes, dirigida_esperando, erro, interrompido) && self.processos.contains_key(chat_id) {
                    // O fluxo (ou subagente) segue em segundo plano e o Claude Code abre outro
                    // turno quando ele acaba; o turno só termina no último `result`. A entrada fica
                    // aberta enquanto os agentes trabalham: a mensagem que o usuário manda nesse
                    // tempo entra no Claude Code na hora, em vez de esperar na fila até eles
                    // acabarem (dono, 23/11/2026: "essa mensagem não tá indo", depois de 53 min de
                    // agentes no fundo). Ela fecha quando o último agente termina (`Fundo(0)`). Sem
                    // agentes, só com a mensagem dirigida engatilhada: o turno dela começa na hora,
                    // no mesmo processo; se não começar, a vigia fecha a entrada e o processo sai.
                    if agentes == 0 {
                        self.vigiar_turno_dirigido(chat_id, cx);
                    }
                    let primeiro = self
                        .fim_adiado
                        .insert(chat_id.to_string(), (texto, custo_usd))
                        .is_none();
                    if let Some(c) = self.chat_mut(chat_id)
                        && janela.is_some()
                    {
                        c.janela = janela;
                    }
                    if primeiro && agentes > 0 {
                        self.avisar(chat_id, crate::modelo::AGENTES_SEGUEM, false);
                    }
                    return;
                }
                // O custo do `result` é o total da sessão até ali: vale o último.
                let (custo_usd, adiado) = match self.fim_adiado.remove(chat_id) {
                    Some((_, antes)) => (custo_usd.max(antes), true),
                    None => (custo_usd, false),
                };
                // O Claude Code segura o `result` do turno que cada agente em segundo plano abre
                // ao terminar e solta todos juntos no fim (medido na 2.1.328 com dois agentes:
                // dois `result` seguidos). O primeiro deles fecha o turno e tira o processo
                // daqui; os seguintes só trazem o gasto deles.
                let ja_terminou = !self.processos.contains_key(chat_id);
                // O turno acabou mesmo: a mensagem que o usuário dirigiu e o Claude Code não
                // recebeu (turno com erro, ou parado) volta para a fila, e o processo sai inteiro
                // para ele não abrir um turno à parte com ela. Menos quando o Parar já a tirou da
                // fila dele: aí ele sai sozinho, com a sessão gravada.
                let dirigidas_perdidas = self.devolver_dirigidas(chat_id);
                let retiradas = self.retiradas.remove(chat_id);
                self.dirigidos.remove(chat_id);
                if let Some(mut processo) = self.processos.remove(chat_id) {
                    // Turno com erro ou parado e agentes ainda em segundo plano: com a entrada
                    // fechada, o Claude Code seguiria esperando por eles (sem teto no Ultracode),
                    // fora da tela, do Parar e da conta de um Ultracode por vez. Saem juntos.
                    let fundo = self.fundo.remove(chat_id).unwrap_or(0) > 0;
                    let geracao = self.geracao_atual.get(chat_id).copied();
                    let sugerir = !erro
                        && !interrompido
                        && !fundo
                        && dirigidas_perdidas == 0
                        && self.config.sugerir_resposta
                        && self.chat_gravado(chat_id).is_some_and(|c| c.agente == Agente::Claude);
                    match geracao.filter(|_| sugerir) {
                        // A sugestão da próxima mensagem só sai com a entrada aberta: ela fica
                        // aberta um pouco mais, à espera dela.
                        Some(geracao) => self.esperar_sugestao(chat_id, processo, geracao, cx),
                        None => {
                            processo.fechar_entrada();
                            if sai_inteiro(fundo, dirigidas_perdidas, retiradas) {
                                processo.matar();
                            }
                            // Guardado por chat até sair: o turno seguinte espera por ele, e o
                            // que foi parado (ou deu erro) não pode voltar a trabalhar por fora.
                            self.encerrando.guardar(
                                chat_id,
                                geracao.unwrap_or(0),
                                processo,
                                erro || interrompido,
                                std::time::Instant::now(),
                            );
                        }
                    }
                }
                if dirigidas_perdidas > 0 {
                    self.avisar(chat_id, &voltou_para_a_fila(dirigidas_perdidas), false);
                }
                let custo_antes = self.chat_gravado(chat_id).map_or(0.0, |c| c.custo_usd);
                if let Some(c) = self.chat_mut(chat_id) {
                    c.somar_custo(custo_usd);
                    c.turno_aberto = false;
                    c.atualizado_em = agora();
                    if janela.is_some() {
                        c.janela = janela;
                    }
                }
                self.registrar_gasto(chat_id, custo_antes);
                if ja_terminou {
                    return;
                }
                // Parado pelo usuário: o Claude Code fecha o turno com erro, mas não é falha (o
                // usuário pode continuar a conversa).
                if self.interrompidos.iter().any(|c| c == chat_id) {
                    self.interrompidos.retain(|c| c != chat_id);
                    self.parcial.remove(chat_id);
                    let revisao = self.revisando.remove(chat_id);
                    self.execucao.insert(chat_id.to_string(), Execucao::Parado);
                    // A revisão pelo outro agente, cortada pela sua mensagem: sai sem deixar nada,
                    // e a mensagem abre o turno dela com o agente do chat.
                    if self.revisoes_do_par.contains_key(chat_id) {
                        self.desfazer_revisao_do_par(chat_id);
                        if let Some(c) = self.chat(chat_id) {
                            let _ = self.dados.salvar_chat(c);
                        }
                        self.entregar_fila(chat_id, cx);
                        self.salvar();
                        return;
                    }
                    match revisao {
                        // A revisão cortada por uma mensagem sua (item 6.1): sem "Interrompido",
                        // pedida de novo no próximo fim, e a mensagem abre o turno dela.
                        Some(revisao) => {
                            if let Some(c) = self.chat_mut(chat_id) {
                                c.memoria_da_sessao = None;
                                c.atualizado_em = c.atualizado_em.min(revisao.atualizado_antes.max(c.lido_em));
                            }
                            self.entregar_fila(chat_id, cx);
                        }
                        None => {
                            self.avisar(chat_id, "Interrompido.", false);
                            self.parados_por_voce.insert(chat_id.to_string(), agora());
                        }
                    }
                    if let Some(c) = self.chat_gravado(chat_id) {
                        let _ = self.dados.salvar_chat(c);
                    }
                    self.salvar();
                    return;
                }
                if erro && self.consertar_sessao(chat_id, &texto, cx) {
                    self.parcial.remove(chat_id);
                    return;
                }
                // Login perdido na conta do turno, com outra conta com login: o mesmo turno roda
                // de novo nela.
                if erro && self.login_perdido(chat_id, &texto, cx) {
                    self.parcial.remove(chat_id);
                    return;
                }
                // Limite do plano numa conta, com outra disponível na escolha automática: o
                // mesmo turno roda de novo na outra conta.
                if erro && self.trocar_de_conta(chat_id, &texto, cx) {
                    return;
                }
                // O turno deu certo nesta conta: o login dela está valendo.
                if !erro && let Some(conta) = self.conta_do_turno.get(chat_id).cloned() {
                    self.marcar_sem_login(&conta, false);
                }
                if erro {
                    self.avisar(
                        chat_id,
                        &format!("O turno terminou com erro: {texto}"),
                        true,
                    );
                    self.execucao
                        .insert(chat_id.to_string(), Execucao::Erro(texto.clone()));
                } else {
                    self.execucao.insert(chat_id.to_string(), Execucao::Parado);
                }
                // Depois de um fim adiado, este `result` pode ser o de um turno do meio (o do
                // primeiro agente que terminou): o que vale como resposta do turno é o que o
                // agente disse por último, que já está na conversa.
                let dito_por_ultimo = || {
                    self.chat_gravado(chat_id)?.itens.iter().rev().find_map(|item| match item {
                        Item::Assistente { texto, .. } => Some(texto.clone()),
                        _ => None,
                    })
                };
                let texto = if adiado && !erro { dito_por_ultimo().unwrap_or(texto) } else { texto };
                self.fim_de_turno(chat_id, erro, texto, cx);
            }
            Evento::Saiu { stderr, codigo } => {
                self.motivos_de_hook.esquecer(chat_id);
                self.compactando.remove(chat_id);
                self.planos_sem_chamada.retain(|_, (chat, _)| chat != chat_id);
                self.retiradas.remove(chat_id);
                self.trocas.remove(chat_id);
                // O processo saiu: o que ele rodava em segundo plano foi junto.
                self.segundo_plano.remove(chat_id);
                // Saiu depois de um `result` adiado, sem outro: os agentes acabaram sem outro turno
                // (o fim normal, com o texto daquele `result`) ou foram cortados junto com o
                // processo (teto da espera, queda), e aí o turno termina com erro.
                if !self.interrompidos.iter().any(|c| c == chat_id)
                    && self.processos.contains_key(chat_id)
                    && let Some((texto, custo_usd)) = self.fim_adiado.remove(chat_id)
                {
                    // Saiu sem abrir outro turno: a mensagem dirigida foi absorvida naquele mesmo
                    // (ou já rodou, no `Inicio`), e não volta para a fila.
                    self.dirigidas.remove(chat_id);
                    self.dirigidos.remove(chat_id);
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
                // Sem `Fim`: o processo caiu ou foi interrompido no meio do turno. O que o usuário
                // dirigiu e o Claude Code não chegou a receber volta para a fila.
                let dirigidas_perdidas = self.devolver_dirigidas(chat_id);
                self.dirigidos.remove(chat_id);
                if self.processos.remove(chat_id).is_some() {
                    if dirigidas_perdidas > 0 {
                        self.avisar(chat_id, &voltou_para_a_fila(dirigidas_perdidas), false);
                    }
                    self.parcial.remove(chat_id);
                    let interrompido = self.interrompidos.contains(&chat_id.to_string());
                    self.interrompidos.retain(|c| c != chat_id);
                    if let Some(c) = self.chat_mut(chat_id) {
                        c.turno_aberto = false;
                    }
                    let revisao = self.revisando.remove(chat_id);
                    if self.revisoes_do_par.contains_key(chat_id) {
                        // O processo do revisor caiu ou foi cortado: cortado pela sua mensagem,
                        // sai sem deixar nada; caído, deixa só a linha de que não terminou.
                        if interrompido {
                            self.desfazer_revisao_do_par(chat_id);
                            self.entregar_fila(chat_id, cx);
                        } else {
                            let motivo = if stderr.trim().is_empty() { "o processo fechou sem resposta".to_string() } else { stderr };
                            self.fechar_revisao_do_par(chat_id, true, &motivo, cx);
                        }
                    } else if let Some(revisao) = revisao {
                        // A revisão da memória caiu ou foi cortada: o chat fica parado como estava
                        // depois do turno de trabalho, e o que esperava na fila entra (item 6.1).
                        self.execucao.insert(chat_id.to_string(), Execucao::Parado);
                        if let Some(c) = self.chat_mut(chat_id) {
                            if interrompido {
                                c.memoria_da_sessao = None;
                            }
                            c.atualizado_em = c.atualizado_em.min(revisao.atualizado_antes.max(c.lido_em));
                        }
                        self.entregar_fila(chat_id, cx);
                    } else if interrompido {
                        self.avisar(chat_id, "Interrompido.", false);
                        self.execucao.insert(chat_id.to_string(), Execucao::Parado);
                        self.parados_por_voce.insert(chat_id.to_string(), agora());
                    } else if self.consertar_sessao(chat_id, &stderr, cx) {
                        // Repetido com a sessão certa.
                    } else {
                        let programa = self.chat_gravado(chat_id).map_or(Agente::Claude, |c| c.agente).programa();
                        let detalhe = if stderr.trim().is_empty() {
                            format!("o processo do {programa} fechou sem resposta")
                        } else {
                            stderr
                        };
                        self.avisar(chat_id, &format!("Falhou: {detalhe}"), true);
                        self.execucao.insert(chat_id.to_string(), Execucao::Erro(detalhe));
                        self.desistir(chat_id);
                    }
                    if let Some(c) = self.chat_gravado(chat_id) {
                        let _ = self.dados.salvar_chat(c);
                    }
                    self.salvar();
                }
            }
        }
    }

    /// A descrição com que o turno que abre agora pede ao Claude Code o título da sessão
    /// (`Processo::pedir_titulo`), ou `None` quando não é o caso: o título do chat não é mais o
    /// automático, o chat é do Codex, a primeira fala é curta demais, o Claude Code instalado é
    /// anterior ao pedido ou este chat já pediu duas vezes nesta abertura do app. A segunda vez
    /// cobre o turno que acabou antes de o título chegar (o processo sai e o pedido se perde); mais
    /// que isso seria uma chamada ao modelo a cada turno de um chat que ele não soube titular.
    fn titulo_a_pedir(&mut self, chat_id: &str) -> Option<String> {
        const VEZES: u8 = 2;
        if !crate::opcoes::titulo_pelo_claude_vale(self.config.titulo_pelo_claude, true) {
            return None;
        }
        if !self.versao_do_claude().is_some_and(|v| v >= claude::VERSAO_TITULO_GERADO) {
            return None;
        }
        let descricao = descricao_para_o_titulo(self.chat(chat_id)?)?;
        let pedidos = self.titulos_pedidos.entry(chat_id.to_string()).or_default();
        (*pedidos < VEZES).then(|| {
            *pedidos += 1;
            descricao
        })
    }

    /// Depois de um turno: o título que ainda é o das primeiras palavras dá lugar ao que o Claude
    /// Code gravou na sessão (`ai-title`), se houver. Uma vez só, e nunca por cima de um nome dado
    /// à mão (`titulo_gerado_para`). Lê só o fim do arquivo da sessão.
    fn trocar_pelo_titulo_gerado(&mut self, chat_id: &str) {
        if !crate::opcoes::titulo_pelo_claude_vale(self.config.titulo_pelo_claude, true) {
            return;
        }
        let Some(chat) = self.chat(chat_id).filter(|c| c.agente == Agente::Claude && titulo_automatico(c)) else {
            return;
        };
        let pasta_conta = self.conta_para(chat).pasta();
        let Some(arquivo) = claude::arquivo_da_sessao(pasta_conta.as_deref(), &chat.cwd, &chat.sessao) else {
            return;
        };
        let Some(titulo) = titulo_gerado_para(chat, &crate::importar::titulos_da_sessao(&arquivo)) else {
            return;
        };
        if let Some(c) = self.chat_mut(chat_id) {
            c.titulo = titulo;
        }
    }

    fn fim_de_turno(&mut self, chat_id: &str, erro: bool, texto: String, cx: &mut Context<Self>) {
        self.parcial.remove(chat_id);
        // O fim da revisão pelo outro agente: o resultado entra na conversa e o chat volta a quem
        // era. Não é um "Terminou" novo (o do turno revisto já saiu).
        if self.revisoes_do_par.contains_key(chat_id) {
            self.fechar_revisao_do_par(chat_id, erro, &texto, cx);
            return;
        }
        // Antes de tudo: o aviso e a linha do "Enquanto você esteve fora" já saem com o título novo.
        self.trocar_pelo_titulo_gerado(chat_id);
        if !erro {
            self.repetidos_sessao.remove(chat_id);
        }
        // O fim da revisão da memória não é um "Terminou" novo: o do turno de trabalho já saiu.
        // Sem notificação, sem registro e sem deixar o chat como não lido outra vez; um erro dela
        // vira só uma linha (item 6.1).
        if let Some(revisao) = self.revisando.remove(chat_id) {
            if erro {
                self.execucao.insert(chat_id.to_string(), Execucao::Parado);
                let linha = texto.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string();
                self.avisar(chat_id, &format!("A revisão da memória não terminou: {linha}"), false);
            }
            if let Some(c) = self.chat_mut(chat_id)
                && c.lido_em >= revisao.atualizado_antes
            {
                c.lido_em = c.lido_em.max(c.atualizado_em);
            }
            if let Some(c) = self.chat_gravado(chat_id) {
                let _ = self.dados.salvar_chat(c);
            }
            if self.na_fila(chat_id) > 0
                && let Some(fila) = self.tirar_prontas(chat_id)
            {
                self.comecar_turno(chat_id, fila, cx);
            }
            self.salvar();
            return;
        }
        if let Some(c) = self.chat_gravado(chat_id) {
            let _ = self.dados.salvar_chat(c);
        }
        let Some(chat) = self.chat_mut(chat_id).map(Chat::copia_sem_itens) else {
            return;
        };
        // Para o resumo de quando o usuário volta ("Enquanto você esteve fora").
        let tipo = if erro { "falhou" } else { "terminou" };
        self.registrar(tipo, &chat.id, &chat.id, &linha_do_fim(&chat.titulo, &texto, erro));
        // Mensagens que chegaram durante o turno viram o próximo (as notas esperam uma de verdade).
        if self.na_fila(chat_id) > 0
            && let Some(fila) = self.tirar_prontas(chat_id)
        {
            self.comecar_turno(chat_id, fila, cx);
        } else if !erro && self.pedir_revisao_do_par(chat_id, cx) {
            // O outro agente revê o turno por baixo, só lendo; o "Terminou" do turno de trabalho
            // sai agora, e a revisão da memória fica para depois dele.
            if !self.janela_ativa {
                self.notificar(&chat, &texto_do_fim(&texto), false);
            }
        } else if !erro && self.pedir_revisao_da_memoria(chat_id, cx) {
            // O turno da memória abriu por baixo; o "Terminou" do turno de trabalho sai agora, sem
            // esperar a revisão (item 6.1).
            if !self.janela_ativa {
                self.notificar(&chat, &texto_do_fim(&texto), false);
            }
        } else if !self.janela_ativa && !erro {
            self.notificar(&chat, &texto_do_fim(&texto), false);
        } else if !self.janela_ativa && erro {
            // O turno com erro avisa em segundo plano como o Terminou, e com urgência: travou
            // trabalho (rodada de uso, item 3.2; o LEIA-ME já prometia).
            self.notificar(&chat, &texto_do_erro(&texto), true);
        }
        self.salvar();
    }

    /// A revisão da memória, nativa (dono, 13/11/2026: "deixa nativo no Vitral o de quando acabar
    /// o chat ele atualizar a memória, igual é aqui"; no VS Code quem faz isso é um hook `Stop`
    /// dele). Ao fim de um turno sem erro, se a sessão mexeu em arquivo desde o último pedido e
    /// ainda não foi pedido nesta sessão, o Vitral manda ao agente o crivo do que vale guardar. É
    /// uma vez por sessão; a conversa mostra só uma linha do Vitral, e o crivo vai por baixo
    /// (`Entrada.extra`). Devolve `true` se abriu o turno.
    fn pedir_revisao_da_memoria(&mut self, chat_id: &str, cx: &mut Context<Self>) -> bool {
        if !self.config.memoria_ao_terminar || self.somente_leitura.is_some() {
            return false;
        }
        let Some(c) = self.chat_mut(chat_id) else {
            return false;
        };
        if c.memoria_da_sessao.as_deref() == Some(c.sessao.as_str()) || !editou_arquivo_desde(&c.itens, c.memoria_pedida_em) {
            return false;
        }
        (c.memoria_da_sessao, c.memoria_pedida_em) = (Some(c.sessao.clone()), agora());
        let revisao = Revisao { desde_item: c.itens.len(), atualizado_antes: c.atualizado_em };
        let entrada = Entrada {
            de: Some("Vitral".into()),
            texto: PEDIDO_DE_REVISAO.into(),
            extra: CRIVO_DA_MEMORIA.into(),
            ..Entrada::default()
        };
        self.revisando.insert(chat_id.to_string(), revisao);
        self.entregar_entrada(chat_id, entrada, cx);
        // Não abriu (a pasta sumiu, por exemplo): nada a rever. O turno que espera o processo
        // anterior sair (`a_abrir`) vai abrir.
        if !self.com_turno(chat_id) {
            self.revisando.remove(chat_id);
        }
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
        if !mcp::pode(&chamada.ferramenta) {
            let erro = format!("Ferramenta \"{}\" não existe para este chat.", chamada.ferramenta);
            let _ = chamada.resposta.try_send(Resposta::erro(erro));
            return;
        }
        let chamada = match self.ferramenta_de_conversa(&chat, chamada, cx) {
            Ok(()) => {
                self.salvar();
                cx.notify();
                return;
            }
            Err(chamada) => chamada,
        };
        let a = &chamada.argumentos;
        let _texto = |chave: &str| a[chave].as_str().unwrap_or_default().trim().to_string();

        let resposta = match chamada.ferramenta.as_str() {
            "permissao" => {
                // A revisão pelo outro agente só lê: o que ela pedir (até o plano do Planejar) é
                // negado sem perguntar a você.
                if self.revisoes_do_par.contains_key(&chat.id) {
                    let resposta = json!({
                        "behavior": "deny",
                        "message": "Esta é uma revisão só de leitura: responda com o texto da revisão, sem mudar nada e sem propor plano.",
                    });
                    let _ = chamada.resposta.try_send(Resposta::ok(resposta.to_string()));
                    return;
                }
                let ferramenta = a["tool_name"].as_str().unwrap_or("?");
                // O pedido que um hook do usuário mandou fazer chega aqui em qualquer modo, até no
                // "Sem perguntar", e sem o motivo: ele veio antes, no evento do hook.
                let motivo_do_hook = self.motivos_de_hook.tirar(&chat.id, ferramenta, std::time::Instant::now());
                // "Permitir sempre" desta conversa: vale já no turno em andamento, menos para o
                // que um hook mandou perguntar.
                if pedidos_de_hook::passa_sem_perguntar(&chat, ferramenta, &a["input"], motivo_do_hook.is_some()) {
                    let resposta = json!({"behavior": "allow", "updatedInput": a["input"]});
                    let _ = chamada.resposta.try_send(Resposta::ok(resposta.to_string()));
                    return;
                }
                // O plano só vem aqui (a chamada chega vazia desde o 2.1.328): fica guardado na
                // chamada, para o cartão da conversa abrir depois de respondido.
                if ferramenta == "ExitPlanMode"
                    && let (Some(chamada_do_plano), Some(plano)) = (a["tool_use_id"].as_str(), a["input"]["plan"].as_str())
                {
                    let achou = self.chat_mut(&chat.id).is_some_and(|c| guardar_plano(&mut c.itens, chamada_do_plano, plano));
                    if achou {
                        self.chats_sujos.insert(chat.id.clone());
                    } else if !plano.trim().is_empty() {
                        self.planos_sem_chamada.insert(chamada_do_plano.to_string(), (chat.id.clone(), plano.to_string()));
                    }
                }
                self.permissoes.push(PedidoPermissao {
                    id: novo_id(),
                    chat: chat.id.clone(),
                    ferramenta: ferramenta.to_string(),
                    entrada: a["input"].clone(),
                    motivo_do_hook,
                    chamada: a["tool_use_id"].as_str().filter(|id| !id.is_empty()).map(str::to_string),
                    resposta: chamada.resposta,
                });
                self.execucao
                    .insert(chat.id.clone(), Execucao::EsperandoVoce);
                let pedido = texto_do_pedido(a["tool_name"].as_str().unwrap_or("?"), &a["input"]);
                self.registrar("permissao", &chat.id, &chat.id, &pedido);
                // Como os outros avisos, só com a janela sem foco: com ela à frente, o pedido já
                // aparece no app (e se responde lá, nunca na notificação).
                if !self.janela_ativa {
                    self.notificar(&chat, &texto_do_pedido(a["tool_name"].as_str().unwrap_or("?"), &a["input"]), true);
                }
                return;
            }
            outra => Resposta::erro(format!("Ferramenta \"{outra}\" não existe para este chat.")),
        };
        let _ = chamada.resposta.try_send(resposta);
        self.salvar();
    }

    /// Grava a conversa em markdown (em `.vitral/exportadas`) para outro chat ler como contexto.
    pub fn exportar_conversa(&self, id: &str) -> anyhow::Result<PathBuf> {
        let chat = self.chat_gravado(id).ok_or_else(|| anyhow::anyhow!("conversa não encontrada"))?;
        let mut texto = format!("# Conversa \"{}\" (pasta {})\n\n", chat.titulo, chat.cwd.display());
        for item in &chat.itens {
            match item {
                Item::Usuario { texto: t, .. } => texto.push_str(&format!("## Usuário\n\n{t}\n\n")),
                Item::Assistente { texto: t, .. } => texto.push_str(&format!("## Claude\n\n{t}\n\n")),
                Item::Entrega { de, texto: t, .. } => {
                    texto.push_str(&format!("## Mensagem de {de}\n\n{t}\n\n"))
                }
                Item::Ferramenta { resumo, .. } => texto.push_str(&format!("- (ferramenta) {resumo}\n")),
                Item::Vez { agente, .. } => texto.push_str(&format!("### Vez do {}\n\n", agente.nome())),
                Item::Revisao { de, texto: t, .. } => texto.push_str(&format!("## Revisão do {}\n\n{t}\n\n", de.nome())),
                Item::Raciocinio { .. } | Item::Aviso { .. } | Item::Cartao { .. } | Item::FimDoAgente { .. } | Item::Desconhecido(_) => {}
            }
        }
        let pasta = self.dados.pasta_exportadas();
        std::fs::create_dir_all(&pasta)?;
        let arquivo = pasta.join(format!("{}-{}.md", slug(&chat.titulo), chat.id));
        std::fs::write(&arquivo, texto)?;
        Ok(arquivo)
    }

    /// O chat conta como lido agora. Só muda (e marca o estado para a gravação agendada, a cada
    /// 5 s em `gravar_sujos`) quando havia algo não lido: chamado a cada quadro com o fim à vista,
    /// ele não grava o estado a cada quadro. Desde que abrir deixou de marcar como lido (rodada de
    /// uso, item 2.2), o lido que só morava na memória se perdia ao fechar pelo × (conferência na
    /// bancada, 16/11/2026).
    pub fn marcar_lido(&mut self, chat: &str) {
        let mudou = self.chat_mut(chat).is_some_and(|c| ler_ate_agora(c, agora()));
        self.estado_sujo |= mudou;
    }

    /// "Marcar como não lido" no menu do chat: o ponto azul volta e fica até a próxima visita.
    pub fn marcar_nao_lido(&mut self, chat: &str, cx: &mut Context<Self>) {
        let mudou = self.chat_mut(chat).is_some_and(marcar_como_nao_lido);
        if mudou {
            self.estado_sujo = true;
            cx.notify();
        }
    }

    /// O usuário abriu o chat (pela lista, por uma aba) ou ativou o painel dele: solta a marca de
    /// "não lido". Quem lê é o fim à vista (`marcar_lido`), como sempre.
    pub fn visitar(&mut self, chat: &str) {
        if let Some(c) = self.chat_mut(chat) {
            visitar_chat(c);
        }
    }

    /// Antes de a janela fechar pelo × ou pelo Alt+F4 (o `on_app_quit` da Janela não roda no
    /// `remove_window`): o estado e as conversas que esperavam a gravação vão para o disco agora.
    pub fn gravar_antes_de_fechar(&mut self) {
        if self.somente_leitura.is_some() {
            return;
        }
        for id in std::mem::take(&mut self.chats_sujos) {
            if let Some(c) = self.chat_gravado(&id) {
                let _ = self.dados.salvar_chat(c);
            }
        }
        self.estado_sujo = false;
        if std::mem::take(&mut self.quadro_sujo) {
            self.salvar_quadro();
        }
        // A hora da saída, para o "Enquanto você esteve fora" da próxima abertura (grava o estado).
        self.visto_em = agora();
        self.salvar();
        self.dados.esperar_gravacoes();
    }

    /// Algo novo desde a última vez que o usuário abriu a conversa.
    pub fn nao_lido(&self, chat: &Chat) -> bool {
        chat.atualizado_em > chat.lido_em && !chat.itens.is_empty() && !self.execucao(&chat.id).rodando()
    }

    // ---------- textos ----------

    fn avisar(&mut self, chat: &str, texto: &str, erro: bool) {
        self.chats_sujos.insert(chat.to_string());
        if let Some(c) = self.chat_mut(chat) {
            c.itens.push(Item::Aviso {
                texto: texto.to_string(),
                erro,
                em: agora(),
            });
        }
    }

    /// Notificação do sistema, numa thread que espera o clique: a primeira linha do
    /// `notify-send` é o id (para a próxima do mesmo chat substituir esta), a seguinte é a ação.
    /// `urgente`: o pedido de permissão e a queda com erro, que travam trabalho (item 3.1).
    fn notificar(&self, chat: &Chat, texto: &str, urgente: bool) {
        let titulo = chat.titulo.clone();
        let corpo = texto.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().to_string();
        // No Windows, o aviso do sistema (toast) com o AppUserModelID do Vitral: o do mesmo chat
        // substitui o anterior, e o clique volta como `Sinal::Abrir`.
        #[cfg(windows)]
        {
            // O toast do GPUI não tem o cenário de lembrete: lá o urgente sai como os outros.
            let _ = urgente;
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
                    .args(argumentos_do_aviso(&titulo, &corpo, aviso.id, urgente))
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
            chats: &self.chats,
            filas: &self.filas,
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
            if let Some(c) = self.chat_gravado(&id) {
                let _ = self.dados.salvar_chat(c);
            }
        }
        // O lido que mudou desde a última passada (`marcar_lido`).
        if std::mem::take(&mut self.estado_sujo) {
            self.salvar();
        }
        if std::mem::take(&mut self.quadro_sujo) {
            self.salvar_quadro();
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

}

/// O que o agente recebe junto com o pedido de revisão da memória: o mesmo crivo do hook `Stop`
/// do dono (`eita/.claude/hooks/lembrar-instrucoes.js`), sem o que é só daquele workspace.
/// A revisão da memória em andamento num chat (item 6.1).
#[derive(Clone, Copy, Debug)]
struct Revisao {
    /// Onde ela começa na conversa (a linha do Vitral que a pede).
    desde_item: usize,
    /// Quando o chat tinha mudado antes dela: o fim dela não o deixa como não lido de novo.
    atualizado_antes: u64,
}

/// A linha que o Vitral põe na conversa ao pedir a revisão da memória (o crivo vai por baixo).
pub const PEDIDO_DE_REVISAO: &str = "Antes de encerrar: reveja o que desta sessão vale guardar na memória e nas instruções.";

/// O item é o pedido da revisão da memória.
pub fn e_revisao_da_memoria(item: &Item) -> bool {
    matches!(item, Item::Entrega { de, texto, .. } if de == "Vitral" && texto == PEDIDO_DE_REVISAO)
}

/// A ferramenta grava algo: o critério da revisão que já escreveu (e não se corta mais) e da
/// linha "Memória: editou …" (item 6.1). Pela ferramenta, não pelo texto.
pub fn ferramenta_de_escrita(nome: &str) -> bool {
    let curto = nome.rsplit("__").next().unwrap_or(nome);
    matches!(curto, "Edit" | "Write" | "MultiEdit" | "NotebookEdit") || curto.to_ascii_lowercase().contains("write")
}

/// A revisão (os itens dela) já usou alguma ferramenta de escrita.
fn revisao_escreveu(itens: &[Item]) -> bool {
    itens.iter().any(|i| matches!(i, Item::Ferramenta { nome, .. } if ferramenta_de_escrita(nome)))
}

const CRIVO_DA_MEMORIA: &str = "\
Passe esta sessão por este crivo. Se o projeto ou o usuário têm uma instrução ou skill sobre como \
manter as instruções (por exemplo uma skill `manter-instrucoes`), siga-a e leia-a antes, se ainda \
não leu nesta sessão.

1. Algo aqui vira REGRA DURÁVEL? Só conta: armadilha que custou tempo, decisão do dono (com data \
e motivo), premissa que se provou falsa, fato de API descoberto ao vivo, ou escolha deliberada que \
parece erro. O que o código ou o git já dizem NÃO conta.
2. Se sim, qual camada? O arquivo de instruções do projeto (AGENTS.md ou CLAUDE.md: regra de como \
se trabalha ali), uma skill (procedimento repetível) ou a memória (fato da operação).
3. Isso CONTRADIZ ou torna obsoleta alguma instrução existente? Corrija-a na mesma passada: \
corrigir vale mais que acrescentar.
4. Mostre o diff do que for gravar.

Se a resposta de (1) for não, responda apenas que não há nada a registrar e encerre. A maioria das \
sessões é assim, e forçar registro é como o acervo apodrece.";

/// A conversa tem uma edição de arquivo que deu certo depois de `desde` (segundos Unix): as
/// ferramentas de escrita do Claude Code e as mudanças de arquivo do Codex, que chegam com os
/// mesmos nomes.
fn editou_arquivo_desde(itens: &[Item], desde: u64) -> bool {
    itens.iter().any(|item| {
        matches!(item, Item::Ferramenta { nome, erro: false, em, .. }
            if *em > desde && matches!(nome.as_str(), "Edit" | "Write" | "MultiEdit" | "NotebookEdit"))
    })
}

/// Um passo da lista do agente: o texto, o que ele diz enquanto faz e em que pé está.
#[derive(Clone, Debug, PartialEq)]
pub struct Passo {
    /// O id do passo nas ferramentas de tarefa (`TaskCreate` e `TaskUpdate`); vazio no `TodoWrite`,
    /// que manda a lista inteira.
    pub id: String,
    pub texto: String,
    pub fazendo: String,
    pub estado: EstadoDoPasso,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstadoDoPasso {
    Falta,
    Fazendo,
    Feito,
}

/// Até quanto tempo atrás uma edição de outro chat no mesmo arquivo gera o aviso (12 h).
const JANELA_DE_EDICAO_CRUZADA: u64 = 12 * 3600;

/// O caminho de um arquivo editado: o absoluto como veio, o relativo a partir da pasta do chat.
fn caminho_do_arquivo(cwd: &Path, arquivo: &str) -> PathBuf {
    let caminho = Path::new(arquivo);
    if caminho.is_absolute() { caminho.to_path_buf() } else { cwd.join(caminho) }
}

/// Os outros chats (fora o `este` e os arquivados) com uma edição em `caminho` desde `desde`: o id
/// e o título de cada um.
fn chats_que_editaram(chats: &[Chat], este: &str, caminho: &Path, desde: u64) -> Vec<(String, String)> {
    chats
        .iter()
        .filter(|c| c.id != este && !c.arquivado && c.atualizado_em >= desde)
        .filter(|c| {
            c.itens.iter().rev().take_while(|i| i.em() == 0 || i.em() >= desde).any(|i| {
                matches!(i, Item::Ferramenta { arquivo: Some(a), mudanca: Some(_), erro: false, em, .. }
                    if *em >= desde && caminho_do_arquivo(&c.cwd, a) == caminho)
            })
        })
        .map(|c| (c.id.clone(), c.titulo.clone()))
        .collect()
}

/// A partir de quanto tempo sem evento o turno andando ganha o aviso "sem sinal" na linha de
/// estado (o MCP_TOOL_TIMEOUT é de 24 h, por causa dos pedidos de permissão: nada mais corta).
const SILENCIO_SUSPEITO: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Quanto o processo que acabou o turno fica com a entrada aberta à espera da sugestão da próxima
/// mensagem. Ela costuma vir em 2 a 5 s (é uma chamada curta, com o cache da conversa).
const ESPERA_DA_SUGESTAO: std::time::Duration = std::time::Duration::from_secs(20);

/// O erro do turno é de login perdido no Claude Code (os textos são os que o 2.1.322 devolveu em
/// 16/11/2026, mais o pedido de `/login` das versões anteriores).
pub fn falha_de_login(erro: &str) -> bool {
    let erro = erro.to_lowercase();
    ["failed to authenticate", "oauth token revoked", "oauth session expired", "please run /login", "invalid api key"]
        .iter()
        .any(|trecho| erro.contains(trecho))
}

/// "Claude", "Codex" ou "Gemini", para títulos e rótulos curtos.
pub fn nome_curto_do_agente(agente: Agente) -> &'static str {
    agente.nome()
}

/// A linha sem as marcas de markdown do começo: título ("## "), citação ("> "), tarefa
/// ("- [ ] "), item de lista ("- ", "* ", "+ ", "1. ", "1) "), repetidas enquanto casarem.
fn sem_marca_de_markdown(linha: &str) -> &str {
    let mut resto = linha.trim();
    loop {
        let antes = resto;
        let titulo = resto.trim_start_matches('#');
        if titulo.len() < resto.len() && resto.len() - titulo.len() <= 6 && titulo.starts_with(' ') {
            resto = titulo;
        }
        for marca in ["> ", "- [ ] ", "- [x] ", "- [X] ", "- ", "* ", "+ "] {
            if let Some(r) = resto.strip_prefix(marca) {
                resto = r;
            }
        }
        let digitos = resto.chars().take_while(char::is_ascii_digit).count();
        if digitos > 0 && (resto[digitos..].starts_with(". ") || resto[digitos..].starts_with(") ")) {
            resto = &resto[digitos + 2..];
        }
        resto = resto.trim_start();
        // Só a marca, sem nada depois (o espaço dela saiu no `trim`).
        if ["-", "*", "+", ">", "- [ ]", "- [x]", "- [X]"].contains(&resto) || (!resto.is_empty() && resto.len() <= 6 && resto.chars().all(|c| c == '#')) {
            return "";
        }
        if resto == antes {
            return resto;
        }
    }
}

/// O título do chat pela primeira linha da mensagem, sem as marcas de markdown do começo (o
/// título saía "- ponto 1": teste de usabilidade, 16/11/2026); se a linha ficar vazia, a seguinte.
fn titulo_de(texto: &str) -> String {
    let linha = texto
        .lines()
        .map(sem_marca_de_markdown)
        .find(|l| !l.is_empty())
        .unwrap_or("Conversa");
    let palavras: Vec<&str> = linha.split_whitespace().take(7).collect();
    let mut titulo = palavras.join(" ");
    if titulo.chars().count() > 48 {
        titulo = format!("{}…", titulo.chars().take(47).collect::<String>());
    }
    titulo
}

/// A primeira fala do usuário que não é um comando de barra: é dela que o título do chat sai.
fn primeira_fala(chat: &Chat) -> Option<&str> {
    chat.itens.iter().find_map(|item| match item {
        Item::Usuario { texto, .. } if !texto.trim().is_empty() && !texto.trim_start().starts_with('/') => Some(texto.as_str()),
        _ => None,
    })
}

/// O título ainda é o que o Vitral deu sozinho: as primeiras palavras da primeira fala (`titulo_de`)
/// ou o "Nova conversa" de quem começou por um comando. O renomeado à mão, o do ramo ("… (ramo)") e
/// o da raia ("Codex · …") não são, e o que já foi trocado pelo gerado também não: é assim que a
/// troca acontece uma vez só, sem marca nenhuma gravada no chat.
fn titulo_automatico(chat: &Chat) -> bool {
    primeira_fala(chat).is_some_and(|fala| chat.titulo == "Nova conversa" || chat.titulo == titulo_de(fala))
}

/// Com menos letras que isto na descrição o Claude Code não titula (medido no 2.1.328: responde
/// `{"title": null}` sem chamar o modelo).
const MENOR_DESCRICAO_DO_TITULO: usize = 10;

/// Até quantas letras da primeira fala vão no pedido do título.
const MAX_DESCRICAO_DO_TITULO: usize = 2000;

/// O que vai no pedido do título gerado (`Processo::pedir_titulo`): a primeira fala, num chat do
/// Claude cujo título ainda é o automático. `None` quando não há o que pedir.
fn descricao_para_o_titulo(chat: &Chat) -> Option<String> {
    if chat.agente != Agente::Claude || !titulo_automatico(chat) {
        return None;
    }
    let fala = primeira_fala(chat)?.trim();
    (fala.chars().count() >= MENOR_DESCRICAO_DO_TITULO).then(|| fala.chars().take(MAX_DESCRICAO_DO_TITULO).collect())
}

/// O título que o Claude Code gerou para a sessão (a linha `ai-title`), se é hora de pô-lo no
/// chat: só no lugar do título automático, nunca num chat do Codex nem numa sessão que o usuário
/// renomeou no Claude Code (`/rename`, a linha `custom-title`).
fn titulo_gerado_para(chat: &Chat, titulos: &crate::importar::TitulosDaSessao) -> Option<String> {
    if chat.agente != Agente::Claude || titulos.proprio.is_some() || !titulo_automatico(chat) {
        return None;
    }
    titulos.gerado.as_deref().map(str::trim).filter(|t| !t.is_empty() && *t != chat.titulo).map(str::to_string)
}

/// Até quantas letras do que o agente disse vão num aviso (o mesmo teto do Paseo).
const MAX_DA_PREVIA: usize = 220;

/// O que o agente disse, para caber num aviso do sistema e numa linha do "Enquanto você esteve
/// fora": sem as marcas de markdown (as do começo da linha, as cercas de código, as réguas, os
/// links, a ênfase e as crases), numa linha só e com até `limite` letras, cortado numa palavra. O
/// título ganha dois pontos antes do que vem depois dele, e os itens de lista ficam separados por
/// " · ". Antes ia a primeira linha crua, e o aviso saía "Terminou: ## Resumo".
fn previa_da_resposta(resposta: &str, limite: usize) -> String {
    let mut previa = String::new();
    // O que a última linha aproveitada era.
    let (mut depois_de_titulo, mut depois_de_item) = (false, false);
    for linha in resposta.lines() {
        let linha = linha.trim();
        if linha.starts_with("```") || linha.starts_with("~~~") || regua(linha) {
            continue;
        }
        let limpa = sem_marcas_em_linha(sem_marca_de_markdown(linha));
        let limpa = limpa.split_whitespace().collect::<Vec<_>>().join(" ");
        if limpa.is_empty() {
            continue;
        }
        let cerquilhas = linha.len() - linha.trim_start_matches('#').len();
        let titulo = (1..=6).contains(&cerquilhas) && linha[cerquilhas..].starts_with(' ');
        let item = item_de_lista(linha);
        if !previa.is_empty() {
            let pontuada = previa.ends_with(['.', '!', '?', ':', ';', ',', '…']);
            previa.push_str(match (pontuada, depois_de_titulo, item || depois_de_item) {
                (false, true, _) => ": ",
                (false, false, true) => " · ",
                _ => " ",
            });
        }
        previa.push_str(&limpa);
        (depois_de_titulo, depois_de_item) = (titulo, item);
        if previa.chars().count() > limite {
            break;
        }
    }
    cortar_em_palavra(&previa, limite)
}

/// A régua ("---", "***", "___") e a linha que separa o cabeçalho de uma tabela ("|---|:--|").
fn regua(linha: &str) -> bool {
    let so_de = |marcas: &[char]| linha.chars().all(|c| c.is_whitespace() || marcas.contains(&c));
    linha.chars().filter(|c| !c.is_whitespace()).count() >= 3
        && (so_de(&['-']) || so_de(&['*']) || so_de(&['_']) || (linha.contains('-') && so_de(&['|', '-', ':'])))
}

/// A linha é um item de lista ("- ", "* ", "+ ", "1. ", "1) "), dentro ou fora de uma citação.
fn item_de_lista(linha: &str) -> bool {
    let resto = linha.trim_start_matches(['>', ' ']);
    let digitos = resto.chars().take_while(char::is_ascii_digit).count();
    ["- ", "* ", "+ "].iter().any(|marca| resto.starts_with(marca))
        || (digitos > 0 && (resto[digitos..].starts_with(". ") || resto[digitos..].starts_with(") ")))
}

/// A linha sem as marcas de markdown do meio dela: `[texto](url)` e `![texto](url)` viram o texto;
/// somem as crases do código em linha (o que está dentro fica como está), o `**`, o `__`, o `*`,
/// o `_` e o `~~` da ênfase que abre e fecha (o sublinhado de `nome_assim` e o asterisco de
/// "2 * 3" ficam) e os `<` `>` de um endereço.
fn sem_marcas_em_linha(linha: &str) -> String {
    let letras: Vec<char> = linha.chars().collect();
    let mut saida = String::with_capacity(linha.len());
    limpar_marcas(&letras, &mut saida);
    saida
}

fn limpar_marcas(l: &[char], saida: &mut String) {
    let repetidas = |i: usize| l[i..].iter().take_while(|c| **c == l[i]).count();
    // Onde começa a sequência de `n` letras `c` que fecha a marca aberta antes de `de`. A da
    // ênfase fecha colada na palavra de antes e sem letra logo depois.
    let fecho = |de: usize, c: char, n: usize, enfase: bool| {
        let mut i = de;
        while i < l.len() {
            if l[i] != c {
                i += 1;
                continue;
            }
            let m = repetidas(i);
            if m == n && i > de && (!enfase || (!l[i - 1].is_whitespace() && l.get(i + m).is_none_or(|d| !d.is_alphanumeric()))) {
                return Some(i);
            }
            i += m;
        }
        None
    };
    // "[texto](url)" aberto em `abre`: onde estão o "](" e o ")" que fecha.
    let link = |abre: usize| {
        let meio = (abre + 1..l.len().saturating_sub(1)).find(|&j| l[j] == ']' && l[j + 1] == '(')?;
        let fim = (meio + 2..l.len()).find(|&j| l[j] == ')')?;
        Some((meio, fim))
    };
    let mut i = 0;
    while i < l.len() {
        let c = l[i];
        if c == '`' {
            let n = repetidas(i);
            if let Some(f) = fecho(i + n, '`', n, false) {
                saida.extend(&l[i + n..f]);
                i = f + n;
            } else {
                i += n;
            }
            continue;
        }
        let imagem = c == '!' && l.get(i + 1) == Some(&'[');
        if (c == '[' || imagem)
            && let Some((meio, fim)) = link(i + usize::from(imagem))
        {
            limpar_marcas(&l[i + 1 + usize::from(imagem)..meio], saida);
            i = fim + 1;
            continue;
        }
        if c == '<'
            && let Some(f) = l[i..].iter().position(|x| *x == '>').map(|p| i + p)
            && !l[i + 1..f].iter().any(|x| x.is_whitespace())
            && l[i + 1..f].windows(3).any(|w| w == [':', '/', '/'])
        {
            saida.extend(&l[i + 1..f]);
            i = f + 1;
            continue;
        }
        if matches!(c, '*' | '_' | '~') {
            let n = repetidas(i);
            let abre = n <= 3
                && (c != '~' || n == 2)
                && l.get(i + n).is_some_and(|d| !d.is_whitespace())
                && (i == 0 || !l[i - 1].is_alphanumeric());
            if abre && let Some(f) = fecho(i + n, c, n, true) {
                limpar_marcas(&l[i + n..f], saida);
                i = f + n;
            } else {
                saida.extend(&l[i..i + n]);
                i += n;
            }
            continue;
        }
        saida.push(c);
        i += 1;
    }
}

/// O texto com até `limite` letras: o que passa é cortado no fim de uma palavra, com reticências.
/// Uma palavra sozinha maior que o limite é cortada nele.
fn cortar_em_palavra(texto: &str, limite: usize) -> String {
    if texto.chars().count() <= limite {
        return texto.to_string();
    }
    let cabe: String = texto.chars().take(limite.saturating_sub(1)).collect();
    let no_fim_da_palavra = texto.chars().nth(limite.saturating_sub(1)).is_some_and(char::is_whitespace);
    let corte = match cabe.rfind(char::is_whitespace) {
        Some(i) if !no_fim_da_palavra && i >= cabe.len() / 2 => &cabe[..i],
        _ => cabe.as_str(),
    };
    format!("{}…", corte.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '·' | '-')))
}

#[cfg(test)]
mod testes_do_titulo_gerado_e_do_aviso {
    use super::*;
    use crate::importar::TitulosDaSessao;

    const PRIMEIRA: &str = "olha esse erro aqui que deu no teste: a função soma devolve 1 para soma(3, 2)";

    fn fala(texto: &str) -> Item {
        Item::Usuario { texto: texto.into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0, marca: None }
    }

    /// Um chat como o `enviar_do_usuario` o deixa depois da primeira mensagem.
    fn chat_com(primeira: &str) -> Chat {
        let mut c = Chat::novo("c".into(), "Nova conversa", PathBuf::from("/tmp"));
        c.titulo = titulo_de(primeira);
        c.itens.push(fala(primeira));
        c
    }

    fn gerado(titulo: &str) -> TitulosDaSessao {
        TitulosDaSessao { proprio: None, gerado: Some(titulo.into()) }
    }

    #[test]
    fn o_titulo_das_primeiras_palavras_e_o_automatico() {
        let c = chat_com(PRIMEIRA);
        assert_eq!(c.titulo, "olha esse erro aqui que deu no");
        assert!(titulo_automatico(&c));
        // Renomeado à mão (F2): não é mais.
        let renomeado = Chat { titulo: "Bug da soma".into(), ..c.clone() };
        assert!(!titulo_automatico(&renomeado));
        // Sem nenhuma fala sua não há de onde o título automático teria saído.
        let vazio = Chat::novo("v".into(), "Nova conversa", PathBuf::from("/tmp"));
        assert!(!titulo_automatico(&vazio));
        // A conversa que começou por um comando ficou "Nova conversa": a primeira fala de verdade
        // é a que conta.
        let mut por_comando = Chat::novo("p".into(), "Nova conversa", PathBuf::from("/tmp"));
        por_comando.itens.extend([fala("/compact"), fala(PRIMEIRA)]);
        assert!(titulo_automatico(&por_comando));
        // O ramo de outro chat ("… (ramo)") e a raia ("Codex · …") têm nome próprio.
        let ramo = Chat { titulo: format!("{} (ramo)", c.titulo), ..c.clone() };
        assert!(!titulo_automatico(&ramo));
    }

    #[test]
    fn o_titulo_gerado_entra_uma_vez_e_so_no_lugar_do_automatico() {
        let mut c = chat_com(PRIMEIRA);
        assert_eq!(titulo_gerado_para(&c, &gerado("Erro na função soma")).as_deref(), Some("Erro na função soma"));
        // Trocado: daí em diante o título é o gerado, e outro `ai-title` não o troca de novo.
        c.titulo = "Erro na função soma".into();
        assert_eq!(titulo_gerado_para(&c, &gerado("Soma que subtrai")), None);
        // Renomeado à mão (F2) fica como está.
        let renomeado = Chat { titulo: "Bug da soma".into(), ..chat_com(PRIMEIRA) };
        assert_eq!(titulo_gerado_para(&renomeado, &gerado("Erro na função soma")), None);
        // Renomeado no Claude Code (`/rename` grava o `custom-title`): também.
        let com_proprio = TitulosDaSessao { proprio: Some("Soma".into()), gerado: Some("Erro na função soma".into()) };
        assert_eq!(titulo_gerado_para(&chat_com(PRIMEIRA), &com_proprio), None);
        // O chat do Codex não tem `ai-title`: nunca.
        let codex = Chat { agente: Agente::Codex, ..chat_com(PRIMEIRA) };
        assert_eq!(titulo_gerado_para(&codex, &gerado("Erro na função soma")), None);
        // Sem `ai-title` na sessão, nada; vazio também não.
        assert_eq!(titulo_gerado_para(&chat_com(PRIMEIRA), &TitulosDaSessao::default()), None);
        assert_eq!(titulo_gerado_para(&chat_com(PRIMEIRA), &gerado("  ")), None);
    }

    #[test]
    fn a_descricao_do_pedido_do_titulo_e_a_primeira_fala() {
        assert_eq!(descricao_para_o_titulo(&chat_com(PRIMEIRA)).as_deref(), Some(PRIMEIRA));
        // O Claude Code não titula com menos de 10 letras (medido no 2.1.328): nem pede.
        assert_eq!(descricao_para_o_titulo(&chat_com("oi, tudo?")), None);
        // Comando de barra não é assunto; vale a primeira fala de verdade.
        let mut c = Chat::novo("p".into(), "Nova conversa", PathBuf::from("/tmp"));
        c.itens.push(fala("/compact"));
        assert_eq!(descricao_para_o_titulo(&c), None);
        c.itens.push(fala(PRIMEIRA));
        assert_eq!(descricao_para_o_titulo(&c).as_deref(), Some(PRIMEIRA));
        // Título que não é o automático não pede nada.
        let renomeado = Chat { titulo: "Bug da soma".into(), ..chat_com(PRIMEIRA) };
        assert_eq!(descricao_para_o_titulo(&renomeado), None);
        let codex = Chat { agente: Agente::Codex, ..chat_com(PRIMEIRA) };
        assert_eq!(descricao_para_o_titulo(&codex), None);
        // A mensagem enorme vai cortada (num limite de letra, não no meio de uma).
        let longa = format!("{PRIMEIRA} {}", "ação ".repeat(2000));
        let c = chat_com(&longa);
        let descricao = descricao_para_o_titulo(&c).unwrap();
        assert!(descricao.chars().count() <= 2000, "{}", descricao.chars().count());
        assert!(longa.starts_with(&descricao));
    }

    #[test]
    fn o_aviso_do_fim_vai_sem_markdown_e_cortado_em_palavra() {
        // O título de markdown não é mais o aviso inteiro ("Terminou: ## Resumo").
        assert_eq!(texto_do_fim("## Resumo\n\nMudei o `motor.rs` e **dois** testes."), "Terminou: Resumo: Mudei o motor.rs e dois testes.");
        // Link, imagem, itálico, riscado e a cerca de código saem; o que estava dentro fica.
        assert_eq!(
            texto_do_fim("Veja [o relatório](docs/x.md), a ![tela](print.png) e _isto_ ~~não~~.\n```sh\ncargo test\n```"),
            "Terminou: Veja o relatório, a tela e isto não. cargo test"
        );
        // Os itens de lista ficam separados; a régua e a linha de separação da tabela somem.
        assert_eq!(
            texto_do_fim("Pronto.\n\n- [x] Corrigi o `soma`\n- Rodei os testes\n\n---\n\nFalta o deploy."),
            "Terminou: Pronto. Corrigi o soma · Rodei os testes · Falta o deploy."
        );
        assert_eq!(texto_do_fim("> **Nota:** 1. primeiro\n2) segundo"), "Terminou: Nota: 1. primeiro · segundo");
        // O sublinhado de um nome e o asterisco de uma conta não são ênfase.
        assert_eq!(texto_do_fim("O `snake_case_aqui` e 2 * 3 * 4 e a_b_c ficam."), "Terminou: O snake_case_aqui e 2 * 3 * 4 e a_b_c ficam.");
        // Até 220 letras, cortado numa palavra, com reticências.
        let longa = "palavra ".repeat(60);
        let aviso = texto_do_fim(&longa);
        let corpo = aviso.strip_prefix("Terminou: ").unwrap();
        assert!(corpo.chars().count() <= 220, "{}", corpo.chars().count());
        assert!(corpo.chars().count() > 200, "{}", corpo.chars().count());
        assert!(corpo.ends_with("palavra…"), "{corpo}");
        // Uma palavra só, maior que o limite, é cortada no limite.
        let colada = "x".repeat(400);
        assert_eq!(texto_do_fim(&colada).chars().count(), "Terminou: ".chars().count() + 220);
        // Acento não quebra o corte.
        let acentos = "ação ".repeat(100);
        assert!(texto_do_fim(&acentos).ends_with("ação…"));
        // Sem texto (ou só marcas), só o "Terminou"; o título sozinho não ganha os dois pontos.
        assert_eq!(texto_do_fim("  \n"), "Terminou");
        assert_eq!(texto_do_fim("```\n```\n---"), "Terminou");
        assert_eq!(texto_do_fim("## Resumo"), "Terminou: Resumo");
        assert_eq!(texto_do_fim("## Feito!\nTudo certo."), "Terminou: Feito! Tudo certo.");
    }

    #[test]
    fn a_linha_do_esteve_fora_e_a_mesma_previa() {
        assert_eq!(linha_do_fim("Loja", "## Resumo\n\nMudei o `motor.rs`.", false), "Loja: Resumo: Mudei o motor.rs.");
        assert_eq!(linha_do_fim("Loja", "", false), "Loja: Terminou");
        assert_eq!(linha_do_fim("Loja", "", true), "Loja: Parou com erro");
        assert_eq!(linha_do_fim("Loja", "API Error: 529\nresto", true), "Loja: API Error: 529 resto");
    }
}

/// Ao abrir: o turno que o Vitral fechado (ou uma queda de energia) cortou fica parado até
/// alguém mandar retomar, com um aviso só.
fn marcar_turnos_cortados(chats: &mut [Chat]) {
    let cortado = "O Vitral foi fechado no meio deste turno. Use Retomar para o Claude continuar de \
                   onde parou.";
    for chat in chats.iter_mut() {
        let ja_avisado = matches!(chat.itens.last(), Some(Item::Aviso { texto, .. }) if texto == cortado);
        if chat.turno_aberto && !ja_avisado {
            chat.itens.push(Item::Aviso {
                texto: cortado.into(),
                erro: false,
                em: agora(),
            });
        }
    }
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

/// O chat nunca rodou: sem sessão e só com os avisos do Vitral (é o que "Arquivar" apaga em vez
/// de guardar). A conversa trazida do Claude Code já rodou lá.
pub fn nunca_rodou(c: &Chat) -> bool {
    c.importada.is_none()
        && !c.sessao_criada
        && c.itens.iter().all(|i| matches!(i, Item::Aviso { .. }))
}

/// O que o arquivamento automático olha no próprio chat: não arquivado, não fixado e sem uso desde
/// `limite`.
fn candidata_a_arquivar(c: &Chat, limite: u64) -> bool {
    !c.arquivado && !c.fixado && c.atualizado_em.max(c.lido_em) < limite
}

/// O chat já recebeu alguma mensagem (do usuário ou entregue pelo Vitral).
fn recebeu_mensagem(chat: &Chat) -> bool {
    chat.itens
        .iter()
        .any(|i| matches!(i, Item::Usuario { .. } | Item::Entrega { .. }))
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

/// As mensagens do turno como itens da conversa, menos as que já estão nela (a repetição de um
/// turno).
fn itens_das_entradas(mensagens: Vec<Entrada>, marca: Option<&str>) -> Vec<Item> {
    let mut marca = marca.map(str::to_string);
    mensagens
        .into_iter()
        .filter(|e| !e.registrada)
        .map(|entrada| {
            match entrada.de {
                Some(de) => Item::Entrega {
                    de,
                    texto: entrada.texto,
                    em: agora(),
                },
                None => Item::Usuario {
                    texto: entrada.texto,
                    anexos: entrada.anexos,
                    ultrathink: entrada.ultrathink,
                    ultracode: entrada.ultracode,
                    em: agora(),
                    // A primeira mensagem do turno leva a marca: voltar a ela desfaz o turno todo.
                    marca: marca.take(),
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

/// O turno em andamento tem uma chamada sem resultado: a linha dela mostra um relógio, que só
/// anda se a conversa for redesenhada (as mensagens são uma vista em cache).
pub fn tem_chamada_rodando(itens: &[Item]) -> bool {
    itens
        .iter()
        .rev()
        .take_while(|i| !matches!(i, Item::Usuario { .. } | Item::Entrega { .. } | Item::Vez { .. }))
        .any(|i| matches!(i, Item::Ferramenta { resultado: None, .. }))
}

/// Quanto uma chamada levou, em segundos: do pedido (`em`) ao resultado (`agora`). A que esperou
/// uma permissão conta de quando ela foi dada (`liberada`): o tempo de você responder não é dela.
/// A que foi para o segundo plano (`no_fundo`) não terminou, e a sem hora não tem de onde contar:
/// as duas ficam em 0, que é "não medido".
fn duracao_da_chamada(em: u64, liberada: Option<u64>, agora: u64, no_fundo: bool) -> u64 {
    if no_fundo || em == 0 {
        return 0;
    }
    agora.saturating_sub(liberada.map_or(em, |quando| quando.max(em)))
}

/// O resultado de uma chamada chegou: entra na chamada com esse `id`, a mais nova primeiro, com
/// quanto ela levou. Cada chamada tem o próprio começo e o próprio resultado, então as que rodam
/// em paralelo não se misturam (o Claude Code manda o resultado de cada uma quando ela termina,
/// fora de ordem; medido na 2.1.328).
fn anotar_resultado(itens: &mut [Item], id: &str, conteudo: String, erro: bool, no_fundo: bool, liberada: Option<u64>, agora: u64) {
    for item in itens.iter_mut().rev() {
        if let Item::Ferramenta {
            id: id_item,
            nome,
            resultado,
            erro: erro_item,
            sub,
            mudanca,
            detalhe,
            em,
            durou,
            ..
        } = item
            && id_item == id
        {
            // O plano aprovado que não veio na chamada nem no pedido (um Claude Code que não
            // mande o id da chamada no pedido): o resultado o traz.
            if nome == "ExitPlanMode" && detalhe.is_none() && !erro {
                *detalhe = claude::plano_do_resultado(&conteudo).and_then(claude::plano_guardado);
            }
            // O Write que criou o arquivo ganha o selo "novo" (item 5.1).
            if nome == "Write" && !erro && claude::write_criou(&conteudo)
                && let Some(m) = mudanca
            {
                m.novo = true;
            }
            // O relatório do subagente sem a moldura que o Claude Code põe nele.
            let conteudo = if matches!(nome.as_str(), "Agent" | "Task") {
                claude::relatorio_do_subagente(&conteudo)
            } else {
                conteudo
            };
            *resultado = Some(git::inicio_do_texto(&conteudo, 20_000));
            *erro_item = erro;
            *durou = duracao_da_chamada(*em, liberada, agora, no_fundo);
            // O subagente terminou: o cartão dele dobra numa linha com a duração.
            if let Some(sub) = sub {
                sub.fim = agora;
            }
            break;
        }
    }
}

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

/// Até quantos caracteres das últimas mensagens vão na passagem de sessão.
const PASSAGEM_CARACTERES: usize = 6000;

/// O que a mensagem retirada da conversa levava, para a caixa recebê-lo de volta.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Retirada {
    pub texto: String,
    pub anexos: Vec<PathBuf>,
    pub reforco: Reforco,
    /// Quantos blocos saíram junto (a resposta, as ferramentas e os avisos do turno).
    pub saiu_depois: usize,
}

/// Onde está a última mensagem do usuário que ainda dá para corrigir: depois dela só pode haver
/// o que aquele turno produziu (resposta, ferramentas e avisos). Com uma entrega de outro chat
/// ou um cartão no meio, tirar a mensagem levaria o que eles trouxeram: aí não dá.
pub fn indice_editavel(itens: &[Item]) -> Option<usize> {
    let ix = itens.iter().rposition(|i| matches!(i, Item::Usuario { .. }))?;
    editavel(itens, ix).then_some(ix)
}

/// A partir de que posição as mensagens do usuário podem ser corrigidas: depois do último item que
/// não dá para tirar da conversa (o que outro chat entregou, um cartão, um item de versão mais
/// nova). O que o próprio Vitral entregou (o pedido de revisar a memória) sai junto sem perda.
pub fn editaveis_desde(itens: &[Item]) -> usize {
    itens
        .iter()
        .rposition(|i| match i {
            Item::Entrega { de, .. } => de != "Vitral",
            Item::Cartao { .. } | Item::Desconhecido(_) => true,
            _ => false,
        })
        .map_or(0, |i| i + 1)
}

/// A mensagem do usuário na posição `ix` pode ser corrigida: corrigir tira ela e tudo o que veio
/// depois (dono, 16/11/2026: "por que esse tem o de editar e o que tem a imagem não?").
pub fn editavel(itens: &[Item], ix: usize) -> bool {
    ix >= editaveis_desde(itens) && matches!(itens.get(ix), Some(Item::Usuario { .. }))
}

/// O começo do primeiro turno de uma sessão nova que tomou o lugar de outra: as últimas 10
/// mensagens (até 6000 caracteres, as mais recentes ficam). As `pular` últimas mensagens já vão no
/// próprio turno.
/// O que o outro agente responde quando reviu o turno e não tem o que mudar
/// (`motor/revisao_do_par.rs`).
const NADA_A_ACRESCENTAR: &str = "Nada a acrescentar";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AlvoDaDupla {
    So(Agente),
    OsDois,
}

/// A quem vai a mensagem num chat com os dois: "@claude" ou "@codex" no começo manda só para um.
/// Põe o `chamado` no chat, ao lado de quem já estava: ele vira o da vez, numa sessão nova, e o de
/// antes guarda a dele no parceiro. A marca de vez do de antes fecha o que ele já fez, para cada
/// um receber depois só o que o outro fez (`o_que_o_outro_fez`).
fn iniciar_dupla(chat: &mut Chat, chamado: Agente) {
    let antes = chat.agente;
    if chat.parceiro.is_some() || chamado == antes {
        return;
    }
    chat.itens.push(Item::Vez { agente: antes, em: agora() });
    chat.parceiro = Some(Parceiro::novo(chamado));
    chat.trocar_vez();
}

/// O aviso de quando a dupla nasce sozinha (pela troca de agente ou pelo @).
fn aviso_da_dupla(chamado: Agente, antes: Agente) -> String {
    format!(
        "O {} entra neste chat com a conversa até aqui; o {} continua nele com a sessão dele. Comece a mensagem com @{} \
         ou @{} para escolher quem responde.",
        chamado.nome(),
        antes.nome(),
        chamado.nome().to_lowercase(),
        antes.nome().to_lowercase()
    )
}

fn alvo_da_dupla(texto: &str) -> AlvoDaDupla {
    let comeco = texto.trim_start().to_lowercase();
    Agente::TODOS
        .into_iter()
        .find(|a| comeco.starts_with(&format!("@{}", a.nome().to_lowercase())))
        .map_or(AlvoDaDupla::OsDois, AlvoDaDupla::So)
}

/// O que `este` agente da dupla ainda não viu: o que o usuário mandou e o que o outro fez desde a
/// última vez dele (tudo o que há, na primeira), do mais recente para trás até caber em
/// `PASSAGEM_CARACTERES`. `None` quando não há nada novo.
fn o_que_o_outro_fez(itens: &[Item], este: Agente, outro: Agente) -> Option<String> {
    let ultima = itens.iter().rposition(|i| matches!(i, Item::Vez { agente, .. } if *agente == este));
    // Depois da última vez dele, o que vem até a próxima marca é dele mesmo.
    let mut do_outro = ultima.is_none();
    let mut linhas = Vec::new();
    for item in &itens[ultima.map_or(0, |i| i + 1)..] {
        match item {
            Item::Vez { agente, .. } => do_outro = *agente != este,
            Item::Usuario { texto, .. } => linhas.push(format!("Usuário: {}", texto.trim())),
            Item::Entrega { de, texto, .. } => linhas.push(format!("[de: {de}] {}", texto.trim())),
            Item::Assistente { texto, .. } if do_outro => linhas.push(format!("{}: {}", outro.nome(), texto.trim())),
            Item::Ferramenta { nome, resumo, arquivo, erro, .. } if do_outro => {
                let alvo = arquivo.as_deref().unwrap_or(resumo.as_str());
                let alvo: String = alvo.chars().take(160).collect();
                linhas.push(format!("{} usou {nome}{}: {alvo}", outro.nome(), if *erro { " (deu erro)" } else { "" }));
            }
            _ => {}
        }
    }
    if linhas.is_empty() {
        return None;
    }
    let mut sobra = PASSAGEM_CARACTERES;
    let mut cabem = Vec::new();
    for linha in linhas.into_iter().rev() {
        let n = linha.chars().count();
        if n > sobra {
            if cabem.is_empty() {
                cabem.push(format!("{}…", linha.chars().take(sobra).collect::<String>()));
            }
            break;
        }
        sobra -= n;
        cabem.push(linha);
    }
    cabem.reverse();
    Some(format!(
        "[Vitral] Neste chat trabalham dois agentes de código na mesma pasta, um de cada vez: você ({}) e o {}. \
         O usuário vê os dois. O que aconteceu desde a sua última vez (o que o {} mudou nos arquivos já está na pasta):\n\n{}",
        este.nome(),
        outro.nome(),
        outro.nome(),
        cabem.join("\n")
    ))
}

fn bloco_de_passagem(itens: &[Item], pular: usize, motivo: MotivoPassagem) -> String {
    let mut falas: Vec<String> = itens
        .iter()
        .rev()
        .filter_map(|i| match i {
            Item::Usuario { texto, .. } => Some(format!("Usuário: {}", texto.trim())),
            Item::Entrega { de, texto, .. } => Some(format!("[de: {de}] {}", texto.trim())),
            Item::Assistente { texto, .. } => Some(format!("Você: {}", texto.trim())),
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
    let mut bloco = String::from(match motivo {
        // O reenvio tirou da conversa a última mensagem e a resposta dela: o que o Claude lembrava
        // daquele turno não vale mais, e o que ele mexeu nos arquivos continua lá.
        MotivoPassagem::Reenvio =>
            "[Vitral] Passagem de sessão: o usuário corrigiu a última mensagem dele, a conversa voltou ao \
             ponto antes dela e esta é uma sessão nova — a mensagem que saiu e a sua resposta a ela não \
             contam mais. O que você já tinha mudado nos arquivos continua como estava. As mensagens que \
             ficaram na tela do usuário:",
        // A cópia isolada saiu do Vitral: a sessão criada nela não abre na pasta de origem.
        MotivoPassagem::SaiuDaCopia =>
            "[Vitral] Passagem de sessão: esta conversa trabalhava numa cópia isolada da pasta (git worktree), \
             e agora mexe direto na pasta de origem, onde você está. A sessão anterior ficou presa à cópia, \
             e esta é uma nova. As mudanças que estavam só na cópia não estão aqui. As últimas mensagens, para \
             continuar de onde parou (a conversa inteira continua na tela do usuário):",
        MotivoPassagem::Expirou =>
            "[Vitral] Passagem de sessão: a sessão anterior do Claude Code desta conversa expirou, e esta é \
             uma nova. As últimas mensagens, para continuar de onde parou (a conversa inteira continua na \
             tela do usuário):",
        // As falas abaixo vieram copiadas do chat de origem, que segue o caminho dele.
        MotivoPassagem::Bifurcou =>
            "[Vitral] Passagem de conversa: esta conversa é um ramo de outra. O usuário escolheu um ponto \
             dela e abriu este chat para seguir por outro caminho a partir dali; a conversa de origem \
             continua à parte e você não vê o que acontecer nela. O que já estava mudado nos arquivos \
             continua como está na pasta. As últimas mensagens até o ponto escolhido, para continuar de \
             lá (a conversa inteira até ali está na tela do usuário):",
        // Nas falas abaixo, "Você" foi o outro agente: o que ele fez nos arquivos continua lá.
        MotivoPassagem::TrocouDeAgente =>
            "[Vitral] Passagem de conversa: até aqui quem respondia nesta conversa era outro agente de \
             código, e o usuário passou a conversa para você. O que foi mudado nos arquivos continua como \
             está na pasta. As últimas mensagens, para continuar de onde parou (onde está \"Você\", leia \
             \"o agente anterior\"; a conversa inteira continua na tela do usuário):",
    });
    for fala in cabem {
        bloco.push_str("\n\n");
        bloco.push_str(&fala);
    }
    bloco
}

/// As ferramentas do Claude Code que mudam a pasta do chat no meio do turno: o chat fica na pasta
/// dele (a sessão do Claude Code é gravada pela pasta).
const SEM_TROCAR_DE_PASTA: [&str; 2] = ["EnterWorktree", "ExitWorktree"];

/// Esforço do turno com Ultrathink: `max` continua `max`; o resto sobe para `xhigh`.
pub fn esforco_do_ultrathink(atual: Option<&str>) -> &'static str {
    if atual == Some("max") { "max" } else { "xhigh" }
}

/// Troca o texto de uma mensagem da fila. Texto em branco não troca nada (quem tira da fila é o
/// `tirar_da_fila`), nem um índice que já não existe, que é de uma nota (ela não aparece na lista)
/// ou que não tem mais o texto `antes`, de quando a edição começou.
fn editar_na_lista(fila: &mut [Entrada], indice: usize, antes: &str, texto: &str) -> bool {
    let texto = texto.trim();
    if texto.is_empty() || indice >= fila.len() || fila[indice].nota || fila[indice].texto != antes {
        return false;
    }
    fila[indice].texto = texto.to_string();
    true
}

/// Troca uma mensagem da fila de lugar com a vizinha (`para_baixo`: a de depois). As notas não
/// contam como lugar (não aparecem na lista): a troca é com a próxima mensagem de verdade.
/// `false` quando ela já está na ponta.
fn mover_na_lista(fila: &mut [Entrada], indice: usize, para_baixo: bool) -> bool {
    if indice >= fila.len() || fila[indice].nota {
        return false;
    }
    let vizinha = if para_baixo {
        (indice + 1..fila.len()).find(|i| !fila[*i].nota)
    } else {
        (0..indice).rev().find(|i| !fila[*i].nota)
    };
    let Some(vizinha) = vizinha else {
        return false;
    };
    fila.swap(indice, vizinha);
    true
}

/// Quanto se espera o Claude Code abrir o turno da mensagem dirigida depois do `result` (medido:
/// uns 30 ms; ver `Processo::dirigir`).
const ESPERA_DO_TURNO_DIRIGIDO: std::time::Duration = std::time::Duration::from_secs(8);

/// O aviso de quando o turno acabou antes de receber o que o usuário mandou nele.
fn voltou_para_a_fila(n: usize) -> String {
    if n == 1 {
        "O turno acabou antes de a sua mensagem entrar nele; ela ficou na fila, para o próximo.".to_string()
    } else {
        format!("O turno acabou antes de as suas {n} mensagens entrarem nele; elas ficaram na fila, para o próximo.")
    }
}

/// O que aconteceu com a mensagem que o usuário mandou com o turno rodando.
enum Dirigido {
    /// Entrou no turno que já roda.
    Foi,
    /// Espera na fila; com motivo, ele vira aviso na conversa (a mensagem não cabia no turno).
    NaFila(Option<&'static str>),
}

/// Como a mensagem escrita com um pedido de permissão aberto responde a ele.
#[derive(Debug, PartialEq)]
enum RespostaAoPedido {
    /// Não responde: o que vale para o turno inteiro (comando de barra, Ultrathink, Ultracode)
    /// espera na fila, e o pedido continua aberto.
    Fila,
    /// O texto vai ao modelo como a explicação da recusa (com os arquivos anexados, pelo caminho).
    Explicacao(String),
    /// Tem imagem, que a recusa não leva: os pedidos são negados com `RESPONDEU_ESCREVENDO` e a
    /// mensagem entra no turno pela entrada do processo.
    Mensagem,
}

fn resposta_ao_pedido(entrada: &Entrada) -> RespostaAoPedido {
    if entrada.de.is_some() || entrada.nota || entrada.ultrathink || entrada.ultracode || entrada.texto.starts_with('/') {
        return RespostaAoPedido::Fila;
    }
    match texto_do_turno(std::slice::from_ref(entrada)) {
        (texto, imagens) if imagens.is_empty() => RespostaAoPedido::Explicacao(texto),
        _ => RespostaAoPedido::Mensagem,
    }
}

/// O que o modelo lê na chamada negada quando a resposta vem logo depois, como mensagem (a que
/// tem imagem).
const RESPONDEU_ESCREVENDO: &str = "O usuário respondeu com uma mensagem em vez de permitir esta ação. A mensagem dele vem a seguir.";

/// A recusa de um pedido de permissão com o que o usuário escreveu, como o modelo a lê: é o texto
/// do "Negar e explicar…" do cartão, e o do Enter com o pedido aberto. No plano, é o pedido de
/// mudanças.
pub fn recusa_explicada(ferramenta: &str, texto: &str) -> String {
    let texto = texto.trim();
    if ferramenta == "ExitPlanMode" {
        format!("O usuário pediu mudanças no plano: {texto}")
    } else {
        format!("O usuário negou esta ação e explicou o que fazer: {texto}")
    }
}

/// O que o modelo lê na chamada negada sem explicação: o Negar do cartão e o "Não responder" da
/// pergunta com opções.
/// Tira os pedidos do chat que o agente já fechou (o canal de resposta fechado do outro lado).
/// Diz se tirou algum.
fn tirar_pedidos_fechados(permissoes: &mut Vec<PedidoPermissao>, chat: &str) -> bool {
    let antes = permissoes.len();
    permissoes.retain(|p| p.chat != chat || !p.resposta.is_closed());
    permissoes.len() != antes
}

fn recusa_sem_texto(ferramenta: &str) -> &'static str {
    if ferramenta == "AskUserQuestion" { "O usuário não quis responder à pergunta." } else { "O usuário negou esta ação." }
}

/// A resposta a uma pergunta com opções (AskUserQuestion), no formato que o Claude Code espera
/// do `--permission-prompt-tool` (medido no 2.1.328, 23/11/2026): `allow` com o `updatedInput`
/// igual à entrada da chamada mais as `answers`, um objeto com o texto inteiro de cada pergunta
/// como chave e a resposta como valor (várias escolhas numa resposta só, separadas por vírgula).
/// Sem as `answers` ele diz ao modelo "The user did not answer the questions."; só com elas, sem
/// as `questions`, recusa o `updatedInput` por não bater com o esquema da ferramenta.
pub fn resposta_da_pergunta(entrada: &Value, respostas: &[(String, String)]) -> Value {
    let mut entrada = if entrada.is_object() { entrada.clone() } else { json!({}) };
    entrada["answers"] = Value::Object(respostas.iter().map(|(pergunta, resposta)| (pergunta.clone(), json!(resposta))).collect());
    json!({ "behavior": "allow", "updatedInput": entrada })
}

/// As respostas de uma pergunta com opções quando o usuário escreve na caixa de mensagem em vez
/// de escolher no cartão: o texto vale para cada pergunta, como a "Outra…" dela. Vazio sem
/// pergunta legível ou sem texto (aí vale a recusa comum).
fn respostas_escritas(entrada: &Value, texto: &str) -> Vec<(String, String)> {
    let texto = texto.trim();
    if texto.is_empty() {
        return Vec::new();
    }
    entrada["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p["question"].as_str().filter(|t| !t.trim().is_empty()))
        .map(|pergunta| (pergunta.to_string(), texto.to_string()))
        .collect()
}

/// O que um pedido aberto é para o sino, a lista e o Ctrl+G: o plano, a pergunta com opções ou
/// uma permissão.
fn pendencia_do_pedido(pedido: &str, chat: &str, ferramenta: &str, entrada: &Value) -> Pendencia {
    let (pedido, chat) = (pedido.to_string(), chat.to_string());
    match ferramenta {
        "ExitPlanMode" => Pendencia::Plano { pedido, chat },
        "AskUserQuestion" => Pendencia::Pergunta { pedido, chat, pergunta: primeira_pergunta(entrada) },
        ferramenta => Pendencia::Permissao {
            pedido,
            chat,
            ferramenta: ferramenta.to_string(),
            resumo: claude::resumo_ferramenta(ferramenta, entrada),
        },
    }
}

/// O texto da primeira pergunta de uma AskUserQuestion, numa linha.
fn primeira_pergunta(entrada: &Value) -> String {
    entrada["questions"][0]["question"].as_str().unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// O marco que entra na conversa quando o Claude Code termina de compactar: "Contexto compactado
/// (automático, 168 mil tokens antes)". O gatilho é o `/compact` (manual) ou o contexto cheio
/// (automático); os tokens são os que a conversa ocupava antes do resumo.
pub fn marco_da_compactacao(manual: bool, antes: Option<u64>) -> String {
    let gatilho = if manual { "manual" } else { "automático" };
    let tamanho = match antes {
        None | Some(0) => None,
        Some(n @ ..1_000) => Some(format!("{n} tokens")),
        Some(n @ ..999_500) => Some(format!("{} mil tokens", (n as f64 / 1e3).round() as u64)),
        Some(n) => Some(format!("{} mi de tokens", format!("{:.1}", n as f64 / 1e6).replace('.', ","))),
    };
    match tamanho {
        Some(tamanho) => format!("{CONTEXTO_COMPACTADO} ({gatilho}, {tamanho} antes)"),
        None => format!("{CONTEXTO_COMPACTADO} ({gatilho})"),
    }
}

/// De quanto em quanto tempo (s) o uso do Gemini é perguntado ao `agy`.
const GEMINI_A_CADA: u64 = 300;

/// O começo do texto do marco de compactação: é por ele que a conversa o reconhece.
pub const CONTEXTO_COMPACTADO: &str = "Contexto compactado";

/// Guarda o texto do plano na chamada ExitPlanMode de id `chamada`, que ainda não o tem. Devolve
/// se achou a chamada e guardou (sem ela, quem chama deixa o plano esperando a chamada chegar).
fn guardar_plano(itens: &mut [Item], chamada: &str, plano: &str) -> bool {
    let Some(plano) = claude::plano_guardado(plano) else {
        return false;
    };
    for item in itens.iter_mut().rev() {
        if let Item::Ferramenta { id, nome, detalhe, .. } = item
            && id == chamada
        {
            if nome != "ExitPlanMode" {
                return false;
            }
            if detalhe.is_none() {
                *detalhe = Some(plano);
            }
            return true;
        }
    }
    false
}

/// Como o Parar encerra o turno.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Parada {
    /// O `interrupt` pela entrada: o Claude Code fecha o turno, grava a sessão e sai.
    Interromper,
    /// Antes do `interrupt`, tira da fila do Claude Code as mensagens dirigidas que ele ainda não
    /// começou (senão ele abriria outro turno com elas).
    RetirarEInterromper,
    /// SIGTERM no processo e em tudo o que ele abriu.
    Matar,
}

/// `esperando_agentes`: o turno já deu o `result` e o processo só espera os agentes do fundo (não
/// há turno a interromper). `pode_retirar`: `retira_dirigidas`.
fn como_parar(esperando_agentes: bool, dirigida_esperando: bool, pode_retirar: bool) -> Parada {
    match (esperando_agentes, dirigida_esperando) {
        (true, _) => Parada::Matar,
        (false, false) => Parada::Interromper,
        (false, true) if pode_retirar => Parada::RetirarEInterromper,
        (false, true) => Parada::Matar,
    }
}

/// O Parar pode tirar a mensagem dirigida da fila do agente: só no Claude Code que conhece o
/// pedido (`claude::VERSAO_RETIRAR_DIRIGIDA`). Sem saber a versão, não.
fn retira_dirigidas(agente: Agente, versao: Option<Versao>) -> bool {
    agente == Agente::Claude && versao.is_some_and(|v| v >= claude::VERSAO_RETIRAR_DIRIGIDA)
}

/// No fim do turno, o processo sai inteiro (SIGTERM) em vez de sair sozinho: com agentes ainda em
/// segundo plano, ou com mensagem dirigida que ele não recebeu e que o Parar não tirou da fila
/// dele (`retiradas`).
fn sai_inteiro(fundo: bool, dirigidas_perdidas: usize, retiradas: bool) -> bool {
    fundo || (dirigidas_perdidas > 0 && !retiradas)
}

/// O que vai para o turno que roda: o que a conversa pede agora, menos o esforço num turno com
/// Ultrathink ou Ultracode (`reforco`), que fica com o dele até o fim.
fn quer_ao_vivo(pedido: claude::AoVivo, tem: &claude::AoVivo, reforco: bool) -> claude::AoVivo {
    if reforco { claude::AoVivo { esforco: tem.esforco.clone(), ..pedido } } else { pedido }
}

/// Quanto a troca de modo, modelo ou esforço tem de ficar parada antes de ir para o turno que
/// roda (`Motor::acertar_turno_depois`).
const ASSENTAR_A_TROCA: std::time::Duration = std::time::Duration::from_millis(600);

/// A conta do Claude está conectada: tem o login guardado e não o perdeu (num turno, ou recusado
/// pela Anthropic na busca do uso). É o que o anel de uso do Claude pede para aparecer.
fn conta_conectada(com_login: bool, perdeu_o_login: bool, recusada: bool) -> bool {
    com_login && !perdeu_o_login && !recusada
}

/// O turno não termina neste `result`: os agentes seguem em segundo plano (o Claude Code abre
/// outro turno quando eles acabam) ou uma mensagem dirigida pode ainda estar na fila interna dele,
/// e aí o turno seguinte vem na hora, no mesmo processo (ver `Processo::dirigir`).
fn adiar_o_fim(agentes_em_fundo: usize, dirigida_esperando: bool, erro: bool, interrompido: bool) -> bool {
    (agentes_em_fundo > 0 || dirigida_esperando) && !erro && !interrompido
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

pub fn texto_da_versao((a, b, c): Versao) -> String {
    format!("{a}.{b}.{c}")
}

/// Por que os fluxos de agentes não vieram num turno com Ultracode, se o próprio Vitral os liga
/// no `--settings`: um `false` em outra configuração da conta vence.
pub const FLUXOS_DESLIGADOS: &str = "os fluxos de agentes estão desligados nesta conta (em Dynamic workflows \
no /config, por disableWorkflows ou pela organização)";

/// Por que o Ultracode não vale (`None`: vale). Os fluxos desligados na conta não entram: a
/// pessoa pode ter mudado a configuração, então a opção só avisa.
pub fn motivo_sem_ultracode(modelo: &str, versao: Option<Versao>) -> Option<String> {
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

/// Por que não dá para fixar a versão exata do modelo (`None`: dá). Só a versão do Claude Code
/// pesa: o papel da conversa e o modelo não mudam nada aqui.
pub fn motivo_sem_versao_exata(versao: Option<Versao>) -> Option<String> {
    let versao = versao.filter(|v| *v < claude::VERSAO_MODELOS_EXATOS)?;
    Some(format!(
        "precisa do Claude Code {} ou mais novo (este é o {}); dá para atualizar nas Configurações",
        texto_da_versao(claude::VERSAO_MODELOS_EXATOS),
        texto_da_versao(versao)
    ))
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
        // O Opus 4.7 e o 4.8 também nasceram com 1M (as versões exatas do seletor oferecem os dois).
        Some(m)
            if [
                "claude-opus-5",
                "claude-opus-4-7",
                "claude-opus-4-8",
                "claude-sonnet-5",
                "claude-haiku-5",
                "claude-fable",
            ]
            .iter()
            .any(|p| m.starts_with(p)) =>
        {
            1_000_000
        }
        _ => 200_000,
    }
}

/// Notificação do sistema: o título do chat e uma linha só de corpo, com o ícone do Vitral.
/// Os argumentos do `notify-send`: o clique (a ação padrão) abre a conversa, e a notificação
/// anterior do mesmo chat é substituída em vez de empilhar.
#[cfg_attr(windows, allow(dead_code))]
fn argumentos_do_aviso(titulo: &str, corpo: &str, substitui: Option<u32>, urgente: bool) -> Vec<String> {
    let mut args: Vec<String> = ["-a", "Vitral", "-i", "vitral", "-h", "string:desktop-entry:vitral"]
        .into_iter()
        .chain(["-A", "default=Abrir", "-p"])
        .map(String::from)
        .collect();
    // O pedido de permissão e a queda com erro saem com urgência alta: o GNOME os deixa na tela
    // até o clique, e o "Terminou" continua sumindo sozinho (rodada de uso, item 3.1).
    if urgente {
        args.extend(["-u".to_string(), "critical".to_string()]);
    }
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
        "AskUserQuestion" => match primeira_pergunta(entrada) {
            pergunta if pergunta.is_empty() => "Tem uma pergunta para você".into(),
            pergunta => format!("Tem uma pergunta para você: {pergunta}"),
        },
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
/// Marca o chat como lido em `agora`, se havia algo não lido; devolve se mudou (aí o estado vai
/// para o disco na próxima passada da gravação).
fn ler_ate_agora(c: &mut Chat, agora: u64) -> bool {
    // Marcado como não lido: só a visita de verdade (`visitar_chat`) solta o ponto azul.
    let mudou = c.lido_em < c.atualizado_em && !c.retido_nao_lido;
    if mudou {
        c.lido_em = agora.max(c.atualizado_em);
    }
    mudou
}

/// "Marcar como não lido": devolve o ponto azul (o `lido_em` fica um segundo antes da última
/// novidade) e segura a marca contra o "lido" de cada quadro. Nada a marcar numa conversa vazia
/// ou que já está não lida; devolve se mudou.
fn marcar_como_nao_lido(c: &mut Chat) -> bool {
    if c.atualizado_em == 0 || c.lido_em < c.atualizado_em {
        return false;
    }
    c.lido_em = c.atualizado_em - 1;
    c.retido_nao_lido = true;
    true
}

/// A visita de verdade ao chat (o usuário o abriu ou ativou o painel dele): solta a marca de
/// "não lido". Não lê por conta própria (abrir não marca como lido, só chegar ao fim): a leitura
/// segue o caminho de sempre, o `ler_ate_agora` de quem está com o fim à vista. Devolve se havia
/// marca.
fn visitar_chat(c: &mut Chat) -> bool {
    std::mem::take(&mut c.retido_nao_lido)
}

/// "Parou com erro: <primeira linha>" da notificação de um turno que falhou.
fn texto_do_erro(erro: &str) -> String {
    match erro.lines().map(str::trim).find(|l| !l.is_empty()) {
        Some(linha) => format!("Parou com erro: {linha}"),
        None => "Parou com erro".to_string(),
    }
}

/// O aviso de um turno que terminou: "Terminou: " e o começo do que o agente disse, legível
/// (`previa_da_resposta`).
fn texto_do_fim(resposta: &str) -> String {
    let previa = previa_da_resposta(resposta, MAX_DA_PREVIA);
    if previa.is_empty() { "Terminou".into() } else { format!("Terminou: {previa}") }
}

/// A linha de um turno que acabou no "Enquanto você esteve fora": o título do chat e a mesma
/// prévia do aviso (a tela mostra a primeira frase dela).
fn linha_do_fim(titulo: &str, texto: &str, erro: bool) -> String {
    let previa = previa_da_resposta(texto, MAX_DA_PREVIA);
    match (previa.is_empty(), erro) {
        (false, _) => format!("{titulo}: {previa}"),
        (true, true) => format!("{titulo}: Parou com erro"),
        (true, false) => format!("{titulo}: Terminou"),
    }
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

/// Quanto o motor espera por mais pedaços de texto antes de avisar a tela: os que chegam nesse
/// tempo saem juntos, num desenho só (no máximo uns 16 por segundo).
pub(crate) const JANELA_DO_TEXTO: std::time::Duration = std::time::Duration::from_millis(60);

/// O que um pedaço de texto mudou na tela.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pedaco {
    /// Nada: o chat não está em nenhum painel e já escrevia.
    Nada,
    /// O texto que já aparecia cresceu: só o painel do chat muda.
    Cresceu,
    /// A conversa passou a escrever: a faixa do turno, a lista e o painel mudam.
    Comecou,
}

/// Junta um pedaço ao texto parcial do chat e diz o que ele mudou na tela.
fn pedaco_de_texto(
    parcial: &mut HashMap<String, String>,
    visiveis: &HashSet<String>,
    chat: &str,
    pedaco: &str,
) -> Pedaco {
    // O texto que a tela já mostrava (o `Motor::parcial` não conta o vazio).
    let escrevia = parcial.get(chat).is_some_and(|texto| !texto.is_empty());
    match (juntar_parcial(parcial, visiveis, chat, pedaco), escrevia) {
        (false, _) => Pedaco::Nada,
        (true, true) => Pedaco::Cresceu,
        (true, false) => Pedaco::Comecou,
    }
}

/// O aviso que um lote de sinais dá à tela.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AvisoDoLote {
    Nada,
    /// Só texto crescendo nestes chats (sem repetir): os painéis deles redesenham, o resto não.
    Texto(Vec<String>),
    Tudo,
}

/// `redesenhar`: algum sinal do lote mudou mais que texto; `texto`: os chats cujo texto cresceu.
pub(crate) fn aviso_do_lote(redesenhar: bool, texto: Vec<String>) -> AvisoDoLote {
    if redesenhar {
        return AvisoDoLote::Tudo;
    }
    let mut chats: Vec<String> = Vec::new();
    for chat in texto {
        if !chats.contains(&chat) {
            chats.push(chat);
        }
    }
    if chats.is_empty() { AvisoDoLote::Nada } else { AvisoDoLote::Texto(chats) }
}

/// O texto de uma resposta cresceu nestes chats, e mais nada mudou: a Janela redesenha só os
/// painéis deles (`Janela::texto_chegando`), e não a janela inteira como no `notify`.
#[derive(Clone, Debug)]
pub struct TextoChegando {
    pub chats: Vec<String>,
}

impl gpui_kit::EventEmitter<TextoChegando> for Motor {}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn o_titulo_sai_sem_as_marcas_de_markdown() {
        assert_eq!(titulo_de("- ponto 1\n- ponto 2"), "ponto 1");
        assert_eq!(titulo_de("## Ajustar o frete"), "Ajustar o frete");
        assert_eq!(titulo_de("> - [ ] revisar cupom"), "revisar cupom");
        assert_eq!(titulo_de("1. primeiro passo"), "primeiro passo");
        assert_eq!(titulo_de("2) segundo"), "segundo");
        // Linha só com a marca: vale a seguinte.
        assert_eq!(titulo_de("- \nconferir pedidos"), "conferir pedidos");
        // O que não é marca fica: "#1" sem espaço, número sem ponto.
        assert_eq!(titulo_de("#1 prioridade"), "#1 prioridade");
        assert_eq!(titulo_de("2024 foi bom"), "2024 foi bom");
        assert_eq!(titulo_de("   "), "Conversa");
    }
    use crate::modelo::Chat;

    /// O pedido que o agente fechou sem esperar (o canal do cartão fechado) sai da lista; o do
    /// outro chat e o que ainda espera ficam.
    #[test]
    fn o_pedido_fechado_pelo_agente_sai_da_lista() {
        let pedido = |chat: &str| {
            let (envia, recebe) = smol::channel::bounded::<Resposta>(1);
            let p = PedidoPermissao {
                id: novo_id(),
                chat: chat.into(),
                ferramenta: "Bash".into(),
                entrada: json!({"command": "ls"}),
                motivo_do_hook: None,
                chamada: None,
                resposta: envia,
            };
            (p, recebe)
        };
        let (fechado, recebe_fechado) = pedido("c1");
        let (vivo, _recebe_vivo) = pedido("c1");
        let (do_outro, recebe_do_outro) = pedido("c2");
        recebe_fechado.close();
        recebe_do_outro.close();
        let vivo_id = vivo.id.clone();
        let mut lista = vec![fechado, vivo, do_outro];
        assert!(tirar_pedidos_fechados(&mut lista, "c1"));
        assert_eq!(lista.iter().map(|p| (p.chat.as_str(), p.id == vivo_id)).collect::<Vec<_>>(), [("c1", true), ("c2", false)]);
        assert!(!tirar_pedidos_fechados(&mut lista, "c1"), "nada mais a tirar");
    }

    fn chat(id: &str) -> Chat {
        let mut c = Chat::novo(id.into(), id, PathBuf::from("/tmp"));
        (c.sessao, c.sessao_criada) = ("s".into(), true);
        (c.criado_em, c.atualizado_em, c.lido_em) = (0, 0, 0);
        c
    }

    #[test]
    fn acha_o_outro_chat_que_editou_o_mesmo_arquivo() {
        let edicao = |arquivo: &str, em: u64, erro: bool| Item::Ferramenta {
            id: "t".into(),
            nome: "Edit".into(),
            resumo: String::new(),
            resultado: None,
            erro,
            em,
            detalhe: None,
            arquivo: Some(arquivo.into()),
            sub: None,
            mudanca: Some(crate::modelo::Mudanca::default()),
            imagens: Vec::new(),
            durou: 0,
        };
        let mut a = chat("a");
        let mut b = chat("b");
        b.titulo = "Fila".into();
        b.atualizado_em = 1_000;
        // Relativo à pasta do chat (/tmp) vale o mesmo que o absoluto.
        b.itens.push(edicao("src/fila.rs", 1_000, false));
        let mut c = chat("c");
        c.atualizado_em = 1_000;
        c.itens.push(edicao("/tmp/src/fila.rs", 1_000, true));
        a.atualizado_em = 1_000;
        let chats = vec![a, b, c];
        let caminho = Path::new("/tmp/src/fila.rs");
        assert_eq!(chats_que_editaram(&chats, "a", caminho, 500), vec![("b".to_string(), "Fila".to_string())]);
        // Edição antiga demais, ou o próprio chat: nada.
        assert!(chats_que_editaram(&chats, "a", caminho, 2_000).is_empty());
        assert!(chats_que_editaram(&chats, "b", caminho, 500).is_empty());
    }

    fn avisos(chat: &Chat) -> usize {
        chat.itens.iter().filter(|i| matches!(i, Item::Aviso { .. })).count()
    }

    #[test]
    fn dupla_manda_para_um_ou_para_os_dois() {
        use super::{AlvoDaDupla, alvo_da_dupla};
        assert_eq!(alvo_da_dupla("@codex revisa o teste"), AlvoDaDupla::So(Agente::Codex));
        assert_eq!(alvo_da_dupla("  @Claude, olha isso"), AlvoDaDupla::So(Agente::Claude));
        assert_eq!(alvo_da_dupla("arruma a fila"), AlvoDaDupla::OsDois);
        // Só no começo: citar o outro no meio não muda o destino.
        assert_eq!(alvo_da_dupla("pede para o @codex ver"), AlvoDaDupla::OsDois);
    }

    #[test]
    fn a_dupla_nasce_com_a_sessao_de_cada_um() {
        use super::iniciar_dupla;
        let mut chat = chat("c");
        let sessao_do_claude = chat.sessao.clone();
        iniciar_dupla(&mut chat, Agente::Codex);
        // O Codex fica na vez, numa sessão nova; o Claude guarda a dele, e a marca dele fecha o que
        // ele fez antes.
        assert_eq!(chat.agente, Agente::Codex);
        assert_ne!(chat.sessao, sessao_do_claude);
        let p = chat.parceiro.as_ref().unwrap();
        assert_eq!((p.agente, p.sessao.as_str()), (Agente::Claude, sessao_do_claude.as_str()));
        assert!(matches!(chat.itens.last(), Some(Item::Vez { agente: Agente::Claude, .. })));
        // De novo, nada muda.
        iniciar_dupla(&mut chat, Agente::Claude);
        assert_eq!(chat.agente, Agente::Codex);
    }

    #[test]
    fn dupla_leva_ao_agente_so_o_que_ele_nao_viu() {
        use super::o_que_o_outro_fez;
        let fala = |t: &str| Item::Assistente { texto: t.into(), em: 0 };
        let vez = |agente| Item::Vez { agente, em: 0 };
        let usuario = |t: &str| Item::Usuario { texto: t.into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0, marca: None };
        let editou = Item::Ferramenta {
            id: "1".into(),
            nome: "Edit".into(),
            resumo: "fila.rs".into(),
            resultado: None,
            erro: false,
            em: 0,
            detalhe: None,
            arquivo: Some("src/ui/fila.rs".into()),
            sub: None,
            mudanca: None,
            imagens: Vec::new(),
            durou: 0,
        };
        let itens = vec![
            usuario("antes da dupla"),
            fala("resposta antiga do Claude"),
            usuario("arruma a fila"),
            vez(Agente::Claude),
            fala("Arrumei a fila."),
            vez(Agente::Codex),
            editou.clone(),
            fala("Faltava o teste; acrescentei."),
        ];
        let para_o_claude = o_que_o_outro_fez(&itens, Agente::Claude, Agente::Codex).unwrap();
        // Só o que veio depois da vez dele, e do Codex.
        assert!(para_o_claude.contains("Codex usou Edit: src/ui/fila.rs"), "{para_o_claude}");
        assert!(para_o_claude.contains("Codex: Faltava o teste; acrescentei."));
        assert!(!para_o_claude.contains("Arrumei a fila."));
        assert!(!para_o_claude.contains("antes da dupla"));
        // Na primeira vez dele, o Codex recebe a conversa desde o começo, até a vez dele.
        let para_o_codex = o_que_o_outro_fez(&itens[..5], Agente::Codex, Agente::Claude).unwrap();
        assert!(para_o_codex.contains("antes da dupla") && para_o_codex.contains("Claude: Arrumei a fila."), "{para_o_codex}");
        // Logo depois da própria vez, nada de novo.
        assert_eq!(o_que_o_outro_fez(&itens[..5], Agente::Claude, Agente::Codex), None);
    }

    #[test]
    fn login_perdido_e_reconhecido_pelo_texto_do_claude_code() {
        use super::falha_de_login;
        assert!(falha_de_login("Failed to authenticate: OAuth token revoked. Please log in again or contact your administrator."));
        assert!(falha_de_login("Failed to authenticate: OAuth session expired and could not be refreshed"));
        assert!(falha_de_login("Invalid API key · Please run /login"));
        // Limite do plano e erro comum não são login perdido.
        assert!(!falha_de_login("You've hit your usage limit."));
        assert!(!falha_de_login("API Error: 500 Internal server error"));
    }

    /// A revisão da memória só é pedida quando houve edição de arquivo que deu certo depois do
    /// último pedido: leitura, comando e edição que falhou não contam.
    #[test]
    fn so_edicao_de_arquivo_pede_a_revisao_da_memoria() {
        use super::editou_arquivo_desde;
        let ferramenta = |nome: &str, em: u64, erro: bool| Item::Ferramenta {
            id: "x".into(),
            nome: nome.into(),
            resumo: String::new(),
            resultado: None,
            erro,
            em,
            detalhe: None,
            arquivo: None,
            sub: None,
            mudanca: None,
            imagens: Vec::new(),
            durou: 0,
        };
        assert!(!editou_arquivo_desde(&[ferramenta("Read", 10, false), ferramenta("Bash", 11, false)], 0));
        assert!(!editou_arquivo_desde(&[ferramenta("Edit", 10, true)], 0));
        assert!(editou_arquivo_desde(&[ferramenta("Read", 10, false), ferramenta("Write", 12, false)], 0));
        // Depois do pedido das 12, só uma edição mais nova conta.
        assert!(!editou_arquivo_desde(&[ferramenta("Edit", 12, false)], 12));
        assert!(editou_arquivo_desde(&[ferramenta("Edit", 12, false), ferramenta("NotebookEdit", 13, false)], 12));
    }

    #[test]
    fn marcar_lido_so_muda_o_que_estava_nao_lido() {
        let mut c = chat("c1");
        (c.atualizado_em, c.lido_em) = (200, 100);
        // Não lido: marca e pede a gravação do estado.
        assert!(ler_ate_agora(&mut c, 250));
        assert_eq!(c.lido_em, 250);
        // Já lido: nada muda, e a gravação não é pedida a cada quadro.
        assert!(!ler_ate_agora(&mut c, 300));
        assert_eq!(c.lido_em, 250);
    }

    #[test]
    fn marcar_como_nao_lido_devolve_o_ponto_e_so_a_visita_de_verdade_o_tira() {
        let mut c = chat("c1");
        (c.atualizado_em, c.lido_em) = (200, 250);
        assert!(marcar_como_nao_lido(&mut c));
        assert!(c.lido_em < c.atualizado_em, "o ponto azul voltou");
        // O `ler_ate_agora` roda a cada quadro com o fim à vista: não pode apagar a marca.
        assert!(!ler_ate_agora(&mut c, 300));
        assert!(c.lido_em < c.atualizado_em);
        // A visita de verdade só solta a marca: quem lê é o fim à vista, no quadro seguinte.
        assert!(visitar_chat(&mut c));
        assert!(!c.retido_nao_lido);
        assert!(c.lido_em < c.atualizado_em);
        assert!(ler_ate_agora(&mut c, 400));
        assert_eq!(c.lido_em, 400);
        // Sem a marca, a visita não muda nada.
        assert!(!visitar_chat(&mut c));
    }

    #[test]
    fn marcar_como_nao_lido_nao_faz_nada_numa_conversa_sem_nada_para_ler() {
        let mut c = chat("c1");
        (c.atualizado_em, c.lido_em) = (0, 0);
        assert!(!marcar_como_nao_lido(&mut c));
        assert!(!c.retido_nao_lido);
        // Já não lida: a marca não muda o que já está.
        (c.atualizado_em, c.lido_em) = (200, 100);
        assert!(!marcar_como_nao_lido(&mut c));
    }

    #[test]
    fn a_chamada_guarda_quanto_levou() {
        // Do pedido ao resultado.
        assert_eq!(duracao_da_chamada(1000, None, 1134, false), 134);
        assert_eq!(duracao_da_chamada(1000, None, 1000, false), 0);
        // A que esperou a permissão conta de quando ela foi dada: os 3 min de você responder não
        // são do comando.
        assert_eq!(duracao_da_chamada(1000, Some(1180), 1194, false), 14);
        // A permissão anotada antes do pedido (relógio torto) não aumenta a conta.
        assert_eq!(duracao_da_chamada(1000, Some(900), 1134, false), 134);
        // A que foi para o segundo plano segue rodando: o resultado dela não é o fim do comando.
        assert_eq!(duracao_da_chamada(1000, None, 1120, true), 0);
        // Sem hora (conversa trazida de antes) e com o relógio voltando: não medido.
        assert_eq!(duracao_da_chamada(0, None, 1134, false), 0);
        assert_eq!(duracao_da_chamada(1000, None, 990, false), 0);

        let chamada = |id: &str, nome: &str, em: u64| Item::Ferramenta {
            id: id.into(),
            nome: nome.into(),
            resumo: String::new(),
            resultado: None,
            erro: false,
            em,
            detalhe: None,
            arquivo: None,
            sub: None,
            mudanca: None,
            durou: 0,
            imagens: Vec::new(),
        };
        let lido = |itens: &[Item], id: &str| {
            itens
                .iter()
                .find_map(|i| match i {
                    Item::Ferramenta { id: outro, resultado, durou, erro, .. } if outro == id => Some((resultado.clone(), *durou, *erro)),
                    _ => None,
                })
                .unwrap()
        };
        // Três comandos pedidos juntos, como o Claude Code 2.1.328 os mostrou: o do segundo plano
        // volta na hora, o rápido em 1 s e o lento só no fim. Cada um fica com o próprio tempo.
        let mut itens = vec![
            Item::Usuario { texto: "roda".into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 1000, marca: None },
            chamada("lento", "Bash", 1000),
            chamada("rapido", "Bash", 1000),
            chamada("fundo", "Bash", 1000),
        ];
        anotar_resultado(&mut itens, "fundo", "Command running in background with ID: b1.".into(), false, true, None, 1000);
        anotar_resultado(&mut itens, "rapido", "rapido".into(), false, false, None, 1001);
        assert_eq!(lido(&itens, "lento"), (None, 0, false));
        anotar_resultado(&mut itens, "lento", "lento".into(), false, false, None, 1134);
        assert_eq!(lido(&itens, "lento"), (Some("lento".into()), 134, false));
        assert_eq!(lido(&itens, "rapido"), (Some("rapido".into()), 1, false));
        assert_eq!(lido(&itens, "fundo").1, 0);
        // O comando que passou do tempo e foi para o segundo plano depois de 2 min: o resultado
        // chega, mas a linha não diz que ele levou 2 min (ele segue rodando).
        itens.push(chamada("estourou", "Bash", 2000));
        anotar_resultado(&mut itens, "estourou", "Command did not complete within its 120s timeout".into(), false, true, None, 2120);
        assert_eq!(lido(&itens, "estourou").1, 0);
        // O que esperou a permissão, e o que falhou (a falha também diz quanto levou).
        itens.push(chamada("pedido", "Bash", 3000));
        anotar_resultado(&mut itens, "pedido", "erro".into(), true, false, Some(3200), 3215);
        assert_eq!(lido(&itens, "pedido"), (Some("erro".into()), 15, true));
        // O resultado de uma chamada que a conversa não tem não mexe em nada.
        anotar_resultado(&mut itens, "outra", "x".into(), false, false, None, 4000);
        assert_eq!(lido(&itens, "lento").1, 134);
        // A duração vai para o disco e volta; a de antes disto (sem o campo) é lida como não medida.
        let gravado = serde_json::to_value(&itens[1]).unwrap();
        assert_eq!(gravado["Ferramenta"]["durou"], 134);
        assert!(serde_json::to_value(&itens[3]).unwrap()["Ferramenta"].get("durou").is_none());
        let mut antigo = gravado.clone();
        antigo["Ferramenta"].as_object_mut().unwrap().remove("durou");
        assert!(matches!(serde_json::from_value::<Item>(antigo).unwrap(), Item::Ferramenta { durou: 0, .. }));
        assert!(matches!(serde_json::from_value::<Item>(gravado).unwrap(), Item::Ferramenta { durou: 134, .. }));
    }

    #[test]
    fn so_a_chamada_sem_resultado_do_turno_de_agora_tem_relogio() {
        let chamada = |id: &str, resultado: Option<&str>| Item::Ferramenta {
            id: id.into(),
            nome: "Bash".into(),
            resumo: String::new(),
            resultado: resultado.map(str::to_string),
            erro: false,
            em: 1000,
            detalhe: None,
            arquivo: None,
            sub: None,
            mudanca: None,
            durou: 0,
            imagens: Vec::new(),
        };
        let usuario = Item::Usuario { texto: "roda".into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 1000, marca: None };
        assert!(!tem_chamada_rodando(&[]));
        assert!(!tem_chamada_rodando(&[usuario.clone(), chamada("a", Some("ok"))]));
        // Duas em paralelo: enquanto uma não volta, há um relógio para andar.
        assert!(tem_chamada_rodando(&[usuario.clone(), chamada("a", None), chamada("b", Some("ok"))]));
        // A que ficou sem resultado num turno anterior (interrompida) não conta no de agora.
        assert!(!tem_chamada_rodando(&[usuario.clone(), chamada("velha", None), usuario.clone(), chamada("a", Some("ok"))]));
    }

    #[test]
    fn revisao_da_memoria_que_ja_escreveu_nao_se_corta() {
        let ferramenta = |nome: &str| Item::Ferramenta {
            id: "f".into(),
            nome: nome.into(),
            resumo: String::new(),
            resultado: None,
            erro: false,
            em: 0,
            detalhe: None,
            arquivo: None,
            sub: None,
            mudanca: None,
            imagens: Vec::new(),
            durou: 0,
        };
        assert!(ferramenta_de_escrita("Edit") && ferramenta_de_escrita("Write") && ferramenta_de_escrita("MultiEdit"));
        assert!(ferramenta_de_escrita("mcp__ai-memory__memory_write_page"));
        assert!(!ferramenta_de_escrita("Read") && !ferramenta_de_escrita("Bash") && !ferramenta_de_escrita("Grep"));
        assert!(!revisao_escreveu(&[ferramenta("Read"), ferramenta("Grep")]));
        assert!(revisao_escreveu(&[ferramenta("Read"), ferramenta("Edit")]));
        let pedido = Item::Entrega { de: "Vitral".into(), texto: PEDIDO_DE_REVISAO.into(), em: 0 };
        assert!(e_revisao_da_memoria(&pedido));
        assert!(!e_revisao_da_memoria(&Item::Entrega { de: "loja (x)".into(), texto: PEDIDO_DE_REVISAO.into(), em: 0 }));
    }

    #[test]
    fn turno_cortado_ganha_aviso_uma_vez() {
        let mut cortado = chat("p");
        cortado.turno_aberto = true;
        let mut chats = vec![cortado, chat("p2")];
        marcar_turnos_cortados(&mut chats);
        marcar_turnos_cortados(&mut chats);
        assert_eq!(avisos(&chats[0]), 1);
        assert_eq!(avisos(&chats[1]), 0);
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
    fn a_repeticao_do_turno_nao_repete_o_pedido() {
        let entrada = |texto: &str, registrada| Entrada {
            de: Some("chat c3".into()),
            texto: texto.into(),
            anexos: Vec::new(),
            registrada,
            ..Default::default()
        };
        let itens = itens_das_entradas(vec![entrada("pedido", true), entrada("resposta", false)], Some("m"));
        assert_eq!(itens.len(), 1);
        assert!(matches!(&itens[0], Item::Entrega { texto, .. } if texto == "resposta"));
        assert!(itens_das_entradas(vec![entrada("pedido", true)], Some("m")).is_empty());
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
        let mut velha = chat("c");
        velha.atualizado_em = dia;
        velha.lido_em = 2 * dia;
        velha.itens.push(Item::Assistente { texto: "pronto".into(), em: 0 });
        assert!(candidata_a_arquivar(&velha, limite));

        trocar_arquivado(&mut velha, agora);
        assert!(velha.arquivado);
        trocar_arquivado(&mut velha, agora);
        assert!(!velha.arquivado);
        assert!(!candidata_a_arquivar(&velha, limite));

        // A não lida continua não lida ao desarquivar.
        let mut nova = chat("n");
        nova.arquivado = true;
        nova.atualizado_em = 2 * dia;
        nova.lido_em = dia;
        nova.itens.push(Item::Assistente { texto: "resposta".into(), em: 0 });
        trocar_arquivado(&mut nova, agora);
        assert!(!nova.arquivado);
        assert_eq!(nova.lido_em, dia);

        let mut fixada = chat("f");
        fixada.fixado = true;
        assert!(!candidata_a_arquivar(&fixada, limite));
    }

    #[test]
    fn esforco_do_ultrathink_sobe_para_extra() {
        for atual in [None, Some(""), Some("low"), Some("medium"), Some("high"), Some("xhigh")] {
            assert_eq!(esforco_do_ultrathink(atual), "xhigh", "{atual:?}");
        }
        assert_eq!(esforco_do_ultrathink(Some("max")), "max");
    }

    const ULTRATHINK: Reforco = Reforco { ultrathink: true, ultracode: false, depois: false };
    const ULTRACODE: Reforco = Reforco { ultrathink: false, ultracode: true, depois: false };

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
        let ambos = Reforco { ultrathink: true, ultracode: true, depois: false };
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
        let itens = itens_das_entradas(vec![entrada], Some("m"));
        assert!(matches!(&itens[0], Item::Usuario { ultracode: true, ultrathink: true, .. }));
        // Comando de barra não leva (e a caixa continua com a chave ligada).
        assert!(!entrada_do_usuario("/compact".into(), Vec::new(), ULTRACODE).ultracode);
        // Mensagem de outro chat ou do Vitral nunca leva Ultracode.
        let mut entrega = entrada_do_usuario("pedido".into(), Vec::new(), Reforco::default());
        entrega.de = Some("chat c3".into());
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
        let item = serde_json::to_value(&itens_das_entradas(vec![lida], Some("m"))[0]).unwrap();
        assert_eq!(item["Usuario"]["ultracode"], true);
    }

    #[test]
    fn ultracode_so_onde_ele_liga_de_verdade() {
        let versao = Some((2, 1, 278));
        assert_eq!(motivo_sem_ultracode("opus", versao), None);
        assert_eq!(motivo_sem_ultracode("", None), None);
        // Antes da 2.1.269 o limite de agentes não existe.
        let antiga = motivo_sem_ultracode("opus", Some((2, 1, 268))).unwrap();
        assert!(antiga.contains("2.1.278") && antiga.contains("2.1.268"), "{antiga}");
        assert!(motivo_sem_ultracode("haiku", versao).unwrap().contains("Extra"));
    }

    /// O anel do Claude só com conta conectada (item 5 da rodada de 23/11).
    #[test]
    fn a_conta_do_claude_conectada_e_a_que_tem_login_e_nao_o_perdeu() {
        use super::conta_conectada;
        assert!(conta_conectada(true, false, false));
        assert!(!conta_conectada(false, false, false), "sem login guardado");
        assert!(!conta_conectada(true, true, false), "perdeu o login num turno");
        assert!(!conta_conectada(true, false, true), "a Anthropic recusou o token");
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
        assert!(adiar_o_fim(1, false, false, false));
        assert!(!adiar_o_fim(0, false, false, false));
        assert!(!adiar_o_fim(2, false, true, false), "erro termina o turno");
        assert!(!adiar_o_fim(2, false, false, true), "parado pelo usuário termina o turno");
    }

    fn textos(fila: &[Entrada]) -> Vec<&str> {
        fila.iter().map(|e| e.texto.as_str()).collect()
    }

    #[test]
    fn mexer_na_fila_sem_perder_as_outras() {
        let msg = |t: &str| Entrada { texto: t.into(), ..Default::default() };
        let mut fila = vec![msg("uma"), msg("duas"), msg("tres")];
        assert!(mover_na_lista(&mut fila, 2, false));
        assert_eq!(textos(&fila), ["uma", "tres", "duas"]);
        assert!(mover_na_lista(&mut fila, 0, true));
        assert_eq!(textos(&fila), ["tres", "uma", "duas"]);
        // Nas pontas não há para onde ir, e a lista fica como estava.
        assert!(!mover_na_lista(&mut fila, 0, false));
        assert!(!mover_na_lista(&mut fila, 2, true));
        assert_eq!(textos(&fila), ["tres", "uma", "duas"]);
        assert!(editar_na_lista(&mut fila, 1, "uma", "  outra  "));
        assert_eq!(textos(&fila), ["tres", "outra", "duas"]);
        // Texto em branco, índice que já saiu e lugar que mudou de mensagem não mexem em nada.
        assert!(!editar_na_lista(&mut fila, 1, "outra", "   "));
        assert!(!editar_na_lista(&mut fila, 9, "nada", "nada"));
        assert!(!editar_na_lista(&mut fila, 1, "a fila andou", "nada"));
        assert_eq!(textos(&fila), ["tres", "outra", "duas"]);
    }

    #[test]
    fn a_nota_do_vitral_nao_conta_como_lugar_na_fila() {
        let msg = |t: &str| Entrada { texto: t.into(), ..Default::default() };
        let nota = Entrada { texto: "nota".into(), nota: true, ..Default::default() };
        // A nota fica no começo (é onde o motor a põe) e as mensagens trocam por cima dela.
        let mut fila = vec![nota, msg("uma"), msg("duas")];
        assert!(mover_na_lista(&mut fila, 2, false));
        assert_eq!(textos(&fila), ["nota", "duas", "uma"]);
        // A própria nota não se move nem se edita: ela não está na lista.
        assert!(!mover_na_lista(&mut fila, 0, true));
        assert!(!editar_na_lista(&mut fila, 0, "nota", "outra"));
        assert_eq!(textos(&fila), ["nota", "duas", "uma"]);
    }

    #[test]
    fn resultado_com_mensagem_dirigida_esperando_tambem_espera() {
        // O Claude Code pode abrir outro turno na hora com ela, no mesmo processo.
        assert!(adiar_o_fim(0, true, false, false));
        assert!(!adiar_o_fim(0, true, true, false), "erro termina o turno");
        assert!(!adiar_o_fim(0, true, false, true), "parado pelo usuário termina o turno");
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
        let args = argumentos_do_aviso("loja · Preços", "Terminou", None, false);
        assert_eq!(
            args.join(" "),
            "-a Vitral -i vitral -h string:desktop-entry:vitral -A default=Abrir -p loja · Preços Terminou"
        );
        let args = argumentos_do_aviso("t", "c", Some(42), false);
        assert!(args.windows(2).any(|w| w == ["--replace-id", "42"]));
        assert!(!args.iter().any(|a| a == "critical"));
        // O pedido de permissão fica na tela até o clique.
        let args = argumentos_do_aviso("t", "c", None, true);
        assert!(args.windows(2).any(|w| w == ["-u", "critical"]));
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
        assert_eq!(texto_do_erro("\nYou've hit your usage limit\nresto"), "Parou com erro: You've hit your usage limit");
        assert_eq!(texto_do_erro(""), "Parou com erro");
        // Desde a 0.9.6 o aviso leva o texto, e não só a primeira linha (ver o teste
        // `o_aviso_do_fim_vai_sem_markdown_e_cortado_em_palavra`).
        assert_eq!(texto_do_fim("\n  Feito: o bug sumiu.\nresto"), "Terminou: Feito: o bug sumiu. resto");
    }

    #[test]
    fn o_que_espera_voce_num_chat() {
        let erro = Execucao::Erro("caiu".into());
        assert_eq!(espera(&Execucao::EsperandoVoce, false, true), Some(Espera::Pedido));
        assert_eq!(espera(&erro, false, true), Some(Espera::Parada));
        assert_eq!(espera(&erro, true, false), Some(Espera::Erro));
        assert_eq!(espera(&Execucao::Parado, false, false), None);
        assert_eq!(espera(&Execucao::Parado, true, false), Some(Espera::Nova));
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

    // O aceite do texto chegando: o primeiro pedaço muda o estado da conversa (ela passa a
    // "escrevendo") e redesenha tudo; os seguintes só fazem o texto crescer, e isso interessa só
    // ao painel que mostra o chat.
    #[test]
    fn so_o_primeiro_pedaco_de_texto_redesenha_a_janela_inteira() {
        let mut parcial = HashMap::new();
        let visiveis = HashSet::from(["aberto".to_string()]);
        assert_eq!(pedaco_de_texto(&mut parcial, &visiveis, "aberto", "Ol"), Pedaco::Comecou);
        assert_eq!(pedaco_de_texto(&mut parcial, &visiveis, "aberto", "á"), Pedaco::Cresceu);
        assert_eq!(pedaco_de_texto(&mut parcial, &visiveis, "aberto", ", tudo bem?"), Pedaco::Cresceu);
        assert_eq!(parcial["aberto"], "Olá, tudo bem?");
        // Fora dos painéis: o primeiro redesenha (a lista passa a dizer que ele escreve), os
        // outros não avisam ninguém.
        assert_eq!(pedaco_de_texto(&mut parcial, &visiveis, "fundo", "a"), Pedaco::Comecou);
        assert_eq!(pedaco_de_texto(&mut parcial, &visiveis, "fundo", "b"), Pedaco::Nada);
        assert_eq!(parcial["fundo"], "ab");
        // Um primeiro pedaço vazio ainda não mostra texto nenhum: o que vier depois é que começa.
        assert_eq!(pedaco_de_texto(&mut parcial, &visiveis, "aberto2", ""), Pedaco::Comecou);
        let visiveis = HashSet::from(["aberto2".to_string()]);
        assert_eq!(pedaco_de_texto(&mut parcial, &visiveis, "aberto2", "x"), Pedaco::Comecou);
        assert_eq!(pedaco_de_texto(&mut parcial, &visiveis, "aberto2", "y"), Pedaco::Cresceu);
    }

    // Os pedaços que chegam dentro da janela de 60 ms saem num aviso só: de texto (só os painéis
    // desses chats) quando o lote não trouxe mais nada, e da janela inteira no resto.
    #[test]
    fn o_lote_de_texto_vira_um_aviso_so_e_estreito() {
        assert_eq!(JANELA_DO_TEXTO, std::time::Duration::from_millis(60));
        assert_eq!(aviso_do_lote(false, Vec::new()), AvisoDoLote::Nada);
        let texto = vec!["a".to_string(), "a".to_string(), "b".to_string(), "a".to_string()];
        assert_eq!(aviso_do_lote(false, texto.clone()), AvisoDoLote::Texto(vec!["a".to_string(), "b".to_string()]));
        // Veio outra coisa junto (uma ferramenta, o fim do turno): a janela inteira.
        assert_eq!(aviso_do_lote(true, texto), AvisoDoLote::Tudo);
        assert_eq!(aviso_do_lote(true, Vec::new()), AvisoDoLote::Tudo);
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
    fn arquivar_apaga_so_o_chat_que_nunca_rodou() {
        let mut c = Chat::novo("c".into(), "Nova conversa", "/p/loja".into());
        c.itens.push(Item::Aviso { texto: "Esta conversa foi movida para loja.".into(), erro: false, em: 0 });
        assert!(nunca_rodou(&c));
        let mut falou = c.clone();
        falou.itens.push(Item::Usuario { texto: "oi".into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0, marca: None });
        assert!(!nunca_rodou(&falou));
        let mut sessao = c.clone();
        sessao.sessao_criada = true;
        assert!(!nunca_rodou(&sessao));
        let trazida = Chat { importada: Some(Default::default()), ..c.clone() };
        assert!(!nunca_rodou(&trazida), "a conversa do Claude Code arquiva, não some");
    }

    #[test]
    fn passagem_de_sessao_leva_as_ultimas_mensagens() {
        let usuario = |t: &str| Item::Usuario { texto: t.into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0, marca: None };
        let mut itens = vec![usuario("primeira")];
        for i in 0..12 {
            itens.push(Item::Assistente { texto: format!("resposta {i}"), em: 0 });
            itens.push(Item::Aviso { texto: "aviso".into(), erro: false, em: 0 });
        }
        itens.push(usuario("a de agora"));
        let bloco = bloco_de_passagem(&itens, 1, MotivoPassagem::Expirou);
        assert!(bloco.starts_with("[Vitral] Passagem de sessão: a sessão anterior do Claude Code desta conversa expirou"));
        // As 10 mais recentes, em ordem, sem os avisos e sem a do próprio turno.
        assert!(!bloco.contains("a de agora") && !bloco.contains("primeira") && !bloco.contains("aviso"));
        assert!(bloco.contains("Você: resposta 2") && !bloco.contains("resposta 1\n"));
        assert!(bloco.find("resposta 2").unwrap() < bloco.find("resposta 11").unwrap());
        assert!(bloco.ends_with("Você: resposta 11"));
        // Mensagens longas: as mais recentes ficam, até 6000 caracteres.
        let longas = vec![usuario(&"a".repeat(5000)), Item::Assistente { texto: "b".repeat(5000), em: 0 }];
        let bloco = bloco_de_passagem(&longas, 0, MotivoPassagem::Expirou);
        assert!(bloco.contains(&format!("Você: {}", "b".repeat(5000))));
        assert!(bloco.contains("…") && !bloco.contains(&"a".repeat(1000)));
        // O chat que saiu de uma cópia diz onde está agora e que o que ficou na cópia não veio.
        let bloco = bloco_de_passagem(&itens, 0, MotivoPassagem::SaiuDaCopia);
        assert!(bloco.contains("trabalhava numa cópia isolada da pasta") && bloco.contains("mexe direto na pasta de origem"));
        assert!(bloco.contains("As mudanças que estavam só na cópia não estão aqui."));
        assert!(bloco.ends_with("Usuário: a de agora"));
    }

    #[test]
    fn so_o_usuario_conta_como_do_usuario() {
        assert!(Entrada { texto: "oi".into(), ..Default::default() }.do_usuario());
        assert!(!Entrada { de: Some("Vitral".into()), ..Default::default() }.do_usuario());
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

/// O Enter com um pedido de permissão aberto responde a ele; o Parar com mensagem dirigida na
/// fila do Claude Code a retira em vez de matar o processo; e o modo, o modelo e o esforço
/// trocados no meio do turno vão para ele. As decisões, sem a janela.
#[cfg(test)]
mod testes_do_turno_que_roda {
    use super::*;

    fn entrada(texto: &str, anexos: &[&str], reforco: Reforco) -> Entrada {
        entrada_do_usuario(texto.into(), anexos.iter().map(PathBuf::from).collect(), reforco)
    }

    /// O critério do pedido: com o pedido aberto, o Enter entrega o texto ao modelo como a
    /// explicação da recusa. O que não cabe na recusa (imagem) entra pela entrada do processo, e
    /// o que vale para o turno inteiro continua indo para a fila, sem negar nada.
    #[test]
    fn a_mensagem_escrita_com_o_pedido_aberto_e_a_resposta_a_ele() {
        let simples = entrada("usa o pnpm, não o npm", &[], Reforco::default());
        assert_eq!(resposta_ao_pedido(&simples), RespostaAoPedido::Explicacao("usa o pnpm, não o npm".into()));
        // O arquivo anexado vai pelo caminho, no texto, como em qualquer mensagem.
        let com_arquivo = entrada("olha o log", &["/tmp/erro.log"], Reforco::default());
        match resposta_ao_pedido(&com_arquivo) {
            RespostaAoPedido::Explicacao(texto) => {
                assert!(texto.starts_with("olha o log") && texto.contains("/tmp/erro.log"), "{texto}")
            }
            outra => panic!("{outra:?}"),
        }
        // A imagem não cabe numa recusa: a mensagem inteira entra no turno.
        let com_imagem = entrada("olha o print", &["/tmp/tela.png"], Reforco::default());
        assert_eq!(resposta_ao_pedido(&com_imagem), RespostaAoPedido::Mensagem);
        // O que vale para o turno inteiro espera a vez, como antes.
        assert_eq!(resposta_ao_pedido(&entrada("/compact", &[], Reforco::default())), RespostaAoPedido::Fila);
        let ultrathink = Reforco { ultrathink: true, ..Reforco::default() };
        assert_eq!(resposta_ao_pedido(&entrada("pensa melhor", &[], ultrathink)), RespostaAoPedido::Fila);
        let ultracode = Reforco { ultracode: true, ..Reforco::default() };
        assert_eq!(resposta_ao_pedido(&entrada("faz tudo", &[], ultracode)), RespostaAoPedido::Fila);
        // O que não é do usuário (a entrega de outro chat) nunca responde a um pedido.
        let de_outro = Entrada { de: Some("Loja".into()), texto: "terminei".into(), ..Default::default() };
        assert_eq!(resposta_ao_pedido(&de_outro), RespostaAoPedido::Fila);
    }

    /// O mesmo caminho do "Negar e explicar…" do cartão: o modelo lê a mesma frase.
    #[test]
    fn a_recusa_escrita_e_a_do_negar_e_explicar() {
        assert_eq!(recusa_explicada("Bash", "usa o pnpm"), "O usuário negou esta ação e explicou o que fazer: usa o pnpm");
        assert_eq!(recusa_explicada("mcp__easypanel__deploy", " espera o galpão fechar \n"), "O usuário negou esta ação e explicou o que fazer: espera o galpão fechar");
        // Responder ao plano escrevendo é pedir mudanças nele.
        assert_eq!(recusa_explicada("ExitPlanMode", "começa pelos testes"), "O usuário pediu mudanças no plano: começa pelos testes");
        assert!(RESPONDEU_ESCREVENDO.contains("mensagem"));
    }

    /// O critério do Parar: com mensagem dirigida esperando, ele a retira e interrompe; o SIGTERM
    /// fica para quando não há turno a interromper (os agentes do fundo) ou não dá para retirar.
    #[test]
    fn o_parar_so_mata_o_processo_quando_nao_da_para_retirar() {
        assert_eq!(como_parar(false, false, true), Parada::Interromper);
        assert_eq!(como_parar(false, false, false), Parada::Interromper);
        assert_eq!(como_parar(false, true, true), Parada::RetirarEInterromper);
        assert_eq!(como_parar(false, true, false), Parada::Matar);
        assert_eq!(como_parar(true, false, true), Parada::Matar);
        assert_eq!(como_parar(true, true, true), Parada::Matar);
        // Só o Claude Code que conhece o pedido (conferido a partir do 2.1.323).
        assert!(retira_dirigidas(Agente::Claude, Some((2, 1, 328))));
        assert!(retira_dirigidas(Agente::Claude, Some((2, 1, 323))));
        assert!(!retira_dirigidas(Agente::Claude, Some((2, 1, 300))));
        assert!(!retira_dirigidas(Agente::Claude, None));
        assert!(!retira_dirigidas(Agente::Codex, Some((2, 1, 328))));
        // No fim do turno parado, o processo que teve a mensagem retirada sai sozinho.
        assert!(!sai_inteiro(false, 1, true));
        assert!(sai_inteiro(false, 1, false));
        assert!(sai_inteiro(true, 0, false));
        assert!(sai_inteiro(true, 1, true));
        assert!(!sai_inteiro(false, 0, false));
    }

    /// O turno com Ultrathink ou Ultracode abriu com o esforço dele: trocar o modo ou o modelo no
    /// meio não o desfaz, e o esforço escolhido na caixa vale no turno seguinte.
    #[test]
    fn o_esforco_do_turno_reforcado_fica_ate_o_fim() {
        let ao_vivo = |modo: &str, modelo: &str, esforco: Option<&str>| claude::AoVivo {
            modo: modo.into(),
            modelo: modelo.into(),
            esforco: esforco.map(str::to_string),
        };
        let tem = ao_vivo("manual", "opus", Some("xhigh"));
        let pedido = ao_vivo("plan", "haiku", Some("low"));
        assert_eq!(quer_ao_vivo(pedido.clone(), &tem, false), pedido);
        assert_eq!(quer_ao_vivo(pedido.clone(), &tem, true), ao_vivo("plan", "haiku", Some("xhigh")));
        assert_eq!(quer_ao_vivo(ao_vivo("manual", "opus", None), &tem, true), tem);
    }
}

/// Testes de aceite da pergunta com opções, do plano que fica na conversa e do marco de
/// compactação (o que o motor decide; o formato do Claude Code está nos testes de `claude.rs`).
#[cfg(test)]
mod testes_da_pergunta_do_plano_e_da_compactacao {
    use super::*;

    fn pergunta() -> Value {
        json!({ "questions": [
            { "question": "Qual cor você prefere?", "header": "Cor", "multiSelect": false,
              "options": [{ "label": "Azul", "description": "A do céu." }, { "label": "Verde", "description": "" }] },
            { "question": "Quais frutas?", "header": "Frutas", "multiSelect": true,
              "options": [{ "label": "Maçã", "description": "" }, { "label": "Uva", "description": "" }] },
        ] })
    }

    /// O que a CLI espera (medido no 2.1.328): `allow` com o `updatedInput` igual à entrada mais
    /// as `answers`, com a pergunta inteira como chave; várias escolhas numa resposta só.
    #[test]
    fn a_resposta_da_pergunta_leva_a_entrada_e_as_respostas() {
        let respostas = [
            ("Qual cor você prefere?".to_string(), "Verde".to_string()),
            ("Quais frutas?".to_string(), "Maçã, Uva".to_string()),
        ];
        let resposta = resposta_da_pergunta(&pergunta(), &respostas);
        assert_eq!(resposta["behavior"], "allow");
        assert_eq!(resposta["updatedInput"]["questions"], pergunta()["questions"], "sem as perguntas a CLI recusa");
        assert_eq!(
            resposta["updatedInput"]["answers"],
            json!({ "Qual cor você prefere?": "Verde", "Quais frutas?": "Maçã, Uva" })
        );
        // O que mais veio na entrada segue como veio.
        let mut com_mais = pergunta();
        com_mais["metadata"] = json!({ "source": "x" });
        assert_eq!(resposta_da_pergunta(&com_mais, &respostas)["updatedInput"]["metadata"], json!({ "source": "x" }));
    }

    #[test]
    fn a_pergunta_nao_e_um_pedido_de_permissao_comum() {
        let entrada = pergunta();
        // No sino, na lista e no Ctrl+G ela é uma pergunta, com o texto dela.
        assert_eq!(
            pendencia_do_pedido("p1", "c1", "AskUserQuestion", &entrada),
            Pendencia::Pergunta { pedido: "p1".into(), chat: "c1".into(), pergunta: "Qual cor você prefere?".into() }
        );
        assert_eq!(pendencia_do_pedido("p2", "c1", "ExitPlanMode", &json!({})), Pendencia::Plano { pedido: "p2".into(), chat: "c1".into() });
        assert!(matches!(pendencia_do_pedido("p3", "c1", "Bash", &json!({ "command": "ls" })), Pendencia::Permissao { .. }));
        assert_eq!(texto_do_pedido("AskUserQuestion", &entrada), "Tem uma pergunta para você: Qual cor você prefere?");
        assert_eq!(texto_do_pedido("AskUserQuestion", &json!({})), "Tem uma pergunta para você");
        // Nunca vira regra de "não perguntar de novo": a regra responderia sem resposta nenhuma.
        assert_eq!(crate::modelo::regra_de_permissao("AskUserQuestion", &entrada, Path::new("/tmp")), None);
        let mut chat = Chat::novo("c1".into(), "t", PathBuf::from("/tmp"));
        chat.permitidas.push("ferramenta:AskUserQuestion".into());
        assert!(!chat.permite("AskUserQuestion", &entrada));
        // Responder escrevendo na caixa de mensagem: o texto é a resposta de cada pergunta (a
        // "Outra…"), e vai como `allow`, não como recusa.
        let escritas = respostas_escritas(&entrada, " roxo com bolinhas ");
        assert_eq!(
            escritas,
            [
                ("Qual cor você prefere?".to_string(), "roxo com bolinhas".to_string()),
                ("Quais frutas?".to_string(), "roxo com bolinhas".to_string()),
            ]
        );
        assert_eq!(resposta_da_pergunta(&entrada, &escritas)["updatedInput"]["answers"]["Quais frutas?"], "roxo com bolinhas");
        assert!(respostas_escritas(&entrada, "  ").is_empty());
        assert!(respostas_escritas(&json!({}), "roxo").is_empty());
        // O "Não responder" do cartão: `allow` sem resposta nenhuma (a CLI diz ao modelo que
        // ninguém respondeu). A recusa de verdade (o Negar de outro lugar) tem a frase dela.
        assert_eq!(resposta_da_pergunta(&entrada, &[])["updatedInput"]["answers"], json!({}));
        assert_eq!(recusa_sem_texto("AskUserQuestion"), "O usuário não quis responder à pergunta.");
        assert_eq!(recusa_sem_texto("Bash"), "O usuário negou esta ação.");
    }

    #[test]
    fn o_marco_da_compactacao_diz_o_gatilho_e_o_tamanho() {
        assert_eq!(marco_da_compactacao(true, Some(42_987)), "Contexto compactado (manual, 43 mil tokens antes)");
        assert_eq!(marco_da_compactacao(false, Some(168_432)), "Contexto compactado (automático, 168 mil tokens antes)");
        assert_eq!(marco_da_compactacao(false, Some(1_240_000)), "Contexto compactado (automático, 1,2 mi de tokens antes)");
        assert_eq!(marco_da_compactacao(true, Some(850)), "Contexto compactado (manual, 850 tokens antes)");
        assert_eq!(marco_da_compactacao(false, None), "Contexto compactado (automático)");
        assert_eq!(marco_da_compactacao(true, Some(0)), "Contexto compactado (manual)");
    }

    fn chamada(id: &str, nome: &str, detalhe: Option<&str>) -> Item {
        Item::Ferramenta {
            id: id.into(),
            nome: nome.into(),
            resumo: nome.into(),
            resultado: None,
            erro: false,
            em: 0,
            detalhe: detalhe.map(str::to_string),
            arquivo: None,
            sub: None,
            mudanca: None,
            imagens: Vec::new(),
            durou: 0,
        }
    }

    fn detalhe_de(item: &Item) -> Option<&str> {
        match item {
            Item::Ferramenta { detalhe, .. } => detalhe.as_deref(),
            _ => None,
        }
    }

    /// No 2.1.328 a chamada do plano vem vazia: o texto dele chega no pedido de permissão (com o
    /// id da chamada) e fica guardado nela, para o cartão da conversa abrir depois de respondido.
    #[test]
    fn o_plano_do_pedido_fica_guardado_na_chamada() {
        let mut itens = vec![chamada("t1", "Read", None), chamada("t2", "ExitPlanMode", None)];
        assert!(guardar_plano(&mut itens, "t2", "# Plano\n\n1. Um.\n"));
        assert_eq!(detalhe_de(&itens[1]), Some("# Plano\n\n1. Um."));
        assert_eq!(detalhe_de(&itens[0]), None);
        // A chamada ainda não chegou (o pedido vem por outro canal): quem chama guarda para depois.
        assert!(!guardar_plano(&mut itens, "t9", "# Outro"));
        // Sem plano não há o que guardar, e outra ferramenta com o mesmo id não é tocada.
        assert!(!guardar_plano(&mut itens, "t2", "  "));
        assert!(!guardar_plano(&mut itens, "t1", "# Plano"));
        assert_eq!(detalhe_de(&itens[0]), None);
    }
}

