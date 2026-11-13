//! A janela do Vitral: barra lateral com os chats, as conversas no meio e o painel do chat
//! (e o arquivo aberto) à direita, em cartões de vidro como na Stained Glass UI.

pub mod componentes;
mod abas_direita;
pub mod arranjo;
mod boas_vindas;
mod abertura;
pub mod atalhos;
mod atualizacao;
mod busca_conversa;
mod caixa_entrada;
mod composer;
mod configuracoes;
mod conversa;
mod dialogos;
mod este_chat;
mod fila;
mod imagens;
mod lateral;
mod links_arquivo;
mod mapa_agentes;
mod mcp_servidores;
mod memoria_equipe;
mod mensagens;
mod menus;
pub mod movimento;
mod painel_contas;
mod fechar;
mod painel_arquivo;
pub(crate) mod paineis;
mod rascunhos;
mod resumo;
mod recuperacao;
mod reenvio;
mod seletor_arquivos;
mod trilho;
mod uso_plano;
mod visor_imagem;
pub mod zoom;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::Duration,
};

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, ResizableState, WindowExt as _, h_resizable, resizable_panel,
    h_flex,
    input::{InputEvent, InputState},
    spinner::compasso_recente,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::{
    modelo::{self, Reforco},
    motor::Motor,
    tema::{Cores, Lucide, esp, r, tam},
};
use componentes::{Flutuante as _, TamanhoBotao, botao_icone, metal};

actions!(vitral, [NovaConversa, Buscar, AlternarPainel, AlternarLista]);
actions!(vitral, [IrParaPainel1, IrParaPainel2, IrParaPainel3, IrParaPainel4]);
actions!(vitral, [FecharGaveta]);
actions!(vitral, [AumentarZoom, DiminuirZoom, ZoomPadrao]);
actions!(vitral, [AbrirMenuVitral, AbrirConfiguracoes, ProximaPendencia, IrAoFim]);
actions!(vitral, [FecharPainel, PainelAnterior, PainelSeguinte, MoverPainelEsquerda, MoverPainelDireita]);
// A seta para cima na caixa vazia e o "?" fora de um campo de texto.
actions!(vitral, [EditarUltima, AbrirAtalhos]);


/// Seções da lista que começam recolhidas.
const RECOLHIDAS_DE_INICIO: [&str; 1] = ["Mais antigas"];

/// Larguras normais da lista e do painel da direita, e o mínimo de cada conversa. Todas as
/// medidas das partes são a 100% e crescem com o zoom (a 150%, a lista normal tem 468 px).
const LATERAL: f32 = 312.;
const DIREITA: f32 = 380.;
const MIN_CONVERSA: f32 = 280.;
/// Menos que isso a conversa não se lê: é o mínimo mesmo com a janela apertada.
const MIN_APERTADO: f32 = 200.;
/// Largura que um painel novo precisa ter para valer a pena abrir.
const MIN_NOVO_PAINEL: f32 = 240.;
/// Até onde a lista e o painel da direita vão no arrasto.
const FAIXA_LISTA: (f32, f32) = (200., 560.);
/// Arrastando a divisória da lista para a esquerda, ela ainda encolhe um pouco além do mínimo;
/// passando deste ponto, recolhe de vez, como a barra lateral do VS Code (dono, 13/11/2026:
/// "quando arrastar bastante pra diminuir a parte da esquerda, ela minimizar").
const LISTA_NO_ARRASTO: f32 = 130.;
const RECOLHER_ABAIXO_DE: f32 = 160.;
const FAIXA_DIREITA: (f32, f32) = (280., 900.);
/// O que a gaveta deixa da conversa à mostra, para o clique fora fechá-la.
const FRESTA_GAVETA: f32 = 40.;

/// Quanto o zoom multiplica as medidas de desenho (1 rem = 15 px a 100%).
pub(super) fn escala(window: &Window) -> f32 {
    window.rem_size().as_f32() / 15.
}

/// Onde fica o painel da direita neste quadro.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direita {
    /// Ao lado das conversas.
    Ancorada,
    /// Não cabe ao lado: abre por cima, como gaveta.
    Gaveta,
    /// Fechado ou sem nada para mostrar.
    Fechada,
}

/// Gaveta aberta por cima das conversas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gaveta {
    Lista,
    Direita,
}

/// Como a largura da janela se divide entre lista, conversas e painel da direita. As larguras
/// são em px de verdade, já com o zoom.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Orcamento {
    /// Largura para as partes (sem sombra, margem e trilho).
    util: f32,
    /// O zoom em que as medidas a 100% foram multiplicadas.
    escala: f32,
    /// A lista fica ancorada à esquerda (senão, só como gaveta).
    lista: bool,
    direita: Direita,
    /// A lista e o painel da direita caberiam ao lado das conversas, se pedidos.
    cabe_lista: bool,
    cabe_direita: bool,
    min_conversa: f32,
    /// Até onde a lista e o painel da direita podem ser alargados sem empurrar as conversas.
    max_lista: f32,
    max_direita: f32,
    /// Quantas conversas cabem lado a lado nesta largura.
    cabem: usize,
}

/// A gaveta fecha quando a parte dela volta a caber ao lado das conversas (ou some).
fn gaveta_fecha_sozinha(gaveta: Gaveta, o: &Orcamento) -> bool {
    match gaveta {
        Gaveta::Lista => o.lista,
        Gaveta::Direita => o.direita != Direita::Gaveta,
    }
}

/// Decide o que fica ancorado: com pouca largura, o painel da direita vira gaveta primeiro,
/// depois a lista, e só então as conversas ficam abaixo de 280 (a 100%). `util` é em px e
/// `escala` é o zoom: a 150% a lista pede 468 px, então numa janela igual ela vira gaveta antes.
fn orcamento(util: f32, escala: f32, n: usize, lista: bool, direita: bool) -> Orcamento {
    let u = |medida: f32| medida * escala;
    let n = n.max(1) as f32;
    let conversas = n * u(MIN_CONVERSA);
    let cabe_lista = u(LATERAL) + conversas <= util;
    let lista = lista && cabe_lista;
    let largura_lista = if lista { u(LATERAL) } else { 0. };
    let cabe_direita = largura_lista + u(DIREITA) + conversas <= util;
    let direita = match (direita, cabe_direita) {
        (false, _) => Direita::Fechada,
        (true, true) => Direita::Ancorada,
        (true, false) => Direita::Gaveta,
    };
    let min_conversa = if conversas <= util {
        u(MIN_CONVERSA)
    } else {
        (util / n).floor().max(u(MIN_APERTADO))
    };
    // O que sobra além das conversas é dividido na proporção das larguras normais.
    let sobra = util - n * min_conversa;
    let ancorado = largura_lista + if direita == Direita::Ancorada { u(DIREITA) } else { 0. };
    let maximo = |normal: f32, (min, max): (f32, f32)| {
        if ancorado > 0. {
            (sobra * u(normal) / ancorado).floor().clamp(u(min), u(max))
        } else {
            u(max)
        }
    };
    Orcamento {
        util,
        escala,
        lista,
        direita,
        cabe_lista,
        cabe_direita,
        min_conversa,
        max_lista: maximo(LATERAL, FAIXA_LISTA),
        max_direita: maximo(DIREITA, FAIXA_DIREITA),
        cabem: ((util / u(MIN_NOVO_PAINEL)).floor() as usize).clamp(1, paineis::MAX_PAINEIS),
    }
}

/// Larguras que o usuário deu arrastando as divisórias, a 100%: com outro zoom elas crescem na
/// mesma proporção, e as conversas mantêm a divisão entre elas.
#[derive(Clone, Debug, PartialEq)]
struct Larguras {
    lista: f32,
    direita: f32,
    /// A fração de cada conversa no espaço das conversas; vazia, partes iguais.
    conversas: Vec<f32>,
}

impl Default for Larguras {
    fn default() -> Self {
        Larguras { lista: LATERAL, direita: DIREITA, conversas: Vec::new() }
    }
}

/// Larguras de cada parte num quadro, em px.
#[derive(Clone, Debug, PartialEq)]
struct LargurasPx {
    lista: Option<f32>,
    conversas: Vec<f32>,
    direita: Option<f32>,
}

impl Larguras {
    /// Guarda o que o arrasto deixou: `tamanhos` em px, na ordem das partes na tela.
    fn guardar(&mut self, tamanhos: &[f32], lista: bool, direita: bool, escala: f32) {
        let mut resto = tamanhos;
        if lista && let Some((l, depois)) = resto.split_first() {
            self.lista = (l / escala).clamp(FAIXA_LISTA.0, FAIXA_LISTA.1);
            resto = depois;
        }
        if direita && let Some((d, antes)) = resto.split_last() {
            self.direita = (d / escala).clamp(FAIXA_DIREITA.0, FAIXA_DIREITA.1);
            resto = antes;
        }
        let total: f32 = resto.iter().sum();
        self.conversas = if resto.len() > 1 && total > 0. {
            resto.iter().map(|c| c / total).collect()
        } else {
            Vec::new()
        };
    }

    /// As larguras guardadas neste orçamento, com `n` conversas: a lista e a direita dentro da
    /// faixa que cabe, e as conversas dividindo o resto.
    fn em_px(&self, o: &Orcamento, n: usize) -> LargurasPx {
        let n = n.max(1);
        let u = |medida: f32| medida * o.escala;
        let lista = o.lista.then(|| u(self.lista).min(o.max_lista).max(u(FAIXA_LISTA.0)).floor());
        let direita = (o.direita == Direita::Ancorada)
            .then(|| u(self.direita).min(o.max_direita).max(u(FAIXA_DIREITA.0)).floor());
        let livre = o.util - lista.unwrap_or(0.) - direita.unwrap_or(0.);
        let conversas = if self.conversas.len() == n {
            self.conversas.iter().map(|f| (livre * f).floor().max(o.min_conversa)).collect()
        } else {
            vec![(livre / n as f32).floor().max(o.min_conversa); n]
        };
        LargurasPx { lista, conversas, direita }
    }
}

/// Largura da área das partes: a janela menos a sombra e a borda (que somem nos lados
/// ladrilhados) e a margem. O trilho não conta mais: ele é a primeira fileira da lista.
fn largura_util(window: &Window) -> f32 {
    let sombra = gpui_kit::component::window_paddings(window);
    let lado = |s: Pixels| if s > px(0.) { s.as_f32() + 1. } else { 0. };
    let largura = window.viewport_size().width.as_f32() - lado(sombra.left) - lado(sombra.right);
    let margem = tam(window, crate::tema::margem_da_janela().as_f32()).as_f32();
    (largura - 2. * margem).max(0.)
}

