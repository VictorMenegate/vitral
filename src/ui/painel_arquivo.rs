//! Painel da direita com um arquivo da pasta aberto: aba com o nome, o cabeçalho do arquivo (o
//! nome, as linhas e a linguagem, a pasta, Mencionar, VS Code), markdown renderizado ou código com
//! número de linha, e o aviso de quando não dá para mostrar. Aberto por uma linha de edição da
//! conversa, mostra a edição no próprio arquivo, com as setas de uma mudança à outra
//! (`edicao.rs`). O número da linha escolhe linhas para mencionar ou comentar (`comentarios.rs`).
//! No diff do git, cada mudança pode voltar sozinha ao último commit (`desfazer_trecho.rs`).

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
    edicao::{Edicao, Fileira, Marca, Vista},
};
use crate::{
    motor::Motor,
    tema::{Cores, FONTE_MONO, Lucide, Tipo as _, altura, esp, r, raio},
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
    /// Lido como markdown (renderizado) ou como linhas; a edição pede as linhas mesmo no .md.
    como_markdown: bool,
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

fn ler_arquivo(completo: &Path, markdown: bool) -> Conteudo {
    // O tamanho antes: um arquivo grande não precisa ser lido para dizer que não abre.
    match std::fs::metadata(completo) {
        Ok(m) if m.is_dir() => return Conteudo::Aviso(Aviso::Erro("É uma pasta, não um arquivo.".into())),
        // Um .png da pasta aparece como imagem; antes caía no "arquivo binário".
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
/// `comentada`: a linha está num trecho com comentário guardado. `bloco`: a mudança do diff do git
/// a que a linha pertence, quando dá para desfazê-la sozinha (o botão aparece com o ponteiro nela).
#[allow(clippy::too_many_arguments)]
fn fileira_de_codigo(
    indice: usize,
    numero: Option<usize>,
    texto: SharedString,
    cores: Vec<(Range<usize>, HighlightStyle)>,
    largura_numero: Rems,
    fundo: Fundo,
    sinal: Option<&'static str>,
    comentada: bool,
    bloco: Option<usize>,
    janela: WeakEntity<Janela>,
) -> AnyElement {
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
    // Com o ponteiro numa linha mudada do diff do git, o botão que desfaz só aquele bloco: por
    // cima da coluna do sinal, que não rola para o lado com o texto.
    let grupo = bloco.map(|_| SharedString::from(format!("fileira-{indice}")));
    let desfazer = bloco.zip(grupo.clone()).map(|(bloco, grupo)| {
        let janela = janela.clone();
        div()
            .id(("desfazer-trecho-da-linha", indice))
            .absolute()
            .left(rems(largura_numero.0 + 14. / 15.))
            .top_0()
            .size(r(20.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(raio::CONTROLE)
            .bg(Cores::dialogo())
            .text_color(Cores::tenue())
            .cursor_pointer()
            .invisible()
            .group_hover(grupo, |s| s.visible())
            .hover(|s| s.text_color(Cores::texto()))
            .child(super::mensagens::icone_de(Lucide::RotateCcw, 12.))
            .tooltip(|window, cx| Tooltip::new("Desfazer só este trecho\nVolta ao último commit; o resto do arquivo fica").build(window, cx))
            .on_click(move |_, window, cx| {
                let _ = janela.update(cx, |this, cx| this.pedir_desfazer_trecho(bloco, window, cx));
            })
    });
    let numero_el = div()
        .id(("numero-da-linha", indice))
        .w(largura_numero)
        .flex_none()
        .flex()
        .justify_end()
        .text_color(if fundo == Fundo::Escolhida {
            Cores::texto()
        } else if comentada {
            Cores::destaque_texto()
        } else {
            Cores::apagado()
        })
        .children(numero.map(|n| n.to_string()))
        .when_some(numero, |s, n| {
            s.cursor_pointer().hover(|s| s.text_color(Cores::texto())).on_click(move |evento, window, cx| {
                let estender = evento.modifiers().shift;
                let _ = janela.update(cx, |this, cx| this.escolher_linha(n as u32, estender, window, cx));
            })
        });
    div()
        .relative()
        .when_some(grupo, |s, grupo| s.group(grupo))
        .flex()
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
        // O trecho com comentário guardado: uma faixa do destaque ao lado da do diff, e o número
        // na cor dele.
        .when(comentada, |s| s.child(div().absolute().left(esp::E4).top_0().bottom_0().w(px(2.)).bg(Cores::destaque())))
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
        .children(desfazer)
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

/// O nome que a barra do arquivo mostra para a pasta: "Sem pasta" na pasta vazia de um chat, senão
/// o da pasta.
fn nome_da_raiz(motor: &Motor, raiz: &std::path::Path) -> String {
    if raiz.starts_with(motor.dados.pasta_avulso("")) {
        return "Sem pasta".into();
    }
    super::dialogos::nome_da_pasta(raiz)
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
            && l.como_markdown == markdown
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
            como_markdown: markdown,
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
        // Na edição, o markdown sai em linhas, como o código: o diff é por linha.
        let edicao = self.arquivo_edicao.clone();
        let markdown = caminho.ends_with(".md") && edicao.is_none();
        let (conteudo, cores, rolagem_codigo) = self.conteudo_arquivo(&completo, markdown, cx);
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
        let barra = self.cabecalho_do_arquivo(&completo, &caminho, pasta, linhas, reserva, na_gaveta, window, cx);
        let desfaz_por_trecho = vista.as_ref().is_some_and(|(v, _)| self.desfaz_por_trecho(v));
        let faixa = edicao.as_ref().map(|e| self.faixa_da_edicao(e, vista.as_ref().map(|(v, _)| v.as_ref()), desfaz_por_trecho, reserva, na_gaveta, window, cx));
        let comentarios = self.faixas_dos_comentarios(reserva, na_gaveta, window, cx);
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
                // Os trechos com comentário guardado ficam marcados no número da linha.
                let comentadas = self.linhas_comentadas(&raiz, &caminho);
                let comentada = move |n: usize| comentadas.iter().any(|&(a, b)| (a as usize..=b as usize).contains(&n));
                let id = SharedString::from(format!("codigo-{caminho}"));
                let lista = match vista {
                    // A edição: as fileiras da vista, com as saídas entre as linhas do arquivo. Ao
                    // abrir e a cada seta, rola até a mudança (com folga em cima).
                    Some((vista, mais_longa)) => {
                        if self.edicao_rolar.replace(false)
                            && let Some(mudanca) = vista.mudancas.get(self.edicao_atual)
                        {
                            rolagem_codigo.scroll_to_item(mudanca.fileira.saturating_sub(4), ScrollStrategy::Top);
                        }
                        let janela = janela.clone();
                        // No diff do git, cada linha mudada sabe o bloco dela, para desfazê-lo sozinho.
                        let blocos = desfaz_por_trecho.then(|| super::desfazer_trecho::bloco_de_cada_fileira(&vista));
                        let bloco_da = move |i: usize| blocos.as_ref().and_then(|b| b.get(i).copied().flatten());
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
                                        fileira_de_codigo(i, Some(linha + 1), texto, cores_linha, largura_numero, fundo, Some(sinal), comentada(linha + 1), bloco_da(i), janela.clone())
                                    }
                                    Fileira::Saiu(texto) => {
                                        fileira_de_codigo(i, None, texto.clone().into(), Vec::new(), largura_numero, Fundo::Saiu, Some("−"), false, bloco_da(i), janela.clone())
                                    }
                                    Fileira::Solta(marca, texto) => {
                                        let (fundo, sinal) = match marca {
                                            Marca::Entra => (Fundo::Entrou, "+"),
                                            Marca::Sai => (Fundo::Saiu, "−"),
                                            Marca::Igual => (Fundo::Nenhum, " "),
                                        };
                                        fileira_de_codigo(i, None, texto.clone().into(), Vec::new(), largura_numero, fundo, Some(sinal), false, None, janela.clone())
                                    }
                                    Fileira::Nota(texto) => nota_de_codigo(texto.clone().into(), largura_numero),
                                })
                                .collect()
                        })
                        .with_width_from_item(Some(mais_longa))
                    }
                    None => {
                        let janela = janela.clone();
                        uniform_list(id, linhas.len(), move |faixa, _, _| {
                            faixa
                                .map(|i| {
                                    let cores_linha = cores.as_ref().and_then(|c| c.get(i)).cloned().unwrap_or_default();
                                    let fundo = if escolhida(i + 1) { Fundo::Escolhida } else { Fundo::Nenhum };
                                    fileira_de_codigo(i, Some(i + 1), linhas[i].clone(), cores_linha, largura_numero, fundo, None, comentada(i + 1), None, janela.clone())
                                })
                                .collect()
                        })
                        .with_width_from_item(Some(mais_longa))
                    }
                };
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        lista
                            .with_sizing_behavior(ListSizingBehavior::Auto)
                            .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
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
            // Ancorado, é a última peça da fileira das conversas: encosta no topo e na direita da
            // janela e emenda na conversa da esquerda pela divisória (ver `aresta_encostada`).
            .when(na_gaveta, |s| s.rounded(crate::tema::raio_da_peca()))
            .when(!na_gaveta, |s| s.bg(Cores::cartao()))
            .overflow_hidden()
            .child(cabecalho)
            .child(barra)
            .children(faixa)
            .children(comentarios)
            .child(corpo)
            .when(!na_gaveta, |s| s.aresta_encostada(crate::tema::Esquerda::Emendada))
            .on_prepaint(medir)
            .into_any_element()
    }
}

