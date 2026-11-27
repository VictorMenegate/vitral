//! Controles: chip, botões fantasma, de perigo, primário e redondo, segmentado, contador,
//! campo com rótulo e campos de busca.

use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};

use gpui_kit::component::{
    Disableable, Icon, Selectable, Sizable, Size,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    input::{Input, InputState},
    kbd::Kbd,
    menu::DropdownMenu,
    tooltip::{ManagedTooltipExt as _, Tooltip},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::base::{Marca, Simbolo, texto_arco_iris};
use super::estado::Selo;
use super::metal::{Encaixe, abertura};
use crate::tema::{Cores, Lucide, Tipo as _, aresta_de_poco, aresta_de_tecla, esp, halo_foco, linear, r, raio, tam};
use crate::ui::movimento::{self, Curva};

/// O Copiar: um botão de linha (24, ícone de 14 tênue, como os outros botões de linha), que troca
/// para o visto verde por 2 s. O do kit desenhava o ícone a 0,75 do botão (18) em `texto`, maior
/// e mais claro que o texto que ele copia (relatório de aparência, 16/11/2026, V12).
#[derive(IntoElement)]
pub struct BotaoCopiar {
    id: ElementId,
    valor: SharedString,
    /// O que o clique com Shift copia no lugar do valor (a tabela separada por tabulação).
    com_shift: Option<SharedString>,
    dica: SharedString,
}

impl BotaoCopiar {
    pub fn new(id: impl Into<ElementId>, valor: impl Into<SharedString>) -> Self {
        BotaoCopiar { id: id.into(), valor: valor.into(), com_shift: None, dica: tr!("Copiar") }
    }

    /// Outro texto para o clique com Shift.
    pub fn com_shift(mut self, valor: impl Into<SharedString>) -> Self {
        self.com_shift = Some(valor.into());
        self
    }

    pub fn dica(mut self, dica: impl Into<SharedString>) -> Self {
        self.dica = dica.into();
        self
    }
}

impl RenderOnce for BotaoCopiar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let copiado = window.use_keyed_state(self.id.clone(), cx, |_, _| false);
        let feito = *copiado.read(cx);
        let icone: Icon = if feito { Lucide::Check.into() } else { Lucide::Copy.into() };
        let (valor, com_shift) = (self.valor, self.com_shift);
        botao_icone(self.id, icone.when(feito, |i| i.text_color(Cores::verde())), self.dica, TamanhoBotao::Linha, window, cx).on_click(
            move |evento, _, cx| {
                cx.stop_propagation();
                let valor = com_shift.as_ref().filter(|_| evento.modifiers().shift).unwrap_or(&valor);
                cx.write_to_clipboard(ClipboardItem::new_string(valor.to_string()));
                copiado.update(cx, |c, cx| {
                    *c = true;
                    cx.notify();
                });
                let copiado = copiado.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(std::time::Duration::from_secs(2)).await;
                    _ = copiado.update(cx, |c, cx| {
                        *c = false;
                        cx.notify();
                    });
                })
                .detach();
            },
        )
    }
}

/// Escudo cheio do modo Manual (o check vazado por máscara).
const ESCUDO_CHEIO: &[u8] = include_bytes!("../../../assets/escudo-cheio.svg");

/// O quadrado cheio do Parar: um desenho, como os ícones, e não um canto da escala.
const QUADRADO_PARAR: &[u8] =
    br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" rx="2.6"/></svg>"#;

/// Ícone e cor de cada modo de permissão: a tabela única que a caixa, o menu do modo e as
/// Configurações leem. Verde quando nada muda sem você, violeta para automação, âmbar sem
/// perguntar, e vermelho sem perguntar fora de uma cópia isolada.
pub fn icone_do_modo(valor: &str) -> (Icon, Hsla) {
    match valor {
        // Cada modo com a sua cor (Linear, 25/11/2026: "melhora esse menu"; antes os comuns eram
        // neutros): o Manual neutro, editar em verde, planejar em azul, automático em violeta e o
        // "Sem perguntar" em âmbar.
        "manual" => (Icon::empty().data(ESCUDO_CHEIO), Cores::tenue()),
        "plan" => (Lucide::Map.into(), Cores::destaque_texto()),
        "acceptEdits" => (Lucide::FilePen.into(), Cores::verde()),
        "auto" => (Lucide::WandSparkles.into(), Cores::violeta()),
        _ => (Lucide::ShieldOff.into(), Cores::ambar()),
    }
}

/// Altura do chip: Pequeno 24 (rótulo 12), Médio 28, Grande 32, e o da caixa de mensagem, 26
/// com 5 dos lados e o símbolo de 16 (protótipo Linear, 25/11/2026).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TamanhoChip {
    Pequeno,
    Medio,
    #[default]
    Grande,
    Caixa,
}

impl TamanhoChip {
    fn altura(self) -> f32 {
        match self {
            TamanhoChip::Pequeno => 24.,
            TamanhoChip::Caixa => 26.,
            TamanhoChip::Medio => 28.,
            TamanhoChip::Grande => 32.,
        }
    }
}