/// Os arquivos de uma pasta na aba Arquivos.
struct ArquivosLidos {
    lista: std::rc::Rc<Vec<String>>,
    /// Quando a lista foi lida; `None` pede uma releitura.
    lidos_em: Option<std::time::Instant>,
    /// A pasta da releitura em andamento, uma por vez.
    relendo: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AbaLateral {
    Conversas,
    Arquivos,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filtro {
    Todos,
    Arquivados,
}

pub struct Janela {
    motor: Option<Entity<Motor>>,
    selecionado: Option<String>,
    filtro: Filtro,
    busca: Entity<InputState>,
    /// Conversas abertas lado a lado e qual delas está ativa.
    paineis: Vec<paineis::Painel>,
    ativo: usize,
    /// Painel ativo antes do último clique num cartão (o clique ativa o painel na captura).
    ativo_antes_do_clique: Option<u64>,
    proximo_painel: u64,
    /// Painel, lado e efeito sob o ponteiro enquanto uma conversa é arrastada.
    alvo_arrasto: Option<(u64, paineis::Lado, paineis::Efeito)>,
    /// Painel sob o ponteiro enquanto arquivos de fora são arrastados para a janela.
    arquivos_sobre: Option<u64>,
    /// O texto não enviado de cada conversa que não está aberta num painel. Os das abertas ficam
    /// na própria caixa; `rascunhos::gravar` junta os dois no interface.json.
    rascunhos: HashMap<String, String>,
    /// A gravação dos rascunhos esperando o fim da digitação (ver `rascunhos::agendar`).
    _gravar_rascunhos: Option<Task<()>>,
    /// Arquivos anexados à próxima mensagem de cada conversa.
    anexos: HashMap<String, Vec<PathBuf>>,
    aba: AbaLateral,
    rolagem_lista: ScrollHandle,
    /// Seções da lista recolhidas ("Mais antigas" começa assim).
    secoes_recolhidas: HashSet<&'static str>,
    /// Arquivo aberto no painel da direita (a pasta e o caminho relativo a ela).
    arquivo_aberto: Option<(PathBuf, String)>,
    /// A linha que um link da conversa pediu: o painel rola até ela ao abrir e a deixa marcada.
    arquivo_linha: std::cell::Cell<Option<u32>>,
    arquivo_marcada: std::cell::Cell<Option<u32>>,
    cache_arquivos: std::cell::RefCell<HashMap<PathBuf, ArquivosLidos>>,
    arquivo_lido: std::cell::RefCell<Option<painel_arquivo::ArquivoLido>>,
    /// Comentário para "pedir mudanças" de cada plano esperando aprovação (pelo id do pedido).
    feedback_plano: HashMap<String, Entity<InputState>>,
    expandidos: HashSet<String>,
    /// Campo de "negar e explicar" aberto num pedido de permissão.
    explicacao: Option<mensagens::Explicacao>,
    rolagem_este_chat: ScrollHandle,
    /// O chat para o qual o painel do chat já voltou ao topo: só volta de novo quando o chat
    /// selecionado muda (rolar a cada quadro prenderia a rolagem).
    este_chat_rolado: Option<String>,
    painel: bool,
    /// Lista da esquerda (conversas/arquivos) aberta ou recolhida.
    lista: bool,
    /// Divisão da largura no último quadro (lista e direita podem estar em gaveta).
    orcamento: Orcamento,
    /// Larguras dadas no arrasto das divisórias, a 100%.
    larguras: Larguras,
    /// Estado das divisórias, recomeçado quando muda a composição ou o zoom: as partes voltam às
    /// `larguras` na escala nova.
    estado_partes: Entity<ResizableState>,
    chave_partes: Option<(usize, bool, bool, u32)>,
    gaveta: Option<Gaveta>,
    /// Sobe a cada abertura da gaveta: a entrada dela toca uma vez por abertura.
    geracao_gaveta: u64,
    foco_gaveta: FocusHandle,
    foco: FocusHandle,
    /// O último chat aberto, como foi gravado no interface.json (só grava quando muda).
    ultimo_chat: Option<String>,
    /// A pasta do último chat criado (o próximo, sem chat aberto, nasce nela), também no
    /// interface.json.
    ultimo_destino: Option<PathBuf>,
    /// O histórico de cada conversa trazida do Claude Code, lido do arquivo dela quando ela abre.
    historicos: mensagens::historico::Historicos,
    _assinaturas: Vec<Subscription>,
    _assinatura_motor: Option<Subscription>,
    _assinatura_dialogo: Option<Subscription>,
    _relogio: Task<()>,
    /// Como a janela está (painéis, abas, larguras, tamanho), do jeito que vai para o
    /// interface.json: o `render` compara com o de agora e marca a gravação quando muda.
    arranjo: Option<arranjo::Arranjo>,
    /// A abertura já pôs a janela como estava: daqui em diante o arranjo é acompanhado.
    arranjo_ligado: bool,
    _gravar_arranjo: Option<Task<()>>,
    /// A imagem anexada que está aberta na "ilha" em cima da caixa (o chat e o arquivo), e a que
    /// o clique fora acabou de fechar (para o clique na própria pílula fechar, e não reabrir).
    ilha: Option<(String, PathBuf)>,
    ilha_fechada: Option<(PathBuf, std::time::Instant)>,
    /// A tela de boas-vindas da primeira abertura, enquanto está aberta.
    boas_vindas: Option<boas_vindas::BoasVindas>,
    /// O trecho de texto selecionado na janela agora (uma parte de uma resposta, por exemplo):
    /// com ele, o painel ativo mostra o "Responder a este trecho", que o cita na caixa.
    trecho: Option<String>,
    /// O trecho citado na próxima mensagem de cada chat: aparece num cartão em cima do campo e
    /// vai na frente do que for escrito, como citação.
    citacoes: HashMap<String, String>,
    /// O alerta de fechar com chats trabalhando está aberto.
    confirmando_saida: bool,
    /// Conversas com o Ultrathink ligado para a próxima mensagem.
    ultrathink: HashSet<String>,
    /// Conversas com o Ultracode ligado para a próxima mensagem (desligado ao abrir, sempre).
    ultracode: HashSet<String>,
    /// O próximo envio escolheu "Mandar depois": com o turno rodando, a mensagem espera na fila
    /// em vez de entrar nele. Vale para um envio só.
    mandar_depois: bool,
    /// Menu do Vitral aberto (F10 ou o logo).
    menu_aberto: bool,
    /// O número do sino no último quadro (`None` antes do primeiro) e a geração do pulso dele,
    /// que sobe quando o número aumenta.
    sino_numero: std::cell::Cell<Option<usize>>,
    sino_geracao: std::cell::Cell<u64>,
    /// Caixa "O que precisa de você" aberta a partir do logo do modo recolhido.
    caixa_aberta: bool,
    /// Pedido de permissão com o corpo aberto, quando há vários (id do pedido).
    pedido_aberto: Option<String>,
    /// Pedidos de permissão com a caixa "Não perguntar de novo" marcada (o "Permitir" passa a
    /// regra); some com o pedido.
    sempre_marcado: HashSet<String>,
    /// "Nova conversa ao lado": a próxima conversa criada abre num painel novo.
    nova_ao_lado: bool,
    /// Painel cuja aba está sendo arrastada.
    arrastando_aba: Option<u64>,
    /// Árvore de arquivos de cada pasta (aba Arquivos).
    arvores: HashMap<String, Entity<gpui_kit::component::tree::TreeState>>,
    /// Foco e realce da lista pelo teclado (id do chat realçado).
    foco_lista: FocusHandle,
    realce_lista: Option<String>,
    /// O painel da direita está abaixo de 22 rem: desenho estreito da Equipe.
    direita_estreita: bool,
    /// O texto que a busca da lista mostra vazia, que encurta na lista apertada.
    dica_busca: &'static str,
    /// Popover da caixa aberto (um por vez em toda a janela): painel e qual.
    pop_caixa: Option<(u64, composer::PopCaixa)>,
    /// A aba (Claude ou Codex) à vista no seletor de modelo do painel; sem ela, a do agente do chat.
    aba_do_modelo: Option<(u64, crate::modelo::Agente)>,
    /// A versão nova pronta esperando o reinício (o ponto no logo e o item do menu) e o último aviso
    /// de atualização mostrado ("pronta 0.3.0"): um por versão.
    atualizacao_pronta: Option<String>,
    atualizacao_avisada: Option<String>,
    /// A aba da direita escolhida por último ("Este chat" ou o arquivo).
    aba_direita: abas_direita::AbaDireita,
    /// A direita está abaixo de 520 px: com mais de uma aba, elas ficam só com o símbolo.
    direita_compacta: bool,
}

/// Redesenhos da janela desde a última conta (o `VITRAL_QUADROS`).
static QUADROS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Conta um redesenho para o `VITRAL_QUADROS` (a Janela e a vitrine chamam no `render`).
pub fn contar_quadro() {
    QUADROS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Com `VITRAL_QUADROS=1`, escreve "quadros/s: N" no stderr a cada segundo em que a janela
/// redesenhou: a conferência do orçamento de movimento. Quando o metal refez o campo nesse
/// segundo, a linha leva o tempo médio de um campo (conta, BMP e decodificação):
/// "quadros/s: 8 · campo: 2,1 ms". O timer só lê os contadores, não redesenha nada.
pub fn iniciar_contador_de_quadros(cx: &mut App) {
    static INICIADO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if std::env::var("VITRAL_QUADROS").as_deref() != Ok("1") {
        return;
    }
    if INICIADO.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let n = QUADROS.swap(0, std::sync::atomic::Ordering::Relaxed);
            let campo = metal::custo_do_campo();
            if n > 0 {
                eprintln!("{}", linha_dos_quadros(n, campo));
            }
        }
    })
    .detach();
}

/// A linha do `VITRAL_QUADROS`: os redesenhos do segundo e, se o metal refez o campo, o tempo
/// médio de um campo, com vírgula.
fn linha_dos_quadros(quadros: u32, campo: Option<(u32, Duration)>) -> String {
    match campo {
        Some((_, tempo)) => {
            let decimos = (tempo.as_secs_f64() * 10_000.).round() as u64;
            format!("quadros/s: {quadros} · campo: {},{} ms", decimos / 10, decimos % 10)
        }
        None => format!("quadros/s: {quadros}"),
    }
}

