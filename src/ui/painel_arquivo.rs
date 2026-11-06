//! Painel da direita com um arquivo do projeto aberto: aba com o nome, a barra do arquivo (pasta,
//! linhas e linguagem, mencionar, VS Code), markdown renderizado ou código com número de linha,
//! e o aviso de quando não dá para mostrar.

use gpui_kit::component::{
    ActiveTheme as _, ElementExt as _, WindowExt as _,
    highlighter::highlight_code,
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::{ScrollableElement as _, ScrollbarAxis},
    text::TextView,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use std::{
    io::ErrorKind,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant, SystemTime},
};

use super::{
    Gaveta, Janela,
    componentes::{
        EstadoVazio, Reflexo as _, TamanhoBotao, Tom, aviso, botao_fantasma, botao_icone,
        contagem,
    },
};
use crate::{
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, esp, r, raio},
};

/// Maior arquivo que abre aqui (o resto fica para o VS Code).
const LIMITE: u64 = 400_000;

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
    /// Chegam do segundo plano pouco depois da leitura; até lá o código sai sem cor.
    cores: Option<CoresDasLinhas>,
    rolagem: UniformListScrollHandle,
    _colorindo: Option<Task<()>>,
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

fn ler_arquivo(completo: &Path, markdown: bool) -> Conteudo {
    // O tamanho antes: um arquivo grande não precisa ser lido para dizer que não abre.
    match std::fs::metadata(completo) {
        Ok(m) if m.is_dir() => return Conteudo::Aviso(Aviso::Erro("É uma pasta, não um arquivo.".into())),
        // Um .png do projeto aparece como imagem; antes caía no "arquivo binário".
        Ok(m) if crate::claude::tipo_imagem(completo).is_some() => {
            return if m.len() > LIMITE_IMAGEM { Conteudo::Aviso(Aviso::Grande(m.len())) } else { Conteudo::Imagem };
        }
        Ok(m) if m.len() > LIMITE => return Conteudo::Aviso(Aviso::Grande(m.len())),
        Ok(_) => {}
        Err(erro) => return Conteudo::Aviso(erro_de_leitura(erro)),
    }
    let texto = match std::fs::read(completo) {
        Ok(bytes) if bytes.is_empty() => return Conteudo::Aviso(Aviso::Vazio),
        Ok(bytes) if bytes.len() as u64 > LIMITE => return Conteudo::Aviso(Aviso::Grande(bytes.len() as u64)),
        Ok(bytes) if bytes.contains(&0) => return Conteudo::Aviso(Aviso::Binario),
        Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
        Err(erro) => return Conteudo::Aviso(erro_de_leitura(erro)),
    };
    if markdown {
        let linhas = texto.lines().count();
        return Conteudo::Markdown(texto.into(), linhas);
    }
    // Tabulação vira 4 espaços: o GPUI não a desenha como recuo.
    let linhas: Vec<SharedString> = texto
        .lines()
        .map(|l| SharedString::from(if l.is_empty() { " ".to_string() } else { l.replace('\t', "    ") }))
        .collect();
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
fn linguagem(caminho: &Path) -> String {
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
    if crate::claude::tipo_imagem(Path::new(caminho)).is_some() {
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

/// Uma linha do código: mono 13/20, o número alinhado à direita em apagado e 16 até o texto.
fn linha_de_codigo(
    numero: usize,
    texto: SharedString,
    cores: Vec<(Range<usize>, HighlightStyle)>,
    largura_numero: Rems,
) -> impl IntoElement + use<> {
    // Uma faixa fora da linha derrubaria o desenho do texto.
    let cores: Vec<_> = cores
        .into_iter()
        .filter(|(f, _)| f.end <= texto.len() && texto.is_char_boundary(f.start) && texto.is_char_boundary(f.end))
        .collect();
    div()
        .flex()
        .h(r(20.))
        .pl(esp::E12)
        .pr(esp::E24)
        .gap(esp::E16)
        .mono()
        .whitespace_nowrap()
        .child(
            div()
                .w(largura_numero)
                .flex_none()
                .flex()
                .justify_end()
                .text_color(Cores::apagado())
                .child(numero.to_string()),
        )
        .child(
            div()
                .flex_none()
                .text_color(Cores::texto())
                .child(StyledText::new(texto).with_highlights(cores)),
        )
}

/// O nome que a barra do arquivo mostra para a pasta: o do projeto na pasta própria, senão o da
/// pasta.
fn nome_da_raiz(motor: &Motor, raiz: &std::path::Path) -> String {
    match motor.projeto_da_pasta(raiz) {
        Some(p) if motor.pastas_do(&p).iter().any(|l| l.propria && l.caminho == raiz) => motor.nome_do_projeto(&p),
        _ => raiz.file_name().map_or_else(|| raiz.display().to_string(), |n| n.to_string_lossy().to_string()),
    }
}

impl Janela {
    /// O conteúdo do arquivo aberto, relido só quando ele muda (a data é conferida no máximo a
    /// cada segundo), com as cores do código se já chegaram e a rolagem.
    fn conteudo_arquivo(
        &self,
        completo: &Path,
        markdown: bool,
        cx: &Context<Self>,
    ) -> (Conteudo, Option<CoresDasLinhas>, UniformListScrollHandle) {
        let mut lido = self.arquivo_lido.borrow_mut();
        let data = || std::fs::metadata(completo).and_then(|m| m.modified()).ok();
        let visto = |l: &ArquivoLido| (l.conteudo.clone(), l.cores.clone(), l.rolagem.clone());
        if let Some(l) = lido.as_mut()
            && l.caminho == completo
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
        let conteudo = ler_arquivo(completo, markdown);
        let colorindo = match &conteudo {
            Conteudo::Codigo(linhas, _) => Some(colorir(completo, modificado, linhas, cx)),
            _ => None,
        };
        let novo = ArquivoLido {
            caminho: completo.to_path_buf(),
            modificado,
            conferido: Instant::now(),
            conteudo,
            cores: None,
            rolagem,
            _colorindo: colorindo,
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
        // Na raiz da pasta, a barra mostra o nome dela (ou o do projeto, se ela é a pasta própria).
        let pasta = match caminho.rsplit_once('/') {
            Some((d, _)) => format!("{d}/"),
            None => nome_da_raiz(motor.read(cx), &raiz),
        };
        let markdown = caminho.ends_with(".md");
        let (conteudo, cores, rolagem_codigo) = self.conteudo_arquivo(&completo, markdown, cx);
        let janela = cx.entity().downgrade();
        let estreita = self.direita_estreita;
        let na_gaveta = self.gaveta == Some(Gaveta::Direita);
        let medir = self.medir_direita(cx);

        let linhas = match &conteudo {
            Conteudo::Codigo(linhas, _) => Some(linhas.len()),
            Conteudo::Markdown(_, n) => Some(*n),
            Conteudo::Aviso(_) | Conteudo::Imagem => None,
        };
        let cabecalho = self.barra_de_abas(motor.read(cx), super::abas_direita::AbaDireita::Arquivo(0), false, "arrastar-arquivo", window, cx);
        let barra = barra_do_arquivo(
            janela.clone(),
            &completo,
            &caminho,
            pasta,
            linhas,
            estreita,
            na_gaveta,
            window,
            cx,
        );
        let corpo = match conteudo {
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
                            .bg(Cores::preto())
                            .overflow_hidden()
                            .child(img(completo.clone()).max_w_full().max_h_full().object_fit(ObjectFit::Contain)),
                    )
                    .when(!detalhe.is_empty(), |s| {
                        s.child(div().flex_none().metadado().text_color(Cores::tenue()).child(detalhe))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| this.abrir_visor("", &alvo, window, cx)))
                    .into_any_element()
            }
            Conteudo::Markdown(texto, _) => div()
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
                        .child(
                            TextView::markdown(SharedString::from(format!("arquivo-{caminho}")), texto)
                                .selectable(true),
                        ),
                )
                .into_any_element(),
            // Só as linhas visíveis são montadas: arquivos de milhares de linhas não pesam.
            Conteudo::Codigo(linhas, mais_longa) => {
                let digitos = linhas.len().max(1).to_string().len().max(2);
                // Dígitos × 0,6 em do mono de 13, mais 4.
                let largura_numero = r(digitos as f32 * 7.8 + 4.);
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        uniform_list(
                            SharedString::from(format!("codigo-{caminho}")),
                            linhas.len(),
                            move |faixa, _, _| {
                                faixa
                                    .map(|i| {
                                        let cores_linha =
                                            cores.as_ref().and_then(|c| c.get(i)).cloned().unwrap_or_default();
                                        linha_de_codigo(i + 1, linhas[i].clone(), cores_linha, largura_numero)
                                    })
                                    .collect()
                            },
                        )
                        .with_sizing_behavior(ListSizingBehavior::Auto)
                        .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                        .with_width_from_item(Some(mais_longa))
                        .track_scroll(&rolagem_codigo)
                        .size_full()
                        .py(esp::E8),
                    )
                    .scrollbar(&rolagem_codigo, ScrollbarAxis::Both)
                    .into_any_element()
            }
        };
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .rounded(raio::PECA)
            .when(!na_gaveta, |s| s.bg(Cores::cartao()).reflexo(raio::PECA))
            .overflow_hidden()
            .child(cabecalho)
            .child(barra)
            .child(corpo)
            .on_prepaint(medir)
            .into_any_element()
    }
}

