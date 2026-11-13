//! A única porta de animação do Vitral. Todo efeito responde a uma ação (abrir, focar, clicar,
//! o ponteiro entrar num controle solto, chegar dado novo), dura de 120 a 240 ms e redesenha no
//! máximo 60 vezes por segundo: cada quadro de animação redesenha a janela inteira. Nada anima em
//! repouso, e com o movimento reduzido (o GNOME sem animações ou `VITRAL_SEM_ANIMACAO=1`) tudo
//! aparece direto no estado final.
//!
//! A exceção é o `laco_a` (12/11/2026, o dono pediu "mais shader" e depois "tenta focar em vez de
//! glass esse metal líquido"): o metal (a chapa atrás da janela, as aberturas e a malha do botão do
//! menu) anda sem parar, devagar, e só com a janela em primeiro plano e o movimento ligado. Quem
//! pede diz o ritmo: `QPS_DO_LACO` (20 por segundo) com um chat da tela trabalhando,
//! `QPS_EM_REPOUSO` no resto do tempo (hoje 0: em repouso o metal fica parado e nada redesenha;
//! o porquê está na constante). O ponteiro mexendo na janela não acorda o metal. Continua sendo a
//! única outra porta.
//!
//! Duas formas:
//! - `entrar` (e `animacao`, para quem precisa do `with_animation`): toca uma vez quando o
//!   elemento aparece e não repete enquanto ele fica na tela. `entrada` só toca para o que é
//!   novo de verdade (a mensagem que chegou, não a conversa que abriu).
//! - `valor`, `valor_desde` e `disparo`: o elemento fica igual e só o valor anda até o alvo
//!   quando ele muda (a pílula do segmentado, a cor do hover, o anel). Na montagem não anima.
//!   A `abertura` da gaveta é um `valor_desde` de 0 a 1, com o id levando a geração.
//!
//! A outra exceção é de duração: o `Segurar` (no fim do arquivo), o "Segure para permitir" do
//! pedido perigoso, leva 900 ms apertado, com ou sem movimento reduzido.

use std::{
    cell::Cell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui_kit::base::animation::{cubic_bezier, ease_out_cubic};
use gpui_kit::component::spinner;
use gpui_kit::*;

use crate::tema::r;

/// Teto de redesenhos por segundo de toda animação do Vitral.
pub const QPS: f32 = 60.;
/// Hover, troca de cor.
pub const COR: Duration = Duration::from_millis(120);
/// Popover, menu, mensagem nova.
pub const ENTRAR: Duration = Duration::from_millis(160);
/// Pílula do segmentado, halo do foco.
pub const MOVER: Duration = Duration::from_millis(180);
/// Anéis, pulso do enviar.
pub const ENCHER: Duration = Duration::from_millis(240);

/// Curvas sem ricochete: nenhuma passa do alvo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Curva {
    /// cubic-bezier(0.16, 1, 0.3, 1): sai rápido e pousa macio (o `easing_enter` do kit).
    Entrar,
    /// cubic-bezier(0.2, 0, 0, 1) (o `easing_move` do kit).
    Mover,
    /// ease-out cúbica.
    Sair,
}

impl Curva {
    pub fn aplicar(self, t: f32) -> f32 {
        let t = t.clamp(0., 1.);
        match self {
            Curva::Entrar => cubic_bezier(0.16, 1., 0.3, 1.)(t),
            Curva::Mover => cubic_bezier(0.2, 0., 0., 1.)(t),
            Curva::Sair => ease_out_cubic(t),
        }
    }
}

/// A animação com o teto de quadros, para o `with_animation`. Como no `valor`, a última batida
/// já pousa no fim: a conta vai até um quadro antes de `d`, o que poupa um redesenho.
pub fn animacao(d: Duration, c: Curva) -> Animation {
    let quadro = Duration::from_secs_f32(1. / QPS);
    Animation::new(d.saturating_sub(quadro).max(quadro)).with_easing(move |t| c.aplicar(t)).with_max_fps(QPS)
}

/// De onde vem o que entra.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum De {
    /// 8 px de baixo (mensagem nova, faixa acima da caixa).
    Baixo,
    /// 6 px de cima (o que abre embaixo de um gatilho).
    Cima,
    /// Só o fade (o painel aberto ao lado).
    Parado,
    /// 24 px da borda esquerda, no `left` (a gaveta da lista, que é absoluta).
    Esquerda,
    /// 24 px da borda direita, no `right` (a gaveta da Equipe).
    Direita,
}

impl De {
    fn deslize(self) -> f32 {
        match self {
            De::Baixo => 8.,
            De::Cima => -6.,
            De::Parado => 0.,
            De::Esquerda | De::Direita => -24.,
        }
    }

    /// O elemento em `t` (0 a 1): a opacidade e o deslize que ainda falta.
    fn aplicar<E: Styled>(self, el: E, t: f32) -> E {
        if t >= 1. {
            return el;
        }
        let falta = r(self.deslize() * (1. - t));
        let el = el.opacity(t);
        match self {
            De::Parado => el,
            De::Baixo | De::Cima => el.top(falta),
            De::Esquerda => el.left(falta),
            De::Direita => el.right(falta),
        }
    }
}

/// Entrada de um elemento no fluxo: opacidade de 0 a 1 e o deslize até o lugar em 160 ms, sem
/// mexer no layout (o `top` de um elemento relativo só o desloca). Toca uma vez por montagem: o
/// `id` precisa ser estável enquanto o elemento fica na tela. Um elemento absoluto vai dentro de
/// um `div` relativo, senão o `top` briga com a posição dele.
pub fn entrar<E: IntoElement + Styled + 'static>(el: E, id: impl Into<ElementId>, de: De) -> AnimationElement<E> {
    el.with_animation(id, animacao(ENTRAR, Curva::Entrar), move |el, t| de.aplicar(el, t))
}

