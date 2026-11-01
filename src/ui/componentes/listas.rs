//! Listas: cabeçalho de seção, cartão de lista, linha e grupo de opções e a lista de escolha
//! com busca.

use std::rc::Rc;

use gpui_kit::component::{
    Disableable as _, Icon, IndexPath, Sizable as _,
    command::{Command, CommandGroup, CommandItem, CommandState},
    scroll::{Scrollbar, ScrollbarHandle},
    switch::Switch,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::base::{Simbolo, texto_arco_iris};
use super::estado::{Selo, Sinal};
use super::superficies::EstadoVazio;
use crate::tema::{Cores, Lucide, Tipo as _, altura, dentro_da_borda, esp, r, raio, tam};

/// Rótulo de seção (24, 12/600 tênue), com contagem, ação e recolher: 16 de respiro em cima (4
/// na primeira, ou o `respiro`) e 4 embaixo. O texto fica no `recuo` (8 por padrão: na lista, com a peça de 8,
/// cai na guia de 16; na Equipe, `.recuo(12.)` põe na guia de 24) e o chevron, à esquerda dele.
#[derive(IntoElement)]
pub struct CabecalhoDeSecao {
    titulo: SharedString,
    contagem: Option<usize>,
    cor: Option<Hsla>,
    acao: Option<AnyElement>,
    recolhivel: Option<(bool, Rc<dyn Fn(&mut Window, &mut App)>)>,
    primeira: bool,
    respiro: Option<f32>,
    recuo: f32,
}

impl CabecalhoDeSecao {
    pub fn new(t: impl Into<SharedString>) -> Self {
        CabecalhoDeSecao {
            titulo: t.into(),
            contagem: None,
            cor: None,
            acao: None,
            recolhivel: None,
            primeira: false,
            respiro: None,
            recuo: 8.,
        }
    }

    pub fn contagem(mut self, n: usize) -> Self {
        self.contagem = Some(n);
        self
    }

    /// Contagem na tinta da cor, com o número na cor.
    pub fn tingida(mut self, cor: Hsla) -> Self {
        self.cor = Some(cor);
        self
    }

    pub fn acao(mut self, e: impl IntoElement) -> Self {
        self.acao = Some(e.into_any_element());
        self
    }

    /// O clique alterna; o chevron fica à esquerda do texto, e o hover clareia os dois.
    pub fn recolhivel(mut self, aberto: bool, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.recolhivel = Some((aberto, Rc::new(f)));
        self
    }

    /// A primeira seção visível: 4 em cima.
    pub fn primeira(mut self, sim: bool) -> Self {
        self.primeira = sim;
        self
    }

    /// O espaço em cima, em px a 100%, no lugar dos 16 (ou dos 4 da primeira), para quem mede o
    /// ritmo por fora (a Equipe, na tinta).
    pub fn respiro(mut self, px: f32) -> Self {
        self.respiro = Some(px);
        self
    }

    /// Da borda do cabeçalho até o texto, em px a 100% (8 na lista, 12 na Equipe).
    pub fn recuo(mut self, px: f32) -> Self {
        self.recuo = px;
        self
    }
}

/// Chevron de 12 encostado no texto (o desenho do Lucide já tem folga dos dois lados).
const CHEVRON: f32 = 12.;

impl RenderOnce for CabecalhoDeSecao {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        const GRUPO: &str = "cabecalho-de-secao";
        let contagem = self.contagem.map(|n| {
            let selo = Selo::contagem(n);
            match self.cor {
                Some(cor) => selo.tingida(cor),
                None => selo,
            }
        });
        let clicavel = self.recolhivel.is_some();
        let seta = self.recolhivel.as_ref().map(|(aberto, _)| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(r(self.recuo - CHEVRON))
                .flex()
                .items_center()
                .group_hover(GRUPO, |s| s.text_color(Cores::texto()))
                .child(
                    Icon::new(if *aberto { Lucide::ChevronDown } else { Lucide::ChevronRight })
                        .with_size(tam(window, CHEVRON)),
                )
        });
        let linha = div()
            .id(SharedString::from(format!("secao-{}", self.titulo)))
            .group(GRUPO)
            .relative()
            .flex()
            .items_center()
            .gap(esp::E6)
            .h(r(24.))
            .mt(match self.respiro {
                Some(px) => r(px),
                None if self.primeira => esp::E4,
                None => esp::ANTES_DE_SECAO,
            })
            .mb(esp::E4)
            .pl(r(self.recuo))
            .pr(esp::E8)
            .rotulo_secao()
            .children(seta)
            .child(div().when(clicavel, |d| d.group_hover(GRUPO, |s| s.text_color(Cores::texto()))).child(self.titulo))
            .children(contagem)
            .child(div().flex_1())
            .children(self.acao);
        match self.recolhivel {
            Some((_, f)) => linha.cursor_pointer().on_click(move |_, window, cx| f(window, cx)).into_any_element(),
            None => linha.into_any_element(),
        }
    }
}

