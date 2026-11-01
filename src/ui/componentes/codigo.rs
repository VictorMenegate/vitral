//! Caixa de código: saída de comando (verificação, preparação) e trechos no chat, no preto de
//! dentro (`Cores::preto`).

use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::controles::BotaoCopiar;
use crate::tema::{Cores, Tipo as _, dentro_da_borda, esp, r, raio};

/// Quantas linhas do fim aparecem antes de "Mostrar tudo".
const LINHAS_DO_FIM: usize = 40;

/// Cabeçalho de 28 (comando, tempo e copiar) e o corpo em mono 12/18, com as últimas 40 linhas
/// e "Mostrar tudo"; altura máxima de 240, rolando.
#[derive(IntoElement)]
pub struct CaixaDeCodigo {
    id: ElementId,
    texto: SharedString,
    comando: Option<SharedString>,
    tempo: Option<SharedString>,
    erro: bool,
    raio_chat: bool,
}

impl CaixaDeCodigo {
    pub fn new(id: impl Into<ElementId>, texto: SharedString) -> Self {
        CaixaDeCodigo { id: id.into(), texto, comando: None, tempo: None, erro: false, raio_chat: false }
    }

    pub fn comando(mut self, c: impl Into<SharedString>) -> Self {
        self.comando = Some(c.into());
        self
    }

    /// "PASSOU em 12 s", "12 s".
    pub fn tempo(mut self, t: impl Into<SharedString>) -> Self {
        self.tempo = Some(t.into());
        self
    }

    /// Saída de um comando que falhou: o tempo e a borda em vermelho.
    pub fn erro(mut self, sim: bool) -> Self {
        self.erro = sim;
        self
    }

    /// Raio 12 dentro do chat (8 nos cartões).
    pub fn raio_chat(mut self) -> Self {
        self.raio_chat = true;
        self
    }
}

/// O fim do texto com no máximo `n` linhas, e quantas ficaram de fora.
fn ultimas_linhas(texto: &str, n: usize) -> (String, usize) {
    let linhas: Vec<&str> = texto.lines().collect();
    let fora = linhas.len().saturating_sub(n);
    (linhas[fora..].join("\n"), fora)
}

impl RenderOnce for CaixaDeCodigo {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tudo = window.use_keyed_state(self.id.clone(), cx, |_, _| false);
        let mostrar_tudo = *tudo.read(cx);
        let (corpo, fora) = if mostrar_tudo {
            (self.texto.to_string(), 0)
        } else {
            ultimas_linhas(&self.texto, LINHAS_DO_FIM)
        };
        let cor_tempo = if self.erro { Cores::vermelho() } else { Cores::tenue() };
        div()
            .id(self.id.clone())
            .flex()
            .flex_col()
            .rounded(if self.raio_chat { raio::CARTAO } else { raio::CONTROLE })
            .bg(Cores::preto())
            .border_1()
            .border_color(if self.erro { Cores::tinta_borda(Cores::vermelho()) } else { Cores::borda() })
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(r(8.))
                    .h(r(28.))
                    .pl(dentro_da_borda(12.))
                    .pr(esp::E2)
                    .border_b_1()
                    .border_color(Cores::linha())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .mono_pequeno()
                            .text_color(Cores::codigo())
                            .children(self.comando.map(|c| format!("$ {c}"))),
                    )
                    .children(self.tempo.map(|t| div().flex_none().metadado().text_color(cor_tempo).child(t)))
                    .child(
                        BotaoCopiar::new(
                            Clipboard::new(SharedString::from(format!("{}-copiar", self.id))).value(self.texto.clone()),
                        ),
                    ),
            )
            .child(
                div()
                    .id("corpo")
                    .max_h(r(240.))
                    .overflow_y_scroll()
                    .px(dentro_da_borda(12.))
                    .py(esp::E8)
                    .font_family(crate::tema::FONTE_MONO)
                    .text_size(r(12.))
                    .line_height(r(18.))
                    .text_color(Cores::texto())
                    .when(fora > 0, |d| {
                        d.child(
                            div()
                                .id("mostrar-tudo")
                                .pb(r(6.))
                                .metadado()
                                .text_color(Cores::destaque_texto())
                                .cursor_pointer()
                                .hover(|s| s.underline())
                                .child(format!("Mostrar tudo ({fora} linhas acima)"))
                                .on_click(move |_, _, cx| tudo.update(cx, |v, cx| {
                                    *v = true;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(corpo),
            )
    }
}

#[cfg(test)]
mod testes {
    use super::ultimas_linhas;

    #[test]
    fn so_o_fim_da_saida() {
        let texto: String = (1..=50).map(|i| format!("linha {i}\n")).collect();
        let (fim, fora) = ultimas_linhas(&texto, 40);
        assert_eq!(fora, 10);
        assert!(fim.starts_with("linha 11\n"));
        assert!(fim.ends_with("linha 50"));
        assert_eq!(ultimas_linhas("a\nb", 40), ("a\nb".to_string(), 0));
    }
}
