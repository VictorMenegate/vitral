//! Peças dos painéis: aba, prévia de arrasto e as faixas de largura do painel e da caixa.

use std::rc::Rc;

use gpui_kit::component::{Icon, Sizable as _, tooltip::Tooltip};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::base::Simbolo;
use super::estado::{Sinal, numero_painel};
use super::metal::aro_de_metal;
use super::superficies::Flutuante as _;
use crate::tema::{Cores, Lucide, Tipo as _, esp, gradiente_quente, halo_foco, r, raio, tam};

/// Grupo de hover da aba: no hover, o × aparece e o ponto do estado some.
pub const GRUPO_DA_ABA: &str = "aba";

/// Como a aba se pinta: única (sobre a linha), ativa (o azul da seleção, só no painel ativo),
/// inativa, selecionada (numa barra de abas) ou normal.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TomAba {
    Unica,
    Ativa,
    Inativa,
    Selecionada,
    #[default]
    Normal,
    /// A aba à vista de um painel sem o foco que tem outras abas ao lado: um degrau mais claro
    /// que elas (que são `Inativa`), para dar para ver qual está aberta.
    InativaAVista,
}

/// Aba de painel ou de arquivo: 32 em pílula, título 13/500, até 400; encolhe com o painel.
pub struct Aba {
    id: ElementId,
    titulo: SharedString,
    simbolo: Option<Simbolo>,
    numero: Option<(usize, bool)>,
    detalhe: Option<(SharedString, bool)>,
    tom: TomAba,
    estado: Option<Sinal>,
    fechar: Option<(SharedString, bool, Rc<dyn Fn(&mut Window, &mut App)>)>,
    arrastando: bool,
    foco: Option<FocusHandle>,
    fim: Option<AnyElement>,
    so_simbolo: bool,
}

impl Aba {
    pub fn new(id: impl Into<ElementId>, titulo: impl Into<SharedString>) -> Self {
        Aba {
            id: id.into(),
            titulo: titulo.into(),
            simbolo: None,
            numero: None,
            detalhe: None,
            tom: TomAba::default(),
            estado: None,
            fechar: None,
            arrastando: false,
            foco: None,
            fim: None,
            so_simbolo: false,
        }
    }

    /// Só o símbolo, sem o título nem o detalhe (as abas da direita estreita); a dica dá o nome.
    pub fn so_simbolo(mut self, sim: bool) -> Self {
        self.so_simbolo = sim;
        self
    }

    pub fn simbolo(mut self, s: Simbolo) -> Self {
        self.simbolo = Some(s);
        self
    }

    /// Número do painel (com vários painéis).
    pub fn numero(mut self, n: usize, ativo: bool) -> Self {
        self.numero = Some((n, ativo));
        self
    }

    /// Texto curto depois do título (o caminho, em mono).
    pub fn detalhe(mut self, t: impl Into<SharedString>, mono: bool) -> Self {
        self.detalhe = Some((t.into(), mono));
        self
    }

    pub fn tom(mut self, t: TomAba) -> Self {
        self.tom = t;
        self
    }

    /// Ponto de 8 no fim, no lugar do ×: some quando o × aparece.
    pub fn estado(mut self, s: Option<Sinal>) -> Self {
        self.estado = s;
        self
    }

    /// O × dentro da aba: sempre visível ou só no hover. O clique do meio também fecha.
    pub fn fechar(
        mut self,
        dica: impl Into<SharedString>,
        sempre: bool,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.fechar = Some((dica.into(), sempre, Rc::new(f)));
        self
    }

    /// A aba de origem de um arrasto: apagada e tracejada.
    pub fn arrastando(mut self, sim: bool) -> Self {
        self.arrastando = sim;
        self
    }

    /// A aba entra no Tab com este foco, com o anel de foco; focada, mostra o ×.
    pub fn foco(mut self, f: &FocusHandle) -> Self {
        self.foco = Some(f.clone());
        self
    }