/// Barra de rolagem fina, como a do GNOME: 4 px quase colada na borda e 6 com o ponteiro em
/// cima. Vai como último filho de uma área que chega até a borda da peça e guarda 6 de margem à
/// direita do conteúdo, e assim, parada, não cobre os cartões.
pub fn barra_de_rolagem<H: ScrollbarHandle + Clone>(id: impl Into<ElementId>, rolagem: &H) -> Div {
    div().absolute().inset_0().child(
        Scrollbar::vertical(rolagem)
            .id(id)
            .viewport_from_layout()
            .styles(|s| {
                s.track(|t| t.width(px(8.)))
                    .thumb(|t| t.width(px(4.)).inset(px(1.)))
                    .thumb_hover(|t| t.width(px(6.)).inset(px(1.)))
                    .thumb_active(|t| t.width(px(6.)).inset(px(1.)))
            }),
    )
}

/// Grupo de hover do `CartaoDeLista`, para o que aparece com o ponteiro no cartão.
pub const GRUPO_DO_CARTAO: &str = "cartao-de-lista";

/// Cartão de uma lista (conversas, trabalhadores, arquivos, contas, rotinas): 52 com duas
/// linhas ou 32 com uma. O conteúdo fica a 8 da borda (o símbolo de uma lista com a peça de 8
/// cai na guia de 16) e o título, a 12 do símbolo de 30 (8 do de 16, numa linha). As ações
/// aparecem no hover no lugar da meta, que tem largura reservada.
///
/// No nível 1 (o trabalhador embaixo do coordenador), a coluna do símbolo tem os mesmos 30 do
/// avatar do pai, com o ponto de estado no meio (o `sinal`, quando há; senão o `simbolo`, que
/// na lista é um ponto de 8 na cor do estado) sobre o traço da árvore, que passa no centro dela e
/// do avatar do pai, e a palavra do estado na `dica_do_ponto`. O título cai na coluna do título
/// do pai. O traço supõe 2 entre os cartões (`esp::E2`).
#[derive(IntoElement)]
pub struct CartaoDeLista {
    id: ElementId,
    titulo: SharedString,
    titulo_rico: Option<StyledText>,
    simbolo: Option<Simbolo>,
    sinal: Option<Sinal>,
    subtitulo: Option<AnyElement>,
    meta: Option<AnyElement>,
    meta_inferior: Option<AnyElement>,
    acoes: Vec<AnyElement>,
    acao_fixa: Option<AnyElement>,
    selecionado: bool,
    aberto_em_outro: bool,
    nao_lido: bool,
    realce: bool,
    nivel: u8,
    ramo: (bool, bool),
    dica_do_ponto: Option<SharedString>,
    uma_linha: bool,
    titulo_no_meio: bool,
    ao_clicar: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

impl CartaoDeLista {
    pub fn new(id: impl Into<ElementId>, titulo: impl Into<SharedString>) -> Self {
        CartaoDeLista {
            id: id.into(),
            titulo: titulo.into(),
            titulo_rico: None,
            simbolo: None,
            sinal: None,
            subtitulo: None,
            meta: None,
            meta_inferior: None,
            acoes: Vec::new(),
            acao_fixa: None,
            selecionado: false,
            aberto_em_outro: false,
            nao_lido: false,
            realce: false,
            nivel: 0,
            ramo: (false, false),
            dica_do_ponto: None,
            uma_linha: false,
            titulo_no_meio: false,
            ao_clicar: None,
        }
    }

    pub fn simbolo(mut self, s: Simbolo) -> Self {
        self.simbolo = Some(s);
        self
    }

    /// Ponto no canto do símbolo.
    pub fn sinal(mut self, s: Option<Sinal>) -> Self {
        self.sinal = s;
        self
    }

