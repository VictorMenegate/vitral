//! As Tarefas (Ctrl+I, o item da lista, ou "Ideias deste chat" no menu de uma conversa): o quadro
//! no lugar das conversas, no jeito do Better Paseo (o plugin do Paseo que põe todos os agentes
//! num quadro; dono, 26/11/2026: "faz o quadro de ideias ser tipo esse better paseo, pode trocar o
//! nome também, e faz ele abrir no lugar dos chats e ainda ter como salvar as ideias").
//!
//! - Cinco raias. **Ideias** é o kanban de `crate::ideias` que já havia: escreve-se a ideia no alto
//!   (Enter anota), as Próximas vêm primeiro, o cartão se arrasta e o clique abre a edição. As
//!   outras quatro são os chats, cada um na raia do estado dele: **Esperando você** (pedido,
//!   erro, parada), **Trabalhando**, **Para ver** (resposta que você não viu) e **Feitas** (as
//!   das últimas duas semanas; "Mostrar todas" traz o resto). O cartão de chat não se arrasta: a
//!   raia sai do estado, como no Better Paseo.
//! - No cartão do chat: parar, arquivar e responder a permissão sem abrir a conversa; o clique
//!   abre a conversa (e o quadro fecha).
//! - Busca (título, pasta, modelo, o primeiro pedido) e filtros por agente e pasta.
//! - Uma ideia que virou chat (Fazendo ou Feita, com o chat vivo) aparece no cartão do chat, com o
//!   selo "Ideia"; sem chat, ela fica na raia da coluna dela.
//! - Com o Linear ligado (`crate::linear`), as ideias são as issues da etiqueta "Ideia" do time; o
//!   cabeçalho diz quando leu, com o ⟳. O quadro relê o Linear ao abrir e a cada minuto aberto.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::atomic::{AtomicU8, Ordering},
    time::Duration,
};

use gpui_kit::component::{
    Sizable as _, Size, WindowExt as _, h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        AnelGirando, CabecalhoDialogo, ControleSegmentado, Etiqueta, Marca, PreviaArrasto, Segmento, Simbolo, TamanhoBotao, Tom, aviso,
        aviso_com_acao, botao_fantasma, botao_icone, botao_perigo, botao_primario, campo_de_busca_pequeno, icone, icone_de_estado, sinal_do_chat,
    },
    configuracoes::{Opcao, menu_escolha},
    dialogos::{com_til, nome_da_pasta},
    tempo_relativo,
};
use crate::{
    ideias::{Coluna, Ideia},
    modelo::{Agente, Chat, Execucao, Item, agora},
    motor::{Espera, Motor, Pendencia, SituacaoDoLinear},
    tema::{Cores, Lucide, Tipo as _, esp, r, raio},
};

/// De quanto em quanto tempo o quadro aberto relê o Linear.
const RELER_A_CADA: Duration = Duration::from_secs(60);
/// Quantas conversas a escolha do chat de uma ideia oferece (as mexidas por último).
const CHATS_NA_ESCOLHA: usize = 40;
/// A largura mínima de uma raia, a 100%: abaixo disso o quadro rola de lado (o protótipo).
const LARGURA_DA_RAIA: f32 = 180.;
/// O vão entre as raias, a 100%.
const VAO_DAS_RAIAS: f32 = 12.;
/// As Feitas mostram as conversas mexidas nestes dias, até `FEITAS_A_VISTA`; o resto fica atrás
/// do "Mostrar todas".
const DIAS_DAS_FEITAS: u64 = 14;
const FEITAS_A_VISTA: usize = 30;

/// De onde vêm as ideias que o quadro mostra (dono, 18/11/2026: "selecionar as do Linear e as do
/// Vitral como um filtro"). Com "Vitral", a ideia nova também fica só no Vitral.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Origem {
    #[default]
    Todas,
    Linear,
    Vitral,
}

impl Origem {
    const TODAS: [Origem; 3] = [Origem::Todas, Origem::Linear, Origem::Vitral];

    fn aceita(self, ideia: &Ideia) -> bool {
        match self {
            Origem::Todas => true,
            Origem::Linear => ideia.linear.is_some(),
            Origem::Vitral => ideia.linear.is_none(),
        }
    }
}

/// O filtro escolhido por último, para o quadro abrir nele de novo (até fechar o app).
static ORIGEM: AtomicU8 = AtomicU8::new(0);

fn origem_lembrada() -> Origem {
    Origem::TODAS.get(usize::from(ORIGEM.load(Ordering::Relaxed))).copied().unwrap_or_default()
}

/// As raias do quadro, da esquerda para a direita.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Raia {
    Ideias,
    Esperando,
    Trabalhando,
    ParaVer,
    Feitas,
}

impl Raia {
    const TODAS: [Raia; 5] = [Raia::Ideias, Raia::Esperando, Raia::Trabalhando, Raia::ParaVer, Raia::Feitas];

    fn titulo(self) -> SharedString {
        match self {
            Raia::Ideias => tr!("Ideias"),
            Raia::Esperando => tr!("Esperando você"),
            Raia::Trabalhando => tr!("Trabalhando"),
            Raia::ParaVer => tr!("Para ver"),
            Raia::Feitas => tr!("Feitas"),
        }
    }

    fn icone(self) -> Lucide {
        match self {
            Raia::Ideias => Lucide::Lightbulb,
            Raia::Esperando => Lucide::Hand,
            Raia::Trabalhando => Lucide::CirclePlay,
            Raia::ParaVer => Lucide::CircleDot,
            Raia::Feitas => Lucide::CircleCheckBig,
        }
    }

    fn cor(self) -> Hsla {
        match self {
            Raia::Ideias => Cores::apagado(),
            Raia::Esperando => Cores::espera(),
            Raia::Trabalhando => Cores::destaque_texto(),
            Raia::ParaVer => Cores::violeta(),
            Raia::Feitas => Cores::verde(),
        }
    }

    fn vazia(self) -> SharedString {
        match self {
            Raia::Ideias => tr!("Escreva aqui em cima o que você não quer esquecer."),
            Raia::Esperando => tr!("Nenhum chat esperando você."),
            Raia::Trabalhando => tr!("Nenhum chat trabalhando agora."),
            Raia::ParaVer => tr!("Nenhuma resposta nova."),
            Raia::Feitas => tr!("Nada feito nas últimas duas semanas."),
        }
    }

    /// A coluna em que uma ideia solta nesta raia fica (as raias só de chats não aceitam ideia).
    fn coluna_da_ideia(self) -> Option<Coluna> {
        match self {
            Raia::Ideias => Some(Coluna::Ideias),
            Raia::Trabalhando => Some(Coluna::Fazendo),
            Raia::Feitas => Some(Coluna::Feitas),
            Raia::Esperando | Raia::ParaVer => None,
        }
    }

    /// A raia de uma ideia sem chat vivo, pela coluna dela.
    fn da_coluna(coluna: Coluna) -> Raia {
        match coluna {
            Coluna::Ideias | Coluna::Proximas => Raia::Ideias,
            Coluna::Fazendo => Raia::Trabalhando,
            Coluna::Feitas => Raia::Feitas,
        }
    }
}

/// A raia de um chat, pelo estado dele: o pedido, o erro e a parada esperam você; a resposta que
/// você não viu fica para ver; o turno andando, trabalhando; o resto, feito.
pub(super) fn raia_do_chat(execucao: &Execucao, espera: Option<Espera>) -> Raia {
    match execucao {
        Execucao::EsperandoVoce => Raia::Esperando,
        Execucao::Rodando { .. } => Raia::Trabalhando,
        _ => match espera {
            Some(Espera::Pedido | Espera::Parada | Espera::Erro) => Raia::Esperando,
            Some(Espera::Nova) => Raia::ParaVer,
            None => Raia::Feitas,
        },
    }
}

/// O cartão que se arrasta entre as colunas.
#[derive(Clone)]
struct ArrastoIdeia {
    id: String,
    titulo: SharedString,
}

/// O cartão aberto para editar: os campos escrevem no motor a cada tecla.
struct Edicao {
    id: String,
    titulo: Entity<InputState>,
    pedido: Entity<TextareaState>,
}

pub(super) struct QuadroDeIdeias {
    motor: Entity<Motor>,
    janela: WeakEntity<Janela>,
    /// O campo do alto da raia Ideias.
    pub(super) nova: Entity<InputState>,
    /// A busca do quadro.
    busca: Entity<InputState>,
    /// Onde nasce o chat de uma ideia sem chat nem pasta: a pasta do chat à vista ao abrir.
    pasta_padrao: Option<PathBuf>,
    /// Aberto pelo menu de uma conversa: só as ideias dela (e o cartão dela), e a nova nasce dela.
    do_chat: Option<String>,
    /// Todas, só as do Linear ou só as do Vitral.
    origem: Origem,
    /// Só os chats (e as ideias dos chats) de um agente.
    agente: Option<Agente>,
    /// Só os chats e as ideias de uma pasta.
    pasta: Option<PathBuf>,
    /// As Feitas inteiras, e não só as das duas últimas semanas.
    feitas_todas: bool,
    edicao: Option<Edicao>,
    /// A tela de ligar o Linear: o campo da chave.
    conexao: Option<Entity<InputState>>,
    _assinaturas: Vec<Subscription>,
    _releitura: Task<()>,
}

