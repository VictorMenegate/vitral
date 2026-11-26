//! Peças dos painéis: aba, prévia de arrasto e as faixas de largura do painel e da caixa.

use std::rc::Rc;

use gpui_kit::component::{Icon, Sizable as _, tooltip::{ManagedTooltipExt as _, Tooltip}};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::base::Simbolo;
use super::estado::{Sinal, numero_da_aba};
use super::superficies::Flutuante as _;
use crate::tema::{Cores, Lucide, Tipo as _, esp, halo_foco, linear, r, raio, tam};

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
}

/// Aba de painel ou de arquivo: 32 em pílula, título 13/500, até 400; encolhe com o painel.
pub struct Aba {
    id: ElementId,
    titulo: SharedString,
    simbolo: Option<Simbolo>,
    numero: Option<(usize, bool)>,
    /// O número ocupa o lugar mas não aparece ("Só segurando Ctrl"): a aba não muda de largura.
    numero_oculto: bool,
    detalhe: Option<(SharedString, bool)>,
    tom: TomAba,
    estado: Option<Sinal>,
    fechar: Option<(SharedString, bool, Rc<dyn Fn(&mut Window, &mut App)>)>,
    arrastando: bool,
    foco: Option<FocusHandle>,
    fim: Option<AnyElement>,
    quadrada: bool,
    /// O estado do chat no contorno da aba (e a chave da vista do contorno que respira).
    borda: Option<(Sinal, String)>,
}

impl Aba {
    pub fn new(id: impl Into<ElementId>, titulo: impl Into<SharedString>) -> Self {
        Aba {
            id: id.into(),
            titulo: titulo.into(),
            simbolo: None,
            numero: None,
            numero_oculto: false,
            detalhe: None,
            tom: TomAba::default(),
            estado: None,
            fechar: None,
            arrastando: false,
            foco: None,
            fim: None,
            quadrada: false,
            borda: None,
        }
    }

    /// O estado do chat no fio preto da aba (dono, 18/11/2026: "colocar algo de quando tá rodando e
    /// quando acaba algo, faz na borda preta"): trabalhando, o fio azul respira; terminou e você não
    /// leu, verde; esperando você, âmbar; erro, vermelho. `chave` nomeia a vista do contorno que
    /// respira (uma por aba).
    pub fn borda(mut self, sinal: Option<Sinal>, chave: impl Into<String>) -> Self {
        self.borda = sinal.map(|s| (s, chave.into()));
        self
    }


    /// Só o símbolo, num quadrado da altura da aba e centrado, sem o lugar do × nem o número: a
    /// aba de fundo que não cabe com o título (o clique do meio ainda fecha; a dica dá o nome).
    /// Com o lugar do × reservado ela ficava uma pílula de 80 com o ícone num canto (o dono viu,
    /// 16/11/2026).
    pub fn quadrada(mut self, sim: bool) -> Self {
        self.quadrada = sim;
        self
    }

    pub fn simbolo(mut self, s: Simbolo) -> Self {
        self.simbolo = Some(s);
        self
    }

