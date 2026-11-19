//! O diálogo "Quadro de ideias" (Ctrl+I, o menu do logo, ou "Ideias deste chat" no menu de uma
//! conversa): o kanban de `crate::ideias`, com as quatro colunas lado a lado. É a lista do que o
//! dono quer fazer no futuro: só ele anota (dono, 18/11/2026: "essas ideias é pra ser coisas do
//! futuro que vou colocando pra não esquecer").
//!
//! - Escreve-se a ideia no alto de Ideias (Enter anota). O cartão se arrasta entre as colunas e
//!   dentro delas; o clique abre a edição (título, pedido, chat, pasta, coluna).
//! - Uma ideia pode ser de um chat (dono, 18/11/2026: "associar elas a um chat ... e conseguir
//!   mandar nele sem criar um outro"): aberto pelo menu de uma conversa, o quadro mostra só as dela
//!   e a ideia nova já nasce dela. "Mandar no chat" põe o pedido na fila dessa conversa; sem chat,
//!   "Abrir chat" cria um com o pedido. Nos dois casos o cartão vai para Fazendo.
//! - Com o Linear ligado (`crate::linear`), as ideias são as issues da etiqueta "Ideia" do time; o
//!   cabeçalho diz quando leu, com o ⟳. O quadro relê o Linear ao abrir e a cada minuto aberto.
//!
//! O desenho é o do resto do app (dono, 18/11/2026: "mais compacto a parte de cima" e "nas cores
//! e estilos do resto"): o cabeçalho numa linha só, as colunas no fundo da lista (o mesmo buraco
//! azul dos balões) e os cartões como as linhas da lista, num véu claro por cima dele.

use std::{
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
        AnelGirando, CabecalhoDialogo, ControleSegmentado, Etiqueta, PreviaArrasto, Segmento, Simbolo, TamanhoBotao, Tom, aviso, aviso_com_acao, botao_fantasma,
        botao_icone, botao_perigo, botao_primario, icone, icone_de_estado, sinal_do_chat,
    },
    configuracoes::{Opcao, menu_escolha},
    dialogos::{com_til, moldura, nome_da_pasta},
    tempo_relativo,
};
use crate::{
    ideias::{Coluna, Ideia},
    modelo::Execucao,
    motor::{Motor, SituacaoDoLinear},
    tema::{Cores, Lucide, Tipo as _, esp, r, raio, tam},
};

/// A largura do diálogo a 100% (o kit o encolhe até caber na janela).
const LARGURA: f32 = 1240.;
/// De quanto em quanto tempo o quadro aberto relê o Linear.
const RELER_A_CADA: Duration = Duration::from_secs(60);
/// Quantas conversas a escolha do chat de uma ideia oferece (as mexidas por último).
const CHATS_NA_ESCOLHA: usize = 40;

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
    /// O campo do alto da coluna Ideias.
    nova: Entity<InputState>,
    /// Onde nasce o chat de uma ideia sem chat nem pasta: a pasta do chat à vista ao abrir.
    pasta_padrao: Option<PathBuf>,
    /// Aberto pelo menu de uma conversa: só as ideias dela, e a nova nasce dela.
    do_chat: Option<String>,
    /// Todas, só as do Linear ou só as do Vitral.
    origem: Origem,
    edicao: Option<Edicao>,
    /// A tela de ligar o Linear: o campo da chave.
    conexao: Option<Entity<InputState>>,
    _assinaturas: Vec<Subscription>,
    _releitura: Task<()>,
}

fn icone_da_coluna(coluna: Coluna) -> Lucide {
    match coluna {
        Coluna::Ideias => Lucide::Lightbulb,
        Coluna::Proximas => Lucide::ListTodo,
        Coluna::Fazendo => Lucide::CirclePlay,
        Coluna::Feitas => Lucide::CircleCheckBig,
    }
}

/// O chat de uma ideia que ainda dá para usar (existe e não foi arquivado).
fn chat_vivo<'a>(m: &'a Motor, ideia: &Ideia) -> Option<&'a crate::modelo::Chat> {
    ideia.chat.as_deref().and_then(|c| m.chat(c)).filter(|c| !c.arquivado)
}

