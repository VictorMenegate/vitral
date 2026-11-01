//! Tema "vidro azul-marinho", tirado da Stained Glass UI do VS Code: janela translúcida com tom
//! navy, peças arredondadas de 20 px, sem contornos, destaque azul #3A7FFF. O vidro fica no fundo
//! (trilho, lista, painéis); o preto entra no que vai por cima (popovers, menus, dicas, avisos) e
//! por dentro (código, saída de comando, diff, o fim da caixa de mensagem e os discos do trilho).

use std::{borrow::Cow, rc::Rc};

use gpui_kit::component::{Theme, ThemeSet};
use gpui_kit::{
    Anchor, App, AssetSource, Background, BoxShadow, FontWeight, Hsla, Pixels, Rems, SharedString,
    Styled, Window, linear_color_stop, linear_gradient, px, rems, rgb, rgba,
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

/// Tipografia da linguagem, em rem (cresce com o zoom).
pub trait Tipo: Styled + Sized {
    fn titulo_tela(self) -> Self {
        self.text_size(r(18.)).line_height(r(24.)).font_weight(FontWeight::SEMIBOLD)
    }
    fn titulo_cartao(self) -> Self {
        self.text_size(r(15.)).line_height(r(20.)).font_weight(FontWeight::SEMIBOLD)
    }
    fn rotulo_secao(self) -> Self {
        self.text_size(r(12.))
            .line_height(r(16.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(Cores::tenue())
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
        self.text_size(r(12.)).line_height(r(16.))
    }
    fn micro(self) -> Self {
        self.text_size(r(11.)).line_height(r(14.)).font_weight(FontWeight::SEMIBOLD)
    }
    fn mono(self) -> Self {
        self.font_family(FONTE_MONO).text_size(r(13.)).line_height(r(20.))
    }
    fn mono_pequeno(self) -> Self {
        self.font_family(FONTE_MONO).text_size(r(12.)).line_height(r(16.))
    }
    fn numero(self) -> Self {
        self.text_size(r(20.)).line_height(r(24.)).font_weight(FontWeight::SEMIBOLD)
    }
}

impl<T: Styled> Tipo for T {}

/// Elevação 2 (caixa de mensagem, prévia de arrasto). No GPUI o blur é o desvio padrão: metade
/// do blur do CSS.
pub fn sombra_elevado() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(1.), rgba(0x00000040).into()).blur_radius(px(1.5)),
        BoxShadow::new(px(0.), px(4.), rgba(0x00000033).into())
            .blur_radius(px(4.))
            .spread_radius(px(-2.)),
    ]
}

/// Elevação 3: menu, popover, dica, aviso (a mesma do `popover_style` do vendor).
pub fn sombra_flutuante() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(12.), rgba(0x00000066).into())
            .blur_radius(px(12.))
            .spread_radius(px(-4.)),
        BoxShadow::new(px(0.), px(2.), rgba(0x0000004d).into())
            .blur_radius(px(3.))
            .spread_radius(px(-1.)),
    ]
}

/// Elevação 4: diálogo (a mesma do `dialog_shadow` do vendor).
pub fn sombra_dialogo() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(24.), rgba(0x00000080).into())
            .blur_radius(px(24.))
            .spread_radius(px(-8.)),
        BoxShadow::new(px(0.), px(2.), rgba(0x0000004d).into()).blur_radius(px(4.)),
    ]
}

/// Sombra interna do recuo (trilho do segmentado, campo de busca).
pub fn sombra_recuo() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(1.), rgba(0x00000059).into()).blur_radius(px(1.)).inset()]
}

/// Anel de foco pelo teclado, o mesmo do kit (`FOCUS_RING_WIDTH`).
pub fn halo_foco() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(0.), rgba(0x3a7fff80).into()).spread_radius(px(3.))]
}

/// Halo do foco da caixa e do painel ativo (E3): o destaque desfocado em volta, com `forca` de 0
/// (sem halo) a 1 (alfa 0,22). Vai junto das sombras que a peça já tem.
pub fn halo_suave(forca: f32) -> BoxShadow {
    BoxShadow::new(px(0.), px(0.), Cores::destaque().opacity(0.22 * forca.clamp(0., 1.))).blur_radius(px(12.))
}

/// Linha de luz de 1 px no topo de dentro da peça (o reflexo do vidro), a `forca` de 0 a 1.
pub fn linha_de_luz(forca: f32) -> BoxShadow {
    BoxShadow::new(px(0.), px(1.), Cores::brilho_topo().opacity(forca)).inset()
}

