//! Peças básicas: ícone em rem, o símbolo que vai à esquerda de chips, linhas e cartões, as
//! marcas, os papéis, o arco-íris e o quadrado do projeto.

use std::sync::{Arc, LazyLock};

use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::estado::Sinal;
use super::graficos::{Anel, BarrinhasDeSinal};
use crate::motor::Motor;
use crate::tema::{ARCO_IRIS, Cores, Lucide, r, raio, tam};
use crate::ui::movimento::{self, Curva};
use crate::ui::icone_projeto::{self, Icone};

/// Ícone no tamanho de desenho (px a 100%), crescendo com o zoom.
pub fn icone(nome: impl Into<Icon>, px: f32, window: &Window) -> Icon {
    nome.into().with_size(tam(window, px))
}

/// O que vai à esquerda de um chip, linha, aba ou cartão.
pub enum Simbolo {
    /// Lucide; a cor só quando ela é o sinal (modo, estado).
    Icone(Icon, Option<Hsla>),
    /// Logo colorido (via `img()`, nunca `svg()`, que pinta numa cor só).
    Marca(Marca),
    Projeto(&'static Icone, u32),
    Papel(Papel),
    Sinal { nivel: u8, total: u8 },
    Anel { fracao: f32, cor: Option<Hsla> },
    /// Imagem guardada (o logo do Vitral): se copia, ao contrário de um `Elemento`.
    Imagem(Arc<Image>),
    Elemento(AnyElement),
}

impl Simbolo {
    pub fn icone(icone: impl Into<Icon>) -> Self {
        Simbolo::Icone(icone.into(), None)
    }

    pub fn icone_cor(icone: impl Into<Icon>, cor: Hsla) -> Self {
        Simbolo::Icone(icone.into(), Some(cor))
    }

    /// Cópia para quem desenha a cada quadro (menus, listas). Um `Elemento` não se copia.
    pub fn copia(&self) -> Option<Simbolo> {
        Some(match self {
            Simbolo::Icone(i, c) => Simbolo::Icone(i.clone(), *c),
            Simbolo::Marca(m) => Simbolo::Marca(*m),
            Simbolo::Projeto(i, c) => Simbolo::Projeto(i, *c),
            Simbolo::Papel(p) => Simbolo::Papel(*p),
            Simbolo::Sinal { nivel, total } => Simbolo::Sinal { nivel: *nivel, total: *total },
            Simbolo::Anel { fracao, cor } => Simbolo::Anel { fracao: *fracao, cor: *cor },
            Simbolo::Imagem(imagem) => Simbolo::Imagem(imagem.clone()),
            Simbolo::Elemento(_) => return None,
        })
    }

    /// Desenha o símbolo num lado de `lado` px a 100%.
    pub fn desenhar(self, lado: f32, window: &Window) -> AnyElement {
        match self {
            Simbolo::Icone(i, cor) => i
                .with_size(tam(window, lado))
                .flex_none()
                .when_some(cor, |i, cor| i.text_color(cor))
                .into_any_element(),
            Simbolo::Marca(marca) => marca.disco(lado).into_any_element(),
            Simbolo::Projeto(icone, cor) => QuadradoDeProjeto::new(icone, cor, lado).into_any_element(),
            Simbolo::Papel(papel) => papel.circulo(lado).into_any_element(),
            Simbolo::Sinal { nivel, total } => {
                BarrinhasDeSinal::new(nivel, total).altura(lado).into_any_element()
            }
            Simbolo::Anel { fracao, cor } => Anel::new("simbolo-anel", fracao)
                .seco()
                .diametro(lado)
                .when_some(cor, |anel, cor| anel.cor(cor))
                .into_any_element(),
            Simbolo::Imagem(imagem) => img(imagem).flex_none().size(r(lado)).into_any_element(),
            Simbolo::Elemento(elemento) => elemento,
        }
    }
}

/// Quem age numa conversa.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Papel {
    Coordenador,
    Trabalhador,
    Mestre,
    Avulso,
}

