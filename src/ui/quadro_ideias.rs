//! O diálogo "Quadro de ideias" (Ctrl+I, ou o menu do logo): o kanban de `crate::ideias`, com as
//! quatro colunas lado a lado. Escreve-se a ideia no alto de Ideias (Enter anota); o cartão se
//! arrasta entre as colunas e dentro delas; o clique abre a edição (título, pedido, pasta), com
//! "Abrir chat", que cria a conversa já com o pedido e leva o cartão para Fazendo. O cartão de
//! Fazendo mostra como está o chat dele. O botão direito tem o mesmo, e mover e apagar.
//! Com o Linear ligado, o quadro é o do time (ver `crate::linear`): a faixa do alto diz de que time
//! e quando foi lido, com o ⟳; cada cartão leva o código da issue (o que é só do Vitral fica sem
//! ele, e não sobe: dono, 18/11/2026, "não precisa ter isso de mandar pro Linear"). O quadro relê
//! o Linear ao abrir e a cada minuto aberto.

use std::{path::PathBuf, time::Duration};

use gpui_kit::component::{
    WindowExt as _, h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        CabecalhoDialogo, Etiqueta, PreviaArrasto, Simbolo, TamanhoBotao, Tom, aviso, aviso_com_acao, botao_fantasma,
        botao_icone, botao_perigo, botao_primario, icone, icone_de_estado, sinal_do_chat,
    },
    configuracoes::{Opcao, menu_escolha},
    dialogos::{com_til, moldura, nome_da_pasta, topo},
    tempo_relativo,
};
use crate::{
    ideias::{Coluna, Ideia},
    modelo::Execucao,
    motor::{Motor, SituacaoDoLinear},
    tema::{Cores, Lucide, Tipo as _, esp, r, raio, tam},
};

/// A largura do diálogo a 100% (o kit o encolhe até caber na janela).
const LARGURA: f32 = 1180.;
/// De quanto em quanto tempo o quadro aberto relê o Linear.
const RELER_A_CADA: Duration = Duration::from_secs(60);

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
    /// Onde nasce o chat das ideias anotadas aqui: a pasta do chat aberto ao abrir o quadro.
    pasta_padrao: Option<PathBuf>,
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

/// Como está o chat de um cartão, em poucas palavras.
fn estado_do_chat(m: &Motor, chat: &str) -> Option<(Option<super::componentes::Sinal>, &'static str)> {
    let c = m.chat(chat)?;
    let palavra = match m.execucao(chat) {
        Execucao::Rodando { .. } => "trabalhando",
        Execucao::EsperandoVoce => "esperando você",
        Execucao::Erro(_) => "com erro",
        Execucao::Parado if m.nao_lido(c) => "resposta nova",
        Execucao::Parado if c.arquivado => "arquivado",
        Execucao::Parado => "parado",
    };
    Some((sinal_do_chat(m, c), palavra))
}

