//! O alto da lista: o logo (o menu do Vitral, F10) e o sino do que precisa de você. A área vazia
//! arrasta a janela. Com Ctrl+B a lista some; o logo, com o selo do sino, vai para o 1º painel
//! (modo recolhido).

use std::{
    path::Path,
    rc::Rc,
    sync::{Arc, LazyLock},
};

use gpui_kit::base::actions::SelectDown;
use gpui_kit::component::{
    Selectable, h_flex,
    menu::{PopupMenu, PopupMenuItem},
    popover::Popover,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    AbaLateral, AbrirAtalhos, AbrirConfiguracoes, AbrirMenuVitral, AlternarLista, Filtro, Gaveta, Janela,
    NovaConversa, ProximaPendencia,
    componentes::{
        BotaoRedondo, Selo, Simbolo,
        FASE_INICIAL_DA_MALHA, VarianteRedonda, anel_do_pulso, cabecalho_objeto_com_dica,
        contagem, malha_viva, rodape_menu_com_dica,
    },
    movimento::{self, Curva},
    uso_plano,
};
use crate::{
    atualizacao,
    motor::{Motor, PrecisaDeVoce},
    tema::{Cores, Lucide, esp, halo_foco, r, raio, tam},
};

/// O logo pequeno (16 a 32 px na tela): o quadrado de metal líquido com o V, parado. É o PNG de
/// 64 (o metal tem grão e não se descreve em vetor; os arquivos saem de `ferramentas/logo`).
/// Guardado uma vez só (o trilho redesenha a cada quadro).
static LOGO: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let png = include_bytes!("../../assets/vitral-64.png");
    Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec()))
});

/// Só o V do logo (com uma sombra escura embaixo), para ir por cima da malha viva do botão do
/// menu: lá o quadrado azul do logo é a própria malha. O V é o desenho que o dono mandou (uma gota
/// à esquerda ligada por um pescoço fino à perna grossa da direita), tirado da imagem dele por
/// `ferramentas/logo/rasterizar.py` (por isso PNG, de 256, e não SVG).
static LOGO_V: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let png = include_bytes!("../../assets/vitral-v.png");
    Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec()))
});

/// O V do logo, para quem o desenha fora daqui (o medalhão das boas-vindas).
pub(super) fn logo_v() -> Arc<Image> {
    LOGO_V.clone()
}

/// Medidas da peça a 100% (crescem com o zoom): o lugar do logo.
pub(super) const LARGURA: f32 = 52.;

/// Dica do sino: o título e, embaixo, o que trava o trabalho (pedidos, paradas, erros) separado
/// das respostas novas.
fn dica_sino(p: &PrecisaDeVoce) -> String {
    let partes: Vec<String> = [
        (p.pedidos, "pedido", "pedidos"),
        (p.paradas, "parada", "paradas"),
        (p.erros, "erro", "erros"),
        (p.novas, "resposta nova", "respostas novas"),
    ]
    .into_iter()
    .filter(|(n, ..)| *n > 0)
    .map(|(n, um, varios)| contagem(n, um, varios))
    .collect();
    if partes.is_empty() {
        "O que precisa de você".into()
    } else {
        format!("O que precisa de você\n{}", partes.join(", "))
    }
}

/// O que vai no canto do sino: o número (vermelho se algo falhou, senão âmbar), um ponto azul
/// quando só há respostas novas, ou nada.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum MarcaDoSino {
    Numero(usize, Hsla),
    Ponto,
    Nada,
}

fn marca_do_sino(p: &PrecisaDeVoce) -> MarcaDoSino {
    match p.numero() {
        0 if p.novas > 0 => MarcaDoSino::Ponto,
        0 => MarcaDoSino::Nada,
        n => MarcaDoSino::Numero(n, if p.erro { Cores::vermelho() } else { Cores::espera() }),
    }
}

/// A cor do pulso do sino: a mesma do número (vermelho se algo falhou, senão âmbar).
fn cor_do_pulso(p: &PrecisaDeVoce) -> Hsla {
    if p.erro { Cores::vermelho() } else { Cores::espera() }
}

/// O selo do sino: sólido, porque fica por cima do botão.
pub(super) fn selo_do_sino(p: &PrecisaDeVoce) -> Option<Selo> {
    match marca_do_sino(p) {
        MarcaDoSino::Numero(n, cor) => Some(Selo::contagem(n).solida(cor)),
        MarcaDoSino::Ponto => Some(Selo::ponto(Cores::destaque())),
        MarcaDoSino::Nada => None,
    }
}

