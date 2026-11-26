//! Peças básicas: ícone em rem, o símbolo que vai à esquerda de chips, linhas e cartões, as
//! marcas, os papéis e o arco-íris.

use std::sync::{Arc, LazyLock};

use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::estado::{Sinal, icone_de_estado};
use super::graficos::{Anel, BarrinhasDeSinal};
use crate::tema::{ARCO_IRIS, Cores, Lucide, r, raio, tam};

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
    /// O glifo solto da marca, sem o disco (os modelos no seletor, como no protótipo).
    Glifo(Marca),
    Papel(Papel),
    Sinal { nivel: u8, total: u8 },
    Anel { fracao: f32, cor: Option<Hsla> },
    /// Imagem guardada (o logo do Vitral): se copia, ao contrário de um `Elemento`.
    Imagem(Arc<Image>),
    /// O estado de um chat, como o ícone dele na lista (`icone_de_estado`): se copia, para as
    /// listas de escolha que desenham a cada quadro.
    Estado(Option<Sinal>),
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
            Simbolo::Glifo(m) => Simbolo::Glifo(*m),
            Simbolo::Papel(p) => Simbolo::Papel(*p),
            Simbolo::Sinal { nivel, total } => Simbolo::Sinal { nivel: *nivel, total: *total },
            Simbolo::Anel { fracao, cor } => Simbolo::Anel { fracao: *fracao, cor: *cor },
            Simbolo::Imagem(imagem) => Simbolo::Imagem(imagem.clone()),
            Simbolo::Estado(sinal) => Simbolo::Estado(*sinal),
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
            Simbolo::Glifo(marca) => marca.glifo(lado),
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
            Simbolo::Estado(sinal) => icone_de_estado(sinal, lado).into_any_element(),
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

/// O símbolo de um chat nas listas: o balão num círculo neutro, o mesmo para todos.
pub fn simbolo_do_chat(lado: f32) -> CirculoDePapel {
    Papel::Avulso.circulo(lado)
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
            .rounded(raio::de(self.lado))
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
    Gemini,
    Linear,
    /// Os agentes ACP novos (`agente_acp`): sem logo no Vitral, um disco na cor do agente com a
    /// inicial (26/11/2026).
    Agente(crate::modelo::Agente),
}

static DISCO_CLAUDE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../../assets/marcas/claude.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});
static DISCO_CODEX: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../../assets/marcas/codex.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});
/// Os glifos soltos, sem o disco: a marca do Claude em coral e a do Codex no degradê azul, como
/// elas aparecem sobre fundo escuro (os chips da caixa e das Configurações).
static GLIFO_CLAUDE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../../assets/marcas/claude-glifo.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});
static GLIFO_CODEX: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../../assets/marcas/codex-glifo.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});
/// A estrela do Gemini no degradê dele (azul, roxo e rosa), no disco branco e solta.
static DISCO_GEMINI: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../../assets/marcas/gemini.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});
static GLIFO_GEMINI: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../../assets/marcas/gemini-glifo.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});
static DISCO_LINEAR: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../../assets/marcas/linear.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});

impl Marca {
    /// A marca de quem responde num chat.
    pub fn do_agente(agente: crate::modelo::Agente) -> Marca {
        match agente {
            crate::modelo::Agente::Claude => Marca::Claude,
            crate::modelo::Agente::Codex => Marca::Codex,
            crate::modelo::Agente::Gemini => Marca::Gemini,
            outro => Marca::Agente(outro),
        }
    }

    /// Um número estável por marca, para os ids dos elementos.
    pub fn indice(self) -> usize {
        match self {
            Marca::Claude => 0,
            Marca::Codex => 1,
            Marca::Gemini => 2,
            Marca::Linear => 3,
            Marca::Agente(a) => 10 + crate::modelo::Agente::TODOS.iter().position(|x| *x == a).unwrap_or(0),
        }
    }

    pub fn nome(self) -> &'static str {
        match self {
            Marca::Claude => "Claude",
            Marca::Codex => "Codex",
            Marca::Gemini => "Gemini",
            Marca::Linear => "Linear",
            Marca::Agente(a) => a.nome(),
        }
    }

    /// Cor do arco de uso enquanto há folga (e a do disco do Linear).
    pub fn cor(self) -> Hsla {
        match self {
            Marca::Claude => Cores::claude(),
            Marca::Codex => Cores::codex(),
            Marca::Gemini => Cores::gemini(),
            Marca::Linear => Cores::linear(),
            Marca::Agente(a) => cor_do_agente(a),
        }
    }

    pub fn disco(self, lado: f32) -> DiscoDeMarca {
        DiscoDeMarca { marca: self, lado, apagada: false }
    }

    /// O glifo solto, sem o disco cheio, no lado `lado` (o Linear, que não tem glifo próprio,
    /// fica no disco).
    pub fn glifo(self, lado: f32) -> AnyElement {
        let imagem = match self {
            Marca::Claude => GLIFO_CLAUDE.clone(),
            Marca::Codex => GLIFO_CODEX.clone(),
            Marca::Gemini => GLIFO_GEMINI.clone(),
            Marca::Linear => return self.disco(lado).into_any_element(),
            Marca::Agente(a) => return monograma(a, lado, false),
        };
        img(imagem).flex_none().size(r(lado)).into_any_element()
    }
}

/// A cor de um agente ACP novo, a do perfil dele.
fn cor_do_agente(agente: crate::modelo::Agente) -> Hsla {
    crate::agente_acp::perfil(agente).map_or_else(Cores::tenue, |p| rgb(p.cor).into())
}

/// O disco de um agente sem logo: a cor dele e a inicial, em preto nas cores claras.
fn monograma(agente: crate::modelo::Agente, lado: f32, apagada: bool) -> AnyElement {
    let cor = cor_do_agente(agente);
    let clara = cor.l > 0.6;
    div()
        .flex_none()
        .size(r(lado))
        .rounded_full()
        .bg(cor)
        .flex()
        .items_center()
        .justify_center()
        .text_size(r((lado * 0.56).max(7.)))
        .line_height(r(lado))
        .font_weight(FontWeight::BOLD)
        .text_color(if clara { rgb(0x111111) } else { rgb(0xffffff) })
        .when(apagada, |d| d.opacity(0.45))
        .child(agente.nome().chars().next().unwrap_or('?').to_string())
        .into_any_element()
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
        if let Marca::Agente(agente) = self.marca {
            return monograma(agente, self.lado, self.apagada);
        }
        let imagem = match self.marca {
            Marca::Claude => DISCO_CLAUDE.clone(),
            Marca::Codex => DISCO_CODEX.clone(),
            Marca::Gemini => DISCO_GEMINI.clone(),
            Marca::Linear | Marca::Agente(_) => DISCO_LINEAR.clone(),
        };
        img(imagem)
            .flex_none()
            .size(r(self.lado))
            .when(self.apagada, |i| i.grayscale(true).opacity(0.45))
            .into_any_element()
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