impl Janela {
    pub fn new(motor: Entity<Motor>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        iniciar_contador_de_quadros(cx);
        let busca = cx.new(|cx| InputState::new(window, cx).placeholder("Buscar"));
        let mut assinaturas = vec![cx.subscribe_in(&busca, window, |_, _, evento, _, cx| {
            if matches!(evento, InputEvent::Change) {
                cx.notify();
            }
        })];
        assinaturas.push(cx.observe_window_activation(window, |this, window, cx| {
            let ativa = window.is_window_active();
            // Sair grava a hora; voltar depois de um tempo fora monta o "Enquanto você esteve fora".
            if let Some(motor) = &this.motor {
                motor.update(cx, |m, cx| {
                    m.janela_ativa = ativa;
                    if ativa {
                        m.usuario_voltou(cx);
                    } else {
                        m.usuario_saiu();
                    }
                });
            }
            // Sem foco, os spinners e o brilho param na fase em que estavam; na volta, voltam a bater.
            if ativa {
                cx.notify();
            }
        }));
        // O metal desta janela sai do mapa quando ela fecha.
        let id_da_janela = window.window_handle().window_id();
        assinaturas.push(cx.on_release(move |_, cx| metal::esquecer_janela(id_da_janela, cx)));
        // O GNOME manda o button-layout depois que a janela abre (e quando muda no Ajustes).
        assinaturas.push(cx.observe_button_layout_changed(window, |_, _, cx| cx.notify()));
        assinaturas.push(cx.on_app_quit(|this, cx| {
            // O que você escreveu e não enviou vai para o disco antes de tudo: é o que você
            // espera encontrar na caixa quando o Vitral abrir de novo.
            this.gravar_ao_sair(cx);
            // E como a janela estava, para ela voltar igual.
            this.gravar_arranjo_ao_sair();
            if let Some(motor) = &this.motor {
                motor.update(cx, |m, _| {
                    m.usuario_saiu();
                    m.encerrar_tudo();
                });
            }
            async {}
        }));
        // A atualização só redesenha a janela quando a versão pronta muda.
        assinaturas.push(cx.observe_global_in::<crate::atualizacao::Atualizacoes>(window, |this, window, cx| {
            this.atualizacao_mudou(window, cx)
        }));
        // Os avisos precisam da raiz da janela, que só existe depois do primeiro quadro.
        cx.on_next_frame(window, |this, window, cx| this.atualizacao_ao_abrir(window, cx));
        Self::vigiar_fechamento(window, cx);

        // Redesenha a cada segundo enquanto houver algo rodando ou uma nova tentativa marcada
        // (o tempo decorrido e a contagem da faixa do turno). Com o compasso dos spinners batendo,
        // a batida já redesenha a duração.
        let _relogio = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let Ok(()) = this.update(cx, |this, cx| {
                    let rodando = this.motor.as_ref().is_some_and(|m| m.read(cx).trabalhando() > 0);
                    if rodando && compasso_recente(cx).is_none() {
                        cx.notify();
                    }
                }) else {
                    break;
                };
            }
        });

        let mut janela = Self {
            motor: None,
            selecionado: None,
            filtro: Filtro::Todos,
            busca,
            paineis: Vec::new(),
            ativo: 0,
            ativo_antes_do_clique: None,
            proximo_painel: 0,
            alvo_arrasto: None,
            arquivos_sobre: None,
            rascunhos: HashMap::new(),
            _gravar_rascunhos: None,
            anexos: HashMap::new(),
            aba: AbaLateral::Conversas,
            rolagem_lista: ScrollHandle::new(),
            secoes_recolhidas: HashSet::from(RECOLHIDAS_DE_INICIO),
            arquivo_aberto: None,
            arquivo_linha: Default::default(),
            arquivo_marcada: Default::default(),
            cache_arquivos: Default::default(),
            arquivo_lido: Default::default(),
            feedback_plano: HashMap::new(),
            expandidos: HashSet::new(),
            explicacao: None,
            rolagem_este_chat: ScrollHandle::new(),
            este_chat_rolado: None,
            // Fechado até você pedir (Ctrl+J, ou um arquivo citado na conversa): a conversa fica com a
            // largura toda. O que você escolheu fica lembrado no interface.json.
            painel: false,
            lista: true,
            orcamento: orcamento(largura_util(window), escala(window), 1, true, false),
            larguras: Larguras::default(),
            estado_partes: cx.new(|_| ResizableState::default()),
            chave_partes: None,
            gaveta: None,
            geracao_gaveta: 0,
            foco_gaveta: cx.focus_handle(),
            foco: cx.focus_handle(),
            ultimo_chat: None,
            ultimo_destino: None,
            historicos: Default::default(),
            _assinaturas: assinaturas,
            _assinatura_motor: None,
            _assinatura_dialogo: None,
            _relogio,
            ilha: None,
            ilha_fechada: None,
            boas_vindas: None,
            trecho: None,
            citacoes: HashMap::new(),
            arranjo: None,
            arranjo_ligado: false,
            _gravar_arranjo: None,
            confirmando_saida: false,
            ultrathink: HashSet::new(),
            ultracode: HashSet::new(),
            mandar_depois: false,
            menu_aberto: false,
            sino_numero: Default::default(),
            sino_geracao: Default::default(),
            caixa_aberta: false,
            pedido_aberto: None,
            sempre_marcado: HashSet::new(),
            nova_ao_lado: false,
            arrastando_aba: None,
            arvores: HashMap::new(),
            foco_lista: cx.focus_handle(),
            realce_lista: None,
            direita_estreita: false,
            dica_busca: "Buscar",
            pop_caixa: None,
            aba_do_modelo: None,
            atualizacao_pronta: None,
            atualizacao_avisada: None,
            aba_direita: abas_direita::AbaDireita::Chat,
            direita_compacta: true,
        };
        window.focus(&janela.foco, cx);
        janela.ligar_motor(motor, window, cx);
        janela
    }

    pub(super) fn trocar_aba(&mut self, aba: AbaLateral, window: &mut Window, cx: &mut Context<Self>) {
        // O texto da busca acompanha a aba e a largura da lista (`lateral`).
        self.aba = aba;
        self.busca.update(cx, |b, cx| b.set_value("", window, cx));
        cx.notify();
    }

    /// Ctrl+B e o botão da lista: recolhe ou mostra a lista. Sem lugar ao lado das conversas,
    /// ela abre por cima, como gaveta.
    pub(super) fn alternar_lista(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.gaveta == Some(Gaveta::Lista) {
            self.fechar_gaveta(window, cx);
        } else if self.orcamento.lista {
            self.lista = false;
        } else {
            // Pedida também na gaveta: ao alargar a janela, ela fica ao lado e a gaveta fecha.
            self.lista = true;
            if !self.orcamento.cabe_lista {
                self.abrir_gaveta(Gaveta::Lista, window, cx);
            }
        }
        cx.notify();
    }

    /// Ctrl+J e o botão do painel: o mesmo para o painel da direita.
    pub(super) fn alternar_painel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.orcamento.direita {
            _ if self.gaveta == Some(Gaveta::Direita) => self.fechar_gaveta(window, cx),
            Direita::Ancorada => self.painel = false,
            Direita::Gaveta => self.abrir_gaveta(Gaveta::Direita, window, cx),
            Direita::Fechada if self.painel => self.painel = false,
            Direita::Fechada => self.mostrar_direita(window, cx),
        }
        // Só o que você pediu aqui fica lembrado: o painel que um link abriu não muda a preferência.
        gravar_interface("painel", serde_json::Value::Bool(self.painel), cx);
        cx.notify();
    }

    /// Mostra o painel da direita: ao lado das conversas, se couber; senão, como gaveta.
    pub(super) fn mostrar_direita(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.painel = true;
        if !self.orcamento.cabe_direita {
            self.abrir_gaveta(Gaveta::Direita, window, cx);
        }
        cx.notify();
    }

    /// Mostra a lista: ao lado das conversas, ou como gaveta quando não cabe.
    pub(super) fn mostrar_lista(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.lista = true;
        if !self.orcamento.cabe_lista && self.gaveta != Some(Gaveta::Lista) {
            self.abrir_gaveta(Gaveta::Lista, window, cx);
        }
        cx.notify();
    }

    /// O foco vai para a gaveta, para o Esc fechá-la.
    fn abrir_gaveta(&mut self, gaveta: Gaveta, window: &mut Window, cx: &mut Context<Self>) {
        self.gaveta = Some(gaveta);
        self.geracao_gaveta += 1;
        window.focus(&self.foco_gaveta, cx);
        cx.notify();
    }

    /// Fecha a gaveta e devolve o foco à caixa do painel ativo, se estava nela.
    pub(super) fn fechar_gaveta(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tirar_gaveta(window, cx);
        cx.notify();
    }

    /// O mesmo, sem pedir outro quadro (serve dentro do render).
    fn tirar_gaveta(&mut self, window: &mut Window, cx: &mut App) {
        self.gaveta = None;
        if self.foco_gaveta.contains_focused(window, cx) {
            let foco = match self.composer_ativo() {
                Some(composer) => composer.read(cx).focus_handle(cx),
                None => self.foco.clone(),
            };
            window.focus(&foco, cx);
        }
    }

    /// Escolher uma conversa na gaveta da lista recolhe a gaveta.
    pub(super) fn recolher_gaveta_da_lista(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.gaveta == Some(Gaveta::Lista) {
            self.fechar_gaveta(window, cx);
        }
    }

    /// Botão de recolher/mostrar a lista da esquerda, com o Ctrl+B tirado do keymap.
    pub(super) fn botao_lista(&self, id: &'static str, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let aberta = self.orcamento.lista || self.gaveta == Some(Gaveta::Lista);
        let (icone, dica) = if aberta {
            (Lucide::PanelLeftClose, "Recolher a lista")
        } else {
            (Lucide::PanelLeftOpen, "Mostrar a lista")
        };
        sem_arrastar().child(
            botao_icone(id, icone, dica, TamanhoBotao::Barra, window, cx)
                .tooltip_with_action(dica, &AlternarLista, None)
                .on_click(cx.listener(|this, _, window, cx| this.alternar_lista(window, cx))),
        )
    }

    /// Arquivos de uma pasta. A primeira leitura é na hora; depois, a cada 20 segundos (ou no
    /// ↻), a lista é relida em segundo plano, e a anterior fica na tela enquanto isso.
    pub(super) fn arquivos_da_pasta(&self, pasta: &std::path::Path, cx: &Context<Self>) -> std::rc::Rc<Vec<String>> {
        let mut cache = self.cache_arquivos.borrow_mut();
        let lidos = cache.entry(pasta.to_path_buf()).or_insert_with(|| ArquivosLidos {
            lista: std::rc::Rc::new(seletor_arquivos::listar(pasta)),
            lidos_em: Some(std::time::Instant::now()),
            relendo: None,
        });
        let vencida = lidos.lidos_em.is_none_or(|q| q.elapsed() >= Duration::from_secs(20));
        if vencida && lidos.relendo.is_none() {
            lidos.relendo = Some(pasta.to_path_buf());
            let pasta = pasta.to_path_buf();
            cx.spawn(async move |this, cx| {
                let lida = {
                    let pasta = pasta.clone();
                    cx.background_spawn(async move { seletor_arquivos::listar(&pasta) }).await
                };
                let _ = this.update(cx, |this, cx| {
                    let mudou = {
                        let mut cache = this.cache_arquivos.borrow_mut();
                        // A leitura de um cache limpo no meio do caminho não vale mais.
                        let Some(lidos) = cache.get_mut(&pasta).filter(|l| l.relendo.as_ref() == Some(&pasta)) else {
                            return;
                        };
                        lidos.relendo = None;
                        lidos.lidos_em = Some(std::time::Instant::now());
                        // A mesma lista fica com o mesmo Rc: a árvore não é refeita à toa.
                        let mudou = *lidos.lista != lida;
                        if mudou {
                            lidos.lista = std::rc::Rc::new(lida);
                        }
                        mudou
                    };
                    if mudou {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        lidos.lista.clone()
    }

    /// O ↻ da aba Arquivos: a próxima vez que a lista for pedida, ela é relida.
    pub(super) fn reler_arquivos(&self, pasta: &std::path::Path) {
        if let Some(lidos) = self.cache_arquivos.borrow_mut().get_mut(pasta) {
            lidos.lidos_em = None;
        }
    }

    /// Antes de abrir um diálogo: o foco vai para a caixa do painel ativo (ou para a janela). O kit
    /// devolve o foco, ao fechar, a quem o tinha na abertura; vindo de um menu, que já foi embora,
    /// ninguém ficaria com ele, e os atalhos parariam.
    pub(super) fn foco_antes_do_dialogo(&self, window: &mut Window, cx: &mut Context<Self>) {
        let foco = match self.composer_ativo() {
            Some(composer) => composer.read(cx).focus_handle(cx),
            None => self.foco.clone(),
        };
        window.focus(&foco, cx);
    }

    /// Grava o último chat aberto, quando muda (a abertura volta nele).
    fn lembrar_ultimo_chat(&mut self, cx: &mut Context<Self>) {
        if self.selecionado.is_some() && self.selecionado != self.ultimo_chat {
            self.ultimo_chat = self.selecionado.clone();
            let valor = serde_json::Value::String(self.ultimo_chat.clone().unwrap_or_default());
            gravar_interface("ultimo_chat", valor, cx);
        }
    }

    /// Liga a janela ao motor (uma vez, ao abrir) e abre na conversa mais recente.
    fn ligar_motor(&mut self, motor: Entity<Motor>, window: &mut Window, cx: &mut Context<Self>) {
        self._assinatura_motor = Some(cx.observe_in(&motor, window, |this, motor, window, cx| {
            // O clique numa notificação: a conversa abre como no sino, e a janela vem para a frente
            // (ou o GNOME avisa que o Vitral está pronto).
            if let Some(chat) = motor.update(cx, |m, _| m.pedido_de_abrir.take()) {
                this.selecionar(Some(chat), window, cx);
                window.activate_window();
            }
            let aberturas = motor.update(cx, |m, _| std::mem::take(&mut m.aberturas));
            for abertura in aberturas {
                this.aplicar_abertura(abertura, window, cx);
                window.activate_window();
            }
            // Os avisos que chegam depois de aberta (o resultado da importação do Claude Code).
            let avisos = motor.update(cx, |m, _| std::mem::take(&mut m.avisos_ao_abrir));
            if !avisos.is_empty() {
                cx.on_next_frame(window, move |_, window, cx| {
                    for texto in avisos {
                        window.push_notification(componentes::aviso(componentes::Tom::Info, texto), cx);
                    }
                });
            }
            // Uma rolagem pela barra ainda não desenhada não pode ser desfeita pelo fim abaixo.
            this.acompanhar_rolagem(cx);
            for painel in &this.paineis {
                if painel.seguindo {
                    painel.rolagem.scroll_to_bottom();
                }
                // O que chega nas conversas abertas já conta como visto (se o painel está no fim).
                if let Some(id) = painel.chat.as_ref().filter(|_| painel.seguindo) {
                    motor.update(cx, |m, _| m.marcar_lido(id));
                }
            }
            cx.notify();
        }));
        // Abre no último chat aberto (ou no mais recente); os avisos da abertura vão para a tela.
        let m = motor.read(cx);
        let interface = zoom::interface();
        let lido = |chave: &str| interface.as_ref().and_then(|i| i.get(chave)?.as_str().map(str::to_string));
        // Abre no último chat aberto e, sem ele, no mais recente.
        let ultimo = lido("ultimo_chat").filter(|id| m.chat(id).is_some_and(|c| !c.arquivado));
        let recente = ultimo.clone().or_else(|| {
            m.chats
                .iter()
                .filter(|c| !c.arquivado)
                .max_by_key(|c| c.atualizado_em)
                .map(|c| c.id.clone())
        });
        self.ultimo_chat = ultimo;
        // Como a janela estava ao fechar, e quais chats de lá ainda dá para abrir (os apagados e
        // os arquivados nesse meio tempo ficam de fora).
        let guardado = arranjo::Arranjo::ler(interface.as_ref());
        let abriveis: HashSet<String> = m
            .chats
            .iter()
            .filter(|c| !c.arquivado)
            .map(|c| c.id.clone())
            .collect();
        self.painel = interface.as_ref().and_then(|i| i.get("painel")?.as_bool()).unwrap_or(false);
        // Os rascunhos antes do `selecionar`: é ele que leva o do chat que abre para a caixa.
        self.rascunhos = rascunhos::ler(interface.as_ref(), |id| m.chat(id).is_some());
        // O último destino de um chat novo é uma pasta; o das versões com projetos (um id de projeto,
        // ou "avulsos") não vale mais.
        self.ultimo_destino = lido("ultimo_destino").map(PathBuf::from).filter(|p| p.is_absolute());
        let avisos = motor.update(cx, |m, _| std::mem::take(&mut m.avisos_ao_abrir));
        let aberturas = motor.update(cx, |m, _| std::mem::take(&mut m.aberturas));
        // Uma vez na abertura: com uma saída gravada, o resumo do que aconteceu com o Vitral fechado.
        motor.update(cx, |m, cx| m.usuario_voltou(cx));
        self.motor = Some(motor);
        // A janela volta como estava (as conversas lado a lado, as abas, as larguras); sem nada
        // guardado, abre no último chat.
        if !self.reabrir_arranjo(guardado, |id| abriveis.contains(id), window, cx) && recente.is_some() {
            self.selecionar(recente, window, cx);
        }
        self.arranjo_ligado = true;
        // Na primeira abertura (nenhum chat ainda e nenhuma pasta de trabalho escolhida), a tela
        // de boas-vindas; `VITRAL_BOAS_VINDAS=1` a abre de novo, para ver o desenho.
        let primeira_vez = self.motor.as_ref().is_some_and(|m| {
            let m = m.read(cx);
            m.config.pasta_de_trabalho.is_none() && m.chats.is_empty()
        });
        if primeira_vez || std::env::var("VITRAL_BOAS_VINDAS").as_deref() == Ok("1") {
            self.abrir_boas_vindas(cx);
        }
        // O kit só monta a raiz da janela (que guarda avisos e diálogos) depois desta construção:
        // os avisos da abertura e o que ela pede (o chat de `vitral <pasta>`) vão no primeiro
        // quadro.
        cx.on_next_frame(window, move |this, window, cx| {
            for texto in avisos {
                window.push_notification(componentes::aviso(componentes::Tom::Info, texto), cx);
            }
            for abertura in aberturas {
                this.aplicar_abertura(abertura, window, cx);
            }
        });
        cx.notify();
    }

    /// Envia o que está na caixa do painel ativo.
    fn enviar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(motor), Some(chat), Some(composer)) =
            (self.motor.clone(), self.selecionado.clone(), self.composer_ativo())
        else {
            return;
        };
        // Enter com um comando pela metade completa o comando em vez de enviar.
        if let Some(prefixo) = self.prefixo_comando(cx)
            && let Some(c) = motor.read(cx).chat(&chat)
        {
            let comandos = self.comandos_de(c);
            if !comandos.iter().any(|x| *x == prefixo)
                && let Some(primeiro) = comandos.iter().find(|x| x.contains(prefixo.as_str()))
            {
                let primeiro = primeiro.clone();
                self.completar_comando(&primeiro, window, cx);
                return;
            }
        }
        let texto = composer.read(cx).value().to_string();
        if texto.trim().is_empty() && self.anexos.get(&chat).is_none_or(Vec::is_empty) {
            return;
        }
        // O `/mcp` é do Vitral: no `-p` o Claude Code só manda abrir o terminal.
        if texto.trim() == "/mcp" {
            composer.update(cx, |c, cx| c.set_value("", window, cx));
            self.abrir_mcp(window, cx);
            return;
        }
        let anexos = self.anexos.remove(&chat).unwrap_or_default();
        // O trecho citado vai na frente da resposta (um comando de barra não o gasta).
        let texto = match self.citacoes.get(&chat) {
            Some(_) if texto.trim_start().starts_with('/') || texto.trim().is_empty() => texto,
            Some(_) => composer::com_citacao(&self.citacoes.remove(&chat).unwrap_or_default(), &texto),
            None => texto,
        };
        // As chaves do Ultrathink e do Ultracode desligam ao enviar; um comando de barra não as gasta.
        // O Ultracode que deixou de valer (o modelo mudou, por exemplo) sai sem ir, como a caixa mostra.
        let vale_ultracode = {
            let m = motor.read(cx);
            m.chat(&chat).is_some_and(|c| m.motivo_sem_ultracode(c).is_none())
        };
        let reforco = Reforco {
            ultrathink: composer::gastar_chave(&mut self.ultrathink, &chat, &texto),
            ultracode: composer::gastar_chave(&mut self.ultracode, &chat, &texto) && vale_ultracode,
            // "Mandar depois": vale só para este envio.
            depois: std::mem::take(&mut self.mandar_depois),
        };
        self.ilha = None;
        motor.update(cx, |m, cx| m.enviar_do_usuario(&chat, texto, anexos, reforco, cx));
        composer.update(cx, |c, cx| c.set_value("", window, cx));
        if let Some(painel) = self.paineis.get_mut(self.ativo) {
            painel.seguindo = true;
            painel.rolagem.scroll_to_bottom();
        }
        cx.notify();
    }

    /// "Mandar depois": o mesmo envio, mas a mensagem espera na fila em vez de entrar no turno que
    /// está rodando.
    pub(super) fn enviar_depois(&mut self, painel: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.mandar_depois = true;
        self.ativar_painel(painel, cx);
        self.enviar_do_painel(painel, window, cx);
        // Um envio recusado (caixa vazia, issue sem o bloco) não guarda a escolha para o seguinte.
        self.mandar_depois = false;
    }

    /// `vitral --nova <pasta> <mensagem>`: abre uma conversa na pasta já com a primeira mensagem.
    pub fn comecar_conversa(&mut self, pasta: PathBuf, mensagem: String, window: &mut Window, cx: &mut Context<Self>) {
        self.nova_conversa_na_pasta(Some(pasta), window, cx);
        if let Some(composer) = self.composer_ativo() {
            composer.update(cx, |c, cx| c.set_value(mensagem, window, cx));
        }
        self.enviar(window, cx);
    }

    /// Um chat novo direto na pasta; sem pasta, numa pasta vazia só dele.
    pub(super) fn nova_conversa_na_pasta(&mut self, pasta: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let id = match motor.update(cx, |m, cx| m.novo_chat(pasta.as_deref(), "", cx)) {
            Ok(id) => id,
            Err(erro) => {
                window.push_notification(componentes::aviso(componentes::Tom::Erro, format!("Não abriu o chat: {erro}")), cx);
                return;
            }
        };
        self.selecionar(Some(id), window, cx);
        if let Some(composer) = self.composer_ativo() {
            let foco = composer.read(cx).focus_handle(cx);
            window.focus(&foco, cx);
        }
    }

    /// Faixa de cima de cada cartão: arrasta a janela e maximiza no clique duplo.
    fn arrastavel(&self, id: &'static str) -> Stateful<Div> {
        self.arrastavel_com_id(id)
    }

    fn arrastavel_com_id(&self, id: impl Into<ElementId>) -> Stateful<Div> {
        let faixa = div().id(id);
        // No Windows quem arrasta é o próprio sistema: a área `Drag` vira a barra de título
        // (HTCAPTION), com o clique duplo, o Aero Snap e o menu do botão direito. Lá o
        // `start_window_move` não faz nada e o `zoom_window` só maximiza. Os botões de dentro da
        // faixa continuam com o `sem_arrastar`: o clique para neles e o sistema não arrasta.
        if cfg!(windows) {
            return faixa.window_control_area(WindowControlArea::Drag);
        }
        faixa
            .on_mouse_down(MouseButton::Left, |evento, window, _| {
                if evento.click_count >= 2 {
                    window.zoom_window();
                } else {
                    window.start_window_move();
                }
            })
    }

    /// Botões da janela como no GNOME: os do `button-layout` (no padrão do Fedora, só o ×), na
    /// ordem dele, em círculos como os do libadwaita.
    fn botoes_janela(&self, cx: &Context<Self>) -> Div {
        grupo_botoes_janela(botoes_do_layout(cx.button_layout()), false)
    }

    /// Canto de cima do painel da direita: os botões da janela quando ele está ao lado das
    /// conversas; na gaveta, o botão de recolhê-la, com o Esc tirado do keymap (os da janela
    /// ficam na última conversa).
    fn botoes_da_direita(&self, separar: bool, window: &Window, cx: &Context<Self>) -> Div {
        if self.gaveta != Some(Gaveta::Direita) {
            // O × daqui é o da JANELA, e ao lado do título do painel ele parecia fechar só o
            // painel. Por isso o painel ganha o botão dele (o ícone de recolher) e um traço o
            // separa dos botões da janela.
            let _ = separar;
            return div()
                .flex()
                .flex_none()
                .items_center()
                .gap(esp::E4)
                .child(
                    sem_arrastar().flex_none().child(
                        botao_icone("recolher-painel", Lucide::PanelRightClose, "Fechar o painel", TamanhoBotao::Barra, window, cx)
                            .tooltip_with_action("Fechar o painel", &AlternarPainel, None)
                            .on_click(cx.listener(|this, _, window, cx| this.alternar_painel(window, cx))),
                    ),
                )
                .child(grupo_botoes_janela(botoes_do_layout(cx.button_layout()), true));
        }
        sem_arrastar().flex_none().child(
            botao_icone("recolher-gaveta", Lucide::PanelRightClose, "Recolher o painel", TamanhoBotao::Barra, window, cx)
                .tooltip_with_action("Recolher o painel", &FecharGaveta, Some("Gaveta"))
                .on_click(cx.listener(|this, _, window, cx| this.fechar_gaveta(window, cx))),
        )
    }

    /// O painel da direita foi pedido e tem o que mostrar: a aba do chat ("Este chat" ou o PR)
    /// pede um chat selecionado; a do arquivo, o arquivo aberto (as outras abas vêm junto).
    fn direita_pedida(&self) -> bool {
        self.painel
            && match self.aba_direita {
                abas_direita::AbaDireita::Arquivo(_) => self.arquivo_aberto.is_some() || self.selecionado.is_some(),
                abas_direita::AbaDireita::Chat => {
                    self.selecionado.is_some() || self.arquivo_aberto.is_some()
                }
            }
    }

    /// O painel da direita está na tela, ao lado das conversas (fora da gaveta).
    pub(super) fn direita_visivel(&self) -> bool {
        self.orcamento.direita == Direita::Ancorada
    }

    /// A gaveta aberta por cima das conversas: um fundo que a fecha no clique e, na borda, a
    /// lista ou o painel da direita, quase opacos para o texto de baixo não aparecer.
    fn gaveta_por_cima(
        &mut self,
        motor: &Entity<Motor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let gaveta = self.gaveta?;
        let (largura, minima, conteudo) = match gaveta {
            Gaveta::Lista => {
                let lista = self.lateral(motor, window, cx);
                (self.larguras.lista, FAIXA_LISTA.0, div().size_full().child(lista).into_any_element())
            }
            // A aba da direita escolhe o conteúdo (o arquivo, o PR ou "Este chat"), na gaveta também.
            Gaveta::Direita => (self.larguras.direita, FAIXA_DIREITA.0, self.conteudo_da_direita(motor, window, cx)),
        };
        let largura = px(largura_da_gaveta(largura, minima, &self.orcamento));
        // Entra deslizando 24 px da borda, com o véu em fade junto (uma vez por abertura).
        let t = movimento::abertura(("gaveta", self.geracao_gaveta as usize), window, cx);
        let de = match gaveta {
            Gaveta::Lista => movimento::De::Esquerda,
            Gaveta::Direita => movimento::De::Direita,
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .child(
                    div()
                        .id("fundo-gaveta")
                        .absolute()
                        .inset_0()
                        .rounded(crate::tema::raio_da_peca())
                        .bg(Cores::veu().opacity(0.5 * t))
                        .occlude()
                        .on_any_mouse_down(
                            cx.listener(|this, _, window, cx| this.fechar_gaveta(window, cx)),
                        ),
                )
                .child(
                    div()
                        .id("gaveta")
                        .key_context("Gaveta")
                        .track_focus(&self.foco_gaveta)
                        .on_action(cx.listener(|this, _: &FecharGaveta, window, cx| {
                            this.fechar_gaveta(window, cx)
                        }))
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .map(|s| match gaveta {
                            Gaveta::Lista => s.left_0(),
                            Gaveta::Direita => s.right_0(),
                        })
                        .w(largura)
                        .flutuante(cx)
                        .bg(Cores::gaveta())
                        .rounded(crate::tema::raio_da_peca())
                        .occlude()
                        .map(|s| movimento::deslizar_gaveta(s, de, t))
                        .child(conteudo),
                ),
        )
    }
}

impl Render for Janela {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        contar_quadro();
        if let Some(motor) = &self.motor {
            crate::tema::definir_transparencia(motor.read(cx).config.transparencia);
        }
        // A chapa de metal atrás de tudo: corre quando um chat dos painéis abertos está
        // trabalhando (o que roda em segundo plano não a acelera).
        let trabalhando = self.motor.as_ref().is_some_and(|motor| {
            let motor = motor.read(cx);
            self.paineis.iter().filter_map(|p| p.chat.as_deref()).any(|chat| motor.execucao(chat).rodando())
        });
        // Nas boas-vindas o metal é o fundo da tela, à vista e andando (dono, 13/11/2026: "faz o
        // shader ficar no fundo"): ele corre como se um chat trabalhasse, e o véu é mais leve.
        let boas_vindas = self.em_boas_vindas();
        let chapa = metal::fundo(trabalhando || boas_vindas, Corners::default(), window, cx);
        let tema = cx.theme();
        let mut base = div()
            .size_full()
            .font_family(tema.font_family.clone())
            .text_color(tema.foreground)
            // A base é meio transparente e fosca (dono, 13/11/2026): o metal perde opacidade e o
            // véu deixa passar o resto, então o que está atrás da janela aparece. Quanto, é a
            // escolha "Transparência" das Configurações (`tema::NIVEIS_DE_TRANSPARENCIA`).
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .opacity(if boas_vindas { 1. } else { crate::tema::opacidade_do_metal() })
                    .child(chapa),
            )
            // O véu da base: a janela inteira é uma camada só (a lista mora direto nela, junto
            // com a borda), e os cartões flutuam por cima.
            .child(div().absolute().top_0().left_0().size_full().bg(if boas_vindas {
                Cores::vidro_leitura().opacity(0.52)
            } else {
                Cores::vidro_leitura()
            }))
            // O grão que faz o transparente ler como fosco; no opaco não há o que foscar.
            .when(crate::tema::transparencia() > 0, |base| base.child(metal::fosco(Corners::default())))
            .child(zoom::captura_roda())
            .children(zoom::pilula(cx));
        base.text_style().font_fallbacks =
            Some(FontFallbacks::from_fonts(crate::tema::FONTES_DE_RESERVA.map(String::from).to_vec()));
        // No Wayland, o ponteiro que sai da janela não mexe na posição guardada, e o hover do
        // último lugar (dicas, botões de hover) ficava preso: uma camada por cima tira o hover.
        // Fica fora durante um arrasto, para não tapar a soltura de arquivos de fora. No Windows o
        // problema não existe, e a camada atrapalharia: passar da área do cliente para a faixa de
        // arrastar (que lá é a barra de título do sistema) conta como sair da janela por um instante,
        // e com a camada por cima a faixa deixaria de ser faixa.
        let fora = (cfg!(not(windows)) && !window.is_window_hovered() && !cx.has_active_drag())
            .then(|| div().id("ponteiro-fora").absolute().inset_0().occlude());
        let Some(motor) = self.motor.clone() else {
            return base.children(fora).into_any_element();
        };
        if let Some(leitura) = motor.read(cx).somente_leitura().cloned() {
            return base.child(self.tela_de_recuperacao(leitura, cx)).children(fora).into_any_element();
        }
        // A primeira abertura: a apresentação, a pasta de trabalho e as preferências.
        if self.em_boas_vindas() {
            return base.child(self.tela_de_boas_vindas(window, cx)).children(fora).into_any_element();
        }
        // As conversas dos painéis abertos: o texto parcial das outras não redesenha a janela.
        let visiveis: HashSet<String> = self.paineis.iter().filter_map(|p| p.chat.clone()).collect();
        if motor.read(cx).visiveis != visiveis {
            motor.update(cx, |m, _| m.visiveis = visiveis);
        }
        // O "/" de um chat que ainda não rodou turno aqui: pergunta ao Claude Code, uma vez por
        // pasta e em segundo plano, quais comandos e skills ele oferece nela.
        let sem_comandos: Vec<PathBuf> = {
            let m = motor.read(cx);
            self.paineis
                .iter()
                .filter_map(|p| m.chat(p.chat.as_deref()?))
                // Sem conferir a pasta aqui (seria uma ida ao disco por painel a cada quadro):
                // a pergunta confere, uma vez.
                .filter(|c| c.comandos.is_empty())
                .map(|c| c.cwd.clone())
                .collect()
        };
        for pasta in sem_comandos {
            if crate::claude::falta_perguntar_comandos(&pasta) {
                cx.spawn(async move |this, cx| {
                    let achou = cx.background_spawn(async move { crate::claude::atualizar_comandos_da_pasta(&pasta) }).await;
                    if achou {
                        let _ = this.update(cx, |_, cx| cx.notify());
                    }
                })
                .detach();
            }
        }
        // O esforço que o automático usa em cada chat à vista: pergunta ao Claude Code, uma vez
        // por conta e modelo e em segundo plano (o seletor de esforço mostra qual é).
        let sem_esforco: Vec<(Option<PathBuf>, String)> = {
            let m = motor.read(cx);
            self.paineis
                .iter()
                .filter_map(|p| m.chat(p.chat.as_deref()?))
                .filter(|c| c.esforco.is_none())
                .filter_map(|c| m.chave_do_esforco(c))
                .collect()
        };
        for (conta, modelo) in sem_esforco {
            if crate::claude::falta_perguntar_esforco(conta.as_deref(), &modelo) {
                let pasta = motor.read(cx).pasta_das_perguntas();
                cx.spawn(async move |this, cx| {
                    let soube = cx
                        .background_spawn(async move { crate::claude::atualizar_esforco_padrao(&pasta, conta.as_deref(), &modelo) })
                        .await;
                    if soube {
                        let _ = this.update(cx, |_, cx| cx.notify());
                    }
                })
                .detach();
            }
        }
        // O texto selecionado na janela, guardado a cada desenho: o clique no "Responder a este
        // trecho" desfaz a seleção antes de chegar ao botão, e o que vale é o que estava marcado.
        self.trecho = gpui_kit::base::TextSelection::has_selection(window, cx)
            .then(|| gpui_kit::base::TextSelection::selected_text(window, cx))
            .filter(|t| !t.trim().is_empty());
        // O deslize da roda do mouse: cada painel que está deslizando anda um passo até o alvo.
        for painel in &mut self.paineis {
            let Some(alvo) = painel.rolagem_suave else {
                continue;
            };
            let atual = painel.rolagem.offset();
            let id = ElementId::NamedInteger(format!("rolagem-suave-{}", painel.id).into(), painel.geracao_rolagem);
            let y = movimento::valor_desde(id, atual.y.as_f32(), alvo, movimento::MOVER, movimento::Curva::Mover, window, cx);
            painel.rolagem.set_offset(point(atual.x, px(y)));
            if (y - alvo).abs() < 0.5 {
                painel.rolagem_suave = None;
            }
        }
        // Antes da lista, para ela já mostrar como lida a conversa que voltou ao fim.
        self.acompanhar_rolagem(cx);
        // As abas de cada painel acompanham o chat à vista (e perdem os chats que sumiram).
        {
            let motor = motor.clone();
            let motor = motor.read(cx);
            self.acertar_abas(motor);
        }
        // A conversa selecionada pode ter sumido (ex.: troca de pasta).
        if let Some(id) = &self.selecionado
            && motor.read(cx).chat(id).is_none()
        {
            self.selecionado = None;
        }
        // Com as abas acertadas: o que mudou no arranjo da janela fica marcado para gravar.
        self.lembrar_arranjo(window, cx);
        // Faixas de soltura só valem durante um arrasto (o sistema não avisa quando ele sai).
        if !cx.has_active_drag() {
            self.alvo_arrasto = None;
            self.arquivos_sobre = None;
            self.arrastando_aba = None;
        }
        // Um campo de comentário por plano esperando aprovação, cada um no seu cartão.
        let planos: Vec<String> = motor
            .read(cx)
            .permissoes
            .iter()
            .filter(|p| p.ferramenta == "ExitPlanMode")
            .map(|p| p.id.clone())
            .collect();
        self.feedback_plano.retain(|id, _| planos.contains(id));
        let pedidos: HashSet<String> = motor.read(cx).permissoes.iter().map(|p| p.id.clone()).collect();
        self.sempre_marcado.retain(|id| pedidos.contains(id));
        for id in planos {
            self.feedback_plano.entry(id).or_insert_with(|| {
                cx.new(|cx| {
                    InputState::new(window, cx).placeholder("O que mudar no plano? (opcional)")
                })
            });
        }
        self.orcamento = orcamento(
            largura_util(window),
            escala(window),
            self.paineis.len(),
            self.lista,
            self.direita_pedida(),
        );
        // A gaveta fecha quando a parte volta a caber ao lado (ou some) e durante um arrasto,
        // em que ela cobriria as zonas de soltura.
        let fecha_gaveta = self.gaveta.is_some_and(|g| gaveta_fecha_sozinha(g, &self.orcamento));
        if self.gaveta.is_some() && (fecha_gaveta || cx.has_active_drag()) {
            self.tirar_gaveta(window, cx);
        }
        let gaveta = self.gaveta_por_cima(&motor, window, cx);
        let janela = base
            .key_context("Vitral")
            .track_focus(&self.foco)
            .on_action(cx.listener(|this, _: &NovaConversa, window, cx| this.novo_chat_direto(window, cx)))
            .on_action(cx.listener(|this, _: &Buscar, window, cx| this.focar_busca(window, cx)))
            // Ctrl+F e F3: buscar dentro da conversa aberta. O `Search` é o que o campo do kit
            // liga ao Ctrl+F; sem atender aqui, o Ctrl+F na caixa de mensagem se perderia.
            .on_action(cx.listener(|this, _: &busca_conversa::BuscarNaConversa, window, cx| {
                this.abrir_busca_na_conversa(window, cx)
            }))
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Search, window, cx| {
                this.abrir_busca_na_conversa(window, cx)
            }))
            .on_action(cx.listener(|this, _: &busca_conversa::AchadoSeguinte, _, cx| this.andar_na_busca(1, cx)))
            .on_action(cx.listener(|this, _: &busca_conversa::AchadoAnterior, _, cx| this.andar_na_busca(-1, cx)))
            .on_action(cx.listener(|this, _: &AlternarPainel, window, cx| {
                this.alternar_painel(window, cx)
            }))
            .on_action(cx.listener(|this, _: &AlternarLista, window, cx| {
                this.alternar_lista(window, cx)
            }))
            .on_action(cx.listener(|this, _: &IrParaPainel1, window, cx| this.focar_painel(0, window, cx)))
            .on_action(cx.listener(|this, _: &IrParaPainel2, window, cx| this.focar_painel(1, window, cx)))
            .on_action(cx.listener(|this, _: &IrParaPainel3, window, cx| this.focar_painel(2, window, cx)))
            .on_action(cx.listener(|this, _: &IrParaPainel4, window, cx| this.focar_painel(3, window, cx)))
            // Cada ação chama o método do módulo dono da tela.
            .on_action(cx.listener(|this, _: &AbrirMenuVitral, window, cx| this.abrir_menu_vitral(window, cx)))
            .on_action(cx.listener(|this, _: &AbrirConfiguracoes, window, cx| {
                this.abrir_configuracoes(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ProximaPendencia, window, cx| this.proxima_pendencia(window, cx)))
            .on_action(cx.listener(|this, _: &FecharPainel, window, cx| this.fechar_painel_ativo(window, cx)))
            .on_action(cx.listener(|this, _: &PainelAnterior, window, cx| this.painel_vizinho(-1, window, cx)))
            .on_action(cx.listener(|this, _: &PainelSeguinte, window, cx| this.painel_vizinho(1, window, cx)))
            .on_action(cx.listener(|this, _: &MoverPainelEsquerda, _, cx| this.mover_painel(-1, cx)))
            .on_action(cx.listener(|this, _: &MoverPainelDireita, _, cx| this.mover_painel(1, cx)))
            .on_action(cx.listener(|this, _: &composer::AbrirModelo, window, cx| {
                this.alternar_pop(composer::PopCaixa::Modelo, window, cx)
            }))
            .on_action(cx.listener(|this, _: &composer::AbrirEsforco, window, cx| {
                this.alternar_pop(composer::PopCaixa::Esforco, window, cx)
            }))
            .on_action(cx.listener(|this, _: &IrAoFim, _, cx| this.ir_ao_fim_ativo(cx)))
            .on_action(cx.listener(|this, _: &AbrirAtalhos, window, cx| {
                this.foco_antes_do_dialogo(window, cx);
                atalhos::abrir(window, cx);
            }))
            .on_action(cx.listener(|this, acao: &links_arquivo::AbrirLinkVitral, window, cx| this.abrir_link_vitral(&acao.0, window, cx)))
            .child(
                h_flex()
                    .size_full()
                    .p(crate::tema::margem_da_janela())
                    .gap(crate::tema::vao_entre_pecas())
                    .items_start()
                    // O trilho virou a fileira de cima da lista: a coluna de ícones à esquerda
                    // saiu, e Ctrl+B continua escondendo as duas coisas de uma vez.
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            // Dois cliques na divisória entre duas conversas deixam todas com a
                            // mesma largura. O ouvinte é da janela, na captura: a alça do kit
                            // fica por cima de tudo e não deixa o clique chegar a quem está em
                            // volta. O `canvas` só serve para saber onde as partes começam.
                            .child(
                                canvas(|_, _, _| {}, {
                                    let janela = cx.entity().downgrade();
                                    move |limites, _, window, _| {
                                        let (janela, origem) = (janela.clone(), limites.origin.x);
                                        window.on_mouse_event(move |evento: &MouseDownEvent, fase, window, cx| {
                                            // Só dentro da área das partes, e nunca com um diálogo
                                            // ou uma gaveta por cima (o clique duplo é deles).
                                            if fase != DispatchPhase::Capture
                                                || evento.button != MouseButton::Left
                                                || evento.click_count != 2
                                                || !limites.contains(&evento.position)
                                                || window.has_active_dialog(cx)
                                            {
                                                return;
                                            }
                                            let _ = janela.update(cx, |this, cx| {
                                                if this.gaveta.is_none()
                                                    && this.na_divisoria_de_conversas(evento.position.x - origem, cx)
                                                {
                                                    this.igualar_conversas(cx);
                                                    cx.stop_propagation();
                                                }
                                            });
                                        });
                                    }
                                })
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full(),
                            )
                            .child(self.partes(&motor, window, cx))
                            .children(gaveta),
                    ),
            )
            .children(self.faixa_de_gravacao(&motor, cx))
            // O uso do plano flutua no canto de baixo, à direita.
            .children(self.uso_no_canto(cx))
            .children(fora)
            .into_any_element();
        self.lembrar_ultimo_chat(cx);
        janela
    }
}

