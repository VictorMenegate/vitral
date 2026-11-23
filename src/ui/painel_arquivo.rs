//! Painel da direita com um arquivo da pasta aberto: aba com o nome, o cabeçalho do arquivo (o
//! nome, as linhas e a linguagem, a pasta, Mencionar, VS Code), markdown renderizado ou código com
//! número de linha, a imagem (o SVG também) e o aviso de quando não dá para mostrar, com o Abrir
//! fora do binário. Aberto por uma linha de edição da
//! conversa, mostra a edição no próprio arquivo, com as setas de uma mudança à outra
//! (`edicao.rs`). O número da linha escolhe linhas para mencionar. Com o foco no painel, o Ctrl+F
//! busca dentro do arquivo (`BuscaNoArquivo`): a barra flutua no alto do conteúdo, com "3 de 12"
//! e as setas, e o markdown passa para as linhas enquanto ela está aberta. Um clique no texto
//! abre o editor por cima da leitura (`editor_arquivo.rs`): salva sozinho e no Ctrl+S, e uma faixa
//! pergunta o que fazer quando o agente grava o mesmo arquivo. O .md e o .svg têm o seletor
//! Prévia | Fonte, e a prévia do .md mostra o frontmatter numa tabelinha (`previa_markdown.rs`).

use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, ElementExt as _, Sizable as _, Size, WindowExt as _, h_flex,
    highlighter::highlight_code,
    input::{Editor as CampoDeCodigo, Escape, InputState, RopeExt as _},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::{ScrollableElement as _, ScrollbarAxis},
    text::TextView,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use std::{
    cell::{Cell, RefCell},
    io::ErrorKind,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant, SystemTime},
};

use super::{
    Gaveta, Janela,
    busca_conversa::{AchadoAnterior, AchadoSeguinte},
    componentes::{
        ControleSegmentado, EstadoVazio, Flutuante as _, Reflexo as _, Segmento, TamanhoBotao, Tom, aviso, botao_fantasma,
        botao_icone, campo_de_busca, contagem,
    },
    edicao::{Edicao, Fileira, Marca, Vista},
    editor_arquivo::Conflito,
    movimento::{self, De},
};
use crate::{
    motor::Motor,
    tema::{Cores, FONTE_MONO, Lucide, Tipo as _, altura, esp, r, raio},
};

// Ctrl+F com o foco no painel do arquivo, o Esc que fecha a barra e o Ctrl+S do editor.
actions!(vitral, [BuscarNoArquivo, FecharBuscaNoArquivo, SalvarArquivo]);

/// A busca dentro do arquivo aberto: o campo da barra, se ela está aberta e os achados. Dura
/// enquanto a janela existe: fechar a barra guarda o termo para o próximo Ctrl+F.
pub(super) struct BuscaNoArquivo {
    pub(super) campo: Entity<InputState>,
    pub(super) aberta: bool,
    /// A barra já estava na tela no quadro anterior (ela entra uma vez por abertura).
    vista: Cell<bool>,
    indice: RefCell<IndiceDoArquivo>,
    /// O painel rola até o achado atual no próximo quadro.
    rolar: Cell<bool>,
}

impl BuscaNoArquivo {
    pub(super) fn nova(campo: Entity<InputState>) -> Self {
        BuscaNoArquivo { campo, aberta: false, vista: Cell::new(false), indice: Default::default(), rolar: Cell::new(false) }
    }
}

/// Os achados do termo nas linhas do arquivo, refeitos só quando o termo ou o arquivo mudam.
#[derive(Default)]
struct IndiceDoArquivo {
    termo: String,
    /// De quais linhas é o índice (o endereço delas: o arquivo relido tem outras).
    linhas: usize,
    achados: Vec<(usize, usize)>,
    atual: usize,
}

impl IndiceDoArquivo {
    /// Põe o índice em dia; `true` quando o termo é outro (aí o painel rola até o primeiro achado).
    fn atualizar(&mut self, termo: &str, linhas: &Rc<Vec<SharedString>>) -> bool {
        let endereco = Rc::as_ptr(linhas) as usize;
        let mesmo_termo = self.termo == termo;
        if mesmo_termo && self.linhas == endereco {
            return false;
        }
        self.termo = termo.to_string();
        self.linhas = endereco;
        self.achados = achados_nas_linhas(linhas, termo);
        // O arquivo relido com o mesmo termo fica no achado em que estava.
        self.atual = if mesmo_termo { self.atual.min(self.achados.len().saturating_sub(1)) } else { 0 };
        !mesmo_termo
    }
}

/// Onde o termo (já normalizado, sem acento) aparece nas linhas: a linha e a posição do achado
/// entre os dela, na ordem do arquivo.
fn achados_nas_linhas(linhas: &[SharedString], termo: &str) -> Vec<(usize, usize)> {
    if termo.is_empty() {
        return Vec::new();
    }
    linhas.iter().enumerate().flat_map(|(linha, texto)| (0..achados_na_linha(texto, termo)).map(move |k| (linha, k))).collect()
}

/// Quantas vezes o termo (já normalizado) aparece na linha, sem se sobrepor. A linha só com ASCII
/// (quase todo código e todo log) é contada sem dobrar o texto: num arquivo de alguns MB a conta
/// roda nas linhas todas a cada tecla da busca.
fn achados_na_linha(texto: &str, termo: &str) -> usize {
    if !(texto.is_ascii() && termo.is_ascii()) {
        return super::acentos::faixas(texto, termo).len();
    }
    let (texto, termo) = (texto.as_bytes(), termo.as_bytes());
    let (mut i, mut achados) = (0, 0);
    while i + termo.len() <= texto.len() {
        if texto[i..i + termo.len()].eq_ignore_ascii_case(termo) {
            achados += 1;
            i += termo.len();
        } else {
            i += 1;
        }
    }
    achados
}

/// Maior arquivo que abre inteiro (com cor, prévia e edição); acima dele, só para ler.
const LIMITE: u64 = super::editor_arquivo::LIMITE_DE_EDICAO;

/// Maior arquivo que abre para ler (o resto fica para o VS Code ou para o aplicativo do sistema).
const TETO_DE_LEITURA: u64 = 5_000_000;

/// No arquivo grande, a linha desenhada vai até aqui (o JSON minificado numa linha só travaria o
/// desenho do texto).
const LINHA_MAXIMA: usize = 2_000;

/// O aviso do arquivo que abre só para ler, pelo tamanho dele; `None`, ele abre inteiro.
fn nota_de_so_leitura(bytes: u64) -> Option<String> {
    (bytes > LIMITE).then(|| format!("Só leitura: arquivo grande ({})", tamanho(bytes)))
}

/// A linha como é desenhada: a tabulação vira 4 espaços (o GPUI não a desenha como recuo) e, no
/// arquivo grande, a linha comprida demais é cortada.
fn linha_desenhada(linha: &str, cortar: bool) -> String {
    if linha.is_empty() {
        return " ".to_string();
    }
    match linha.char_indices().nth(LINHA_MAXIMA) {
        Some((fim, _)) if cortar => format!("{} …", &linha[..fim]).replace('\t', "    "),
        _ => linha.replace('\t', "    "),
    }
}

/// A largura da coluna do número da linha (em px a 100%): os dígitos × 0,6 em do mono de 13, mais 4.
fn largura_do_numero(linhas: usize) -> f32 {
    linhas.max(1).to_string().len().max(2) as f32 * LETRA_DO_CODIGO + 4.
}

/// O avanço de uma letra do código (em px a 100%): 0,6 em do mono de 13.
const LETRA_DO_CODIGO: f32 = 7.8;

/// O arquivo tem prévia além do texto: o .md renderizado e o .svg desenhado.
fn tem_previa(caminho: &Path) -> bool {
    let extensao = caminho.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
    extensao == "md" || extensao == "svg"
}

/// O limite das imagens é outro: o de texto barraria um print comum, e desenhar uma imagem custa
/// a memória dela decodificada, não o tamanho do arquivo.
const LIMITE_IMAGEM: u64 = 20_000_000;

/// Cores de sintaxe de cada linha do código (faixas em bytes dentro da linha).
type CoresDasLinhas = Rc<Vec<Vec<(Range<usize>, HighlightStyle)>>>;

/// Arquivo aberto já lido, para o painel não ir ao disco a cada quadro.
pub(super) struct ArquivoLido {
    caminho: PathBuf,
    modificado: Option<SystemTime>,
    conferido: Instant,
    conteudo: Conteudo,
    /// O tamanho do arquivo lido: acima do limite da edição ele abre só para ler.
    bytes: u64,
    /// A prévia do .md (o frontmatter e o texto com os links ligados).
    previa: Option<Rc<PreviaLida>>,
    /// Lido antes de o editor gravar por cima: a próxima leitura vai ao disco de qualquer jeito.
    velho: bool,
    /// Chegam do segundo plano pouco depois da leitura; até lá o código sai sem cor.
    cores: Option<CoresDasLinhas>,
    rolagem: UniformListScrollHandle,
    _colorindo: Option<Task<()>>,
    /// Lido para mostrar pronto (o markdown renderizado, o SVG desenhado) ou em linhas; a edição
    /// pede as linhas mesmo no .md e no .svg.
    rico: bool,
    /// A edição posta nas linhas (a do pedido, pelo endereço dele), com a fileira mais longa (dá
    /// a largura da rolagem para o lado): refeita só quando o arquivo ou a edição mudam.
    vista: Option<(usize, Rc<Vista>, usize)>,
}

#[derive(Clone)]
enum Conteudo {
    Aviso(Aviso),
    /// O texto e quantas linhas tem.
    Markdown(SharedString, usize),
    /// Linhas do código e o índice da mais longa (dá a largura da rolagem para o lado).
    Codigo(Rc<Vec<SharedString>>, usize),
    /// Imagem: desenhada, e não "arquivo binário".
    Imagem,
}

/// Por que o arquivo não aparece aqui.
#[derive(Clone, Debug, PartialEq)]
enum Aviso {
    /// O tamanho, em bytes.
    Grande(u64),
    Binario,
    NaoExiste,
    Erro(String),
    Vazio,
}

fn erro_de_leitura(erro: std::io::Error) -> Aviso {
    match erro.kind() {
        ErrorKind::NotFound => Aviso::NaoExiste,
        ErrorKind::PermissionDenied => Aviso::Erro("Sem permissão para ler este arquivo.".into()),
        _ => Aviso::Erro(erro.to_string()),
    }
}

/// O começo do arquivo tem um byte nulo: é binário (PDF, planilha, zip), e não texto grande.
fn comeco_binario(completo: &Path) -> bool {
    use std::io::Read as _;
    let mut comeco = Vec::new();
    std::fs::File::open(completo).is_ok_and(|arquivo| arquivo.take(8192).read_to_end(&mut comeco).is_ok()) && comeco.contains(&0)
}

/// Lê o arquivo para o painel. `rico`: pronto para ler (o markdown renderizado, o SVG desenhado);
/// sem ele, em linhas, que é o que a edição mostra.
fn ler_arquivo(completo: &Path, rico: bool) -> Conteudo {
    let extensao = completo.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
    let markdown = rico && extensao == "md";
    // O SVG é texto e também é imagem: desenhado quando se abre para ver.
    let imagem = crate::claude::tipo_imagem(completo).is_some() || (rico && extensao == "svg");
    // O tamanho antes: um arquivo grande não precisa ser lido para dizer que não abre.
    match std::fs::metadata(completo) {
        Ok(m) if m.is_dir() => return Conteudo::Aviso(Aviso::Erro("É uma pasta, não um arquivo.".into())),
        // Um .png da pasta aparece como imagem; antes caía no "arquivo binário".
        Ok(m) if imagem => {
            return if m.len() > LIMITE_IMAGEM { Conteudo::Aviso(Aviso::Grande(m.len())) } else { Conteudo::Imagem };
        }
        // O PDF de 2 MB é binário, e não "grande demais": a saída dele é o aplicativo do sistema.
        Ok(m) if m.len() > LIMITE && comeco_binario(completo) => return Conteudo::Aviso(Aviso::Binario),
        Ok(m) if m.len() > TETO_DE_LEITURA => return Conteudo::Aviso(Aviso::Grande(m.len())),
        Ok(_) => {}
        Err(erro) => return Conteudo::Aviso(erro_de_leitura(erro)),
    }
    let (texto, grande) = match std::fs::read(completo) {
        Ok(bytes) if bytes.is_empty() => return Conteudo::Aviso(Aviso::Vazio),
        Ok(bytes) if bytes.len() as u64 > TETO_DE_LEITURA => return Conteudo::Aviso(Aviso::Grande(bytes.len() as u64)),
        Ok(bytes) if bytes.contains(&0) => return Conteudo::Aviso(Aviso::Binario),
        Ok(bytes) => (String::from_utf8_lossy(&bytes).to_string(), bytes.len() as u64 > LIMITE),
        Err(erro) => return Conteudo::Aviso(erro_de_leitura(erro)),
    };
    // O .md grande sai em linhas: a prévia de um texto desse tamanho travaria a janela.
    if markdown && !grande {
        let linhas = texto.lines().count();
        return Conteudo::Markdown(texto.into(), linhas);
    }
    let linhas: Vec<SharedString> = texto.lines().map(|l| SharedString::from(linha_desenhada(l, grande))).collect();
    let mais_longa = (0..linhas.len()).max_by_key(|&i| linhas[i].chars().count()).unwrap_or(0);
    Conteudo::Codigo(Rc::new(linhas), mais_longa)
}