    pub fn construir(self, window: &Window, _: &App) -> Stateful<Div> {
        // A aba do chat tem fundo sempre (pedido do dono, 12/11/2026), e opaco: é o miolo do aro
        // de metal que acende com o ponteiro em cima, o mesmo contorno da caixa de mensagem. As
        // aba à vista do painel com o foco mora numa casca preta (`casca_da_aba`) e é o campo dela,
        // com o material do campo da caixa de mensagem (dono, 13/11/2026: "tem que ficar mais
        // parecido com a que digito aqui"); as outras são lisas, só com o contorno.
        let campo = matches!(self.tom, TomAba::Unica | TomAba::Ativa);
        let (fundo, texto, borda): (Background, Hsla, Hsla) = match self.tom {
            TomAba::Unica | TomAba::Ativa => (gradiente_quente(), Cores::forte(), Cores::borda()),
            // A aba de um painel sem o foco: lisa, quase da cor do painel, só com o contorno.
            TomAba::Inativa => (Cores::tinta_solida(Cores::forte(), 0.03).into(), Cores::tenue(), Cores::borda()),
            TomAba::Selecionada => (Cores::cartao_elevado().into(), Cores::forte(), Cores::borda()),
            TomAba::Normal => (Cores::tinta_solida(Cores::forte(), 0.05).into(), Cores::tenue(), transparent_black()),
            TomAba::InativaAVista => (Cores::tinta_solida(Cores::forte(), 0.10).into(), Cores::texto(), transparent_black()),
        };
        let focada = self.foco.as_ref().is_some_and(|f| f.is_focused(window));
        let fechar_a_vista = self.fechar.as_ref().is_some_and(|(_, sempre, _)| *sempre || focada);
        let tem_fechar = self.fechar.is_some();
        let fechar_meio = self.fechar.as_ref().map(|(_, _, f)| f.clone());
        let fechar = self.fechar.map(|(dica, _, f)| {
            div()
                .id("fechar")
                .flex_none()
                .size(r(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(raio::de(20.))
                .text_color(Cores::tenue())
                .cursor_pointer()
                .hover(|s| s.bg(Cores::hover()).text_color(Cores::texto()))
                .when(!fechar_a_vista, |s| s.invisible().group_hover(GRUPO_DA_ABA, |s| s.visible()))
                .child(Icon::new(Lucide::X).with_size(tam(window, 14.)))
                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    f(window, cx);
                })
        });
        // O ponto do estado divide o lugar com o ×; o espaço fica reservado para o título não
        // pular no hover.
        let ponto = self.estado.filter(|_| !fechar_a_vista).map(|sinal| {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .when(tem_fechar, |s| s.group_hover(GRUPO_DA_ABA, |s| s.invisible()))
                .child(sinal.ponto(8.))
        });
        let fim = (fechar.is_some() || ponto.is_some())
            .then(|| div().relative().flex_none().size(r(20.)).children(ponto).children(fechar));
        // Ícone solto em 16; o círculo do chat em 18.
        let simbolo = self.simbolo.map(|s| {
            let lado = if matches!(s, Simbolo::Icone(..)) { 16. } else { 18. };
            s.desenhar(lado, window)
        });
        div()
            .id(self.id)
            .group(GRUPO_DA_ABA)
            .flex()
            .flex_shrink(1.)
            .items_center()
            // O símbolo e o título com mais ar, e mais folga do lado do símbolo que do lado do ×
            // (que já tem a área de clique dele).
            .gap(esp::E8)
            .h(r(32.))
            .max_w(r(400.))
            .min_w_0()
            .pl(esp::E12)
            .pr(esp::E8)
            .relative()
            .rounded(raio::de(32.))
            .border_1()
            .border_color(borda)
            .text_color(texto)
            .map(|s| match self.tom {
                TomAba::Inativa | TomAba::Normal => s.hover(|s| s.text_color(Cores::texto())),
                TomAba::Unica | TomAba::Ativa | TomAba::Selecionada | TomAba::InativaAVista => s,
            })
            // O aro de metal (só com o ponteiro em cima) e, por cima dele, o miolo opaco da aba.
            .when(!self.arrastando, |s| {
                s.child(aro_de_metal(raio::de(32.), px(2.), 1.).invisible().group_hover(GRUPO_DA_ABA, |s| s.visible()))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(raio::de(32.))
                            .bg(fundo)
                            // O brilho de cima do campo da caixa de mensagem.
                            .when(campo, |s| s.shadow(vec![BoxShadow::new(px(0.), px(1.), Cores::brilho_topo()).inset()])),
                    )
            })
            .when(self.arrastando, |s| s.opacity(0.45).border_dashed().border_color(Cores::borda_forte()))
            .when_some(self.foco, |s, foco| {
                s.track_focus(&foco)
                    .tab_index(0)
                    .focus_visible(|s| s.border_color(Cores::destaque()).shadow(halo_foco()))
            })
            .children(simbolo)
            .when(!self.so_simbolo, |s| s.child(div().min_w_0().truncate().rotulo().child(self.titulo)))
            .children(self.detalhe.filter(|_| !self.so_simbolo).map(|(t, mono)| {
                div()
                    .flex_none()
                    .max_w(r(160.))
                    .truncate()
                    .text_color(Cores::tenue())
                    .map(|d| if mono { d.mono_pequeno() } else { d.metadado() })
                    .child(t)
            }))
            // O número do painel fica à direita, antes do × (dono, 13/11/2026).
            .children(self.numero.map(|(n, ativo)| numero_painel(n, ativo)))
            .children(self.fim)
            .children(fim)
            .when_some(fechar_meio, |s, f| {
                s.on_aux_click(move |ev: &ClickEvent, window, cx| {
                    if ev.is_middle_click() {
                        f(window, cx);
                    }
                })
            })
    }
}

