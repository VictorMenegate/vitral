//! O visor: uma imagem do chat em tamanho cheio, por cima da conversa. Abre pela miniatura da
//! bolha, pela imagem de uma chamada ou pelo chip da caixa; Esc fecha e as setas andam pelas
//! outras imagens da conversa. Dá para mexer direto na imagem (dono, 13/11/2026: "dar zoom e
//! mexer direto na parte de ver"): a roda aproxima e afasta em volta do ponteiro, arrastar move,
//! dois cliques trocam entre caber na janela e o tamanho real, e a barra tem os mesmos passos.

use std::path::{Path, PathBuf};

use std::{cell::Cell, rc::Rc};

use gpui_kit::component::{Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{TamanhoBotao, botao_fantasma, botao_icone},
    imagens,
};
use crate::{
    modelo::Item,
    tema::{Cores, Lucide, Tipo as _, esp, r, raio, tam},
};

// As setas andam pelas imagens da conversa; o Esc é o do diálogo.
actions!(vitral, [ImagemAnterior, ImagemSeguinte]);

/// Margem entre o diálogo do visor e a borda da janela.
const FOLGA: f32 = 24.;
/// Altura da barra de cima (nome, medidas e botões), que sai da área da imagem.
const BARRA: f32 = 44.;

/// Até quanto se afasta e se aproxima, em vezes o tamanho que cabe na janela.
const ZOOM_MINIMO: f32 = 0.25;
const ZOOM_MAXIMO: f32 = 24.;
/// Um passo dos botões de zoom.
const PASSO_DO_ZOOM: f32 = 1.25;

pub(super) struct VisorImagem {
    /// Todas as imagens da conversa, na ordem em que foram mandadas.
    imagens: Vec<PathBuf>,
    atual: usize,
    /// Quantas vezes o tamanho que cabe na janela (1 é caber).
    zoom: f32,
    /// O quanto a imagem saiu do centro da área, arrastada ou pelo zoom em volta do ponteiro.
    desloc: Point<Pixels>,
    /// Um arrasto em curso: onde o ponteiro estava e onde a imagem estava quando ele começou.
    arrasto: Option<(Point<Pixels>, Point<Pixels>)>,
    /// A área da imagem no último desenho (para o zoom saber onde está o ponteiro dentro dela).
    area: Rc<Cell<Bounds<Pixels>>>,
    foco: FocusHandle,
}

/// Quanto a imagem encolhe para caber na área (nunca cresce: imagem pequena fica no tamanho dela).
fn encaixe(medidas: (u32, u32), area: Size<Pixels>) -> f32 {
    let (largura, altura) = (medidas.0.max(1) as f32, medidas.1.max(1) as f32);
    (area.width.as_f32() / largura).min(area.height.as_f32() / altura).min(1.)
}

/// Até onde a imagem pode sair do centro num eixo: só o que passa da área (cabendo, fica no meio).
fn limite(tamanho: f32, area: f32) -> f32 {
    ((tamanho - area) / 2.).max(0.)
}

/// O deslocamento novo para o zoom mudar em volta do ponteiro: o ponto debaixo dele (a `p` do
/// centro da área) continua debaixo dele.
fn desloc_no_zoom(p: Point<Pixels>, desloc: Point<Pixels>, de: f32, para: f32) -> Point<Pixels> {
    let razao = para / de;
    point(p.x - (p.x - desloc.x) * razao, p.y - (p.y - desloc.y) * razao)
}

impl VisorImagem {
    fn new(imagens: Vec<PathBuf>, atual: usize, cx: &mut Context<Self>) -> Self {
        VisorImagem {
            imagens,
            atual,
            zoom: 1.,
            desloc: Point::default(),
            arrasto: None,
            area: Rc::new(Cell::new(Bounds::default())),
            foco: cx.focus_handle(),
        }
    }

    /// Volta a caber na janela, no meio.
    fn caber(&mut self, cx: &mut Context<Self>) {
        self.zoom = 1.;
        self.desloc = Point::default();
        cx.notify();
    }

