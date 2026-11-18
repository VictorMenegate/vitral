//! Caixa de código: saída de comando (verificação, preparação) e trechos no chat, no preto de
//! dentro (`Cores::preto`).

use gpui_kit::component::ElementExt as _;
use gpui_kit::component::scroll::{ScrollableElement as _, ScrollbarAxis};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::controles::BotaoCopiar;
use crate::tema::{Cores, Tipo as _, dentro_da_borda, esp, r, raio};
use crate::ui::busca_conversa::{Realce, faixas, realcar, realcar_trecho};

/// Quantas linhas do fim aparecem antes de "Mostrar tudo".
const LINHAS_DO_FIM: usize = 40;

/// Cabeçalho de 28 (comando, tempo e copiar) e o corpo em mono 12/18, com as últimas 40 linhas
/// e "Mostrar tudo"; altura máxima de 240, rolando. Sem comando e sem tempo não há cabeçalho (era
/// uma faixa vazia de 28 só para o Copiar): o Copiar vai para o canto de cima, no hover.
#[derive(IntoElement)]
pub struct CaixaDeCodigo {
    id: ElementId,
    texto: SharedString,
    comando: Option<SharedString>,
    tempo: Option<SharedString>,
    erro: bool,
    raio_chat: bool,
    /// A busca na conversa achou aqui: as ocorrências ficam em âmbar, como no resto da conversa.
    realce: Realce,
}

impl CaixaDeCodigo {
    pub fn new(id: impl Into<ElementId>, texto: SharedString) -> Self {
        CaixaDeCodigo {
            id: id.into(),
            texto,
            comando: None,
            tempo: None,
            erro: false,
            raio_chat: false,
            realce: Realce::default(),
        }
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
    /// Realça aqui o que a busca da conversa achou.
    pub fn realce(mut self, realce: &Realce) -> Self {
        self.realce = realce.clone();
        self
    }

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
        // Com a busca da conversa achando aqui, a saída aparece inteira: as ocorrências contam no
        // texto todo, e o corte das 40 últimas linhas escondia as de cima (conferência na
        // bancada, 16/11/2026).
        let achou_aqui = !self.realce.vazio() && !faixas(&self.texto, &self.realce.termo).is_empty();
        let mostrar_tudo = *tudo.read(cx) || achou_aqui;
        let (corpo, fora) = if mostrar_tudo {
            (self.texto.to_string(), 0)
        } else {
            ultimas_linhas(&self.texto, LINHAS_DO_FIM)
        };
        let rolagem = window
            .use_keyed_state((self.id.clone(), "rolagem-corpo"), cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        // A caixa rola até a ocorrência atual uma vez por salto (o número sobe a cada F3).
        let salto = self.realce.salto_aqui();
        let rolado = window.use_keyed_state((self.id.clone(), "salto-corpo"), cx, |_, _| u64::MAX);
        let rolar_agora = salto.filter(|s| *rolado.read(cx) != *s);
        if let Some(s) = rolar_agora {
            rolado.update(cx, |r, _| *r = s);
        }
        let conteudo = match self.realce.atual {
            // A atual está aqui: linha por linha, para saber onde fica a dela e rolar até lá.
            Some(atual) => linhas_com_realce(&corpo, &self.realce.termo, atual, rolar_agora.map(|_| rolagem.clone())),
            None => realcar(corpo, &self.realce).into_any_element(),
        };
        let cor_tempo = if self.erro { Cores::vermelho() } else { Cores::tenue() };
        let para_copiar = self.comando.as_ref().map(|c| format!("$ {c}\n\n{}", self.texto)).unwrap_or_else(|| self.texto.to_string());
        let grupo = SharedString::from(format!("{}-caixa", self.id));
        let com_cabecalho = self.comando.is_some() || self.tempo.is_some();
        // O Copiar só com o ponteiro na caixa: à vista em toda caixa, era ruído.
        let copiar = div()
            .flex_none()
            .invisible()
            .group_hover(grupo.clone(), |s| s.visible())
            .child(BotaoCopiar::new(SharedString::from(format!("{}-copiar", self.id)), para_copiar));
        let (copiar_no_cabecalho, copiar_no_canto) = if com_cabecalho { (Some(copiar), None) } else { (None, Some(copiar)) };
        div()
            .id(self.id.clone())
            .group(grupo)
            .relative()
            .flex()
            .flex_col()
            .rounded(if self.raio_chat { raio::CARTAO } else { raio::CONTROLE })
            .bg(Cores::preto())
            .border_1()
            .border_color(if self.erro { Cores::tinta_borda(Cores::vermelho()) } else { Cores::borda() })
            .overflow_hidden()
            .when(com_cabecalho, |caixa| {
                caixa.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(r(8.))
                        .h(r(28.))
                        .pl(dentro_da_borda(12.))
                        .pr(esp::E2)
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
                        .children(copiar_no_cabecalho),
                )
            })
            .child(
                // Sem quebrar as linhas: a saída é código, e a linha quebrada no meio da
                // indentação embaralhava tudo (dono, 17/11/2026: "tá bem zuado essas partes"). A
                // caixa rola de lado também; o miolo tem a largura da linha mais longa.
                div()
                    .id("corpo")
                    .max_h(r(240.))
                    .overflow_scroll()
                    .track_scroll(&rolagem)
                    .scrollbar(&rolagem, ScrollbarAxis::Both)
                    .flex()
                    .font_family(crate::tema::FONTE_MONO)
                    .text_size(r(12.))
                    .line_height(r(18.))
                    .text_color(Cores::texto())
                    .child(div().flex_none().min_w_full().flex().flex_col().px(dentro_da_borda(12.)).py(esp::E8).whitespace_nowrap()
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
                    .child(conteudo)),
            )
            // Sem cabeçalho, o Copiar flutua no canto de cima, opaco, por cima do fim da primeira
            // linha.
            .children(copiar_no_canto.map(|copiar| {
                div().absolute().top(esp::E4).right(esp::E4).rounded(raio::CONTROLE).bg(Cores::preto()).child(copiar)
            }))
    }
}

/// A saída linha por linha, com o realce de cada uma e, na linha da ocorrência `atual` (contada
/// no texto todo), o pedido de rolar a caixa até ela, centrada.
fn linhas_com_realce(texto: &str, termo: &str, atual: usize, rolar: Option<ScrollHandle>) -> AnyElement {
    let mut antes = 0;
    let mut rolar = rolar;
    let linhas = texto.split('\n').map(|linha| {
        let n = faixas(linha, termo).len();
        let aqui = (atual >= antes && atual < antes + n).then(|| atual - antes);
        antes += n;
        let mut d = div().min_h(r(18.)).child(realcar_trecho(linha.to_string(), termo, aqui));
        if let Some(rolagem) = aqui.and(rolar.take()) {
            d = d.on_prepaint(move |limites, window, _| {
                let vista = rolagem.bounds();
                if vista.size.height <= px(0.) {
                    return;
                }
                let atual = rolagem.offset();
                let no_conteudo = limites.origin.y - vista.origin.y - atual.y;
                let alvo = (-(no_conteudo - (vista.size.height - limites.size.height) / 2.)).clamp(-rolagem.max_offset().y, px(0.));
                if alvo != atual.y {
                    rolagem.set_offset(point(atual.x, alvo));
                    window.refresh();
                }
            });
        }
        d.into_any_element()
    });
    div().flex().flex_col().children(linhas.collect::<Vec<_>>()).into_any_element()
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
