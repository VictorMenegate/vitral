use std::collections::{HashMap, hash_map::Entry};

use crate::{Icon, IconName, Sizable, Size};
use gpui::{
    App, EntityId, Global, Hsla, IntoElement, ParentElement, RenderOnce, Styled as _,
    Transformation, Window, WindowId, div, ease_in_out, percentage, prelude::FluentBuilder as _,
};
use instant::{Duration, Instant};

/// Vitral: batidas por segundo do compasso que move spinners e brilhos (e, de carona, os laços
/// do app: o metal e o anel girando).
pub const BATIDAS_POR_SEGUNDO: u32 = 12;

/// Vitral: um compasso só para todos os spinners (e o brilho do texto) da janela. Com uma
/// animação limitada por spinner, cada um agendava o próprio redesenho, e spinners que surgiam em
/// momentos diferentes somavam 12 redesenhos por segundo cada.
struct Compasso {
    inicio: Instant,
    /// Janelas com batida agendada e as views a redesenhar nela.
    pendentes: HashMap<WindowId, Vec<EntityId>>,
    /// Vitral: quando algum spinner ou brilho pediu o compasso pela última vez.
    ultimo_pedido: Option<Instant>,
    /// Vitral: a fase da última batida, que fica parada com a janela sem foco.
    ultima_fase: Duration,
    /// Vitral: o compasso parou numa janela sem foco; na volta, segue da fase parada.
    congelado: bool,
}

impl Global for Compasso {}

/// Vitral: inscreve a view que está desenhando na próxima batida da janela (agendando a batida se
/// ainda não houver) e devolve o tempo do compasso, para todos ficarem na mesma fase. Com a janela
/// sem foco, não agenda nada e devolve a fase parada: quem está ao lado do editor não redesenha
/// 12 vezes por segundo o turno inteiro.
pub fn compasso(window: &mut Window, cx: &mut App) -> Duration {
    let agora = cx.background_executor().now();
    if !cx.has_global::<Compasso>() {
        cx.set_global(Compasso {
            inicio: agora,
            pendentes: HashMap::new(),
            ultimo_pedido: None,
            ultima_fase: Duration::ZERO,
            congelado: false,
        });
    }
    let janela = window.window_handle().window_id();
    let view = window.current_view();
    let ativa = window.is_window_active();
    let estado = cx.global_mut::<Compasso>();
    if !ativa {
        estado.congelado = true;
        return estado.ultima_fase;
    }
    if estado.congelado {
        estado.congelado = false;
        estado.inicio = agora.checked_sub(estado.ultima_fase).unwrap_or(agora);
    }
    estado.ultimo_pedido = Some(agora);
    let decorrido = agora.saturating_duration_since(estado.inicio);
    estado.ultima_fase = decorrido;
    let agendar = match estado.pendentes.entry(janela) {
        Entry::Occupied(mut views) => {
            if !views.get().contains(&view) {
                views.get_mut().push(view);
            }
            false
        }
        Entry::Vacant(vaga) => {
            vaga.insert(vec![view]);
            true
        }
    };
    if agendar {
        let intervalo = Duration::from_secs(1) / BATIDAS_POR_SEGUNDO;
        // Na grade do compasso: janelas diferentes batem juntas.
        let resto = decorrido.as_nanos() % intervalo.as_nanos();
        let espera = intervalo - Duration::from_nanos(resto as u64);
        cx.spawn(async move |cx| {
            cx.background_executor().timer(espera).await;
            cx.update(|cx| {
                let views = cx
                    .global_mut::<Compasso>()
                    .pendentes
                    .remove(&janela)
                    .unwrap_or_default();
                for view in views {
                    cx.notify(view);
                }
            });
        })
        .detach();
    }
    decorrido
}