/// Chip: símbolo, rótulo e seta, sem fundo em repouso. Construído sobre o `Button` do kit, então
/// serve de gatilho de `dropdown_menu` e de `Popover`.
pub struct Chip {
    id: ElementId,
    rotulo: SharedString,
    simbolo: Option<Simbolo>,
    tamanho: TamanhoChip,
    mudado: bool,
    cor_rotulo: Option<Hsla>,
    selo: Option<Selo>,
    seta: bool,
    aberto: bool,
    alternavel: Option<bool>,
    so_icone: bool,
    alerta: Option<Hsla>,
    arco_iris: bool,
    dica: Option<SharedString>,
    dica_acao: Option<(SharedString, Box<dyn Action>, Option<&'static str>)>,
    desabilitado: Option<SharedString>,
    ao_clicar: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

impl Chip {
    pub fn new(id: impl Into<ElementId>, rotulo: impl Into<SharedString>) -> Self {
        Chip {
            id: id.into(),
            rotulo: rotulo.into(),
            simbolo: None,
            tamanho: TamanhoChip::default(),
            mudado: true,
            cor_rotulo: None,
            selo: None,
            seta: false,
            aberto: false,
            alternavel: None,
            so_icone: false,
            alerta: None,
            arco_iris: false,
            dica: None,
            dica_acao: None,
            desabilitado: None,
            ao_clicar: None,
        }
    }

    pub fn simbolo(mut self, s: Simbolo) -> Self {
        self.simbolo = Some(s);
        self
    }

    pub fn tamanho(mut self, t: TamanhoChip) -> Self {
        self.tamanho = t;
        self
    }

    /// Valor fora do padrão da pasta: rótulo em `texto`; no padrão, `tenue`.
    pub fn mudado(mut self, sim: bool) -> Self {
        self.mudado = sim;
        self
    }

    /// Ciano no esforço.
    pub fn cor_rotulo(mut self, cor: Hsla) -> Self {
        self.cor_rotulo = Some(cor);
        self
    }

    /// ChevronDown de 12, tênue.
    pub fn seta(mut self, sim: bool) -> Self {
        self.seta = sim;
        self
    }

    /// Menu ou popover aberto: `pressionado()` com borda, e a seta para cima.
    pub fn aberto(mut self, sim: bool) -> Self {
        self.aberto = sim;
        self
    }

    /// Alternável (dias da rotina): marcado em `selecao()` com borda de destaque.
    pub fn alternavel(mut self, marcado: bool) -> Self {
        self.alternavel = Some(marcado);
        self
    }

    /// Só o símbolo (painel estreito); a dica passa a ser obrigatória.
    pub fn so_icone(mut self, sim: bool) -> Self {
        self.so_icone = sim;
        self
    }

    /// Tinta e borda na cor, rótulo na cor (Sem perguntar fora da cópia isolada).
    pub fn alerta(mut self, cor: Hsla) -> Self {
        self.alerta = Some(cor);
        self
    }

    /// Rótulo pintado letra a letra no arco-íris (o Ultrathink armado no esforço).
    pub fn arco_iris(mut self) -> Self {
        self.arco_iris = true;
        self
    }

    pub fn dica(mut self, d: impl Into<SharedString>) -> Self {
        self.dica = Some(d.into());
        self
    }

    /// Dica com o atalho tirado do keymap.
    pub fn dica_acao(
        mut self,
        d: impl Into<SharedString>,
        acao: Box<dyn Action>,
        contexto: Option<&'static str>,
    ) -> Self {
        self.dica_acao = Some((d.into(), acao, contexto));
        self
    }

    /// Desabilitado a 0,45, com o motivo na dica.
    pub fn desabilitado(mut self, motivo: impl Into<SharedString>) -> Self {
        self.desabilitado = Some(motivo.into());
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.ao_clicar = Some(Rc::new(f));
        self
    }

    pub fn construir(self, window: &Window, cx: &App) -> Button {
        let altura = self.tamanho.altura();
        let pequeno = self.tamanho == TamanhoChip::Pequeno;
        let da_caixa = self.tamanho == TamanhoChip::Caixa;
        let lado_simbolo = if da_caixa { 16. } else { 14. };
        let marcado = self.alternavel == Some(true);
        let cor_texto = self
            .alerta
            .or(self.cor_rotulo)
            .unwrap_or(if marcado {
                Cores::forte()
            } else if self.mudado {
                Cores::texto()
            } else {
                Cores::tenue()
            });
        let variante = ButtonCustomVariant::new(cx)
            .foreground(cor_texto)
            .hover(self.alerta.map_or_else(Cores::hover, |c| c.opacity(0.18)))
            .active(self.alerta.map_or_else(Cores::pressionado, |c| c.opacity(0.24)));
        let seta = self.seta && !self.so_icone;
        let mut botao = Button::new(self.id)
            .custom(variante)
            .h(r(altura))
            .rounded(raio::de(altura))
            .border_1()
            .border_color(transparent_black())
            .map(|b| match (self.so_icone, da_caixa) {
                (true, _) => b.w(r(altura)).px_0(),
                // A marca e o nome mais juntos (dono, 26/11/2026: "diminui o espaço entre o logo e
                // o Opus"; eram os 6 do protótipo).
                (false, true) => b.px(linear::CONTROLE_LADO).gap(esp::E4),
                (false, false) => b.px(r(8.)),
            })
            // A marca no chip é o glifo solto, sem o disco cheio: o disco coral a 24 px do escudo
            // âmbar era a mancha mais forte da faixa de caixas (relatório de aparência,
            // 16/11/2026, V9). Nas listas, popovers, na dupla e no anel de uso o disco fica.
            .when_some(self.simbolo, |b, s| {
                b.child(match s {
                    Simbolo::Marca(marca) => marca.glifo(lado_simbolo),
                    outro => outro.desenhar(lado_simbolo, window),
                })
            })
            .when(!self.so_icone, |b| {
                b.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(r(if pequeno { 12. } else { 13. }))
                        .line_height(r(18.))
                        .font_weight(FontWeight::MEDIUM)
                        .map(|s| {
                            if self.arco_iris {
                                s.child(texto_arco_iris(&self.rotulo))
                            } else {
                                s.child(self.rotulo)
                            }
                        }),
                )
            })
            .when_some(self.selo.filter(|_| !self.so_icone), |b, s| b.child(s))
            .when(seta, |b| {
                let seta = if self.aberto { Lucide::ChevronUp } else { Lucide::ChevronDown };
                b.child(
                    div()
                        .flex_none()
                        .text_color(Cores::tenue())
                        .child(Icon::new(seta).with_size(tam(window, 12.))),
                )
            })
            .when(self.aberto, |b| b.selected(true).border_color(Cores::borda()))
            .when_some(self.alerta, |b, c| b.bg(Cores::tinta(c)).border_color(Cores::tinta_borda(c)))
            .when_some(self.alternavel, |b, marcado| {
                if marcado {
                    b.bg(Cores::selecao()).border_color(Cores::destaque_borda())
                } else {
                    b.border_color(Cores::linha())
                }
            });
        if let Some((dica, acao, contexto)) = self.dica_acao {
            botao = botao.tooltip_with_action(dica, acao.as_ref(), contexto);
        } else if let Some(dica) = self.dica {
            botao = botao.tooltip(dica);
        }
        if let Some(motivo) = self.desabilitado {
            botao = botao.disabled(true).opacity(0.45).tooltip(motivo);
        }
        if let Some(f) = self.ao_clicar {
            botao = botao.on_click(move |ev, window, cx| f(ev, window, cx));
        }
        botao
    }
}

/// Chip que abre um menu ou popover: o kit marca o gatilho como aberto, e aí entra o desenho de
/// aberto do Chip (borda, seta para cima e sem dica). Os dois desenhos chegam prontos, porque o
/// Chip só se constrói com a janela.
#[derive(IntoElement)]
pub struct GatilhoChip {
    fechado: Button,
    aberto: Button,
    esta_aberto: bool,
}

impl GatilhoChip {
    pub fn new(fechado: Button, aberto: Button) -> Self {
        GatilhoChip { fechado, aberto, esta_aberto: false }
    }
}

impl Selectable for GatilhoChip {
    fn selected(mut self, selected: bool) -> Self {
        self.esta_aberto = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.esta_aberto
    }
}

impl Styled for GatilhoChip {
    fn style(&mut self) -> &mut StyleRefinement {
        self.fechado.style()
    }
}

impl InteractiveElement for GatilhoChip {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.fechado.interactivity()
    }
}

impl RenderOnce for GatilhoChip {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        if self.esta_aberto { self.aberto } else { self.fechado }
    }
}

impl DropdownMenu for GatilhoChip {}

/// Fantasma: transparente, `hover()` e `pressionado()`; alternância ligada (`.selected(true)`)
/// pinta o pressionado, neutro.
pub fn variante_fantasma(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(Cores::texto())
        .hover(Cores::hover())
        .active(Cores::pressionado())
}

/// Perigo: rótulo vermelho, tinta vermelha no hover.
pub fn variante_perigo(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(Cores::vermelho())
        .hover(Cores::perigo_hover())
        .active(Cores::vermelho().opacity(0.18))
}

/// Altura do botão: Linha 24 (ícone 14), Barra 28 (ícone 16), Controle 32.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TamanhoBotao {
    Linha,
    /// O controle da caixa de mensagem, 26 (protótipo Linear, 25/11/2026).
    Caixa,
    Barra,
    Controle,
}

