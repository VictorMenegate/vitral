//! A página do .md no jeito do Notion (protótipo `tarefas/linear/vitral-linear.html`, "a página
//! do .md, editável"): cada bloco (`pagina_md.rs`) aparece pronto para ler e, com um clique, vira o
//! campo onde se escreve o markdown dele. O "/" abre os tipos de bloco; "# ", "- ", "1. ", "[] ",
//! "> ", "```" e "---" no começo de um texto viram o tipo; o Enter abre o bloco de baixo (a lista
//! continua) e o Backspace no começo desfaz o tipo ou junta com o de cima. A alça, à esquerda com o
//! ponteiro em cima, põe um bloco embaixo ("+") e arrasta o bloco. Gravar é o editor do arquivo
//! (`editor_arquivo.rs`): a página passa o texto inteiro a ele, que grava 800 ms depois e cuida do
//! conflito com o agente. O que não foi mexido volta ao arquivo como estava.

use std::path::{Path, PathBuf};

use gpui_kit::component::{
    h_flex,
    tooltip::{ManagedTooltipExt as _, Tooltip},
    input::{Backspace, Enter, Escape, Indent, IndentInline, InputEvent, MoveDown, MoveUp, Textarea, TextareaState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    pagina_md::{self, Bloco, Pagina, Tipo},
};
use crate::tema::{Cores, FONTE_MONO, Lucide, Tipo as _, esp, r, raio};

// Ctrl+B, Ctrl+I e Ctrl+E com o foco num bloco da página.
actions!(vitral, [NegritoNaPagina, ItalicoNaPagina, CodigoNaPagina]);

/// O contexto do teclado do bloco em edição (os atalhos de formatação valem só nele).
pub(super) const CONTEXTO: &str = "PaginaMd";

/// A página aberta: o documento e o bloco em edição.
pub(super) struct PaginaAberta {
    completo: PathBuf,
    doc: Pagina,
    /// O texto de onde o documento saiu: o arquivo que muda por fora (sem nada em edição) é lido de novo.
    lido: String,
    foco: Option<Foco>,
    menu: Option<MenuDaBarra>,
}

/// O bloco em edição e o campo dele.
struct Foco {
    id: u64,
    campo: Entity<TextareaState>,
    /// A barra que aparece em cima do bloco com texto selecionado.
    barra: Entity<BarraDaSelecao>,
    _assinatura: Subscription,
}

/// Com texto selecionado no bloco: negrito, itálico, código, Mencionar (com as linhas) e Copiar,
/// em cima do bloco (a `barra-trecho` do protótipo). Numa vista própria, que acompanha o campo: a
/// seleção muda sem redesenhar a janela.
pub(super) struct BarraDaSelecao {
    campo: Entity<TextareaState>,
    janela: WeakEntity<Janela>,
    /// As linhas do arquivo do bloco (a menção as mostra).
    linhas: Option<(u32, u32)>,
    _observa: Subscription,
}

impl Render for BarraDaSelecao {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.campo.read(cx).selected_range().is_empty() {
            return div().into_any_element();
        }
        let janela = self.janela.clone();
        let acao = move |f: fn(&mut Janela, &mut Window, &mut Context<Janela>)| {
            let janela = janela.clone();
            move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                // O foco fica no campo (e a seleção também).
                window.prevent_default();
                cx.stop_propagation();
                let _ = janela.update(cx, |j, cx| f(j, window, cx));
            }
        };
        let botao = |id: &'static str| {
            h_flex()
                .id(id)
                .flex_none()
                .h(r(26.))
                .rounded(raio::de(24.))
                .text_size(r(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(Cores::texto_da_fala())
                .cursor_pointer()
                .hover(|s| s.bg(Cores::cartao_elevado()).text_color(Cores::texto()))
        };
        let formato = |id: &'static str| botao(id).w(r(26.)).justify_center();
        let faixa = self.linhas.map(|(a, b)| if b > a { format!("{a}-{b}") } else { a.to_string() });
        let barra = h_flex()
            .mb(esp::E6)
            .p(crate::tema::linear::MOLDURA_FOLGA)
            .gap(crate::tema::linear::CONTROLES_VAO)
            .rounded(raio::cartao())
            // Chapada, sem sombra (o `.barra-trecho` do protótipo).
            .bg(Cores::preto())
            .occlude()
            .child(formato("fmt-negrito").font_weight(FontWeight::BOLD).child("B").on_mouse_down(MouseButton::Left, acao(|j, w, cx| j.marcar_na_pagina("**", w, cx))))
            .child(formato("fmt-italico").italic().child("I").on_mouse_down(MouseButton::Left, acao(|j, w, cx| j.marcar_na_pagina("*", w, cx))))
            .child(
                formato("fmt-codigo")
                    .child(super::componentes::icone(Lucide::Code, 14., window))
                    .on_mouse_down(MouseButton::Left, acao(|j, w, cx| j.marcar_na_pagina("`", w, cx))),
            )
            .child(div().flex_none().w(px(1.)).h(r(16.)).mx(esp::E2).bg(Cores::forte().opacity(0.08)))
            .child(
                botao("mencionar-na-pagina")
                    .pl(esp::E8)
                    .pr(crate::tema::linear::ITEM_LADO)
                    .gap(esp::E6)
                    .child(div().flex_none().text_color(Cores::tenue()).child(super::componentes::icone(Lucide::AtSign, 14., window)))
                    .child("Mencionar")
                    .children(faixa.map(|f| div().flex_none().text_color(Cores::tenue()).tabular().child(f)))
                    .on_mouse_down(MouseButton::Left, acao(|j, w, cx| j.mencionar_da_pagina(w, cx))),
            )
            .child(
                botao("copiar-na-pagina")
                    .pl(esp::E8)
                    .pr(crate::tema::linear::ITEM_LADO)
                    .gap(esp::E6)
                    .child(div().flex_none().text_color(Cores::tenue()).child(super::componentes::icone(Lucide::Copy, 14., window)))
                    .child("Copiar")
                    .on_mouse_down(MouseButton::Left, acao(|j, _, cx| j.copiar_da_pagina(cx))),
            );
        // Entra como os menus, a cada seleção nova (a barra-trecho do protótipo).
        super::movimento::entrar(barra, "entra-barra-da-pagina", super::movimento::De::Cima).into_any_element()
    }
}

