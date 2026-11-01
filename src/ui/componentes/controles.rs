//! Controles: chip, botões fantasma, de perigo, primário e redondo, segmentado, contador,
//! campo com rótulo e campos de busca.

use std::{cell::RefCell, ops::RangeInclusive, rc::Rc};

use gpui_kit::component::{
    Disableable, Icon, Selectable, Sizable, Size, StyledExt as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    clipboard::Clipboard,
    input::{Input, InputState},
    kbd::Kbd,
    menu::DropdownMenu,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::base::{Marca, Simbolo, texto_arco_iris};
use super::estado::Selo;
use super::superficies::desenho_teclas;
use crate::tema::{Cores, Lucide, Tipo as _, esp, halo_foco, r, raio, sombra_recuo, tam};
use crate::ui::movimento::{self, Curva};

/// O Copiar do kit (troca para o check por 2 s) em 24, crescendo com o zoom: o tamanho só se
/// sabe ao desenhar, com a janela.
#[derive(IntoElement)]
pub struct BotaoCopiar(Clipboard);

impl BotaoCopiar {
    pub fn new(c: Clipboard) -> Self {
        BotaoCopiar(c)
    }
}

impl RenderOnce for BotaoCopiar {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        self.0.with_size(Size::Size(tam(window, 24.)))
    }
}

/// Escudo cheio do modo Manual (o check vazado por máscara).
const ESCUDO_CHEIO: &[u8] = include_bytes!("../../../assets/escudo-cheio.svg");

/// O quadrado cheio do Parar: um desenho, como os ícones, e não um canto da escala.
const QUADRADO_PARAR: &[u8] =
    br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" rx="2"/></svg>"#;

/// Ícone e cor de cada modo de permissão: a tabela única que a caixa, o menu do modo e as
/// Configurações leem. Verde quando nada muda sem você, violeta para automação, âmbar sem
/// perguntar, e vermelho sem perguntar fora de uma cópia isolada.
pub fn icone_do_modo(valor: &str, sem_copia: bool) -> (Icon, Hsla) {
    match valor {
        "manual" => (Icon::empty().data(ESCUDO_CHEIO), Cores::verde()),
        "plan" => (Lucide::Map.into(), Cores::verde()),
        "acceptEdits" => (Lucide::FilePen.into(), Cores::violeta()),
        "auto" => (Lucide::WandSparkles.into(), Cores::violeta()),
        _ if sem_copia => (Lucide::ShieldOff.into(), Cores::vermelho()),
        _ => (Lucide::ShieldOff.into(), Cores::ambar()),
    }
}

/// Altura do chip: Pequeno 24 (rótulo 12), Médio 28, Grande 32 (caixa).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TamanhoChip {
    Pequeno,
    Medio,
    #[default]
    Grande,
}

