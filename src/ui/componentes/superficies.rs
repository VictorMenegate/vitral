//! Superfícies flutuantes (popover, cabeçalhos e rodapés de menu, avisos), alerta em linha,
//! estado vazio e o desenho das teclas de atalho.

use std::rc::Rc;

use gpui_kit::component::{
    Icon, Sizable as _, StyledExt as _, ThemeStyled as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    kbd::Kbd,
    menu::PopupMenuItem,
    notification::{Notification, NotificationType},
    popover::Popover,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::base::Simbolo;
use super::controles::{TamanhoBotao, botao_fantasma, botao_primario, variante_fantasma};
use super::dialogo::{RodapeDialogo, largura::ALERTA};
use super::estado::Selo;
use crate::tema::{
    Cores, Lucide, Tipo as _, dentro_da_borda, esp, gradiente_quente_em, gradiente_reflexo, linha_de_luz, r, raio,
    tam,
};

/// Superfície desenhada à mão que flutua sobre texto: a mesma do `popover_style` do kit
/// (opaca, com anel, sombra, a linha de luz no topo e o raio de peça).
pub trait Flutuante: Styled + Sized {
    fn flutuante(self, cx: &App) -> Self {
        self.popover_style(cx)
    }
}

impl<T: Styled> Flutuante for T {}

/// O reflexo do vidro numa peça ou cartão: a linha de luz de 1 px no topo de dentro e o brilho de
/// #ffffff08 que some nos primeiros 40 px, numa camada por baixo do conteúdo (chame antes dos
/// filhos) com o raio de fora da peça. Nada anima. Nos cartões com reflexo o contorno desce de
/// `Cores::borda()` para `Cores::linha()`.
pub trait Reflexo: ParentElement + Sized {
    fn reflexo(self, raio: Pixels) -> Self {
        self.child(camada_de_reflexo(raio))
    }
}

impl<T: ParentElement> Reflexo for T {}

/// A camada do `.reflexo()`, para quem monta a peça à mão.
pub fn camada_de_reflexo(raio: Pixels) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(r(40.))
        .max_h(relative(1.))
        .rounded_t((raio - px(1.)).max(px(0.)))
        .bg(gradiente_reflexo())
        .shadow(vec![linha_de_luz(1.)])
}

/// Popover da linguagem: raio de peça e margem 16. Fundo, anel e sombra vêm do kit, e a seta sai
/// com o mesmo contorno.
pub fn popover(id: impl Into<ElementId>) -> Popover {
    Popover::new(id).rounded(raio::PECA).p(r(16.))
}

/// Larguras únicas de popover (sempre limitadas à janela menos 32).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LarguraPopover {
    P320,
    P360,
    P400,
    /// O das issues, que tem lista e detalhe (a largura P dos diálogos).
    P440,
    P560,
}

impl LarguraPopover {
    pub fn px(self) -> f32 {
        match self {
            LarguraPopover::P320 => 320.,
            LarguraPopover::P360 => 360.,
            LarguraPopover::P400 => 400.,
            LarguraPopover::P440 => 440.,
            LarguraPopover::P560 => 560.,
        }
    }
}

/// Cabeçalho de popover: de escolha (esforço, modelo) ou de objeto (conta, projeto).
pub enum CabecalhoPopover {
    Escolha {
        simbolo: Option<Simbolo>,
        titulo: SharedString,
        valor: Option<(SharedString, Hsla)>,
        dica: Option<SharedString>,
    },
    Objeto {
        simbolo: Simbolo,
        nome: SharedString,
        detalhe: Option<SharedString>,
        selo: Option<Selo>,
    },
}

fn linha_objeto(
    simbolo: Option<AnyElement>,
    nome: SharedString,
    detalhe: Option<SharedString>,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(esp::E12)
        .min_w_0()
        .children(simbolo)
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(r(14.))
                        .line_height(r(20.))
                        .font_semibold()
                        .text_color(Cores::texto())
                        .child(nome),
                )
                // Caminho e branch: o começo e o fim é que dizem qual é.
                .when_some(detalhe, |s, d| {
                    s.child(
                        div()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_middle()
                            .metadado()
                            .text_color(Cores::tenue())
                            .child(d),
                    )
                }),
        )
}

