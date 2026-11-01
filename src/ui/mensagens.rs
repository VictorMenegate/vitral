//! As mensagens de uma conversa: texto em markdown, ferramentas numa trilha calma, eventos das
//! tarefas, entregas de outros chats, pedidos de permissão e plano, a faixa do turno e a rolagem
//! de cada painel.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, StyledExt as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    clipboard::Clipboard,
    h_flex,
    input::{Enter, Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::ScrollableElement as _,
    shimmer::ShimmerText,
    spinner::Spinner,
    text::{TextView, TextViewStyle},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    IrAoFim, Janela,
    componentes::{
        Alerta, BotaoCopiar, CabecalhoDeSecao, CaixaDeCodigo, Chip, Confirmacao, EstadoVazio, FaixaCaixa, Flutuante as _,
        LarguraPainel, Papel as Quem, PilulaDeEstado, Reflexo as _, Selo, TamanhoBotao, TamanhoChip, Tom,
        TomConfirmacao, aviso, aviso_erro, botao_fantasma, botao_icone, botao_perigo, botao_primario, confirmar,
        EstadoVisual, contagem, estado_visual, icone_do_modo, rotulo_tarefa, teclas, texto_arco_iris,
    },
    composer::{self, PararTurno, nome_do_modo},
    duracao,
    equipe::variante_tinta,
    movimento::{self, De},
    paineis::tamanho_da_conversa,
};
use crate::{
    modelo::{Chat, EstadoTarefa, Execucao, Item, Papel, Reforco, Tarefa},
    motor::Motor,
    tema::{
        Cores, FONTE_MONO, Lucide, Tipo as _, dentro_da_borda, esp, gradiente_destaque, halo_foco, r, raio,
        sombra_elevado, tam,
    },
};

/// O logo pequeno, para as mensagens que o próprio Vitral entrega.
static LOGO: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../assets/vitral-pequeno.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});

/// Ícone no tamanho de desenho (cresce com o zoom), sem precisar da janela.
fn icone_de(nome: impl Into<Icon>, lado: f32) -> Icon {
    nome.into().size(r(lado)).flex_none()
}

/// Botão fantasma com texto; o clique não chega à linha de baixo (que abre e fecha a mensagem).
fn fantasma(
    id: impl Into<ElementId>,
    icone: Option<Lucide>,
    rotulo: impl Into<SharedString>,
    t: TamanhoBotao,
    f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    window: &Window,
    cx: &App,
) -> Button {
    botao_fantasma(id, icone.map(Icon::from), rotulo, t, window, cx).on_click(move |e, window, cx| {
        cx.stop_propagation();
        f(e, window, cx)
    })
}

/// Botão só com ícone (a dica diz o que ele faz).
fn botao_so_icone(
    id: impl Into<ElementId>,
    icone: Lucide,
    dica: impl Into<SharedString>,
    t: TamanhoBotao,
    f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    window: &Window,
    cx: &App,
) -> Button {
    botao_icone(id, icone, dica, t, window, cx).on_click(move |e, window, cx| {
        cx.stop_propagation();
        f(e, window, cx)
    })
}

/// Largo = Larga, Médio = Estreita, Estreito e Mínimo = Mínima (seção 2.5 do plano).
fn larga(nivel: LarguraPainel) -> bool {
    nivel == LarguraPainel::Largo
}

fn minima(nivel: LarguraPainel) -> bool {
    matches!(nivel, LarguraPainel::Estreito | LarguraPainel::Minimo)
}

/// Fundos da linha que sai e da que entra num trecho de código.
fn fundo_sai() -> Hsla {
    rgba(0xf8717114).into()
}

fn fundo_entra() -> Hsla {
    rgba(0x4ade8014).into()
}

/// O tipo de cada bloco da conversa, para o espaço entre eles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bloco {
    Balao,
    Texto,
    /// Trilha de ferramentas, eventos de tarefa e avisos de uma linha.
    Log,
    Recebida,
    Marco,
    Alerta,
    Resumo,
    /// Cartão de entrega.
    Cartao,
}

/// Espaço antes de um bloco: 24 antes do seu balão (entre turnos), 16 depois dele, 12 dentro do
/// turno, 16 em volta de marco e resumo e nada entre linhas de log, que ficam coladas no passo
/// de 28. Nada fica reservado para as ações: elas flutuam no hover.
fn espaco_antes(anterior: Option<Bloco>, atual: Bloco) -> f32 {
    use Bloco::*;
    let Some(anterior) = anterior else {
        return 0.;
    };
    match (anterior, atual) {
        (_, Balao) => 24.,
        (Balao, _) => 16.,
        (Marco | Resumo, _) | (_, Marco | Resumo) => 16.,
        (Log, Log) => 0.,
        (Recebida, Log | Recebida) => 4.,
        _ => 12.,
    }
}

/// A entrada de um bloco da conversa (E5): nenhuma para o que já estava nela quando abriu no
/// painel (`base`) ou para o que não tem índice; para o resto, `Some(nova)`, com `nova` só no
/// quadro em que o bloco aparece (`vistos`). Trocar de conversa ou rolar não anima nada.
fn entrada_do_bloco(indice: Option<usize>, base: usize, vistos: usize) -> Option<bool> {
    indice.filter(|i| *i >= base).map(|i| i >= vistos)
}

impl Janela {
    pub(super) fn mensagens(
        &mut self,
        motor: &Motor,
        chat: &Chat,
        ix: usize,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let painel_id = self.paineis[ix].id;
        let rolagem = self.paineis[ix].rolagem.clone();
        let foco = self.paineis[ix].modo_foco;
        let nivel = self.paineis[ix].medida.nivel;
        let resumo = indice_do_resumo(motor, chat);
        let contexto = Contexto::novo(motor, chat, nivel);
        let finais = finais_de_turno(&chat.itens);
        // Cada bloco com o índice do (primeiro) item dele, que identifica a entrada.
        let mut blocos: Vec<(Bloco, Option<usize>, AnyElement)> = Vec::new();
        if chat.itens.is_empty() {
            blocos.push((Bloco::Texto, None, self.dica_inicial(chat, painel_id, window, cx)));
        }
        // Conversas antigas guardaram as buscas de ferramenta; não interessam. O modo foco mantém
        // só as ações sobre tarefas.
        let visiveis: Vec<(usize, &Item)> = chat
            .itens
            .iter()
            .enumerate()
            .filter(|(_, item)| match item {
                Item::Ferramenta { nome, .. } => {
                    nome != "ToolSearch" && (!foco || acao_de_tarefa(nome).is_some())
                }
                _ => true,
            })
            .collect();
        let mut i = 0;
        while i < visiveis.len() {
            let (indice, item) = visiveis[i];
            if matches!(item, Item::Ferramenta { .. }) {
                let mut fim = i + 1;
                while fim < visiveis.len() && matches!(visiveis[fim].1, Item::Ferramenta { .. }) {
                    fim += 1;
                }
                let ao_vivo = contexto.rodando && fim == visiveis.len();
                let elemento = self.ferramentas(&contexto, &visiveis[i..fim], ao_vivo, window, cx);
                blocos.push((Bloco::Log, Some(indice), elemento));
                i = fim;
                continue;
            }
            i += 1;
            if Some(indice) == resumo
                && let Item::Assistente { texto } = item
            {
                blocos.push((Bloco::Resumo, Some(indice), cartao_resumo(motor, chat, indice, texto, cx)));
                continue;
            }
            let (bloco, elemento) = self.item(&contexto, indice, item, finais.contains(&indice), window, cx);
            blocos.push((bloco, Some(indice), elemento));
        }
        let parcial = motor.parcial(&chat.id);
        if let Some(parcial) = parcial {
            // O mesmo id do texto que vai pousar neste índice: o documento já lido continua, e a
            // entrada não se repete quando ele pousa.
            let indice = chat.itens.len();
            let id = SharedString::from(format!("{}:{indice}", chat.id));
            blocos.push((
                Bloco::Texto,
                Some(indice),
                div()
                    .corpo()
                    .text_color(Cores::texto())
                    .child(markdown(id, parcial.to_string(), cx))
                    .into_any_element(),
            ));
        }
        let (base, vistos) = (self.paineis[ix].base_animacao, self.paineis[ix].vistos);
        let mut lista = v_flex()
            .map(coluna)
            .when(minima(nivel), |s| s.px(esp::E12))
            .pt(esp::E12)
            .pb(esp::E12);
        let mut anterior = None;
        for (bloco, indice, elemento) in blocos {
            let caixa = div().mt(r(espaco_antes(anterior, bloco))).child(elemento);
            lista = match (indice, entrada_do_bloco(indice, base, vistos)) {
                (Some(i), Some(nova)) => lista.child(movimento::entrada(("entra", i), nova, De::Baixo, caixa)),
                _ => lista.child(caixa),
            };
            anterior = Some(bloco);
        }

        // `seguindo` já foi conferido no começo do desenho (`acompanhar_rolagem`). No fim, o
        // painel volta a ele a cada desenho: com a largura nova (abrir ao lado, Ctrl+B, zoom), as
        // linhas quebram de novo e o conteúdo cresce sem o deslocamento mudar. A marca só vale no
        // prepaint, depois do layout.
        let tamanho = (chat.itens.len(), parcial.map_or(0, str::len));
        let painel = &mut self.paineis[ix];
        painel.vistos = painel.vistos.max(tamanho_da_conversa(chat.itens.len(), parcial.is_some()));
        if painel.seguindo {
            painel.leitura.vista = tamanho;
            if std::mem::take(&mut painel.manter_lugar) {
                // Abriu um bloco (o relatório, uma saída): o painel fica onde está, e a próxima
                // conferência, já com o layout novo, decide pela folga de 48 se ele continua no
                // fim ou se a pílula aparece.
                painel.leitura.deslocamento = px(f32::INFINITY);
            } else {
                painel.rolagem.scroll_to_bottom();
            }
        }
        let (seguindo, novidade) = (painel.seguindo, painel.leitura.vista != tamanho);
        let pilula_nova = !seguindo && !painel.pilula_vista;
        painel.pilula_vista = !seguindo;

        div()
            .relative()
            .flex_1()
            // Em janela baixa, o plano e os pedidos de permissão encolhem e rolam antes: as
            // mensagens nunca somem de todo.
            .min_h(MINIMO_MENSAGENS)
            .w_full()
            .child(
                div()
                    .id(SharedString::from(format!("mensagens-{painel_id}")))
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&rolagem)
                    .on_scroll(cx.listener(|this, _: &ScrollEvent, _, cx| {
                        // A roda já mexeu no deslocamento: decide agora, antes que o motor peça
                        // para voltar ao fim.
                        this.acompanhar_rolagem(cx);
                        cx.notify();
                    }))
                    .child(lista)
                    .vertical_scrollbar(&rolagem),
            )
            .when(!seguindo, |s| s.child(pilula_do_fim(painel_id, novidade, minima(nivel), pilula_nova, cx)))
    }

    /// Conversa vazia: o coordenador convida a escrever (com duas sugestões que só preenchem a
    /// caixa); o trabalhador espera a tarefa.
    fn dica_inicial(&self, chat: &Chat, painel_id: u64, window: &Window, cx: &Context<Self>) -> AnyElement {
        let janela = cx.entity().downgrade();
        match chat.papel {
            Papel::Coordenador => {
                let sugestao = |i: usize, texto: &'static str| {
                    let janela = janela.clone();
                    Chip::new(("sugestao", i), texto)
                        .tamanho(TamanhoChip::Medio)
                        .construir(window, cx)
                        .on_click(move |_, window, cx| {
                            let _ = janela.update(cx, |this, cx| this.escrever_na_caixa(painel_id, texto, window, cx));
                        })
                };
                EstadoVazio::new(Lucide::Bot, "Diga o que você quer fazer")
                    .texto(
                        "O coordenador planeja, distribui tarefas para trabalhadores (cada um numa cópia \
                         isolada do projeto), revisa e pede ajustes até ficar bom, e no fim te manda um \
                         resumo.",
                    )
                    .amplo()
                    .acao(sugestao(0, "Explique como este projeto está organizado."))
                    .acao(sugestao(1, "Quero planejar uma mudança: "))
                    .into_any_element()
            }
            Papel::Trabalhador => {
                let pai = chat.pai.clone();
                let abrir = pai.map(|pai| {
                    fantasma(
                        "abrir-coordenador",
                        Some(Lucide::CornerLeftUp),
                        "Abrir o coordenador",
                        TamanhoBotao::Controle,
                        abrir_chat(pai, cx),
                        window,
                        cx,
                    )
                });
                EstadoVazio::new(Lucide::Wrench, "Ainda sem tarefa")
                    .texto("Esta conversa recebe as tarefas do coordenador.")
                    .when_some(abrir, |e, abrir| e.acao(abrir))
                    .into_any_element()
            }
        }
    }

    /// Escreve na caixa do painel e põe o foco nela (quem envia é você, com Enter).
    fn escrever_na_caixa(&mut self, painel: u64, texto: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.paineis.iter().find(|p| p.id == painel).map(|p| p.composer.clone()) else {
            return;
        };
        composer.update(cx, |c, cx| c.set_value(texto.to_string(), window, cx));
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    /// Um item da conversa e o tipo de bloco dele. `com_acoes`: o texto é o último do turno e
    /// ganha o Copiar no hover.
    fn item(
        &self,
        contexto: &Contexto,
        indice: usize,
        item: &Item,
        com_acoes: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> (Bloco, AnyElement) {
        let chave = format!("{}:{indice}", contexto.chat.id);
        match item {
            Item::Usuario {
                texto,
                anexos,
                ponto,
                ultrathink,
                ultracode,
            } => {
                // Sem a cópia da tarefa, não há arquivos para voltar.
                let ponto = ponto.clone().filter(|_| !contexto.sem_copia);
                let reforco = Reforco { ultrathink: *ultrathink, ultracode: *ultracode };
                (Bloco::Balao, self.balao_usuario(contexto, &chave, texto, anexos, ponto, reforco, window, cx))
            }
            Item::Assistente { texto } => (Bloco::Texto, self.texto_assistente(&chave, texto, com_acoes, cx)),
            Item::Entrega { de, texto, ponto } => match contexto.tarefa(item) {
                Some(tarefa) => self.entrega_de_tarefa(contexto, indice, item, tarefa, window, cx),
                None => {
                    let ponto = ponto.clone().filter(|_| !contexto.sem_copia);
                    (Bloco::Recebida, self.mensagem_recebida(contexto, &chave, de, texto, ponto, window, cx))
                }
            },
            Item::Ferramenta { .. } => (Bloco::Log, self.ferramentas(contexto, &[(indice, item)], false, window, cx)),
            Item::Aviso { texto, erro } => aviso_do_chat(&chave, texto, *erro),
        }
    }

    /// O que você escreveu: balão neutro, do material da caixa, com o reflexo do vidro, alinhado à
    /// direita. As ações flutuam à esquerda dele, na altura da última linha, só no hover (na
    /// largura mínima, no canto de cima).
    #[allow(clippy::too_many_arguments)]
    fn balao_usuario(
        &self,
        contexto: &Contexto,
        chave: &str,
        texto: &str,
        anexos: &[PathBuf],
        ponto: Option<String>,
        reforco: Reforco,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let nivel = contexto.nivel;
        let largura = if larga(nivel) {
            0.80
        } else if minima(nivel) {
            0.92
        } else {
            0.88
        };
        let grupo = SharedString::from(format!("msg-{chave}"));
        let linhas = texto.lines().count();
        let longo = linhas > 14;
        let chave_tudo = format!("{chave}:tudo");
        let tudo = self.expandidos.contains(&chave_tudo);
        let corpo = match comando_de_barra(texto).filter(|_| anexos.is_empty()) {
            Some(comando) => div()
                .h(r(28.))
                .px(esp::E12)
                .flex()
                .items_center()
                .rounded(raio::CONTROLE)
                .bg(Cores::preto())
                .mono()
                .text_color(Cores::texto())
                .child(comando.to_string())
                .into_any_element(),
            None => {
                let mostrar = longo.then(|| {
                    let rotulo = if tudo {
                        "Mostrar menos".to_string()
                    } else {
                        format!("Mostrar tudo ({linhas} linhas)")
                    };
                    let id = SharedString::from(format!("tudo-{chave}"));
                    fantasma(id, None, rotulo, TamanhoBotao::Linha, alternar(chave_tudo, cx), window, cx)
                });
                v_flex()
                    .px(dentro_da_borda(12.))
                    .py(dentro_da_borda(8.))
                    .gap(esp::E8)
                    .rounded(raio::CARTAO)
                    .bg(Cores::cartao_elevado())
                    .border_1()
                    .border_color(Cores::linha())
                    .reflexo(raio::CARTAO)
                    .child(
                        div().corpo().text_color(Cores::texto()).child(
                            markdown(SharedString::from(chave.to_string()), texto.to_string(), cx)
                                .when(longo && !tudo, |t| t.max_lines(12)),
                        ),
                    )
                    .when_some(mostrar, |s, b| s.child(h_flex().child(b)))
                    .when(!anexos.is_empty(), |s| {
                        s.child(
                            h_flex()
                                .flex_wrap()
                                .gap(esp::E6)
                                .children(anexos.iter().enumerate().map(|(i, a)| anexo(chave, i, a))),
                        )
                    })
                    .when(reforco != Reforco::default(), |s| {
                        s.child(
                            h_flex()
                                .justify_end()
                                .gap(esp::E6)
                                .when(reforco.ultrathink, |s| s.child(selo_ultrathink()))
                                .when(reforco.ultracode, |s| s.child(selo_ultracode())),
                        )
                    })
                    .into_any_element()
            }
        };
        let mut acoes: Vec<AnyElement> = Vec::new();
        if let Some(ponto) = ponto {
            acoes.push(botao_voltar(chave, &contexto.chat.id, ponto, window, cx).into_any_element());
        }
        acoes.push(copiar(chave, texto).into_any_element());
        // A largura máxima fica no invólucro: o balão encolhe ao texto e a barra se mede por ele.
        // Na largura mínima não sobra lugar à esquerda: a barra vai para o canto de cima.
        let barra = div().absolute().map(|s| {
            if minima(nivel) { s.top(r(-14.)).right_0() } else { s.bottom_0().right(relative(1.)).pr(esp::E8) }
        });
        v_flex()
            .w_full()
            .items_end()
            .group(grupo.clone())
            .child(
                div()
                    .relative()
                    .max_w(relative(largura))
                    .child(corpo)
                    .child(barra.child(barra_de_acoes(grupo, acoes, cx))),
            )
            .into_any_element()
    }

    /// Resposta do assistente: sem balão, na largura da coluna. O último texto de cada turno ganha
    /// o Copiar numa barra que flutua no canto de cima à direita, só no hover.
    fn texto_assistente(&self, chave: &str, texto: &str, com_acoes: bool, cx: &Context<Self>) -> AnyElement {
        let grupo = SharedString::from(format!("msg-{chave}"));
        v_flex()
            .relative()
            .w_full()
            .group(grupo.clone())
            .child(
                div()
                    .corpo()
                    .text_color(Cores::texto())
                    .child(markdown(SharedString::from(chave.to_string()), texto.to_string(), cx)),
            )
            .when(com_acoes, |s| {
                s.child(
                    div()
                        .absolute()
                        .top(r(-14.))
                        .right_0()
                        .child(barra_de_acoes(grupo, vec![copiar(chave, texto).into_any_element()], cx)),
                )
            })
            .into_any_element()
    }

    /// Mensagem que outro chat (ou o Vitral) entregou: quem mandou e uma prévia; a linha do nome
    /// abre e fecha o texto inteiro.
    fn mensagem_recebida(
        &self,
        contexto: &Contexto,
        chave: &str,
        de: &str,
        texto: &str,
        ponto: Option<String>,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let nivel = contexto.nivel;
        let aberto = self.expandidos.contains(chave);
        let grupo = SharedString::from(format!("msg-{chave}"));
        let (quem, nome) = remetente(contexto.motor, de);
        let avatar = match quem {
            Some(papel) => papel.circulo(24.).into_any_element(),
            None => img(LOGO.clone()).flex_none().size(r(20.)).into_any_element(),
        };
        let mut acoes: Vec<AnyElement> = Vec::new();
        if let Some(ponto) = ponto {
            acoes.push(botao_voltar(chave, &contexto.chat.id, ponto, window, cx).into_any_element());
        }
        acoes.push(copiar(chave, texto).into_any_element());
        let cabecalho = h_flex()
            .id(SharedString::from(format!("recebida-{chave}")))
            .h(r(28.))
            .gap(r(8.))
            .rounded(raio::CONTROLE)
            .cursor_pointer()
            .child(div().flex_none().size(r(24.)).flex().items_center().justify_center().child(avatar))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(r(13.))
                    .line_height(r(18.))
                    .font_semibold()
                    .text_color(Cores::texto())
                    .child(nome),
            )
            .child(div().flex_1())
            .child(lugar_de_acoes(grupo.clone(), acoes, Some(aberto)))
            .tab_index(0)
            .focus_visible(|s| s.shadow(halo_foco()))
            .on_click(alternar(chave.to_string(), cx));
        let corpo = if aberto {
            div()
                .pl(r(32.))
                .corpo()
                .text_color(Cores::texto())
                .child(markdown(SharedString::from(format!("{chave}-t")), texto.to_string(), cx))
        } else {
            // A prévia termina com reticências na coluna da seta, nunca cortada no meio da letra.
            div()
                .min_w_0()
                .pl(r(32.))
                .pr(r(32.))
                .corpo_ui()
                .text_color(Cores::tenue())
                .line_clamp(if larga(nivel) { 2 } else { 1 })
                .text_ellipsis()
                .child(previa(texto))
        };
        v_flex().w_full().group(grupo).child(cabecalho).child(corpo).into_any_element()
    }
}