/// O chat de uma ideia que ainda dá para usar (existe e não foi arquivado).
fn chat_vivo<'a>(m: &'a Motor, ideia: &Ideia) -> Option<&'a Chat> {
    ideia.chat.as_deref().and_then(|c| m.chat(c)).filter(|c| !c.arquivado)
}

/// Como está o chat de um cartão de ideia, em poucas palavras.
fn estado_do_chat(m: &Motor, c: &Chat) -> SharedString {
    match m.execucao(&c.id) {
        Execucao::Rodando { .. } => tr!("trabalhando"),
        Execucao::EsperandoVoce => tr!("esperando você"),
        Execucao::Erro(_) => tr!("com erro"),
        Execucao::Parado if m.nao_lido(c) => tr!("resposta nova"),
        Execucao::Parado => tr!("parado"),
    }
}

/// O modelo do chat, curto, para o selo do cartão.
fn modelo_do_chat(c: &Chat) -> String {
    match (c.agente, c.modelo_atual.as_deref().or(c.modelo.as_deref()).filter(|m| !m.is_empty())) {
        (Agente::Codex, _) => "Codex".to_string(),
        (Agente::Gemini, Some(m)) => crate::agente_gemini::nome_do_modelo(m),
        (Agente::Gemini, None) => "Gemini".to_string(),
        (_, Some(m)) => super::composer::nome_curto_modelo(m),
        _ => "Claude".to_string(),
    }
}

/// A primeira linha com texto, sem as marcas do Markdown (o "##" de um título, o "-" de um item,
/// a crase e o negrito): o cartão mostra o texto, não a marcação.
fn primeira_linha(texto: &str) -> Option<String> {
    texto
        .lines()
        .map(|l| l.trim().trim_start_matches(['#', '>', '-', '*', ' ']).replace("**", "").replace('`', ""))
        .find(|l| !l.trim().is_empty())
}

/// O que o cartão do chat diz embaixo do título, pela raia: o que ele pede, o que está fazendo,
/// ou o começo da última resposta.
fn detalhe_do_chat(c: &Chat, raia: Raia, pendencia: Option<&Pendencia>) -> Option<String> {
    match raia {
        Raia::Esperando => pendencia.map(|p| match p {
            Pendencia::Permissao { ferramenta, resumo, .. } => tr!("Permissão · {ferramenta} · {resumo}", ferramenta = ferramenta, resumo = resumo).to_string(),
            Pendencia::Plano { .. } => tr!("Plano pronto para aprovar").to_string(),
            Pendencia::Pergunta { pergunta, .. } => tr!("Pergunta · {pergunta}", pergunta = pergunta).to_string(),
            Pendencia::Erro { texto, .. } => tr!("Falhou · {motivo}", motivo = primeira_linha(texto).unwrap_or_default()).to_string(),
            Pendencia::Parada { motivo, .. } => tr!("Parou · {motivo}", motivo = motivo).to_string(),
            Pendencia::Nova { .. } => tr!("Resposta nova").to_string(),
        }),
        Raia::Trabalhando => Some(
            c.itens
                .iter()
                .rev()
                .find_map(|i| match i {
                    Item::Ferramenta { resumo, .. } => Some(super::mensagens::detalhe_do_passo(resumo)),
                    _ => None,
                })
                .unwrap_or_else(|| tr!("Pensando…").to_string()),
        ),
        _ => c.itens.iter().rev().find_map(|i| match i {
            Item::Assistente { texto, .. } => primeira_linha(texto),
            _ => None,
        }),
    }
}

/// O que a busca procura num chat: o título, a pasta, o modelo e o primeiro pedido.
fn texto_do_chat(c: &Chat) -> String {
    let pedido = c
        .itens
        .iter()
        .find_map(|i| match i {
            Item::Usuario { texto, .. } | Item::Entrega { texto, .. } => Some(texto.chars().take(400).collect::<String>()),
            _ => None,
        })
        .unwrap_or_default();
    format!("{} {} {} {pedido}", c.titulo, c.cwd.to_string_lossy(), modelo_do_chat(c))
}

/// Texto que não cabe é cortado na borda, sem reticências (o dono não as quer nos títulos).
fn cortado() -> Div {
    div().min_w_0().overflow_hidden().whitespace_nowrap()
}

fn icone_da_coluna(coluna: Coluna) -> Lucide {
    match coluna {
        Coluna::Ideias => Lucide::Lightbulb,
        Coluna::Proximas => Lucide::ListTodo,
        Coluna::Fazendo => Lucide::CirclePlay,
        Coluna::Feitas => Lucide::CircleCheckBig,
    }
}

/// O fundo de um cartão do quadro: o cinza da caixa sobre o preto da raia, um degrau mais claro com
/// o ponteiro, sem borda (o protótipo).
fn base_do_cartao(id: impl Into<ElementId>, grupo: SharedString) -> Stateful<Div> {
    let fundo = Cores::cartao_elevado();
    div()
        .id(id)
        .group(grupo)
        .relative()
        .flex_none()
        .px(esp::E12)
        .py(esp::E8)
        .rounded(raio::controle())
        .bg(fundo)
        .hover(move |s| s.bg(fundo.blend(Cores::hover())))
        .cursor_pointer()
}