    /// O número fica invisível, guardando o lugar.
    pub fn numero_oculto(mut self, oculto: bool) -> Self {
        self.numero_oculto = oculto;
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

    /// Algo depois do título e do número, antes do × (o lápis do rascunho).
    pub fn fim(mut self, e: impl IntoElement) -> Self {
        self.fim = Some(e.into_any_element());
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
        // abas moram na mesma casca preta (`casca_da_aba`); a selecionada é o campo dela,
        // com o material do campo da caixa de mensagem (dono, 13/11/2026: "tem que ficar mais
        // parecido com a que digito aqui"); as outras usam o mesmo contorno e um tom mais discreto.
        let (fundo, texto, borda): (Background, Hsla, Hsla) = match self.tom {
            // O contorno é o fio preto de 1 px, o da casca de antes, numa linha só (dono,
            // 16/11/2026: "diminui as bordas").
            // Linear (25/11/2026): a aba aberta na cor cheia do escolhido (a mesma do seu balão), em
            // todo painel; as outras no quase preto da moldura; sem contorno.
            TomAba::Unica | TomAba::Ativa | TomAba::Inativa => (Cores::selecao().into(), Cores::forte(), transparent_black()),
            TomAba::Selecionada => (Cores::cartao_elevado().into(), Cores::forte(), transparent_black()),
            TomAba::Normal => (Cores::preto().into(), Cores::tenue(), transparent_black()),
        };
        // O estado no contorno (dono, 18/11/2026: "faz na borda preta", "tenta não fazer muito
        // chamativo" e, com o fio de 1 px apagado, "dá pra ver nada"): um fio de 2 px na cor do
        // estado por cima do preto, com um véu de 6% da mesma cor no miolo, sem halo. Trabalhando,
        // o azul respira devagar, numa vista própria; os outros ficam acesos, parados.
        let cor_da_borda = Sinal::cor;
        let respira = self.borda.map(|(s, chave)| ContornoDeEstado {
            chave,
            raio: raio::de(30.),
            cor: cor_da_borda(s),
            respira: s == Sinal::Rodando,
        });
        let focada = self.foco.as_ref().is_some_and(|f| f.is_focused(window));
        let sempre = self.fechar.as_ref().is_some_and(|(_, sempre, _)| *sempre);
        let fechar_a_vista = sempre || (self.fechar.is_some() && focada);
        let tem_estado = self.estado.is_some();
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
                .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
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
        // O × que só aparece no hover não guarda lugar: ele surge por cima do fim da aba (do número
        // do painel ou do fim do título), num disco do fundo da aba, e a aba não muda de tamanho
        // (dono, 16/11/2026: "esse espaço pro x não ter, e ficar quando passar o mouse em cima,
        // sem crescer o tamanho"). O lugar só fica reservado quando o × está sempre à vista ou
        // quando há o ponto do estado, que divide o lugar com ele.
        // Com o foco (o clique na aba já o dá), o × aparece por cima como no hover, sem guardar
        // lugar (dono, 19/11/2026: "quando segura o × fica do lado igual tinha tirado já").
        let reserva = !self.quadrada && (sempre || tem_estado);
        // A aba só com o símbolo não tem o × por cima: ele cobriria o símbolo inteiro, e o clique
        // para trocar de aba fecharia ela. Fecha no clique do meio ou depois de ativa.
        let so_simbolo = self.quadrada;
        let (fechar_no_lugar, fechar_por_cima) = match fechar {
            Some(f) if !reserva && !so_simbolo => (None, Some(f)),
            outro => (outro, None),
        };
        let fim = (reserva && (fechar_no_lugar.is_some() || ponto.is_some()))
            .then(|| div().relative().flex_none().size(r(20.)).children(ponto).children(fechar_no_lugar));
        let fundo_do_x = fundo.clone();
        let por_cima = fechar_por_cima.map(|f| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right(esp::E4)
                .flex()
                .items_center()
                // Focada, à vista; não na aba que está sendo arrastada, que fica apagada.
                .when(!focada || self.arrastando, |s| s.invisible().group_hover(GRUPO_DA_ABA, |s| s.visible()))
                .child(div().rounded(raio::de(24.)).bg(fundo_do_x).p(esp::E2).child(f))
        });
        // Ícone solto em 16; o círculo do chat em 18.
        let tem_simbolo = self.simbolo.is_some();
        let com_numero = self.numero.is_some() && !self.quadrada;
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
            // O símbolo junto do título (6), 8 antes dele e 12 depois do título: o desenho do ícone
            // já tem um respiro dentro, e com 12 dos dois lados a folga da esquerda parecia maior
            // (dono, 17/11/2026: "melhorar o espaçamento do nome do chat com o ícone" e "o espaço
            // da esquerda do ícone tá muito grande").
            .gap(linear::ABA_VAO)
            .h(r(30.))
            .max_w(r(400.))
            .min_w_0()
            // Sem o ícone (as abas de chat, desde 18/11/2026), 12 dos dois lados: com 8 na esquerda
            // o título ficava torto na pílula (dono, 23/11/2026: "esses textos dos chats não estão
            // centralizados").
            // Protótipo Linear (25/11/2026): 11 dos lados, 6 na direita com o número.
            .map(|s| match (self.quadrada, tem_simbolo) {
                (true, _) => s.w(r(30.)).flex_none().justify_center(),
                (false, true) => s.pl(esp::E8).pr(linear::ABA_LADO),
                (false, false) => s.px(linear::ABA_LADO),
            })
            .when(com_numero, |s| s.pr(esp::E6))
            .relative()
            .rounded(raio::de(30.))
            .border_1()
            .border_color(borda)
            .text_color(texto)
            .map(|s| match self.tom {
                TomAba::Inativa | TomAba::Normal => s.hover(|s| s.text_color(Cores::texto())),
                TomAba::Unica | TomAba::Ativa | TomAba::Selecionada => s,
            })
            // Com o ponteiro em cima, o contorno vira o fio claro e o miolo clareia um pouco: um
            // anel liso e igual na volta toda. O aro de metal de 2 px que acendia aqui ficava
            // claro de um lado e escuro do outro (dono, 16/11/2026: "o contorno de quando tá com o
            // mouse em cima ainda tá bem ruim").
            // No Linear o ponteiro só clareia o texto: o fundo e o contorno ficam.
            .when(!self.arrastando, |s| s.child(div().absolute().inset_0().rounded(raio::de(30.)).bg(fundo)))
            .children(respira.filter(|_| !self.arrastando))
            .when(self.arrastando, |s| s.opacity(0.45).border_dashed().border_color(Cores::borda_forte()))
            .when_some(self.foco, |s, foco| {
                s.track_focus(&foco)
                    .tab_index(0)
                    .focus_visible(|s| s.border_color(Cores::destaque()).shadow(halo_foco()))
            })
            .children(simbolo)
            // O título com reticências quando não cabe (Linear, 25/11/2026: no protótipo o dono
            // pediu "Pesage…" no lugar do corte seco no meio da letra, que ele tinha escolhido em
            // 17/11).
            .when(!so_simbolo, |s| s.child(div().min_w_0().truncate().rotulo().child(self.titulo)))
            .children(self.detalhe.filter(|_| !so_simbolo).map(|(t, mono)| {
                div()
                    .flex_none()
                    .max_w(r(160.))
                    .truncate()
                    .text_color(Cores::tenue())
                    .map(|d| if mono { d.mono_pequeno() } else { d.metadado() })
                    .child(t)
            }))
            // O número do painel fica à direita, antes do × (dono, 13/11/2026).
            // Colado no título: 4 em vez dos 6 do vão da aba (dono, 17/11/2026: "diminui o espaço
            // até o número").
            .children(self.numero.filter(|_| !self.quadrada).map(|(n, _)| div().flex_none().when(self.numero_oculto, |d| d.opacity(0.)).child(numero_da_aba(n))))
            .children(self.fim)
            .children(fim)
            .children(por_cima)
            .when_some(fechar_meio, |s, f| {
                s.on_aux_click(move |ev: &ClickEvent, window, cx| {
                    if ev.is_middle_click() {
                        f(window, cx);
                    }
                })
            })
    }
}