/// O selo do item "O que precisa de você" do menu: tingido, porque fica na linha.
fn selo_do_menu(p: &PrecisaDeVoce) -> Option<Selo> {
    match marca_do_sino(p) {
        MarcaDoSino::Numero(n, cor) => Some(Selo::contagem(n).tingida(cor)),
        MarcaDoSino::Ponto => Some(Selo::ponto(Cores::destaque())),
        MarcaDoSino::Nada => None,
    }
}

/// "~/Projetos": a pasta de trabalho com a casa abreviada.
pub(super) fn pasta_curta(raiz: &Path) -> String {
    match dirs::home_dir().and_then(|casa| raiz.strip_prefix(casa).ok().map(Path::to_path_buf)) {
        Some(resto) if resto.as_os_str().is_empty() => "~".into(),
        Some(resto) => format!("~/{}", resto.display()),
        None => raiz.display().to_string(),
    }
}

/// Contêiner dos botões da peça: o clique (esquerdo ou direito) neles não move a janela nem abre
/// o menu dela.
fn sem_a_peca() -> Div {
    div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
}

/// Item de menu que chama um método da janela.
fn item(
    rotulo: impl Into<SharedString>,
    icone: Lucide,
    janela: &WeakEntity<Janela>,
    f: impl Fn(&mut Janela, &mut Window, &mut Context<Janela>) + 'static,
) -> PopupMenuItem {
    let janela = janela.clone();
    PopupMenuItem::new(rotulo).icon(icone).on_click(move |_, window, cx| {
        let _ = janela.update(cx, |this, cx| f(this, window, cx));
    })
}

/// O topo do menu do Vitral: o logo, "Vitral" e quantos chats há, com a pasta de dados na dica
/// (fixo, fora do teclado).
fn cabecalho_vitral(dados: &Path, chats: usize) -> PopupMenuItem {
    let quantos = contagem(chats, "chat", "chats");
    cabecalho_objeto_com_dica(Simbolo::Imagem(LOGO.clone()), "Vitral", Some(quantos.into()), pasta_curta(dados))
}

/// A linha técnica do pé do menu do Vitral: a versão e o commit curto (ou o canal, no compilado
/// no PC); a frase inteira fica na dica.
fn linha_tecnica() -> String {
    let commit = atualizacao::commit();
    let curto = commit.get(..7).unwrap_or(commit);
    let origem = if curto.is_empty() { atualizacao::canal() } else { curto };
    format!("{} · {origem}", atualizacao::rotulo())
}

type ConstruirMenu = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

/// O botão do menu: o medalhão é a malha viva (o metal líquido da receita do dono) com o V branco
/// por cima, e ocupa o botão inteiro (sem disco atrás); no hover e com o menu aberto ganha um aro,
/// a malha passa do ritmo de repouso para o vivo e a luz dela segue o ponteiro. No modo recolhido
/// ele leva o selo do sino, que some junto com o trilho. Com uma versão nova pronta, um ponto azul
/// de 8 embaixo.
#[derive(IntoElement)]
struct BotaoLogo {
    lado: f32,
    medalhao: f32,
    aberto: bool,
    selo: Option<Selo>,
    /// No modo recolhido, o pulso do sino (a geração e a cor), que vai no logo com o selo.
    pulso: Option<(u64, Hsla)>,
    /// A versão pronta esperando o reinício.
    pronta: Option<SharedString>,
}

impl Selectable for BotaoLogo {
    fn selected(mut self, selected: bool) -> Self {
        self.aberto = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.aberto
    }
}