/// A barra embaixo da aba: a pasta (cortada no começo, o caminho inteiro na dica), "386 linhas ·
/// Rust", Mencionar, VS Code e o "⋯". No painel estreito, só a pasta e os ícones.
#[allow(clippy::too_many_arguments)]
fn barra_do_arquivo(
    janela: WeakEntity<Janela>,
    completo: &Path,
    caminho: &str,
    pasta: String,
    linhas: Option<usize>,
    estreita: bool,
    na_gaveta: bool,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let info = linhas.filter(|_| !estreita).map(|n| {
        let mut texto = contagem(n, "linha", "linhas");
        if let Some(nome) = nome_da_linguagem(caminho) {
            texto.push_str(" · ");
            texto.push_str(nome);
        }
        texto
    });
    let absoluto = SharedString::from(completo.display().to_string());
    let botao = |id: &'static str, icone: Lucide, rotulo: &'static str, dica: &'static str| {
        if estreita {
            botao_icone(id, icone, dica, TamanhoBotao::Barra, window, cx)
        } else {
            botao_fantasma(id, Some(icone.into()), rotulo, TamanhoBotao::Barra, window, cx).tooltip(dica)
        }
    };
    let (mencao, editor) = (caminho.to_string(), completo.to_path_buf());
    let j_mencionar = janela.clone();
    let menu = {
        let (conteudo, relativo) = (completo.to_path_buf(), caminho.to_string());
        let onde = completo.parent().map(Path::to_path_buf).unwrap_or_else(|| completo.to_path_buf());
        botao_icone("menu-arquivo", Lucide::Ellipsis, "Mais opções do arquivo", TamanhoBotao::Barra, window, cx)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let (conteudo, relativo, onde) = (conteudo.clone(), relativo.clone(), onde.clone());
                menu.item(PopupMenuItem::new("Copiar o conteúdo").icon(Lucide::Copy).on_click(
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
                .separator()
                .item(
                    PopupMenuItem::new("Mostrar na pasta")
                        .icon(Lucide::FolderOpen)
                        .external()
                        .on_click(move |_, _, cx| crate::sistema::abrir_pasta(&onde, cx)),
                )
            })
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(r(36.))
        .px(esp::E12)
        .gap(esp::E8)
        .border_b_1()
        // Na gaveta, opaca, a linha() some: lá vai a borda().
        .border_color(if na_gaveta { Cores::borda() } else { Cores::linha() })
        .child(
            div()
                .id("caminho-arquivo")
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(esp::E8)
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_start()
                        .mono_pequeno()
                        .text_color(Cores::tenue())
                        .child(pasta),
                )
                .children(info.map(|i| {
                    div().flex_none().metadado().text_color(Cores::apagado()).whitespace_nowrap().child(i)
                }))
                .tooltip(move |window, cx| Tooltip::new(absoluto.clone()).build(window, cx)),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(esp::E4)
                .child(botao("mencionar-arquivo", Lucide::AtSign, "Mencionar", "Mencionar na conversa").on_click(
                    move |_, window, cx| {
                        let _ = j_mencionar.update(cx, |this, cx| this.mencionar(&mencao, window, cx));
                    },
                ))
                .child(
                    botao("editor-arquivo", Lucide::CodeXml, "VS Code", "Abrir no VS Code")
                        .on_click(move |_, _, _| crate::sistema::abrir_no_editor(&editor)),
                )
                .child(menu),
        )
        .into_any_element()
}

