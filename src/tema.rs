//! Tema do Vitral. Nasceu como "vidro azul-marinho", tirado da Stained Glass UI do VS Code (janela
//! translúcida com tom navy, peças de 20 px, destaque azul #3A7FFF). Em 12/11/2026 o dono pediu
//! "mais moderno e com menos cara de IA", e o acabamento em uso passou a ser o "Grafite": as mesmas
//! peças em degraus, num cinza frio quase sem cor, chapado (sem gradiente, reflexo nem halo) e com
//! um destaque só, o azul. Os cantos continuam bem arredondados (10, 14 e 20, pílula nos botões e
//! chips): o dono viu a versão de cantos curtos e disse que prefere assim. O marinho de antes
//! continua em `superficie::MARINHO`: trocar o `ATUAL` volta a ele.

use std::{borrow::Cow, rc::Rc};

use gpui_kit::component::{Theme, ThemeSet};
use gpui_kit::{
    Anchor, App, AssetSource, Background, BoxShadow, FontFeatures, FontWeight, Hsla, Pixels, Rems, SharedString,
    Styled, Window, linear_color_stop, linear_gradient, px, rems, rgba,
};

/// Medida de desenho a 100% (1 rem = 15 px); cresce com o zoom.
pub const fn r(px: f32) -> Rems {
    rems(px / 15.)
}

/// A mesma medida em Pixels, para as APIs que só aceitam Pixels (`Icon::with_size`,
/// `PopupMenu::min_w`, `canvas`).
pub fn tam(window: &Window, px: f32) -> Pixels {
    window.rem_size() * (px / 15.)
}

/// Raios: três medidas e a pílula, fixos (o `Button::rounded` só aceita Pixels, e canto que não
/// cresce não incomoda). Dentro de quem tem padding p, o raio é max(raio de fora − p, CONTROLE),
/// para os cantos ficarem concêntricos; o teste `raios_na_escala` barra o resto.
pub mod raio {
    use gpui_kit::{Pixels, px};
    /// Campos, linhas (lista, menu, árvore, log), teclas, blocos dentro de cartão, miniaturas.
    pub const CONTROLE: Pixels = px(10.);
    /// Cartões dentro de painéis e popovers, blocos de código e tabelas, balões, alertas, grupos.
    pub const CARTAO: Pixels = px(14.);
    /// Peças: trilho, lista, conversa, Equipe, gavetas, popovers e menus, diálogos, avisos e a
    /// caixa de mensagem.
    pub const PECA: Pixels = px(20.);
    /// Cheio (o GPUI corta o raio na metade do lado menor): o `rounded_full()` de quem só aceita
    /// Pixels. Botões, chips, abas, segmentado, selos, avatares, pontos, anéis e barras.
    pub const PILULA: Pixels = px(999.);

    /// O padrão de canto dos controles (botões, campos, abas, chips, selos): bem arredondado,
    /// nunca redondo. É o desenho do logo em escala: 45% da altura (16 num controle de 36, 14 num
    /// de 32, 9 num chip de 20; redondo seria 50%). Só ponto, barra fina e anel continuam redondos.
    pub fn de(altura: f32) -> Pixels {
        px((altura * 0.45).round())
    }

    /// O quadrado do projeto é um ícone: o de 32 (o do trilho) tem o raio de controle, e os
    /// outros lados levam o mesmo desenho em escala.
    pub fn do_icone(lado: f32) -> Pixels {
        CONTROLE * (lado / 32.)
    }
}

/// Espaços da grade, em rem: só 2, 4, 6, 8, 12, 16, 24 e 32 (o teste `espacos_na_escala` barra
/// o resto nos `gap`, `p*` e `m*`). Os atalhos do Tailwind (`gap_2()`, `p_3()`) medem em rem e,
/// com 1 rem = 15 px, saem fora da grade.
pub mod esp {
    use super::r;
    use gpui_kit::Rems;
    /// Só óptico, dentro de controle: entre segmentos, entre cartões da lista.
    pub const E2: Rems = r(2.);
    /// Entre botões de ícone vizinhos; entre linhas de um mesmo bloco.
    pub const E4: Rems = r(4.);
    /// Só dentro de controle: ícone e rótulo em chip, selo e pílula.
    pub const E6: Rems = r(6.);
    /// Padding das peças; frestas; entre cartões; ícone e texto em linhas.
    pub const E8: Rems = r(8.);
    /// Padding de toda caixa no fluxo (cartão, código, balão, log, alerta); vão dentro do turno.
    pub const E12: Rems = r(12.);
    /// Padding de popover e da caixa de mensagem; respiro antes de cabeçalho de seção.
    pub const E16: Rems = r(16.);
    /// Entre turnos; margem de diálogo.
    pub const E24: Rems = r(24.);
    /// Estados vazios e telas cheias.
    pub const E32: Rems = r(32.);

    /// Padding horizontal da lista, da Equipe e da gaveta.
    pub const PECA: Rems = E8;
    /// Conteúdo de qualquer caixa no fluxo.
    pub const CAIXA: Rems = E12;
    /// Popover e caixa de mensagem.
    pub const FLUTUANTE: Rems = E16;
    /// Da borda da peça até a coluna-guia da lista.
    pub const GUIA_LISTA: Rems = E16;
    /// Da borda do painel até a coluna-guia da Equipe.
    pub const GUIA_EQUIPE: Rems = E24;
    pub const ENTRE_TURNOS: Rems = E24;
    pub const NO_TURNO: Rems = E12;
    /// Respiro visível, da peça de cima ao topo da linha de 24 do cabeçalho de seção.
    pub const ANTES_DE_SECAO: Rems = E16;
}

/// Padding de quem tem `border_1()`: o conteúdo fica a `px` da borda de fora contando o traço de
/// 1 (o cartão com conteúdo a 12 da borda leva `.p(dentro_da_borda(12.))`).
pub const fn dentro_da_borda(px: f32) -> Rems {
    r(px - 1.)
}

/// Alturas de controles e linhas, em rem.
pub mod altura {
    use super::r;
    use gpui_kit::Rems;
    /// Selo (o kbd tem 18).
    pub const MINI: Rems = r(20.);
    /// Botão só ícone nas ações de hover e dentro de cartões; Chip pequeno.
    pub const ACAO_LINHA: Rems = r(24.);
    /// Botão só ícone em cabeçalho e barra; Chip médio; segmentado pequeno.
    pub const BARRA: Rems = r(28.);
    /// Campo, botão com texto, Chip grande, segmentado médio, linha de uma linha.
    pub const CONTROLE: Rems = r(32.);
    /// Botões do trilho, segmentado grande.
    pub const GRANDE: Rems = r(36.);
    pub const CABECALHO: Rems = r(44.);
    /// Item de menu ou de lista de escolha com descrição.
    pub const ESCOLHA_2: Rems = r(48.);
    /// Cartão de lista com duas linhas.
    pub const LINHA_2: Rems = r(52.);
}

pub const FONTE_MONO: &str = "JetBrains Mono";

/// Só no rótulo "Ultrathink": a referência clareada para passar de 4,5:1 no flutuante.
pub const ARCO_IRIS: [u32; 8] = [
    0xf08a5d, 0xf5b642, 0x9bd46a, 0x4ade80, 0x3fd1b5, 0x5cc8f0, 0x7aa7ff, 0xa98bff,
];

/// Cores dos projetos, na ordem que o ramo `icones` grava (a automática é `soma % 8`). Ficam de
/// fora o destaque e as cores de estado.
pub const CORES_PROJETO: [(u32, &str); 8] = [
    (0x8fb6ff, "Azul"),
    (0x8fd3f5, "Céu"),
    (0x5ad1c4, "Turquesa"),
    (0x8088ff, "Índigo"),
    (0xb39dff, "Lilás"),
    (0xe08ef0, "Orquídea"),
    (0xf49ac1, "Rosa"),
    (0xc8b98f, "Areia"),
];

/// Dígitos de largura igual (`tnum` do OpenType; a Inter embutida tem a tabela): o número que muda
/// ("4 s", "5 s", "24%") não faz a coluna andar. Um `Arc` só, clonado a cada uso.
static TABULAR: std::sync::LazyLock<FontFeatures> =
    std::sync::LazyLock::new(|| FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)])));

