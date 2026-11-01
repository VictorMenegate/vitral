//! Anéis, barrinhas de sinal e a barra segmentada.

use std::f32::consts::TAU;

use gpui_kit::base::plot::shape::{Arc as Arco, ArcData};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::base::Marca;
use crate::tema::{ARCO_IRIS, Cores, esp, r, raio};
use crate::ui::movimento::{self, Curva};

/// Cor de um uso: a base até 69%, âmbar de 70% a 89%, vermelho a partir de 90%. Usa a
/// porcentagem inteira, a mesma que o texto mostra.
pub fn cor_da_faixa(fracao: f32, base: Hsla) -> Hsla {
    match (fracao * 100.).round() as i32 {
        ..=69 => base,
        70..=89 => Cores::ambar(),
        _ => Cores::vermelho(),
    }
}

/// Trilho inteiro e, por cima, o arco da fração (do topo, no sentido horário) com as pontas
/// redondas.
fn pintar_anel(
    bounds: Bounds<Pixels>,
    raio_fora: f32,
    espessura: f32,
    fracao: Option<f32>,
    cor: Hsla,
    window: &mut Window,
) {
    let arco = Arco::new().inner_radius(raio_fora - espessura).outer_radius(raio_fora);
    arco.paint(&ArcData::new(&(), 0, 1., 0., TAU), Cores::borda_forte(), &bounds, window);
    let Some(f) = fracao.map(|f| f.clamp(0., 1.)).filter(|f| *f > 0.) else {
        return;
    };
    arco.paint(&ArcData::new(&(), 1, f, 0., f * TAU), cor, &bounds, window);
    if f < 1. {
        let centro = bounds.center();
        let meio = raio_fora - espessura / 2.;
        for angulo in [0., f * TAU] {
            let ponto = point(
                centro.x + px(meio * angulo.sin()),
                centro.y - px(meio * angulo.cos()),
            );
            let lado = px(espessura);
            let caixa = Bounds::centered_at(ponto, size(lado, lado));
            window.paint_quad(fill(caixa, cor).corner_radii(lado / 2.));
        }
    }
}

/// Anel de progresso: contexto, uso da conta, progresso da Equipe. O arco desliza até a fração
/// nova em 240 ms (o `id` guarda de onde ele vem; precisa ser único entre os irmãos).
#[derive(IntoElement)]
pub struct Anel {
    id: ElementId,
    fracao: f32,
    cor: Option<Hsla>,
    diametro: f32,
    espessura: Option<f32>,
    centro: Option<AnyElement>,
    dica: Option<SharedString>,
    movimento: MovimentoDoAnel,
}

/// Como o arco chega à fração.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MovimentoDoAnel {
    /// Desliza quando a fração muda; na montagem já está nela.
    Desliza,
    /// Enche de 0 na montagem (o popover de uso que abre) e desliza depois.
    Enche,
    /// Sempre seco (o anel dentro de um `Simbolo`, que não tem id próprio).
    Seco,
}

impl Anel {
    pub fn new(id: impl Into<ElementId>, fracao: f32) -> Self {
        Anel {
            id: id.into(),
            fracao,
            cor: None,
            diametro: 20.,
            espessura: None,
            centro: None,
            dica: None,
            movimento: MovimentoDoAnel::Desliza,
        }
    }

    /// Enche de 0 até a fração ao aparecer (popover de uso aberto).
    pub fn encher(mut self) -> Self {
        self.movimento = MovimentoDoAnel::Enche;
        self
    }

    /// Sem transição.
    pub fn seco(mut self) -> Self {
        self.movimento = MovimentoDoAnel::Seco;
        self
    }

    /// Sem cor, segue a faixa sobre o destaque (até 70%, âmbar, vermelho).
    pub fn cor(mut self, cor: Hsla) -> Self {
        self.cor = Some(cor);
        self
    }

    /// 20, 22, 28 ou 36.
    pub fn diametro(mut self, d: f32) -> Self {
        self.diametro = d;
        self
    }

    pub fn espessura(mut self, e: f32) -> Self {
        self.espessura = Some(e);
        self
    }

    pub fn centro(mut self, e: impl IntoElement) -> Self {
        self.centro = Some(e.into_any_element());
        self
    }

    pub fn dica(mut self, d: impl Into<SharedString>) -> Self {
        self.dica = Some(d.into());
        self
    }
}