    /// No nível 1, a dica do ponto sobre o traço (a palavra do estado e a explicação).
    pub fn dica_do_ponto(mut self, d: impl Into<SharedString>) -> Self {
        self.dica_do_ponto = Some(d.into());
        self
    }

    /// Título com o trecho da busca em forte.
    pub fn titulo_rico(mut self, t: StyledText) -> Self {
        self.titulo_rico = Some(t);
        self
    }

    pub fn subtitulo(mut self, e: impl IntoElement) -> Self {
        self.subtitulo = Some(e.into_any_element());
        self
    }

    /// Linha 1 da coluna da direita (horário, pílula compacta).
    pub fn meta(mut self, e: impl IntoElement) -> Self {
        self.meta = Some(e.into_any_element());
        self
    }

    /// Linha 2 da coluna da direita: no máximo 2 itens.
    pub fn meta_inferior(mut self, e: impl IntoElement) -> Self {
        self.meta_inferior = Some(e.into_any_element());
        self
    }

    /// Até 2 botões de 24 (ou o gatilho de um menu, como o "…"), no hover, no foco e no realce,
    /// no lugar da meta.
    pub fn acao(mut self, b: impl IntoElement) -> Self {
        if self.acoes.len() < 2 {
            self.acoes.push(b.into_any_element());
        }
        self
    }

    /// Sempre visível (o interruptor da rotina).
    pub fn acao_fixa(mut self, e: impl IntoElement) -> Self {
        self.acao_fixa = Some(e.into_any_element());
        self
    }

    pub fn selecionado(mut self, s: bool) -> Self {
        self.selecionado = s;
        self
    }

    /// Aberto em outro painel: a barra a 40%.
    pub fn aberto_em_outro(mut self, s: bool) -> Self {
        self.aberto_em_outro = s;
        self
    }

    pub fn nao_lido(mut self, n: bool) -> Self {
        self.nao_lido = n;
        self
    }

    /// Realce do teclado: mostra as ações como no hover.
    pub fn realce(mut self, r: bool) -> Self {
        self.realce = r;
        self
    }

    /// 1: trabalhador, na árvore do coordenador.
    pub fn nivel(mut self, n: u8) -> Self {
        self.nivel = n;
        self
    }

    /// Onde o cartão de nível 1 fica no ramo: o primeiro leva o traço até o avatar do pai (o
    /// cartão de 52 logo acima) e o último termina o traço no ponto.
    pub fn ramo(mut self, primeiro: bool, ultimo: bool) -> Self {
        self.ramo = (primeiro, ultimo);
        self
    }

    /// 32 de altura, raio de controle.
    pub fn uma_linha(mut self) -> Self {
        self.uma_linha = true;
        self
    }

