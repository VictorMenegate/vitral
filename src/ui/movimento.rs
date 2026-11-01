//! A única porta de animação do Vitral. Todo efeito responde a uma ação (abrir, focar, clicar,
//! o ponteiro entrar num controle solto, chegar dado novo), dura de 120 a 240 ms e redesenha no
//! máximo 60 vezes por segundo: cada quadro de animação redesenha a janela inteira. Nada anima em
//! repouso, e com o movimento reduzido (o GNOME sem animações ou `VITRAL_SEM_ANIMACAO=1`) tudo
//! aparece direto no estado final.
//!
//! Duas formas:
//! - `entrar` (e `animacao`, para quem precisa do `with_animation`): toca uma vez quando o
//!   elemento aparece e não repete enquanto ele fica na tela. `entrada` só toca para o que é
//!   novo de verdade (a mensagem que chegou, não a conversa que abriu).
//! - `valor`, `valor_desde` e `disparo`: o elemento fica igual e só o valor anda até o alvo
//!   quando ele muda (a pílula do segmentado, a cor do hover, o anel). Na montagem não anima.
//!   A `abertura` da gaveta é um `valor_desde` de 0 a 1, com o id levando a geração.

use std::{
    cell::Cell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui_kit::base::animation::{cubic_bezier, ease_out_cubic};
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

/// Liga o movimento reduzido conforme a escolha guardada e o GNOME; uma vez, na abertura. Ligado
/// aqui, o kit não desliga depois.
pub fn iniciar(cx: &mut App) {
    let escolha = Animacoes::do_json(super::zoom::interface().as_ref().and_then(|v| v.get("animacoes")));
    let sistema = reduzido_no_sistema();
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

/// Movimento reduzido no sistema: o GNOME com as animações desligadas ou `VITRAL_SEM_ANIMACAO=1`
/// (a bancada). Lê o `gsettings` uma vez, na abertura.
fn reduzido_no_sistema() -> bool {
    if std::env::var("VITRAL_SEM_ANIMACAO").is_ok_and(|v| v == "1") {
        return true;
    }
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "enable-animations"])
        .output()
        .ok()
        .filter(|saida| saida.status.success())
        .is_some_and(|saida| String::from_utf8_lossy(&saida.stdout).trim() == "false")
}

#[cfg(test)]
mod testes {
    use super::{Animacoes, Curva, Disparo, Interpolavel, MESMO_QUADRO, QPS, Transicao};
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