impl Papel {
    pub fn icone(self) -> Icon {
        match self {
            Papel::Coordenador => Lucide::Bot.into(),
            Papel::Trabalhador => Lucide::Wrench.into(),
            Papel::Mestre => Lucide::Orbit.into(),
            Papel::Avulso => Lucide::MessageCircle.into(),
        }
    }

    pub fn circulo(self, lado: f32) -> CirculoDePapel {
        CirculoDePapel { papel: self, lado }
    }
}

/// Círculo de quem age: neutro, e o Mestre na tinta do destaque.
#[derive(IntoElement)]
pub struct CirculoDePapel {
    papel: Papel,
    lado: f32,
}

impl RenderOnce for CirculoDePapel {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let mestre = self.papel == Papel::Mestre;
        div()
            .flex_none()
            .size(r(self.lado))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(if mestre { Cores::tinta(Cores::destaque()) } else { Cores::hover() })
            .text_color(if mestre { Cores::destaque_texto() } else { Cores::tenue() })
            .child(icone(self.papel.icone(), (self.lado * 0.5).max(12.), window))
    }
}

/// Marca de um serviço: as IAs e o Linear (o GitHub é o ícone `Github` do Lucide).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Marca {
    Claude,
    Codex,
    Linear,
}

static DISCO_CLAUDE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../../assets/marcas/claude.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});
static DISCO_CODEX: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../../assets/marcas/codex.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});
static DISCO_LINEAR: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../../assets/marcas/linear.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});

impl Marca {
    pub fn nome(self) -> &'static str {
        match self {
            Marca::Claude => "Claude",
            Marca::Codex => "Codex",
            Marca::Linear => "Linear",
        }
    }

    /// Cor do arco de uso enquanto há folga (e a do disco do Linear).
    pub fn cor(self) -> Hsla {
        match self {
            Marca::Claude => Cores::claude(),
            Marca::Codex => Cores::codex(),
            Marca::Linear => Cores::linear(),
        }
    }

    pub fn disco(self, lado: f32) -> DiscoDeMarca {
        DiscoDeMarca { marca: self, lado, apagada: false }
    }
}

/// O logo da marca num disco colorido.
#[derive(IntoElement)]
pub struct DiscoDeMarca {
    marca: Marca,
    lado: f32,
    apagada: bool,
}

impl DiscoDeMarca {
    /// Serviço ausente (o Codex sem instalar): cinza e esmaecido.
    pub fn apagada(mut self) -> Self {
        self.apagada = true;
        self
    }
}

impl RenderOnce for DiscoDeMarca {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let imagem = match self.marca {
            Marca::Claude => DISCO_CLAUDE.clone(),
            Marca::Codex => DISCO_CODEX.clone(),
            Marca::Linear => DISCO_LINEAR.clone(),
        };
        img(imagem)
            .flex_none()
            .size(r(self.lado))
            .when(self.apagada, |i| i.grayscale(true).opacity(0.45))
    }
}

/// Só no rótulo "Ultrathink": cada letra numa cor do arco-íris.
pub fn texto_arco_iris(texto: &str) -> StyledText {
    let letras: Vec<(usize, char)> = texto.char_indices().collect();
    let n = letras.len().max(2) - 1;
    let destaques = letras.iter().enumerate().map(|(i, (inicio, c))| {
        let posicao = i as f32 / n as f32 * (ARCO_IRIS.len() - 1) as f32;
        let (a, b) = (posicao.floor() as usize, posicao.ceil() as usize);
        let t = posicao - a as f32;
        let (ca, cb) = (Rgba::from(rgb(ARCO_IRIS[a])), Rgba::from(rgb(ARCO_IRIS[b])));
        let cor = Rgba {
            r: ca.r + (cb.r - ca.r) * t,
            g: ca.g + (cb.g - ca.g) * t,
            b: ca.b + (cb.b - ca.b) * t,
            a: 1.,
        };
        let estilo = HighlightStyle { color: Some(cor.into()), ..Default::default() };
        (*inicio..inicio + c.len_utf8(), estilo)
    });
    StyledText::new(SharedString::from(texto.to_string())).with_highlights(destaques)
}

