//! Estado e sinais: pontos de sinal, pílula de estado, selos e o número do painel, e as funções
//! que leem o motor para dar um desenho só a cada situação (`sinal_do_chat`).

use std::{rc::Rc, time::Duration};

use gpui_kit::component::{Icon, Sizable as _, spinner::compasso_recente, tooltip::Tooltip};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::{
    modelo::{Chat, Execucao},
    motor::Motor,
    tema::{Cores, Tipo as _, esp, r, raio, tam},
};

/// Sinal de atividade, na ordem de prioridade (o primeiro que valer ganha).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sinal {
    Erro,
    Pedido,
    /// Parada que precisa de Retomar (a parada por você não tem sinal).
    Parada,
    Nova,
    Rodando,
}

impl Sinal {
    pub fn cor(self) -> Hsla {
        match self {
            Sinal::Erro => Cores::vermelho(),
            Sinal::Pedido | Sinal::Parada => Cores::espera(),
            Sinal::Nova | Sinal::Rodando => Cores::destaque(),
        }
    }

    /// Rodando é aro sem miolo; os outros são ponto cheio.
    pub fn vazado(self) -> bool {
        self == Sinal::Rodando
    }

    /// Ponto de `lado` px (6 dentro do texto, 8 solto ou no canto), sem aro. O de rodando
    /// respira (o aro vai de 0,55 a 1 em 1,6 s) só enquanto o compasso dos spinners já bate.
    pub fn ponto(self, lado: f32) -> PontoDeSinal {
        PontoDeSinal { sinal: self, lado }
    }
}

/// O ponto de um `Sinal`.
#[derive(IntoElement)]
pub struct PontoDeSinal {
    sinal: Sinal,
    lado: f32,
}

/// Uma respiração do ponto de rodando.
const RESPIRO: Duration = Duration::from_millis(1600);

/// A opacidade do aro de rodando na fase `fase` do compasso: 1 no começo de cada respiro, 0,55
/// no meio.
fn opacidade_do_respiro(fase: Duration) -> f32 {
    let t = (fase.as_nanos() % RESPIRO.as_nanos()) as f32 / RESPIRO.as_nanos() as f32;
    0.775 + 0.225 * (t * std::f32::consts::TAU).cos()
}

impl RenderOnce for PontoDeSinal {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let ponto = div().flex_none().size(r(self.lado)).rounded_full();
        if self.sinal.vazado() {
            let alfa = compasso_recente(cx).map_or(1., opacidade_do_respiro);
            ponto.border_2().border_color(self.sinal.cor().opacity(alfa))
        } else {
            ponto.bg(self.sinal.cor())
        }
    }
}

/// O ícone de um chat É o estado dele (na aba e na linha da lista): o balão igual em todos não
/// dizia nada (dono, 13/11/2026: "tô achando esse ícone meio inútil"). Trabalhando, um anel
/// girando; esperando você (permissão, parada), um disco azul-claro; terminou e você não leu, azul;
/// erro, vermelho; parado e lido, o balão apagado.
#[derive(IntoElement)]
pub struct IconeDeEstado {
    sinal: Option<Sinal>,
    lado: f32,
}

pub fn icone_de_estado(sinal: Option<Sinal>, lado: f32) -> IconeDeEstado {
    IconeDeEstado { sinal, lado }
}

impl RenderOnce for IconeDeEstado {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let caixa = div().flex_none().size(r(self.lado)).flex().items_center().justify_center();
        match self.sinal {
            Some(Sinal::Rodando) => caixa.child(super::graficos::AnelGirando::new(self.lado - 2.).cor(Cores::destaque())),
            Some(sinal) => caixa.child(
                div()
                    .size(r(self.lado))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(Cores::tinta_solida(sinal.cor(), 0.22))
                    .child(div().size(r((self.lado * 0.45).round())).rounded_full().bg(sinal.cor())),
            ),
            None => caixa.child(super::icone(crate::tema::Lucide::MessageCircle, self.lado - 2., window).text_color(Cores::apagado())),
        }
    }
}

/// Estado de uma tarefa ou chat, como a pílula mostra.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Estado {
    Rodando,
    Verificando,
    EsperandoVoce,
    NaFila,
    EmRevisao,
    Pronto,
    Erro,
    /// Parada por você: neutra.
    Parado,
    /// Parada que precisa de Retomar: âmbar.
    ParadoAtencao,
}

impl Estado {
    pub const TODOS: [Estado; 9] = [
        Estado::Rodando,
        Estado::Verificando,
        Estado::EsperandoVoce,
        Estado::NaFila,
        Estado::EmRevisao,
        Estado::Pronto,
        Estado::Erro,
        Estado::Parado,
        Estado::ParadoAtencao,
    ];

