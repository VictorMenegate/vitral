//! Trilho à esquerda, numa peça de vidro própria: o logo (menu do Vitral, F10), o lugar do Mestre,
//! um atalho por projeto (filtra a lista; "+N" quando não cabem todos), o sino do que precisa de
//! você e o uso dos planos. A área vazia arrasta a janela. Com Ctrl+B o trilho some junto com a
//! lista; o logo, com o selo do sino, e os anéis de uso vão para o 1º painel (modo recolhido).

use std::{
    cell::Cell,
    collections::HashMap,
    path::Path,
    rc::Rc,
    sync::{Arc, LazyLock},
};

use gpui_kit::base::actions::SelectDown;
use gpui_kit::component::{
    ElementExt as _, Selectable, WindowExt as _, h_flex,
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    popover::Popover,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    AbaLateral, AbrirConfiguracoes, AbrirMenuVitral, AlternarLista, Filtro, IrParaProjeto, Janela,
    NovaConversa, ProximaPendencia,
    componentes::{
        BotaoRedondo, EventoEscolha, GrupoDeEscolha, ItemDeEscolha, ListaDeEscolha, QuadradoDeProjeto, Reflexo as _,
        Selo, Simbolo, Sinal, TamanhoBotao, VarianteRedonda, anel_do_pulso, botao_fantasma,
        cabecalho_objeto_com_dica, contagem, popover, rodape_menu_com_dica, sinal_do_projeto,
    },
    icone_projeto,
    menus::itens_do_projeto,
    movimento::{self, Curva},
    uso_plano,
};
use crate::{
    atualizacao,
    modelo::{Execucao, Papel},
    motor::{Motor, PrecisaDeVoce},
    tema::{Cores, Lucide, Tipo as _, esp, halo_foco, r, raio, tam},
};

/// O logo pequeno, desenhado para 16 a 32 px. Ele declara 32 px: o GPUI rasteriza o SVG no dobro
/// do tamanho declarado e reduz na placa de vídeo sem mipmaps, e de 256 para 32 px o chumbo fino
/// sairia serrilhado. Guardado uma vez só (o trilho redesenha a cada quadro).
static LOGO: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../assets/vitral-pequeno.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});

/// Medidas da peça a 100% (crescem com o zoom): largura, lugar de um projeto, o que vem antes do
/// primeiro lugar (margem 4, logo 36, 8, separador 1 e 4), o lugar do Mestre com a folga e o que
/// vem depois do último (folga mínima 8, e sino, Claude e Codex de 36 com 8 depois de cada um).
const LARGURA: f32 = 52.;
const LUGAR: f32 = 40.;
const ACIMA: f32 = 53.;
const MESTRE: f32 = 52.;
const ABAIXO: f32 = 140.;

thread_local! {
    /// Altura da peça no último quadro, em px a 100%: os lugares de projeto saem dela. É medida
    /// no desenho, porque o trilho não recebe a janela.
    static ALTURA: Cell<f32> = const { Cell::new(0.) };
}

/// Quantos lugares de 40 cabem entre o logo (ou o Mestre) e o sino; sem medida ainda, todos.
fn vagas(altura: f32, mestre: bool) -> usize {
    if altura <= 0. {
        return usize::MAX;
    }
    let zona = altura - ACIMA - ABAIXO - if mestre { MESTRE } else { 0. };
    (zona / LUGAR).floor().max(1.) as usize
}