    /// Título cortado no meio (e-mail, caminho): o começo e o fim é que dizem qual é.
    pub fn titulo_no_meio(mut self) -> Self {
        self.titulo_no_meio = true;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.ao_clicar = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for CartaoDeLista {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        const GRUPO: &str = GRUPO_DO_CARTAO;
        let arvore = self.nivel > 0;
        let uma_linha = self.uma_linha || arvore;
        let (altura, raio_cartao, lado_simbolo) =
            if uma_linha { (32., raio::CONTROLE, 16.) } else { (52., raio::CARTAO, 30.) };
        let vao = if uma_linha && !arvore { esp::E8 } else { esp::E12 };
        let forte = self.nao_lido || self.selecionado;
        let cor_barra = Cores::destaque();
        let barra = (self.selecionado || self.aberto_em_outro).then(|| {
            div()
                .absolute()
                .left(r(2.))
                .top(r(if uma_linha { 7. } else { 10. }))
                .bottom(r(if uma_linha { 7. } else { 10. }))
                .w(px(3.))
                .rounded_full()
                .bg(if self.selecionado { cor_barra } else { cor_barra.opacity(0.4) })
        });
        let simbolo = if arvore {
            Some(coluna_da_arvore(self.simbolo, self.sinal, self.dica_do_ponto, self.ramo, window))
        } else {
            self.simbolo.map(|s| {
                div()
                    .relative()
                    .flex_none()
                    .child(s.desenhar(lado_simbolo, window))
                    .when_some(self.sinal, |d, sinal| {
                        d.child(div().absolute().top(r(-3.)).right(r(-3.)).child(sinal.ponto(8.)))
                    })
                    .into_any_element()
            })
        };
        // No nível 1, 13/500 como o rotulo_tarefa.
        let titulo = div()
            .min_w_0()
            .map(|d| {
                if self.titulo_no_meio {
                    d.overflow_hidden().whitespace_nowrap().text_ellipsis_middle()
                } else {
                    d.truncate()
                }
            })
            .text_size(r(if self.nivel > 0 { 13. } else { 14. }))
            .line_height(r(20.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(if forte { Cores::forte() } else { Cores::texto() })
            .map(|d| match self.titulo_rico {
                Some(rico) => d.child(rico),
                None => d.child(self.titulo.clone()),
            });
        let tem_acoes = !self.acoes.is_empty();
        // O lugar das ações: 24 cada, com 4 entre elas.
        let lugar_acoes = match self.acoes.len() {
            0 => 0.,
            n => n as f32 * 28. - 4.,
        };
        let mostra_acoes = self.realce;
        // O lugar das ações só fica reservado com elas na tela (hover, realce): fora disso, o
        // título vai até a meta. O id deixa o hover do grupo valer já no layout.
        let direita = (self.meta.is_some() || self.meta_inferior.is_some() || tem_acoes).then(|| {
            div()
                .id("lugar-acoes")
                .relative()
                .flex_none()
                .min_w(r(if mostra_acoes { lugar_acoes } else { 0. }))
                .when(tem_acoes && !mostra_acoes, |d| d.group_hover(GRUPO, |s| s.min_w(r(lugar_acoes))))
                .h_full()
                .child(
                    div()
                        .h_full()
                        .flex()
                        .flex_col()
                        .items_end()
                        .justify_center()
                        .gap(r(2.))
                        .metadado()
                        .text_color(Cores::apagado())
                        .group_hover(GRUPO, |s| s.text_color(Cores::tenue()))
                        .when(tem_acoes, |d| d.group_hover(GRUPO, |s| s.invisible()))
                        .when(tem_acoes && mostra_acoes, |d| d.invisible())
                        .children(self.meta)
                        .children(self.meta_inferior),
                )
                .when(tem_acoes, |d| {
                    // Opacidade, e não `invisible`: o menu aberto do "…" continua na tela quando o
                    // ponteiro sai do cartão.
                    d.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .flex()
                            .items_center()
                            .gap(r(4.))
                            .when(!mostra_acoes, |d| d.opacity(0.).group_hover(GRUPO, |s| s.opacity(1.)))
                            .children(self.acoes),
                    )
                })
        });
        let (fundo, hover) =
            if self.selecionado { (Cores::selecao(), Cores::selecao()) } else { (transparent_black(), Cores::hover()) };
        div()
            .id(self.id)
            .group(GRUPO)
            .relative()
            .flex()
            .items_center()
            .gap(vao)
            .h(r(altura))
            .px(dentro_da_borda(8.))
            .rounded(raio_cartao)
            .bg(fundo)
            .border_1()
            .border_color(transparent_black())
            .when(self.realce, |d| d.border_color(Cores::destaque_borda()))
            .hover(move |s| s.bg(hover))
            .when(self.ao_clicar.is_some(), |d| d.cursor_pointer())
            .children(barra)
            .children(simbolo)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .justify_center()
                    .child(titulo)
                    .when_some(self.subtitulo.filter(|_| !uma_linha), |d, sub| {
                        d.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .metadado()
                                .text_color(Cores::tenue())
                                .child(sub),
                        )
                    }),
            )
            .children(direita)
            .children(self.acao_fixa)
            .when_some(self.ao_clicar, |d, f| d.on_click(move |ev, window, cx| f(ev, window, cx)))
    }
}

/// A coluna de 30 do nível 1: o traço da árvore no meio (do topo do cartão ao fundo, mais a fresta
/// de 2 até o próximo; no primeiro, também os 13 até o avatar do pai, que fica a 11 do fundo do
/// cartão de 52; no último, só até o ponto) e o ponto por cima. O aro de rodando é vazado, então
/// o traço para a 2 dele; com dica, o ponto tem a dele (a palavra do estado).
fn coluna_da_arvore(
    simbolo: Option<Simbolo>,
    sinal: Option<Sinal>,
    dica: Option<SharedString>,
    ramo: (bool, bool),
    window: &Window,
) -> AnyElement {
    let (primeiro, ultimo) = ramo;
    let traco = || div().absolute().left_0().w(px(1.)).bg(Cores::borda());
    let vao = r(if sinal.is_some_and(Sinal::vazado) { 12. } else { 0. });
    let linha = div()
        .absolute()
        .left(relative(0.5))
        .w(px(1.))
        .top(px(-1.))
        .bottom(px(-1.))
        .flex()
        .flex_col()
        .child(div().flex_1().bg(Cores::borda()))
        .child(div().flex_none().h(vao))
        .child(div().flex_1().when(!ultimo, |d| d.bg(Cores::borda())))
        .when(!ultimo, |d| d.child(traco().top(relative(1.)).h(esp::E2)))
        .when(primeiro, |d| d.child(traco().bottom(relative(1.)).h(r(13.))));
    let ponto = match (sinal, simbolo) {
        (Some(sinal), _) => Some(sinal.ponto(8.).into_any_element()),
        (None, Some(s)) => Some(s.desenhar(20., window)),
        (None, None) => None,
    };
    let ponto = match (ponto, dica) {
        (Some(ponto), Some(dica)) => Some(
            div()
                .id("ponto-da-arvore")
                .flex_none()
                .size(r(30.))
                .flex()
                .items_center()
                .justify_center()
                .child(ponto)
                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                .into_any_element(),
        ),
        (ponto, _) => ponto,
    };
    div()
        .relative()
        .flex_none()
        .w(r(30.))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .child(linha)
        .children(ponto)
        .into_any_element()
}

/// "#2 Aplicar cupom": o número em tênue e o título 13/500 (14/500 em cartão).
pub fn rotulo_tarefa(n: u64, titulo: &str, grande: bool) -> impl IntoElement {
    div()
        .flex()
        .min_w_0()
        .gap(r(6.))
        .text_size(r(if grande { 14. } else { 13. }))
        .font_weight(FontWeight::MEDIUM)
        .child(div().flex_none().text_color(Cores::tenue()).child(format!("#{n}")))
        .child(div().min_w_0().truncate().text_color(Cores::texto()).child(titulo.to_string()))
}

/// O que vai à direita de uma linha de opção.
pub enum Controle {
    Interruptor { ligado: bool, ao_mudar: Rc<dyn Fn(bool, &mut Window, &mut App)> },
    Elemento(AnyElement),
    Nenhum,
}

/// Linha de preferência no estilo do libadwaita: título, descrição e controle à direita.
#[derive(IntoElement)]
pub struct LinhaDeOpcao {
    id: ElementId,
    titulo: SharedString,
    arco_iris: bool,
    simbolo: Option<(Lucide, Hsla)>,
    descricao: Option<AnyElement>,
    aviso: Option<SharedString>,
    info: Option<SharedString>,
    controle: Controle,
    campo: Option<AnyElement>,
    desabilitada: Option<SharedString>,
}

impl LinhaDeOpcao {
    pub fn new(id: impl Into<ElementId>, titulo: impl Into<SharedString>) -> Self {
        LinhaDeOpcao {
            id: id.into(),
            titulo: titulo.into(),
            arco_iris: false,
            simbolo: None,
            descricao: None,
            aviso: None,
            info: None,
            controle: Controle::Nenhum,
            campo: None,
            desabilitada: None,
        }
    }

