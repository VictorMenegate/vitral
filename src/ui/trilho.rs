//! Trilho à esquerda, numa peça de vidro própria: o logo (menu do Vitral, F10), o lugar do Mestre,
//! os projetos numa área que rola (o clique abre o projeto na lista; arrastar reordena; soltar um
//! chat o move para lá), Avulsos, o "+" do projeto novo, o sino do que precisa de você e o uso dos
//! planos. A área vazia arrasta a janela. Com Ctrl+B o trilho some junto com a lista; o logo, com o
//! selo do sino, e os anéis de uso vão para o 1º painel (modo recolhido).

use std::{
    collections::HashMap,
    path::Path,
    rc::Rc,
    sync::{Arc, LazyLock},
};

use gpui_kit::base::actions::SelectDown;
use gpui_kit::component::{
    Selectable, WindowExt as _, h_flex,
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    popover::Popover,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    AbaLateral, AbrirConfiguracoes, AbrirMenuVitral, AbrirMestre, AlternarLista, Filtro, IrParaProjeto, Janela,
    NovaConversa, NovoProjeto, ProximaPendencia, Vista,
    componentes::{
        BotaoRedondo, Confirmacao, EventoEscolha, Flutuante as _, GrupoDeEscolha, ItemDeEscolha, ListaDeEscolha,
        PreviaArrasto, QuadradoDeProjeto, Reflexo as _, Selo, Simbolo, Sinal, TamanhoBotao, Tom, TomConfirmacao,
        VarianteRedonda, anel_do_pulso, aviso, botao_fantasma, cabecalho_objeto_com_dica, confirmar, contagem,
        rodape_menu_com_dica, sinal_do_projeto,
    },
    icone_projeto,
    menus::itens_do_projeto,
    movimento::{self, Curva},
    paineis::{ArrastoChat, ArrastoProjeto},
    uso_plano,
};
use crate::{
    atualizacao,
    modelo::{Cartao, Execucao},
    motor::{Espera, Motor, PrecisaDeVoce},
    tema::{Cores, Lucide, Tipo as _, esp, halo_foco, r, raio, tam},
};

/// O logo pequeno, desenhado para 16 a 32 px. Ele declara 32 px: o GPUI rasteriza o SVG no dobro
/// do tamanho declarado e reduz na placa de vídeo sem mipmaps, e de 256 para 32 px o chumbo fino
/// sairia serrilhado. Guardado uma vez só (o trilho redesenha a cada quadro).
static LOGO: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../assets/vitral-pequeno.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});

/// Medidas da peça a 100% (crescem com o zoom): largura e lugar de um projeto.
pub(super) const LARGURA: f32 = 52.;
const LUGAR: f32 = 40.;
/// Perto da borda da área dos projetos (a 100%), o arrasto rola a área.
const BORDA_DA_ROLAGEM: f32 = 24.;

/// Solto sobre um projeto, o arrastado vai antes dele quando o ponteiro está na metade de cima.
fn antes_ou_depois(limites: Bounds<Pixels>, y: Pixels) -> bool {
    y < limites.origin.y + limites.size.height / 2.
}

/// Quanto a área dos projetos rola num movimento do arrasto a menos de `borda` da beirada (para
/// cima é positivo, como o deslocamento do `ScrollHandle`): mais perto, mais rápido.
fn rolagem_na_borda(limites: Bounds<Pixels>, y: Pixels, borda: Pixels) -> Pixels {
    let de_cima = y - limites.origin.y;
    let de_baixo = limites.origin.y + limites.size.height - y;
    if de_cima >= px(0.) && de_cima < borda {
        (borda - de_cima) / 2.
    } else if de_baixo >= px(0.) && de_baixo < borda {
        -(borda - de_baixo) / 2.
    } else {
        px(0.)
    }
}

/// O que um projeto tem rodando e esperando você (as esperas contam famílias, como os cartões de
/// "Esperando você") e o sinal do atalho.
#[derive(Default)]
struct Atividade {
    rodando: usize,
    espera: PrecisaDeVoce,
    sinal: Option<Sinal>,
}

impl Atividade {
    fn esperando(&self) -> usize {
        self.espera.numero() + self.espera.novas
    }
}

/// A atividade de cada projeto, numa passada pelos chats.
fn atividades(motor: &Motor) -> HashMap<String, Atividade> {
    let mut mapa: HashMap<String, Atividade> = HashMap::new();
    for c in motor.chats.iter().filter(|c| !motor.arquivada(c)) {
        let a = mapa.entry(c.projeto.clone()).or_default();
        if matches!(motor.execucao(&c.id), Execucao::Rodando { .. }) || motor.esperando_vaga(&c.id) {
            a.rodando += 1;
        }
        if c.papel.na_lista()
            && let Some((_, espera, erro)) = motor.espera_da_familia(c)
        {
            a.espera.somar(espera, erro);
        }
    }
    for (projeto, a) in &mut mapa {
        a.sinal = sinal_do_projeto(motor, projeto);
    }
    mapa
}

/// Dica do atalho de projeto: o nome e, embaixo, o que está acontecendo nele (sem as partes
/// zeradas); com o projeto aberto na lista, como voltar à visão geral.
fn dica_projeto(projeto: &str, a: Option<&Atividade>, filtrado: bool) -> String {
    let mut partes = Vec::new();
    if let Some(a) = a {
        if a.rodando > 0 {
            partes.push(format!("{} rodando", a.rodando));
        }
        if a.esperando() > 0 {
            partes.push(format!("{} esperando você", a.esperando()));
        }
    }
    if filtrado {
        partes.push("clique para voltar à Visão geral".into());
    }
    if partes.is_empty() {
        projeto.to_string()
    } else {
        format!("{projeto}\n{}", partes.join(" · "))
    }
}

/// Dica do sino: o título e, embaixo, o que trava o trabalho (pedidos, paradas, erros) separado
/// das respostas novas.
fn dica_sino(p: &PrecisaDeVoce) -> String {
    let partes: Vec<String> = [
        (p.pedidos, "pedido", "pedidos"),
        (p.paradas, "parada", "paradas"),
        (p.erros, "erro", "erros"),
        (p.novas, "resposta nova", "respostas novas"),
    ]
    .into_iter()
    .filter(|(n, ..)| *n > 0)
    .map(|(n, um, varios)| contagem(n, um, varios))
    .collect();
    if partes.is_empty() {
        "O que precisa de você".into()
    } else {
        format!("O que precisa de você\n{}", partes.join(", "))
    }
}

/// O que vai no canto do sino: o número (vermelho se algo falhou, senão âmbar), um ponto azul
/// quando só há respostas novas, ou nada.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum MarcaDoSino {
    Numero(usize, Hsla),
    Ponto,
    Nada,
}

fn marca_do_sino(p: &PrecisaDeVoce) -> MarcaDoSino {
    match p.numero() {
        0 if p.novas > 0 => MarcaDoSino::Ponto,
        0 => MarcaDoSino::Nada,
        n => MarcaDoSino::Numero(n, if p.erro { Cores::vermelho() } else { Cores::ambar() }),
    }
}

/// A cor do pulso do sino: a mesma do número (vermelho se algo falhou, senão âmbar).
fn cor_do_pulso(p: &PrecisaDeVoce) -> Hsla {
    if p.erro { Cores::vermelho() } else { Cores::ambar() }
}

/// O selo do sino: sólido, porque fica por cima do botão.
pub(super) fn selo_do_sino(p: &PrecisaDeVoce) -> Option<Selo> {
    match marca_do_sino(p) {
        MarcaDoSino::Numero(n, cor) => Some(Selo::contagem(n).solida(cor)),
        MarcaDoSino::Ponto => Some(Selo::ponto(Cores::destaque())),
        MarcaDoSino::Nada => None,
    }
}

/// O selo do item "O que precisa de você" do menu: tingido, porque fica na linha.
fn selo_do_menu(p: &PrecisaDeVoce) -> Option<Selo> {
    match marca_do_sino(p) {
        MarcaDoSino::Numero(n, cor) => Some(Selo::contagem(n).tingida(cor)),
        MarcaDoSino::Ponto => Some(Selo::ponto(Cores::destaque())),
        MarcaDoSino::Nada => None,
    }
}