impl TamanhoBotao {
    fn altura(self) -> f32 {
        match self {
            TamanhoBotao::Linha => 24.,
            TamanhoBotao::Caixa => 26.,
            TamanhoBotao::Barra => 28.,
            TamanhoBotao::Controle => 32.,
        }
    }

    fn icone(self) -> f32 {
        if self == TamanhoBotao::Linha { 14. } else { 16. }
    }
}

/// Medidas e conteúdo comuns: ícone (tênue ao lado de rótulo) e rótulo 13/500.
fn montar(
    botao: Button,
    icone: Option<Icon>,
    rotulo: Option<SharedString>,
    t: TamanhoBotao,
    tenue: bool,
    window: &Window,
) -> Button {
    let so_icone = rotulo.is_none();
    botao
        .h(r(t.altura()))
        .rounded(raio::de(t.altura()))
        .map(|b| {
            if so_icone {
                b.w(r(t.altura())).px_0()
            } else {
                b.px(if t == TamanhoBotao::Linha { esp::E8 } else { esp::E12 })
            }
        })
        .when_some(icone, |b, icone| {
            b.child(
                div()
                    .flex_none()
                    .when(tenue, |d| d.text_color(Cores::tenue()))
                    .child(icone.with_size(tam(window, t.icone()))),
            )
        })
        // O da caixa (e das Configurações) com o texto em 11,5 (protótipo Linear, 25/11/2026).
        .when_some(rotulo, |b, rotulo| {
            b.child(div().min_w_0().truncate().rotulo().when(t == TamanhoBotao::Caixa, |d| d.text_size(r(11.5))).child(rotulo))
        })
}

pub fn botao_fantasma(
    id: impl Into<ElementId>,
    icone: Option<Icon>,
    rotulo: impl Into<SharedString>,
    t: TamanhoBotao,
    window: &Window,
    cx: &App,
) -> Button {
    let botao = Button::new(id).custom(variante_fantasma(cx));
    montar(botao, icone, Some(rotulo.into()), t, true, window)
}

/// Só ícone, sempre com dica.
pub fn botao_icone(
    id: impl Into<ElementId>,
    icone: impl Into<Icon>,
    dica: impl Into<SharedString>,
    t: TamanhoBotao,
    window: &Window,
    cx: &App,
) -> Button {
    let botao = Button::new(id)
        .custom(variante_fantasma(cx).foreground(Cores::tenue()))
        .tooltip(dica);
    montar(botao, Some(icone.into()), None, t, false, window)
}

pub fn botao_perigo(
    id: impl Into<ElementId>,
    icone: Option<Icon>,
    rotulo: impl Into<SharedString>,
    t: TamanhoBotao,
    window: &Window,
    cx: &App,
) -> Button {
    let botao = Button::new(id).custom(variante_perigo(cx));
    montar(botao, icone, Some(rotulo.into()), t, false, window)
}

/// Botão principal com texto (`button.primary` do tema, #2f6fe8: branco a 4,6:1).
pub fn botao_primario(
    id: impl Into<ElementId>,
    icone: Option<Icon>,
    rotulo: impl Into<SharedString>,
    t: TamanhoBotao,
    window: &Window,
    _: &App,
) -> Button {
    montar(Button::new(id).primary(), icone, Some(rotulo.into()), t, false, window)
}