fn linha_escolha(
    simbolo: Option<AnyElement>,
    titulo: SharedString,
    valor: Option<(SharedString, Hsla)>,
    direita: Option<AnyElement>,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(r(8.))
        .min_h(r(20.))
        .children(simbolo)
        .child(div().rotulo_secao().child(titulo))
        .when_some(valor, |s, (valor, cor)| {
            s.child(
                div()
                    .text_size(r(13.))
                    .font_semibold()
                    .text_color(cor)
                    .child(valor),
            )
        })
        .child(div().flex_1())
        .children(direita)
}

/// Corpo padrão de popover: cabeçalho, conteúdo e rodapé depois de um separador.
#[derive(IntoElement)]
pub struct CorpoPopover {
    largura: LarguraPopover,
    cabecalho: Option<CabecalhoPopover>,
    corpo: Vec<AnyElement>,
    rodape: Vec<AnyElement>,
}

impl CorpoPopover {
    pub fn new(largura: LarguraPopover) -> Self {
        CorpoPopover { largura, cabecalho: None, corpo: Vec::new(), rodape: Vec::new() }
    }

    pub fn cabecalho(mut self, c: CabecalhoPopover) -> Self {
        self.cabecalho = Some(c);
        self
    }

    pub fn rodape(mut self, e: impl IntoElement) -> Self {
        self.rodape.push(e.into_any_element());
        self
    }
}

impl ParentElement for CorpoPopover {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.corpo.extend(elements);
    }
}

impl RenderOnce for CorpoPopover {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        // A margem de 16 de cada lado é do popover; aqui fica o conteúdo.
        let largura = (tam(window, self.largura.px()) - tam(window, 32.))
            .min(window.viewport_size().width - px(64.));
        let cabecalho = self.cabecalho.map(|c| match c {
            CabecalhoPopover::Escolha { simbolo, titulo, valor, dica } => linha_escolha(
                simbolo.map(|s| s.desenhar(14., window)),
                titulo,
                valor,
                dica.map(|d| div().metadado().text_color(Cores::tenue()).child(d).into_any_element()),
            ),
            CabecalhoPopover::Objeto { simbolo, nome, detalhe, selo } => {
                linha_objeto(Some(simbolo.desenhar(20., window)), nome, detalhe)
                    .child(div().flex_1())
                    .children(selo)
            }
        });
        let tem_rodape = !self.rodape.is_empty();
        div()
            .flex()
            .flex_col()
            .w(largura)
            .gap(r(12.))
            .children(cabecalho)
            .children(self.corpo)
            .when(tem_rodape, |s| {
                s.child(div().h(px(1.)).bg(Cores::borda()))
                    .child(div().flex().flex_col().gap(r(8.)).children(self.rodape))
            })
    }
}

/// Cabeçalho de objeto no topo do menu (logo e todo "…"): símbolo 20, nome e detalhe. Linha
/// fixa de 44, fora do teclado; o separador vem logo depois.
pub fn cabecalho_objeto(
    simbolo: Simbolo,
    nome: impl Into<SharedString>,
    detalhe: Option<SharedString>,
) -> PopupMenuItem {
    let nome = nome.into();
    PopupMenuItem::static_element(move |window, _| {
        linha_objeto(simbolo.copia().map(|s| s.desenhar(20., window)), nome.clone(), detalhe.clone())
            .h(r(44.))
    })
}

/// O mesmo cabeçalho com uma dica (o detalhe curto, "ws", e o caminho inteiro na dica).
pub fn cabecalho_objeto_com_dica(
    simbolo: Simbolo,
    nome: impl Into<SharedString>,
    detalhe: Option<SharedString>,
    dica: impl Into<SharedString>,
) -> PopupMenuItem {
    let (nome, dica) = (nome.into(), dica.into());
    PopupMenuItem::static_element(move |window, _| {
        let dica = dica.clone();
        linha_objeto(simbolo.copia().map(|s| s.desenhar(20., window)), nome.clone(), detalhe.clone())
            .id("cabecalho-objeto")
            .h(r(44.))
            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
    })
}