impl RenderOnce for BotaoLogo {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = ElementId::from("logo");
        let pulso = self.pulso.and_then(|(geracao, cor)| {
            let t = movimento::disparo(movimento::chave(&id, "pulso"), geracao, movimento::ENCHER, Curva::Sair, window, cx);
            t.map(|t| anel_do_pulso(cor, t))
        });
        // A malha anda devagar em repouso e lisa com o ponteiro em cima ou o menu aberto.
        let pairando = movimento::Hover::de(movimento::chave(&id, "pairando"), window, cx);
        let vivo = pairando.ativo(cx) || self.aberto;
        let ritmo = if vivo { movimento::QPS_DO_LACO } else { movimento::QPS_EM_REPOUSO };
        let fase = movimento::laco_a(window, cx, ritmo) + FASE_INICIAL_DA_MALHA;
        div()
            .id(id)
            .relative()
            .flex_none()
            .size(r(self.lado))
            .on_hover(pairando.ouvinte())
            .flex()
            .items_center()
            .justify_center()
            // O logo é um quadrado de cantos bem arredondados (o padrão `raio::de`): o aro do
            // hover acompanha.
            .rounded(raio::de(self.medalhao) + px(2.))
            .cursor_pointer()
            .tab_index(0)
            .focus_visible(|s| s.shadow(halo_foco()))
            .border_2()
            .border_color(if self.aberto { Cores::borda_forte() } else { transparent_black() })
            .when(!self.aberto, |s| {
                let dica = match &self.pronta {
                    Some(versao) => format!("Menu do Vitral\nVersão {versao} pronta"),
                    None => "Menu do Vitral".to_string(),
                };
                s.hover(|s| s.border_color(Cores::borda())).tooltip(move |window, cx| {
                    Tooltip::new(dica.clone()).action(&AbrirMenuVitral, None).build(window, cx)
                })
            })
            .child(
                div()
                    .relative()
                    .flex_none()
                    .size(r(self.medalhao))
                    .child(div().absolute().top_0().left_0().size_full().child(malha_viva(
                        fase,
                        raio::de(self.medalhao),
                        vivo && movimento::anima(window, cx),
                    )))
                    .child(img(LOGO_V.clone()).absolute().top_0().left_0().size_full())
                    // A aresta de luz da tecla: o logo e o "+" são um par.
                    .child(div().absolute().inset_0().rounded(raio::de(self.medalhao)).shadow(crate::tema::aresta_de_tecla())),
            )
            .children(pulso)
            .when_some(self.selo, |s, selo| {
                s.child(div().absolute().top(r(-3.)).right(r(-3.)).child(selo))
            })
            .when(self.pronta.is_some(), |s| {
                // O aro opaco separa o ponto das cores do medalhão.
                s.child(
                    div()
                        .absolute()
                        .bottom(r(-2.))
                        .right(r(-2.))
                        .size(r(12.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(Cores::dialogo())
                        .child(div().size(r(8.)).rounded_full().bg(Cores::destaque())),
                )
            })
    }
}

/// Um elemento qualquer como gatilho de popover: o logo do modo recolhido abre dois, o menu e,
/// por fora dele, a caixa do sino.
#[derive(IntoElement)]
pub(super) struct Gatilho(pub AnyElement);

impl Selectable for Gatilho {
    fn selected(self, _: bool) -> Self {
        self
    }

    fn is_selected(&self) -> bool {
        false
    }
}

impl RenderOnce for Gatilho {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.0
    }
}

impl Janela {
    /// A geração do pulso do sino: sobe só quando o número do que precisa de você aumenta. Abrir
    /// o app (a primeira conta), o número cair e marcar como lido não pulsam.
    fn geracao_do_sino(&self, precisa: &PrecisaDeVoce) -> u64 {
        let numero = precisa.numero();
        if self.sino_numero.replace(Some(numero)).is_some_and(|antes| numero > antes) {
            self.sino_geracao.set(self.sino_geracao.get() + 1);
        }
        self.sino_geracao.get()
    }

    /// O alto da lista enxuta: o logo (o menu do Vitral) e, ao lado, as abas Conversas e Arquivos.
    pub(super) fn topo_enxuto(&self, motor: &Entity<Motor>, abas: Option<Div>, cx: &Context<Self>) -> impl IntoElement + use<> {
        let m = motor.read(cx);
        let precisa = super::resumo::precisa_em_tudo(m);
        self.arrastavel_com_id("arrastar-trilho-deitado")
            .on_mouse_down(MouseButton::Right, |evento, window, _| window.show_window_menu(evento.position))
            .relative()
            .flex()
            .items_center()
            .flex_none()
            .w_full()
            .h(r(LARGURA))
            .px(esp::E4)
            .gap(esp::E8)
            .child(sem_a_peca().child(self.botao_logo(m, false, precisa, cx)))
            .children(abas.map(|abas| super::sem_arrastar().flex().flex_1().min_w_0().child(abas.flex_1().mb(r(0.)))))
    }