/// A abertura de uma gaveta por cima da conversa: o progresso de 0 a 1 em 180 ms, uma vez por
/// `id` (que leva a geração da abertura, para tocar de novo só na próxima). A gaveta entra pelo
/// `deslizar_gaveta`, e o véu atrás vai junto, pelo mesmo progresso. Fechar é seco; com o
/// movimento reduzido, já sai em 1.
pub fn abertura(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> f32 {
    valor_desde(id, 0f32, 1f32, MOVER, Curva::Entrar, window, cx)
}

/// A gaveta (absoluta, na borda) em `t` da `abertura`: 24 px de fora e de 0 a 1 de opacidade.
pub fn deslizar_gaveta<E: Styled>(el: E, de: De, t: f32) -> E {
    de.aplicar(el, t)
}

/// Algo que precisa da janela e do `App` mutáveis para desenhar (o estado guardado do
/// `valor`), montado por quem só tem o `&Context` da Janela.
#[derive(IntoElement)]
pub struct ComEstado(Box<dyn FnOnce(&mut Window, &mut App) -> AnyElement>);

pub fn com_estado(f: impl FnOnce(&mut Window, &mut App) -> AnyElement + 'static) -> ComEstado {
    ComEstado(Box::new(f))
}

impl RenderOnce for ComEstado {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        (self.0)(window, cx)
    }
}

/// A entrada de algo que acabou de aparecer (a mensagem nova, a faixa do turno): como o `entrar`,
/// mas só quando `nova`, que vale só no primeiro quadro em que ele aparece; o estado fica guardado
/// pelo `id` enquanto ele está na tela. Remontado (outro painel abriu, a lista recolheu), ele
/// volta sem `nova` e aparece pronto, sem nenhum redesenho a mais.
pub fn entrada<E: IntoElement + Styled + 'static>(id: impl Into<ElementId>, nova: bool, de: De, el: E) -> ComEstado {
    let id = id.into();
    com_estado(move |window, cx| {
        let inicial = if nova { 0. } else { 1. };
        let t = valor_desde(id, inicial, 1f32, ENTRAR, Curva::Entrar, window, cx);
        de.aplicar(el, t).into_any_element()
    })
}

/// A chave de um estado guardado de um componente (`movimento::valor`, `Hover`): o `id` dele e
/// um nome, para não colidir com os irmãos nem com os outros estados dele.
pub fn chave(id: &ElementId, nome: &'static str) -> ElementId {
    ElementId::NamedChild(std::sync::Arc::new(id.clone()), nome.into())
}

/// O que `valor` sabe interpolar.
pub trait Interpolavel: Clone + PartialEq + 'static {
    fn interpolar(&self, alvo: &Self, t: f32) -> Self;
}

impl Interpolavel for f32 {
    fn interpolar(&self, alvo: &Self, t: f32) -> Self {
        self + (alvo - self) * t
    }
}

/// Cores andam com o alfa pré-multiplicado: do transparente ao claro sem passar pelo cinza.
impl Interpolavel for Hsla {
    fn interpolar(&self, alvo: &Self, t: f32) -> Self {
        let (a, b) = (Rgba::from(*self), Rgba::from(*alvo));
        let alfa = a.a + (b.a - a.a) * t;
        if alfa <= f32::EPSILON {
            return Hsla::from(Rgba { a: 0., ..b });
        }
        let canal = |x: f32, y: f32| (x * a.a + (y * b.a - x * a.a) * t) / alfa;
        Hsla::from(Rgba { r: canal(a.r, b.r), g: canal(a.g, b.g), b: canal(a.b, b.b), a: alfa })
    }
}

impl<A: Interpolavel, B: Interpolavel> Interpolavel for (A, B) {
    fn interpolar(&self, alvo: &Self, t: f32) -> Self {
        (self.0.interpolar(&alvo.0, t), self.1.interpolar(&alvo.1, t))
    }
}

/// Duas mudanças de alvo mais próximas que isto são dois donos com o mesmo id no mesmo quadro.
const MESMO_QUADRO: Duration = Duration::from_millis(2);

/// A fração de `d` que um quadro do relógio ocupa. Quando falta menos que isso, o valor já pousa
/// no alvo neste quadro: o seguinte passaria do fim de qualquer jeito, e na cauda da curva a
/// diferença não se vê. Poupa um redesenho por efeito.
fn um_quadro(d: Duration) -> f32 {
    1. / (QPS * d.as_secs_f32()).max(1.)
}

/// Um valor indo de `de` a `para`. A conta não depende da janela, para os testes.
struct Transicao<T> {
    de: T,
    para: T,
    inicio: Option<Instant>,
    duracao: Duration,
    curva: Curva,
    mudou_em: Option<Instant>,
    animando: Rc<Cell<bool>>,
}

impl<T: Interpolavel> Transicao<T> {
    fn nova(inicial: T, alvo: T, d: Duration, c: Curva, agora: Instant, reduzido: bool) -> Self {
        let anima = inicial != alvo && !reduzido;
        Transicao {
            de: if anima { inicial } else { alvo.clone() },
            para: alvo,
            inicio: anima.then_some(agora),
            duracao: d,
            curva: c,
            mudou_em: None,
            animando: Rc::new(Cell::new(false)),
        }
    }

    fn progresso(&self, agora: Instant) -> f32 {
        match self.inicio {
            Some(inicio) if !self.duracao.is_zero() => {
                agora.saturating_duration_since(inicio).as_secs_f32() / self.duracao.as_secs_f32()
            }
            _ => 1.,
        }
    }

    fn atual(&self, agora: Instant) -> T {
        let t = self.progresso(agora);
        if t >= 1. { self.para.clone() } else { self.de.interpolar(&self.para, self.curva.aplicar(t)) }
    }

    /// O valor deste quadro e se ainda anda. Um alvo novo parte de onde o valor está (uma
    /// mudança no meio do caminho não salta).
    fn avancar(&mut self, alvo: T, d: Duration, c: Curva, agora: Instant, reduzido: bool) -> (T, bool) {
        if self.para != alvo {
            let repetido = self.mudou_em.is_some_and(|m| agora.saturating_duration_since(m) < MESMO_QUADRO);
            let seco = reduzido || repetido;
            self.de = if seco { alvo.clone() } else { self.atual(agora) };
            self.para = alvo;
            self.inicio = (!seco).then_some(agora);
            self.duracao = d;
            self.curva = c;
            self.mudou_em = Some(agora);
        }
        if reduzido || self.progresso(agora) + um_quadro(self.duracao) >= 1. {
            self.inicio = None;
            self.de = self.para.clone();
            self.animando.set(false);
            return (self.para.clone(), false);
        }
        self.animando.set(true);
        (self.atual(agora), true)
    }
}