/// Tipografia da linguagem, em rem (cresce com o zoom). Duas vozes: a Inter, que conversa, e a
/// JetBrains Mono miúda (`etiqueta`, `rotulo_secao`, as teclas), que põe à vista os fatos e as
/// medidas, como num aparelho de medição.
pub trait Tipo: Styled + Sized {
    /// Dígitos de largura igual (já vem no `metadado`, no `micro` e no `numero`).
    fn tabular(self) -> Self {
        self.font_features(TABULAR.clone())
    }
    /// O título grande da tela vazia (28/34).
    fn titulo_heroi(self) -> Self {
        self.text_size(r(28.)).line_height(r(34.)).font_weight(FontWeight::SEMIBOLD)
    }
    fn titulo_tela(self) -> Self {
        self.text_size(r(18.)).line_height(r(24.)).font_weight(FontWeight::SEMIBOLD)
    }
    fn titulo_cartao(self) -> Self {
        self.text_size(r(15.)).line_height(r(20.)).font_weight(FontWeight::SEMIBOLD)
    }
    /// Rótulo de seção: mono de 11 apagado, como etiqueta técnica. A caixa alta e o tracinho na
    /// frente ficam no desenho (`etiqueta_de_secao`, `CabecalhoDeSecao`), nunca no dado.
    fn rotulo_secao(self) -> Self {
        self.font_family(FONTE_MONO)
            .text_size(r(11.))
            .line_height(r(16.))
            .font_weight(FontWeight::NORMAL)
            .text_color(Cores::apagado())
    }
    /// A segunda voz: mono 11/14 regular, das etiquetas de fio, numerais e medidas.
    fn etiqueta(self) -> Self {
        self.font_family(FONTE_MONO).text_size(r(11.)).line_height(r(14.)).font_weight(FontWeight::NORMAL)
    }
    fn corpo(self) -> Self {
        self.text_size(r(15.)).line_height(r(23.))
    }
    fn corpo_ui(self) -> Self {
        self.text_size(r(14.)).line_height(r(20.))
    }
    fn rotulo(self) -> Self {
        self.text_size(r(13.)).line_height(r(18.)).font_weight(FontWeight::MEDIUM)
    }
    fn metadado(self) -> Self {
        self.text_size(r(12.)).line_height(r(16.)).tabular()
    }
    fn micro(self) -> Self {
        self.text_size(r(11.)).line_height(r(14.)).font_weight(FontWeight::SEMIBOLD).tabular()
    }
    fn mono(self) -> Self {
        self.font_family(FONTE_MONO).text_size(r(13.)).line_height(r(20.))
    }
    fn mono_pequeno(self) -> Self {
        self.font_family(FONTE_MONO).text_size(r(12.)).line_height(r(16.))
    }
    fn numero(self) -> Self {
        self.text_size(r(20.)).line_height(r(24.)).font_weight(FontWeight::SEMIBOLD).tabular()
    }
}

impl<T: Styled> Tipo for T {}

/// Elevação 2 (caixa de mensagem, prévia de arrasto). No GPUI o blur é o desvio padrão: metade
/// do blur do CSS.
/// Preto de sombra com o alfa `base` (0 a 255) multiplicado pelo da superfície.
fn preto_de_sombra(base: u32) -> Hsla {
    let alfa = ((base as f32 * superficie::atual().sombra).round() as u32).min(0xff);
    rgba(alfa).into()
}

pub fn sombra_elevado() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(1.), preto_de_sombra(0x40)).blur_radius(px(1.5)),
        BoxShadow::new(px(0.), px(4.), preto_de_sombra(0x33))
            .blur_radius(px(4.))
            .spread_radius(px(-2.)),
    ]
}

/// Elevação 3: menu, popover, dica, aviso (a mesma do `popover_style` do vendor).
pub fn sombra_flutuante() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(12.), preto_de_sombra(0x66))
            .blur_radius(px(12.))
            .spread_radius(px(-4.)),
        BoxShadow::new(px(0.), px(2.), preto_de_sombra(0x4d))
            .blur_radius(px(3.))
            .spread_radius(px(-1.)),
    ]
}

/// Elevação 4: diálogo (a mesma do `dialog_shadow` do vendor).
pub fn sombra_dialogo() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(24.), preto_de_sombra(0x80))
            .blur_radius(px(24.))
            .spread_radius(px(-8.)),
        BoxShadow::new(px(0.), px(2.), preto_de_sombra(0x4d)).blur_radius(px(4.)),
    ]
}

/// Os três materiais do Vitral, feitos só de luz e sombra de 1 px por dentro (nada vaza para fora
/// da peça, que é translúcida e escureceria o que está atrás).
///
/// Vidro, nas peças (lista, conversa, painéis da direita): um fio claro de 1 px na volta toda,
/// luz na própria quina de cima, sombra de 1 px na base e um brilho que entra uns 40 px pelo topo.
/// Vai na camada do `camada_de_aresta`, o último filho da peça: a sombra interna pinta antes dos
/// filhos, e o que viesse depois (o pontilhado do pé) passaria por cima do fio.
pub fn aresta_de_vidro() -> Vec<BoxShadow> {
    // Cada lado é uma sombra interna deslocada 1 px (o furo anda 1 px e sobra uma tira de 1 px
    // daquele lado, que acompanha o arco dos cantos). Um anel só (`spread_radius(1)`) passaria
    // também pela base, e o claro dele com a sombra por cima dava o tom do fundo: a base sumia.
    let lado = |x: f32, y: f32, cor: Hsla| BoxShadow::new(px(x), px(y), cor).inset();
    vec![
        // O brilho primeiro, para o fio e a luz pintarem por cima dele.
        BoxShadow::new(px(0.), px(28.), Cores::brilho_de_dentro())
            .blur_radius(px(12.))
            .spread_radius(px(-12.))
            .inset(),
        // O fio claro nos dois lados e em cima.
        lado(1., 0., Cores::aro_da_peca()),
        lado(-1., 0., Cores::aro_da_peca()),
        lado(0., 1., Cores::aro_da_peca()),
        // A luz na própria quina de cima, por cima do fio.
        lado(0., 1., Cores::brilho_topo()),
        // A sombra na base.
        lado(0., -1., Cores::sombra_de_baixo()),
    ]
}

/// Tecla, no que está em relevo e escolhido (a pílula do segmentado, a tecla de atalho): luz de
/// 1 px em cima e sombra de 1 px embaixo, por dentro. Nunca em cada linha de uma lista.
pub fn aresta_de_tecla() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(1.), Cores::forte().opacity(0.16)).inset(),
        BoxShadow::new(px(0.), px(-1.), Cores::preto_puro().opacity(0.30)).inset(),
    ]
}

/// Poço, no que é cavado (o trilho do segmentado, o campo de busca, o contador): a sombra cai
/// de cima para dentro e a quina de baixo pega 1 px de luz.
pub fn aresta_de_poco() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(2.), Cores::preto_puro().opacity(0.55)).blur_radius(px(3.)).inset(),
        BoxShadow::new(px(0.), px(-1.), Cores::forte().opacity(0.07)).inset(),
    ]
}

/// Anel de foco pelo teclado, o mesmo do kit (`FOCUS_RING_WIDTH`).
pub fn halo_foco() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(0.), Cores::destaque().opacity(0.5)).spread_radius(px(3.))]
}

/// Linha de luz de 1 px no topo de dentro da peça (o reflexo do vidro), a `forca` de 0 a 1.
pub fn linha_de_luz(forca: f32) -> BoxShadow {
    BoxShadow::new(px(0.), px(1.), Cores::brilho_topo().opacity(forca)).inset()
}

/// O gradiente do topo da peça, na camada de 40 px do `.reflexo()`: um escuro que vai sumindo,
/// no lugar do claro de antes. O topo é onde fica o título; escurecer, em vez de clarear, afunda
/// a peça no fundo e deixa o texto com mais contraste.
pub fn gradiente_reflexo() -> Background {
    let alfa = ((26. * superficie::atual().reflexo).round() as u32).min(0xff);
    linear_gradient(
        180.,
        linear_color_stop(rgba(0x00000000 | alfa), 0.),
        linear_color_stop(rgba(0x00000000), 1.),
    )
}

/// A beirada de cima da doca (o metal no pé da conversa): o escuro do cartão some para baixo.
pub fn gradiente_da_doca() -> Background {
    linear_gradient(180., linear_color_stop(Cores::cartao(), 0.), linear_color_stop(Cores::cartao().opacity(0.), 1.))
}

/// Assinatura do Vitral: azul à esquerda indo para o preto (caixa de mensagem).
pub fn gradiente_quente() -> Background {
    gradiente_quente_em(90.)
}

/// O mesmo gradiente em outro ângulo (135° no círculo do estado vazio amplo).
pub fn gradiente_quente_em(angulo: f32) -> Background {
    linear_gradient(
        angulo,
        linear_color_stop(Cores::caixa_quente(), 0.),
        linear_color_stop(Cores::caixa_preta(), 0.55),
    )
}

/// Variante azul: resumo do coordenador, entrega final, cabeçalho do Mestre.
pub fn gradiente_destaque() -> Background {
    linear_gradient(
        90.,
        // Um tom só, de ponta a ponta: o azul marca o cartão sem virar degradê.
        linear_color_stop(Cores::destaque().opacity(0.08), 0.),
        linear_color_stop(Cores::destaque().opacity(0.08), 1.),
    )
}

/// Catálogo completo do Lucide. Só aparecem os ícones registrados em `IconesExtras` (ou os 101
/// que o kit já embute); um que falte sai em branco, e o teste `icones_lucide_registrados` acusa.
pub use gpui_kit::assets::IconName as Lucide;

gpui_kit::assets::icon_assets!(
    IconesExtras,
    [
        Activity,
        Archive,
        ArchiveRestore,
        AtSign,
        BellOff,
        Binary,
        Brain,
        CalendarClock,
        ChartColumn,
        CheckCheck,
        CircleArrowUp,
        CircleDashed,
        CircleDot,
        CirclePause,
        CircleUserRound,
        Clock,
        CodeXml,
        Columns2,
        Columns3,
        Compass,
        Container,
        CornerLeftUp,
        Download,
        Eraser,
        FileCheck,
        FileCode,
        FileDiff,
        FileImage,
        FileMinus,
        FilePen,
        FilePenLine,
        FilePlus,
        FileSymlink,
        FileX,
        Files,
        Focus,
        FolderInput,
        FolderPlus,
        FolderSearch,
        FolderX,
        Gauge,
        GitBranch,
        GitCommitHorizontal,
        GitFork,
        GitMerge,
        GitPullRequest,
        GitPullRequestClosed,
        GitPullRequestDraft,
        Hand,
        Image,
        ImageOff,
        Keyboard,
        LayoutGrid,
        Link,
        LockKeyholeOpen,
        ListChecks,
        ListTree,
        MessageCircle,
        MessageCircleQuestionMark,
        MessageSquare,
        MessageSquarePlus,
        MessageSquareQuote,
        MessagesSquare,
        Milestone,
        MonitorPlay,
        MoveDown,
        MoveUp,
        NotebookPen,
        Orbit,
        Paperclip,
        Pencil,
        Pin,
        PinOff,
        RotateCcw,
        RotateCcwClock,
        Rows3,
        ScrollText,
        ShieldAlert,
        ShieldCheck,
        ShieldOff,
        ShieldQuestionMark,
        Shuffle,
        Sparkles,
        SquareCheck,
        SquareSlash,
        Tag,
        Terminal,
        TextSearch,
        TextWrap,
        Trash,
        Unplug,
        UsersRound,
        WandSparkles,
        WifiOff,
        Workflow,
        Wrench,
        X,
        Zap,
        ZoomIn,
        ZoomOut,
    ]
);