impl QuadroDeIdeias {
    fn new(
        motor: Entity<Motor>,
        janela: WeakEntity<Janela>,
        pasta_padrao: Option<PathBuf>,
        do_chat: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let origem = origem_lembrada();
        let nova = cx.new(|cx| InputState::new(window, cx).placeholder(dica_da_nova(origem, do_chat.is_some())));
        let busca = cx.new(|cx| InputState::new(window, cx).placeholder(tr!("Buscar")));
        let assinatura = cx.subscribe_in(&nova, window, |this, campo, evento: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = evento {
                let titulo = campo.read(cx).value().to_string();
                let chat = this.do_chat.clone();
                // A ideia de um chat fica na pasta dele; a solta, na do filtro ou na do chat à vista.
                let pasta = match chat.as_deref().and_then(|c| this.motor.read(cx).chat(c)) {
                    Some(c) if !this.motor.read(cx).sem_pasta(c) => Some(c.cwd.clone()),
                    Some(_) => None,
                    None => this.pasta.clone().or_else(|| this.pasta_padrao.clone()),
                };
                let so_no_vitral = this.origem == Origem::Vitral;
                if this.motor.update(cx, |m, cx| m.nova_ideia(&titulo, pasta, chat, so_no_vitral, cx)).is_some() {
                    campo.update(cx, |c, cx| c.set_value("", window, cx));
                }
            }
        });
        let ao_buscar = cx.subscribe(&busca, |_, _, evento: &InputEvent, cx| {
            if let InputEvent::Change = evento {
                cx.notify();
            }
        });
        // Lê o Linear agora e a cada minuto, enquanto o quadro estiver aberto (a tarefa morre com ele).
        motor.update(cx, |m, cx| m.ler_linear(cx));
        let releitura = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(RELER_A_CADA).await;
            if this.update(cx, |q, cx| q.motor.update(cx, |m, cx| m.ler_linear(cx))).is_err() {
                break;
            }
        });
        QuadroDeIdeias {
            motor,
            janela,
            nova,
            busca,
            pasta_padrao,
            do_chat,
            origem,
            agente: None,
            pasta: None,
            feitas_todas: false,
            edicao: None,
            conexao: None,
            _assinaturas: vec![assinatura, ao_buscar],
            _releitura: releitura,
        }
    }

    /// A tela de ligar o Linear (a chave pessoal).
    fn abrir_conexao(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let campo = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("lin_api_…"));
        let foco = campo.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        self.conexao = Some(campo);
        cx.notify();
    }

    fn editar(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ideia) = self.motor.read(cx).quadro().achar(id).cloned() else {
            return;
        };
        let titulo = cx.new(|cx| InputState::new(window, cx).placeholder(tr!("Título")).default_value(ideia.titulo.clone()));
        let pedido = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(5, 14)
                .placeholder(tr!("O que o chat deve fazer: o contexto, onde mexer, como saber que ficou pronto."))
                .default_value(ideia.pedido.clone())
        });
        let (id_t, id_p) = (ideia.id.clone(), ideia.id.clone());
        self._assinaturas.truncate(2);
        self._assinaturas.push(cx.subscribe_in(&titulo, window, move |this, campo, evento: &InputEvent, _, cx| {
            if let InputEvent::Change = evento {
                let texto = campo.read(cx).value().to_string();
                this.motor.update(cx, |m, cx| m.editar_ideia(&id_t, |i| i.titulo = texto, cx));
            }
        }));
        self._assinaturas.push(cx.subscribe_in(&pedido, window, move |this, campo, evento: &InputEvent, _, cx| {
            if let InputEvent::Change = evento {
                let texto = campo.read(cx).value().to_string();
                this.motor.update(cx, |m, cx| m.editar_ideia(&id_p, |i| i.pedido = texto, cx));
            }
        }));
        let foco = titulo.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        self.edicao = Some(Edicao { id: ideia.id, titulo, pedido });
        cx.notify();
    }

    /// Volta ao quadro. Um título apagado vira a primeira linha do pedido.
    fn voltar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.conexao = None;
        if let Some(edicao) = self.edicao.take() {
            self.motor.update(cx, |m, cx| {
                m.editar_ideia(
                    &edicao.id,
                    |i| {
                        i.titulo = i.titulo.trim().to_string();
                        if i.titulo.is_empty() {
                            i.titulo = i.pedido.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("Sem título").chars().take(60).collect();
                        }
                    },
                    cx,
                )
            });
        }
        self._assinaturas.truncate(2);
        let foco = self.nova.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    /// "Mandar no chat" (a ideia de um chat vai para ele, na fila) ou "Abrir chat" (um novo, com o
    /// pedido; `novo` força o novo mesmo com chat). O quadro fecha e a conversa aparece.
    fn mandar(&mut self, id: &str, novo: bool, window: &mut Window, cx: &mut Context<Self>) {
        let pasta = self.pasta_padrao.clone();
        match self.motor.update(cx, |m, cx| m.mandar_ideia(id, pasta, novo, cx)) {
            Ok(chat) => self.ir_ao_chat(chat, window, cx),
            Err(erro) => window.push_notification(aviso(Tom::Erro, tr!("Não mandou a ideia: {erro}", erro = erro)), cx),
        }
    }

    /// Abre a conversa no lugar do quadro.
    fn ir_ao_chat(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.janela.update(cx, |j, cx| {
            j.fechar_quadro(window, cx);
            j.selecionar(Some(chat), window, cx);
        });
    }

    fn fechar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.janela.update(cx, |j, cx| j.fechar_quadro(window, cx));
    }

    fn apagar(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(apagada) = self.motor.update(cx, |m, cx| m.apagar_ideia(id, cx)) else {
            return;
        };
        if self.edicao.as_ref().is_some_and(|e| e.id == id) {
            self.edicao = None;
            self._assinaturas.truncate(2);
        }
        let motor = self.motor.clone();
        let texto = tr!("Ideia apagada: {titulo}", titulo = apagada.titulo);
        window.push_notification(
            aviso_com_acao(Tom::Info, texto, "Desfazer", move |_, cx| {
                let ideia = apagada.clone();
                motor.update(cx, |m, cx| m.devolver_ideia(ideia, cx));
            }),
            cx,
        );
        cx.notify();
    }

    /// O termo da busca, sem acento e em minúscula (vazio: sem busca).
    fn termo(&self, cx: &App) -> String {
        super::acentos::normalizar(self.busca.read(cx).value().trim())
    }

    /// As ideias que o quadro mostra: as da origem escolhida, as do chat do quadro de um chat, as do
    /// agente e da pasta dos filtros, e as que têm o termo da busca.
    fn mostra_ideia(&self, ideia: &Ideia, m: &Motor, termo: &str) -> bool {
        let chat = chat_vivo(m, ideia);
        (!m.linear_ligado() || self.origem.aceita(ideia))
            && self.do_chat.as_ref().is_none_or(|c| ideia.chat.as_ref() == Some(c))
            && self.agente.is_none_or(|a| chat.is_none_or(|c| c.agente == a))
            && self.pasta.as_ref().is_none_or(|p| ideia.pasta.as_ref() == Some(p) || chat.is_some_and(|c| c.cwd == *p))
            && (termo.is_empty()
                || super::acentos::normalizar(&format!("{} {} {}", ideia.titulo, ideia.pedido, ideia.nota)).contains(termo))
    }

    /// Os chats que o quadro mostra, pelos mesmos filtros.
    fn mostra_chat(&self, c: &Chat, termo: &str) -> bool {
        !c.arquivado
            && self.do_chat.as_ref().is_none_or(|d| *d == c.id)
            && self.agente.is_none_or(|a| c.agente == a)
            && self.pasta.as_ref().is_none_or(|p| c.cwd == *p)
            && (termo.is_empty() || super::acentos::normalizar(&texto_do_chat(c)).contains(termo))
    }

    fn escolher_origem(&mut self, origem: Origem, window: &mut Window, cx: &mut Context<Self>) {
        self.origem = origem;
        ORIGEM.store(Origem::TODAS.iter().position(|o| *o == origem).unwrap_or(0) as u8, Ordering::Relaxed);
        let dica = dica_da_nova(origem, self.do_chat.is_some());
        self.nova.update(cx, |c, cx| c.set_placeholder(dica, window, cx));
        cx.notify();
    }

    /// Todas, Linear ou Vitral, com quantas há em cada (só com o Linear ligado).
    fn filtro_de_origem(&self, m: &Motor, cx: &Context<Self>) -> Option<AnyElement> {
        if !m.linear_ligado() {
            return None;
        }
        let do_quadro = |o: Origem| m.quadro().ideias.iter().filter(|i| o.aceita(i) && self.do_chat.as_ref().is_none_or(|c| i.chat.as_ref() == Some(c))).count();
        let segmentos = [
            Segmento::new(tr!("Todas {n}", n = do_quadro(Origem::Todas))),
            Segmento::new(format!("Linear {}", do_quadro(Origem::Linear))).dica(tr!("As issues com a etiqueta Ideia no Linear")),
            Segmento::new(format!("Vitral {}", do_quadro(Origem::Vitral))).dica(tr!("As que ficam só no Vitral; a nova anotada aqui também fica")),
        ];
        let atual = Origem::TODAS.iter().position(|o| *o == self.origem).unwrap_or(0);
        let vista = cx.entity().downgrade();
        Some(
            ControleSegmentado::new("origem-das-ideias", segmentos, atual, move |n, window, cx| {
                let _ = vista.update(cx, |q, cx| q.escolher_origem(Origem::TODAS[n], window, cx));
            })
            .with_size(Size::Small)
            .into_any_element(),
        )
    }

    /// Todos os agentes ou só um (só os que têm chat aparecem).
    fn filtro_de_agente(&self, m: &Motor, cx: &Context<Self>) -> Option<AnyElement> {
        let presentes: Vec<Agente> =
            [Agente::Claude, Agente::Codex, Agente::Gemini].into_iter().filter(|a| m.chats.iter().any(|c| !c.arquivado && c.agente == *a)).collect();
        if presentes.len() < 2 {
            return None;
        }
        let mut segmentos = vec![Segmento::new(tr!("Todos"))];
        segmentos.extend(presentes.iter().map(|a| Segmento::new(Marca::do_agente(*a).nome())));
        let atual = self.agente.and_then(|a| presentes.iter().position(|p| *p == a)).map_or(0, |i| i + 1);
        let vista = cx.entity().downgrade();
        Some(
            ControleSegmentado::new("agente-do-quadro", segmentos, atual, move |n, _, cx| {
                let agente = n.checked_sub(1).and_then(|i| presentes.get(i).copied());
                let _ = vista.update(cx, |q, cx| {
                    q.agente = agente;
                    cx.notify();
                });
            })
            .with_size(Size::Small)
            .into_any_element(),
        )
    }

    /// Todas as pastas ou só uma.
    fn filtro_de_pasta(&self, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let mut opcoes = vec![Opcao::new("", tr!("Todas as pastas"))];
        opcoes.extend(m.pastas_dos_chats().iter().map(|p| Opcao::new(p.to_string_lossy(), nome_da_pasta(p)).descricao(com_til(p))));
        let atual = self.pasta.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let vista = cx.entity().downgrade();
        menu_escolha(
            "pasta-do-quadro",
            Anchor::TopLeft,
            opcoes,
            &atual,
            "",
            move |valor, _, cx| {
                let pasta = (!valor.is_empty()).then(|| PathBuf::from(valor));
                let _ = vista.update(cx, |q, cx| {
                    q.pasta = pasta;
                    cx.notify();
                });
            },
            window,
            cx,
        )
    }

    /// O cabeçalho: o nome, como está o Linear e os botões dele, e o fechar.
    fn cabecalho_do_quadro(&self, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let (texto, cor, botoes): (String, Hsla, Vec<AnyElement>) = match m.situacao_do_linear() {
            SituacaoDoLinear::Desligado { erro, .. } => (
                erro.unwrap_or_default(),
                Cores::vermelho(),
                vec![
                    botao_fantasma("ligar-linear", Some(Lucide::Link.into()), tr!("Ligar o Linear"), TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|q, _, window, cx| q.abrir_conexao(window, cx)))
                        .into_any_element(),
                ],
            ),
            SituacaoDoLinear::Ligado { time, lendo, lido_em, erro } => {
                let time = time.unwrap_or_else(|| tr!("o time").to_string());
                let texto = match (&erro, lendo, lido_em) {
                    (Some(erro), ..) => erro.clone(),
                    (None, true, _) => tr!("Lendo o Linear · {time}", time = time).to_string(),
                    (None, false, Some(quando)) => match tempo_relativo(quando) {
                        agora if agora == "agora" => tr!("Linear · {time} · lido agora", time = time).to_string(),
                        ha => tr!("Linear · {time} · lido há {ha}", time = time, ha = ha).to_string(),
                    },
                    (None, false, None) => format!("Linear · {time}"),
                };
                let botoes = vec![
                    botao_icone("reler-linear", Lucide::RotateCcw, tr!("Ler o Linear agora"), TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|q, _, _, cx| q.motor.update(cx, |m, cx| m.ler_linear(cx))))
                        .into_any_element(),
                    botao_icone("desligar-linear", Lucide::Unplug, tr!("Desligar o Linear (as ideias de lá saem do quadro)"), TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|q, _, _, cx| q.motor.update(cx, |m, cx| m.desligar_linear(cx))))
                        .into_any_element(),
                ];
                (texto, if erro.is_some() { Cores::vermelho() } else { Cores::apagado() }, botoes)
            }
        };
        h_flex()
            .flex_none()
            .h(r(32.))
            .gap(esp::E8)
            .child(div().flex_none().text_color(Cores::tenue()).child(icone(Lucide::Kanban, 18., window)))
            .child(div().flex_none().text_size(r(18.)).line_height(r(24.)).font_weight(FontWeight::SEMIBOLD).text_color(Cores::forte()).child(tr!("Tarefas")))
            // Carregando: o anel do "trabalhando" ao lado do texto, até o Linear responder.
            .children(m.lendo_o_linear().then(|| div().flex_none().child(AnelGirando::new(14.).cor(Cores::destaque()).vivo("ideias-lendo"))))
            .child(cortado().flex_1().metadado().text_color(cor).child(texto))
            .children(botoes)
            .child(
                botao_icone("fechar-quadro", Lucide::X, tr!("Voltar às conversas (Esc)"), TamanhoBotao::Linha, window, cx)
                    .on_click(cx.listener(|q, _, window, cx| q.fechar(window, cx))),
            )
            .into_any_element()
    }

    /// A fileira dos filtros: o chat do quadro de um chat (com o × que mostra todos), o agente, a
    /// pasta, a origem das ideias e a busca.
    fn filtros(&self, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let do_chat = self.do_chat.as_deref().and_then(|c| m.chat(c)).map(|c| {
            h_flex()
                .id("filtro-do-chat")
                .flex_none()
                .min_w_0()
                .max_w(r(320.))
                .h(r(24.))
                .pl(esp::E8)
                .pr(esp::E2)
                .gap(esp::E4)
                .rounded(raio::de(24.))
                .bg(Cores::selecao())
                .child(div().flex_none().text_color(Cores::tenue()).child(icone(Lucide::MessageSquare, 12., window)))
                .child(cortado().metadado().text_color(Cores::texto()).child(c.titulo.clone()))
                .child(
                    botao_icone("tirar-filtro", Lucide::X, tr!("Ver todos os chats e ideias"), TamanhoBotao::Linha, window, cx).on_click(cx.listener(|q, _, _, cx| {
                        q.do_chat = None;
                        cx.notify();
                    })),
                )
        });
        h_flex()
            .flex_none()
            .w_full()
            .gap(esp::E8)
            .children(do_chat)
            .children(self.filtro_de_agente(m, cx))
            .child(self.filtro_de_pasta(m, window, cx))
            .children(self.filtro_de_origem(m, cx))
            .child(div().flex_1())
            .child(div().flex_none().w(r(240.)).child(campo_de_busca_pequeno("busca-do-quadro", &self.busca, window, cx)))
            .into_any_element()
    }

    /// Uma raia: o cabeçalho (o ícone do estado, o nome e quantos há) e o corpo que rola, no preto
    /// das peças flutuantes, com os cartões no cinza da caixa (o protótipo).
    #[allow(clippy::too_many_arguments)]
    fn raia(&self, raia: Raia, cartoes: Vec<(SharedString, AnyElement)>, n: usize, extra: Option<AnyElement>, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        // Os cartões entram em cascata ao abrir o quadro (o `.card` do protótipo: subindo, 60 ms
        // por raia e 35 por cartão); um cartão que aparece depois entra sozinho, ao montar. Só os
        // primeiros escalonam, para a raia comprida não demorar.
        let cartoes: Vec<AnyElement> = cartoes
            .into_iter()
            .enumerate()
            .map(|(i, (chave, cartao))| {
                let atraso = std::time::Duration::from_millis(raia as u64 * 60 + i.min(10) as u64 * 35);
                // A chave é a do chat ou da ideia, com a raia: o cartão que muda de lugar na raia não toca
                // de novo, e o que muda de raia entra nela.
                super::movimento::entrar_depois_em(div().child(cartao), chave, super::movimento::De::Baixo, atraso, super::movimento::CARTAO).into_any_element()
            })
            .collect();
        let mais = (raia == Raia::Ideias).then(|| {
            let foco = self.nova.read(cx).focus_handle(cx);
            botao_icone("nova-ideia", Lucide::Plus, tr!("Nova ideia"), TamanhoBotao::Linha, window, cx).on_click(move |_, window, cx| window.focus(&foco, cx))
        });
        let cabeca = h_flex()
            .flex_none()
            .h(r(32.))
            .px(esp::E8)
            .gap(esp::E8)
            .child(div().flex_none().text_color(raia.cor()).child(icone(raia.icone(), 16., window)))
            .child(div().flex_none().text_size(r(14.)).line_height(r(18.)).font_weight(FontWeight::SEMIBOLD).text_color(Cores::texto()).child(raia.titulo()))
            .child(div().flex_none().metadado().text_color(Cores::tenue()).child(n.to_string()))
            .child(div().flex_1())
            .children(mais);
        // O campo de ideia nova no cinza dos cartões, sem a borda clara (o `.nova-ideia` do protótipo).
        let entrada = (raia == Raia::Ideias).then(|| {
            div()
                .flex_none()
                .px(esp::E4)
                .rounded(raio::controle())
                .bg(Cores::cartao_elevado())
                .child(Input::new(&self.nova).small().appearance(false))
        });
        // Raia vazia enquanto o Linear não respondeu: o anel girando, e não o "escreva aqui".
        let vazia = (n == 0).then(|| {
            if raia == Raia::Ideias && m.lendo_o_linear() {
                v_flex()
                    .items_center()
                    .gap(esp::E8)
                    .py(esp::E16)
                    .child(AnelGirando::new(18.).cor(Cores::destaque()).vivo("ideias-lendo-raia"))
                    .child(div().metadado().text_color(Cores::apagado()).child(tr!("Lendo o Linear")))
            } else {
                v_flex().px(esp::E8).py(esp::E12).text_center().metadado().text_color(Cores::apagado()).child(raia.vazia())
            }
        });
        let corpo = div()
            .id(("raia-do-quadro", raia as usize))
            .flex_1()
            .min_h_0()
            .rounded(raio::cartao())
            .bg(Cores::flutuante())
            .overflow_y_scroll()
            .when_some(raia.coluna_da_ideia(), |d, coluna| {
                d.drag_over::<ArrastoIdeia>(|s, _, _, _| s.bg(Cores::tinta_leve(Cores::destaque()))).on_drop(cx.listener(
                    move |this, a: &ArrastoIdeia, _, cx| {
                        this.motor.update(cx, |m, cx| m.mover_ideia(&a.id, coluna, None, cx));
                    },
                ))
            })
            .child(v_flex().p(esp::E8).gap(esp::E8).children(entrada).children(cartoes).children(extra).children(vazia));
        v_flex().flex_1().min_w(r(LARGURA_DA_RAIA)).h_full().gap(esp::E6).child(cabeca).child(corpo).into_any_element()
    }

    /// O cartão de uma ideia: o título, o porquê ou o começo do pedido, e embaixo o código da
    /// issue, quem pegou e o chat dela (ou a pasta). Arrasta entre as raias de ideia.
    fn cartao(&self, ideia: &Ideia, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let grupo: SharedString = format!("ideia-{}", ideia.id).into();
        let titulo: SharedString = if ideia.titulo.trim().is_empty() { tr!("Sem título") } else { ideia.titulo.clone().into() };
        // Embaixo do título: o porquê, ou o começo do pedido quando ele diz mais que o título.
        let detalhe = if !ideia.nota.trim().is_empty() {
            Some(ideia.nota.trim().to_string())
        } else {
            let pedido = ideia.pedido.trim();
            (!pedido.is_empty() && pedido != ideia.titulo.trim()).then(|| pedido.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" "))
        };
        let chat = chat_vivo(m, ideia);
        let mut fatos: Vec<AnyElement> = Vec::new();
        if ideia.coluna == Coluna::Proximas {
            fatos.push(Etiqueta::new(tr!("Próxima")).icone(Lucide::ListTodo).baixa().into_any_element());
        }
        if let Some(no) = &ideia.linear {
            fatos.push(Etiqueta::new(no.identificador.clone()).baixa().into_any_element());
            if let Some(quem) = &no.responsavel {
                fatos.push(Etiqueta::new(quem.clone()).icone(Lucide::CircleUser).baixa().into_any_element());
            }
        }
        fatos.extend(ideia.etiquetas.iter().map(|e| Etiqueta::new(e.clone()).baixa().into_any_element()));
        match chat {
            // No quadro de um chat, todos os cartões são dele: o estado basta.
            Some(c) => {
                let texto = if self.do_chat.is_some() { estado_do_chat(m, c).to_string() } else { c.titulo.clone() };
                fatos.push(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(esp::E4)
                        .child(icone_de_estado(sinal_do_chat(m, c), 12.).vivo(format!("ideia-{}", ideia.id)))
                        .child(cortado().flex_1().metadado().text_color(Cores::tenue()).child(texto))
                        .into_any_element(),
                );
            }
            None => fatos.extend(ideia.pasta.as_ref().map(|p| Etiqueta::new(nome_da_pasta(p)).icone(Lucide::Folder).baixa().into_any_element())),
        }
        let id = ideia.id.clone();
        let acao = match chat {
            Some(c) => botao_icone(
                SharedString::from(format!("mandar-{}", ideia.id)),
                Lucide::Send,
                tr!("Mandar no chat “{titulo}”", titulo = c.titulo),
                TamanhoBotao::Linha,
                window,
                cx,
            ),
            None => botao_icone(SharedString::from(format!("mandar-{}", ideia.id)), Lucide::Send, tr!("Abrir chat com esta ideia"), TamanhoBotao::Linha, window, cx),
        }
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.mandar(&id, false, window, cx);
        }));
        let (id_clique, id_soltura, coluna) = (ideia.id.clone(), ideia.id.clone(), ideia.coluna);
        let arrasto = ArrastoIdeia { id: ideia.id.clone(), titulo: titulo.clone() };
        let vista = cx.entity().downgrade();
        let menu = DadosDoMenu {
            id: ideia.id.clone(),
            coluna,
            chat: chat.map(|c| c.titulo.clone()),
            linear: ideia.linear.as_ref().map(|no| no.url.clone()).filter(|url| !url.is_empty()),
        };
        base_do_cartao(grupo.clone(), grupo.clone())
            .child(
                v_flex()
                    .gap(esp::E4)
                    .child(div().text_size(r(14.)).line_height(r(19.)).font_weight(FontWeight::SEMIBOLD).text_color(Cores::forte()).line_clamp(2).child(titulo.clone()))
                    .children(detalhe.map(|d| div().metadado().text_color(Cores::tenue()).line_clamp(2).child(d)))
                    .child(
                        h_flex()
                            .pt(esp::E2)
                            .gap(esp::E4)
                            .h(r(22.))
                            .child(h_flex().flex_1().min_w_0().overflow_hidden().gap(esp::E4).children(fatos))
                            .child(div().flex_none().invisible().group_hover(grupo.clone(), |s| s.visible()).child(acao)),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.editar(&id_clique, window, cx)))
            .on_drag(arrasto, |a, _, _, cx| cx.new(|_| PreviaArrasto { titulo: a.titulo.clone(), simbolo: Some(Simbolo::icone(Lucide::Lightbulb)) }))
            // Solto em cima de um cartão, o arrastado entra logo antes dele.
            .drag_over::<ArrastoIdeia>(|s, _, _, _| s.bg(Cores::tinta_leve(Cores::destaque())))
            .on_drop(cx.listener(move |this, a: &ArrastoIdeia, _, cx| {
                cx.stop_propagation();
                this.motor.update(cx, |m, cx| m.mover_ideia(&a.id, coluna, Some(&id_soltura), cx));
            }))
            .context_menu(move |menu_vazio, _, _| menu_do_cartao(menu_vazio, vista.clone(), &menu))
            .into_any_element()
    }

    /// O cartão de um chat: o estado, a marca do agente e a pasta em cima (com parar e arquivar
    /// no ponteiro), o título, o que ele pede ou faz, os selos e há quanto tempo mexeu. Esperando
    /// uma permissão, os botões de responder; o clique abre a conversa.
    #[allow(clippy::too_many_arguments)]
    fn cartao_do_chat(&self, c: &Chat, raia: Raia, ideias: usize, pendencia: Option<&Pendencia>, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let grupo: SharedString = format!("chat-no-quadro-{}", c.id).into();
        let id_botao = |nome: &str| SharedString::from(format!("{nome}-{}", c.id));
        let mut acoes: Vec<AnyElement> = Vec::new();
        if raia == Raia::Trabalhando {
            let (motor, chat) = (self.motor.clone(), c.id.clone());
            acoes.push(
                botao_icone(id_botao("parar"), Lucide::Square, tr!("Parar"), TamanhoBotao::Linha, window, cx)
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        motor.update(cx, |m, cx| m.parar(&chat, cx));
                    })
                    .into_any_element(),
            );
        } else {
            let (motor, chat) = (self.motor.clone(), c.id.clone());
            acoes.push(
                botao_icone(id_botao("arquivar"), Lucide::Archive, tr!("Arquivar"), TamanhoBotao::Linha, window, cx)
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        motor.update(cx, |m, cx| m.arquivar_chat(&chat, cx));
                    })
                    .into_any_element(),
            );
        }
        let topo = h_flex()
            .h(r(20.))
            .gap(esp::E6)
            .child(icone_de_estado(sinal_do_chat(m, c), 14.).vivo(format!("quadro-{}", c.id)))
            .child(Marca::do_agente(c.agente).glifo(12.))
            .child(cortado().flex_1().metadado().text_color(Cores::tenue()).child(nome_da_pasta(&c.cwd)))
            .child(h_flex().flex_none().gap(esp::E2).invisible().group_hover(grupo.clone(), |s| s.visible()).children(acoes));
        let detalhe = detalhe_do_chat(c, raia, pendencia);
        let mut selos: Vec<AnyElement> = vec![Etiqueta::new(modelo_do_chat(c)).baixa().into_any_element()];
        if ideias > 0 {
            let texto = if ideias == 1 { tr!("Ideia").to_string() } else { tr!("{n} ideias", n = ideias).to_string() };
            selos.push(Etiqueta::new(texto).icone(Lucide::Lightbulb).baixa().into_any_element());
        }
        if raia == Raia::Trabalhando {
            let vivos = super::mapa_agentes::agentes_do_chat(&c.itens, true, agora())
                .iter()
                .filter(|a| a.estado == super::mapa_agentes::Estado::Rodando)
                .count();
            if vivos > 0 {
                let texto = if vivos == 1 { tr!("1 agente").to_string() } else { tr!("{n} agentes", n = vivos).to_string() };
                selos.push(Etiqueta::new(texto).icone(Lucide::UsersRound).baixa().into_any_element());
            }
        }
        if c.fixado {
            selos.push(Etiqueta::new(tr!("Fixada")).icone(Lucide::Pin).baixa().into_any_element());
        }
        // A permissão se responde aqui mesmo, como no aviso: o comando perigoso ou comprido leva à
        // conversa, onde o cartão mostra o comando inteiro.
        let botoes = match pendencia {
            Some(Pendencia::Permissao { pedido, ferramenta, .. }) => {
                let comando = m.permissoes.iter().find(|x| x.id == *pedido).and_then(|x| x.entrada["command"].as_str().map(str::to_string));
                let perigoso = ferramenta == "Bash" && comando.as_deref().is_some_and(|c| !super::mensagens::trechos_perigosos(c).is_empty());
                let comprido = comando.as_deref().is_some_and(|c| c.lines().count() > 2 || c.chars().count() > 160);
                let (m1, m2, p1, p2) = (self.motor.clone(), self.motor.clone(), pedido.clone(), pedido.clone());
                let negar = botao_fantasma(id_botao("negar"), None, tr!("Negar"), TamanhoBotao::Linha, window, cx).on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    m1.update(cx, |m, cx| m.responder_permissao(&p1, false, None, cx));
                });
                let sim = if perigoso || comprido {
                    let chat = c.id.clone();
                    botao_fantasma(id_botao("ver-comando"), Some(Lucide::ArrowRight.into()), tr!("Ver o comando"), TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.ir_ao_chat(chat.clone(), window, cx);
                        }))
                        .into_any_element()
                } else {
                    botao_primario(id_botao("permitir"), None, tr!("Permitir"), TamanhoBotao::Linha, window, cx)
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            m2.update(cx, |m, cx| m.responder_permissao(&p2, true, None, cx));
                        })
                        .into_any_element()
                };
                Some(h_flex().pt(esp::E2).gap(esp::E6).child(negar).child(sim))
            }
            _ => None,
        };
        let chat = c.id.clone();
        let menu = DadosDoChat { chat: c.id.clone(), rodando: raia == Raia::Trabalhando, fixado: c.fixado };
        let vista = cx.entity().downgrade();
        base_do_cartao(grupo.clone(), grupo.clone())
            .child(
                v_flex()
                    .gap(esp::E4)
                    .child(topo)
                    .child(div().text_size(r(14.)).line_height(r(19.)).font_weight(FontWeight::SEMIBOLD).text_color(Cores::forte()).line_clamp(2).child(c.titulo.clone()))
                    .children(detalhe.map(|d| div().metadado().text_color(Cores::tenue()).line_clamp(2).child(d)))
                    .children(botoes)
                    .child(
                        h_flex()
                            .pt(esp::E2)
                            .gap(esp::E4)
                            .child(h_flex().flex_1().min_w_0().overflow_hidden().gap(esp::E4).children(selos))
                            .child(div().flex_none().metadado().text_color(Cores::apagado()).child(match tempo_relativo(c.atualizado_em) {
                                agora if agora == "agora" => tr!("agora").to_string(),
                                ha => tr!("há {ha}", ha = ha).to_string(),
                            })),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.ir_ao_chat(chat.clone(), window, cx)))
            .context_menu(move |menu_vazio, _, _| menu_do_chat(menu_vazio, vista.clone(), &menu))
            .into_any_element()
    }

    fn tela_da_conexao(&self, campo: &Entity<InputState>, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let (ligando, erro, recusas) = match m.situacao_do_linear() {
            SituacaoDoLinear::Desligado { ligando, erro, recusas } => (ligando, erro, recusas),
            SituacaoDoLinear::Ligado { .. } => (false, None, 0),
        };
        // A chave recusada: o campo fica vermelho e treme (de novo a cada recusa).
        let recusada = erro.as_deref() == Some(crate::linear::RECUSOU_A_CHAVE);
        let caixa = Input::new(campo).when(recusada, |c| c.focus_bordered(false).border_color(Cores::vermelho()));
        let caixa = if recusada {
            super::movimento::tremer(div().relative().child(caixa), ("chave-linear", recusas as usize)).into_any_element()
        } else {
            caixa.into_any_element()
        };
        let campo_ligar = campo.clone();
        let ligar = botao_primario("testar-linear", None, if ligando { tr!("Testando") } else { tr!("Testar e ligar") }, TamanhoBotao::Controle, window, cx)
            .on_click(cx.listener(move |q, _, _, cx| {
                let chave = campo_ligar.read(cx).value().to_string();
                q.motor.update(cx, |m, cx| m.ligar_linear(chave, cx));
            }));
        let pagina = botao_fantasma("pagina-chave-linear", Some(Lucide::ExternalLink.into()), tr!("Criar a chave no Linear"), TamanhoBotao::Controle, window, cx)
            .on_click(|_, _, cx| cx.open_url(crate::linear::PAGINA_DA_CHAVE));
        let vista = cx.entity().downgrade();
        v_flex()
            .w_full()
            .gap(esp::E16)
            .child(CabecalhoDialogo::new(tr!("Ligar o Linear")).sem_fechar().voltar(move |window, cx| {
                let _ = vista.update(cx, |q, cx| q.voltar(window, cx));
            }))
            .child(
                v_flex()
                    .w_full()
                    .max_w(r(560.))
                    .mx_auto()
                    .gap(esp::E12)
                    .child(div().corpo_ui().text_color(Cores::tenue()).child(
                        tr!("Cole a sua chave pessoal do Linear (Settings, Security & access, Personal API keys). As ideias passam a ser as issues com a etiqueta Ideia do time EFI, e o que você muda aqui muda lá."),
                    ))
                    .child(caixa)
                    .children(erro.map(|e| div().metadado().text_color(Cores::vermelho()).child(e)))
                    .child(div().metadado().text_color(Cores::apagado()).child(tr!(
                        "Fica em {pasta}, só para você.",
                        pasta = com_til(&crate::linear::arquivo_da_chave(&m.dados.config))
                    )))
                    .child(h_flex().pt(esp::E8).gap(esp::E8).child(pagina).child(div().flex_1()).child(ligar)),
            )
            .into_any_element()
    }

    fn tela_da_edicao(&self, edicao: &Edicao, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let Some(ideia) = m.quadro().achar(&edicao.id) else {
            return div().into_any_element();
        };
        let rotulo = |t: SharedString| div().flex_none().rotulo().text_color(Cores::texto()).child(t);
        let chat = chat_vivo(m, ideia);

        // O chat da ideia: um novo (na pasta escolhida) ou uma conversa que já existe.
        let mut conversas: Vec<&crate::modelo::Chat> = m.chats.iter().filter(|c| !c.arquivado).collect();
        conversas.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
        let mut opcoes = vec![Opcao::new("", tr!("Um chat novo")).descricao(tr!("Criado com o pedido, quando você mandar"))];
        let atual = chat.map(|c| c.id.clone()).unwrap_or_default();
        let fora_da_lista = chat.filter(|c| !conversas.iter().take(CHATS_NA_ESCOLHA).any(|o| o.id == c.id));
        for c in conversas.iter().take(CHATS_NA_ESCOLHA).copied().chain(fora_da_lista) {
            opcoes.push(Opcao::new(c.id.clone(), c.titulo.clone()).descricao(nome_da_pasta(&c.cwd)));
        }
        let (motor, id) = (self.motor.clone(), ideia.id.clone());
        let escolha_do_chat = menu_escolha(
            "chat-da-ideia",
            Anchor::TopLeft,
            opcoes,
            &atual,
            &atual,
            move |valor, _, cx| {
                let pasta = motor.read(cx).chat(&valor).filter(|c| !motor.read(cx).sem_pasta(c)).map(|c| c.cwd.clone());
                motor.update(cx, |m, cx| {
                    m.editar_ideia(
                        &id,
                        |i| {
                            i.chat = (!valor.is_empty()).then(|| valor.clone());
                            if i.chat.is_some() {
                                i.pasta = pasta;
                            }
                        },
                        cx,
                    )
                });
            },
            window,
            cx,
        );

        // A pasta, só para o chat novo: as dos chats, a do cartão e "Sem pasta". Sem pasta no
        // cartão (uma issue de outra pessoa), vale a do chat à vista ao abrir o quadro.
        let pasta = chat.is_none().then(|| {
            let pasta_do_cartao = ideia.pasta.clone().or_else(|| self.pasta_padrao.clone());
            let mut pastas = m.pastas_dos_chats();
            if let Some(p) = &pasta_do_cartao
                && !pastas.contains(p)
            {
                pastas.insert(0, p.clone());
            }
            let mut opcoes: Vec<Opcao> = pastas.iter().map(|p| Opcao::new(p.to_string_lossy(), nome_da_pasta(p)).descricao(com_til(p))).collect();
            opcoes.push(Opcao::new("", tr!("Sem pasta")).descricao(tr!("Uma pasta vazia só do chat")));
            let atual = pasta_do_cartao.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            let (motor, id) = (self.motor.clone(), ideia.id.clone());
            h_flex().gap(esp::E8).child(rotulo(tr!("Pasta"))).child(menu_escolha(
                "pasta-da-ideia",
                Anchor::TopLeft,
                opcoes,
                &atual,
                &atual,
                move |valor, _, cx| {
                    let pasta = (!valor.is_empty()).then(|| PathBuf::from(valor));
                    motor.update(cx, |m, cx| m.editar_ideia(&id, |i| i.pasta = pasta, cx));
                },
                window,
                cx,
            ))
        });
        let colunas: Vec<Opcao> = Coluna::TODAS.iter().map(|c| Opcao::new(c.titulo(), crate::i18n::traduzir(c.titulo()).into_owned())).collect();
        let (motor, id) = (self.motor.clone(), ideia.id.clone());
        let coluna = menu_escolha(
            "coluna-da-ideia",
            Anchor::TopLeft,
            colunas,
            ideia.coluna.titulo(),
            ideia.coluna.titulo(),
            move |valor, _, cx| {
                if let Some(c) = Coluna::TODAS.into_iter().find(|c| c.titulo() == valor) {
                    motor.update(cx, |m, cx| m.mover_ideia(&id, c, None, cx));
                }
            },
            window,
            cx,
        );
        let nota = (!ideia.nota.trim().is_empty()).then(|| {
            v_flex().gap(esp::E4).child(rotulo(tr!("Por quê"))).child(div().corpo_ui().text_color(Cores::tenue()).child(ideia.nota.clone()))
        });
        let (id_apagar, id_mandar) = (ideia.id.clone(), ideia.id.clone());
        let apagar = botao_perigo("apagar-ideia", Some(Lucide::Trash.into()), tr!("Apagar"), TamanhoBotao::Controle, window, cx)
            .on_click(cx.listener(move |this, _, window, cx| this.apagar(&id_apagar, window, cx)));
        let ir = chat.map(|c| {
            let chat = c.id.clone();
            botao_fantasma("ir-ao-chat-da-ideia", Some(Lucide::MessageSquare.into()), tr!("Ir ao chat"), TamanhoBotao::Controle, window, cx)
                .on_click(cx.listener(move |this, _, window, cx| this.ir_ao_chat(chat.clone(), window, cx)))
        });
        let no_linear = ideia.linear.as_ref().filter(|l| !l.url.is_empty()).map(|l| {
            let url = l.url.clone();
            botao_fantasma("abrir-no-linear", Some(Lucide::ExternalLink.into()), tr!("{id} no Linear", id = l.identificador), TamanhoBotao::Controle, window, cx)
                .on_click(move |_, _, cx| cx.open_url(&url))
        });
        let rotulo_mandar = if chat.is_some() { tr!("Mandar no chat") } else { tr!("Abrir chat") };
        let mandar = botao_primario("mandar-ideia", Some(Lucide::Send.into()), rotulo_mandar, TamanhoBotao::Controle, window, cx)
            .on_click(cx.listener(move |this, _, window, cx| this.mandar(&id_mandar, false, window, cx)));
        let vista = cx.entity().downgrade();
        v_flex()
            .w_full()
            .gap(esp::E16)
            .child(
                // Sem o × de diálogo: o quadro não é diálogo, e a seta (ou o Esc) volta.
                CabecalhoDialogo::new(tr!("Ideia"))
                    .sem_fechar()
                    .subtitulo(div().font_weight(FontWeight::NORMAL).child(if chat.is_some() {
                        tr!("O pedido entra na fila do chat dela")
                    } else {
                        tr!("O pedido vai como a primeira mensagem de um chat novo")
                    }))
                    .voltar(move |window, cx| {
                        let _ = vista.update(cx, |q, cx| q.voltar(window, cx));
                    }),
            )
            .child(
                v_flex()
                    .w_full()
                    .max_w(r(760.))
                    .mx_auto()
                    .gap(esp::E12)
                    .child(v_flex().gap(esp::E4).child(rotulo(tr!("Título"))).child(Input::new(&edicao.titulo)))
                    .child(v_flex().gap(esp::E4).child(rotulo(tr!("Pedido para o chat"))).child(Textarea::new(&edicao.pedido)).child(
                        div().metadado().text_color(Cores::apagado()).child(tr!("Vazio, vai o título. O texto se grava sozinho.")),
                    ))
                    .children(nota)
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_x(esp::E24)
                            .gap_y(esp::E8)
                            .child(h_flex().gap(esp::E8).child(rotulo(tr!("Chat"))).child(escolha_do_chat))
                            .children(pasta)
                            .child(h_flex().gap(esp::E8).child(rotulo(tr!("Coluna"))).child(coluna)),
                    )
                    .child(
                        h_flex()
                            .pt(esp::E8)
                            .gap(esp::E8)
                            .child(apagar)
                            .child(div().flex_1())
                            .children(no_linear)
                            .children(ir)
                            .child(mandar),
                    ),
            )
            .into_any_element()
    }
}