/// O valor que anda até `alvo` quando ele muda: em `d`, na curva `c`. Devolve o valor deste
/// quadro e pede o próximo (no relógio de 60 qps) só enquanto anda. Na montagem já sai no alvo.
/// `d` e `c` valem para a mudança que começa neste quadro (o halo entra em 180 e sai em 120).
/// O `id` é um estado guardado: precisa ser único entre os irmãos.
pub fn valor<T: Interpolavel>(
    id: impl Into<ElementId>,
    alvo: T,
    d: Duration,
    c: Curva,
    window: &mut Window,
    cx: &mut App,
) -> T {
    transicao(id.into(), None, alvo, d, c, window, cx)
}

/// Como `valor`, mas na montagem parte de `inicial` (o anel do popover de uso enche de 0).
pub fn valor_desde<T: Interpolavel>(
    id: impl Into<ElementId>,
    inicial: T,
    alvo: T,
    d: Duration,
    c: Curva,
    window: &mut Window,
    cx: &mut App,
) -> T {
    transicao(id.into(), Some(inicial), alvo, d, c, window, cx)
}

fn transicao<T: Interpolavel>(
    id: ElementId,
    inicial: Option<T>,
    alvo: T,
    d: Duration,
    c: Curva,
    window: &mut Window,
    cx: &mut App,
) -> T {
    let agora = cx.background_executor().now();
    let reduzido = cx.reduce_motion();
    let estado = window.use_keyed_state(id, cx, {
        let alvo = alvo.clone();
        move |_, _| Transicao::nova(inicial.unwrap_or_else(|| alvo.clone()), alvo, d, c, agora, reduzido)
    });
    let (valor, anda, sinal) = estado.update(cx, |t, _| {
        let (valor, anda) = t.avancar(alvo, d, c, agora, reduzido);
        (valor, anda, t.animando.clone())
    });
    if anda {
        pedir_quadro(sinal, window, cx);
    }
    valor
}

/// Um disparo que toca uma vez quando `chave` muda (o pulso do enviar sobe a geração a cada
/// envio; o realce da pílula usa o estado). Na montagem não toca. Devolve o progresso, já na
/// curva, enquanto toca.
pub fn disparo(
    id: impl Into<ElementId>,
    chave: u64,
    d: Duration,
    c: Curva,
    window: &mut Window,
    cx: &mut App,
) -> Option<f32> {
    let agora = cx.background_executor().now();
    let reduzido = cx.reduce_motion();
    let estado = window.use_keyed_state(id.into(), cx, move |_, _| Disparo {
        chave,
        inicio: None,
        animando: Rc::new(Cell::new(false)),
    });
    let (progresso, sinal) = estado.update(cx, |e, _| (e.avancar(chave, d, agora, reduzido), e.animando.clone()));
    let progresso = progresso?;
    pedir_quadro(sinal, window, cx);
    Some(c.aplicar(progresso))
}

struct Disparo {
    chave: u64,
    inicio: Option<Instant>,
    animando: Rc<Cell<bool>>,
}

impl Disparo {
    /// O progresso (0 a 1) enquanto toca.
    fn avancar(&mut self, chave: u64, d: Duration, agora: Instant, reduzido: bool) -> Option<f32> {
        if self.chave != chave {
            self.chave = chave;
            self.inicio = (!reduzido).then_some(agora);
        }
        let progresso = self.inicio.map(|i| agora.saturating_duration_since(i).as_secs_f32() / d.as_secs_f32().max(1e-6));
        match progresso {
            Some(t) if t + um_quadro(d) < 1. && !reduzido => {
                self.animando.set(true);
                Some(t)
            }
            _ => {
                self.inicio = None;
                self.animando.set(false);
                None
            }
        }
    }
}

/// O ponteiro em cima de um controle solto, guardado entre quadros, para o hover suave. O
/// elemento põe `.on_hover(hover.ouvinte())` e lê `hover.ativo(cx)` para escolher o alvo de um
/// `valor`. Nunca em linha de lista, item de menu ou cartão: passar o ponteiro por uma lista
/// viraria animação contínua.
#[derive(Clone)]
pub struct Hover(Entity<bool>);

impl Hover {
    pub fn de(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> Self {
        Hover(window.use_keyed_state(id.into(), cx, |_, _| false))
    }

    pub fn ativo(&self, cx: &App) -> bool {
        *self.0.read(cx)
    }

    pub fn ouvinte(&self) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
        let estado = self.0.clone();
        move |dentro, _, cx| {
            estado.update(cx, |e, cx| {
                if *e != *dentro {
                    *e = *dentro;
                    cx.notify();
                }
            })
        }
    }
}

/// Um relógio por janela, como o compasso dos spinners: tudo o que anda pelo `valor` e pelo
/// `disparo` redesenha junto, no máximo `QPS` vezes por segundo. Na batida só se redesenha quem
/// ainda anda.
#[derive(Default)]
struct Relogio {
    pendentes: HashMap<WindowId, Vec<(EntityId, Rc<Cell<bool>>)>>,
}

impl Global for Relogio {}

fn pedir_quadro(animando: Rc<Cell<bool>>, window: &mut Window, cx: &mut App) {
    let janela = window.window_handle().window_id();
    let view = window.current_view();
    let lista = cx.default_global::<Relogio>().pendentes.entry(janela).or_default();
    let agendar = lista.is_empty();
    lista.push((view, animando));
    if !agendar {
        return;
    }
    let intervalo = Duration::from_secs_f32(1. / QPS);
    cx.spawn(async move |cx| {
        cx.background_executor().timer(intervalo).await;
        cx.update(|cx| {
            let lista = cx.default_global::<Relogio>().pendentes.remove(&janela).unwrap_or_default();
            let mut views: Vec<EntityId> = lista.into_iter().filter(|(_, a)| a.get()).map(|(v, _)| v).collect();
            views.sort();
            views.dedup();
            for view in views {
                cx.notify(view);
            }
        });
    })
    .detach();
}