    /// Cor do ponto e da tinta; `None` é neutro.
    pub fn cor(self) -> Option<Hsla> {
        match self {
            Estado::Rodando | Estado::Verificando | Estado::EmRevisao => Some(Cores::destaque()),
            Estado::EsperandoVoce | Estado::ParadoAtencao => Some(Cores::espera()),
            Estado::Pronto => Some(Cores::verde()),
            Estado::Erro => Some(Cores::vermelho()),
            Estado::NaFila | Estado::Parado => None,
        }
    }

    /// Cor do texto (o destaque puro não passa de 4,5:1 em texto pequeno).
    pub fn cor_texto(self) -> Hsla {
        match self {
            Estado::Rodando | Estado::Verificando | Estado::EmRevisao => Cores::destaque_texto(),
            _ => self.cor().unwrap_or_else(Cores::tenue),
        }
    }

    /// Texto padrão, sempre minúsculo (é adjetivo).
    pub fn rotulo(self) -> &'static str {
        match self {
            Estado::Rodando => "rodando",
            Estado::Verificando => "verificando",
            Estado::EsperandoVoce => "esperando você",
            Estado::NaFila => "na fila",
            Estado::EmRevisao => "em revisão",
            Estado::Pronto => "pronto",
            Estado::Erro => "falhou",
            Estado::Parado | Estado::ParadoAtencao => "parada",
        }
    }
}

/// "● rodando · 2 min 13 s": pílula de 22 na tinta da cor, sem borda.
#[derive(IntoElement)]
pub struct PilulaDeEstado {
    estado: Estado,
    rotulo: Option<SharedString>,
    detalhe: Option<SharedString>,
    compacta: bool,
    cor: Option<Hsla>,
}

impl PilulaDeEstado {
    pub fn new(estado: Estado) -> Self {
        PilulaDeEstado { estado, rotulo: None, detalhe: None, compacta: false, cor: None }
    }

    /// Troca o texto padrão ("aprovada", "cancelada").
    pub fn rotulo(mut self, r: impl Into<SharedString>) -> Self {
        self.rotulo = Some(r.into());
        self
    }

    /// Depois de " · ", em tênue: "2 min 13 s", "rodada 2".
    pub fn detalhe(mut self, d: impl Into<SharedString>) -> Self {
        self.detalhe = Some(d.into());
        self
    }

    /// Só o ponto, numa área de 14 onde a dica pega; o texto vai para a dica.
    pub fn compacta(mut self) -> Self {
        self.compacta = true;
        self
    }
}

impl RenderOnce for PilulaDeEstado {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let cor = self.cor.or(self.estado.cor());
        let cor_texto = self.cor.unwrap_or(self.estado.cor_texto());
        let rotulo = self.rotulo.unwrap_or_else(|| self.estado.rotulo().into());
        let ponto = div()
            .flex_none()
            .size(r(6.))
            .rounded_full()
            .bg(cor.unwrap_or_else(Cores::tenue));
        if self.compacta {
            let dica: SharedString = match &self.detalhe {
                Some(d) => format!("{rotulo} · {d}").into(),
                None => rotulo.clone(),
            };
            return div()
                .id(SharedString::from(format!("pilula-{:?}-{rotulo}", self.estado)))
                .flex_none()
                .size(r(14.))
                .flex()
                .items_center()
                .justify_center()
                .child(ponto)
                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                .into_any_element();
        }
        div()
            .flex_none()
            .h(r(22.))
            .px(r(8.))
            .flex()
            .items_center()
            .gap(r(6.))
            .rounded(raio::de(22.))
            .bg(cor.map(Cores::tinta).unwrap_or_else(Cores::hover))
            .child(ponto)
            .child(
                div()
                    .metadado()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cor_texto)
                    .whitespace_nowrap()
                    .child(rotulo)
                    .when_some(self.detalhe, |s, d| {
                        s.child(div().text_color(Cores::tenue()).child(format!(" · {d}")))
                            .flex()
                    }),
            )
            .into_any_element()
    }
}