impl Janela {
    /// A faixa fixa de quando uma gravação falhou: nada novo começa até voltar a gravar (o motor
    /// tenta de novo sozinho). Sem ×: some quando gravar.
    fn faixa_de_gravacao(&self, motor: &Entity<Motor>, cx: &Context<Self>) -> Option<AnyElement> {
        let (erro, _) = motor.read(cx).falha_gravacao.clone()?;
        let texto = format!("Não estou conseguindo gravar ({erro}). Nada novo começa até voltar a gravar.");
        Some(
            div()
                .absolute()
                .top(esp::E8)
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .px(esp::E16)
                .child(componentes::Alerta::new(componentes::Tom::Erro, texto))
                .into_any_element(),
        )
    }

    /// Lista, conversas lado a lado e painel da direita, com divisórias que redimensionam. Só
    /// entram as partes que o orçamento deixou ancoradas; as outras vão para a gaveta.
    fn partes(
        &mut self,
        motor: &Entity<Motor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let o = self.orcamento;
        let n = self.paineis.len().max(1);
        let lista = o.lista;
        let painel = o.direita == Direita::Ancorada;
        // Com outra composição ou outro zoom, as divisórias recomeçam das larguras guardadas, já
        // na escala nova: nada pula, e o que foi arrastado a 100% fica 1,5× a 150%.
        let chave = (n, painel, lista, (o.escala * 100.).round() as u32);
        if self.chave_partes != Some(chave) {
            self.chave_partes = Some(chave);
            self.estado_partes.update(cx, |estado, _| estado.clear());
        }
        let larguras = self.larguras.em_px(&o, n);
        let min_conversa = px(o.min_conversa);
        let u = |medida: f32| px(medida * o.escala);
        let janela = cx.entity().downgrade();
        // O id muda com a composição, para o estado de dentro de cada parte não passar a outra.
        let mut grupo = h_resizable(SharedString::from(format!("partes-{n}-{painel}-{lista}")))
            .with_state(&self.estado_partes)
            .on_resize(move |estado, _, cx| {
                let tamanhos: Vec<f32> = estado.read(cx).sizes().iter().map(|t| t.as_f32()).collect();
                let _ = janela.update(cx, |this, cx| {
                    // A lista arrastada bem abaixo do mínimo recolhe (o Ctrl+B a traz de volta,
                    // na largura mínima).
                    if lista && tamanhos.first().is_some_and(|l| l / o.escala < RECOLHER_ABAIXO_DE) {
                        this.lista = false;
                        this.larguras.lista = FAIXA_LISTA.0;
                        this.chave_partes = None;
                        cx.notify();
                        return;
                    }
                    this.larguras.guardar(&tamanhos, lista, painel, o.escala)
                });
            });
        if let Some(largura) = larguras.lista {
            grupo = grupo.child(
                resizable_panel()
                    .size(px(largura))
                    .size_range(u(LISTA_NO_ARRASTO)..px(o.max_lista))
                    .fixed()
                    .child(div().size_full().pr(esp::E4).child(self.lateral(motor, window, cx))),
            );
        }
        if self.paineis.is_empty() {
            // A tela de começar também aceita uma conversa arrastada da lista.
            let conteudo = self.conversa(motor, None, window, cx).into_any_element();
            grupo = grupo.child(
                resizable_panel()
                    .size(px(larguras.conversas[0]))
                    .child(div().size_full().px(esp::E4).child(self.zona_de_soltura(0, conteudo, cx))),
            );
        } else {
            let ultimo = self.paineis.len() - 1;
            for ix in 0..self.paineis.len() {
                let id = self.paineis[ix].id;
                let conteudo = self.conversa(motor, Some(ix), window, cx).into_any_element();
                // Meio vão de cada lado, menos na beirada da janela: lá a margem é só a da janela,
                // igual à de cima (o dono viu "a borda do lado maior que a de cima", 13/11/2026).
                let na_esquerda = ix == 0 && larguras.lista.is_none();
                let na_direita = ix == ultimo && !painel;
                grupo = grupo.child(
                    resizable_panel().size(px(larguras.conversas[ix])).size_range(min_conversa..Pixels::MAX).child(
                        div()
                            .size_full()
                            .when(!na_esquerda, |d| d.pl(esp::E4))
                            .when(!na_direita, |d| d.pr(esp::E4))
                            .child(self.zona_de_soltura(id, conteudo, cx)),
                    ),
                );
            }
        }
        if painel {
            let conteudo = self.conteudo_da_direita(motor, window, cx);
            grupo = grupo.child(
                resizable_panel()
                    .size(px(larguras.direita.unwrap_or(o.max_direita)))
                    .size_range(u(FAIXA_DIREITA.0)..px(o.max_direita))
                    .fixed()
                    .child(div().size_full().pl(esp::E4).child(conteudo)),
            );
        }
        grupo
    }
}