/// Quantas vezes por segundo um laço vivo pede para redesenhar a janela (um chat da tela
/// trabalhando, o ponteiro em cima do botão do menu). Está acima do compasso dos spinners, então
/// na prática o laço bate junto com ele, a 12 por segundo (ver `laco_a`).
pub const QPS_DO_LACO: f32 = 20.;
/// O ritmo do metal em repouso (ninguém trabalhando na tela). Uma constante só, para o fundo da
/// janela e para o botão do menu. Em `0.` o metal fica PARADO em repouso e nada redesenha; ele
/// anda só com um chat da tela trabalhando ou com o ponteiro em cima do botão do menu. O ponteiro
/// mexendo na janela já acordou o metal, e saiu: redesenhava a janela inteira 20 vezes por
/// segundo a cada movimento, e a rolagem engasgava (dono, 12/11/2026).
///
/// É a política B do portão de custo, medida em 12/11/2026 no binário de debug, na bancada, com
/// um chat parado aberto e o ponteiro parado na janela, 30 s: a 8 por segundo o app gastava 30%
/// de um núcleo (o limite era 8%). O custo não é o metal (o campo leva de 2 a 4 ms; com ele
/// congelado, 28%): cada batida redesenha a Janela inteira, e isso custa uns 35 ms de
/// processador no debug. Parado: 0%. Para voltar a animar em repouso (por exemplo se o release
/// medir bem menos), é só pôr 8 aqui.
pub const QPS_EM_REPOUSO: f32 = 0.;

/// Mais que isto sem ninguém pedir a fase é uma pausa (a janela ficou sem foco, o metal dormiu em
/// repouso): na volta a fase continua de onde estava, sem salto.
const PAUSA_DO_LACO: Duration = Duration::from_millis(300);

/// O relógio dos laços: a fase (uma só para o app, em `f64`: em `f32` ela anda em degraus depois
/// de um dia) só anda enquanto alguém a pede com a janela em primeiro plano. `proxima` guarda, por
/// janela, a hora da batida marcada; `bateu_em`, a da última que valeu.
#[derive(Default)]
struct Lacos {
    fase: f64,
    ultimo: Option<Instant>,
    proxima: HashMap<WindowId, Instant>,
    bateu_em: HashMap<WindowId, Instant>,
}

impl Global for Lacos {}

impl Lacos {
    /// Anda a fase até `agora` e a devolve. O que passou desde a última leitura entra inteiro, a
    /// não ser depois de uma pausa: aí a fase continua de onde estava.
    fn andar(&mut self, agora: Instant) -> f64 {
        let passo = self.ultimo.map_or(Duration::ZERO, |u| agora.saturating_duration_since(u));
        if passo <= PAUSA_DO_LACO {
            self.fase += passo.as_secs_f64();
        }
        self.ultimo = Some(agora);
        self.fase
    }

    /// A hora da próxima batida de `janela` no ritmo de `intervalo`: no compasso da última (o
    /// tempo do desenho não se soma ao intervalo, senão 20 por segundo viravam 13), mas sempre com
    /// meia batida de folga depois de agora, para um desenho pesado não emendar no seguinte.
    fn quando(&self, janela: WindowId, agora: Instant, intervalo: Duration) -> Instant {
        match self.bateu_em.get(&janela) {
            Some(bateu) if agora.saturating_duration_since(*bateu) < intervalo => {
                (*bateu + intervalo).max(agora + intervalo / 2)
            }
            _ => agora + intervalo,
        }
    }

    /// Marca a batida de `janela` para `quando`, se não houver uma marcada para antes (ou quase
    /// junto): diz se é preciso armar o timer. Um pedido mais rápido que o já marcado marca outra
    /// batida, mais cedo, e a velha, ao chegar, não é mais a marcada.
    fn marcar(&mut self, janela: WindowId, quando: Instant) -> bool {
        let cedo = self.proxima.get(&janela).is_none_or(|marcada| quando + MESMO_QUADRO < *marcada);
        if cedo {
            self.proxima.insert(janela, quando);
        }
        cedo
    }

    /// A batida de `quando` chegou: vale só se ainda é a marcada.
    fn bateu(&mut self, janela: WindowId, quando: Instant) -> bool {
        let vale = self.proxima.get(&janela) == Some(&quando);
        if vale {
            self.proxima.remove(&janela);
            self.bateu_em.insert(janela, quando);
        }
        vale
    }
}

/// Movimento ligado e janela em primeiro plano: só assim um laço anda.
pub fn anima(window: &Window, cx: &App) -> bool {
    !cx.reduce_motion() && window.is_window_active()
}

/// Há uma batida de laço marcada para esta janela.
pub fn laco_agendado(window: &Window, cx: &App) -> bool {
    let janela = window.window_handle().window_id();
    cx.try_global::<Lacos>().is_some_and(|l| l.proxima.contains_key(&janela))
}

/// A fase (em segundos) de um efeito que anda sem parar enquanto está na tela, e o pedido do
/// próximo quadro a `qps` por segundo. Quem desenha chama a cada quadro: com o movimento ligado e
/// a janela em primeiro plano, a fase avança e o próximo quadro é pedido; senão ela fica onde
/// parou e nada é pedido (o efeito congela, sem salto). Com `qps <= 0.` devolve a fase sem andar
/// nem pedir nada (o repouso da política B). Sem ninguém chamando, nada redesenha.
pub fn laco_a(window: &mut Window, cx: &mut App, qps: f32) -> f64 {
    let anima = anima(window, cx) && qps > 0.;
    let agora = cx.background_executor().now();
    let janela = window.window_handle().window_id();
    let view = window.current_view();
    let lacos = cx.default_global::<Lacos>();
    if !anima {
        return lacos.fase;
    }
    let fase = lacos.andar(agora);
    // No ritmo do compasso dos spinners (12 por segundo) ou acima dele, o laço não arma batida
    // própria: entra na do compasso. Cada batida redesenha a janela inteira (uns 20 a 30 ms de
    // processador no release, medido em 13/11/2026), e as duas grades somadas davam até 28
    // redesenhos por segundo com um chat trabalhando: metade da thread da interface, com a
    // digitação e a rolagem esperando atrás (dono: "tá bem travado"). Numa grade só são 12.
    if qps >= spinner::BATIDAS_POR_SEGUNDO as f32 {
        spinner::compasso(window, cx);
        return fase;
    }
    let lacos = cx.default_global::<Lacos>();
    let quando = lacos.quando(janela, agora, Duration::from_secs_f32(1. / qps.min(QPS)));
    if lacos.marcar(janela, quando) {
        let espera = quando.saturating_duration_since(agora);
        cx.spawn(async move |cx| {
            cx.background_executor().timer(espera).await;
            cx.update(|cx| {
                if cx.default_global::<Lacos>().bateu(janela, quando) {
                    cx.notify(view);
                }
            });
        })
        .detach();
    }
    fase
}