/// Claro: o "Permitir" do pedido de permissão (base AI Tool Approval). Fundo claro e texto
/// escuro, para o azul chapado não disputar com a moldura do pedido.
pub fn botao_claro(
    id: impl Into<ElementId>,
    icone: Option<Icon>,
    rotulo: impl Into<SharedString>,
    t: TamanhoBotao,
    window: &Window,
    cx: &App,
) -> Button {
    let variante = ButtonCustomVariant::new(cx)
        .color(Cores::claro())
        .foreground(Cores::sobre_cor())
        .hover(Cores::forte())
        .active(Cores::texto());
    montar(Button::new(id).custom(variante), icone, Some(rotulo.into()), t, false, window)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VarianteRedonda {
    /// Destaque com ícone branco (nova conversa).
    Primario,
    /// Claro com ícone escuro (enviar, parar).
    Claro,
    /// `hover()` em repouso (trilho, anexar).
    Neutro,
    /// Sem fundo em repouso.
    Plano,
    Perigo,
    /// Metal líquido debaixo de um véu, com o ícone branco: uma tecla em relevo (novo chat,
    /// enviar). O par do logo.
    Metal,
}

/// Botão redondo só com ícone (ou o quadrado de parar), sempre com dica. O fundo segue o ponteiro
/// em 120 ms (o hover suave dos controles soltos), a troca de `vazio` também, e `pulso` dá o anel
/// claro do clique.
#[derive(IntoElement)]
pub struct BotaoRedondo {
    id: ElementId,
    icone: Option<Icon>,
    marca: Option<Marca>,
    dica: SharedString,
    variante: VarianteRedonda,
    diametro: f32,
    vazio: bool,
    pulso: Option<u64>,
    pulso_cor: Option<Hsla>,
    selo: Option<Selo>,
    acao: Option<(Rc<dyn Action>, Option<&'static str>)>,
    focavel: bool,
    desabilitado: bool,
    selecionado: bool,
    ao_clicar: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

impl BotaoRedondo {
    /// `icone` em `None` desenha o quadrado de parar.
    pub fn new(id: impl Into<ElementId>, icone: Option<Icon>, dica: impl Into<SharedString>) -> Self {
        BotaoRedondo {
            id: id.into(),
            icone,
            marca: None,
            dica: dica.into(),
            variante: VarianteRedonda::Neutro,
            diametro: 32.,
            vazio: false,
            pulso: None,
            pulso_cor: None,
            selo: None,
            acao: None,
            focavel: false,
            desabilitado: false,
            selecionado: false,
            ao_clicar: None,
        }
    }

    pub fn variante(mut self, v: VarianteRedonda) -> Self {
        self.variante = v;
        self
    }

    /// 24, 28, 32 ou 36.
    pub fn diametro(mut self, d: f32) -> Self {
        self.diametro = d;
        self
    }

    /// Enviar sem texto: só a borda e o ícone apagado (mostra que existe). Ao ganhar texto, o
    /// fundo acende e o ícone escurece em 120 ms (o "pronto" do enviar).
    pub fn vazio(mut self, sim: bool) -> Self {
        self.vazio = sim;
        self
    }

    /// Um anel claro que cresce 6 px e some em 240 ms toda vez que `geracao` muda (a caixa sobe a
    /// geração a cada envio). Na montagem não pulsa.
    pub fn pulso(mut self, geracao: u64) -> Self {
        self.pulso = Some(geracao);
        self
    }

    /// A cor do anel do `pulso` (o sino pulsa em âmbar, ou vermelho com erro); o padrão é o claro.
    pub fn pulso_cor(mut self, cor: Hsla) -> Self {
        self.pulso_cor = Some(cor);
        self
    }

    /// No canto de cima à direita, sem aro.
    pub fn selo(mut self, s: Selo) -> Self {
        self.selo = Some(s);
        self
    }

    /// A dica ganha o atalho da ação, tirado do keymap.
    pub fn dica_acao(mut self, acao: Box<dyn Action>, contexto: Option<&'static str>) -> Self {
        self.acao = Some((Rc::from(acao), contexto));
        self
    }

    /// Entra no Tab, com o halo do foco; Enter e Espaço clicam (ou abrem o popover do gatilho).
    pub fn focavel(mut self, sim: bool) -> Self {
        self.focavel = sim;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.ao_clicar = Some(Rc::new(f));
        self
    }
}

impl Disableable for BotaoRedondo {
    fn disabled(mut self, disabled: bool) -> Self {
        self.desabilitado = disabled;
        self
    }
}

impl Selectable for BotaoRedondo {
    fn selected(mut self, selected: bool) -> Self {
        self.selecionado = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selecionado
    }
}

impl RenderOnce for BotaoRedondo {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (fundo, hover, ativo, cor) = match self.variante {
            // No Linear (25/11/2026), o azul-marinho do escolhido com o ícone branco; o ponteiro
            // clareia 6% (o novo chat, o enviar e o parar do protótipo).
            VarianteRedonda::Primario => (
                Cores::selecao(),
                Cores::selecao().blend(Cores::forte().opacity(0.06)),
                Cores::selecao().blend(Cores::forte().opacity(0.10)),
                Cores::forte(),
            ),
            VarianteRedonda::Claro => (Cores::claro(), Cores::forte(), Cores::texto(), Cores::sobre_cor()),
            VarianteRedonda::Neutro => (Cores::hover(), Cores::pressionado(), Cores::borda_forte(), Cores::texto()),
            VarianteRedonda::Plano => (transparent_black(), Cores::hover(), Cores::pressionado(), Cores::tenue()),
            VarianteRedonda::Perigo => (
                Cores::tinta(Cores::vermelho()),
                Cores::vermelho().opacity(0.22),
                Cores::vermelho().opacity(0.28),
                Cores::vermelho(),
            ),
            // O fundo é a abertura de metal (um filho, abaixo): aqui só a cor do ícone.
            VarianteRedonda::Metal => (
                transparent_black(),
                transparent_black(),
                transparent_black(),
                Cores::sobre_destaque(),
            ),
        };
        // Em metal: o véu fica como está (o contraste do ícone depende dele); o ponteiro acende
        // o fio da tecla e o aperto a escurece.
        let de_metal = self.variante == VarianteRedonda::Metal && !self.vazio;
        let interativo = !self.desabilitado;
        let ponteiro = movimento::Hover::de(movimento::chave(&self.id, "hover"), window, cx);
        let com_ponteiro = interativo && ponteiro.ativo(cx);
        let alvo_fundo = if self.selecionado {
            ativo
        } else if com_ponteiro {
            hover
        } else if self.vazio {
            transparent_black()
        } else {
            fundo
        };
        let (alvo_icone, alvo_borda) = if self.vazio {
            (Cores::apagado(), Cores::borda())
        } else if de_metal {
            (cor, Cores::forte().opacity(if com_ponteiro { 0.55 } else { 0.16 }))
        } else {
            (cor, Cores::borda().opacity(0.))
        };
        let ((fundo, cor), borda) = movimento::valor(
            movimento::chave(&self.id, "cores"),
            ((alvo_fundo, alvo_icone), alvo_borda),
            movimento::COR,
            Curva::Sair,
            window,
            cx,
        );
        let pulso = self.pulso.and_then(|geracao| {
            movimento::disparo(movimento::chave(&self.id, "pulso"), geracao, movimento::ENCHER, Curva::Sair, window, cx)
        });
        let cor_pulso = self.pulso_cor.unwrap_or_else(Cores::claro);
        let lado_icone = if self.diametro <= 24. { 14. } else { 16. };
        let (dica, acao) = (self.dica, self.acao);
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .size(r(self.diametro))
            .flex()
            .items_center()
            .justify_center()
            .rounded(raio::de(self.diametro))
            .bg(fundo)
            .text_color(cor)
            .border_1()
            .border_color(borda)
            // A tecla de metal: a chapa debaixo do véu, uma sombra curta que a solta da base e,
            // por cima, a aresta de luz (a sombra interna do próprio botão ficaria atrás da chapa).
            .when(de_metal, |s| {
                let cantos = Corners::all(raio::de(self.diametro));
                s.shadow(crate::tema::sombra_elevado())
                    // O véu é o azul do destaque, não preto: o botão continua sendo "o azul" da
                    // tela, com o metal vivo por baixo (a 60% o ícone branco fica acima de 3:1
                    // mesmo com a bolha mais clara atrás).
                    .child(abertura(Encaixe::Cobrir, Some(Cores::destaque().opacity(0.6)), cantos))
                    .child(div().absolute().inset_0().rounded(raio::de(self.diametro)).shadow(aresta_de_tecla()))
            })
            .map(|s| match (self.marca, self.icone) {
                (Some(marca), _) => s.child(marca.disco(lado_icone)),
                (None, Some(icone)) => s.child(icone.with_size(tam(window, lado_icone))),
                // Só o quadrado, sem anel girando em volta: quem diz que o chat trabalha é a
                // conversa e a linha de estado (dono, 13/11/2026: "já tem nos outros lugares").
                // 8 (Linear, 25/11/2026: o quadrado do protótipo).
                (None, None) => s.child(Icon::empty().data(QUADRADO_PARAR).with_size(tam(window, 8.))),
            })
            .when_some(pulso, |s, t| s.child(anel_do_pulso(cor_pulso, t)))
            .when(interativo, |s| {
                s.cursor_pointer()
                    .active(move |s| if de_metal { s.opacity(0.82) } else { s.bg(ativo) })
                    .when_some(self.ao_clicar, |s, f| s.on_click(move |ev, window, cx| f(ev, window, cx)))
            })
            .when(self.focavel && interativo, |s| s.tab_index(0).focus_visible(|s| s.shadow(halo_foco())))
            .when(self.desabilitado, |s| s.opacity(0.45))
            // O hover acende o botão; a dica é a gerenciada, a mesma dos botões do kit (espera só
            // na primeira). Com o popover dele aberto, a dica sairia por cima do popover.
            .map(|s| {
                let ouvinte = interativo.then(|| ponteiro.ouvinte());
                if self.selecionado {
                    return match ouvinte {
                        Some(ouvinte) => s.on_hover(ouvinte),
                        None => s,
                    };
                }
                s.dica_com_hover(
                    move |dentro, window, cx| {
                        if let Some(ouvinte) = &ouvinte {
                            ouvinte(dentro, window, cx);
                        }
                    },
                    move |window, cx| {
                        Tooltip::new(dica.clone())
                            .when_some(acao.clone(), |t, (acao, contexto)| t.action(acao.as_ref(), contexto))
                            .build(window, cx)
                    },
                )
            })
            .when_some(self.selo, |s, selo| s.child(div().absolute().top(r(-2.)).right(r(-2.)).child(selo)))
    }
}

/// O anel do pulso em `t` (0 a 1): cresce 6 px e some. Serve ao `BotaoRedondo` e ao logo do
/// trilho no modo recolhido, que leva o selo do sino.
pub fn anel_do_pulso(cor: Hsla, t: f32) -> Div {
    div()
        .absolute()
        .inset_0()
        .rounded(raio::de(36.))
        .shadow(vec![BoxShadow::new(px(0.), px(0.), cor.opacity(0.4 * (1. - t))).spread_radius(px(6. * t))])
}

/// O rótulo do segmento com a largura do semibold reservada: o escolhido engrossa a letra sem
/// mudar de largura, então os vizinhos e a pílula que desliza não andam ao trocar. A cópia em
/// semibold, invisível, dá a largura; o rótulo de verdade fica por cima, centrado, no peso que o
/// segmento pôs.
fn rotulo_de_largura_fixa(rotulo: SharedString) -> Div {
    div()
        .relative()
        .child(div().invisible().font_weight(FontWeight::SEMIBOLD).child(rotulo.clone()))
        .child(div().absolute().top_0().left_0().size_full().flex().justify_center().child(rotulo))
}

/// Uma opção do controle segmentado.
pub struct Segmento {
    pub rotulo: SharedString,
    pub icone: Option<Icon>,
    pub dica: Option<SharedString>,
    pub ponto: Option<Hsla>,
    pub desabilitado: bool,
}

impl Segmento {
    pub fn new(rotulo: impl Into<SharedString>) -> Self {
        Segmento { rotulo: rotulo.into(), icone: None, dica: None, ponto: None, desabilitado: false }
    }

    pub fn icone(mut self, i: impl Into<Icon>) -> Self {
        self.icone = Some(i.into());
        self
    }

    pub fn dica(mut self, d: impl Into<SharedString>) -> Self {
        self.dica = Some(d.into());
        self
    }

    /// Ponto de 6 depois do rótulo (há algo naquela vista).
    pub fn ponto(mut self, cor: Hsla) -> Self {
        self.ponto = Some(cor);
        self
    }
}

impl From<&'static str> for Segmento {
    fn from(rotulo: &'static str) -> Self {
        Segmento::new(rotulo)
    }
}

impl From<SharedString> for Segmento {
    fn from(rotulo: SharedString) -> Self {
        Segmento::new(rotulo)
    }
}

impl From<String> for Segmento {
    fn from(rotulo: String) -> Self {
        Segmento::new(rotulo)
    }
}

/// Escolha entre poucas opções, em pílula, com a cor por instância (azul nas abas, ciano no
/// esforço). Um ponto de Tab; ←/→, Home e End aplicam na hora. A pílula do escolhido desliza até
/// o novo em 180 ms.
#[derive(IntoElement)]
pub struct ControleSegmentado {
    id: ElementId,
    opcoes: Vec<Segmento>,
    atual: usize,
    cor: Hsla,
    tamanho: Size,
    largura_total: bool,
    so_icone: bool,
    compacto: bool,
    no_flutuante: bool,
    solto: bool,
    chapado: bool,
    desabilitado: bool,
    ao_mudar: Rc<dyn Fn(usize, &mut Window, &mut App)>,
}

impl ControleSegmentado {
    pub fn new(
        id: impl Into<ElementId>,
        opcoes: impl IntoIterator<Item = impl Into<Segmento>>,
        atual: usize,
        ao_mudar: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        ControleSegmentado {
            id: id.into(),
            opcoes: opcoes.into_iter().map(Into::into).collect(),
            atual,
            cor: Cores::destaque(),
            tamanho: Size::Medium,
            largura_total: false,
            so_icone: false,
            compacto: false,
            no_flutuante: false,
            solto: false,
            chapado: false,
            desabilitado: false,
            ao_mudar: Rc::new(ao_mudar),
        }
    }

    /// O das Configurações no jeito do protótipo Linear (25/11/2026): 26 de altura, no preto, sem
    /// fio nem poço, as opções em 11,5 e a escolhida no azul do escolhido.
    pub fn chapado(mut self, sim: bool) -> Self {
        self.chapado = sim;
        self
    }

    /// O segmentado do alto da lista: o trilho é um poço sem fio (o mesmo desenho da busca) e a
    /// escolhida é uma ficha de metal em relevo que desliza de uma aba para a outra.
    pub fn solto(mut self, sim: bool) -> Self {
        self.solto = sim;
        self
    }

    pub fn cor(mut self, cor: Hsla) -> Self {
        self.cor = cor;
        self
    }

    pub fn largura_total(mut self, sim: bool) -> Self {
        self.largura_total = sim;
        self
    }

    pub fn so_icone(mut self, sim: bool) -> Self {
        self.so_icone = sim;
        self
    }

    /// Segmentos com 4 de margem no lugar de 12: muitas opções num espaço curto (os seis níveis
    /// do esforço no popover de 400).
    pub fn compacto(mut self, sim: bool) -> Self {
        self.compacto = sim;
        self
    }

    /// Dentro de um popover (quase preto): o trilho vira um claro leve, porque o recuo escuro
    /// some ali.
    pub fn no_flutuante(mut self, sim: bool) -> Self {
        self.no_flutuante = sim;
        self
    }
}

impl Sizable for ControleSegmentado {
    fn with_size(mut self, size: impl Into<Size>) -> Self {
        self.tamanho = size.into();
        self
    }
}

impl Disableable for ControleSegmentado {
    fn disabled(mut self, disabled: bool) -> Self {
        self.desabilitado = disabled;
        self
    }
}

/// Para onde a tecla leva o segmentado de `n` opções a partir de `atual`.
fn segmento_pela_tecla(tecla: &str, atual: usize, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let novo = match tecla {
        "left" => atual.checked_sub(1)?,
        "right" => (atual + 1).min(n - 1),
        "home" => 0,
        "end" => n - 1,
        _ => return None,
    };
    (novo != atual).then_some(novo)
}

/// Onde cada segmento estava no quadro anterior, relativo ao controle, e o rem de então: com a
/// medida, a pílula do escolhido desliza até o novo; sem ela (o primeiro quadro, ou o zoom que
/// acabou de mudar), o escolhido se pinta sozinho, seco.
#[derive(Default)]
struct MedidasDoSegmentado {
    rem: Pixels,
    caixa: Option<Bounds<Pixels>>,
    segmentos: Vec<Option<Bounds<Pixels>>>,
    /// O escolhido no quadro anterior: no primeiro clique depois de abrir, a pílula sai dele.
    anterior: Option<usize>,
}

impl MedidasDoSegmentado {
    /// Esquerda e largura do segmento `i` (em px), se a medida vale para este quadro.
    fn lugar(&self, i: usize, n: usize, rem: Pixels) -> Option<(f32, f32)> {
        if self.rem != rem || self.segmentos.len() != n {
            return None;
        }
        // As duas medidas são do lado de dentro do traço de 1 do segmento; a pílula cobre o traço.
        let (caixa, segmento) = (self.caixa?, (*self.segmentos.get(i)?)?);
        Some(((segmento.origin.x - caixa.origin.x).as_f32() - 1., segmento.size.width.as_f32() + 2.))
    }
}

/// Um `canvas` que guarda os limites do pai a cada quadro, sem pedir redesenho.
fn medir(guardar: impl FnOnce(Bounds<Pixels>) + 'static) -> impl IntoElement {
    canvas(move |limites, _, _| guardar(limites), |_, _, _, _| {}).absolute().top_0().left_0().size_full()
}

impl RenderOnce for ControleSegmentado {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let foco = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle().tab_stop(true))
            .read(cx)
            .clone();
        let chapado = self.chapado;
        let altura = match self.tamanho {
            _ if chapado => 26.,
            Size::Small | Size::XSmall => 28.,
            Size::Large => 36.,
            // Uma altura sob medida (as abas do alto da lista têm a do logo).
            Size::Size(altura) => altura.as_f32(),
            _ => 32.,
        };
        let cor = self.cor;
        // O segmentado comum é neutro (um vidro mais claro com um fio em volta); só o de outra
        // cor (um estado, um aviso) pinta a escolha.
        let neutro = cor == Cores::destaque();
        // O escolhido do neutro (e do solto) é a ficha de metal, a mesma do alto da lista, do
        // esforço e das listas de escolha; a pílula tingida fica só para o de outra cor. Antes o
        // neutro sem `solto` (as Configurações, o boas-vindas) saía numa pílula cinza lisa, mais
        // clara que o trilho: o mesmo componente marcava o escolhido de dois jeitos opostos
        // (relatório de design, 16/11/2026). `solto` agora só tira o fio do trilho.
        let em_ficha = neutro || self.solto;
        let cor_texto = if neutro { Cores::forte() } else { cor };
        let (fundo_escolhido, borda_escolhida) =
            if neutro { (Cores::selecao(), Cores::borda_forte()) } else { (Cores::selecao_de(cor), Cores::tinta_borda(cor)) };
        let (atual, n) = (self.atual, self.opcoes.len());
        let ao_mudar = self.ao_mudar.clone();
        let ativo = !self.desabilitado;
        let no_flutuante = self.no_flutuante;
        let solto = self.solto;
        let bloqueados: Vec<bool> = self.opcoes.iter().map(|s| s.desabilitado).collect();
        let rem = window.rem_size();
        let medidas = window
            .use_keyed_state(movimento::chave(&self.id, "medidas"), cx, |_, _| {
                Rc::new(RefCell::new(MedidasDoSegmentado::default()))
            })
            .read(cx)
            .clone();
        let (alvo, inicial) = {
            let m = medidas.borrow();
            (m.lugar(atual, n, rem), m.lugar(m.anterior.unwrap_or(atual), n, rem))
        };
        let lugar = alvo.map(|alvo| {
            let chave = movimento::chave(&self.id, "pilula");
            let inicial = inicial.unwrap_or(alvo);
            movimento::valor_desde(chave, inicial, alvo, movimento::DESLIZAR, Curva::Mover, window, cx)
        });
        {
            let mut m = medidas.borrow_mut();
            m.rem = rem;
            m.segmentos.resize(n, None);
            m.anterior = Some(atual);
        }
        let pilula = lugar.map(|(esquerda, largura)| {
            div()
                .absolute()
                .top(esp::E2)
                .bottom(esp::E2)
                .left(px(esquerda))
                .w(px(largura))
                .rounded(raio::de(altura) - px(2.))
                .map(|s| {
                    if em_ficha {
                        // A escolhida vai na ficha do escolhido: o metal com a aresta de tecla.
                        s.child(super::superficies::ficha_do_escolhido(raio::de(altura) - px(2.)))
                    } else {
                        // A pílula escolhida fica em relevo (tecla) dentro do trilho cavado (poço).
                        s.border_1().border_color(borda_escolhida).bg(fundo_escolhido).shadow(aresta_de_tecla())
                    }
                })
        });
        let tem_pilula = pilula.is_some();
        let caixa = medidas.clone();
        div()
            .id(self.id)
            .track_focus(&foco)
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(esp::E2)
            .p(esp::E2)
            .h(r(altura))
            .rounded(raio::de(altura))
            .map(|s| {
                if chapado {
                    s.bg(Cores::preto())
                } else if no_flutuante {
                    s.bg(Cores::recuo_no_flutuante())
                } else {
                    s.bg(Cores::recuo()).shadow(aresta_de_poco())
                }
            })
            .border_1()
            .border_color(if solto || chapado { transparent_black() } else { Cores::borda() })
            .focus_visible(|s| s.border_color(Cores::destaque()).shadow(halo_foco()))
            .when(self.largura_total, |s| s.w_full())
            .when(ativo, |s| {
                s.on_key_down(move |evento, window, cx| {
                    if let Some(novo) = segmento_pela_tecla(evento.keystroke.key.as_str(), atual, n)
                        && !bloqueados[novo]
                    {
                        cx.stop_propagation();
                        ao_mudar(novo, window, cx);
                    }
                })
            })
            .when(!ativo, |s| s.opacity(0.45))
            .child(medir(move |limites| caixa.borrow_mut().caixa = Some(limites)))
            .children(pilula)
            .children(self.opcoes.into_iter().enumerate().map(|(i, seg)| {
                let escolhido = i == atual;
                let ao_mudar = self.ao_mudar.clone();
                let medidas = medidas.clone();
                div()
                    .id(("segmento", i))
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(esp::E6)
                    .h_full()
                    .px(if self.so_icone || chapado {
                        esp::E8
                    } else if self.compacto {
                        esp::E4
                    } else {
                        esp::E12
                    })
                    .rounded(raio::de(altura) - px(2.))
                    .when(self.largura_total, |s| s.flex_1())
                    .border_1()
                    .border_color(if escolhido && !tem_pilula && !em_ficha { borda_escolhida } else { transparent_black() })
                    .when(escolhido && !tem_pilula && !em_ficha, |s| s.bg(fundo_escolhido).shadow(aresta_de_tecla()))
                    .text_size(r(if chapado { 11.5 } else { 13. }))
                    .whitespace_nowrap()
                    .font_weight(if escolhido && !chapado { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                    .text_color(if escolhido { cor_texto } else if chapado { Cores::texto_da_fala() } else { Cores::tenue() })
                    .when(seg.desabilitado, |s| s.opacity(0.45))
                    .when(!escolhido && ativo && !seg.desabilitado, |s| {
                        s.cursor_pointer()
                            .hover(|s| s.bg(Cores::hover()).text_color(Cores::texto()))
                            .on_click(move |_, window, cx| ao_mudar(i, window, cx))
                    })
                    .child(medir(move |limites| {
                        if let Some(lugar) = medidas.borrow_mut().segmentos.get_mut(i) {
                            *lugar = Some(limites);
                        }
                    }))
                    // No primeiro quadro (sem medida, sem a pílula que desliza), a ficha no próprio
                    // segmento.
                    .when(escolhido && !tem_pilula && em_ficha, |s| {
                        s.child(super::superficies::ficha_do_escolhido(raio::de(altura) - px(2.)))
                    })
                    .when_some(seg.icone, |s, icone| s.child(icone.with_size(tam(window, 14.))))
                    .when(!self.so_icone, |s| s.child(rotulo_de_largura_fixa(seg.rotulo.clone())))
                    .when_some(seg.ponto, |s, c| s.child(div().size(r(6.)).rounded_full().bg(c)))
                    .when_some(seg.dica.or_else(|| self.so_icone.then_some(seg.rotulo)), |s, dica| {
                        s.dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                    })
            }))
    }
}

/// Número com − e + numa pílula de recuo (Ao mesmo tempo, dias).
#[derive(IntoElement)]
pub struct Contador {
    id: ElementId,
    valor: u32,
    faixa: RangeInclusive<u32>,
    sufixo: Option<&'static str>,
    ao_mudar: Rc<dyn Fn(u32, &mut Window, &mut App)>,
}

impl Contador {
    /// A unidade colada ao número ("%").
    pub fn sufixo(mut self, s: &'static str) -> Self {
        self.sufixo = Some(s);
        self
    }

    pub fn new(
        id: impl Into<ElementId>,
        valor: u32,
        faixa: RangeInclusive<u32>,
        ao_mudar: impl Fn(u32, &mut Window, &mut App) + 'static,
    ) -> Self {
        Contador { id: id.into(), valor, faixa, sufixo: None, ao_mudar: Rc::new(ao_mudar) }
    }
}

impl RenderOnce for Contador {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let (valor, min, max) = (self.valor, *self.faixa.start(), *self.faixa.end());
        let botao = |sufixo: &'static str, icone: Lucide, dica: SharedString, novo: Option<u32>| {
            let ao_mudar = self.ao_mudar.clone();
            BotaoRedondo::new(
                SharedString::from(format!("{}-{sufixo}", self.id)),
                Some(icone.into()),
                dica,
            )
            .variante(VarianteRedonda::Plano)
            .diametro(22.)
            .disabled(novo.is_none())
            .on_click(move |_, window, cx| {
                if let Some(novo) = novo {
                    ao_mudar(novo, window, cx);
                }
            })
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(r(2.))
            .h(r(26.))
            .px(r(2.))
            .rounded(raio::de(26.))
            // No preto, como os outros controles das Configurações (protótipo Linear, 25/11/2026).
            .bg(Cores::preto())
            .child(botao("menos", Lucide::Minus, tr!("Diminuir"), (valor > min).then(|| valor - 1)))
            .child(
                div()
                    // Com unidade, a largura do maior número: o − e o + não andam de 90% para 100%.
                    .min_w(r(if self.sufixo.is_some() { 44. } else { 36. }))
                    .text_center()
                    .text_size(r(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .tabular()
                    .text_color(Cores::texto())
                    .child(format!("{valor}{}", self.sufixo.unwrap_or_default())),
            )
            .child(botao("mais", Lucide::Plus, tr!("Aumentar"), (valor < max).then(|| valor + 1)))
    }
}

/// Campo de formulário: rótulo 13/500 em cima, a entrada e a ajuda (ou o erro) embaixo.
#[derive(IntoElement)]
pub struct Campo {
    rotulo: SharedString,
    entrada: AnyElement,
    ajuda: Option<SharedString>,
    erro: Option<SharedString>,
    opcional: bool,
}

impl Campo {
    pub fn new(rotulo: impl Into<SharedString>, entrada: impl IntoElement) -> Self {
        Campo {
            rotulo: rotulo.into(),
            entrada: entrada.into_any_element(),
            ajuda: None,
            erro: None,
            opcional: false,
        }
    }

    pub fn ajuda(mut self, t: impl Into<SharedString>) -> Self {
        self.ajuda = Some(t.into());
        self
    }

    /// O erro toma o lugar da ajuda, em vermelho.
    pub fn erro(mut self, t: Option<SharedString>) -> Self {
        self.erro = t;
        self
    }

    pub fn opcional(mut self) -> Self {
        self.opcional = true;
        self
    }
}

impl RenderOnce for Campo {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let embaixo = match (self.erro, self.ajuda) {
            (Some(erro), _) => Some((erro, Cores::vermelho())),
            (None, Some(ajuda)) => Some((ajuda, Cores::tenue())),
            (None, None) => None,
        };
        div()
            .flex()
            .flex_col()
            .gap(r(6.))
            .child(
                div()
                    .flex()
                    .gap(r(6.))
                    .rotulo()
                    .text_color(Cores::texto())
                    .child(self.rotulo)
                    .when(self.opcional, |s| {
                        s.child(div().font_weight(FontWeight::NORMAL).text_color(Cores::tenue()).child(tr!("opcional")))
                    }),
            )
            .child(self.entrada)
            .when_some(embaixo, |s, (texto, cor)| s.child(div().metadado().text_color(cor).child(texto)))
    }
}

/// Busca de 32 no preto, com a lupa e o atalho do keymap enquanto está vazia e sem foco.
pub fn campo_de_busca(
    id: impl Into<ElementId>,
    estado: &Entity<InputState>,
    atalho: Option<Box<dyn Action>>,
    window: &Window,
    cx: &App,
) -> Stateful<Div> {
    campo_de_busca_de(id, estado, atalho, 32., window, cx)
}

/// O campo de busca com a altura dada (o do alto da lista tem a do logo e a do "+").
pub fn campo_de_busca_de(
    id: impl Into<ElementId>,
    estado: &Entity<InputState>,
    atalho: Option<Box<dyn Action>>,
    altura: f32,
    window: &Window,
    cx: &App,
) -> Stateful<Div> {
    campo_de_busca_com(id, estado, atalho, altura, (linear::BUSCA_ESQUERDA, linear::BUSCA_DIREITA), esp::E6, 13.5, window, cx)
}

/// A busca de 28 da aba Arquivos da direita: 9 dos lados, 7 entre a lupa e o texto, em 12,5.
pub fn campo_de_busca_pequeno(id: impl Into<ElementId>, estado: &Entity<InputState>, window: &Window, cx: &App) -> Stateful<Div> {
    campo_de_busca_com(id, estado, None, 28., (linear::ITEM_LADO, linear::ITEM_LADO), linear::PASSO_VAO, 12.5, window, cx)
}

#[allow(clippy::too_many_arguments)]
fn campo_de_busca_com(
    id: impl Into<ElementId>,
    estado: &Entity<InputState>,
    atalho: Option<Box<dyn Action>>,
    altura: f32,
    (esquerda, direita): (Rems, Rems),
    vao: Rems,
    letra: f32,
    window: &Window,
    cx: &App,
) -> Stateful<Div> {
    let focado = estado.read(cx).focus_handle(cx).is_focused(window);
    let vazio = estado.read(cx).value().is_empty();
    // O atalho em texto simples ("Ctrl K"), sem as teclas em caixa, como no Linear (25/11/2026).
    let teclas = atalho
        .filter(|_| vazio && !focado)
        .and_then(|acao| Kbd::binding_for_action(acao.as_ref(), None, window))
        .map(|kbd| {
            div()
                .flex_none()
                .text_size(r(11.))
                .text_color(Cores::tenue_da_lista())
                .child(kbd.partes().join(" "))
        });
    div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .h(r(altura))
        .pl(esquerda)
        .pr(direita)
        // Chapada no preto, sem fio nem poço (protótipo Linear, 25/11/2026).
        .rounded(raio::de(altura))
        .bg(Cores::preto())
        .child(
            Input::new(estado)
                .appearance(false)
                .cleanable(true)
                .px_0()
                .gap(vao)
                .prefix(
                    div()
                        .text_color(Cores::tenue())
                        .child(Icon::new(Lucide::Search).with_size(tam(window, 14.))),
                )
                .text_size(r(letra)),
        )
        .children(teclas)
}

/// Busca plana de 44, sem fundo, com uma linha embaixo (dentro de lista de escolha).
pub fn campo_de_busca_plano(
    id: impl Into<ElementId>,
    estado: &Entity<InputState>,
    window: &Window,
    _: &App,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .h(r(44.))
        .px(r(16.))
        .border_b_1()
        .border_color(Cores::borda())
        .child(
            Input::new(estado)
                .appearance(false)
                .cleanable(true)
                .prefix(
                    div()
                        .text_color(Cores::tenue())
                        .child(Icon::new(Lucide::Search).with_size(tam(window, 16.))),
                )
                .text_size(r(14.)),
        )
}

// A moldura dupla (base AI Tool Approval): o pedido que espera você é um cartão de raio 16 dentro
// de uma moldura de raio 20, com 4 px de folga. Serve aos pedidos de permissão e ao plano.

/// O que a moldura de fora diz.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EstadoDaMoldura {
    /// Aberto, esperando a sua decisão.
    Esperando,
    /// O comando perigoso: vermelho, nunca metal.
    Perigo,
    /// Recolhido (há outro pedido aberto).
    Recolhida,
}