    /// Texto, ou um elemento ("Medindo…" com o spinner), embaixo do título.
    pub fn descricao(mut self, d: impl IntoElement) -> Self {
        self.descricao = Some(d.into_any_element());
        self
    }

    /// Texto longo numa dica no (i), ao lado do título.
    pub fn info(mut self, d: impl Into<SharedString>) -> Self {
        self.info = Some(d.into());
        self
    }

    /// A linha inteira alterna o interruptor.
    pub fn interruptor(mut self, ligado: bool, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.controle = Controle::Interruptor { ligado, ao_mudar: Rc::new(f) };
        self
    }

    /// Menu, segmentado ou contador à direita.
    pub fn controle(mut self, e: impl IntoElement) -> Self {
        self.controle = Controle::Elemento(e.into_any_element());
        self
    }

    /// Campo embaixo do título (linha vertical).
    pub fn campo(mut self, e: impl IntoElement) -> Self {
        self.campo = Some(e.into_any_element());
        self
    }

    /// Só "Ultrathink".
    pub fn arco_iris(mut self) -> Self {
        self.arco_iris = true;
        self
    }

    /// Ícone antes do título, com o título na mesma cor (o Ultracode, em violeta).
    pub fn simbolo(mut self, icone: Lucide, cor: Hsla) -> Self {
        self.simbolo = Some((icone, cor));
        self
    }