    /// Muda o zoom em volta de um ponto da tela (o ponteiro) ou, sem ele, do centro da área.
    fn aproximar(&mut self, para: f32, em_volta: Option<Point<Pixels>>, cx: &mut Context<Self>) {
        let para = para.clamp(ZOOM_MINIMO, ZOOM_MAXIMO);
        // Perto de caber, encaixa em caber: é o ponto de descanso.
        let para = if (para - 1.).abs() < 0.03 { 1. } else { para };
        let centro = self.area.get().center();
        let p = em_volta.map_or(Point::default(), |ponto| point(ponto.x - centro.x, ponto.y - centro.y));
        self.desloc = desloc_no_zoom(p, self.desloc, self.zoom, para);
        self.zoom = para;
        cx.notify();
    }

    /// O zoom que mostra a imagem no tamanho real dela.
    fn zoom_real(&self) -> f32 {
        let area = self.area.get().size;
        match imagens::anexo(self.caminho()).medidas {
            Some(medidas) if area.width > px(0.) => (1. / encaixe(medidas, area)).clamp(ZOOM_MINIMO, ZOOM_MAXIMO),
            _ => 1.,
        }
    }

    fn caminho(&self) -> &Path {
        &self.imagens[self.atual]
    }

    /// Anda uma imagem para o lado, parando nas pontas (a conversa tem começo e fim).
    fn andar(&mut self, adiante: bool, cx: &mut Context<Self>) {
        let proximo = if adiante { self.atual + 1 } else { self.atual.wrapping_sub(1) };
        if proximo < self.imagens.len() {
            self.atual = proximo;
            self.arrasto = None;
            self.caber(cx);
        }
    }

    /// A barra de cima: o nome e as medidas à esquerda, a contagem e os botões à direita.
    fn barra(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let anexo = imagens::anexo(self.caminho());
        let detalhe = anexo.detalhe();
        let caminho = self.caminho().to_path_buf();
        let varias = self.imagens.len() > 1;
        let cabendo = self.zoom == 1. && self.desloc == Point::default();
        let (rotulo, icone) = if cabendo { ("Tamanho real", Lucide::Maximize) } else { ("Ajustar à janela", Lucide::Minimize) };
        // A porcentagem é a do tamanho real da imagem, não a do "caber".
        let area = self.area.get().size;
        let porcento = anexo
            .medidas
            .filter(|_| area.width > px(0.))
            .map(|medidas| format!("{:.0}%", encaixe(medidas, area) * self.zoom * 100.));
        h_flex()
            .h(r(BARRA))
            .w_full()
            .flex_none()
            .items_center()
            .gap(esp::E8)
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .truncate()
                            .corpo_ui()
                            .text_color(Cores::forte())
                            .child(imagens::nome_do_arquivo(&caminho)),
                    )
                    .when(!detalhe.is_empty(), |s| {
                        s.child(div().metadado().text_color(Cores::tenue()).child(detalhe))
                    }),
            )
            .when(varias, |s| {
                s.child(
                    h_flex()
                        .flex_none()
                        .items_center()
                        .gap(esp::E4)
                        .child(
                            botao_icone("visor-anterior", Lucide::ChevronLeft, "Imagem anterior", TamanhoBotao::Barra, window, cx)
                                .disabled(self.atual == 0)
                                .on_click(cx.listener(|this, _, _, cx| this.andar(false, cx))),
                        )
                        .child(
                            div()
                                .metadado()
                                .text_color(Cores::tenue())
                                .child(format!("{} de {}", self.atual + 1, self.imagens.len())),
                        )
                        .child(
                            botao_icone("visor-seguinte", Lucide::ChevronRight, "Imagem seguinte", TamanhoBotao::Barra, window, cx)
                                .disabled(self.atual + 1 == self.imagens.len())
                                .on_click(cx.listener(|this, _, _, cx| this.andar(true, cx))),
                        ),
                )
            })
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap(esp::E4)
                    .child(
                        botao_icone("visor-menos", Lucide::ZoomOut, "Afastar", TamanhoBotao::Barra, window, cx)
                            .disabled(self.zoom <= ZOOM_MINIMO)
                            .on_click(cx.listener(|this, _, _, cx| this.aproximar(this.zoom / PASSO_DO_ZOOM, None, cx))),
                    )
                    .children(porcento.map(|p| div().min_w(r(40.)).text_center().metadado().text_color(Cores::tenue()).child(p)))
                    .child(
                        botao_icone("visor-mais", Lucide::ZoomIn, "Aproximar", TamanhoBotao::Barra, window, cx)
                            .disabled(self.zoom >= ZOOM_MAXIMO)
                            .on_click(cx.listener(|this, _, _, cx| this.aproximar(this.zoom * PASSO_DO_ZOOM, None, cx))),
                    ),
            )
            .child(
                botao_fantasma("visor-zoom", Some(icone.into()), rotulo, TamanhoBotao::Barra, window, cx)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if cabendo {
                            this.aproximar(this.zoom_real(), None, cx);
                        } else {
                            this.caber(cx);
                        }
                    })),
            )
            .child({
                let fora = caminho.clone();
                botao_fantasma("visor-fora", Some(Lucide::ExternalLink.into()), "Abrir fora", TamanhoBotao::Barra, window, cx)
                    .on_click(move |_, _, cx| crate::sistema::abrir_pasta(&fora, cx))
            })
            .child(
                botao_icone("visor-fechar", Lucide::X, "Fechar (Esc)", TamanhoBotao::Barra, window, cx)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
    }
}