/// A moldura de fora, de raio 20 e 4 de folga; o miolo (`miolo_da_moldura`) vai dentro.
pub fn moldura_dupla(estado: EstadoDaMoldura) -> Div {
    let moldura = div().flex().flex_col().w_full().p(esp::E4).rounded(raio::PECA);
    match estado {
        // Aberta e não perigosa: o preto da moldura da caixa, com o fio azul do que espera você.
        // Era o metal vivo, que brilhava e mexia sem parar ao lado da conversa (dono, 27/11/2026:
        // "melhora o design ... e o de permitir também"; o protótipo Linear não tem relevo).
        EstadoDaMoldura::Esperando => moldura.bg(Cores::preto()).border_1().border_color(Cores::tinta_solida(Cores::destaque(), 0.6)),
        EstadoDaMoldura::Perigo => moldura.bg(Cores::tinta_solida(Cores::vermelho(), 0.35)),
        EstadoDaMoldura::Recolhida => moldura.bg(Cores::preto()).border_1().border_color(Cores::borda()),
    }
}

/// O miolo da moldura dupla: opaco, de raio 16 (é ele que deixa só o aro de fora à vista).
pub fn miolo_da_moldura() -> Div {
    div().flex().flex_col().w_full().rounded(raio::peca() - px(4.)).bg(Cores::cartao_elevado())
}