    /// Uma linha em âmbar embaixo da descrição (custo, carga); some com a linha desabilitada.
    pub fn aviso(mut self, a: impl Into<SharedString>) -> Self {
        self.aviso = Some(a.into());
        self
    }

    /// 0,45, com o motivo no lugar da descrição.
    pub fn desabilitada(mut self, motivo: impl Into<SharedString>) -> Self {
        self.desabilitada = Some(motivo.into());
        self
    }
}

impl RenderOnce for LinhaDeOpcao {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let desabilitada = self.desabilitada.is_some();
        let descricao = self.desabilitada.map(IntoElement::into_any_element).or(self.descricao);
        let aviso = self.aviso.filter(|_| !desabilitada);
        let titulo = div()
            .flex()
            .items_center()
            .gap(r(6.))
            .text_size(r(14.))
            .line_height(r(20.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(self.simbolo.map_or_else(Cores::texto, |(_, cor)| cor))
            .when_some(self.simbolo, |d, (icone, _)| d.child(Icon::new(icone).with_size(tam(window, 14.))))
            .map(|d| {
                if self.arco_iris {
                    d.child(texto_arco_iris(&self.titulo))
                } else {
                    d.child(self.titulo.clone())
                }
            })
            .when_some(self.info, |d, info| {
                d.child(
                    div()
                        .id("info")
                        .text_color(Cores::tenue())
                        .child(Icon::new(Lucide::Info).with_size(tam(window, 14.)))
                        .tooltip(move |window, cx| Tooltip::new(info.clone()).build(window, cx)),
                )
            });
        let (direita, alternar) = match self.controle {
            Controle::Interruptor { ligado, ao_mudar } => {
                let f = ao_mudar.clone();
                let switch = Switch::new(SharedString::from(format!("{}-interruptor", self.id)))
                    .checked(ligado)
                    .disabled(desabilitada)
                    .on_click(move |novo, window, cx| f(*novo, window, cx));
                (Some(switch.into_any_element()), Some((ligado, ao_mudar)))
            }
            Controle::Elemento(e) => (Some(e), None),
            Controle::Nenhum => (None, None),
        };
        div()
            .id(self.id)
            .flex()
            .flex_col()
            .gap(esp::E8)
            .p(esp::E12)
            .when(desabilitada, |d| d.opacity(0.45))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(r(16.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(titulo)
                            .when_some(descricao, |d, t| {
                                d.child(div().metadado().text_color(Cores::tenue()).child(t))
                            })
                            .when_some(aviso, |d, t| d.child(div().metadado().text_color(Cores::ambar()).child(t))),
                    )
                    .children(direita),
            )
            .children(self.campo)
            .when_some(alternar.filter(|_| !desabilitada), |d, (ligado, f)| {
                d.cursor_pointer().on_click(move |_, window, cx| f(!ligado, window, cx))
            })
    }
}

/// Grupo de preferências: rótulo de seção e as linhas numa caixa com divisórias.
#[derive(IntoElement)]
pub struct GrupoDeOpcoes {
    titulo: Option<SharedString>,
    linhas: Vec<AnyElement>,
}

impl GrupoDeOpcoes {
    pub fn new(titulo: Option<SharedString>) -> Self {
        GrupoDeOpcoes { titulo, linhas: Vec::new() }
    }