#[derive(Clone)]
enum TipoSelo {
    Neutro,
    Cor(Hsla),
    Novo,
    Contagem(usize),
    Capacidades(Vec<(Icon, Hsla, &'static str)>),
    Ponto(Hsla),
}

/// Selo pequeno: rótulo, contagem, ponto ou capacidades de um modelo.
#[derive(IntoElement, Clone)]
pub struct Selo {
    tipo: TipoSelo,
    rotulo: Option<SharedString>,
    icone: Option<(Icon, Hsla)>,
    tingida: Option<Hsla>,
    solida: Option<Hsla>,
    vazado: bool,
    lado: f32,
    clicavel: Option<(ElementId, Rc<dyn Fn(&mut Window, &mut App)>)>,
}

impl Selo {
    fn de(tipo: TipoSelo, rotulo: Option<SharedString>) -> Self {
        Selo {
            tipo,
            rotulo,
            icone: None,
            tingida: None,
            solida: None,
            vazado: false,
            lado: 8.,
            clicavel: None,
        }
    }

    /// "1M de contexto", "padrão": contorno, 11/600 tênue.
    pub fn neutro(r: impl Into<SharedString>) -> Self {
        Self::de(TipoSelo::Neutro, Some(r.into()))
    }

    /// "rápido", "beta": tinta da cor, rótulo na cor.
    pub fn cor(r: impl Into<SharedString>, cor: Hsla) -> Self {
        Self::de(TipoSelo::Cor(cor), Some(r.into()))
    }

    pub fn novo() -> Self {
        Self::de(TipoSelo::Novo, Some("novo".into()))
    }

    /// Número neutro; `tingida` põe na tinta da cor e `solida`, sobre a cor cheia.
    pub fn contagem(n: usize) -> Self {
        Self::de(TipoSelo::Contagem(n), None)
    }

    pub fn tingida(mut self, cor: Hsla) -> Self {
        self.tingida = Some(cor);
        self
    }

    /// Só quando fica por cima de algo (sino, avatar).
    pub fn solida(mut self, cor: Hsla) -> Self {
        self.solida = Some(cor);
        self
    }

    pub fn ponto(cor: Hsla) -> Self {
        Self::de(TipoSelo::Ponto(cor), None)
    }

    /// Ponto só com o aro (rodando).
    pub fn vazado(mut self) -> Self {
        self.vazado = true;
        self
    }

    /// Lado do ponto (8 por padrão).
    pub fn lado(mut self, px: f32) -> Self {
        self.lado = px;
        self
    }

    /// Ícone antes do rótulo.
    pub fn icone(mut self, icone: impl Into<Icon>, cor: Hsla) -> Self {
        self.icone = Some((icone.into(), cor));
        self
    }

    /// Ícones de 14 separados por divisória, cada um com a sua dica.
    pub fn capacidades(itens: Vec<(Icon, Hsla, &'static str)>) -> Self {
        Self::de(TipoSelo::Capacidades(itens), None)
    }

    pub fn clicavel(mut self, id: impl Into<ElementId>, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.clicavel = Some((id.into(), Rc::new(f)));
        self
    }
}

impl RenderOnce for Selo {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let pilula = || div().flex_none().flex().items_center().rounded(raio::de(20.)).micro().whitespace_nowrap();
        let corpo = match self.tipo {
            TipoSelo::Ponto(cor) => {
                let ponto = div().flex_none().size(r(self.lado)).rounded_full();
                if self.vazado { ponto.border_2().border_color(cor) } else { ponto.bg(cor) }.into_any_element()
            }
            TipoSelo::Contagem(n) => {
                let texto = if n > 99 { "99+".to_string() } else { n.to_string() };
                let (fundo, cor) = match (self.solida, self.tingida) {
                    (Some(c), _) => (c, Cores::sobre_cor()),
                    (None, Some(c)) => (Cores::tinta(c), c),
                    (None, None) => (Cores::traco(), Cores::tenue()),
                };
                pilula()
                    .min_w(r(18.))
                    .h(r(18.))
                    .px(esp::E4)
                    .justify_center()
                    .bg(fundo)
                    .text_color(cor)
                    .child(texto)
                    .into_any_element()
            }
            TipoSelo::Capacidades(itens) => {
                let n = itens.len();
                let id: Vec<&str> = itens.iter().map(|(_, _, dica)| *dica).collect();
                pilula()
                    .id(SharedString::from(format!("capacidades-{}", id.join("-"))))
                    .h(r(22.))
                    .px(r(4.))
                    .border_1()
                    .border_color(Cores::borda())
                    .children(itens.into_iter().enumerate().map(|(i, (icone, cor, dica))| {
                        div()
                            .id(("capacidade", i))
                            .flex()
                            .items_center()
                            .px(r(4.))
                            .h_full()
                            .when(i + 1 < n, |s| s.border_r_1().border_color(Cores::borda()))
                            .child(icone.with_size(tam(window, 14.)).text_color(cor))
                            .tooltip(move |window, cx| Tooltip::new(dica).build(window, cx))
                    }))
                    .into_any_element()
            }
            tipo => {
                let (fundo, borda, cor) = match tipo {
                    TipoSelo::Cor(c) => (Cores::tinta(c), None, c),
                    TipoSelo::Novo => (Cores::destaque().opacity(0.18), None, Cores::destaque_texto()),
                    _ => (gpui_kit::transparent_black(), Some(Cores::borda()), Cores::tenue()),
                };
                pilula()
                    .h(r(20.))
                    .px(r(6.))
                    .gap(r(4.))
                    .bg(fundo)
                    .when_some(borda, |s, b| s.border_1().border_color(b))
                    .text_color(cor)
                    .when_some(self.icone, |s, (icone, cor)| {
                        s.child(icone.with_size(tam(window, 12.)).text_color(cor))
                    })
                    .children(self.rotulo)
                    .into_any_element()
            }
        };
        match self.clicavel {
            Some((id, f)) => div()
                .id(id)
                .cursor_pointer()
                .child(corpo)
                .on_click(move |_, window, cx| f(window, cx))
                .into_any_element(),
            None => corpo,
        }
    }
}

/// Etiqueta de fio: a segunda voz do Vitral, um fato curto em mono 11 numa pílula de contorno (a
/// pasta, o ramo, "revisar", "saída", o relógio). Não é botão nem selo de contagem: é o que um
/// aparelho escreveria do lado do mostrador. 20 de altura (16 na `baixa`).
#[derive(IntoElement)]
pub struct Etiqueta {
    texto: SharedString,
    icone: Option<Icon>,
    tom: Option<Hsla>,
    baixa: bool,
    fundo: bool,
}

impl Etiqueta {
    pub fn new(texto: impl Into<SharedString>) -> Self {
        Etiqueta { texto: texto.into(), icone: None, tom: None, baixa: false, fundo: false }
    }

    /// Ícone de 11 antes do texto, apagado.
    pub fn icone(mut self, i: impl Into<Icon>) -> Self {
        self.icone = Some(i.into());
        self
    }

    /// O fio na tinta da cor e o texto na cor ("direto" em âmbar, "revisar").
    pub fn tom(mut self, cor: Hsla) -> Self {
        self.tom = Some(cor);
        self
    }

    /// 16 de altura, para caber numa linha de lista.
    pub fn baixa(mut self) -> Self {
        self.baixa = true;
        self
    }

    /// Com o fundo cavado (`recuo`), para quem fica por cima de texto ou de pontilhado.
    pub fn fundo(mut self) -> Self {
        self.fundo = true;
        self
    }
}

impl RenderOnce for Etiqueta {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let (fio, cor) = match self.tom {
            Some(c) => (Cores::tinta_borda(c), c),
            None => (Cores::borda_forte(), Cores::tenue()),
        };
        div()
            .flex_none()
            .h(r(if self.baixa { 16. } else { 20. }))
            .px(esp::E6)
            .gap(esp::E4)
            .flex()
            .items_center()
            .rounded(raio::de(if self.baixa { 16. } else { 20. }))
            .border_1()
            .border_color(fio)
            .when(self.fundo, |s| s.bg(Cores::recuo()))
            .etiqueta()
            .tabular()
            .whitespace_nowrap()
            .text_color(cor)
            .when_some(self.icone, |s, icone| {
                s.child(icone.with_size(tam(window, 11.)).text_color(Cores::apagado()))
            })
            .child(self.texto)
    }
}

/// Número do painel (Alt+1…4): destaque cheio no ativo, recuo com borda nos outros.
pub fn numero_painel(n: usize, ativo: bool) -> impl IntoElement {
    div()
        .flex_none()
        .size(r(18.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(raio::de(18.))
        .micro()
        .relative()
        .map(|s| {
            if ativo {
                // O número do painel ativo é uma tecla de metal azul, como o "+" e o enviar.
                let cantos = gpui_kit::Corners::all(raio::de(18.));
                s.text_color(Cores::sobre_destaque())
                    .child(super::metal::abertura(super::metal::Encaixe::Cobrir, Some(Cores::destaque().opacity(0.6)), cantos))
                    .child(div().absolute().inset_0().rounded(raio::de(18.)).shadow(crate::tema::aresta_de_tecla()))
            } else {
                s.bg(Cores::recuo()).border_1().border_color(Cores::borda()).text_color(Cores::tenue())
            }
        })
        .child(div().relative().child(n.to_string()))
}

/// O que um chat tem para o sinal, já filtrado (arquivado não espera nada).
#[derive(Clone, Copy, Default, Debug)]
struct FatosDoSinal {
    erro: bool,
    pedido: bool,
    /// Parou e precisa de Retomar (a parada por você não conta).
    parada: bool,
    nova: bool,
    /// Processo ativo.
    rodando: bool,
}

fn fatos_do_sinal(motor: &Motor, c: &Chat) -> FatosDoSinal {
    let execucao = motor.execucao(&c.id);
    let espera = !c.arquivado;
    FatosDoSinal {
        erro: espera && matches!(execucao, Execucao::Erro(_)),
        pedido: espera && execucao == Execucao::EsperandoVoce,
        parada: espera && motor.precisa_retomar(&c.id),
        nova: espera && motor.nao_lido(c),
        rodando: matches!(execucao, Execucao::Rodando { .. }),
    }
}

/// O sinal mais forte de um grupo de chats: erro, pedido, parada, resposta nova e rodando.
fn sinal_de(fatos: &[FatosDoSinal]) -> Option<Sinal> {
    let algum = |f: fn(&FatosDoSinal) -> bool| fatos.iter().any(f);
    if algum(|f| f.erro) {
        Some(Sinal::Erro)
    } else if algum(|f| f.pedido) {
        Some(Sinal::Pedido)
    } else if algum(|f| f.parada) {
        Some(Sinal::Parada)
    } else if algum(|f| f.nova) {
        Some(Sinal::Nova)
    } else if algum(|f| f.rodando) {
        Some(Sinal::Rodando)
    } else {
        None
    }
}

/// O sinal de um chat.
pub fn sinal_do_chat(motor: &Motor, chat: &Chat) -> Option<Sinal> {
    sinal_de(&[fatos_do_sinal(motor, chat)])
}

/// O motivo da parada em poucas palavras ("Vitral fechou"), o mesmo na prévia da lista e no sino.
pub fn motivo_curto(motivo: &str) -> String {
    if motivo.contains("Vitral") { "Vitral fechou".into() } else { motivo.to_string() }
}

#[cfg(test)]
mod testes {
    use super::{Estado, FatosDoSinal, Sinal, motivo_curto, sinal_de};

    #[test]
    fn motivo_da_parada_em_poucas_palavras() {
        assert_eq!(motivo_curto("o Vitral foi fechado no meio do turno"), "Vitral fechou");
        assert_eq!(motivo_curto("outro motivo"), "outro motivo");
    }

    /// A cor de espera fica só para o que precisa de você; a revisão anda na família do rodando.
    #[test]
    fn cor_de_espera_so_no_que_precisa_de_voce() {
        use crate::tema::Cores;
        let espera: Vec<Estado> = Estado::TODOS.into_iter().filter(|e| e.cor() == Some(Cores::espera())).collect();
        assert_eq!(espera, [Estado::EsperandoVoce, Estado::ParadoAtencao]);
        assert_eq!(Estado::EmRevisao.cor(), Some(Cores::destaque()));
        assert_eq!(Estado::EmRevisao.cor_texto(), Cores::destaque_texto());
        assert_eq!(Estado::Rodando.cor_texto(), Cores::destaque_texto());
    }

    #[test]
    fn rodando_respira_de_um_a_055() {
        use super::opacidade_do_respiro;
        use std::time::Duration;
        assert!((opacidade_do_respiro(Duration::ZERO) - 1.).abs() < 1e-4);
        assert!((opacidade_do_respiro(Duration::from_millis(800)) - 0.55).abs() < 1e-4);
        assert!((opacidade_do_respiro(Duration::from_millis(1600)) - 1.).abs() < 1e-4);
    }

    #[test]
    fn sinal_segue_a_prioridade_da_familia() {
        let nada = FatosDoSinal::default();
        assert_eq!(sinal_de(&[nada, nada]), None);
        let rodando = FatosDoSinal { rodando: true, ..nada };
        let nova = FatosDoSinal { nova: true, ..nada };
        let parada = FatosDoSinal { parada: true, ..nada };
        let pedido = FatosDoSinal { pedido: true, rodando: true, ..nada };
        let erro = FatosDoSinal { erro: true, ..nada };
        assert_eq!(sinal_de(&[rodando]), Some(Sinal::Rodando));
        assert_eq!(sinal_de(&[rodando, nova]), Some(Sinal::Nova));
        assert_eq!(sinal_de(&[nova, parada]), Some(Sinal::Parada));
        assert_eq!(sinal_de(&[parada, pedido]), Some(Sinal::Pedido));
        assert_eq!(sinal_de(&[pedido, erro, nova]), Some(Sinal::Erro));
        // Parada por você não pede Retomar: sem ponto.
        assert_eq!(sinal_de(&[FatosDoSinal { parada: false, ..nada }]), None);
        assert!(Sinal::Rodando.vazado() && !Sinal::Nova.vazado());
    }
}