    /// O sino: o número do que precisa de você (ou um ponto, se só há respostas novas) e o
    /// popover "O que precisa de você".
    pub(super) fn botao_sino(&self, precisa: PrecisaDeVoce, cx: &Context<Self>) -> impl IntoElement + use<> {
        let botao = BotaoRedondo::new("trilho-sino", Some(Lucide::Bell.into()), dica_sino(&precisa))
            .variante(VarianteRedonda::Plano)
            .diametro(36.)
            .dica_acao(Box::new(ProximaPendencia), None)
            .focavel(true)
            .pulso(self.geracao_do_sino(&precisa))
            .pulso_cor(cor_do_pulso(&precisa));
        let botao = match selo_do_sino(&precisa) {
            Some(selo) => botao.selo(selo),
            None => botao,
        };
        self.popover_caixa("caixa-trilho", botao, Anchor::LeftCenter, cx)
    }

    /// O logo e o menu do Vitral, controlado por `menu_aberto` para o F10 abrir o mesmo. No trilho
    /// ele abre à direita; no modo recolhido, embaixo do logo, com o que o trilho mostrava.
    fn botao_logo(&self, motor: &Motor, compacto: bool, precisa: PrecisaDeVoce, cx: &Context<Self>) -> impl IntoElement + use<> {
        let construir = self.itens_menu(motor, compacto, precisa, cx);
        let janela = cx.entity().downgrade();
        // O logo enche o botão; sobra só o aro de 2 do hover. Na lista enxuta ele divide a linha
        // com a busca e o "+", e os três têm a mesma altura à vista (36).
        let (lado, medalhao) = if compacto { (32., 28.) } else { (44., 40.) };
        Popover::new("menu-vitral")
            .appearance(false)
            .overlay_closable(false)
            .anchor(if compacto { Anchor::TopLeft } else { Anchor::LeftCenter })
            .offset(px(8.))
            .open(self.menu_aberto)
            .on_open_change(move |aberto, _, cx| {
                let _ = janela.update(cx, |this, cx| {
                    if this.menu_aberto != *aberto {
                        this.menu_aberto = *aberto;
                        cx.notify();
                    }
                });
            })
            .trigger(BotaoLogo {
                lado,
                medalhao,
                aberto: false,
                selo: compacto.then(|| selo_do_sino(&precisa)).flatten(),
                pulso: compacto.then(|| (self.geracao_do_sino(&precisa), cor_do_pulso(&precisa))),
                pronta: self.atualizacao_pronta.clone().map(SharedString::from),
            })
            .content(move |_, window, cx| {
                // Feito uma vez por abertura (o conteúdo é pedido a cada quadro) e refeito
                // depois de fechar, como o dropdown do kit.
                let guardado = window.use_keyed_state("menu-vitral-itens", cx, |_, _| None::<Entity<PopupMenu>>);
                if let Some(menu) = guardado.read(cx).clone() {
                    return menu;
                }
                let construir = construir.clone();
                let menu = PopupMenu::build(window, cx, move |menu, window, cx| construir(menu, window, cx));
                menu.focus_handle(cx).focus(window, cx);
                // Pelo F10, o primeiro item já vem realçado (o ↓ pula o cabeçalho).
                if window.last_input_was_keyboard() {
                    window.on_next_frame(|window, cx| window.dispatch_action(Box::new(SelectDown), cx));
                }
                let (popover, fraco) = (cx.entity().downgrade(), guardado.downgrade());
                window
                    .subscribe(&menu, cx, move |_, _: &DismissEvent, window, cx| {
                        if let Some(popover) = popover.upgrade() {
                            popover.update(cx, |p, cx| p.dismiss(window, cx));
                        }
                        let _ = fraco.update(cx, |g, _| *g = None);
                    })
                    .detach();
                guardado.update(cx, |g, _| *g = Some(menu.clone()));
                menu
            })
    }