/// Como está o chat de um cartão, em poucas palavras.
fn estado_do_chat(m: &Motor, c: &crate::modelo::Chat) -> &'static str {
    match m.execucao(&c.id) {
        Execucao::Rodando { .. } => "trabalhando",
        Execucao::EsperandoVoce => "esperando você",
        Execucao::Erro(_) => "com erro",
        Execucao::Parado if m.nao_lido(c) => "resposta nova",
        Execucao::Parado => "parado",
    }
}

/// Texto que não cabe é cortado na borda, sem reticências (o dono não as quer nos títulos).
fn cortado() -> Div {
    div().min_w_0().overflow_hidden().whitespace_nowrap()
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
        let dica = match (origem, do_chat.is_some()) {
            (Origem::Vitral, _) => "Nova ideia só no Vitral (Enter anota)",
            (_, true) => "Ideia para este chat (Enter anota)",
            _ => "Nova ideia (Enter anota)",
        };
        let nova = cx.new(|cx| InputState::new(window, cx).placeholder(dica));
        let assinatura = cx.subscribe_in(&nova, window, |this, campo, evento: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = evento {
                let titulo = campo.read(cx).value().to_string();
                let chat = this.do_chat.clone();
                // A ideia de um chat fica na pasta dele; a solta, na do chat à vista.
                let pasta = match chat.as_deref().and_then(|c| this.motor.read(cx).chat(c)) {
                    Some(c) if !this.motor.read(cx).sem_pasta(c) => Some(c.cwd.clone()),
                    Some(_) => None,
                    None => this.pasta_padrao.clone(),
                };
                let so_no_vitral = this.origem == Origem::Vitral;
                if this.motor.update(cx, |m, cx| m.nova_ideia(&titulo, pasta, chat, so_no_vitral, cx)).is_some() {
                    campo.update(cx, |c, cx| c.set_value("", window, cx));
                }
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
            pasta_padrao,
            do_chat,
            origem,
            edicao: None,
            conexao: None,
            _assinaturas: vec![assinatura],
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
        let titulo = cx.new(|cx| InputState::new(window, cx).placeholder("Título").default_value(ideia.titulo.clone()));
        let pedido = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(5, 14)
                .placeholder("O que o chat deve fazer: o contexto, onde mexer, como saber que ficou pronto.")
                .default_value(ideia.pedido.clone())
        });
        let (id_t, id_p) = (ideia.id.clone(), ideia.id.clone());
        self._assinaturas.truncate(1);
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
        self._assinaturas.truncate(1);
        let foco = self.nova.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    /// "Mandar no chat" (a ideia de um chat vai para ele, na fila) ou "Abrir chat" (um novo, com o
    /// pedido; `novo` força o novo mesmo com chat). O diálogo fecha e a conversa aparece.
    fn mandar(&mut self, id: &str, novo: bool, window: &mut Window, cx: &mut Context<Self>) {
        let pasta = self.pasta_padrao.clone();
        match self.motor.update(cx, |m, cx| m.mandar_ideia(id, pasta, novo, cx)) {
            Ok(chat) => self.ir_ao_chat(chat, window, cx),
            Err(erro) => window.push_notification(aviso(Tom::Erro, format!("Não mandou a ideia: {erro}")), cx),
        }
    }

    fn ir_ao_chat(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        window.close_dialog(cx);
        let _ = self.janela.update(cx, |j, cx| j.selecionar(Some(chat), window, cx));
    }

    fn apagar(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(apagada) = self.motor.update(cx, |m, cx| m.apagar_ideia(id, cx)) else {
            return;
        };
        if self.edicao.as_ref().is_some_and(|e| e.id == id) {
            self.edicao = None;
            self._assinaturas.truncate(1);
        }
        let motor = self.motor.clone();
        let texto = format!("Ideia apagada: {}", apagada.titulo);
        window.push_notification(
            aviso_com_acao(Tom::Info, texto, "Desfazer", move |_, cx| {
                let ideia = apagada.clone();
                motor.update(cx, |m, cx| m.devolver_ideia(ideia, cx));
            }),
            cx,
        );
        cx.notify();
    }

    /// As ideias que o quadro mostra: as da origem escolhida e, no quadro de um chat, só as dele.
    fn mostra(&self, ideia: &Ideia, ligado: bool) -> bool {
        (!ligado || self.origem.aceita(ideia)) && self.do_chat.as_ref().is_none_or(|c| ideia.chat.as_ref() == Some(c))
    }

    fn escolher_origem(&mut self, origem: Origem, window: &mut Window, cx: &mut Context<Self>) {
        self.origem = origem;
        ORIGEM.store(Origem::TODAS.iter().position(|o| *o == origem).unwrap_or(0) as u8, Ordering::Relaxed);
        let dica = match (origem, self.do_chat.is_some()) {
            (Origem::Vitral, _) => "Nova ideia só no Vitral (Enter anota)",
            (_, true) => "Ideia para este chat (Enter anota)",
            _ => "Nova ideia (Enter anota)",
        };
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
            Segmento::new(format!("Todas {}", do_quadro(Origem::Todas))),
            Segmento::new(format!("Linear {}", do_quadro(Origem::Linear))).dica("As issues com a etiqueta Ideia no Linear"),
            Segmento::new(format!("Vitral {}", do_quadro(Origem::Vitral))).dica("As que ficam só no Vitral; a nova anotada aqui também fica"),
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

    /// O cabeçalho do quadro numa linha: o nome, o chat do filtro (com o × que mostra todas), como
    /// está o Linear e os botões dele, e o fechar.
    fn cabecalho_do_quadro(&self, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let filtro = self.do_chat.as_deref().and_then(|c| m.chat(c)).map(|c| {
            h_flex()
                .id("filtro-do-chat")
                .min_w_0()
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
                    botao_icone("tirar-filtro", Lucide::X, "Ver as ideias de todos os chats", TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|q, _, _, cx| {
                            q.do_chat = None;
                            cx.notify();
                        })),
                )
        });
        let (texto, cor, botoes): (String, Hsla, Vec<AnyElement>) = match m.situacao_do_linear() {
            SituacaoDoLinear::Desligado { erro, .. } => (
                erro.unwrap_or_default(),
                Cores::vermelho(),
                vec![
                    botao_fantasma("ligar-linear", Some(Lucide::Link.into()), "Ligar o Linear", TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|q, _, window, cx| q.abrir_conexao(window, cx)))
                        .into_any_element(),
                ],
            ),
            SituacaoDoLinear::Ligado { time, lendo, lido_em, erro } => {
                let time = time.unwrap_or_else(|| "o time".into());
                let texto = match (&erro, lendo, lido_em) {
                    (Some(erro), ..) => erro.clone(),
                    (None, true, _) => format!("Lendo o Linear · {time}"),
                    (None, false, Some(quando)) => match tempo_relativo(quando) {
                        agora if agora == "agora" => format!("Linear · {time} · lido agora"),
                        ha => format!("Linear · {time} · lido há {ha}"),
                    },
                    (None, false, None) => format!("Linear · {time}"),
                };
                let botoes = vec![
                    botao_icone("reler-linear", Lucide::RotateCcw, "Ler o Linear agora", TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|q, _, _, cx| q.motor.update(cx, |m, cx| m.ler_linear(cx))))
                        .into_any_element(),
                    botao_icone("desligar-linear", Lucide::Unplug, "Desligar o Linear (as ideias de lá saem do quadro)", TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|q, _, _, cx| q.motor.update(cx, |m, cx| m.desligar_linear(cx))))
                        .into_any_element(),
                ];
                (texto, if erro.is_some() { Cores::vermelho() } else { Cores::apagado() }, botoes)
            }
        };
        h_flex()
            .flex_none()
            .h(r(28.))
            .gap(esp::E8)
            .child(div().flex_none().text_color(Cores::tenue()).child(icone(Lucide::Lightbulb, 16., window)))
            .child(div().flex_none().text_size(r(15.)).line_height(r(20.)).font_weight(FontWeight::SEMIBOLD).text_color(Cores::forte()).child("Ideias"))
            .children(filtro)
            .children(self.filtro_de_origem(m, cx))
            // Carregando: o anel do "trabalhando" ao lado do texto, até o Linear responder.
            .children(m.lendo_o_linear().then(|| div().flex_none().child(AnelGirando::new(14.).cor(Cores::destaque()).vivo("ideias-lendo"))))
            .child(cortado().flex_1().metadado().text_color(cor).child(texto))
            .children(botoes)
            .child(
                botao_icone("fechar-ideias", Lucide::X, "Fechar (Esc)", TamanhoBotao::Linha, window, cx)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .into_any_element()
    }

    fn coluna(&self, coluna: Coluna, ideias: Vec<Ideia>, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let n = ideias.len();
        let cabeca = h_flex()
            .flex_none()
            .h(r(20.))
            .px(esp::E4)
            .gap(esp::E6)
            .child(div().flex_none().text_color(Cores::apagado()).child(icone(icone_da_coluna(coluna), 14., window)))
            .child(div().rotulo().text_color(Cores::texto()).child(coluna.titulo()))
            .child(div().metadado().text_color(Cores::apagado()).child(n.to_string()));
        let entrada = (coluna == Coluna::Ideias).then(|| div().flex_none().child(Input::new(&self.nova).small()));
        let cartoes: Vec<AnyElement> = ideias.iter().map(|i| self.cartao(i, m, window, cx)).collect();
        // Coluna vazia enquanto o Linear não respondeu: o anel girando, e não o "escreva aqui".
        let lendo = m.lendo_o_linear();
        let vazia = (n == 0).then(|| {
            if lendo {
                v_flex()
                    .items_center()
                    .gap(esp::E8)
                    .py(esp::E16)
                    .child(AnelGirando::new(18.).cor(Cores::destaque()).vivo(format!("ideias-lendo-{}", coluna as usize)))
                    .child(div().metadado().text_color(Cores::apagado()).child("Lendo o Linear"))
            } else {
                v_flex().px(esp::E8).py(esp::E12).text_center().metadado().text_color(Cores::apagado()).child(coluna.vazia())
            }
        });
        let raio_da_coluna = raio::cartao();
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .gap(esp::E6)
            .child(cabeca)
            .children(entrada)
            .child(
                // O fundo da lista (o metal, o véu e o buraco), como os balões da conversa.
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .rounded(raio_da_coluna)
                    .child(super::mensagens::fundo_da_lista(raio_da_coluna))
                    .child(
                        div()
                            .id(("coluna-de-ideias", coluna as usize))
                            .relative()
                            .size_full()
                            .rounded(raio_da_coluna)
                            .overflow_y_scroll()
                            .drag_over::<ArrastoIdeia>(|s, _, _, _| s.bg(Cores::tinta_leve(Cores::destaque())))
                            .on_drop(cx.listener(move |this, a: &ArrastoIdeia, _, cx| {
                                this.motor.update(cx, |m, cx| m.mover_ideia(&a.id, coluna, None, cx));
                            }))
                            .child(v_flex().gap(esp::E6).p(esp::E6).children(cartoes).children(vazia)),
                    ),
            )
            .into_any_element()
    }

    fn cartao(&self, ideia: &Ideia, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let grupo: SharedString = format!("ideia-{}", ideia.id).into();
        let titulo: SharedString = if ideia.titulo.trim().is_empty() { "Sem título".into() } else { ideia.titulo.clone().into() };
        // Embaixo do título: o porquê, ou o começo do pedido quando ele diz mais que o título.
        let detalhe = if !ideia.nota.trim().is_empty() {
            Some(ideia.nota.trim().to_string())
        } else {
            let pedido = ideia.pedido.trim();
            (!pedido.is_empty() && pedido != ideia.titulo.trim()).then(|| pedido.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" "))
        };
        let chat = chat_vivo(m, ideia);
        // A linha de baixo, numa linha só: o código da issue, quem pegou e, no resto do espaço, o
        // chat da ideia com o estado dele (ou a pasta). O que não cabe é cortado na borda.
        let mut fatos: Vec<AnyElement> = Vec::new();
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
                format!("Mandar no chat \u{201c}{}\u{201d}", c.titulo),
                TamanhoBotao::Linha,
                window,
                cx,
            ),
            None => botao_icone(SharedString::from(format!("mandar-{}", ideia.id)), Lucide::Send, "Abrir chat com esta ideia", TamanhoBotao::Linha, window, cx),
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
        div()
            .id(grupo.clone())
            .group(grupo.clone())
            .relative()
            .px(esp::E12)
            .py(esp::E8)
            .rounded(raio::controle())
            // Como uma linha da lista: um véu claro sobre o fundo dela, que acende com o ponteiro.
            .bg(Cores::hover())
            .border_1()
            .border_color(Cores::borda())
            .hover(|s| s.bg(Cores::selecao()).border_color(Cores::borda_forte()))
            .cursor_pointer()
            .child(
                v_flex()
                    .gap(esp::E2)
                    .child(div().rotulo().text_color(Cores::forte()).line_clamp(3).child(titulo.clone()))
                    .children(detalhe.map(|d| div().metadado().text_color(Cores::tenue()).line_clamp(2).child(d)))
                    .when(!fatos.is_empty(), |s| {
                        s.child(
                            h_flex()
                                .pt(esp::E4)
                                .gap(esp::E4)
                                .h(r(22.))
                                .child(h_flex().flex_1().min_w_0().overflow_hidden().gap(esp::E4).children(fatos))
                                .child(div().flex_none().invisible().group_hover(grupo.clone(), |s| s.visible()).child(acao)),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.editar(&id_clique, window, cx)))
            .on_drag(arrasto, |a, _, _, cx| cx.new(|_| PreviaArrasto { titulo: a.titulo.clone(), simbolo: Some(Simbolo::icone(Lucide::Lightbulb)) }))
            // Solto em cima de um cartão, o arrastado entra logo antes dele.
            .drag_over::<ArrastoIdeia>(|s, _, _, _| s.border_color(Cores::destaque()))
            .on_drop(cx.listener(move |this, a: &ArrastoIdeia, _, cx| {
                cx.stop_propagation();
                this.motor.update(cx, |m, cx| m.mover_ideia(&a.id, coluna, Some(&id_soltura), cx));
            }))
            .context_menu(move |menu_vazio, _, _| menu_do_cartao(menu_vazio, vista.clone(), &menu))
            .into_any_element()
    }

    fn tela_da_conexao(&self, campo: &Entity<InputState>, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let (ligando, erro) = match m.situacao_do_linear() {
            SituacaoDoLinear::Desligado { ligando, erro } => (ligando, erro),
            SituacaoDoLinear::Ligado { .. } => (false, None),
        };
        let campo_ligar = campo.clone();
        let ligar = botao_primario("testar-linear", None, if ligando { "Testando" } else { "Testar e ligar" }, TamanhoBotao::Controle, window, cx)
            .on_click(cx.listener(move |q, _, _, cx| {
                let chave = campo_ligar.read(cx).value().to_string();
                q.motor.update(cx, |m, cx| m.ligar_linear(chave, cx));
            }));
        let pagina = botao_fantasma("pagina-chave-linear", Some(Lucide::ExternalLink.into()), "Criar a chave no Linear", TamanhoBotao::Controle, window, cx)
            .on_click(|_, _, cx| cx.open_url(crate::linear::PAGINA_DA_CHAVE));
        let vista = cx.entity().downgrade();
        v_flex()
            .w_full()
            .gap(esp::E16)
            .child(CabecalhoDialogo::new("Ligar o Linear").voltar(move |window, cx| {
                let _ = vista.update(cx, |q, cx| q.voltar(window, cx));
            }))
            .child(
                v_flex()
                    .w_full()
                    .max_w(r(560.))
                    .mx_auto()
                    .gap(esp::E12)
                    .child(div().corpo_ui().text_color(Cores::tenue()).child(
                        "Cole a sua chave pessoal do Linear (Settings, Security & access, Personal API keys). As ideias passam a ser \
                         as issues com a etiqueta Ideia do time EFI, e o que você muda aqui muda lá.",
                    ))
                    .child(Input::new(campo))
                    .children(erro.map(|e| div().metadado().text_color(Cores::vermelho()).child(e)))
                    .child(div().metadado().text_color(Cores::apagado()).child(format!(
                        "Fica em {}, só para você.",
                        com_til(&crate::linear::arquivo_da_chave(&m.dados.config))
                    )))
                    .child(h_flex().pt(esp::E8).gap(esp::E8).child(pagina).child(div().flex_1()).child(ligar)),
            )
            .into_any_element()
    }

    fn tela_da_edicao(&self, edicao: &Edicao, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let Some(ideia) = m.quadro().achar(&edicao.id) else {
            return div().into_any_element();
        };
        let rotulo = |t: &'static str| div().flex_none().rotulo().text_color(Cores::texto()).child(t);
        let chat = chat_vivo(m, ideia);

        // O chat da ideia: um novo (na pasta escolhida) ou uma conversa que já existe.
        let mut conversas: Vec<&crate::modelo::Chat> = m.chats.iter().filter(|c| !c.arquivado).collect();
        conversas.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
        let mut opcoes = vec![Opcao::new("", "Um chat novo").descricao("Criado com o pedido, quando você mandar")];
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
            opcoes.push(Opcao::new("", "Sem pasta").descricao("Uma pasta vazia só do chat"));
            let atual = pasta_do_cartao.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            let (motor, id) = (self.motor.clone(), ideia.id.clone());
            h_flex().gap(esp::E8).child(rotulo("Pasta")).child(menu_escolha(
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
        let colunas: Vec<Opcao> = Coluna::TODAS.iter().map(|c| Opcao::new(c.titulo(), c.titulo())).collect();
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
            v_flex().gap(esp::E4).child(rotulo("Por quê")).child(div().corpo_ui().text_color(Cores::tenue()).child(ideia.nota.clone()))
        });
        let (id_apagar, id_mandar) = (ideia.id.clone(), ideia.id.clone());
        let apagar = botao_perigo("apagar-ideia", Some(Lucide::Trash.into()), "Apagar", TamanhoBotao::Controle, window, cx)
            .on_click(cx.listener(move |this, _, window, cx| this.apagar(&id_apagar, window, cx)));
        let ir = chat.map(|c| {
            let chat = c.id.clone();
            botao_fantasma("ir-ao-chat-da-ideia", Some(Lucide::MessageSquare.into()), "Ir ao chat", TamanhoBotao::Controle, window, cx)
                .on_click(cx.listener(move |this, _, window, cx| this.ir_ao_chat(chat.clone(), window, cx)))
        });
        let no_linear = ideia.linear.as_ref().filter(|l| !l.url.is_empty()).map(|l| {
            let url = l.url.clone();
            botao_fantasma("abrir-no-linear", Some(Lucide::ExternalLink.into()), format!("{} no Linear", l.identificador), TamanhoBotao::Controle, window, cx)
                .on_click(move |_, _, cx| cx.open_url(&url))
        });
        let rotulo_mandar = if chat.is_some() { "Mandar no chat" } else { "Abrir chat" };
        let mandar = botao_primario("mandar-ideia", Some(Lucide::Send.into()), rotulo_mandar, TamanhoBotao::Controle, window, cx)
            .on_click(cx.listener(move |this, _, window, cx| this.mandar(&id_mandar, false, window, cx)));
        let vista = cx.entity().downgrade();
        v_flex()
            .w_full()
            .gap(esp::E16)
            .child(
                CabecalhoDialogo::new("Ideia")
                    .subtitulo(div().font_weight(FontWeight::NORMAL).child(if chat.is_some() {
                        "O pedido entra na fila do chat dela"
                    } else {
                        "O pedido vai como a primeira mensagem de um chat novo"
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
                    .child(v_flex().gap(esp::E4).child(rotulo("Título")).child(Input::new(&edicao.titulo)))
                    .child(v_flex().gap(esp::E4).child(rotulo("Pedido para o chat")).child(Textarea::new(&edicao.pedido)).child(
                        div().metadado().text_color(Cores::apagado()).child("Vazio, vai o título. O texto se grava sozinho."),
                    ))
                    .children(nota)
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_x(esp::E24)
                            .gap_y(esp::E8)
                            .child(h_flex().gap(esp::E8).child(rotulo("Chat")).child(escolha_do_chat))
                            .children(pasta)
                            .child(h_flex().gap(esp::E8).child(rotulo("Coluna")).child(coluna)),
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
            .item(item(&format!("Mandar no chat \u{201c}{titulo}\u{201d}"), Lucide::Send, Box::new(move |q, w, cx| q.mandar(&a, false, w, cx))))
            .item(item("Abrir num chat novo", Lucide::MessageSquarePlus, Box::new(move |q, w, cx| q.mandar(&b, true, w, cx)))),
        None => menu.item(item("Abrir chat com esta ideia", Lucide::Send, Box::new(move |q, w, cx| q.mandar(&a, true, w, cx)))),
    };
    menu = menu.item(item("Editar", Lucide::Pencil, Box::new(move |q, w, cx| q.editar(&e, w, cx))));
    if let Some(url) = d.linear.clone() {
        menu = menu.item(PopupMenuItem::new("Abrir no Linear").icon(Lucide::ExternalLink).on_click(move |_, _, cx| cx.open_url(&url)));
    }
    menu = menu.separator();
    for destino in Coluna::TODAS.into_iter().filter(|c| *c != d.coluna) {
        let id = id.clone();
        menu = menu.item(item(&format!("Mover para {}", destino.titulo()), icone_da_coluna(destino), Box::new(move |q, _, cx| {
            q.motor.update(cx, |m, cx| m.mover_ideia(&id, destino, None, cx));
        })));
    }
    menu.separator().item(item("Apagar", Lucide::Trash, Box::new(move |q, w, cx| q.apagar(&x, w, cx))))
}

impl Render for QuadroDeIdeias {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let motor = self.motor.clone();
        let m = motor.read(cx);
        if let Some(campo) = &self.conexao
            && !m.linear_ligado()
        {
            return self.tela_da_conexao(campo, m, window, cx);
        }
        if let Some(edicao) = &self.edicao
            && m.quadro().achar(&edicao.id).is_some()
        {
            return self.tela_da_edicao(edicao, m, window, cx);
        }
        // A altura acompanha a janela: o quadro fica perto de tomar a tela, e cada coluna rola.
        let altura = (window.viewport_size().height - tam(window, 120.)).max(tam(window, 260.));
        let colunas: Vec<AnyElement> = Coluna::TODAS
            .into_iter()
            .map(|c| {
                let ideias: Vec<Ideia> = m.quadro().da_coluna(c).into_iter().filter(|i| self.mostra(i, m.linear_ligado())).cloned().collect();
                self.coluna(c, ideias, m, window, cx)
            })
            .collect();
        v_flex()
            .id("quadro-de-ideias")
            .w_full()
            .h(altura)
            .gap(esp::E12)
            .child(self.cabecalho_do_quadro(m, window, cx))
            .child(h_flex().flex_1().min_h_0().w_full().items_start().gap(esp::E8).children(colunas))
            .into_any_element()
    }
}

impl Janela {
    /// Ctrl+I e o menu do logo: todas as ideias.
    pub(super) fn abrir_quadro_de_ideias(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.abrir_quadro(None, window, cx);
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
        self.foco_antes_do_dialogo(window, cx);
        let foco = vista.read(cx).nova.read(cx).focus_handle(cx);
        window.open_dialog(cx, move |dialogo, window, _| {
            moldura(dialogo, LARGURA, window)
                // Mais justo que os outros diálogos: o espaço é dos cartões.
                .p(r(16.))
                .margin_top(tam(window, 24.))
                // O Enter do campo de ideia nova (e o do título) chega ao diálogo como Confirmar:
                // sem isto, anotar fechava o quadro.
                .on_ok(|_, _, _| false)
                .child(vista.clone())
        });
        window.focus(&foco, cx);
    }
}