/// O gradiente do reflexo: #ffffff08 no topo até nada, na camada de 40 px do `.reflexo()`.
pub fn gradiente_reflexo() -> Background {
    linear_gradient(180., linear_color_stop(rgba(0xffffff08), 0.), linear_color_stop(rgba(0xffffff00), 1.))
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
        linear_color_stop(rgba(0x3a7fff29), 0.),
        linear_color_stop(rgba(0x3a7fff00), 0.6),
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
        Keyboard,
        LayoutGrid,
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
        Workflow,
        Wrench,
        X,
        Zap,
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

pub fn aplicar(cx: &mut App) {
    let conjunto: ThemeSet = serde_json::from_str(TEMA).expect("tema do Vitral");
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

/// Cores próprias do Vitral que não existem no tema dos componentes.
pub struct Cores;

impl Cores {
    /// Vidro da janela (o `background` do tema): o chumbo das frestas e da margem, mais escuro
    /// que as peças, para elas se destacarem.
    pub fn vidro() -> Hsla {
        rgba(0x060810d4).into()
    }
    /// Fundo das colunas com texto pequeno (trilho e lista): garante o contraste no pior caso e
    /// fica no mesmo tom da conversa sobre o vidro escuro.
    pub fn vidro_leitura() -> Hsla {
        rgba(0x151b2cbf).into()
    }
    /// Tudo o que flutua sobre texto (popover, menu, dica, aviso): quase preto e opaco, por cima
    /// do vidro azul (o `popover.background` do tema).
    pub fn flutuante() -> Hsla {
        rgba(0x0c0e13ff).into()
    }
    /// A gaveta (a lista ou a Equipe por cima da conversa, em janela estreita): flutua, então é
    /// opaca, mas continua no azul das peças.
    pub fn gaveta() -> Hsla {
        rgba(0x1b2339ff).into()
    }
    /// O preto de dentro: código, saída de comando, diff e o disco dos botões redondos do trilho.
    pub fn preto() -> Hsla {
        rgba(0x08090cff).into()
    }
    /// Trilho do segmentado, campo de busca, kbd, selos no vidro.
    pub fn recuo() -> Hsla {
        rgba(0x0a0f1c80).into()
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
        rgba(0x070b1699).into()
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
        Hsla::from(rgb(0x121726)).blend(cor.opacity(alfa))
    }
    /// Hover e realce de item perigoso.
    pub fn perigo_hover() -> Hsla {
        Self::vermelho().opacity(0.14)
    }
    /// Campos, chip aberto, selos de contorno, separador em superfície opaca.
    pub fn borda() -> Hsla {
        rgba(0xffffff14).into()
    }
    /// Contorno de popover, diálogo e lista de escolha; trilho dos anéis.
    pub fn borda_forte() -> Hsla {
        rgba(0xffffff1f).into()
    }
    /// Títulos, item selecionado ou não lido.
    pub fn forte() -> Hsla {
        rgba(0xffffffff).into()
    }
    /// Corpo (o `foreground` do tema).
    pub fn texto() -> Hsla {
        rgba(0xdfe6fbff).into()
    }
    /// Texto sobre selo sólido de cor.
    pub fn sobre_cor() -> Hsla {
        rgba(0x0f1422ff).into()
    }
    /// Só o esforço.
    pub fn ciano() -> Hsla {
        rgba(0x67e7f9ff).into()
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
        rgba(0x1c2c55f0).into()
    }
    /// Fim do gradiente da caixa: o quase preto dos flutuantes, com o alfa do começo.
    pub fn caixa_preta() -> Hsla {
        rgba(0x0c0e13f0).into()
    }
    /// Reflexo azul na borda direita da caixa (camada por cima do gradiente).
    pub fn caixa_brilho() -> Background {
        linear_gradient(
            90.,
            linear_color_stop(rgba(0x3a7fff00), 0.78),
            linear_color_stop(rgba(0x3a7fff14), 1.),
        )
    }
    pub fn brilho_topo() -> Hsla {
        rgba(0xffffff0f).into()
    }
    pub fn sombra_caixa() -> Hsla {
        rgba(0x00000059).into()
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
        rgba(0x141a2ae6).into()
    }
    pub fn cartao_elevado() -> Hsla {
        rgba(0x1a2136f0).into()
    }
    /// Diálogos: opacos, senão o texto da conversa aparece através deles.
    pub fn dialogo() -> Hsla {
        rgba(0x161c2fff).into()
    }
    pub fn destaque() -> Hsla {
        rgba(0x3a7fffff).into()
    }
    /// Azul para texto pequeno: o #3A7FFF fica abaixo de 4,5:1 no vidro.
    pub fn destaque_texto() -> Hsla {
        rgba(0x6b9bffff).into()
    }
    /// Item escolhido numa lista (par com `hover`): azul, mais forte que o hover.
    pub fn selecao() -> Hsla {
        rgba(0x3a7fff33).into()
    }
    /// Mouse por cima de um item de lista: neutro, para não parecer mais escolhido que a seleção.
    pub fn hover() -> Hsla {
        rgba(0xffffff12).into()
    }
    /// Contorno do painel ativo e da caixa com foco: forte o bastante para ver no vidro.
    pub fn destaque_borda() -> Hsla {
        rgba(0x3a7fff99).into()
    }
    pub fn codigo() -> Hsla {
        rgba(0x9ccbffff).into()
    }
    pub fn tenue() -> Hsla {
        rgba(0x9ba8cfff).into()
    }
    /// Metadados (horário, estados vazios, avisos): opaco para dar 4,5:1 até sobre a neblina.
    pub fn apagado() -> Hsla {
        rgba(0x8a97bfff).into()
    }
    /// Só para o que é decorativo (separadores, ícone da busca): pode sumir no fundo.
    pub fn fantasma() -> Hsla {
        rgba(0x9ba8cf99).into()
    }
    pub fn linha() -> Hsla {
        rgba(0xffffff0d).into()
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
        let json: serde_json::Value = serde_json::from_str(TEMA).unwrap();
        let cores = &json["themes"][0]["colors"];
        let fundo = cor_do_tema(cores["background"].as_str().unwrap());
        assert!(mesma_cor(Cores::vidro(), fundo), "Cores::vidro() diferente do background do tema");
        // Os flutuantes do kit (menus, popovers, dicas, avisos) e os desenhados à mão.
        let popover = cor_do_tema(cores["popover.background"].as_str().unwrap());
        assert!(mesma_cor(Cores::flutuante(), popover), "Cores::flutuante() diferente do popover.background");
    }

    /// Texto e cores semânticas passam de 4,5:1 nas superfícies de leitura, com papel de parede
    /// branco atrás da janela (o pior caso).
    #[test]
    fn contraste_dos_tokens_no_pior_caso() {
        let branco = Rgba { r: 1., g: 1., b: 1., a: 1. };
        let vidro = sobre(Cores::vidro(), branco);
        let painel = sobre(Cores::cartao(), vidro);
        let flutuante = sobre(Cores::flutuante(), vidro);
        // O brilho do `.reflexo()` (#ffffff08) clareia o topo das peças, onde fica o título. A
        // linha de luz é de 1 px, sem texto em cima.
        let reflexo = |fundo: Rgba| sobre(gpui_kit::rgba(0xffffff08).into(), fundo);
        let superficies = [
            ("vidro de leitura", sobre(Cores::vidro_leitura(), vidro)),
            ("painel", painel),
            ("flutuante", flutuante),
            ("diálogo", sobre(Cores::dialogo(), vidro)),
            ("gaveta", sobre(Cores::gaveta(), vidro)),
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
        let branco = Rgba { r: 1., g: 1., b: 1., a: 1. };
        let vidro = sobre(Cores::vidro(), branco);
        let fundos = [("flutuante", sobre(Cores::flutuante(), vidro)), ("preto", Rgba::from(Cores::preto()))];
        for (nome_fundo, fundo) in fundos {
            let realce = |cor: Hsla| contraste(sobre(cor, fundo), fundo);
            let (hover, selecao) = (realce(Cores::hover()), realce(Cores::selecao()));
            assert!(hover >= 1.12, "hover sobre {nome_fundo}: {hover:.3}:1");
            assert!(selecao > hover + 0.05, "seleção sobre {nome_fundo}: {selecao:.3}:1 (hover {hover:.3})");
            assert!(realce(Cores::pressionado()) > selecao, "pressionado sobre {nome_fundo}");
            assert!(realce(Cores::borda()) >= 1.12, "borda sobre {nome_fundo}");
        }
        // No flutuante o recuo de sempre some; o claro do recuo_no_flutuante, não.
        let flutuante = fundos[0].1;
        assert!(contraste(sobre(Cores::recuo(), flutuante), flutuante) < 1.02);
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