    pub fn linha(mut self, l: impl IntoElement) -> Self {
        self.linhas.push(l.into_any_element());
        self
    }
}

impl RenderOnce for GrupoDeOpcoes {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let n = self.linhas.len();
        div()
            .flex()
            .flex_col()
            .gap(r(6.))
            .when_some(self.titulo, |d, t| d.child(div().px(r(4.)).rotulo_secao().child(t)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(raio::CARTAO)
                    .bg(Cores::grupo())
                    .border_1()
                    .border_color(Cores::borda())
                    .children(self.linhas.into_iter().enumerate().map(|(i, linha)| {
                        div()
                            .when(i + 1 < n, |d| d.border_b_1().border_color(Cores::linha()))
                            .child(linha)
                    })),
            )
    }
}

/// Um item da lista de escolha.
pub struct ItemDeEscolha {
    pub chave: SharedString,
    pub simbolo: Option<Simbolo>,
    pub titulo: SharedString,
    pub descricao: Option<SharedString>,
    pub selos: Vec<Selo>,
    pub desabilitado: Option<SharedString>,
    /// Outras palavras que a busca encontra.
    pub termos: Option<SharedString>,
}

impl ItemDeEscolha {
    pub fn new(chave: impl Into<SharedString>, titulo: impl Into<SharedString>) -> Self {
        ItemDeEscolha {
            chave: chave.into(),
            simbolo: None,
            titulo: titulo.into(),
            descricao: None,
            selos: Vec::new(),
            desabilitado: None,
            termos: None,
        }
    }
}

pub struct GrupoDeEscolha {
    pub titulo: Option<SharedString>,
    pub itens: Vec<ItemDeEscolha>,
}

pub enum EventoEscolha {
    Escolheu(SharedString),
    Fechou,
}

/// Lista com busca (modelo, "/", "@", "Ir para…"), sobre o `Command` do kit: busca sempre com o
/// foco, ↑/↓ pulando os desabilitados, Enter escolhe, Esc em dois tempos.
pub struct ListaDeEscolha {
    estado: Entity<CommandState>,
    placeholder: SharedString,
    grupos: Rc<Vec<GrupoDeEscolha>>,
    filtrados: Option<Rc<Vec<GrupoDeEscolha>>>,
    escolhido: Option<SharedString>,
    filtro: Option<Rc<dyn Fn(&str) -> Vec<GrupoDeEscolha>>>,
    rodape: Option<Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>>,
    simbolos_dos_grupos: Vec<(SharedString, Rc<Simbolo>)>,
}

impl EventEmitter<EventoEscolha> for ListaDeEscolha {}

impl Focusable for ListaDeEscolha {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.estado.read(cx).focus_handle(cx)
    }
}

impl ListaDeEscolha {
    pub fn new(placeholder: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        ListaDeEscolha {
            estado: cx.new(|cx| CommandState::new(window, cx)),
            placeholder: placeholder.to_string().into(),
            grupos: Rc::new(Vec::new()),
            filtrados: None,
            escolhido: None,
            filtro: None,
            rodape: None,
            simbolos_dos_grupos: Vec::new(),
        }
    }

    /// Símbolo 16 antes do rótulo do grupo de título `titulo` (o logo do Claude no grupo
    /// "Claude", e os itens sem símbolo).
    pub fn simbolo_do_grupo(&mut self, titulo: impl Into<SharedString>, simbolo: Simbolo) {
        let titulo = titulo.into();
        self.simbolos_dos_grupos.retain(|(t, _)| *t != titulo);
        self.simbolos_dos_grupos.push((titulo, Rc::new(simbolo)));
    }

    pub fn definir(&mut self, grupos: Vec<GrupoDeEscolha>, escolhido: Option<SharedString>, cx: &mut Context<Self>) {
        self.grupos = Rc::new(grupos);
        self.filtrados = None;
        self.escolhido = escolhido;
        cx.notify();
    }

    /// Busca própria (arquivos: o nome antes da pasta); a lista mostra o que ela devolver.
    pub fn filtro_proprio(&mut self, f: impl Fn(&str) -> Vec<GrupoDeEscolha> + 'static) {
        self.filtro = Some(Rc::new(f));
    }

    pub fn rodape(&mut self, e: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) {
        self.rodape = Some(Rc::new(e));
    }

    /// Limpa a busca, realça o escolhido e foca a busca.
    pub fn abrir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filtrados = None;
        let escolhido = self.escolhido.clone().and_then(|chave| posicao(&self.grupos, &chave));
        self.estado.update(cx, |estado, cx| {
            estado.set_query("", window, cx);
            estado.set_selected_index(escolhido, window, cx);
        });
        self.estado.update(cx, |estado, cx| estado.focus(window, cx));
        cx.notify();
    }
}

/// Onde está a chave, nas coordenadas do `Command` (grupo, item).
fn posicao(grupos: &[GrupoDeEscolha], chave: &str) -> Option<IndexPath> {
    grupos.iter().enumerate().find_map(|(g, grupo)| {
        let i = grupo.itens.iter().position(|item| item.chave == chave)?;
        Some(IndexPath::new(i).section(g))
    })
}