    /// Os itens do menu do Vitral, em três grupos: criar, ajustes e a pasta, e a versão no rodapé.
    /// No modo recolhido, antes deles, mostrar a lista e o sino. Com
    /// uma versão nova pronta, "Reiniciar para a 0.3.0" logo acima das Configurações.
    fn itens_menu(&self, motor: &Motor, compacto: bool, precisa: PrecisaDeVoce, cx: &Context<Self>) -> ConstruirMenu {
        let janela = cx.entity().downgrade();
        let pronta = self.atualizacao_pronta.clone();
        let raiz = motor.dados.pasta.clone();
        let sem_codex = motor.codex_lido && motor.codex.is_none();
        let chats = motor.chats.iter().filter(|c| !c.arquivado).count();
        // O item diz qual é a pasta de hoje: é nela que o "+" cria.
        let rotulo_da_pasta: SharedString = match motor.config.pasta_de_trabalho.as_deref().and_then(|p| p.file_name()) {
            Some(nome) => format!("Pasta de trabalho: {}…", nome.to_string_lossy()).into(),
            None => "Escolher a pasta de trabalho…".into(),
        };
        Rc::new(move |menu, window, _| {
            let gerenciador = raiz.clone();
            menu.min_w(tam(window, 260.))
                .max_w(tam(window, 360.))
                .max_h(window.viewport_size().height - tam(window, 32.))
                .scrollable(true)
                .item(cabecalho_vitral(&raiz, chats))
                .separator()
                .when(compacto, |menu| itens_recolhido(menu, &janela, &precisa))
                // Com a lista à vista, recolhê-la é daqui (o "Mostrar a lista" é do menu recolhido).
                .when(!compacto, |menu| {
                    menu.item(
                        item("Recolher a lista", Lucide::PanelLeftClose, &janela, |this, window, cx| {
                            this.alternar_lista(window, cx)
                        })
                        .action(Box::new(AlternarLista)),
                    )
                })
                .item(
                    item("Novo chat", Lucide::MessageSquarePlus, &janela, |this, window, cx| {
                        this.novo_chat_direto(window, cx)
                    })
                    .action(Box::new(NovaConversa)),
                )
                .item(item("Novo chat em outra pasta…", Lucide::FolderOpen, &janela, |this, window, cx| {
                    this.dialogo_nova_conversa(window, cx)
                }))
                .item(item(rotulo_da_pasta.clone(), Lucide::FolderInput, &janela, |this, window, cx| {
                    this.trocar_pasta_de_trabalho(window, cx)
                }))
                .item(item("Importar do Claude Code…", Lucide::Download, &janela, |this, _, cx| {
                    if let Some(motor) = this.motor.clone() {
                        motor.update(cx, |m, cx| m.importar_do_claude_code(false, cx));
                    }
                }))
                .separator()
                .item(item("Contas do Claude", Lucide::CircleUser, &janela, |this, window, cx| {
                    this.dialogo_contas(window, cx)
                }))
                .item(item("Servidores MCP", Lucide::Container, &janela, |this, window, cx| this.abrir_mcp(window, cx)))
                .item(item("Memória da equipe…", Lucide::Brain, &janela, |this, window, cx| {
                    if let Some(motor) = this.motor.clone() {
                        super::memoria_equipe::abrir(motor, window, cx);
                    }
                }))
                // O instalador oficial do Codex é um script do sh: no Windows não há o item.
                .when(sem_codex && cfg!(not(windows)), |menu| {
                    menu.item(
                        PopupMenuItem::new("Instalar o Codex…")
                            .icon(Lucide::Download)
                            .on_click(|_, _, _| uso_plano::instalar_codex()),
                    )
                })
                .when_some(pronta.clone(), |menu, versao| {
                    let janela = janela.clone();
                    menu.item(PopupMenuItem::new(format!("Reiniciar para a {versao}")).icon(Lucide::RotateCw).on_click(
                        move |_, window, cx| {
                            let _ = janela.update(cx, |this, cx| this.reiniciar(window, cx));
                        },
                    ))
                })
                .item(
                    item("Configurações", Lucide::Settings, &janela, |this, window, cx| {
                        this.abrir_configuracoes(window, cx)
                    })
                    .action(Box::new(AbrirConfiguracoes)),
                )
                .item(
                    PopupMenuItem::new("Atalhos do teclado")
                        .icon(Lucide::Keyboard)
                        .action(Box::new(AbrirAtalhos))
                        .on_click(|_, window, cx| super::atalhos::abrir(window, cx)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Abrir a pasta de dados")
                        .icon(Lucide::FolderOpen)
                        .external()
                        .on_click(move |_, _, cx| crate::sistema::abrir_pasta(&gerenciador, cx)),
                )
                .separator()
                .item(rodape_menu_com_dica(linha_tecnica(), atualizacao::origem()))
        })
    }

    /// F10: abre ou fecha o menu do Vitral (no trilho, ou no logo do modo recolhido).
    pub(super) fn abrir_menu_vitral(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // O logo mora na fileira do alto da lista: com a lista na gaveta (ou recolhida sem caber
        // ao lado), abre a gaveta primeiro, senão o menu não teria onde aparecer.
        if self.lista && !self.orcamento.lista && self.gaveta != Some(Gaveta::Lista) {
            self.mostrar_lista(window, cx);
            self.menu_aberto = true;
        } else {
            self.menu_aberto = !self.menu_aberto;
        }
        cx.notify();
    }

    /// Começo do cabeçalho do painel 1 com a lista fora da tela. No modo recolhido (Ctrl+B), o
    /// logo com o selo do sino, que abre o mesmo menu e, pelo item do sino, a caixa; com a lista
    /// na gaveta (janela estreita), o botão de mostrá-la.
    pub(super) fn logo_compacto(&self, id: &'static str, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let Some(motor) = self.motor.as_ref().filter(|_| !self.lista) else {
            return self.botao_lista(id, window, cx).into_any_element();
        };
        let m = motor.read(cx);
        let logo = self.botao_logo(m, true, super::resumo::precisa_em_tudo(m), cx).into_any_element();
        sem_a_peca()
            .flex_none()
            .child(self.popover_caixa("caixa-logo", Gatilho(logo), Anchor::TopLeft, cx))
            .into_any_element()
    }
}