impl Janela {
    /// Se `x` (medido do começo das partes) cai numa divisória entre duas conversas lado a lado,
    /// com 6 px de folga.
    fn na_divisoria_de_conversas(&self, x: Pixels, cx: &Context<Self>) -> bool {
        let n = self.paineis.len();
        if n < 2 {
            return false;
        }
        let o = self.orcamento;
        let medidos: Vec<f32> = self.estado_partes.read(cx).sizes().iter().map(|t| t.as_f32()).collect();
        let larguras = self.larguras.em_px(&o, n);
        let antes = usize::from(o.lista);
        // As larguras de agora (as do arrasto, se houve) ou, sem medida, as guardadas.
        let (lista, conversas): (f32, Vec<f32>) = if medidos.len() >= antes + n {
            (medidos[..antes].iter().sum(), medidos[antes..antes + n].to_vec())
        } else {
            (larguras.lista.unwrap_or(0.), larguras.conversas.clone())
        };
        let mut borda = lista;
        conversas[..n - 1].iter().any(|largura| {
            borda += largura;
            (x.as_f32() - borda).abs() <= 6.
        })
    }

    /// As conversas lado a lado voltam a dividir o espaço por igual.
    fn igualar_conversas(&mut self, cx: &mut Context<Self>) {
        self.larguras.conversas.clear();
        // As divisórias recomeçam das larguras guardadas.
        self.chave_partes = None;
        cx.notify();
    }
}