impl Render for VisorImagem {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Sem foco em lugar nenhum, as setas vêm para cá.
        if window.focused(cx).is_none() {
            self.foco.focus(window, cx);
        }
        let area = area_da_imagem(window);
        let caminho = self.caminho().to_path_buf();
        let anexo = imagens::anexo(&caminho);
        let barra = self.barra(window, cx);
        let corpo = if !anexo.existe {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap(esp::E8)
                .child(super::mensagens::icone_de(Lucide::ImageOff, 32.).text_color(Cores::tenue()))
                .child(div().corpo_ui().text_color(Cores::tenue()).child("Esta imagem não está mais no disco."))
                .child(div().metadado().text_color(Cores::apagado()).child(caminho.display().to_string()))
                .into_any_element()
        } else {
            // A imagem no tamanho que cabe, vezes o zoom, no meio da área mais o deslocamento
            // (que só vale no que passa da área: cabendo, ela fica no meio).
            let medidas = anexo.medidas.unwrap_or((area.width.as_f32() as u32, area.height.as_f32() as u32));
            let escala = encaixe(medidas, area) * self.zoom;
            let (largura, altura) = (medidas.0 as f32 * escala, medidas.1 as f32 * escala);
            let (lim_x, lim_y) = (limite(largura, area.width.as_f32()), limite(altura, area.height.as_f32()));
            self.desloc = point(
                px(self.desloc.x.as_f32().clamp(-lim_x, lim_x)),
                px(self.desloc.y.as_f32().clamp(-lim_y, lim_y)),
            );
            let esquerda = (area.width.as_f32() - largura) / 2. + self.desloc.x.as_f32();
            let topo = (area.height.as_f32() - altura) / 2. + self.desloc.y.as_f32();
            let onde = self.area.clone();
            div()
                .id("visor-area")
                .relative()
                .size_full()
                .overflow_hidden()
                .cursor(if self.arrasto.is_some() { CursorStyle::ClosedHand } else { CursorStyle::OpenHand })
                // Só para saber onde a área está na tela (o zoom em volta do ponteiro).
                .child(canvas(move |limites, _, _| onde.set(limites), |_, _, _, _| {}).absolute().size_full())
                .child(img(caminho).absolute().left(px(esquerda)).top(px(topo)).w(px(largura)).h(px(altura)))
                .on_scroll(cx.listener(|this, evento: &ScrollEvent, window, cx| {
                    let passo = evento.delta.pixel_delta(window.line_height()).y.as_f32();
                    if passo != 0. {
                        this.aproximar(this.zoom * (passo * 0.004).exp(), Some(evento.position), cx);
                    }
                    cx.stop_propagation();
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, evento: &MouseDownEvent, _, cx| {
                        if evento.click_count >= 2 {
                            // Dois cliques: do "caber" para o tamanho real naquele ponto, e de volta.
                            this.arrasto = None;
                            if this.zoom == 1. {
                                this.aproximar(this.zoom_real().max(2.), Some(evento.position), cx);
                            } else {
                                this.caber(cx);
                            }
                        } else {
                            this.arrasto = Some((evento.position, this.desloc));
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_move(cx.listener(|this, evento: &MouseMoveEvent, _, cx| {
                    let Some((de, antes)) = this.arrasto else {
                        return;
                    };
                    if evento.pressed_button != Some(MouseButton::Left) {
                        this.arrasto = None;
                    } else {
                        this.desloc = point(antes.x + evento.position.x - de.x, antes.y + evento.position.y - de.y);
                    }
                    cx.notify();
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.arrasto = None;
                        cx.notify();
                    }),
                )
                .into_any_element()
        };
        v_flex()
            .track_focus(&self.foco)
            .key_context("Visor")
            .on_action(cx.listener(|this, _: &ImagemAnterior, _, cx| this.andar(false, cx)))
            .on_action(cx.listener(|this, _: &ImagemSeguinte, _, cx| this.andar(true, cx)))
            .w_full()
            .child(barra)
            .child(
                div()
                    .w_full()
                    .h(area.height)
                    .flex()
                    .rounded(raio::CARTAO)
                    // O xadrez não cabe aqui: o preto do app já é o fundo neutro do resto.
                    .bg(Cores::preto())
                    .overflow_hidden()
                    .child(corpo),
            )
    }
}

/// O espaço que sobra para a imagem dentro da janela, tirando a folga do diálogo, a margem dele e
/// a barra de cima.
fn area_da_imagem(window: &Window) -> Size<Pixels> {
    let folga = gpui_kit::component::window_paddings(window);
    let vista = window.viewport_size();
    let largura = vista.width - folga.left - folga.right - tam(window, FOLGA * 2. + 32.);
    let altura = vista.height - folga.top - folga.bottom - tam(window, FOLGA * 2. + BARRA + 40.);
    size(largura.max(px(120.)), altura.max(px(120.)))
}

/// A largura do diálogo: a janela inteira menos a folga.
fn largura_do_visor(window: &Window) -> Pixels {
    let folga = gpui_kit::component::window_paddings(window);
    let vista = window.viewport_size();
    (vista.width - folga.left - folga.right - tam(window, FOLGA * 2.)).max(px(240.))
}

impl Janela {
    /// Abre o visor na imagem clicada, com as outras imagens do chat ao lado dela nas setas.
    pub(super) fn abrir_visor(&mut self, chat: &str, caminho: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let lista = self
            .motor
            .as_ref()
            .and_then(|m| m.read(cx).chat(chat).map(|c| imagens_dos_itens(&c.itens, &c.cwd)))
            .unwrap_or_default();
        let (lista, atual) = match lista.iter().position(|c| c == caminho) {
            Some(i) => (lista, i),
            // Um anexo que ainda não entrou na conversa (o chip da caixa): só ele.
            None => (vec![caminho.to_path_buf()], 0),
        };
        let visor = cx.new(|cx| VisorImagem::new(lista, atual, cx));
        let foco = visor.read(cx).foco.clone();
        self.foco_antes_do_dialogo(window, cx);
        window.open_dialog(cx, move |dialogo, window, _| {
            dialogo
                .bg(Cores::dialogo())
                .w(largura_do_visor(window))
                .margin_top(tam(window, FOLGA))
                .rounded(raio::PECA)
                .close_button(false)
                // Enter não faz nada aqui: o visor só lê.
                .on_ok(|_, _, _| false)
                .child(visor.clone())
        });
        window.focus(&foco, cx);
    }
}

/// As imagens de uma conversa, na ordem e sem repetir: é por elas que as setas andam. Entram os
/// anexos das suas mensagens e as que a resposta aponta em `![](…)`, as mesmas que ganham
/// miniatura na conversa. O que saiu do disco fica de fora: na conversa ele é um chip, e a seta
/// não deve parar num aviso.
fn imagens_dos_itens(itens: &[Item], base: &Path) -> Vec<PathBuf> {
    let mut lista: Vec<PathBuf> = Vec::new();
    let mut juntar = |caminho: PathBuf| {
        if !lista.contains(&caminho) && imagens::anexo(&caminho).existe {
            lista.push(caminho);
        }
    };
    for item in itens {
        match item {
            Item::Usuario { anexos, .. } => {
                for caminho in anexos {
                    if crate::claude::tipo_imagem(caminho).is_some() {
                        juntar(caminho.clone());
                    }
                }
            }
            Item::Assistente { texto, .. } => {
                for caminho in imagens::imagens_do_texto(texto, Some(base)) {
                    juntar(caminho);
                }
            }
            // As que uma chamada leu ou gravou (a captura de tela que o chat foi olhar).
            Item::Ferramenta { arquivo: Some(arquivo), erro: false, .. } => {
                if let Some(inteiro) = super::links_arquivo::resolver(base, arquivo).map(PathBuf::from)
                    && crate::claude::tipo_imagem(&inteiro).is_some()
                {
                    juntar(inteiro);
                }
            }
            _ => {}
        }
    }
    lista
}

#[cfg(test)]
mod testes_do_zoom {
    use super::{desloc_no_zoom, encaixe, limite};
    use gpui_kit::{point, px, size};

    #[test]
    fn a_imagem_grande_encolhe_para_caber_e_a_pequena_fica_como_e() {
        let area = size(px(1000.), px(500.));
        assert_eq!(encaixe((2000, 500), area), 0.5);
        assert_eq!(encaixe((500, 2000), area), 0.25);
        assert_eq!(encaixe((100, 50), area), 1.);
    }

    #[test]
    fn so_sai_do_centro_o_que_passa_da_area() {
        assert_eq!(limite(800., 1000.), 0.);
        assert_eq!(limite(1400., 1000.), 200.);
    }

    #[test]
    fn o_zoom_gira_em_volta_do_ponteiro() {
        // O ponto debaixo do ponteiro (100 à direita do centro) continua debaixo dele.
        let p = point(px(100.), px(0.));
        let novo = desloc_no_zoom(p, point(px(0.), px(0.)), 1., 2.);
        assert_eq!(novo, point(px(-100.), px(0.)));
        // No centro, o zoom não desloca nada.
        assert_eq!(desloc_no_zoom(point(px(0.), px(0.)), point(px(0.), px(0.)), 1., 3.), point(px(0.), px(0.)));
    }
}

#[cfg(test)]
mod testes {
    use super::imagens_dos_itens;
    use crate::modelo::Item;
    use std::path::{Path, PathBuf};

    fn usuario(anexos: &[&str]) -> Item {
        Item::Usuario {
            texto: String::new(),
            anexos: anexos.iter().map(PathBuf::from).collect(),
            ultrathink: false,
            ultracode: false,
            em: 0,
        }
    }

    /// Anexo e markdown entram, na ordem e sem repetir; o que não é imagem, o que sumiu do disco
    /// e o caminho citado fora de um `![](…)` ficam de fora.
    #[test]
    fn a_lista_do_visor_e_so_de_imagem_que_esta_la() {
        let pasta = std::env::temp_dir().join(format!("vitral-visor-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        for nome in ["um.png", "dois.jpg", "tres.png", "notas.md"] {
            std::fs::write(pasta.join(nome), b"\x89PNG\r\n\x1a\n").unwrap();
        }
        let caminho = |nome: &str| pasta.join(nome).display().to_string();
        let itens = vec![
            usuario(&[&caminho("um.png"), &caminho("notas.md"), &caminho("sumiu.png")]),
            Item::Assistente { texto: "o print ![](tres.png), e `dois.jpg` solto".into(), em: 0 },
            usuario(&[&caminho("dois.jpg"), &caminho("um.png")]),
        ];
        assert_eq!(
            imagens_dos_itens(&itens, &pasta),
            vec![pasta.join("um.png"), pasta.join("tres.png"), pasta.join("dois.jpg")]
        );
        let _ = std::fs::remove_dir_all(&pasta);
    }
}