/// O canto do botão do Mestre: o número de cartões abertos mais a espera dele (pedido, parada ou
/// erro; vermelho no erro), em âmbar; sem nada disso, o ponto azul da resposta nova que você ainda
/// não leu.
pub(super) fn selos_do_mestre(cartoes: &[Cartao], espera: Option<Espera>, nao_lido: bool) -> MarcaDoSino {
    let abertos = cartoes.iter().filter(|c| c.aberto()).count();
    let esperando = matches!(espera, Some(Espera::Pedido | Espera::Parada | Espera::Erro));
    match abertos + usize::from(esperando) {
        0 if nao_lido => MarcaDoSino::Ponto,
        0 => MarcaDoSino::Nada,
        n => MarcaDoSino::Numero(n, if espera == Some(Espera::Erro) { Cores::vermelho() } else { Cores::ambar() }),
    }
}

/// A dica do botão do Mestre: o nome e, embaixo, o que ele espera de você ou o que está fazendo.
fn dica_mestre(cartoes: usize, espera: Option<Espera>, nao_lido: bool, rodando: bool) -> String {
    let mut partes = Vec::new();
    if cartoes > 0 {
        partes.push(contagem(cartoes, "cartão esperando você", "cartões esperando você"));
    }
    match espera {
        Some(Espera::Pedido) => partes.push("pedido de permissão".into()),
        Some(Espera::Parada) => partes.push("parou no meio".into()),
        Some(Espera::Erro) => partes.push("o último turno deu erro".into()),
        _ => {}
    }
    if rodando {
        partes.push("pensando…".into());
    } else if nao_lido && partes.is_empty() {
        partes.push("resposta nova".into());
    }
    if partes.is_empty() {
        "Mestre\nVê todos os projetos e manda o trabalho para os chats".into()
    } else {
        format!("Mestre\n{}", partes.join(" · "))
    }
}

/// "~/Projetos": a pasta de trabalho com a casa abreviada.
pub(super) fn pasta_curta(raiz: &Path) -> String {
    match dirs::home_dir().and_then(|casa| raiz.strip_prefix(casa).ok().map(Path::to_path_buf)) {
        Some(resto) if resto.as_os_str().is_empty() => "~".into(),
        Some(resto) => format!("~/{}", resto.display()),
        None => raiz.display().to_string(),
    }
}

/// Contêiner dos botões da peça: o clique (esquerdo ou direito) neles não move a janela nem abre
/// o menu dela.
fn sem_a_peca() -> Div {
    div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
}

/// Item de menu que chama um método da janela.
fn item(
    rotulo: &'static str,
    icone: Lucide,
    janela: &WeakEntity<Janela>,
    f: impl Fn(&mut Janela, &mut Window, &mut Context<Janela>) + 'static,
) -> PopupMenuItem {
    let janela = janela.clone();
    PopupMenuItem::new(rotulo).icon(icone).on_click(move |_, window, cx| {
        let _ = janela.update(cx, |this, cx| f(this, window, cx));
    })
}

/// Cabeçalho do menu do Vitral: o logo, "Vitral" e o nome da pasta de trabalho, com o caminho
/// na dica (fixo, fora do teclado).
/// O topo do menu do Vitral: o logo e quantos projetos há, com a pasta de dados na dica.
fn cabecalho_vitral(dados: &Path, projetos: usize) -> PopupMenuItem {
    let quantos = contagem(projetos, "projeto", "projetos");
    cabecalho_objeto_com_dica(Simbolo::Imagem(LOGO.clone()), "Vitral", Some(quantos.into()), pasta_curta(dados))
}

/// Um projeto no menu do modo recolhido.
struct ProjetoNoMenu {
    id: String,
    nome: SharedString,
    icone: &'static icone_projeto::Icone,
    cor: u32,
    sinal: Option<Sinal>,
    rodando: usize,
    filtrado: bool,
}

/// Um projeto arquivado no menu do Vitral.
struct ProjetoArquivado {
    id: String,
    nome: SharedString,
    icone: &'static icone_projeto::Icone,
    cor: u32,
}

type ConstruirMenu = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

/// O logo redondo: o medalhão ocupa o botão inteiro (sem disco atrás); no hover e com o menu
/// aberto ganha um aro. No modo recolhido ele leva o selo do
/// sino, que some junto com o trilho. Com uma versão nova pronta, um ponto azul de 8 embaixo.
#[derive(IntoElement)]
struct BotaoLogo {
    lado: f32,
    medalhao: f32,
    aberto: bool,
    selo: Option<Selo>,
    /// No modo recolhido, o pulso do sino (a geração e a cor), que vai no logo com o selo.
    pulso: Option<(u64, Hsla)>,
    /// A versão pronta esperando o reinício.
    pronta: Option<SharedString>,
}

impl Selectable for BotaoLogo {
    fn selected(mut self, selected: bool) -> Self {
        self.aberto = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.aberto
    }
}

impl RenderOnce for BotaoLogo {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = ElementId::from("logo");
        let pulso = self.pulso.and_then(|(geracao, cor)| {
            let t = movimento::disparo(movimento::chave(&id, "pulso"), geracao, movimento::ENCHER, Curva::Sair, window, cx);
            t.map(|t| anel_do_pulso(cor, t))
        });
        div()
            .id(id)
            .relative()
            .flex_none()
            .size(r(self.lado))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .tab_index(0)
            .focus_visible(|s| s.shadow(halo_foco()))
            .border_2()
            .border_color(if self.aberto { Cores::borda_forte() } else { transparent_black() })
            .when(!self.aberto, |s| {
                let dica = match &self.pronta {
                    Some(versao) => format!("Menu do Vitral\nVersão {versao} pronta"),
                    None => "Menu do Vitral".to_string(),
                };
                s.hover(|s| s.border_color(Cores::borda())).tooltip(move |window, cx| {
                    Tooltip::new(dica.clone()).action(&AbrirMenuVitral, None).build(window, cx)
                })
            })
            .child(img(LOGO.clone()).flex_none().size(r(self.medalhao)))
            .children(pulso)
            .when_some(self.selo, |s, selo| {
                s.child(div().absolute().top(r(-3.)).right(r(-3.)).child(selo))
            })
            .when(self.pronta.is_some(), |s| {
                // O aro opaco separa o ponto das cores do medalhão.
                s.child(
                    div()
                        .absolute()
                        .bottom(r(-2.))
                        .right(r(-2.))
                        .size(r(12.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(Cores::dialogo())
                        .child(div().size(r(8.)).rounded_full().bg(Cores::destaque())),
                )
            })
    }
}

/// Um elemento qualquer como gatilho de popover: o logo do modo recolhido abre dois, o menu e,
/// por fora dele, a caixa do sino.
#[derive(IntoElement)]
pub(super) struct Gatilho(pub AnyElement);

impl Selectable for Gatilho {
    fn selected(self, _: bool) -> Self {
        self
    }

    fn is_selected(&self) -> bool {
        false
    }
}

impl RenderOnce for Gatilho {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.0
    }
}

/// A chave de um projeto arquivado em "Todos os projetos" (nenhum nome de projeto tem "/").
const CHAVE_ARQUIVADO: &str = "/arquivado/";

/// Os projetos como itens da lista de escolha: quadrado, nome e o sinal à direita. Os arquivados
/// vêm num grupo à parte, e escolher um deles o desarquiva.
fn grupos_de_projetos(motor: &Motor) -> Vec<GrupoDeEscolha> {
    let atividades = atividades(motor);
    let itens = motor
        .projetos_visiveis()
        .into_iter()
        .map(|projeto| {
            let (icone, cor) = icone_projeto::visual(projeto);
            let sinal = atividades.get(&projeto.id).and_then(|a| a.sinal);
            let mut item = ItemDeEscolha::new(projeto.id.clone(), projeto.nome.clone());
            item.simbolo = Some(Simbolo::Projeto(icone, cor));
            item.selos = sinal
                .map(|s| if s.vazado() { Selo::ponto(s.cor()).vazado() } else { Selo::ponto(s.cor()) })
                .into_iter()
                .collect();
            item
        })
        .collect();
    let arquivados: Vec<ItemDeEscolha> = motor
        .projetos_arquivados()
        .into_iter()
        .map(|projeto| {
            let (icone, cor) = icone_projeto::visual(projeto);
            let mut item = ItemDeEscolha::new(format!("{CHAVE_ARQUIVADO}{}", projeto.id), projeto.nome.clone());
            item.simbolo = Some(Simbolo::Projeto(icone, cor));
            item.descricao = Some("Escolher desarquiva".into());
            item
        })
        .collect();
    let mut grupos = vec![GrupoDeEscolha { titulo: None, itens }];
    if !arquivados.is_empty() {
        grupos.push(GrupoDeEscolha { titulo: Some("Arquivados".into()), itens: arquivados });
    }
    grupos
}