/// Largura da gaveta, em px: a guardada no zoom atual, sem tapar a conversa inteira.
fn largura_da_gaveta(guardada: f32, minima: f32, o: &Orcamento) -> f32 {
    let u = |medida: f32| medida * o.escala;
    u(guardada).min(o.util - u(FRESTA_GAVETA)).max(u(minima)).floor()
}

/// Grava uma chave do interface.json em segundo plano (com fsync, fora da thread da janela).
pub(super) fn gravar_interface(chave: &'static str, valor: serde_json::Value, cx: &App) {
    cx.background_spawn(async move {
        zoom::gravar_na_interface(chave, valor);
    })
    .detach();
}

/// Contêiner para botões dentro de uma faixa arrastável: o clique neles não move a janela.
pub fn sem_arrastar() -> Div {
    div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// Botões da direita do `button-layout`. Se o usuário pôs todos à esquerda, eles vêm para a
/// direita mesmo assim (o canto esquerdo é do trilho); sem configuração (e fora do Linux, onde o
/// GPUI não lê nenhuma), – □ ×, o mesmo do `WindowButtonLayout::linux_default`.
fn botoes_do_layout(layout: Option<WindowButtonLayout>) -> Vec<WindowButton> {
    let layout = layout.unwrap_or(WindowButtonLayout {
        left: [None; 3],
        right: [Some(WindowButton::Minimize), Some(WindowButton::Maximize), Some(WindowButton::Close)],
    });
    let direita: Vec<WindowButton> = layout.right.into_iter().flatten().collect();
    if direita.is_empty() {
        layout.left.into_iter().flatten().collect()
    } else {
        direita
    }
}

fn grupo_botoes_janela(botoes: Vec<WindowButton>, separar: bool) -> Div {
    // No Windows, os botões são áreas do sistema (`Min`, `Max`, `Close`): o clique vai para ele, que
    // minimiza, maximiza ou restaura e mostra o Snap Layouts no □; o × vira um WM_CLOSE, que passa
    // pela pergunta de `fechar.rs`. Por isso lá nada para o clique no caminho (o `sem_arrastar`
    // pararia, e o sistema nunca o veria), e cada botão tapa a faixa de baixo (`occlude`), senão a
    // área dela, que vem antes, ganharia.
    let grupo = if cfg!(windows) { div() } else { sem_arrastar() };
    grupo
        .flex()
        .flex_none()
        .items_center()
        .gap(esp::E8)
        .when(separar && !botoes.is_empty(), |s| {
            s.ml(esp::E4).child(div().w(px(1.)).h(r(16.)).bg(Cores::traco()))
        })
        .children(botoes.into_iter().map(|botao| {
            let (id, icone) = match botao {
                WindowButton::Minimize => ("minimizar", IconName::WindowMinimize),
                WindowButton::Maximize => ("maximizar", IconName::WindowMaximize),
                WindowButton::Close => ("fechar", IconName::WindowClose),
            };
            div()
                .id(id)
                .size(r(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(crate::tema::raio::de(24.))
                // Sem disco em repouso: o botão só ganha fundo com o ponteiro em cima.
                // O fechar acende em vermelho e diz o que fecha: é o Vitral inteiro.
                .map(|b| match botao {
                    WindowButton::Close => b.hover(|s| s.bg(Cores::vermelho().opacity(0.55))),
                    _ => b.hover(|s| s.bg(rgba(0xffffff26))),
                })
                .active(|s| s.bg(rgba(0xffffff40)))
                .when(!cfg!(windows), |b| {
                    let dica = match botao {
                        WindowButton::Minimize => "Minimizar",
                        WindowButton::Maximize => "Maximizar",
                        WindowButton::Close => "Fechar o Vitral",
                    };
                    b.tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(dica).build(window, cx))
                })
                .child(Icon::new(icone).size(r(14.)).text_color(Cores::tenue()))
                .map(|b| {
                    if cfg!(windows) {
                        let area = match botao {
                            WindowButton::Minimize => WindowControlArea::Min,
                            WindowButton::Maximize => WindowControlArea::Max,
                            WindowButton::Close => WindowControlArea::Close,
                        };
                        return b.occlude().window_control_area(area);
                    }
                    b.on_click(move |_, window, cx| match botao {
                        WindowButton::Minimize => window.minimize_window(),
                        WindowButton::Maximize => window.zoom_window(),
                        // Pergunta antes se algo está trabalhando (o remove_window não pergunta).
                        WindowButton::Close => fechar::fechar_janela(window, cx),
                    })
                })
        }))
}

pub fn tempo_relativo(instante: u64) -> String {
    let passado = modelo::agora().saturating_sub(instante);
    match passado {
        0..60 => "agora".into(),
        60..3600 => format!("{} min", passado / 60),
        3600..86400 => format!("{} h", passado / 3600),
        _ => format!("{} d", passado / 86400),
    }
}

/// Há quanto tempo (o turno, a tarefa): "19 s", "2 min 13 s", "1 h 05 min".
pub fn duracao(desde: u64) -> String {
    componentes::duracao_curta(modelo::agora().saturating_sub(desde))
}

#[cfg(test)]
mod testes_quadros {
    use super::linha_dos_quadros;
    use std::time::Duration;

    #[test]
    fn linha_do_contador_com_e_sem_o_campo() {
        assert_eq!(linha_dos_quadros(8, None), "quadros/s: 8");
        assert_eq!(linha_dos_quadros(8, Some((8, Duration::from_micros(2_140)))), "quadros/s: 8 · campo: 2,1 ms");
        assert_eq!(linha_dos_quadros(20, Some((20, Duration::from_micros(960)))), "quadros/s: 20 · campo: 1,0 ms");
    }
}

// O `button-layout` (e o `WindowButtonLayout::parse`) é do Linux.
#[cfg(all(test, target_os = "linux"))]
mod testes_botoes_janela {
    use super::botoes_do_layout;
    use gpui_kit::{WindowButton, WindowButtonLayout};

    fn botoes(layout: &str) -> Vec<WindowButton> {
        botoes_do_layout(Some(WindowButtonLayout::parse(layout).unwrap()))
    }

    #[test]
    fn segue_o_layout_do_gnome() {
        // O padrão do Fedora: o menu do app (ignorado) à esquerda e só o × à direita.
        assert_eq!(botoes("appmenu:close"), vec![WindowButton::Close]);
        assert_eq!(
            botoes("appmenu:minimize,maximize,close"),
            vec![WindowButton::Minimize, WindowButton::Maximize, WindowButton::Close]
        );
    }

    #[test]
    fn botoes_a_esquerda_vem_para_a_direita() {
        assert_eq!(botoes("close,minimize:"), vec![WindowButton::Close, WindowButton::Minimize]);
    }

    #[test]
    fn sem_layout_usa_os_tres_e_layout_vazio_nao_mostra_nada() {
        assert_eq!(
            botoes_do_layout(None),
            vec![WindowButton::Minimize, WindowButton::Maximize, WindowButton::Close]
        );
        assert_eq!(botoes_do_layout(None), botoes_do_layout(Some(WindowButtonLayout::linux_default())));
        assert!(botoes(":").is_empty());
    }
}

#[cfg(test)]
mod testes_orcamento {
    use super::{Direita, Gaveta, Larguras, gaveta_fecha_sozinha, largura_da_gaveta, orcamento};

    // Área das partes na janela mínima (960), no tamanho inicial (1440) e em meia tela de
    // 2560 a 100% (1280) e a 125% (1024), com a sombra de 20 px dos dois lados.
    const MINIMA: f32 = 846.;
    const INICIAL: f32 = 1326.;
    const MEIA_100: f32 = 1166.;
    const MEIA_125: f32 = 910.;

    #[test]
    fn com_largura_tudo_fica_ancorado() {
        let o = orcamento(INICIAL, 1., 2, true, true);
        assert!(o.lista);
        assert_eq!(o.direita, Direita::Ancorada);
        assert_eq!(o.min_conversa, 280.);
    }

    #[test]
    fn a_direita_vira_gaveta_antes_da_lista() {
        // Na janela mínima, 312 + 380 + 280 não cabem: a direita sai, a lista fica.
        let o = orcamento(MINIMA, 1., 1, true, true);
        assert!(o.lista);
        assert_eq!(o.direita, Direita::Gaveta);
        assert!(!o.cabe_direita);
        // Com 3 conversas no tamanho inicial, idem.
        let o = orcamento(INICIAL, 1., 3, true, true);
        assert!(o.lista);
        assert_eq!(o.direita, Direita::Gaveta);
        // Com a lista recolhida, a direita volta para o lado.
        let o = orcamento(MINIMA, 1., 1, false, true);
        assert_eq!(o.direita, Direita::Ancorada);
        assert!(o.cabe_lista);
    }

    #[test]
    fn sem_lugar_a_lista_tambem_vira_gaveta() {
        let o = orcamento(MEIA_125, 1., 3, true, true);
        assert!(!o.lista);
        assert!(!o.cabe_lista);
        assert_eq!(o.direita, Direita::Gaveta);
        assert_eq!(o.min_conversa, 280.);
    }

    #[test]
    fn so_entao_as_conversas_ficam_abaixo_de_280() {
        // 4 conversas na janela mínima: 211 px cada, e nada transborda.
        let o = orcamento(MINIMA, 1., 4, true, true);
        assert!(!o.lista);
        assert_eq!(o.direita, Direita::Gaveta);
        assert_eq!(o.min_conversa, 211.);
        assert!(4. * o.min_conversa <= MINIMA);
        // Nunca abaixo de 200, mesmo que transborde.
        assert_eq!(orcamento(500., 1., 4, true, true).min_conversa, 200.);
    }

    #[test]
    fn gaveta_da_lista_fecha_ao_alargar_se_a_lista_foi_pedida() {
        // Ctrl+B em meia tela abre a lista em gaveta; maximizada, ela cabe ao lado.
        assert!(!gaveta_fecha_sozinha(Gaveta::Lista, &orcamento(MEIA_125, 1., 3, true, true)));
        assert!(gaveta_fecha_sozinha(Gaveta::Lista, &orcamento(INICIAL, 1., 3, true, true)));
        // Sem a lista pedida, a gaveta ficaria por cima mesmo com espaço ao lado.
        assert!(!gaveta_fecha_sozinha(Gaveta::Lista, &orcamento(INICIAL, 1., 3, false, true)));
        // A da direita fecha quando cabe ao lado ou não há mais o que mostrar.
        assert!(!gaveta_fecha_sozinha(Gaveta::Direita, &orcamento(MINIMA, 1., 1, true, true)));
        assert!(gaveta_fecha_sozinha(Gaveta::Direita, &orcamento(MINIMA, 1., 1, false, true)));
        assert!(gaveta_fecha_sozinha(Gaveta::Direita, &orcamento(MINIMA, 1., 1, true, false)));
    }

    #[test]
    fn direita_fechada_nao_vira_gaveta() {
        let o = orcamento(MINIMA, 1., 1, true, false);
        assert_eq!(o.direita, Direita::Fechada);
        assert!(!o.cabe_direita);
    }

    #[test]
    fn cabem_menos_paineis_na_janela_estreita() {
        assert_eq!(orcamento(INICIAL, 1., 1, true, true).cabem, 4);
        assert_eq!(orcamento(MEIA_100, 1., 1, true, true).cabem, 4);
        assert_eq!(orcamento(MEIA_125, 1., 1, true, true).cabem, 3);
        assert_eq!(orcamento(MINIMA, 1., 1, true, true).cabem, 3);
        assert_eq!(orcamento(100., 1., 1, true, true).cabem, 1);
    }

    #[test]
    fn alargar_lista_e_direita_nao_empurra_as_conversas_para_fora() {
        for (util, n) in [(INICIAL, 1), (INICIAL, 2), (MEIA_100, 1), (1800., 3), (2400., 4)] {
            let o = orcamento(util, 1., n, true, true);
            let mut ocupado = n as f32 * o.min_conversa;
            if o.lista {
                assert!(o.max_lista >= 312.);
                ocupado += o.max_lista;
            }
            if o.direita == Direita::Ancorada {
                assert!(o.max_direita >= 380.);
                ocupado += o.max_direita;
            }
            assert!(ocupado <= util, "{util} com {n}: {ocupado}");
        }
    }

    #[test]
    fn a_150_as_partes_pedem_uma_vez_e_meia() {
        // Na janela de 2560 maximizada a 150% (2412 de área), tudo ancorado, com 1,5×.
        let o = orcamento(2412., 1.5, 2, true, true);
        assert!(o.lista);
        assert_eq!(o.direita, Direita::Ancorada);
        assert_eq!(o.min_conversa, 420.);
        let px = Larguras::default().em_px(&o, 2);
        assert_eq!(px.lista, Some(468.));
        assert_eq!(px.direita, Some(570.));
        // Na janela inicial de 1440, a 150%, a direita vira gaveta (468 + 570 + 420 > 1286).
        let o = orcamento(1286., 1.5, 1, true, true);
        assert!(o.lista);
        assert_eq!(o.direita, Direita::Gaveta);
        // O mesmo orçamento a 100% numa janela 1,5× menor dá as mesmas decisões.
        for (util, n) in [(INICIAL, 1), (INICIAL, 3), (MINIMA, 1), (MEIA_125, 3)] {
            let a = orcamento(util, 1., n, true, true);
            let b = orcamento(util * 1.5, 1.5, n, true, true);
            assert_eq!((a.lista, a.direita, a.cabem), (b.lista, b.direita, b.cabem), "{util} com {n}");
            assert!((b.min_conversa - a.min_conversa * 1.5).abs() <= 1.5);
        }
    }

    #[test]
    fn o_que_foi_arrastado_cresce_com_o_zoom() {
        // A 100%, a lista foi a 400 e as duas conversas ficaram em 60/40.
        let mut l = Larguras::default();
        l.guardar(&[400., 480., 320., 380.], true, true, 1.);
        assert_eq!(l.lista, 400.);
        assert_eq!(l.direita, 380.);
        assert_eq!(l.conversas, vec![0.6, 0.4]);
        // A 150%, numa janela que comporta: 600 de lista e a mesma divisão.
        let o = orcamento(2412., 1.5, 2, true, true);
        let px = l.em_px(&o, 2);
        assert_eq!(px.lista, Some(600.));
        assert_eq!(px.direita, Some(570.));
        let livre = 2412. - 600. - 570.;
        assert_eq!(px.conversas, vec![(livre * 0.6f32).floor(), (livre * 0.4f32).floor()]);
        // Guardar a 150% e voltar a 100% devolve o que era.
        let mut de_volta = Larguras::default();
        de_volta.guardar(&[600., 745., 497., 570.], true, true, 1.5);
        assert_eq!(de_volta.lista, 400.);
        assert_eq!(de_volta.direita, 380.);
        // Outra quantidade de conversas divide por igual.
        assert_eq!(l.em_px(&orcamento(INICIAL, 1., 3, true, false), 3).conversas.len(), 3);
    }

    #[test]
    fn larguras_guardadas_ficam_na_faixa_e_no_que_cabe() {
        let mut l = Larguras::default();
        // Sem a direita e sem a lista ancoradas, só as conversas contam.
        l.guardar(&[500., 500.], false, false, 1.);
        assert_eq!((l.lista, l.direita), (312., 380.));
        assert_eq!(l.conversas, vec![0.5, 0.5]);
        // Arrastada além da faixa, fica no limite; na janela pequena, no máximo que cabe.
        l.guardar(&[900., 600.], true, false, 1.);
        assert_eq!(l.lista, 560.);
        assert!(l.conversas.is_empty());
        let o = orcamento(MINIMA, 1., 1, true, false);
        let px = l.em_px(&o, 1);
        assert_eq!(px.lista, Some(o.max_lista));
        assert!(px.lista.unwrap() + px.conversas[0] <= MINIMA);
    }

    #[test]
    fn a_gaveta_segue_a_largura_guardada_e_o_zoom() {
        let o = orcamento(MINIMA, 1., 1, true, true);
        assert_eq!(largura_da_gaveta(380., 280., &o), 380.);
        let o = orcamento(MINIMA * 1.5, 1.5, 1, true, true);
        assert_eq!(largura_da_gaveta(380., 280., &o), 570.);
        // Numa janela apertada, deixa a fresta da conversa à mostra, mas não fica abaixo do mínimo.
        let o = orcamento(500., 1.5, 1, true, true);
        assert_eq!(largura_da_gaveta(380., 280., &o), 440.);
        let o = orcamento(300., 1., 1, true, true);
        assert_eq!(largura_da_gaveta(380., 280., &o), 280.);
    }
}