/// As Animações das Configurações: seguir o GNOME (o padrão) ou desligadas no app todo. Ficam em
/// `interface.json` (`"animacoes": "desligadas"`), junto do zoom.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Animacoes {
    #[default]
    SeguirGnome,
    Desligadas,
}

impl Animacoes {
    fn do_json(valor: Option<&serde_json::Value>) -> Self {
        match valor.and_then(|v| v.as_str()) {
            Some("desligadas") => Animacoes::Desligadas,
            _ => Animacoes::SeguirGnome,
        }
    }

    fn nome(self) -> &'static str {
        match self {
            Animacoes::SeguirGnome => "gnome",
            Animacoes::Desligadas => "desligadas",
        }
    }

    /// Com o movimento reduzido: desligadas aqui, ou o sistema sem animações.
    fn reduz(self, sistema: bool) -> bool {
        self == Animacoes::Desligadas || sistema
    }
}

/// A escolha e o que o sistema dizia na abertura.
struct Preferencia {
    escolha: Animacoes,
    sistema: bool,
}

impl Global for Preferencia {}

/// Liga o movimento reduzido conforme a escolha guardada e o sistema; uma vez, na abertura (depois
/// do `gpui_kit::init`). Ligado aqui, o kit não desliga depois.
pub fn iniciar(cx: &mut App) {
    let escolha = Animacoes::do_json(super::zoom::interface().as_ref().and_then(|v| v.get("animacoes")));
    let sistema = reduzido_no_sistema(cx);
    cx.set_global(Preferencia { escolha, sistema });
    if escolha.reduz(sistema) {
        cx.set_reduce_motion(true);
    }
}

/// A escolha das Configurações e se o sistema está sem animações.
pub fn animacoes(cx: &App) -> (Animacoes, bool) {
    cx.try_global::<Preferencia>().map_or((Animacoes::SeguirGnome, false), |p| (p.escolha, p.sistema))
}

/// Troca a escolha na hora (todas as janelas redesenham) e grava em segundo plano.
pub fn escolher_animacoes(escolha: Animacoes, cx: &mut App) {
    let sistema = animacoes(cx).1;
    cx.set_global(Preferencia { escolha, sistema });
    cx.set_reduce_motion(escolha.reduz(sistema));
    if escolha == Animacoes::SeguirGnome {
        // Volta a acompanhar o kit, que segue o sistema ao vivo.
        gpui_kit::base::apply_system_reduce_motion(cx);
    }
    cx.refresh_windows();
    let nome = escolha.nome();
    cx.background_spawn(async move {
        super::zoom::gravar_na_interface("animacoes", serde_json::json!(nome));
    })
    .detach();
}

/// A escolha "seguir o sistema" e o aviso dela nas Configurações.
pub const SEGUIR_O_SISTEMA: &str = if cfg!(windows) { "Seguir o Windows" } else { "Seguir o GNOME" };
pub const SISTEMA_SEM_ANIMACOES: &str = if cfg!(windows) {
    "O Windows está sem os efeitos de animação: tudo aparece pronto."
} else {
    "O GNOME está sem animações: tudo aparece pronto."
};

/// Movimento reduzido no sistema: o GNOME com as animações desligadas, o Windows com os "Efeitos de
/// animação" desligados ou `VITRAL_SEM_ANIMACAO=1` (a bancada). No Linux, lê o `gsettings` uma vez, na
/// abertura (o kit lê o portal, que responde depois). No Windows, o que o kit leu no `init`
/// (`SPI_GETCLIENTAREAANIMATION`), que já está no `reduce_motion`.
fn reduzido_no_sistema(cx: &App) -> bool {
    if std::env::var("VITRAL_SEM_ANIMACAO").is_ok_and(|v| v == "1") {
        return true;
    }
    if cfg!(windows) {
        return cx.reduce_motion();
    }
    crate::processos::novo_comando("gsettings")
        .args(["get", "org.gnome.desktop.interface", "enable-animations"])
        .output()
        .ok()
        .filter(|saida| saida.status.success())
        .is_some_and(|saida| String::from_utf8_lossy(&saida.stdout).trim() == "false")
}

#[cfg(test)]
mod testes {
    use super::{Animacoes, Curva, Disparo, Interpolavel, Lacos, MESMO_QUADRO, PAUSA_DO_LACO, QPS, Transicao};
    use gpui_kit::{Hsla, Rgba, rgba};
    use std::{
        cell::Cell,
        rc::Rc,
        time::{Duration, Instant},
    };

    const D: Duration = Duration::from_millis(160);

    #[test]
    fn curvas_comecam_em_zero_terminam_em_um_e_nao_passam_do_alvo() {
        for c in [Curva::Entrar, Curva::Mover, Curva::Sair] {
            assert!(c.aplicar(0.).abs() < 1e-4, "{c:?}");
            assert!((c.aplicar(1.) - 1.).abs() < 1e-4, "{c:?}");
            let mut antes = 0.;
            for i in 1..=100 {
                let v = c.aplicar(i as f32 / 100.);
                assert!(v <= 1. + 1e-4 && v + 1e-4 >= antes, "{c:?} em {i}%");
                antes = v;
            }
        }
        // Entrar sai rápido: na metade do tempo já passou de 80% do caminho.
        assert!(Curva::Entrar.aplicar(0.5) > 0.8);
    }