/// O atalho de um projeto no trilho: o ponteiro no lugar de 40 acende o quadrado em 120 ms (o
/// hover suave), e a vista do projeto liga o aro na mesma transição (a 40% quando só o chat do
/// painel ativo é dele). Arrasta para reordenar e aceita um chat solto (move para o projeto).
#[derive(IntoElement)]
struct AtalhoDeProjeto {
    ix: usize,
    projeto: String,
    nome: SharedString,
    icone: &'static icone_projeto::Icone,
    cor: u32,
    aberto: bool,
    do_painel: bool,
    sinal: Option<Sinal>,
    dica: SharedString,
    /// Outro projeto arrastado por cima: a linha antes (`true`) ou depois dele.
    alvo: Option<bool>,
    janela: WeakEntity<Janela>,
    motor: Entity<Motor>,
}

impl RenderOnce for AtalhoDeProjeto {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let AtalhoDeProjeto { ix, projeto, nome, icone, cor, aberto, do_painel, sinal, dica, alvo, janela, motor } = self;
        let id = ElementId::from(SharedString::from(format!("trilho-projeto-{projeto}")));
        let hover = movimento::Hover::de(movimento::chave(&id, "ponteiro"), window, cx);
        let quadrado = QuadradoDeProjeto::new(icone, cor, 32.)
            .aro(if aberto { 1. } else if do_painel { 0.4 } else { 0. })
            .realcado(aberto || hover.ativo(cx))
            .sinal(sinal)
            .suave(id.clone());
        let tinta = Hsla::from(rgb(cor));
        let arrasto = ArrastoProjeto { id: projeto.clone(), nome: nome.clone() };
        let (j_clique, j_menu, j_mover, j_chat, j_solta_chat, j_solta) =
            (janela.clone(), janela.clone(), janela.clone(), janela.clone(), janela.clone(), janela);
        let (p_clique, p_mover, p_chat, p_solta_chat, p_solta) =
            (projeto.clone(), projeto.clone(), projeto.clone(), projeto.clone(), projeto.clone());
        sem_a_peca().child(
            div()
                .id(id)
                .relative()
                .size(r(LUGAR))
                .flex()
                .items_center()
                .justify_center()
                .rounded(raio::CARTAO)
                .cursor_pointer()
                .tab_index(0)
                .focus_visible(|s| s.shadow(halo_foco()))
                .on_hover(hover.ouvinte())
                .child(quadrado)
                .when_some(alvo, |s, antes| {
                    s.child(
                        div()
                            .absolute()
                            .left(r(4.))
                            .right(r(4.))
                            .h(px(2.))
                            .rounded_full()
                            .bg(Cores::destaque())
                            .map(|l| if antes { l.top(px(-1.)) } else { l.bottom(px(-1.)) }),
                    )
                })
                .tooltip(move |window, cx| {
                    let dica = Tooltip::new(dica.clone());
                    // Ctrl+1…9 vão para os nove primeiros.
                    if ix < 9 { dica.action(&IrParaProjeto(ix), None) } else { dica }.build(window, cx)
                })
                .on_click(move |_, window, cx| {
                    let _ = j_clique.update(cx, |this, cx| this.alternar_vista(p_clique.clone(), window, cx));
                })
                .on_drag(arrasto, move |a, _, _, cx| {
                    cx.new(|_| PreviaArrasto { titulo: a.nome.clone(), simbolo: Simbolo::Projeto(icone, cor) })
                })
                .on_drag_move(move |ev: &DragMoveEvent<ArrastoProjeto>, _, cx| {
                    let dentro = ev.bounds.contains(&ev.event.position) && ev.drag(cx).id != p_mover;
                    let alvo = dentro.then(|| (p_mover.clone(), antes_ou_depois(ev.bounds, ev.event.position.y)));
                    let _ = j_mover.update(cx, |this, cx| {
                        let era_daqui = this.alvo_trilho.as_ref().is_some_and(|(p, _)| *p == p_mover);
                        if (alvo.is_some() || era_daqui) && this.alvo_trilho != alvo {
                            this.alvo_trilho = alvo;
                            cx.notify();
                        }
                    });
                })
                .on_drag_move(move |ev: &DragMoveEvent<ArrastoChat>, _, cx| {
                    let dentro = ev.bounds.contains(&ev.event.position);
                    let _ = j_chat.update(cx, |this, cx| this.chat_sobre_o_trilho(&p_chat, dentro.then_some(ev.bounds), cx));
                })
                .drag_over::<ArrastoChat>(move |s, _, _, _| s.bg(tinta.opacity(0.3)).border_1().border_color(tinta))
                .on_drop(move |arrasto: &ArrastoChat, window, cx| {
                    let _ = j_solta_chat.update(cx, |this, cx| this.soltar_chat(&arrasto.chat, &p_solta_chat, window, cx));
                })
                .on_drop(move |arrasto: &ArrastoProjeto, _, cx| {
                    let _ = j_solta.update(cx, |this, cx| this.soltar_projeto(&arrasto.id, &p_solta, cx));
                })
                .context_menu(move |menu, window, cx| {
                    itens_do_projeto(menu, j_menu.clone(), &motor, &projeto, true, window, cx)
                }),
        )
    }
}

/// Avulsos, fixo embaixo dos projetos: os chats sem projeto. O clique abre a lista deles; aceita
/// um chat solto (tira do projeto).
#[derive(IntoElement)]
struct AtalhoAvulsos {
    aberto: bool,
    do_painel: bool,
    sinal: Option<Sinal>,
    dica: SharedString,
    janela: WeakEntity<Janela>,
}