/// "1,2 MB", "450 KB" (em potências de mil, como o limite).
fn tamanho(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.).replace('.', ",")
    } else {
        format!("{} KB", (bytes + 500) / 1000)
    }
}

/// A linguagem para o realce: a extensão, ou o nome para arquivos sem ela (Makefile, Dockerfile).
pub(super) fn linguagem(caminho: &Path) -> String {
    let nome = caminho.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    match nome.rsplit_once('.') {
        Some((_, extensao)) if !extensao.is_empty() => extensao.to_string(),
        _ => nome,
    }
}

/// O nome da linguagem na barra do arquivo ("Rust", "Markdown"), pela extensão.
fn nome_da_linguagem(caminho: &str) -> Option<&'static str> {
    let nome = caminho.rsplit('/').next().unwrap_or(caminho);
    let extensao = nome.rsplit_once('.').map(|(_, e)| e.to_lowercase());
    Some(match (nome, extensao.as_deref()) {
        ("Makefile" | "makefile", _) => "Makefile",
        ("Dockerfile", _) => "Dockerfile",
        (_, Some("rs")) => "Rust",
        (_, Some("py")) => "Python",
        (_, Some("js" | "mjs" | "cjs" | "jsx")) => "JavaScript",
        (_, Some("ts" | "mts" | "cts" | "tsx")) => "TypeScript",
        (_, Some("json" | "jsonc")) => "JSON",
        (_, Some("toml")) => "TOML",
        (_, Some("yaml" | "yml")) => "YAML",
        (_, Some("md" | "markdown")) => "Markdown",
        (_, Some("txt")) => "Texto",
        (_, Some("html" | "htm")) => "HTML",
        (_, Some("css")) => "CSS",
        (_, Some("scss" | "sass")) => "SCSS",
        (_, Some("sh" | "bash" | "zsh" | "fish")) => "Shell",
        (_, Some("go")) => "Go",
        (_, Some("java")) => "Java",
        (_, Some("kt" | "kts")) => "Kotlin",
        (_, Some("c" | "h")) => "C",
        (_, Some("cpp" | "cc" | "cxx" | "hpp" | "hh")) => "C++",
        (_, Some("cs")) => "C#",
        (_, Some("rb")) => "Ruby",
        (_, Some("php")) => "PHP",
        (_, Some("sql")) => "SQL",
        (_, Some("swift")) => "Swift",
        (_, Some("lua")) => "Lua",
        (_, Some("xml")) => "XML",
        (_, Some("svg")) => "SVG",
        (_, Some("vue")) => "Vue",
        (_, Some("svelte")) => "Svelte",
        (_, Some("dart")) => "Dart",
        (_, Some("ex" | "exs")) => "Elixir",
        (_, Some("zig")) => "Zig",
        (_, Some("nix")) => "Nix",
        _ => return None,
    })
}

/// Ícone da aba: imagem, código, texto (markdown e .txt) ou um arquivo qualquer.
pub(super) fn icone_do_arquivo(caminho: &str) -> Lucide {
    if crate::claude::imagem_de_ver(Path::new(caminho)) {
        return Lucide::FileImage;
    }
    match nome_da_linguagem(caminho) {
        Some("Markdown" | "Texto") => Lucide::FileText,
        Some(_) => Lucide::FileCode,
        None => Lucide::File,
    }
}

/// Reparte as cores do texto inteiro (as linhas juntadas com '\n') pelas linhas, com as faixas
/// contadas a partir do começo de cada linha.
fn cores_por_linha(
    linhas: &[SharedString],
    estilos: Vec<(Range<usize>, HighlightStyle)>,
) -> Vec<Vec<(Range<usize>, HighlightStyle)>> {
    let mut inicios = Vec::with_capacity(linhas.len());
    let mut posicao = 0;
    for linha in linhas {
        inicios.push(posicao);
        posicao += linha.len() + 1;
    }
    let mut saida = vec![Vec::new(); linhas.len()];
    let mut atual = 0;
    for (faixa, estilo) in estilos {
        while atual + 1 < linhas.len() && inicios[atual + 1] <= faixa.start {
            atual += 1;
        }
        let mut i = atual;
        while i < linhas.len() && inicios[i] < faixa.end {
            let (comeco, fim) = (inicios[i], inicios[i] + linhas[i].len());
            let (de, ate) = (faixa.start.max(comeco), faixa.end.min(fim));
            if de < ate {
                saida[i].push((de - comeco..ate - comeco, estilo));
            }
            i += 1;
        }
    }
    saida
}

/// O fundo de uma fileira do código: as linhas escolhidas (as do link da conversa ou as que você
/// marcou) e, na edição, a que entrou e a que saiu.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fundo {
    Nenhum,
    Escolhida,
    Entrou,
    Saiu,
}

/// Uma fileira do código: mono 13/20, o número alinhado à direita em apagado e 16 até o texto. Na
/// edição, a coluna do sinal (+ e −) entre os dois, a faixa da cor na borda e o fundo da cor (o
/// mesmo do diff da trilha). O número escolhe a linha para mencionar; com Shift, estende a escolha
/// (dono, 16/11/2026: "uma seleção de linhas no painel também pode ser mencionada").
#[allow(clippy::too_many_arguments)]
fn fileira_de_codigo(
    indice: usize,
    numero: Option<usize>,
    texto: SharedString,
    cores: Vec<(Range<usize>, HighlightStyle)>,
    largura_numero: Rems,
    fundo: Fundo,
    sinal: Option<&'static str>,
    achados: Vec<(Range<usize>, HighlightStyle)>,
    editavel: bool,
    janela: WeakEntity<Janela>,
) -> AnyElement {
    // Os achados da busca (o fundo âmbar) por cima das cores da sintaxe.
    let cores: Vec<_> = if achados.is_empty() { cores } else { combine_highlights(cores, achados).collect() };
    // Uma faixa fora da linha derrubaria o desenho do texto.
    let cores: Vec<_> = cores
        .into_iter()
        .filter(|(f, _)| f.end <= texto.len() && texto.is_char_boundary(f.start) && texto.is_char_boundary(f.end))
        .collect();
    let faixa = match fundo {
        Fundo::Entrou => Some(Cores::verde()),
        Fundo::Saiu => Some(Cores::vermelho()),
        Fundo::Nenhum | Fundo::Escolhida => None,
    };
    let janela_do_numero = janela.clone();
    let numero_el = div()
        .id(("numero-da-linha", indice))
        .w(largura_numero)
        .flex_none()
        .flex()
        .justify_end()
        .text_color(if fundo == Fundo::Escolhida { Cores::texto() } else { Cores::apagado() })
        .children(numero.map(|n| n.to_string()))
        .when_some(numero, |s, n| {
            s.cursor_pointer().hover(|s| s.text_color(Cores::texto())).on_click(move |evento, _, cx| {
                // O clique no número escolhe a linha, e não abre o editor.
                cx.stop_propagation();
                let estender = evento.modifiers().shift;
                let _ = janela_do_numero.update(cx, |this, cx| this.escolher_linha(n as u32, estender, cx));
            })
        });
    div()
        .id(("fileira-de-codigo", indice))
        .relative()
        .flex()
        // A fileira vai até a borda do painel: o clique depois do fim de uma linha curta vale, e o
        // fundo da linha escolhida não acaba no meio.
        .min_w_full()
        // Um clique no texto abre o editor ali.
        .when_some(numero.filter(|_| editavel), |s, n| {
            s.cursor_text().on_click(move |evento, window, cx| {
                let x = evento.position().x;
                let _ = janela.update(cx, |this, cx| this.clicou_no_texto(n - 1, x, window, cx));
            })
        })
        .h(r(20.))
        .pl(esp::E12)
        .pr(esp::E24)
        .gap(if sinal.is_some() { esp::E8 } else { esp::E16 })
        .mono()
        .whitespace_nowrap()
        .map(|s| match fundo {
            Fundo::Escolhida => s.bg(Cores::selecao()),
            Fundo::Entrou => s.bg(super::mensagens::fundo_entra()),
            Fundo::Saiu => s.bg(super::mensagens::fundo_sai()),
            Fundo::Nenhum => s,
        })
        .when_some(faixa, |s, cor| s.child(div().absolute().left_0().top_0().bottom_0().w(px(2.)).bg(cor)))
        .child(numero_el)
        .when_some(sinal, |s, sinal| {
            s.child(div().flex_none().w(r(8.)).text_color(faixa.unwrap_or(Cores::apagado())).child(sinal))
        })
        .child(
            div()
                .flex_none()
                .text_color(if fundo == Fundo::Saiu { Cores::tenue() } else { Cores::texto() })
                .child(StyledText::new(if texto.is_empty() { SharedString::from(" ") } else { texto }).with_highlights(cores)),
        )
        .into_any_element()
}

/// Uma nota entre as fileiras da edição ("… mais 12 linhas saíram", "O arquivo mudou depois
/// desta edição"), na coluna do texto, em tom tênue.
fn nota_de_codigo(texto: SharedString, largura_numero: Rems) -> AnyElement {
    div()
        .flex()
        .items_center()
        .h(r(20.))
        .pl(esp::E12)
        .pr(esp::E24)
        .gap(esp::E8)
        .whitespace_nowrap()
        .metadado()
        .text_color(Cores::tenue())
        .child(div().flex_none().w(largura_numero))
        .child(div().flex_none().w(r(8.)))
        .child(texto)
        .into_any_element()
}

/// O que o aviso de um arquivo que não abre aqui oferece.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Saida {
    /// O aplicativo padrão do sistema (o leitor de PDF, a planilha).
    Fora,
    Editor,
    Fechar,
    /// O arquivo vazio abre no editor, para você escrever nele.
    Escrever,
}

/// As saídas de cada aviso, na ordem dos botões. O binário (PDF, planilha, zip) só tem o que
/// fazer fora: o VS Code não o mostra. O grande demais pode ser texto (VS Code) ou não (fora).
fn saidas_do_aviso(aviso: &Aviso) -> &'static [Saida] {
    match aviso {
        Aviso::Binario => &[Saida::Fora],
        Aviso::Grande(_) => &[Saida::Editor, Saida::Fora],
        Aviso::NaoExiste | Aviso::Erro(_) => &[Saida::Fechar],
        Aviso::Vazio => &[Saida::Escrever],
    }
}

/// O nome do tipo no aviso do binário, pela extensão ("Arquivo PDF", "Planilha"); sem saber,
/// "Arquivo binário".
fn titulo_do_binario(caminho: &Path) -> &'static str {
    let extensao = caminho.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
    match extensao.as_str() {
        "pdf" => "Arquivo PDF",
        "xlsx" | "xls" | "ods" | "numbers" => "Planilha",
        "docx" | "doc" | "odt" | "rtf" | "pages" => "Documento",
        "pptx" | "ppt" | "odp" | "key" => "Apresentação",
        "zip" | "gz" | "tgz" | "tar" | "rar" | "7z" | "xz" | "bz2" | "zst" => "Arquivo compactado",
        "mp4" | "mov" | "mkv" | "webm" | "avi" => "Vídeo",
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "opus" => "Áudio",
        "bmp" | "ico" | "avif" | "tiff" | "tif" | "heic" | "psd" => "Imagem",
        "ttf" | "otf" | "woff" | "woff2" => "Fonte",
        "dwg" | "dxf" => "Desenho do CAD",
        _ => "Arquivo binário",
    }
}

/// A linha em que o VS Code abre: a primeira das escolhidas; na edição, a da mudança à vista; ou
/// a que o link da conversa marcou.
fn linha_do_editor(selecao: Option<(u32, u32)>, mudanca: Option<(u32, u32)>, marcada: Option<u32>) -> Option<u32> {
    selecao.or(mudanca).map(|(a, _)| a).or(marcada)
}

/// Para onde a seta da edição leva.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Passo {
    /// Outra mudança do mesmo arquivo.
    Mudanca(usize),
    /// Outro arquivo da lista de Mudanças: na primeira mudança dele, ou na última (`no_fim`,
    /// quando se volta).
    Arquivo { indice: usize, no_fim: bool },
}

/// O passo das setas da edição. `arquivos` é a posição do arquivo aberto na lista de Mudanças em
/// que ele está e quantos ela tem: com mais de um, a seta passa da última mudança ao arquivo
/// seguinte (e da primeira ao anterior), dando a volta na lista; sem lista, dá a volta dentro do
/// arquivo. `None`: não há para onde ir.
fn passo_da_revisao(atual: usize, mudancas: usize, arquivos: Option<(usize, usize)>, passo: isize) -> Option<Passo> {
    let atual = atual.min(mudancas.saturating_sub(1));
    let alvo = atual as isize + passo;
    if (0..mudancas as isize).contains(&alvo) {
        return Some(Passo::Mudanca(alvo as usize));
    }
    match arquivos.filter(|(_, total)| *total > 1) {
        Some((posicao, total)) => {
            let indice = (posicao as isize + passo.signum()).rem_euclid(total as isize) as usize;
            Some(Passo::Arquivo { indice, no_fim: passo < 0 })
        }
        None if mudancas > 0 => Some(Passo::Mudanca(alvo.rem_euclid(mudancas as isize) as usize)),
        None => None,
    }
}