/// O que o botão direito de um cartão precisa saber.
struct DadosDoMenu {
    id: String,
    coluna: Coluna,
    /// O título do chat da ideia, quando ela tem um.
    chat: Option<String>,
    linear: Option<String>,
}

/// O botão direito num cartão.
fn menu_do_cartao(menu: PopupMenu, vista: WeakEntity<QuadroDeIdeias>, d: &DadosDoMenu) -> PopupMenu {
    let item = |rotulo: &str, icone: Lucide, f: Box<dyn Fn(&mut QuadroDeIdeias, &mut Window, &mut Context<QuadroDeIdeias>)>| {
        let vista = vista.clone();
        PopupMenuItem::new(rotulo.to_string()).icon(icone).on_click(move |_, window, cx| {
            let _ = vista.update(cx, |q, cx| f(q, window, cx));
        })
    };
    let id = d.id.clone();
    let (a, b, e, x) = (id.clone(), id.clone(), id.clone(), id.clone());
    let mut menu = match &d.chat {
        Some(titulo) => menu
            .item(item(&tr!("Mandar no chat “{titulo}”", titulo = titulo), Lucide::Send, Box::new(move |q, w, cx| q.mandar(&a, false, w, cx))))
            .item(item(&tr!("Abrir num chat novo"), Lucide::MessageSquarePlus, Box::new(move |q, w, cx| q.mandar(&b, true, w, cx)))),
        None => menu.item(item(&tr!("Abrir chat com esta ideia"), Lucide::Send, Box::new(move |q, w, cx| q.mandar(&a, true, w, cx)))),
    };
    menu = menu.item(item(&tr!("Editar"), Lucide::Pencil, Box::new(move |q, w, cx| q.editar(&e, w, cx))));
    if let Some(url) = d.linear.clone() {
        menu = menu.item(PopupMenuItem::new(tr!("Abrir no Linear")).icon(Lucide::ExternalLink).on_click(move |_, _, cx| cx.open_url(&url)));
    }
    menu = menu.separator();
    for destino in Coluna::TODAS.into_iter().filter(|c| *c != d.coluna) {
        let id = id.clone();
        menu = menu.item(item(&tr!("Mover para {coluna}", coluna = crate::i18n::traduzir(destino.titulo())), icone_da_coluna(destino), Box::new(move |q, _, cx| {
            q.motor.update(cx, |m, cx| m.mover_ideia(&id, destino, None, cx));
        })));
    }
    menu.separator().item(item(&tr!("Apagar"), Lucide::Trash, Box::new(move |q, w, cx| q.apagar(&x, w, cx))))
}