/// Os ícones do kit mais os extras do Vitral.
pub struct Icones;

impl AssetSource for Icones {
    fn load(&self, caminho: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = IconesExtras.load(caminho)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(caminho)
    }

    fn list(&self, caminho: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut lista = gpui_kit::assets::Assets.list(caminho)?;
        lista.extend(IconesExtras.list(caminho)?);
        lista.sort();
        lista.dedup();
        Ok(lista)
    }
}

const TEMA: &str = r##"{
  "name": "Vitral",
  "themes": [{
    "name": "Vidro azul-marinho",
    "mode": "dark",
    "font.family": "Inter",
    "font.size": 15,
    "mono_font.family": "JetBrains Mono",
    "mono_font.size": 13,
    "radius": 10,
    "radius.lg": 20,
    "shadow": false,
    "colors": {
      "background": "#060810d4",
      "foreground": "#dfe6fb",
      "border": "#ffffff14",
      "window.border": "#00000000",
      "ring": "#3a7fff",
      "caret": "#9ccbff",
      "selection.background": "#3a7fff59",
      "accent.background": "#3a7fff33",
      "accent.foreground": "#ffffff",
      "primary.background": "#3a7fff",
      "primary.hover.background": "#3374f0",
      "primary.active.background": "#2a64d6",
      "primary.foreground": "#ffffff",
      "button.primary.background": "#2f6fe8",
      "button.primary.hover.background": "#2a64d6",
      "button.primary.active.background": "#2458c0",
      "button.primary.foreground": "#ffffff",
      "button.danger.background": "#f871711f",
      "button.danger.hover.background": "#f8717124",
      "button.danger.active.background": "#f871712e",
      "button.danger.foreground": "#f87171",
      "switch.background": "#ffffff1f",
      "switch.thumb.background": "#dfe6fb",
      "secondary.background": "#ffffff0d",
      "secondary.hover.background": "#ffffff17",
      "secondary.active.background": "#ffffff1f",
      "secondary.foreground": "#dfe6fb",
      "muted.background": "#ffffff0f",
      "muted.foreground": "#9ba8cf",
      "popover.background": "#0c0e13",
      "popover.foreground": "#dfe6fb",
      "input.border": "#ffffff1a",
      "list.background": "#00000000",
      "list.hover.background": "#ffffff12",
      "list.active.background": "#3a7fff33",
      "list.active.border": "#3a7fff00",
      "list.even.background": "#00000000",
      "link.foreground": "#8fc0ff",
      "link.hover.foreground": "#b4d4ff",
      "link.active.foreground": "#8fc0ff",
      "scrollbar.background": "#00000000",
      "scrollbar.thumb.background": "#9ba8cf40",
      "scrollbar.thumb.hover.background": "#9ba8cf66",
      "sidebar.background": "#00000000",
      "sidebar.border": "#00000000",
      "sidebar.foreground": "#dfe6fb",
      "sidebar.accent.background": "#3a7fff24",
      "sidebar.accent.foreground": "#eaf2ff",
      "sidebar.primary.background": "#3a7fff",
      "sidebar.primary.foreground": "#ffffff",
      "tab.background": "#00000000",
      "tab.active.background": "#121726d1",
      "tab.foreground": "#9ba8cf",
      "tab.active.foreground": "#eaf2ff",
      "tab_bar.background": "#00000000",
      "title_bar.background": "#00000000",
      "title_bar.border": "#00000000",
      "overlay": "#070b1699",
      "success.background": "#4ade80",
      "success.foreground": "#4ade80",
      "warning.background": "#f5b642",
      "warning.foreground": "#f5b642",
      "danger.background": "#f87171",
      "danger.foreground": "#f87171",
      "info.background": "#8fd3f5",
      "info.foreground": "#8fd3f5",
      "base.blue": "#3a7fff",
      "base.green": "#4ade80",
      "base.yellow": "#f5b642",
      "base.red": "#f87171",
      "base.cyan": "#8fd3f5",
      "base.magenta": "#b39dff"
    },
    "highlight": {
      "editor_background": "#00000000",
      "editor_foreground": "#dfe6fb",
      "editor_line_number": "#9ba8cf66",
      "editor_active_line_number": "#dfe6fb",
      "editor_active_line": "#ffffff08",
      "editor.gutter.background": "#00000000",
      "syntax": {
        "keyword": { "color": "#b39dff" },
        "preproc": { "color": "#b39dff" },
        "label": { "color": "#b39dff" },
        "string": { "color": "#8fe3a8" },
        "string.escape": { "color": "#8fd3f5" },
        "string.regex": { "color": "#8fd3f5" },
        "string.special": { "color": "#8fe3a8" },
        "text.literal": { "color": "#8fe3a8" },
        "text.code.span": { "color": "#8fe3a8" },
        "function": { "color": "#8fc0ff" },
        "type": { "color": "#8fd3f5" },
        "constructor": { "color": "#8fd3f5" },
        "enum": { "color": "#8fd3f5" },
        "variant": { "color": "#8fd3f5" },
        "number": { "color": "#f5b642" },
        "boolean": { "color": "#f5b642" },
        "constant": { "color": "#f5b642" },
        "comment": { "color": "#8a97bf", "font_style": "italic" },
        "comment_doc": { "color": "#8a97bf", "font_style": "italic" },
        "punctuation": { "color": "#9ba8cf" },
        "operator": { "color": "#9ba8cf" },
        "property": { "color": "#dfe6fb" },
        "attribute": { "color": "#8fc0ff" },
        "tag": { "color": "#8fc0ff" },
        "variable.special": { "color": "#8fc0ff" },
        "title": { "color": "#8fc0ff", "font_weight": 600 },
        "emphasis": { "font_style": "italic" },
        "emphasis.strong": { "font_weight": 700 },
        "link_text": { "color": "#8fc0ff" },
        "link_uri": { "color": "#8fc0ff", "font_style": "italic" }
      }
    }
  }]
}"##;

/// Largura dos avisos a 100% (o zoom a multiplica, em `zoom.rs`).
pub const LARGURA_AVISO: f32 = 380.;

/// Inter e JetBrains Mono vão dentro do binário (OFL; a licença está em assets/fontes/OFL.txt e vai
/// no pacote): o Windows não tem nenhuma das duas, e no Linux o Vitral não depende das fontes da
/// distribuição. Só os pesos que o Vitral usa: 400, 500, 600 e 700 e o itálico do texto; no código, o
/// normal, o itálico dos comentários e o negrito.
const FONTES: [&[u8]; 8] = [
    include_bytes!("../assets/fontes/Inter-Regular.ttf"),
    include_bytes!("../assets/fontes/Inter-Medium.ttf"),
    include_bytes!("../assets/fontes/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fontes/Inter-Bold.ttf"),
    include_bytes!("../assets/fontes/Inter-Italic.ttf"),
    include_bytes!("../assets/fontes/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fontes/JetBrainsMono-Italic.ttf"),
    include_bytes!("../assets/fontes/JetBrainsMono-Bold.ttf"),
];

/// As fontes do símbolo e do emoji que faltam nas duas, do sistema. O Noto Color Emoji (COLRv1)
/// sai em branco no Linux, então lá vai o monocromático; no Windows, as da Segoe.
pub const FONTES_DE_RESERVA: [&str; 2] =
    if cfg!(windows) { ["Segoe UI Emoji", "Segoe UI Symbol"] } else { ["Noto Emoji", "Noto Sans Symbols 2"] };

/// O JSON do tema com as superficies, bordas e realces da superfície no lugar dos de
/// hoje (a variante 1 devolve o TEMA sem mudanca).
fn tema_json() -> String {
    let v = superficie::atual();
    let mut json = TEMA.to_string();
    for (chave, cor) in [
        ("background", v.vidro),
        // O `border` do kit também pinta os separadores dos menus: fica com a divisória, e só o
        // contorno dos campos segue a borda transparente.
        ("border", v.linha),
        ("input.border", v.borda),
        ("popover.background", v.flutuante),
        ("overlay", v.veu),
        ("list.hover.background", v.hover),
        ("list.active.background", v.selecao),
        ("accent.background", v.selecao),
        ("sidebar.accent.background", v.selecao),
        ("tab.active.background", v.cartao_elevado),
        // O anel de foco do kit (menus, campos) é neutro: o azul fica para a ação.
        ("ring", 0xffffff40),
        ("primary.foreground", v.sobre_destaque),
        ("button.primary.foreground", v.sobre_destaque),
        ("sidebar.primary.foreground", v.sobre_destaque),
    ] {
        let alvo = format!("\"{chave}\": \"");
        if let Some(i) = json.find(&alvo) {
            let inicio = i + alvo.len();
            let fim = inicio + json[inicio..].find('"').expect("cor do tema fechada");
            json.replace_range(inicio..fim, &format!("#{cor:08x}"));
        }
    }
    // Os textos do kit (o corpo, o tênue e o dos comentários) seguem os da variante: no JSON eles
    // estão com os do marinho, em várias chaves.
    let m = &superficie::MARINHO;
    for (antes, depois) in [(m.texto, v.texto), (m.tenue, v.tenue), (m.apagado, v.apagado)] {
        json = json.replace(&format!("#{:06x}", antes >> 8), &format!("#{:06x}", depois >> 8));
    }
    // O destaque e as variações dele (o foco, a seleção de texto, o botão primário, os links).
    for (antes, depois) in [
        (0x3a7fff, v.destaque >> 8),
        (0x2f6fe8, v.destaque >> 8),
        (0x3374f0, v.destaque_hover >> 8),
        (0x2a64d6, v.destaque_hover >> 8),
        (0x2458c0, v.destaque_ativo >> 8),
        (0x8fc0ff, v.destaque_texto >> 8),
    ] {
        json = json.replace(&format!("#{antes:06x}"), &format!("#{depois:06x}"));
    }
    json
}

