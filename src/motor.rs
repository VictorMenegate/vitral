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

use gpui_kit::{AppContext as _, Context, Task};
use serde_json::{Value, json};
use smol::channel::{Sender, unbounded};

use crate::{
    claude::{self, Evento, Mensagem, Opcoes, Processo, VERSAO_ULTRACODE, Versao},
    codex::{self, Codex},
    contas::{self, Conta, Plano},
    git,
    mcp::{self, Chamada, Resposta},
    modelo::{
        Chat, Config, Disco, Entrada, Estado, EstadoRef, EstadoTarefa, Execucao, Filas, Item, Papel,
        Reforco, Tarefa, Verificacao, agora, novo_id,
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
const TENTANDO: &str = "o processo caiu; tentando de novo";
/// Tarefa que acabou sem a cópia isolada: o trabalhador dela nunca roda.
const SEM_COPIA_CRIADA: &str = "A cópia isolada desta tarefa não foi criada, então o trabalhador \
                                não roda. Peça ao coordenador uma tarefa nova.";

enum Sinal {
    /// Evento do processo de um chat; o número separa um processo do seguinte do mesmo chat.
    Claude(String, u64, Evento),
    Mcp(Chamada),
    /// O clique numa notificação do sistema: abrir a conversa.
    Abrir(String),
}

/// A notificação de um chat que ainda está na tela: a próxima do mesmo chat a substitui.
#[derive(Default)]
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

pub struct Motor {
    pub disco: Disco,
    pub config: Config,
    pub chats: Vec<Chat>,
    pub tarefas: Vec<Tarefa>,
    proxima_tarefa: u64,
    execucao: HashMap<String, Execucao>,
    processos: HashMap<String, Processo>,
    geracao: u64,
    geracao_atual: HashMap<String, u64>,
    /// O que entrou no último turno de cada chat, para "Tentar de novo".
    ultima_entrada: HashMap<String, Vec<Entrada>>,
    /// Contas do Claude (a principal primeiro) e o uso do plano de cada uma.
    pub contas: Vec<Conta>,
    uso_por_conta: HashMap<String, Uso>,
    /// Até quando cada conta que bateu no limite fica de fora (segundos Unix).
    limite_ate: HashMap<String, u64>,
    /// Nome do plano, limites por modelo e créditos extras de cada conta (do `.claude.json`).
    plano_por_conta: HashMap<String, Plano>,
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
    /// Chats com mensagens ainda não gravadas: vão para o disco a cada poucos segundos.
    chats_sujos: HashSet<String>,
    /// Chats cujo turno já foi repetido depois de um erro de sessão (uma vez só).
    repetidos_sessao: HashSet<String>,
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
    avisos: Arc<std::sync::Mutex<HashMap<String, Aviso>>>,
    /// Chats que algum painel aberto mostra (a Janela atualiza a cada quadro): o texto parcial
    /// dos outros não redesenha a janela.
    pub visiveis: HashSet<String>,
    /// Marcado ao fechar ou trocar de pasta: a etapa seguinte de uma entrega ou preparação em
    /// segundo plano (revisor, script de preparação) não abre processo depois da parada.
    encerrado: Arc<AtomicBool>,
    servidor: Option<mcp::Servidor>,
    sinais: Sender<Sinal>,
    _recebedor: Task<()>,
}

impl Motor {
    pub fn new(raiz: &Path, cx: &mut Context<Self>) -> Self {
        let disco = Disco::new(raiz);
        let (estado, config) = disco.carregar();
        let Estado {
            mut chats,
            mut tarefas,
            proxima_tarefa,
            filas,
        } = estado;
        let antes: Vec<usize> = chats.iter().map(|c| c.itens.len()).collect();
        let (entregas_a_retomar, preparos_a_retomar) =
            marcar_interrompidas(&mut chats, &mut tarefas, &filas);
        // Avisos e marcas de parada vão para o disco já: se o PC cair de novo antes de qualquer
        // ação, a próxima abertura não marca as mesmas tarefas nem repete o aviso.
        for (c, n) in chats.iter().zip(antes) {
            if c.itens.len() != n {
                disco.salvar_chat(c);
            }
        }
        if !chats.is_empty() || !tarefas.is_empty() {
            disco.salvar_estado(&EstadoRef {
                chats: &chats,
                tarefas: &tarefas,
                proxima_tarefa,
                filas: &filas,
            });
        }
        disco.esperar_gravacoes();
        let (sinais, recebe) = unbounded::<Sinal>();

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

        crate::modelo::lembrar_recente(raiz);
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
        // As mensagens do turno em andamento vão para o disco a cada poucos segundos (o PC cai).
        cx.spawn(async move |motor, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(5))
                    .await;
                if motor.update(cx, |m, _| m.gravar_sujos()).is_err() {
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
        let motor = Self {
            disco,
            config,
            chats,
            tarefas,
            proxima_tarefa,
            execucao: HashMap::new(),
            processos: HashMap::new(),
            geracao: 0,
            geracao_atual: HashMap::new(),
            ultima_entrada: HashMap::new(),
            contas: contas::carregar(),
            uso_por_conta: HashMap::new(),
            limite_ate: HashMap::new(),
            plano_por_conta: HashMap::new(),
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
            chats_sujos: HashSet::new(),
            repetidos_sessao: HashSet::new(),
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
            servidor,
            sinais,
            _recebedor,
        };
        // O último uso do plano que o Claude Code guardou de cada conta e o do Codex.
        motor.reler_uso(cx);
        motor
    }

    // ---------- leitura para a interface ----------

    pub fn raiz(&self) -> &Path {
        &self.disco.raiz
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

    pub fn na_fila(&self, chat: &str) -> usize {
        self.filas.get(chat).map(Vec::len).unwrap_or(0)
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
                })
                .map(|t| t.trabalhador.as_str()),
        );
        chats.len()
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
                }
                EstadoTarefa::EmRevisao => {
                    t.entrega_pendente.is_some()
                        && !self.verificando.contains_key(&t.id)
                        && !self.entregas_a_retomar.contains(&t.id)
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
        espera(
            &self.execucao(&c.id),
            c.papel == Papel::Coordenador,
            self.nao_lido(c),
            self.precisa_retomar(&c.id),
        )
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
            c.papel == Papel::Coordenador && !self.arquivada(c) && projeto.is_none_or(|p| c.projeto == p)
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
            ativos.iter().filter(|c| c.papel == Papel::Coordenador && self.nao_lido(c)).collect();
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

    /// Por que a tarefa não pode ser aprovada agora, pelo coordenador ou pelo usuário. Tarefa que
    /// parou no meio (Vitral fechado, Parar) pode: o usuário decide pelo que vê no diff.
    pub fn recusa_aprovar(&self, id: u64) -> Option<String> {
        let Some(t) = self.tarefa(id) else {
            return Some("Tarefa não encontrada.".into());
        };
        let ocupado = |chat: &str| {
            self.processos.contains_key(chat)
                || self.esperando_vaga(chat)
                || self.retentativas.contains_key(chat)
        };
        let recusa = match t.estado {
            EstadoTarefa::Aprovada => format!("A tarefa #{id} já foi aprovada e juntada."),
            EstadoTarefa::Cancelada => format!(
                "A tarefa #{id} foi cancelada: a cópia ficou guardada, mas não vai ser juntada."
            ),
            EstadoTarefa::Falhou => {
                format!("A tarefa #{id} falhou sem chegar a ter cópia isolada; não há o que juntar.")
            }
            _ if self.juntando.contains(&id) => {
                format!("A tarefa #{id} já está sendo juntada; o resultado chega sozinho.")
            }
            _ if self.verificando.contains_key(&id) => format!(
                "A verificação e o revisor ainda estão conferindo a tarefa #{id}; espere o relatório."
            ),
            _ if !t.preparada => {
                "A cópia desta tarefa ainda está sendo preparada e o trabalhador nem começou.".into()
            }
            _ if ocupado(&t.trabalhador) => format!(
                "O trabalhador da tarefa #{id} ainda está rodando; espere o relatório antes de aprovar."
            ),
            _ if self.na_fila(&t.trabalhador) > 0 => format!(
                "O trabalhador da tarefa #{id} tem mensagem esperando para entrar (um ajuste ou uma \
                 resposta); espere o relatório antes de aprovar."
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
            Some(t) if !t.preparada && !t.estado.ativa() => "Sem cópia: fale com o coordenador",
            Some(t) if t.estado.encerrada() && chat.is_some_and(|c| !c.cwd.is_dir()) => {
                "Cópia removida: fale com o coordenador"
            }
            _ => "Mensagem ao trabalhador…",
        }
    }

    /// Os projetos à vista: no trilho, nos filtros, no "Nova conversa" e nas rotinas.
    pub fn projetos(&self) -> Vec<String> {
        self.config.separar_arquivados(self.todos_os_projetos()).0
    }

    /// Os arquivados, que voltam pelo menu do Vitral ou por "Todos os projetos".
    pub fn projetos_arquivados(&self) -> Vec<String> {
        self.config.separar_arquivados(self.todos_os_projetos()).1
    }

    fn todos_os_projetos(&self) -> Vec<String> {
        let mut nomes = self.disco.projetos();
        for chat in &self.chats {
            if !nomes.contains(&chat.projeto) {
                nomes.push(chat.projeto.clone());
            }
        }
        nomes
    }

    pub fn projeto_arquivado(&self, projeto: &str) -> bool {
        self.config.projeto_arquivado(projeto)
    }

    /// A conversa está em Arquivadas: arquivada ela mesma ou com o projeto.
    pub fn arquivada(&self, c: &Chat) -> bool {
        self.config.conversa_arquivada(c)
    }

    /// Arquiva ou desarquiva o projeto: só a marca na configuração, e as conversas ficam como
    /// estavam. Na volta, as já lidas renovam o `lido_em`, como ao desarquivar uma conversa:
    /// senão o arquivamento automático as guardaria de novo na passada seguinte.
    pub fn arquivar_projeto(&mut self, projeto: &str, arquivar: bool, cx: &mut Context<Self>) {
        self.config.projetos.entry(projeto.to_string()).or_default().arquivado = arquivar;
        self.disco.salvar_config(&self.config);
        if !arquivar {
            let agora = agora();
            for c in self.chats.iter_mut().filter(|c| c.projeto == projeto && !c.arquivado) {
                if c.lido_em >= c.atualizado_em || c.itens.is_empty() {
                    c.lido_em = c.lido_em.max(agora);
                }
            }
            self.salvar();
        }
        cx.notify();
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
            })
            .map(|t| t.id)
            .collect();
        Ocupacao { conversas, tarefas }
    }

    /// O que vai junto quando o projeto sai: conversas, cópias das tarefas e rotinas.
    pub fn resumo_do_projeto(&self, projeto: &str) -> ResumoDoProjeto {
        let (conversas, tarefas) = do_projeto(&self.chats, &self.tarefas, projeto);
        let papel = |p: Papel| self.chats.iter().filter(|c| conversas.contains(&c.id) && c.papel == p).count();
        ResumoDoProjeto {
            conversas: papel(Papel::Coordenador),
            trabalhadores: papel(Papel::Trabalhador),
            copias: self
                .tarefas
                .iter()
                .filter(|t| tarefas.contains(&t.id))
                .filter_map(|t| t.worktree.clone())
                .filter(|c| c.exists())
                .collect(),
            rotinas: self.config.rotinas.iter().filter(|r| r.projeto == projeto).count(),
        }
    }

    /// Manda a pasta do projeto para a Lixeira com `gio trash` e, só se ela foi, as cópias das
    /// tarefas (também para a Lixeira) e o que o Vitral guarda dele: conversas, tarefas, filas,
    /// rotinas, notas e configuração. Com a lixeira falhando, nada sai. Recusa com algo rodando.
    pub fn mover_projeto_para_lixeira(
        &mut self,
        projeto: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<NaLixeira, String>> {
        if !self.ocupacao_do_projeto(projeto).livre() {
            return Task::ready(Err(format!("{projeto} tem conversa ou tarefa trabalhando agora")));
        }
        let pasta = match pasta_para_lixeira(&self.disco.raiz, projeto) {
            Ok(pasta) => pasta,
            Err(erro) => return Task::ready(Err(erro)),
        };
        let resumo = self.resumo_do_projeto(projeto);
        let area = self.disco.pasta_worktrees(projeto);
        let projeto = projeto.to_string();
        cx.spawn(async move |motor, cx| {
            let feito = cx
                .background_spawn(async move {
                    let tinha_pasta = pasta.symlink_metadata().is_ok();
                    if tinha_pasta {
                        git::na_vez_do_projeto(&pasta, || crate::sistema::para_a_lixeira(&pasta))
                            .map_err(|erro| format!("{erro:#}"))?;
                    }
                    let mut erros = Vec::new();
                    for copia in &resumo.copias {
                        if let Err(erro) = crate::sistema::para_a_lixeira(copia) {
                            erros.push(format!("{}: {erro:#}", copia.display()));
                        }
                    }
                    let _ = std::fs::remove_dir(&area);
                    Ok::<_, String>((tinha_pasta, resumo, erros))
                })
                .await?;
            let (tinha_pasta, resumo, erros) = feito;
            motor
                .update(cx, |m, cx| {
                    let retirado =
                        tirar_do_estado(&mut m.chats, &mut m.tarefas, &mut m.filas, &mut m.config, &projeto);
                    m.esquecer(&retirado);
                    m.salvar();
                    m.disco.salvar_config(&m.config);
                    for id in &retirado.chats {
                        m.disco.apagar_chat(id);
                    }
                    m.disco.apagar_notas(&projeto);
                    m.disco.esquecer_projetos();
                    cx.notify();
                    NaLixeira { pasta: tinha_pasta, conversas: resumo.conversas, erros }
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
            self.fundo.remove(id);
            self.fim_adiado.remove(id);
            self.visiveis.remove(id);
        }
        self.espera.retain(|c| !saiu(c));
        self.interrompidos.retain(|c| !saiu(c));
        self.aguardando_resposta.retain(|c| !saiu(c));
        let (deles, outros): (Vec<_>, Vec<_>) = self.permissoes.drain(..).partition(|p| saiu(&p.chat));
        self.permissoes = outros;
        for pedido in deles {
            let _ = pedido.resposta.try_send(Resposta::ok(
                json!({"behavior": "deny", "message": "O projeto foi para a lixeira."}).to_string(),
            ));
        }
        for id in &r.tarefas {
            self.verificando.remove(id);
            self.juntando.remove(id);
            self.preparando.remove(id);
        }
        self.entregas_a_retomar.retain(|t| !r.tarefas.contains(t));
        self.preparos_a_retomar.retain(|t| !r.tarefas.contains(t));
        if self.pedido_de_abrir.as_ref().is_some_and(saiu) {
            self.pedido_de_abrir = None;
        }
    }

    pub fn caminho_projeto(&self, projeto: &str) -> PathBuf {
        self.disco.raiz.join(projeto)
    }

    pub fn ativar_git(&self, projeto: &str, cx: &mut Context<Self>) -> Task<anyhow::Result<git::RepositorioNovo>> {
        let caminho = self.caminho_projeto(projeto);
        cx.background_spawn(async move { git::iniciar_repositorio(&caminho) })
    }

    /// Cria a pasta de um projeto novo (com git, se pedido); a lista de projetos a acha sozinha.
    pub fn criar_projeto(&self, nome: &str, com_git: bool, cx: &mut Context<Self>) -> Task<anyhow::Result<()>> {
        let caminho = Self::nome_de_projeto(nome).map(|n| self.caminho_projeto(&n));
        cx.background_spawn(async move {
            let caminho = caminho?;
            std::fs::create_dir(&caminho).map_err(|erro| match erro.kind() {
                std::io::ErrorKind::AlreadyExists => anyhow::anyhow!("já existe uma pasta com esse nome"),
                _ => erro.into(),
            })?;
            if com_git {
                git::iniciar_repositorio(&caminho)
                    .map_err(|erro| anyhow::anyhow!("a pasta foi criada, mas o git não: {erro:#}"))?;
            }
            Ok(())
        })
    }

    /// O nome de um projeto novo, sem espaços nas pontas. Recusa o que sairia da pasta de
    /// trabalho ou ficaria escondido (como as pastas com ponto, que a lista ignora).
    pub fn nome_de_projeto(texto: &str) -> anyhow::Result<String> {
        let nome = texto.trim();
        if nome.is_empty() {
            anyhow::bail!("Dê um nome ao projeto.");
        }
        if nome.contains('/') || nome.contains('\0') {
            anyhow::bail!("O nome não pode ter “/”.");
        }
        if nome.starts_with('.') {
            anyhow::bail!("O nome não pode começar com ponto.");
        }
        Ok(nome.to_string())
    }

    // ---------- ações da interface ----------

    pub fn novo_coordenador(&mut self, projeto: &str, cx: &mut Context<Self>) -> String {
        let id = novo_id();
        let cwd = self.caminho_projeto(projeto);
        self.chats.push(Chat {
            id: id.clone(),
            projeto: projeto.to_string(),
            titulo: "Nova conversa".into(),
            papel: Papel::Coordenador,
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
            criado_em: agora(),
            atualizado_em: agora(),
            custo_usd: 0.0,
            custo_da_sessao: None,
            fixado: false,
            arquivado: false,
            lido_em: agora(),
            turno_aberto: false,
            modo_antes_do_plano: None,
            itens: Vec::new(),
        });
        self.salvar();
        cx.notify();
        id
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
        // Comandos que o Vitral resolve sozinho.
        if matches!(texto.as_str(), "/clear" | "/nova" | "/new") {
            self.nova_sessao(chat, cx);
            return;
        }
        if let Some(c) = self.chat_mut(chat)
            && c.itens.is_empty()
            && c.papel == Papel::Coordenador
            && !texto.starts_with('/')
        {
            c.titulo = titulo_de(&texto);
        }
        if let Err(recusa) = self.mensagem_direta(chat, &texto, cx) {
            self.avisar(chat, &recusa, true);
            self.salvar();
            cx.notify();
            return;
        }
        self.tentativas_auto.remove(chat);
        self.entregar_entrada(chat, entrada_do_usuario(texto, anexos, reforco), cx);
    }

    /// Mensagem do usuário direto a um trabalhador. Com a tarefa em revisão, vira uma nova rodada
    /// (com verificação, revisor e relatório) e o coordenador fica sabendo; sem a cópia da tarefa
    /// encerrada, não há onde rodar.
    fn mensagem_direta(&mut self, chat_id: &str, texto: &str, cx: &mut Context<Self>) -> Result<(), String> {
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
                "A tarefa #{id} está sendo juntada no projeto agora; a mensagem não foi enviada."
            ));
        }
        if !t.preparada && !t.estado.ativa() {
            return Err(SEM_COPIA_CRIADA.into());
        }
        if t.estado.encerrada() && sem_copia {
            return Err(format!(
                "A tarefa #{id} está {} e a cópia dela foi removida; a mensagem não foi enviada. \
                 Fale com o coordenador.",
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
            "O usuário falou direto com o trabalhador da tarefa #{id}, que estava em revisão, e isso \
             abriu a rodada {rodada}:\n{}\nO novo relatório chega sozinho quando ele terminar.",
            git::inicio_do_texto(texto, 600)
        );
        self.entregar(&coordenador, Some("Vitral".into()), aviso, cx);
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
        self.salvar();
        cx.notify();
    }

    /// Arquiva várias conversas de uma vez (o "Arquivar estas" da lista e o automático).
    pub fn arquivar_varios(&mut self, ids: &[String], cx: &mut Context<Self>) {
        for c in self.chats.iter_mut().filter(|c| ids.contains(&c.id)) {
            c.arquivado = true;
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
            .map(|c| c.id.clone())
            .collect();
        if !parados.is_empty() {
            self.arquivar_varios(&parados, cx);
        }
    }

    pub fn mudar_config(&mut self, f: impl FnOnce(&mut Config), cx: &mut Context<Self>) {
        f(&mut self.config);
        self.disco.salvar_config(&self.config);
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
        for chat in &self.chats {
            self.disco.salvar_chat(chat);
        }
        self.salvar();
        self.disco.esperar_gravacoes();
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
                registrada: false,
                ultrathink: false,
                ultracode: false,
            },
            cx,
        );
    }

    fn entregar_entrada(&mut self, chat: &str, entrada: Entrada, cx: &mut Context<Self>) {
        // Qualquer mensagem do coordenador (ou do usuário) responde a uma pergunta pendente.
        let do_coordenador = match &entrada.de {
            None => true,
            Some(origem) => self
                .chat(chat)
                .and_then(|c| c.pai.as_ref())
                .and_then(|pai| self.chat(pai))
                .is_some_and(|pai| {
                    origem == "coordenador" || origem.ends_with(&format!("({})", pai.id))
                }),
        };
        if do_coordenador {
            self.aguardando_resposta.retain(|c| c != chat);
        }
        self.filas.entry(chat.to_string()).or_default().push(entrada);
        let ocupado = self.processos.contains_key(chat) || self.esperando_vaga(chat);
        if ocupado {
            self.salvar();
            cx.notify();
            return;
        }
        // Livre: entra tudo o que estava esperando, junto com a mensagem nova.
        self.entregar_fila(chat, cx);
    }

    /// Começa um turno com as mensagens que estavam na fila de um chat parado.
    pub fn entregar_fila(&mut self, chat: &str, cx: &mut Context<Self>) {
        if self.processos.contains_key(chat) || self.esperando_vaga(chat) {
            return;
        }
        if let Some(fila) = self.filas.remove(chat)
            && !fila.is_empty()
        {
            self.comecar_turno(chat, fila, cx);
        }
    }

    /// Repete o último turno de um chat que falhou.
    pub fn tentar_de_novo(&mut self, chat: &str, cx: &mut Context<Self>) {
        if self.processos.contains_key(chat) {
            return;
        }
        if let Some(mensagens) = self.ultima_entrada.get(chat).cloned() {
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
            self.retomar_entregas(cx);
        } else if let Some(t) = &tarefa
            && precisa_preparar(t, &chat, &self.filas)
        {
            // O Vitral fechou durante a preparação da cópia: ela é refeita e o pedido vai depois.
            self.preparar_copia(t.id, None, cx);
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
            .filter(|c| c.papel == Papel::Trabalhador)
            .and_then(|c| c.tarefa)
            .filter(|t| self.tarefa(*t).is_some_and(|t| t.estado.ativa()))
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
        let mut mensagens = self.ultima_entrada.get(chat_id).cloned()?;
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
        if chat.papel == Papel::Trabalhador
            && let Some(id) = ativa
        {
            let relatorio = format!(
                "O processo do Claude Code caiu e a tarefa não terminou: {detalhe}\nO que já estava \
                 feito continua na cópia da tarefa."
            );
            self.tarefa_terminou(id, true, relatorio, cx);
            if !self.janela_ativa {
                self.notificar(&chat, "Parou com erro: o trabalhador caiu e a tarefa voltou ao coordenador");
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
        let Some(chat) = self.chat_mut(chat_id).map(Chat::copia_sem_itens) else {
            return;
        };
        // Repetição do turno: se for esperar na fila, não entra de novo na conversa depois.
        if !registrar {
            for entrada in &mut mensagens {
                entrada.registrada = true;
            }
        }
        // Um turno novo toma o lugar da nova tentativa agendada.
        self.retentativas.remove(chat_id);
        // Sem a cópia pronta, o trabalhador não roda: rodaria na pasta original, sem o pedido.
        if let Some(t) = chat
            .tarefa
            .and_then(|t| self.tarefa(t))
            .filter(|t| !t.preparada)
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
            if self.preparando.contains(&t.id) || self.preparos_a_retomar.contains(&t.id) {
                self.avisar(
                    chat_id,
                    "A cópia da tarefa ainda está sendo preparada; a mensagem entra quando ela \
                     ficar pronta.",
                    false,
                );
            } else {
                self.preparar_copia(t.id, None, cx);
            }
            self.salvar();
            cx.notify();
            return;
        }
        if self.copia_apagada(&chat) {
            self.avisar(chat_id, "A cópia desta tarefa já foi apagada; esta conversa não roda mais.", true);
            cx.notify();
            return;
        }
        // A pasta sumiu (cópia de tarefa já juntada, projeto apagado): o Claude Code nem abriria.
        if !chat.cwd.is_dir() {
            self.avisar(
                chat_id,
                &format!(
                    "A pasta desta conversa não existe mais ({}); a mensagem não foi enviada.",
                    chat.cwd.display()
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
        // Trabalhador sem vaga espera, com as mensagens guardadas.
        if chat.papel == Papel::Trabalhador
            && (self.trabalhadores_ativos() >= self.config.max_trabalhadores || outro_ultracode)
        {
            self.filas
                .entry(chat_id.to_string())
                .or_default()
                .extend(mensagens);
            if !self.esperando_vaga(chat_id) {
                self.espera.push_back(chat_id.to_string());
                if outro_ultracode {
                    self.avisar(chat_id, "O Ultracode espera o da outra conversa terminar.", false);
                }
            }
            cx.notify();
            return;
        }
        if sem_ultracode.is_some() {
            for entrada in &mut mensagens {
                entrada.ultracode = false;
            }
        }

        let (texto, imagens) = texto_do_turno(&mensagens);
        let ultrathink = mensagens.iter().any(|e| e.ultrathink);
        self.ultima_entrada
            .insert(chat_id.to_string(), mensagens.clone());
        // Ponto de volta: foto dos arquivos antes de quem edita (trabalhador) começar o turno.
        let novas = mensagens.iter().any(|e| !e.registrada);
        let ponto = (novas && chat.papel == Papel::Trabalhador && git::e_repositorio(&chat.cwd))
            .then(|| git::foto(&chat.cwd).ok())
            .flatten();
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
        let mut opcoes = self.opcoes(&chat, servidor.url(chat.papel, &chat.id));
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
            self.disco.salvar_chat(c);
        }
        self.salvar();
        cx.notify();
    }

    fn opcoes(&self, chat: &Chat, url_mcp: String) -> Opcoes {
        let notas = self.disco.notas(&chat.projeto);
        let projeto = self.caminho_projeto(&chat.projeto);
        let (prompt_sistema, bloqueadas, pastas_extras, modelo_padrao) = match chat.papel {
            Papel::Coordenador => (
                prompts::coordenador(&chat.projeto, &projeto, self.raiz(), &self.config, &notas),
                // Quem abre agentes são os trabalhadores: nem subagente nem fluxo de agentes aqui,
                // em turno nenhum.
                ["Edit", "Write", "NotebookEdit", "Task", "Workflow"]
                    .map(String::from)
                    .to_vec(),
                vec![self.disco.pasta_worktrees(&chat.projeto)],
                self.config.modelo_coordenador.clone(),
            ),
            Papel::Trabalhador => {
                let isolado = chat.cwd != projeto;
                // Na cópia isolada, a pasta original do projeto fica só para leitura (as regras de
                // bloqueio valem até no modo sem permissões; `//` marca caminho absoluto).
                let bloqueadas = if isolado {
                    ["Edit", "Write", "NotebookEdit"]
                        .map(|f| format!("{f}(/{}/**)", projeto.display()))
                        .to_vec()
                } else {
                    Vec::new()
                };
                (
                    prompts::trabalhador(&chat.projeto, &projeto, &chat.cwd, isolado, &notas),
                    bloqueadas,
                    Vec::new(),
                    self.config.modelo_trabalhador.clone(),
                )
            }
        };
        if let Some(pasta) = pastas_extras.first() {
            let _ = std::fs::create_dir_all(pasta);
        }
        // Trabalhador: portas próprias (servidores de teste não brigam entre cópias) e, se a
        // tarefa tem verificação, um hook que roda os testes quando ele para.
        // O `git status` dos agentes não segura o index.lock (derrubaria uma junção do Vitral).
        let mut ambiente = vec![("GIT_OPTIONAL_LOCKS".to_string(), "0".to_string())];
        let mut configuracoes = None;
        if chat.papel == Papel::Trabalhador
            && let Some(tarefa) = chat.tarefa.and_then(|t| self.tarefa(t))
        {
            ambiente.push(("VITRAL_PORT".into(), porta_da_tarefa(tarefa.id).to_string()));
            if let Some(verificacao) = &tarefa.verificacao {
                ambiente.push(("VITRAL_VERIFICACAO".into(), verificacao.clone()));
                configuracoes = crate::revisor::configuracao_hook_verificacao();
            }
        }
        Opcoes {
            cwd: chat.cwd.clone(),
            sessao: chat.sessao.clone(),
            retomar: chat.sessao_criada,
            modelo: chat.modelo.clone().or(Some(modelo_padrao)),
            permissao: self.permissao_de(chat),
            prompt_sistema,
            url_mcp,
            bloqueadas,
            pastas_extras,
            // O MCP do usuário pode ter centenas de ferramentas: fora por padrão (no Haiku, que não
            // adia a carga delas, o pedido estoura o contexto).
            so_mcp_vitral: chat.papel == Papel::Coordenador || !self.config.mcp_trabalhadores,
            esforco: chat.esforco.clone(),
            ultracode: false,
            pasta_conta: None,
            ambiente,
            configuracoes,
        }
    }

    /// Por que o Ultracode não vale nesta conversa agora (`None`: vale). O de outra conversa
    /// rodando não entra: esse só faz esperar a vez.
    pub fn motivo_sem_ultracode(&self, chat: &Chat) -> Option<String> {
        let padrao = match chat.papel {
            Papel::Coordenador => &self.config.modelo_coordenador,
            Papel::Trabalhador => &self.config.modelo_trabalhador,
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
        let Some(mensagens) = self.ultima_entrada.get_mut(chat_id) else {
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
            .is_some_and(|m| m.iter().any(|e| e.ultracode))
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
        let Some(mensagens) = self.ultima_entrada.get(chat_id).cloned() else {
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
        let aviso = if existe {
            "Retomando a sessão anterior."
        } else {
            "A sessão do Claude desta conversa sumiu (pode ter se perdido numa queda); continuando \
             numa sessão nova, que não lembra das mensagens acima."
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
        for conta in &self.contas {
            let (lido, plano) = lidos
                .iter()
                .find(|(id, ..)| *id == conta.id)
                .map(|(_, uso, plano)| (*uso, plano.clone()))
                .unwrap_or_default();
            let antes = self.uso_por_conta.get(&conta.id).copied().unwrap_or_default();
            let depois = antes.juntar(lido, agora);
            if depois != antes {
                self.uso_por_conta.insert(conta.id.clone(), depois);
                mudou = true;
            }
            if self.plano_por_conta.get(&conta.id) != Some(&plano) {
                self.plano_por_conta.insert(conta.id.clone(), plano);
                mudou = true;
            }
        }
        mudou
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
            self.disco.salvar_config(&self.config);
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
            .filter(|id| self.chat(id).is_some_and(|c| c.papel == Papel::Trabalhador))
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
                    if let Some(c) = self.chat(chat_id) {
                        self.disco.salvar_chat(c);
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
                        self.disco.salvar_chat(c);
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
                        self.notificar(c, "Parou com erro: o trabalhador caiu e a tarefa voltou ao coordenador");
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
                        self.disco.salvar_chat(c);
                    }
                    self.liberar_vaga(cx);
                    self.salvar();
                }
            }
        }
    }

    fn fim_de_turno(&mut self, chat_id: &str, erro: bool, texto: String, cx: &mut Context<Self>) {
        self.parcial.remove(chat_id);
        if !erro {
            self.tentativas_auto.remove(chat_id);
            self.repetidos_sessao.remove(chat_id);
        }
        if let Some(c) = self.chat(chat_id) {
            self.disco.salvar_chat(c);
        }
        let Some(chat) = self.chat_mut(chat_id).map(Chat::copia_sem_itens) else {
            return;
        };

        if chat.papel == Papel::Trabalhador
            && let Some(id) = chat.tarefa
            && self.tarefa(id).is_some_and(|t| t.estado.ativa())
        {
            // Parou para perguntar algo, ou já tem resposta/ajuste esperando: ainda não é a
            // entrega; o relatório sai no fim do próximo turno.
            let continua = self.filas.get(chat_id).is_some_and(|f| !f.is_empty());
            if (self.aguardando_resposta.contains(&chat.id) || continua) && !erro {
                if let Some(t) = self.tarefa_mut(id) {
                    t.progresso = Some("esperando resposta do coordenador".into());
                }
            } else {
                self.tarefa_terminou(id, erro, texto.clone(), cx);
            }
        }

        // Mensagens que chegaram durante o turno viram o próximo.
        if let Some(fila) = self.filas.remove(chat_id)
            && !fila.is_empty()
        {
            self.comecar_turno(chat_id, fila, cx);
        } else if chat.papel == Papel::Coordenador && !self.janela_ativa {
            let ativas = self.tarefas_do(chat_id).any(|t| t.estado.ativa());
            if !ativas {
                let aprovadas =
                    self.tarefas_do(chat_id).filter(|t| t.estado == EstadoTarefa::Aprovada).count();
                self.notificar(&chat, &texto_do_fim(aprovadas, &texto));
            }
        }
        self.liberar_vaga(cx);
        self.salvar();
    }

    fn liberar_vaga(&mut self, cx: &mut Context<Self>) {
        // Uma volta na fila só. O Ultracode que ainda espera o de outra conversa fica no lugar
        // dele, sem passar de novo por comecar_turno (que o avisaria mais uma vez).
        let mut esperam_ultracode = Vec::new();
        for _ in 0..self.espera.len() {
            if self.trabalhadores_ativos() >= self.config.max_trabalhadores {
                break;
            }
            let Some(proximo) = self.espera.pop_front() else {
                break;
            };
            if self.espera_outro_ultracode(&proximo) {
                esperam_ultracode.push(proximo);
                continue;
            }
            let fila = self.filas.remove(&proximo).unwrap_or_default();
            if !fila.is_empty() {
                self.comecar_turno(&proximo, fila, cx);
            }
        }
        for chat in esperam_ultracode.into_iter().rev() {
            self.espera.push_front(chat);
        }
    }

    /// O trabalhador terminou o turno: roda a verificação e manda o relatório ao coordenador.
    fn tarefa_terminou(&mut self, id: u64, erro: bool, relatorio: String, cx: &mut Context<Self>) {
        let Some(tarefa) = self.tarefa_mut(id) else {
            return;
        };
        tarefa.estado = EstadoTarefa::EmRevisao;
        tarefa.relatorio = Some(relatorio.clone());
        tarefa.progresso = Some(VERIFICANDO.into());
        tarefa.parada = None;
        // Gravado antes de verificar: se o Vitral cair no meio, a entrega é refeita ao abrir.
        tarefa.entrega_pendente = Some(erro);
        let tarefa = tarefa.clone();
        self.verificando.insert(id, tarefa.rodada);
        let projeto = self.caminho_projeto(&tarefa.projeto);
        // O revisor usa a mesma conta do trabalhador e o modelo configurado.
        let revisar = self.config.revisor && !erro && tarefa.worktree.is_some();
        let modelo_revisor = Some(self.config.modelo_revisor.clone())
            .filter(|m| !m.is_empty())
            .or_else(|| self.chat(&tarefa.trabalhador).and_then(|c| c.modelo.clone()))
            .or_else(|| Some(self.config.modelo_trabalhador.clone()).filter(|m| !m.is_empty()));
        let pasta_conta = self
            .chat(&tarefa.trabalhador)
            .map(|c| self.conta_para(c))
            .and_then(|c| c.pasta());
        // Arquivos que as outras tarefas em andamento do mesmo projeto estão mexendo.
        let outras: Vec<(u64, PathBuf, String)> = self
            .tarefas
            .iter()
            .filter(|t| t.id != id && t.projeto == tarefa.projeto && !t.estado.encerrada())
            .filter_map(|t| Some((t.id, t.worktree.clone()?, t.base.clone()?)))
            .collect();
        let encerrado = self.encerrado.clone();

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
                        .map(|c| git::verificar(&onde, &c));
                    // Depois de um `git merge` pedido num conflito, a base certa é o que veio do projeto.
                    let base = match (&tarefa.worktree, &tarefa.base) {
                        (Some(p), Some(b)) => Some(git::base_efetiva(&projeto, p, b)),
                        _ => None,
                    };
                    let (estatistica, meus) = match (&tarefa.worktree, &base) {
                        (Some(p), Some(b)) => (
                            git::diff_stat(p, b).ok(),
                            git::arquivos_mudados(p, b),
                        ),
                        _ => (None, Vec::new()),
                    };
                    let mut sobreposicao = Vec::new();
                    for (outra, pasta, base) in outras {
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
                    let revisao = match (&tarefa.worktree, &base) {
                        (Some(p), Some(b)) if revisar && !meus.is_empty() && !parou() => Some(
                            crate::revisor::revisar(p, b, &tarefa, modelo_revisor.as_deref(), pasta_conta)
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
                let titulo = &tarefa.titulo;
                let rodada = tarefa.rodada;
                let mut texto = format!(
                    "A tarefa #{id} \"{titulo}\" terminou a rodada {rodada}{}.\n\nRelatório do trabalhador:\n{relatorio}\n",
                    if erro { " com ERRO" } else { "" }
                );
                match &estatistica {
                    Some(e) if !e.trim().is_empty() => {
                        texto.push_str(&format!("\nArquivos alterados (git diff --stat):\n{e}\n"))
                    }
                    Some(_) => texto.push_str("\nNenhum arquivo foi alterado na cópia da tarefa.\n"),
                    None => {}
                }
                if let Some(v) = &verificacao {
                    texto.push_str(&format!(
                        "\nVerificação `{}`: {}{}\n```\n{}\n```\n",
                        v.comando,
                        if v.passou { "PASSOU" } else { "FALHOU" },
                        v.tempo().map(|t| format!(" em {t}")).unwrap_or_default(),
                        v.saida.trim()
                    ));
                }
                let revisao = match revisao {
                    Some(Ok(r)) => {
                        texto.push_str(&format!("\n{}", crate::revisor::texto(&r)));
                        Some(r)
                    }
                    Some(Err(erro)) => {
                        texto.push_str(&format!("\nO revisor independente não conseguiu revisar: {erro}\n"));
                        None
                    }
                    None => None,
                };
                if !sobreposicao.is_empty() {
                    texto.push_str(&format!(
                        "\nAtenção: outras tarefas em andamento mexem nos mesmos arquivos ({}). \
                         Junte uma de cada vez e espere conflito na segunda.\n",
                        sobreposicao.join("; ")
                    ));
                }
                texto.push_str(&format!(
                    "\nRevise (ver_diff {id}; se o diff for grande vem um resumo e você pede os arquivos que \
                     importam) e decida: aprovar_tarefa ou pedir_ajuste."
                ));
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
                if vale {
                    motor.entregar(&tarefa.coordenador, Some("Vitral".into()), texto, cx);
                }
                motor.salvar();
                motor.retomar_entregas(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Refaz, uma de cada vez, as entregas que ficaram no meio da verificação quando o Vitral
    /// fechou, e depois as preparações de cópia. A marca de parada fica enquanto roda: se cair de
    /// novo, espera o Retomar.
    fn retomar_entregas(&mut self, cx: &mut Context<Self>) {
        if !self.verificando.is_empty() || !self.preparando.is_empty() {
            return;
        }
        while let Some(id) = self.entregas_a_retomar.pop_front() {
            let Some(t) = self.tarefa_mut(id) else {
                continue;
            };
            let (EstadoTarefa::EmRevisao, Some(erro)) = (t.estado, t.entrega_pendente) else {
                continue;
            };
            let relatorio = t.relatorio.clone().unwrap_or_default();
            self.tarefa_terminou(id, erro, relatorio, cx);
            if let Some(t) = self.tarefa_mut(id) {
                t.parada = Some(PARADA_VERIFICACAO.into());
            }
            self.salvar();
            cx.notify();
            return;
        }
        while let Some(id) = self.preparos_a_retomar.pop_front() {
            let pendente = self.tarefa(id).is_some_and(|t| {
                self.chat(&t.trabalhador)
                    .is_some_and(|c| precisa_preparar(t, c, &self.filas))
            });
            if !pendente {
                continue;
            }
            if let Some(t) = self.tarefa_mut(id) {
                t.parada = Some(PARADA_PREPARACAO.into());
            }
            self.preparar_copia(id, None, cx);
            return;
        }
    }

    // ---------- ferramentas MCP ----------

    fn tratar_ferramenta(&mut self, chamada: Chamada, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_mut(&chamada.chat).map(Chat::copia_sem_itens) else {
            let _ = chamada
                .resposta
                .try_send(Resposta::erro("Chat desconhecido para o Vitral."));
            return;
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
                // Como os outros avisos, só com a janela sem foco: com ela à frente, o pedido já
                // aparece no app (e se responde lá, nunca na notificação).
                if !self.janela_ativa {
                    self.notificar(&chat, &texto_do_pedido(a["tool_name"].as_str().unwrap_or("?"), &a["input"]));
                }
                return;
            }
            "listar_chats" => Resposta::ok(self.texto_chats()),
            "mensagem_para_chat" => {
                let destino = texto("chat");
                let corpo = texto("texto");
                if self.chat(&destino).is_none() {
                    Resposta::erro(format!("Não existe chat \"{destino}\". Use listar_chats."))
                } else if destino == chat.id {
                    Resposta::erro("Esse é você mesmo.")
                } else if self.chat(&destino).is_some_and(|c| !c.cwd.is_dir()) {
                    Resposta::erro(
                        "A pasta desse chat não existe mais (a cópia da tarefa já foi removida); a \
                         mensagem não foi entregue.",
                    )
                } else if self
                    .chat(&destino)
                    .and_then(|c| c.tarefa)
                    .and_then(|t| self.tarefa(t))
                    .is_some_and(|t| !t.preparada && !t.estado.ativa())
                {
                    Resposta::erro(
                        "A cópia isolada da tarefa desse trabalhador não foi criada e ele não roda; \
                         a mensagem não foi entregue. Crie uma tarefa nova se ainda fizer sentido.",
                    )
                } else {
                    let origem = format!("{} ({})", chat.titulo, chat.id);
                    self.entregar(&destino, Some(origem), corpo, cx);
                    Resposta::ok("Mensagem entregue (ou na fila, se o chat estiver ocupado).")
                }
            }
            "perguntar_coordenador" => match (&chat.pai, chat.tarefa) {
                (Some(pai), Some(id)) => {
                    let pergunta = format!(
                        "Pergunta do trabalhador da tarefa #{id} ({}): {}\n\nResponda com \
                         mensagem_para_chat para o chat {}.",
                        chat.titulo,
                        texto("pergunta"),
                        chat.id
                    );
                    if !self.aguardando_resposta.contains(&chat.id) {
                        self.aguardando_resposta.push(chat.id.clone());
                    }
                    self.entregar(&pai.clone(), Some(chat.titulo.clone()), pergunta, cx);
                    Resposta::ok(
                        "Pergunta enviada. A resposta chega como nova mensagem; se puder continuar \
                         sem ela, continue.",
                    )
                }
                _ => Resposta::erro("Este chat não tem coordenador."),
            },
            "informar_progresso" => {
                if let Some(id) = chat.tarefa
                    && let Some(t) = self.tarefa_mut(id)
                {
                    t.progresso = Some(texto("texto"));
                }
                Resposta::ok("ok")
            }
            "listar_tarefas" => Resposta::ok(self.texto_tarefas(&chat.id)),
            "anotar_decisao" => match self.disco.anotar(&chat.projeto, &texto("texto")) {
                Ok(()) => {
                    Resposta::ok("Anotado. Vale para os próximos trabalhadores deste projeto.")
                }
                Err(erro) => Resposta::erro(format!("Não anotou: {erro}")),
            },
            "criar_tarefa" if chat.papel == Papel::Coordenador => {
                self.criar_tarefa(&chat, a.clone(), chamada.resposta, cx);
                return;
            }
            "ver_diff" | "pedir_ajuste" | "aprovar_tarefa" | "cancelar_tarefa"
                if chat.papel == Papel::Coordenador =>
            {
                let Some(id) = numero("tarefa") else {
                    let _ = chamada
                        .resposta
                        .try_send(Resposta::erro("Informe o número da tarefa."));
                    return;
                };
                if !self.tarefa(id).is_some_and(|t| t.coordenador == chat.id) {
                    let _ = chamada.resposta.try_send(Resposta::erro(format!(
                        "A tarefa #{id} não é deste coordenador."
                    )));
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
                            Ok(ressalva) => {
                                self.aprovar_tarefa(id, texto("nota"), ressalva, chamada.resposta, cx)
                            }
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
                        let ignorar: Vec<usize> = lista_de(&a["ignorar_problemas"])
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|v| {
                                v.as_u64()
                                    .or_else(|| v.as_str()?.trim().trim_start_matches('#').parse().ok())
                            })
                            .map(|n| n as usize)
                            .collect();
                        self.pedir_ajuste(id, texto("feedback"), incluir, ignorar, cx)
                    }
                    _ => self.cancelar_tarefa(id, texto("motivo"), cx),
                }
            }
            outra => Resposta::erro(format!("Ferramenta \"{outra}\" não existe para este chat.")),
        };
        let _ = chamada.resposta.try_send(resposta);
        self.salvar();
    }

    fn criar_tarefa(
        &mut self,
        coordenador: &Chat,
        argumentos: Value,
        resposta: Sender<Resposta>,
        cx: &mut Context<Self>,
    ) {
        let texto = |chave: &str| {
            argumentos[chave]
                .as_str()
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        let titulo = texto("titulo");
        let objetivo = texto("objetivo");
        let criterios: Vec<String> = argumentos["criterios"]
            .as_array()
            .map(|lista| {
                lista
                    .iter()
                    .filter_map(|c| c.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if titulo.is_empty() || objetivo.is_empty() || criterios.is_empty() {
            let _ = resposta.try_send(Resposta::erro(
                "criar_tarefa precisa de titulo, objetivo e pelo menos um critério.",
            ));
            return;
        }
        let verificacao = Some(texto("verificacao")).filter(|v| !v.is_empty());
        let modelo = Some(texto("modelo")).filter(|v| !v.is_empty());

        let id = self.proxima_tarefa;
        self.proxima_tarefa += 1;
        let projeto = coordenador.projeto.clone();
        let caminho_projeto = self.caminho_projeto(&projeto);
        let trabalhador = novo_id();
        self.tarefas.push(Tarefa {
            id,
            projeto: projeto.clone(),
            coordenador: coordenador.id.clone(),
            trabalhador: trabalhador.clone(),
            titulo: titulo.clone(),
            objetivo,
            criterios,
            verificacao,
            estado: EstadoTarefa::Rodando,
            rodada: 1,
            worktree: None,
            branch: None,
            base: None,
            copia_limpa: false,
            progresso: Some("preparando a cópia do projeto".into()),
            relatorio: None,
            ultima_verificacao: None,
            criada_em: agora(),
            revisao: None,
            sobreposicao: Vec::new(),
            preparacao: None,
            parada: None,
            entrega_pendente: None,
            preparada: false,
            ressalva: None,
        });
        self.chats.push(Chat {
            id: trabalhador.clone(),
            projeto: projeto.clone(),
            titulo: format!("#{id} {titulo}"),
            papel: Papel::Trabalhador,
            pai: Some(coordenador.id.clone()),
            tarefa: Some(id),
            sessao: uuid::Uuid::new_v4().to_string(),
            sessao_criada: false,
            cwd: caminho_projeto.clone(),
            modelo,
            modelo_atual: None,
            permissao: None,
            permitidas: Vec::new(),
            esforco: None,
            // O trabalhador usa a mesma conta escolhida para o coordenador.
            conta: coordenador.conta.clone(),
            contexto: None,
            janela: None,
            comandos: Vec::new(),
            criado_em: agora(),
            atualizado_em: agora(),
            custo_usd: 0.0,
            custo_da_sessao: None,
            fixado: false,
            arquivado: false,
            lido_em: agora(),
            turno_aberto: false,
            modo_antes_do_plano: None,
            itens: Vec::new(),
        });
        self.preparar_copia(id, Some(resposta), cx);
    }

    /// Cria a cópia isolada da tarefa (git worktree), copia os arquivos locais e roda a preparação
    /// do projeto; só então o trabalhador recebe o pedido. Sem `resposta` (a chamada criar_tarefa do
    /// coordenador), é uma preparação refeita depois de o Vitral fechar no meio.
    fn preparar_copia(&mut self, id: u64, resposta: Option<Sender<Resposta>>, cx: &mut Context<Self>) {
        let Some(tarefa) = self.tarefa(id).cloned() else {
            return;
        };
        if !self.preparando.insert(id) {
            return;
        }
        let caminho_projeto = self.caminho_projeto(&tarefa.projeto);
        // A cópia que ficou pela metade é desta tarefa, e o trabalhador nunca rodou nela.
        let antiga = (!tarefa.preparada)
            .then(|| tarefa.worktree.clone().zip(tarefa.branch.clone()))
            .flatten();
        let area = self.disco.pasta_worktrees(&tarefa.projeto);
        let destino = area.join(format!("t{id}"));
        let branch = format!("vitral/t{id}-{}", slug(&tarefa.titulo));
        let config_projeto = self
            .config
            .projetos
            .get(&tarefa.projeto)
            .cloned()
            .unwrap_or_default();
        if let Some(t) = self.tarefa_mut(id) {
            t.progresso = Some(
                if resposta.is_some() {
                    "preparando a cópia do projeto"
                } else {
                    "preparando a cópia de novo"
                }
                .into(),
            );
        }
        cx.spawn(async move |motor, cx| {
            let projeto = caminho_projeto.clone();
            let copia = cx
                .background_spawn(async move {
                    if !git::usa_git(&projeto) {
                        return None;
                    }
                    // Na vez do projeto: uma junção no meio mudaria o HEAD e disputaria o índice.
                    git::na_vez_do_projeto(&projeto, || {
                        if let Some((pasta, branch)) = antiga {
                            let _ = git::limpar_copia(&projeto, &pasta, &area, &branch, &[]);
                        }
                        Some(git::criar_worktree(&projeto, &destino, &branch))
                    })
                })
                .await;
            // Gravada antes da preparação, que demora: se o Vitral fechar, ela é apagada ao refazer.
            let pasta = match &copia {
                Some(Ok(c)) => {
                    let segue = motor.update(cx, |m, cx| {
                        if let Some(t) = m.tarefa_mut(id) {
                            t.worktree = Some(c.caminho.clone());
                            t.branch = Some(c.branch.clone());
                            t.base = Some(c.base.clone());
                        }
                        m.salvar();
                        cx.notify();
                        // Fechou ou trocou de pasta enquanto a cópia era criada: a preparação
                        // (npm install e afins) não roda, e é refeita ao abrir de novo.
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
                        let padroes = if config_projeto.copiar.is_empty() {
                            vec![".env*".to_string()]
                        } else {
                            config_projeto.copiar.clone()
                        };
                        let copiados =
                            git::copiar_arquivos_locais(&caminho_projeto, &pasta, &padroes);
                        let preparacao = Some(config_projeto.preparacao.trim().to_string())
                            .filter(|p| !p.is_empty())
                            .map(|comando| git::verificar(&pasta, &comando));
                        (copiados, preparacao)
                    })
                    .await
                }
                None => (Vec::new(), None),
            };
            let _ = motor.update(cx, |motor, cx| {
                if !motor.encerrado.load(Ordering::SeqCst) {
                    motor.copia_pronta(id, copia, copiados, preparacao, resposta, cx)
                }
            });
        })
        .detach();
        self.salvar();
        cx.notify();
    }

    /// Fim da preparação: o trabalhador recebe o pedido da tarefa (com o que chegou para ele
    /// enquanto isso) e o coordenador, a resposta de criar_tarefa.
    fn copia_pronta(
        &mut self,
        id: u64,
        copia: Option<anyhow::Result<git::CopiaIsolada>>,
        copiados: Vec<String>,
        preparacao: Option<Verificacao>,
        resposta: Option<Sender<Resposta>>,
        cx: &mut Context<Self>,
    ) {
        self.preparando.remove(&id);
        let Some(tarefa) = self.tarefa(id).cloned() else {
            return;
        };
        let trabalhador = tarefa.trabalhador.clone();
        let mut aviso = String::new();
        match copia {
            Some(Ok(copia)) => {
                if copia.sujo {
                    aviso.push_str(
                        " Atenção: o projeto tem mudanças sem commit, e a cópia da tarefa \
                         não as vê (ela parte do último commit).",
                    );
                }
                if let Some(c) = self.chat_mut(&trabalhador) {
                    c.cwd = copia.caminho.clone();
                }
                if let Some(t) = self.tarefa_mut(id) {
                    t.worktree = Some(copia.caminho);
                    t.branch = Some(copia.branch);
                    t.base = Some(copia.base);
                }
            }
            Some(Err(erro)) => {
                // Num projeto com git, o trabalhador nunca roda na pasta original.
                if let Some(t) = self.tarefa_mut(id) {
                    if t.estado.ativa() {
                        t.estado = EstadoTarefa::Falhou;
                    }
                    t.progresso = None;
                    t.parada = None;
                }
                self.avisar(
                    &trabalhador,
                    &format!("Não deu para criar a cópia isolada desta tarefa: {erro:#}"),
                    true,
                );
                let texto = format!(
                    "A tarefa #{id} \"{}\" não começou: não deu para criar a cópia isolada dela \
                     ({erro:#}). Ela ficou como falhou e o trabalhador não rodou; crie a tarefa de \
                     novo se ainda fizer sentido.",
                    tarefa.titulo
                );
                match resposta {
                    Some(resposta) => {
                        let _ = resposta.try_send(Resposta::erro(texto));
                    }
                    None => self.entregar(&tarefa.coordenador, Some("Vitral".into()), texto, cx),
                }
                self.salvar();
                self.retomar_entregas(cx);
                cx.notify();
                return;
            }
            None => aviso.push_str(
                " O projeto não usa git, então o trabalhador mexe direto na pasta do projeto: \
                 evite tarefas simultâneas nos mesmos arquivos.",
            ),
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
            // O pedido é do coordenador; o que chegou durante a preparação vai junto.
            let mut mensagens = vec![Entrada {
                de: Some("coordenador".into()),
                texto: primeira,
                anexos: Vec::new(),
                registrada: false,
                ultrathink: false,
                ultracode: false,
            }];
            mensagens.extend(self.filas.remove(&trabalhador).unwrap_or_default());
            self.comecar_turno(&trabalhador, mensagens, cx);
        }
        if let Some(resposta) = resposta {
            let texto = if ativa {
                let fila = if self.esperando_vaga(&trabalhador) {
                    " Ela está na fila e começa quando houver vaga."
                } else {
                    ""
                };
                format!(
                    "Tarefa #{id} criada; trabalhador {trabalhador} iniciado.{fila}{aviso} O relatório \
                     chega para você automaticamente quando ele terminar."
                )
            } else {
                format!(
                    "Tarefa #{id} criada, mas ficou {} antes de o trabalhador começar.",
                    tarefa.estado.nome()
                )
            };
            let _ = resposta.try_send(Resposta::ok(texto));
        }
        self.salvar();
        self.retomar_entregas(cx);
        cx.notify();
    }

    fn ver_diff(&mut self, id: u64, arquivos: Vec<String>, resposta: Sender<Resposta>, cx: &mut Context<Self>) {
        let tarefa = self.tarefa(id).cloned().unwrap();
        let projeto = self.caminho_projeto(&tarefa.projeto);
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
        let (Some(pasta), Some(base)) = (tarefa.worktree, tarefa.base) else {
            let _ = resposta.try_send(Resposta::ok(
                "Esta tarefa roda direto na pasta do projeto (sem git): leia os arquivos ou rode `git \
                 diff` se houver repositório.",
            ));
            return;
        };
        cx.background_spawn(async move {
            let base = git::base_efetiva(&projeto, &pasta, &base);
            let texto = git::diff_para_coordenador(&pasta, &projeto, &base, &arquivos, &apontados, 20_000)
                .unwrap_or_else(|erro| format!("Não deu para ler o diff: {erro}"));
            let _ = resposta.try_send(Resposta::ok(texto));
        })
        .detach();
    }

    pub fn diff_da_tarefa(&self, id: u64, cx: &mut Context<Self>) -> Task<Result<String, LeituraDiff>> {
        let tarefa = self.tarefa(id).cloned();
        let projeto = tarefa.as_ref().map(|t| self.caminho_projeto(&t.projeto));
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
            git::diff_completo(&pasta, &git::base_efetiva(&projeto, &pasta, &base))
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
        self.aprovar_tarefa(id, "Aprovada pelo usuário no Vitral.".into(), ressalva, avisa, cx);
        let coordenador = self.tarefa(id).map(|t| t.coordenador.clone());
        cx.spawn(async move |motor, cx| {
            let Ok(resposta) = aviso.recv().await else {
                return;
            };
            if let Some(coordenador) = coordenador
                && !resposta.erro
            {
                let texto = format!("O usuário aprovou a tarefa #{id} pelo painel. {}", resposta.texto);
                let _ = motor.update(cx, |m, cx| m.entregar(&coordenador, Some("Vitral".into()), texto, cx));
            }
            let _ = envia.send(resposta).await;
        })
        .detach();
        recebe
    }

    /// O usuário pede ajuste direto ao trabalhador (ex.: comentários na revisão das mudanças).
    pub fn ajuste_do_usuario(&mut self, id: u64, feedback: String, cx: &mut Context<Self>) -> Resposta {
        // Nada vai anexado: no Revisar, o usuário escolhe as sugestões do revisor com "Usar".
        let resposta = self.pedir_ajuste_de(id, feedback.clone(), "usuário", "", "", cx);
        if !resposta.erro
            && let Some(coordenador) = self.tarefa(id).map(|t| t.coordenador.clone())
        {
            let texto = format!(
                "O usuário pediu ajustes na tarefa #{id} direto ao trabalhador:\n{feedback}\nO novo \
                 relatório chega para você como sempre."
            );
            self.entregar(&coordenador, Some("Vitral".into()), texto, cx);
        }
        self.salvar();
        resposta
    }

    pub fn descartar_arquivo(&self, id: u64, arquivo: String, cx: &mut Context<Self>) -> Task<anyhow::Result<Option<String>>> {
        let tarefa = self.tarefa(id).cloned();
        let projeto = tarefa.as_ref().map(|t| self.caminho_projeto(&t.projeto));
        cx.background_spawn(async move {
            let tarefa = tarefa.ok_or_else(|| anyhow::anyhow!("tarefa não encontrada"))?;
            let (Some(pasta), Some(base), Some(projeto)) = (tarefa.worktree, tarefa.base, projeto) else {
                anyhow::bail!("a tarefa não tem cópia isolada");
            };
            // Volta ao que está no projeto, não à base antiga (que desfaria o que outra tarefa juntou).
            git::descartar_arquivo(&pasta, &git::base_efetiva(&projeto, &pasta, &base), &arquivo)
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
                anyhow::bail!("a cópia desta tarefa já foi apagada");
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
                t.copia_apagada() || (t.worktree.is_none() && chat.cwd != self.caminho_projeto(&t.projeto))
            })
    }

    /// Tarefas cuja cópia ainda existe e já não vai ser juntada: as encerradas e, com `revisao`,
    /// também as em revisão (que viram canceladas ao limpar).
    /// A cópia parada da tarefa já não vai ser juntada (a tarefa encerrou ou o coordenador foi
    /// arquivado): dá para apagar.
    pub fn pode_apagar_copia(&self, id: u64) -> bool {
        self.tarefa(id).is_some_and(|t| {
            self.copias_paradas(true).any(|p| p.id == id)
                && (t.estado.encerrada() || self.chat(&t.coordenador).is_some_and(|c| self.arquivada(c)))
        })
    }

    pub fn copias_paradas<'a>(&'a self, revisao: bool) -> impl Iterator<Item = &'a Tarefa> + 'a {
        self.tarefas.iter().filter(move |t| {
            t.worktree.is_some()
                && !t.copia_limpa
                && (t.estado.encerrada() || (revisao && t.estado == EstadoTarefa::EmRevisao))
                // Em revisão com progresso: a verificação e o revisor ainda rodam na cópia.
                && !(t.estado == EstadoTarefa::EmRevisao && t.progresso.is_some())
                && !self.processos.contains_key(&t.trabalhador)
                && !self.esperando_vaga(&t.trabalhador)
                && !self.retentativas.contains_key(&t.trabalhador)
                && !self.juntando.contains(&t.id)
                && !self.verificando.contains_key(&t.id)
                && !self.preparando.contains(&t.id)
        })
    }

    /// Apaga a pasta, o branch e os pontos de volta das cópias das tarefas paradas. As em revisão
    /// viram canceladas: sem cópia não há o que juntar. Devolve quantas apagou e os erros.
    pub fn limpar_copias(&mut self, ids: &[u64], cx: &mut Context<Self>) -> Task<(usize, Vec<String>)> {
        let lote: Vec<(u64, PathBuf, PathBuf, PathBuf, String, Vec<String>)> = self
            .copias_paradas(true)
            .filter(|t| ids.contains(&t.id))
            .filter_map(|t| {
                let (pasta, branch) = (t.worktree.clone()?, t.branch.clone()?);
                let pontos = self.pontos_so_do_chat(&t.trabalhador);
                let area = self.disco.pasta_worktrees(&t.projeto);
                Some((t.id, self.caminho_projeto(&t.projeto), pasta, area, branch, pontos))
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
            let resultados: Vec<(u64, anyhow::Result<()>)> = cx
                .background_spawn(async move {
                    lote.into_iter()
                        .map(|(id, projeto, pasta, area, branch, pontos)| {
                            // Na vez do projeto: uma junção ou cópia nova no meio disputaria o índice.
                            let limpou = git::na_vez_do_projeto(&projeto, || {
                                git::limpar_copia(&projeto, &pasta, &area, &branch, &pontos)
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
                        let Some(t) = m.tarefa_mut(id) else {
                            continue;
                        };
                        if let Err(erro) = resultado {
                            t.copia_limpa = false;
                            erros.push(format!("#{id}: {erro:#}"));
                            continue;
                        }
                        apagadas += 1;
                        t.worktree = None;
                        t.branch = None;
                        t.progresso = None;
                        if !t.estado.encerrada() {
                            t.estado = EstadoTarefa::Cancelada;
                        }
                        let trabalhador = t.trabalhador.clone();
                        m.avisar(&trabalhador, "A cópia desta tarefa foi apagada; esta conversa não roda mais.", false);
                        if let Some(c) = m.chat(&trabalhador) {
                            m.disco.salvar_chat(c);
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
        let mut projetos: Vec<PathBuf> = self.tarefas.iter().map(|t| self.caminho_projeto(&t.projeto)).collect();
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
        cx.spawn(async move |motor, cx| {
            cx.background_spawn(async move {
                for projeto in projetos {
                    git::na_vez_do_projeto(&projeto, || git::podar(&projeto));
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

    /// Grava a conversa em markdown (em `.vitral/exportadas`) para outro chat ler como contexto.
    pub fn exportar_conversa(&self, id: &str) -> anyhow::Result<PathBuf> {
        let chat = self.chat(id).ok_or_else(|| anyhow::anyhow!("conversa não encontrada"))?;
        let mut texto = format!("# Conversa \"{}\" (projeto {})\n\n", chat.titulo, chat.projeto);
        for item in &chat.itens {
            match item {
                Item::Usuario { texto: t, .. } => texto.push_str(&format!("## Usuário\n\n{t}\n\n")),
                Item::Assistente { texto: t } => texto.push_str(&format!("## Claude\n\n{t}\n\n")),
                Item::Entrega { de, texto: t, .. } => {
                    texto.push_str(&format!("## Mensagem de {de}\n\n{t}\n\n"))
                }
                Item::Ferramenta { resumo, .. } => texto.push_str(&format!("- (ferramenta) {resumo}\n")),
                Item::Aviso { .. } => {}
            }
        }
        let pasta = self.disco.pasta().join("exportadas");
        std::fs::create_dir_all(&pasta)?;
        let arquivo = pasta.join(format!("{}-{}.md", slug(&chat.titulo), chat.id));
        std::fs::write(&arquivo, texto)?;
        Ok(arquivo)
    }

    fn rodar_rotinas(&mut self, cx: &mut Context<Self>) {
        let (dia, minutos, hoje) = crate::modelo::agora_local();
        let na_hora: Vec<usize> = self
            .config
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
            self.config.rotinas[i].ultima = hoje.clone();
            let rotina = self.config.rotinas[i].clone();
            if !self.config.projeto_arquivado(&rotina.projeto) {
                self.rodar_rotina(&rotina, cx);
            }
        }
        self.disco.salvar_config(&self.config);
    }

    /// Abre um coordenador novo no projeto da rotina com o pedido dela.
    pub fn rodar_rotina(&mut self, rotina: &crate::modelo::Rotina, cx: &mut Context<Self>) -> String {
        let id = self.novo_coordenador(&rotina.projeto, cx);
        if let Some(c) = self.chat_mut(&id) {
            c.titulo = format!("Rotina: {}", titulo_de(&rotina.texto));
        }
        self.enviar_do_usuario(&id, rotina.texto.clone(), Vec::new(), Reforco::default(), cx);
        id
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

    /// Ajuste do coordenador: vão junto os problemas do revisor e a saída da verificação desta
    /// entrega, para ele não ter de copiar (e resumir) tudo no feedback.
    fn pedir_ajuste(
        &mut self,
        id: u64,
        feedback: String,
        incluir: Option<Vec<String>>,
        ignorar: Vec<usize>,
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
        self.pedir_ajuste_de(id, feedback, "coordenador", &anexo, &junto, cx)
    }

    fn pedir_ajuste_de(
        &mut self,
        id: u64,
        feedback: String,
        quem: &str,
        anexo: &str,
        junto: &str,
        cx: &mut Context<Self>,
    ) -> Resposta {
        let maximo = self.config.max_rodadas;
        let juntando = self.juntando.contains(&id);
        let Some(tarefa) = self.tarefa_mut(id) else {
            return Resposta::erro("Tarefa não encontrada.");
        };
        if tarefa.estado.encerrada() {
            return Resposta::erro(format!("A tarefa #{id} já está {}.", tarefa.estado.nome()));
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
        let rodada = tarefa.rodada;
        let trabalhador = tarefa.trabalhador.clone();
        self.tentativas_auto.remove(&trabalhador);
        let anexo = if anexo.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", anexo.trim_end())
        };
        let mensagem = format!(
            "Ajuste pedido pelo {quem} (rodada {rodada}):\n{feedback}{anexo}\n\nQuando terminar, mande \
             um novo relatório."
        );
        self.entregar(&trabalhador, Some("coordenador".into()), mensagem, cx);
        if rodada > maximo {
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

    fn cancelar_tarefa(&mut self, id: u64, motivo: String, cx: &mut Context<Self>) -> Resposta {
        if self.juntando.contains(&id) {
            return Resposta::erro(format!(
                "A tarefa #{id} está sendo juntada no projeto agora; espere o resultado."
            ));
        }
        let Some(tarefa) = self.tarefa_mut(id) else {
            return Resposta::erro("Tarefa não encontrada.");
        };
        if tarefa.estado == EstadoTarefa::Aprovada {
            return Resposta::erro(format!("A tarefa #{id} já foi aprovada e juntada."));
        }
        tarefa.estado = EstadoTarefa::Cancelada;
        tarefa.progresso = None;
        let trabalhador = tarefa.trabalhador.clone();
        self.parar(&trabalhador, cx);
        let motivo = if motivo.is_empty() {
            "sem motivo informado".into()
        } else {
            motivo
        };
        self.avisar(
            &trabalhador,
            &format!("Tarefa cancelada pelo coordenador: {motivo}"),
            false,
        );
        Resposta::ok(format!(
            "Tarefa #{id} cancelada. A cópia dela fica guardada até o usuário limpar (painel Equipe)."
        ))
    }

    /// Junta a cópia da tarefa no projeto. `ressalva` é o que o coordenador aprovou mesmo assim
    /// (verificação falhando, problema grave): vai no commit e aparece para o usuário.
    fn aprovar_tarefa(
        &mut self,
        id: u64,
        nota: String,
        ressalva: Option<String>,
        resposta: Sender<Resposta>,
        cx: &mut Context<Self>,
    ) {
        if let Some(recusa) = self.recusa_aprovar(id) {
            let _ = resposta.try_send(Resposta::erro(recusa));
            return;
        }
        let Some(tarefa) = self.tarefa(id).cloned() else {
            return;
        };
        if tarefa.copia_apagada() {
            let _ = resposta.try_send(Resposta::erro(format!(
                "A tarefa #{id} está {} e a cópia dela já foi apagada: não há o que juntar.",
                tarefa.estado.nome()
            )));
            return;
        }
        let (Some(pasta), Some(branch), Some(base)) = (
            tarefa.worktree.clone(),
            tarefa.branch.clone(),
            tarefa.base.clone(),
        ) else {
            self.marcar_aprovada(id, ressalva);
            let _ = resposta.try_send(Resposta::ok(format!(
                "Tarefa #{id} aprovada. Ela rodou direto na pasta do projeto, então não há o que juntar."
            )));
            self.salvar();
            cx.notify();
            return;
        };
        let projeto = self.caminho_projeto(&tarefa.projeto);
        let area = self.disco.pasta_worktrees(&tarefa.projeto);
        let mut mensagem = if nota.is_empty() {
            format!("{} (tarefa #{id})", tarefa.titulo)
        } else {
            format!("{}\n\n{nota}\n\nTarefa #{id} do Vitral", tarefa.titulo)
        };
        if let Some(ressalva) = &ressalva {
            mensagem.push_str(&format!("\n\nAprovada apesar de: {ressalva}"));
        }
        let lembrete = if ressalva.is_some() {
            " A ressalva ficou no commit e o usuário foi avisado; cite-a no RESUMO."
        } else {
            ""
        };
        self.juntando.insert(id);
        cx.notify();
        let pontos = self.pontos_so_do_chat(&tarefa.trabalhador);
        cx.spawn(async move |motor, cx| {
            let projeto_bg = projeto.clone();
            let resultado = cx
                .background_spawn(async move {
                    // Uma junção por vez no projeto, e nenhuma cópia nova sendo criada no meio.
                    git::na_vez_do_projeto(&projeto_bg, || {
                        let juncao = git::aprovar(&projeto_bg, &pasta, &branch, &base, &mensagem);
                        if matches!(juncao, Ok(git::Juncao::Juntou { .. } | git::Juncao::SemMudancas)) {
                            let _ = git::limpar_copia(&projeto_bg, &pasta, &area, &branch, &pontos);
                        }
                        juncao
                    })
                })
                .await;
            let _ = motor.update(cx, |motor, cx| {
                motor.juntando.remove(&id);
                let texto = match resultado {
                    Ok(git::Juncao::Juntou { commits }) => {
                        motor.marcar_aprovada(id, ressalva);
                        Resposta::ok(format!(
                            "Tarefa #{id} aprovada e juntada em {}:\n{commits}{lembrete}",
                            projeto.display()
                        ))
                    }
                    Ok(git::Juncao::SemMudancas) => {
                        motor.marcar_aprovada(id, ressalva);
                        Resposta::ok(format!(
                            "Tarefa #{id} aprovada; ela não tinha mudanças para juntar.{lembrete}"
                        ))
                    }
                    Ok(git::Juncao::ProjetoSujo { arquivos }) => Resposta::erro(format!(
                        "Não juntei a tarefa #{id}: a pasta do projeto ({}) tem mudanças sem commit \
                         em: {}. Isso não é conflito entre tarefas. Se foi um trabalhador que escreveu \
                         fora da cópia dele, avise o usuário; não peça ao trabalhador para mexer na \
                         pasta do projeto. O usuário decide se faz commit, stash ou descarta essas \
                         mudanças.",
                        projeto.display(),
                        arquivos.join(", ")
                    )),
                    Ok(git::Juncao::SoltosNoProjeto { arquivos }) => Resposta::erro(format!(
                        "Não juntei a tarefa #{id}: ela cria arquivos que já existem soltos (fora do \
                         git) na pasta do projeto ({}): {}. O git não sobrescreve esses arquivos. \
                         Avise o usuário: ele decide se apaga ou move esses arquivos do projeto (ou faz \
                         commit deles); depois aprove de novo. Não peça ao trabalhador para mexer na \
                         pasta do projeto.",
                        projeto.display(),
                        arquivos.join(", ")
                    )),
                    Ok(git::Juncao::Conflito {
                        arquivos,
                        detalhe,
                        alvo,
                    }) => Resposta::erro(format!(
                        "Conflito com o que já entrou no projeto; nada foi juntado e o projeto não foi \
                         tocado. Arquivos: {}. Detalhe: {detalhe}\nPeça ao trabalhador (pedir_ajuste) \
                         para rodar `git merge {alvo}` na cópia dele, resolver os conflitos mantendo as \
                         duas mudanças, fazer commit e rodar os testes. Depois aprove de novo.",
                        if arquivos.is_empty() {
                            "?".into()
                        } else {
                            arquivos.join(", ")
                        }
                    )),
                    Ok(git::Juncao::Ocupado { detalhe }) => Resposta::erro(format!(
                        "Nada foi juntado: outro git estava usando a pasta ({}). Não é conflito, então \
                         não peça ajuste ao trabalhador: aprove de novo em instantes. Se continuar, \
                         pode ser um .git/index.lock que sobrou de um git que caiu; avise o usuário.",
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

    /// A tarefa entrou no projeto: com ressalva, o coordenador vê na conversa o que foi ignorado.
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
            self.avisar(
                &coordenador,
                &format!("A tarefa #{id} foi aprovada apesar de: {ressalva}"),
                true,
            );
        }
    }

    // ---------- textos ----------

    fn texto_chats(&self) -> String {
        let mut texto = String::new();
        for c in self.chats.iter().filter(|c| !self.arquivada(c)) {
            let papel = match c.papel {
                Papel::Coordenador => "coordenador",
                Papel::Trabalhador => "trabalhador",
            };
            let estado = if self.execucao(&c.id).rodando() {
                "rodando"
            } else {
                "parado"
            };
            texto.push_str(&format!(
                "- {} | projeto {} | {papel} | {} | {estado}\n",
                c.id, c.projeto, c.titulo
            ));
        }
        if texto.is_empty() {
            texto.push_str("Nenhum chat.");
        }
        texto
    }

    fn texto_tarefas(&self, coordenador: &str) -> String {
        let mut texto = String::new();
        for t in self.tarefas_do(coordenador) {
            let verificacao = match &t.ultima_verificacao {
                Some(v) if v.passou => " | verificação passou",
                Some(_) => " | verificação FALHOU",
                None => "",
            };
            let estado = match &t.parada {
                Some(motivo) if self.tarefa_parada(t) => {
                    format!("parada ({}; volta quando o usuário retomar)", motivo_para_o_claude(motivo))
                }
                _ => t.estado.nome().to_string(),
            };
            let ressalva = if t.ressalva.is_some() { " com ressalva" } else { "" };
            texto.push_str(&format!(
                "- #{} {} | {estado}{ressalva} | rodada {}{verificacao} | trabalhador {}\n",
                t.id, t.titulo, t.rodada, t.trabalhador
            ));
        }
        if texto.is_empty() {
            texto.push_str("Nenhuma tarefa ainda.");
        }
        texto
    }

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
        let titulo = format!("{} · {}", chat.projeto, chat.titulo);
        let corpo = texto.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().to_string();
        let (avisos, sinais, id) = (self.avisos.clone(), self.sinais.clone(), chat.id.clone());
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
                let filho = std::process::Command::new("notify-send")
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
        self.disco.salvar_estado(&EstadoRef {
            chats: &self.chats,
            tarefas: &self.tarefas,
            proxima_tarefa: self.proxima_tarefa,
            filas: &self.filas,
        });
    }

    /// Grava as mensagens dos chats que mudaram desde a última passada.
    fn gravar_sujos(&mut self) {
        for id in std::mem::take(&mut self.chats_sujos) {
            if let Some(c) = self.chat(&id) {
                self.disco.salvar_chat(c);
            }
        }
    }
}

/// Lista de textos de um argumento MCP; aceita também um texto só, ou a lista escrita como JSON.
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

fn slug(texto: &str) -> String {
    let mut s = String::new();
    for c in texto.to_lowercase().chars() {
        let c = match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    s.trim_matches('-').chars().take(30).collect()
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
                "O Vitral fechou enquanto a cópia desta tarefa era preparada. Ela é preparada de \
                 novo agora, e o pedido vai para o trabalhador quando ficar pronta.",
            ));
        } else if tarefa.estado.ativa() && tarefa.parada.is_none() {
            let comecou = trabalhador.is_some_and(recebeu_mensagem);
            let na_fila = filas.get(&tarefa.trabalhador).is_some_and(|f| !f.is_empty());
            let (motivo, aviso) = if comecou {
                (
                    PARADA_FECHOU,
                    "O Vitral foi fechado no meio desta tarefa. Use Retomar (ou mande uma mensagem) \
                     para continuar; o relatório vai para o coordenador quando terminar.",
                )
            } else if na_fila {
                (
                    "esperava vaga quando o Vitral fechou",
                    "Esta tarefa esperava vaga quando o Vitral fechou e ainda não começou. Use \
                     Retomar para começar.",
                )
            } else {
                (
                    "o Vitral fechou durante a preparação da cópia",
                    "O Vitral fechou enquanto a cópia desta tarefa era preparada, e o trabalhador \
                     não recebeu o pedido. Use Retomar para mandar.",
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
        if chat.tarefa.is_some_and(|t| ativas.contains(&t)) {
            // A parada da tarefa já cobre o turno cortado do trabalhador.
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
/// versões antigas) o pedido nunca foi nem está na fila.
fn precisa_preparar(tarefa: &Tarefa, trabalhador: &Chat, filas: &Filas) -> bool {
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

/// O que sai com o projeto, para a pergunta antes da lixeira.
#[derive(Clone, Debug)]
pub struct ResumoDoProjeto {
    pub conversas: usize,
    pub trabalhadores: usize,
    /// As cópias das tarefas que ainda estão no disco.
    pub copias: Vec<PathBuf>,
    pub rotinas: usize,
}

/// O resultado da lixeira: se havia pasta, quantas conversas saíram e as cópias que não foram
/// para a Lixeira ("caminho: motivo").
#[derive(Clone, Debug, PartialEq)]
pub struct NaLixeira {
    pub pasta: bool,
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
/// coordenador dele) e os trabalhadores dessas tarefas, na ordem do estado.
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

/// Tira do estado tudo o que é do projeto (`do_projeto`), as filas dessas conversas, as rotinas
/// e a configuração do projeto. O resto fica como estava.
pub fn tirar_do_estado(
    chats: &mut Vec<Chat>,
    tarefas: &mut Vec<Tarefa>,
    filas: &mut Filas,
    config: &mut Config,
    projeto: &str,
) -> Retirado {
    let (conversas, ids) = do_projeto(chats, tarefas, projeto);
    let copias = tarefas.iter().filter(|t| ids.contains(&t.id)).filter_map(|t| t.worktree.clone()).collect();
    chats.retain(|c| !conversas.contains(&c.id));
    tarefas.retain(|t| !ids.contains(&t.id));
    filas.retain(|id, _| !conversas.contains(id));
    let antes = config.rotinas.len();
    config.rotinas.retain(|r| r.projeto != projeto);
    let rotinas = antes - config.rotinas.len();
    config.projetos.remove(projeto);
    Retirado { chats: conversas, tarefas: ids, copias, rotinas }
}

/// A pasta que vai para a lixeira: uma subpasta direta da pasta de trabalho, com um nome que a
/// lista aceitaria. Nunca a própria pasta de trabalho, nem a `.vitral`.
pub fn pasta_para_lixeira(raiz: &Path, projeto: &str) -> Result<PathBuf, String> {
    let nome = Motor::nome_de_projeto(projeto).map_err(|e| e.to_string())?;
    if nome != projeto || matches!(nome.as_str(), "." | "..") || nome.contains('\\') {
        return Err(format!("“{projeto}” não é um nome de projeto"));
    }
    let pasta = raiz.join(&nome);
    if pasta.parent() != Some(raiz) || pasta == raiz {
        return Err(format!("“{projeto}” não fica dentro da pasta de trabalho"));
    }
    Ok(pasta)
}

/// O que o arquivamento automático olha no próprio chat: coordenador à vista, não fixado e
/// sem uso desde `limite`.
fn candidata_a_arquivar(c: &Chat, limite: u64) -> bool {
    c.papel == Papel::Coordenador && !c.arquivado && !c.fixado && c.atualizado_em.max(c.lido_em) < limite
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
                },
            }
        })
        .collect()
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
        registrada: false,
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
        return Some("só nos trabalhadores".into());
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

/// "Terminou: 3 tarefas aprovadas"; sem tarefas, a primeira linha da resposta.
fn texto_do_fim(aprovadas: usize, resposta: &str) -> String {
    if aprovadas > 0 {
        let plural = if aprovadas == 1 { "tarefa aprovada" } else { "tarefas aprovadas" };
        return format!("Terminou: {aprovadas} {plural}");
    }
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
        Chat {
            id: id.into(),
            projeto: "p".into(),
            titulo: id.into(),
            papel,
            pai: None,
            tarefa,
            sessao: "s".into(),
            sessao_criada: true,
            cwd: PathBuf::from("/tmp"),
            modelo: None,
            modelo_atual: None,
            permissao: None,
            permitidas: Vec::new(),
            esforco: None,
            conta: None,
            contexto: None,
            janela: None,
            comandos: Vec::new(),
            criado_em: 0,
            atualizado_em: 0,
            custo_usd: 0.0,
            custo_da_sessao: None,
            fixado: false,
            arquivado: false,
            lido_em: 0,
            turno_aberto: false,
            modo_antes_do_plano: None,
            itens: Vec::new(),
        }
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
                ultrathink: false,
                ultracode: false,
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
                ultrathink: false,
                ultracode: false,
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
    fn coordenador_cortado_ganha_aviso_uma_vez() {
        let mut coordenador = chat("c", Papel::Coordenador, None);
        coordenador.turno_aberto = true;
        let mut chats = vec![coordenador, chat("c2", Papel::Coordenador, None)];
        let mut tarefas = Vec::new();
        marcar_interrompidas(&mut chats, &mut tarefas, &Filas::new());
        marcar_interrompidas(&mut chats, &mut tarefas, &Filas::new());
        assert_eq!(avisos(&chats[0]), 1);
        assert_eq!(avisos(&chats[1]), 0);
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
            ultrathink: false,
            ultracode: false,
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
        assert_eq!(motivo_sem_ultracode(Papel::Coordenador, "opus", versao).as_deref(), Some("só nos trabalhadores"));
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
        assert_eq!(texto_do_fim(3, "pronto"), "Terminou: 3 tarefas aprovadas");
        assert_eq!(texto_do_fim(1, ""), "Terminou: 1 tarefa aprovada");
        assert_eq!(texto_do_fim(0, "\n  Feito: o bug sumiu.\nresto"), "Terminou: Feito: o bug sumiu.");
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
    fn lixeira_tira_do_estado_so_o_que_e_do_projeto() {
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
        ];
        let mut tarefas = vec![
            tarefa_de(1, "loja", "c1", "w1", Some("/v/.vitral/worktrees/loja/t1")),
            tarefa_de(2, "site", "c2", "w2", Some("/v/.vitral/worktrees/site/t2")),
            tarefa_de(3, "loja", "c1", "w3", None),
        ];
        let mut filas = Filas::new();
        for id in ["c1", "w1", "c2"] {
            filas.insert(id.into(), Vec::new());
        }
        let mut config = Config::default();
        config.projetos.insert("loja".into(), Default::default());
        config.projetos.insert("site".into(), Default::default());
        for (id, projeto) in [("r1", "loja"), ("r2", "site"), ("r3", "loja")] {
            config.rotinas.push(crate::modelo::Rotina { id: id.into(), projeto: projeto.into(), ..Default::default() });
        }

        let retirado = tirar_do_estado(&mut chats, &mut tarefas, &mut filas, &mut config, "loja");
        assert_eq!(retirado.chats, ["c1", "w1", "w3"]);
        assert_eq!(retirado.tarefas, [1, 3]);
        assert_eq!(retirado.copias, [PathBuf::from("/v/.vitral/worktrees/loja/t1")]);
        assert_eq!(retirado.rotinas, 2);
        let ids: Vec<&str> = chats.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["c2", "w2"]);
        assert_eq!(tarefas.iter().map(|t| t.id).collect::<Vec<_>>(), [2]);
        assert_eq!(filas.keys().collect::<Vec<_>>(), ["c2"]);
        assert_eq!(config.rotinas.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["r2"]);
        assert!(!config.projetos.contains_key("loja") && config.projetos.contains_key("site"));

        // Um projeto sem nada no estado não mexe em nada.
        let antes = (chats.len(), tarefas.len(), config.rotinas.len());
        assert_eq!(tirar_do_estado(&mut chats, &mut tarefas, &mut filas, &mut config, "nada"), Retirado::default());
        assert_eq!((chats.len(), tarefas.len(), config.rotinas.len()), antes);
    }

    #[test]
    fn lixeira_so_aceita_uma_subpasta_direta_da_pasta_de_trabalho() {
        let raiz = Path::new("/home/v/Projetos");
        assert_eq!(pasta_para_lixeira(raiz, "loja"), Ok(PathBuf::from("/home/v/Projetos/loja")));
        assert_eq!(pasta_para_lixeira(raiz, "minha loja"), Ok(PathBuf::from("/home/v/Projetos/minha loja")));
        for ruim in ["", " ", "..", ".", ".vitral", "a/b", "../fora", "/etc", " loja", "loja "] {
            assert!(pasta_para_lixeira(raiz, ruim).is_err(), "{ruim:?}");
        }
    }

    #[test]
    fn ocupacao_livre_so_sem_conversa_nem_tarefa() {
        assert!(Ocupacao::default().livre());
        assert!(!Ocupacao { conversas: vec!["c1".into()], tarefas: Vec::new() }.livre());
        assert!(!Ocupacao { conversas: Vec::new(), tarefas: vec![4] }.livre());
    }
}