/// A fração deste quadro: a de agora ou a que desliza até ela.
fn fracao_animada(
    id: &ElementId,
    alvo: f32,
    movimento: MovimentoDoAnel,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    let chave = movimento::chave(id, "fracao");
    match movimento {
        MovimentoDoAnel::Seco => alvo,
        MovimentoDoAnel::Desliza => movimento::valor(chave, alvo, movimento::ENCHER, Curva::Mover, window, cx),
        MovimentoDoAnel::Enche => movimento::valor_desde(chave, 0., alvo, movimento::ENCHER, Curva::Mover, window, cx),
    }
}

impl RenderOnce for Anel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let diametro = self.diametro;
        let espessura = self.espessura.unwrap_or(if diametro <= 22. { 2.5 } else { 3. });
        let alvo = self.fracao.clamp(0., 1.);
        let cor = self.cor.unwrap_or_else(|| cor_da_faixa(alvo, Cores::destaque()));
        let fracao = fracao_animada(&self.id, alvo, self.movimento, window, cx);
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .size(r(diametro))
            .flex()
            .items_center()
            .justify_center()
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let escala = bounds.size.width.as_f32() / diametro;
                        let raio_fora = bounds.size.width.min(bounds.size.height).as_f32() / 2.;
                        pintar_anel(bounds, raio_fora, espessura * escala, Some(fracao), cor, window);
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .children(self.centro)
            .when_some(self.dica, |s, dica| {
                s.tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            })
    }
}

/// Uso de um serviço no trilho: o disco da marca com o anel de 5 h por fora e, no padrão
/// (o dono pediu as duas porcentagens), o da semana por dentro, mais fino.
#[derive(IntoElement)]
pub struct AnelDeUso {
    id: ElementId,
    marca: Marca,
    cinco_h: Option<f32>,
    semana: Option<f32>,
    duplo: bool,
    diametro: f32,
    bloqueado: bool,
    apagado: bool,
    encher: bool,
}

impl AnelDeUso {
    pub fn new(id: impl Into<ElementId>, marca: Marca, cinco_h: Option<f32>) -> Self {
        AnelDeUso {
            id: id.into(),
            marca,
            cinco_h,
            semana: None,
            duplo: true,
            diametro: 36.,
            bloqueado: false,
            apagado: false,
            encher: false,
        }
    }

    /// Os arcos enchem de 0 ao aparecer (popover de uso aberto); depois, deslizam.
    pub fn encher(mut self) -> Self {
        self.encher = true;
        self
    }

    pub fn semana(mut self, f: Option<f32>) -> Self {
        self.semana = f;
        self
    }

    /// Só o anel de 5 h (a semana fica na dica e no popover).
    pub fn unico(mut self) -> Self {
        self.duplo = false;
        self
    }

    /// 36 no trilho, 32 recolhido, 36 nas Contas, 20 no menu.
    pub fn diametro(mut self, d: f32) -> Self {
        self.diametro = d;
        self
    }

    /// Limite atingido: anel cheio em vermelho.
    pub fn bloqueado(mut self, sim: bool) -> Self {
        self.bloqueado = sim;
        self
    }

    /// Serviço ausente: disco cinza, sem arco.
    pub fn apagado(mut self, sim: bool) -> Self {
        self.apagado = sim;
        self
    }
}

impl RenderOnce for AnelDeUso {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let diametro = self.diametro;
        let base = self.marca.cor();
        let (fora, dentro) = if self.apagado {
            (None, None)
        } else if self.bloqueado {
            (Some(1.), self.semana)
        } else {
            (self.cinco_h, self.semana)
        };
        let cor_fora = if self.bloqueado { Cores::vermelho() } else { cor_da_faixa(fora.unwrap_or(0.), base) };
        let cor_dentro = cor_da_faixa(dentro.unwrap_or(0.), base);
        // Os dois arcos andam juntos; sem número, sem arco.
        let alvo = (fora.unwrap_or(0.).clamp(0., 1.), dentro.unwrap_or(0.).clamp(0., 1.));
        let chave = movimento::chave(&self.id, "arcos");
        let (f, d) = if self.encher {
            movimento::valor_desde(chave, (0., 0.), alvo, movimento::ENCHER, Curva::Mover, window, cx)
        } else {
            movimento::valor(chave, alvo, movimento::ENCHER, Curva::Mover, window, cx)
        };
        let (fora, dentro) = (fora.map(|_| f), dentro.map(|_| d));
        let duplo = self.duplo && diametro >= 28.;
        let disco = diametro - if duplo { 14. } else { 12. };
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .size(r(diametro))
            .flex()
            .items_center()
            .justify_center()
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let escala = bounds.size.width.as_f32() / diametro;
                        let raio_fora = bounds.size.width.min(bounds.size.height).as_f32() / 2.;
                        pintar_anel(bounds, raio_fora, 2.5 * escala, fora, cor_fora, window);
                        if duplo {
                            let raio_dentro = raio_fora - 3.5 * escala;
                            pintar_anel(bounds, raio_dentro, 1.5 * escala, dentro, cor_dentro, window);
                        }
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child({
                let disco = self.marca.disco(disco);
                if self.apagado { disco.apagada() } else { disco }
            })
    }
}