/// A margem direita do código guarda a coluna do Copiar (24, com 8 de cada lado): ele nunca fica
/// por cima de um caractere.
const COLUNA_DO_COPIAR: Rems = r(40.);

/// Markdown com o estilo das mensagens; os blocos de código ganham o Copiar, com a linguagem na
/// dica.
fn markdown(id: SharedString, texto: String, cx: &App) -> TextView {
    TextView::markdown(id, texto)
        .style(estilo_texto(cx))
        .blockquote_style(
            StyleRefinement::default()
                .border_l_2()
                .border_color(Cores::borda_forte())
                .pl(esp::E12)
                .text_color(Cores::tenue()),
        )
        .selectable(true)
        .code_block_actions(move |bloco, _, _| {
            let dica = match bloco.lang().filter(|l| !l.is_empty()) {
                Some(l) => format!("Copiar código {l}"),
                None => "Copiar código".to_string(),
            };
            BotaoCopiar::new(Clipboard::new("copiar").value(bloco.code()).tooltip(dica))
        })
}

/// O texto das mensagens: parágrafos a 12, títulos de 18, 16 e 15, código sem o azul do kit,
/// com o conteúdo a 12 da borda e a coluna do Copiar à direita.
fn estilo_texto(cx: &App) -> TextViewStyle {
    // O rem do tema já vem com o zoom: os títulos crescem junto.
    let rem = cx.theme().font_size;
    TextViewStyle::default()
        .paragraph_gap(esp::E12)
        .heading_font_size(move |nivel, _| {
            rem * match nivel {
                1 => 18. / 15.,
                2 => 16. / 15.,
                _ => 1.,
            }
        })
        .code_block(
            StyleRefinement::default()
                .p(dentro_da_borda(12.))
                .pr(COLUNA_DO_COPIAR)
                .bg(Cores::preto())
                .border_color(Cores::borda())
                .rounded(raio::CARTAO),
        )
        .inline_code(HighlightStyle {
            color: Some(Cores::codigo()),
            background_color: Some(Cores::hover()),
            ..Default::default()
        })
        .table(StyleRefinement::default().rounded(raio::CARTAO).border_color(Cores::borda()))
}

/// As ações de uma mensagem numa barra flutuante (28 em pílula, botões de 24): aparece seca no
/// hover da mensagem, sem reservar altura. Quem chama a posiciona. Um fade aqui viraria animação
/// em cadeia ao passar o ponteiro pela conversa.
fn barra_de_acoes(grupo: SharedString, acoes: Vec<AnyElement>, cx: &App) -> Div {
    h_flex()
        .h(r(28.))
        .p(esp::E2)
        .gap(esp::E2)
        .flutuante(cx)
        .rounded(raio::PILULA)
        .invisible()
        .group_hover(grupo, |s| s.visible())
        .children(acoes)
}

/// À direita de uma linha: a seta de abrir e, com o ponteiro em cima, as ações no lugar dela.
/// Em repouso só a coluna da seta fica reservada, e a pílula encosta nela; no hover, a linha sob
/// o ponteiro abre espaço para as ações.
fn lugar_de_acoes(grupo: SharedString, acoes: Vec<AnyElement>, aberto: Option<bool>) -> Div {
    let n = acoes.len() as f32;
    let largura = (n * 24. + (n - 1.).max(0.) * 4.).max(24.);
    let tem_acoes = !acoes.is_empty();
    div()
        .relative()
        .flex_none()
        .w(r(24.))
        .group_hover(grupo.clone(), |s| s.w(r(largura)))
        .h(r(24.))
        .when_some(aberto, |s, aberto| {
            s.child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .size(r(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(Cores::tenue())
                    .when(tem_acoes, |s| s.group_hover(grupo.clone(), |s| s.invisible()))
                    .child(icone_de(if aberto { Lucide::ChevronDown } else { Lucide::ChevronRight }, 12.)),
            )
        })
        .child(
            h_flex()
                .absolute()
                .top_0()
                .right_0()
                .h(r(24.))
                .gap(r(4.))
                .invisible()
                .group_hover(grupo, |s| s.visible())
                .children(acoes),
        )
}

fn copiar(chave: &str, texto: &str) -> BotaoCopiar {
    BotaoCopiar::new(
        Clipboard::new(SharedString::from(format!("copiar-{chave}")))
            .value(SharedString::from(texto.to_string()))
            .tooltip("Copiar"),
    )
}

/// Volta os arquivos a antes da mensagem, depois de confirmar com o botão vermelho.
fn botao_voltar(chave: &str, chat: &str, ponto: String, window: &Window, cx: &Context<Janela>) -> Button {
    let janela = cx.entity().downgrade();
    let chat = chat.to_string();
    botao_so_icone(
        SharedString::from(format!("voltar-{chave}")),
        Lucide::Undo2,
        "Voltar os arquivos a antes desta mensagem",
        TamanhoBotao::Linha,
        move |_, window, cx| {
            let (janela, chat, ponto) = (janela.clone(), chat.clone(), ponto.clone());
            let pergunta = Confirmacao {
                tom: TomConfirmacao::Perigo,
                icone: Lucide::Undo2.into(),
                titulo: "Voltar os arquivos a este ponto?".into(),
                texto: "Os arquivos da pasta desta conversa voltam a como estavam antes desta mensagem; o \
                        que foi criado depois é apagado. As mensagens e os commits não mudam."
                    .into(),
                detalhes: Vec::new(),
                ok: "Voltar os arquivos".into(),
                cancelar: "Cancelar".into(),
                ok_perigo: true,
            };
            confirmar(
                pergunta,
                move |window, cx| {
                    let (chat, ponto) = (chat.clone(), ponto.clone());
                    let _ = janela.update(cx, |this, cx| this.voltar_ao_ponto(chat, ponto, window, cx));
                },
                None,
                window,
                cx,
            );
        },
        window,
        cx,
    )
}

/// Abre o chat (Ctrl+clique, ao lado), sem abrir ou fechar a linha de baixo.
fn abrir_chat(chat: String, cx: &Context<Janela>) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |this, evento: &ClickEvent, window, cx| {
        cx.stop_propagation();
        if evento.modifiers().control {
            this.abrir_ao_lado(chat.clone(), None, window, cx)
        } else {
            this.selecionar(Some(chat.clone()), window, cx)
        }
    })
}

/// "Ultrathink" em arco-íris: a mensagem foi com o esforço extra.
fn selo_ultrathink() -> Div {
    div()
        .h(r(20.))
        .px(r(6.))
        .flex()
        .items_center()
        .rounded(raio::PILULA)
        .bg(Cores::grupo())
        .micro()
        .child(texto_arco_iris("Ultrathink"))
}

/// "Ultracode" em violeta, com o ícone do fluxo: a mensagem foi com vários agentes.
fn selo_ultracode() -> Div {
    div()
        .h(r(20.))
        .px(r(6.))
        .flex()
        .items_center()
        .gap(r(4.))
        .rounded(raio::PILULA)
        .bg(Cores::grupo())
        .micro()
        .text_color(Cores::violeta())
        .child(icone_de(Lucide::Workflow, 12.))
        .child("Ultracode")
}

/// Anexo do balão: miniatura de 56 para imagem; chip com o nome e o tamanho para o resto.
fn anexo(chave: &str, i: usize, caminho: &Path) -> AnyElement {
    let id = SharedString::from(format!("anexo-{chave}-{i}"));
    let dica = SharedString::from(caminho.display().to_string());
    let dica = move |window: &mut Window, cx: &mut App| Tooltip::new(dica.clone()).build(window, cx);
    if crate::claude::tipo_imagem(caminho).is_some() {
        return div()
            .id(id)
            .flex_none()
            .size(r(56.))
            .rounded(raio::CONTROLE)
            .border_1()
            .border_color(Cores::borda())
            .child(
                img(caminho.to_path_buf())
                    .size_full()
                    .rounded(raio::CONTROLE - px(1.))
                    .object_fit(ObjectFit::Cover),
            )
            .tooltip(dica)
            .into_any_element();
    }
    let nome = caminho
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let tamanho = tamanho_do_anexo(caminho);
    h_flex()
        .id(id)
        .h(r(24.))
        .max_w_full()
        .px(r(8.))
        .gap(r(6.))
        .rounded(raio::PILULA)
        .bg(Cores::recuo())
        .metadado()
        .child(icone_de(Lucide::File, 14.).text_color(Cores::tenue()))
        .child(div().min_w_0().truncate().text_color(Cores::texto()).child(nome))
        .when_some(tamanho, |s, t| s.child(div().flex_none().text_color(Cores::tenue()).child(t)))
        .tooltip(dica)
        .into_any_element()
}

thread_local! {
    /// O tamanho de cada anexo já desenhado: o disco é lido uma vez por arquivo, não a cada quadro.
    static TAMANHOS: std::cell::RefCell<HashMap<PathBuf, Option<SharedString>>> = Default::default();
}

fn tamanho_do_anexo(caminho: &Path) -> Option<SharedString> {
    TAMANHOS.with_borrow_mut(|tamanhos| {
        tamanhos
            .entry(caminho.to_path_buf())
            .or_insert_with(|| std::fs::metadata(caminho).ok().map(|m| tamanho_curto(m.len()).into()))
            .clone()
    })
}

/// "2 KB", "1,4 MB".
fn tamanho_curto(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{} KB", (bytes as f64 / 1024.).round() as u64),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.).replace('.', ","),
    }
}

/// Um comando de barra sozinho ("/compact"), que vira uma pílula em vez do balão.
fn comando_de_barra(texto: &str) -> Option<&str> {
    let texto = texto.trim();
    (texto.starts_with('/') && !texto.contains('\n') && texto.chars().count() <= 60).then_some(texto)
}

/// Texto de uma linha sem a marcação do markdown: nenhuma crase ou asterisco aparece na tela.
fn sem_marcacao(texto: &str) -> String {
    texto.replace("**", "").replace('`', "")
}

/// O começo de uma mensagem numa linha só, sem a marcação.
fn previa(texto: &str) -> String {
    sem_marcacao(&texto.split_whitespace().take(80).collect::<Vec<_>>().join(" "))
}

/// Quem mandou uma mensagem entregue: o papel (o círculo) e o nome; `None` é o próprio Vitral.
fn remetente(motor: &Motor, de: &str) -> (Option<Quem>, String) {
    if de == "Vitral" {
        return (None, "Vitral".into());
    }
    if de == "coordenador" {
        return (Some(Quem::Coordenador), "Coordenador".into());
    }
    let (nome, id) = nome_e_id(de);
    let papel = match id.and_then(|id| motor.chat(id)) {
        Some(c) if c.papel == Papel::Trabalhador => Quem::Trabalhador,
        Some(_) => Quem::Coordenador,
        None if nome.starts_with('#') => Quem::Trabalhador,
        None => Quem::Avulso,
    };
    (Some(papel), sem_marcacao(nome))
}

/// "Título (id)" vira o título e o id do chat.
fn nome_e_id(de: &str) -> (&str, Option<&str>) {
    match de.strip_suffix(')').and_then(|s| s.rsplit_once(" (")) {
        Some((nome, id)) if !id.is_empty() && !id.contains(' ') => (nome, Some(id)),
        _ => (de, None),
    }
}

/// O que as linhas de uma conversa precisam saber além do próprio item.
struct Contexto<'a> {
    motor: &'a Motor,
    chat: &'a Chat,
    rodando: bool,
    nivel: LarguraPainel,
    /// A cópia da tarefa foi apagada: não há arquivos para voltar.
    sem_copia: bool,
    /// Última mensagem ou aviso: chamadas sem resultado antes dele foram interrompidas.
    marco: Option<usize>,
    /// Último item que fala de cada tarefa (só ele leva a pílula de estado).
    ultimas: HashMap<u64, usize>,
    /// Última entrega sobre cada tarefa (só ela pode virar cartão).
    entregas: HashMap<u64, usize>,
}

impl<'a> Contexto<'a> {
    fn novo(motor: &'a Motor, chat: &'a Chat, nivel: LarguraPainel) -> Self {
        let mut contexto = Contexto {
            motor,
            chat,
            rodando: motor.execucao(&chat.id).rodando(),
            nivel,
            sem_copia: motor.copia_apagada(chat),
            marco: chat.itens.iter().rposition(|i| {
                matches!(i, Item::Usuario { .. } | Item::Entrega { .. } | Item::Aviso { .. })
            }),
            ultimas: HashMap::new(),
            entregas: HashMap::new(),
        };
        let (mut ultimas, mut entregas) = (HashMap::new(), HashMap::new());
        for (indice, item) in chat.itens.iter().enumerate() {
            let Some(tarefa) = contexto.tarefa(item) else {
                continue;
            };
            match item {
                Item::Entrega { .. } => {
                    entregas.insert(tarefa.id, indice);
                }
                _ if contexto.tipo(indice, item) == Tipo::Comum => continue,
                _ => {}
            }
            ultimas.insert(tarefa.id, indice);
        }
        contexto.ultimas = ultimas;
        contexto.entregas = entregas;
        contexto
    }

    /// Chamada ainda sem resultado no turno que está rodando.
    fn pendente(&self, indice: usize, item: &Item) -> bool {
        matches!(item, Item::Ferramenta { resultado: None, .. })
            && self.rodando
            && self.marco.is_none_or(|m| indice > m)
    }