    #[test]
    fn na_montagem_nao_anima_e_a_mudanca_anda_a_partir_de_onde_esta() {
        let t0 = Instant::now();
        let mut t = Transicao::nova(0f32, 0., D, Curva::Mover, t0, false);
        assert_eq!(t.avancar(0., D, Curva::Mover, t0, false), (0., false));
        let (v, anda) = t.avancar(10., D, Curva::Mover, t0 + Duration::from_millis(16), false);
        assert!(anda && v.abs() < 1e-3);
        let (meio, anda) = t.avancar(10., D, Curva::Mover, t0 + Duration::from_millis(96), false);
        assert!(anda && meio > 0. && meio < 10.);
        // Volta no meio do caminho: parte do valor de agora, não salta.
        let (volta, _) = t.avancar(0., D, Curva::Mover, t0 + Duration::from_millis(97), false);
        assert!((volta - meio).abs() < 0.5);
        let fim = t0 + Duration::from_millis(97) + D;
        assert_eq!(t.avancar(0., D, Curva::Mover, fim, false), (0., false));
        assert!(!t.animando.get());
    }

    #[test]
    fn movimento_reduzido_e_id_repetido_vao_direto_ao_alvo() {
        let t0 = Instant::now();
        let mut t = Transicao::nova(0f32, 0., D, Curva::Mover, t0, false);
        assert_eq!(t.avancar(5., D, Curva::Mover, t0, true), (5., false));
        // Dois donos com o mesmo id no mesmo quadro: o segundo vence, sem animar.
        let um = t0 + Duration::from_millis(50);
        assert!(t.avancar(1., D, Curva::Mover, um, false).1);
        let (v, anda) = t.avancar(2., D, Curva::Mover, um + MESMO_QUADRO / 2, false);
        assert_eq!((v, anda), (2., false));
        assert!(!t.animando.get());
        // Valor inicial diferente do alvo (encher ao abrir) anda desde a montagem.
        let mut enche = Transicao::nova(0f32, 0.6, D, Curva::Mover, t0, false);
        assert!(enche.avancar(0.6, D, Curva::Mover, t0 + Duration::from_millis(20), false).1);
        assert!(!Transicao::nova(0f32, 0.6, D, Curva::Mover, t0, true).avancar(0.6, D, Curva::Mover, t0, true).1);
    }

    #[test]
    fn disparo_toca_quando_a_chave_muda_e_nao_na_montagem() {
        let t0 = Instant::now();
        let mut d = Disparo { chave: 3, inicio: None, animando: Rc::new(Cell::new(false)) };
        assert_eq!(d.avancar(3, D, t0, false), None);
        assert!(d.avancar(4, D, t0, false).is_some());
        assert!(d.avancar(4, D, t0 + Duration::from_millis(100), false).is_some());
        assert_eq!(d.avancar(4, D, t0 + D, false), None);
        assert_eq!(d.avancar(5, D, t0 + D, true), None);
    }

    #[test]
    fn cor_anda_com_alfa_pre_multiplicado() {
        let transparente: Hsla = rgba(0xeef2ff00).into();
        let claro: Hsla = rgba(0xeef2ffff).into();
        let meio = Rgba::from(transparente.interpolar(&claro, 0.5));
        assert!((meio.a - 0.5).abs() < 0.01);
        // Do transparente ao claro sem escurecer no caminho.
        assert!(meio.r > 0.9 && meio.b > 0.95, "{meio:?}");
        let preto_vazio: Hsla = rgba(0x00000000).into();
        let m = Rgba::from(preto_vazio.interpolar(&claro, 0.5));
        assert!(m.r > 0.9, "{m:?}");
        assert_eq!((1f32, 2f32).interpolar(&(3., 6.), 0.5), (2., 4.));
    }

    /// O orçamento: quantos redesenhos uma transição pede, contando o quadro da mudança, com o
    /// relógio batendo em cima de 1/QPS. O hover de 120 fica em 8 (entrar e sair, 16), a pílula
    /// e a abertura da gaveta de 180 em 11 e o pulso de 240 (o do sino também) em 15.
    #[test]
    fn teto_de_quadros_por_efeito() {
        let quadros = |d: Duration| {
            let t0 = Instant::now();
            let mut t = Transicao::nova(0f32, 0., d, Curva::Mover, t0, false);
            let mut n = 1;
            let mut anda = t.avancar(1., d, Curva::Mover, t0, false).1;
            while anda {
                let agora = t0 + Duration::from_secs_f32(n as f32 / QPS);
                anda = t.avancar(1., d, Curva::Mover, agora, false).1;
                n += 1;
            }
            n
        };
        assert!(quadros(super::COR) <= 8, "{}", quadros(super::COR));
        assert!(quadros(super::ENTRAR) <= 10, "{}", quadros(super::ENTRAR));
        assert!(quadros(super::MOVER) <= 11, "{}", quadros(super::MOVER));
        assert!(quadros(super::ENCHER) <= 15, "{}", quadros(super::ENCHER));
        // A entrada de popovers e menus do vendor tem o mesmo teto.
        assert_eq!(gpui_kit::component::popover::ENTRADA_QPS, QPS);
    }