/// Onde o cabeçalho de escolha procura o atalho no keymap.
pub enum OndeAtalho {
    /// Num contexto só (ou fora de contexto, com `None`).
    Contexto(Option<&'static str>),
    /// No caminho de foco do elemento, que acha predicados com `>` ("Composer > Input").
    Foco(FocusHandle),
}

/// Cabeçalho de escolha (modo, conta): título 12/600 e, à direita, as teclas do atalho do keymap
/// (no desenho de teclas do sino e das dicas) seguidas de um texto ("Shift Tab troca").
pub fn cabecalho_escolha(
    titulo: impl Into<SharedString>,
    atalho: Option<(Box<dyn Action>, OndeAtalho, &'static str)>,
) -> PopupMenuItem {
    let titulo = titulo.into();
    PopupMenuItem::static_element(move |window, _| {
        let direita = atalho.as_ref().and_then(|(acao, onde, depois)| {
            let kbd = match onde {
                OndeAtalho::Contexto(contexto) => Kbd::binding_for_action(acao.as_ref(), *contexto, window),
                OndeAtalho::Foco(foco) => Kbd::binding_for_action_in(acao.as_ref(), foco, window),
            }?;
            Some(
                div()
                    .flex()
                    .items_center()
                    .gap(esp::E6)
                    .metadado()
                    .text_color(Cores::tenue())
                    .child(desenho_teclas(kbd.partes()))
                    .child(*depois)
                    .into_any_element(),
            )
        });
        linha_escolha(None, titulo.clone(), None, direita).h(r(28.))
    })
}

/// Item de duas linhas (48): título 14/500 e descrição 12 tênue, com o check à direita quando
/// escolhido. Sem `.icon()`, para não reservar coluna vazia nos outros itens.
pub fn item_descrito(
    simbolo: Option<Simbolo>,
    titulo: impl Into<SharedString>,
    descricao: impl Into<SharedString>,
    escolhido: bool,
) -> PopupMenuItem {
    let (titulo, descricao) = (titulo.into(), descricao.into());
    PopupMenuItem::element(move |window, _| {
        div()
            .flex()
            .items_center()
            .gap(esp::E12)
            .h(r(48.))
            .min_w_0()
            .children(simbolo.as_ref().and_then(Simbolo::copia).map(|s| s.desenhar(16., window)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_size(r(14.))
                            .line_height(r(20.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(titulo.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .metadado()
                            .text_color(Cores::tenue())
                            .child(descricao.clone()),
                    ),
            )
    })
    .checked(escolhido)
}

/// Rodapé de dica do menu (28, 12 tênue) com o atalho à direita; vai depois de um separador.
pub fn rodape_menu(
    texto: impl Into<SharedString>,
    atalho: Option<(Box<dyn Action>, Option<&'static str>)>,
) -> PopupMenuItem {
    let texto = texto.into();
    PopupMenuItem::static_element(move |window, _| {
        let teclas = atalho
            .as_ref()
            .and_then(|(acao, contexto)| teclas(acao.as_ref(), *contexto, window));
        div()
            .flex()
            .items_center()
            .gap(r(12.))
            .h(r(28.))
            .metadado()
            .text_color(Cores::tenue())
            .child(div().flex_1().min_w_0().truncate().child(texto.clone()))
            .children(teclas)
    })
}

/// Rodapé de texto (28, 12 tênue) com uma dica: a versão no menu do logo, com o commit na dica.
pub fn rodape_menu_com_dica(texto: impl Into<SharedString>, dica: impl Into<SharedString>) -> PopupMenuItem {
    let (texto, dica) = (texto.into(), dica.into());
    PopupMenuItem::static_element(move |_, _| {
        let dica = dica.clone();
        div()
            .id("rodape-menu")
            .flex()
            .items_center()
            .h(r(28.))
            .metadado()
            .text_color(Cores::tenue())
            .child(div().flex_1().min_w_0().truncate().child(texto.clone()))
            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
    })
}

/// Tom de alertas e avisos: a cor fica no ícone (e na tinta do alerta).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tom {
    Info,
    Atencao,
    Erro,
    Pronto,
}

impl Tom {
    pub fn icone(self) -> Icon {
        match self {
            Tom::Info => Lucide::Info.into(),
            Tom::Atencao => Lucide::TriangleAlert.into(),
            Tom::Erro => Lucide::CircleX.into(),
            Tom::Pronto => Lucide::CircleCheck.into(),
        }
    }

    pub fn cor(self) -> Hsla {
        match self {
            Tom::Info => Cores::tenue(),
            Tom::Atencao => Cores::ambar(),
            Tom::Erro => Cores::vermelho(),
            Tom::Pronto => Cores::verde(),
        }
    }
}

/// Alerta em linha: o que precisa ficar na tela (aviso de painel, validação de formulário).
#[derive(IntoElement)]
pub struct Alerta {
    tom: Tom,
    titulo: Option<SharedString>,
    texto: SharedString,
    /// O mesmo texto com trechos realçados (o termo do Ctrl+F), quando há.
    texto_rico: Option<StyledText>,
    acao: Option<AnyElement>,
    fechar: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    compacto: bool,
}

impl Alerta {
    pub fn new(tom: Tom, texto: impl Into<SharedString>) -> Self {
        Alerta { tom, titulo: None, texto: texto.into(), texto_rico: None, acao: None, fechar: None, compacto: false }
    }

    pub fn titulo(mut self, t: impl Into<SharedString>) -> Self {
        self.titulo = Some(t.into());
        self
    }

    /// O texto com trechos em outro fundo (o termo achado na busca da conversa); precisa ser o
    /// mesmo texto, que continua valendo para a chave do fechar.
    pub fn texto_rico(mut self, t: StyledText) -> Self {
        self.texto_rico = Some(t);
        self
    }

    pub fn acao(mut self, e: impl IntoElement) -> Self {
        self.acao = Some(e.into_any_element());
        self
    }

    pub fn fechar(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.fechar = Some(Rc::new(f));
        self
    }

    /// Numa linha, sem caixa: só o ícone na cor e o texto 12 tênue.
    pub fn compacto(mut self) -> Self {
        self.compacto = true;
        self
    }
}

impl RenderOnce for Alerta {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let cor = self.tom.cor();
        if self.compacto {
            return div()
                .flex()
                .items_start()
                .gap(r(6.))
                .metadado()
                .text_color(Cores::tenue())
                .child(
                    div()
                        .flex_none()
                        .h(r(16.))
                        .flex()
                        .items_center()
                        .text_color(cor)
                        .child(self.tom.icone().with_size(tam(window, 14.))),
                )
                .child(match self.texto_rico {
                    Some(rico) => div().min_w_0().child(rico),
                    None => div().min_w_0().child(self.texto),
                })
                .into_any_element();
        }
        let id_fechar = SharedString::from(format!("alerta-fechar-{}", self.texto));
        let (fundo, borda) = if self.tom == Tom::Info {
            (Cores::hover(), Cores::borda())
        } else {
            (Cores::tinta(cor), Cores::tinta_borda(cor))
        };
        div()
            .flex()
            .items_start()
            .gap(esp::E8)
            .p(dentro_da_borda(12.))
            .rounded(raio::CARTAO)
            .bg(fundo)
            .border_1()
            .border_color(borda)
            .child(
                div()
                    .flex_none()
                    .h(r(20.))
                    .flex()
                    .items_center()
                    .text_color(cor)
                    .child(self.tom.icone().with_size(tam(window, 16.))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(r(2.))
                    .when_some(self.titulo, |s, t| {
                        s.child(
                            div()
                                .text_size(r(14.))
                                .line_height(r(20.))
                                .font_semibold()
                                .text_color(Cores::texto())
                                .child(t),
                        )
                    })
                    .child(
                        div()
                            .text_size(r(13.))
                            .line_height(r(18.))
                            .text_color(Cores::tenue())
                            .map(|s| match self.texto_rico {
                                Some(rico) => s.child(rico),
                                None => s.child(self.texto),
                            }),
                    )
                    .when_some(self.acao, |s, a| s.child(div().pt(r(6.)).child(a))),
            )
            .when_some(self.fechar, |s, f| {
                s.child(
                    Button::new(id_fechar)
                        .custom(variante_fantasma(cx).foreground(Cores::tenue()))
                        .icon(Lucide::X)
                        .xsmall()
                        .rounded(raio::PILULA)
                        .tooltip("Fechar")
                        .on_click(move |_, window, cx| f(window, cx)),
                )
            })
            .into_any_element()
    }
}

/// Marca de tipo para os avisos que se substituem em vez de empilhar.
pub struct AvisoVitral;

/// Mesmo assunto, mesmo aviso: o novo substitui o anterior.
pub trait ChaveAviso {
    fn chave(self, k: &'static str) -> Self;
}

impl ChaveAviso for Notification {
    fn chave(self, k: &'static str) -> Self {
        self.id1::<AvisoVitral>(k)
    }
}

/// Aviso (toast) do que você acabou de fazer. Erro fica até fechar.
pub fn aviso(tom: Tom, texto: impl Into<SharedString>) -> Notification {
    let aviso = Notification::new().message(texto);
    match tom {
        Tom::Info => aviso.icon(Icon::new(Lucide::Info).text_color(Cores::tenue())),
        Tom::Pronto => aviso.with_type(NotificationType::Success),
        Tom::Atencao => aviso.with_type(NotificationType::Warning),
        Tom::Erro => aviso.with_type(NotificationType::Error).autohide(false),
    }
}

/// Aviso com um botão de ação (Desfazer, Abrir): fica até a ação ou o ×.
pub fn aviso_com_acao(
    tom: Tom,
    texto: impl Into<SharedString>,
    rotulo: &'static str,
    f: impl Fn(&mut Window, &mut App) + 'static,
) -> Notification {
    let f = Rc::new(f);
    aviso(tom, texto).action(move |_, _, cx| {
        let f = f.clone();
        botao_do_aviso(rotulo, cx).on_click(cx.listener(move |aviso, _, window, cx| {
            f(window, cx);
            aviso.dismiss(window, cx);
        }))
    })
}

/// O botão da ação de um aviso (28, texto 13/600 em azul, sem fundo).
pub fn botao_do_aviso(rotulo: &'static str, cx: &App) -> Button {
    Button::new("aviso-acao")
        .custom(variante_fantasma(cx).foreground(Cores::destaque_texto()))
        .h(r(28.))
        .px(esp::E12)
        .rounded(raio::PILULA)
        .child(div().text_size(r(13.)).font_semibold().child(rotulo))
}

/// "Não deu para …": o resumo no título, a primeira linha da causa no texto e "Detalhes", que
/// abre a causa inteira com "Copiar".
pub fn aviso_erro(resumo: impl Into<SharedString>, erro: &anyhow::Error) -> Notification {
    let resumo: SharedString = resumo.into();
    let causa: SharedString = format!("{erro:#}").into();
    let primeira = causa.lines().next().unwrap_or_default().to_string();
    let (titulo, completa) = (resumo.clone(), causa.clone());
    aviso_com_acao(Tom::Erro, primeira, "Detalhes", move |window, cx| {
        let (titulo, completa) = (titulo.clone(), completa.clone());
        window.open_alert_dialog(cx, move |alerta, window, cx| {
            let copiar = completa.clone();
            let rodape = RodapeDialogo::new()
                .esquerda(
                    botao_fantasma("detalhes-copiar", Some(Lucide::Copy.into()), "Copiar", TamanhoBotao::Controle, window, cx)
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copiar.to_string()))
                        }),
                )
                .principal(
                    botao_primario("detalhes-fechar", None, "Fechar", TamanhoBotao::Controle, window, cx)
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                );
            alerta
                .bg(Cores::dialogo())
                .width(tam(window, ALERTA))
                .p(r(24.))
                .rounded(raio::PECA)
                .border_color(Cores::borda_forte())
                .title(div().titulo_tela().text_color(Cores::forte()).child(titulo.clone()))
                .description(
                    div()
                        .max_h(r(320.))
                        .overflow_hidden()
                        .mono_pequeno()
                        .text_color(Cores::texto())
                        .child(completa.clone()),
                )
                .footer(rodape)
        });
    })
    .title(resumo)
}

/// Estado vazio: compacto (listas, painéis, popovers) ou amplo (painel sem conversa).
#[derive(IntoElement)]
pub struct EstadoVazio {
    icone: Icon,
    titulo: SharedString,
    texto: Option<SharedString>,
    acoes: Vec<AnyElement>,
    amplo: bool,
}

impl EstadoVazio {
    pub fn new(icone: impl Into<Icon>, titulo: impl Into<SharedString>) -> Self {
        EstadoVazio { icone: icone.into(), titulo: titulo.into(), texto: None, acoes: Vec::new(), amplo: false }
    }

    pub fn texto(mut self, t: impl Into<SharedString>) -> Self {
        self.texto = Some(t.into());
        self
    }

    pub fn acao(mut self, e: impl IntoElement) -> Self {
        self.acoes.push(e.into_any_element());
        self
    }

    /// Círculo de 64 com o gradiente quente, título 18/600 e texto 14.
    pub fn amplo(mut self) -> Self {
        self.amplo = true;
        self
    }
}

impl RenderOnce for EstadoVazio {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let amplo = self.amplo;
        let circulo = if amplo {
            div()
                .size(r(64.))
                .rounded_full()
                .bg(gradiente_quente_em(135.))
                .text_color(Cores::forte())
                .child(self.icone.with_size(tam(window, 28.)))
        } else {
            div()
                .size(r(40.))
                .rounded_full()
                .bg(Cores::linha())
                .text_color(Cores::tenue())
                .child(self.icone.with_size(tam(window, 20.)))
        };
        div()
            .flex()
            .flex_col()
            .items_center()
            .text_center()
            .gap(r(8.))
            .py(r(24.))
            .child(circulo.flex().flex_none().items_center().justify_center())
            .child(
                div()
                    .map(|s| if amplo { s.titulo_tela() } else { s.text_size(r(14.)).line_height(r(20.)).font_semibold() })
                    .text_color(if amplo { Cores::forte() } else { Cores::texto() })
                    .child(self.titulo),
            )
            .when_some(self.texto, |s, t| {
                s.child(
                    div()
                        .max_w(r(if amplo { 420. } else { 300. }))
                        .map(|s| if amplo { s.corpo_ui() } else { s.text_size(r(13.)).line_height(r(18.)) })
                        .text_color(Cores::tenue())
                        .child(t),
                )
            })
            .when(!self.acoes.is_empty(), |s| {
                s.child(div().flex().flex_wrap().justify_center().gap(r(8.)).pt(r(4.)).children(self.acoes))
            })
    }
}

/// Teclas separadas de uma combinação, cada uma no seu desenho de tecla.
pub fn desenho_teclas(partes: Vec<SharedString>) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .gap(esp::E4)
        .children(partes.into_iter().map(|tecla| {
            div()
                .h(r(18.))
                .px(esp::E4)
                .flex()
                .items_center()
                .rounded(raio::CONTROLE)
                .bg(Cores::recuo())
                .border_1()
                .border_color(Cores::borda())
                .micro()
                .text_color(Cores::tenue())
                .child(tecla)
        }))
        .into_any_element()
}