/// Quanto ocupam as ações do cabeçalho do arquivo (em px a 100%): só os ícones (Mencionar, VS
/// Code e o "⋯", de 28, com 4 entre eles) ou com os rótulos.
const ACOES_SO_ICONES: f32 = 3. * 28. + 2. * 4.;
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
        reserva: Option<Pixels>,
        na_gaveta: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let janela = cx.entity().downgrade();
        let nome = caminho.rsplit('/').next().unwrap_or(caminho).to_string();
        let selecao = self.arquivo_selecao;
        let info = match selecao {
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
        let livre = largura - 12. - aneis - 16. - 8.;
        let (com_rotulos, com_info) = o_que_cabe(livre, &nome, info.as_deref());
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
        let menu = {
            let (conteudo, relativo, inteiro) = (completo.to_path_buf(), caminho.to_string(), completo.to_path_buf());
            let onde = completo.parent().map(Path::to_path_buf).unwrap_or_else(|| completo.to_path_buf());
            let janela = janela.clone();
            // Com um trecho em vista (a seleção ou a mudança), o arquivo inteiro também se menciona.
            let com_trecho = self.alvo_da_mencao().is_some();
            botao_icone("menu-arquivo", Lucide::Ellipsis, "Mais opções do arquivo", TamanhoBotao::Barra, window, cx)
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                    let (conteudo, relativo, onde, inteiro, janela) =
                        (conteudo.clone(), relativo.clone(), onde.clone(), inteiro.clone(), janela.clone());
                    menu.when(com_trecho, |menu| {
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
                    .separator()
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
                    .child(div().flex_none().text_color(Cores::tenue()).child(super::componentes::icone(icone_do_arquivo(caminho), 16., window)))
                    .child(div().flex_shrink(1.).min_w(r(24.)).truncate().rotulo().text_color(Cores::forte()).child(nome))
                    .when(com_info, |s| {
                        s.children(info.map(|i| {
                            div()
                                .flex_none()
                                .whitespace_nowrap()
                                .metadado()
                                .text_color(if selecao.is_some() { Cores::destaque_texto() } else { Cores::apagado() })
                                .child(i)
                        }))
                    })
                    .children(desfazer)
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
                    .child(botao("mencionar-arquivo", Lucide::AtSign, "Mencionar", dica_mencao).on_click(move |_, window, cx| {
                        let _ = j_mencionar.update(cx, |this, cx| this.mencionar_do_painel(window, cx));
                    }))
                    .child(
                        botao("editor-arquivo", Lucide::CodeXml, "VS Code", "Abrir no VS Code".into())
                            .on_click(move |_, _, _| crate::sistema::abrir_no_editor(&editor)),
                    )
                    .child(menu),
            )
            .into_any_element()
    }

    /// A faixa da edição, embaixo do cabeçalho: "Edição", o +N −M e "2 de 5", com as setas que
    /// passam de uma mudança à outra e o × que volta ao arquivo sem a edição. No registro de antes
    /// de guardar o diff, um aviso discreto no lugar.
    fn faixa_da_edicao(
        &self,
        edicao: &Edicao,
        vista: Option<&Vista>,
        desfaz_por_trecho: bool,
        reserva: Option<Pixels>,
        na_gaveta: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let mudancas = vista.map_or(0, |v| v.mudancas.len());
        // No diff do git, a mudança à vista (a das setas) volta sozinha ao último commit.
        let desfazer = (desfaz_por_trecho && mudancas > 0).then(|| {
            botao_icone("desfazer-trecho", Lucide::RotateCcw, "Desfazer só esta mudança\nVolta ao último commit; o resto do arquivo fica", TamanhoBotao::Linha, window, cx)
                .on_click(cx.listener(move |this, _, window, cx| this.pedir_desfazer_trecho(this.edicao_atual % mudancas, window, cx)))
        });
        let fechar = botao_icone("fechar-edicao", Lucide::X, "Ver o arquivo sem a edição", TamanhoBotao::Linha, window, cx)
            .on_click(cx.listener(|this, _, _, cx| this.sair_da_edicao(cx)));
        let setas = (mudancas > 1).then(|| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(esp::E2)
                .child(
                    botao_icone("mudanca-anterior", Lucide::ChevronUp, "Mudança anterior", TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|this, _, _, cx| this.ir_para_mudanca(-1, cx))),
                )
                .child(
                    botao_icone("mudanca-seguinte", Lucide::ChevronDown, "Próxima mudança", TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|this, _, _, cx| this.ir_para_mudanca(1, cx))),
                )
        });
        let esquerda = if edicao.sem_registro {
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(esp::E8)
                .child(div().flex_none().text_color(Cores::tenue()).child(super::componentes::icone(Lucide::Info, 14., window)))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .metadado()
                        .text_color(Cores::tenue())
                        .child("Este registro não guardou a edição: o arquivo aparece como está agora."),
                )
        } else {
            let posicao = (mudancas > 0).then(|| format!("{} de {}", (self.edicao_atual % mudancas) + 1, mudancas));
            div()
                .flex()
                .flex_1()
                .min_w_0()
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
            .children(desfazer)
            .children(setas)
            .child(fechar)
            .into_any_element()
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

    /// A edição posta nas linhas do arquivo aberto, como o último desenho a montou.
    pub(super) fn vista_aberta(&self) -> Option<Rc<Vista>> {
        self.arquivo_lido.borrow().as_ref()?.vista.as_ref().map(|(_, vista, _)| vista.clone())
    }

    /// O arquivo aberto mudou no disco agora mesmo (um trecho desfeito): o próximo desenho o relê
    /// sem esperar o segundo da conferência, e continua rolado onde estava.
    pub(super) fn reler_arquivo_aberto(&self) {
        if let Some(l) = self.arquivo_lido.borrow_mut().as_mut() {
            l.modificado = None;
            l.conferido = Instant::now().checked_sub(Duration::from_secs(2)).unwrap_or(l.conferido);
        }
    }

    /// As linhas que o Mencionar leva: as escolhidas ou, na edição, as da mudança à vista.
    fn alvo_da_mencao(&self) -> Option<(u32, u32)> {
        self.arquivo_selecao.or_else(|| {
            self.arquivo_edicao.as_ref()?;
            let lido = self.arquivo_lido.borrow();
            let (_, vista, _) = lido.as_ref()?.vista.as_ref()?;
            let mudanca = vista.mudancas.get(self.edicao_atual % vista.mudancas.len().max(1))?;
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
    pub(super) fn escolher_linha(&mut self, linha: u32, estender: bool, window: &mut Window, cx: &mut Context<Self>) {
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
        // O campo do comentário acompanha a escolha: o texto que o trecho já tem, ou vazio.
        self.acertar_campo_do_comentario(window, cx);
        cx.notify();
    }

    /// As setas da edição: a mudança anterior ou a seguinte (dá a volta), rolando até ela.
    fn ir_para_mudanca(&mut self, passo: isize, cx: &mut Context<Self>) {
        let total = self.arquivo_lido.borrow().as_ref().and_then(|l| l.vista.as_ref()).map_or(0, |(_, v, _)| v.mudancas.len());
        if total == 0 {
            return;
        }
        self.edicao_atual = (self.edicao_atual as isize + passo).rem_euclid(total as isize) as usize;
        self.edicao_rolar.set(true);
        cx.notify();
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