/// O nome que a barra do arquivo mostra para a pasta: "Sem pasta" na pasta vazia de um chat, senão
/// o da pasta.
fn nome_da_raiz(motor: &Motor, raiz: &std::path::Path) -> String {
    if raiz.starts_with(motor.dados.pasta_avulso("")) {
        return "Sem pasta".into();
    }
    super::dialogos::nome_da_pasta(raiz)
}

/// O que o painel mostra do arquivo lido: o conteúdo, as cores do código se já chegaram, a
/// rolagem, o tamanho do arquivo e, no .md, a prévia pronta.
struct Visto {
    conteudo: Conteudo,
    cores: Option<CoresDasLinhas>,
    rolagem: UniformListScrollHandle,
    bytes: u64,
    previa: Option<Rc<PreviaLida>>,
}

/// A prévia de um .md: as linhas do frontmatter e o texto com os links de arquivo já ligados.
/// Feita uma vez por leitura do arquivo, e não a cada quadro.
struct PreviaLida {
    alto: Vec<(SharedString, SharedString)>,
    corpo: SharedString,
}

impl Janela {
    /// O conteúdo do arquivo aberto, relido só quando ele muda (a data é conferida no máximo a
    /// cada segundo), com as cores do código se já chegaram e a rolagem.
    fn conteudo_arquivo(&self, raiz: &Path, completo: &Path, rico: bool, cx: &Context<Self>) -> Visto {
        let mut lido = self.arquivo_lido.borrow_mut();
        let data = || std::fs::metadata(completo).and_then(|m| m.modified()).ok();
        let visto = |l: &ArquivoLido| Visto {
            conteudo: l.conteudo.clone(),
            cores: l.cores.clone(),
            rolagem: l.rolagem.clone(),
            bytes: l.bytes,
            previa: l.previa.clone(),
        };
        if let Some(l) = lido.as_mut()
            && l.caminho == completo
            && l.rico == rico
            && !l.velho
        {
            if l.conferido.elapsed() < Duration::from_secs(1) {
                return visto(l);
            }
            l.conferido = Instant::now();
            if data() == l.modificado {
                return visto(l);
            }
        }
        // O mesmo arquivo relido continua onde estava rolado; outro começa do topo.
        let rolagem = lido
            .as_ref()
            .filter(|l| l.caminho == completo)
            .map(|l| l.rolagem.clone())
            .unwrap_or_default();
        let modificado = data();
        let bytes = std::fs::metadata(completo).map(|m| m.len()).unwrap_or(0);
        let conteudo = ler_arquivo(completo, rico);
        let colorindo = match &conteudo {
            // O arquivo grande sai sem cor: colorir alguns MB levaria dezenas de segundos.
            Conteudo::Codigo(linhas, _) if bytes <= LIMITE => Some(colorir(completo, modificado, linhas, cx)),
            _ => None,
        };
        let previa = match &conteudo {
            Conteudo::Markdown(texto, _) => {
                let (alto, corpo) = super::previa_markdown::frontmatter(texto);
                let corpo = super::previa_markdown::ligar_links(corpo, completo.parent().unwrap_or(raiz), raiz);
                let alto = alto.into_iter().map(|(chave, valor)| (chave.into(), valor.into())).collect();
                Some(Rc::new(PreviaLida { alto, corpo: corpo.into() }))
            }
            _ => None,
        };
        let novo = ArquivoLido {
            caminho: completo.to_path_buf(),
            modificado,
            conferido: Instant::now(),
            conteudo,
            bytes,
            velho: false,
            previa,
            cores: None,
            rolagem,
            _colorindo: colorindo,
            rico,
            vista: None,
        };
        let resposta = visto(&novo);
        *lido = Some(novo);
        resposta
    }

    pub(super) fn painel_arquivo(&self, motor: &Entity<Motor>, window: &Window, cx: &Context<Self>) -> AnyElement {
        let Some((raiz, caminho)) = self.arquivo_aberto.clone() else {
            return div().into_any_element();
        };
        let completo = raiz.join(&caminho);
        // Na raiz da pasta, a barra mostra o nome dela ("Sem pasta" na pasta vazia de um chat).
        let pasta = match caminho.rsplit_once('/') {
            Some((d, _)) => format!("{d}/"),
            None => nome_da_raiz(motor.read(cx), &raiz),
        };
        // Na edição, o markdown e o SVG saem em linhas, como o código: o diff é por linha.
        // Com a busca aberta também (ela é pelas linhas), e quando você escolhe a Fonte.
        let edicao = self.arquivo_edicao.clone();
        let com_previa = tem_previa(&completo);
        let rico = edicao.is_none() && !self.busca_arquivo.aberta && !(com_previa && self.arquivo_fonte);
        // Com o editor aberto a leitura fica parada: o arquivo não é relido nem colorido de novo
        // a cada gravação.
        let editando = self.editando(&completo);
        let Visto { conteudo, cores, rolagem: rolagem_codigo, bytes, previa } = if editando {
            Visto { conteudo: Conteudo::Aviso(Aviso::Vazio), cores: None, rolagem: Default::default(), bytes: 0, previa: None }
        } else {
            self.conteudo_arquivo(&raiz, &completo, rico, cx)
        };
        // O arquivo que abre inteiro se edita com um clique no texto; o grande é só para ler.
        let editavel = bytes <= LIMITE;
        let janela = cx.entity().downgrade();
        let na_gaveta = self.gaveta == Some(Gaveta::Direita);
        let medir = self.medir_direita(cx);
        let vista = match (&edicao, &conteudo) {
            (Some(e), Conteudo::Codigo(linhas, _)) if !e.sem_registro => Some(self.vista_da_edicao(e, linhas)),
            _ => None,
        };

        let linhas = match &conteudo {
            Conteudo::Codigo(linhas, _) => Some(linhas.len()),
            Conteudo::Markdown(_, n) => Some(*n),
            Conteudo::Aviso(_) | Conteudo::Imagem => None,
        };
        // Os anéis de uso do canto da janela ficam embaixo do × (dono, 13/11 e 16/11/2026): as
        // fileiras de cima do painel guardam o lugar deles; o conteúdo, não.
        let reserva = self.reserva_dos_aneis(window, cx);
        let cabecalho = self.barra_de_abas(motor.read(cx), super::abas_direita::AbaDireita::Arquivo(0), false, "arrastar-arquivo", window, cx);
        // O seletor Prévia | Fonte só existe para o que tem prévia e abre inteiro, fora da edição
        // do agente e da busca (que são pelas linhas). Com o editor aberto ele fica: a Prévia
        // grava o que falta e mostra o resultado.
        let seletor = com_previa && edicao.is_none() && !self.busca_arquivo.aberta && (editando || editavel);
        let barra = self.cabecalho_do_arquivo(&completo, &caminho, pasta, linhas, seletor, reserva, na_gaveta, window, cx);
        let faixa = edicao.as_ref().map(|e| self.faixa_da_edicao(e, vista.as_ref().map(|(v, _)| v.as_ref()), reserva, na_gaveta, window, cx));
        // O aviso discreto do arquivo que abre só para ler, e a pergunta do conflito no editor.
        let faixa_de_leitura = match &conteudo {
            Conteudo::Codigo(..) => nota_de_so_leitura(bytes).map(|nota| faixa_de_nota(nota, reserva, na_gaveta, window)),
            _ => None,
        };
        let faixa_do_conflito = self
            .editor_arquivo
            .as_ref()
            .filter(|_| editando)
            .and_then(|e| e.conflito_para_a_faixa())
            .map(|(conflito, nova)| self.faixa_do_conflito(conflito, nova, reserva, na_gaveta, window, cx));
        // O achado atual e quantos são, para a barra da busca (só o arquivo em linhas tem).
        let mut contagem_da_busca: Option<(usize, usize)> = None;
        let mut corpo_do_editor = self.editor_arquivo.as_ref().filter(|_| editando).map(|editor| {
            // O texto do editor fica onde estava o da leitura, para nada pular no clique: o
            // campo do kit tem 6 px de margem, a calha de pelo menos 3 dígitos e mais 6 px, e 8 px
            // em cima, sem crescer com o zoom; a leitura tem 12, o número, 16 e 8, que crescem.
            let escala = window.rem_size().as_f32() / 15.;
            let linhas = editor.estado.read(cx).text().lines_len();
            let digitos = linhas.max(1).to_string().len().max(3) as f32;
            let do_campo = 6. + digitos * LETRA_DO_CODIGO * escala + 6.;
            let da_leitura = (12. + largura_do_numero(linhas) + 16.) * escala;
            div()
                .flex_1()
                .min_h_0()
                .pl(px((da_leitura - do_campo).max(0.)))
                .pt(px((8. * escala - 8.).max(0.)))
                // O Esc que o campo não usou (fechar um menu dele) volta à leitura.
                .on_action(cx.listener(|this, _: &Escape, window, cx| this.sair_do_editor(window, cx)))
                .child(
                    CampoDeCodigo::new(&editor.estado)
                        .appearance(false)
                        .bordered(false)
                        // A calha fica por cima do texto que rola para o lado: opaca (a cor da
                        // gaveta tem 2% de transparência), na cor da superfície em que o painel está.
                        .gutter_background(if na_gaveta { Cores::gaveta() } else { Cores::cartao_opaco() }.alpha(1.))
                        .h(relative(1.))
                        .text_size(r(13.))
                        .line_height(r(20.)),
                )
                .into_any_element()
        });
        let corpo = match conteudo {
            _ if editando => corpo_do_editor.take().unwrap_or_else(|| div().into_any_element()),
            Conteudo::Aviso(aviso) => aviso_do_arquivo(aviso, janela.clone(), completo.clone(), window, cx),
            // Sobre o preto, como no visor: imagem com transparência não some no vidro. O clique
            // leva ao visor, com o tamanho real e o "Abrir fora".
            Conteudo::Imagem => {
                let lido = super::imagens::anexo(&completo);
                let alvo = completo.clone();
                let detalhe = lido.detalhe();
                v_flex()
                    .id("imagem-arquivo")
                    .flex_1()
                    .min_h_0()
                    .gap(esp::E8)
                    .p(esp::E16)
                    .items_center()
                    .cursor_pointer()
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(raio::CARTAO)
                            .bg(super::imagens::fundo_da_imagem(&completo))
                            .overflow_hidden()
                            .child(img(completo.clone()).max_w_full().max_h_full().object_fit(ObjectFit::Contain)),
                    )
                    .when(!detalhe.is_empty(), |s| {
                        s.child(div().flex_none().metadado().text_color(Cores::tenue()).child(detalhe))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| this.abrir_visor("", &alvo, window, cx)))
                    .into_any_element()
            }
            Conteudo::Markdown(texto, _) => {
                // O frontmatter vira a tabelinha do alto; o resto, com os links de arquivo ligados.
                let (alto, corpo) = match previa.as_deref() {
                    Some(previa) => (tabela_do_frontmatter(&previa.alto), previa.corpo.clone()),
                    None => (None, texto),
                };
                div()
                    .id("conteudo-arquivo")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .max_w(r(720.))
                            .mx_auto()
                            .px(esp::E16)
                            .py(esp::E16)
                            .corpo_ui()
                            .children(alto)
                            .child(
                                TextView::markdown(SharedString::from(format!("arquivo-{caminho}")), corpo)
                                    .selectable(true)
                                    .on_link_click(|url, _, window, cx| match super::previa_markdown::destino_do_link(url) {
                                        super::previa_markdown::Destino::Arquivo => {
                                            window.dispatch_action(Box::new(super::links_arquivo::AbrirLinkVitral(url.to_string())), cx)
                                        }
                                        super::previa_markdown::Destino::Web => cx.open_url(url),
                                        super::previa_markdown::Destino::Nenhum => {}
                                    }),
                            ),
                    )
                    .into_any_element()
            }
            // Só as linhas visíveis são montadas: arquivos de milhares de linhas não pesam.
            Conteudo::Codigo(linhas, mais_longa) => {
                let largura_numero = r(largura_do_numero(linhas.len()));
                // O link da conversa pediu uma linha: rola até ela uma vez (com folga em cima) e
                // a deixa marcada enquanto o arquivo estiver aberto.
                if let Some(linha) = self.arquivo_linha.take() {
                    let alvo = (linha as usize).saturating_sub(1).min(linhas.len().saturating_sub(1));
                    rolagem_codigo.scroll_to_item(alvo.saturating_sub(4), ScrollStrategy::Top);
                }
                // As linhas escolhidas: as do link, ou as marcadas no número (e as da pílula de um
                // trecho, que abre com elas).
                let (selecao, marcada) = (self.arquivo_selecao, self.arquivo_marcada.get());
                let escolhida = move |n: usize| {
                    selecao.is_some_and(|(a, b)| (a as usize..=b as usize).contains(&n)) || marcada == Some(n as u32)
                };
                // A busca no arquivo: o termo e o achado atual (a linha e qual achado dela).
                let busca = self.busca_arquivo.aberta.then(|| {
                    let termo = super::acentos::normalizar(&self.busca_arquivo.campo.read(cx).value());
                    let mut indice = self.busca_arquivo.indice.borrow_mut();
                    if indice.atualizar(&termo, &linhas) {
                        self.busca_arquivo.rolar.set(true);
                    }
                    contagem_da_busca = Some((indice.atual, indice.achados.len()));
                    (SharedString::from(termo), indice.achados.get(indice.atual).copied())
                });
                let achado_atual = busca.as_ref().and_then(|(_, atual)| *atual);
                let rolar_ao_achado = self.busca_arquivo.rolar.replace(false);
                // O fundo de cada achado numa linha do arquivo (o do atual, mais forte).
                let achados_da_linha = move |linha: usize, texto: &str| match &busca {
                    Some((termo, atual)) if !termo.is_empty() => {
                        let na_linha = atual.filter(|(l, _)| *l == linha).map(|(_, k)| k);
                        super::busca_conversa::destaques_do_trecho(texto, termo, na_linha)
                    }
                    _ => Vec::new(),
                };
                let id = SharedString::from(format!("codigo-{caminho}"));
                let lista = match vista {
                    // A edição: as fileiras da vista, com as saídas entre as linhas do arquivo. Ao
                    // abrir e a cada seta, rola até a mudança (com folga em cima).
                    Some((vista, mais_longa)) => {
                        if self.edicao_rolar.replace(false)
                            && let Some(mudanca) = vista.mudancas.get(self.edicao_atual.min(vista.mudancas.len().saturating_sub(1)))
                        {
                            rolagem_codigo.scroll_to_item(mudanca.fileira.saturating_sub(4), ScrollStrategy::Top);
                        }
                        // O achado atual da busca, pela fileira em que a linha dele está.
                        if rolar_ao_achado
                            && let Some((linha, _)) = achado_atual
                            && let Some(fileira) = vista.fileiras.iter().position(|f| matches!(f, Fileira::Arquivo { linha: l, .. } if *l == linha))
                        {
                            rolagem_codigo.scroll_to_item(fileira, ScrollStrategy::Center);
                        }
                        let janela = janela.clone();
                        uniform_list(id, vista.fileiras.len(), move |faixa, _, _| {
                            faixa
                                .map(|i| match &vista.fileiras[i] {
                                    Fileira::Arquivo { linha, entrou } => {
                                        let cores_linha = cores.as_ref().and_then(|c| c.get(*linha)).cloned().unwrap_or_default();
                                        let fundo = match (escolhida(linha + 1), entrou) {
                                            (true, _) => Fundo::Escolhida,
                                            (false, true) => Fundo::Entrou,
                                            (false, false) => Fundo::Nenhum,
                                        };
                                        let sinal = if *entrou { "+" } else { " " };
                                        let texto = linhas.get(*linha).cloned().unwrap_or_default();
                                        let achados = achados_da_linha(*linha, &texto);
                                        fileira_de_codigo(i, Some(linha + 1), texto, cores_linha, largura_numero, fundo, Some(sinal), achados, editavel, janela.clone())
                                    }
                                    Fileira::Saiu(texto) => {
                                        fileira_de_codigo(i, None, texto.clone().into(), Vec::new(), largura_numero, Fundo::Saiu, Some("−"), Vec::new(), false, janela.clone())
                                    }
                                    Fileira::Solta(marca, texto) => {
                                        let (fundo, sinal) = match marca {
                                            Marca::Entra => (Fundo::Entrou, "+"),
                                            Marca::Sai => (Fundo::Saiu, "−"),
                                            Marca::Igual => (Fundo::Nenhum, " "),
                                        };
                                        fileira_de_codigo(i, None, texto.clone().into(), Vec::new(), largura_numero, fundo, Some(sinal), Vec::new(), false, janela.clone())
                                    }
                                    Fileira::Nota(texto) => nota_de_codigo(texto.clone().into(), largura_numero),
                                })
                                .collect()
                        })
                        .with_width_from_item(Some(mais_longa))
                    }
                    None => {
                        if rolar_ao_achado && let Some((linha, _)) = achado_atual {
                            rolagem_codigo.scroll_to_item(linha, ScrollStrategy::Center);
                        }
                        let janela = janela.clone();
                        uniform_list(id, linhas.len(), move |faixa, _, _| {
                            faixa
                                .map(|i| {
                                    let cores_linha = cores.as_ref().and_then(|c| c.get(i)).cloned().unwrap_or_default();
                                    let fundo = if escolhida(i + 1) { Fundo::Escolhida } else { Fundo::Nenhum };
                                    let achados = achados_da_linha(i, &linhas[i]);
                                    fileira_de_codigo(i, Some(i + 1), linhas[i].clone(), cores_linha, largura_numero, fundo, None, achados, editavel, janela.clone())
                                })
                                .collect()
                        })
                        .with_width_from_item(Some(mais_longa))
                    }
                };
                // Com a barra da busca aberta, as primeiras linhas descem para não ficar atrás dela.
                let respiro_de_cima = if self.busca_arquivo.aberta { r(60.) } else { esp::E8 };
                // A esquerda da lista, para o clique no texto saber em que coluna caiu.
                let esquerda = self.codigo_esquerda.clone();
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .on_prepaint(move |limites, _, _| esquerda.set(limites.origin.x))
                    .child(
                        lista
                            .with_sizing_behavior(ListSizingBehavior::Auto)
                            .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                            .track_scroll(&rolagem_codigo)
                            .size_full()
                            .pt(respiro_de_cima)
                            .pb(esp::E8),
                    )
                    .scrollbar(&rolagem_codigo, ScrollbarAxis::Both)
                    .into_any_element()
            }
        };
        // A barra da busca flutua no alto do conteúdo, à direita, e nunca mais larga que o painel.
        // Com os anéis de uso no canto da janela, ela guarda o lugar deles, como as fileiras de cima.
        let barra_da_busca = self.barra_da_busca_no_arquivo(contagem_da_busca, window, cx).map(|(barra, nova)| {
            let direita = reserva.unwrap_or(esp::E8.to_pixels(window.rem_size()));
            div().absolute().top(esp::E8).right(direita).left(esp::E8).flex().justify_end().child(
                div().w(r(340.)).max_w(relative(1.)).child(movimento::entrada("entra-busca-no-arquivo", nova, De::Cima, barra)),
            )
        });
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            // O clique no painel leva o foco para ele: daí o Ctrl+F é do arquivo, e não da conversa.
            .key_context("Arquivo")
            .track_focus(&self.foco_arquivo)
            .on_action(cx.listener(|this, _: &BuscarNoArquivo, window, cx| this.abrir_busca_no_arquivo(window, cx)))
            // O campo da barra liga o Ctrl+F ao `Search` do kit: com o foco nele, a busca continua a do arquivo.
            .on_action(cx.listener(|this, _: &gpui_kit::component::input::Search, window, cx| this.abrir_busca_no_arquivo(window, cx)))
            .on_action(cx.listener(|this, _: &FecharBuscaNoArquivo, window, cx| this.fechar_busca_no_arquivo(window, cx)))
            .on_action(cx.listener(|this, _: &SalvarArquivo, window, cx| this.salvar_arquivo_agora(window, cx)))
            .on_action(cx.listener(|this, _: &AchadoSeguinte, _, cx| this.f3_no_arquivo(1, cx)))
            .on_action(cx.listener(|this, _: &AchadoAnterior, _, cx| this.f3_no_arquivo(-1, cx)))
            // Ancorado, é a última peça da fileira das conversas: encosta no topo e na direita da
            // janela e emenda na conversa da esquerda pela divisória (ver `aresta_encostada`).
            .when(na_gaveta, |s| s.rounded(crate::tema::raio_da_peca()))
            .when(!na_gaveta, |s| s.bg(Cores::cartao()))
            .overflow_hidden()
            .child(cabecalho)
            .child(barra)
            .children(faixa)
            .children(faixa_de_leitura)
            .children(faixa_do_conflito)
            .child(div().relative().flex_1().min_h_0().flex().flex_col().child(corpo).children(barra_da_busca))
            .when(!na_gaveta, |s| s.aresta_encostada(crate::tema::Esquerda::Emendada))
            .on_prepaint(medir)
            .into_any_element()
    }
}