/// Os projetos à vista com `vagas` lugares e quantos ficam no "+N" (0: cabem todos, com o "Novo
/// projeto" no fim). O ativo sempre aparece: além do limite, ele fica no último lugar antes do
/// "+N".
fn zona(total: usize, vagas: usize, ativo: Option<usize>) -> (Vec<usize>, usize) {
    if total < vagas {
        return ((0..total).collect(), 0);
    }
    let cabem = vagas.saturating_sub(1).min(total);
    let mut vistos: Vec<usize> = (0..cabem).collect();
    if let Some(a) = ativo.filter(|a| *a >= cabem)
        && let Some(ultimo) = vistos.last_mut()
    {
        *ultimo = a;
    }
    (vistos, total - cabem)
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
        if c.papel == Papel::Coordenador
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
/// zeradas); com o filtro ligado, como voltar a ver todos.
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
        partes.push("clique para ver todos".into());
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
enum MarcaDoSino {
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
fn cabecalho_vitral(raiz: &Path) -> PopupMenuItem {
    let caminho = pasta_curta(raiz);
    let nome = raiz.file_name().map_or_else(|| caminho.clone(), |n| n.to_string_lossy().into_owned());
    cabecalho_objeto_com_dica(Simbolo::Imagem(LOGO.clone()), "Vitral", Some(nome.into()), caminho)
}

/// Um projeto no menu do modo recolhido.
struct ProjetoNoMenu {
    nome: SharedString,
    icone: &'static icone_projeto::Icone,
    cor: u32,
    sinal: Option<Sinal>,
    rodando: usize,
    filtrado: bool,
}

/// Um projeto arquivado no menu do Vitral.
struct ProjetoArquivado {
    nome: SharedString,
    icone: &'static icone_projeto::Icone,
    cor: u32,
}

type ConstruirMenu = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

/// O logo redondo: o medalhão ocupa o botão inteiro (sem disco atrás); no hover e com o menu
/// aberto ganha um aro. No modo recolhido ele leva o selo do
/// sino, que some junto com o trilho.
#[derive(IntoElement)]
struct BotaoLogo {
    lado: f32,
    medalhao: f32,
    aberto: bool,
    selo: Option<Selo>,
    /// No modo recolhido, o pulso do sino (a geração e a cor), que vai no logo com o selo.
    pulso: Option<(u64, Hsla)>,
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
                s.hover(|s| s.border_color(Cores::borda())).tooltip(|window, cx| {
                    Tooltip::new("Menu do Vitral").action(&AbrirMenuVitral, None).build(window, cx)
                })
            })
            .child(img(LOGO.clone()).flex_none().size(r(self.medalhao)))
            .children(pulso)
            .when_some(self.selo, |s, selo| {
                s.child(div().absolute().top(r(-3.)).right(r(-3.)).child(selo))
            })
    }
}

/// O "+N" no último lugar: abre "Todos os projetos".
#[derive(IntoElement)]
struct BotaoMais {
    resto: usize,
    aberto: bool,
}

impl Selectable for BotaoMais {
    fn selected(mut self, selected: bool) -> Self {
        self.aberto = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.aberto
    }
}