    /// A tarefa deste coordenador de que o item fala, se houver.
    fn tarefa(&self, item: &Item) -> Option<&'a Tarefa> {
        if self.chat.papel != Papel::Coordenador {
            return None;
        }
        let tarefa = self.motor.tarefa(tarefa_do_item(item)?)?;
        if tarefa.coordenador != self.chat.id {
            return None;
        }
        // Mensagem com "#N" no remetente: confere que veio mesmo do trabalhador dessa tarefa.
        if let Item::Entrega { de, texto, .. } = item
            && de != "Vitral"
            && !texto.starts_with(&format!("Pergunta do trabalhador da tarefa #{} ", tarefa.id))
            && !de.ends_with(&format!("({})", tarefa.trabalhador))
        {
            return None;
        }
        Some(tarefa)
    }

    fn tipo(&self, indice: usize, item: &Item) -> Tipo {
        let Item::Ferramenta { nome, resultado, .. } = item else {
            return Tipo::Comum;
        };
        let Some(acao) = acao_de_tarefa(nome) else {
            return Tipo::Comum;
        };
        // Interrompida antes do resultado: fica como chamada comum, apagada.
        if resultado.is_none() && !self.pendente(indice, item) {
            return Tipo::Comum;
        }
        let criacao = acao == "criar_tarefa";
        if criacao && resultado.is_none() {
            return Tipo::Criacao;
        }
        match (self.tarefa(item).is_some(), criacao) {
            (false, _) => Tipo::Comum,
            (true, true) => Tipo::Criacao,
            (true, false) => Tipo::Tarefa,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tipo {
    Comum,
    Criacao,
    Tarefa,
}

#[derive(Debug, PartialEq)]
enum Segmento {
    Trilha,
    Criacoes,
    Evento,
}

/// Divide chamadas seguidas em trilhas de chamadas comuns, blocos de tarefas criadas em sequência
/// e eventos soltos das outras ações sobre tarefas.
fn segmentar(tipos: &[Tipo]) -> Vec<(Segmento, std::ops::Range<usize>)> {
    let mut segmentos = Vec::new();
    let mut i = 0;
    while i < tipos.len() {
        let mut fim = i + 1;
        if tipos[i] != Tipo::Tarefa {
            while fim < tipos.len() && tipos[fim] == tipos[i] {
                fim += 1;
            }
        }
        let segmento = match tipos[i] {
            Tipo::Comum => Segmento::Trilha,
            Tipo::Criacao if fim - i > 1 => Segmento::Criacoes,
            _ => Segmento::Evento,
        };
        segmentos.push((segmento, i..fim));
        i = fim;
    }
    segmentos
}

/// O último texto do assistente em cada turno (um turno começa numa mensagem sua ou entregue).
fn finais_de_turno(itens: &[Item]) -> HashSet<usize> {
    let mut finais = HashSet::new();
    let mut visto = false;
    for (i, item) in itens.iter().enumerate().rev() {
        match item {
            Item::Usuario { .. } | Item::Entrega { .. } => visto = false,
            Item::Assistente { .. } if !visto => {
                finais.insert(i);
                visto = true;
            }
            _ => {}
        }
    }
    finais
}

/// Linha de log (chamada, evento, cabeçalho de trilha): 28 de altura, ícone de 14 a 12 da borda
/// e o texto a 8 dele. Coladas, dão o passo de 28.
const ALTURA_DO_LOG: Rems = r(28.);

/// O resultado aberto de uma linha de log, embaixo do texto dela.
const RECUO_DO_LOG: Rems = r(32.);

/// Trilho de 1 px que liga os ícones de linhas de log seguidas: no centro do ícone (12 + 7),
/// de 2 px abaixo dele até 1 px antes do ícone da linha de baixo.
fn trilho_do_log() -> Div {
    div()
        .absolute()
        .left(r(19.))
        .top(r(23.))
        .bottom(r(-6.))
        .w(px(1.))
        .bg(Cores::borda())
}

impl Janela {
    /// Chamadas de ferramenta seguidas: as comuns numa trilha, as de tarefa como eventos.
    fn ferramentas(
        &self,
        contexto: &Contexto,
        chamadas: &[(usize, &Item)],
        ao_vivo: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let tipos: Vec<Tipo> = chamadas.iter().map(|(i, item)| contexto.tipo(*i, item)).collect();
        let segmentos = segmentar(&tipos);
        let ultimo = segmentos.len().saturating_sub(1);
        let mut partes: Vec<AnyElement> = segmentos
            .into_iter()
            .enumerate()
            .map(|(k, (segmento, faixa))| {
                let parte = &chamadas[faixa];
                match segmento {
                    Segmento::Trilha => self.trilha(contexto, parte, ao_vivo && k == ultimo, cx),
                    Segmento::Criacoes => self.criacoes(contexto, parte, window, cx),
                    Segmento::Evento => self.evento_da_acao(contexto, parte[0].0, parte[0].1, true, false, window, cx),
                }
            })
            .collect();
        if partes.len() == 1 {
            return partes.remove(0);
        }
        v_flex().children(partes).into_any_element()
    }

    /// Várias chamadas comuns seguidas, uma linha cada, ligadas pelo trilho. De 4 em diante ganham
    /// um cabeçalho com o resumo ("5 ações · leu 3 · rodou 2") e começam recolhidas; ao vivo, a
    /// última fica à vista.
    fn trilha(
        &self,
        contexto: &Contexto,
        chamadas: &[(usize, &Item)],
        ao_vivo: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        if let [(indice, item)] = chamadas {
            return self.chamada(contexto, *indice, item, false, cx);
        }
        let recolhivel = chamadas.len() >= 4;
        // Os itens só crescem no fim: o índice da primeira chamada identifica o grupo.
        let chave = format!("{}:{}:grupo", contexto.chat.id, chamadas[0].0);
        let aberto = !recolhivel || self.expandidos.contains(&chave);
        let mostradas = if aberto {
            chamadas
        } else if ao_vivo {
            &chamadas[chamadas.len() - 1..]
        } else {
            &[]
        };
        let cabecalho = recolhivel.then(|| {
            let nomes: Vec<&str> = chamadas
                .iter()
                .filter_map(|(_, item)| match item {
                    Item::Ferramenta { nome, .. } => Some(nome.as_str()),
                    _ => None,
                })
                .collect();
            let falhas = chamadas
                .iter()
                .filter(|(_, item)| matches!(item, Item::Ferramenta { erro: true, .. }))
                .count();
            let rodando_escondida = chamadas[..chamadas.len() - mostradas.len()]
                .iter()
                .any(|(i, item)| contexto.pendente(*i, item));
            h_flex()
                .id(SharedString::from(format!("trilha-{chave}")))
                .h(ALTURA_DO_LOG)
                .px(esp::E12)
                .gap(esp::E8)
                .rounded(raio::CONTROLE)
                .metadado()
                .cursor_pointer()
                .hover(|s| s.bg(Cores::hover()))
                // O único ponto de Tab da trilha; Enter ou Espaço abre e fecha.
                .tab_index(0)
                .focus_visible(|s| s.shadow(halo_foco()))
                .child(
                    div()
                        .flex_none()
                        .size(r(14.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(Cores::tenue())
                        .child(icone_de(if aberto { Lucide::ChevronDown } else { Lucide::ChevronRight }, 12.)),
                )
                .child(
                    div()
                        .flex_none()
                        .font_semibold()
                        .text_color(Cores::texto())
                        .child(contagem(chamadas.len(), "ação", "ações")),
                )
                .map(|s| {
                    if larga(contexto.nivel) {
                        s.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(Cores::tenue())
                                .child(format!("· {}", resumo_da_trilha(&nomes))),
                        )
                    } else {
                        s.child(div().flex_1())
                    }
                })
                .when(falhas > 0, |s| {
                    s.child(Selo::cor(contagem(falhas, "falhou", "falharam"), Cores::vermelho()))
                })
                .when(rodando_escondida, |s| s.child(Spinner::new().small().color(Cores::destaque())))
                .on_click(alternar(chave.clone(), cx))
        });
        let ultima = mostradas.len().saturating_sub(1);
        v_flex()
            .w_full()
            .children(cabecalho)
            .children(
                mostradas
                    .iter()
                    .enumerate()
                    .map(|(k, (indice, item))| self.chamada(contexto, *indice, item, k < ultima, cx)),
            )
            .into_any_element()
    }

    /// Uma chamada comum: ícone do tipo (cor só na falha), verbo, detalhe em mono e a meta. O
    /// clique mostra o resultado.
    fn chamada(
        &self,
        contexto: &Contexto,
        indice: usize,
        item: &Item,
        com_trilho: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Item::Ferramenta {
            nome,
            resumo,
            resultado,
            erro,
            ..
        } = item
        else {
            return div().into_any_element();
        };
        let chave = format!("{}:{indice}", contexto.chat.id);
        let aberto = self.expandidos.contains(&chave);
        let pendente = contexto.pendente(indice, item);
        // Sem resultado e fora do turno que está rodando: foi interrompida.
        let interrompida = resultado.is_none() && !pendente;
        let icone = if pendente {
            Spinner::new().small().color(Cores::destaque()).into_any_element()
        } else {
            let cor = if *erro {
                Cores::vermelho()
            } else if interrompida {
                Cores::fantasma()
            } else {
                Cores::tenue()
            };
            icone_de(icone_da_ferramenta(nome), 14.).text_color(cor).into_any_element()
        };
        let verbo = verbo_da_ferramenta(nome, pendente);
        let detalhe = sem_marcacao(&detalhe_da_chamada(nome, resumo));
        let meta = if *erro {
            Some(("falhou", Cores::texto()))
        } else if interrompida {
            Some(("interrompida", Cores::apagado()))
        } else {
            None
        };
        let (estreita, minima) = (!larga(contexto.nivel), minima(contexto.nivel));
        let dica = SharedString::from(format!(
            "{verbo} ({})\n{detalhe}{}",
            nome_curto(nome),
            meta.map(|(m, _)| format!(" · {m}")).unwrap_or_default()
        ));
        let linha = h_flex()
            .id(SharedString::from(format!("ferramenta-{chave}")))
            .h(ALTURA_DO_LOG)
            .px(esp::E12)
            .gap(esp::E8)
            .rounded(raio::CONTROLE)
            .metadado()
            .cursor_pointer()
            .map(|s| if *erro { s.bg(fundo_sai()) } else { s.hover(|s| s.bg(Cores::hover())) })
            .child(div().flex_none().size(r(14.)).flex().items_center().justify_center().child(icone))
            .when(!minima, |s| {
                s.child(
                    div()
                        .flex_none()
                        .min_w(r(56.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(Cores::texto())
                        .child(verbo.to_string()),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(FONTE_MONO)
                    .text_color(Cores::tenue())
                    .child(detalhe),
            )
            .when_some(meta.filter(|_| !estreita), |s, (meta, cor)| {
                s.child(div().flex_none().text_color(cor).child(meta))
            })
            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            .on_click(alternar(chave.clone(), cx));
        v_flex()
            .relative()
            .w_full()
            .child(linha)
            .when(aberto, |s| {
                s.child(div().pl(RECUO_DO_LOG).py(esp::E4).child(bloco_resultado(
                    &chave,
                    resultado.as_deref(),
                    *erro,
                    if pendente { "(rodando…)" } else { "(interrompida, sem resultado)" },
                )))
            })
            .when(com_trilho, |s| s.child(trilho_do_log()))
            .into_any_element()
    }

    /// Tarefas criadas em sequência: "Distribuiu 3 tarefas" e um evento por tarefa, ligados pelo
    /// trilho.
    fn criacoes(
        &self,
        contexto: &Contexto,
        chamadas: &[(usize, &Item)],
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let criando = chamadas
            .iter()
            .any(|(_, item)| matches!(item, Item::Ferramenta { resultado: None, .. }));
        let n = chamadas.len();
        v_flex()
            .w_full()
            .child(
                h_flex()
                    .relative()
                    .h(ALTURA_DO_LOG)
                    .px(esp::E12)
                    .gap(esp::E8)
                    .metadado()
                    .child(icone_de(Lucide::Network, 14.).text_color(Cores::tenue()))
                    .child(div().font_semibold().text_color(Cores::texto()).child(format!(
                        "{} {}",
                        if criando { "Distribuindo" } else { "Distribuiu" },
                        contagem(n, "tarefa", "tarefas")
                    )))
                    .child(trilho_do_log()),
            )
            .children(
                chamadas
                    .iter()
                    .enumerate()
                    .map(|(k, (indice, item))| {
                        self.evento_da_acao(contexto, *indice, item, false, k + 1 < n, window, cx)
                    }),
            )
            .into_any_element()
    }

    /// Uma ação do coordenador sobre uma tarefa, como evento ("pediu ajuste em #2 Validar…"). A
    /// seta mostra o resultado; no hover, abrir o chat da tarefa e copiar.
    fn evento_da_acao(
        &self,
        contexto: &Contexto,
        indice: usize,
        item: &Item,
        com_verbo: bool,
        com_trilho: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Item::Ferramenta {
            nome,
            resumo,
            resultado,
            erro,
            ..
        } = item
        else {
            return div().into_any_element();
        };
        let acao = acao_de_tarefa(nome).unwrap_or_default();
        let chave = format!("{}:{indice}", contexto.chat.id);
        let aberto = self.expandidos.contains(&chave);
        let tarefa = contexto.tarefa(item);
        let pilula = tarefa
            .filter(|t| contexto.ultimas.get(&t.id) == Some(&indice))
            .map(|t| PilulaCurta::da(contexto.motor, t));
        let simbolo = if resultado.is_none() {
            Spinner::new().small().color(Cores::destaque()).into_any_element()
        } else if *erro {
            icone_de(Lucide::CircleX, 14.).text_color(Cores::vermelho()).into_any_element()
        } else {
            // Uma cor forte por linha: com pílula, o ícone fica neutro.
            let cor = if pilula.is_some() { Cores::tenue() } else { cor_da_acao(acao, resultado.as_deref()) };
            icone_de(icone_da_acao(acao), 14.).text_color(cor).into_any_element()
        };
        let (sujeito, nome_do_sujeito) = match tarefa {
            Some(t) => {
                let titulo = sem_marcacao(&t.titulo);
                (rotulo_tarefa(t.id, &titulo, false).into_any_element(), format!("#{} {titulo}", t.id))
            }
            None => {
                let detalhe = sem_marcacao(&detalhe_da_chamada(nome, resumo));
                let elemento = div()
                    .min_w_0()
                    .truncate()
                    .text_size(r(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(Cores::texto())
                    .child(detalhe.clone())
                    .into_any_element();
                (elemento, detalhe)
            }
        };
        let mut acoes: Vec<AnyElement> = Vec::new();
        if let Some(t) = tarefa {
            acoes.push(
                botao_so_icone(
                    SharedString::from(format!("abrir-{chave}")),
                    Lucide::MessageSquare,
                    "Abrir a conversa da tarefa\nCtrl+clique abre ao lado",
                    TamanhoBotao::Linha,
                    abrir_chat(t.trabalhador.clone(), cx),
                    window,
                    cx,
                )
                .into_any_element(),
            );
        }
        if let Some(texto) = resultado {
            acoes.push(copiar(&chave, texto).into_any_element());
        }
        let linha = LinhaDeEvento {
            id: SharedString::from(format!("tarefa-{chave}")),
            simbolo,
            verbo: com_verbo.then(|| verbo_da_acao(acao, resultado.as_deref(), *erro)),
            sujeito,
            nome_do_sujeito,
            frase: None,
            pilula,
            aberto: resultado.is_some().then_some(aberto),
            acoes,
            ao_clicar: resultado.is_some().then(|| Rc::new(alternar(chave.clone(), cx)) as Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>),
        }
        .desenhar(contexto.nivel);
        v_flex()
            .relative()
            .w_full()
            .child(linha)
            .when(aberto, |s| {
                s.child(div().pl(RECUO_DO_LOG).py(esp::E4).child(bloco_resultado(
                    &chave,
                    resultado.as_deref(),
                    *erro,
                    "(rodando…)",
                )))
            })
            .when(com_trilho, |s| s.child(trilho_do_log()))
            .into_any_element()
    }

    /// Aviso do Vitral (ou mensagem do trabalhador) sobre uma tarefa. A última entrega de uma
    /// tarefa em revisão vira cartão; as outras, uma linha de evento.
    fn entrega_de_tarefa(
        &self,
        contexto: &Contexto,
        indice: usize,
        item: &Item,
        tarefa: &Tarefa,
        window: &Window,
        cx: &Context<Self>,
    ) -> (Bloco, AnyElement) {
        let Item::Entrega { texto, ponto, .. } = item else {
            return (Bloco::Log, div().into_any_element());
        };
        let chave = format!("{}:{indice}", contexto.chat.id);
        let aberto = self.expandidos.contains(&chave);
        let ultima = contexto.ultimas.get(&tarefa.id) == Some(&indice);
        let frase = sem_marcacao(&frase_da_entrega(texto, tarefa.id, &tarefa.titulo));
        if contexto.entregas.get(&tarefa.id) == Some(&indice) && tarefa.estado == EstadoTarefa::EmRevisao {
            return (Bloco::Cartao, self.cartao_de_entrega(contexto, &chave, texto, tarefa, frase, ultima, window, cx));
        }
        let pilula = ultima.then(|| PilulaCurta::da(contexto.motor, tarefa));
        let falhou = pilula.is_none() && frase.contains("ERRO");
        let simbolo = if falhou {
            icone_de(Lucide::CircleX, 14.).text_color(Cores::vermelho())
        } else {
            icone_de(Lucide::Wrench, 14.).text_color(Cores::tenue())
        };
        let mut acoes: Vec<AnyElement> = vec![
            botao_so_icone(
                SharedString::from(format!("abrir-{chave}")),
                Lucide::MessageSquare,
                "Abrir a conversa da tarefa\nCtrl+clique abre ao lado",
                TamanhoBotao::Linha,
                abrir_chat(tarefa.trabalhador.clone(), cx),
                window,
                cx,
            )
            .into_any_element(),
        ];
        if let Some(ponto) = ponto.clone().filter(|_| !contexto.sem_copia) {
            acoes.push(botao_voltar(&chave, &contexto.chat.id, ponto, window, cx).into_any_element());
        }
        acoes.push(copiar(&chave, texto).into_any_element());
        let titulo = sem_marcacao(&tarefa.titulo);
        let linha = LinhaDeEvento {
            id: SharedString::from(format!("entrega-{chave}")),
            simbolo: simbolo.into_any_element(),
            verbo: None,
            sujeito: rotulo_tarefa(tarefa.id, &titulo, false).into_any_element(),
            nome_do_sujeito: format!("#{} {titulo}", tarefa.id),
            frase: Some(frase),
            pilula,
            aberto: Some(aberto),
            acoes,
            ao_clicar: Some(Rc::new(alternar(chave.clone(), cx))),
        }
        .desenhar(contexto.nivel);
        let elemento = v_flex()
            .w_full()
            .child(linha)
            .when(aberto, |s| {
                s.child(div().pl(RECUO_DO_LOG).pt(esp::E4).corpo_ui().text_color(Cores::texto()).child(markdown(
                    SharedString::from(format!("{chave}-t")),
                    texto.clone(),
                    cx,
                )))
            })
            .into_any_element();
        (Bloco::Log, elemento)
    }

    /// A entrega que espera revisão, num cartão com o reflexo do vidro: "#4 Aplicar cupom" com a
    /// pílula, a rodada com o que a verificação e o revisor acharam (o tempo e a frase inteira na
    /// dica) e as ações. Conteúdo a 12 da borda; a meta e o ícone dos botões na coluna do título.
    #[allow(clippy::too_many_arguments)]
    fn cartao_de_entrega(
        &self,
        contexto: &Contexto,
        chave: &str,
        texto: &str,
        tarefa: &Tarefa,
        frase: String,
        ultima: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let nivel = contexto.nivel;
        let aberto = self.expandidos.contains(chave);
        let pilula = ultima.then(|| PilulaCurta::da(contexto.motor, tarefa));
        let (icone_meta, meta, dica_meta) = meta_da_entrega(tarefa, &frase);
        let dica_meta = SharedString::from(dica_meta);
        let revisar = (tarefa.worktree.is_some() && !tarefa.copia_apagada()).then(|| {
            let id_tarefa = tarefa.id;
            cx.listener(move |this, _: &ClickEvent, window, cx| {
                cx.stop_propagation();
                this.dialogo_diff(id_tarefa, window, cx)
            })
        });
        let abrir = abrir_chat(tarefa.trabalhador.clone(), cx);
        let alternar = alternar(chave.to_string(), cx);
        let so_icones = minima(nivel);
        let ids = (
            SharedString::from(format!("revisar-{chave}")),
            SharedString::from(format!("abrir-{chave}")),
            SharedString::from(format!("relatorio-{chave}")),
        );
        let botoes = {
            let (id_revisar, id_abrir, id_relatorio) = ids;
            // O Revisar em tinta, como nos cartões da Equipe que não são o primeiro a pedir ação:
            // o azul sólido fica só lá.
            let revisar = revisar.map(|f| {
                let b = if so_icones {
                    botao_icone(id_revisar, Lucide::Eye, "Revisar", TamanhoBotao::Barra, window, cx)
                } else {
                    botao_primario(id_revisar, Some(Lucide::Eye.into()), "Revisar", TamanhoBotao::Barra, window, cx)
                };
                b.custom(variante_tinta(cx)).on_click(f)
            });
            let abrir = if so_icones {
                botao_icone(
                    id_abrir,
                    Lucide::MessageSquare,
                    "Abrir a conversa da tarefa\nCtrl+clique abre ao lado",
                    TamanhoBotao::Barra,
                    window,
                    cx,
                )
            } else {
                let icone = Some(Lucide::MessageSquare.into());
                botao_fantasma(id_abrir, icone, "Abrir conversa", TamanhoBotao::Barra, window, cx)
                    .tooltip("Ctrl+clique abre ao lado")
            }
            .on_click(abrir);
            let (seta, dica) = if aberto {
                (Lucide::ChevronUp, "Esconder o relatório")
            } else {
                (Lucide::ChevronDown, "Mostrar o relatório")
            };
            h_flex()
                .gap(esp::E4)
                .children(revisar)
                .child(abrir)
                .child(div().flex_1())
                .child(botao_icone(id_relatorio, seta, dica, TamanhoBotao::Barra, window, cx).on_click(alternar))
        };
        v_flex()
            .w_full()
            .p(dentro_da_borda(12.))
            .gap(esp::E4)
            .rounded(raio::CARTAO)
            .bg(Cores::cartao_elevado())
            .border_1()
            .border_color(Cores::linha())
            .reflexo(raio::CARTAO)
            .child(
                h_flex()
                    .gap(esp::E8)
                    .min_w_0()
                    .child(icone_de(Lucide::Wrench, 16.).text_color(Cores::tenue()))
                    .child(div().flex_1().min_w_0().child(rotulo_tarefa(tarefa.id, &sem_marcacao(&tarefa.titulo), true)))
                    .children(pilula.map(|p| p.desenhar(minima(nivel)))),
            )
            .child(
                h_flex()
                    .id(SharedString::from(format!("meta-{chave}")))
                    .pl(esp::E24)
                    .gap(esp::E6)
                    .metadado()
                    .text_color(Cores::tenue())
                    .when_some(icone_meta, |s, (icone, cor)| s.child(icone_de(icone, 12.).text_color(cor)))
                    .child(div().min_w_0().when(larga(nivel), |s| s.truncate()).child(meta))
                    .tooltip(move |window, cx| Tooltip::new(dica_meta.clone()).build(window, cx)),
            )
            // O ícone do primeiro botão cai na coluna da meta: com rótulo, 12 daqui e 12 do botão;
            // só com o ícone (28), o centro dele no centro do ícone da meta.
            .child(div().pl(if so_icones { esp::E16 } else { esp::E12 }).child(botoes))
            .when(aberto, |s| {
                s.child(div().pl(esp::E24).corpo_ui().text_color(Cores::texto()).child(markdown(
                    SharedString::from(format!("{chave}-t")),
                    texto.to_string(),
                    cx,
                )))
            })
            .into_any_element()
    }
}

/// A linha do cartão de entrega ("Rodada 1 · verificação falhou · 4 observações do revisor"),
/// o ícone da verificação (a cor fica só nele) e a dica: o que o Vitral escreveu, a verificação
/// com o tempo e o parecer inteiro do revisor.
fn meta_da_entrega(tarefa: &Tarefa, frase: &str) -> (Option<(Lucide, Hsla)>, String, String) {
    let erro = frase.contains("ERRO");
    let mut dica = vec![com_maiuscula(frase)];
    if tarefa.conferindo() {
        let texto = tarefa.progresso.as_deref().map(sem_marcacao).unwrap_or_else(|| "conferindo a entrega".into());
        let meta = format!("{} · {}…", meta_curta(tarefa.rodada, erro, None, None), texto.trim_end_matches('…'));
        return (None, meta, dica.join("\n"));
    }
    let valem = tarefa.resultados_valem();
    let verificacao = tarefa.ultima_verificacao.as_ref().filter(|_| valem);
    let revisao = tarefa.revisao.as_ref().filter(|_| valem);
    if let Some(v) = verificacao {
        let tempo = v.tempo().map(|t| format!(" em {t}")).unwrap_or_default();
        dica.push(format!("Verificação {}{tempo}", if v.passou { "passou" } else { "falhou" }));
    }
    if let Some(r) = revisao {
        let parecer = if r.veredito == "aprovar" { "Revisor aprovaria" } else { "Revisor pediu ajustes" };
        dica.push(match r.problemas.len() {
            0 => parecer.to_string(),
            n => format!("{parecer} · {}", contagem(n, "observação", "observações")),
        });
    }
    let icone = verificacao.map(|v| {
        if v.passou { (Lucide::Check, Cores::verde()) } else { (Lucide::CircleX, Cores::vermelho()) }
    });
    let meta = meta_curta(
        tarefa.rodada,
        erro,
        verificacao.map(|v| v.passou),
        revisao.map(|r| (r.veredito == "aprovar", r.problemas.len())),
    );
    (icone, meta, dica.join("\n"))
}

fn com_maiuscula(texto: &str) -> String {
    let mut letras = texto.chars();
    letras.next().map(|p| p.to_uppercase().chain(letras).collect()).unwrap_or_default()
}

/// "Rodada 2 · verificação passou · revisor aprovaria · 1 observação": a rodada e o que a
/// verificação e o revisor (aprovaria?, observações) acharam, sem o tempo.
fn meta_curta(rodada: u32, erro: bool, verificacao: Option<bool>, revisor: Option<(bool, usize)>) -> String {
    let mut partes = vec![if erro { format!("Rodada {rodada} com erro") } else { format!("Rodada {rodada}") }];
    match verificacao {
        Some(true) => partes.push("verificação passou".into()),
        Some(false) => partes.push("verificação falhou".into()),
        None => {}
    }
    match revisor {
        Some((true, n)) => {
            partes.push("revisor aprovaria".into());
            if n > 0 {
                partes.push(contagem(n, "observação", "observações"));
            }
        }
        Some((false, 0)) => partes.push("revisor pediu ajustes".into()),
        Some((false, n)) => partes.push(contagem(n, "observação do revisor", "observações do revisor")),
        None => {}
    }
    partes.join(" · ")
}

/// A pílula de uma tarefa num evento da conversa: só a palavra do estado ("em revisão"), com o
/// detalhe e a explicação na dica; na largura mínima, só o ponto.
struct PilulaCurta {
    visual: EstadoVisual,
    tarefa: u64,
}

impl PilulaCurta {
    fn da(motor: &Motor, t: &Tarefa) -> Self {
        PilulaCurta { visual: estado_visual(motor, t), tarefa: t.id }
    }

    fn desenhar(self, so_ponto: bool) -> AnyElement {
        let v = self.visual;
        let dica: SharedString = match &v.detalhe {
            Some(d) => format!("{} · {d}\n{}", v.rotulo, v.dica).into(),
            None => format!("{}\n{}", v.rotulo, v.dica).into(),
        };
        let pilula = PilulaDeEstado::new(v.estado).rotulo(v.rotulo);
        if so_ponto {
            let pilula = match v.detalhe {
                Some(d) => pilula.detalhe(d),
                None => pilula,
            };
            return pilula.compacta().into_any_element();
        }
        div()
            .id(("pilula-tarefa", self.tarefa as usize))
            .flex_none()
            .child(pilula)
            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            .into_any_element()
    }
}

/// Uma frase do chat sobre uma tarefa: linha de 28 com o símbolo, o verbo, a tarefa, a frase e a
/// pílula. Uma cor forte só (a pílula, ou o ícone quando não há pílula); no hover, as ações
/// aparecem no lugar da seta. Fora da largura larga, o verbo e a frase vão para a dica (o ícone
/// já diz o que foi) e o título da tarefa fica com o espaço.
struct LinhaDeEvento {
    id: SharedString,
    simbolo: AnyElement,
    verbo: Option<&'static str>,
    sujeito: AnyElement,
    /// O sujeito em texto, para a dica.
    nome_do_sujeito: String,
    frase: Option<String>,
    pilula: Option<PilulaCurta>,
    aberto: Option<bool>,
    acoes: Vec<AnyElement>,
    ao_clicar: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

impl LinhaDeEvento {
    fn desenhar(self, nivel: LarguraPainel) -> Stateful<Div> {
        let grupo = SharedString::from(format!("{}-grupo", self.id));
        let estreita = !larga(nivel);
        let (verbo, frase) = if estreita { (None, None) } else { (self.verbo, self.frase.clone()) };
        let dica = (estreita && (self.verbo.is_some() || self.frase.is_some())).then(|| {
            let mut dica = match self.verbo {
                Some(v) => format!("{v} {}", self.nome_do_sujeito),
                None => self.nome_do_sujeito.clone(),
            };
            if let Some(f) = &self.frase {
                dica.push_str(&format!("\n{f}"));
            }
            SharedString::from(dica)
        });
        let com_frase = frase.is_some();
        h_flex()
            .id(self.id)
            .group(grupo.clone())
            .h(ALTURA_DO_LOG)
            .px(esp::E12)
            .gap(esp::E8)
            .rounded(raio::CONTROLE)
            .text_size(r(13.))
            .line_height(r(18.))
            .hover(|s| s.bg(Cores::hover()))
            .child(div().flex_none().size(r(14.)).flex().items_center().justify_center().child(self.simbolo))
            .when_some(verbo, |s, verbo| {
                s.child(div().flex_none().whitespace_nowrap().text_color(Cores::tenue()).child(verbo))
            })
            .child(
                div()
                    .min_w_0()
                    .map(|d| if com_frase { d.flex_shrink(1.).max_w(relative(0.6)) } else { d.flex_1() })
                    .child(self.sujeito),
            )
            .when_some(frase, |s, frase| {
                s.child(div().flex_1().min_w_0().truncate().text_color(Cores::tenue()).child(frase))
            })
            .when_some(self.pilula, |s, p| s.child(p.desenhar(minima(nivel))))
            .child(lugar_de_acoes(grupo, self.acoes, self.aberto))
            .when_some(dica, |s, dica| s.tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx)))
            .when_some(self.ao_clicar, |s, f| {
                s.cursor_pointer()
                    .tab_index(0)
                    .focus_visible(|s| s.shadow(halo_foco()))
                    .on_click(move |e, window, cx| f(e, window, cx))
            })
    }
}

/// Abre ou fecha um item da conversa (a chave fica em `expandidos`, começando pelo id do chat).
/// Ao abrir, os painéis dessa conversa no fim ficam onde estão em vez de seguir o fim do bloco.
fn alternar(
    chave: String,
    cx: &Context<Janela>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |this: &mut Janela, _: &ClickEvent, _, cx| {
        if !this.expandidos.remove(&chave) {
            this.expandidos.insert(chave.clone());
            for painel in this.paineis.iter_mut().filter(|p| p.seguindo) {
                let desta = painel.chat.as_deref().and_then(|c| chave.strip_prefix(c));
                painel.manter_lugar |= desta.is_some_and(|resto| resto.starts_with(':'));
            }
        }
        cx.notify();
    })
}

/// O resultado de uma chamada aberta, na caixa de código do chat (as últimas 40 linhas).
fn bloco_resultado(chave: &str, resultado: Option<&str>, erro: bool, vazio: &str) -> CaixaDeCodigo {
    let texto = resultado.unwrap_or(vazio);
    CaixaDeCodigo::new(SharedString::from(format!("saida-{chave}")), texto.to_string().into())
        .raio_chat()
        .when(erro, |c| c.erro(true).tempo("Saiu com erro"))
}

/// Ferramentas do coordenador que agem sobre uma tarefa e viram eventos na conversa.
const ACOES_DE_TAREFA: [&str; 5] = [
    "criar_tarefa",
    "ver_diff",
    "pedir_ajuste",
    "aprovar_tarefa",
    "cancelar_tarefa",
];

fn acao_de_tarefa(nome: &str) -> Option<&str> {
    nome.strip_prefix("mcp__vitral__")
        .filter(|acao| ACOES_DE_TAREFA.contains(acao))
}

fn nome_curto(nome: &str) -> &str {
    nome.strip_prefix("mcp__vitral__").unwrap_or(nome)
}

fn icone_da_ferramenta(nome: &str) -> Lucide {
    match nome_curto(nome) {
        "Read" => Lucide::BookOpen,
        "Edit" | "MultiEdit" => Lucide::FilePenLine,
        "Write" => Lucide::FilePlus,
        "NotebookEdit" => Lucide::NotebookPen,
        "Bash" | "BashOutput" | "KillShell" => Lucide::Terminal,
        "Grep" => Lucide::TextSearch,
        "Glob" => Lucide::FolderSearch,
        "WebFetch" | "WebSearch" => Lucide::Globe,
        "Task" | "Agent" => Lucide::Bot,
        "Workflow" => Lucide::Workflow,
        "TodoWrite" => Lucide::ListChecks,
        "listar_tarefas" => Lucide::ListTree,
        "anotar_decisao" => Lucide::ScrollText,
        "mensagem_para_chat" | "listar_chats" => Lucide::MessagesSquare,
        "perguntar_coordenador" => Lucide::MessageCircleQuestionMark,
        "informar_progresso" => Lucide::Milestone,
        acao if ACOES_DE_TAREFA.contains(&acao) => icone_da_acao(acao),
        n if n.starts_with("mcp__") => Lucide::Network,
        _ => Lucide::Asterisk,
    }
}

/// O verbo de uma chamada, no passado ou ao vivo ("Leu", "Lendo"). O nome real vai na dica.
fn verbo_da_ferramenta(nome: &str, ao_vivo: bool) -> &str {
    let curto = nome_curto(nome);
    let (passado, gerundio) = match curto {
        "Read" => ("Leu", "Lendo"),
        "Edit" | "MultiEdit" | "NotebookEdit" => ("Editou", "Editando"),
        "Write" => ("Gravou", "Gravando"),
        "Bash" | "BashOutput" => ("Rodou", "Rodando"),
        "Grep" => ("Buscou", "Buscando"),
        "Glob" => ("Listou", "Listando"),
        "WebFetch" => ("Abriu", "Abrindo"),
        "WebSearch" => ("Pesquisou", "Pesquisando"),
        "Task" | "Agent" => ("Subagente", "Subagente"),
        "Workflow" => ("Fluxo de agentes", "Fluxo de agentes"),
        "TodoWrite" => ("Atualizou os passos", "Atualizando os passos"),
        "listar_tarefas" => ("Listou as tarefas", "Listando as tarefas"),
        "listar_chats" => ("Listou as conversas", "Listando as conversas"),
        "anotar_decisao" => ("Anotou", "Anotando"),
        "mensagem_para_chat" => ("Mandou mensagem", "Mandando mensagem"),
        "perguntar_coordenador" => ("Perguntou", "Perguntando"),
        "informar_progresso" => ("Informou", "Informando"),
        "criar_tarefa" => ("Criou a tarefa", "Criando a tarefa"),
        "ver_diff" => ("Olhou as mudanças", "Olhando as mudanças"),
        "pedir_ajuste" => ("Pediu ajuste", "Pedindo ajuste"),
        "aprovar_tarefa" => ("Aprovou", "Aprovando"),
        "cancelar_tarefa" => ("Cancelou", "Cancelando"),
        n if n.starts_with("mcp__") => ("Usou", "Usando"),
        _ => return curto,
    };
    if ao_vivo { gerundio } else { passado }
}

/// O que vem depois do verbo: o detalhe do resumo ou, numa ferramenta MCP de fora, "srv · x".
fn detalhe_da_chamada(nome: &str, resumo: &str) -> String {
    if !nome.starts_with("mcp__vitral__")
        && let Some((servidor, ferramenta)) = nome.strip_prefix("mcp__").and_then(|n| n.split_once("__"))
    {
        return format!("{servidor} · {ferramenta}");
    }
    resumo.split_once(" · ").map(|(_, d)| d.to_string()).unwrap_or_default()
}

/// "leu 3 · editou 1 · rodou 1": as chamadas contadas pelo verbo, na ordem em que aparecem.
fn resumo_da_trilha(nomes: &[&str]) -> String {
    let mut contagem: Vec<(String, usize)> = Vec::new();
    for nome in nomes {
        let verbo = verbo_da_ferramenta(nome, false).to_lowercase();
        match contagem.iter_mut().find(|(v, _)| *v == verbo) {
            Some((_, c)) => *c += 1,
            None => contagem.push((verbo, 1)),
        }
    }
    contagem
        .iter()
        .map(|(v, c)| format!("{v} {c}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn icone_da_acao(acao: &str) -> Lucide {
    match acao {
        "criar_tarefa" => Lucide::Plus,
        "ver_diff" => Lucide::Eye,
        "pedir_ajuste" => Lucide::RotateCw,
        "aprovar_tarefa" => Lucide::Check,
        "cancelar_tarefa" => Lucide::Ban,
        _ => Lucide::Network,
    }
}

/// A cor do ícone de um evento sem pílula: âmbar no ajuste, verde na aprovação.
fn cor_da_acao(acao: &str, resultado: Option<&str>) -> Hsla {
    match (acao, resultado) {
        ("pedir_ajuste", Some(_)) => Cores::ambar(),
        ("aprovar_tarefa", Some(_)) => Cores::verde(),
        _ => Cores::tenue(),
    }
}

/// O verbo do evento de uma ação sobre tarefa, conforme ela ainda roda, deu certo ou falhou. Abre
/// a linha, então começa com maiúscula, como a trilha das ferramentas.
fn verbo_da_acao(acao: &str, resultado: Option<&str>, erro: bool) -> &'static str {
    let estado = resultado.map(|_| erro);
    match (acao, estado) {
        ("criar_tarefa", None) => "Criando",
        ("criar_tarefa", _) => "Criou",
        ("ver_diff", None) => "Abrindo as mudanças de",
        ("ver_diff", Some(false)) => "Olhou as mudanças de",
        ("ver_diff", Some(true)) => "Não abriu as mudanças de",
        ("pedir_ajuste", None) => "Pedindo ajuste em",
        ("pedir_ajuste", Some(false)) => "Pediu ajuste em",
        ("pedir_ajuste", Some(true)) => "Não pediu ajuste em",
        ("aprovar_tarefa", None) => "Juntando",
        ("aprovar_tarefa", Some(false)) if resultado.is_some_and(|r| r.contains("não tinha mudanças")) => {
            "Aprovou sem mudanças"
        }
        // Projeto sem git: a tarefa rodou na própria pasta e não há junção.
        ("aprovar_tarefa", Some(false)) if resultado.is_some_and(|r| r.contains("rodou direto na pasta")) => {
            "Aprovou"
        }
        ("aprovar_tarefa", Some(false)) => "Aprovou e juntou",
        ("aprovar_tarefa", Some(true)) => "Não juntou",
        ("cancelar_tarefa", None) => "Cancelando",
        ("cancelar_tarefa", Some(false)) => "Cancelou",
        ("cancelar_tarefa", Some(true)) => "Não cancelou",
        _ => "Mexeu em",
    }
}

fn numero_no_inicio(texto: &str) -> Option<u64> {
    let fim = texto.find(|c: char| !c.is_ascii_digit()).unwrap_or(texto.len());
    texto[..fim].parse().ok()
}

fn numero_depois(texto: &str, marca: &str) -> Option<u64> {
    numero_no_inicio(&texto[texto.find(marca)? + marca.len()..])
}

/// A tarefa de que um item fala, lida dos textos que o próprio Vitral gera (o formato salvo não
/// guarda o número).
fn tarefa_do_item(item: &Item) -> Option<u64> {
    match item {
        Item::Ferramenta {
            nome,
            resumo,
            resultado,
            erro,
            ..
        } => match acao_de_tarefa(nome)? {
            "criar_tarefa" if !*erro => {
                numero_no_inicio(resultado.as_deref()?.strip_prefix("Tarefa #")?)
            }
            "criar_tarefa" => None,
            _ => numero_depois(resumo, "tarefa #"),
        },
        Item::Entrega { de, texto, .. } if de == "Vitral" => {
            numero_depois(texto.lines().find(|l| !l.trim().is_empty())?, "#")
        }
        Item::Entrega { de, .. } => numero_no_inicio(de.strip_prefix('#')?),
        _ => None,
    }
}

/// O que aconteceu, numa frase curta tirada da primeira linha de uma entrega sobre a tarefa `n`
/// ("terminou a rodada 1"), sem repetir o número e o título.
fn frase_da_entrega(texto: &str, n: u64, titulo: &str) -> String {
    let primeira = texto
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let frase = if let Some(resto) = primeira.strip_prefix(&format!("A tarefa #{n} \"")) {
        resto
            .strip_prefix(&format!("{titulo}\" "))
            .or_else(|| resto.split_once("\" ").map(|(_, d)| d))
            .unwrap_or(resto)
            .to_string()
    } else if primeira.starts_with(&format!("O usuário aprovou a tarefa #{n} ")) {
        "você aprovou pelo painel".to_string()
    } else if primeira.starts_with(&format!("O usuário pediu ajustes na tarefa #{n} ")) {
        "você pediu ajustes direto ao trabalhador".to_string()
    } else if let Some(resto) =
        primeira.strip_prefix(&format!("Pergunta do trabalhador da tarefa #{n} ("))
    {
        format!(
            "perguntou: {}",
            resto.split_once("): ").map(|(_, p)| p).unwrap_or(resto)
        )
    } else {
        primeira.to_string()
    };
    frase.trim().trim_end_matches(['.', ':']).to_string()
}

/// A última resposta do coordenador depois que todas as tarefas acabaram é o resumo para você.
fn indice_do_resumo(motor: &Motor, chat: &Chat) -> Option<usize> {
    if chat.papel != Papel::Coordenador || motor.execucao(&chat.id).rodando() {
        return None;
    }
    let mut tarefas = motor.tarefas_do(&chat.id).peekable();
    tarefas.peek()?;
    if tarefas.any(|t| !t.estado.encerrada()) {
        return None;
    }
    let ultimo = chat
        .itens
        .iter()
        .rposition(|i| matches!(i, Item::Assistente { .. }))?;
    let depois_de_tudo = chat.itens[ultimo..]
        .iter()
        .all(|i| !matches!(i, Item::Entrega { .. } | Item::Usuario { .. }));
    depois_de_tudo.then_some(ultimo)
}

/// "3 tarefas aprovadas · 1 cancelada".
fn meta_do_resumo(estados: &[EstadoTarefa]) -> String {
    let conta = |e: EstadoTarefa| estados.iter().filter(|x| **x == e).count();
    [
        (conta(EstadoTarefa::Aprovada), "tarefa aprovada", "tarefas aprovadas"),
        (conta(EstadoTarefa::Cancelada), "cancelada", "canceladas"),
        (conta(EstadoTarefa::Falhou), "falhou", "falharam"),
    ]
    .into_iter()
    .filter(|(n, ..)| *n > 0)
    .map(|(n, singular, plural)| contagem(n, singular, plural))
    .collect::<Vec<_>>()
    .join(" · ")
}

/// O resumo para você: cartão com o gradiente azul numa camada com o próprio raio (o recorte do
/// pai não arredonda).
fn cartao_resumo(
    motor: &Motor,
    chat: &Chat,
    indice: usize,
    texto: &str,
    cx: &Context<Janela>,
) -> AnyElement {
    let chave = format!("{}:{indice}", chat.id);
    let estados: Vec<EstadoTarefa> = motor.tarefas_do(&chat.id).map(|t| t.estado).collect();
    div()
        .relative()
        .w_full()
        .rounded(raio::CARTAO)
        .bg(Cores::cartao_elevado())
        .border_1()
        .border_color(Cores::borda())
        .shadow(sombra_elevado())
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .rounded(raio::CARTAO - px(1.))
                .bg(gradiente_destaque()),
        )
        .child(
            v_flex()
                .relative()
                .p(dentro_da_borda(16.))
                .gap(esp::E12)
                .child(
                    h_flex()
                        .gap(esp::E8)
                        .child(icone_de(Lucide::CircleCheck, 16.).text_color(Cores::verde()))
                        .child(div().flex_none().titulo_cartao().text_color(Cores::forte()).child("Resumo do coordenador"))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .metadado()
                                .text_color(Cores::tenue())
                                .child(meta_do_resumo(&estados)),
                        )
                        .child(copiar(&chave, texto)),
                )
                .child(div().corpo().text_color(Cores::texto()).child(markdown(
                    SharedString::from(chave.clone()),
                    texto.to_string(),
                    cx,
                ))),
        )
        .into_any_element()
}

/// Como cada aviso do chat aparece.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ClasseAvisoChat {
    /// Centralizado entre traços: sessão nova, turno interrompido.
    Marco,
    /// Uma linha tênue: nova tentativa, conta no limite, cópia sendo preparada.
    Info,
    /// Erro, no alerta vermelho.
    Alerta,
}

/// Os textos vêm todos de `motor.rs`.
fn tipo_aviso(texto: &str, erro: bool) -> ClasseAvisoChat {
    const MARCOS: [&str; 4] = ["Sessão nova", "Interrompido", "Saiu do modo Planejar", "Retomando a sessão"];
    if erro {
        ClasseAvisoChat::Alerta
    } else if MARCOS.iter().any(|m| texto.starts_with(m)) {
        ClasseAvisoChat::Marco
    } else {
        ClasseAvisoChat::Info
    }
}

/// O título de um erro (o que vem antes do primeiro ": ") e o resto.
fn partes_do_erro(texto: &str) -> (Option<String>, String) {
    match texto.split_once(": ") {
        Some((titulo, resto)) if !resto.trim().is_empty() && titulo.chars().count() <= 80 => {
            (Some(titulo.to_string()), resto.to_string())
        }
        _ => (None, texto.to_string()),
    }
}

fn aviso_do_chat(chave: &str, texto: &str, erro: bool) -> (Bloco, AnyElement) {
    match tipo_aviso(texto, erro) {
        ClasseAvisoChat::Marco => {
            let (icone, cor, rotulo) = if texto.starts_with("Interrompido") {
                (Lucide::CirclePause, Cores::tenue(), "Turno interrompido".to_string())
            } else if texto.starts_with("Saiu do modo Planejar") {
                (Lucide::Map, Cores::verde(), texto.to_string())
            } else {
                (Lucide::RefreshCw, Cores::tenue(), texto.to_string())
            };
            let traco = || div().flex_1().min_w(r(16.)).h(px(1.)).bg(Cores::linha());
            let elemento = h_flex()
                .w_full()
                .min_h(r(24.))
                .gap(r(8.))
                .child(traco())
                .child(
                    h_flex()
                        .flex_shrink(1.)
                        .min_w_0()
                        .max_w(relative(0.8))
                        .gap(r(6.))
                        .metadado()
                        .text_color(Cores::tenue())
                        .child(icone_de(icone, 12.).text_color(cor))
                        .child(div().min_w_0().line_clamp(2).text_ellipsis().child(sem_marcacao(&rotulo))),
                )
                .child(traco())
                .into_any_element();
            (Bloco::Marco, elemento)
        }
        ClasseAvisoChat::Info => {
            let icone = if texto.to_lowercase().contains("tentando de novo") { Lucide::RotateCw } else { Lucide::Info };
            // Uma linha de log como as outras (28, ícone a 12), que cresce se o texto quebrar.
            let elemento = h_flex()
                .items_start()
                .min_h(ALTURA_DO_LOG)
                .py(esp::E4)
                .gap(esp::E8)
                .px(esp::E12)
                .child(div().flex_none().pt(esp::E2).child(icone_de(icone, 14.).text_color(Cores::tenue())))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(r(13.))
                        .line_height(r(18.))
                        .text_color(Cores::tenue())
                        .child(sem_marcacao(texto)),
                )
                .into_any_element();
            (Bloco::Log, elemento)
        }
        ClasseAvisoChat::Alerta => {
            let (titulo, detalhe) = partes_do_erro(texto);
            let alerta = Alerta::new(Tom::Erro, sem_marcacao(&detalhe)).acao(copiar(chave, texto));
            let alerta = match titulo {
                Some(t) => alerta.titulo(sem_marcacao(&t)),
                None => alerta,
            };
            (Bloco::Alerta, alerta.into_any_element())
        }
    }
}

impl Janela {
    fn voltar_ao_ponto(&mut self, chat: String, ponto: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let tarefa = motor.update(cx, |m, cx| m.voltar_ao_ponto(&chat, ponto, cx));
        cx.spawn_in(window, async move |_, cx| {
            let resultado = tarefa.await;
            let _ = cx.update(|window, cx| match resultado {
                Ok(0) => window.push_notification(aviso(Tom::Info, "Nenhum arquivo tinha mudado desde esse ponto."), cx),
                Ok(1) => window.push_notification(aviso(Tom::Pronto, "1 arquivo voltou ao ponto escolhido."), cx),
                Ok(n) => window.push_notification(aviso(Tom::Pronto, format!("{n} arquivos voltaram ao ponto escolhido.")), cx),
                Err(erro) => window.push_notification(aviso_erro("Não deu para voltar os arquivos", &erro), cx),
            });
        })
        .detach();
    }
}

/// Onde o usuário está num painel: o deslocamento da última conferência (para saber quando foi
/// ele que rolou) e o tamanho da conversa da última vez que ele estava no fim.
#[derive(Default)]
pub(super) struct Leitura {
    deslocamento: Pixels,
    vista: (usize, usize),
}

/// O campo aberto embaixo de um pedido de permissão para negar explicando.
pub(super) struct Explicacao {
    pedido: String,
    campo: Entity<InputState>,
    _enter: Subscription,
}

/// O que a faixa do turno mostra: vale a primeira situação que se aplicar.
#[derive(Debug, PartialEq)]
enum Situacao {
    Rodando { desde: u64, fila: usize },
    EsperandoVaga,
    NovaTentativa { em: u64 },
    Parada { motivo: String, atencao: bool },
    Falhou,
    Fila(usize),
}

fn situacao_do_turno(motor: &Motor, chat: &str) -> Option<Situacao> {
    match motor.execucao(chat) {
        // Esperando você: os pedidos de permissão já aparecem logo abaixo.
        Execucao::EsperandoVoce => return None,
        Execucao::Rodando { desde } => return Some(Situacao::Rodando { desde, fila: motor.na_fila(chat) }),
        _ => {}
    }
    if motor.esperando_vaga(chat) {
        return Some(Situacao::EsperandoVaga);
    }
    if let Some(em) = motor.nova_tentativa_em(chat) {
        return Some(Situacao::NovaTentativa { em });
    }
    if let Some(motivo) = motor.parada(chat) {
        return Some(Situacao::Parada { atencao: motor.precisa_retomar(chat), motivo });
    }
    if motor.pode_tentar_de_novo(chat) {
        return Some(Situacao::Falhou);
    }
    let fila = motor.na_fila(chat);
    (fila > 0).then_some(Situacao::Fila(fila))
}

impl Janela {
    /// Faixa do turno, acima da caixa e sempre à vista: o que o chat faz agora (ou por que parou)
    /// e a ação que cabe. Some sem deixar buraco quando não há nada.
    pub(super) fn faixa_atividade(
        &self,
        motor: &Motor,
        chat: &Chat,
        painel_id: u64,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let situacao = situacao_do_turno(motor, &chat.id)?;
        let medida = self.paineis.iter().find(|p| p.id == painel_id).map(|p| p.medida).unwrap_or_default();
        let (estreita, minima) = (!larga(medida.nivel), minima(medida.nivel));
        // Alinha com o texto da caixa (a margem da coluna mais o recuo do texto nela) e, à direita,
        // com o enviar.
        let caixa_estreita = medida.faixa == FaixaCaixa::Estreita;
        let margem = tam(window, if caixa_estreita { 12. } else { 16. });
        let (recuo, fim) = (margem + composer::recuo_do_texto(caixa_estreita, window), margem + composer::borda_ao_conteudo(caixa_estreita, window));
        let atividade = match &situacao {
            Situacao::Rodando { .. } => {
                let escrevendo = motor.parcial(&chat.id).is_some();
                let texto = texto_atividade(&chat.itens, escrevendo);
                // A resposta já veio, mas os agentes do fluxo seguem trabalhando.
                if !escrevendo && texto == "Pensando…" && motor.esperando_agentes(&chat.id) {
                    "Agentes trabalhando em segundo plano…".to_string()
                } else {
                    sem_marcacao(&texto)
                }
            }
            _ => String::new(),
        };
        let (motor_e, id) = (self.motor.clone(), chat.id.clone());
        let acao = move |f: fn(&mut Motor, &str, &mut Context<Motor>)| {
            let (motor_e, id) = (motor_e.clone(), id.clone());
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                if let Some(m) = &motor_e {
                    m.update(cx, |m, cx| f(m, &id, cx));
                }
            }
        };
        let (retomar, tentar, entregar) = (
            acao(|m, id, cx| m.retomar(id, cx)),
            acao(|m, id, cx| m.tentar_de_novo(id, cx)),
            acao(|m, id, cx| m.entregar_fila(id, cx)),
        );
        let texto = |t: String| div().min_w_0().truncate().text_size(r(13.)).line_height(r(18.)).child(t);
        let esc = move |rotulo: &'static str, window: &Window| {
            h_flex()
                .flex_none()
                .gap(r(6.))
                .metadado()
                .children(teclas(&PararTurno, Some("Caixa"), window))
                .when(!estreita, |s| s.child(rotulo))
                .into_any_element()
        };
        let (esquerda, direita): (AnyElement, Option<AnyElement>) = match situacao {
            Situacao::Rodando { desde, fila } => (
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(r(8.))
                    .child(
                        // Passada mais lenta: a 12 quadros por segundo, o brilho anda em
                        // passos menores.
                        ShimmerText::new(atividade)
                            .id(("atividade", painel_id as usize))
                            .duration(Duration::from_secs(3))
                            .highlight_color(Cores::forte())
                            .rotulo()
                            .min_w_0()
                            .truncate(),
                    )
                    .when(!minima, |s| {
                        s.child(div().flex_none().metadado().text_color(Cores::apagado()).child(duracao(desde)))
                    })
                    .when(fila > 0, |s| {
                        s.child(
                            h_flex()
                                .flex_none()
                                .gap(r(4.))
                                .child(Selo::contagem(fila))
                                .child(div().metadado().text_color(Cores::apagado()).child("na fila")),
                        )
                    })
                    .into_any_element(),
                Some(esc("para parar", window)),
            ),
            Situacao::EsperandoVaga => (
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(r(8.))
                    .child(icone_de(Lucide::Clock, 14.).text_color(Cores::tenue()))
                    .child(texto("Na fila: começa quando um trabalhador terminar".into()))
                    .into_any_element(),
                Some(esc("tirar da fila", window)),
            ),
            Situacao::NovaTentativa { em } => {
                let faltam = em.saturating_sub(crate::modelo::agora());
                (
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(r(8.))
                        .child(icone_de(Lucide::RotateCw, 14.).text_color(Cores::ambar()))
                        .child(texto(format!("Caiu; tentando de novo em {faltam} s")))
                        .into_any_element(),
                    None,
                )
            }
            Situacao::Parada { motivo, atencao } => (
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(r(8.))
                    .child(icone_de(Lucide::Pause, 14.).text_color(if atencao {
                        Cores::ambar()
                    } else {
                        Cores::tenue()
                    }))
                    .child(texto(format!("Parada: {motivo}")))
                    .into_any_element(),
                Some(
                    botao_fantasma("faixa-retomar", Some(Lucide::Play.into()), "Retomar", TamanhoBotao::Barra, window, cx)
                        .on_click(retomar)
                        .into_any_element(),
                ),
            ),
            Situacao::Falhou => (
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(r(8.))
                    .child(icone_de(Lucide::CircleX, 14.).text_color(Cores::vermelho()))
                    .child(texto("O último turno falhou".into()))
                    .into_any_element(),
                Some(
                    botao_fantasma(
                        "faixa-tentar",
                        Some(Lucide::RotateCw.into()),
                        "Tentar de novo",
                        TamanhoBotao::Barra,
                        window,
                        cx,
                    )
                    .on_click(tentar)
                    .into_any_element(),
                ),
            ),
            Situacao::Fila(n) => (
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(r(8.))
                    .child(icone_de(Lucide::ListChecks, 14.).text_color(Cores::tenue()))
                    .child(texto(contagem(n, "mensagem esperando na fila", "mensagens esperando na fila")))
                    .into_any_element(),
                Some(
                    botao_fantasma("faixa-entregar", None, "Entregar agora", TamanhoBotao::Barra, window, cx)
                        .on_click(entregar)
                        .into_any_element(),
                ),
            ),
        };
        Some(
            h_flex()
                .map(coluna)
                .pl(recuo)
                .pr(fim)
                .h(r(28.))
                .gap(esp::E8)
                .text_color(Cores::tenue())
                .child(esquerda)
                .children(direita)
                .into_any_element(),
        )
    }

    /// Confere os painéis que o usuário rolou (roda, barra, ou o conteúdo que encolheu) desde a
    /// última vez: o que saiu do fim deixa de segui-lo; o que voltou ao fim conta como visto.
    /// Qualquer subida solta o fim (o painel no fim volta a ele a cada desenho, e um passo da
    /// roda ou do touchpad é menor que a folga); descendo, a folga de 48 já prende de novo.
    pub(super) fn acompanhar_rolagem(&mut self, cx: &mut Context<Self>) {
        for painel in &mut self.paineis {
            let deslocamento = painel.rolagem.offset().y;
            if deslocamento == painel.leitura.deslocamento {
                continue;
            }
            let subiu = deslocamento > painel.leitura.deslocamento;
            painel.leitura.deslocamento = deslocamento;
            let folga = if subiu { px(1.) } else { px(48.) };
            painel.seguindo = distancia_do_fim(&painel.rolagem) <= folga;
            if painel.seguindo
                && let (Some(chat), Some(motor)) = (&painel.chat, &self.motor)
            {
                motor.update(cx, |m, _| m.marcar_lido(chat));
            }
        }
    }

    /// Ctrl+End: o painel ativo volta ao fim da conversa.
    pub(super) fn ir_ao_fim_ativo(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.paineis.get(self.ativo).map(|p| p.id) {
            self.ir_para_o_fim(id, cx);
        }
    }

    /// Volta o painel ao fim da conversa e marca o que chegou como visto.
    fn ir_para_o_fim(&mut self, painel_id: u64, cx: &mut Context<Self>) {
        let Some(painel) = self.paineis.iter_mut().find(|p| p.id == painel_id) else {
            return;
        };
        painel.seguindo = true;
        painel.rolagem.scroll_to_bottom();
        if let (Some(chat), Some(motor)) = (painel.chat.clone(), self.motor.clone()) {
            motor.update(cx, |m, _| m.marcar_lido(&chat));
        }
        cx.notify();
    }

    pub(super) fn permissoes(
        &self,
        motor: &Motor,
        chat: &Chat,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let pedidos: Vec<(String, String, serde_json::Value)> = motor
            .permissoes
            .iter()
            .filter(|p| p.chat == chat.id)
            .map(|p| (p.id.clone(), p.ferramenta.clone(), p.entrada.clone()))
            .collect();
        let manual = motor.permissao_de(chat) == "manual";
        let (nivel, faixa) = self
            .paineis
            .iter()
            .find(|p| p.chat.as_deref() == Some(chat.id.as_str()))
            .map(|p| (p.medida.nivel, p.medida.faixa))
            .unwrap_or_default();
        // Com vários pedidos, só um mostra o corpo: o escolhido ou o primeiro.
        let aberto = self
            .pedido_aberto
            .clone()
            .filter(|a| pedidos.iter().any(|(id, ..)| id == a))
            .or_else(|| pedidos.first().map(|(id, ..)| id.clone()));
        let quantos = pedidos.len();
        // Os pedidos encolhem e rolam quando falta altura, para a caixa e as mensagens não saírem
        // da tela; vários de uma vez (chamadas em paralelo) nunca passam de 400 px.
        v_flex()
            .id(SharedString::from(format!("permissoes-{}", chat.id)))
            .map(coluna)
            .when(faixa == FaixaCaixa::Estreita, |s| s.px(r(12.)))
            .gap(r(8.))
            .min_h_0()
            .overflow_y_scroll()
            .when(quantos > 0, |s| s.pb(r(2.)))
            .when(quantos > 1, |s| {
                s.max_h(r(400.)).child(
                    div()
                        .flex_none()
                        .child(CabecalhoDeSecao::new("Pedidos").contagem(quantos).tingida(Cores::ambar()).primeira(true)),
                )
            })
            .children(pedidos.into_iter().enumerate().map(|(k, (id, ferramenta, entrada))| {
                let corpo = aberto.as_deref() == Some(id.as_str());
                if ferramenta == "ExitPlanMode" {
                    return self.cartao_plano(motor, chat, &id, &entrada, nivel, corpo, window, cx);
                }
                let posicao = (quantos > 1).then_some((k + 1, quantos));
                self.cartao_permissao(chat, manual, &id, &ferramenta, &entrada, corpo, posicao, nivel, window, cx)
            }))
    }

    /// Um pedido de permissão: o que exatamente vai ser feito (o comando inteiro, o trecho que
    /// muda, o começo do arquivo) e as respostas. Âmbar no cabeçalho; vermelho, e sem "permitir
    /// sempre", quando o comando tem um trecho perigoso.
    #[allow(clippy::too_many_arguments)]
    fn cartao_permissao(
        &self,
        chat: &Chat,
        manual: bool,
        id: &str,
        ferramenta: &str,
        entrada: &serde_json::Value,
        corpo_aberto: bool,
        posicao: Option<(usize, usize)>,
        nivel: LarguraPainel,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id_texto = |nome: &str| SharedString::from(format!("{nome}-{id}"));
        let perigoso = ferramenta == "Bash"
            && !trechos_perigosos(entrada["command"].as_str().unwrap_or_default()).is_empty();
        let tom = if perigoso { Cores::vermelho() } else { Cores::ambar() };
        let (titulo, subtitulo) = titulo_do_pedido(ferramenta, entrada, &chat.cwd);
        let caminho_no_subtitulo = matches!(ferramenta, "Edit" | "MultiEdit" | "Write" | "NotebookEdit");
        let fora = entrada["file_path"]
            .as_str()
            .or(entrada["notebook_path"].as_str())
            .is_some_and(|c| caminho_no_chat(c, &chat.cwd).1);
        let regra = crate::modelo::regra_de_permissao(ferramenta, entrada, &chat.cwd)
            .filter(|_| !perigoso)
            .map(|regra| rotulo_da_regra(&regra, manual));
        let explicando = self
            .explicacao
            .as_ref()
            .filter(|e| e.pedido == id)
            .map(|e| e.campo.clone());
        let corpo = corpo_aberto.then(|| corpo_permissao(id, ferramenta, entrada));
        let (estreita, minima) = (!larga(nivel), minima(nivel));
        let responder = |permitir: bool| {
            let id = id.to_string();
            cx.listener(move |this: &mut Janela, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                if let Some(m) = &this.motor {
                    m.update(cx, |m, cx| m.responder_permissao(&id, permitir, None, cx));
                }
            })
        };
        let (negar, permitir) = (responder(false), responder(true));
        let sempre = {
            let id = id.to_string();
            Rc::new(cx.listener(move |this: &mut Janela, _: &ClickEvent, _, cx| {
                if let Some(m) = &this.motor {
                    m.update(cx, |m, cx| m.permitir_sempre(&id, cx));
                }
            }))
        };
        let explicar = {
            let id = id.to_string();
            cx.listener(move |this, _: &ClickEvent, window, cx| this.abrir_explicacao(id.clone(), window, cx))
        };
        let cancelar_explicacao = cx.listener(|this, _: &ClickEvent, _, cx| {
            this.explicacao = None;
            cx.notify();
        });
        let enviar_explicacao = cx.listener(|this, _: &ClickEvent, _, cx| this.negar_explicando(cx));
        let abrir = {
            let id = id.to_string();
            cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.pedido_aberto = Some(id.clone());
                cx.notify();
            })
        };
        let ids = [
            "cab", "negar", "permitir", "explicar", "sempre", "mais", "cancelar-explicacao", "negar-explicando",
        ]
        .map(id_texto);
        let [id_cab, id_negar, id_permitir, id_explicar, id_sempre, id_mais, id_cancelar, id_enviar] = ids;
        let principal = |id: SharedString, window: &Window, cx: &App| {
            if perigoso {
                botao_perigo(id, None, "Permitir mesmo assim", TamanhoBotao::Controle, window, cx)
            } else {
                botao_primario(id, None, "Permitir", TamanhoBotao::Controle, window, cx)
            }
        };
        let cabecalho = h_flex()
            .id(id_cab)
            .flex_none()
            .h(r(44.))
            .px(dentro_da_borda(12.))
            .gap(esp::E8)
            .bg(Cores::tinta(tom))
            .map(|s| {
                if corpo_aberto {
                    s.rounded_t(raio::CARTAO - px(1.))
                } else {
                    s.rounded(raio::CARTAO - px(1.))
                }
            })
            .child(icone_de(Lucide::ShieldQuestionMark, 16.).text_color(tom))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().titulo_cartao().text_color(Cores::forte()).truncate().child(titulo))
                    .when_some(subtitulo, |s, t| {
                        s.child(
                            div()
                                .metadado()
                                .text_color(Cores::tenue())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .map(|s| if caminho_no_subtitulo { s.text_ellipsis_start() } else { s.text_ellipsis() })
                                .child(sem_marcacao(&t)),
                        )
                    }),
            )
            .when(fora && !minima, |s| {
                s.child(div().flex_none().metadado().text_color(Cores::ambar()).child("fora da pasta"))
            })
            .when_some(posicao, |s, (k, n)| {
                s.child(div().flex_none().metadado().text_color(Cores::tenue()).child(format!("{k} de {n}")))
            });
        let mut cartao = v_flex()
            .w_full()
            .rounded(raio::CARTAO)
            .bg(Cores::cartao_elevado())
            .border_1()
            .border_color(Cores::tinta_borda(tom))
            .shadow(sombra_elevado());
        let Some(corpo) = corpo else {
            // Recolhido (há outro pedido aberto): só o cabeçalho, com Negar e Permitir.
            let negar = botao_fantasma(id_negar, None, "Negar", TamanhoBotao::Barra, window, cx).on_click(negar);
            let permitir = if perigoso {
                botao_perigo(id_permitir, None, "Permitir mesmo assim", TamanhoBotao::Barra, window, cx)
            } else {
                botao_primario(id_permitir, None, "Permitir", TamanhoBotao::Barra, window, cx)
            }
            .on_click(permitir);
            return cartao.child(
                cabecalho
                    .cursor_pointer()
                    .on_click(abrir)
                    .child(h_flex().flex_none().gap(r(6.)).child(negar).child(permitir)),
            )
            .into_any_element();
        };
        cartao = cartao.child(cabecalho);
        let rodape = match explicando {
            Some(campo) => h_flex()
                .gap(r(8.))
                .child(div().flex_1().min_w_0().child(Input::new(&campo)))
                .child(
                    botao_fantasma(id_cancelar, None, "Cancelar", TamanhoBotao::Controle, window, cx)
                        .on_click(cancelar_explicacao),
                )
                .child(
                    botao_primario(id_enviar, None, "Negar e enviar", TamanhoBotao::Controle, window, cx)
                        .on_click(enviar_explicacao),
                )
                .into_any_element(),
            None => {
                let icone_explicar = Lucide::MessageSquareQuote;
                let dica_explicar = "Nega e diz ao Claude o que fazer em vez disso";
                let explicar = if estreita {
                    botao_icone(id_explicar, icone_explicar, "Negar e explicar…", TamanhoBotao::Controle, window, cx)
                } else {
                    botao_fantasma(
                        id_explicar,
                        Some(icone_explicar.into()),
                        "Negar e explicar…",
                        TamanhoBotao::Controle,
                        window,
                        cx,
                    )
                    .tooltip(dica_explicar)
                }
                .on_click(explicar);
                let negar = botao_fantasma(id_negar, None, "Negar", TamanhoBotao::Controle, window, cx).on_click(negar);
                let principal = principal(id_permitir, window, cx).on_click(permitir);
                if minima {
                    // "Permitir sempre" vai para o "…"; o Permitir ocupa a largura toda.
                    let mais = regra.map(|(rotulo, dica)| {
                        let sempre = sempre.clone();
                        botao_icone(id_mais, Lucide::Ellipsis, "Mais respostas", TamanhoBotao::Controle, window, cx)
                            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                                let sempre = sempre.clone();
                                menu.item(
                                    PopupMenuItem::new(rotulo.clone())
                                        .on_click(move |e, window, cx| sempre(e, window, cx)),
                                )
                                .item(PopupMenuItem::static_element({
                                    let dica = dica.clone();
                                    move |_, _| {
                                        div()
                                            .max_w(r(260.))
                                            .metadado()
                                            .text_color(Cores::tenue())
                                            .child(dica.clone())
                                    }
                                }))
                            })
                    });
                    v_flex()
                        .gap(r(8.))
                        .child(h_flex().gap(r(8.)).child(explicar).child(negar).child(div().flex_1()).children(mais))
                        .child(principal.w_full())
                        .into_any_element()
                } else {
                    let sempre = regra.map(|(rotulo, dica)| {
                        botao_fantasma(id_sempre, None, rotulo, TamanhoBotao::Controle, window, cx)
                            .tooltip(dica)
                            .on_click(move |e, window, cx| sempre(e, window, cx))
                    });
                    h_flex()
                        .flex_wrap()
                        .justify_end()
                        .gap(r(8.))
                        .child(explicar)
                        .child(div().flex_1())
                        .child(h_flex().gap(r(8.)).child(negar).children(sempre).child(principal))
                        .into_any_element()
                }
            }
        };
        cartao.child(
            v_flex()
                .p(dentro_da_borda(12.))
                .gap(esp::E12)
                .child(corpo)
                .when(perigoso, |s| {
                    s.child(
                        h_flex()
                            .items_start()
                            .gap(esp::E6)
                            .child(div().flex_none().pt(esp::E2).child(
                                icone_de(Lucide::TriangleAlert, 14.).text_color(Cores::vermelho()),
                            ))
                            .child(
                                div()
                                    .metadado()
                                    .text_color(Cores::texto())
                                    .child("O trecho em vermelho apaga, força ou roda algo difícil de desfazer."),
                            ),
                    )
                })
                .child(rodape),
        )
        .into_any_element()
    }

    /// Abre, embaixo do pedido, o campo para negar dizendo o que fazer em vez disso.
    fn abrir_explicacao(&mut self, pedido: String, window: &mut Window, cx: &mut Context<Self>) {
        let campo = cx.new(|cx| {
            InputState::new(window, cx).placeholder("O que ele deve fazer em vez disso?")
        });
        let enter = cx.subscribe_in(&campo, window, |this, _, evento: &InputEvent, _, cx| {
            if matches!(evento, InputEvent::PressEnter { .. }) {
                this.negar_explicando(cx);
            }
        });
        let foco = campo.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        self.explicacao = Some(Explicacao {
            pedido,
            campo,
            _enter: enter,
        });
        cx.notify();
    }

    /// Nega o pedido do campo aberto, mandando ao Claude o que o usuário escreveu.
    fn negar_explicando(&mut self, cx: &mut Context<Self>) {
        let Some(explicacao) = self.explicacao.take() else {
            return;
        };
        let texto = explicacao.campo.read(cx).value().trim().to_string();
        let mensagem = (!texto.is_empty())
            .then(|| format!("O usuário negou esta ação e explicou o que fazer: {texto}"));
        if let Some(m) = &self.motor {
            m.update(cx, |m, cx| m.responder_permissao(&explicacao.pedido, false, mensagem, cx));
        }
        cx.notify();
    }

    /// Devolve o plano com o comentário do campo; ele refaz o plano.
    fn pedir_mudancas_no_plano(&mut self, id: &str, cx: &mut Context<Self>) {
        // O campo some junto com o pedido (ver Janela::render).
        let texto = self
            .feedback_plano
            .get(id)
            .map(|c| c.read(cx).value().to_string())
            .unwrap_or_default();
        let mensagem = if texto.trim().is_empty() {
            "O usuário pediu para revisar o plano.".to_string()
        } else {
            format!("O usuário pediu mudanças no plano: {texto}")
        };
        if let Some(m) = &self.motor {
            m.update(cx, |m, cx| m.responder_permissao(id, false, Some(mensagem), cx));
        }
    }

    /// No modo Planejar, o Claude apresenta o plano pedindo para sair do modo: aprovar deixa ele
    /// executar (no modo que o subtítulo diz, ou num dos dois do menu ao lado); "pedir mudanças"
    /// devolve o comentário e ele refaz o plano.
    #[allow(clippy::too_many_arguments)]
    fn cartao_plano(
        &self,
        motor: &Motor,
        chat: &Chat,
        id: &str,
        entrada: &serde_json::Value,
        nivel: LarguraPainel,
        corpo_aberto: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let plano = entrada["plan"].as_str().unwrap_or("(plano vazio)").to_string();
        let depois = motor.modo_ao_aprovar_plano(chat);
        let (icone_modo, cor_modo) = icone_do_modo(&depois, false);
        let nome_modo = nome_do_modo(&depois);
        let campo = self.feedback_plano.get(id).cloned();
        let com_texto = campo.as_ref().is_some_and(|c| !c.read(cx).value().trim().is_empty());
        let texto = markdown(SharedString::from(format!("plano-txt-{id}")), plano.clone(), cx);
        let (estreita, minima) = (!larga(nivel), minima(nivel));
        let aprovar = {
            let id = id.to_string();
            cx.listener(move |this: &mut Janela, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                if let Some(m) = &this.motor {
                    m.update(cx, |m, cx| m.responder_permissao(&id, true, None, cx));
                }
            })
        };
        // As outras duas saídas do Planejar: o modo passa a ser o escolhido antes de aprovar.
        let aprovar_em = |modo: &'static str| {
            let (id, chat) = (id.to_string(), chat.id.clone());
            Rc::new(cx.listener(move |this: &mut Janela, _: &ClickEvent, _, cx| {
                if let Some(m) = &this.motor {
                    m.update(cx, |m, cx| {
                        m.mudar_chat(&chat, |c| c.permissao = Some(modo.to_string()), cx);
                        m.responder_permissao(&id, true, None, cx);
                    });
                }
            }))
        };
        let (aprovar_editando, aprovar_perguntando) = (aprovar_em("acceptEdits"), aprovar_em("manual"));
        let mudar = {
            let id = id.to_string();
            cx.listener(move |this, _: &ClickEvent, _, cx| this.pedir_mudancas_no_plano(&id, cx))
        };
        let enter = {
            let id = id.to_string();
            cx.listener(move |this, _: &Enter, _, cx| this.pedir_mudancas_no_plano(&id, cx))
        };
        let abrir = {
            let id = id.to_string();
            cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.pedido_aberto = Some(id.clone());
                cx.notify();
            })
        };
        let ids = ["cab-plano", "rolagem-plano", "aprovar-plano", "mais-plano", "mudar-plano", "copiar-plano"]
            .map(|nome| SharedString::from(format!("{nome}-{id}")));
        let [id_cab, id_rolagem, id_aprovar, id_mais, id_mudar, id_copiar] = ids;
        let cabecalho = h_flex()
            .id(id_cab)
            .flex_none()
            .h(r(44.))
            .px(dentro_da_borda(12.))
            .gap(esp::E8)
            .bg(Cores::tinta(Cores::ambar()))
            .map(|s| {
                if corpo_aberto {
                    s.rounded_t(raio::CARTAO - px(1.))
                } else {
                    s.rounded(raio::CARTAO - px(1.))
                }
            })
            .child(icone_de(Lucide::Map, 16.).text_color(Cores::ambar()))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().titulo_cartao().text_color(Cores::forte()).truncate().child("Plano para aprovar"))
                    .child(
                        h_flex()
                            .gap(r(4.))
                            .metadado()
                            .text_color(Cores::tenue())
                            .child(div().flex_none().child("Depois de aprovar:"))
                            .child(icone_modo.size(r(12.)).flex_none().text_color(cor_modo))
                            .child(div().min_w_0().truncate().child(nome_modo)),
                    ),
            );
        let cartao = v_flex()
            .w_full()
            .rounded(raio::CARTAO)
            .bg(Cores::cartao_elevado())
            .border_1()
            .border_color(Cores::tinta_borda(Cores::ambar()))
            .shadow(sombra_elevado());
        if !corpo_aberto {
            // Recolhido (há outro pedido aberto): só o cabeçalho, com Aprovar.
            let aprovar =
                botao_primario(id_aprovar, Some(Lucide::Check.into()), "Aprovar", TamanhoBotao::Barra, window, cx)
                    .on_click(aprovar);
            return cartao.child(cabecalho.cursor_pointer().on_click(abrir).child(aprovar)).into_any_element();
        }
        let copiar_plano =
            BotaoCopiar::new(Clipboard::new(id_copiar).value(SharedString::from(plano)).tooltip("Copiar o plano"));
        let rotulo_aprovar = if estreita { "Aprovar" } else { "Aprovar e executar" };
        let aprovar = botao_primario(id_aprovar, Some(Lucide::Check.into()), rotulo_aprovar, TamanhoBotao::Controle, window, cx)
            .on_click(aprovar);
        let menu = Button::new(id_mais)
            .primary()
            .icon(Icon::new(Lucide::ChevronDown).size(r(14.)))
            .h(r(32.))
            .w(r(28.))
            .rounded(raio::PILULA)
            .tooltip("Aprovar em outro modo")
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                let (editando, perguntando) = (aprovar_editando.clone(), aprovar_perguntando.clone());
                menu.item(
                    PopupMenuItem::new("Aprovar e aceitar edições")
                        .icon(icone_do_modo("acceptEdits", false).0)
                        .on_click(move |e, window, cx| editando(e, window, cx)),
                )
                .item(
                    PopupMenuItem::new("Aprovar e perguntar antes")
                        .icon(icone_do_modo("manual", false).0)
                        .on_click(move |e, window, cx| perguntando(e, window, cx)),
                )
            });
        let mudar = botao_fantasma(
            id_mudar,
            None,
            if com_texto { "Enviar mudanças" } else { "Pedir mudanças" },
            TamanhoBotao::Controle,
            window,
            cx,
        )
        .on_click(mudar);
        let par = h_flex().gap(px(1.)).child(aprovar.when(minima, |b| b.flex_1())).child(menu);
        let rodape = if minima {
            v_flex().gap(r(8.)).child(par.w_full()).child(mudar.w_full()).into_any_element()
        } else {
            h_flex().gap(r(8.)).child(mudar).child(div().flex_1()).child(par).into_any_element()
        };
        cartao
            .max_h(r(520.))
            // Quando falta altura, é o texto que encolhe e rola; cabeçalho, campo e botões ficam.
            .min_h(MINIMO_PLANO)
            .child(cabecalho.child(copiar_plano))
            .child(
                div()
                    .id(id_rolagem.clone())
                    .flex_1()
                    .min_h_0()
                    .px(dentro_da_borda(12.))
                    .py(esp::E12)
                    .corpo()
                    .text_color(Cores::texto())
                    .child(texto)
                    .overflow_y_scrollbar()
                    .id(id_rolagem),
            )
            .when_some(campo, |s, campo| {
                s.child(
                    div().flex_none().px(dentro_da_borda(12.)).child(
                        div()
                            .h(r(32.))
                            .px(esp::E4)
                            .flex()
                            .items_center()
                            .rounded(raio::CONTROLE)
                            .bg(Cores::recuo())
                            .on_action(enter)
                            .child(Input::new(&campo).appearance(false).w_full()),
                    ),
                )
            })
            .child(div().flex_none().px(dentro_da_borda(12.)).pt(esp::E12).pb(dentro_da_borda(12.)).child(rodape))
        .into_any_element()
    }
}