/// A largura livre da faixa da edição (em px a 100%) a partir da qual "arquivo 2 de 8" cabe por
/// extenso ao lado de "Edição +N −M 2 de 5", das setas e do ×; abaixo dela, "2/8".
const FAIXA_COM_A_FRASE: f32 = 400.;

/// Quanto ocupam as ações do cabeçalho do arquivo (em px a 100%): só os ícones (Mencionar, VS
/// Code e o "⋯", de 28, com 4 entre eles) ou com os rótulos.
const ACOES_SO_ICONES: f32 = 3. * 28. + 2. * 4.;

/// Quanto ocupa o seletor Prévia | Fonte no cabeçalho (em px a 100%): com os nomes ou só os símbolos.
const SELETOR_COM_ROTULOS: f32 = 138.;
const SELETOR_SO_ICONES: f32 = 70.;
const ACOES_COM_ROTULOS: f32 = 116. + 96. + 28. + 2. * 4.;

/// O que cabe no cabeçalho do arquivo: os rótulos das ações, as linhas e a linguagem. A conta é
/// pela largura do painel e pelo tamanho dos textos (a 100%, em px), para nada passar por baixo
/// das ações nem dos anéis: o que não cabe sai inteiro, e o nome é o último a encolher (dono,
/// 16/11/2026: "59 linhas · JavaScr" ficava cortado e por baixo do "@ Mencionar").
fn o_que_cabe(livre: f32, nome: &str, info: Option<&str>) -> (bool, bool) {
    let largura_nome = nome.chars().count() as f32 * 7.4 + 4.;
    let largura_info = info.map_or(0., |i| i.chars().count() as f32 * 6.6 + 8.);
    let com_rotulos = livre - largura_nome - largura_info - 8. >= ACOES_COM_ROTULOS;
    let acoes = if com_rotulos { ACOES_COM_ROTULOS } else { ACOES_SO_ICONES };
    let com_info = info.is_some() && livre - acoes - largura_nome.min(140.) - 8. >= largura_info;
    (com_rotulos, com_info)
}