/// Passo de 48 com descrição e 32 sem; o check fica à direita, na coluna dos selos.
fn linha_de_escolha(item: &ItemDeEscolha, escolhido: bool, window: &Window) -> Div {
    let descrito = item.descricao.is_some();
    div()
        .flex()
        .items_center()
        .gap(if descrito { esp::E12 } else { esp::E8 })
        .w_full()
        .h(if descrito { altura::ESCOLHA_2 } else { altura::CONTROLE })
        .children(item.simbolo.as_ref().and_then(Simbolo::copia).map(|s| s.desenhar(20., window)))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(r(14.))
                        .line_height(r(20.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if escolhido { Cores::forte() } else { Cores::texto() })
                        .child(item.titulo.clone()),
                )
                .when_some(item.descricao.clone(), |d, t| {
                    d.child(div().truncate().metadado().text_color(Cores::tenue()).child(t))
                }),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(esp::E4)
                .children(item.selos.iter().cloned())
                .when(escolhido, |d| {
                    d.child(
                        div()
                            .text_color(Cores::destaque_texto())
                            .child(Icon::new(Lucide::Check).with_size(tam(window, 16.))),
                    )
                }),
        )
}

impl Render for ListaDeEscolha {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lista = cx.entity().downgrade();
        let grupos = self.filtrados.clone().unwrap_or_else(|| self.grupos.clone());
        let escolhido = self.escolhido.clone();
        let mut comando = Command::new(&self.estado)
            .placeholder(self.placeholder.clone())
            .bordered(false)
            .max_h(r(440.))
            .empty(|estado, _, cx| {
                EstadoVazio::new(Lucide::Search, "Nada encontrado")
                    .texto(format!("Nada encontrado para “{}”", estado.query(cx)))
            });
        for (g, grupo) in grupos.iter().enumerate() {
            let mut grupo_cmd = CommandGroup::new();
            if let Some(titulo) = &grupo.titulo {
                grupo_cmd = grupo_cmd.label(titulo.clone());
                if let Some((_, simbolo)) = self.simbolos_dos_grupos.iter().find(|(t, _)| t == titulo) {
                    let simbolo = simbolo.clone();
                    grupo_cmd = grupo_cmd.label_prefix(move |window, _| {
                        simbolo.copia().map(|s| s.desenhar(16., window)).unwrap_or_else(|| div().into_any_element())
                    });
                }
            }
            for (i, item) in grupo.itens.iter().enumerate() {
                let marcado = escolhido.as_ref() == Some(&item.chave);
                let grupos = grupos.clone();
                let mut linha = CommandItem::new()
                    .label(item.titulo.clone())
                    .disabled(item.desabilitado.is_some())
                    .child(move |window, _| linha_de_escolha(&grupos[g].itens[i], marcado, window));
                if let Some(termos) = &item.termos {
                    linha = linha.keywords([termos.clone()]);
                }
                grupo_cmd = grupo_cmd.item(linha);
            }
            comando = comando.group(grupo_cmd);
        }
        let ao_escolher = grupos.clone();
        comando = comando
            .on_confirm({
                let lista = lista.clone();
                move |ix, _, cx| {
                    let Some(item) = ao_escolher.get(ix.section).and_then(|g| g.itens.get(ix.row)) else {
                        return;
                    };
                    let chave = item.chave.clone();
                    let _ = lista.update(cx, |_, cx| cx.emit(EventoEscolha::Escolheu(chave)));
                }
            })
            .on_cancel({
                let lista = lista.clone();
                move |_, cx| {
                    let _ = lista.update(cx, |_, cx| cx.emit(EventoEscolha::Fechou));
                }
            });
        if let Some(filtro) = self.filtro.clone() {
            comando = comando.filterable(false).on_query(move |busca, _, cx| {
                let resultado = Rc::new(filtro(busca));
                let _ = lista.update(cx, |l, cx| {
                    l.filtrados = Some(resultado);
                    cx.notify();
                });
            });
        }
        if let Some(rodape) = self.rodape.clone() {
            comando = comando.footer(move |_, window, cx| rodape(window, cx));
        }
        div().w_full().child(comando)
    }
}