    /// O laço: a fase anda com o tempo e não salta depois de uma pausa; a batida mais cedo
    /// substitui a marcada, e a velha, ao chegar, não vale mais.
    #[test]
    fn laco_anda_sem_salto_e_a_batida_mais_cedo_vence() {
        let t0 = Instant::now();
        let mut lacos = Lacos::default();
        assert_eq!(lacos.andar(t0), 0.);
        let fase = lacos.andar(t0 + Duration::from_millis(125));
        assert!((fase - 0.125).abs() < 1e-9, "{fase}");
        // Dois pedidos no mesmo quadro (o fundo e o botão do menu) não andam a fase duas vezes.
        assert_eq!(lacos.andar(t0 + Duration::from_millis(125)), fase);
        // Depois de uma pausa, continua de onde estava.
        let volta = t0 + Duration::from_millis(125) + PAUSA_DO_LACO + Duration::from_secs(40);
        assert_eq!(lacos.andar(volta), fase);
        assert!(lacos.andar(volta + Duration::from_millis(50)) > fase);
        // Um dia aberto e a fase ainda anda de 50 em 50 ms (em f32 andaria em degraus).
        lacos.fase = 86_400.;
        let antes = lacos.andar(volta + Duration::from_millis(100));
        assert!((lacos.andar(volta + Duration::from_millis(150)) - antes - 0.05).abs() < 1e-6);

        let janela = gpui_kit::WindowId::from(7u64);
        let (devagar, rapido) = (t0 + Duration::from_millis(125), t0 + Duration::from_millis(50));
        assert!(lacos.marcar(janela, devagar));
        // Outro pedido no mesmo ritmo, ou mais lento, espera a batida marcada.
        assert!(!lacos.marcar(janela, devagar + MESMO_QUADRO / 2));
        assert!(!lacos.marcar(janela, devagar + Duration::from_millis(80)));
        // O mais rápido marca outra, mais cedo; a velha não vale ao chegar.
        assert!(lacos.marcar(janela, rapido));
        assert!(!lacos.bateu(janela, devagar));
        assert!(lacos.bateu(janela, rapido));
        assert!(!lacos.bateu(janela, rapido));
        assert!(lacos.marcar(janela, devagar));

        // O compasso: a batida seguinte conta da última, não do fim do desenho; com o desenho
        // pesado, meia batida de folga; depois de uma pausa, um intervalo inteiro.
        let passo = Duration::from_millis(50);
        let desenhou = rapido + Duration::from_millis(20);
        assert_eq!(lacos.quando(janela, desenhou, passo), rapido + passo);
        let pesado = rapido + Duration::from_millis(45);
        assert_eq!(lacos.quando(janela, pesado, passo), pesado + passo / 2);
        let depois = rapido + Duration::from_secs(3);
        assert_eq!(lacos.quando(janela, depois, passo), depois + passo);
        assert_eq!(lacos.quando(gpui_kit::WindowId::from(8u64), t0, passo), t0 + passo);
    }