impl Janela {
    /// O cabeçalho do arquivo, embaixo da aba: o ícone e o nome, as linhas e a linguagem, a pasta
    /// (cortada no começo; o caminho inteiro na dica) e, à direita, Mencionar, VS Code e o "⋯",
    /// com rótulo só quando cabe. Com linhas escolhidas, no lugar das linhas e da linguagem, quais
    /// são, com o × que desfaz a escolha.
    #[allow(clippy::too_many_arguments)]
    fn cabecalho_do_arquivo(
        &self,
        completo: &Path,
        caminho: &str,
        pasta: String,
        linhas: Option<usize>,
        seletor: bool,
        reserva: Option<Pixels>,
        na_gaveta: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let janela = cx.entity().downgrade();
        let nome = caminho.rsplit('/').next().unwrap_or(caminho).to_string();
        let selecao = self.arquivo_selecao;
        // Com o editor aberto, no lugar das linhas e da linguagem: "editando", o ponto do que
        // falta gravar e o × que volta à leitura.
        let editando = self.editor_arquivo.as_ref().filter(|e| e.sessao.caminho() == completo);
        let info = match selecao {
            _ if editando.is_some() => Some("editando".to_string()),
            Some((a, b)) if b > a => Some(format!("linhas {a} a {b}")),
            Some((a, _)) => Some(format!("linha {a}")),
            None => linhas.map(|n| {
                let mut texto = contagem(n, "linha", "linhas");
                if let Some(linguagem) = nome_da_linguagem(caminho) {
                    texto.push_str(" · ");
                    texto.push_str(linguagem);
                }
                texto
            }),
        };
        // A largura livre para o nome, a informação e as ações (a 100%, em px): o painel menos o
        // respiro dos lados, o ícone e a folga dos anéis. Antes da primeira medida, a do estreito.
        let largura = if self.direita_largura > 0. { self.direita_largura * 15. } else { 330. };
        let escala = window.rem_size().as_f32() / 15.;
        let aneis = reserva.map_or(8., |r| r.as_f32() / escala);
        // O ponto e o × do editor ocupam 6 + 24 e os respiros.
        let livre = largura - 12. - aneis - 16. - 8. - if editando.is_some() { 42. } else { 0. };
        // O seletor Prévia | Fonte fica antes das ações, com rótulo quando elas também têm.
        let (com_rotulos, com_info) = match seletor {
            false => o_que_cabe(livre, &nome, info.as_deref()),
            true => match o_que_cabe(livre - SELETOR_COM_ROTULOS - 4., &nome, info.as_deref()) {
                (true, com_info) => (true, com_info),
                (false, _) => (false, o_que_cabe(livre - SELETOR_SO_ICONES - 4., &nome, info.as_deref()).1),
            },
        };
        let absoluto = SharedString::from(completo.display().to_string());
        let botao = |id: &'static str, icone: Lucide, rotulo: &'static str, dica: SharedString| {
            if com_rotulos {
                botao_fantasma(id, Some(icone.into()), rotulo, TamanhoBotao::Barra, window, cx).tooltip(dica)
            } else {
                botao_icone(id, icone, dica, TamanhoBotao::Barra, window, cx)
            }
        };
        let dica_mencao = SharedString::from(match self.alvo_da_mencao() {
            Some((a, b)) if self.arquivo_selecao.is_none() && self.arquivo_edicao.is_some() => {
                if b > a { format!("Mencionar esta mudança na conversa (linhas {a} a {b})") } else { format!("Mencionar esta mudança na conversa (linha {a})") }
            }
            Some((a, b)) if b > a => format!("Mencionar as linhas {a} a {b} na conversa"),
            Some((a, _)) => format!("Mencionar a linha {a} na conversa"),
            None => "Mencionar o arquivo na conversa".to_string(),
        });
        let editor = completo.to_path_buf();
        // Com uma linha em vista (a escolhida, a da mudança ou a do link), o VS Code abre nela.
        let linha_editor = linha_do_editor(self.arquivo_selecao, self.alvo_da_mencao(), self.arquivo_marcada.get());
        let dica_editor = SharedString::from(match linha_editor {
            Some(linha) => format!("Abrir no VS Code, na linha {linha}"),
            None => "Abrir no VS Code".to_string(),
        });
        let menu = {
            let (conteudo, relativo, inteiro, fora) =
                (completo.to_path_buf(), caminho.to_string(), completo.to_path_buf(), completo.to_path_buf());
            let onde = completo.parent().map(Path::to_path_buf).unwrap_or_else(|| completo.to_path_buf());
            let janela = janela.clone();
            // Com um trecho em vista (a seleção ou a mudança), o arquivo inteiro também se menciona.
            let com_trecho = self.alvo_da_mencao().is_some();
            // No diff de "Na pasta", a opção de deixar de fora o que só mudou de espaço.
            let com_texto = linhas.is_some();
            let sem_espaco = match self.arquivo_edicao.as_deref().map(|e| &e.origem) {
                Some(super::edicao::Origem::Git { sem_espaco }) => Some(*sem_espaco),
                _ => None,
            };
            botao_icone("menu-arquivo", Lucide::Ellipsis, "Mais opções do arquivo", TamanhoBotao::Barra, window, cx)
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                    let (conteudo, relativo, onde, inteiro, janela, fora) =
                        (conteudo.clone(), relativo.clone(), onde.clone(), inteiro.clone(), janela.clone(), fora.clone());
                    menu.when(com_trecho, |menu| {
                        let janela = janela.clone();
                        menu.item(PopupMenuItem::new("Mencionar o arquivo inteiro").icon(Lucide::AtSign).on_click(
                            move |_, window, cx| {
                                let _ = janela.update(cx, |this, cx| this.mencionar_arquivo(&inteiro, None, window, cx));
                            },
                        ))
                        .separator()
                    })
                    .item(PopupMenuItem::new("Copiar o conteúdo").icon(Lucide::Copy).on_click(
                        move |_, window, cx| match std::fs::read_to_string(&conteudo) {
                            Ok(texto) => {
                                cx.write_to_clipboard(ClipboardItem::new_string(texto));
                                window.push_notification(aviso(Tom::Pronto, "Conteúdo copiado."), cx);
                            }
                            Err(_) => window.push_notification(aviso(Tom::Erro, "Não deu para ler o arquivo como texto."), cx),
                        },
                    ))
                    .item(PopupMenuItem::new("Copiar o caminho").icon(Lucide::Copy).on_click(move |_, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(relativo.clone()));
                        window.push_notification(aviso(Tom::Pronto, "Caminho copiado."), cx);
                    }))
                    .when(com_texto, |menu| {
                        let janela = janela.clone();
                        menu.separator().item(PopupMenuItem::new("Buscar no arquivo").icon(Lucide::Search).on_click(move |_, window, cx| {
                            let _ = janela.update(cx, |this, cx| this.abrir_busca_no_arquivo(window, cx));
                        }))
                    })
                    .when_some(sem_espaco, |menu, ligado| {
                        let janela = janela.clone();
                        menu.separator().item(PopupMenuItem::new("Ignorar espaço em branco").checked(ligado).on_click(
                            move |_, window, cx| {
                                let _ = janela.update(cx, |this, cx| this.alternar_diff_sem_espaco(window, cx));
                            },
                        ))
                    })
                    .separator()
                    // O aplicativo padrão do sistema: o navegador para o .html, a planilha para o .csv.
                    .item(
                        PopupMenuItem::new("Abrir fora")
                            .icon(Lucide::ExternalLink)
                            .external()
                            .on_click(move |_, _, cx| crate::sistema::abrir_fora(&fora, cx)),
                    )
                    .item(
                        PopupMenuItem::new("Mostrar na pasta")
                            .icon(Lucide::FolderOpen)
                            .external()
                            .on_click(move |_, _, cx| crate::sistema::abrir_pasta(&onde, cx)),
                    )
                })
        };
        let desfazer = selecao.map(|_| {
            botao_icone("desfazer-escolha", Lucide::X, "Desfazer a escolha das linhas", TamanhoBotao::Linha, window, cx)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.arquivo_selecao = None;
                    this.arquivo_ancora = None;
                    cx.notify();
                }))
        });
        // O que falta gravar (âmbar; vermelho quando a gravação falhou) e a volta à leitura.
        let do_editor = editando.map(|e| {
            let (cor, estado) = match (&e.erro, e.por_salvar) {
                (Some(erro), _) => (Cores::vermelho(), format!("Não deu para salvar: {erro}")),
                (None, true) => (Cores::ambar(), "Ainda não salvo: grava sozinho quando você para de digitar, ou no Ctrl+S".to_string()),
                (None, false) => (gpui_kit::transparent_black().into(), "Salvo".to_string()),
            };
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(esp::E4)
                .child(
                    div()
                        .id("ponto-do-editor")
                        .flex_none()
                        .size(r(6.))
                        .rounded_full()
                        .bg(cor)
                        .tooltip(move |window, cx| Tooltip::new(estado.clone()).build(window, cx)),
                )
                .child(
                    botao_icone("sair-do-editor", Lucide::X, "Voltar à leitura (Esc)", TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|this, _, window, cx| this.sair_do_editor(window, cx))),
                )
        });
        let seletor = seletor.then(|| {
            let janela = janela.clone();
            // Estreito, só os símbolos (o nome vai na dica).
            let opcoes = if com_rotulos {
                [Segmento::new("Prévia"), Segmento::new("Fonte")]
            } else {
                [Segmento::new("Prévia").icone(Lucide::Eye), Segmento::new("Fonte").icone(Lucide::Braces)]
            };
            ControleSegmentado::new("previa-ou-fonte", opcoes, usize::from(self.arquivo_fonte), move |novo, window, cx| {
                let _ = janela.update(cx, |this, cx| this.escolher_vista_do_arquivo(novo == 1, window, cx));
            })
            .with_size(Size::Small)
            .so_icone(!com_rotulos)
        });
        // Editando, o lápis no lugar do símbolo do arquivo (o que resta à vista no painel estreito).
        let simbolo = match editando {
            Some(_) => div().flex_none().text_color(Cores::destaque_texto()).child(super::componentes::icone(Lucide::Pencil, 16., window)),
            None => div().flex_none().text_color(Cores::tenue()).child(super::componentes::icone(icone_do_arquivo(caminho), 16., window)),
        };
        let em_destaque = selecao.is_some() || editando.is_some();
        let j_mencionar = janela.clone();
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(altura::GRANDE)
            .pl(esp::E12)
            .map(|s| match reserva {
                Some(folga) => s.pr(folga),
                None => s.pr(esp::E8),
            })
            .gap(esp::E8)
            .border_b_1()
            // Na gaveta, opaca, a linha() some: lá vai a borda().
            .border_color(if na_gaveta { Cores::borda() } else { Cores::linha() })
            .child(
                div()
                    .id("caminho-arquivo")
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap(esp::E8)
                    .child(simbolo)
                    // O nome é cortado no meio: a extensão fica à vista.
                    .child(
                        div()
                            .flex_shrink(1.)
                            .min_w(r(24.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_middle()
                            .rotulo()
                            .text_color(Cores::forte())
                            .child(nome),
                    )
                    .when(com_info, |s| {
                        s.children(info.map(|i| {
                            div()
                                .flex_none()
                                .whitespace_nowrap()
                                .metadado()
                                .text_color(if em_destaque { Cores::destaque_texto() } else { Cores::apagado() })
                                .child(i)
                        }))
                    })
                    .children(desfazer)
                    .children(do_editor)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_start()
                            .mono_pequeno()
                            .text_color(Cores::tenue())
                            .child(pasta),
                    )
                    .tooltip(move |window, cx| Tooltip::new(absoluto.clone()).build(window, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(esp::E4)
                    .children(seletor)
                    .child(botao("mencionar-arquivo", Lucide::AtSign, "Mencionar", dica_mencao).on_click(move |_, window, cx| {
                        let _ = j_mencionar.update(cx, |this, cx| this.mencionar_do_painel(window, cx));
                    }))
                    .child(
                        botao("editor-arquivo", Lucide::CodeXml, "VS Code", dica_editor)
                            .on_click(move |_, _, _| crate::sistema::abrir_no_editor(&editor, linha_editor)),
                    )
                    .child(menu),
            )
            .into_any_element()
    }

    /// A faixa do conflito no editor: o agente gravou (ou apagou) o arquivo com texto seu por
    /// gravar. "Recarregar" fica com o do disco e "Manter o meu" com o seu, que passa por cima na
    /// gravação seguinte. No painel estreito os botões descem para a linha de baixo.
    fn faixa_do_conflito(
        &self,
        conflito: Conflito,
        nova: bool,
        reserva: Option<Pixels>,
        na_gaveta: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let (frase, dica_manter) = match conflito {
            Conflito::Mudou => ("O arquivo mudou no disco.", "Fica o que você escreveu: ao salvar, passa por cima do que está no disco"),
            Conflito::Apagado => ("O arquivo foi apagado. O seu texto continua aqui.", "Fica o que você escreveu: ao salvar, o arquivo é criado de novo"),
        };
        let recarregar = (conflito == Conflito::Mudou).then(|| {
            botao_fantasma("recarregar-arquivo", None, "Recarregar", TamanhoBotao::Linha, window, cx)
                .tooltip("Fica o que está no disco: o que você escreveu se perde")
                .on_click(cx.listener(|this, _, window, cx| this.recarregar_editor(window, cx)))
        });
        let manter = botao_fantasma("manter-o-meu", None, "Manter o meu", TamanhoBotao::Linha, window, cx)
            .tooltip(dica_manter)
            .on_click(cx.listener(|this, _, window, cx| this.manter_o_meu_no_editor(window, cx)));
        let faixa = div()
            .id("conflito-do-arquivo")
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .min_h(altura::CONTROLE)
            .py(esp::E4)
            .pl(esp::E12)
            .map(|s| match reserva {
                Some(folga) => s.pr(folga),
                None => s.pr(esp::E8),
            })
            .gap_x(esp::E8)
            .gap_y(esp::E2)
            .border_b_1()
            .border_color(if na_gaveta { Cores::borda() } else { Cores::linha() })
            .bg(Cores::tinta_leve(Cores::ambar()))
            .child(div().flex_none().text_color(Cores::ambar()).child(super::componentes::icone(Lucide::TriangleAlert, 14., window)))
            .child(div().flex_1().min_w(r(132.)).metadado().text_color(Cores::texto()).child(frase))
            .child(div().flex().flex_none().items_center().gap(esp::E4).children(recarregar).child(manter));
        movimento::entrada("entra-conflito-do-arquivo", nova, De::Cima, faixa).into_any_element()
    }

    /// Onde o código está rolado (a leitura em linhas), para o editor abrir no mesmo lugar.
    pub(super) fn rolagem_do_codigo(&self) -> Option<Point<Pixels>> {
        let lido = self.arquivo_lido.borrow();
        let lido = lido.as_ref()?;
        matches!(lido.conteudo, Conteudo::Codigo(..)).then(|| lido.rolagem.0.borrow().base_handle.offset())
    }

    /// O editor gravou por cima do arquivo: a próxima leitura vai ao disco, e a lista volta com a
    /// linha `primeira` (a partir de 0) no alto.
    pub(super) fn reler_arquivo_aberto(&self, primeira: Option<usize>) {
        if let Some(lido) = self.arquivo_lido.borrow_mut().as_mut() {
            lido.velho = true;
            if let Some(linha) = primeira {
                lido.rolagem.scroll_to_item(linha, ScrollStrategy::Top);
            }
        }
    }

    /// Um clique no texto da linha (a partir de 0), em `x` na janela: abre o editor com o cursor ali.
    fn clicou_no_texto(&mut self, linha: usize, x: Pixels, window: &mut Window, cx: &mut Context<Self>) {
        let (total, rolado) = {
            let lido = self.arquivo_lido.borrow();
            let Some(lido) = lido.as_ref() else { return };
            let Conteudo::Codigo(linhas, _) = &lido.conteudo else { return };
            (linhas.len(), lido.rolagem.0.borrow().base_handle.offset().x)
        };
        // Onde o texto começa: a margem da fileira, o número e o respiro (na edição do agente,
        // também a coluna do sinal), menos o que o código rolou para o lado.
        let com_sinal = self.arquivo_edicao.as_ref().is_some_and(|e| !e.sem_registro);
        let antes = 12. + largura_do_numero(total) + if com_sinal { 24. } else { 16. };
        let escala = window.rem_size().as_f32() / 15.;
        let comeco = self.codigo_esquerda.get() + px(antes * escala) + rolado;
        let coluna = ((x - comeco).as_f32() / (LETRA_DO_CODIGO * escala)).round().max(0.) as usize;
        self.entrar_no_editor(linha, coluna, window, cx);
    }

    /// O seletor Prévia | Fonte do .md e do .svg. A Prévia fecha o editor (gravando o que falta).
    fn escolher_vista_do_arquivo(&mut self, fonte: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !fonte {
            self.largar_editor(window, cx);
        }
        self.arquivo_fonte = fonte;
        cx.notify();
    }

    /// A faixa da edição, embaixo do cabeçalho: "Edição", o +N −M e "2 de 5", com as setas que
    /// passam de uma mudança à outra e o × que volta ao arquivo sem a edição. Aberta pelas
    /// Mudanças de um chat com mais de um arquivo, diz também "arquivo 2 de 8", e a seta da última
    /// mudança passa ao arquivo seguinte da lista. No registro de antes de guardar o diff, um
    /// aviso discreto no lugar.
    fn faixa_da_edicao(
        &self,
        edicao: &Edicao,
        vista: Option<&Vista>,
        reserva: Option<Pixels>,
        na_gaveta: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let mudancas = vista.map_or(0, |v| v.mudancas.len());
        let atual = self.edicao_atual.min(mudancas.saturating_sub(1));
        let lugar = self.lugar_do_arquivo_no_turno(edicao, cx).filter(|(_, total)| *total > 1);
        let fechar = botao_icone("fechar-edicao", Lucide::X, "Ver o arquivo sem a edição", TamanhoBotao::Linha, window, cx)
            .on_click(cx.listener(|this, _, _, cx| this.sair_da_edicao(cx)));
        // A dica diz para onde a seta vai: outra mudança ou outro arquivo.
        let seta = |id: &'static str, icone: Lucide, passo: isize| {
            // Com uma mudança só e sem outro arquivo, a seta não levaria a lugar nenhum.
            let destino = passo_da_revisao(atual, mudancas, lugar, passo).filter(|d| *d != Passo::Mudanca(atual))?;
            let dica = match (destino, passo > 0) {
                (Passo::Mudanca(_), true) => "Próxima mudança",
                (Passo::Mudanca(_), false) => "Mudança anterior",
                (Passo::Arquivo { .. }, true) => "Próximo arquivo das Mudanças",
                (Passo::Arquivo { .. }, false) => "Arquivo anterior das Mudanças",
            };
            Some(
                botao_icone(id, icone, dica, TamanhoBotao::Linha, window, cx)
                    .on_click(cx.listener(move |this, _, window, cx| this.ir_para_mudanca(passo, window, cx))),
            )
        };
        let (anterior, seguinte) = (seta("mudanca-anterior", Lucide::ChevronUp, -1), seta("mudanca-seguinte", Lucide::ChevronDown, 1));
        let setas = (anterior.is_some() || seguinte.is_some())
            .then(|| div().flex().flex_none().items_center().gap(esp::E2).children(anterior).children(seguinte));
        // Quando a faixa não tem lugar para as frases (o painel na largura mínima, os anéis de
        // uso no canto), elas encolhem: "2/8" e um ícone, com a frase na dica.
        let escala = window.rem_size().as_f32() / 15.;
        let livre = self.direita_largura * 15. - reserva.map_or(8., |r| r.as_f32() / escala);
        let curto = self.direita_estreita || livre < FAIXA_COM_A_FRASE;
        // "arquivo 2 de 8".
        let do_turno = lugar.map(|(posicao, total)| {
            let longo = SharedString::from(format!("arquivo {} de {}", posicao + 1, total));
            let texto = if curto { SharedString::from(format!("{}/{}", posicao + 1, total)) } else { longo.clone() };
            div()
                .id("arquivo-do-turno")
                .flex()
                .flex_none()
                .items_center()
                .gap(esp::E4)
                .whitespace_nowrap()
                .metadado()
                .text_color(Cores::tenue())
                .child(super::componentes::icone(Lucide::Files, 12., window))
                .child(texto)
                .tooltip(move |window, cx| Tooltip::new(format!("Nas Mudanças deste chat: {longo}")).build(window, cx))
        });
        let so_espaco = matches!(edicao.origem, super::edicao::Origem::Git { sem_espaco: true });
        let esquerda = if edicao.sem_registro {
            // O diff sem espaço em branco que ficou vazio não é um registro antigo: só mudou espaço.
            let (simbolo, texto) = if so_espaco {
                (Lucide::FileDiff, "Só mudou espaço em branco.")
            } else {
                (Lucide::Info, "Este registro não guardou a edição: o arquivo aparece como está agora.")
            };
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(esp::E8)
                .child(div().flex_none().text_color(Cores::tenue()).child(super::componentes::icone(simbolo, 14., window)))
                .child(div().flex_1().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(texto))
                .children(do_turno)
        } else {
            let posicao = (mudancas > 0).then(|| format!("{} de {}", atual + 1, mudancas));
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .items_center()
                .gap(esp::E8)
                .child(div().flex_none().text_color(Cores::tenue()).child(super::componentes::icone(Lucide::FileDiff, 14., window)))
                .child(div().flex_none().rotulo().text_color(Cores::texto()).child("Edição"))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .gap(esp::E6)
                        .font_family(FONTE_MONO)
                        .metadado()
                        .when(edicao.entram > 0, |s| s.child(div().text_color(Cores::verde()).child(format!("+{}", edicao.entram))))
                        .when(edicao.saem > 0, |s| s.child(div().text_color(Cores::vermelho()).child(format!("−{}", edicao.saem)))),
                )
                .children(posicao.map(|p| div().flex_none().whitespace_nowrap().metadado().text_color(Cores::tenue()).child(p)))
                // O diff está filtrado: a frase, ou o funil com a frase na dica.
                .when(so_espaco, |s| {
                    let marca = div().id("diff-sem-espaco").flex_none().whitespace_nowrap().metadado().text_color(Cores::tenue());
                    s.child(if curto {
                        marca
                            .child(super::componentes::icone(Lucide::Funnel, 12., window))
                            .tooltip(|window, cx| Tooltip::new("Sem as mudanças só de espaço em branco").build(window, cx))
                    } else {
                        marca.child("sem espaço em branco")
                    })
                })
                .child(div().flex_1())
                .children(do_turno)
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(altura::CONTROLE)
            .pl(esp::E12)
            .map(|s| match reserva {
                Some(folga) => s.pr(folga),
                None => s.pr(esp::E8),
            })
            .gap(esp::E8)
            .border_b_1()
            .border_color(if na_gaveta { Cores::borda() } else { Cores::linha() })
            .child(esquerda)
            .children(setas)
            .child(fechar)
            .into_any_element()
    }

    /// Onde o arquivo da edição aberta está na lista de Mudanças do chat dela: a posição e quantos
    /// arquivos a lista tem. A conta (a lista inteira de mudanças do chat) fica guardada pela
    /// edição e pelo tamanho da conversa: o painel é desenhado a cada quadro.
    fn lugar_do_arquivo_no_turno(&self, edicao: &Edicao, cx: &App) -> Option<(usize, usize)> {
        let super::edicao::Origem::Chat { chat, itens, caminho } = &edicao.origem else {
            return None;
        };
        let chat = self.motor.as_ref()?.read(cx).chat(chat)?;
        let chave = (edicao as *const Edicao as usize, chat.itens.len());
        if let Some((de, tamanho, lugar)) = self.lugar_no_turno.get()
            && (de, tamanho) == chave
        {
            return lugar;
        }
        let (neste, antes) = super::mensagens::mudancas_do_chat(&chat.itens);
        let lugar = super::edicao::lugar_na_revisao(&neste, &antes, itens, caminho.as_deref()).map(|(lista, posicao)| (posicao, lista.len()));
        self.lugar_no_turno.set(Some((chave.0, chave.1, lugar)));
        lugar
    }

    /// A edição posta nas linhas do arquivo lido, refeita só quando o arquivo ou a edição mudam.
    fn vista_da_edicao(&self, edicao: &Rc<Edicao>, linhas: &Rc<Vec<SharedString>>) -> (Rc<Vista>, usize) {
        let mut lido = self.arquivo_lido.borrow_mut();
        let Some(l) = lido.as_mut() else {
            return (Rc::new(super::edicao::vista(&edicao.trechos, linhas.as_slice())), 0);
        };
        if let Some((chave, vista, mais_longa)) = &l.vista
            && *chave == Rc::as_ptr(edicao) as usize
        {
            return (vista.clone(), *mais_longa);
        }
        let vista = Rc::new(super::edicao::vista(&edicao.trechos, linhas.as_slice()));
        let largura = |f: &Fileira| match f {
            Fileira::Arquivo { linha, .. } => linhas.get(*linha).map_or(0, |t| t.chars().count()),
            Fileira::Saiu(t) | Fileira::Solta(_, t) | Fileira::Nota(t) => t.chars().count(),
        };
        let mais_longa = (0..vista.fileiras.len()).max_by_key(|&i| largura(&vista.fileiras[i])).unwrap_or(0);
        l.vista = Some((Rc::as_ptr(edicao) as usize, vista.clone(), mais_longa));
        (vista, mais_longa)
    }

    /// As linhas que o Mencionar leva: as escolhidas ou, na edição, as da mudança à vista.
    fn alvo_da_mencao(&self) -> Option<(u32, u32)> {
        self.arquivo_selecao.or_else(|| {
            self.arquivo_edicao.as_ref()?;
            let lido = self.arquivo_lido.borrow();
            let (_, vista, _) = lido.as_ref()?.vista.as_ref()?;
            let mudanca = vista.mudancas.get(self.edicao_atual.min(vista.mudancas.len().saturating_sub(1)))?;
            mudanca.linhas.map(|(a, b)| (a as u32, b as u32))
        })
    }

    /// Mencionar, no cabeçalho do arquivo: a seleção, a mudança à vista ou o arquivo inteiro.
    fn mencionar_do_painel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((raiz, caminho)) = self.arquivo_aberto.clone() else { return };
        let linhas = self.alvo_da_mencao();
        self.mencionar_arquivo(&raiz.join(caminho), linhas, window, cx);
    }

    /// Um clique no número da linha: escolhe a linha (o segundo clique nela desfaz); com Shift,
    /// estende a escolha desde a primeira.
    pub(super) fn escolher_linha(&mut self, linha: u32, estender: bool, cx: &mut Context<Self>) {
        match (estender, self.arquivo_ancora) {
            (true, Some(ancora)) => self.arquivo_selecao = Some((ancora.min(linha), ancora.max(linha))),
            _ if self.arquivo_selecao == Some((linha, linha)) => {
                self.arquivo_selecao = None;
                self.arquivo_ancora = None;
            }
            _ => {
                self.arquivo_selecao = Some((linha, linha));
                self.arquivo_ancora = Some(linha);
            }
        }
        self.arquivo_marcada.set(None);
        cx.notify();
    }

    /// As setas da edição: a mudança anterior ou a seguinte, rolando até ela. Na última (ou na
    /// primeira), passa ao arquivo seguinte (ou anterior) da lista de Mudanças do chat, na ordem
    /// dela e dando a volta; fora de uma lista, dá a volta dentro do arquivo.
    fn ir_para_mudanca(&mut self, passo: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edicao) = self.arquivo_edicao.clone() else { return };
        let total = self.arquivo_lido.borrow().as_ref().and_then(|l| l.vista.as_ref()).map_or(0, |(_, v, _)| v.mudancas.len());
        let lugar = self.lugar_do_arquivo_no_turno(&edicao, cx);
        match passo_da_revisao(self.edicao_atual, total, lugar, passo) {
            Some(Passo::Mudanca(i)) => {
                self.edicao_atual = i;
                self.edicao_rolar.set(true);
                cx.notify();
            }
            Some(Passo::Arquivo { indice, no_fim }) => {
                let super::edicao::Origem::Chat { chat, itens, caminho } = &edicao.origem else { return };
                // A lista de agora: o turno pode ter mudado mais arquivos desde que este abriu.
                let alvo = self.motor.as_ref().and_then(|m| m.read(cx).chat(chat)).and_then(|c| {
                    let (neste, antes) = super::mensagens::mudancas_do_chat(&c.itens);
                    let (lista, _) = super::edicao::lugar_na_revisao(&neste, &antes, itens, caminho.as_deref())?;
                    lista.get(indice).map(|a| a.itens.clone())
                });
                let Some(itens) = alvo else { return };
                self.abrir_edicao(&super::edicao::AbrirEdicao { chat: chat.clone(), itens, caminho: None }, window, cx);
                // Voltando, o arquivo anterior abre na última mudança dele (a posição se prende
                // ao tamanho quando a vista dele ficar pronta).
                if no_fim {
                    self.edicao_atual = usize::MAX;
                }
            }
            None => {}
        }
    }

    /// "Ignorar espaço em branco", no "…" do arquivo aberto com o diff de "Na pasta": liga ou
    /// desliga e refaz o diff. Vale até fechar o Vitral.
    fn alternar_diff_sem_espaco(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.diff_sem_espaco = !self.diff_sem_espaco;
        if let Some((raiz, caminho)) = self.arquivo_aberto.clone() {
            self.abrir_diff_do_git(raiz, caminho, false, window, cx);
        }
    }

    /// Ctrl+F com o foco no arquivo (ou "Buscar no arquivo", no "…" e na paleta): abre a barra da
    /// busca, ou volta o foco a ela, com o termo de antes selecionado. A imagem não tem o que buscar.
    pub(super) fn abrir_busca_no_arquivo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((raiz, caminho)) = self.arquivo_aberto.clone() else {
            window.push_notification(aviso(Tom::Info, "Abra um arquivo à direita para buscar nele."), cx);
            return;
        };
        if crate::claude::tipo_imagem(&raiz.join(&caminho)).is_some() {
            return;
        }
        // A busca é pelas linhas da leitura: o editor grava o que falta e sai.
        self.largar_editor(window, cx);
        // Pela paleta, a direita pode estar fechada ou na outra aba.
        if !matches!(self.aba_direita, super::abas_direita::AbaDireita::Arquivo(_)) || !(self.direita_visivel() || self.gaveta == Some(Gaveta::Direita)) {
            self.aba_direita = super::abas_direita::AbaDireita::Arquivo(0);
            self.mostrar_direita(window, cx);
        }
        self.busca_arquivo.aberta = true;
        self.busca_arquivo.rolar.set(true);
        let campo = self.busca_arquivo.campo.clone();
        campo.update(cx, |campo, cx| campo.select_all(window, cx));
        window.focus(&campo.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// Esc (ou o ×): fecha a barra e devolve o foco ao painel do arquivo; o termo fica guardado, e
    /// o markdown volta a aparecer renderizado.
    fn fechar_busca_no_arquivo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.busca_arquivo.aberta {
            // Sem barra aberta, o Esc é de quem vem depois (recolher a gaveta, parar o turno).
            cx.propagate();
            return;
        }
        self.busca_arquivo.aberta = false;
        window.focus(&self.foco_arquivo, cx);
        cx.notify();
    }

    /// Enter e as setas da barra (e o F3): o achado seguinte ou o anterior, dando a volta.
    pub(super) fn andar_na_busca_do_arquivo(&mut self, passo: i32, cx: &mut Context<Self>) {
        {
            let mut indice = self.busca_arquivo.indice.borrow_mut();
            if indice.achados.is_empty() {
                return;
            }
            indice.atual = super::busca_conversa::andar(indice.achados.len(), indice.atual, passo);
        }
        self.busca_arquivo.rolar.set(true);
        cx.notify();
    }

    /// F3 e Shift+F3 com o foco no arquivo: andam na busca dele quando ela está aberta; fechada,
    /// são da busca da conversa.
    fn f3_no_arquivo(&mut self, passo: i32, cx: &mut Context<Self>) {
        if self.busca_arquivo.aberta {
            self.andar_na_busca_do_arquivo(passo, cx);
        } else {
            cx.propagate();
        }
    }

    /// O foco está no painel do arquivo, no campo da busca dele ou no editor.
    pub(super) fn foco_no_arquivo(&self, window: &Window, cx: &App) -> bool {
        self.foco_arquivo.is_focused(window)
            || self.busca_arquivo.campo.read(cx).focus_handle(cx).is_focused(window)
            || self.editor_arquivo.as_ref().is_some_and(|e| e.estado.read(cx).focus_handle(cx).contains_focused(window, cx))
    }

    /// O arquivo está aberto na gaveta da direita, por cima das conversas: aí o Ctrl+F é dele.
    pub(super) fn arquivo_na_gaveta(&self, cx: &App) -> bool {
        self.gaveta == Some(Gaveta::Direita)
            && self.arquivo_aberto.is_some()
            && self.motor.as_ref().is_some_and(|m| matches!(self.aba_visivel(m.read(cx)), super::abas_direita::AbaDireita::Arquivo(_)))
    }

    /// A barra da busca no arquivo: o campo, "3 de 12" ("3/12" no painel estreito), as setas e o
    /// fechar; com ela, se acabou de abrir (entra uma vez). `contagem` é o achado atual e quantos são.
    fn barra_da_busca_no_arquivo(&self, contagem: Option<(usize, usize)>, window: &Window, cx: &Context<Self>) -> Option<(Stateful<Div>, bool)> {
        let busca = &self.busca_arquivo;
        if !busca.aberta {
            busca.vista.set(false);
            return None;
        }
        let nova = !busca.vista.replace(true);
        let total = contagem.map_or(0, |(_, total)| total);
        let estreita = self.direita_estreita;
        let com_termo = !busca.campo.read(cx).value().trim().is_empty();
        let contador = com_termo.then(|| match contagem {
            Some((atual, total)) if total > 0 && estreita => format!("{}/{}", atual + 1, total),
            Some((atual, total)) if total > 0 => format!("{} de {}", atual + 1, total),
            _ if estreita => "0".to_string(),
            _ => "nenhum achado".to_string(),
        });
        let seta = |id: &'static str, icone: Lucide, dica: &'static str, acao: &dyn Action, passo: i32| {
            botao_icone(id, icone, dica, TamanhoBotao::Barra, window, cx)
                .tooltip_with_action(dica, acao, None)
                .disabled(total == 0)
                .on_click(cx.listener(move |this, _, _, cx| this.andar_na_busca_do_arquivo(passo, cx)))
        };
        let barra = h_flex()
            .id("busca-no-arquivo")
            .relative()
            .w_full()
            .p(esp::E8)
            .gap(esp::E4)
            .rounded(raio::PECA)
            .flutuante(cx)
            .child(
                div()
                    .flex_1()
                    .min_w(r(96.))
                    .on_action(cx.listener(|this, _: &Escape, window, cx| this.fechar_busca_no_arquivo(window, cx)))
                    .child(campo_de_busca("campo-busca-no-arquivo", &busca.campo, None, window, cx).bg(Cores::recuo_no_flutuante())),
            )
            .children(contador.map(|texto| {
                div()
                    .flex_none()
                    .px(esp::E4)
                    .whitespace_nowrap()
                    .metadado()
                    .text_color(if total == 0 { Cores::ambar() } else { Cores::tenue() })
                    .child(texto)
            }))
            .child(seta("achado-anterior-no-arquivo", Lucide::ChevronUp, "Achado anterior", &AchadoAnterior, -1))
            .child(seta("achado-seguinte-no-arquivo", Lucide::ChevronDown, "Próximo achado", &AchadoSeguinte, 1))
            .child(
                botao_icone("fechar-busca-no-arquivo", Lucide::X, "Fechar a busca (Esc)", TamanhoBotao::Barra, window, cx)
                    .on_click(cx.listener(|this, _, window, cx| this.fechar_busca_no_arquivo(window, cx))),
            );
        Some((barra, nova))
    }

    /// O × da faixa da edição: o arquivo fica aberto, sem as marcas.
    fn sair_da_edicao(&mut self, cx: &mut Context<Self>) {
        self.arquivo_edicao = None;
        self.esquecer_vista();
        cx.notify();
    }

    /// Larga a edição posta nas linhas: a próxima é montada de novo (a chave é o endereço da
    /// edição, que pode voltar a ser usado depois que ela some).
    pub(super) fn esquecer_vista(&self) {
        if let Some(l) = self.arquivo_lido.borrow_mut().as_mut() {
            l.vista = None;
        }
    }
}

