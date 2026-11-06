//! O visor: uma imagem do chat em tamanho cheio, por cima da conversa. Abre pela miniatura da
//! bolha ou pelo chip da caixa; Esc fecha, as setas andam pelas outras imagens da conversa, e um
//! botão troca entre caber na janela e o tamanho real (com rolagem).

use std::path::{Path, PathBuf};

use gpui_kit::component::{
    Disableable as _, WindowExt as _,
    scroll::{ScrollableElement as _, ScrollbarAxis},
    h_flex, v_flex,
};
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

pub(super) struct VisorImagem {
    /// Todas as imagens da conversa, na ordem em que foram mandadas.
    imagens: Vec<PathBuf>,
    atual: usize,
    /// Em vez de caber na janela, a imagem fica no tamanho dela e a área rola.
    tamanho_real: bool,
    foco: FocusHandle,
    rolagem: ScrollHandle,
}

impl VisorImagem {
    fn new(imagens: Vec<PathBuf>, atual: usize, cx: &mut Context<Self>) -> Self {
        VisorImagem { imagens, atual, tamanho_real: false, foco: cx.focus_handle(), rolagem: ScrollHandle::new() }
    }

    fn caminho(&self) -> &Path {
        &self.imagens[self.atual]
    }

    /// Anda uma imagem para o lado, parando nas pontas (a conversa tem começo e fim).
    fn andar(&mut self, adiante: bool, cx: &mut Context<Self>) {
        let proximo = if adiante { self.atual + 1 } else { self.atual.wrapping_sub(1) };
        if proximo < self.imagens.len() {
            self.atual = proximo;
            self.tamanho_real = false;
            self.rolagem.set_offset(point(px(0.), px(0.)));
            cx.notify();
        }
    }

    /// A barra de cima: o nome e as medidas à esquerda, a contagem e os botões à direita.
    fn barra(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let anexo = imagens::anexo(self.caminho());
        let detalhe = anexo.detalhe();
        let caminho = self.caminho().to_path_buf();
        let varias = self.imagens.len() > 1;
        let (rotulo, icone) = if self.tamanho_real {
            ("Ajustar à janela", Lucide::ZoomOut)
        } else {
            ("Tamanho real", Lucide::ZoomIn)
        };
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
                botao_fantasma("visor-zoom", Some(icone.into()), rotulo, TamanhoBotao::Barra, window, cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tamanho_real = !this.tamanho_real;
                        this.rolagem.set_offset(point(px(0.), px(0.)));
                        cx.notify();
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
        } else if self.tamanho_real {
            // No tamanho real a imagem pode passar da janela dos dois lados: a área rola.
            let (largura, altura) = anexo.medidas.unwrap_or((0, 0));
            div()
                .id("visor-rolagem")
                .size_full()
                .overflow_scroll()
                .track_scroll(&self.rolagem)
                .child(
                    img(caminho)
                        .flex_none()
                        .when(largura > 0, |s| s.w(px(largura as f32)))
                        .when(altura > 0, |s| s.h(px(altura as f32))),
                )
                .scrollbar(&self.rolagem, ScrollbarAxis::Both)
                .into_any_element()
        } else {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(img(caminho).max_w(area.width).max_h(area.height).object_fit(ObjectFit::Contain))
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
            _ => {}
        }
    }
    lista
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
            ponto: None,
            ultrathink: false,
            ultracode: false,
            referencias: Vec::new(),
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