/// O aviso no lugar do conteúdo, centralizado, com o que dá para fazer.
fn aviso_do_arquivo(
    aviso: Aviso,
    janela: WeakEntity<Janela>,
    completo: PathBuf,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let editor = || {
        botao_fantasma("aviso-editor", Some(Lucide::CodeXml.into()), "Abrir no VS Code", TamanhoBotao::Barra, window, cx)
            .on_click(move |_, _, _| crate::sistema::abrir_no_editor(&completo))
            .into_any_element()
    };
    let fechar = || {
        botao_fantasma("aviso-fechar", None, "Fechar", TamanhoBotao::Barra, window, cx)
            .on_click(move |_, _, cx| {
                let _ = janela.update(cx, |this, cx| this.fechar_arquivo(cx));
            })
            .into_any_element()
    };
    let (icone, titulo, texto, acao): (Lucide, &str, Option<String>, Option<AnyElement>) = match aviso {
        Aviso::Grande(bytes) => (
            Lucide::File,
            "Arquivo grande demais",
            Some(format!("Tem {}; aqui abrem arquivos de até {}.", tamanho(bytes), tamanho(LIMITE))),
            Some(editor()),
        ),
        Aviso::Binario => (
            Lucide::Binary,
            "Arquivo binário",
            Some("Não dá para mostrar o conteúdo aqui.".into()),
            Some(editor()),
        ),
        Aviso::NaoExiste => (
            Lucide::FileX,
            "O arquivo não existe mais",
            Some("Ele pode ter sido apagado ou renomeado.".into()),
            Some(fechar()),
        ),
        Aviso::Erro(erro) => (Lucide::FileX, "Não deu para ler o arquivo", Some(erro), Some(fechar())),
        Aviso::Vazio => (Lucide::File, "Arquivo vazio", None, None),
    };
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
    use super::{Aviso, Conteudo, cores_por_linha, icone_do_arquivo, ler_arquivo, nome_da_linguagem, tamanho};
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
        assert_eq!(aviso("grande.txt", &vec![b'a'; 400_001]), Some(Aviso::Grande(400_001)));
        assert_eq!(aviso("ok.rs", b"fn main() {}\n"), None);
        assert!(matches!(ler_arquivo(&pasta.join("sumiu.rs"), false), Conteudo::Aviso(Aviso::NaoExiste)));
        assert!(matches!(ler_arquivo(&pasta, false), Conteudo::Aviso(Aviso::Erro(_))));
        let _ = std::fs::remove_dir_all(&pasta);
    }
}