impl RenderOnce for AtalhoAvulsos {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = ElementId::from("trilho-avulsos");
        let ponteiro = movimento::Hover::de(movimento::chave(&id, "hover"), window, cx);
        let realce = self.aberto || ponteiro.ativo(cx);
        let aro = if self.aberto { 1. } else if self.do_painel { 0.4 } else { 0. };
        let alvo = (if realce { Cores::pressionado() } else { Cores::hover() }, aro);
        let (fundo, aro) = movimento::valor(movimento::chave(&id, "cores"), alvo, movimento::COR, Curva::Sair, window, cx);
        let dica = self.dica;
        let (j_clique, j_chat, j_solta, j_menu) =
            (self.janela.clone(), self.janela.clone(), self.janela.clone(), self.janela);
        sem_a_peca().child(
            div()
                .id(id)
                .relative()
                .size(r(LUGAR))
                .flex()
                .items_center()
                .justify_center()
                .rounded(raio::CARTAO)
                .cursor_pointer()
                .tab_index(0)
                .focus_visible(|s| s.shadow(halo_foco()))
                .on_hover(ponteiro.ouvinte())
                .child(
                    div()
                        .relative()
                        .size(r(32.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(raio::CONTROLE)
                        .bg(fundo)
                        .child(svg().path(Lucide::MessageCircle.path()).flex_none().size(r(18.)).text_color(Cores::tenue()))
                        .when(aro > 0., |s| {
                            s.child(
                                div()
                                    .absolute()
                                    .top(r(-4.))
                                    .left(r(-4.))
                                    .right(r(-4.))
                                    .bottom(r(-4.))
                                    .rounded(raio::CONTROLE + px(4.))
                                    .border_2()
                                    .border_color(Cores::tenue().opacity(aro)),
                            )
                        })
                        .when_some(self.sinal, |s, sinal| {
                            s.child(div().absolute().top(r(-4.)).right(r(-4.)).child(sinal.ponto(8.)))
                        }),
                )
                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                .on_click(move |_, window, cx| {
                    let _ = j_clique.update(cx, |this, cx| this.alternar_avulsos(window, cx));
                })
                .on_drag_move(move |ev: &DragMoveEvent<ArrastoChat>, _, cx| {
                    let dentro = ev.bounds.contains(&ev.event.position);
                    let _ = j_chat.update(cx, |this, cx| this.chat_sobre_o_trilho("", dentro.then_some(ev.bounds), cx));
                })
                .drag_over::<ArrastoChat>(|s, _, _, _| s.bg(Cores::pressionado()).border_1().border_color(Cores::borda_forte()))
                .on_drop(move |arrasto: &ArrastoChat, window, cx| {
                    let _ = j_solta.update(cx, |this, cx| this.soltar_chat(&arrasto.chat, "", window, cx));
                })
                .context_menu(move |menu, _, _| {
                    let (j1, j2) = (j_menu.clone(), j_menu.clone());
                    menu.item(item("Novo chat avulso", Lucide::MessageSquarePlus, &j1, |this, window, cx| {
                        this.nova_conversa("", window, cx)
                    }))
                    .item(item("Projetos arquivados…", Lucide::Archive, &j2, |this, window, cx| {
                        this.dialogo_projetos_arquivados(window, cx)
                    }))
                }),
        )
    }
}

/// O Mestre logo abaixo do logo: o círculo de 36 na tinta do destaque com o Orbit, que acende no
/// ponteiro em 120 ms e, com o Mestre aberto no painel ativo, fica na seleção com o aro de 2 do
/// destaque. O selo é o de `selos_do_mestre`; rodando, sem selo, o ponto vazado da atividade.
#[derive(IntoElement)]
struct BotaoMestre {
    ativo: bool,
    selo: Option<Selo>,
    sinal: Option<Sinal>,
    dica: SharedString,
    janela: WeakEntity<Janela>,
}

impl RenderOnce for BotaoMestre {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = ElementId::from("trilho-mestre");
        let ponteiro = movimento::Hover::de(movimento::chave(&id, "hover"), window, cx);
        let fundo = if self.ativo {
            Cores::selecao()
        } else if ponteiro.ativo(cx) {
            Cores::destaque().opacity(0.24)
        } else {
            Cores::tinta(Cores::destaque())
        };
        let alvo = (fundo, if self.ativo { 1f32 } else { 0. });
        let (fundo, aro) = movimento::valor(movimento::chave(&id, "cores"), alvo, movimento::COR, Curva::Sair, window, cx);
        let (dica, janela) = (self.dica, self.janela);
        let sinal = self.sinal.filter(|_| self.selo.is_none());
        sem_a_peca().child(
            div()
                .id(id)
                .relative()
                .size(r(LUGAR))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .tab_index(0)
                .focus_visible(|s| s.shadow(halo_foco()))
                .on_hover(ponteiro.ouvinte())
                .child(
                    div()
                        .relative()
                        .size(r(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(fundo)
                        .child(svg().path(Lucide::Orbit.path()).flex_none().size(r(18.)).text_color(Cores::destaque_texto()))
                        .when(aro > 0., |s| {
                            s.child(
                                div()
                                    .absolute()
                                    .top(r(-3.))
                                    .left(r(-3.))
                                    .right(r(-3.))
                                    .bottom(r(-3.))
                                    .rounded_full()
                                    .border_2()
                                    .border_color(Cores::destaque().opacity(aro)),
                            )
                        })
                        .when_some(self.selo, |s, selo| s.child(div().absolute().top(r(-3.)).right(r(-3.)).child(selo)))
                        .when_some(sinal, |s, sinal| s.child(div().absolute().top(r(-2.)).right(r(-2.)).child(sinal.ponto(8.)))),
                )
                .tooltip(move |window, cx| Tooltip::new(dica.clone()).action(&AbrirMestre, None).build(window, cx))
                .on_click(move |_, window, cx| {
                    let _ = janela.update(cx, |this, cx| this.abrir_mestre(window, cx));
                }),
        )
    }
}

/// O "Novo projeto" no último lugar: o contorno e o +, que acendem suave como os quadrados.
#[derive(IntoElement)]
struct BotaoNovoProjeto {
    primeiro: bool,
    janela: WeakEntity<Janela>,
}

impl RenderOnce for BotaoNovoProjeto {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = ElementId::from("trilho-novo-projeto");
        let ponteiro = movimento::Hover::de(movimento::chave(&id, "hover"), window, cx);
        let alvo = if ponteiro.ativo(cx) {
            ((Cores::hover(), Cores::borda_forte()), Cores::texto())
        } else {
            ((transparent_black(), Cores::borda()), Cores::tenue())
        };
        let ((fundo, borda), cor) =
            movimento::valor(movimento::chave(&id, "cores"), alvo, movimento::COR, Curva::Sair, window, cx);
        let dica = if self.primeiro { "Novo projeto\nCrie o primeiro projeto" } else { "Novo projeto" };
        let janela = self.janela;
        sem_a_peca().child(
            div()
                .id(id)
                .size(r(LUGAR))
                .flex()
                .items_center()
                .justify_center()
                .rounded(raio::CARTAO)
                .cursor_pointer()
                .tab_index(0)
                .focus_visible(|s| s.shadow(halo_foco()))
                .on_hover(ponteiro.ouvinte())
                .child(
                    div()
                        .size(r(32.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(raio::CONTROLE)
                        .border_1()
                        .border_color(borda)
                        .bg(fundo)
                        .child(svg().path(Lucide::Plus.path()).flex_none().size(r(16.)).text_color(cor)),
                )
                .tooltip(move |window, cx| Tooltip::new(dica).build(window, cx))
                .on_click(move |_, window, cx| {
                    let _ = janela.update(cx, |this, cx| this.dialogo_criar_projeto(window, cx));
                }),
        )
    }
}

impl Janela {
    /// A geração do pulso do sino: sobe só quando o número do que precisa de você aumenta. Abrir
    /// o app (a primeira conta), o número cair e marcar como lido não pulsam.
    fn geracao_do_sino(&self, precisa: &PrecisaDeVoce) -> u64 {
        let numero = precisa.numero();
        if self.sino_numero.replace(Some(numero)).is_some_and(|antes| numero > antes) {
            self.sino_geracao.set(self.sino_geracao.get() + 1);
        }
        self.sino_geracao.get()
    }

    pub(super) fn trilho(&self, motor: &Entity<Motor>, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let m = motor.read(cx);
        let projetos: Vec<String> = m.projetos_visiveis().iter().map(|p| p.id.clone()).collect();
        let atividades = atividades(m);
        let precisa = super::cartoes::precisa_com_o_mestre(m);
        let mestre = self.botao_mestre(m, cx);
        // O projeto do chat do painel ativo ganha o aro a 40%.
        let do_painel = self.selecionado.as_ref().and_then(|id| m.chat(id)).map(|c| c.projeto.clone());
        let atalhos: Vec<AnyElement> = projetos
            .iter()
            .enumerate()
            .map(|(ix, projeto)| {
                self.atalho_projeto(ix, projeto, motor, atividades.get(projeto), do_painel.as_deref(), cx).into_any_element()
            })
            .collect();
        let avulsos = atividades.get("");
        let dica_avulsos = dica_projeto("Avulsos · chats sem projeto", avulsos, self.vista == Vista::Avulsos);
        let avulsos = AtalhoAvulsos {
            aberto: self.vista == Vista::Avulsos,
            do_painel: do_painel.as_deref() == Some("") && self.vista != Vista::Avulsos,
            sinal: avulsos.and_then(|a| a.sinal),
            dica: dica_avulsos.into(),
            janela: cx.entity().downgrade(),
        };
        let borda = tam(window, BORDA_DA_ROLAGEM);
        let (rolagem_p, rolagem_c) = (self.rolagem_trilho.clone(), self.rolagem_trilho.clone());
        let area = div()
            .id("trilho-projetos")
            .flex()
            .flex_col()
            .items_center()
            .flex_shrink(1.)
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.rolagem_trilho)
            .on_drag_move(move |ev: &DragMoveEvent<ArrastoProjeto>, window, _| {
                rolar_na_borda(&rolagem_p, ev.bounds, ev.event.position.y, borda, window)
            })
            .on_drag_move(move |ev: &DragMoveEvent<ArrastoChat>, window, _| {
                rolar_na_borda(&rolagem_c, ev.bounds, ev.event.position.y, borda, window)
            })
            .children(atalhos);