/// A dica do campo de ideia nova, pela origem e pelo quadro de um chat.
fn dica_da_nova(origem: Origem, do_chat: bool) -> SharedString {
    match (origem, do_chat) {
        (Origem::Vitral, _) => tr!("Nova ideia só no Vitral (Enter anota)"),
        (_, true) => tr!("Ideia para este chat (Enter anota)"),
        _ => tr!("Nova ideia (Enter anota)"),
    }
}

/// O que o botão direito no cartão de um chat precisa saber.
struct DadosDoChat {
    chat: String,
    rodando: bool,
    fixado: bool,
}

/// O botão direito no cartão de um chat: abrir, parar, fixar e arquivar.
fn menu_do_chat(menu: PopupMenu, vista: WeakEntity<QuadroDeIdeias>, d: &DadosDoChat) -> PopupMenu {
    let item = |rotulo: &str, icone: Lucide, f: Box<dyn Fn(&mut QuadroDeIdeias, &mut Window, &mut Context<QuadroDeIdeias>)>| {
        let vista = vista.clone();
        PopupMenuItem::new(rotulo.to_string()).icon(icone).on_click(move |_, window, cx| {
            let _ = vista.update(cx, |q, cx| f(q, window, cx));
        })
    };
    let (a, b, c, e) = (d.chat.clone(), d.chat.clone(), d.chat.clone(), d.chat.clone());
    let mut menu = menu.item(item(&tr!("Abrir a conversa"), Lucide::MessageSquare, Box::new(move |q, w, cx| q.ir_ao_chat(a.clone(), w, cx))));
    if d.rodando {
        menu = menu.item(item(&tr!("Parar"), Lucide::Square, Box::new(move |q, _, cx| q.motor.update(cx, |m, cx| m.parar(&b, cx)))));
    }
    menu.item(item(
        &if d.fixado { tr!("Desafixar") } else { tr!("Fixar") },
        if d.fixado { Lucide::PinOff } else { Lucide::Pin },
        Box::new(move |q, _, cx| q.motor.update(cx, |m, cx| m.alternar_fixado(&c, cx))),
    ))
    .separator()
    .item(item(&tr!("Arquivar"), Lucide::Archive, Box::new(move |q, _, cx| {
        q.motor.update(cx, |m, cx| {
            m.arquivar_chat(&e, cx);
        })
    })))
}