/// Largura máxima da coluna de leitura: 860 px com o texto em 16 px. Em rem, para o zoom não
/// mudar quantas letras cabem na linha.
const COLUNA: Rems = rems(53.75);
/// Margem da coluna de leitura de cada lado (12 na largura mínima e na caixa estreita).
pub(super) const MARGEM_DA_COLUNA: Rems = esp::E16;
/// Altura que as mensagens guardam mesmo em janela baixa (120 px).
const MINIMO_MENSAGENS: Rems = rems(7.5);
/// Até aqui o plano encolhe: título, umas linhas do texto, o campo e os botões (200 px).
const MINIMO_PLANO: Rems = rems(12.5);

/// Coluna de leitura comum: mensagens, pedidos de permissão, plano e caixa de mensagem alinham
/// pelas mesmas bordas em qualquer largura de painel.
pub(super) fn coluna<E: Styled>(elemento: E) -> E {
    elemento.w_full().max_w(COLUNA).mx_auto().px(MARGEM_DA_COLUNA)
}

/// O painel está no fim da conversa (ou a menos de 48 px dele).
/// Quanto falta rolar até o fim. O deslocamento da roda só é limitado no prepaint: aqui ele
/// conta já limitado, para uma conversa curta não parecer fora do fim.
fn distancia_do_fim(rolagem: &ScrollHandle) -> Pixels {
    let maximo = rolagem.max_offset().y;
    maximo + rolagem.offset().y.clamp(-maximo, px(0.))
}

