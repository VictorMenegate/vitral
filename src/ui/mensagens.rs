//! As mensagens de uma conversa: texto em markdown, ferramentas numa trilha calma, entregas de
//! outros chats, pedidos de permissão e plano, a faixa do turno e a rolagem
//! de cada painel.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use gpui_kit::component::{
    ActiveTheme as _, Icon, StyledExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    clipboard::Clipboard,
    h_flex,
    input::{Enter, Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::ScrollableElement as _,
    shimmer::ShimmerText,
    text::{TextView, TextViewStyle},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    IrAoFim, Janela,
    componentes::{
        Alerta, AnelGirando, BotaoCopiar, BotaoDeSegurar, CabecalhoDeSecao, EstadoDaMoldura, Etiqueta, azulejo,
        botao_claro, miolo_da_moldura, moldura_dupla, CaixaDeCodigo, EstadoVazio, FaixaCaixa, Flutuante as _,
        LarguraPainel, Papel as Quem, Selo, TamanhoBotao, Tom, botao_fantasma, botao_icone, botao_primario, relogio,
        contagem, icone_do_modo, teclas, texto_arco_iris,
    },
    busca_conversa::{self, Campo, Realce, RolarAoAchado, TextoAchado},
    composer::{self, PararTurno, nome_do_modo},
    imagens,
    movimento::{self, De},
    paineis::tamanho_da_conversa,
};
use crate::{
    modelo::{Chat, Execucao, Item, Reforco},
    motor::Motor,
    tema::{
        Cores, FONTE_MONO, Lucide, Tipo as _, aresta_de_poco, dentro_da_borda, esp, halo_foco, r, raio, tam,
    },
};

pub(super) mod historico;

/// O logo pequeno, para as mensagens que o próprio Vitral entrega.
static LOGO: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let png = include_bytes!("../../assets/vitral-64.png");
    Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec()))
});

/// Ícone no tamanho de desenho (cresce com o zoom), sem precisar da janela.
pub(super) fn icone_de(nome: impl Into<Icon>, lado: f32) -> Icon {
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
    /// Trilha de ferramentas e avisos de uma linha.
    Log,
    Recebida,
    Marco,
    Alerta,
}

