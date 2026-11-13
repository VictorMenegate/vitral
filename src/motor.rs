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
        Agente, Chat, Config, Entrada, Estado, EstadoRef, Execucao, Filas, Item, Ocorrencia, Reforco, agora, novo_id,
    },
    prompts,
};

const CORTADO: &str = "o Vitral foi fechado no meio do turno";

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
    interrompidos: Vec<String>,
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
    /// Se a conta tem os fluxos de agentes (a ferramenta `Workflow`): `false` só depois de um
    /// turno com Ultracode em que eles não vieram. Não trava a opção (a pessoa pode ter mudado a
    /// configuração da conta): só a avisa em âmbar.
    fluxos_por_conta: HashMap<String, bool>,
    /// Fluxos e subagentes que o processo atual de cada chat ainda roda em segundo plano.
    fundo: HashMap<String, usize>,
    /// Quando cada chat começou o bloco de raciocínio em curso (para dizer quanto durou).
    pensando: HashMap<String, std::time::Instant>,
    /// O `result` que chegou com agentes ainda em segundo plano (texto e custo): o turno só
    /// termina quando eles acabam, ou quando o processo sai.
    fim_adiado: HashMap<String, (String, f64)>,
    /// As mensagens que o usuário dirigiu ao turno de cada chat e que o Claude Code ainda pode ter
    /// esperando na fila interna dele (ver `Processo::dirigir`): saem daqui quando ele as absorve
    /// (a primeira volta de ferramenta) ou quando abre outro turno com elas.
    dirigidas: HashMap<String, Vec<Entrada>>,
    /// Quantas mensagens o usuário dirigiu ao turno que está rodando em cada chat, para a faixa
    /// dizer que ele foi dirigido. Zera a cada turno novo.
    dirigidos: HashMap<String, usize>,
    pub janela_ativa: bool,
    /// A conversa que o clique numa notificação pediu para abrir; a Janela consome.
    pub pedido_de_abrir: Option<String>,
    /// As notificações na tela, por chat.
    #[cfg_attr(windows, allow(dead_code))]
    avisos: Arc<std::sync::Mutex<HashMap<String, Aviso>>>,
    /// Chats que algum painel aberto mostra (a Janela atualiza a cada quadro): o texto parcial
    /// dos outros não redesenha a janela.
    pub visiveis: HashSet<String>,
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
        let mut motor = Self {
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
            interrompidos: Vec::new(),
            chats_sujos: HashSet::new(),
            repetidos_sessao: HashSet::new(),
            passagens: HashMap::new(),
            ciclos: Ciclos::default(),
            resumo_volta: None,
            permissoes: Vec::new(),
            versao_claude: None,
            atualizando_claude: false,
            resposta_do_claude: None,
            fluxos_por_conta: HashMap::new(),
            fundo: HashMap::new(),
            pensando: HashMap::new(),
            fim_adiado: HashMap::new(),
            dirigidas: HashMap::new(),
            dirigidos: HashMap::new(),
            janela_ativa: true,
            pedido_de_abrir: None,
            avisos: Arc::default(),
            visiveis: HashSet::new(),
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

    pub fn chat(&self, id: &str) -> Option<&Chat> {
        self.chats.iter().find(|c| c.id == id)
    }

    /// Mexe no registro do subagente aberto pela chamada `pai` (Task/Agent) do chat, criando-o
    /// na primeira vez. Chamada que não existe (ou que não é do chat) não faz nada.
    fn no_subagente(&mut self, chat: &str, pai: &str, f: impl FnOnce(&mut crate::modelo::Subagente)) {
        let Some(c) = self.chat_mut(chat) else {
            return;
        };
        let achado = c.itens.iter_mut().rev().find_map(|item| match item {
            Item::Ferramenta { id, sub, .. } if id == pai => Some(sub),
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

    pub fn rodando(&self) -> impl Iterator<Item = &Chat> {
        self.chats
            .iter()
            .filter(|c| self.execucao(&c.id).rodando())
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
        let mut chats: HashSet<&str> = self.rodando().map(|c| c.id.as_str()).collect();
        if com_fila {
            chats.extend(self.chats.iter().map(|c| c.id.as_str()).filter(|id| self.na_fila(id) > 0));
        }
        chats.len()
    }

    /// Por que o chat parou no meio, quando dá para retomar.
    pub fn parada(&self, chat_id: &str) -> Option<String> {
        let chat = self.chat(chat_id)?;
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
        let ativo = |id: &str| self.chat(id).is_some_and(|c| !c.arquivado);
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
        !self.execucao(id).rodando() && !self.processos.contains_key(id)
    }

    /// Esquece o que o motor guarda em memória das conversas que saíram do estado. Um
    /// processo que tenha aberto depois da conferência sai junto; um pedido de permissão é negado.
    fn esquecer(&mut self, chats: &[String]) {
        let saiu = |id: &String| chats.contains(id);
        for id in chats {
            if let Some(mut processo) = self.processos.remove(id) {
                processo.matar();
            }
            self.execucao.remove(id);
            self.geracao_atual.remove(id);
            self.ultima_entrada.remove(id);
            self.conta_do_turno.remove(id);
            self.parcial.remove(id);
            self.chats_sujos.remove(id);
            self.repetidos_sessao.remove(id);
            self.passagens.remove(id);
            self.fundo.remove(id);
            self.fim_adiado.remove(id);
            self.dirigidas.remove(id);
            self.dirigidos.remove(id);
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
        // O usuário falou: os ciclos de mensagens automáticas desse chat recomeçam.
        self.ciclos.zerar(chat);
        let entrada = entrada_do_usuario(texto, anexos, reforco);
        // Com o turno rodando, a mensagem do usuário entra nele (o "Mandar depois" é que escolhe a
        // fila). Só a dele: o que vem de outro chat continua esperando a vez.
        if !reforco.depois {
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
        // descarta (ele a rodaria como turno novo, depois do Parar), então o processo sai inteiro e
        // ela volta para a fila do Vitral.
        let dirigida_esperando = self.dirigidas.get(chat).is_some_and(|d| !d.is_empty());
        if let Some(processo) = self.processos.get_mut(chat) {
            // Esperando os agentes em segundo plano, não há turno para interromper: o processo
            // sai com eles (SIGTERM em tudo que ele abriu).
            if self.fim_adiado.contains_key(chat) || dirigida_esperando {
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
        if self.chat(chat).is_some_and(|c| !c.arquivado) {
            self.entregar_fila(chat, cx);
        }
        self.salvar();
        cx.notify();
    }

    /// O chat que nunca rodou: sem sessão, só com os avisos do Vitral, sem fila e sem nada
    /// trabalhando.
    pub fn chat_vazio(&self, chat: &str) -> bool {
        self.chat(chat).is_some_and(nunca_rodou) && self.filas.get(chat).is_none_or(Vec::is_empty) && self.chat_livre(chat)
    }

    /// "Arquivar" no menu do chat: o chat vazio sai do Vitral em vez de ir para Arquivadas (com a
    /// pasta do chat sem pasta, se ela continua vazia). Devolve se apagou.
    pub fn arquivar_chat(&mut self, chat: &str, cx: &mut Context<Self>) -> bool {
        if !self.chat_vazio(chat) {
            if self.chat(chat).is_some_and(|c| !c.arquivado) {
                self.alternar_arquivado(chat, cx);
            }
            return false;
        }
        let cwd = self.chat(chat).map(|c| c.cwd.clone());
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

    /// Para todos os processos (ao fechar a janela).
    pub fn encerrar_tudo(&mut self) {
        for (_, mut processo) in self.processos.drain() {
            processo.matar();
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
        if let Some(fila) = self.filas.remove(chat) {
            self.comecar_turno(chat, fila, cx);
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
        // Esperando os agentes do fundo não há turno a dirigir (a entrada já está fechada), e um
        // turno sendo parado está indo embora.
        if self.fim_adiado.contains_key(chat_id) || self.interrompidos.iter().any(|c| c == chat_id) {
            return Dirigido::NaFila(None);
        }
        if !matches!(self.execucao(chat_id), Execucao::Rodando { .. }) {
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
        let mensagem = Mensagem { texto: &texto, imagens: &imagens };
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
            c.itens.extend(itens_das_entradas(vec![entrada.clone()]));
            c.atualizado_em = agora();
        }
        if let Some(c) = self.chat(chat_id) {
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

    /// O turno que roda neste chat recebe mensagem agora (o Enviar e o "Mandar agora" entram nele).
    pub fn turno_recebe_mensagem(&self, chat: &str) -> bool {
        self.somente_leitura.is_none()
            && matches!(self.execucao(chat), Execucao::Rodando { .. })
            && !self.fim_adiado.contains_key(chat)
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

    /// Tira da conversa a última mensagem do usuário e o que veio depois dela, e devolve o que
    /// estava nela para a caixa. A sessão do Claude Code não volta atrás: ela é trocada por uma
    /// nova, e o próximo turno começa com o bloco de passagem do que ficou na tela — senão o
    /// Claude continuaria lembrando da mensagem que saiu. Os arquivos da pasta não voltam.
    pub fn retirar_ultima_mensagem(&mut self, chat_id: &str, cx: &mut Context<Self>) -> Option<Retirada> {
        if self.somente_leitura.is_some() || self.processos.contains_key(chat_id) {
            return None;
        }
        let ix = indice_editavel(&self.chat(chat_id)?.itens)?;
        let c = self.chat_mut(chat_id)?;
        let mut saindo = c.itens.split_off(ix);
        let Item::Usuario { texto, anexos, ultrathink, ultracode, .. } = saindo.remove(0) else {
            // O `indice_editavel` já garante que o primeiro é a mensagem; se não for, nada muda.
            c.itens.append(&mut saindo);
            return None;
        };
        c.atualizado_em = agora();
        c.turno_aberto = false;
        // Com sessão criada, o próximo turno abre uma nova com a passagem; sem ela, nada a trocar.
        if c.sessao_criada {
            c.sessao = uuid::Uuid::new_v4().to_string();
            c.sessao_criada = false;
            c.contexto = None;
            self.passagens.insert(chat_id.to_string(), MotivoPassagem::Reenvio);
        }
        // O turno que saiu não é mais repetível, e o que ele estava escrevendo não tem mais lugar.
        self.ultima_entrada.remove(chat_id);
        self.parcial.remove(chat_id);
        self.execucao.insert(chat_id.to_string(), Execucao::Parado);
        if let Some(c) = self.chat(chat_id) {
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
    /// ao Claude. Devolve o texto dela, para a caixa ou para o aviso.
    pub fn tirar_da_fila(&mut self, chat: &str, indice: usize, cx: &mut Context<Self>) -> Option<String> {
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
        Some(entrada.texto)
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
        let Some(chat) = self.chat(chat_id).cloned() else {
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
            self.notificar(&chat, "Parou com erro: o processo do Claude caiu");
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
        // Ultracode só com o que ele precisa: sem isso, o Claude Code o ignora calado. Um por vez no
        // Vitral inteiro: a fonte do PC não segura dois fluxos de agentes.
        let codex = chat.agente == Agente::Codex;
        if codex && !crate::agente_codex::instalado() {
            self.avisar(chat_id, "O Codex não está instalado neste computador (ou não está no PATH); a mensagem não foi enviada.", true);
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
        let expirou = !codex && chat.sessao_criada && claude::sessao_expirou(pasta_conta.as_deref(), &chat.cwd, &chat.sessao);
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
            let itens = self.chat(chat_id).map_or(&[][..], |c| c.itens.as_slice());
            bloco_de_passagem(itens, ja_na_conversa, motivo)
        });

        let (texto, imagens) = texto_do_turno(&mensagens);
        let texto = match passagem {
            Some(bloco) => format!("{bloco}\n\n---\n\n{texto}"),
            None => texto,
        };
        let ultrathink = mensagens.iter().any(|e| e.ultrathink);
        let turno = turno_do_lote(self.ultima_entrada.get(chat_id).map(|(t, _)| t.as_str()), &mensagens);
        self.ultima_entrada.insert(chat_id.to_string(), (turno, mensagens.clone()));
        {
            let c = self.chat_mut(chat_id).unwrap();
            c.itens.extend(itens_das_entradas(mensagens));
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
        // No `-p` a palavra só injeta o lembrete; o esforço do turno sobe aqui, como no terminal.
        if ultrathink {
            opcoes.esforco = Some(esforco_do_ultrathink(opcoes.esforco.as_deref()).into());
        }
        // Vence o esforço acima: o Ultracode roda sempre em Extra.
        opcoes.ultracode = ultracode;
        // A sessão foi criada, mas a marca não chegou ao disco (queda no primeiro turno).
        if !codex
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
        };
        match Processo::iniciar(&opcoes, mensagem, move |evento| {
            let _ = sinais.send_blocking(Sinal::Claude(id.clone(), geracao, evento));
        }) {
            Ok(processo) => {
                self.processos.insert(chat_id.to_string(), processo);
                self.geracao_atual.insert(chat_id.to_string(), geracao);
                self.fundo.remove(chat_id);
                self.fim_adiado.remove(chat_id);
                self.dirigidas.remove(chat_id);
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
        if let Some(c) = self.chat(chat_id) {
            let _ = self.dados.salvar_chat(c);
        }
        self.salvar();
        cx.notify();
    }

    /// O modelo que vai no turno deste chat. No Codex, só o escolhido no chat (sem ele, vale o
    /// padrão do próprio Codex). No Claude, a família vai como a versão mais nova que o Claude
    /// Code instalado oferece (ver `claude::modelo_para_o_turno`); num Claude Code sem as versões
    /// exatas, como está.
    fn modelo_do_turno(&self, chat: &Chat) -> Option<String> {
        if chat.agente == Agente::Codex {
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
            None => chat.modelo.as_deref().and_then(crate::agente_codex::esforco_padrao),
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
        let prompt_sistema = prompts::do_chat(&chat.cwd, onde, mcp_da_equipe.is_some());
        // Por arquivo, o prompt novo vale já no turno de uma sessão retomada; num Claude Code mais
        // velho (ou antes de saber a versão), vai na linha de comando.
        let prompt_arquivo = self.versao_claude.filter(|v| *v >= claude::VERSAO_PROMPT_POR_ARQUIVO).and_then(|_| {
            let arquivo = self.dados.arquivo_prompt(&chat.id);
            let gravou = arquivo.parent().is_some_and(|p| std::fs::create_dir_all(p).is_ok())
                && std::fs::write(&arquivo, &prompt_sistema).is_ok();
            gravou.then_some(arquivo)
        });
        let codex = chat.agente == Agente::Codex;
        Opcoes {
            agente: chat.agente,
            cwd: chat.cwd.clone(),
            sessao: chat.sessao.clone(),
            retomar: chat.sessao_criada,
            // A conversa importada é do Claude Code: no Codex ela entra pelo bloco de passagem.
            bifurcar: crate::importar::bifurcar_de(chat.importada.as_ref(), chat.sessao_criada).filter(|_| !codex),
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
            mcp_da_equipe,
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
        let Some(antes) = self.chat(chat_id).map(|c| c.agente) else {
            return false;
        };
        if antes == agente {
            self.mudar_chat(chat_id, |c| c.modelo = modelo, cx);
            return true;
        }
        if self.processos.contains_key(chat_id) {
            self.avisar(chat_id, "Troque entre o Claude e o Codex quando este turno terminar.", false);
            cx.notify();
            return false;
        }
        let mut com_conversa = false;
        if let Some(c) = self.chat_mut(chat_id) {
            com_conversa = c.sessao_criada || c.importada.is_some() || c.itens.iter().any(|i| matches!(i, Item::Usuario { .. }));
            (c.agente, c.modelo, c.modelo_atual) = (agente, modelo, None);
            (c.sessao, c.sessao_criada) = (uuid::Uuid::new_v4().to_string(), false);
            (c.contexto, c.janela, c.custo_da_sessao) = (None, None, None);
            // Os comandos de barra e as skills são do Claude Code.
            if agente == Agente::Codex {
                c.comandos.clear();
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

    /// Abre a raia de `de`: um chat novo do outro agente (o Codex para um chat do Claude, e o
    /// contrário), na mesma pasta, para receber o mesmo pedido. Nasce em Planejar, que só lê: os
    /// dois trabalham na mesma pasta, e dois agentes mexendo nos mesmos arquivos se atropelam.
    /// Devolve o id do chat novo.
    pub fn abrir_raia(&mut self, de: &str, pedido: &str, cx: &mut Context<Self>) -> Result<String, String> {
        let (pasta, agente) = self.chat(de).map(|c| (c.cwd.clone(), c.agente)).ok_or("o chat não existe mais")?;
        let outro = match agente {
            Agente::Claude => Agente::Codex,
            Agente::Codex => Agente::Claude,
        };
        if outro == Agente::Codex && !crate::agente_codex::instalado() {
            return Err("o Codex não está instalado neste computador".into());
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

    /// Pergunta ao Codex instalado quais modelos ele oferece (em segundo plano) e redesenha se a
    /// lista mudou: ao abrir o app.
    pub fn reler_modelos_do_codex(&self, cx: &mut Context<Self>) {
        let pasta = self.dados.cache.join("perguntas");
        cx.spawn(async move |motor, cx| {
            let mudou = cx.background_spawn(async move { crate::agente_codex::atualizar_modelos(&pasta) }).await;
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
        if chat.agente == Agente::Codex {
            return Some("ele é do Claude Code, e este chat responde pelo Codex".into());
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
        if c.agente == Agente::Codex {
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
            Evento::SubFerramenta { pai, nome, entrada } => {
                if nome == "mcp__vitral__permissao" || nome == "ToolSearch" {
                    return;
                }
                let passo = claude::resumo_ferramenta(&nome, &entrada);
                self.no_subagente(chat_id, &pai, |sub| {
                    sub.ferramentas += 1;
                    sub.atividade = passo.clone();
                    sub.passos.push(passo);
                    if sub.passos.len() > crate::modelo::PASSOS_DO_SUBAGENTE {
                        sub.passos.remove(0);
                    }
                });
            }
            Evento::SubTexto { pai, texto } => {
                let linha: String = texto.trim().lines().next().unwrap_or_default().chars().take(160).collect();
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
                let resumo = claude::resumo_ferramenta(&nome, &entrada);
                let (detalhe, arquivo) = claude::detalhe_ferramenta(&nome, &entrada);
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
                    });
                }
            }
            Evento::ResultadoFerramenta { id, conteudo, erro } => {
                self.chats_sujos.insert(chat_id.to_string());
                // É aqui que o Claude Code absorve no turno a mensagem dirigida (medido na
                // 2.1.278): daqui em diante ela não volta mais como turno à parte.
                self.dirigidas.remove(chat_id);
                if let Some(c) = self.chat_mut(chat_id) {
                    for item in c.itens.iter_mut().rev() {
                        if let Item::Ferramenta {
                            id: id_item,
                            resultado,
                            erro: erro_item,
                            sub,
                            ..
                        } = item
                            && *id_item == id
                        {
                            *resultado = Some(git::inicio_do_texto(&conteudo, 20_000));
                            *erro_item = erro;
                            // O subagente terminou: o cartão dele dobra numa linha com a duração.
                            if let Some(sub) = sub {
                                sub.fim = agora();
                            }
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
                let agentes = self.fundo.get(chat_id).copied().unwrap_or(0);
                let dirigida_esperando = self.dirigidas.get(chat_id).is_some_and(|d| !d.is_empty());
                if adiar_o_fim(agentes, dirigida_esperando, erro, interrompido)
                    && let Some(processo) = self.processos.get_mut(chat_id)
                {
                    // O fluxo (ou subagente) segue em segundo plano e o Claude Code abre outro
                    // turno quando ele acaba: com a entrada fechada, ele espera os agentes e sai,
                    // e o turno só termina no último `result`. Só com a mensagem dirigida
                    // engatilhada, a entrada fica aberta: o turno dela começa na hora, no mesmo
                    // processo, e continua recebendo mensagens (fechada, o que o usuário mandasse
                    // durante esse turno inteiro ia para a fila; dono, 13/11/2026). Se esse turno
                    // não começar, a vigia fecha a entrada e o processo sai.
                    if agentes > 0 {
                        processo.fechar_entrada();
                    } else {
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
                // O turno acabou mesmo: a mensagem que o usuário dirigiu e o Claude Code não
                // recebeu (turno com erro, ou parado) volta para a fila, e o processo sai inteiro
                // para ele não abrir um turno à parte com ela.
                let dirigidas_perdidas = self.devolver_dirigidas(chat_id);
                self.dirigidos.remove(chat_id);
                if let Some(mut processo) = self.processos.remove(chat_id) {
                    processo.fechar_entrada();
                    // Turno com erro ou parado e agentes ainda em segundo plano: com a entrada
                    // fechada, o Claude Code seguiria esperando por eles (sem teto no Ultracode),
                    // fora da tela, do Parar e da conta de um Ultracode por vez. Saem juntos.
                    if self.fundo.remove(chat_id).unwrap_or(0) > 0 || dirigidas_perdidas > 0 {
                        processo.matar();
                    }
                    self.encerrando.push(processo);
                }
                if dirigidas_perdidas > 0 {
                    self.avisar(chat_id, &voltou_para_a_fila(dirigidas_perdidas), false);
                }
                if let Some(c) = self.chat_mut(chat_id) {
                    c.somar_custo(custo_usd);
                    c.turno_aberto = false;
                    c.atualizado_em = agora();
                    if janela.is_some() {
                        c.janela = janela;
                    }
                }
                // Parado pelo usuário: o Claude Code fecha o turno com erro, mas não é falha (o
                // usuário pode continuar a conversa).
                if self.interrompidos.iter().any(|c| c == chat_id) {
                    self.interrompidos.retain(|c| c != chat_id);
                    self.parcial.remove(chat_id);
                    self.avisar(chat_id, "Interrompido.", false);
                    self.execucao.insert(chat_id.to_string(), Execucao::Parado);
                    if let Some(c) = self.chat(chat_id) {
                        let _ = self.dados.salvar_chat(c);
                    }
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
                    if interrompido {
                        self.avisar(chat_id, "Interrompido.", false);
                        self.execucao.insert(chat_id.to_string(), Execucao::Parado);
                    } else if self.consertar_sessao(chat_id, &stderr, cx) {
                        // Repetido com a sessão certa.
                    } else {
                        let programa = self.chat(chat_id).map_or(Agente::Claude, |c| c.agente).programa();
                        let detalhe = if stderr.trim().is_empty() {
                            format!("o processo do {programa} fechou sem resposta")
                        } else {
                            stderr
                        };
                        self.avisar(chat_id, &format!("Falhou: {detalhe}"), true);
                        self.execucao.insert(chat_id.to_string(), Execucao::Erro(detalhe));
                        self.desistir(chat_id);
                    }
                    if let Some(c) = self.chat(chat_id) {
                        let _ = self.dados.salvar_chat(c);
                    }
                    self.salvar();
                }
            }
        }
    }

    fn fim_de_turno(&mut self, chat_id: &str, erro: bool, texto: String, cx: &mut Context<Self>) {
        self.parcial.remove(chat_id);
        if !erro {
            self.repetidos_sessao.remove(chat_id);
        }
        if let Some(c) = self.chat(chat_id) {
            let _ = self.dados.salvar_chat(c);
        }
        let Some(chat) = self.chat_mut(chat_id).map(Chat::copia_sem_itens) else {
            return;
        };
        // Para o resumo de quando o usuário volta ("Enquanto você esteve fora").
        let (tipo, fim) = if erro { ("falhou", "Parou com erro") } else { ("terminou", "Terminou") };
        let linha = texto.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
        let resumo = if linha.is_empty() { format!("{}: {fim}", chat.titulo) } else { format!("{}: {linha}", chat.titulo) };
        self.registrar(tipo, &chat.id, &chat.id, &resumo);
        // Mensagens que chegaram durante o turno viram o próximo (as notas esperam uma de verdade).
        if self.na_fila(chat_id) > 0
            && let Some(fila) = self.filas.remove(chat_id)
        {
            self.comecar_turno(chat_id, fila, cx);
        } else if !erro && self.pedir_revisao_da_memoria(chat_id, cx) {
            // O turno da memória já abriu; o aviso de "terminou" vem no fim dele.
        } else if !self.janela_ativa && !erro {
            self.notificar(&chat, &texto_do_fim(&texto));
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
        let entrada = Entrada {
            de: Some("Vitral".into()),
            texto: "Antes de encerrar: reveja o que desta sessão vale guardar na memória e nas instruções.".into(),
            extra: CRIVO_DA_MEMORIA.into(),
            ..Entrada::default()
        };
        self.entregar_entrada(chat_id, entrada, cx);
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
                self.registrar("permissao", &chat.id, &chat.id, &pedido);
                // Como os outros avisos, só com a janela sem foco: com ela à frente, o pedido já
                // aparece no app (e se responde lá, nunca na notificação).
                if !self.janela_ativa {
                    self.notificar(&chat, &texto_do_pedido(a["tool_name"].as_str().unwrap_or("?"), &a["input"]));
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
        let chat = self.chat(id).ok_or_else(|| anyhow::anyhow!("conversa não encontrada"))?;
        let mut texto = format!("# Conversa \"{}\" (pasta {})\n\n", chat.titulo, chat.cwd.display());
        for item in &chat.itens {
            match item {
                Item::Usuario { texto: t, .. } => texto.push_str(&format!("## Usuário\n\n{t}\n\n")),
                Item::Assistente { texto: t, .. } => texto.push_str(&format!("## Claude\n\n{t}\n\n")),
                Item::Entrega { de, texto: t, .. } => {
                    texto.push_str(&format!("## Mensagem de {de}\n\n{t}\n\n"))
                }
                Item::Ferramenta { resumo, .. } => texto.push_str(&format!("- (ferramenta) {resumo}\n")),
                Item::Raciocinio { .. } | Item::Aviso { .. } | Item::Cartao { .. } | Item::Desconhecido(_) => {}
            }
        }
        let pasta = self.dados.pasta_exportadas();
        std::fs::create_dir_all(&pasta)?;
        let arquivo = pasta.join(format!("{}-{}.md", slug(&chat.titulo), chat.id));
        std::fs::write(&arquivo, texto)?;
        Ok(arquivo)
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
    fn notificar(&self, chat: &Chat, texto: &str) {
        let titulo = chat.titulo.clone();
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

}

/// O que o agente recebe junto com o pedido de revisão da memória: o mesmo crivo do hook `Stop`
/// do dono (`eita/.claude/hooks/lembrar-instrucoes.js`), sem o que é só daquele workspace.
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

/// "Claude" ou "Codex", para títulos e rótulos curtos.
pub fn nome_curto_do_agente(agente: Agente) -> &'static str {
    match agente {
        Agente::Claude => "Claude",
        Agente::Codex => "Codex",
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
fn itens_das_entradas(mensagens: Vec<Entrada>) -> Vec<Item> {
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
    itens[ix..]
        .iter()
        .all(|i| {
            matches!(
                i,
                Item::Usuario { .. } | Item::Assistente { .. } | Item::Ferramenta { .. } | Item::Aviso { .. }
            )
        })
        .then_some(ix)
}

/// O começo do primeiro turno de uma sessão nova que tomou o lugar de outra: as últimas 10
/// mensagens (até 6000 caracteres, as mais recentes ficam). As `pular` últimas mensagens já vão no
/// próprio turno.
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

#[cfg(test)]
mod testes {
    use super::*;
    use crate::modelo::Chat;

    fn chat(id: &str) -> Chat {
        let mut c = Chat::novo(id.into(), id, PathBuf::from("/tmp"));
        (c.sessao, c.sessao_criada) = ("s".into(), true);
        (c.criado_em, c.atualizado_em, c.lido_em) = (0, 0, 0);
        c
    }

    fn avisos(chat: &Chat) -> usize {
        chat.itens.iter().filter(|i| matches!(i, Item::Aviso { .. })).count()
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
        };
        assert!(!editou_arquivo_desde(&[ferramenta("Read", 10, false), ferramenta("Bash", 11, false)], 0));
        assert!(!editou_arquivo_desde(&[ferramenta("Edit", 10, true)], 0));
        assert!(editou_arquivo_desde(&[ferramenta("Read", 10, false), ferramenta("Write", 12, false)], 0));
        // Depois do pedido das 12, só uma edição mais nova conta.
        assert!(!editou_arquivo_desde(&[ferramenta("Edit", 12, false)], 12));
        assert!(editou_arquivo_desde(&[ferramenta("Edit", 12, false), ferramenta("NotebookEdit", 13, false)], 12));
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
        let itens = itens_das_entradas(vec![entrada("pedido", true), entrada("resposta", false)]);
        assert_eq!(itens.len(), 1);
        assert!(matches!(&itens[0], Item::Entrega { texto, .. } if texto == "resposta"));
        assert!(itens_das_entradas(vec![entrada("pedido", true)]).is_empty());
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
        let itens = itens_das_entradas(vec![entrada]);
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
        let item = serde_json::to_value(&itens_das_entradas(vec![lida])[0]).unwrap();
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
        falou.itens.push(Item::Usuario { texto: "oi".into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0 });
        assert!(!nunca_rodou(&falou));
        let mut sessao = c.clone();
        sessao.sessao_criada = true;
        assert!(!nunca_rodou(&sessao));
        let trazida = Chat { importada: Some(Default::default()), ..c.clone() };
        assert!(!nunca_rodou(&trazida), "a conversa do Claude Code arquiva, não some");
    }

    #[test]
    fn passagem_de_sessao_leva_as_ultimas_mensagens() {
        let usuario = |t: &str| Item::Usuario { texto: t.into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0 };
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