/// A casca da aba à vista no painel com o foco: o preto da caixa de mensagem em volta dela,
/// reduzido a um fio (1 px de folga mais 1 px de borda), com a aba fazendo de campo. O dono viu a
/// casca com 4 e com 2 de folga e pediu mais fina, e entre as abas todas num trilho só e cada aba
/// uma peça, ficou com as peças separadas (13/11/2026): só esta leva a casca, e as outras abas e
/// as dos painéis sem o foco são lisas, com o contorno.
pub fn casca_da_aba() -> Div {
    div().flex().items_center().min_w_0().p(px(1.)).rounded(raio::de(34.)).border_1().border_color(Cores::preto()).bg(Cores::preto())
}

/// Aba em miniatura que segue o ponteiro ao arrastar: opaca, em pílula como a aba, até 280.
pub struct PreviaArrasto {
    pub titulo: SharedString,
    pub simbolo: Simbolo,
}

impl Render for PreviaArrasto {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let simbolo = self.simbolo.copia().map(|s| s.desenhar(16., window));
        div()
            .flex()
            .items_center()
            .gap(r(8.))
            .h(r(32.))
            .max_w(r(280.))
            .px(esp::E12)
            .flutuante(cx)
            .border_1()
            .border_color(Cores::borda_forte())
            .children(simbolo)
            .child(div().min_w_0().truncate().rotulo().text_color(Cores::forte()).child(self.titulo.clone()))
    }
}

/// Folga para a faixa não ficar trocando com a largura parada no limite.
const FOLGA: f32 = 0.5;

/// Largura do painel de conversa: limites de 320, 400 e 520 px (em rem a 100%).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LarguraPainel {
    Minimo,
    Estreito,
    #[default]
    Medio,
    Largo,
}

impl LarguraPainel {
    const LIMITES: [f32; 3] = [320. / 15., 400. / 15., 520. / 15.];
    const TODOS: [LarguraPainel; 4] =
        [LarguraPainel::Minimo, LarguraPainel::Estreito, LarguraPainel::Medio, LarguraPainel::Largo];

    /// Nível para a largura `rem`; só muda de nível quando passa do limite pela folga.
    pub fn de(rem: f32, anterior: Self) -> Self {
        let antes = anterior as usize;
        let acima = |folga: f32| Self::LIMITES.iter().filter(|l| rem >= **l + folga).count();
        let cru = acima(0.);
        let nivel = if cru > antes {
            acima(FOLGA).max(antes)
        } else if cru < antes {
            acima(-FOLGA).min(antes)
        } else {
            cru
        };
        Self::TODOS[nivel]
    }
}

/// Faixa da caixa de mensagem (limites de 35 e 46 rem de caixa).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FaixaCaixa {
    Estreita,
    #[default]
    Media,
    Larga,
}

impl FaixaCaixa {
    const LIMITE_LARGA: f32 = 46.;
    const LIMITE_MEDIA: f32 = 35.;

    pub fn de(nominal_rem: f32, anterior: Self) -> Self {
        let larga = if anterior == FaixaCaixa::Larga {
            nominal_rem >= Self::LIMITE_LARGA - FOLGA
        } else {
            nominal_rem >= Self::LIMITE_LARGA + FOLGA
        };
        let media = if anterior == FaixaCaixa::Estreita {
            nominal_rem >= Self::LIMITE_MEDIA + FOLGA
        } else {
            nominal_rem >= Self::LIMITE_MEDIA - FOLGA
        };
        if larga {
            FaixaCaixa::Larga
        } else if media {
            FaixaCaixa::Media
        } else {
            FaixaCaixa::Estreita
        }
    }
}