impl QuadroDeIdeias {
    fn new(motor: Entity<Motor>, janela: WeakEntity<Janela>, pasta_padrao: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let nova = cx.new(|cx| InputState::new(window, cx).placeholder("Nova ideia… (Enter anota)"));
        let assinatura = cx.subscribe_in(&nova, window, |this, campo, evento: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = evento {
                let titulo = campo.read(cx).value().to_string();
                let pasta = this.pasta_padrao.clone();
                if this.motor.update(cx, |m, cx| m.nova_ideia(&titulo, pasta, cx)).is_some() {
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

    /// A faixa do alto do quadro: o Linear ligado (o time, quando leu, ⟳) ou o convite para ligar.
    fn faixa_do_linear(&self, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let (texto, cor, botoes): (String, Hsla, Vec<AnyElement>) = match m.situacao_do_linear() {
            SituacaoDoLinear::Desligado { erro, .. } => (
                erro.unwrap_or_else(|| "Ligue o Linear para este quadro ser o do time EFI (Eficiência Operacional).".into()),
                Cores::tenue(),
                vec![
                    botao_fantasma("ligar-linear", Some(Lucide::Link.into()), "Ligar o Linear…", TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|q, _, window, cx| q.abrir_conexao(window, cx)))
                        .into_any_element(),
                ],
            ),
            SituacaoDoLinear::Ligado { time, lendo, lido_em, erro } => {
                let time = time.unwrap_or_else(|| "o time".into());
                let texto = match (&erro, lendo, lido_em) {
                    (Some(erro), ..) => erro.clone(),
                    (None, true, _) => format!("Linear · {time} · lendo…"),
                    (None, false, Some(quando)) => match tempo_relativo(quando) {
                        agora if agora == "agora" => format!("Linear · {time} · lido agora"),
                        ha => format!("Linear · {time} · lido há {ha}"),
                    },
                    (None, false, None) => format!("Linear · {time}"),
                };
                let mut botoes = Vec::new();
                botoes.push(
                    botao_icone("reler-linear", Lucide::RotateCcw, "Ler o Linear agora", TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|q, _, _, cx| q.motor.update(cx, |m, cx| m.ler_linear(cx))))
                        .into_any_element(),
                );
                botoes.push(
                    botao_icone("desligar-linear", Lucide::Unplug, "Desligar o Linear (os cartões das issues saem do quadro)", TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|q, _, _, cx| q.motor.update(cx, |m, cx| m.desligar_linear(cx))))
                        .into_any_element(),
                );
                (texto, if erro.is_some() { Cores::vermelho() } else { Cores::tenue() }, botoes)
            }
        };
        h_flex()
            .flex_none()
            .gap(esp::E8)
            .child(div().flex_1().min_w_0().truncate().metadado().text_color(cor).child(texto))
            .children(botoes)
            .into_any_element()
    }

    fn tela_da_conexao(&self, campo: &Entity<InputState>, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let (ligando, erro) = match m.situacao_do_linear() {
            SituacaoDoLinear::Desligado { ligando, erro } => (ligando, erro),
            SituacaoDoLinear::Ligado { .. } => (false, None),
        };
        let campo_ligar = campo.clone();
        let ligar = botao_primario("testar-linear", None, if ligando { "Testando…" } else { "Testar e ligar" }, TamanhoBotao::Controle, window, cx)
            .on_click(cx.listener(move |q, _, _, cx| {
                let chave = campo_ligar.read(cx).value().to_string();
                q.motor.update(cx, |m, cx| m.ligar_linear(chave, cx));
            }));
        let pagina = botao_fantasma("pagina-chave-linear", Some(Lucide::ExternalLink.into()), "Criar a chave no Linear", TamanhoBotao::Controle, window, cx)
            .on_click(|_, _, cx| cx.open_url(crate::linear::PAGINA_DA_CHAVE));
        v_flex()
            .w_full()
            .max_w(r(560.))
            .mx_auto()
            .gap(esp::E12)
            .child(div().corpo_ui().text_color(Cores::tenue()).child(
                "Cole a sua chave pessoal do Linear (Settings, Security & access, Personal API keys). O quadro passa a ser o do time \
                 EFI: cada cartão é uma issue, e o que você muda aqui muda lá.",
            ))
            .child(Input::new(campo))
            .children(erro.map(|e| div().metadado().text_color(Cores::vermelho()).child(e)))
            .child(div().metadado().text_color(Cores::apagado()).child(format!(
                "Fica em {}, só para você.",
                com_til(&crate::linear::arquivo_da_chave(&m.dados.config))
            )))
            .child(h_flex().pt(esp::E8).gap(esp::E8).child(pagina).child(div().flex_1()).child(ligar))
            .into_any_element()
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

    /// "Abrir chat": a conversa nasce com o pedido, o diálogo fecha e ela aparece no painel ativo.
    fn abrir_chat(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let pasta = self.pasta_padrao.clone();
        match self.motor.update(cx, |m, cx| m.abrir_chat_da_ideia(id, pasta, cx)) {
            Ok(chat) => self.ir_ao_chat(chat, window, cx),
            Err(erro) => window.push_notification(aviso(Tom::Erro, format!("Não abriu o chat: {erro}")), cx),
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

    /// O cabeçalho do diálogo: o quadro, ou a ideia aberta com a seta de voltar.
    fn cabecalho(vista: &Entity<Self>, cx: &App) -> CabecalhoDialogo {
        let q = vista.read(cx);
        if q.conexao.is_some() && !q.motor.read(cx).linear_ligado() {
            let vista = vista.downgrade();
            return CabecalhoDialogo::new("Ligar o Linear").voltar(move |window, cx| {
                let _ = vista.update(cx, |q, cx| q.voltar(window, cx));
            });
        }
        match &q.edicao {
            Some(_) => {
                let vista = vista.downgrade();
                CabecalhoDialogo::new("Ideia")
                    .subtitulo(div().font_weight(FontWeight::NORMAL).child("O pedido vai como a primeira mensagem do chat"))
                    .voltar(move |window, cx| {
                        let _ = vista.update(cx, |q, cx| q.voltar(window, cx));
                    })
            }
            None => {
                let n = q.motor.read(cx).quadro().ideias.len();
                let subtitulo = match n {
                    0 => "Anote ideias e abra um chat com elas quando quiser".to_string(),
                    _ => "Arraste entre as colunas · clique num cartão para editar".to_string(),
                };
                CabecalhoDialogo::new("Quadro de ideias").subtitulo(div().font_weight(FontWeight::NORMAL).child(subtitulo))
            }
        }
    }

    fn coluna(&self, coluna: Coluna, ideias: Vec<Ideia>, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let n = ideias.len();
        let cabeca = h_flex()
            .flex_none()
            .h(r(28.))
            .px(esp::E4)
            .gap(esp::E8)
            .child(div().flex_none().text_color(Cores::tenue()).child(icone(icone_da_coluna(coluna), 16., window)))
            .child(div().rotulo().text_color(Cores::forte()).child(coluna.titulo()))
            .child(div().metadado().text_color(Cores::apagado()).child(n.to_string()));
        let entrada = (coluna == Coluna::Ideias).then(|| div().flex_none().child(Input::new(&self.nova)));
        let cartoes: Vec<AnyElement> = ideias.iter().map(|i| self.cartao(i, m, window, cx)).collect();
        let vazia = (n == 0).then(|| {
            div().px(esp::E8).py(esp::E16).text_center().metadado().text_color(Cores::apagado()).child(coluna.vazia())
        });
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .gap(esp::E8)
            .child(cabeca)
            .children(entrada)
            .child(
                div()
                    .id(("coluna-de-ideias", coluna as usize))
                    .flex_1()
                    .min_h_0()
                    .rounded(raio::CARTAO)
                    .bg(Cores::recuo())
                    .border_1()
                    .border_color(Cores::borda())
                    .overflow_y_scroll()
                    .drag_over::<ArrastoIdeia>(|s, _, _, _| s.border_color(Cores::destaque_borda()).bg(Cores::tinta_leve(Cores::destaque())))
                    .on_drop(cx.listener(move |this, a: &ArrastoIdeia, _, cx| {
                        this.motor.update(cx, |m, cx| m.mover_ideia(&a.id, coluna, None, cx));
                    }))
                    .child(v_flex().gap(esp::E8).p(esp::E8).children(cartoes).children(vazia)),
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
        let mut etiquetas: Vec<AnyElement> = Vec::new();
        match &ideia.linear {
            Some(no) => {
                etiquetas.push(Etiqueta::new(no.identificador.clone()).baixa().into_any_element());
                if let Some(quem) = &no.responsavel {
                    etiquetas.push(Etiqueta::new(quem.clone()).icone(Lucide::CircleUser).baixa().into_any_element());
                }
            }
            None => {}
        }
        etiquetas.extend(ideia.etiquetas.iter().map(|e| Etiqueta::new(e.clone()).baixa().into_any_element()));
        let pasta = ideia.pasta.as_ref().map(|p| Etiqueta::new(nome_da_pasta(p)).icone(Lucide::Folder).baixa());
        let chat = ideia.chat.as_deref().and_then(|c| estado_do_chat(m, c)).map(|(sinal, palavra)| {
            h_flex()
                .gap(esp::E4)
                .child(icone_de_estado(sinal, 14.).vivo(format!("ideia-{}", ideia.id)))
                .child(div().metadado().text_color(Cores::tenue()).child(palavra))
        });
        let acao = match ideia.chat.clone().filter(|c| m.chat(c).is_some()) {
            Some(chat) => botao_icone(SharedString::from(format!("ir-{}", ideia.id)), Lucide::MessageSquare, "Ir ao chat", TamanhoBotao::Linha, window, cx)
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.ir_ao_chat(chat.clone(), window, cx);
                })),
            None => {
                let id = ideia.id.clone();
                botao_icone(SharedString::from(format!("abrir-{}", ideia.id)), Lucide::Send, "Abrir chat com esta ideia", TamanhoBotao::Linha, window, cx)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.abrir_chat(&id, window, cx);
                    }))
            }
        };
        let (id_clique, id_soltura, coluna) = (ideia.id.clone(), ideia.id.clone(), ideia.coluna);
        let arrasto = ArrastoIdeia { id: ideia.id.clone(), titulo: titulo.clone() };
        let vista = cx.entity().downgrade();
        let (id_menu, tem_chat) = (ideia.id.clone(), ideia.chat.is_some());
        let linear_do_menu = ideia.linear.as_ref().map(|no| no.url.clone()).filter(|url| !url.is_empty());
        div()
            .id(grupo.clone())
            .group(grupo.clone())
            .relative()
            .p(esp::E12)
            .rounded(raio::CARTAO)
            .bg(Cores::cartao_opaco())
            .border_1()
            .border_color(Cores::borda())
            .hover(|s| s.border_color(Cores::borda_forte()))
            .cursor_pointer()
            .child(
                v_flex()
                    .gap(esp::E4)
                    .child(div().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).line_clamp(3).child(titulo.clone()))
                    .children(detalhe.map(|d| div().metadado().text_color(Cores::tenue()).line_clamp(3).child(d)))
                    .child(
                        h_flex()
                            .pt(esp::E4)
                            .gap(esp::E4)
                            .min_h(r(24.))
                            .child(h_flex().flex_1().min_w_0().flex_wrap().gap(esp::E4).children(etiquetas).children(pasta).children(chat))
                            .child(div().flex_none().invisible().group_hover(grupo.clone(), |s| s.visible()).child(acao)),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.editar(&id_clique, window, cx)))
            .on_drag(arrasto, |a, _, _, cx| cx.new(|_| PreviaArrasto { titulo: a.titulo.clone(), simbolo: Simbolo::icone(Lucide::Lightbulb) }))
            // Solto em cima de um cartão, o arrastado entra logo antes dele.
            .drag_over::<ArrastoIdeia>(|s, _, _, _| s.border_color(Cores::destaque()))
            .on_drop(cx.listener(move |this, a: &ArrastoIdeia, _, cx| {
                cx.stop_propagation();
                this.motor.update(cx, |m, cx| m.mover_ideia(&a.id, coluna, Some(&id_soltura), cx));
            }))
            .context_menu(move |menu, _, _| menu_do_cartao(menu, vista.clone(), &id_menu, coluna, tem_chat, &linear_do_menu))
            .into_any_element()
    }

    fn tela_da_edicao(&self, edicao: &Edicao, m: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let Some(ideia) = m.quadro().achar(&edicao.id) else {
            return div().into_any_element();
        };
        let rotulo = |t: &'static str| div().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child(t);
        // A pasta: as dos chats, a do cartão (se não estiver entre elas) e "Sem pasta".
        // Sem pasta no cartão (uma issue de outra pessoa), vale a do chat à vista ao abrir o quadro.
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
        let pasta = menu_escolha(
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
        );
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
        let origem = ideia.origem.strip_prefix("chat:").and_then(|c| m.chat(c)).map(|c| {
            div().metadado().text_color(Cores::apagado()).child(format!("Anotada pelo chat \u{201c}{}\u{201d}", c.titulo))
        });
        let chat_vivo = ideia.chat.clone().filter(|c| m.chat(c).is_some());
        let (id_apagar, id_abrir) = (ideia.id.clone(), ideia.id.clone());
        let apagar = botao_perigo("apagar-ideia", Some(Lucide::Trash.into()), "Apagar", TamanhoBotao::Controle, window, cx)
            .on_click(cx.listener(move |this, _, window, cx| this.apagar(&id_apagar, window, cx)));
        let ir = chat_vivo.map(|chat| {
            botao_fantasma("ir-ao-chat-da-ideia", Some(Lucide::MessageSquare.into()), "Ir ao chat", TamanhoBotao::Controle, window, cx)
                .on_click(cx.listener(move |this, _, window, cx| this.ir_ao_chat(chat.clone(), window, cx)))
        });
        let no_linear = ideia.linear.as_ref().filter(|l| !l.url.is_empty()).map(|l| {
            let url = l.url.clone();
            botao_fantasma("abrir-no-linear", Some(Lucide::ExternalLink.into()), format!("{} no Linear", l.identificador), TamanhoBotao::Controle, window, cx)
                .on_click(move |_, _, cx| cx.open_url(&url))
        });
        let rotulo_abrir = if ideia.chat.is_some() { "Abrir outro chat" } else { "Abrir chat" };
        let abrir = botao_primario("abrir-chat-da-ideia", Some(Lucide::Send.into()), rotulo_abrir, TamanhoBotao::Controle, window, cx)
            .on_click(cx.listener(move |this, _, window, cx| this.abrir_chat(&id_abrir, window, cx)));
        v_flex()
            .w_full()
            .max_w(r(720.))
            .mx_auto()
            .gap(esp::E16)
            .child(v_flex().gap(esp::E4).child(rotulo("Título")).child(Input::new(&edicao.titulo)))
            .child(v_flex().gap(esp::E4).child(rotulo("Pedido para o chat")).child(Textarea::new(&edicao.pedido)).child(
                div().metadado().text_color(Cores::apagado()).child("Vazio, vai o título. O texto se grava sozinho."),
            ))
            .children(nota)
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(esp::E24)
                    .child(h_flex().gap(esp::E8).child(rotulo("Pasta")).child(pasta))
                    .child(h_flex().gap(esp::E8).child(rotulo("Coluna")).child(coluna)),
            )
            .children(origem)
            .child(
                h_flex()
                    .pt(esp::E8)
                    .gap(esp::E8)
                    .child(apagar)
                    .child(div().flex_1())
                    .children(no_linear)
                    .children(ir)
                    .child(abrir),
            )
            .into_any_element()
    }
}