pub fn aplicar(cx: &mut App) {
    if let Err(erro) = cx.text_system().add_fonts(FONTES.iter().map(|f| Cow::Borrowed(*f)).collect()) {
        eprintln!("vitral: as fontes embutidas não carregaram ({erro:#}); ficam as do sistema");
    }
    let conjunto: ThemeSet = serde_json::from_str(&tema_json()).expect("tema do Vitral");
    let config = Rc::new(conjunto.themes.into_iter().next().unwrap());
    Theme::update(cx, |tema| {
        tema.apply_config(&config);
        // Avisos no topo, no centro: o canto de cima à direita é o dos botões da janela.
        tema.notification.placement = Anchor::TopCenter;
        tema.notification.margins.top = px(12.);
        tema.notification.width = px(LARGURA_AVISO);
        tema.notification.max_items = 3;
    });
    // As Animações desligadas nas Configurações, o GNOME sem animações ou a bancada com
    // VITRAL_SEM_ANIMACAO=1: tudo direto no estado final, inclusive o spinner, que fica parado.
    crate::ui::movimento::iniciar(cx);
}

/// As cores das superfícies num lugar só: o chumbo das frestas, os degraus de claro de cada peça,
/// as bordas, a luz de cima e os realces. Saiu de um estudo de seis acabamentos que o dono
/// comparou lado a lado; esta é a "Degraus", a escolhida: cada superfície num nível de claro bem
/// diferente, com a moldura quase preta.
pub mod superficie {
    pub struct Variante {
        /// Fundo da janela (o `background` do tema). No Grafite é opaco e fica debaixo da chapa de
        /// metal: só aparece se o metal não puder ser pintado.
        pub vidro: u32,
        /// Fundo do trilho e da lista.
        pub vidro_leitura: u32,
        /// Fundo da conversa e dos painéis da direita.
        pub cartao: u32,
        pub cartao_elevado: u32,
        /// Popover, menu, dica e aviso.
        pub flutuante: u32,
        pub dialogo: u32,
        pub gaveta: u32,
        /// Código, saída de comando e diff.
        pub preto: u32,
        /// Campo de busca, trilho do segmentado, kbd.
        pub recuo: u32,
        /// Véu atrás dos diálogos.
        pub veu: u32,
        /// O escuro por cima do metal numa placa de cabeçalho (56%): com ele, `forte` e `texto`
        /// passam de 4,5:1 mesmo com a bolha mais clara atrás (o mínimo é 0x84).
        pub veu_de_placa: u32,
        /// O escuro por cima do metal num botão de ação ou numa gota (36%): o ícone branco passa
        /// de 3:1 (o mínimo é 0x4c).
        pub veu_de_botao: u32,
        /// Contorno de controle (campo, chip, selo).
        pub borda: u32,
        /// Contorno de popover, diálogo e lista de escolha.
        pub borda_forte: u32,
        /// Divisória por dentro de uma superfície.
        pub linha: u32,
        /// O que é traço e não contorno: trilha de barra, linha de árvore, separador e o fundo do
        /// selo neutro. Fica visível mesmo com `borda` transparente.
        pub traco: u32,
        /// Aro de 1 px por dentro das peças (alfa 0 = peça sem aro, como hoje).
        pub aro: u32,
        /// Linha de luz no topo de dentro das peças.
        pub brilho_topo: u32,
        /// Sombra de 1 px na base de dentro das peças (a `aresta_de_vidro`).
        pub sombra_de_baixo: u32,
        /// O brilho que entra uns 40 px pelo topo das peças. Travado em 3%: com 5% o apagado
        /// sobre a lista reprova no contraste.
        pub brilho_de_dentro: u32,
        /// Fio de pontos: separador e trilho, no lugar da linha cheia.
        pub fio: u32,
        /// A trama fina de fundo (placa de cabeçalho, cabeçalho de código).
        pub pontilhado_fino: u32,
        /// Quanto do gradiente de reflexo entra no topo das peças.
        pub reflexo: f32,
        /// Multiplicador das sombras dos elevados, flutuantes e diálogos.
        pub sombra: f32,
        pub hover: u32,
        pub selecao: u32,
        /// Começo e fim do gradiente da caixa de mensagem.
        pub caixa_quente: u32,
        pub caixa_preta: u32,
        /// Margem entre a janela e as peças, vão entre elas e o raio dos cantos: o acabamento
        /// "denso", que o dono escolheu olhando as seis opções lado a lado.
        pub margem: f32,
        pub vao: f32,
        pub raio_peca: f32,
        /// Corpo (o `foreground` do tema), o tênue, o dos metadados e o decorativo.
        pub texto: u32,
        pub tenue: u32,
        pub apagado: u32,
        pub fantasma: u32,
        /// Código em linha no texto.
        pub codigo: u32,
        /// A base (opaca) onde a cor do projeto é misturada, no quadrado dele.
        pub base_da_tinta: u32,
        /// O destaque (o único da tela): o cheio, o do ponteiro em cima, o apertado, o de texto
        /// pequeno (que precisa de 4,5:1 nas peças) e o que vai escrito por cima do cheio.
        pub destaque: u32,
        pub destaque_hover: u32,
        pub destaque_ativo: u32,
        pub destaque_texto: u32,
        pub sobre_destaque: u32,
    }

    /// O acabamento de antes de 12/11/2026: "Degraus" no azul-marinho da Stained Glass UI, com o
    /// reflexo no topo das peças, o gradiente na caixa de mensagem e a moldura quase preta.
    pub const MARINHO: Variante = Variante {
        vidro: 0x02030aee,
        vidro_leitura: 0x0e1322ff,
        cartao: 0x171e30ff,
        cartao_elevado: 0x1e2740ff,
        flutuante: 0x1b2339ff,
        dialogo: 0x1e2740ff,
        gaveta: 0x1e2740ff,
        preto: 0x05060aff,
        recuo: 0x00000066,
        veu: 0x020309cc,
        veu_de_placa: 0x0000008f,
        veu_de_botao: 0x0000005c,
        borda: 0x00000000,
        borda_forte: 0xffffff14,
        linha: 0xffffff0f,
        traco: 0xffffff14,
        aro: 0x00000000,
        brilho_topo: 0xffffff0f,
        // O marinho não tem a aresta: sem sombra embaixo nem brilho por dentro.
        sombra_de_baixo: 0x00000000,
        brilho_de_dentro: 0x00000000,
        fio: 0xffffff14,
        pontilhado_fino: 0xffffff0a,
        reflexo: 1.,
        sombra: 1.3,
        hover: 0xffffff14,
        selecao: 0x3a7fff3d,
        caixa_quente: 0x24396bf7,
        caixa_preta: 0x10131cf7,
        margem: 4.,
        vao: 4.,
        raio_peca: 14.,
        texto: 0xdfe6fbff,
        tenue: 0x9ba8cfff,
        apagado: 0x8a97bfff,
        fantasma: 0x9ba8cf99,
        codigo: 0x9ccbffff,
        base_da_tinta: 0x121726ff,
        destaque: 0x3a7fffff,
        destaque_hover: 0x3374f0ff,
        destaque_ativo: 0x2a64d6ff,
        destaque_texto: 0x6b9bffff,
        sobre_destaque: 0xffffffff,
    };