/// Vitral: o tempo do compasso, só enquanto ele bate (um spinner ou brilho pediu a batida nos
/// últimos 150 ms). Não agenda nada: é para o que anda de carona, como o ponto de "rodando" que
/// respira. Sem compasso (nada girando, ou movimento reduzido), `None`.
pub fn compasso_recente(cx: &App) -> Option<Duration> {
    let estado = cx.try_global::<Compasso>()?;
    let agora = cx.background_executor().now();
    let recente = agora.saturating_duration_since(estado.ultimo_pedido?) <= Duration::from_millis(150);
    (recente && !cx.reduce_motion()).then(|| agora.saturating_duration_since(estado.inicio))
}

/// A cycling loading spinner.
#[derive(IntoElement)]
pub struct Spinner {
    size: Size,
    icon: Icon,
    speed: Duration,
    easing: Box<dyn Fn(f32) -> f32>,
    color: Option<Hsla>,
}

impl Spinner {
    /// Create a new loading spinner.
    pub fn new() -> Self {
        Self {
            size: Size::Medium,
            speed: Duration::from_secs_f64(0.8),
            easing: Box::new(ease_in_out),
            icon: Icon::new(IconName::Loader),
            color: None,
        }
    }

    /// Set specified icon for the spinner.
    ///
    /// Default is [`IconName::Loader`].
    ///
    /// Please ensure the icon used is suitable for a loading spinner.
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = icon.into();
        self
    }

    /// Set the icon color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Set the easing function.
    pub fn ease(mut self, easing: impl Fn(f32) -> f32 + 'static) -> Self {
        self.easing = Box::new(easing);
        self
    }
}

impl Sizable for Spinner {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Vitral: gira no compasso da janela, a 12 quadros por segundo para todos os spinners
        // juntos. Com movimento reduzido fica parado e não agenda nada.
        let fase = if cx.reduce_motion() {
            0.
        } else {
            let volta = self.speed.as_nanos().max(1);
            (compasso(window, cx).as_nanos() % volta) as f32 / volta as f32
        };
        div()
            .child(
                self.icon
                    .with_size(self.size)
                    .when_some(self.color, |this, color| this.text_color(color))
                    .transform(Transformation::rotate(percentage((self.easing)(fase)))),
            )
            .into_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Render, TestAppContext, px, size};
    use std::{cell::Cell, rc::Rc};

    struct SpinnerHost;

    impl Render for SpinnerHost {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            Spinner::new()
        }
    }

    struct VariosSpinners {
        quantos: usize,
        desenhos: Rc<Cell<usize>>,
    }

    impl Render for VariosSpinners {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            self.desenhos.set(self.desenhos.get() + 1);
            div().children((0..self.quantos).map(|_| Spinner::new()))
        }
    }

    #[gpui::test]
    fn spinners_que_surgem_em_momentos_diferentes_dividem_o_compasso(cx: &mut TestAppContext) {
        let desenhos = Rc::new(Cell::new(0));
        let window = cx.open_window(size(px(100.), px(100.)), {
            let desenhos = desenhos.clone();
            move |_, _| VariosSpinners {
                quantos: 1,
                desenhos,
            }
        });
        cx.run_until_parked();
        // Mais três spinners, cada um surgindo fora da batida dos anteriores.
        for _ in 0..3 {
            cx.executor().advance_clock(Duration::from_millis(30));
            cx.run_until_parked();
            window
                .update(cx, |view, _, cx| {
                    view.quantos += 1;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
        }
        desenhos.set(0);
        for _ in 0..100 {
            cx.executor().advance_clock(Duration::from_millis(10));
            cx.run_until_parked();
        }
        // Um segundo: 12 batidas para os quatro juntos, não 12 para cada.
        let n = desenhos.get();
        assert!((11..=13).contains(&n), "{n} desenhos em um segundo");
    }

    #[gpui::test]
    fn reduced_motion_spinner_is_static_and_requests_no_frame(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.open_window(size(px(100.), px(100.)), |_, _| SpinnerHost);
        cx.run_until_parked();

        assert_eq!(
            window
                .update(cx, |_, window, cx| window.simulate_next_frame(cx))
                .unwrap(),
            0
        );
    }
}