impl Render for QuadroDeIdeias {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let motor = self.motor.clone();
        let m = motor.read(cx);
        let tela = |filho: AnyElement| {
            div().id("quadro-tela").size_full().overflow_y_scroll().p(esp::E24).child(filho).into_any_element()
        };
        if let Some(campo) = &self.conexao
            && !m.linear_ligado()
        {
            return self.com_esc(tela(self.tela_da_conexao(campo, m, window, cx)), cx);
        }
        if let Some(edicao) = &self.edicao
            && m.quadro().achar(&edicao.id).is_some()
        {
            return self.com_esc(tela(self.tela_da_edicao(edicao, m, window, cx)), cx);
        }
        let termo = self.termo(cx);
        let mut por_raia: HashMap<usize, Vec<(SharedString, AnyElement)>> = HashMap::new();
        let mut contagem: HashMap<usize, usize> = HashMap::new();
        let mut por = |raia: Raia, chave: String, cartao: AnyElement| {
            *contagem.entry(raia as usize).or_default() += 1;
            por_raia.entry(raia as usize).or_default().push((format!("entra-{chave}-{}", raia as usize).into(), cartao));
        };

        // As ideias: as próximas primeiro, depois as outras, na ordem do quadro. A que virou chat
        // (Fazendo ou Feita, com o chat vivo) vai no cartão do chat.
        let mut ideias_do_chat: HashMap<String, usize> = HashMap::new();
        let ideias: Vec<&Ideia> = [Coluna::Proximas, Coluna::Ideias, Coluna::Fazendo, Coluna::Feitas]
            .into_iter()
            .flat_map(|c| m.quadro().da_coluna(c))
            .filter(|i| self.mostra_ideia(i, m, &termo))
            .collect();
        for ideia in ideias {
            let viva = chat_vivo(m, ideia);
            if let Some(c) = viva.filter(|_| matches!(ideia.coluna, Coluna::Fazendo | Coluna::Feitas)) {
                *ideias_do_chat.entry(c.id.clone()).or_default() += 1;
                continue;
            }
            por(Raia::da_coluna(ideia.coluna), format!("ideia-{}", ideia.id), self.cartao(ideia, m, window, cx));
        }