/// Pílula para voltar ao fim de um painel rolado para cima: só a seta, ou "Novas mensagens" com o
/// ponto azul quando chegou coisa nova desde que o usuário subiu. Entra com fade e 8 px de baixo
/// (`nova` no primeiro quadro em que aparece).
fn pilula_do_fim(
    painel_id: u64,
    novidade: bool,
    minima: bool,
    nova: bool,
    cx: &Context<Janela>,
) -> impl IntoElement + use<> {
    let com_rotulo = novidade && !minima;
    let pilula = h_flex()
        .id(("ir-ao-fim", painel_id as usize))
        .tab_index(0)
        .focus_visible(|s| s.shadow(halo_foco()))
        .h(r(28.))
        .gap(esp::E6)
        .justify_center()
        .flutuante(cx)
        .rounded_full()
        .map(|s| if novidade { s.px(if com_rotulo { esp::E12 } else { esp::E8 }) } else { s.w(r(28.)) })
        .text_color(Cores::texto())
        .cursor_pointer()
        // Opaco também no hover: a pílula fica por cima do texto.
        .hover(|s| s.bg(Cores::flutuante().blend(Cores::pressionado())))
        .child(icone_de(Lucide::ArrowDown, 14.))
        .when(novidade, |s| s.child(div().flex_none().size(r(6.)).rounded_full().bg(Cores::destaque())))
        .when(com_rotulo, |s| s.child(div().rotulo().child("Novas mensagens")))
        .tooltip(|window, cx| Tooltip::new("Ir para o fim").action(&IrAoFim, None).build(window, cx))
        .on_click(cx.listener(move |this, _, _, cx| this.ir_para_o_fim(painel_id, cx)));
    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom(esp::E12)
        .flex()
        .justify_center()
        .child(movimento::entrada(("entra-pilula-fim", painel_id as usize), nova, De::Baixo, pilula))
}