/// O atalho de uma ação, tirado do keymap, em teclas separadas (rodapés de dica).
pub fn teclas(acao: &dyn Action, contexto: Option<&str>, window: &Window) -> Option<AnyElement> {
    let kbd = Kbd::binding_for_action(acao, contexto, window)?;
    Some(desenho_teclas(kbd.partes()))
}

#[cfg(test)]
mod testes {
    use super::{cabecalho_escolha, cabecalho_objeto, item_descrito, rodape_menu};
    use crate::tema::Lucide;
    use crate::ui::componentes::Simbolo;
    use gpui_kit::component::{kbd::Kbd, menu::PopupMenuItem};
    use gpui_kit::{Keystroke, div};

    gpui_kit::actions!(teste_menu, [AcaoDeTeste]);

    fn partes(combinacao: &'static str) -> Vec<String> {
        let kbd = Kbd::new(Keystroke::parse(combinacao).unwrap());
        kbd.partes().iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn teclas_do_atalho_uma_a_uma() {
        assert_eq!(partes("ctrl-shift-n"), ["Ctrl", "Shift", "N"]);
        assert_eq!(partes("ctrl-+"), ["Ctrl", "+"]);
        assert_eq!(partes("escape"), ["Esc"]);
        assert_eq!(partes("super-a"), ["Super", "A"]);
        assert_eq!(partes("alt-shift-pageup"), ["Alt", "Shift", "Page Up"]);
        assert_eq!(Kbd::format(&Keystroke::parse("super-a").unwrap()), "Super+A");
    }

    #[test]
    fn linhas_fixas_e_desabilitados_ficam_fora_do_teclado() {
        assert!(!PopupMenuItem::static_element(|_, _| div()).navegavel());
        assert!(!cabecalho_objeto(Simbolo::icone(Lucide::Folder), "Vitral", None).navegavel());
        assert!(!cabecalho_escolha("Modo de permissão", None).navegavel());
        assert!(!rodape_menu("Cole imagens direto na caixa", None).navegavel());
        assert!(!PopupMenuItem::separator().navegavel());
        assert!(!PopupMenuItem::label("Projetos").navegavel());
        assert!(PopupMenuItem::new("Apagar a cópia…").danger(true).navegavel());
        assert!(!PopupMenuItem::new("Apagar a cópia…").danger(true).disabled(true).navegavel());
        assert!(item_descrito(None, "Manual", "Pede aprovação", true).navegavel());
    }

    #[test]
    fn item_de_elemento_com_acao_mostra_o_atalho() {
        assert!(PopupMenuItem::element(|_, _| div()).action(Box::new(AcaoDeTeste)).mostra_atalho());
        assert!(!PopupMenuItem::element(|_, _| div()).mostra_atalho());
        assert!(item_descrito(None, "Manual", "Pede aprovação", false)
            .action(Box::new(AcaoDeTeste))
            .mostra_atalho());
        assert!(PopupMenuItem::new("Nova conversa").action(Box::new(AcaoDeTeste)).mostra_atalho());
        assert!(!PopupMenuItem::static_element(|_, _| div()).mostra_atalho());
    }
}