/// O menu do "/": o que se digitou depois da barra, quantos caracteres a escolha apaga e o item
/// com o cursor.
struct MenuDaBarra {
    filtro: String,
    apagar: usize,
    cursor: usize,
}

/// O bloco arrastado pela alça.
#[derive(Clone)]
pub(super) struct ArrastoDeBloco {
    id: u64,
    nome: SharedString,
}

impl Render for ArrastoDeBloco {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(esp::E8)
            .py(esp::E4)
            .rounded(raio::CONTROLE)
            .bg(Cores::cartao_elevado())
            .text_size(r(12.))
            .text_color(Cores::texto())
            .max_w(r(260.))
            .truncate()
            .child(self.nome.clone())
    }
}

/// A medida de cada tipo: tamanho e altura da letra, o peso e o espaço em cima e embaixo (o CSS
/// `.pagina` do protótipo).
fn medida(tipo: Tipo) -> (f32, f32, FontWeight, Rems, Rems) {
    use crate::tema::pagina as m;
    match tipo {
        Tipo::Titulo1 => (23., 29.9, FontWeight(650.), m::H1_ANTES, esp::E6),
        Tipo::Titulo2 => (17., 22.95, FontWeight(620.), esp::E16, esp::E2),
        Tipo::Titulo3 => (14.5, 23.2, FontWeight::SEMIBOLD, m::H3_ANTES, r(0.)),
        Tipo::Codigo => (12., 19., FontWeight::NORMAL, esp::E4, esp::E4),
        Tipo::Citacao => (13.5, 21.6, FontWeight::NORMAL, esp::E2, esp::E2),
        _ => (13.5, 21.6, FontWeight::NORMAL, r(0.), r(0.)),
    }
}

fn cor_do_tipo(tipo: Tipo) -> Hsla {
    match tipo {
        Tipo::Titulo1 | Tipo::Titulo2 | Tipo::Titulo3 => Cores::texto(),
        Tipo::Citacao => Cores::tenue(),
        _ => Cores::texto_da_fala(),
    }
}

/// O texto lido de um bloco, com o negrito, o itálico, o código e os links.
fn texto_lido(bloco: &Bloco) -> (pagina_md::Lido, StyledText) {
    let lido = pagina_md::ler_linha(&bloco.texto);
    let destaques: Vec<(std::ops::Range<usize>, HighlightStyle)> = lido
        .pedacos
        .iter()
        .map(|(faixa, e)| {
            let mut h = HighlightStyle::default();
            if e.negrito {
                h.font_weight = Some(FontWeight::SEMIBOLD);
                h.color = Some(Cores::texto());
            }
            if e.italico {
                h.font_style = Some(FontStyle::Italic);
            }
            if e.codigo {
                h.background_color = Some(Cores::cartao_elevado());
                h.color = Some(Cores::texto());
            }
            if e.riscado {
                h.strikethrough = Some(StrikethroughStyle { thickness: px(1.), color: None });
            }
            if e.link.is_some() {
                h.color = Some(Cores::destaque_texto());
            }
            (faixa.clone(), h)
        })
        .collect();
    let texto = StyledText::new(lido.texto.clone()).with_highlights(destaques);
    (lido, texto)
}

impl Janela {
    /// O documento da página de `completo`, lido do editor aberto, ou de `do_disco` quando o arquivo
    /// mudou por fora e nada está em edição.
    fn preparar_pagina(&self, completo: &Path, do_disco: Option<&str>, cx: &App) {
        let do_editor = self.editor_arquivo.as_ref().filter(|e| e.sessao.caminho() == completo).map(|e| e.estado.read(cx).value().to_string());
        let mut pagina = self.pagina.borrow_mut();
        let precisa = match pagina.as_ref() {
            None => true,
            Some(p) => p.completo != completo || (p.foco.is_none() && do_editor.is_none() && do_disco.is_some_and(|t| t != p.lido)),
        };
        if precisa {
            let texto = do_editor.or_else(|| do_disco.map(str::to_string)).unwrap_or_default();
            *pagina = Some(PaginaAberta { completo: completo.to_path_buf(), doc: Pagina::ler(&texto), lido: texto, foco: None, menu: None });
        }
    }

    /// Esquece a página: a próxima leitura vem do editor ou do disco (a Fonte mexeu, o disco mudou).
    pub(super) fn esquecer_pagina(&self) {
        *self.pagina.borrow_mut() = None;
    }