/// O aviso no lugar do conteúdo, centralizado, com o que dá para fazer: o binário (PDF, planilha)
/// abre fora, no aplicativo do sistema; o texto grande demais, no VS Code ou fora.
fn aviso_do_arquivo(
    aviso: Aviso,
    janela: WeakEntity<Janela>,
    completo: PathBuf,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let botoes: Vec<AnyElement> = saidas_do_aviso(&aviso)
        .iter()
        .map(|saida| match saida {
            Saida::Fora => {
                let alvo = completo.clone();
                botao_fantasma("aviso-fora", Some(Lucide::ExternalLink.into()), "Abrir fora", TamanhoBotao::Barra, window, cx)
                    .tooltip("Abrir no aplicativo padrão do sistema")
                    .on_click(move |_, _, cx| crate::sistema::abrir_fora(&alvo, cx))
                    .into_any_element()
            }
            Saida::Editor => {
                let alvo = completo.clone();
                botao_fantasma("aviso-editor", Some(Lucide::CodeXml.into()), "Abrir no VS Code", TamanhoBotao::Barra, window, cx)
                    .on_click(move |_, _, _| crate::sistema::abrir_no_editor(&alvo, None))
                    .into_any_element()
            }
            Saida::Fechar => {
                let janela = janela.clone();
                botao_fantasma("aviso-fechar", None, "Fechar", TamanhoBotao::Barra, window, cx)
                    .on_click(move |_, window, cx| {
                        let _ = janela.update(cx, |this, cx| this.fechar_arquivo(window, cx));
                    })
                    .into_any_element()
            }
            Saida::Escrever => {
                let janela = janela.clone();
                botao_fantasma("aviso-escrever", Some(Lucide::Pencil.into()), "Escrever nele", TamanhoBotao::Barra, window, cx)
                    .on_click(move |_, window, cx| {
                        let _ = janela.update(cx, |this, cx| this.entrar_no_editor(0, 0, window, cx));
                    })
                    .into_any_element()
            }
        })
        .collect();
    let (icone, titulo, texto): (Lucide, &str, Option<String>) = match aviso {
        Aviso::Grande(bytes) => (
            Lucide::File,
            "Arquivo grande demais",
            Some(format!("Tem {}; aqui abrem arquivos de até {}.", tamanho(bytes), tamanho(TETO_DE_LEITURA))),
        ),
        Aviso::Binario => (
            Lucide::Binary,
            titulo_do_binario(&completo),
            Some("Não dá para mostrar aqui. Ele abre no aplicativo do sistema.".into()),
        ),
        Aviso::NaoExiste => (Lucide::FileX, "O arquivo não existe mais", Some("Ele pode ter sido apagado ou renomeado.".into())),
        Aviso::Erro(erro) => (Lucide::FileX, "Não deu para ler o arquivo", Some(erro)),
        Aviso::Vazio => (Lucide::File, "Arquivo vazio", None),
    };
    let acao = (!botoes.is_empty()).then(|| div().flex().flex_wrap().justify_center().gap(esp::E4).children(botoes));
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .px(esp::E16)
        .child(
            EstadoVazio::new(icone, titulo.to_string())
                .when_some(texto, |e, t| e.texto(t))
                .when_some(acao, |e, a| e.acao(a)),
        )
        .into_any_element()
}