    /// "Grafite" (12/11/2026, o dono quis "mais moderno e com menos cara de IA" e apontou o
    /// recent.design e o dithercn.dev como referência): os mesmos degraus de claro, num quase preto
    /// frio e sem cor, com um destaque só, lavanda. Chapado: sem a linha de luz nem o reflexo no topo das
    /// peças, sem gradiente na caixa e sem halo; o que separa as peças é o degrau de tom e o que
    /// marca o foco é a borda. A moldura tem a cor da lista (antes era um chumbo quase preto e a
    /// janela parecia ter uma borda preta em volta do menu). A seleção é neutra: o azul fica para
    /// o que é ação (o "+", o enviar, os links, o foco), um destaque só por tela. Depois ele pediu
    /// "partes transparentes e bordas estilizadas": o vidro voltou (a moldura e a lista translúcidas,
    /// a caixa de mensagem também), com um fio de contorno e a linha de luz no topo. No plano
    /// visual de 12/11 ("muito básico"), as peças ganharam a aresta de vidro inteira (fio nos
    /// lados, luz na quina, sombra na base, brilho de 3%), o painel ativo trocou a borda azul pelas
    /// cantoneiras e as linhas cheias viraram fio de pontos. Ele ainda achou "muito básico", colou
    /// a receita do metal líquido e pediu "tenta focar em vez de glass esse metal líquido": atrás
    /// de tudo passou a haver uma chapa de metal opaca (`componentes/metal.rs`), e o papel de
    /// parede saiu de cena. O app é a casca escura por cima dela.
    pub const GRAFITE: Variante = Variante {
        // O `vidro` é opaco: é a reserva debaixo do metal e o que a raiz do kit pinta. São duas
        // camadas: a BASE é a janela inteira, com a lista direto nela (sem cartão próprio), e é o
        // metal debaixo do `vidro_leitura`, um véu azul-escuro que deixa passar 11% dele; por cima
        // flutuam os cartões (a conversa, os painéis da direita), grafite e quase opacos (5%). O
        // que flutua sobre texto (menus, popovers) é praticamente opaco. Os alfas saem da conta
        // de contraste contra a cor mais clara do metal (`TETO_DO_METAL`).
        vidro: 0x0d0f13ff,
        vidro_leitura: 0x0a1533e3,
        cartao: 0x14161df2,
        cartao_elevado: 0x1c1f29ff,
        flutuante: 0x171a23f7,
        dialogo: 0x1c1f29fa,
        gaveta: 0x1c1f29fa,
        preto: 0x08090cff,
        recuo: 0x00000066,
        veu: 0x05060899,
        veu_de_placa: 0x0000008f,
        veu_de_botao: 0x0000005c,
        // As bordas são um fio claro: nos controles, nas peças (o aro) e, mais forte, no que flutua.
        // No topo das peças e da caixa, uma linha de luz, como a quina de um vidro.
        borda: 0xffffff12,
        borda_forte: 0xffffff1f,
        linha: 0xffffff0f,
        traco: 0xffffff14,
        // A aresta de vidro das peças: o fio em volta, a luz na quina de cima, a sombra na base e
        // o brilho de 3% que entra pelo topo (ver `aresta_de_vidro`).
        aro: 0xffffff14,
        brilho_topo: 0xffffff24,
        sombra_de_baixo: 0x00000073,
        brilho_de_dentro: 0xffffff08,
        fio: 0xffffff24,
        pontilhado_fino: 0xffffff0a,
        reflexo: 0.,
        sombra: 1.2,
        hover: 0xffffff14,
        selecao: 0xffffff1c,
        // O campo da caixa de mensagem é opaco: é o miolo da moldura dupla (e do aro de metal).
        caixa_quente: 0x1c1f29ff,
        caixa_preta: 0x1c1f29ff,
        // A base aparece em volta dos cartões: 8 px dão a ela largura de camada, não de fresta.
        margem: 8.,
        vao: 8.,
        raio_peca: 16.,
        texto: 0xe2e5ecff,
        tenue: 0x9da3b3ff,
        apagado: 0x8e94a5ff,
        fantasma: 0x9da3b399,
        codigo: 0xc8cfe0ff,
        base_da_tinta: 0x14161dff,
        // Um azul cheio (o dono pediu "um azul mais azul"), com branco por cima.
        destaque: 0x2b63ffff,
        destaque_hover: 0x4a7bffff,
        destaque_ativo: 0x1f52e6ff,
        destaque_texto: 0x7c9dffff,
        sobre_destaque: 0xffffffff,
    };

    /// O acabamento em uso.
    pub const ATUAL: Variante = GRAFITE;

    pub fn atual() -> &'static Variante {
        &ATUAL
    }
}

/// O raio dos cantos de cada peça (trilho, lista, conversa, painéis).
pub fn raio_da_peca() -> gpui_kit::Pixels {
    gpui_kit::px(superficie::ATUAL.raio_peca)
}

/// A margem entre a janela e as peças.
pub fn margem_da_janela() -> gpui_kit::Pixels {
    gpui_kit::px(superficie::ATUAL.margem)
}

/// O vão entre as peças.
pub fn vao_entre_pecas() -> gpui_kit::Pixels {
    gpui_kit::px(superficie::ATUAL.vao)
}

/// Cores próprias do Vitral que não existem no tema dos componentes.
pub struct Cores;

impl Cores {
    /// Fundo da janela (o `background` do tema), que a raiz do kit pinta. Fica debaixo da chapa
    /// de metal (`componentes::metal::fundo`), que é quem aparece nas frestas e na margem.
    pub fn vidro() -> Hsla {
        rgba(superficie::atual().vidro).into()
    }
    /// Fundo das colunas com texto pequeno (trilho e lista): fumê sobre a chapa de metal, que o
    /// tinge de azul. Garante o contraste no pior caso (a bolha mais clara atrás).
    pub fn vidro_leitura() -> Hsla {
        rgba(superficie::atual().vidro_leitura).into()
    }
    /// Tudo o que flutua sobre texto (popover, menu, dica, aviso): quase preto e opaco, por cima
    /// do vidro azul (o `popover.background` do tema).
    pub fn flutuante() -> Hsla {
        rgba(superficie::atual().flutuante).into()
    }
    /// A gaveta (a lista ou a Equipe por cima da conversa, em janela estreita): flutua, então é
    /// opaca, mas continua no azul das peças.
    pub fn gaveta() -> Hsla {
        rgba(superficie::atual().gaveta).into()
    }
    /// O preto de dentro: código, saída de comando, diff e o disco dos botões redondos do trilho.
    pub fn preto() -> Hsla {
        rgba(superficie::atual().preto).into()
    }
    /// Trilho do segmentado, campo de busca, kbd, selos no vidro.
    pub fn recuo() -> Hsla {
        rgba(superficie::atual().recuo).into()
    }
    /// O recuo dentro de um flutuante: no quase preto o escuro some, então ali ele é um claro leve.
    pub fn recuo_no_flutuante() -> Hsla {
        rgba(0xffffff0d).into()
    }
    /// Fundo do GrupoDeOpcoes.
    pub fn grupo() -> Hsla {
        rgba(0xffffff0a).into()
    }
    /// Véu atrás dos diálogos (o `overlay` do tema).
    pub fn veu() -> Hsla {
        rgba(superficie::atual().veu).into()
    }
    /// O escuro por cima do metal numa placa de cabeçalho: em cima dele só `forte` e `texto`.
    pub fn veu_de_placa() -> Hsla {
        rgba(superficie::atual().veu_de_placa).into()
    }
    /// O escuro por cima do metal num botão de ação ou numa gota: em cima dele só ícone branco.
    pub fn veu_de_botao() -> Hsla {
        rgba(superficie::atual().veu_de_botao).into()
    }
    pub fn pressionado() -> Hsla {
        rgba(0xffffff1f).into()
    }
    /// Escolhido num segmentado de outra cor (ciano no esforço).
    pub fn selecao_de(cor: Hsla) -> Hsla {
        cor.opacity(0.20)
    }
    /// Fundo de pílula de estado e de alerta.
    pub fn tinta(cor: Hsla) -> Hsla {
        cor.opacity(0.12)
    }
    pub fn tinta_borda(cor: Hsla) -> Hsla {
        cor.opacity(0.30)
    }
    /// A cor misturada ao marinho, opaca: fundo do quadrado do projeto.
    pub fn tinta_solida(cor: Hsla, alfa: f32) -> Hsla {
        Hsla::from(rgba(superficie::atual().base_da_tinta)).blend(cor.opacity(alfa))
    }
    /// Hover e realce de item perigoso.
    pub fn perigo_hover() -> Hsla {
        Self::vermelho().opacity(0.14)
    }
    /// Campos, chip aberto, selos de contorno, separador em superfície opaca.
    pub fn borda() -> Hsla {
        rgba(superficie::atual().borda).into()
    }
    /// Contorno de popover, diálogo e lista de escolha; trilho dos anéis.
    pub fn borda_forte() -> Hsla {
        rgba(superficie::atual().borda_forte).into()
    }
    /// Títulos, item selecionado ou não lido.
    pub fn forte() -> Hsla {
        rgba(0xffffffff).into()
    }
    /// Corpo (o `foreground` do tema).
    pub fn texto() -> Hsla {
        rgba(superficie::atual().texto).into()
    }
    /// Texto sobre selo sólido de cor.
    pub fn sobre_cor() -> Hsla {
        rgba(0x0f1422ff).into()
    }
    /// Só o esforço. Era um ciano (#67E7F9); no acabamento chapado as barrinhas e o rótulo do
    /// esforço são neutros, como os outros chips da caixa: o destaque da tela é um só.
    pub fn ciano() -> Hsla {
        Self::texto()
    }
    /// Automação e IA.
    pub fn violeta() -> Hsla {
        rgba(0xb39dffff).into()
    }
    /// Cores de marca: só no logo e no anel de uso da conta.
    pub fn claude() -> Hsla {
        rgba(0xd97757ff).into()
    }
    pub fn codex() -> Hsla {
        rgba(0x5b6cf9ff).into()
    }
    pub fn linear() -> Hsla {
        rgba(0x5e6ad2ff).into()
    }
    /// Começo do gradiente da caixa de mensagem: o azul do vidro (o dono trocou o laranja pelo azul).
    pub fn caixa_quente() -> Hsla {
        rgba(superficie::atual().caixa_quente).into()
    }
    /// Fim do gradiente da caixa: o quase preto dos flutuantes, com o alfa do começo.
    pub fn caixa_preta() -> Hsla {
        rgba(superficie::atual().caixa_preta).into()
    }
    /// Reflexo azul na borda direita da caixa (camada por cima do gradiente).
    pub fn caixa_brilho() -> Background {
        linear_gradient(
            90.,
            linear_color_stop(rgba(0x3a7fff00), 0.78),
            linear_color_stop(rgba(0x3a7fff00), 1.),
        )
    }
    pub fn brilho_topo() -> Hsla {
        rgba(superficie::atual().brilho_topo).into()
    }
    pub fn sombra_caixa() -> Hsla {
        preto_de_sombra(0x59)
    }
    /// Fundo do BotaoRedondo claro.
    pub fn claro() -> Hsla {
        rgba(0xeef2ffff).into()
    }
    /// Barrinhas de sinal apagadas.
    pub fn barra_apagada() -> Hsla {
        rgba(0xffffff2e).into()
    }
    /// Cartões (barra lateral, conversa, painel): navy mais claro e mais opaco que o fundo.
    pub fn cartao() -> Hsla {
        rgba(superficie::atual().cartao).into()
    }
    pub fn cartao_elevado() -> Hsla {
        rgba(superficie::atual().cartao_elevado).into()
    }
    /// Diálogos: opacos, senão o texto da conversa aparece através deles.
    pub fn dialogo() -> Hsla {
        rgba(superficie::atual().dialogo).into()
    }
    pub fn destaque() -> Hsla {
        rgba(superficie::atual().destaque).into()
    }
    pub fn destaque_hover() -> Hsla {
        rgba(superficie::atual().destaque_hover).into()
    }
    pub fn destaque_ativo() -> Hsla {
        rgba(superficie::atual().destaque_ativo).into()
    }
    /// O destaque para texto pequeno: o cheio pode ficar abaixo de 4,5:1 nas peças.
    pub fn destaque_texto() -> Hsla {
        rgba(superficie::atual().destaque_texto).into()
    }
    /// Texto e ícone por cima do destaque cheio (o "+", o botão primário).
    pub fn sobre_destaque() -> Hsla {
        rgba(superficie::atual().sobre_destaque).into()
    }
    /// Item escolhido numa lista (par com `hover`): azul, mais forte que o hover.
    pub fn selecao() -> Hsla {
        rgba(superficie::atual().selecao).into()
    }
    /// Mouse por cima de um item de lista: neutro, para não parecer mais escolhido que a seleção.
    pub fn hover() -> Hsla {
        rgba(superficie::atual().hover).into()
    }
    /// Contorno do painel ativo e da caixa com foco: forte o bastante para ver no vidro.
    pub fn destaque_borda() -> Hsla {
        Self::destaque().opacity(0.6)
    }
    pub fn codigo() -> Hsla {
        rgba(superficie::atual().codigo).into()
    }
    pub fn tenue() -> Hsla {
        rgba(superficie::atual().tenue).into()
    }
    /// Metadados (horário, estados vazios, avisos): opaco para dar 4,5:1 até sobre a neblina.
    pub fn apagado() -> Hsla {
        rgba(superficie::atual().apagado).into()
    }
    /// Só para o que é decorativo (separadores, ícone da busca): pode sumir no fundo.
    pub fn fantasma() -> Hsla {
        rgba(superficie::atual().fantasma).into()
    }
    /// Aro de 1 px por dentro das peças (o fio da `aresta_de_vidro`): transparente na variante
    /// que não tem aro.
    pub fn aro_da_peca() -> Hsla {
        rgba(superficie::atual().aro).into()
    }
    /// A sombra de 1 px na base de dentro das peças.
    pub fn sombra_de_baixo() -> Hsla {
        rgba(superficie::atual().sombra_de_baixo).into()
    }
    /// O brilho de 3% que entra pelo topo das peças.
    pub fn brilho_de_dentro() -> Hsla {
        rgba(superficie::atual().brilho_de_dentro).into()
    }
    /// O fio de pontos (separador e trilho) e o tracinho dos rótulos de seção.
    pub fn fio() -> Hsla {
        rgba(superficie::atual().fio).into()
    }
    /// A trama fina de fundo (placa de cabeçalho, cabeçalho de código).
    pub fn pontilhado_fino() -> Hsla {
        rgba(superficie::atual().pontilhado_fino).into()
    }
    /// Preto puro, só como base das sombras de 1 px (a opacidade vem de quem usa).
    pub fn preto_puro() -> Hsla {
        rgba(0x000000ff).into()
    }
    pub fn linha() -> Hsla {
        rgba(superficie::atual().linha).into()
    }
    /// Trilha de barra, linha de árvore, separador e fundo de selo neutro: o traço que continua à
    /// vista com as peças e os controles sem contorno.
    pub fn traco() -> Hsla {
        rgba(superficie::atual().traco).into()
    }
    pub fn verde() -> Hsla {
        rgba(0x4ade80ff).into()
    }
    pub fn ambar() -> Hsla {
        rgba(0xf5b642ff).into()
    }
    pub fn vermelho() -> Hsla {
        rgba(0xf87171ff).into()
    }
}