/// Lados permitidos do quadrado do projeto.
pub const LADOS_DO_QUADRADO: [f32; 7] = [18., 20., 24., 30., 32., 40., 56.];

/// O lugar (projeto): quadrado arredondado na tinta da cor, com o ícone na cor.
#[derive(IntoElement)]
pub struct QuadradoDeProjeto {
    icone: &'static Icone,
    cor: u32,
    lado: f32,
    aro: f32,
    realcado: bool,
    sinal: Option<Sinal>,
    suave: Option<ElementId>,
}

impl QuadradoDeProjeto {
    pub fn new(icone: &'static Icone, cor: u32, lado: f32) -> Self {
        QuadradoDeProjeto { icone, cor, lado, aro: 0., realcado: false, sinal: None, suave: None }
    }

    /// O hover suave do trilho: a troca de `realcado` e de `ativo` anda em 120 ms. Quem desenha
    /// segue o ponteiro com um `movimento::Hover` no lugar de 40 e passa `.realcado(com_ponteiro)`.
    pub fn suave(mut self, id: impl Into<ElementId>) -> Self {
        self.suave = Some(id.into());
        self
    }

    /// Aro de 2 px na cor, a 2 px do quadrado.
    pub fn ativo(mut self, sim: bool) -> Self {
        self.aro = if sim { 1. } else { 0. };
        self
    }

    /// O mesmo aro, mais fraco (`0.4`: o projeto do chat aberto, sem a vista dele).
    pub fn aro(mut self, opacidade: f32) -> Self {
        self.aro = opacidade;
        self
    }

    /// Base mais forte (o hover vem do pai).
    pub fn realcado(mut self, sim: bool) -> Self {
        self.realcado = sim;
        self
    }

    /// Ponto de 8 no canto, metade para fora.
    pub fn sinal(mut self, sinal: Option<Sinal>) -> Self {
        self.sinal = sinal;
        self
    }
}

impl RenderOnce for QuadradoDeProjeto {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let cor = Hsla::from(rgb(self.cor));
        let raio = raio::do_icone(self.lado);
        let lado_icone = (self.lado * 18. / 32.).max(12.);
        let alvo = (if self.realcado { 0.26 } else { 0.18 }, self.aro);
        let (base, aro) = match &self.suave {
            Some(id) => movimento::valor(movimento::chave(id, "realce"), alvo, movimento::COR, Curva::Sair, window, cx),
            None => alvo,
        };
        div()
            .relative()
            .flex_none()
            .size(r(self.lado))
            .flex()
            .items_center()
            .justify_center()
            .rounded(raio)
            .bg(Cores::tinta_solida(cor, base))
            .child(icone_projeto::desenho(self.icone, lado_icone, cor).with_size(tam(window, lado_icone)))
            .when(aro > 0., |s| {
                s.child(
                    div()
                        .absolute()
                        .top(r(-4.))
                        .left(r(-4.))
                        .right(r(-4.))
                        .bottom(r(-4.))
                        .rounded(raio + px(4.))
                        .border_2()
                        .border_color(cor.opacity(aro)),
                )
            })
            .when_some(self.sinal, |s, sinal| {
                s.child(div().absolute().top(r(-4.)).right(r(-4.)).child(sinal.ponto(8.)))
            })
    }
}

/// O quadrado do projeto com o ícone e a cor escolhidos (ou o palpite).
pub fn quadrado_do_projeto(motor: &Motor, projeto: &str, lado: f32) -> QuadradoDeProjeto {
    let (icone, cor) = icone_projeto::visual_do(motor, projeto);
    QuadradoDeProjeto::new(icone, cor, lado)
}