impl RenderOnce for BotaoMais {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = ElementId::from("trilho-mais");
        let ponteiro = movimento::Hover::de(movimento::chave(&id, "hover"), window, cx);
        let alvo = if self.aberto || ponteiro.ativo(cx) { Cores::pressionado() } else { Cores::hover() };
        let fundo = movimento::valor(movimento::chave(&id, "fundo"), alvo, movimento::COR, Curva::Sair, window, cx);
        div()
            .id(id)
            .flex_none()
            .size(r(32.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(raio::CONTROLE)
            .cursor_pointer()
            .tab_index(0)
            .focus_visible(|s| s.shadow(halo_foco()))
            .on_hover(ponteiro.ouvinte())
            .border_1()
            .border_color(if self.aberto { Cores::borda() } else { transparent_black() })
            .bg(fundo)
            .when(!self.aberto, |s| s.tooltip(|window, cx| Tooltip::new("Todos os projetos").build(window, cx)))
            .micro()
            .text_color(Cores::texto())
            .child(format!("+{}", self.resto))
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
        .projetos()
        .into_iter()
        .map(|projeto| {
            let (icone, cor) = icone_projeto::visual(motor, &projeto);
            let sinal = atividades.get(&projeto).and_then(|a| a.sinal);
            let mut item = ItemDeEscolha::new(projeto.clone(), projeto);
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
            let (icone, cor) = icone_projeto::visual(motor, &projeto);
            let mut item = ItemDeEscolha::new(format!("{CHAVE_ARQUIVADO}{projeto}"), projeto);
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

/// Mede a peça e, quando a altura muda, pede outro quadro com os lugares novos.
fn medir_peca(janela: WeakEntity<Janela>) -> impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static {
    move |limites, window, _| {
        let altura = limites.size.height.as_f32() / window.rem_size().as_f32() * 15.;
        if (altura - ALTURA.get()).abs() < 0.5 {
            return;
        }
        ALTURA.set(altura);
        window.on_next_frame(move |_, cx| {
            let _ = janela.update(cx, |_, cx| cx.notify());
        });
    }
}

/// O atalho de um projeto no trilho: o ponteiro no lugar de 40 acende o quadrado em 120 ms (o
/// hover suave), e o filtro liga o aro na mesma transição.
#[derive(IntoElement)]
struct AtalhoDeProjeto {
    ix: usize,
    projeto: String,
    icone: &'static icone_projeto::Icone,
    cor: u32,
    filtrado: bool,
    sinal: Option<Sinal>,
    dica: SharedString,
    janela: WeakEntity<Janela>,
    motor: Entity<Motor>,
}

impl RenderOnce for AtalhoDeProjeto {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let AtalhoDeProjeto { ix, projeto, icone, cor, filtrado, sinal, dica, janela, motor } = self;
        let id = ElementId::from(("trilho-projeto", ix));
        let hover = movimento::Hover::de(movimento::chave(&id, "ponteiro"), window, cx);
        let quadrado = QuadradoDeProjeto::new(icone, cor, 32.)
            .ativo(filtrado)
            .realcado(filtrado || hover.ativo(cx))
            .sinal(sinal)
            .suave(id.clone());
        let (j_clique, j_menu) = (janela.clone(), janela);
        let nome = projeto.clone();
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
                .on_hover(hover.ouvinte())
                .child(quadrado)
                .tooltip(move |window, cx| {
                    let dica = Tooltip::new(dica.clone());
                    // Ctrl+1…9 vão para os nove primeiros.
                    if ix < 9 { dica.action(&IrParaProjeto(ix), None) } else { dica }.build(window, cx)
                })
                .on_click(move |_, window, cx| {
                    let _ = j_clique.update(cx, |this, cx| this.alternar_filtro(nome.clone(), window, cx));
                })
                .context_menu(move |menu, window, cx| {
                    itens_do_projeto(menu, j_menu.clone(), &motor, &projeto, true, window, cx)
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

    pub(super) fn trilho(&self, motor: &Entity<Motor>, cx: &Context<Self>) -> impl IntoElement + use<> {
        let m = motor.read(cx);
        let projetos = m.projetos();
        let atividades = atividades(m);
        let precisa = m.precisa_de_voce(None);
        let mestre = self.botao_mestre(m, cx);
        let ativo = self
            .projeto_filtro
            .as_ref()
            .and_then(|f| projetos.iter().position(|p| p == f));
        let (vistos, resto) = zona(projetos.len(), vagas(ALTURA.get(), mestre.is_some()), ativo);
        let atalhos: Vec<AnyElement> = vistos
            .into_iter()
            .map(|ix| {
                let projeto = &projetos[ix];
                self.atalho_projeto(ix, projeto, motor, atividades.get(projeto), cx).into_any_element()
            })
            .collect();
        let ultimo = if resto > 0 {
            self.popover_todos(resto, motor, cx).into_any_element()
        } else {
            self.botao_novo_projeto(projetos.is_empty(), cx).into_any_element()
        };

        self.arrastavel_com_id("arrastar-trilho")
            .on_mouse_down(MouseButton::Right, |evento, window, _| {
                window.show_window_menu(evento.position)
            })
            .on_prepaint(medir_peca(cx.entity().downgrade()))
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
            .when(!projetos.is_empty(), |s| {
                s.child(div().flex_none().w(r(20.)).h(px(1.)).mt(esp::E8).mb(esp::E4).bg(Cores::borda()))
            })
            .when(projetos.is_empty(), |s| s.child(div().h(r(13.))))
            .children(atalhos)
            .child(ultimo)
            .child(div().flex_1().min_h(r(8.)))
            .child(sem_a_peca().child(self.botao_sino(precisa, cx)))
            .child(sem_a_peca().mt(esp::E8).child(self.uso_plano(cx)))
    }

    /// Um lugar de 40 com o quadrado do projeto: liga ou desliga o filtro; o clique direito abre o
    /// menu do projeto.
    fn atalho_projeto(
        &self,
        ix: usize,
        projeto: &str,
        motor: &Entity<Motor>,
        atividade: Option<&Atividade>,
        cx: &Context<Self>,
    ) -> AtalhoDeProjeto {
        let (icone, cor) = icone_projeto::visual(motor.read(cx), projeto);
        let filtrado = self.projeto_filtro.as_deref() == Some(projeto);
        AtalhoDeProjeto {
            ix,
            projeto: projeto.to_string(),
            icone,
            cor,
            filtrado,
            sinal: atividade.and_then(|a| a.sinal),
            dica: dica_projeto(projeto, atividade, filtrado).into(),
            janela: cx.entity().downgrade(),
            motor: motor.clone(),
        }
    }

    /// "Novo projeto" no fim dos atalhos, quando todos cabem: só o contorno e o +.
    fn botao_novo_projeto(&self, primeiro: bool, cx: &Context<Self>) -> BotaoNovoProjeto {
        BotaoNovoProjeto { primeiro, janela: cx.entity().downgrade() }
    }

    /// O "+N" e o popover "Todos os projetos": busca, Enter ou clique liga o filtro, e "Novo
    /// projeto…" no rodapé.
    fn popover_todos(&self, resto: usize, motor: &Entity<Motor>, cx: &Context<Self>) -> impl IntoElement + use<> {
        let janela = cx.entity().downgrade();
        let motor = motor.clone();
        let filtro = self.projeto_filtro.clone();
        sem_a_peca().size(r(LUGAR)).flex().items_center().justify_center().child(
            popover("trilho-todos")
                .anchor(Anchor::LeftCenter)
                .offset(px(8.))
                .arrow(true)
                .trigger(BotaoMais { resto, aberto: false })
                .content(move |_, window, cx| {
                    // Feita uma vez por abertura, como o menu do Vitral: o estado do elemento some
                    // quando o popover fecha.
                    let guardada = window.use_keyed_state("trilho-lista-projetos", cx, |_, _| None::<Entity<ListaDeEscolha>>);
                    let lista = match guardada.read(cx).clone() {
                        Some(lista) => lista,
                        None => {
                            let popover = cx.entity().downgrade();
                            let (j1, j2, p1) = (janela.clone(), janela.clone(), popover.clone());
                            let grupos = grupos_de_projetos(motor.read(cx));
                            let escolhido = filtro.clone().map(SharedString::from);
                            let lista = cx.new(|cx| {
                                let mut lista = ListaDeEscolha::new("Buscar projeto", window, cx);
                                lista.definir(grupos, escolhido, cx);
                                lista.rodape(move |window, cx| {
                                    let (janela, popover) = (j1.clone(), p1.clone());
                                    botao_fantasma("todos-novo-projeto", Some(Lucide::FolderPlus.into()), "Novo projeto…", TamanhoBotao::Barra, window, cx)
                                        .on_click(move |_, window, cx| {
                                            let _ = popover.update(cx, |p, cx| p.dismiss(window, cx));
                                            let _ = janela.update(cx, |this, cx| this.dialogo_criar_projeto(window, cx));
                                        })
                                        .into_any_element()
                                });
                                lista
                            });
                            window
                                .subscribe(&lista, cx, move |_, evento: &EventoEscolha, window, cx| {
                                    if let EventoEscolha::Escolheu(chave) = evento {
                                        let _ = j2.update(cx, |this, cx| this.escolher_projeto(chave, window, cx));
                                    }
                                    let _ = popover.update(cx, |p, cx| p.dismiss(window, cx));
                                })
                                .detach();
                            lista.update(cx, |l, cx| l.abrir(window, cx));
                            guardada.update(cx, |g, _| *g = Some(lista.clone()));
                            lista
                        }
                    };
                    div().w(tam(window, 288.).min(window.viewport_size().width - px(64.))).child(lista)
                }),
        )
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
    /// No modo recolhido, antes deles, o que o trilho mostrava: a lista, o sino e os projetos.
    fn itens_menu(&self, motor: &Motor, compacto: bool, precisa: PrecisaDeVoce, cx: &Context<Self>) -> ConstruirMenu {
        let janela = cx.entity().downgrade();
        let raiz = motor.raiz().to_path_buf();
        let sem_codex = motor.codex_lido && motor.codex.is_none();
        let todos = motor.projetos();
        let total = todos.len();
        let arquivados: Rc<Vec<ProjetoArquivado>> = Rc::new(
            motor
                .projetos_arquivados()
                .into_iter()
                .map(|nome| {
                    let (icone, cor) = icone_projeto::visual(motor, &nome);
                    ProjetoArquivado { nome: nome.into(), icone, cor }
                })
                .collect(),
        );
        let projetos: Rc<Vec<ProjetoNoMenu>> = Rc::new(if compacto {
            let atividades = atividades(motor);
            todos
                .into_iter()
                .take(9)
                .map(|nome| {
                    let (icone, cor) = icone_projeto::visual(motor, &nome);
                    let a = atividades.get(&nome);
                    ProjetoNoMenu {
                        filtrado: self.projeto_filtro.as_deref() == Some(nome.as_str()),
                        sinal: a.and_then(|a| a.sinal),
                        rodando: a.map_or(0, |a| a.rodando),
                        nome: nome.into(),
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
                .item(cabecalho_vitral(&raiz))
                .separator()
                .when(compacto, |menu| itens_recolhido(menu, &janela, &precisa, &projetos, total))
                .item(
                    item("Nova conversa", Lucide::MessageSquarePlus, &janela, |this, window, cx| {
                        this.dialogo_nova_conversa(window, cx)
                    })
                    .action(Box::new(NovaConversa)),
                )
                .item(item("Novo projeto…", Lucide::FolderPlus, &janela, |this, window, cx| {
                    this.dialogo_criar_projeto(window, cx)
                }))
                .item(item("Rotinas", Lucide::CalendarClock, &janela, |this, window, cx| {
                    this.dialogo_rotinas(window, cx)
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
                .item(
                    item("Configurações", Lucide::Settings, &janela, |this, window, cx| {
                        this.abrir_configuracoes(window, cx)
                    })
                    .action(Box::new(AbrirConfiguracoes)),
                )
                .separator()
                .item(item("Trocar a pasta…", Lucide::FolderInput, &janela, |this, window, cx| {
                    this.escolher_pasta(window, cx)
                }))
                .item(
                    PopupMenuItem::new("Abrir a pasta")
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

    /// Lugar do Mestre logo abaixo do logo: nada até o motor ter o chat Mestre (L6 do Mestre).
    fn botao_mestre(&self, _motor: &Motor, _cx: &Context<Self>) -> Option<AnyElement> {
        None
    }

    /// Liga o filtro do projeto (ou desliga, se já era ele) e mostra a lista, onde ele aparece.
    fn alternar_filtro(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        self.projeto_filtro = if self.projeto_filtro.as_deref() == Some(projeto.as_str()) {
            None
        } else {
            Some(projeto)
        };
        self.mostrar_lista(window, cx);
    }

    /// A escolha em "Todos os projetos" liga o filtro; um arquivado volta antes.
    fn escolher_projeto(&mut self, chave: &str, window: &mut Window, cx: &mut Context<Self>) {
        let projeto = match chave.strip_prefix(CHAVE_ARQUIVADO) {
            Some(nome) => {
                self.desarquivar_projeto(nome.to_string(), cx);
                nome
            }
            None => chave,
        };
        self.projeto_filtro = Some(projeto.to_string());
        self.mostrar_lista(window, cx);
    }

    /// Ctrl+1…9: o filtro do projeto N do trilho, como o clique no atalho.
    pub(super) fn ir_para_projeto(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(projeto) = self.motor.as_ref().and_then(|m| m.read(cx).projetos().into_iter().nth(n)) else {
            return;
        };
        self.alternar_filtro(projeto, window, cx);
    }

    /// "Todos os projetos…" do menu do modo recolhido: a mesma lista do "+N", num diálogo.
    fn dialogo_todos_projetos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let grupos = grupos_de_projetos(motor.read(cx));
        let escolhido = self.projeto_filtro.clone().map(SharedString::from);
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
        let logo = self.botao_logo(m, true, m.precisa_de_voce(None), cx).into_any_element();
        sem_a_peca()
            .flex_none()
            .child(self.popover_caixa("caixa-logo", Gatilho(logo), Anchor::TopLeft, cx))
            .into_any_element()
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
            let nome = projeto.nome.to_string();
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
                    let _ = janela.update(cx, |this, cx| this.alternar_filtro(nome, window, cx));
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
        let (lista, janela, nome) = (arquivados.clone(), janela.clone(), projeto.nome.to_string());
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
    /// "Ver na lista" da caixa do sino (e o que o clique no sino fazia antes): sem filtro de
    /// projeto, busca nem arquivadas, a lista do topo, onde fica "Esperando você".
    pub(super) fn mostrar_esperando(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.projeto_filtro = None;
        self.filtro = Filtro::Todos;
        self.rolagem_lista.set_offset(Point::default());
        self.trocar_aba(AbaLateral::Conversas, window, cx);
        self.mostrar_lista(window, cx);
    }
}

#[cfg(test)]
mod testes {
    use super::{Atividade, MarcaDoSino, dica_projeto, dica_sino, marca_do_sino, vagas, zona};
    use crate::{
        motor::{Espera, PrecisaDeVoce},
        tema::Cores,
    };

    #[test]
    fn dicas_sem_partes_zeradas() {
        let nada = PrecisaDeVoce::default();
        assert_eq!(dica_sino(&nada), "O que precisa de você");
        assert_eq!(dica_projeto("site", None, false), "site");
        assert_eq!(dica_projeto("site", Some(&Atividade::default()), true), "site\nclique para ver todos");
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

    #[test]
    fn lugares_mais_n_e_o_ativo_sempre_a_vista() {
        // 920 de altura (peça de 904): 17 lugares; a 600 com o Mestre, 5 ou menos.
        assert_eq!(vagas(904., false), 17);
        assert_eq!(vagas(584., true), 8);
        assert_eq!(vagas(0., false), usize::MAX);
        assert_eq!(vagas(100., true), 1);
        // Cabem todos e o "Novo projeto".
        assert_eq!(zona(3, 5, None), (vec![0, 1, 2], 0));
        assert_eq!(zona(4, 5, Some(3)), (vec![0, 1, 2, 3], 0));
        assert_eq!(zona(0, 1, None), (vec![], 0));
        // Não cabem: os primeiros K − 1 e o "+N" no último lugar.
        assert_eq!(zona(5, 5, None), (vec![0, 1, 2, 3], 1));
        assert_eq!(zona(16, 5, Some(1)), (vec![0, 1, 2, 3], 12));
        // O ativo além do limite toma o último lugar antes do "+N".
        assert_eq!(zona(16, 5, Some(10)), (vec![0, 1, 2, 10], 12));
        // Um lugar só: tudo no "+N".
        assert_eq!(zona(3, 1, Some(2)), (vec![], 3));
        // Sem medida ainda, todos.
        assert_eq!(zona(16, vagas(0., false), None).1, 0);
    }
}