#[cfg(test)]
pub(crate) mod testes {
    use super::{CORES_PROJETO, Cores, Icones, TEMA, raio};
    use gpui_kit::{Hsla, Rgba, rgb};
    use gpui_kit::component::{
        ThemeMode, ThemeSet,
        highlighter::{HighlightTheme, highlight_code},
    };
    use gpui_kit::{AssetSource as _, assets::IconName};

    #[test]
    fn tema_traz_as_cores_de_sintaxe() {
        let conjunto: ThemeSet = serde_json::from_str(TEMA).expect("tema do Vitral");
        let realce = conjunto.themes[0].highlight.as_ref().expect("seção highlight");
        assert!(realce.editor_background.is_some());
        assert!(realce.syntax.keyword.is_some());
        assert!(realce.syntax.comment_doc.is_some());
        assert!(realce.syntax.title.and_then(|t| t.font_weight).is_some());
    }

    #[test]
    fn codigo_sai_com_as_cores_do_tema() {
        let conjunto: ThemeSet = serde_json::from_str(TEMA).expect("tema do Vitral");
        let estilo = conjunto.themes[0].highlight.clone().expect("seção highlight");
        let palavra_chave = estilo.syntax.keyword.and_then(|k| k.color);
        let tema = HighlightTheme {
            name: "Vitral".into(),
            appearance: ThemeMode::Dark,
            style: estilo,
        };
        let cores = highlight_code("rs", "fn main() { let x = 1; }", &tema);
        assert!(cores.iter().any(|(faixa, cor)| faixa.start == 0 && cor.color == palavra_chave));
    }