    /// O corpo do painel com a página do .md.
    pub(super) fn corpo_da_pagina(&self, completo: &Path, do_disco: Option<&str>, window: &Window, cx: &Context<Self>) -> AnyElement {
        self.preparar_pagina(completo, do_disco, cx);
        let pagina = self.pagina.borrow();
        let Some(p) = pagina.as_ref() else { return div().into_any_element() };
        let frente = p.doc.frente.as_ref().filter(|f| !f.pares.is_empty()).map(|f| {
            // As propriedades do frontmatter, como no protótipo: chave em mono, valor ao lado.
            div()
                .grid()
                .grid_cols(2)
                .mx(crate::tema::pagina::CAMPO_X)
                .mb(esp::E16)
                .px(esp::E12)
                .py(esp::E8)
                .rounded(raio::CARTAO)
                .bg(Cores::preto())
                .children(f.pares.iter().flat_map(|(k, v)| {
                    [
                        div().pr(esp::E16).py(crate::tema::pagina::CAMPO_X).font_family(FONTE_MONO).text_size(r(11.5)).line_height(r(18.)).text_color(Cores::apagado()).child(k.clone()),
                        div().py(crate::tema::pagina::CAMPO_X).text_size(r(12.5)).line_height(r(18.)).text_color(Cores::texto_da_fala()).child(v.clone()),
                    ]
                }))
        });
        let janela = cx.entity();
        let fim = div()
            .id("pagina-fim")
            .h(crate::tema::pagina::FIM)
            .cursor_text()
            .on_click(cx.listener(|this, _, window, cx| this.clicar_no_fim_da_pagina(window, cx)))
            .on_drop(cx.listener(|this, arrasto: &ArrastoDeBloco, window, cx| this.mover_bloco(arrasto.id, None, window, cx)));
        let blocos: Vec<AnyElement> =
            (0..p.doc.blocos.len()).map(|i| self.bloco_da_pagina(p, i, janela.clone(), window, cx)).collect();
        div()
            .id("conteudo-arquivo")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(r(crate::tema::pagina::LARGURA))
                    .pl(crate::tema::pagina::ESQUERDA)
                    .pr(crate::tema::pagina::DIREITA)
                    .pt(crate::tema::pagina::TOPO)
                    .text_size(r(13.5))
                    .line_height(r(21.6))
                    .text_color(Cores::texto_da_fala())
                    .children(frente)
                    .children(blocos)
                    .child(fim),
            )
            .into_any_element()
    }

    /// Um bloco: a alça, a marca (lista, número, caixinha) e o texto lido ou o campo.
    fn bloco_da_pagina(&self, p: &PaginaAberta, i: usize, janela: Entity<Janela>, window: &Window, cx: &Context<Self>) -> AnyElement {
        let b = &p.doc.blocos[i];
        let id = b.id;
        let (tamanho, altura, peso, antes, depois) = medida(b.tipo);
        let focado = p.foco.as_ref().filter(|f| f.id == id);
        let grupo = SharedString::from(format!("bloco-{id}"));
        let alca_topo = match b.tipo {
            Tipo::Titulo1 => r(7.),
            Tipo::Titulo2 => r(4.),
            Tipo::Codigo => r(8.),
            Tipo::Divisor => r(1.),
            _ => r(2.),
        };
        let nome: SharedString = match b.tipo {
            Tipo::Divisor => "Divisor".into(),
            Tipo::Tabela => "Tabela".into(),
            _ => pagina_md::ler_linha(&b.texto).texto.chars().take(40).collect::<String>().into(),
        };
        // A alça: o "+" põe um bloco embaixo (com o menu do "/"), os pontos arrastam.
        let botao_da_alca = |id_el: &'static str, icone: Lucide, dica: &'static str| {
            div()
                .id(id_el)
                .w(r(21.))
                .h(r(23.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(raio::CONTROLE)
                .text_color(Cores::apagado())
                .cursor_pointer()
                .hover(|s| s.bg(Cores::cartao_elevado()).text_color(Cores::texto_da_fala()))
                .child(super::componentes::icone(icone, 13., window))
                .dica_no_ponteiro(move |window, cx| Tooltip::new(dica).build(window, cx))
        };
        let alca = h_flex()
            .absolute()
            .left(r(-46.))
            .top(alca_topo)
            .opacity(0.)
            .group_hover(grupo.clone(), |s| s.opacity(1.))
            .child(botao_da_alca("b-mais", Lucide::Plus, "Bloco abaixo").on_click(cx.listener(move |this, _, window, cx| this.bloco_abaixo(id, window, cx))))
            .child(
                botao_da_alca("b-arrastar", Lucide::GripVertical, "Arraste para mover")
                    .cursor_grab()
                    .on_drag(ArrastoDeBloco { id, nome }, |arrasto, _, _, cx| cx.new(|_| arrasto.clone())),
            );
        // A marca da esquerda: o ponto da lista, o número, a caixinha da tarefa.
        let marca = match b.tipo {
            Tipo::Lista => Some(div().flex_none().w(r(18.)).pt(esp::E2).text_center().text_color(Cores::tenue()).child("•").into_any_element()),
            Tipo::Numerada => Some(
                div()
                    .flex_none()
                    .min_w(r(18.))
                    .pr(esp::E2)
                    .pt(esp::E2)
                    .text_right()
                    .text_color(Cores::tenue())
                    .child(format!("{}.", p.doc.numero(i)))
                    .into_any_element(),
            ),
            Tipo::Tarefa => {
                let feito = b.feito;
                Some(
                    div()
                        .id("caixinha")
                        .flex_none()
                        .size(r(15.))
                        .mt(esp::E4)
                        .mr(crate::tema::pagina::CAIXINHA_DIREITA)
                        .ml(esp::E2)
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(raio::de(11.))
                        .border(px(1.5))
                        .border_color(if feito { Cores::destaque() } else { Cores::apagado() })
                        .when(feito, |s| s.bg(Cores::destaque()))
                        .text_color(Cores::forte())
                        .cursor_pointer()
                        .when(feito, |s| s.child(super::componentes::icone(Lucide::Check, 10., window)))
                        .on_click(cx.listener(move |this, _, window, cx| this.alternar_tarefa(id, window, cx)))
                        .into_any_element(),
                )
            }
            _ => None,
        };
        let recuo = b.recuo.chars().count() as f32 / 2.;
        let conteudo = match (b.tipo, focado) {
            (Tipo::Divisor, _) => div()
                .flex_1()
                .py(crate::tema::pagina::TOPO)
                .child(div().mx(crate::tema::pagina::CAMPO_X).h(px(1.)).bg(Cores::forte().opacity(0.08)))
                .into_any_element(),
            (Tipo::Tabela, _) => tabela(&b.tabela).into_any_element(),
            (_, Some(foco)) => self.campo_do_bloco(b, foco, p.menu.as_ref(), tamanho, altura, peso, window, cx),
            _ => self.leitura_do_bloco(b, tamanho, altura, peso, janela, cx),
        };
        let bloco = div()
            .id(("bloco", id as usize))
            .group(grupo)
            .relative()
            .flex()
            .items_start()
            .gap(esp::E4)
            .when(i > 0 || p.doc.frente.is_some(), |s| s.mt(antes))
            .mb(depois)
            .when(recuo > 0., |s| s.pl(r(18. * recuo)))
            // Soltar um bloco arrastado: ele entra antes deste, com a linha azul em cima.
            .drag_over::<ArrastoDeBloco>(|s, _, _, _| s.border_t(px(2.)).border_color(Cores::destaque_texto()))
            .on_drop(cx.listener(move |this, arrasto: &ArrastoDeBloco, window, cx| this.mover_bloco(arrasto.id, Some(id), window, cx)))
            .child(alca)
            .children(marca)
            .child(conteudo);
        // O bloco que acabou de nascer (Enter, "+", o fim da página) entra de cima, uma vez.
        if p.doc.novo(id) {
            super::movimento::entrar(bloco, ("entra-bloco", id as usize), super::movimento::De::Cima).into_any_element()
        } else {
            bloco.into_any_element()
        }
    }

    /// O bloco pronto para ler: o clique põe o cursor no lugar dele no markdown (o link abre).
    fn leitura_do_bloco(&self, b: &Bloco, tamanho: f32, altura: f32, peso: FontWeight, janela: Entity<Janela>, cx: &Context<Self>) -> AnyElement {
        let id = b.id;
        let (lido, texto) = texto_lido(b);
        let layout = texto.layout().clone();
        let vazio = b.texto.is_empty();
        let feito = b.tipo == Tipo::Tarefa && b.feito;
        let pedacos = lido.pedacos.clone();
        let no_markdown = lido.no_markdown.clone();
        let corpo = match b.tipo {
            Tipo::Codigo => div()
                .w_full()
                .px(esp::E12)
                .py(crate::tema::pagina::TOPO)
                .rounded(raio::CARTAO)
                .bg(Cores::preto())
                .font_family(FONTE_MONO)
                .whitespace_nowrap()
                .overflow_x_hidden()
                .children(b.texto.split('\n').map(|l| div().min_h(r(19.)).child(l.to_string())))
                .into_any_element(),
            _ if vazio => div().text_color(Cores::apagado()).child(if b.tipo == Tipo::Texto { "" } else { b.tipo.marcador() }).into_any_element(),
            _ => div().child(texto).into_any_element(),
        };
        let _ = janela;
        div()
            .id("leitura")
            .flex_1()
            .min_w_0()
            .px(crate::tema::pagina::CAMPO_X)
            .py(esp::E2)
            // A citação tem o fio da esquerda reto.
            .when(b.tipo != Tipo::Citacao, |s| s.rounded(raio::CONTROLE))
            .text_size(r(tamanho))
            .line_height(r(altura))
            .font_weight(peso)
            .text_color(if feito { Cores::apagado() } else { cor_do_tipo(b.tipo) })
            .when(feito, |s| s.line_through())
            .when(b.tipo == Tipo::Citacao, |s| s.border_l_2().border_color(Cores::destaque_texto()).pl(esp::E12))
            .cursor_text()
            .child(corpo)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, evento: &MouseDownEvent, window, cx| {
                    let indice = layout.index_for_position(evento.position).unwrap_or_else(|i| i);
                    // O link abre, como na leitura do markdown.
                    if let Some(link) = pedacos.iter().find(|(f, e)| f.contains(&indice) && e.link.is_some()).and_then(|(_, e)| e.link.clone()) {
                        match super::previa_markdown::destino_do_link(&link) {
                            super::previa_markdown::Destino::Web => cx.open_url(&link),
                            super::previa_markdown::Destino::Arquivo => {
                                window.dispatch_action(Box::new(super::links_arquivo::AbrirLinkVitral(link.clone())), cx)
                            }
                            super::previa_markdown::Destino::Nenhum => {}
                        }
                        return;
                    }
                    let onde = no_markdown.get(indice).copied().unwrap_or(usize::MAX);
                    this.focar_bloco(id, onde, window, cx);
                    cx.stop_propagation();
                }),
            )
            .into_any_element()
    }

    /// O bloco em edição: o campo com o markdown dele, no tamanho do tipo, e o menu do "/" embaixo.
    #[allow(clippy::too_many_arguments)]
    fn campo_do_bloco(
        &self,
        b: &Bloco,
        foco: &Foco,
        menu: Option<&MenuDaBarra>,
        tamanho: f32,
        altura: f32,
        peso: FontWeight,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let codigo = b.tipo == Tipo::Codigo;
        let campo = Textarea::new(&foco.campo)
            .appearance(false)
            .respiro_vertical(px(0.))
            .respiro_lateral(px(0.))
            .text_size(r(tamanho))
            .line_height(r(altura))
            .font_weight(peso)
            .text_color(cor_do_tipo(b.tipo))
            .when(codigo, |c| c.font_family(FONTE_MONO));
        let menu = menu.map(|m| self.menu_da_barra(m, window, cx));
        // A barra da seleção, em cima do bloco (encostada no canto de cima à esquerda dele).
        let barra = deferred(anchored().anchor(Anchor::BottomLeft).snap_to_window_with_margin(px(8.)).child(foco.barra.clone())).with_priority(1);
        v_flex()
            .flex_1()
            .min_w_0()
            .child(barra)
            .key_context(CONTEXTO)
            .capture_action(cx.listener(|this, _: &Backspace, window, cx| this.backspace_na_pagina(window, cx)))
            .capture_action(cx.listener(|this, _: &MoveUp, window, cx| this.seta_na_pagina(true, window, cx)))
            .capture_action(cx.listener(|this, _: &MoveDown, window, cx| this.seta_na_pagina(false, window, cx)))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| this.esc_na_pagina(window, cx)))
            .capture_action(cx.listener(|this, acao: &Enter, window, cx| this.enter_capturado(acao.secondary, window, cx)))
            .capture_action(cx.listener(|this, _: &Indent, window, cx| this.tab_na_pagina(window, cx)))
            .capture_action(cx.listener(|this, _: &IndentInline, window, cx| this.tab_na_pagina(window, cx)))
            .on_action(cx.listener(|this, _: &NegritoNaPagina, window, cx| this.marcar_na_pagina("**", window, cx)))
            .on_action(cx.listener(|this, _: &ItalicoNaPagina, window, cx| this.marcar_na_pagina("*", window, cx)))
            .on_action(cx.listener(|this, _: &CodigoNaPagina, window, cx| this.marcar_na_pagina("`", window, cx)))
            .child(
                div()
                    .px(if codigo { esp::E12 } else { crate::tema::pagina::CAMPO_X })
                    .py(if codigo { crate::tema::pagina::TOPO } else { esp::E2 })
                    .rounded(if codigo { raio::CARTAO } else { raio::CONTROLE })
                    .when(codigo, |s| s.bg(Cores::preto()))
                    .when(b.tipo == Tipo::Citacao, |s| s.border_l_2().border_color(Cores::destaque_texto()).pl(esp::E12))
                    .child(campo),
            )
            .children(menu)
            .into_any_element()
    }

    /// O menu do "/": os tipos que casam com o filtro, com a sigla num quadradinho.
    fn menu_da_barra(&self, menu: &MenuDaBarra, window: &Window, cx: &Context<Self>) -> AnyElement {
        let tipos = pagina_md::tipos_do_filtro(&menu.filtro);
        let cursor = menu.cursor.min(tipos.len().saturating_sub(1));
        let itens = tipos.into_iter().enumerate().map(|(i, (tipo, nome, sigla))| {
            let marcado = i == cursor;
            div()
                .id(("tipo-de-bloco", i))
                .flex()
                .items_center()
                .gap(esp::E8)
                .h(r(30.))
                .px(esp::E6)
                .rounded(raio::CONTROLE)
                .text_size(r(12.5))
                .text_color(if marcado { Cores::texto() } else { Cores::texto_da_fala() })
                .when(marcado, |s| s.bg(Cores::cartao_elevado()))
                .hover(|s| s.bg(Cores::cartao_elevado()).text_color(Cores::texto()))
                .cursor_pointer()
                .child(
                    div()
                        .flex_none()
                        .size(r(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(raio::CONTROLE)
                        .bg(Cores::cartao_elevado())
                        .text_color(Cores::texto_da_fala())
                        .text_size(r(10.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .map(|s| if tipo == Tipo::Tarefa { s.child(super::componentes::icone(Lucide::Check, 11., window)) } else { s.child(sigla) }),
                )
                .child(nome)
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(cx.listener(move |this, _, window, cx| this.escolher_tipo(tipo, window, cx)))
        });
        deferred(
            anchored().snap_to_window_with_margin(px(8.)).child(super::movimento::entrar(
                v_flex()
                    .mt(esp::E6)
                    .w(r(220.))
                    .p(esp::E4)
                    .rounded(raio::CARTAO)
                    // Chapado, sem fio claro nem sombra (o `.menu-flutua` do protótipo).
                    .bg(Cores::preto())
                    .child(div().px(esp::E6).py(esp::E4).text_size(r(11.)).font_weight(FontWeight::SEMIBOLD).text_color(Cores::tenue()).child("Blocos"))
                    .children(itens),
                "entra-menu-da-barra",
                super::movimento::De::Cima,
            )),
        )
        .with_priority(1)
        .into_any_element()
    }

    // ---- o que acontece ----

    /// Põe o bloco `id` em edição com o cursor em `onde` (byte do markdown; além do fim vai ao fim).
    pub(super) fn focar_bloco(&mut self, id: u64, onde: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (texto, tipo) = {
            let pagina = self.pagina.borrow();
            let Some(p) = pagina.as_ref() else { return };
            let Some(b) = p.doc.indice(id).map(|i| &p.doc.blocos[i]) else { return };
            if !b.tipo.editavel() {
                return;
            }
            (b.texto.clone(), b.tipo)
        };
        let onde = onde.min(texto.len());
        let codigo = tipo == Tipo::Codigo;
        let campo = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 2000)
                .submit_on_enter(!codigo)
                .placeholder(tipo.marcador())
                .default_value(texto)
        });
        campo.update(cx, |c, cx| {
            c.set_selected_range(onde..onde, cx);
            c.focus(window, cx);
        });
        let esta = campo.entity_id();
        let assinatura = cx.subscribe_in(&campo, window, move |this, _, evento: &InputEvent, window, cx| match evento {
            InputEvent::Change => this.campo_da_pagina_mudou(window, cx),
            InputEvent::PressEnter { shift: false, .. } => this.enter_na_pagina(window, cx),
            InputEvent::Blur => this.saiu_do_campo(esta, cx),
            _ => {}
        });
        let linhas = self.pagina.borrow().as_ref().and_then(|p| {
            let b = &p.doc.blocos[p.doc.indice(id)?];
            p.doc.linhas_do_trecho(id, 0, b.texto.len())
        });
        let janela = cx.entity().downgrade();
        let do_campo = campo.clone();
        let barra = cx.new(|cx| BarraDaSelecao {
            _observa: cx.observe(&do_campo, |_, _, cx| cx.notify()),
            campo: do_campo,
            janela,
            linhas,
        });
        if let Some(p) = self.pagina.borrow_mut().as_mut() {
            p.foco = Some(Foco { id, campo, barra, _assinatura: assinatura });
            p.menu = None;
        }
        cx.notify();
    }

    /// O Mencionar da barra da seleção: o trecho, com as linhas dele no arquivo, vai para a caixa.
    fn mencionar_da_pagina(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, campo, texto, _)) = self.campo_atual(cx) else { return };
        let faixa = campo.read(cx).selected_range();
        let (a, b) = (faixa.start.min(texto.len()), faixa.end.min(texto.len()));
        let Some((completo, linhas)) = self.pagina.borrow().as_ref().map(|p| (p.completo.clone(), p.doc.linhas_do_trecho(id, a, b))) else { return };
        let trecho = texto.get(a..b).unwrap_or_default().trim_end().to_string();
        self.mencionar_pedaco(&completo, linhas, Some(&trecho).filter(|t| !t.is_empty()).map(String::as_str), window, cx);
    }

    /// O Copiar da barra da seleção.
    fn copiar_da_pagina(&mut self, cx: &mut Context<Self>) {
        let Some((_, campo, _, _)) = self.campo_atual(cx) else { return };
        let texto = campo.read(cx).selected_value().to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(texto));
    }

    /// O campo perdeu o foco (clicou fora): o bloco volta a ser lido. Se o foco foi para outro
    /// bloco, aquele já é o da edição.
    fn saiu_do_campo(&mut self, campo: EntityId, cx: &mut Context<Self>) {
        if let Some(p) = self.pagina.borrow_mut().as_mut()
            && p.foco.as_ref().is_some_and(|f| f.campo.entity_id() == campo)
        {
            p.foco = None;
            p.menu = None;
        }
        cx.notify();
    }

    /// O texto e o cursor do campo em edição.
    fn campo_atual(&self, cx: &App) -> Option<(u64, Entity<TextareaState>, String, usize)> {
        let pagina = self.pagina.borrow();
        let f = pagina.as_ref()?.foco.as_ref()?;
        let c = f.campo.read(cx);
        Some((f.id, f.campo.clone(), c.value().to_string(), c.cursor()))
    }

    /// Uma tecla no campo: o texto vai para o bloco; o atalho do começo troca o tipo; o "/" abre o
    /// menu; e a página inteira vai para o editor do arquivo.
    fn campo_da_pagina_mudou(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, _, texto, cursor)) = self.campo_atual(cx) else { return };
        let antes = texto.get(..cursor).unwrap_or(&texto).to_string();
        let mut virou = None;
        {
            let mut pagina = self.pagina.borrow_mut();
            let Some(p) = pagina.as_mut() else { return };
            let Some(i) = p.doc.indice(id) else { return };
            let tipo = p.doc.blocos[i].tipo;
            p.doc.blocos[i].texto = texto.clone();
            if tipo == Tipo::Texto
                && let Some(novo) = pagina_md::atalho(&antes)
            {
                p.doc.blocos[i].texto = texto[cursor..].to_string();
                p.doc.blocos[i].tipo = novo;
                p.menu = None;
                virou = Some(novo);
            } else if tipo != Tipo::Codigo
                && let Some((filtro, apagar)) = pagina_md::barra(&antes)
                && !pagina_md::tipos_do_filtro(&filtro).is_empty()
            {
                let cursor = p.menu.as_ref().filter(|m| m.filtro == filtro).map_or(0, |m| m.cursor);
                p.menu = Some(MenuDaBarra { filtro, apagar, cursor });
            } else {
                p.menu = None;
            }
        }
        match virou {
            Some(Tipo::Divisor) => self.divisor_no_lugar(id, window, cx),
            Some(_) => self.focar_bloco(id, 0, window, cx),
            None => {}
        }
        self.pagina_mudou(window, cx);
    }

    /// O bloco `id` virou divisor: o texto dele (se houver) desce para um bloco novo, em edição.
    fn divisor_no_lugar(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let novo = {
            let mut pagina = self.pagina.borrow_mut();
            let Some(p) = pagina.as_mut() else { return };
            let Some(i) = p.doc.indice(id) else { return };
            p.doc.blocos[i].tipo = Tipo::Divisor;
            let resto = std::mem::take(&mut p.doc.blocos[i].texto);
            let b = p.doc.bloco(Tipo::Texto, resto);
            let novo = b.id;
            p.doc.blocos.insert(i + 1, b);
            p.foco = None;
            novo
        };
        self.focar_bloco(novo, 0, window, cx);
    }

    /// A página mudou: o texto inteiro vai para o editor do arquivo, que grava sozinho.
    fn pagina_mudou(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((completo, texto)) = self.pagina.borrow().as_ref().map(|p| (p.completo.clone(), p.doc.escrever())) else { return };
        if !self.editando(&completo) {
            self.abrir_o_editor(0, 0, false, window, cx);
            if !self.editando(&completo) {
                return;
            }
        }
        self.texto_da_pagina_no_editor(texto, window, cx);
        cx.notify();
    }

    /// Enter: com o menu aberto, escolhe; num bloco de código, o Ctrl+Enter sai dele. O resto segue
    /// para o campo (que avisa o Enter da página, `enter_na_pagina`).
    fn enter_capturado(&mut self, secundario: bool, window: &mut Window, cx: &mut Context<Self>) {
        let escolhido = self.pagina.borrow().as_ref().and_then(|p| {
            let m = p.menu.as_ref()?;
            let tipos = pagina_md::tipos_do_filtro(&m.filtro);
            tipos.get(m.cursor.min(tipos.len().saturating_sub(1))).map(|(t, _, _)| *t)
        });
        if let Some(tipo) = escolhido {
            self.escolher_tipo(tipo, window, cx);
            return;
        }
        let no_codigo = self.tipo_em_edicao() == Some(Tipo::Codigo);
        if no_codigo && secundario {
            self.sair_do_codigo(window, cx);
            return;
        }
        cx.propagate();
    }

    fn tipo_em_edicao(&self) -> Option<Tipo> {
        let pagina = self.pagina.borrow();
        let p = pagina.as_ref()?;
        let id = p.foco.as_ref()?.id;
        p.doc.indice(id).map(|i| p.doc.blocos[i].tipo)
    }

    /// O Enter do campo: corta o bloco no cursor e abre o de baixo (a lista continua no mesmo
    /// tipo). Num item de lista ou numa citação vazia, o Enter volta o bloco a texto.
    fn enter_na_pagina(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, _, texto, cursor)) = self.campo_atual(cx) else { return };
        let novo = {
            let mut pagina = self.pagina.borrow_mut();
            let Some(p) = pagina.as_mut() else { return };
            let Some(i) = p.doc.indice(id) else { return };
            let tipo = p.doc.blocos[i].tipo;
            if (tipo.de_lista() || tipo == Tipo::Citacao) && texto.trim().is_empty() {
                p.doc.blocos[i].tipo = Tipo::Texto;
                p.doc.blocos[i].feito = false;
                None
            } else {
                let cursor = cursor.min(texto.len());
                p.doc.blocos[i].texto = texto[..cursor].to_string();
                let tipo_novo = if tipo.de_lista() { tipo } else { Tipo::Texto };
                let mut b = p.doc.bloco(tipo_novo, texto[cursor..].to_string());
                b.recuo = p.doc.blocos[i].recuo.clone();
                let novo = b.id;
                p.doc.blocos.insert(i + 1, b);
                Some(novo)
            }
        };
        match novo {
            Some(novo) => self.focar_bloco(novo, 0, window, cx),
            None => self.focar_bloco(id, 0, window, cx),
        }
        self.pagina_mudou(window, cx);
    }

    /// Backspace no começo do bloco: o tipo volta a texto; o texto junta com o bloco de cima.
    fn backspace_na_pagina(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, campo, texto, cursor)) = self.campo_atual(cx) else { return cx.propagate() };
        if cursor != 0 || !campo.read(cx).selected_range().is_empty() {
            return cx.propagate();
        }
        enum Feito {
            Texto,
            Juntou(u64, usize),
            Removeu(Option<u64>),
            Nada,
        }
        let feito = {
            let mut pagina = self.pagina.borrow_mut();
            let Some(p) = pagina.as_mut() else { return cx.propagate() };
            let Some(i) = p.doc.indice(id) else { return cx.propagate() };
            if p.doc.blocos[i].tipo != Tipo::Texto {
                p.doc.blocos[i].tipo = Tipo::Texto;
                p.doc.blocos[i].feito = false;
                Feito::Texto
            } else if i == 0 {
                Feito::Nada
            } else {
                match p.doc.blocos[i - 1].tipo {
                    Tipo::Divisor => {
                        p.doc.blocos.remove(i - 1);
                        Feito::Removeu(Some(id))
                    }
                    Tipo::Codigo | Tipo::Tabela => {
                        if texto.is_empty() {
                            p.doc.blocos.remove(i);
                            Feito::Removeu(p.doc.blocos.get(i - 1).filter(|b| b.tipo.editavel()).map(|b| b.id))
                        } else {
                            Feito::Nada
                        }
                    }
                    _ => {
                        let de_cima = &mut p.doc.blocos[i - 1];
                        let lugar = de_cima.texto.len();
                        de_cima.texto.push_str(&texto);
                        let alvo = de_cima.id;
                        p.doc.blocos.remove(i);
                        Feito::Juntou(alvo, lugar)
                    }
                }
            }
        };
        match feito {
            Feito::Nada => return,
            Feito::Texto => self.focar_bloco(id, 0, window, cx),
            Feito::Juntou(alvo, lugar) => self.focar_bloco(alvo, lugar, window, cx),
            Feito::Removeu(Some(alvo)) if alvo == id => self.focar_bloco(id, 0, window, cx),
            Feito::Removeu(Some(alvo)) => self.focar_bloco(alvo, usize::MAX, window, cx),
            Feito::Removeu(None) => {
                if let Some(p) = self.pagina.borrow_mut().as_mut() {
                    p.foco = None;
                }
            }
        }
        self.pagina_mudou(window, cx);
    }

    /// As setas: no menu, andam nele; na primeira (ou última) linha do bloco, passam ao bloco de
    /// cima (ou de baixo). O campo anda primeiro; se o cursor não saiu do lugar, era a ponta.
    fn seta_na_pagina(&mut self, cima: bool, window: &mut Window, cx: &mut Context<Self>) {
        let com_menu = {
            let mut pagina = self.pagina.borrow_mut();
            match pagina.as_mut().and_then(|p| p.menu.as_mut()) {
                Some(m) => {
                    let n = pagina_md::tipos_do_filtro(&m.filtro).len().max(1);
                    m.cursor = if cima { (m.cursor + n - 1) % n } else { (m.cursor + 1) % n };
                    true
                }
                None => false,
            }
        };
        if com_menu {
            cx.notify();
            return;
        }
        let Some((id, campo, texto, antes)) = self.campo_atual(cx) else { return cx.propagate() };
        cx.propagate();
        cx.defer_in(window, move |this, window, cx| {
            let agora = campo.read(cx).cursor();
            let na_ponta = agora == antes && (if cima { !texto[..antes.min(texto.len())].contains('\n') } else { !texto[antes.min(texto.len())..].contains('\n') });
            if !na_ponta {
                return;
            }
            let vizinho = this.pagina.borrow().as_ref().and_then(|p| {
                let i = p.doc.indice(id)?;
                let mut alcance: Box<dyn Iterator<Item = usize>> = if cima { Box::new((0..i).rev()) } else { Box::new(i + 1..p.doc.blocos.len()) };
                alcance.find(|&k| p.doc.blocos[k].tipo.editavel()).map(|k| p.doc.blocos[k].id)
            });
            if let Some(v) = vizinho {
                this.focar_bloco(v, if cima { usize::MAX } else { 0 }, window, cx);
            }
        });
    }

    /// Esc: fecha o menu; sem ele, sai da edição (o foco volta ao painel).
    fn esc_na_pagina(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut pagina = self.pagina.borrow_mut();
        let Some(p) = pagina.as_mut() else { return };
        if p.menu.take().is_none() {
            if p.foco.as_ref().is_some_and(|f| p.doc.indice(f.id).is_some_and(|i| p.doc.blocos[i].tipo == Tipo::Codigo)) {
                drop(pagina);
                self.sair_do_codigo(window, cx);
                return;
            }
            p.foco = None;
            drop(pagina);
            window.focus(&self.foco_arquivo, cx);
        }
        cx.notify();
    }

    /// Tab: no menu, escolhe; no código, segue para o campo (o recuo).
    fn tab_na_pagina(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let com_menu = self.pagina.borrow().as_ref().is_some_and(|p| p.menu.is_some());
        if com_menu {
            self.enter_capturado(false, window, cx);
        } else {
            cx.propagate();
        }
    }

    /// Sai do bloco de código para o de baixo (um texto novo, se não houver).
    fn sair_do_codigo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let alvo = {
            let mut pagina = self.pagina.borrow_mut();
            let Some(p) = pagina.as_mut() else { return };
            let Some(id) = p.foco.as_ref().map(|f| f.id) else { return };
            let Some(i) = p.doc.indice(id) else { return };
            match p.doc.blocos.get(i + 1).filter(|b| b.tipo.editavel()) {
                Some(b) => b.id,
                None => {
                    let b = p.doc.bloco(Tipo::Texto, "");
                    let novo = b.id;
                    p.doc.blocos.insert(i + 1, b);
                    novo
                }
            }
        };
        self.focar_bloco(alvo, 0, window, cx);
    }

    /// Escolheu um tipo no menu do "/": a barra e o filtro saem do texto e o bloco vira o tipo.
    fn escolher_tipo(&mut self, tipo: Tipo, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, _, texto, cursor)) = self.campo_atual(cx) else { return };
        let fim_do_texto = {
            let mut pagina = self.pagina.borrow_mut();
            let Some(p) = pagina.as_mut() else { return };
            let apagar = p.menu.take().map_or(0, |m| m.apagar);
            let Some(i) = p.doc.indice(id) else { return };
            let cursor = cursor.min(texto.len());
            let inicio = texto[..cursor].char_indices().rev().nth(apagar.saturating_sub(1)).map_or(0, |(k, _)| k);
            let novo = format!("{}{}", &texto[..inicio], &texto[cursor..]);
            let b = &mut p.doc.blocos[i];
            b.texto = novo;
            b.tipo = tipo;
            b.feito = false;
            inicio
        };
        if tipo == Tipo::Divisor {
            self.divisor_no_lugar(id, window, cx);
        } else {
            self.focar_bloco(id, fim_do_texto, window, cx);
        }
        self.pagina_mudou(window, cx);
    }

    /// O "+" da alça: um texto novo embaixo, já com o "/" e o menu aberto.
    fn bloco_abaixo(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let novo = {
            let mut pagina = self.pagina.borrow_mut();
            let Some(p) = pagina.as_mut() else { return };
            let Some(i) = p.doc.indice(id) else { return };
            let b = p.doc.bloco(Tipo::Texto, "/");
            let novo = b.id;
            p.doc.blocos.insert(i + 1, b);
            novo
        };
        self.focar_bloco(novo, 1, window, cx);
        if let Some(p) = self.pagina.borrow_mut().as_mut() {
            p.menu = Some(MenuDaBarra { filtro: String::new(), apagar: 1, cursor: 0 });
        }
        cx.notify();
    }

    /// O clique no fim da página: o último bloco vazio, ou um texto novo no fim.
    fn clicar_no_fim_da_pagina(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let alvo = {
            let mut pagina = self.pagina.borrow_mut();
            let Some(p) = pagina.as_mut() else { return };
            match p.doc.blocos.last() {
                Some(b) if b.tipo == Tipo::Texto && b.texto.is_empty() => b.id,
                _ => {
                    let b = p.doc.bloco(Tipo::Texto, "");
                    let novo = b.id;
                    p.doc.blocos.push(b);
                    novo
                }
            }
        };
        self.focar_bloco(alvo, 0, window, cx);
    }

    /// A caixinha da tarefa.
    fn alternar_tarefa(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = self.pagina.borrow_mut().as_mut()
            && let Some(i) = p.doc.indice(id)
        {
            p.doc.blocos[i].feito = !p.doc.blocos[i].feito;
        }
        self.pagina_mudou(window, cx);
    }

    /// O bloco arrastado pela alça entra antes de `antes_de` (ou no fim).
    fn mover_bloco(&mut self, id: u64, antes_de: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        if antes_de == Some(id) {
            return;
        }
        {
            let mut pagina = self.pagina.borrow_mut();
            let Some(p) = pagina.as_mut() else { return };
            let Some(de) = p.doc.indice(id) else { return };
            let bloco = p.doc.blocos.remove(de);
            let para = antes_de.and_then(|a| p.doc.indice(a)).unwrap_or(p.doc.blocos.len());
            p.doc.blocos.insert(para, bloco);
        }
        self.pagina_mudou(window, cx);
    }

    /// Ctrl+B, Ctrl+I, Ctrl+E: a marca em volta da seleção (ou tirada, se já está).
    fn marcar_na_pagina(&mut self, marca: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.tipo_em_edicao() == Some(Tipo::Codigo) {
            return cx.propagate();
        }
        let Some((_, campo, texto, _)) = self.campo_atual(cx) else { return };
        let faixa = campo.read(cx).selected_range();
        let (novo, selecao) = pagina_md::marcar(&texto, faixa.start.min(texto.len()), faixa.end.min(texto.len()), marca);
        campo.update(cx, |c, cx| {
            c.set_value(novo, window, cx);
            c.set_selected_range(selecao, cx);
        });
        self.campo_da_pagina_mudou(window, cx);
    }
}

/// A tabela do .md, para ler (a edição dela é pela Fonte).
fn tabela(linhas: &[Vec<String>]) -> Div {
    let colunas = linhas.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let celula = |texto: &str, cabeca: bool| {
        let lido = pagina_md::ler_linha(texto);
        div()
            .pr(crate::tema::pagina::COLUNA)
            .py(if cabeca { crate::tema::pagina::CELULA_CABECA } else { esp::E6 })
            .border_b_1()
            .border_color(Cores::forte().opacity(if cabeca { 0.08 } else { 0.04 }))
            .text_color(if cabeca { Cores::tenue() } else { Cores::texto_da_fala() })
            .when(cabeca, |s| s.font_weight(FontWeight::SEMIBOLD))
            .child(lido.texto)
    };
    div()
        .flex_1()
        .mx(crate::tema::pagina::CAMPO_X)
        .mt(esp::E4)
        .mb(esp::E12)
        .grid()
        .grid_cols(colunas as u16)
        .text_size(r(12.5))
        .line_height(r(18.))
        .children(linhas.iter().enumerate().flat_map(|(n, fileira)| {
            (0..colunas).map(move |c| celula(fileira.get(c).map_or("", String::as_str), n == 0)).collect::<Vec<_>>()
        }))
}