impl TamanhoChip {
    fn altura(self) -> f32 {
        match self {
            TamanhoChip::Pequeno => 24.,
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

    /// Selo depois do rótulo, antes da seta ("Arquivos ⁽³⁾"); some com o rótulo no `so_icone`.
    pub fn selo(mut self, s: Selo) -> Self {
        self.selo = Some(s);
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
            .rounded(raio::PILULA)
            .border_1()
            .border_color(transparent_black())
            .map(|b| if self.so_icone { b.w(r(altura)).px_0() } else { b.px(r(8.)) })
            .when_some(self.simbolo, |b, s| b.child(s.desenhar(14., window)))
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

/// Perigo calmo (Descartar em cada arquivo do diff): tênue em repouso e a tinta vermelha no
/// hover, para a lista não ficar vermelha.
pub fn variante_perigo_calma(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(Cores::tenue())
        .hover(Cores::perigo_hover())
        .active(Cores::vermelho().opacity(0.18))
}

/// Altura do botão: Linha 24 (ícone 14), Barra 28 (ícone 16), Controle 32.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TamanhoBotao {
    Linha,
    Barra,
    Controle,
}

impl TamanhoBotao {
    fn altura(self) -> f32 {
        match self {
            TamanhoBotao::Linha => 24.,
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
        .rounded(raio::PILULA)
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
        .when_some(rotulo, |b, rotulo| b.child(div().min_w_0().truncate().rotulo().child(rotulo)))
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

    /// O disco colorido da marca no lugar do ícone (o botão do Linear no cabeçalho da conversa).
    pub fn marca(mut self, m: Marca) -> Self {
        self.marca = Some(m);
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
            VarianteRedonda::Primario => (
                Cores::destaque(),
                rgb(0x3374f0).into(),
                rgb(0x2a64d6).into(),
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
        };
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
        let (alvo_icone, alvo_borda) =
            if self.vazio { (Cores::apagado(), Cores::borda()) } else { (cor, Cores::borda().opacity(0.)) };
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
            .rounded_full()
            .bg(fundo)
            .text_color(cor)
            .border_1()
            .border_color(borda)
            .map(|s| match (self.marca, self.icone) {
                (Some(marca), _) => s.child(marca.disco(lado_icone)),
                (None, Some(icone)) => s.child(icone.with_size(tam(window, lado_icone))),
                (None, None) => s.child(Icon::empty().data(QUADRADO_PARAR).with_size(tam(window, 10.))),
            })
            .when_some(pulso, |s, t| s.child(anel_do_pulso(cor_pulso, t)))
            .when(interativo, |s| {
                s.cursor_pointer()
                    .on_hover(ponteiro.ouvinte())
                    .active(move |s| s.bg(ativo))
                    .when_some(self.ao_clicar, |s, f| s.on_click(move |ev, window, cx| f(ev, window, cx)))
            })
            .when(self.focavel && interativo, |s| s.tab_index(0).focus_visible(|s| s.shadow(halo_foco())))
            .when(self.desabilitado, |s| s.opacity(0.45))
            // Com o popover dele aberto, a dica sairia por cima do popover.
            .when(!self.selecionado, |s| {
                s.tooltip(move |window, cx| {
                    Tooltip::new(dica.clone())
                        .when_some(acao.clone(), |t, (acao, contexto)| t.action(acao.as_ref(), contexto))
                        .build(window, cx)
                })
            })
            .when_some(self.selo, |s, selo| s.child(div().absolute().top(r(-3.)).right(r(-3.)).child(selo)))
    }
}

/// O anel do pulso em `t` (0 a 1): cresce 6 px e some. Serve ao `BotaoRedondo` e ao logo do
/// trilho no modo recolhido, que leva o selo do sino.
pub fn anel_do_pulso(cor: Hsla, t: f32) -> Div {
    div()
        .absolute()
        .inset_0()
        .rounded_full()
        .shadow(vec![BoxShadow::new(px(0.), px(0.), cor.opacity(0.4 * (1. - t))).spread_radius(px(6. * t))])
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

    /// A 0,45, sem hover e fora do clique e das setas, com o motivo na dica.
    pub fn desabilitado(mut self, motivo: impl Into<SharedString>) -> Self {
        self.desabilitado = true;
        self.dica = Some(motivo.into());
        self
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
            desabilitado: false,
            ao_mudar: Rc::new(ao_mudar),
        }
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
        let altura = match self.tamanho {
            Size::Small | Size::XSmall => 28.,
            Size::Large => 36.,
            _ => 32.,
        };
        let cor = self.cor;
        let cor_texto = if cor == Cores::destaque() { Cores::destaque_texto() } else { cor };
        let (atual, n) = (self.atual, self.opcoes.len());
        let ao_mudar = self.ao_mudar.clone();
        let ativo = !self.desabilitado;
        let no_flutuante = self.no_flutuante;
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
            movimento::valor_desde(chave, inicial, alvo, movimento::MOVER, Curva::Mover, window, cx)
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
                .rounded_full()
                .bg(Cores::selecao_de(cor))
                .border_1()
                .border_color(Cores::tinta_borda(cor))
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
            .rounded_full()
            .map(|s| if no_flutuante { s.bg(Cores::recuo_no_flutuante()) } else { s.bg(Cores::recuo()).shadow(sombra_recuo()) })
            .border_1()
            .border_color(transparent_black())
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
                    .px(if self.so_icone {
                        esp::E8
                    } else if self.compacto {
                        esp::E4
                    } else {
                        esp::E12
                    })
                    .rounded_full()
                    .when(self.largura_total, |s| s.flex_1())
                    .border_1()
                    .border_color(if escolhido && !tem_pilula { Cores::tinta_borda(cor) } else { transparent_black() })
                    .when(escolhido && !tem_pilula, |s| s.bg(Cores::selecao_de(cor)))
                    .text_size(r(13.))
                    .whitespace_nowrap()
                    .font_weight(if escolhido { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                    .text_color(if escolhido { cor_texto } else { Cores::tenue() })
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
                    .when_some(seg.icone, |s, icone| s.child(icone.with_size(tam(window, 14.))))
                    .when(!self.so_icone, |s| s.child(seg.rotulo.clone()))
                    .when_some(seg.ponto, |s, c| s.child(div().size(r(6.)).rounded_full().bg(c)))
                    .when_some(seg.dica.or_else(|| self.so_icone.then_some(seg.rotulo)), |s, dica| {
                        s.tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
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
    ao_mudar: Rc<dyn Fn(u32, &mut Window, &mut App)>,
}

impl Contador {
    pub fn new(
        id: impl Into<ElementId>,
        valor: u32,
        faixa: RangeInclusive<u32>,
        ao_mudar: impl Fn(u32, &mut Window, &mut App) + 'static,
    ) -> Self {
        Contador { id: id.into(), valor, faixa, ao_mudar: Rc::new(ao_mudar) }
    }
}

impl RenderOnce for Contador {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let (valor, min, max) = (self.valor, *self.faixa.start(), *self.faixa.end());
        let botao = |sufixo: &'static str, icone: Lucide, dica: &'static str, novo: Option<u32>| {
            let ao_mudar = self.ao_mudar.clone();
            BotaoRedondo::new(
                SharedString::from(format!("{}-{sufixo}", self.id)),
                Some(icone.into()),
                dica,
            )
            .variante(VarianteRedonda::Plano)
            .diametro(24.)
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
            .h(r(28.))
            .px(r(2.))
            .rounded_full()
            .bg(Cores::recuo())
            .child(botao("menos", Lucide::Minus, "Diminuir", (valor > min).then(|| valor - 1)))
            .child(
                div()
                    .min_w(r(24.))
                    .text_center()
                    .text_size(r(13.))
                    .font_semibold()
                    .text_color(Cores::texto())
                    .child(valor.to_string()),
            )
            .child(botao("mais", Lucide::Plus, "Aumentar", (valor < max).then(|| valor + 1)))
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
                        s.child(div().font_weight(FontWeight::NORMAL).text_color(Cores::tenue()).child("opcional"))
                    }),
            )
            .child(self.entrada)
            .when_some(embaixo, |s, (texto, cor)| s.child(div().metadado().text_color(cor).child(texto)))
    }
}

/// Busca de 32 em recuo, com a lupa e o atalho do keymap enquanto está vazia e sem foco. A lupa
/// fica a 8 da borda de fora (o contorno, 6 e a folga do desenho dela): numa peça de 8, cai na
/// guia de 16 da lista.
pub fn campo_de_busca(
    id: impl Into<ElementId>,
    estado: &Entity<InputState>,
    atalho: Option<Box<dyn Action>>,
    window: &Window,
    cx: &App,
) -> Stateful<Div> {
    let focado = estado.read(cx).focus_handle(cx).is_focused(window);
    let vazio = estado.read(cx).value().is_empty();
    let teclas = atalho
        .filter(|_| vazio && !focado)
        .and_then(|acao| Kbd::binding_for_action(acao.as_ref(), None, window))
        .map(|kbd| desenho_teclas(kbd.partes()));
    div()
        .id(id)
        .flex()
        .items_center()
        .h(r(32.))
        .pl(esp::E6)
        .pr(esp::E4)
        .rounded(raio::CONTROLE)
        .bg(Cores::recuo())
        .border_1()
        .border_color(transparent_black())
        .when(focado, |s| s.border_color(Cores::destaque_borda()).shadow(halo_foco()))
        .child(
            Input::new(estado)
                .appearance(false)
                .cleanable(true)
                .px_0()
                .prefix(
                    div()
                        .text_color(Cores::fantasma())
                        .child(Icon::new(Lucide::Search).with_size(tam(window, 14.))),
                )
                .text_size(r(14.)),
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