/// A faixa de um aviso discreto, embaixo do cabeçalho ("Só leitura: arquivo grande (1,2 MB)").
fn faixa_de_nota(texto: String, reserva: Option<Pixels>, na_gaveta: bool, window: &Window) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(altura::BARRA)
        .pl(esp::E12)
        .map(|s| match reserva {
            Some(folga) => s.pr(folga),
            None => s.pr(esp::E8),
        })
        .gap(esp::E8)
        .border_b_1()
        .border_color(if na_gaveta { Cores::borda() } else { Cores::linha() })
        .child(div().flex_none().text_color(Cores::tenue()).child(super::componentes::icone(Lucide::Info, 14., window)))
        .child(div().flex_1().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(texto))
        .into_any_element()
}

/// O frontmatter de um .md, no alto da prévia: uma linha por chave, com a chave em mono tênue e o
/// valor ao lado. Sem frontmatter, nada.
fn tabela_do_frontmatter(linhas: &[(SharedString, SharedString)]) -> Option<Div> {
    if linhas.is_empty() {
        return None;
    }
    // A coluna das chaves tem a largura da maior delas (o mono de 12 anda 7,2 por letra), até 150.
    let largura = (linhas.iter().map(|(chave, _)| chave.chars().count()).max().unwrap_or(0) as f32 * 7.2 + 2.).min(150.);
    Some(
        v_flex().mb(esp::E16).rounded(raio::CONTROLE).border_1().border_color(Cores::linha()).overflow_hidden().children(
            linhas.iter().enumerate().map(|(i, (chave, valor))| {
                div()
                    .flex()
                    .items_start()
                    .gap(esp::E12)
                    .px(esp::E12)
                    .py(esp::E6)
                    .when(i > 0, |s| s.border_t_1().border_color(Cores::linha()))
                    .child(div().flex_none().w(r(largura)).truncate().mono_pequeno().line_height(r(18.)).text_color(Cores::tenue()).child(chave.clone()))
                    .child(div().flex_1().min_w_0().text_size(r(13.)).line_height(r(18.)).text_color(Cores::texto()).child(valor.clone()))
            }),
        ),
    )
}

/// Calcula as cores do código em segundo plano (o syntect leva centenas de milissegundos num
/// arquivo grande) e guarda no arquivo lido, se ele ainda for o mesmo.
fn colorir(
    completo: &Path,
    modificado: Option<SystemTime>,
    linhas: &Rc<Vec<SharedString>>,
    cx: &Context<Janela>,
) -> Task<()> {
    let linguagem = linguagem(completo);
    let tema = cx.theme().highlight_theme.clone();
    let textos: Vec<SharedString> = linhas.to_vec();
    let caminho = completo.to_path_buf();
    cx.spawn(async move |janela, cx| {
        let cores = cx
            .background_spawn(async move {
                let texto = textos.iter().map(|t| t.as_ref()).collect::<Vec<&str>>().join("\n");
                let estilos = highlight_code(&linguagem, &texto, tema.as_ref());
                cores_por_linha(&textos, estilos)
            })
            .await;
        let _ = janela.update(cx, |janela, cx| {
            if let Some(l) = janela.arquivo_lido.borrow_mut().as_mut()
                && l.caminho == caminho
                && l.modificado == modificado
            {
                l.cores = Some(Rc::new(cores));
                cx.notify();
            }
        });
    })
}

