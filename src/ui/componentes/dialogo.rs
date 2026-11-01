//! Diálogos: larguras, cabeçalho, rodapé, a confirmação padrão e a tela cheia.

use std::rc::Rc;

use gpui_kit::component::{Icon, Sizable as _, WindowExt as _, button::Button, tooltip::Tooltip};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::controles::{TamanhoBotao, botao_fantasma, botao_icone, botao_perigo, botao_primario};
use crate::tema::{Cores, Lucide, Tipo as _, esp, r, raio, tam};

/// Larguras de diálogo a 100%: alerta, pequeno, médio e grande.
pub mod largura {
    pub const ALERTA: f32 = 420.;
    pub const P: f32 = 440.;
    pub const M: f32 = 560.;
    pub const G: f32 = 640.;
}

/// Título 18/600, subtítulo 14 tênue e o × de 28.
#[derive(IntoElement)]
pub struct CabecalhoDialogo {
    titulo: SharedString,
    subtitulo: Option<AnyElement>,
    simbolo: Option<AnyElement>,
    voltar: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    sem_fechar: bool,
    uma_linha: bool,
}

impl CabecalhoDialogo {
    pub fn new(t: impl Into<SharedString>) -> Self {
        CabecalhoDialogo {
            titulo: t.into(),
            subtitulo: None,
            simbolo: None,
            voltar: None,
            sem_fechar: false,
            uma_linha: false,
        }
    }

    pub fn subtitulo(mut self, e: impl IntoElement) -> Self {
        self.subtitulo = Some(e.into_any_element());
        self
    }

    pub fn simbolo(mut self, e: impl IntoElement) -> Self {
        self.simbolo = Some(e.into_any_element());
        self
    }

    /// Seta para voltar ao passo anterior, à esquerda do título.
    pub fn voltar(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.voltar = Some(Rc::new(f));
        self
    }

    pub fn sem_fechar(mut self) -> Self {
        self.sem_fechar = true;
        self
    }

    /// O título numa linha só, cortado no fim, com ele inteiro na dica (o título da tarefa na
    /// Revisão, que pode ser longo).
    pub fn uma_linha(mut self) -> Self {
        self.uma_linha = true;
        self
    }
}

impl RenderOnce for CabecalhoDialogo {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let voltar = self.voltar.map(|f| {
            botao_icone("dialogo-voltar", Lucide::ArrowLeft, "Voltar", TamanhoBotao::Barra, window, cx)
                .on_click(move |_, window, cx| f(window, cx))
        });
        let fechar = (!self.sem_fechar).then(|| {
            botao_icone("dialogo-fechar", Lucide::X, "Fechar", TamanhoBotao::Barra, window, cx)
                .on_click(|_, window, cx| window.close_dialog(cx))
        });
        div()
            .flex()
            .items_start()
            .gap(r(12.))
            .children(voltar)
            .children(self.simbolo)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(r(2.))
                    .map(|d| {
                        let titulo = div().titulo_tela().text_color(Cores::forte());
                        if self.uma_linha {
                            let dica = self.titulo.clone();
                            d.child(
                                titulo
                                    .id("dialogo-titulo")
                                    .truncate()
                                    .child(self.titulo)
                                    .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx)),
                            )
                        } else {
                            d.child(titulo.child(self.titulo))
                        }
                    })
                    .when_some(self.subtitulo, |d, s| {
                        d.child(div().corpo_ui().text_color(Cores::tenue()).child(s))
                    }),
            )
            .children(fechar)
    }
}

/// Rodapé: uma ação à esquerda e, à direita, cancelar e a principal.
#[derive(IntoElement, Default)]
pub struct RodapeDialogo {
    esquerda: Option<Button>,
    cancelar: Option<Button>,
    principal: Option<Button>,
}

impl RodapeDialogo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn esquerda(mut self, b: Button) -> Self {
        self.esquerda = Some(b);
        self
    }

    pub fn cancelar(mut self, b: Button) -> Self {
        self.cancelar = Some(b);
        self
    }

    pub fn principal(mut self, b: Button) -> Self {
        self.principal = Some(b);
        self
    }
}