/// O que o agente faz agora, tirado da última chamada ainda sem resultado no turno ("Lendo
/// motor.rs…"); sem chamada, "Escrevendo…" ou "Pensando…".
fn texto_atividade(itens: &[Item], escrevendo: bool) -> String {
    let atual = itens
        .iter()
        .rev()
        .take_while(|i| !matches!(i, Item::Usuario { .. } | Item::Entrega { .. } | Item::Aviso { .. }))
        .find_map(|i| match i {
            Item::Ferramenta {
                nome,
                resumo,
                resultado: None,
                ..
            } if nome != "ToolSearch" => Some((nome.as_str(), resumo.as_str())),
            _ => None,
        });
    let Some((nome, resumo)) = atual else {
        return if escrevendo { "Escrevendo…" } else { "Pensando…" }.to_string();
    };
    let detalhe = resumo
        .split_once(" · ")
        .map(|(_, d)| d.trim_end_matches('…').trim())
        .filter(|d| !d.is_empty());
    let verbo = verbo_da_ferramenta(nome, true);
    // "Lendo motor.rs…", "Rodando: cargo test…"; sem detalhe, "Lendo um arquivo…".
    let com = |dois_pontos: bool, sem: &str| match detalhe {
        Some(d) if dois_pontos => format!("{verbo}: {d}…"),
        Some(d) => format!("{verbo} {d}…"),
        None => format!("{verbo} {sem}…"),
    };
    match nome_curto(nome) {
        "Read" | "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => com(false, "um arquivo"),
        "Bash" | "BashOutput" => com(true, "um comando"),
        "Grep" => com(false, "nos arquivos"),
        "Glob" => com(false, "os arquivos"),
        "WebFetch" => com(false, "uma página"),
        "WebSearch" => com(true, "na web"),
        "Task" | "Agent" => match detalhe {
            Some(d) => format!("Subagente: {d}…"),
            None => "Rodando um subagente…".into(),
        },
        "Workflow" => match detalhe {
            Some(d) => format!("Fluxo de agentes: {d}…"),
            None => "Montando o fluxo de agentes…".into(),
        },
        "criar_tarefa" => match detalhe {
            Some(d) => format!("Criando a tarefa “{d}”…"),
            None => "Criando uma tarefa…".into(),
        },
        "ver_diff" => match detalhe {
            Some(d) => format!("Olhando as mudanças da {d}…"),
            None => "Olhando as mudanças…".into(),
        },
        "pedir_ajuste" => match detalhe {
            Some(d) => format!("Pedindo ajuste na {d}…"),
            None => "Pedindo um ajuste…".into(),
        },
        "aprovar_tarefa" => match detalhe {
            Some(d) => format!("Juntando a {d}…"),
            None => "Juntando uma tarefa…".into(),
        },
        "cancelar_tarefa" => match detalhe {
            Some(d) => format!("Cancelando a {d}…"),
            None => "Cancelando uma tarefa…".into(),
        },
        "perguntar_coordenador" => "Perguntando ao coordenador…".into(),
        "mensagem_para_chat" => "Mandando mensagem a outra conversa…".into(),
        "TodoWrite" | "listar_tarefas" | "listar_chats" | "anotar_decisao" | "informar_progresso" => {
            format!("{verbo}…")
        }
        outro => match outro.strip_prefix("mcp__") {
            Some(resto) => format!("Usando {}…", resto.rsplit("__").next().unwrap_or(resto)),
            None => format!("{}…", resumo.trim_end_matches('…')),
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MarcaDeLinha {
    Igual,
    Sai,
    Entra,
    /// Divisa entre as edições de um MultiEdit.
    Separador,
}

/// As linhas de uma edição para o pedido de permissão: o que sai e o que entra, com até duas
/// linhas iguais de cada lado como contexto.
fn linhas_da_edicao<'a>(antigo: &'a str, novo: &'a str) -> Vec<(MarcaDeLinha, &'a str)> {
    let a: Vec<&str> = antigo.lines().collect();
    let b: Vec<&str> = novo.lines().collect();
    let comeco = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let fim = a[comeco..]
        .iter()
        .rev()
        .zip(b[comeco..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let mut linhas: Vec<(MarcaDeLinha, &str)> = Vec::new();
    linhas.extend(a[comeco.saturating_sub(2)..comeco].iter().map(|l| (MarcaDeLinha::Igual, *l)));
    linhas.extend(a[comeco..a.len() - fim].iter().map(|l| (MarcaDeLinha::Sai, *l)));
    linhas.extend(b[comeco..b.len() - fim].iter().map(|l| (MarcaDeLinha::Entra, *l)));
    linhas.extend(a[a.len() - fim..].iter().take(2).map(|l| (MarcaDeLinha::Igual, *l)));
    linhas
}

/// Trechos (em bytes) de um comando que apagam, forçam ou rodam algo difícil de desfazer. A caixa
/// do sino usa para trocar o "Permitir" por "Permitir mesmo assim".
pub(super) fn trechos_perigosos(comando: &str) -> Vec<std::ops::Range<usize>> {
    const PERIGOSOS: [&str; 27] = [
        "rm", "rm -rf", "rm -fr", "rm -r", "sudo", "--force", "git push -f", "git reset --hard",
        "git clean -f", "git branch -D", "git stash drop", "git stash clear", "--no-verify",
        "-delete", "| sh", "| bash", "|sh", "|bash", "mkfs", "dd if=", "chmod -R", "chown -R",
        "kill -9", "pkill", "killall", "shutdown", "reboot",
    ];
    // "rm" não vale dentro de "--rm" nem de "rmdir"; "sudo" não vale em "pseudo".
    let colado_antes = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
    let colado_depois = |c: char| c.is_alphanumeric() || c == '_';
    let mut trechos = Vec::new();
    for padrao in PERIGOSOS {
        let ultimo = padrao.rsplit(' ').next().unwrap_or(padrao);
        let abre = padrao.starts_with(char::is_alphanumeric);
        let fecha = !ultimo.starts_with('-') && ultimo.ends_with(char::is_alphanumeric);
        for (inicio, _) in comando.match_indices(padrao) {
            let fim = inicio + padrao.len();
            if abre && comando[..inicio].chars().next_back().is_some_and(colado_antes) {
                continue;
            }
            if fecha && comando[fim..].chars().next().is_some_and(colado_depois) {
                continue;
            }
            trechos.push(inicio..fim);
        }
    }
    trechos.sort_by_key(|t| t.start);
    let mut juntos: Vec<std::ops::Range<usize>> = Vec::new();
    for trecho in trechos {
        match juntos.last_mut() {
            Some(ultimo) if trecho.start <= ultimo.end => ultimo.end = ultimo.end.max(trecho.end),
            _ => juntos.push(trecho),
        }
    }
    juntos
}

/// O caminho de um pedido, relativo à pasta do chat; o `bool` diz se ele fica fora dela.
/// Usa o mesmo critério da regra de "permitir sempre", para o aviso e a regra não discordarem.
fn caminho_no_chat(caminho: &str, pasta: &std::path::Path) -> (String, bool) {
    let caminho = std::path::Path::new(caminho);
    let fora = !crate::modelo::dentro_da_pasta(caminho, pasta);
    match caminho.strip_prefix(pasta) {
        Ok(relativo) if !fora && !relativo.as_os_str().is_empty() => (relativo.display().to_string(), false),
        _ => (caminho.display().to_string(), fora),
    }
}

/// Título e subtítulo do pedido de permissão ("Rodar este comando?" e a descrição dele).
fn titulo_do_pedido(ferramenta: &str, entrada: &serde_json::Value, pasta: &Path) -> (String, Option<String>) {
    let campo = |chave: &str| entrada[chave].as_str().filter(|s| !s.is_empty());
    let caminho = campo("file_path").or(campo("notebook_path"));
    let arquivo = caminho
        .and_then(|c| Path::new(c).file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "o arquivo".into());
    let relativo = caminho.map(|c| caminho_no_chat(c, pasta).0);
    match ferramenta {
        "Bash" => ("Rodar este comando?".into(), campo("description").map(str::to_string)),
        "Edit" => (format!("Editar {arquivo}?"), relativo),
        "MultiEdit" => {
            let trechos = entrada["edits"].as_array().map_or(0, Vec::len);
            let detalhe = contagem(trechos, "trecho", "trechos");
            (format!("Editar {arquivo}?"), Some(relativo.map_or(detalhe.clone(), |c| format!("{c} · {detalhe}"))))
        }
        "Write" => {
            let linhas = contagem(campo("content").map_or(0, |c| c.lines().count()), "linha", "linhas");
            (format!("Gravar {arquivo}?"), Some(relativo.map_or(linhas.clone(), |c| format!("{c} · {linhas}"))))
        }
        "NotebookEdit" if campo("edit_mode") == Some("delete") => (format!("Apagar uma célula de {arquivo}?"), relativo),
        "NotebookEdit" => (format!("Editar uma célula de {arquivo}?"), relativo),
        "WebFetch" => {
            let url = campo("url").unwrap_or_default();
            let dominio = url.split("://").last().unwrap_or(url).split('/').next().unwrap_or_default();
            (format!("Abrir {dominio}?"), Some(url.to_string()).filter(|u| !u.is_empty()))
        }
        "WebSearch" => ("Pesquisar na web?".into(), campo("query").map(str::to_string)),
        _ => {
            let resumo = Some(crate::claude::resumo_ferramenta(ferramenta, entrada));
            match ferramenta.strip_prefix("mcp__").and_then(|n| n.split_once("__")) {
                Some((servidor, nome)) => (format!("Usar {nome} ({servidor})?"), resumo),
                None => (format!("Usar {}?", nome_curto(ferramenta)), resumo),
            }
        }
    }
}

/// Rótulo e dica do botão de "permitir sempre": o que passa a valer sem perguntar.
fn rotulo_da_regra(regra: &str, manual: bool) -> (String, String) {
    const DESFAZER: &str = "Para desfazer, use o menu do modo, embaixo da caixa de mensagem.";
    if regra == "edicoes" {
        let modo = if manual {
            " Nos próximos turnos, a conversa fica em Aceitar edições."
        } else {
            ""
        };
        return (
            "Permitir edições nesta conversa".into(),
            format!(
                "As edições de arquivos desta pasta passam sem perguntar, já neste turno.{modo} \
                 {DESFAZER}"
            ),
        );
    }
    if regra.starts_with("bash:") {
        return (
            "Sempre permitir este comando".into(),
            format!("Este comando, exatamente igual, passa sem perguntar nesta conversa. {DESFAZER}"),
        );
    }
    let nome = nome_curto(regra.strip_prefix("ferramenta:").unwrap_or(regra));
    (
        format!("Sempre permitir {nome}"),
        format!("{nome} passa sem perguntar nesta conversa. {DESFAZER}"),
    )
}

/// O que o pedido vai fazer, por inteiro: o comando com os trechos arriscados em vermelho, o
/// trecho que a edição troca, o começo do arquivo novo ou, nas outras ferramentas, a entrada.
fn corpo_permissao(id: &str, ferramenta: &str, entrada: &serde_json::Value) -> AnyElement {
    let campo = |chave: &str| entrada[chave].as_str().unwrap_or_default();
    let nota = |texto: &str| div().metadado().text_color(Cores::tenue()).child(texto.to_string());
    match ferramenta {
        "Bash" => {
            let comando = campo("command").trim_end().to_string();
            let perigosos = trechos_perigosos(&comando);
            let vermelho = HighlightStyle {
                color: Some(Cores::vermelho()),
                font_weight: Some(FontWeight::SEMIBOLD),
                ..Default::default()
            };
            caixa_de_codigo(id)
                .text_color(Cores::texto())
                .child(StyledText::new(comando).with_highlights(perigosos.iter().map(|t| (t.clone(), vermelho))))
                .into_any_element()
        }
        "Edit" => v_flex()
            .gap(r(6.))
            .child(bloco_de_linhas(id, &linhas_da_edicao(campo("old_string"), campo("new_string")), 80))
            .when(entrada["replace_all"].as_bool() == Some(true), |s| {
                s.child(nota("Troca todas as ocorrências no arquivo."))
            })
            .into_any_element(),
        "MultiEdit" => {
            let mut linhas = Vec::new();
            for (i, edicao) in entrada["edits"].as_array().into_iter().flatten().enumerate() {
                if i > 0 {
                    linhas.push((MarcaDeLinha::Separador, ""));
                }
                linhas.extend(linhas_da_edicao(
                    edicao["old_string"].as_str().unwrap_or_default(),
                    edicao["new_string"].as_str().unwrap_or_default(),
                ));
            }
            bloco_de_linhas(id, &linhas, 80).into_any_element()
        }
        "Write" => {
            let linhas: Vec<_> = campo("content").lines().map(|l| (MarcaDeLinha::Entra, l)).collect();
            bloco_de_linhas(id, &linhas, 20).into_any_element()
        }
        "NotebookEdit" if campo("edit_mode") == "delete" => nota("Apaga a célula.").into_any_element(),
        "NotebookEdit" => {
            let linhas: Vec<_> = campo("new_source").lines().map(|l| (MarcaDeLinha::Entra, l)).collect();
            bloco_de_linhas(id, &linhas, 20).into_any_element()
        }
        _ => {
            let json = serde_json::to_string_pretty(entrada).unwrap_or_default();
            let linhas: Vec<(MarcaDeLinha, &str)> = json.lines().map(|l| (MarcaDeLinha::Igual, l)).collect();
            if entrada.as_object().is_some_and(|o| !o.is_empty()) {
                bloco_de_linhas(id, &linhas, 40).into_any_element()
            } else {
                nota("Sem parâmetros.").into_any_element()
            }
        }
    }
}

/// A caixa de código do pedido: preta, raio de controle, mono 12/18 e rolagem a partir de 220.
fn caixa_de_codigo(id: &str) -> Stateful<Div> {
    v_flex()
        .id(SharedString::from(format!("permissao-corpo-{id}")))
        .max_h(r(220.))
        .overflow_y_scroll()
        .px(esp::E12)
        .py(esp::E8)
        .rounded(raio::CONTROLE)
        .bg(Cores::preto())
        .font_family(FONTE_MONO)
        .text_size(r(12.))
        .line_height(r(18.))
}

/// Linhas com o sinal de cada uma ("−" sai, "+" entra) na caixa de código; passando de
/// `limite`, diz quantas ficaram de fora. O sinal vai na cor e o texto em `texto`.
fn bloco_de_linhas(id: &str, linhas: &[(MarcaDeLinha, &str)], limite: usize) -> impl IntoElement + use<> {
    let resto = linhas.len().saturating_sub(limite);
    caixa_de_codigo(id)
        .children(linhas.iter().take(limite).map(|(marca, texto)| {
            if *marca == MarcaDeLinha::Separador {
                return div().my(r(4.)).h(px(1.)).bg(Cores::borda()).into_any_element();
            }
            let (sinal, cor, fundo): (&str, Hsla, Option<Hsla>) = match marca {
                MarcaDeLinha::Sai => ("−", Cores::vermelho(), Some(fundo_sai())),
                MarcaDeLinha::Entra => ("+", Cores::verde(), Some(fundo_entra())),
                _ => (" ", Cores::tenue(), None),
            };
            let texto = if texto.is_empty() { " ".to_string() } else { texto.replace('\t', "    ") };
            h_flex()
                .items_start()
                .gap(r(8.))
                .px(r(4.))
                .text_color(if fundo.is_some() { Cores::texto() } else { Cores::tenue() })
                .when_some(fundo, |s, f| s.bg(f))
                .child(div().flex_none().w(r(8.)).text_color(cor).child(sinal))
                .child(div().flex_1().min_w_0().child(texto))
                .into_any_element()
        }))
        .when(linhas.is_empty(), |s| s.child(div().text_color(Cores::tenue()).child("(vazio)")))
        .when(resto > 0, |s| {
            s.child(div().pt(r(4.)).text_color(Cores::tenue()).child(format!("+{resto} linhas")))
        })
}

#[cfg(test)]
mod testes {
    // Sem `super::*`: o `gpui_kit::*` lá de cima traz um `test` que troca o `#[test]`.
    use super::{
        Bloco, ClasseAvisoChat, MarcaDeLinha, Segmento, Tipo, caminho_no_chat, com_maiuscula, comando_de_barra,
        detalhe_da_chamada, entrada_do_bloco, espaco_antes, finais_de_turno, frase_da_entrega, linhas_da_edicao, meta_curta,
        meta_do_resumo, nome_e_id, partes_do_erro, previa, resumo_da_trilha, segmentar, sem_marcacao, tamanho_curto,
        tarefa_do_item, texto_atividade, tipo_aviso, titulo_do_pedido, trechos_perigosos, verbo_da_acao,
        verbo_da_ferramenta,
    };
    use crate::modelo::{Chat, EstadoTarefa, Item, regra_de_permissao};
    use serde_json::json;
    use std::path::Path;

    fn ferramenta(nome: &str, resumo: &str, resultado: Option<&str>, erro: bool) -> Item {
        Item::Ferramenta {
            id: "x".into(),
            nome: nome.into(),
            resumo: resumo.into(),
            resultado: resultado.map(str::to_string),
            erro,
        }
    }

    fn entrega(de: &str, texto: &str) -> Item {
        Item::Entrega {
            de: de.into(),
            texto: texto.into(),
            ponto: None,
        }
    }

    fn usuario(texto: &str) -> Item {
        Item::Usuario { texto: texto.into(), anexos: Vec::new(), ponto: None, ultrathink: false, ultracode: false }
    }

    fn assistente(texto: &str) -> Item {
        Item::Assistente { texto: texto.into() }
    }

    #[test]
    fn acha_a_tarefa_nos_textos_do_vitral() {
        let criada = ferramenta(
            "mcp__vitral__criar_tarefa",
            "criar_tarefa · Validar entrada",
            Some("Tarefa #12 criada; trabalhador abc iniciado."),
            false,
        );
        assert_eq!(tarefa_do_item(&criada), Some(12));
        let criando = ferramenta("mcp__vitral__criar_tarefa", "criar_tarefa · Validar", None, false);
        assert_eq!(tarefa_do_item(&criando), None);
        let falhou = ferramenta("mcp__vitral__criar_tarefa", "criar_tarefa · X", Some("Tarefa #3 deu ruim"), true);
        assert_eq!(tarefa_do_item(&falhou), None);
        let diff = ferramenta("mcp__vitral__ver_diff", "ver_diff · tarefa #7", None, false);
        assert_eq!(tarefa_do_item(&diff), Some(7));
        let ajuste = ferramenta("mcp__vitral__pedir_ajuste", "pedir_ajuste · tarefa #40", Some("ok"), false);
        assert_eq!(tarefa_do_item(&ajuste), Some(40));
        // Ferramentas que não agem sobre uma tarefa ficam de fora, mesmo com "#N" no texto.
        let anotar = ferramenta("mcp__vitral__anotar_decisao", "anotar_decisao · tarefa #2", None, false);
        assert_eq!(tarefa_do_item(&anotar), None);
        let bash = ferramenta("Bash", "Bash · tarefa #2", None, false);
        assert_eq!(tarefa_do_item(&bash), None);

        let terminou = entrega("Vitral", "A tarefa #5 \"Login\" terminou a rodada 2.\n\nRelatório #9");
        assert_eq!(tarefa_do_item(&terminou), Some(5));
        let aprovou = entrega("Vitral", "O usuário aprovou a tarefa #8 pelo painel. Tarefa #8 aprovada");
        assert_eq!(tarefa_do_item(&aprovou), Some(8));
        let pergunta = entrega("#3 Login", "Pergunta do trabalhador da tarefa #3 (#3 Login): qual porta?");
        assert_eq!(tarefa_do_item(&pergunta), Some(3));
        let outro_chat = entrega("Coordenador do site (abc)", "Oi #4");
        assert_eq!(tarefa_do_item(&outro_chat), None);
        assert_eq!(tarefa_do_item(&usuario("veja a tarefa #2")), None);
    }

    #[test]
    fn frase_curta_da_entrega_sem_repetir_numero_e_titulo() {
        let titulo = "Validar \"entrada\"";
        assert_eq!(
            frase_da_entrega(
                "A tarefa #1 \"Validar \"entrada\"\" terminou a rodada 1 com ERRO.\n\nRelatório:",
                1,
                titulo
            ),
            "terminou a rodada 1 com ERRO"
        );
        // Título que mudou desde a mensagem: corta no fim das aspas.
        assert_eq!(
            frase_da_entrega("A tarefa #2 \"Antigo\" terminou a rodada 3.", 2, "Novo"),
            "terminou a rodada 3"
        );
        assert_eq!(
            frase_da_entrega("O usuário aprovou a tarefa #4 pelo painel. Tarefa #4 aprovada", 4, "x"),
            "você aprovou pelo painel"
        );
        assert_eq!(
            frase_da_entrega("O usuário pediu ajustes na tarefa #4 direto ao trabalhador:\nmais testes", 4, "x"),
            "você pediu ajustes direto ao trabalhador"
        );
        assert_eq!(
            frase_da_entrega("Pergunta do trabalhador da tarefa #3 (#3 Login): qual porta?\n\nResponda", 3, "Login"),
            "perguntou: qual porta?"
        );
        assert_eq!(frase_da_entrega("\n  Já terminei o que dava.\n", 3, "Login"), "Já terminei o que dava");
    }

    #[test]
    fn trilha_resume_pelo_verbo_na_ordem_em_que_aparece() {
        assert_eq!(resumo_da_trilha(&["Read", "Bash", "Read", "Edit", "Read"]), "leu 3 · rodou 1 · editou 1");
        // Edit e MultiEdit contam juntos; ferramenta MCP de fora vira "usou".
        assert_eq!(resumo_da_trilha(&["Edit", "MultiEdit", "mcp__github__create_issue"]), "editou 2 · usou 1");
        assert_eq!(resumo_da_trilha(&[]), "");
    }

    #[test]
    fn verbo_de_cada_ferramenta() {
        assert_eq!(verbo_da_ferramenta("Read", false), "Leu");
        assert_eq!(verbo_da_ferramenta("Read", true), "Lendo");
        assert_eq!(verbo_da_ferramenta("Write", false), "Gravou");
        assert_eq!(verbo_da_ferramenta("BashOutput", false), "Rodou");
        assert_eq!(verbo_da_ferramenta("Glob", true), "Listando");
        assert_eq!(verbo_da_ferramenta("TodoWrite", false), "Atualizou os passos");
        assert_eq!(verbo_da_ferramenta("mcp__vitral__perguntar_coordenador", true), "Perguntando");
        assert_eq!(verbo_da_ferramenta("mcp__github__create_issue", false), "Usou");
        // Ferramenta desconhecida: o nome curto.
        assert_eq!(verbo_da_ferramenta("KillShell", false), "KillShell");
        assert_eq!(detalhe_da_chamada("mcp__github__create_issue", "mcp__github__create_issue"), "github · create_issue");
        assert_eq!(detalhe_da_chamada("Read", "Read · motor.rs"), "motor.rs");
        assert_eq!(detalhe_da_chamada("mcp__vitral__anotar_decisao", "anotar_decisao · usar Decimal"), "usar Decimal");
        assert_eq!(detalhe_da_chamada("TodoWrite", "TodoWrite"), "");
    }

    #[test]
    fn chamadas_seguidas_se_dividem_por_tipo() {
        use Tipo::*;
        let segmentos = segmentar(&[Comum, Comum, Criacao, Criacao, Criacao, Tarefa, Tarefa, Comum, Criacao]);
        assert_eq!(
            segmentos,
            vec![
                (Segmento::Trilha, 0..2),
                (Segmento::Criacoes, 2..5),
                (Segmento::Evento, 5..6),
                (Segmento::Evento, 6..7),
                (Segmento::Trilha, 7..8),
                (Segmento::Evento, 8..9),
            ]
        );
        assert!(segmentar(&[]).is_empty());
    }

    #[test]
    fn verbo_segue_o_resultado_da_acao() {
        assert_eq!(verbo_da_acao("aprovar_tarefa", Some("Tarefa #1 aprovada e juntada"), false), "Aprovou e juntou");
        assert_eq!(verbo_da_acao("aprovar_tarefa", Some("Conflito com o que já entrou"), true), "Não juntou");
        assert_eq!(
            verbo_da_acao("aprovar_tarefa", Some("Tarefa #1 aprovada; ela não tinha mudanças para juntar."), false),
            "Aprovou sem mudanças"
        );
        assert_eq!(
            verbo_da_acao(
                "aprovar_tarefa",
                Some("Tarefa #1 aprovada. Ela rodou direto na pasta do projeto, então não há o que juntar."),
                false
            ),
            "Aprovou"
        );
        assert_eq!(verbo_da_acao("pedir_ajuste", None, false), "Pedindo ajuste em");
        assert_eq!(verbo_da_acao("criar_tarefa", Some("Tarefa #2 criada"), false), "Criou");
    }

    #[test]
    fn espaco_entre_os_blocos_da_conversa() {
        use Bloco::*;
        assert_eq!(espaco_antes(None, Balao), 0.);
        // Entre turnos, 24 antes do balão; 16 depois dele. Nada fica reservado para as ações.
        assert_eq!(espaco_antes(Some(Texto), Balao), 24.);
        assert_eq!(espaco_antes(Some(Log), Balao), 24.);
        assert_eq!(espaco_antes(Some(Balao), Texto), 16.);
        assert_eq!(espaco_antes(Some(Balao), Marco), 16.);
        // Linhas de log coladas, no passo de 28; recebida seguida de log, quase.
        assert_eq!(espaco_antes(Some(Log), Log), 0.);
        assert_eq!(espaco_antes(Some(Recebida), Log), 4.);
        assert_eq!(espaco_antes(Some(Recebida), Recebida), 4.);
        assert_eq!(espaco_antes(Some(Log), Recebida), 12.);
        // Marco e resumo pedem ar dos dois lados.
        assert_eq!(espaco_antes(Some(Texto), Marco), 16.);
        assert_eq!(espaco_antes(Some(Marco), Log), 16.);
        assert_eq!(espaco_antes(Some(Log), Resumo), 16.);
        // O resto, dentro do turno.
        assert_eq!(espaco_antes(Some(Texto), Log), 12.);
        assert_eq!(espaco_antes(Some(Log), Cartao), 12.);
        assert_eq!(espaco_antes(Some(Alerta), Texto), 12.);
    }

    #[test]
    fn so_entra_o_que_chegou_com_a_conversa_aberta() {
        // Abriu com 5 itens: base e vistos em 5.
        assert_eq!(entrada_do_bloco(Some(3), 5, 5), None);
        assert_eq!(entrada_do_bloco(None, 5, 5), None);
        // O item 5 chega: novo neste quadro; no seguinte, segue guardado mas sem recomeçar.
        assert_eq!(entrada_do_bloco(Some(5), 5, 5), Some(true));
        assert_eq!(entrada_do_bloco(Some(5), 5, 6), Some(false));
        // O texto do streaming entra no índice em que vai pousar: ao pousar, não entra de novo.
        assert_eq!(entrada_do_bloco(Some(6), 5, crate::ui::paineis::tamanho_da_conversa(6, true)), Some(false));
    }

    #[test]
    fn meta_do_cartao_de_entrega_curta() {
        assert_eq!(meta_curta(1, false, Some(false), Some((false, 4))), "Rodada 1 · verificação falhou · 4 observações do revisor");
        assert_eq!(
            meta_curta(2, false, Some(true), Some((true, 1))),
            "Rodada 2 · verificação passou · revisor aprovaria · 1 observação"
        );
        assert_eq!(meta_curta(3, false, None, Some((false, 0))), "Rodada 3 · revisor pediu ajustes");
        assert_eq!(meta_curta(1, true, None, None), "Rodada 1 com erro");
        assert_eq!(com_maiuscula("terminou a rodada 1"), "Terminou a rodada 1");
        assert_eq!(com_maiuscula(""), "");
    }

    #[test]
    fn ultimo_texto_de_cada_turno() {
        let itens = [
            usuario("oi"),
            assistente("vou ler"),
            ferramenta("Read", "Read · a.rs", Some("ok"), false),
            assistente("pronto"),
            entrega("Vitral", "A tarefa #1 \"x\" terminou"),
            assistente("certo"),
            usuario("valeu"),
        ];
        let finais = finais_de_turno(&itens);
        assert!(finais.contains(&3) && finais.contains(&5));
        assert_eq!(finais.len(), 2);
        assert!(finais_de_turno(&[assistente("só")]).contains(&0));
    }

    #[test]
    fn classe_de_cada_aviso() {
        assert_eq!(tipo_aviso("Interrompido.", false), ClasseAvisoChat::Marco);
        assert_eq!(tipo_aviso("Sessão nova: o Claude não lembra das mensagens acima.", false), ClasseAvisoChat::Marco);
        assert_eq!(tipo_aviso("Retomando a sessão anterior.", false), ClasseAvisoChat::Marco);
        assert_eq!(tipo_aviso("Tentando de novo.", false), ClasseAvisoChat::Info);
        assert_eq!(
            tipo_aviso("A conta x chegou ao limite do plano; continuando na conta y.", false),
            ClasseAvisoChat::Info
        );
        assert_eq!(tipo_aviso("Falhou: o processo caiu", true), ClasseAvisoChat::Alerta);
        assert_eq!(
            partes_do_erro("O turno terminou com erro: limite"),
            (Some("O turno terminou com erro".into()), "limite".into())
        );
        assert_eq!(partes_do_erro("Erro sem detalhe"), (None, "Erro sem detalhe".into()));
        assert_eq!(partes_do_erro("Falhou: "), (None, "Falhou: ".into()));
    }

    #[test]
    fn titulo_e_subtitulo_de_cada_pedido() {
        let pasta = Path::new("/home/v/loja");
        let titulo = |f: &str, e: serde_json::Value| titulo_do_pedido(f, &e, pasta);
        assert_eq!(
            titulo("Bash", json!({"command": "cargo test", "description": "Roda os testes"})),
            ("Rodar este comando?".into(), Some("Roda os testes".into()))
        );
        assert_eq!(titulo("Bash", json!({"command": "ls"})), ("Rodar este comando?".into(), None));
        assert_eq!(
            titulo("Edit", json!({"file_path": "/home/v/loja/src/cupom.rs"})),
            ("Editar cupom.rs?".into(), Some("src/cupom.rs".into()))
        );
        assert_eq!(
            titulo("MultiEdit", json!({"file_path": "/home/v/loja/a.rs", "edits": [{}, {}, {}]})),
            ("Editar a.rs?".into(), Some("a.rs · 3 trechos".into()))
        );
        assert_eq!(
            titulo("Write", json!({"file_path": "/etc/hosts", "content": "a\nb"})),
            ("Gravar hosts?".into(), Some("/etc/hosts · 2 linhas".into()))
        );
        assert_eq!(
            titulo("NotebookEdit", json!({"notebook_path": "/home/v/loja/n.ipynb", "edit_mode": "delete"})).0,
            "Apagar uma célula de n.ipynb?"
        );
        assert_eq!(
            titulo("WebFetch", json!({"url": "https://docs.rs/gpui/latest"})),
            ("Abrir docs.rs?".into(), Some("https://docs.rs/gpui/latest".into()))
        );
        assert_eq!(titulo("WebSearch", json!({"query": "gpui"})).0, "Pesquisar na web?");
        assert_eq!(titulo("mcp__github__create_issue", json!({})).0, "Usar create_issue (github)?");
        assert_eq!(titulo("Agente", json!({})).0, "Usar Agente?");
    }

    #[test]
    fn textos_sem_marcacao_na_tela() {
        assert_eq!(sem_marcacao("Verificação falhou: `cargo test` e **dois** erros"), "Verificação falhou: cargo test e dois erros");
        assert_eq!(previa("Tarefa #2:\n\n  Aplicar `cupom`\nno checkout"), "Tarefa #2: Aplicar cupom no checkout");
        assert_eq!(comando_de_barra(" /compact "), Some("/compact"));
        assert_eq!(comando_de_barra("/clear\nmais"), None);
        assert_eq!(comando_de_barra("não é /comando"), None);
        assert_eq!(tamanho_curto(512), "512 B");
        assert_eq!(tamanho_curto(2048), "2 KB");
        assert_eq!(tamanho_curto(1_468_006), "1,4 MB");
    }

    #[test]
    fn nome_de_quem_mandou() {
        assert_eq!(nome_e_id("Coordenador do site (abc)"), ("Coordenador do site", Some("abc")));
        assert_eq!(nome_e_id("#3 Login"), ("#3 Login", None));
        assert_eq!(nome_e_id("Tarefa (com espaço)"), ("Tarefa (com espaço)", None));
    }

    #[test]
    fn resumo_conta_as_tarefas_por_estado() {
        use EstadoTarefa::*;
        assert_eq!(meta_do_resumo(&[Aprovada, Aprovada, Aprovada, Cancelada]), "3 tarefas aprovadas · 1 cancelada");
        assert_eq!(meta_do_resumo(&[Aprovada]), "1 tarefa aprovada");
        assert_eq!(meta_do_resumo(&[Falhou, Cancelada, Cancelada]), "2 canceladas · 1 falhou");
        assert_eq!(meta_do_resumo(&[]), "");
    }

    #[test]
    fn atividade_vem_da_chamada_em_andamento_do_turno() {
        let ler = ferramenta("Read", "Read · motor.rs", None, false);
        let feita = ferramenta("Bash", "Bash · Roda os testes", Some("ok"), false);
        assert_eq!(texto_atividade(&[usuario("oi"), feita.clone(), ler.clone()], false), "Lendo motor.rs…");
        // A última sem resultado é a que vale, mesmo com uma terminada depois.
        assert_eq!(texto_atividade(&[usuario("oi"), ler.clone(), feita.clone()], false), "Lendo motor.rs…");
        // Sem resultado de um turno anterior não conta: foi interrompida.
        assert_eq!(texto_atividade(&[ler.clone(), usuario("de novo")], false), "Pensando…");
        assert_eq!(texto_atividade(&[usuario("oi"), feita], true), "Escrevendo…");
        let bash = ferramenta("Bash", "Bash · cargo test -p vitral…", None, false);
        assert_eq!(texto_atividade(&[bash], false), "Rodando: cargo test -p vitral…");
        let criar = ferramenta("mcp__vitral__criar_tarefa", "criar_tarefa · Validar entrada", None, false);
        assert_eq!(texto_atividade(&[criar], false), "Criando a tarefa “Validar entrada”…");
        let diff = ferramenta("mcp__vitral__ver_diff", "ver_diff · tarefa #3", None, false);
        assert_eq!(texto_atividade(&[diff], false), "Olhando as mudanças da tarefa #3…");
        let outro = ferramenta("mcp__github__create_issue", "mcp__github__create_issue", None, false);
        assert_eq!(texto_atividade(&[outro], false), "Usando create_issue…");
        let sem_detalhe = ferramenta("Edit", "Edit", None, false);
        assert_eq!(texto_atividade(&[sem_detalhe], false), "Editando um arquivo…");
        let gravar = ferramenta("Write", "Write · cupom.rs", None, false);
        assert_eq!(texto_atividade(&[gravar], false), "Gravando cupom.rs…");
        let passos = ferramenta("TodoWrite", "TodoWrite · lista de tarefas", None, false);
        assert_eq!(texto_atividade(&[passos], false), "Atualizando os passos…");
    }

    #[test]
    fn edicao_mostra_o_que_sai_e_o_que_entra_com_pouco_contexto() {
        let antigo = "a\nb\nc\nvelho\nd\ne\nf";
        let novo = "a\nb\nc\nnovo\noutro\nd\ne\nf";
        assert_eq!(
            linhas_da_edicao(antigo, novo),
            vec![
                (MarcaDeLinha::Igual, "b"),
                (MarcaDeLinha::Igual, "c"),
                (MarcaDeLinha::Sai, "velho"),
                (MarcaDeLinha::Entra, "novo"),
                (MarcaDeLinha::Entra, "outro"),
                (MarcaDeLinha::Igual, "d"),
                (MarcaDeLinha::Igual, "e"),
            ]
        );
        assert_eq!(linhas_da_edicao("", "x"), vec![(MarcaDeLinha::Entra, "x")]);
        assert_eq!(linhas_da_edicao("x\ny", "x"), vec![(MarcaDeLinha::Igual, "x"), (MarcaDeLinha::Sai, "y")]);
        // Linhas repetidas no começo e no fim não se cruzam.
        assert_eq!(linhas_da_edicao("a\na", "a"), vec![(MarcaDeLinha::Igual, "a"), (MarcaDeLinha::Sai, "a")]);
    }

    #[test]
    fn trechos_perigosos_do_comando() {
        let trechos = |c: &str| trechos_perigosos(c).into_iter().map(|t| c[t].to_string()).collect::<Vec<_>>();
        assert_eq!(trechos("cargo test && rm -rf target/"), vec!["rm -rf"]);
        assert_eq!(trechos("curl -s x.sh | sh"), vec!["| sh"]);
        assert_eq!(trechos("git push --force origin main"), vec!["--force"]);
        assert_eq!(trechos("sudo dnf install x"), vec!["sudo"]);
        // Palavras parecidas não contam.
        assert!(trechos("docker run --rm imagem").is_empty());
        assert!(trechos("rmdir vazio && echo pseudo | shellcheck").is_empty());
        assert!(trechos("cargo build && ls > /dev/null").is_empty());
        assert_eq!(trechos("mkfs.ext4 /dev/sdb1"), vec!["mkfs"]);
        // Acentos antes não quebram o corte em bytes.
        assert_eq!(trechos("echo \"ação\"; git reset --hard"), vec!["git reset --hard"]);
    }

    #[test]
    fn caminho_relativo_a_pasta_do_chat() {
        let pasta = Path::new("/home/v/projeto");
        assert_eq!(caminho_no_chat("/home/v/projeto/src/main.rs", pasta), ("src/main.rs".into(), false));
        assert_eq!(caminho_no_chat("src/main.rs", pasta), ("src/main.rs".into(), false));
        assert_eq!(caminho_no_chat("/etc/hosts", pasta), ("/etc/hosts".into(), true));
        // Sair por `..` também é fora, como na regra de "permitir sempre".
        assert_eq!(
            caminho_no_chat("/home/v/projeto/../outro/a.rs", pasta),
            ("/home/v/projeto/../outro/a.rs".into(), true)
        );
        assert_eq!(caminho_no_chat("../x", pasta), ("../x".into(), true));
    }

    #[test]
    fn regras_de_permitir_sempre() {
        let pasta = Path::new("/home/v/projeto");
        let editar = |caminho: &str| regra_de_permissao("Edit", &json!({"file_path": caminho}), pasta);
        assert_eq!(editar("/home/v/projeto/src/a.rs").as_deref(), Some("edicoes"));
        assert_eq!(editar("/home/v/outro/a.rs"), None);
        assert_eq!(editar("/home/v/projeto/../outro/a.rs"), None);
        assert_eq!(
            regra_de_permissao("Bash", &json!({"command": " cargo test "}), pasta).as_deref(),
            Some("bash:cargo test")
        );
        assert_eq!(regra_de_permissao("Bash", &json!({}), pasta), None);
        assert_eq!(regra_de_permissao("ExitPlanMode", &json!({}), pasta), None);
        assert_eq!(regra_de_permissao("WebFetch", &json!({"url": "x"}), pasta).as_deref(), Some("ferramenta:WebFetch"));

        let mut chat: Chat = serde_json::from_value(json!({
            "id": "c", "projeto": "p", "titulo": "t", "papel": "Trabalhador", "sessao": "s",
            "sessao_criada": false, "cwd": "/home/v/projeto", "criado_em": 0, "atualizado_em": 0,
            "custo_usd": 0.0
        }))
        .unwrap();
        assert!(!chat.permite("Bash", &json!({"command": "cargo test"})));
        chat.permitidas = vec!["bash:cargo test".into(), "edicoes".into()];
        assert!(chat.permite("Bash", &json!({"command": "cargo test"})));
        assert!(!chat.permite("Bash", &json!({"command": "cargo test && rm -rf /"})));
        assert!(chat.permite("Write", &json!({"file_path": "/home/v/projeto/novo.txt"})));
        assert!(!chat.permite("Write", &json!({"file_path": "/home/v/.bashrc"})));
    }
}