    /// Todo ícone do Lucide usado no código precisa carregar, senão sai em branco.
    #[test]
    fn icones_lucide_registrados() {
        let mut pastas = vec![std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))];
        let mut usados = Vec::new();
        while let Some(pasta) = pastas.pop() {
            for entrada in std::fs::read_dir(pasta).unwrap().flatten() {
                let caminho = entrada.path();
                if caminho.is_dir() {
                    pastas.push(caminho);
                } else if caminho.extension().is_some_and(|e| e == "rs") {
                    let texto = std::fs::read_to_string(&caminho).unwrap();
                    for (i, _) in texto.match_indices("Lucide::") {
                        let nome: String = texto[i + 8..].chars().take_while(|c| c.is_alphanumeric()).collect();
                        if !nome.is_empty() {
                            usados.push(nome);
                        }
                    }
                }
            }
        }
        usados.sort();
        usados.dedup();
        assert!(!usados.is_empty());
        for nome in usados {
            let icone = IconName::ALL
                .iter()
                .find(|i| format!("{i:?}") == nome)
                .unwrap_or_else(|| panic!("Lucide::{nome} não existe"));
            let carregado = Icones.load(&icone.path()).ok().flatten();
            assert!(carregado.is_some(), "Lucide::{nome} não está em IconesExtras");
        }
    }

    /// Cor opaca de `cor` pintada sobre `fundo` (opaco).
    fn sobre(cor: Hsla, fundo: Rgba) -> Rgba {
        let c = Rgba::from(cor);
        let mistura = |a: f32, b: f32| a * c.a + b * (1. - c.a);
        Rgba { r: mistura(c.r, fundo.r), g: mistura(c.g, fundo.g), b: mistura(c.b, fundo.b), a: 1. }
    }

    fn luminancia(c: Rgba) -> f32 {
        let canal = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        0.2126 * canal(c.r) + 0.7152 * canal(c.g) + 0.0722 * canal(c.b)
    }

    fn contraste(a: Rgba, b: Rgba) -> f32 {
        let (la, lb) = (luminancia(a), luminancia(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    /// Uma cor do tema ("#rrggbb" ou "#rrggbbaa").
    fn cor_do_tema(texto: &str) -> Rgba {
        let hex = texto.trim_start_matches('#');
        let valor = u32::from_str_radix(hex, 16).unwrap();
        gpui_kit::rgba(if hex.len() == 6 { valor << 8 | 0xff } else { valor })
    }

    fn mesma_cor(a: Hsla, b: Rgba) -> bool {
        let a = Rgba::from(a);
        [(a.r, b.r), (a.g, b.g), (a.b, b.b), (a.a, b.a)].iter().all(|(x, y)| (x - y).abs() < 0.002)
    }

    #[test]
    fn vidro_e_flutuante_iguais_aos_do_tema() {
        let json: serde_json::Value = serde_json::from_str(&super::tema_json()).unwrap();
        let cores = &json["themes"][0]["colors"];
        let fundo = cor_do_tema(cores["background"].as_str().unwrap());
        assert!(mesma_cor(Cores::vidro(), fundo), "Cores::vidro() diferente do background do tema");
        // Os flutuantes do kit (menus, popovers, dicas, avisos) e os desenhados à mão.
        let popover = cor_do_tema(cores["popover.background"].as_str().unwrap());
        assert!(mesma_cor(Cores::flutuante(), popover), "Cores::flutuante() diferente do popover.background");
    }

    /// Texto e cores semânticas passam de 4,5:1 nas superfícies de leitura, com a cor mais clara
    /// que a chapa de metal entrega atrás delas (o pior caso: a janela é opaca, o que fica atrás
    /// do texto é o metal, não o papel de parede).
    #[test]
    fn contraste_dos_tokens_no_pior_caso() {
        let atras = Rgba::from(rgb(crate::ui::componentes::metal::TETO_DO_METAL));
        let painel = sobre(Cores::cartao(), atras);
        let flutuante = sobre(Cores::flutuante(), atras);
        // O `.reflexo()` escurece o topo das peças, onde fica o título (#0000001a): o texto
        // precisa continuar legível sobre ele. A linha de luz é de 1 px, sem texto em cima.
        let reflexo = |fundo: Rgba| sobre(gpui_kit::rgba(0x0000001a).into(), fundo);
        // A `aresta_de_vidro` clareia o topo das peças com o brilho de dentro: os títulos ficam
        // em cima dele. Travado em 3% (com 5% o apagado sobre a lista reprova).
        let brilho = |fundo: Rgba| sobre(Cores::brilho_de_dentro(), fundo);
        let leitura = sobre(Cores::vidro_leitura(), atras);
        let superficies = [
            // A base (a lista mora nela) não tem a aresta de vidro: sem brilho por cima.
            ("vidro de leitura", leitura),
            ("painel", painel),
            ("painel com brilho", brilho(painel)),
            ("flutuante", flutuante),
            ("diálogo", sobre(Cores::dialogo(), atras)),
            ("gaveta", sobre(Cores::gaveta(), atras)),
            ("painel com reflexo", reflexo(painel)),
            ("flutuante com reflexo", reflexo(flutuante)),
            ("cartão elevado com reflexo", reflexo(sobre(Cores::cartao_elevado(), painel))),
            // As partes pretas: código e diff no painel, o fim da caixa de mensagem.
            ("preto", sobre(Cores::preto(), painel)),
            ("fim preto da caixa", sobre(Cores::caixa_preta(), painel)),
            ("recuo no flutuante", sobre(Cores::recuo_no_flutuante(), flutuante)),
        ];
        let textos = [
            ("forte", Cores::forte()),
            ("texto", Cores::texto()),
            ("tenue", Cores::tenue()),
            ("apagado", Cores::apagado()),
            ("destaque_texto", Cores::destaque_texto()),
            ("ciano", Cores::ciano()),
            ("verde", Cores::verde()),
            ("ambar", Cores::ambar()),
            ("vermelho", Cores::vermelho()),
            ("violeta", Cores::violeta()),
            ("codigo", Cores::codigo()),
        ];
        for (nome_fundo, fundo) in superficies {
            for (nome, cor) in textos {
                let c = contraste(sobre(cor, fundo), fundo);
                assert!(c >= 4.5, "{nome} sobre {nome_fundo}: {c:.2}:1");
            }
        }
        // Sobre o metal com véu: na placa de cabeçalho só vão `forte` e `texto` (4,5:1); no botão
        // de ação e na gota, só o ícone branco (3:1). No metal cru, nada.
        let placa = sobre(Cores::veu_de_placa(), atras);
        for (nome, cor) in [("forte", Cores::forte()), ("texto", Cores::texto())] {
            let c = contraste(sobre(cor, placa), placa);
            assert!(c >= 4.5, "{nome} sobre placa de metal: {c:.2}:1");
        }
        let botao = sobre(Cores::veu_de_botao(), atras);
        let c = contraste(sobre(Cores::sobre_destaque(), botao), botao);
        assert!(c >= 3., "ícone sobre botão de metal: {c:.2}:1");
    }

    /// As cores de sintaxe do tema passam de 4,5:1 no preto dos blocos de código e do diff (as
    /// linhas trocadas ganham só 12% de verde ou vermelho por baixo).
    #[test]
    fn sintaxe_legivel_no_preto() {
        let json: serde_json::Value = serde_json::from_str(TEMA).unwrap();
        let realce = &json["themes"][0]["highlight"];
        let preto = Rgba::from(Cores::preto());
        let fundos = [
            ("preto", preto),
            ("linha que entrou", sobre(Cores::verde().opacity(0.12), preto)),
            ("linha que saiu", sobre(Cores::vermelho().opacity(0.12), preto)),
        ];
        let mut cores: Vec<(String, Rgba)> = realce["syntax"]
            .as_object()
            .unwrap()
            .iter()
            .filter_map(|(nome, estilo)| Some((nome.clone(), cor_do_tema(estilo["color"].as_str()?))))
            .collect();
        cores.push(("editor_foreground".into(), cor_do_tema(realce["editor_foreground"].as_str().unwrap())));
        assert!(cores.len() > 20);
        for (nome_fundo, fundo) in fundos {
            for (nome, cor) in &cores {
                let c = contraste(sobre((*cor).into(), fundo), fundo);
                assert!(c >= 4.5, "{nome} sobre {nome_fundo}: {c:.2}:1");
            }
        }
    }

    /// Hover, seleção e pressionado continuam à vista no quase preto dos flutuantes e no preto do
    /// código, e a seleção segue mais forte que o hover.
    #[test]
    fn realces_visiveis_no_preto() {
        let atras = Rgba::from(rgb(crate::ui::componentes::metal::TETO_DO_METAL));
        let fundos = [("flutuante", sobre(Cores::flutuante(), atras)), ("preto", Rgba::from(Cores::preto()))];
        for (nome_fundo, fundo) in fundos {
            let realce = |cor: Hsla| contraste(sobre(cor, fundo), fundo);
            let (hover, selecao) = (realce(Cores::hover()), realce(Cores::selecao()));
            assert!(hover >= 1.12, "hover sobre {nome_fundo}: {hover:.3}:1");
            assert!(selecao > hover + 0.05, "seleção sobre {nome_fundo}: {selecao:.3}:1 (hover {hover:.3})");
            assert!(realce(Cores::pressionado()) > selecao, "pressionado sobre {nome_fundo}");
            // O acabamento escolhido não contorna as peças (`borda()` é transparente de
            // propósito): quem precisa aparecer é o contorno forte, o de popover e diálogo.
            assert!(realce(Cores::borda_forte()) >= 1.12, "borda forte sobre {nome_fundo}");
            // Mas trilha, linha de árvore, separador e selo neutro não são contorno: continuam
            // à vista (eles usam o `traco()`, e os separadores do kit, a `linha()`).
            assert!(realce(Cores::traco()) >= 1.12, "traço sobre {nome_fundo}");
            assert!(realce(Cores::linha()) >= 1.08, "linha sobre {nome_fundo}");
        }
        // O recuo é um escuro por cima: no acabamento de hoje ele aparece também no flutuante
        // (antes sumia ali, e por isso existe o `recuo_no_flutuante`, que segue valendo como o
        // claro de quem precisa de recuo sobre um fundo já escuro).
        let flutuante = fundos[0].1;
        assert!(contraste(sobre(Cores::recuo(), flutuante), flutuante) >= 1.05);
        assert!(contraste(sobre(Cores::recuo_no_flutuante(), flutuante), flutuante) >= 1.08);
    }

    pub(crate) fn arquivos_rs(pasta: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut pastas = vec![pasta.to_path_buf()];
        let mut arquivos = Vec::new();
        while let Some(pasta) = pastas.pop() {
            for entrada in std::fs::read_dir(pasta).unwrap().flatten() {
                let caminho = entrada.path();
                if caminho.is_dir() {
                    pastas.push(caminho);
                } else if caminho.extension().is_some_and(|e| e == "rs") {
                    arquivos.push(caminho);
                }
            }
        }
        arquivos.sort();
        arquivos
    }

    /// Métodos de espaço que o teste confere.
    const ESPACOS: [&str; 17] =
        ["gap_x", "gap_y", "gap", "px", "py", "pt", "pb", "pl", "pr", "mx", "my", "mt", "mb", "ml", "mr", "p", "m"];

    /// O que está fora da grade num texto: `.gap(r(10.))`, `.px(r(-3.))` e os atalhos do Tailwind
    /// (`.gap_2()`, `.py_0p5()`; o `_0()` e o `_auto()` passam). Valor dentro de `if` escapa.
    fn fora_da_grade(texto: &str) -> Vec<String> {
        const ESCALA: [f32; 9] = [0., 2., 4., 6., 8., 12., 16., 24., 32.];
        let mut achados = Vec::new();
        for (i, _) in texto.match_indices('.') {
            let resto = &texto[i + 1..];
            let nome: String = resto.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            let depois = &resto[nome.len()..];
            if ESPACOS.contains(&nome.as_str()) {
                let Some(dentro) = depois.strip_prefix("(r(") else { continue };
                let numero: String = dentro.chars().take_while(|c| *c != ')').collect();
                let Ok(valor) = numero.trim().trim_end_matches('.').parse::<f32>() else { continue };
                if !ESCALA.contains(&valor.abs()) {
                    achados.push(format!(".{nome}(r({numero}))"));
                }
            } else if depois.starts_with("()")
                && ESPACOS.iter().any(|base| {
                    nome.strip_prefix(*base)
                        .and_then(|s| s.strip_prefix('_'))
                        .is_some_and(|s| s.starts_with(|c: char| c.is_ascii_digit()) && s != "0")
                })
            {
                achados.push(format!(".{nome}()"));
            }
        }
        achados
    }

    #[test]
    fn espacos_na_escala() {
        assert_eq!(fora_da_grade(".gap(r(10.)).px(r(8.)).mt(r(-4.)).p(r(3.))"), [".gap(r(10.))", ".p(r(3.))"]);
        assert_eq!(fora_da_grade(".gap_2().px_0().mx_auto().py_0p5()"), [".gap_2()", ".py_0p5()"]);
        assert!(fora_da_grade(".size(r(10.)).top(r(3.)).px(r(altura))").is_empty());
        let raiz = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui"));
        let mut erros = Vec::new();
        for caminho in arquivos_rs(raiz) {
            let nome = caminho.strip_prefix(raiz).unwrap().to_string_lossy().to_string();
            let texto = std::fs::read_to_string(&caminho).unwrap();
            for achado in fora_da_grade(&texto) {
                erros.push(format!("{nome}: {achado}"));
            }
        }
        assert!(erros.is_empty(), "espaço fora da escala 2/4/6/8/12/16/24/32:\n{}", erros.join("\n"));
    }

    /// Os raios fora da escala num texto: `.rounded(px(6.))`, `.rounded_t(r(4.))`, um número solto,
    /// os atalhos do Tailwind (`.rounded_md()`, `.rounded_tl_lg()`; o `_full()` passa), o
    /// `ButtonRounded` do kit e o `.radius(..)`/`.corner_radii(..)` com valor solto. Passa o que
    /// vem de `raio::` e o que vem por variável (`.rounded(raio_caixa)`).
    fn fora_dos_raios(texto: &str) -> Vec<String> {
        const SOLTOS: [&str; 5] = ["px(", "r(", "rems(", "tam(", "relative("];
        let mut achados = Vec::new();
        for (i, _) in texto.match_indices('.') {
            let resto = &texto[i + 1..];
            let nome: String = resto.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            let depois = &resto[nome.len()..];
            let lados = ["", "_t", "_b", "_l", "_r", "_tl", "_tr", "_bl", "_br"];
            let com_valor = ["corner_radii", "radius"].contains(&nome.as_str())
                || lados.iter().any(|l| nome == format!("rounded{l}"));
            if com_valor && let Some(dentro) = depois.strip_prefix('(') {
                let dentro = dentro.trim_start();
                if SOLTOS.iter().any(|s| dentro.starts_with(s)) || dentro.starts_with(|c: char| c.is_ascii_digit()) {
                    // O argumento até fechar o parêntese.
                    let mut nivel = 1;
                    let fim = dentro
                        .char_indices()
                        .find(|&(_, c)| {
                            nivel += (c == '(') as i32 - (c == ')') as i32;
                            nivel == 0
                        })
                        .map_or(dentro.len(), |(j, _)| j);
                    achados.push(format!(".{nome}({})", &dentro[..fim]));
                }
            } else if depois.starts_with("()")
                && nome.starts_with("rounded_")
                && !nome.ends_with("_full")
                && !lados.contains(&&nome["rounded".len()..])
            {
                achados.push(format!(".{nome}()"));
            }
        }
        for (i, _) in texto.match_indices("ButtonRounded::") {
            let variante: String = texto[i + 15..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            achados.push(format!("ButtonRounded::{variante}"));
        }
        achados
    }

    /// Só três raios e a pílula em `src/ui` (`raio::CONTROLE`, `CARTAO`, `PECA`, `PILULA` e o
    /// `rounded_full()`), e o tema do kit com os mesmos valores.
    #[test]
    fn raios_na_escala() {
        assert_eq!(
            fora_dos_raios(".rounded(px(6.)).rounded_t(r(4.)).rounded(raio::CARTAO).rounded(8.)"),
            [".rounded(px(6.))", ".rounded_t(r(4.))", ".rounded(8.)"]
        );
        assert_eq!(fora_dos_raios(".rounded_md().rounded_full().rounded_tl_lg().rounded_t_full()"), [".rounded_md()", ".rounded_tl_lg()"]);
        assert_eq!(fora_dos_raios("b.rounded(ButtonRounded::Small)"), ["ButtonRounded::Small"]);
        assert!(fora_dos_raios(".rounded(raio::PECA - px(1.)).rounded(raio_caixa).corner_radii(lado / 2.)").is_empty());
        let raiz = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui"));
        // Quem pode fugir da escala, com o porquê. Fica vazia.
        const EXCECOES: [&str; 0] = [];
        let mut erros = Vec::new();
        for caminho in arquivos_rs(raiz) {
            let nome = caminho.strip_prefix(raiz).unwrap().to_string_lossy().to_string();
            let texto = std::fs::read_to_string(&caminho).unwrap();
            for achado in fora_dos_raios(&texto) {
                if !EXCECOES.iter().any(|e| achado.starts_with(e)) {
                    erros.push(format!("{nome}: {achado}"));
                }
            }
        }
        assert!(erros.is_empty(), "raio fora da escala (raio::CONTROLE, CARTAO, PECA, PILULA):\n{}", erros.join("\n"));
        let json: serde_json::Value = serde_json::from_str(TEMA).unwrap();
        let tema = &json["themes"][0];
        assert_eq!(tema["radius"].as_f64(), Some(f64::from(raio::CONTROLE.as_f32())));
        assert_eq!(tema["radius.lg"].as_f64(), Some(f64::from(raio::PECA.as_f32())));
        assert_eq!(raio::do_icone(32.), raio::CONTROLE);
    }

    /// Toda animação passa por `ui/movimento.rs` (teto de 60 qps, curva curta, movimento
    /// reduzido); fora dele, nada de animação própria nem laço.
    #[test]
    fn movimento_com_teto() {
        let proibidos = [
            concat!("Animation", "::new("),
            concat!(".repeat", "()"),
            concat!("request_animation", "_frame"),
            concat!("transition", "("),
            concat!("spring", "("),
        ];
        let raiz = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        let mut erros = Vec::new();
        for caminho in arquivos_rs(raiz) {
            if caminho.ends_with("ui/movimento.rs") {
                continue;
            }
            let texto = std::fs::read_to_string(&caminho).unwrap();
            for proibido in proibidos {
                if texto.contains(proibido) {
                    erros.push(format!("{}: {proibido}", caminho.strip_prefix(raiz).unwrap().display()));
                }
            }
        }
        assert!(erros.is_empty(), "animação fora de ui/movimento.rs:\n{}", erros.join("\n"));
    }

    /// A retícula (o xadrez e a hachura do shader) só sai pelos invólucros de
    /// `componentes/graficos.rs` (`trama`, `fio_de_pontos`, `hachura`): o lado inteiro em pixel do
    /// aparelho fica num lugar só, e a queda para o liso, se um renderizador perder o padrão, é
    /// uma edição só.
    #[test]
    fn reticula_so_pelos_graficos() {
        let proibidos = [concat!("checkerboard", "("), concat!("pattern_slash", "(")];
        let raiz = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui"));
        let mut erros = Vec::new();
        for caminho in arquivos_rs(raiz) {
            if caminho.ends_with("componentes/graficos.rs") {
                continue;
            }
            let texto = std::fs::read_to_string(&caminho).unwrap();
            for proibido in proibidos {
                if texto.contains(proibido) {
                    erros.push(format!("{}: {proibido}", caminho.strip_prefix(raiz).unwrap().display()));
                }
            }
        }
        assert!(erros.is_empty(), "retícula fora de componentes/graficos.rs:\n{}", erros.join("\n"));
    }

    /// O metal (a chapa da janela e os recortes dela) só sai por `componentes/metal.rs`: a
    /// textura troca a cada batida, e a ordem "pinta a nova, solta a velha" fica num lugar só.
    #[test]
    fn metal_so_pelo_motor() {
        let proibidos = [concat!("paint_", "image("), concat!("drop_", "image(")];
        let raiz = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui"));
        let mut erros = Vec::new();
        for caminho in arquivos_rs(raiz) {
            if caminho.ends_with("componentes/metal.rs") {
                continue;
            }
            let texto = std::fs::read_to_string(&caminho).unwrap();
            for proibido in proibidos {
                if texto.contains(proibido) {
                    erros.push(format!("{}: {proibido}", caminho.strip_prefix(raiz).unwrap().display()));
                }
            }
        }
        assert!(erros.is_empty(), "metal fora de componentes/metal.rs:\n{}", erros.join("\n"));
    }

    /// A barra de rolagem aparece e some seca: o fade do kit pedia quadros no ritmo do monitor
    /// a cada mudança de tamanho do conteúdo.
    #[test]
    fn barra_de_rolagem_sem_fade() {
        use gpui_kit::component::{scroll::ScrollbarMode, scrollbar_motion};
        use std::time::Duration;
        for modo in [ScrollbarMode::Scrolling, ScrollbarMode::Hover, ScrollbarMode::Always] {
            let m = scrollbar_motion(modo);
            assert_eq!(m.enter(), Duration::ZERO);
            assert_eq!(m.exit(), Duration::ZERO);
            assert_eq!(m.expand(), Duration::ZERO);
            assert_eq!(m.idle(), Duration::from_millis(1200));
        }
    }

    /// O ícone do projeto (sem letra) precisa de 3:1 sobre o próprio quadrado.
    #[test]
    fn cores_de_projeto_sobre_a_tinta() {
        let marinho = Rgba::from(Hsla::from(rgb(0x121726)));
        for (cor, nome) in CORES_PROJETO {
            let cor = Hsla::from(rgb(cor));
            let fundo = Rgba::from(Cores::tinta_solida(cor, 0.18));
            let c = contraste(sobre(cor, marinho), fundo);
            assert!(c > 3., "{nome}: {c:.2}:1");
        }
    }
}