impl RenderOnce for RodapeDialogo {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(r(8.))
            .pt(r(8.))
            .children(self.esquerda)
            .child(div().flex_1())
            .children(self.cancelar)
            .children(self.principal)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TomConfirmacao {
    Neutro,
    Atencao,
    Perigo,
}

impl TomConfirmacao {
    fn cor(self) -> Hsla {
        match self {
            TomConfirmacao::Neutro => Cores::destaque_texto(),
            TomConfirmacao::Atencao => Cores::ambar(),
            TomConfirmacao::Perigo => Cores::vermelho(),
        }
    }
}

/// O que a confirmação pergunta. Cada detalhe é (ícone, texto, se vai na cor do tom).
pub struct Confirmacao {
    pub tom: TomConfirmacao,
    pub icone: Icon,
    pub titulo: SharedString,
    pub texto: SharedString,
    pub detalhes: Vec<(Icon, SharedString, bool)>,
    pub ok: SharedString,
    pub cancelar: SharedString,
    pub ok_perigo: bool,
}

/// Alerta de confirmação com rodapé próprio (o do kit é centralizado) e a moldura dos diálogos.
/// Com o OK perigoso, o Enter só fecha: a ação destrutiva pede o clique. O `ao_fechar` vale para
/// qualquer saída: Esc e Enter passam pelo `on_close` do kit, e os botões o chamam antes do
/// `close_dialog`, que não passa.
pub fn confirmar(
    c: Confirmacao,
    ao_confirmar: impl Fn(&mut Window, &mut App) + 'static,
    ao_fechar: Option<Rc<dyn Fn(&mut App)>>,
    window: &mut Window,
    cx: &mut App,
) {
    let ao_confirmar: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(ao_confirmar);
    let c = Rc::new(c);
    let largura_alerta = tam(window, largura::ALERTA);
    window.open_alert_dialog(cx, move |alerta, window, cx| {
        let cor = c.tom.cor();
        let detalhes = c.detalhes.iter().map(|(icone, texto, destaque)| {
            div()
                .flex()
                .items_start()
                .gap(r(8.))
                .text_size(r(13.))
                .line_height(r(18.))
                .text_color(if *destaque { cor } else { Cores::tenue() })
                .child(div().flex_none().pt(r(2.)).child(icone.clone().with_size(tam(window, 14.))))
                .child(div().min_w_0().child(texto.clone()))
        });
        let descricao = div()
            .flex()
            .flex_col()
            .gap(r(8.))
            .child(div().corpo_ui().text_color(Cores::tenue()).child(c.texto.clone()))
            .when(!c.detalhes.is_empty(), |s| {
                s.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(r(6.))
                        .px(r(12.))
                        .py(r(8.))
                        .rounded(raio::CARTAO)
                        .bg(Cores::recuo())
                        .children(detalhes),
                )
            });
        let fechar = {
            let ao_fechar = ao_fechar.clone();
            move |window: &mut Window, cx: &mut App| {
                if let Some(f) = &ao_fechar {
                    f(cx);
                }
                window.close_dialog(cx);
            }
        };
        let ok = {
            let (ao_confirmar, fechar) = (ao_confirmar.clone(), fechar.clone());
            let botao = if c.ok_perigo {
                botao_perigo("confirmar-ok", None, c.ok.clone(), TamanhoBotao::Controle, window, cx)
            } else {
                botao_primario("confirmar-ok", None, c.ok.clone(), TamanhoBotao::Controle, window, cx)
            };
            botao.on_click(move |_, window, cx| {
                ao_confirmar(window, cx);
                fechar(window, cx);
            })
        };
        let cancelar = botao_fantasma("confirmar-cancelar", None, c.cancelar.clone(), TamanhoBotao::Controle, window, cx)
            .on_click(move |_, window, cx| fechar(window, cx));
        let enter_confirma = !c.ok_perigo;
        let ao_confirmar = ao_confirmar.clone();
        let ao_fechar = ao_fechar.clone();
        let alerta = alerta
            .bg(Cores::dialogo())
            .width(largura_alerta)
            .p(r(24.))
            .rounded(raio::PECA)
            .border_color(Cores::borda_forte())
            .icon(
                div()
                    .size(r(40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(Cores::tinta(cor))
                    .text_color(cor)
                    .child(c.icone.clone().with_size(tam(window, 20.))),
            )
            .title(div().titulo_tela().text_color(Cores::forte()).child(c.titulo.clone()))
            .description(descricao)
            .footer(RodapeDialogo::new().cancelar(cancelar).principal(ok))
            .on_ok(move |_, window, cx| {
                if enter_confirma {
                    ao_confirmar(window, cx);
                }
                true
            });
        match ao_fechar {
            Some(f) => alerta.on_close(move |_, _, cx| f(cx)),
            None => alerta,
        }
    });
}

/// Tela inteira centrada num cartão (boas-vindas e, depois, a recuperação do Mestre).
#[derive(IntoElement)]
pub struct TelaCheia {
    simbolo: Option<AnyElement>,
    titulo: SharedString,
    texto: Option<SharedString>,
    corpo: Vec<AnyElement>,
}

impl TelaCheia {
    pub fn new(titulo: impl Into<SharedString>) -> Self {
        TelaCheia { simbolo: None, titulo: titulo.into(), texto: None, corpo: Vec::new() }
    }

    pub fn simbolo(mut self, e: impl IntoElement) -> Self {
        self.simbolo = Some(e.into_any_element());
        self
    }

    pub fn texto(mut self, t: impl Into<SharedString>) -> Self {
        self.texto = Some(t.into());
        self
    }
}

impl ParentElement for TelaCheia {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.corpo.extend(elements);
    }
}

impl RenderOnce for TelaCheia {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let largura = tam(window, 520.).min(window.viewport_size().width - px(32.));
        div().flex().flex_col().flex_1().size_full().items_center().justify_center().child(
            div()
                .flex()
                .flex_col()
                .w(largura)
                .p(esp::E32)
                .gap(esp::E24)
                .rounded(raio::PECA)
                .bg(Cores::cartao())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(r(12.))
                        .children(self.simbolo)
                        .child(div().titulo_tela().text_color(Cores::forte()).child(self.titulo)),
                )
                .when_some(self.texto, |d, t| d.child(div().corpo_ui().text_color(Cores::tenue()).child(t)))
                .children(self.corpo),
        )
    }
}