/// Barrinhas de intensidade (esforço): 5 barras de 3 com folga 2, alinhadas embaixo.
#[derive(IntoElement)]
pub struct BarrinhasDeSinal {
    nivel: u8,
    total: u8,
    cor: Hsla,
    altura: f32,
    sugerido: bool,
    arco_iris: bool,
}

impl BarrinhasDeSinal {
    pub fn new(nivel: u8, total: u8) -> Self {
        BarrinhasDeSinal { nivel, total, cor: Cores::ciano(), altura: 14., sugerido: false, arco_iris: false }
    }

    pub fn cor(mut self, cor: Hsla) -> Self {
        self.cor = cor;
        self
    }

    /// 14 no chip, 12 no cabeçalho do popover.
    pub fn altura(mut self, a: f32) -> Self {
        self.altura = a;
        self
    }

    /// "Padrão": o nível do modelo, com as ativas a 45%.
    pub fn sugerido(mut self) -> Self {
        self.sugerido = true;
        self
    }

    /// Todas acesas, cada uma numa cor do arco-íris (o Ultrathink armado).
    pub fn arco_iris(mut self) -> Self {
        self.arco_iris = true;
        self
    }
}

impl RenderOnce for BarrinhasDeSinal {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        const ALTURAS: [f32; 5] = [4., 6., 9., 11., 14.];
        let escala = self.altura / 14.;
        let total = self.total.max(1) as usize;
        let ativa = if self.sugerido { self.cor.opacity(0.45) } else { self.cor };
        div()
            .flex_none()
            .flex()
            .items_end()
            .gap(r(2. * escala))
            .h(r(self.altura))
            .children((0..total).map(|i| {
                let altura = ALTURAS[(i * (ALTURAS.len() - 1)) / (total - 1).max(1)];
                let cor = if self.arco_iris {
                    rgb(ARCO_IRIS[i * (ARCO_IRIS.len() - 1) / (total - 1).max(1)]).into()
                } else if i < self.nivel as usize {
                    ativa
                } else {
                    Cores::barra_apagada()
                };
                div().w(r(3. * escala)).h(r(altura * escala)).rounded_full().bg(cor)
            }))
    }
}

/// Proporção de estados numa barra fina (tarefas da Equipe), com dica por segmento.
#[derive(IntoElement)]
pub struct BarraSegmentada {
    partes: Vec<(usize, Hsla, SharedString)>,
    fina: bool,
}

impl BarraSegmentada {
    pub fn new(partes: Vec<(usize, Hsla, SharedString)>) -> Self {
        BarraSegmentada { partes, fina: false }
    }

    pub fn fina(mut self) -> Self {
        self.fina = true;
        self
    }
}

impl RenderOnce for BarraSegmentada {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let total: usize = self.partes.iter().map(|(n, _, _)| n).sum::<usize>().max(1);
        let id: Vec<&str> = self.partes.iter().map(|(_, _, dica)| dica.as_ref()).collect();
        div()
            .id(SharedString::from(format!("barra-{}", id.join("-"))))
            .w_full()
            .flex()
            .gap(esp::E2)
            .h(r(if self.fina { 4. } else { 6. }))
            .rounded(raio::PILULA)
            .bg(Cores::recuo())
            .children(self.partes.into_iter().enumerate().filter(|(_, (n, _, _))| *n > 0).map(
                |(i, (n, cor, dica))| {
                    div()
                        .id(("segmento", i))
                        .h_full()
                        .w(relative(n as f32 / total as f32))
                        .rounded(raio::PILULA)
                        .bg(cor)
                        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                },
            ))
    }
}

#[cfg(test)]
mod testes {
    use super::cor_da_faixa;
    use crate::tema::Cores;

    #[test]
    fn faixa_de_uso_nos_limites() {
        let base = Cores::claude();
        assert_eq!(cor_da_faixa(0.69, base), base);
        assert_eq!(cor_da_faixa(0.70, base), Cores::ambar());
        assert_eq!(cor_da_faixa(0.89, base), Cores::ambar());
        assert_eq!(cor_da_faixa(0.90, base), Cores::vermelho());
        assert_eq!(cor_da_faixa(0., base), base);
        assert_eq!(cor_da_faixa(1.2, base), Cores::vermelho());
    }
}