/// O topo do menu no modo recolhido: mostrar a barra lateral e o sino.
fn itens_recolhido(menu: PopupMenu, janela: &WeakEntity<Janela>, precisa: &PrecisaDeVoce) -> PopupMenu {
    let selo = selo_do_menu(precisa);
    let caixa = janela.clone();
    menu.item(
        item("Mostrar a lista", Lucide::PanelLeftOpen, janela, |this, window, cx| {
            this.alternar_lista(window, cx)
        })
        .action(Box::new(AlternarLista)),
    )
    .item(
        PopupMenuItem::element(move |_, _| {
            h_flex()
                .gap(esp::E8)
                .min_w_0()
                .child(div().flex_1().min_w_0().truncate().child("O que precisa de você"))
                .children(selo.clone())
        })
        .icon(Lucide::Bell)
        .on_click(move |_, _, cx| {
            let _ = caixa.update(cx, |this, cx| {
                this.caixa_aberta = true;
                cx.notify();
            });
        }),
    )

    .separator()
}

impl Janela {
    /// "Ver na lista" da caixa do sino (e o que o clique no sino fazia antes): a lista, sem
    /// arquivadas, no topo, onde fica "Esperando você".
    pub(super) fn mostrar_esperando(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filtro = Filtro::Todos;
        self.rolagem_lista.set_offset(Point::default());
        self.trocar_aba(AbaLateral::Conversas, window, cx);
        self.mostrar_lista(window, cx);
    }
}

#[cfg(test)]
mod testes {
    use super::{MarcaDoSino, dica_sino, marca_do_sino};
    use crate::{
        motor::{Espera, PrecisaDeVoce},
        tema::Cores,
    };

    #[test]
    fn dicas_sem_partes_zeradas() {
        let nada = PrecisaDeVoce::default();
        assert_eq!(dica_sino(&nada), "O que precisa de você");
        let mut espera = PrecisaDeVoce::default();
        for e in [Espera::Pedido, Espera::Parada, Espera::Nova, Espera::Nova] {
            espera.somar(e, false);
        }
        assert_eq!(dica_sino(&espera), "O que precisa de você\n1 pedido, 1 parada, 2 respostas novas");
    }

    #[test]
    fn sino_conta_pedidos_erros_e_paradas_e_as_novas_viram_ponto() {
        let mut p = PrecisaDeVoce::default();
        assert_eq!(marca_do_sino(&p), MarcaDoSino::Nada);
        p.somar(Espera::Nova, false);
        assert_eq!(marca_do_sino(&p), MarcaDoSino::Ponto);
        p.somar(Espera::Pedido, false);
        p.somar(Espera::Parada, false);
        assert_eq!(marca_do_sino(&p), MarcaDoSino::Numero(2, Cores::espera()));
        // A demonstração: 2 pedidos, 1 erro e 1 parada dão "4", em vermelho.
        let mut demo = PrecisaDeVoce::default();
        for (e, erro) in [(Espera::Pedido, false), (Espera::Pedido, true), (Espera::Erro, true), (Espera::Parada, false)] {
            demo.somar(e, erro);
        }
        assert_eq!(marca_do_sino(&demo), MarcaDoSino::Numero(4, Cores::vermelho()));
    }

}