/// A casca da aba: o preto da caixa de mensagem em volta dela virou só o fio de 1 px, que é a
/// própria borda da aba, em preto. O dono viu a casca com 4 e com 2 de folga e pediu mais fina; em
/// 16/11/2026 pediu o mesmo desenho em todos os painéis e abas e, no mesmo dia, "diminui as
/// bordas": a folga de 1 e a borda da casca saíram, e o fio da aba deixou de vir dobrado (eram uns
/// 3 px de contorno escuro). Fica a peça que agrupa a aba na faixa; o tom do miolo distingue a
/// selecionada.
pub fn casca_da_aba() -> Div {
    div().flex().items_center().min_w_0().rounded(raio::de(30.))
}

/// Aba em miniatura que segue o ponteiro ao arrastar: opaca, em pílula como a aba, até 280. O chat
/// vai sem símbolo, como a aba e a linha da lista (dono, 19/11/2026: "quando arrasta aparece o
/// logo").
pub struct PreviaArrasto {
    pub titulo: SharedString,
    pub simbolo: Option<Simbolo>,
}

impl Render for PreviaArrasto {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let simbolo = self.simbolo.as_ref().and_then(Simbolo::copia).map(|s| s.desenhar(16., window));
        div()
            .flex()
            .items_center()
            .gap(r(8.))
            .h(r(32.))
            .max_w(r(280.))
            .px(esp::E12)
            .flutuante(cx)
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

/// A largura nominal da caixa, que escolhe a faixa dela: a do painel (até 860 px) menos a margem
/// de 16 de cada lado. A caixa de verdade para antes, na coluna de leitura (675 px); a faixa larga
/// continua valendo no painel largo, e os controles dela cabem nessa largura (conferido na
/// bancada, 18/11/2026).
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

/// O estado do chat no contorno da aba: um fio de 2 px na cor do estado, em cima do fio preto, com
/// um véu de 6% da mesma cor no miolo (por baixo do título). Trabalhando, ele respira devagar (de
/// 0,5 a 1 em 2,4 s), numa vista própria (a batida redesenha só ele, e não a janela); os outros, e
/// todos com o movimento reduzido, ficam acesos, parados.
#[derive(IntoElement)]
struct ContornoDeEstado {
    chave: String,
    raio: Pixels,
    cor: Hsla,
    respira: bool,
}

impl RenderOnce for ContornoDeEstado {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (raio, cor) = (self.raio, self.cor);
        // Em cima do fio de 1 px da aba (o absoluto conta de dentro dele).
        let lugar = div().absolute().top(px(-1.)).left(px(-1.)).right(px(-1.)).bottom(px(-1.));
        let fio = move |alfa: f32| {
            div().size_full().rounded(raio).border_2().border_color(cor.opacity(alfa)).bg(cor.opacity(0.06 * alfa))
        };
        if !self.respira || cx.reduce_motion() {
            return lugar.child(fio(0.9));
        }
        let versao = u64::from(Rgba::from(cor).b.to_bits()) ^ u64::from(raio.as_f32().to_bits()) << 32;
        lugar.child(super::vista_viva::em_vista_propria(self.chave, StyleRefinement::default().size_full(), versao, cx, move |window, cx| {
            let fase = crate::ui::movimento::laco_a(window, cx, crate::ui::movimento::QPS_DO_LACO);
            let t = (fase / 2.4).fract() as f32;
            let alfa = 0.5 + 0.5 * (0.5 + 0.5 * (t * std::f32::consts::TAU).cos());
            fio(alfa).into_any_element()
        }))
    }
}