        // Os chats, cada um na raia do estado dele, o mexido por último primeiro.
        let pendencias = m.pendencias();
        let pendencia_de = |chat: &str| pendencias.iter().find(|p| p.chat() == chat);
        let mut chats: Vec<(&Chat, Raia)> = m
            .chats
            .iter()
            .filter(|c| self.mostra_chat(c, &termo))
            .map(|c| (c, raia_do_chat(&m.execucao(&c.id), m.espera_do_chat(c))))
            .collect();
        chats.sort_by_key(|(c, _)| std::cmp::Reverse(c.atualizado_em));
        let recente = agora().saturating_sub(DIAS_DAS_FEITAS * 24 * 3600);
        let feitas_total = chats.iter().filter(|(_, raia)| *raia == Raia::Feitas).count();
        let mut feitas_a_vista = 0;
        for (c, raia) in &chats {
            if *raia == Raia::Feitas {
                // Com busca ou filtro de pasta, todas; sem, as das duas últimas semanas, até 30.
                let todas = self.feitas_todas || !termo.is_empty() || self.pasta.is_some() || self.do_chat.is_some();
                if !todas && (c.atualizado_em < recente || feitas_a_vista >= FEITAS_A_VISTA) {
                    continue;
                }
                feitas_a_vista += 1;
            }
            let ideias = ideias_do_chat.get(&c.id).copied().unwrap_or(0);
            por(*raia, format!("chat-{}", c.id), self.cartao_do_chat(c, *raia, ideias, pendencia_de(&c.id), m, window, cx));
        }
        let escondidas = feitas_total.saturating_sub(feitas_a_vista);
        let mostrar_todas = (escondidas > 0 && !self.feitas_todas).then(|| {
            botao_fantasma("feitas-todas", None, tr!("Mostrar mais {n}", n = escondidas), TamanhoBotao::Linha, window, cx)
                .on_click(cx.listener(|q, _, _, cx| {
                    q.feitas_todas = true;
                    cx.notify();
                }))
                .into_any_element()
        });
        let mut mostrar_todas = mostrar_todas;
        let raias: Vec<AnyElement> = Raia::TODAS
            .into_iter()
            .map(|raia| {
                let cartoes = por_raia.remove(&(raia as usize)).unwrap_or_default();
                let n = contagem.get(&(raia as usize)).copied().unwrap_or(0) + if raia == Raia::Feitas { escondidas } else { 0 };
                let extra = if raia == Raia::Feitas { mostrar_todas.take() } else { None };
                self.raia(raia, cartoes, n, extra, m, window, cx)
            })
            .collect();
        let minimo = LARGURA_DA_RAIA * Raia::TODAS.len() as f32 + VAO_DAS_RAIAS * (Raia::TODAS.len() - 1) as f32;
        let quadro = v_flex()
            .id("quadro-de-tarefas")
            .size_full()
            .child(v_flex().flex_none().px(esp::E16).pt(esp::E12).pb(esp::E12).gap(esp::E8).child(self.cabecalho_do_quadro(m, window, cx)).child(self.filtros(m, window, cx)))
            .child(
                div()
                    .id("tabuleiro")
                    .flex_1()
                    .min_h_0()
                    .overflow_x_scroll()
                    .px(esp::E16)
                    .pb(esp::E16)
                    .child(h_flex().h_full().w_full().min_w(r(minimo)).items_start().gap(r(VAO_DAS_RAIAS)).children(raias)),
            )
            .into_any_element();
        // Trocar de vista sobe de leve (o `subir` do protótipo).
        self.com_esc(super::movimento::subir(div().size_full().child(quadro), "quadro-entra", super::movimento::De::Baixo).into_any_element(), cx)
    }
}