    #[test]
    fn animacoes_guardadas_e_quando_reduzem() {
        let ler = |texto: &str| {
            let valor: serde_json::Value = serde_json::from_str(texto).unwrap();
            Animacoes::do_json(valor.get("animacoes"))
        };
        assert_eq!(ler(r#"{"zoom": 1.2, "animacoes": "desligadas"}"#), Animacoes::Desligadas);
        assert_eq!(ler(r#"{"animacoes": "gnome"}"#), Animacoes::SeguirGnome);
        assert_eq!(ler(r#"{"zoom": 1.2}"#), Animacoes::SeguirGnome);
        assert_eq!(ler(r#"{"animacoes": 3}"#), Animacoes::SeguirGnome);
        for escolha in [Animacoes::SeguirGnome, Animacoes::Desligadas] {
            assert_eq!(ler(&serde_json::json!({ "animacoes": escolha.nome() }).to_string()), escolha);
        }
        assert!(!Animacoes::SeguirGnome.reduz(false));
        assert!(Animacoes::SeguirGnome.reduz(true));
        assert!(Animacoes::Desligadas.reduz(false));
    }
}

// O segurar para confirmar (12/11/2026, a base AI Tool Approval): o "Segure para permitir" do
// pedido perigoso só confirma depois de `SEGURAR` apertado. É a exceção à duração curta do resto
// do arquivo: não é um efeito, é o gesto que confirma, e por isso vale igual com o movimento
// reduzido (só o enchimento não é desenhado). Pelo teclado, o primeiro Enter arma e o segundo,
// em até `ARMADO`, confirma.

/// Quanto o "Segure para permitir" precisa ficar apertado.
pub const SEGURAR: Duration = Duration::from_millis(900);
/// O mínimo entre o Enter que arma o botão de segurar e o que confirma.
const ENTRE_ENTERS: Duration = Duration::from_millis(250);
/// Quanto o primeiro Enter fica armado esperando o segundo.
pub const ARMADO: Duration = Duration::from_secs(3);

/// O estado de um botão de segurar, sem janela (os testes andam o relógio à mão).
#[derive(Default)]
struct EstadoDoSegurar {
    /// Quando o aperto de agora começou.
    apertado: Option<Instant>,
    /// Quando o primeiro Enter armou.
    armado: Option<Instant>,
    animando: Rc<Cell<bool>>,
}

impl EstadoDoSegurar {
    fn apertar(&mut self, agora: Instant) {
        self.apertado = Some(agora);
    }

    fn soltar(&mut self) {
        self.apertado = None;
        self.animando.set(false);
    }

    /// A batida do timer do aperto que começou em `marca`: completa só se o botão segue apertado
    /// desde ela (soltar e apertar de novo marca outro aperto) e o tempo inteiro já passou.
    fn completou(&mut self, marca: Instant, agora: Instant) -> bool {
        let vale = self.apertado == Some(marca) && agora.saturating_duration_since(marca) >= SEGURAR;
        if vale {
            self.soltar();
        }
        vale
    }

    /// De 0 a 1 enquanto apertado.
    fn progresso(&self, agora: Instant) -> Option<f32> {
        self.apertado
            .map(|m| (agora.saturating_duration_since(m).as_secs_f32() / SEGURAR.as_secs_f32()).min(1.))
    }

    fn armado(&self, agora: Instant) -> bool {
        self.armado.is_some_and(|a| agora.saturating_duration_since(a) < ARMADO)
    }

    /// Um Enter: o primeiro arma; o segundo, com o primeiro ainda armado, confirma. Um segundo
    /// Enter colado no primeiro (menos de `ENTRE_ENTERS`) não confirma nem rearma: é o mesmo
    /// gesto (um toque duplo sem querer, ou a repetição de um teclado que não avisa que é
    /// repetição).
    fn enter(&mut self, agora: Instant) -> bool {
        if let Some(armou) = self.armado
            && agora.saturating_duration_since(armou) < ENTRE_ENTERS
        {
            return false;
        }
        if self.armado(agora) {
            self.armado = None;
            return true;
        }
        self.armado = Some(agora);
        false
    }
}

/// Um botão de segurar, guardado entre quadros pelo `id`.
#[derive(Clone)]
pub struct Segurar(Entity<EstadoDoSegurar>);

impl Segurar {
    pub fn de(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> Self {
        Segurar(window.use_keyed_state(id.into(), cx, |_, _| EstadoDoSegurar::default()))
    }

    /// Marca o aperto e um timer de `SEGURAR`: ao bater, se o botão segue apertado desde então,
    /// solta e chama `ao_completar`. Soltar antes (ou sair com o ponteiro) cancela.
    pub fn apertar(&self, ao_completar: impl FnOnce(&mut App) + 'static, cx: &mut App) {
        let marca = cx.background_executor().now();
        self.0.update(cx, |e, cx| {
            e.apertar(marca);
            cx.notify();
        });
        let estado = self.0.downgrade();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(SEGURAR).await;
            cx.update(|cx| {
                let agora = cx.background_executor().now();
                let Some(estado) = estado.upgrade() else {
                    return;
                };
                let completou = estado.update(cx, |e, cx| {
                    cx.notify();
                    e.completou(marca, agora)
                });
                if completou {
                    ao_completar(cx);
                }
            });
        })
        .detach();
    }

    pub fn soltar(&self, cx: &mut App) {
        self.0.update(cx, |e, cx| {
            if e.apertado.is_some() {
                e.soltar();
                cx.notify();
            }
        });
    }

    /// O enchimento (0 a 1) enquanto apertado, pedindo quadro no relógio de 60 qps; `None` solto
    /// ou com o movimento reduzido (o tempo vale igual, só não se desenha).
    pub fn progresso(&self, window: &mut Window, cx: &mut App) -> Option<f32> {
        let agora = cx.background_executor().now();
        let (progresso, sinal) = {
            let e = self.0.read(cx);
            (e.progresso(agora), e.animando.clone())
        };
        let progresso = progresso.filter(|_| !cx.reduce_motion());
        sinal.set(progresso.is_some());
        if progresso.is_some() {
            pedir_quadro(sinal, window, cx);
        }
        progresso
    }

    /// O primeiro Enter armado, esperando o segundo.
    pub fn armado(&self, cx: &App) -> bool {
        self.0.read(cx).armado(cx.background_executor().now())
    }

    /// Um Enter no botão: o primeiro arma (e desarma sozinho em `ARMADO`); o segundo confirma.
    pub fn enter(&self, ao_confirmar: impl FnOnce(&mut App), cx: &mut App) {
        let agora = cx.background_executor().now();
        let confirma = self.0.update(cx, |e, cx| {
            cx.notify();
            e.enter(agora)
        });
        if confirma {
            ao_confirmar(cx);
            return;
        }
        let estado = self.0.downgrade();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(ARMADO).await;
            cx.update(|cx| {
                if let Some(estado) = estado.upgrade() {
                    estado.update(cx, |_, cx| cx.notify());
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod testes_do_segurar {
    use super::{ARMADO, EstadoDoSegurar, SEGURAR};
    use std::time::{Duration, Instant};

    /// Um clique curto nunca permite: soltar antes do tempo cancela, e o timer daquele aperto,
    /// ao bater, não completa nada.
    #[test]
    fn aperta_e_solta_antes_do_tempo_nao_completa() {
        let t0 = Instant::now();
        let mut e = EstadoDoSegurar::default();
        e.apertar(t0);
        e.soltar();
        assert!(!e.completou(t0, t0 + SEGURAR));
        assert_eq!(e.progresso(t0 + SEGURAR), None);
        // Apertar de novo marca outro aperto: o timer do primeiro não vale para ele.
        let t1 = t0 + Duration::from_millis(400);
        e.apertar(t1);
        assert!(!e.completou(t0, t0 + SEGURAR));
        // E o timer do aperto certo, se bater cedo, também não.
        assert!(!e.completou(t1, t1 + SEGURAR - Duration::from_millis(1)));
    }

    #[test]
    fn segurando_o_tempo_inteiro_completa_uma_vez() {
        let t0 = Instant::now();
        let mut e = EstadoDoSegurar::default();
        e.apertar(t0);
        let meio = e.progresso(t0 + SEGURAR / 2).unwrap();
        assert!((meio - 0.5).abs() < 0.01, "{meio}");
        assert!(e.completou(t0, t0 + SEGURAR));
        // Completou e soltou: a mesma batida não confirma duas vezes.
        assert!(!e.completou(t0, t0 + SEGURAR));
        assert_eq!(e.progresso(t0 + SEGURAR), None);
    }

    #[test]
    fn enter_arma_e_o_segundo_confirma_dentro_do_prazo() {
        let t0 = Instant::now();
        let mut e = EstadoDoSegurar::default();
        assert!(!e.enter(t0));
        assert!(e.armado(t0 + Duration::from_secs(1)));
        assert!(e.enter(t0 + Duration::from_secs(1)));
        // Confirmado, desarma: o próximo Enter arma de novo.
        assert!(!e.armado(t0 + Duration::from_secs(1)));
        // Passado o prazo, o segundo Enter só arma outra vez.
        let t1 = t0 + Duration::from_secs(10);
        assert!(!e.enter(t1));
        assert!(!e.enter(t1 + ARMADO));
        assert!(e.armado(t1 + ARMADO));
    }
}

#[cfg(test)]
mod testes_do_enter_duplo {
    use super::{ARMADO, ENTRE_ENTERS, EstadoDoSegurar};
    use std::time::{Duration, Instant};

    /// O perigoso pelo teclado pede dois Enters de verdade: um segundo colado no primeiro (a
    /// repetição da tecla segurada, um toque duplo sem querer) não confirma; passado o intervalo
    /// mínimo, confirma; e o armado expira.
    #[test]
    fn segundo_enter_colado_nao_confirma() {
        let t0 = Instant::now();
        let mut estado = EstadoDoSegurar::default();
        assert!(!estado.enter(t0), "o primeiro só arma");
        assert!(!estado.enter(t0 + Duration::from_millis(40)), "a repetição da tecla não confirma");
        assert!(!estado.enter(t0 + ENTRE_ENTERS - Duration::from_millis(1)));
        assert!(estado.enter(t0 + ENTRE_ENTERS + Duration::from_millis(10)), "o segundo Enter de verdade confirma");
        // Confirmado, desarma: o próximo Enter arma de novo.
        assert!(!estado.enter(t0 + Duration::from_secs(1)));
        // E o armado expira sozinho: o Enter seguinte só arma outra vez.
        assert!(!estado.enter(t0 + Duration::from_secs(1) + ARMADO + Duration::from_millis(10)));
    }
}