#[cfg(test)]
mod testes {
    use super::{
        Aviso, Conteudo, LINHA_MAXIMA, Passo, Saida, TETO_DE_LEITURA, achados_na_linha, achados_nas_linhas, cores_por_linha, icone_do_arquivo,
        ler_arquivo, linha_do_editor, nome_da_linguagem, nota_de_so_leitura, passo_da_revisao, saidas_do_aviso, tamanho,
        titulo_do_binario,
    };
    use crate::tema::Lucide;
    use gpui_kit::{HighlightStyle, SharedString};

    #[test]
    fn cores_se_repartem_pelas_linhas() {
        let linhas: Vec<SharedString> = vec!["fn a() {".into(), "  /* x".into(), "y */ }".into()];
        let estilo = HighlightStyle::default();
        // "fn" (0..2), o comentário de "/*" até "*/" (11..20) e a chave final (21..22).
        let cores = cores_por_linha(&linhas, vec![(0..2, estilo), (11..20, estilo), (21..22, estilo)]);
        let faixas: Vec<Vec<std::ops::Range<usize>>> =
            cores.iter().map(|l| l.iter().map(|(f, _)| f.clone()).collect()).collect();
        assert_eq!(faixas, vec![vec![0..2], vec![2..6], vec![0..4, 5..6]]);
    }

    #[test]
    fn tamanho_em_mb_e_kb() {
        assert_eq!(tamanho(1_200_000), "1,2 MB");
        assert_eq!(tamanho(1_048_576), "1,0 MB");
        assert_eq!(tamanho(400_000), "400 KB");
        assert_eq!(tamanho(450_499), "450 KB");
    }

    #[test]
    fn linguagem_e_icone_pela_extensao() {
        assert_eq!(nome_da_linguagem("src/ui/mod.rs"), Some("Rust"));
        assert_eq!(nome_da_linguagem("web/App.TSX"), Some("TypeScript"));
        assert_eq!(nome_da_linguagem("Makefile"), Some("Makefile"));
        assert_eq!(nome_da_linguagem("Cargo.lock"), None);
        assert_eq!(icone_do_arquivo("LEIA-ME.md"), Lucide::FileText);
        assert_eq!(icone_do_arquivo("src/main.rs"), Lucide::FileCode);
        assert_eq!(icone_do_arquivo("logo.png"), Lucide::FileImage);
        assert_eq!(icone_do_arquivo("Cargo.lock"), Lucide::File);
    }

    #[test]
    fn avisos_de_leitura() {
        let pasta = std::env::temp_dir().join(format!("vitral-teste-arquivo-{}", std::process::id()));
        std::fs::create_dir_all(&pasta).unwrap();
        let aviso = |nome: &str, bytes: &[u8]| {
            let caminho = pasta.join(nome);
            std::fs::write(&caminho, bytes).unwrap();
            match ler_arquivo(&caminho, false) {
                Conteudo::Aviso(a) => Some(a),
                _ => None,
            }
        };
        assert_eq!(aviso("vazio.txt", b""), Some(Aviso::Vazio));
        assert_eq!(aviso("binario.bin", b"a\0b"), Some(Aviso::Binario));
        // Até 23/11 o teto era de 400 KB; o item "arquivo grande" o levou a `TETO_DE_LEITURA`.
        assert_eq!(aviso("grande.txt", &vec![b'a'; TETO_DE_LEITURA as usize + 1]), Some(Aviso::Grande(TETO_DE_LEITURA + 1)));
        assert_eq!(aviso("ok.rs", b"fn main() {}\n"), None);
        assert!(matches!(ler_arquivo(&pasta.join("sumiu.rs"), false), Conteudo::Aviso(Aviso::NaoExiste)));
        assert!(matches!(ler_arquivo(&pasta, false), Conteudo::Aviso(Aviso::Erro(_))));
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn arquivo_grande_abre_so_para_ler_ate_o_teto() {
        let pasta = std::env::temp_dir().join(format!("vitral-teste-grande-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        let linhas_de = |nome: &str, texto: &str, rico: bool| {
            std::fs::write(pasta.join(nome), texto).unwrap();
            match ler_arquivo(&pasta.join(nome), rico) {
                Conteudo::Codigo(linhas, _) => Some(linhas),
                _ => None,
            }
        };
        // Um log de 540 KB: antes era "grande demais"; agora abre, em linhas.
        let log = "2026-11-23 12:00:00 pedido 123456 separado no galpao\n".repeat(10_000);
        assert_eq!(linhas_de("saida.log", &log, true).map(|l| l.len()), Some(10_000));
        // 60 mil linhas curtas também.
        assert_eq!(linhas_de("ids.csv", &"1234567890\n".repeat(60_000), true).map(|l| l.len()), Some(60_000));
        // O .md grande abre em linhas: a prévia de um texto desse tamanho travaria a janela.
        assert_eq!(linhas_de("historico.md", &"- uma linha do historico de pedidos\n".repeat(15_000), true).map(|l| l.len()), Some(15_000));
        // A linha comprida demais (um JSON minificado) é cortada no desenho.
        let minificado = format!("{{\"a\":\"{}\"}}\nfim\n", "x".repeat(500_000));
        let linhas = linhas_de("pacote.min.json", &minificado, true).unwrap();
        assert_eq!(linhas.len(), 2);
        assert!(linhas[0].chars().count() <= LINHA_MAXIMA + 2, "a linha ficou com {} letras", linhas[0].chars().count());
        assert!(linhas[0].ends_with('…'));
        assert_eq!(linhas[1].as_ref(), "fim");
        // O aviso discreto diz por que não dá para editar; o arquivo comum não tem aviso.
        assert_eq!(nota_de_so_leitura(540_000).as_deref(), Some("Só leitura: arquivo grande (540 KB)"));
        assert_eq!(nota_de_so_leitura(2_300_000).as_deref(), Some("Só leitura: arquivo grande (2,3 MB)"));
        assert_eq!(nota_de_so_leitura(400_000), None);
        assert_eq!(nota_de_so_leitura(1_200), None);
        // Acima do teto continua o aviso, com as saídas de antes.
        std::fs::write(pasta.join("dump.sql"), vec![b'a'; TETO_DE_LEITURA as usize + 1]).unwrap();
        assert!(matches!(ler_arquivo(&pasta.join("dump.sql"), true), Conteudo::Aviso(Aviso::Grande(n)) if n == TETO_DE_LEITURA + 1));
        assert_eq!(saidas_do_aviso(&Aviso::Grande(TETO_DE_LEITURA + 1)), [Saida::Editor, Saida::Fora]);
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn binario_grande_e_binario_e_abre_fora() {
        let pasta = std::env::temp_dir().join(format!("vitral-teste-fora-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        // Um PDF de 500 KB: antes era "grande demais", com o VS Code como saída.
        let mut pdf = b"%PDF-1.7\n".to_vec();
        pdf.extend(std::iter::repeat_n([0u8, 1, 2, 255], 125_000).flatten());
        std::fs::write(pasta.join("relatorio.pdf"), &pdf).unwrap();
        assert!(matches!(ler_arquivo(&pasta.join("relatorio.pdf"), false), Conteudo::Aviso(Aviso::Binario)));
        // O texto acima do teto de leitura continua "grande demais" (até 23/11 o teto era de 400 KB).
        std::fs::write(pasta.join("dados.csv"), vec![b'a'; TETO_DE_LEITURA as usize + 1]).unwrap();
        assert!(matches!(ler_arquivo(&pasta.join("dados.csv"), false), Conteudo::Aviso(Aviso::Grande(n)) if n == TETO_DE_LEITURA + 1));
        assert_eq!(saidas_do_aviso(&Aviso::Binario), [Saida::Fora]);
        assert_eq!(saidas_do_aviso(&Aviso::Grande(500_000)), [Saida::Editor, Saida::Fora]);
        assert_eq!(saidas_do_aviso(&Aviso::NaoExiste), [Saida::Fechar]);
        assert_eq!(saidas_do_aviso(&Aviso::Erro("x".into())), [Saida::Fechar]);
        // O arquivo vazio abre no editor (até 23/11 não havia o que fazer com ele).
        assert_eq!(saidas_do_aviso(&Aviso::Vazio), [Saida::Escrever]);
        assert_eq!(titulo_do_binario(&pasta.join("relatorio.PDF")), "Arquivo PDF");
        assert_eq!(titulo_do_binario(&pasta.join("vendas.xlsx")), "Planilha");
        assert_eq!(titulo_do_binario(&pasta.join("dump.bin")), "Arquivo binário");
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn o_svg_abre_como_imagem_e_em_linhas_quando_se_pede_o_texto() {
        let pasta = std::env::temp_dir().join(format!("vitral-teste-svg-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        let svg = pasta.join("logo.svg");
        std::fs::write(&svg, "<svg xmlns=\"http://www.w3.org/2000/svg\">\n<circle r=\"4\"/>\n</svg>\n").unwrap();
        assert!(matches!(ler_arquivo(&svg, true), Conteudo::Imagem));
        // Na edição e na busca (Ctrl+F), o painel pede as linhas.
        assert!(matches!(ler_arquivo(&svg, false), Conteudo::Codigo(linhas, _) if linhas.len() == 3));
        assert_eq!(icone_do_arquivo("assets/logo.svg"), Lucide::FileImage);
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn o_vs_code_abre_na_linha_escolhida_na_da_mudanca_ou_na_do_link() {
        assert_eq!(linha_do_editor(None, None, None), None);
        assert_eq!(linha_do_editor(None, None, Some(120)), Some(120));
        assert_eq!(linha_do_editor(None, Some((40, 44)), Some(120)), Some(40));
        assert_eq!(linha_do_editor(Some((7, 9)), Some((40, 44)), Some(120)), Some(7));
    }

    #[test]
    fn as_setas_da_edicao_passam_ao_proximo_arquivo_do_turno() {
        // Dentro do arquivo, de uma mudança à outra.
        assert_eq!(passo_da_revisao(0, 3, Some((0, 8)), 1), Some(Passo::Mudanca(1)));
        assert_eq!(passo_da_revisao(2, 3, Some((0, 8)), -1), Some(Passo::Mudanca(1)));
        // Na última mudança, o arquivo seguinte da lista; na primeira, o anterior, pelo fim dele.
        assert_eq!(passo_da_revisao(2, 3, Some((0, 8)), 1), Some(Passo::Arquivo { indice: 1, no_fim: false }));
        assert_eq!(passo_da_revisao(0, 3, Some((1, 8)), -1), Some(Passo::Arquivo { indice: 0, no_fim: true }));
        // Nas pontas da lista, dá a volta.
        assert_eq!(passo_da_revisao(2, 3, Some((7, 8)), 1), Some(Passo::Arquivo { indice: 0, no_fim: false }));
        assert_eq!(passo_da_revisao(0, 3, Some((0, 8)), -1), Some(Passo::Arquivo { indice: 7, no_fim: true }));
        // O arquivo sem mudança para mostrar (saiu do disco, registro antigo) não prende a seta.
        assert_eq!(passo_da_revisao(0, 0, Some((3, 8)), 1), Some(Passo::Arquivo { indice: 4, no_fim: false }));
        // Fora de uma lista (o diff de "Na pasta") ou com um arquivo só, dá a volta dentro dele.
        assert_eq!(passo_da_revisao(2, 3, None, 1), Some(Passo::Mudanca(0)));
        assert_eq!(passo_da_revisao(0, 3, Some((0, 1)), -1), Some(Passo::Mudanca(2)));
        assert_eq!(passo_da_revisao(0, 0, None, 1), None);
        assert_eq!(passo_da_revisao(0, 0, Some((0, 1)), 1), None);
        // A posição que passou do fim (o arquivo encolheu) vale como a última.
        assert_eq!(passo_da_revisao(9, 3, Some((0, 2)), 1), Some(Passo::Arquivo { indice: 1, no_fim: false }));
    }

    #[test]
    fn a_busca_conta_igual_na_linha_so_com_ascii() {
        // O atalho das linhas sem acento dá a mesma conta que a busca dobrada.
        for (linha, termo) in [
            ("2026-11-23 12:00:00 INFO pedido 1050000 conferido", "1050000"),
            ("let Config = CONFIG.config();", "config"),
            ("aaaa", "aa"),
            ("aaa", "aa"),
            ("curto", "um termo mais comprido que a linha"),
            ("sem nada", "xyz"),
        ] {
            assert_eq!(achados_na_linha(linha, termo), crate::ui::acentos::faixas(linha, termo).len(), "{linha:?} / {termo:?}");
        }
        assert_eq!(achados_na_linha("aaaa", "aa"), 2);
        // Com acento na linha ou no termo, a busca dobrada.
        assert_eq!(achados_na_linha("Configuração e configuracao", "configuracao"), 2);
    }

    #[test]
    fn a_busca_no_arquivo_acha_sem_acento_e_conta_cada_achado() {
        let linhas: Vec<SharedString> =
            vec!["// Configuração da loja".into(), "let configuracao = configuracao_padrao();".into(), "fim".into()];
        let termo = crate::ui::acentos::normalizar("CONFIGURACAO");
        assert_eq!(achados_nas_linhas(&linhas, &termo), [(0, 0), (1, 0), (1, 1)]);
        assert!(achados_nas_linhas(&linhas, "").is_empty());
        assert!(achados_nas_linhas(&linhas, "nada").is_empty());
    }
}