impl QuadroDeIdeias {
    /// O Esc volta: da edição e da conexão ao quadro, e do quadro às conversas. Com texto na busca,
    /// o primeiro Esc a limpa.
    fn com_esc(&self, filho: AnyElement, cx: &Context<Self>) -> AnyElement {
        div()
            .size_full()
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key != "escape" {
                    return;
                }
                cx.stop_propagation();
                if this.edicao.is_some() || this.conexao.is_some() {
                    this.voltar(window, cx);
                } else if !this.busca.read(cx).value().is_empty() {
                    this.busca.update(cx, |b, cx| b.set_value("", window, cx));
                } else {
                    this.fechar(window, cx);
                }
            }))
            .child(filho)
            .into_any_element()
    }
}

impl Janela {
    /// Ctrl+I, o item da lista e o menu do logo: abre as Tarefas no lugar das conversas, ou volta
    /// a elas se já estão abertas.
    pub(super) fn abrir_quadro_de_ideias(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quadro.is_some() {
            self.fechar_quadro(window, cx);
        } else {
            self.abrir_quadro(None, window, cx);
        }
    }

    /// "Ideias deste chat", no menu de uma conversa: só as dela, e a nova já nasce dela.
    pub(super) fn abrir_quadro_de_ideias_do_chat(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        self.abrir_quadro(Some(chat), window, cx);
    }

    fn abrir_quadro(&mut self, do_chat: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let pasta_padrao = {
            let m = motor.read(cx);
            match self.selecionado.as_deref().and_then(|id| m.chat(id)) {
                Some(c) if !m.sem_pasta(c) => Some(c.cwd.clone()),
                Some(_) => None,
                None => m.config.pasta_de_trabalho.clone(),
            }
        };
        let janela = cx.entity().downgrade();
        let vista = cx.new(|cx| QuadroDeIdeias::new(motor, janela, pasta_padrao, do_chat, window, cx));
        let foco = vista.read(cx).nova.read(cx).focus_handle(cx);
        self.quadro = Some(vista);
        window.focus(&foco, cx);
        // O item da lista acende (a lista é uma vista em cache).
        self.vista_da_lista.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// Volta às conversas (o ×, o Esc, Ctrl+I de novo, o item Conversas, ou um chat aberto).
    pub(super) fn fechar_quadro(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quadro.take().is_none() {
            return;
        }
        self.voltou_do_quadro_em = Some(std::time::Instant::now());
        self.vista_da_lista.update(cx, |_, cx| cx.notify());
        self.focar_composer(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod testes {
    use super::{Raia, raia_do_chat};
    use crate::modelo::Execucao;
    use crate::motor::Espera;

    /// A raia sai do estado do chat, como no Better Paseo: o pedido, o erro e a parada esperam
    /// você; o turno andando trabalha; a resposta não vista fica para ver; o resto está feito.
    #[test]
    fn a_raia_do_chat_sai_do_estado() {
        assert_eq!(raia_do_chat(&Execucao::EsperandoVoce, Some(Espera::Pedido)), Raia::Esperando);
        assert_eq!(raia_do_chat(&Execucao::Rodando { desde: 0 }, None), Raia::Trabalhando);
        assert_eq!(raia_do_chat(&Execucao::Erro("x".into()), Some(Espera::Erro)), Raia::Esperando);
        assert_eq!(raia_do_chat(&Execucao::Parado, Some(Espera::Parada)), Raia::Esperando);
        assert_eq!(raia_do_chat(&Execucao::Parado, Some(Espera::Nova)), Raia::ParaVer);
        assert_eq!(raia_do_chat(&Execucao::Parado, None), Raia::Feitas);
    }
}