/// O botão direito num cartão.
fn menu_do_cartao(menu: PopupMenu, vista: WeakEntity<QuadroDeIdeias>, id: &str, coluna: Coluna, tem_chat: bool, linear: &Option<String>) -> PopupMenu {
    let item = |rotulo: &str, icone: Lucide, f: Box<dyn Fn(&mut QuadroDeIdeias, &mut Window, &mut Context<QuadroDeIdeias>)>| {
        let vista = vista.clone();
        PopupMenuItem::new(rotulo.to_string()).icon(icone).on_click(move |_, window, cx| {
            let _ = vista.update(cx, |q, cx| f(q, window, cx));
        })
    };
    let id = id.to_string();
    let (a, e, x) = (id.clone(), id.clone(), id.clone());
    let mut menu = menu
        .item(item(if tem_chat { "Abrir outro chat com esta ideia" } else { "Abrir chat com esta ideia" }, Lucide::Send, Box::new(move |q, w, cx| q.abrir_chat(&a, w, cx))))
        .item(item("Editar", Lucide::Pencil, Box::new(move |q, w, cx| q.editar(&e, w, cx))));
    if let Some(url) = linear.clone() {
        menu = menu.item(PopupMenuItem::new("Abrir no Linear").icon(Lucide::ExternalLink).on_click(move |_, _, cx| cx.open_url(&url)));
    }
    menu = menu.separator();
    for destino in Coluna::TODAS.into_iter().filter(|c| *c != coluna) {
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
        if let Some(edicao) = &self.edicao {
            if m.quadro().achar(&edicao.id).is_some() {
                return self.tela_da_edicao(edicao, m, window, cx);
            }
        }
        // A altura acompanha a janela: o diálogo fica perto de tomar a tela, e cada coluna rola.
        let altura = (window.viewport_size().height - tam(window, 230.)).max(tam(window, 260.)).min(tam(window, 820.));
        let colunas: Vec<AnyElement> = Coluna::TODAS
            .into_iter()
            .map(|c| {
                let ideias: Vec<Ideia> = m.quadro().da_coluna(c).into_iter().cloned().collect();
                self.coluna(c, ideias, m, window, cx)
            })
            .collect();
        v_flex()
            .w_full()
            .gap(esp::E12)
            .child(self.faixa_do_linear(m, window, cx))
            .child(h_flex().w_full().h(altura).items_start().gap(esp::E12).children(colunas))
            .into_any_element()
    }
}

impl Janela {
    /// Ctrl+I e o menu do logo. As ideias anotadas aqui nascem na pasta do chat à vista.
    pub(super) fn abrir_quadro_de_ideias(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        let vista = cx.new(|cx| QuadroDeIdeias::new(motor, janela, pasta_padrao, window, cx));
        self.foco_antes_do_dialogo(window, cx);
        let foco = vista.read(cx).nova.read(cx).focus_handle(cx);
        window.open_dialog(cx, move |dialogo, window, cx| {
            moldura(dialogo, LARGURA, window)
                .margin_top(tam(window, 48.))
                // O Enter do campo de ideia nova (e o do título) chega ao diálogo como Confirmar:
                // sem isto, anotar fechava o quadro.
                .on_ok(|_, _, _| false)
                .title(topo(QuadroDeIdeias::cabecalho(&vista, cx)))
                .child(vista.clone())
        });
        window.focus(&foco, cx);
    }
}