/// Coluna de leitura (até 860 px) menos a margem de 16 de cada lado: a largura nominal da caixa.
fn caixa_nominal(rem: f32) -> f32 {
    rem.min(860. / 15.) - 32. / 15.
}

/// Medida de um painel, feita uma vez por quadro no cartão.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct MedidaPainel {
    pub rem: f32,
    pub nivel: LarguraPainel,
    pub faixa: FaixaCaixa,
}

impl MedidaPainel {
    /// Guarda a largura e devolve `true` só quando o nível ou a faixa mudam.
    pub fn atualizar(&mut self, largura_cartao: Pixels, rem: Pixels) -> bool {
        self.rem = largura_cartao / rem;
        let nivel = LarguraPainel::de(self.rem, self.nivel);
        let faixa = FaixaCaixa::de(caixa_nominal(self.rem), self.faixa);
        let mudou = nivel != self.nivel || faixa != self.faixa;
        self.nivel = nivel;
        self.faixa = faixa;
        mudou
    }
}

#[cfg(test)]
mod testes {
    use super::{FaixaCaixa, LarguraPainel, MedidaPainel};
    use gpui_kit::px;

    #[test]
    fn largura_do_painel_com_folga() {
        use LarguraPainel::*;
        let rem = |px: f32| px / 15.;
        assert_eq!(LarguraPainel::de(rem(200.), Medio), Minimo);
        assert_eq!(LarguraPainel::de(rem(360.), Medio), Estreito);
        assert_eq!(LarguraPainel::de(rem(460.), Minimo), Medio);
        assert_eq!(LarguraPainel::de(rem(800.), Minimo), Largo);
        // Logo abaixo do limite de 400, o Médio fica; um pouco mais e cai.
        assert_eq!(LarguraPainel::de(rem(398.), Medio), Medio);
        assert_eq!(LarguraPainel::de(rem(390.), Medio), Estreito);
        // Logo acima do limite, o Estreito ainda não sobe.
        assert_eq!(LarguraPainel::de(rem(402.), Estreito), Estreito);
        assert_eq!(LarguraPainel::de(rem(410.), Estreito), Medio);
        assert_eq!(LarguraPainel::de(rem(522.), Medio), Medio);
        assert_eq!(LarguraPainel::de(rem(530.), Medio), Largo);
    }

    #[test]
    fn faixa_da_caixa_com_folga() {
        use FaixaCaixa::*;
        assert_eq!(FaixaCaixa::de(30., Media), Estreita);
        assert_eq!(FaixaCaixa::de(40., Media), Media);
        assert_eq!(FaixaCaixa::de(50., Media), Larga);
        assert_eq!(FaixaCaixa::de(46.2, Media), Media);
        assert_eq!(FaixaCaixa::de(46.6, Media), Larga);
        assert_eq!(FaixaCaixa::de(45.7, Larga), Larga);
        assert_eq!(FaixaCaixa::de(45.4, Larga), Media);
        assert_eq!(FaixaCaixa::de(35.2, Estreita), Estreita);
        assert_eq!(FaixaCaixa::de(35.6, Estreita), Media);
        assert_eq!(FaixaCaixa::de(34.7, Media), Media);
        assert_eq!(FaixaCaixa::de(34.4, Media), Estreita);
    }

    #[test]
    fn medida_avisa_so_quando_muda_de_faixa() {
        let mut medida = MedidaPainel::default();
        assert!(medida.atualizar(px(300.), px(15.)));
        assert_eq!(medida.nivel, LarguraPainel::Minimo);
        assert!(!medida.atualizar(px(302.), px(15.)));
        assert!(medida.atualizar(px(1200.), px(15.)));
        assert_eq!(medida.nivel, LarguraPainel::Largo);
        assert_eq!(medida.faixa, FaixaCaixa::Larga);
        // A 150% o rem é maior: os mesmos pixels valem menos rem.
        assert!(medida.atualizar(px(900.), px(22.5)));
        assert_eq!(medida.nivel, LarguraPainel::Largo);
        assert_eq!(medida.faixa, FaixaCaixa::Media);
    }
}