/// O azulejo de 28 com o ícone de 14 do cabeçalho de um pedido (a ferramenta, o plano).
pub fn azulejo(icone: Lucide, cor: Hsla) -> Div {
    div()
        .flex_none()
        .size(r(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(raio::CONTROLE)
        .bg(Cores::hover())
        .child(Icon::new(icone).size(r(14.)).text_color(cor))
}

/// "Segure para permitir": o botão do pedido perigoso. Só confirma depois de `SEGURAR` (900 ms)
/// apertado, enchendo da esquerda para a direita; soltar ou sair com o ponteiro cancela, e um
/// clique curto não faz nada. Pelo teclado, o primeiro Enter arma ("Enter de novo para
/// permitir", 3 s) e o segundo confirma.
#[derive(IntoElement)]
pub struct BotaoDeSegurar {
    id: ElementId,
    rotulo: SharedString,
    ao_completar: Option<Rc<dyn Fn(&mut App)>>,
    tamanho: TamanhoBotao,
    largo: bool,
}

impl BotaoDeSegurar {
    pub fn new(id: impl Into<ElementId>, rotulo: impl Into<SharedString>) -> Self {
        BotaoDeSegurar { id: id.into(), rotulo: rotulo.into(), ao_completar: None, tamanho: TamanhoBotao::Controle, largo: false }
    }

    pub fn ao_completar(mut self, f: impl Fn(&mut App) + 'static) -> Self {
        self.ao_completar = Some(Rc::new(f));
        self
    }

    pub fn tamanho(mut self, t: TamanhoBotao) -> Self {
        self.tamanho = t;
        self
    }

    /// Ocupa a largura (o pedido na largura mínima).
    pub fn largo(mut self) -> Self {
        self.largo = true;
        self
    }
}

impl RenderOnce for BotaoDeSegurar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let segurar = movimento::Segurar::de(movimento::chave(&self.id, "segurar"), window, cx);
        let enchido = segurar.progresso(window, cx);
        let rotulo = if segurar.armado(cx) { tr!("Enter de novo para permitir") } else { self.rotulo };
        let completar = self.ao_completar;
        let (apertar, soltar, soltar_fora, sair, teclar) =
            (segurar.clone(), segurar.clone(), segurar.clone(), segurar.clone(), segurar);
        let (ao_apertar, ao_teclar) = (completar.clone(), completar);
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .when(self.largo, |s| s.w_full())
            .h(r(self.tamanho.altura()))
            .px(esp::E12)
            .flex()
            .items_center()
            .justify_center()
            .rounded(raio::PILULA)
            .bg(Cores::tinta(Cores::vermelho()))
            .border_1()
            .border_color(Cores::tinta_borda(Cores::vermelho()))
            .text_color(Cores::vermelho())
            .cursor_pointer()
            .tab_index(0)
            .focus_visible(|s| s.shadow(halo_foco()))
            .when_some(enchido, |s, t| {
                s.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .h_full()
                        .w(relative(t))
                        .rounded(raio::PILULA)
                        .bg(Cores::vermelho().opacity(0.35)),
                )
            })
            .child(div().relative().min_w_0().truncate().rotulo().child(rotulo))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                if let Some(f) = ao_apertar.clone() {
                    apertar.apertar(move |cx| f(cx), cx);
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| soltar.soltar(cx))
            .on_mouse_up_out(MouseButton::Left, move |_, _, cx| soltar_fora.soltar(cx))
            .on_key_down(move |evento, _, cx| {
                // Só o Enter solto e sozinho: a repetição da tecla segurada mandaria o segundo
                // Enter (o que confirma) sem o usuário soltar o primeiro, e Ctrl ou Shift com
                // Enter são outros gestos.
                if evento.keystroke.key == "enter" && !evento.is_held && !evento.keystroke.modifiers.modified() {
                    cx.stop_propagation();
                    if let Some(f) = ao_teclar.clone() {
                        teclar.enter(move |cx| f(cx), cx);
                    }
                }
            })
            // Sair de cima solta o aperto; a dica é a gerenciada.
            .dica_com_hover(
                move |dentro, _, cx| {
                    if !*dentro {
                        sair.soltar(cx);
                    }
                },
                |window, cx| Tooltip::new(tr!("Segure por 1 segundo (ou Enter duas vezes)")).build(window, cx),
            )
    }
}

#[cfg(test)]
mod testes {
    use super::segmento_pela_tecla;

    #[test]
    fn segmentado_anda_pelas_setas_e_pelas_pontas() {
        assert_eq!(segmento_pela_tecla("right", 1, 5), Some(2));
        assert_eq!(segmento_pela_tecla("left", 1, 5), Some(0));
        assert_eq!(segmento_pela_tecla("left", 0, 5), None);
        assert_eq!(segmento_pela_tecla("right", 4, 5), None);
        assert_eq!(segmento_pela_tecla("home", 3, 5), Some(0));
        assert_eq!(segmento_pela_tecla("end", 3, 5), Some(4));
        assert_eq!(segmento_pela_tecla("x", 3, 5), None);
    }
}