        self.arrastavel_com_id("arrastar-trilho")
            .on_mouse_down(MouseButton::Right, |evento, window, _| {
                window.show_window_menu(evento.position)
            })
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .flex_none()
            .w(r(LARGURA))
            .h_full()
            .pt(esp::E4)
            .pb(esp::E8)
            .rounded(raio::PECA)
            .bg(Cores::vidro_leitura())
            .reflexo(raio::PECA)
            .child(sem_a_peca().child(self.botao_logo(m, false, precisa, cx)))
            .children(mestre.map(|e| div().mt(esp::E8).child(e)))
            .child(div().flex_none().w(r(20.)).h(px(1.)).mt(esp::E8).mb(esp::E4).bg(Cores::borda()))
            .child(area)
            .child(avulsos)
            .child(self.botao_novo_projeto(projetos.is_empty(), cx))
            .child(div().flex_1().min_h(r(8.)))
            .child(sem_a_peca().child(self.botao_sino(precisa, cx)))
            .child(sem_a_peca().mt(esp::E8).child(self.uso_plano(cx)))
            .children(self.faixa_mover_para(m, cx))
    }

    /// Um lugar de 40 com o quadrado do projeto: abre o projeto na lista (ou volta à visão geral);
    /// o clique direito abre o menu do projeto.
    fn atalho_projeto(
        &self,
        ix: usize,
        projeto: &str,
        motor: &Entity<Motor>,
        atividade: Option<&Atividade>,
        do_painel: Option<&str>,
        cx: &Context<Self>,
    ) -> AtalhoDeProjeto {
        let (icone, cor) = icone_projeto::visual_do(motor.read(cx), projeto);
        let aberto = self.vista.projeto() == Some(projeto);
        let nome = motor.read(cx).nome_do_projeto(projeto);
        AtalhoDeProjeto {
            ix,
            projeto: projeto.to_string(),
            nome: nome.clone().into(),
            icone,
            cor,
            aberto,
            do_painel: !aberto && do_painel == Some(projeto),
            sinal: atividade.and_then(|a| a.sinal),
            dica: dica_projeto(&nome, atividade, aberto).into(),
            alvo: self.alvo_trilho.as_ref().filter(|(p, _)| p == projeto).map(|(_, antes)| *antes),
            janela: cx.entity().downgrade(),
            motor: motor.clone(),
        }
    }