/// Espaço antes de um bloco: 24 antes do seu balão (entre turnos), 16 depois dele, 12 dentro do
/// turno, 16 em volta de marco e nada entre linhas de log, que ficam coladas no passo
/// de 28. Nada fica reservado para as ações: elas flutuam no hover.
fn espaco_antes(anterior: Option<Bloco>, atual: Bloco) -> f32 {
    use Bloco::*;
    let Some(anterior) = anterior else {
        return 0.;
    };
    match (anterior, atual) {
        (_, Balao) => 24.,
        (Balao, _) => 16.,
        (Marco, _) | (_, Marco) => 16.,
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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn mensagens(
        &mut self,
        motor: &Motor,
        chat: &Chat,
        ix: usize,
        barra_busca: Option<AnyElement>,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let painel_id = self.paineis[ix].id;
        let rolagem = self.paineis[ix].rolagem.clone();
        let foco = self.paineis[ix].modo_foco;
        let nivel = self.paineis[ix].medida.nivel;
        let contexto = Contexto::novo(motor, chat, nivel, painel_id, &self.paineis[ix].busca.indice, &rolagem);
        let agora = crate::modelo::agora();
        // Cada bloco com o índice do (primeiro) item dele, que identifica a entrada.
        let mut blocos: Vec<(Bloco, Option<usize>, AnyElement)> = Vec::new();
        if chat.itens.is_empty() {
            blocos.push((Bloco::Texto, None, self.dica_inicial(motor, chat, window, cx)));
        }
        // Conversas antigas guardaram as buscas de ferramenta; não interessam. O modo foco tira
        // todas as ferramentas.
        let mut visiveis: Vec<(usize, &Item)> = chat
            .itens
            .iter()
            .enumerate()
            .filter(|(_, item)| match item {
                Item::Ferramenta { nome, .. } => nome != "ToolSearch" && !foco,
                _ => true,
            })
            .collect();
        // Só os últimos itens são desenhados (ver `ITENS_NA_TELA`); com a busca aberta, todos, para
        // o Ctrl+F achar e rolar até qualquer um.
        let mostrar = if self.paineis[ix].busca.aberta { usize::MAX } else { self.paineis[ix].mostrar };
        let escondidos = visiveis.len().saturating_sub(mostrar);
        if escondidos > 0 {
            visiveis.drain(..escondidos);
            let mais = escondidos.min(ITENS_NA_TELA);
            let botao = botao_fantasma(
                SharedString::from(format!("{}:anteriores", chat.id)),
                Some(Lucide::ChevronUp.into()),
                format!("Mostrar anteriores ({mais} de {escondidos})"),
                TamanhoBotao::Barra,
                window,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(painel) = this.paineis.iter_mut().find(|p| p.id == painel_id) {
                    painel.mostrar += ITENS_NA_TELA;
                    // Quem pediu os de antes está lendo lá em cima: o painel não volta ao fim.
                    painel.seguindo = false;
                }
                cx.notify();
            }));
            blocos.push((Bloco::Log, None, h_flex().w_full().justify_center().child(botao).into_any_element()));
        } else if chat.importada.is_some() {
            // A conversa trazida do Claude Code: o que foi dito lá vem antes, lido do arquivo dela.
            blocos.extend(self.blocos_do_historico(motor, chat, nivel, window, cx));
        }
        // O dia de cada turno: o separador entra onde a data muda (pela posição em `visiveis`).
        let horas: Vec<u64> = visiveis.iter().map(|(_, item)| item.em()).collect();
        let datas: HashMap<usize, String> = separadores_de_data(&horas, agora).into_iter().collect();
        let mut i = 0;
        while i < visiveis.len() {
            let (indice, item) = visiveis[i];
            if let Some(dia) = datas.get(&i) {
                blocos.push((Bloco::Marco, None, separador_de_dia(dia)));
            }
            // O raciocínio faz parte da trilha: fica no meio das chamadas, sem parti-la.
            if matches!(item, Item::Ferramenta { .. } | Item::Raciocinio { .. }) {
                let mut fim = i + 1;
                // Uma trilha que passa da meia-noite se parte: o separador do dia novo entra nela.
                while fim < visiveis.len()
                    && matches!(visiveis[fim].1, Item::Ferramenta { .. } | Item::Raciocinio { .. })
                    && !datas.contains_key(&fim)
                {
                    fim += 1;
                }
                let ao_vivo = contexto.rodando && fim == visiveis.len();
                let elemento = self.trilha(&contexto, &visiveis[i..fim], ao_vivo, cx);
                blocos.push((Bloco::Log, Some(indice), elemento));
                i = fim;
                continue;
            }
            i += 1;
            let (bloco, elemento) = self.item(&contexto, indice, item, window, cx);
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
        // Sem ferramenta rodando e sem texto chegando, o turno está pensando: a linha viva fica
        // aqui, no fim da conversa, no lugar onde a próxima coisa vai aparecer (dono, 13/11/2026:
        // "deixa o pensando no chat mesmo, quando fica em baixo parece que tá fazendo nada").
        if parcial.is_none()
            && matches!(motor.execucao(&chat.id), Execucao::Rodando { .. })
            && texto_atividade(&chat.itens, false, motor.dirigidas_no_turno(&chat.id)) == "Pensando…"
        {
            let rotulo = if motor.esperando_agentes(&chat.id) { "Agentes trabalhando em segundo plano…" } else { "Pensando…" };
            blocos.push((Bloco::Log, None, linha_pensando(painel_id, rotulo, cx.theme().font_family.clone())));
        }
        let (base, vistos) = (self.paineis[ix].base_animacao, self.paineis[ix].vistos);
        // Na última conversa, estreita, o texto não passa por baixo dos anéis de uso do canto.
        let folga_do_uso = self.uso_sobre_as_mensagens(ix, cx);
        let mut lista = v_flex()
            .map(coluna)
            .when(minima(nivel), |s| s.px(esp::E12))
            .when(folga_do_uso, |s| s.pr(r(super::uso_plano::FOLGA_DO_USO)))
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
                    .on_scroll(cx.listener(move |this, evento: &ScrollEvent, _, cx| {
                        // A roda do mouse (em linhas) anda em degraus: o degrau vira um deslize.
                        // O touchpad (em pixels) já vem liso e passa direto.
                        if matches!(evento.delta, ScrollDelta::Lines(_)) {
                            this.suavizar_roda(painel_id);
                        }
                        // A roda já mexeu no deslocamento: decide agora, antes que o motor peça
                        // para voltar ao fim.
                        this.acompanhar_rolagem(cx);
                        cx.notify();
                    }))
                    .child(lista)
                    .vertical_scrollbar(&rolagem),
            )
            .when(!seguindo, |s| s.child(pilula_do_fim(painel_id, novidade, minima(nivel), pilula_nova, cx)))
            // A barra do Ctrl+F, por cima das mensagens (montada em `conversa`).
            .children(barra_busca)
    }

    /// Conversa vazia: o chat diz onde trabalha.
    fn dica_inicial(&self, motor: &Motor, chat: &Chat, _window: &Window, _cx: &Context<Self>) -> AnyElement {
        EstadoVazio::new(Lucide::MessagesSquare, "Converse com este chat")
            .texto(onde_o_chat_trabalha(motor, chat))
            .amplo()
            .into_any_element()
    }

    /// Escreve na caixa do painel e põe o foco nela (quem envia é você, com Enter).
    pub(super) fn escrever_na_caixa(&mut self, painel: u64, texto: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.paineis.iter().find(|p| p.id == painel).map(|p| p.composer.clone()) else {
            return;
        };
        composer.update(cx, |c, cx| composer::por_texto(c, texto.to_string(), window, cx));
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    /// Um item da conversa e o tipo de bloco dele.
    fn item(
        &self,
        contexto: &Contexto,
        indice: usize,
        item: &Item,
        window: &Window,
        cx: &Context<Self>,
    ) -> (Bloco, AnyElement) {
        let chave = format!("{}:{indice}", contexto.chat.id);
        match item {
            Item::Usuario {
                texto,
                anexos,
                ultrathink,
                ultracode,
                ..
            } => {
                let reforco = Reforco { ultrathink: *ultrathink, ultracode: *ultracode, depois: false };
                let realce = contexto.busca.realce(indice, Campo::Texto);
                let editavel = contexto.editavel == Some(indice);
                (Bloco::Balao, self.balao_usuario(contexto, &chave, texto, anexos, reforco, item.em(), realce, editavel, window, cx))
            }
            Item::Assistente { texto, .. } => {
                (Bloco::Texto, self.texto_assistente(contexto, indice, &chave, texto, item.em(), cx))
            }
            Item::Entrega { de, texto, .. } => {
                (Bloco::Recebida, self.mensagem_recebida(contexto, indice, &chave, de, texto, item.em(), window, cx))
            }
            Item::Ferramenta { .. } | Item::Raciocinio { .. } => (Bloco::Log, self.trilha(contexto, &[(indice, item)], false, cx)),
            Item::Aviso { texto, erro, .. } => {
                let realce = contexto.busca.realce(indice, Campo::Texto);
                let (bloco, elemento) = aviso_do_chat(&chave, texto, *erro, &realce);
                let rolar = RolarAoAchado::novo(format!("rolar-{chave}").into(), &realce, contexto.rolagem, elemento);
                (bloco, rolar.into_any_element())
            }
            Item::Cartao { id, .. } => aviso_do_chat(&chave, &format!("Cartão #{id}"), false, &Realce::default()),
            Item::Desconhecido(_) => {
                aviso_do_chat(&chave, "(mensagem de uma versão mais nova)", false, &Realce::default())
            }
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
        reforco: Reforco,
        em: u64,
        realce: Realce,
        editavel: bool,
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
                // Os anexos ficam fora do balão, em cima dele: a imagem é uma peça própria e o balão
                // abraça só o texto. Dentro, a imagem esticava o balão até a largura dela e o
                // texto curto ficava solto num cartão grande (dono, 13/11/2026: "melhora como
                // ficam as mensagens minhas").
                let fora = (!anexos.is_empty()).then(|| {
                    h_flex()
                        .flex_wrap()
                        .justify_end()
                        .gap(esp::E6)
                        .children(anexos.iter().enumerate().map(|(i, a)| anexo(chave, i, a, &contexto.chat.id, cx)))
                });
                let com_texto = !texto.trim().is_empty();
                // O balão é azul bem baixo (o destaque misturado à base, opaco): diz de quem é a
                // fala sem a moldura de cartão, e o canto de baixo à direita é o da ponta.
                let balao = (com_texto || mostrar.is_some() || reforco != Reforco::default()).then(|| {
                    v_flex()
                        .px(dentro_da_borda(12.))
                        .py(dentro_da_borda(8.))
                        .gap(esp::E8)
                        .rounded(raio::PECA)
                        .rounded_br(raio::CONTROLE)
                        .bg(Cores::tinta_solida(Cores::destaque(), 0.2))
                        .border_1()
                        .border_color(Cores::destaque().opacity(0.24))
                        .when(com_texto, |s| {
                            s.child(div().corpo().text_color(Cores::forte()).child(
                                markdown_achado(contexto, chave.to_string().into(), texto.to_string(), realce)
                                    .linhas((longo && !tudo).then_some(12)),
                            ))
                        })
                        .when_some(mostrar, |s, b| s.child(h_flex().child(b)))
                        .when(reforco != Reforco::default(), |s| {
                            s.child(
                                h_flex()
                                    .justify_end()
                                    .gap(esp::E6)
                                    .when(reforco.ultrathink, |s| s.child(selo_ultrathink()))
                                    .when(reforco.ultracode, |s| s.child(selo_ultracode())),
                            )
                        })
                });
                v_flex().items_end().gap(esp::E6).children(fora).children(balao).into_any_element()
            }
        };
        let mut acoes: Vec<AnyElement> = Vec::new();
        acoes.extend(hora_na_barra(em));
        // Só na última mensagem: o texto volta para a caixa e a conversa volta a antes dela.
        if editavel {
            acoes.push(super::reenvio::botao_editar(chave, contexto.painel, window, cx).into_any_element());
        }
        acoes.push(copiar(chave, texto).into_any_element());
        // A largura máxima fica no invólucro: o balão encolhe ao texto e a barra se mede por ele.
        // Só no painel largo sobra lugar à esquerda do balão; nos outros a barra vai para o canto
        // de cima (à esquerda ela saía cortada pela borda do cartão: dono, 13/11/2026).
        let barra = div().absolute().map(|s| {
            if larga(nivel) { s.bottom_0().right(relative(1.)).pr(esp::E8) } else { s.top(r(-14.)).right_0() }
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

    /// Resposta do assistente: sem balão, na largura da coluna. Cada texto ganha o Copiar numa
    /// barra que flutua no canto de cima à direita, só no hover (antes só o último do turno tinha,
    /// e copiar um trecho do meio dava trabalho), e a hora ao lado dele.
    fn texto_assistente(
        &self,
        contexto: &Contexto,
        indice: usize,
        chave: &str,
        texto: &str,
        em: u64,
        cx: &Context<Self>,
    ) -> AnyElement {
        let grupo = SharedString::from(format!("msg-{chave}"));
        // Os arquivos citados que estão no disco viram links: abrem no painel da direita.
        let cwd = &contexto.chat.cwd;
        let ligado = super::links_arquivo::ligar_arquivos(texto, &mut |c| super::links_arquivo::resolver(cwd, c));
        let realce = contexto.busca.realce(indice, Campo::Texto);
        // As imagens que a resposta aponta em `![](…)`: o kit desenha a imagem do markdown na
        // altura de uma linha de texto (tamanho de ícone) ou, sozinha num parágrafo, sem teto
        // nenhum de altura. Nenhum dos dois serve à conversa, então elas ganham aqui a mesma
        // miniatura dos anexos, que abre no visor.
        let apontadas = imagens::imagens_do_texto(texto, Some(&contexto.chat.cwd));
        let chat_id = contexto.chat.id.clone();
        v_flex()
            .relative()
            .w_full()
            .gap(esp::E8)
            .group(grupo.clone())
            .child(
                div().corpo().text_color(Cores::texto()).child(
                    markdown_achado(contexto, chave.to_string().into(), ligado, realce).estilo(|vista, cx| {
                        estilo_das_mensagens(vista, cx).on_link_click(|url, _, window, cx| {
                            if url.starts_with("vitral:") {
                                if super::links_arquivo::link_permitido(url, true) {
                                    window.dispatch_action(Box::new(super::links_arquivo::AbrirLinkVitral(url.to_string())), cx);
                                }
                            } else {
                                cx.open_url(url);
                            }
                        })
                    }),
                ),
            )
            .when(!apontadas.is_empty(), |s| {
                s.child(h_flex().flex_wrap().gap(esp::E6).children(apontadas.iter().enumerate().map(|(i, caminho)| {
                    let dica = SharedString::from(format!(
                        "{} · {}\n{}",
                        imagens::nome_do_arquivo(caminho),
                        imagens::anexo(caminho).detalhe(),
                        caminho.display()
                    ));
                    miniatura(
                        format!("resposta-{chave}-{i}"),
                        caminho,
                        &chat_id,
                        move |window, cx| Tooltip::new(dica.clone()).build(window, cx),
                        cx,
                    )
                })))
            })
            .child(div().absolute().top(r(-14.)).right_0().child(barra_de_acoes(
                grupo,
                hora_na_barra(em).into_iter().chain([copiar(chave, texto).into_any_element()]).collect(),
                cx,
            )))
            .into_any_element()
    }

    /// Mensagem que outro chat (ou o Vitral) entregou: quem mandou e uma prévia; a linha do nome
    /// abre e fecha o texto inteiro.
    #[allow(clippy::too_many_arguments)]
    fn mensagem_recebida(
        &self,
        contexto: &Contexto,
        indice: usize,
        chave: &str,
        de: &str,
        texto: &str,
        em: u64,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let nivel = contexto.nivel;
        let realce = contexto.busca.realce(indice, Campo::Texto);
        let aberto = self.expandidos.contains(chave);
        let grupo = SharedString::from(format!("msg-{chave}"));
        let (quem, nome) = remetente(contexto.motor, de);
        // O chat que mandou: "Abrir o chat" nas ações.
        let de_chat = chat_que_mandou(contexto.motor, de);
        let avatar = match quem {
            Some(papel) => papel.circulo(24.).into_any_element(),
            None => img(LOGO.clone()).flex_none().size(r(20.)).into_any_element(),
        };
        let mut acoes: Vec<AnyElement> = Vec::new();
        acoes.extend(hora_na_barra(em));
        if let Some(c) = de_chat {
            let (alvo, titulo) = (c.id.clone(), c.titulo.clone());
            acoes.push(
                botao_so_icone(
                    SharedString::from(format!("abrir-de-{chave}")),
                    Lucide::ArrowRight,
                    format!("Abrir o chat “{titulo}”\nCtrl+clique abre ao lado"),
                    TamanhoBotao::Linha,
                    abrir_chat(alvo, cx),
                    window,
                    cx,
                )
                .into_any_element(),
            );
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
            div().pl(r(32.)).corpo().text_color(Cores::texto()).child(markdown_achado(
                contexto,
                format!("{chave}-t").into(),
                texto.to_string(),
                realce.clone(),
            ))
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
                .child(busca_conversa::realcar(previa(texto), &realce))
        };
        let corpo = v_flex().w_full().group(grupo).child(cabecalho).child(corpo).into_any_element();
        // Aberta, o próprio texto pede para aparecer; fechada, quem rola até a prévia é o bloco.
        let realce = if aberto { Realce::default() } else { realce };
        RolarAoAchado::novo(format!("rolar-{chave}").into(), &realce, contexto.rolagem, corpo).into_any_element()
    }
}

/// A margem direita do código guarda a coluna do Copiar (24, com 8 de cada lado): ele nunca fica
/// por cima de um caractere.
const COLUNA_DO_COPIAR: Rems = r(40.);

/// Markdown com o estilo das mensagens; os blocos de código ganham o Copiar, com a linguagem na
/// dica.
pub(super) fn markdown(id: SharedString, texto: String, cx: &App) -> TextView {
    estilo_das_mensagens(TextView::markdown(id, texto), cx)
}

/// O estilo das mensagens num `TextView`, venha ele de um id (o `markdown`) ou de uma vista
/// guardada (o realce da busca, em `busca_conversa`).
fn estilo_das_mensagens(vista: TextView, cx: &App) -> TextView {
    vista
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

/// Ctrl+F: o markdown de uma mensagem com o termo da busca realçado atrás das letras, que rola
/// a conversa até a ocorrência atual. Sem busca, desenha igual ao `markdown`.
fn markdown_achado(contexto: &Contexto, chave: SharedString, texto: String, realce: Realce) -> TextoAchado {
    TextoAchado::novo(chave, texto, realce).rolagem(contexto.rolagem).estilo(estilo_das_mensagens)
}

/// O texto das mensagens: parágrafos a 12, títulos de 18, 16 e 15, código sem o azul do kit,
/// com o conteúdo a 12 da borda e a coluna do Copiar à direita.
fn estilo_texto(cx: &App) -> TextViewStyle {
    estilo_com_titulos(cx, [18., 16., 15.])
}

/// O estilo das mensagens com os títulos de nível 1, 2 e dos outros em px a 100%.
fn estilo_com_titulos(cx: &App, titulos: [f32; 3]) -> TextViewStyle {
    // O rem do tema já vem com o zoom: os títulos crescem junto.
    let rem = cx.theme().font_size;
    TextViewStyle::default()
        .paragraph_gap(esp::E12)
        .heading_font_size(move |nivel, _| {
            rem * match nivel {
                1 => titulos[0] / 15.,
                2 => titulos[1] / 15.,
                _ => titulos[2] / 15.,
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
fn barra_de_acoes(grupo: SharedString, acoes: Vec<AnyElement>, _: &App) -> Div {
    h_flex()
        .h(r(28.))
        .p(esp::E2)
        .gap(esp::E2)
        // Uma tecla em relevo, sem o anel azul do flutuante: opaca, com fio claro e a luz em cima.
        .bg(Cores::cartao_elevado())
        .border_1()
        .border_color(Cores::borda_forte())
        .shadow([crate::tema::aresta_de_tecla(), crate::tema::sombra_elevado()].concat())
        .rounded(raio::de(28.))
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

/// A hora da mensagem na barra de ações ("14:32"), na frente dos botões: aparece no hover, junto
/// com o Copiar, e nada ocupa a tela em repouso. Item sem hora (conversa de antes dela) não tem.
fn hora_na_barra(em: u64) -> Option<AnyElement> {
    (em > 0).then(|| {
        div()
            .flex_none()
            .px(esp::E6)
            .metadado()
            .text_color(Cores::tenue())
            .child(crate::modelo::hora_local(em))
            .into_any_element()
    })
}

/// Onde entram os separadores de data, pela posição de cada item visível: o rótulo do dia quando
/// ele abre um dia diferente do último item que tinha hora. Item sem hora (conversa de antes dela)
/// não abre dia nem corta o agrupamento.
fn separadores_de_data(horas: &[u64], agora: u64) -> Vec<(usize, String)> {
    let mut saida = Vec::new();
    let mut dia_mostrado = None;
    for (i, &em) in horas.iter().enumerate() {
        if em == 0 {
            continue;
        }
        let dia = crate::modelo::dia_local(em);
        if dia_mostrado != Some(dia) {
            saida.push((i, crate::modelo::nome_do_dia(em, agora)));
            dia_mostrado = Some(dia);
        }
    }
    saida
}

/// O dia entre dois turnos, no molde do marco: o nome do dia no meio de uma linha fina.
fn separador_de_dia(rotulo: &str) -> AnyElement {
    let traco = || div().flex_1().min_w(r(16.)).h(px(1.)).bg(Cores::linha());
    h_flex()
        .w_full()
        .min_h(r(24.))
        .gap(r(8.))
        .child(traco())
        .child(
            div()
                .flex_none()
                .max_w(relative(0.8))
                .truncate()
                .metadado()
                .font_semibold()
                .text_color(Cores::tenue())
                .child(rotulo.to_string()),
        )
        .child(traco())
        .into_any_element()
}

fn copiar(chave: &str, texto: &str) -> BotaoCopiar {
    BotaoCopiar::new(
        Clipboard::new(SharedString::from(format!("copiar-{chave}")))
            .value(SharedString::from(texto.to_string()))
            .tooltip("Copiar"),
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
        .rounded(raio::de(20.))
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
        .rounded(raio::de(20.))
        .bg(Cores::grupo())
        .micro()
        .text_color(Cores::violeta())
        .child(icone_de(Lucide::Workflow, 12.))
        .child("Ultracode")
}

/// Um passo de subagente como aparece: o resumo da ferramenta ("Read · admin.py") vira "Leu
/// admin.py"; o que ele disse fica como está.
pub(super) fn detalhe_do_passo(passo: &str) -> String {
    match passo.split_once(" · ") {
        Some((nome, resto)) if !nome.contains(' ') => format!("{} {resto}", verbo_da_ferramenta(nome, false)),
        _ => passo.to_string(),
    }
}

/// "9,8 s", "1 min 20 s": quanto um bloco de raciocínio durou.
pub(super) fn duracao_curta(milis: u64) -> String {
    match milis {
        0..10_000 => format!("{:.1} s", milis as f64 / 1000.).replace('.', ","),
        10_000..60_000 => format!("{} s", milis / 1000),
        _ => format!("{} min {} s", milis / 60_000, (milis % 60_000) / 1000),
    }
}

/// Quantas linhas entram e quantas saem no pedido de uma edição (o detalhe com "+ " e "- " em
/// cada linha). O "… mais N linhas" de um bloco cortado conta para o sinal do bloco; um detalhe
/// cortado no tamanho ("… (cortado)") não dá conta certa, e fica sem número.
fn contagem_do_diff(pedido: &str) -> Option<(usize, usize)> {
    if pedido.contains("… (cortado)") {
        return None;
    }
    let (mut entram, mut saem, mut ultimo) = (0usize, 0usize, b' ');
    for linha in pedido.lines() {
        match linha.as_bytes().first() {
            Some(b'+') => {
                entram += 1;
                ultimo = b'+';
            }
            Some(b'-') => {
                saem += 1;
                ultimo = b'-';
            }
            _ => {
                let a_mais = linha.strip_prefix("… mais ").and_then(|r| r.split(' ').next()).and_then(|n| n.parse::<usize>().ok());
                match (a_mais, ultimo) {
                    (Some(n), b'+') => entram += n,
                    (Some(n), b'-') => saem += n,
                    _ => {}
                }
            }
        }
    }
    (entram + saem > 0).then_some((entram, saem))
}

/// Quantos itens da conversa um painel desenha de cada vez: os últimos, com "Mostrar anteriores"
/// para os de antes. A conversa não é virtualizada: cada redesenho da janela monta e mede todos
/// os blocos de todos os painéis, e isso custa a cada passo da roda, a cada movimento do
/// ponteiro e a cada letra. Medido em 13/11/2026 no binário otimizado, com duas conversas de 400
/// itens lado a lado: 6,2 s de processador para 80 passos de rolagem (78 ms por passo).
pub(super) const ITENS_NA_TELA: usize = 60;

/// A imagem de uma chamada, embaixo da linha dela (altura e largura máximas), e a de um grupo
/// recolhido, menor, em fileira.
const IMAGEM_DA_CHAMADA: (f32, f32) = (220., 520.);
const IMAGEM_DO_GRUPO: (f32, f32) = (76., 132.);
/// Quantas imagens o grupo recolhido mostra na grade; as outras viram "+N" (abrir o grupo mostra
/// todas, cada uma na linha da chamada dela).
const IMAGENS_NA_GRADE: usize = 8;

/// Miniatura de uma imagem: na proporção dela, com o véu e a lupa no hover, e o clique abre o
/// visor. Serve ao anexo do balão e às imagens que o markdown da resposta aponta.
fn miniatura<D: Fn(&mut Window, &mut App) -> AnyView + 'static>(
    chave: String,
    caminho: &Path,
    chat: &str,
    dica: D,
    cx: &Context<Janela>,
) -> impl IntoElement + use<D> {
    miniatura_de(chave, caminho, chat, (imagens::ALTURA_MINIATURA, imagens::LARGURA_MAXIMA_MINIATURA), dica, cx)
}

/// A imagem que uma chamada leu ou gravou (um Read ou Write de um .png, .jpg…), se ainda está no
/// disco: é o progresso do trabalho à vista (uma captura de tela que o chat tirou e foi olhar).
fn imagem_da_chamada(item: &Item, cwd: &Path) -> Option<PathBuf> {
    let Item::Ferramenta { arquivo: Some(arquivo), erro: false, .. } = item else {
        return None;
    };
    let inteiro = PathBuf::from(super::links_arquivo::resolver(cwd, arquivo)?);
    crate::claude::tipo_imagem(&inteiro).is_some().then_some(inteiro)
}

/// A imagem de uma chamada, embaixo da linha dela: grande o bastante para se ver o que é, e o
/// clique abre o visor (com zoom).
fn imagem_no_log(chave: String, caminho: &Path, chat: &str, medida: (f32, f32), cx: &Context<Janela>) -> impl IntoElement + use<> {
    miniatura_de(chave, caminho, chat, medida, |window, cx| Tooltip::new("Abrir a imagem").build(window, cx), cx)
}

/// A miniatura com a altura e a largura máxima dadas.
fn miniatura_de<D: Fn(&mut Window, &mut App) -> AnyView + 'static>(
    chave: String,
    caminho: &Path,
    chat: &str,
    (altura_max, largura_max): (f32, f32),
    dica: D,
    cx: &Context<Janela>,
) -> impl IntoElement + use<D> {
    let lido = imagens::anexo(caminho);
    let (largura, altura) = imagens::medida_da_miniatura(lido.medidas, altura_max, largura_max);
    let lado_minimo = imagens::LADO_MINIMO_MINIATURA;
    let grupo = SharedString::from(format!("mini-{chave}"));
    let (chat, alvo) = (chat.to_string(), caminho.to_path_buf());
    div()
        .id(SharedString::from(chave))
        .group(grupo.clone())
        .relative()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .w(r(largura.max(lado_minimo)))
        .h(r(altura.max(lado_minimo)))
        .rounded(raio::CONTROLE)
        .bg(Cores::preto())
        .border_1()
        .border_color(Cores::borda())
        .overflow_hidden()
        .cursor_pointer()
        .child(img(caminho.to_path_buf()).flex_none().w(r(largura)).h(r(altura)))
        // No hover, o véu com a lupa diz que a imagem abre.
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(Cores::veu())
                .invisible()
                .group_hover(grupo, |s| s.visible())
                .child(icone_de(Lucide::Maximize, 16.).text_color(Cores::forte())),
        )
        .on_click(cx.listener(move |this, _, window, cx| this.abrir_visor(&chat, &alvo, window, cx)))
        .tooltip(dica)
}

/// Anexo do balão: miniatura na proporção da imagem, que o clique abre no visor; chip com o nome
/// e o tamanho para o resto. O que saiu do disco vira um aviso, e não um vazio mudo.
fn anexo(chave: &str, i: usize, caminho: &Path, chat: &str, cx: &Context<Janela>) -> AnyElement {
    let id = format!("anexo-{chave}-{i}");
    let lido = imagens::anexo(caminho);
    let nome = imagens::nome_do_arquivo(caminho);
    let detalhe = lido.detalhe();
    let imagem = crate::claude::tipo_imagem(caminho).is_some();
    let dica = SharedString::from(match (lido.existe, detalhe.is_empty()) {
        (false, _) => format!("Não está mais em {}", caminho.display()),
        (true, true) => format!("{nome}\n{}", caminho.display()),
        (true, false) => format!("{nome} · {detalhe}\n{}", caminho.display()),
    });
    let dica = move |window: &mut Window, cx: &mut App| Tooltip::new(dica.clone()).build(window, cx);
    if imagem && lido.existe {
        return miniatura(id, caminho, chat, dica, cx).into_any_element();
    }
    let (icone, cor) = match (imagem, lido.existe) {
        (true, false) => (Lucide::ImageOff, Cores::apagado()),
        (false, false) => (Lucide::FileX, Cores::apagado()),
        (true, true) => (Lucide::Image, Cores::tenue()),
        (false, true) => (Lucide::File, Cores::tenue()),
    };
    h_flex()
        .id(SharedString::from(id))
        .h(r(24.))
        .max_w_full()
        .px(r(8.))
        .gap(r(6.))
        .rounded(raio::de(24.))
        .bg(Cores::recuo())
        .metadado()
        .child(icone_de(icone, 14.).text_color(cor))
        .child(div().min_w_0().truncate().text_color(if lido.existe { Cores::texto() } else { Cores::tenue() }).child(nome))
        .child(
            div()
                .flex_none()
                .text_color(Cores::tenue())
                .child(lido.tamanho.map(|t| t.to_string()).unwrap_or_else(|| "sumiu".into())),
        )
        .tooltip(dica)
        .into_any_element()
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

/// Quem mandou uma mensagem entregue: o círculo e o nome; `None` é o próprio Vitral. As conversas
/// antigas guardam mensagens do Mestre, do coordenador e dos ajudantes (o "#7"), cada uma com o
/// círculo de quem era.
fn remetente(motor: &Motor, de: &str) -> (Option<Quem>, String) {
    if de == "Vitral" {
        return (None, "Vitral".into());
    }
    if de == "coordenador" {
        return (Some(Quem::Coordenador), "Conversa antiga".into());
    }
    if de == "Mestre" {
        return (Some(Quem::Mestre), "Mestre".into());
    }
    if chat_que_mandou(motor, de).is_some() {
        return (Some(Quem::Avulso), sem_marcacao(de));
    }
    let (nome, _) = nome_e_id(de);
    let papel = if nome.starts_with('#') { Quem::Trabalhador } else { Quem::Avulso };
    (Some(papel), sem_marcacao(nome))
}

/// O chat de `chat a1b2c3 "Título"` (como os chats assinam o que mandam uns aos outros).
fn chat_que_mandou<'a>(motor: &'a Motor, de: &str) -> Option<&'a Chat> {
    let id = de.strip_prefix("chat ")?.split_whitespace().next()?;
    motor.chat(id)
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
    /// O painel em que esta conversa está desenhada (o "Editar" devolve o texto à caixa dele).
    painel: u64,
    /// A última mensagem do usuário, quando ela ainda dá para corrigir (ver `reenvio`).
    editavel: Option<usize>,
    /// Última mensagem ou aviso: chamadas sem resultado antes dele foram interrompidas.
    marco: Option<usize>,
    /// A busca na conversa: de onde sai o realce de cada texto.
    busca: &'a busca_conversa::Indice,
    /// A rolagem do painel, para a conversa andar até o achado.
    rolagem: &'a ScrollHandle,
}

impl<'a> Contexto<'a> {
    fn novo(
        motor: &'a Motor,
        chat: &'a Chat,
        nivel: LarguraPainel,
        painel: u64,
        busca: &'a busca_conversa::Indice,
        rolagem: &'a ScrollHandle,
    ) -> Self {
        Contexto {
            motor,
            chat,
            rodando: motor.execucao(&chat.id).rodando(),
            nivel,
            painel,
            editavel: super::reenvio::indice_editavel(motor, chat),
            marco: marco_da_conversa(&chat.itens, motor.dirigidas_no_turno(&chat.id)),
            busca,
            rolagem,
        }
    }

    /// Chamada ainda sem resultado no turno que está rodando.
    fn pendente(&self, indice: usize, item: &Item) -> bool {
        matches!(item, Item::Ferramenta { resultado: None, .. })
            && self.rodando
            && self.marco.is_none_or(|m| indice > m)
    }
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
        .bg(Cores::traco())
}

impl Janela {

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
            let pensou = chamadas.iter().filter(|(_, item)| matches!(item, Item::Raciocinio { .. })).count();
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
                        .child(contagem(nomes.len(), "ação", "ações")),
                )
                .map(|s| {
                    if larga(contexto.nivel) {
                        s.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(Cores::tenue())
                                .child(format!("· {}{}", resumo_da_trilha(&nomes), if pensou > 0 { format!(" · pensou {pensou}") } else { String::new() })),
                        )
                    } else {
                        s.child(div().flex_1())
                    }
                })
                .when(falhas > 0, |s| {
                    s.child(Selo::cor(contagem(falhas, "falhou", "falharam"), Cores::vermelho()))
                })
                .when(rodando_escondida, |s| s.child(AnelGirando::new(14.).cor(Cores::destaque())))
                .on_click(alternar(chave.clone(), cx))
        });
        // Recolhido, o grupo ainda mostra as imagens que as chamadas escondidas leram ou gravaram.
        let escondidas = &chamadas[..chamadas.len() - mostradas.len()];
        let mut imagens_do_grupo: Vec<(usize, PathBuf)> = Vec::new();
        for (indice, item) in escondidas {
            if let Some(imagem) = imagem_da_chamada(item, &contexto.chat.cwd)
                && !imagens_do_grupo.iter().any(|(_, outra)| *outra == imagem)
            {
                imagens_do_grupo.push((*indice, imagem));
            }
        }
        // Em grade de miniaturas pequenas (dono, 13/11/2026: uma embaixo da outra ficava grande
        // demais), as primeiras `IMAGENS_NA_GRADE` e um "+N" para o resto.
        let fileira = (!imagens_do_grupo.is_empty()).then(|| {
            let a_mais = imagens_do_grupo.len().saturating_sub(IMAGENS_NA_GRADE);
            h_flex()
                .flex_wrap()
                .items_center()
                .gap(esp::E6)
                .pl(RECUO_DO_LOG)
                .py(esp::E4)
                .children(imagens_do_grupo.iter().take(IMAGENS_NA_GRADE).map(|(indice, imagem)| {
                    imagem_no_log(format!("img-{}:{indice}:g", contexto.chat.id), imagem, &contexto.chat.id, IMAGEM_DO_GRUPO, cx)
                }))
                // O "+N" é um ladrilho que abre o visor na primeira que não coube (dono, 13/11/2026:
                // "como acesso esse mais 2?"); dali as setas passam pelas outras.
                .when_some(imagens_do_grupo.get(IMAGENS_NA_GRADE).cloned(), |s, (indice, primeira)| {
                    let chat = contexto.chat.id.clone();
                    let dica = SharedString::from(if a_mais == 1 { "Ver a outra imagem".to_string() } else { format!("Ver as outras {a_mais} imagens") });
                    s.child(
                        div()
                            .id(SharedString::from(format!("img-mais-{chat}:{indice}")))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .h(r(IMAGEM_DO_GRUPO.0))
                            .min_w(r(IMAGEM_DO_GRUPO.0))
                            .px(esp::E12)
                            .rounded(raio::CONTROLE)
                            .border_1()
                            .border_color(Cores::borda())
                            .cursor_pointer()
                            .hover(|s| s.bg(Cores::hover()))
                            .corpo_ui()
                            .text_color(Cores::texto())
                            .child(format!("+{a_mais}"))
                            .on_click(cx.listener(move |this, _, window, cx| this.abrir_visor(&chat, &primeira, window, cx)))
                            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx)),
                    )
                })
        });
        let ultima = mostradas.len().saturating_sub(1);
        v_flex()
            .w_full()
            .children(cabecalho)
            .children(fileira)
            .children(
                mostradas
                    .iter()
                    .enumerate()
                    .map(|(k, (indice, item))| self.chamada(contexto, *indice, item, k < ultima, cx)),
            )
            .into_any_element()
    }

    /// Um bloco de raciocínio na trilha: "Pensou" e quanto durou; o clique mostra o texto, quando
    /// o Claude Code o entrega (referência do dono: 21st.dev, "AI Thinking and Tool calls").
    fn raciocinio(&self, contexto: &Contexto, indice: usize, texto: &str, milis: u64, com_trilho: bool, cx: &Context<Self>) -> AnyElement {
        let chave = format!("{}:{indice}", contexto.chat.id);
        let tem_texto = !texto.trim().is_empty();
        let aberto = tem_texto && self.expandidos.contains(&chave);
        let linha = h_flex()
            .id(SharedString::from(format!("raciocinio-{chave}")))
            .h(ALTURA_DO_LOG)
            .px(esp::E12)
            .gap(esp::E8)
            .rounded(raio::CONTROLE)
            .metadado()
            .child(div().flex_none().size(r(14.)).flex().items_center().justify_center().child(icone_de(Lucide::Brain, 14.).text_color(Cores::tenue())))
            .child(div().flex_none().font_weight(FontWeight::MEDIUM).text_color(Cores::texto()).child("Pensou"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(FONTE_MONO)
                    .text_color(Cores::tenue())
                    .child(if milis > 0 { format!("por {}", duracao_curta(milis)) } else { String::new() }),
            )
            .when(tem_texto, |s| {
                s.cursor_pointer()
                    .hover(|s| s.bg(Cores::hover()))
                    .child(div().flex_none().text_color(Cores::tenue()).child(icone_de(if aberto { Lucide::ChevronDown } else { Lucide::ChevronRight }, 12.)))
                    .on_click(alternar(chave.clone(), cx))
            });
        v_flex()
            .relative()
            .w_full()
            .child(linha)
            .when(aberto, |s| {
                s.child(
                    div().pl(RECUO_DO_LOG).py(esp::E4).child(
                        div()
                            .id(SharedString::from(format!("raciocinio-texto-{chave}")))
                            .w_full()
                            .max_h(r(280.))
                            .overflow_y_scroll()
                            .py(esp::E8)
                            .px(esp::E12)
                            .rounded(raio::CONTROLE)
                            .bg(Cores::hover())
                            .metadado()
                            .text_color(Cores::tenue())
                            .child(texto.trim().to_string()),
                    ),
                )
            })
            .when(com_trilho, |s| s.child(trilho_do_log()))
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
        if let Item::Raciocinio { texto, milis, .. } = item {
            return self.raciocinio(contexto, indice, texto, *milis, com_trilho, cx);
        }
        let Item::Ferramenta {
            nome,
            resumo,
            resultado,
            erro,
            detalhe: pedido,
            arquivo,
            sub,
            em,
            ..
        } = item
        else {
            return div().into_any_element();
        };
        let chave = format!("{}:{indice}", contexto.chat.id);
        let aberto = self.expandidos.contains(&chave);
        let pendente = contexto.pendente(indice, item);
        // O subagente (Task/Agent): quantas ferramentas usou e há quanto tempo roda (ou quanto
        // levou), na linha; o que ele faz agora, embaixo dela enquanto roda; e os passos dele,
        // ao abrir. É o cartão de subagente da referência do dono (21st.dev, 13/11/2026).
        let do_sub = sub.as_ref().map(|s| {
            let segundos = if s.fim > 0 { s.fim.saturating_sub(*em) } else { crate::modelo::agora().saturating_sub(*em) };
            let ferramentas = contagem(s.ferramentas as usize, "ferramenta", "ferramentas");
            (format!("{ferramentas} · {}", relogio(segundos)), s)
        });
        let atividade_viva = do_sub.as_ref().filter(|(_, s)| pendente && !s.atividade.is_empty()).map(|(_, s)| {
            h_flex()
                .pl(RECUO_DO_LOG)
                .h(r(22.))
                .gap(esp::E6)
                .metadado()
                .child(
                    ShimmerText::new(sem_marcacao(&detalhe_do_passo(&s.atividade)))
                        .id(SharedString::from(format!("sub-{chave}")))
                        .duration(Duration::from_secs(3))
                        .highlight_color(Cores::forte())
                        .min_w_0()
                        .truncate(),
                )
        });
        let passos = do_sub.as_ref().filter(|(_, s)| !s.passos.is_empty()).map(|(_, s)| {
            div().pl(RECUO_DO_LOG).pt(esp::E4).child(
                v_flex()
                    .w_full()
                    .py(esp::E8)
                    .px(esp::E12)
                    .gap(esp::E2)
                    .rounded(raio::CONTROLE)
                    .bg(Cores::hover())
                    .mono_pequeno()
                    .text_color(Cores::tenue())
                    .children(s.passos.iter().map(|p| div().min_w_0().truncate().child(detalhe_do_passo(p)))),
            )
        });
        let meta_do_sub = do_sub.map(|(texto, _)| texto);
        // Há quanto tempo a chamada roda, na ponta da linha (depois de 3 s, para as rápidas não
        // piscarem um relógio): num comando longo é o que diz que ela segue viva.
        let rodando_ha = (pendente && item.em() > 0).then(|| crate::modelo::agora().saturating_sub(item.em())).filter(|s| *s >= 3);
        let meta_do_sub = meta_do_sub.or_else(|| rodando_ha.map(relogio));
        // O arquivo que a chamada leu ou mudou, se ainda está no disco: abre ao lado pelo ícone
        // que aparece no hover da linha.
        let abrir = arquivo
            .as_deref()
            .and_then(|a| super::links_arquivo::resolver(&contexto.chat.cwd, a))
            .map(|inteiro| super::links_arquivo::link_de(&inteiro, None));
        let grupo = SharedString::from(format!("chamada-{chave}"));
        // Sem resultado e fora do turno que está rodando: foi interrompida.
        let interrompida = resultado.is_none() && !pendente;
        let icone = if pendente {
            AnelGirando::new(14.).cor(Cores::destaque()).into_any_element()
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
        let linhas_do_diff = pedido
            .as_deref()
            .filter(|_| matches!(nome.as_str(), "Edit" | "MultiEdit" | "Write" | "NotebookEdit"))
            .and_then(contagem_do_diff);
        let verbo = verbo_da_ferramenta(nome, pendente);
        let detalhe = sem_marcacao(&detalhe_da_chamada(nome, resumo));
        // Ctrl+F: o resumo da chamada fica na linha e o resultado dentro do bloco que ela abre.
        let (no_resumo, no_resultado) =
            (contexto.busca.realce(indice, Campo::Resumo), contexto.busca.realce(indice, Campo::Resultado));
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
                    .child(busca_conversa::realcar(detalhe, &no_resumo)),
            )
            .when_some(meta_do_sub.filter(|_| !minima), |s, texto| {
                s.child(div().flex_none().font_family(FONTE_MONO).text_color(Cores::apagado()).child(texto))
            })
            // Numa edição, quanto entrou e quanto saiu, na própria linha ("+5 −2"), como na trilha
            // de referência que o dono mandou (21st.dev, "AI Thinking and Tool calls", 13/11/2026).
            .when_some(linhas_do_diff.filter(|_| !minima && !*erro), |s, (entram, saem)| {
                s.child(
                    h_flex()
                        .flex_none()
                        .gap(esp::E6)
                        .font_family(FONTE_MONO)
                        .when(entram > 0, |s| s.child(div().text_color(Cores::verde()).child(format!("+{entram}"))))
                        .when(saem > 0, |s| s.child(div().text_color(Cores::vermelho()).child(format!("−{saem}")))),
                )
            })
            .when_some(meta.filter(|_| !estreita), |s, (meta, cor)| {
                s.child(div().flex_none().text_color(cor).child(meta))
            })
            .group(grupo.clone())
            .when_some(abrir, |s, alvo| {
                s.child(
                    div()
                        .id(SharedString::from(format!("abrir-arquivo-{chave}")))
                        .flex_none()
                        .size(r(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(raio::CONTROLE)
                        .text_color(Cores::tenue())
                        .opacity(0.)
                        .group_hover(grupo.clone(), |s| s.opacity(1.))
                        .hover(|s| s.bg(Cores::hover()).text_color(Cores::texto()))
                        .child(icone_de(Lucide::PanelRight, 13.))
                        .tooltip(|window, cx| Tooltip::new("Abrir o arquivo ao lado").build(window, cx))
                        .on_click(move |_, window, cx| {
                            // Não abre nem fecha o detalhe da linha.
                            cx.stop_propagation();
                            window.dispatch_action(Box::new(super::links_arquivo::AbrirLinkVitral(alvo.clone())), cx);
                        }),
                )
            })
            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            .on_click(alternar(chave.clone(), cx));
        // Aberta, a linha mostra primeiro o que a chamada pediu (o diff, o que gravou). O resultado
        // de uma edição que deu certo é só "o arquivo foi atualizado": fica de fora.
        let so_o_pedido = pedido.is_some() && !*erro && resultado.is_some() && matches!(nome.as_str(), "Edit" | "MultiEdit" | "Write" | "NotebookEdit");
        // A imagem que a chamada leu ou gravou fica à vista embaixo da linha, sem precisar abrir
        // (dono, 13/11/2026: "dar pra ver as imagens que ele tá acessando… isso é o progresso
        // dele"). O resultado de ler uma imagem não tem texto: a caixa vazia não aparece.
        let imagem = imagem_da_chamada(item, &contexto.chat.cwd);
        let sem_texto = imagem.is_some() && resultado.as_deref().is_some_and(|r| r.trim().is_empty());
        let comando = pedido.as_deref().and_then(|p| p.strip_prefix("$ ")).filter(|_| nome == "Bash");
        let bloco = v_flex()
            .relative()
            .w_full()
            .child(linha)
            .children(atividade_viva)
            .when_some(imagem.as_deref(), |s, imagem| {
                s.child(div().pl(RECUO_DO_LOG).py(esp::E4).child(imagem_no_log(
                    format!("img-{chave}"),
                    imagem,
                    &contexto.chat.id,
                    IMAGEM_DA_CHAMADA,
                    cx,
                )))
            })
            .when(aberto, |s| {
                s.children(passos).when_some(pedido.as_deref().filter(|_| comando.is_none()), |s, pedido| {
                    s.child(div().pl(RECUO_DO_LOG).pt(esp::E4).child(bloco_do_pedido(pedido)))
                })
                .when_some(comando, |s, comando| {
                    s.child(div().pl(RECUO_DO_LOG).pt(esp::E4).child(bloco_do_pedido(&format!("$ {comando}"))))
                })
                // Rodando ainda não há saída: em vez da caixa do resultado vazia, com o Copiar e
                // um "(rodando…)" dentro, uma linha só (dono, 13/11/2026: "melhora essa parte que
                // fica o rodando").
                .when(pendente && resultado.is_none(), |s| {
                    let texto = match rodando_ha {
                        Some(segundos) => format!("A saída aparece quando terminar · rodando há {}", relogio(segundos)),
                        None => "A saída aparece quando terminar".to_string(),
                    };
                    s.child(div().pl(RECUO_DO_LOG).pt(esp::E6).pb(esp::E4).child(
                        div().px(esp::E12).metadado().font_family(FONTE_MONO).text_color(Cores::apagado()).child(texto),
                    ))
                })
                .when(!so_o_pedido && !sem_texto && !(pendente && resultado.is_none()), |s| {
                    s.child(div().pl(RECUO_DO_LOG).py(esp::E4).child(
                        bloco_resultado(&chave, resultado.as_deref(), *erro, "(interrompida, sem resultado)").realce(&no_resultado),
                    ))
                })
                .when(so_o_pedido, |s| s.pb(esp::E4))
            })
            .when(com_trilho, |s| s.child(trilho_do_log()))
            .into_any_element();
        // A conversa anda até a chamada quando o achado atual está nela (no resumo ou na saída).
        let realce = if no_resumo.atual.is_some() { no_resumo } else { no_resultado };
        RolarAoAchado::novo(format!("rolar-{chave}").into(), &realce, contexto.rolagem, bloco).into_any_element()
    }

}

/// Abre ou fecha um item da conversa (a chave fica em `expandidos`, começando pelo id do chat).
/// Ao abrir, os painéis dessa conversa no fim ficam onde estão em vez de seguir o fim do bloco.
pub(super) fn alternar(
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

/// Quantas linhas do pedido de uma chamada aparecem; o resto vira "… mais N linhas".
const LINHAS_DO_PEDIDO: usize = 80;

/// O que a chamada pediu (`Item::Ferramenta::detalhe`): o que saiu em vermelho, o que entrou em
/// verde, o comando e o resto no tom do texto, em mono, numa caixa que rola de lado.
fn bloco_do_pedido(pedido: &str) -> impl IntoElement + use<> {
    let total = pedido.lines().count();
    let linhas = pedido.lines().take(LINHAS_DO_PEDIDO).map(|linha| {
        let (cor, fundo) = match linha.as_bytes().first() {
            Some(b'+') => (Cores::verde(), Some(fundo_entra())),
            Some(b'-') => (Cores::vermelho(), Some(fundo_sai())),
            Some(b'$') => (Cores::texto(), None),
            _ => (Cores::tenue(), None),
        };
        div()
            .px(esp::E12)
            .whitespace_nowrap()
            .text_color(cor)
            .when_some(fundo, |s, fundo| s.bg(fundo))
            // A linha vazia mantém a altura.
            .child(if linha.is_empty() { SharedString::from(" ") } else { SharedString::from(linha.to_string()) })
    });
    div()
        .id("pedido-da-chamada")
        .w_full()
        .py(esp::E8)
        .rounded(raio::CONTROLE)
        .bg(Cores::hover())
        .mono_pequeno()
        .overflow_x_scroll()
        .children(linhas)
        .when(total > LINHAS_DO_PEDIDO, |s| {
            s.child(div().px(esp::E12).text_color(Cores::apagado()).child(format!("… mais {} linhas", total - LINHAS_DO_PEDIDO)))
        })
}

/// O resultado de uma chamada aberta, na caixa de código do chat (as últimas 40 linhas).
fn bloco_resultado(chave: &str, resultado: Option<&str>, erro: bool, vazio: &str) -> CaixaDeCodigo {
    let texto = resultado.unwrap_or(vazio);
    CaixaDeCodigo::new(SharedString::from(format!("saida-{chave}")), texto.to_string().into())
        .raio_chat()
        .when(erro, |c| c.erro(true).tempo("Saiu com erro"))
}

pub(super) fn nome_curto(nome: &str) -> &str {
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
        "estado" => Lucide::Gauge,
        "ver_chat" => Lucide::Eye,
        "historico" => Lucide::Clock,
        "propor_divisao" => Lucide::GitFork,
        "sugerir_projeto" => Lucide::FolderPlus,
        "lembrar" | "esquecer" => Lucide::Brain,
        "entregar_tarefa" => Lucide::CircleCheck,
        "revisar_minhas_mudancas" => Lucide::ListChecks,
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
        "listar_tarefas" => ("Listou os ajudantes", "Listando os ajudantes"),
        "listar_chats" => ("Listou as conversas", "Listando as conversas"),
        "anotar_decisao" => ("Anotou", "Anotando"),
        "mensagem_para_chat" => ("Mandou mensagem", "Mandando mensagem"),
        "perguntar_coordenador" => ("Perguntou", "Perguntando"),
        "informar_progresso" => ("Informou", "Informando"),
        "estado" => ("Olhou o estado", "Olhando o estado"),
        "ver_chat" => ("Leu o chat", "Lendo o chat"),
        "historico" => ("Leu o histórico", "Lendo o histórico"),
        "propor_divisao" => ("Propôs a divisão", "Propondo a divisão"),
        "sugerir_projeto" => ("Sugeriu o projeto", "Sugerindo o projeto"),
        "lembrar" => ("Lembrou", "Lembrando"),
        "esquecer" => ("Esqueceu", "Esquecendo"),
        "entregar_tarefa" => ("Entregou", "Entregando"),
        "revisar_minhas_mudancas" => ("Pediu o revisor", "Pedindo o revisor"),
        "criar_tarefa" => ("Criou o ajudante", "Criando o ajudante"),
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
pub(super) fn detalhe_da_chamada(nome: &str, resumo: &str) -> String {
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

/// Onde o chat trabalha, para a conversa vazia: direto na pasta dele, ou na pasta só dele (o chat
/// sem pasta).
fn onde_o_chat_trabalha(motor: &Motor, chat: &Chat) -> String {
    if motor.sem_pasta(chat) {
        return "Este chat tem uma pasta só dele: os arquivos que ele criar ficam lá.".into();
    }
    format!("Este chat mexe direto em {}.", super::trilho::pasta_curta(&chat.cwd))
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
    const MARCOS: [&str; 5] =
        ["Sessão nova", "Interrompido", "Saiu do modo Planejar", "Retomando a sessão", crate::importar::MARCO];
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

fn aviso_do_chat(chave: &str, texto: &str, erro: bool, realce: &Realce) -> (Bloco, AnyElement) {
    match tipo_aviso(texto, erro) {
        ClasseAvisoChat::Marco => {
            let (icone, cor, rotulo) = if texto.starts_with("Interrompido") {
                (Lucide::CirclePause, Cores::tenue(), "Turno interrompido".to_string())
            } else if texto.starts_with("Saiu do modo Planejar") {
                (Lucide::Map, Cores::verde(), texto.to_string())
            } else if texto.starts_with(crate::importar::MARCO) {
                (Lucide::Download, Cores::tenue(), texto.to_string())
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
                        .child(div().min_w_0().line_clamp(2).text_ellipsis().child(busca_conversa::realcar(sem_marcacao(&rotulo), realce))),
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
                        .child(busca_conversa::realcar(sem_marcacao(texto), realce)),
                )
                .into_any_element();
            (Bloco::Log, elemento)
        }
        ClasseAvisoChat::Alerta => {
            let (titulo, detalhe) = partes_do_erro(texto);
            // No alerta o realce fica no corpo (o título é só o começo da mesma frase).
            let alerta = Alerta::new(Tom::Erro, sem_marcacao(&detalhe)).acao(copiar(chave, texto));
            let alerta = match titulo {
                Some(t) => alerta.titulo(sem_marcacao(&t)),
                None => alerta,
            };
            let alerta = if realce.vazio() {
                alerta
            } else {
                alerta.texto_rico(busca_conversa::realcar(sem_marcacao(&detalhe), realce))
            };
            (Bloco::Alerta, alerta.into_any_element())
        }
    }
}

/// Onde o usuário está num painel: o deslocamento da última conferência (para saber quando foi
/// ele que rolou) e o tamanho da conversa da última vez que ele estava no fim.
#[derive(Default)]
pub struct Leitura {
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
    /// `dirigidas`: mensagens que o usuário mandou para dentro deste turno.
    Rodando { desde: u64, fila: usize, dirigidas: usize },
    Parada { motivo: String, atencao: bool },
    Falhou,
    /// A pasta onde o chat trabalha sumiu.
    OrigemSumiu { pasta: PathBuf },
    Fila(usize),
}

/// O que um chat parado tem a dizer acima da caixa: a pasta dele sumiu (a do chat sem pasta o
/// Vitral cria de novo).
fn situacao_do_chat(motor: &Motor, chat: &Chat) -> Option<Situacao> {
    (!motor.sem_pasta(chat) && !chat.cwd.exists()).then(|| Situacao::OrigemSumiu { pasta: chat.cwd.clone() })
}

/// A marca de "dirigido" na faixa do turno: a mensagem que o usuário mandou entrou no turno que
/// já rodava, sem pará-lo nem abrir outro.
fn selo_dirigido(painel: u64, n: usize, estreita: bool) -> AnyElement {
    let rotulo = if n == 1 { "dirigido".to_string() } else { format!("dirigido {n}×") };
    let dica: SharedString = if n == 1 {
        "A sua mensagem entrou neste turno: ele não parou nem começou de novo".into()
    } else {
        "As suas mensagens entraram neste turno: ele não parou nem começou de novo".into()
    };
    div()
        .id(("selo-dirigido", painel as usize))
        .flex_none()
        .child(
            h_flex()
                .gap(r(4.))
                .metadado()
                .text_color(Cores::tenue())
                .child(icone_de(Lucide::MessageSquarePlus, 12.))
                .when(!estreita, |s| s.child(rotulo)),
        )
        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        .into_any_element()
}

/// A linha viva do turno que pensa, com o desenho de uma chamada em andamento na trilha: o anel
/// girando e o rótulo com o brilho passando.
fn linha_pensando(painel: u64, rotulo: &'static str, fonte: SharedString) -> AnyElement {
    h_flex()
        .h(ALTURA_DO_LOG)
        .px(esp::E12)
        .gap(esp::E8)
        .child(div().flex_none().size(r(14.)).flex().items_center().justify_center().child(AnelGirando::new(14.).cor(Cores::destaque())))
        .child(
            ShimmerText::new(rotulo)
                .id(("pensando", painel as usize))
                .duration(Duration::from_secs(3))
                .highlight_color(Cores::forte())
                .font_family(fonte)
                .metadado()
                .font_weight(FontWeight::MEDIUM),
        )
        .into_any_element()
}

fn situacao_do_turno(motor: &Motor, chat: &str) -> Option<Situacao> {
    match motor.execucao(chat) {
        // Esperando você: os pedidos de permissão já aparecem logo abaixo.
        Execucao::EsperandoVoce => return None,
        Execucao::Rodando { desde } => {
            return Some(Situacao::Rodando {
                desde,
                fila: motor.na_fila(chat),
                dirigidas: motor.dirigidas_no_turno(chat),
            });
        }
        _ => {}
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
    /// A linha de estado viva da caixa (dentro da moldura, embaixo do campo): o que o turno em
    /// andamento faz agora, à esquerda, e a tecla que o para, à direita. `None` quando o chat não
    /// está num turno em andamento: a caixa mostra as teclas.
    pub(super) fn linha_de_estado(
        &self,
        motor: &Motor,
        chat: &Chat,
        painel_id: u64,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<(Option<AnyElement>, Option<AnyElement>)> {
        let Some(situacao @ Situacao::Rodando { .. }) = situacao_do_turno(motor, &chat.id) else {
            return None;
        };
        let painel = self.paineis.iter().find(|p| p.id == painel_id);
        let medida = painel.map(|p| p.medida).unwrap_or_default();
        // Com a conversa no fim, o que o turno faz já está à vista nela (a chamada em andamento, o
        // "Pensando…"): a linha da caixa não repete, fica com o relógio, a contagem e a fila
        // (dono, 13/11/2026: "por que aparece o rodando em cima e embaixo?"). Rolada para cima, a
        // conversa não mostra mais o fim, e aí a linha diz.
        let no_fim = painel.is_none_or(|p| p.seguindo);
        let (estreita, minima) = (!larga(medida.nivel), minima(medida.nivel));
        let esc = move |rotulo: &'static str, window: &Window| {
            h_flex()
                .flex_none()
                .gap(esp::E6)
                .children(teclas(&PararTurno, Some("Caixa"), window))
                .when(!estreita, |s| s.child(rotulo))
                .into_any_element()
        };
        // O que o chat faz vai na voz que conversa (a Inter 12); relógio, contagem e teclas, na mono
        // da linha.
        let fonte = cx.theme().font_family.clone();
        let (esquerda, direita) = match situacao {
            Situacao::Rodando { desde, fila, dirigidas } => {
                let escrevendo = motor.parcial(&chat.id).is_some();
                let atividade = texto_atividade(&chat.itens, escrevendo, dirigidas);
                // A resposta já veio, mas os agentes do fluxo seguem trabalhando.
                let atividade = if !escrevendo && atividade == "Pensando…" && motor.esperando_agentes(&chat.id) {
                    "Agentes trabalhando em segundo plano…".to_string()
                } else {
                    sem_marcacao(&atividade)
                };
                let (acoes, _) = numeros_do_turno(&chat.itens);
                let agentes = super::mapa_agentes::agentes_vivos(motor, chat);
                (
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(esp::E8)
                        .child(AnelGirando::new(12.).cor(Cores::destaque_texto()))
                        .when(!no_fim, |s| {
                            s.child(
                                // Passada mais lenta: a 12 quadros por segundo, o brilho anda em
                                // passos menores.
                                ShimmerText::new(atividade)
                                    .id(("atividade", painel_id as usize))
                                    .duration(Duration::from_secs(3))
                                    .highlight_color(Cores::forte())
                                    .font_family(fonte.clone())
                                    .metadado()
                                    .min_w_0()
                                    .truncate(),
                            )
                        })
                        .when(!minima || no_fim, |s| {
                            s.child(
                                div()
                                    .flex_none()
                                    .etiqueta()
                                    .text_color(Cores::apagado())
                                    .child(relogio(crate::modelo::agora().saturating_sub(desde))),
                            )
                        })
                        .when(!estreita && acoes > 0, |s| {
                            s.child(div().flex_none().etiqueta().text_color(Cores::apagado()).child(format!("ação {acoes}")))
                        })
                        // Os subagentes que trabalham agora: o clique abre o mapa deles.
                        .when(agentes > 0, |s| {
                            let chat_id = chat.id.clone();
                            let rotulo = if estreita { agentes.to_string() } else { super::componentes::contagem(agentes, "agente", "agentes") };
                            s.child(
                                Button::new(("pilula-agentes", painel_id as usize))
                                    .custom(super::componentes::variante_fantasma(cx))
                                    .h(r(22.))
                                    .px(r(6.))
                                    .rounded(raio::de(22.))
                                    .tooltip("Ver os agentes desta conversa: o que cada um está fazendo")
                                    .child(
                                        h_flex()
                                            .gap(r(4.))
                                            .metadado()
                                            .text_color(Cores::tenue())
                                            .child(icone_de(Lucide::Workflow, 12.))
                                            .child(rotulo),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.abrir_mapa_de_agentes(chat_id.clone(), window, cx)
                                    })),
                            )
                        })
                        // A mensagem que você mandou entrou neste turno, sem reiniciá-lo.
                        .when(dirigidas > 0, |s| s.child(selo_dirigido(painel_id, dirigidas, estreita)))
                        // A fila é um controle: o clique abre a lista do que espera, para tirar o
                        // que você não quer mais.
                        .when(fila > 0, |s| {
                            s.child(div().flex_none().child(self.pilula_da_fila(&chat.id, painel_id, fila, true, estreita, window, cx)))
                        })
                        .into_any_element(),
                    Some(esc("para parar", window)),
                )
            }
            _ => return None,
        };
        Some((Some(esquerda), direita))
    }

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
        // O que roda (ou parou) vale mais; parado, a pasta que sumiu, e só então
        // a fila que espera.
        let situacao = match situacao_do_turno(motor, &chat.id) {
            Some(Situacao::Fila(n)) => situacao_do_chat(motor, chat).unwrap_or(Situacao::Fila(n)),
            Some(s) => s,
            None => situacao_do_chat(motor, chat)?,
        };
        // O turno em andamento mora na linha de estado da caixa, dentro da moldura
        // (`linha_de_estado`): acima da caixa só fica o que pede um botão seu.
        if matches!(situacao, Situacao::Rodando { .. }) {
            return None;
        }
        let medida = self.paineis.iter().find(|p| p.id == painel_id).map(|p| p.medida).unwrap_or_default();
        let estreita = !larga(medida.nivel);
        // Alinha com o texto da caixa (a margem da coluna mais o recuo do texto nela) e, à direita,
        // com o enviar.
        let caixa_estreita = medida.faixa == FaixaCaixa::Estreita;
        let margem = tam(window, if caixa_estreita { 12. } else { 16. });
        let (recuo, fim) = (margem + composer::recuo_do_texto(caixa_estreita, window), margem + composer::borda_ao_conteudo(caixa_estreita, window));
        // A fila é da conversa, não do turno: depois de um reinício (ou de uma parada, ou de uma
        // falha) o chat não está mais rodando e ela continua esperando, à vista.
        let fila_fora = match situacao {
            Situacao::Fila(_) => None,
            _ => Some(motor.na_fila(&chat.id)).filter(|n| *n > 0),
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
        let (esquerda, direita): (AnyElement, Option<AnyElement>) = match situacao {
            // Na linha de estado da caixa (o `return` de cima).
            Situacao::Rodando { .. } => return None,
            Situacao::Parada { motivo, atencao } => (
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(r(8.))
                    .child(icone_de(Lucide::Pause, 14.).text_color(if atencao {
                        Cores::espera()
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
            Situacao::OrigemSumiu { pasta } => (
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(r(8.))
                    .child(icone_de(Lucide::FolderX, 14.).text_color(Cores::vermelho()))
                    .child(texto(format!("A pasta {} não foi encontrada", super::dialogos::com_til(&pasta))))
                    .into_any_element(),
                None,
            ),
            Situacao::Fila(n) => (
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(r(8.))
                    .child(icone_de(Lucide::ListChecks, 14.).text_color(Cores::tenue()))
                    .child(self.pilula_da_fila(&chat.id, painel_id, n, false, estreita, window, cx))
                    .into_any_element(),
                Some(
                    botao_fantasma("faixa-entregar", None, "Entregar agora", TamanhoBotao::Barra, window, cx)
                        .on_click(entregar)
                        .into_any_element(),
                ),
            ),
        };
        let esquerda = match fila_fora {
            Some(n) => h_flex()
                .flex_1()
                .min_w_0()
                .gap(r(8.))
                .child(esquerda)
                .child(div().flex_none().child(self.pilula_da_fila(&chat.id, painel_id, n, false, estreita, window, cx)))
                .into_any_element(),
            None => esquerda,
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

    /// A roda do mouse acabou de dar um degrau na conversa do painel: o deslocamento volta para
    /// onde estava e o degrau vira o alvo de um deslize curto, que o `render` da Janela anda
    /// (dono, 13/11/2026: "faz uma rolagem ser mais fluida"). Degraus seguidos somam no alvo.
    fn suavizar_roda(&mut self, painel_id: u64) {
        let Some(painel) = self.paineis.iter_mut().find(|p| p.id == painel_id) else {
            return;
        };
        let (antes, novo) = (painel.leitura.deslocamento, painel.rolagem.offset());
        if !antes.as_f32().is_finite() || novo.y == antes {
            return;
        }
        let maximo = painel.rolagem.max_offset().y.as_f32();
        let de = painel.rolagem_suave.unwrap_or(antes.as_f32());
        let alvo = (de + (novo.y - antes).as_f32()).clamp(-maximo, 0.);
        if painel.rolagem_suave.is_none() {
            painel.geracao_rolagem += 1;
        }
        painel.rolagem_suave = Some(alvo);
        painel.rolagem.set_offset(point(novo.x, antes));
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
        painel.rolagem_suave = None;
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
                        .child(CabecalhoDeSecao::new("Pedidos").contagem(quantos).tingida(Cores::espera()).primeira(true)),
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

    /// Um pedido de permissão (o desenho é o `cartao_de_pedido`): as respostas daqui mexem no
    /// motor. O "Permitir" com a caixa "Não perguntar de novo" marcada passa a regra
    /// (`permitir_sempre`); desmarcada, permite só esta vez. O perigoso nunca vira regra e só se
    /// permite segurando (ou com dois Enter).
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
        let explicando = self.explicacao.as_ref().filter(|e| e.pedido == id).map(|e| e.campo.clone());
        let responder = |permitir: bool| {
            let id = id.to_string();
            cx.listener(move |this: &mut Janela, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                if let Some(m) = &this.motor {
                    m.update(cx, |m, cx| m.responder_permissao(&id, permitir, None, cx));
                }
            })
        };
        // A regra só existe fora do perigoso: o mesmo filtro do desenho.
        let com_regra = regra_do_pedido(ferramenta, entrada, &chat.cwd, manual).is_some();
        let permitir = {
            let id = id.to_string();
            cx.listener(move |this: &mut Janela, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                let sempre = com_regra && this.sempre_marcado.contains(&id);
                if let Some(m) = &this.motor {
                    m.update(cx, |m, cx| {
                        if sempre {
                            m.permitir_sempre(&id, cx)
                        } else {
                            m.responder_permissao(&id, true, None, cx)
                        }
                    });
                }
            })
        };
        let uma_vez = {
            let (motor, id) = (self.motor.clone(), id.to_string());
            move |cx: &mut App| {
                if let Some(m) = &motor {
                    m.update(cx, |m, cx| m.responder_permissao(&id, true, None, cx));
                }
            }
        };
        let marcar = {
            let id = id.to_string();
            cx.listener(move |this: &mut Janela, marcado: &bool, _, cx| {
                if *marcado {
                    this.sempre_marcado.insert(id.clone());
                } else {
                    this.sempre_marcado.remove(&id);
                }
                cx.notify();
            })
        };
        let explicar = {
            let id = id.to_string();
            cx.listener(move |this, _: &ClickEvent, window, cx| this.abrir_explicacao(id.clone(), window, cx))
        };
        let cancelar_explicacao = cx.listener(|this, _: &ClickEvent, _, cx| {
            this.explicacao = None;
            cx.notify();
        });
        let abrir = {
            let id = id.to_string();
            cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.pedido_aberto = Some(id.clone());
                cx.notify();
            })
        };
        let pedido = Pedido {
            id: id.to_string(),
            ferramenta: ferramenta.to_string(),
            entrada: entrada.clone(),
            pasta: chat.cwd.clone(),
            aberto: corpo_aberto,
            posicao,
            nivel,
            manual,
            marcado: self.sempre_marcado.contains(id),
            explicando,
        };
        let respostas = RespostasDoPedido {
            negar: Rc::new(responder(false)),
            permitir: Rc::new(permitir),
            uma_vez: Rc::new(uma_vez),
            marcar: Rc::new(marcar),
            explicar: Rc::new(explicar),
            cancelar_explicacao: Rc::new(cancelar_explicacao),
            enviar_explicacao: Rc::new(cx.listener(|this, _: &ClickEvent, _, cx| this.negar_explicando(cx))),
            abrir: Rc::new(abrir),
        };
        cartao_de_pedido(pedido, respostas, window, cx)
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
        let (icone_modo, cor_modo) = icone_do_modo(&depois);
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
        // O cabeçalho de azulejo, sem a faixa tingida: o âmbar vai no ícone.
        let cabecalho = h_flex()
            .id(id_cab)
            .flex_none()
            .h(r(44.))
            .px(dentro_da_borda(12.))
            .gap(esp::E8)
            .child(azulejo(Lucide::Map, Cores::ambar()))
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
        // A moldura dupla: o cartão de raio 16 dentro da moldura de raio 20.
        let cartao = miolo_da_moldura();
        if !corpo_aberto {
            // Recolhido (há outro pedido aberto): só o cabeçalho, com Aprovar.
            let aprovar =
                botao_primario(id_aprovar, Some(Lucide::Check.into()), "Aprovar", TamanhoBotao::Barra, window, cx)
                    .on_click(aprovar);
            return moldura_dupla(EstadoDaMoldura::Recolhida)
                .child(cartao.child(cabecalho.cursor_pointer().on_click(abrir).child(aprovar)))
                .into_any_element();
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
            .rounded(raio::de(32.))
            .tooltip("Aprovar em outro modo")
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                let (editando, perguntando) = (aprovar_editando.clone(), aprovar_perguntando.clone());
                menu.item(
                    PopupMenuItem::new("Aprovar e aceitar edições")
                        .icon(icone_do_modo("acceptEdits").0)
                        .on_click(move |e, window, cx| editando(e, window, cx)),
                )
                .item(
                    PopupMenuItem::new("Aprovar e perguntar antes")
                        .icon(icone_do_modo("manual").0)
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
        let cartao = cartao
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
            .child(div().flex_none().px(dentro_da_borda(12.)).pt(esp::E12).pb(dentro_da_borda(12.)).child(rodape));
        moldura_dupla(EstadoDaMoldura::Esperando).min_h_0().child(cartao.flex_1().min_h_0()).into_any_element()
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
        .rounded(raio::de(28.))
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

/// O marco da conversa: o último item que fecha o que veio antes dele (mensagem, entrega ou
/// aviso), e a partir do qual uma chamada sem resultado conta como interrompida. As `dirigidas`
/// últimas mensagens do usuário não fecham nada: elas entraram no turno que já rodava, e a
/// ferramenta de antes delas continua trabalhando.
fn marco_da_conversa(itens: &[Item], dirigidas: usize) -> Option<usize> {
    let mut dirigidas = dirigidas;
    itens.iter().rposition(|i| match i {
        Item::Usuario { .. } if dirigidas > 0 => {
            dirigidas -= 1;
            false
        }
        Item::Usuario { .. } | Item::Entrega { .. } | Item::Aviso { .. } => true,
        _ => false,
    })
}

/// Os números do turno que está aberto (desde a última mensagem sua ou entrega): quantas ações
/// (chamadas de ferramenta) e quantos arquivos distintos ele editou ou gravou.
fn numeros_do_turno(itens: &[Item]) -> (usize, usize) {
    let mut arquivos: Vec<&str> = Vec::new();
    let mut acoes = 0;
    for item in itens.iter().rev() {
        match item {
            Item::Usuario { .. } | Item::Entrega { .. } => break,
            Item::Ferramenta { nome, arquivo, .. } if nome != "ToolSearch" => {
                acoes += 1;
                let edita = matches!(nome_curto(nome), "Edit" | "MultiEdit" | "Write" | "NotebookEdit");
                if let Some(a) = arquivo.as_deref().filter(|a| edita && !arquivos.contains(a)) {
                    arquivos.push(a);
                }
            }
            _ => {}
        }
    }
    (acoes, arquivos.len())
}

/// O que o agente faz agora, tirado da última chamada ainda sem resultado no turno ("Lendo
/// motor.rs…"); sem chamada, "Escrevendo…" ou "Pensando…". As `dirigidas` últimas mensagens do
/// usuário entraram neste mesmo turno e não cortam a procura.
fn texto_atividade(itens: &[Item], escrevendo: bool, dirigidas: usize) -> String {
    let mut dirigidas = dirigidas;
    let atual = itens
        .iter()
        .rev()
        .take_while(|i| match i {
            // A mensagem dirigida não fecha o turno: a chamada antes dela ainda é a de agora.
            Item::Usuario { .. } if dirigidas > 0 => {
                dirigidas -= 1;
                true
            }
            Item::Usuario { .. } | Item::Entrega { .. } | Item::Aviso { .. } => false,
            _ => true,
        })
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
            Some(d) => format!("Criando o ajudante “{d}”…"),
            None => "Criando um ajudante…".into(),
        },
        "ver_diff" => match detalhe {
            Some(d) => format!("Olhando as mudanças do {d}…"),
            None => "Olhando as mudanças…".into(),
        },
        "pedir_ajuste" => match detalhe {
            Some(d) => format!("Pedindo ajuste ao {d}…"),
            None => "Pedindo um ajuste…".into(),
        },
        "aprovar_tarefa" => match detalhe {
            Some(d) => format!("Juntando o {d}…"),
            None => "Juntando um chat…".into(),
        },
        "cancelar_tarefa" => match detalhe {
            Some(d) => format!("Cancelando o {d}…"),
            None => "Cancelando um pedido…".into(),
        },
        "perguntar_coordenador" => "Perguntando a quem pediu…".into(),
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

/// Um pedido de permissão na tela: o que o `cartao_de_pedido` desenha (o da conversa e a amostra
/// da vitrine).
pub(super) struct Pedido {
    pub id: String,
    pub ferramenta: String,
    pub entrada: serde_json::Value,
    /// A pasta do chat (caminhos relativos, "fora da pasta", a regra).
    pub pasta: PathBuf,
    /// Com o corpo aberto; recolhido, só o cabeçalho numa linha.
    pub aberto: bool,
    /// "2 de 3", com vários pedidos.
    pub posicao: Option<(usize, usize)>,
    pub nivel: LarguraPainel,
    pub manual: bool,
    /// A caixa "Não perguntar de novo" marcada.
    pub marcado: bool,
    /// O campo de "negar e explicar" aberto.
    pub explicando: Option<Entity<InputState>>,
}

type AoClicar = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// As respostas de um pedido. `permitir` é o "Permitir" do pedido aberto e não perigoso (quem
/// monta decide se a caixa marcada vira regra); `uma_vez` é o do recolhido e o do segurar.
pub(super) struct RespostasDoPedido {
    pub negar: AoClicar,
    pub permitir: AoClicar,
    pub uma_vez: Rc<dyn Fn(&mut App)>,
    pub marcar: Rc<dyn Fn(&bool, &mut Window, &mut App)>,
    pub explicar: AoClicar,
    pub cancelar_explicacao: AoClicar,
    pub enviar_explicacao: AoClicar,
    pub abrir: AoClicar,
}

impl RespostasDoPedido {
}

/// O que a caixa "Não perguntar de novo" passaria a permitir (o objeto e a dica); `None` sem
/// regra possível e sempre no comando perigoso, que nunca vira regra.
fn regra_do_pedido(ferramenta: &str, entrada: &serde_json::Value, pasta: &Path, manual: bool) -> Option<(String, String)> {
    if pedido_perigoso(ferramenta, entrada) {
        return None;
    }
    crate::modelo::regra_de_permissao(ferramenta, entrada, pasta).map(|regra| rotulo_da_regra(&regra, manual))
}

/// O comando tem um trecho que apaga, força ou roda algo difícil de desfazer.
fn pedido_perigoso(ferramenta: &str, entrada: &serde_json::Value) -> bool {
    ferramenta == "Bash" && !trechos_perigosos(entrada["command"].as_str().unwrap_or_default()).is_empty()
}

/// O cartão de um pedido de permissão (base AI Tool Approval), em moldura dupla: por fora, a
/// moldura de raio 20 (a que espera você; vermelha no perigoso; preta no recolhido); dentro, com 4
/// de folga, o cartão opaco de raio 16. No cartão: o azulejo com o ícone da ferramenta, o título
/// afirmativo, a etiqueta da ferramenta em mono, o comando cavado com "$", o motivo embaixo e o
/// rodapé com a caixa "Não perguntar de novo", "Negar" fantasma e "Permitir" claro. O perigoso não
/// tem a caixa e o botão é "Segure para permitir".
pub(super) fn cartao_de_pedido(p: Pedido, r_: RespostasDoPedido, window: &Window, cx: &App) -> AnyElement {
    let id = p.id.as_str();
    let id_texto = |nome: &str| SharedString::from(format!("{nome}-{id}"));
    let entrada = &p.entrada;
    let perigoso = pedido_perigoso(&p.ferramenta, entrada);
    let (titulo, motivo) = titulo_do_pedido(&p.ferramenta, entrada, &p.pasta);
    let caminho_no_motivo = matches!(p.ferramenta.as_str(), "Edit" | "MultiEdit" | "Write" | "NotebookEdit");
    let fora = entrada["file_path"]
        .as_str()
        .or(entrada["notebook_path"].as_str())
        .is_some_and(|c| caminho_no_chat(c, &p.pasta).1);
    let regra = regra_do_pedido(&p.ferramenta, entrada, &p.pasta, p.manual);
    let minima = minima(p.nivel);
    let cabecalho = h_flex()
        .id(id_texto("cab"))
        .w_full()
        .gap(esp::E8)
        .child(azulejo(
            icone_da_ferramenta(&p.ferramenta),
            if perigoso { Cores::vermelho() } else { Cores::texto() },
        ))
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap(esp::E8)
                .child(div().min_w_0().truncate().titulo_cartao().text_color(Cores::forte()).child(titulo))
                .child(div().flex_none().etiqueta().text_color(Cores::apagado()).child(etiqueta_da_ferramenta(&p.ferramenta))),
        )
        // Em qualquer largura: sem o aviso, um arquivo de fora da pasta parecia dela.
        .when(fora, |s| s.child(Etiqueta::new("fora da pasta").tom(Cores::ambar())))
        .when_some(p.posicao, |s, (k, n)| {
            s.child(div().flex_none().etiqueta().text_color(Cores::tenue()).child(format!("{k} de {n}")))
        });
    let uma_vez = r_.uma_vez.clone();
    let segurar = |id: SharedString, t: TamanhoBotao| {
        let uma_vez = uma_vez.clone();
        BotaoDeSegurar::new(id, "Segure para permitir").tamanho(t).ao_completar(move |cx| uma_vez(cx))
    };
    let estado = if perigoso {
        EstadoDaMoldura::Perigo
    } else if p.aberto {
        EstadoDaMoldura::Esperando
    } else {
        EstadoDaMoldura::Recolhida
    };
    if !p.aberto {
        // Recolhido (há outro pedido aberto): o cabeçalho numa linha, com Negar e Permitir (só
        // esta vez; no perigoso, segurando). O clique no resto abre o corpo.
        let negar = r_.negar.clone();
        let negar = botao_fantasma(id_texto("negar"), None, "Negar", TamanhoBotao::Barra, window, cx)
            .on_click(move |e, window, cx| negar(e, window, cx));
        let permitir = if perigoso {
            segurar(id_texto("segurar"), TamanhoBotao::Barra).into_any_element()
        } else {
            botao_claro(id_texto("permitir"), None, "Permitir", TamanhoBotao::Barra, window, cx)
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    uma_vez(cx)
                })
                .into_any_element()
        };
        let abrir = r_.abrir.clone();
        return moldura_dupla(estado)
            .child(
                miolo_da_moldura().py(esp::E8).px(esp::E12).child(
                    cabecalho
                        .cursor_pointer()
                        .on_click(move |e, window, cx| abrir(e, window, cx))
                        .child(h_flex().flex_none().gap(esp::E6).child(negar).child(permitir)),
                ),
            )
            .into_any_element();
    }
    let botao = |f: &AoClicar| {
        let f = f.clone();
        move |e: &ClickEvent, window: &mut Window, cx: &mut App| f(e, window, cx)
    };
    let rodape = match p.explicando {
        Some(campo) => h_flex()
            .gap(esp::E8)
            .child(div().flex_1().min_w_0().child(Input::new(&campo)))
            .child(
                botao_fantasma(id_texto("cancelar-explicacao"), None, "Cancelar", TamanhoBotao::Controle, window, cx)
                    .on_click(botao(&r_.cancelar_explicacao)),
            )
            .child(
                botao_primario(id_texto("negar-explicando"), None, "Negar e enviar", TamanhoBotao::Controle, window, cx)
                    .on_click(botao(&r_.enviar_explicacao)),
            )
            .into_any_element(),
        None => {
            let explicar = botao_icone(
                id_texto("explicar"),
                Lucide::MessageSquareQuote,
                "Negar e explicar…\nNega e diz ao Claude o que fazer em vez disso",
                TamanhoBotao::Controle,
                window,
                cx,
            )
            .on_click(botao(&r_.explicar));
            let negar =
                botao_fantasma(id_texto("negar"), None, "Negar", TamanhoBotao::Controle, window, cx).on_click(botao(&r_.negar));
            // A caixa "Não perguntar de novo: ..." só existe fora do perigoso e quando há regra.
            let caixa = regra.map(|(objeto, dica)| {
                let marcar = r_.marcar.clone();
                Checkbox::new(id_texto("sempre"))
                    .label(format!("Não perguntar de novo: {objeto}"))
                    .checked(p.marcado)
                    .tooltip(dica)
                    .on_click(move |marcado, window, cx| marcar(marcado, window, cx))
                    .text_color(Cores::tenue())
                    .metadado()
            });
            let principal = if perigoso {
                segurar(id_texto("segurar"), TamanhoBotao::Controle).map(|b| if minima { b.largo() } else { b }).into_any_element()
            } else {
                botao_claro(id_texto("permitir"), None, "Permitir", TamanhoBotao::Controle, window, cx)
                    .when(minima, |b| b.w_full())
                    .on_click(botao(&r_.permitir))
                    .into_any_element()
            };
            if minima {
                // Na largura mínima, a caixa vai em cima e o Permitir ocupa a largura.
                v_flex()
                    .gap(esp::E8)
                    .children(caixa)
                    .child(h_flex().gap(esp::E8).child(explicar).child(negar))
                    .child(principal)
                    .into_any_element()
            } else {
                h_flex()
                    .gap(esp::E8)
                    .child(div().flex_1().min_w_0().children(caixa))
                    .child(explicar)
                    .child(negar)
                    .child(principal)
                    .into_any_element()
            }
        }
    };
    moldura_dupla(estado)
        .child(
            miolo_da_moldura()
                .p(esp::E12)
                .gap(esp::E12)
                .child(cabecalho)
                .child(
                    v_flex()
                        .gap(esp::E6)
                        .child(corpo_permissao(id, &p.ferramenta, entrada))
                        // O que era subtítulo vira o motivo, embaixo do comando.
                        .when_some(motivo, |s, m| {
                            s.child(
                                div()
                                    .metadado()
                                    .text_color(Cores::tenue())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .map(|s| if caminho_no_motivo { s.text_ellipsis_start() } else { s.text_ellipsis() })
                                    .child(sem_marcacao(&m)),
                            )
                        }),
                )
                .when(perigoso, |s| {
                    s.child(
                        h_flex()
                            .items_start()
                            .gap(esp::E6)
                            .child(div().flex_none().pt(esp::E2).child(icone_de(Lucide::TriangleAlert, 14.).text_color(Cores::vermelho())))
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

/// A etiqueta da ferramenta no cabeçalho do pedido, em mono: "bash", "edit", "write", "web" ou o
/// servidor do MCP.
fn etiqueta_da_ferramenta(ferramenta: &str) -> String {
    if let Some((servidor, _)) = ferramenta.strip_prefix("mcp__").and_then(|n| n.split_once("__")) {
        return servidor.to_string();
    }
    match nome_curto(ferramenta) {
        "Bash" => "bash".into(),
        "Edit" | "MultiEdit" => "edit".into(),
        "Write" => "write".into(),
        "NotebookEdit" => "notebook".into(),
        "WebFetch" | "WebSearch" => "web".into(),
        outro => outro.to_lowercase(),
    }
}

/// Título afirmativo do pedido de permissão ("Rodar um comando", "Editar views.py") e o motivo,
/// que vai embaixo do comando (a descrição dele, o caminho, a consulta).
fn titulo_do_pedido(ferramenta: &str, entrada: &serde_json::Value, pasta: &Path) -> (String, Option<String>) {
    let campo = |chave: &str| entrada[chave].as_str().filter(|s| !s.is_empty());
    let caminho = campo("file_path").or(campo("notebook_path"));
    let arquivo = caminho
        .and_then(|c| Path::new(c).file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "o arquivo".into());
    let relativo = caminho.map(|c| caminho_no_chat(c, pasta).0);
    match ferramenta {
        "Bash" => ("Rodar um comando".into(), campo("description").map(str::to_string)),
        "Edit" => (format!("Editar {arquivo}"), relativo),
        "MultiEdit" => {
            let trechos = entrada["edits"].as_array().map_or(0, Vec::len);
            let detalhe = contagem(trechos, "trecho", "trechos");
            (format!("Editar {arquivo}"), Some(relativo.map_or(detalhe.clone(), |c| format!("{c} · {detalhe}"))))
        }
        "Write" => {
            let linhas = contagem(campo("content").map_or(0, |c| c.lines().count()), "linha", "linhas");
            (format!("Gravar {arquivo}"), Some(relativo.map_or(linhas.clone(), |c| format!("{c} · {linhas}"))))
        }
        "NotebookEdit" if campo("edit_mode") == Some("delete") => (format!("Apagar uma célula de {arquivo}"), relativo),
        "NotebookEdit" => (format!("Editar uma célula de {arquivo}"), relativo),
        "WebFetch" => {
            let url = campo("url").unwrap_or_default();
            let dominio = url.split("://").last().unwrap_or(url).split('/').next().unwrap_or_default();
            (format!("Abrir {dominio}"), Some(url.to_string()).filter(|u| !u.is_empty()))
        }
        "WebSearch" => ("Pesquisar na web".into(), campo("query").map(str::to_string)),
        _ => {
            let resumo = Some(crate::claude::resumo_ferramenta(ferramenta, entrada));
            // O servidor do MCP vai na etiqueta da ferramenta, ao lado do título.
            match ferramenta.strip_prefix("mcp__").and_then(|n| n.split_once("__")) {
                Some((_, nome)) => (format!("Usar {nome}"), resumo),
                None => (format!("Usar {}", nome_curto(ferramenta)), resumo),
            }
        }
    }
}

/// O objeto da caixa "Não perguntar de novo: ..." ("edições nesta conversa", "este comando", o nome
/// da ferramenta) e a dica dela: o que passa a valer sem perguntar.
fn rotulo_da_regra(regra: &str, manual: bool) -> (String, String) {
    const DESFAZER: &str = "Para desfazer, use o menu do modo, embaixo da caixa de mensagem.";
    if regra == "edicoes" {
        let modo = if manual {
            " Nos próximos turnos, a conversa fica em Aceitar edições."
        } else {
            ""
        };
        return (
            "edições nesta conversa".into(),
            format!(
                "As edições de arquivos passam sem perguntar nesta conversa, já neste turno.{modo} \
                 {DESFAZER}"
            ),
        );
    }
    if regra.starts_with("bash:") {
        return (
            "este comando".into(),
            format!("Este comando, exatamente igual, passa sem perguntar nesta conversa. {DESFAZER}"),
        );
    }
    let nome = nome_curto(regra.strip_prefix("ferramenta:").unwrap_or(regra));
    (
        nome.to_string(),
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
            // A caixa mostra umas 11 linhas e rola: com o comando maior que isso, o cartão avisa
            // (sem o aviso, o que ficou abaixo da dobra parecia não existir).
            let linhas = comando.lines().count();
            let longo = linhas > 11 || comando.chars().count() > 900;
            // Cavado, com o "$" na frente, como numa linha de terminal.
            let caixa = caixa_de_codigo(id).shadow(aresta_de_poco()).text_color(Cores::texto()).child(
                h_flex()
                    .items_start()
                    .gap(esp::E8)
                    .child(div().flex_none().text_color(Cores::fantasma()).child("$"))
                    .child(
                        div().flex_1().min_w_0().child(
                            StyledText::new(comando).with_highlights(perigosos.iter().map(|t| (t.clone(), vermelho))),
                        ),
                    ),
            );
            v_flex()
                .gap(r(6.))
                .child(caixa)
                .when(longo, |s| s.child(nota("Comando comprido: role a caixa para ler tudo antes de permitir.")))
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
                return div().my(r(4.)).h(px(1.)).bg(Cores::traco()).into_any_element();
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
        Bloco, ClasseAvisoChat, MarcaDeLinha, comando_de_barra, detalhe_da_chamada, entrada_do_bloco, espaco_antes,
        linhas_da_edicao, marco_da_conversa, nome_e_id, partes_do_erro, previa, resumo_da_trilha, sem_marcacao,
        separadores_de_data, etiqueta_da_ferramenta, numeros_do_turno, regra_do_pedido, texto_atividade, tipo_aviso,
        titulo_do_pedido, trechos_perigosos, verbo_da_ferramenta,
    };
    use crate::modelo::{Chat, Item, regra_de_permissao};
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn duracao_do_raciocinio_e_passo_do_subagente() {
        use super::{detalhe_do_passo, duracao_curta};
        assert_eq!(duracao_curta(9_800), "9,8 s");
        assert_eq!(duracao_curta(42_300), "42 s");
        assert_eq!(duracao_curta(80_000), "1 min 20 s");
        assert_eq!(detalhe_do_passo("Read · admin.py"), "Leu admin.py");
        assert_eq!(detalhe_do_passo("Conferindo os testes do carrinho."), "Conferindo os testes do carrinho.");
    }

    #[test]
    fn o_diff_da_edicao_e_contado_na_linha() {
        use super::contagem_do_diff;
        assert_eq!(contagem_do_diff("- a\n- b\n+ c"), Some((1, 2)));
        assert_eq!(contagem_do_diff("+ a\n+ b\n… mais 40 linhas"), Some((42, 0)));
        assert_eq!(contagem_do_diff("- a\n… mais 3 linhas\n+ b"), Some((1, 4)));
        assert_eq!(contagem_do_diff("- a\n…\n+ b"), Some((1, 1)));
        assert_eq!(contagem_do_diff("+ a\n… (cortado)"), None);
        assert_eq!(contagem_do_diff("$ ls"), None);
    }

    fn ferramenta(nome: &str, resumo: &str, resultado: Option<&str>, erro: bool) -> Item {
        Item::Ferramenta {
            id: "x".into(),
            nome: nome.into(),
            resumo: resumo.into(),
            resultado: resultado.map(str::to_string),
            erro,
            em: 0,
            detalhe: None,
            arquivo: None,
            sub: None,
        }
    }

    fn usuario(texto: &str) -> Item {
        Item::Usuario { texto: texto.into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0 }
    }

    fn assistente(texto: &str) -> Item {
        Item::Assistente { texto: texto.into(), em: 0 }
    }

    /// 12/03/2026 ao meio-dia UTC: ao meio-dia a data local é a mesma em qualquer fuso de verdade.
    const MEIO_DIA: u64 = 1_773_316_800;
    const DIA: u64 = 86_400;

    #[test]
    fn separador_de_data_a_cada_dia_novo() {
        // Dois itens no mesmo dia, um no dia seguinte e outro bem depois: três separadores.
        let horas = [MEIO_DIA - DIA, MEIO_DIA - DIA + 60, MEIO_DIA, MEIO_DIA + 10 * DIA];
        let separadores = separadores_de_data(&horas, MEIO_DIA);
        assert_eq!(
            separadores,
            [(0, "Ontem".to_string()), (2, "Hoje".to_string()), (3, "22 de março".to_string())]
        );
    }

    #[test]
    fn separador_de_data_pula_item_sem_hora() {
        // Conversa de antes da hora: nenhum separador, e o agrupamento não quebra.
        assert!(separadores_de_data(&[0, 0, 0], MEIO_DIA).is_empty());
        // Com um item sem hora no meio do dia, o dia continua o mesmo (um separador só).
        let horas = [MEIO_DIA, 0, MEIO_DIA + 60];
        assert_eq!(separadores_de_data(&horas, MEIO_DIA), [(0, "Hoje".to_string())]);
        // O primeiro item com hora abre o dia, mesmo vindo depois dos sem hora.
        let horas = [0, MEIO_DIA, MEIO_DIA + 60];
        assert_eq!(separadores_de_data(&horas, MEIO_DIA), [(1, "Hoje".to_string())]);
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
        // O marco pede ar dos dois lados.
        assert_eq!(espaco_antes(Some(Texto), Marco), 16.);
        assert_eq!(espaco_antes(Some(Marco), Log), 16.);
        // O resto, dentro do turno.
        assert_eq!(espaco_antes(Some(Texto), Log), 12.);
        assert_eq!(espaco_antes(Some(Log), Alerta), 12.);
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
            ("Rodar um comando".into(), Some("Roda os testes".into()))
        );
        assert_eq!(titulo("Bash", json!({"command": "ls"})), ("Rodar um comando".into(), None));
        assert_eq!(
            titulo("Edit", json!({"file_path": "/home/v/loja/src/cupom.rs"})),
            ("Editar cupom.rs".into(), Some("src/cupom.rs".into()))
        );
        assert_eq!(
            titulo("MultiEdit", json!({"file_path": "/home/v/loja/a.rs", "edits": [{}, {}, {}]})),
            ("Editar a.rs".into(), Some("a.rs · 3 trechos".into()))
        );
        assert_eq!(
            titulo("Write", json!({"file_path": "/etc/hosts", "content": "a\nb"})),
            ("Gravar hosts".into(), Some("/etc/hosts · 2 linhas".into()))
        );
        assert_eq!(
            titulo("NotebookEdit", json!({"notebook_path": "/home/v/loja/n.ipynb", "edit_mode": "delete"})).0,
            "Apagar uma célula de n.ipynb"
        );
        assert_eq!(
            titulo("WebFetch", json!({"url": "https://docs.rs/gpui/latest"})),
            ("Abrir docs.rs".into(), Some("https://docs.rs/gpui/latest".into()))
        );
        assert_eq!(titulo("WebSearch", json!({"query": "gpui"})).0, "Pesquisar na web");
        assert_eq!(titulo("mcp__github__create_issue", json!({})).0, "Usar create_issue");
        assert_eq!(titulo("Agente", json!({})).0, "Usar Agente");
        // Título afirmativo: nenhum termina em pergunta.
        for f in ["Bash", "Edit", "Write", "WebFetch", "WebSearch", "mcp__github__create_issue", "Agente"] {
            assert!(!titulo(f, json!({"command": "ls", "file_path": "/home/v/loja/a.rs"})).0.ends_with('?'), "{f}");
        }
    }

    #[test]
    fn etiqueta_da_ferramenta_no_cabecalho_do_pedido() {
        assert_eq!(etiqueta_da_ferramenta("Bash"), "bash");
        assert_eq!(etiqueta_da_ferramenta("MultiEdit"), "edit");
        assert_eq!(etiqueta_da_ferramenta("Write"), "write");
        assert_eq!(etiqueta_da_ferramenta("WebSearch"), "web");
        assert_eq!(etiqueta_da_ferramenta("mcp__github__create_issue"), "github");
        assert_eq!(etiqueta_da_ferramenta("Agente"), "agente");
    }

    /// O comando perigoso nunca vira regra (nem com a caixa marcada); o comum vira "este comando".
    #[test]
    fn perigoso_nunca_vira_regra() {
        let pasta = Path::new("/home/v/loja");
        assert_eq!(regra_do_pedido("Bash", &json!({"command": "rm -rf build"}), pasta, true), None);
        assert_eq!(regra_do_pedido("Bash", &json!({"command": "git push --force"}), pasta, true), None);
        let comum = regra_do_pedido("Bash", &json!({"command": "cargo test"}), pasta, true);
        assert_eq!(comum.map(|(objeto, _)| objeto), Some("este comando".to_string()));
    }

    #[test]
    fn textos_sem_marcacao_na_tela() {
        assert_eq!(sem_marcacao("Verificação falhou: `cargo test` e **dois** erros"), "Verificação falhou: cargo test e dois erros");
        assert_eq!(previa("Tarefa #2:\n\n  Aplicar `cupom`\nno checkout"), "Tarefa #2: Aplicar cupom no checkout");
        assert_eq!(comando_de_barra(" /compact "), Some("/compact"));
        assert_eq!(comando_de_barra("/clear\nmais"), None);
        assert_eq!(comando_de_barra("não é /comando"), None);
    }

    #[test]
    fn nome_de_quem_mandou() {
        assert_eq!(nome_e_id("Coordenador do site (abc)"), ("Coordenador do site", Some("abc")));
        assert_eq!(nome_e_id("#3 Login"), ("#3 Login", None));
        assert_eq!(nome_e_id("Tarefa (com espaço)"), ("Tarefa (com espaço)", None));
    }

    #[test]
    fn atividade_vem_da_chamada_em_andamento_do_turno() {
        let ler = ferramenta("Read", "Read · motor.rs", None, false);
        let feita = ferramenta("Bash", "Bash · Roda os testes", Some("ok"), false);
        assert_eq!(texto_atividade(&[usuario("oi"), feita.clone(), ler.clone()], false, 0), "Lendo motor.rs…");
        // A última sem resultado é a que vale, mesmo com uma terminada depois.
        assert_eq!(texto_atividade(&[usuario("oi"), ler.clone(), feita.clone()], false, 0), "Lendo motor.rs…");
        // Sem resultado de um turno anterior não conta: foi interrompida.
        assert_eq!(texto_atividade(&[ler.clone(), usuario("de novo")], false, 0), "Pensando…");
        assert_eq!(texto_atividade(&[usuario("oi"), feita], true, 0), "Escrevendo…");
        let bash = ferramenta("Bash", "Bash · cargo test -p vitral…", None, false);
        assert_eq!(texto_atividade(&[bash], false, 0), "Rodando: cargo test -p vitral…");
        let criar = ferramenta("mcp__vitral__criar_tarefa", "criar_tarefa · Validar entrada", None, false);
        assert_eq!(texto_atividade(&[criar], false, 0), "Criando o ajudante “Validar entrada”…");
        let diff = ferramenta("mcp__vitral__ver_diff", "ver_diff · chat 3", None, false);
        assert_eq!(texto_atividade(&[diff], false, 0), "Olhando as mudanças do chat 3…");
        let outro = ferramenta("mcp__github__create_issue", "mcp__github__create_issue", None, false);
        assert_eq!(texto_atividade(&[outro], false, 0), "Usando create_issue…");
        let sem_detalhe = ferramenta("Edit", "Edit", None, false);
        assert_eq!(texto_atividade(&[sem_detalhe], false, 0), "Editando um arquivo…");
        let gravar = ferramenta("Write", "Write · cupom.rs", None, false);
        assert_eq!(texto_atividade(&[gravar], false, 0), "Gravando cupom.rs…");
        let passos = ferramenta("TodoWrite", "TodoWrite · lista de tarefas", None, false);
        assert_eq!(texto_atividade(&[passos], false, 0), "Atualizando os passos…");
    }

    #[test]
    fn numeros_do_turno_contam_so_o_turno_aberto() {
        let mut editar = ferramenta("Edit", "Edit · views.py", Some("ok"), false);
        if let crate::modelo::Item::Ferramenta { arquivo, .. } = &mut editar {
            *arquivo = Some("produtos/views.py".into());
        }
        let ler = ferramenta("Read", "Read · views.py", Some("ok"), false);
        let rodar = ferramenta("Bash", "Bash · python3 manage.py test", None, false);
        // O turno de antes não conta; duas edições do mesmo arquivo são um arquivo.
        let itens = [ler.clone(), usuario("ajusta"), ler, editar.clone(), editar, rodar];
        assert_eq!(numeros_do_turno(&itens), (4, 1));
        assert_eq!(numeros_do_turno(&[usuario("oi")]), (0, 0));
    }

    #[test]
    fn a_mensagem_dirigida_nao_fecha_a_chamada_do_turno() {
        let ler = ferramenta("Read", "Read · motor.rs", None, false);
        let itens = [usuario("oi"), ler.clone(), usuario("e olha isto também")];
        // Sem dirigida, a mensagem fecha o que veio antes: a chamada ficou sem resultado.
        assert_eq!(texto_atividade(&itens, false, 0), "Pensando…");
        assert_eq!(marco_da_conversa(&itens, 0), Some(2));
        // Dirigida ao turno que já rodava: a chamada de antes dela continua sendo a de agora.
        assert_eq!(texto_atividade(&itens, false, 1), "Lendo motor.rs…");
        assert_eq!(marco_da_conversa(&itens, 1), Some(0));
        // Duas dirigidas seguidas.
        let duas = [usuario("oi"), ler, usuario("mais uma"), usuario("e outra")];
        assert_eq!(texto_atividade(&duas, false, 2), "Lendo motor.rs…");
        assert_eq!(marco_da_conversa(&duas, 2), Some(0));
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

    // Caminhos do Unix; os do Windows (C:\…, e a regra //c/… do Claude Code) são do Lote W2.
    #[cfg(unix)]
    #[test]
    fn caminho_relativo_a_pasta_do_chat() {
        use super::caminho_no_chat;
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
        // Caminhos do Unix (no Windows, "/home/v/outro" nem é absoluto; os de lá são do Lote W2).
        let unix = cfg!(unix);
        if unix {
            assert_eq!(editar("/home/v/projeto/src/a.rs").as_deref(), Some("edicoes"));
            assert_eq!(editar("/home/v/outro/a.rs"), None);
            assert_eq!(editar("/home/v/projeto/../outro/a.rs"), None);
        }
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
        assert!(!unix || !chat.permite("Write", &json!({"file_path": "/home/v/.bashrc"})));
    }
}