    /// "Mover para X" ao lado do projeto sob o chat arrastado. Fica por cima de tudo, fora do corte
    /// da área que rola.
    fn faixa_mover_para(&self, motor: &Motor, cx: &App) -> Option<AnyElement> {
        let (destino, limites) = self.chat_no_trilho.clone()?;
        let texto = format!("Mover para {}", motor.nome_do_projeto(&destino));
        let posicao = point(limites.origin.x + limites.size.width + px(8.), limites.origin.y + limites.size.height / 2.);
        Some(
            deferred(
                anchored().anchor(Anchor::LeftCenter).position(posicao).child(
                    h_flex()
                        .gap(esp::E8)
                        .h(r(32.))
                        .px(esp::E12)
                        .flutuante(cx)
                        .child(svg().path(Lucide::FolderInput.path()).flex_none().size(r(16.)).text_color(Cores::destaque_texto()))
                        .child(div().whitespace_nowrap().rotulo().text_color(Cores::forte()).child(texto)),
                ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// "Novo projeto" no fim dos atalhos, quando todos cabem: só o contorno e o +.
    fn botao_novo_projeto(&self, primeiro: bool, cx: &Context<Self>) -> BotaoNovoProjeto {
        BotaoNovoProjeto { primeiro, janela: cx.entity().downgrade() }
    }

    /// O sino: o número do que precisa de você (ou um ponto, se só há respostas novas) e o
    /// popover "O que precisa de você".
    fn botao_sino(&self, precisa: PrecisaDeVoce, cx: &Context<Self>) -> impl IntoElement + use<> {
        let botao = BotaoRedondo::new("trilho-sino", Some(Lucide::Bell.into()), dica_sino(&precisa))
            .variante(VarianteRedonda::Plano)
            .diametro(36.)
            .dica_acao(Box::new(ProximaPendencia), None)
            .focavel(true)
            .pulso(self.geracao_do_sino(&precisa))
            .pulso_cor(cor_do_pulso(&precisa));
        let botao = match selo_do_sino(&precisa) {
            Some(selo) => botao.selo(selo),
            None => botao,
        };
        self.popover_caixa("caixa-trilho", botao, Anchor::LeftCenter, cx)
    }

    /// O logo e o menu do Vitral, controlado por `menu_aberto` para o F10 abrir o mesmo. No trilho
    /// ele abre à direita; no modo recolhido, embaixo do logo, com o que o trilho mostrava.
    fn botao_logo(&self, motor: &Motor, compacto: bool, precisa: PrecisaDeVoce, cx: &Context<Self>) -> impl IntoElement + use<> {
        let construir = self.itens_menu(motor, compacto, precisa, cx);
        let janela = cx.entity().downgrade();
        // O medalhão enche o botão; sobra só o aro de 2 do hover.
        let (lado, medalhao) = if compacto { (32., 28.) } else { (44., 40.) };
        Popover::new("menu-vitral")
            .appearance(false)
            .overlay_closable(false)
            .anchor(if compacto { Anchor::TopLeft } else { Anchor::LeftCenter })
            .offset(px(8.))
            .open(self.menu_aberto)
            .on_open_change(move |aberto, _, cx| {
                let _ = janela.update(cx, |this, cx| {
                    if this.menu_aberto != *aberto {
                        this.menu_aberto = *aberto;
                        cx.notify();
                    }
                });
            })
            .trigger(BotaoLogo {
                lado,
                medalhao,
                aberto: false,
                selo: compacto.then(|| selo_do_sino(&precisa)).flatten(),
                pulso: compacto.then(|| (self.geracao_do_sino(&precisa), cor_do_pulso(&precisa))),
                pronta: self.atualizacao_pronta.clone().map(SharedString::from),
            })
            .content(move |_, window, cx| {
                // Feito uma vez por abertura (o conteúdo é pedido a cada quadro) e refeito
                // depois de fechar, como o dropdown do kit.
                let guardado = window.use_keyed_state("menu-vitral-itens", cx, |_, _| None::<Entity<PopupMenu>>);
                if let Some(menu) = guardado.read(cx).clone() {
                    return menu;
                }
                let construir = construir.clone();
                let menu = PopupMenu::build(window, cx, move |menu, window, cx| construir(menu, window, cx));
                menu.focus_handle(cx).focus(window, cx);
                // Pelo F10, o primeiro item já vem realçado (o ↓ pula o cabeçalho).
                if window.last_input_was_keyboard() {
                    window.on_next_frame(|window, cx| window.dispatch_action(Box::new(SelectDown), cx));
                }
                let (popover, fraco) = (cx.entity().downgrade(), guardado.downgrade());
                window
                    .subscribe(&menu, cx, move |_, _: &DismissEvent, window, cx| {
                        if let Some(popover) = popover.upgrade() {
                            popover.update(cx, |p, cx| p.dismiss(window, cx));
                        }
                        let _ = fraco.update(cx, |g, _| *g = None);
                    })
                    .detach();
                guardado.update(cx, |g, _| *g = Some(menu.clone()));
                menu
            })
    }

    /// Os itens do menu do Vitral, em três grupos: criar, ajustes e a pasta, e a versão no rodapé.
    /// No modo recolhido, antes deles, o que o trilho mostrava: a lista, o sino e os projetos. Com
    /// uma versão nova pronta, "Reiniciar para a 0.3.0" logo acima das Configurações.
    fn itens_menu(&self, motor: &Motor, compacto: bool, precisa: PrecisaDeVoce, cx: &Context<Self>) -> ConstruirMenu {
        let janela = cx.entity().downgrade();
        let pronta = self.atualizacao_pronta.clone();
        let raiz = motor.dados.pasta.clone();
        let sem_codex = motor.codex_lido && motor.codex.is_none();
        let todos = motor.projetos_visiveis();
        let total = todos.len();
        let arquivados: Rc<Vec<ProjetoArquivado>> = Rc::new(
            motor
                .projetos_arquivados()
                .into_iter()
                .map(|p| {
                    let (icone, cor) = icone_projeto::visual(p);
                    ProjetoArquivado { id: p.id.clone(), nome: p.nome.clone().into(), icone, cor }
                })
                .collect(),
        );
        let projetos: Rc<Vec<ProjetoNoMenu>> = Rc::new(if compacto {
            let atividades = atividades(motor);
            todos
                .into_iter()
                .take(9)
                .map(|p| {
                    let (icone, cor) = icone_projeto::visual(p);
                    let a = atividades.get(&p.id);
                    ProjetoNoMenu {
                        filtrado: self.vista.projeto() == Some(p.id.as_str()),
                        sinal: a.and_then(|a| a.sinal),
                        rodando: a.map_or(0, |a| a.rodando),
                        id: p.id.clone(),
                        nome: p.nome.clone().into(),
                        icone,
                        cor,
                    }
                })
                .collect()
        } else {
            Vec::new()
        });
        Rc::new(move |menu, window, _| {
            let gerenciador = raiz.clone();
            menu.min_w(tam(window, 260.))
                .max_w(tam(window, 360.))
                .max_h(window.viewport_size().height - tam(window, 32.))
                .scrollable(true)
                .item(cabecalho_vitral(&raiz, total))
                .separator()
                .when(compacto, |menu| itens_recolhido(menu, &janela, &precisa, &projetos, total))
                .item(
                    item("Falar com o Mestre", Lucide::Orbit, &janela, |this, window, cx| this.abrir_mestre(window, cx))
                        .action(Box::new(AbrirMestre)),
                )
                .item(
                    item("Novo chat…", Lucide::MessageSquarePlus, &janela, |this, window, cx| {
                        this.dialogo_nova_conversa(window, cx)
                    })
                    .action(Box::new(NovaConversa)),
                )
                .item(
                    item("Novo projeto…", Lucide::FolderPlus, &janela, |this, window, cx| {
                        this.dialogo_criar_projeto(window, cx)
                    })
                    .action(Box::new(NovoProjeto)),
                )
                .item(item("Rotinas", Lucide::CalendarClock, &janela, |this, window, cx| {
                    this.dialogo_rotinas(window, cx)
                }))
                .item(item("Importar do Claude Code…", Lucide::Download, &janela, |this, _, cx| {
                    if let Some(motor) = this.motor.clone() {
                        motor.update(cx, |m, cx| m.importar_do_claude_code(false, cx));
                    }
                }))
                .separator()
                .when(!arquivados.is_empty(), |menu| itens_arquivados(menu, &janela, &arquivados))
                .item(item("Contas do Claude", Lucide::CircleUser, &janela, |this, window, cx| {
                    this.dialogo_contas(window, cx)
                }))
                .when(sem_codex, |menu| {
                    menu.item(
                        PopupMenuItem::new("Instalar o Codex…")
                            .icon(Lucide::Download)
                            .on_click(|_, _, _| uso_plano::instalar_codex()),
                    )
                })
                .when_some(pronta.clone(), |menu, versao| {
                    let janela = janela.clone();
                    menu.item(PopupMenuItem::new(format!("Reiniciar para a {versao}")).icon(Lucide::RotateCw).on_click(
                        move |_, window, cx| {
                            let _ = janela.update(cx, |this, cx| this.reiniciar(window, cx));
                        },
                    ))
                })
                .item(
                    item("Configurações", Lucide::Settings, &janela, |this, window, cx| {
                        this.abrir_configuracoes(window, cx)
                    })
                    .action(Box::new(AbrirConfiguracoes)),
                )
                .item(item("Memória do Mestre…", Lucide::Brain, &janela, |this, window, cx| {
                    this.dialogo_memoria(window, cx)
                }))
                .separator()
                .item(
                    PopupMenuItem::new("Abrir a pasta de dados")
                        .icon(Lucide::FolderOpen)
                        .external()
                        .on_click(move |_, _, cx| cx.open_url(&format!("file://{}", gerenciador.display()))),
                )
                .separator()
                .item(rodape_menu_com_dica(atualizacao::rotulo(), atualizacao::origem()))
        })
    }

    /// F10: abre ou fecha o menu do Vitral (no trilho, ou no logo do modo recolhido).
    pub(super) fn abrir_menu_vitral(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.menu_aberto = !self.menu_aberto;
        cx.notify();
    }

    /// O Mestre logo abaixo do logo (some só se o motor não tiver o chat dele).
    fn botao_mestre(&self, motor: &Motor, cx: &Context<Self>) -> Option<AnyElement> {
        let mestre = motor.chat(motor.mestre_id()?)?;
        let espera = motor.espera_do_chat(mestre);
        let aberto_aqui = self.selecionado.as_deref() == Some(mestre.id.as_str());
        // A resposta nova do Mestre aberto no painel ativo você já está lendo.
        let nao_lido = motor.nao_lido(mestre) && !aberto_aqui;
        let rodando = motor.execucao(&mestre.id).rodando();
        let cartoes = motor.cartoes().iter().filter(|c| c.aberto()).count();
        let selo = match selos_do_mestre(motor.cartoes(), espera, nao_lido) {
            MarcaDoSino::Numero(n, cor) => Some(Selo::contagem(n).solida(cor)),
            MarcaDoSino::Ponto => Some(Selo::ponto(Cores::destaque())),
            MarcaDoSino::Nada => None,
        };
        Some(
            BotaoMestre {
                ativo: aberto_aqui,
                selo,
                sinal: super::componentes::sinal_do_chat(motor, mestre),
                dica: dica_mestre(cartoes, espera, nao_lido, rodando).into(),
                janela: cx.entity().downgrade(),
            }
            .into_any_element(),
        )
    }

    /// Ctrl+M, o botão do trilho e "Falar com o Mestre": o Mestre no painel ativo (a vista da lista
    /// fica como está), com o foco na caixa dele.
    pub(super) fn abrir_mestre(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.motor.as_ref().and_then(|m| m.read(cx).mestre_id().map(str::to_string)) else {
            return;
        };
        self.selecionar(Some(id), window, cx);
        if let Some(composer) = self.composer_ativo() {
            let foco = composer.read(cx).focus_handle(cx);
            window.focus(&foco, cx);
        }
    }

    /// Abre o projeto na lista (ou volta à visão geral, se já era ele) e mostra a lista.
    pub(super) fn alternar_vista(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        let vista = if self.vista.projeto() == Some(projeto.as_str()) { Vista::Geral } else { Vista::Projeto(projeto) };
        self.abrir_vista(vista, window, cx);
    }

    /// Avulsos: o mesmo, com os chats sem projeto.
    fn alternar_avulsos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let vista = if self.vista == Vista::Avulsos { Vista::Geral } else { Vista::Avulsos };
        self.abrir_vista(vista, window, cx);
    }

    /// Troca a vista, volta a lista ao topo nas conversas e a mostra. Um projeto escolhido fica à
    /// vista no trilho.
    pub(super) fn abrir_vista(&mut self, vista: Vista, window: &mut Window, cx: &mut Context<Self>) {
        if self.vista != vista {
            self.filtro = Filtro::Todos;
            self.realce_lista = None;
            self.rolagem_lista.set_offset(Point::default());
            if vista == Vista::Avulsos && self.aba == AbaLateral::Arquivos {
                self.trocar_aba(AbaLateral::Conversas, window, cx);
            }
        }
        if let Some(motor) = &self.motor
            && let Some(ix) = vista.projeto().and_then(|p| motor.read(cx).projetos_visiveis().iter().position(|x| x.id == p))
        {
            self.rolagem_trilho.scroll_to_item(ix);
        }
        self.mudar_vista(vista, cx);
        self.mostrar_lista(window, cx);
    }

    /// A escolha em "Todos os projetos" abre o projeto; um arquivado volta antes.
    fn escolher_projeto(&mut self, chave: &str, window: &mut Window, cx: &mut Context<Self>) {
        let projeto = match chave.strip_prefix(CHAVE_ARQUIVADO) {
            Some(nome) => {
                self.desarquivar_projeto(nome.to_string(), cx);
                nome
            }
            None => chave,
        };
        self.abrir_vista(Vista::Projeto(projeto.to_string()), window, cx);
    }

    /// Ctrl+1…9: o projeto N do trilho, como o clique no atalho.
    pub(super) fn ir_para_projeto(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(projeto) = self
            .motor
            .as_ref()
            .and_then(|m| m.read(cx).projetos_visiveis().get(n).map(|p| p.id.clone()))
        else {
            return;
        };
        self.alternar_vista(projeto, window, cx);
    }

    /// O chat arrastado entrou (com os limites do lugar) ou saiu de um lugar do trilho.
    fn chat_sobre_o_trilho(&mut self, destino: &str, limites: Option<Bounds<Pixels>>, cx: &mut Context<Self>) {
        let era_daqui = self.chat_no_trilho.as_ref().is_some_and(|(p, _)| p == destino);
        let novo = limites.map(|l| (destino.to_string(), l));
        if (novo.is_some() || era_daqui) && self.chat_no_trilho != novo {
            self.chat_no_trilho = novo;
            cx.notify();
        }
    }

    /// Soltar um projeto sobre outro: vai antes ou depois dele, conforme a linha mostrava.
    fn soltar_projeto(&mut self, id: &str, alvo: &str, cx: &mut Context<Self>) {
        let antes = match self.alvo_trilho.take() {
            Some((p, antes)) if p == alvo => antes,
            _ => true,
        };
        if id != alvo
            && let Some(motor) = self.motor.clone()
        {
            motor.update(cx, |m, cx| m.reordenar_projeto(id, alvo, antes, cx));
        }
        cx.notify();
    }

    /// Soltar um chat num projeto do trilho (ou em Avulsos): move, perguntando antes quando o
    /// motor avisa de algo; a recusa vira aviso. O painel não fecha.
    pub(super) fn soltar_chat(&mut self, chat: &str, destino: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.chat_no_trilho = None;
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let nome = motor.read(cx).nome_do_projeto(destino);
        match motor.read(cx).pode_mover(chat, destino) {
            Err(recusa) => window.push_notification(aviso(Tom::Atencao, recusa), cx),
            Ok(None) => mover_agora(&motor, chat, destino, &nome, window, cx),
            Ok(Some(alerta)) => {
                let (chat, destino) = (chat.to_string(), destino.to_string());
                confirmar(
                    Confirmacao {
                        tom: TomConfirmacao::Neutro,
                        icone: Lucide::FolderInput.into(),
                        titulo: format!("Mover para {nome}?").into(),
                        texto: alerta.into(),
                        detalhes: Vec::new(),
                        ok: "Mover".into(),
                        cancelar: "Cancelar".into(),
                        ok_perigo: false,
                    },
                    move |window, cx| mover_agora(&motor, &chat, &destino, &nome, window, cx),
                    None,
                    window,
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// "Todos os projetos…" do menu do modo recolhido: a mesma lista do "+N", num diálogo.
    fn dialogo_todos_projetos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let grupos = grupos_de_projetos(motor.read(cx));
        let escolhido = self.vista.projeto().map(|p| SharedString::from(p.to_string()));
        let janela = cx.entity().downgrade();
        let lista = cx.new(|cx| {
            let mut lista = ListaDeEscolha::new("Buscar projeto", window, cx);
            lista.definir(grupos, escolhido, cx);
            lista.rodape(move |window, cx| {
                let janela = janela.clone();
                botao_fantasma("todos-novo-projeto", Some(Lucide::FolderPlus.into()), "Novo projeto…", TamanhoBotao::Barra, window, cx)
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        let _ = janela.update(cx, |this, cx| this.dialogo_criar_projeto(window, cx));
                    })
                    .into_any_element()
            });
            lista
        });
        self._assinatura_dialogo = Some(cx.subscribe_in(&lista, window, |this, _, evento: &EventoEscolha, window, cx| {
            window.close_dialog(cx);
            if let EventoEscolha::Escolheu(chave) = evento {
                this.escolher_projeto(chave, window, cx);
            }
        }));
        self.foco_antes_do_dialogo(window, cx);
        let vista = lista.clone();
        window.open_dialog(cx, move |dialogo, window, _| {
            dialogo
                .bg(Cores::dialogo())
                .w(tam(window, 400.))
                .title(div().titulo_tela().text_color(Cores::forte()).child("Todos os projetos"))
                .child(vista.clone())
        });
        lista.update(cx, |l, cx| l.abrir(window, cx));
    }

    /// Começo do cabeçalho do painel 1 com a lista fora da tela. No modo recolhido (Ctrl+B), o
    /// logo com o selo do sino, que abre o mesmo menu e, pelo item do sino, a caixa; com a lista
    /// na gaveta (janela estreita), o botão de mostrá-la.
    pub(super) fn logo_compacto(&self, id: &'static str, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let Some(motor) = self.motor.as_ref().filter(|_| !self.lista) else {
            return self.botao_lista(id, window, cx).into_any_element();
        };
        let m = motor.read(cx);
        let logo = self.botao_logo(m, true, super::cartoes::precisa_com_o_mestre(m), cx).into_any_element();
        sem_a_peca()
            .flex_none()
            .child(self.popover_caixa("caixa-logo", Gatilho(logo), Anchor::TopLeft, cx))
            .into_any_element()
    }
}

/// Move o chat e avisa onde ele ficou.
fn mover_agora(motor: &Entity<Motor>, chat: &str, destino: &str, nome: &str, window: &mut Window, cx: &mut App) {
    match motor.update(cx, |m, cx| m.mover_chat(chat, destino, cx)) {
        Ok(()) => window.push_notification(aviso(Tom::Pronto, format!("Chat movido para {nome}")), cx),
        Err(recusa) => window.push_notification(aviso(Tom::Atencao, recusa), cx),
    }
}

/// O arrasto perto da beirada da área dos projetos a rola, sem passar do começo nem do fim.
fn rolar_na_borda(rolagem: &ScrollHandle, limites: Bounds<Pixels>, y: Pixels, borda: Pixels, window: &mut Window) {
    let passo = rolagem_na_borda(limites, y, borda);
    if passo == px(0.) {
        return;
    }
    let atual = rolagem.offset();
    let minimo = -rolagem.max_offset().y;
    let novo = (atual.y + passo).clamp(minimo, px(0.));
    if novo != atual.y {
        rolagem.set_offset(point(atual.x, novo));
        window.refresh();
    }
}

/// O topo do menu no modo recolhido: mostrar a barra lateral, o sino e os projetos (até 9, com
/// Ctrl+1…9). Escolher um projeto liga o filtro e traz a barra lateral de volta.
fn itens_recolhido(
    menu: PopupMenu,
    janela: &WeakEntity<Janela>,
    precisa: &PrecisaDeVoce,
    projetos: &Rc<Vec<ProjetoNoMenu>>,
    total: usize,
) -> PopupMenu {
    let selo = selo_do_menu(precisa);
    let caixa = janela.clone();
    menu.item(
        item("Mostrar a lista", Lucide::PanelLeftOpen, janela, |this, window, cx| {
            this.alternar_lista(window, cx)
        })
        .action(Box::new(AlternarLista)),
    )
    .item(
        PopupMenuItem::element(move |_, _| {
            h_flex()
                .gap(esp::E8)
                .min_w_0()
                .child(div().flex_1().min_w_0().truncate().child("O que precisa de você"))
                .children(selo.clone())
        })
        .icon(Lucide::Bell)
        .on_click(move |_, _, cx| {
            let _ = caixa.update(cx, |this, cx| {
                this.caixa_aberta = true;
                cx.notify();
            });
        }),
    )
    .separator()
    .when(!projetos.is_empty(), |menu| {
        let mut menu = menu.label("Projetos");
        for (i, projeto) in projetos.iter().enumerate() {
            let lista = projetos.clone();
            let janela = janela.clone();
            let nome = projeto.id.clone();
            menu = menu.item(
                PopupMenuItem::element(move |_, _| {
                    let p = &lista[i];
                    h_flex()
                        .gap(esp::E8)
                        .min_w_0()
                        .child(QuadradoDeProjeto::new(p.icone, p.cor, 20.))
                        .child(div().flex_1().min_w_0().truncate().child(p.nome.clone()))
                        .when_some(p.sinal, |s, sinal| s.child(sinal.ponto(6.)))
                        .when(p.rodando > 0, |s| {
                            s.child(
                                div()
                                    .flex_none()
                                    .metadado()
                                    .text_color(Cores::tenue())
                                    .child(format!("{} rodando", p.rodando)),
                            )
                        })
                })
                .checked(projeto.filtrado)
                .action(Box::new(IrParaProjeto(i)))
                .on_click(move |_, window, cx| {
                    let nome = nome.clone();
                    let _ = janela.update(cx, |this, cx| this.alternar_vista(nome, window, cx));
                }),
            );
        }
        menu.when(total > projetos.len(), |menu| {
            menu.item(item("Todos os projetos…", Lucide::LayoutGrid, janela, |this, window, cx| {
                this.dialogo_todos_projetos(window, cx)
            }))
        })
        .separator()
    })
}

/// A seção "Projetos arquivados" do menu do Vitral: cada um com o quadrado, o nome e
/// "Desarquivar", que o devolve ao trilho.
fn itens_arquivados(menu: PopupMenu, janela: &WeakEntity<Janela>, arquivados: &Rc<Vec<ProjetoArquivado>>) -> PopupMenu {
    let mut menu = menu.label("Projetos arquivados");
    for (i, projeto) in arquivados.iter().enumerate() {
        let (lista, janela, nome) = (arquivados.clone(), janela.clone(), projeto.id.clone());
        menu = menu.item(
            PopupMenuItem::element(move |_, _| {
                let p = &lista[i];
                h_flex()
                    .gap(esp::E8)
                    .min_w_0()
                    .child(div().flex_none().opacity(0.6).child(QuadradoDeProjeto::new(p.icone, p.cor, 20.)))
                    .child(div().flex_1().min_w_0().truncate().text_color(Cores::tenue()).child(p.nome.clone()))
                    .child(
                        h_flex()
                            .flex_none()
                            .gap(esp::E4)
                            .metadado()
                            .text_color(Cores::destaque_texto())
                            .child(svg().path(Lucide::ArchiveRestore.path()).flex_none().size(r(14.)).text_color(Cores::destaque_texto()))
                            .child("Desarquivar"),
                    )
            })
            .on_click(move |_, _, cx| {
                let _ = janela.update(cx, |this, cx| this.desarquivar_projeto(nome.clone(), cx));
            }),
        );
    }
    menu.separator()
}

impl Janela {
    /// "Ver na lista" da caixa do sino (e o que o clique no sino fazia antes): a visão geral, sem
    /// busca nem arquivadas, no topo, onde fica "Esperando você".
    pub(super) fn mostrar_esperando(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mudar_vista(Vista::Geral, cx);
        self.filtro = Filtro::Todos;
        self.rolagem_lista.set_offset(Point::default());
        self.trocar_aba(AbaLateral::Conversas, window, cx);
        self.mostrar_lista(window, cx);
    }
}

#[cfg(test)]
mod testes {
    use super::{
        Atividade, MarcaDoSino, antes_ou_depois, dica_mestre, dica_projeto, dica_sino, marca_do_sino, rolagem_na_borda,
        selos_do_mestre,
    };
    use crate::modelo::{Cartao, EstadoCartao, TipoCartao};
    use gpui_kit::{Bounds, point, px, size};
    use crate::{
        motor::{Espera, PrecisaDeVoce},
        tema::Cores,
    };

    #[test]
    fn dicas_sem_partes_zeradas() {
        let nada = PrecisaDeVoce::default();
        assert_eq!(dica_sino(&nada), "O que precisa de você");
        assert_eq!(dica_projeto("site", None, false), "site");
        assert_eq!(
            dica_projeto("site", Some(&Atividade::default()), true),
            "site\nclique para voltar à Visão geral"
        );
        let mut espera = PrecisaDeVoce::default();
        for e in [Espera::Pedido, Espera::Parada, Espera::Nova, Espera::Nova] {
            espera.somar(e, false);
        }
        assert_eq!(dica_sino(&espera), "O que precisa de você\n1 pedido, 1 parada, 2 respostas novas");
        let a = Atividade { rodando: 2, espera, sinal: None };
        assert_eq!(dica_projeto("loja-api", Some(&a), false), "loja-api\n2 rodando · 4 esperando você");
    }

    #[test]
    fn sino_conta_pedidos_erros_e_paradas_e_as_novas_viram_ponto() {
        let mut p = PrecisaDeVoce::default();
        assert_eq!(marca_do_sino(&p), MarcaDoSino::Nada);
        p.somar(Espera::Nova, false);
        assert_eq!(marca_do_sino(&p), MarcaDoSino::Ponto);
        p.somar(Espera::Pedido, false);
        p.somar(Espera::Parada, false);
        assert_eq!(marca_do_sino(&p), MarcaDoSino::Numero(2, Cores::ambar()));
        // A demonstração: 2 pedidos, 1 erro e 1 parada dão "4", em vermelho.
        let mut demo = PrecisaDeVoce::default();
        for (e, erro) in [(Espera::Pedido, false), (Espera::Pedido, true), (Espera::Erro, true), (Espera::Parada, false)] {
            demo.somar(e, erro);
        }
        assert_eq!(marca_do_sino(&demo), MarcaDoSino::Numero(4, Cores::vermelho()));
    }

    fn cartao(id: u64, estado: EstadoCartao) -> Cartao {
        Cartao {
            id,
            chat: "mestre".into(),
            criado_em: 0,
            decidido_em: 0,
            estado,
            tipo: TipoCartao::Memoria { texto: "x".into() },
            resto: Default::default(),
        }
    }

    #[test]
    fn selos_do_mestre_contam_cartoes_abertos_e_a_espera_e_o_ponto_e_do_nao_lido() {
        let nenhum: Vec<Cartao> = Vec::new();
        assert_eq!(selos_do_mestre(&nenhum, None, false), MarcaDoSino::Nada);
        assert_eq!(selos_do_mestre(&nenhum, None, true), MarcaDoSino::Ponto);
        // Os decididos não contam; a resposta nova também não vira número.
        let cartoes = vec![cartao(1, EstadoCartao::Aberto), cartao(2, EstadoCartao::Aceito), cartao(3, EstadoCartao::Aberto)];
        assert_eq!(selos_do_mestre(&cartoes, Some(Espera::Nova), true), MarcaDoSino::Numero(2, Cores::ambar()));
        assert_eq!(selos_do_mestre(&cartoes, Some(Espera::Pedido), false), MarcaDoSino::Numero(3, Cores::ambar()));
        // O erro conta um e pinta de vermelho.
        assert_eq!(selos_do_mestre(&nenhum, Some(Espera::Erro), true), MarcaDoSino::Numero(1, Cores::vermelho()));
        assert_eq!(
            dica_mestre(2, None, false, true),
            "Mestre\n2 cartões esperando você · pensando…"
        );
        assert_eq!(dica_mestre(0, None, true, false), "Mestre\nresposta nova");
    }

    #[test]
    fn antes_ou_depois_pela_metade_do_lugar() {
        let lugar = Bounds { origin: point(px(0.), px(100.)), size: size(px(40.), px(40.)) };
        assert!(antes_ou_depois(lugar, px(100.)));
        assert!(antes_ou_depois(lugar, px(119.)));
        assert!(!antes_ou_depois(lugar, px(120.)));
        assert!(!antes_ou_depois(lugar, px(139.)));
    }

    #[test]
    fn a_area_rola_so_perto_da_beirada() {
        let area = Bounds { origin: point(px(0.), px(100.)), size: size(px(52.), px(400.)) };
        let borda = px(24.);
        assert_eq!(rolagem_na_borda(area, px(300.), borda), px(0.));
        // Perto de cima rola para cima (deslocamento positivo), mais rápido mais perto.
        assert_eq!(rolagem_na_borda(area, px(104.), borda), px(10.));
        assert_eq!(rolagem_na_borda(area, px(100.), borda), px(12.));
        assert_eq!(rolagem_na_borda(area, px(490.), borda), px(-7.));
        // Fora da área, nada.
        assert_eq!(rolagem_na_borda(area, px(90.), borda), px(0.));
        assert_eq!(rolagem_na_borda(area, px(510.), borda), px(0.));
    }
}
