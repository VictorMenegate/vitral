//! As mensagens de uma conversa: texto em markdown, ferramentas numa trilha calma, entregas de
//! outros chats, pedidos de permissão e plano, a faixa do turno e a rolagem
//! de cada painel.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use gpui_kit::base::InteractiveElementExt as _;

use gpui_kit::component::{
    ActiveTheme as _, ElementExt as _, Icon, StyledExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{Enter, Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::{ScrollableElement as _, Scrollbar, ScrollbarMode},
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
        AnelGirando, BotaoCopiar, CartaoDeErro, BotaoDeSegurar, CabecalhoDeSecao, EstadoDaMoldura, Etiqueta, azulejo,
        botao_claro, miolo_da_moldura, moldura_dupla, CaixaDeCodigo, EstadoVazio, FaixaCaixa, Flutuante as _,
        LarguraPainel, Marca, Papel as Quem, Selo, TamanhoBotao, botao_fantasma, botao_icone, botao_primario, relogio,
        contagem, icone_do_modo, motivo_curto, teclas, texto_arco_iris,
    },
    busca_conversa::{self, Campo, Realce, RolarAoAchado, TextoAchado},
    composer::{self, PararTurno, nome_do_modo},
    imagens,
    movimento::{self, De},
    paineis::tamanho_da_conversa,
};
use crate::{
    modelo::{Agente, Chat, Execucao, Item, Reforco},
    motor::Motor,
    tema::{
        Cores, FONTE_MONO, Lucide, Tipo as _, aresta_de_poco, dentro_da_borda, esp, halo_foco, r, raio, tam,
    },
};

pub(super) mod historico;
mod erros;

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
    /// O separador de dia ("Hoje"): começa um turno, com o espaço de entre turnos antes dele.
    Dia,
    Alerta,
}

/// Espaço antes de um bloco: 32 antes do seu balão e do separador de dia (entre turnos), 12 dentro
/// do turno (também do balão até a resposta e do dia até o balão), 16 em volta de marco e nada
/// entre linhas de log, que ficam coladas no passo de 28. Com 24 entre turnos e 16 do balão até a
/// resposta, o vão de fora era só 1,4 vez o de dentro, e a trilha ficava solta no meio; com mais
/// de 2 vezes, o olho agrupa a pergunta com a resposta (relatório de aparência, 16/11/2026, V3).
/// Nada fica reservado para as ações: elas flutuam no hover.
fn espaco_antes(anterior: Option<Bloco>, atual: Bloco) -> Rems {
    use Bloco::*;
    let Some(anterior) = anterior else {
        return r(0.);
    };
    match (anterior, atual) {
        (_, Dia) => esp::ENTRE_TURNOS,
        (Dia, _) => esp::NO_TURNO,
        (_, Balao) => esp::ENTRE_TURNOS,
        // O marco vem antes do balão: o balão seguido de marco continua em 16.
        (Marco, _) | (_, Marco) => esp::E16,
        (Balao, _) => esp::NO_TURNO,
        (Log, Log) => r(0.),
        (Recebida, Log | Recebida) => esp::E4,
        _ => esp::NO_TURNO,
    }
}

/// A entrada de um bloco da conversa (E5): nenhuma para o que já estava nela quando abriu no
/// painel (`base`) ou para o que não tem índice; para o resto, `Some(nova)`, com `nova` só no
/// quadro em que o bloco aparece (`vistos`). Trocar de conversa ou rolar não anima nada.
fn entrada_do_bloco(indice: Option<usize>, base: usize, vistos: usize) -> Option<bool> {
    indice.filter(|i| *i >= base).map(|i| i >= vistos)
}

fn deslocamento_apos_prepend(offset: Pixels, altura_antes: Pixels, altura_depois: Pixels) -> Pixels {
    offset - (altura_depois - altura_antes).max(px(0.))
}

/// Onde a leitura de um chat parou num painel: o item no alto da tela e quanto dele já tinha
/// subido. Guarda o item, e não o pixel, porque a altura muda com a largura (rodada de uso, 2.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ancora {
    pub item: usize,
    pub desloc: Pixels,
}

/// Onde a conversa abre ao trocar de chat num painel (rodada de uso, 2.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlvoDeAbertura {
    /// No começo da última resposta (com a linha "Trabalhou…" logo acima), se ela não couber.
    InicioDaResposta,
    /// Onde você parou de ler este chat neste painel.
    Ancora(Ancora),
}

/// O que fazer com o alvo, já com os blocos medidos.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Abertura {
    /// No fim, seguindo o que chegar.
    Fim,
    /// Com este ponto da lista no alto da tela.
    Em(Pixels),
    /// O item da âncora ainda não está desenhado: mais itens antes.
    CarregarMais,
}

/// Um respiro acima do começo da resposta, para ela não abrir colada na borda.
const RESPIRO_DA_ABERTURA: Pixels = px(12.);

/// A âncora de leitura: o primeiro bloco que ainda aparece abaixo de `topo` (o alto da tela, a
/// partir do alto da lista) e quanto dele já subiu.
pub(super) fn ancora_de(posicoes: &[(usize, Pixels, Pixels)], topo: Pixels) -> Option<Ancora> {
    let &(item, alto, _) = posicoes.iter().find(|(_, _, pe)| *pe > topo)?;
    Some(Ancora { item, desloc: (topo - alto).max(px(0.)) })
}

/// Onde abrir: no começo da resposta (`inicio`: o primeiro item depois da sua última mensagem) só
/// quando o que vem dali até o fim não cabe na `vista`; na âncora, com o deslocamento que ela
/// guardou, e no fim quando o item dela saiu (o Corrigir o tirou).
pub(super) fn onde_abrir(
    alvo: AlvoDeAbertura,
    posicoes: &[(usize, Pixels, Pixels)],
    altura: Pixels,
    vista: Pixels,
    inicio: Option<usize>,
    itens: usize,
) -> Abertura {
    match alvo {
        AlvoDeAbertura::InicioDaResposta => {
            let Some(&(_, alto, _)) = inicio.and_then(|inicio| posicoes.iter().find(|(i, ..)| *i >= inicio)) else {
                return Abertura::Fim;
            };
            let alto = (alto - RESPIRO_DA_ABERTURA).max(px(0.));
            if altura - alto <= vista { Abertura::Fim } else { Abertura::Em(alto) }
        }
        AlvoDeAbertura::Ancora(ancora) => {
            if ancora.item >= itens {
                return Abertura::Fim;
            }
            // O bloco que contém o item: o último que começa nele ou antes (uma trilha junta vários).
            match posicoes.iter().rev().find(|(i, ..)| *i <= ancora.item) {
                Some(&(i, alto, _)) => Abertura::Em(if i == ancora.item { alto + ancora.desloc } else { alto }),
                // O primeiro desenhado já vem depois dela: ela está mais para cima.
                None if !posicoes.is_empty() => Abertura::CarregarMais,
                None => Abertura::Fim,
            }
        }
    }
}

/// O primeiro item que você não viu e que não é mensagem sua: onde entra o separador "Novo".
/// Item sem hora (conversa de antes dela) não conta.
pub(super) fn item_do_separador_novo(itens: &[Item], lido_em: u64) -> Option<usize> {
    itens.iter().position(|item| item.em() > lido_em && !matches!(item, Item::Usuario { .. }))
}

/// O primeiro item depois da sua última mensagem (ou da última entrega, sem mensagem sua): onde
/// começa a resposta que a conversa abre no alto.
pub(super) fn inicio_da_ultima_resposta(itens: &[Item]) -> Option<usize> {
    let ultima = itens
        .iter()
        .rposition(|i| matches!(i, Item::Usuario { .. }))
        .or_else(|| itens.iter().rposition(|i| matches!(i, Item::Entrega { .. })));
    match ultima {
        Some(i) => (i + 1 < itens.len()).then_some(i + 1),
        None => (!itens.is_empty()).then_some(0),
    }
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
        if chat.itens.is_empty() && chat.importada.is_none() {
            blocos.push((Bloco::Texto, None, self.dica_inicial(motor, chat, window, cx)));
        }
        // Conversas antigas guardaram as buscas de ferramenta; não interessam. O modo foco tira
        // todas as ferramentas.
        let mut visiveis: Vec<(usize, &Item)> = chat
            .itens
            .iter()
            .enumerate()
            .filter(|(i, item)| match item {
                Item::Ferramenta { nome, .. } => nome != "ToolSearch" && !foco,
                Item::Assistente { .. } => !resposta_repete_o_erro(&chat.itens, *i),
                _ => true,
            })
            .collect();
        // Só os últimos itens são desenhados (ver `ITENS_NA_TELA`); com a busca aberta, todos, para
        // o Ctrl+F achar e rolar até qualquer um.
        let mostrar = if self.paineis[ix].busca.aberta { usize::MAX } else { self.paineis[ix].mostrar };
        // O marco "O Vitral fechou no meio do turno" no fim, com a faixa logo abaixo dizendo
        // "Parada · Vitral fechou" com o Retomar: o mesmo fato duas vezes antes da caixa. Sai o
        // marco (só o último, só com essa faixa, e não com a busca aberta); o texto gravado fica
        // (relatório de aparência, 16/11/2026, V15).
        if !self.paineis[ix].busca.aberta
            && visiveis.last().is_some_and(|(_, item)| matches!(item, Item::Aviso { texto, erro: false, .. } if texto.starts_with(VITRAL_FECHADO)))
            && matches!(situacao_do_turno(motor, &chat.id), Some(Situacao::Parada { ref motivo, .. }) if motivo_curto(motivo) == "Vitral fechou")
        {
            visiveis.pop();
        }
        let escondidos = visiveis.len().saturating_sub(mostrar);
        if escondidos > 0 {
            visiveis.drain(..escondidos);
        } else if chat.importada.is_some() {
            // A conversa trazida do Claude Code: o que foi dito lá vem antes, lido do arquivo dela.
            blocos.extend(self.blocos_do_historico(motor, chat, nivel, ix, window, cx));
        }
        // O dia de cada turno: o separador entra onde a data muda (pela posição em `visiveis`).
        let horas: Vec<u64> = visiveis.iter().map(|(_, item)| item.em()).collect();
        let datas: HashMap<usize, String> = separadores_de_data(&horas, agora).into_iter().collect();
        // O separador "Novo", antes do primeiro bloco que tem o primeiro item que você não viu
        // (quando ele está carregado; numa trilha, antes dela).
        let mut novo = self.paineis[ix]
            .novo_desde
            .and_then(|lido| item_do_separador_novo(&chat.itens, lido))
            .filter(|n| escondidos == 0 || visiveis.first().is_some_and(|(i, _)| i <= n));
        // Os turnos prontos recolhem numa linha "Trabalhou…" (rodada de uso, 2.1). No modo foco as
        // ferramentas já saíram, e não há o que recolher.
        let turnos = if foco { Vec::new() } else { turnos_recolhiveis(&chat.itens, contexto.rodando) };
        let mut turno_posto: Option<usize> = None;
        // A linha do turno não entra com fade: o primeiro item dele, aberto, já usa o índice.
        let mut sem_entrada: std::collections::HashSet<usize> = Default::default();
        let mut i = 0;
        while i < visiveis.len() {
            let (indice, item) = visiveis[i];
            if let Some(dia) = datas.get(&i) {
                blocos.push((Bloco::Dia, None, separador_de_dia(dia)));
            }
            // A revisão da memória vira uma linha: "Revendo a memória…" enquanto roda, e depois
            // "Memória revista · nada a guardar" ou "Memória: editou AGENTS.md" (item 6.1).
            if crate::motor::e_revisao_da_memoria(item) {
                let fim_item = chat.itens[indice + 1..]
                    .iter()
                    .position(|i| matches!(i, Item::Usuario { .. } | Item::Entrega { .. }))
                    .map_or(chat.itens.len(), |p| indice + 1 + p);
                let chave = format!("{}:{indice}:memoria", chat.id);
                let aberta = self.expandidos.contains(&chave);
                let rodando = fim_item == chat.itens.len() && motor.revendo_memoria(&chat.id);
                sem_entrada.insert(blocos.len());
                blocos.push((Bloco::Log, Some(indice), self.linha_da_memoria(&contexto, &chave, &chat.itens[indice..fim_item], rodando, aberta, window, cx)));
                if !aberta {
                    i = visiveis[i..].iter().position(|(k, _)| *k >= fim_item).map_or(visiveis.len(), |p| i + p);
                    continue;
                }
            }
            if let Some(turno) = turnos.iter().copied().find(|t| t.contem(indice)).filter(|t| turno_posto != Some(t.mensagem)) {
                turno_posto = Some(turno.mensagem);
                let aberto = self.expandidos.contains(&turno.chave(&chat.id));
                let fim = visiveis[i..].iter().position(|(k, _)| *k >= turno.resposta).map_or(visiveis.len(), |p| i + p);
                // O separador "Novo" de um item lá dentro fica antes da linha.
                if novo.is_some_and(|n| n < turno.resposta) {
                    novo = None;
                    blocos.push((Bloco::Dia, None, separador_novo()));
                }
                sem_entrada.insert(blocos.len());
                let elemento = self.linha_do_turno(&contexto, turno, aberto, &visiveis[i..fim], cx);
                blocos.push((Bloco::Log, Some(indice), elemento));
                if !aberto {
                    // Recolhido, só o que não pode sumir: a pergunta, o aviso de erro e o cartão.
                    for &(k, escondido) in &visiveis[i..fim] {
                        if fica_fora_do_recolhido(escondido) {
                            let (bloco, elemento) = self.item(&contexto, k, escondido, window, cx);
                            blocos.push((bloco, Some(k), elemento));
                        }
                    }
                    i = fim;
                    continue;
                }
            }
            if novo.is_some_and(|n| indice >= n) {
                novo = None;
                blocos.push((Bloco::Dia, None, separador_novo()));
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
            // Dois avisos do mesmo problema viram um cartão: o do Vitral (o login perdido sem outra
            // conta, todas as contas no limite) dá o título e a frase, e o erro do turno logo depois
            // dá o texto do agente. A dobra é na tela: vale para as conversas já gravadas e não
            // perde o texto cru (balão e cartão de erro, 16/11/2026).
            if let Item::Aviso { texto: antes, .. } = item
                && erros::precede_o_erro(antes)
                && let Some(&(indice_erro, Item::Aviso { texto, erro: true, .. })) = visiveis.get(i + 1)
                && texto.starts_with(erros::PREFIXO_DO_TURNO)
                && !datas.contains_key(&(i + 1))
            {
                let chave = format!("{}:{indice_erro}", chat.id);
                let elemento = self.cartao_de_erro(&contexto, &chave, indice_erro, texto, Some((indice, antes.as_str())), window, cx);
                blocos.push((Bloco::Alerta, Some(indice), elemento));
                i += 2;
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
        let mut lista = v_flex()
            .flex_none()
            .map(coluna)
            .when(minima(nivel), |s| s.px(esp::E12))
            .pt(esp::E12)
            .pb(esp::E12);
        let mut anterior = None;
        // Onde cada bloco ficou neste quadro (o índice do item, o alto e o pé, na tela): o
        // `on_prepaint` da lista, que vem depois dos blocos, os passa para a régua do painel.
        let medidos: Rc<std::cell::RefCell<Vec<(usize, Pixels, Pixels)>>> = Default::default();
        for (k, (bloco, indice, elemento)) in blocos.into_iter().enumerate() {
            let mut caixa = div().mt(espaco_antes(anterior, bloco)).child(elemento);
            if let Some(i) = indice {
                let medidos = medidos.clone();
                caixa = caixa.on_prepaint(move |limites, _, _| medidos.borrow_mut().push((i, limites.top(), limites.bottom())));
            }
            let entrada = if sem_entrada.contains(&k) { None } else { entrada_do_bloco(indice, base, vistos) };
            lista = match (indice, entrada) {
                (Some(i), Some(nova)) => lista.child(movimento::entrada(("entra", i), nova, De::Baixo, caixa)),
                _ => lista.child(caixa),
            };
            anterior = Some(bloco);
        }
        let janela = cx.entity().downgrade();
        let inicio_da_resposta = inicio_da_ultima_resposta(&chat.itens);
        let (chat_id, n_itens) = (chat.id.clone(), chat.itens.len());
        lista = lista.on_prepaint(move |bounds, window, cx| {
            let mut redesenhar = false;
            let _ = janela.update(cx, |j, cx| {
                let Some(p) = j.paineis.iter_mut().find(|p| p.id == painel_id) else { return; };
                p.altura_conteudo = bounds.size.height;
                {
                    let mut posicoes = p.posicoes.borrow_mut();
                    posicoes.clear();
                    posicoes.extend(medidos.borrow().iter().map(|&(i, alto, pe)| (i, alto - bounds.top(), pe - bounds.top())));
                }
                if let Some((altura, offset)) = p.preservar_topo.take() {
                    let novo = point(offset.x, deslocamento_apos_prepend(offset.y, altura, bounds.size.height));
                    p.rolagem.set_offset(novo);
                    p.leitura.deslocamento = novo.y;
                    redesenhar = true;
                }
                // A abertura do chat (rodada de uso, 2.2): no começo da resposta nova ou na âncora.
                if let Some(alvo) = p.alvo {
                    let vista = p.rolagem.bounds().size.height;
                    let decisao = onde_abrir(alvo, &p.posicoes.borrow(), bounds.size.height, vista, inicio_da_resposta, n_itens);
                    let mut lido = false;
                    match decisao {
                        Abertura::CarregarMais if p.mostrar < n_itens => p.mostrar += ITENS_NA_TELA,
                        Abertura::Em(y) => {
                            p.alvo = None;
                            let novo = point(px(0.), -y);
                            p.rolagem.set_offset(novo);
                            p.leitura.deslocamento = novo.y;
                            p.seguindo = false;
                        }
                        Abertura::Fim | Abertura::CarregarMais => {
                            p.alvo = None;
                            p.seguindo = true;
                            p.rolagem.scroll_to_bottom();
                            lido = true;
                        }
                    }
                    redesenhar = true;
                    // Coube na tela: o fim já está à vista, e o chat conta como lido.
                    if lido && let Some(motor) = j.motor.clone() {
                        motor.update(cx, |m, _| m.marcar_lido(&chat_id));
                    }
                }
            });
            if redesenhar {
                window.on_next_frame(move |_, cx| { let _ = janela.update(cx, |_, cx| cx.notify()); });
            }
        });

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
                    .relative()
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .overflow_y_scroll()
                    .lock_scroll_axis()
                    .track_scroll(&rolagem)
                    .on_scroll(cx.listener(move |this, _evento: &ScrollEvent, _, cx| {
                        // A roda já mexeu no deslocamento: decide agora, antes que o motor peça
                        // para voltar ao fim.
                        this.acompanhar_rolagem(cx);
                        this.carregar_anteriores(painel_id, cx);
                        cx.notify();
                    }))
                    .child(lista),
            )
            // Fora do fim, o pé da área rolável esmaece em 24 até o cartão: a última linha não
            // fica cortada a seco logo acima da faixa ou da caixa. No topo, nada: ali o cartão é
            // translúcido e o dono tirou a linha do cabeçalho (12/11/2026) (relatório de design,
            // 16/11/2026).
            .when(!seguindo, |s| {
                s.child(div().absolute().left_0().right_0().bottom_0().h(esp::E24).bg(crate::tema::gradiente_do_pe_do_cartao()))
            })
            .child(div().absolute().inset_0().child(
                Scrollbar::vertical(&rolagem).id(("barra-conversa", painel_id as usize))
                    .mode(ScrollbarMode::Always).viewport_from_layout(),
            ))
            .when(!seguindo, |s| s.child(pilula_do_fim(painel_id, novidade, minima(nivel), pilula_nova, cx)))
            // A barra do Ctrl+F, por cima das mensagens (montada em `conversa`).
            .children(barra_busca)
    }

    fn carregar_anteriores(&mut self, painel_id: u64, cx: &mut Context<Self>) {
        let Some(ix) = self.paineis.iter().position(|p| p.id == painel_id) else { return; };
        let p = &self.paineis[ix];
        if p.rolagem.offset().y < -px(240.) || p.preservar_topo.is_some() || p.busca.aberta { return; }
        let nativos = self.motor.as_ref().and_then(|m| m.read(cx).chat(p.chat.as_deref()?)).map_or(0, |chat| {
            chat.itens.iter().filter(|item| !matches!(item, Item::Ferramenta { nome, .. } if nome == "ToolSearch" || p.modo_foco)).count()
        });
        let tem_mais = if p.mostrar < nativos {
            self.paineis[ix].mostrar += ITENS_NA_TELA;
            true
        } else { self.mais_historico(ix) };
        if tem_mais {
            let p = &mut self.paineis[ix];
            p.seguindo = false;
            p.preservar_topo = Some((p.altura_conteudo, p.rolagem.offset()));
            cx.notify();
        }
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
                let editavel = contexto.editavel.is_some_and(|desde| indice >= desde);
                (Bloco::Balao, self.balao_usuario(contexto, &chave, texto, anexos, reforco, item.em(), realce, editavel, window, cx))
            }
            Item::Assistente { texto, .. } => {
                (Bloco::Texto, self.texto_assistente(contexto, indice, &chave, texto, item.em(), cx))
            }
            Item::Entrega { de, texto, .. } => {
                (Bloco::Recebida, self.mensagem_recebida(contexto, indice, &chave, de, texto, item.em(), window, cx))
            }
            Item::Ferramenta { .. } | Item::Raciocinio { .. } => (Bloco::Log, self.trilha(contexto, &[(indice, item)], false, cx)),
            Item::Aviso { texto, erro, .. } if tipo_aviso(texto, *erro) == ClasseAvisoChat::Alerta => {
                (Bloco::Alerta, self.cartao_de_erro(contexto, &chave, indice, texto, None, window, cx))
            }
            Item::Aviso { texto, erro, .. } => {
                let realce = contexto.busca.realce(indice, Campo::Texto);
                let (bloco, elemento) = aviso_do_chat(texto, *erro, &realce);
                let rolar = RolarAoAchado::novo(format!("rolar-{chave}").into(), &realce, contexto.rolagem, elemento);
                (bloco, rolar.into_any_element())
            }
            Item::Cartao { id, .. } => aviso_do_chat(&format!("Cartão #{id}"), false, &Realce::default()),
            Item::Vez { agente, .. } => (Bloco::Marco, marca_da_vez(*agente)),
            Item::Desconhecido(_) => {
                aviso_do_chat("(mensagem de uma versão mais nova)", false, &Realce::default())
            }
        }
    }

    /// O que você escreveu: balão azul baixo, na largura toda da coluna, com a ponta no canto de
    /// baixo à direita; as ações flutuam no canto de cima à direita, só no hover.
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
        let grupo = SharedString::from(format!("msg-{chave}"));
        let linhas = texto.lines().count();
        let longo = linhas > 14;
        let chave_tudo = format!("{chave}:tudo");
        let tudo = self.expandidos.contains(&chave_tudo);
        let corpo = match comando_de_barra(texto).filter(|_| anexos.is_empty()) {
            // O comando sozinho ("/compact") é uma pílula no material do balão: a barra preta na
            // largura toda não parecia mensagem sua (balão e cartão de erro, 16/11/2026).
            Some(comando) => h_flex()
                .w_full()
                .child(
                    div()
                        .flex_none()
                        .h(r(32.))
                        .px(dentro_da_borda(12.))
                        .flex()
                        .items_center()
                        .rounded(raio::de(32.))
                        .bg(Cores::balao())
                        .border_1()
                        .border_color(Cores::balao_borda())
                        .mono()
                        .text_color(Cores::codigo())
                        .child(comando.to_string()),
                )
                .into_any_element(),
            None => {
                let mostrar = longo.then(|| {
                    let rotulo = if tudo {
                        "Mostrar menos".to_string()
                    } else {
                        format!("Mostrar tudo ({linhas} linhas)")
                    };
                    let id = SharedString::from(format!("tudo-{chave}"));
                    let icone = if tudo { Lucide::ChevronUp } else { Lucide::ChevronDown };
                    fantasma(id, Some(icone), rotulo, TamanhoBotao::Linha, alternar(chave_tudo, cx), window, cx)
                });
                // Os anexos entram no balão, em pílulas em cima do texto, como na extensão do
                // Claude Code (dono, 16/11/2026), no poço escuro do balão.
                let pilulas = (!anexos.is_empty()).then(|| {
                    h_flex().flex_wrap().gap(esp::E6).children(
                        anexos.iter().enumerate().map(|(i, a)| pilula_de_arquivo(format!("anexo-{chave}-{i}"), a, &contexto.chat.id, true, cx)),
                    )
                });
                let com_texto = !texto.trim().is_empty();
                let folga = if minima(contexto.nivel) { 12. } else { 16. };
                // O balão é azul bem baixo (o destaque misturado à base, opaco): diz de quem é a
                // fala sem a moldura de cartão, e o canto de baixo à direita é o da ponta.
                // Na largura toda da coluna (dono, 16/11/2026: "faz o campo das minhas mensagens
                // ficar na largura toda"). Raio 16, o das peças: com 20 numa faixa de 46 a
                // mensagem de uma linha virava pílula (balão e cartão de erro, 16/11/2026).
                let balao = (com_texto || pilulas.is_some() || mostrar.is_some() || reforco != Reforco::default()).then(|| {
                    v_flex()
                        .w_full()
                        .px(dentro_da_borda(folga))
                        .py(dentro_da_borda(12.))
                        .gap(esp::E8)
                        .rounded(crate::tema::raio_da_peca())
                        .rounded_br(raio::CONTROLE)
                        .bg(Cores::balao())
                        .border_1()
                        .border_color(Cores::balao_borda())
                        .children(pilulas)
                        .when(com_texto, |s| {
                            // A fala um degrau abaixo da resposta (14/22 no texto, não no branco),
                            // e os caminhos que existem viram link, como na resposta. O Copiar e o
                            // Editar levam o texto original.
                            let cwd = &contexto.chat.cwd;
                            let ligado = super::links_arquivo::ligar_arquivos(texto, &mut |c| super::links_arquivo::resolver(cwd, c));
                            let cortado = longo && !tudo;
                            s.child(
                                div()
                                    .relative()
                                    .fala()
                                    .text_color(Cores::texto())
                                    .child(
                                        markdown_achado(contexto, chave.to_string().into(), com_quebras_duras(&ligado), realce)
                                            .estilo(estilo_do_balao)
                                            .linhas(cortado.then_some(12)),
                                    )
                                    .when(cortado, |s| {
                                        s.child(
                                            div()
                                                .absolute()
                                                .left_0()
                                                .right_0()
                                                .bottom_0()
                                                .h(r(24.))
                                                .bg(crate::tema::gradiente_do_pe_do_balao()),
                                        )
                                    }),
                            )
                        })
                        // O chevron cai na coluna do texto.
                        .when_some(mostrar, |s, b| s.child(h_flex().ml(r(-8.)).child(b)))
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
                v_flex().w_full().children(balao).into_any_element()
            }
        };
        let mut acoes: Vec<AnyElement> = Vec::new();
        acoes.extend(hora_na_barra(em));
        // Em qualquer mensagem sua: o texto volta para a caixa e a conversa volta a antes dela.
        if editavel {
            let ix = chave.rsplit(':').next().and_then(|i| i.parse().ok());
            if let Some(ix) = ix {
                acoes.push(super::reenvio::botao_editar(chave, contexto.painel, ix, window, cx).into_any_element());
            }
        }
        acoes.push(copiar(chave, texto).into_any_element());
        // O balão ocupa a coluna inteira: a barra de ações flutua no canto de cima, no trecho reto
        // da borda (a 16, na coluna do texto); no canto ela montava na curva.
        let barra = div().absolute().top(r(-14.)).right(if minima(contexto.nivel) { esp::E12 } else { esp::E16 });
        v_flex()
            .w_full()
            .map(com_alcance_da_barra)
            .group(grupo.clone())
            .child(div().relative().w_full().child(corpo).child(barra.child(barra_de_acoes(grupo, acoes, cx))))
            .into_any_element()
    }

    /// O erro de um turno: o cartão com o título em português, o que fazer, o Copiar no canto, as
    /// ações (só no erro vivo, com o turno parado) e o texto do agente atrás de "Detalhes" (aberto
    /// no erro que o Vitral não conhece). `antes` é o aviso do Vitral dobrado nele (o mesmo
    /// problema dito duas vezes), com o índice dele, para a busca rolar até o cartão.
    #[allow(clippy::too_many_arguments)]
    fn cartao_de_erro(
        &self,
        contexto: &Contexto,
        chave: &str,
        indice: usize,
        texto: &str,
        antes: Option<(usize, &str)>,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let chat = contexto.chat;
        let lido = erros::ler_erro(texto, chat.agente, antes.map(|(_, t)| t));
        let (icone, cor) = erros::aparencia(lido.tipo);
        let realce = contexto.busca.realce(indice, Campo::Texto);
        let id = |nome: &str| SharedString::from(format!("erro-{chave}-{nome}"));
        let mut cartao = CartaoDeErro::new(icone, cor, lido.titulo.clone())
            .canto(BotaoCopiar::new(id("copiar"), lido.para_copiar.clone()).dica("Copiar o erro"));
        if let Some(frase) = lido.frase.clone() {
            cartao = if lido.cru.is_none() && !realce.vazio() {
                cartao.frase(busca_conversa::realcar(frase, &realce))
            } else {
                cartao.frase(frase)
            };
        }
        // As ações só no erro vivo e com o turno parado: no que já ficou para trás sobra o
        // "Detalhes". No máximo duas, e nunca o "Tentar de novo", que mora na faixa.
        if erros::erro_vivo(&chat.itens, indice) && !contexto.rodando {
            for acao in self.acoes_do_erro(contexto, &lido, chave, window, cx) {
                cartao = cartao.acao(acao);
            }
        }
        if let Some(cru) = &lido.cru {
            let chave_detalhes = format!("{chave}:detalhes");
            let chave_tudo = format!("{chave_detalhes}:tudo");
            // A busca abre sozinha, como a trilha.
            let aberto = !lido.recolhido || self.expandidos.contains(&chave_detalhes) || !realce.vazio();
            if lido.recolhido {
                let icone = if aberto { Lucide::ChevronDown } else { Lucide::ChevronRight };
                cartao = cartao.acao(fantasma(id("detalhes"), Some(icone), "Detalhes", TamanhoBotao::Linha, alternar(chave_detalhes, cx), window, cx));
            }
            if aberto {
                let tudo = self.expandidos.contains(&chave_tudo);
                let longo = cru.lines().count() > 8 || cru.chars().count() > 640;
                let cortar = longo && !tudo;
                let mostrar = longo.then(|| {
                    let (icone, rotulo) = if tudo { (Lucide::ChevronUp, "Mostrar menos") } else { (Lucide::ChevronDown, "Mostrar tudo") };
                    h_flex().ml(r(-8.)).child(fantasma(id("tudo"), Some(icone), rotulo, TamanhoBotao::Linha, alternar(chave_tudo, cx), window, cx))
                });
                // 14 do cartão menos 8: o mínimo, 10.
                let original = v_flex()
                    .w_full()
                    .gap(esp::E4)
                    .bg(Cores::recuo())
                    .rounded(raio::CONTROLE)
                    .px(esp::E8)
                    .py(esp::E6)
                    .mono_pequeno()
                    .text_color(Cores::tenue())
                    .child(
                        TextoAchado::novo(format!("{chave}:original").into(), erros::erro_em_markdown(cru), realce.clone())
                            .rolagem(contexto.rolagem)
                            .estilo(estilo_do_erro)
                            .linhas(cortar.then_some(8)),
                    )
                    .children(mostrar);
                cartao = cartao.original(original, !lido.recolhido);
            }
        }
        let elemento = RolarAoAchado::novo(format!("rolar-{chave}").into(), &realce, contexto.rolagem, cartao.into_any_element()).into_any_element();
        // Um achado só no aviso dobrado também rola até o cartão (sem marcar a palavra).
        match antes {
            Some((i, _)) => {
                let realce_antes = contexto.busca.realce(i, Campo::Texto);
                RolarAoAchado::novo(format!("rolar-{}:{i}", chat.id).into(), &realce_antes, contexto.rolagem, elemento).into_any_element()
            }
            None => elemento,
        }
    }

    /// As ações do erro vivo, pela tabela da especificação (balão e cartão de erro, 16/11/2026).
    fn acoes_do_erro(&self, contexto: &Contexto, lido: &erros::ErroLido, chave: &str, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        use erros::TipoDeErro;
        let chat = contexto.chat;
        let id = |nome: &str| SharedString::from(format!("erro-{chave}-{nome}"));
        let mut acoes: Vec<AnyElement> = Vec::new();
        // Continuar no outro agente: troca quem responde e repete o turno (a passagem
        // `TrocouDeAgente` já pula a mensagem repetida). Na dupla, o outro já está no chat: a vez
        // passa a ele, que recebe o que este fez, em vez de trocar o agente do chat (coordenação,
        // 16/11/2026).
        let outro = chat.agente.outro();
        let pode_continuar = lido.agente == chat.agente
            && contexto.motor.pode_tentar_de_novo(&chat.id)
            && (outro != Agente::Codex || crate::agente_codex::instalado());
        let continuar = |acoes: &mut Vec<AnyElement>| {
            if !pode_continuar {
                return;
            }
            let (motor_e, id_chat, dupla) = (self.motor.clone(), chat.id.clone(), chat.parceiro.is_some());
            let rotulo = format!("Continuar no {}", outro.nome());
            acoes.push(
                fantasma(
                    id("continuar"),
                    Some(Lucide::Shuffle),
                    rotulo,
                    TamanhoBotao::Linha,
                    move |_, _, cx| {
                        let Some(m) = &motor_e else { return };
                        m.update(cx, |m, cx| {
                            if dupla {
                                m.mudar_chat(&id_chat, |c| { c.trocar_vez(); }, cx);
                                m.tentar_de_novo(&id_chat, cx);
                            } else if m.definir_agente(&id_chat, outro, None, cx) {
                                m.tentar_de_novo(&id_chat, cx);
                            }
                        });
                    },
                    window,
                    cx,
                )
                .into_any_element(),
            );
        };
        let contas = |rotulo: &str, icone: Lucide, nome: &str| {
            fantasma(id(nome), Some(icone), rotulo.to_string(), TamanhoBotao::Linha, cx.listener(|this, _, window, cx| this.dialogo_contas(window, cx)), window, cx)
                .into_any_element()
        };
        let instalar = |rotulo: &str, icone: Lucide, nome: &str| {
            fantasma(id(nome), Some(icone), rotulo.to_string(), TamanhoBotao::Linha, |_, _, _| super::uso_plano::instalar_codex(), window, cx)
                .into_any_element()
        };
        match (lido.tipo, lido.agente) {
            (TipoDeErro::Limite, Agente::Codex) => {
                if let Some(url) = lido.url_de_uso.clone() {
                    acoes.push(
                        fantasma(id("uso"), Some(Lucide::ExternalLink), "Ver o uso", TamanhoBotao::Linha, move |_, _, cx| cx.open_url(&url), window, cx)
                            .into_any_element(),
                    );
                }
                continuar(&mut acoes);
            }
            (TipoDeErro::Limite, Agente::Claude) => {
                acoes.push(contas("Contas do Claude", Lucide::CircleUserRound, "contas"));
                continuar(&mut acoes);
            }
            // As Contas já no login da conta que caiu, e não na lista (rodada de uso, item 8.3).
            (TipoDeErro::Login, Agente::Claude) => {
                let m = contexto.motor;
                let conta = m
                    .conta_do_ultimo_turno(&chat.id)
                    .filter(|c| m.conta_sem_login(&c.id))
                    .or_else(|| m.contas.iter().find(|c| m.conta_sem_login(&c.id)))
                    .map(|c| c.id.clone());
                acoes.push(
                    fantasma(
                        id("entrar"),
                        Some(Lucide::KeyRound),
                        "Entrar de novo",
                        TamanhoBotao::Linha,
                        cx.listener(move |this, _, window, cx| this.entrar_de_novo_na_conta(conta.clone(), window, cx)),
                        window,
                        cx,
                    )
                    .into_any_element(),
                );
            }
            (TipoDeErro::Login, Agente::Codex) => acoes.push(instalar("Entrar no Codex", Lucide::KeyRound, "entrar")),
            (TipoDeErro::SemCodex, _) => acoes.push(instalar("Instalar o Codex", Lucide::Download, "instalar")),
            (TipoDeErro::Contexto, Agente::Claude) => {
                let painel = contexto.painel;
                acoes.push(
                    fantasma(
                        id("compactar"),
                        Some(Lucide::Minimize),
                        "Compactar",
                        TamanhoBotao::Linha,
                        cx.listener(move |this, _, window, cx| this.por_na_caixa(painel, "/compact ", window, cx)),
                        window,
                        cx,
                    )
                    .into_any_element(),
                );
            }
            _ => {}
        }
        acoes
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
        // Uma citação em bloco ("> ...") ganha o Copiar dela, como o bloco de código (dono,
        // 16/11/2026: "esse tipo de mensagem aqui tem que ter botão pra copiar": o agente escreve
        // numa citação a mensagem pronta para mandar a outra pessoa). O kit não tem gancho para
        // as ações da citação, então a resposta com citação é partida em trechos, e cada citação
        // é desenhada à parte, com o mesmo estilo e o botão no canto. A busca conta as ocorrências
        // trecho a trecho, na ordem do texto. Sem citação, a resposta é um texto só, como antes.
        let trechos = trechos_com_citacoes(&ligado);
        let corpo = if trechos.iter().any(|(citacao, _)| *citacao) {
            let mut antes = 0;
            v_flex()
                .gap(esp::E12)
                .children(trechos.into_iter().enumerate().map(|(i, (citacao, trecho))| {
                    let n = if realce.vazio() { 0 } else { super::busca_conversa::faixas(&trecho, &realce.termo).len() };
                    let realce_do_trecho = realce.trecho(antes, n);
                    antes += n;
                    // O primeiro trecho guarda a chave do texto inteiro (a do texto que chegava).
                    let chave_do_trecho = if i == 0 { chave.to_string() } else { format!("{chave}:trecho{i}") };
                    let copiar_citacao = citacao.then(|| texto_da_citacao(&trecho));
                    let texto = div().corpo().text_color(Cores::texto()).child(
                        markdown_achado(contexto, chave_do_trecho.clone().into(), trecho, realce_do_trecho).estilo(estilo_da_resposta),
                    );
                    match copiar_citacao {
                        Some(copiar) => div()
                            .relative()
                            .pr(esp::E32)
                            .child(texto)
                            .child(div().absolute().top_0().right_0().child(
                                BotaoCopiar::new(SharedString::from(format!("copiar-{chave_do_trecho}")), copiar).dica("Copiar a citação"),
                            ))
                            .into_any_element(),
                        None => texto.into_any_element(),
                    }
                }))
                .into_any_element()
        } else {
            div()
                .corpo()
                .text_color(Cores::texto())
                .child(markdown_achado(contexto, chave.to_string().into(), ligado, realce).estilo(estilo_da_resposta))
                .into_any_element()
        };
        let resposta = v_flex()
            .relative()
            .w_full()
            .gap(esp::E8)
            .child(corpo)
            .when(!apontadas.is_empty(), |s| {
                s.child(h_flex().flex_wrap().gap(esp::E6).children(apontadas.iter().enumerate().map(|(i, caminho)| {
                    pilula_de_arquivo(format!("resposta-{chave}-{i}"), caminho, &chat_id, false, cx)
                })))
            })
            // A barra fica no pé da resposta, onde o olho termina: em cima ela cobria metade da
            // primeira linha, que quase sempre vai até a borda (relatório de aparência,
            // 16/11/2026, V7). O seu balão continua com ela em cima.
            .child(div().absolute().bottom(r(-8.)).right_0().child(barra_de_acoes(
                grupo.clone(),
                hora_na_barra(em).into_iter().chain([copiar(chave, texto).into_any_element()]).collect(),
                cx,
            )));
        div().w_full().map(com_alcance_da_barra_embaixo).group(grupo).child(resposta).into_any_element()
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
        // A citação na cor do texto: é onde o agente põe a mensagem pronta para mandar, e no
        // tênue era o texto longo menos legível da resposta. O fio e o recuo marcam a citação
        // (relatório de aparência, 16/11/2026, V13).
        .blockquote_style(
            StyleRefinement::default()
                .border_l_2()
                .border_color(Cores::borda_forte())
                .pl(esp::E12)
                .text_color(Cores::texto()),
        )
        .selectable(true)
        .code_block_actions(move |bloco, _, _| {
            let dica = match bloco.lang().filter(|l| !l.is_empty()) {
                Some(l) => format!("Copiar código {l}"),
                None => "Copiar código".to_string(),
            };
            BotaoCopiar::new("copiar", bloco.code()).dica(dica)
        })
}

/// A resposta do índice `i` é o próprio erro com que o turno terminou: o Claude Code manda o erro
/// no `result` e às vezes também como mensagem, e a conversa mostrava o texto duas vezes, como
/// resposta e no cartão do erro logo abaixo (dono, 16/11/2026). Ela fica gravada; só não é
/// desenhada (nem contada pela busca), e fica o cartão.
pub(super) fn resposta_repete_o_erro(itens: &[Item], i: usize) -> bool {
    let Some(Item::Assistente { texto, .. }) = itens.get(i) else {
        return false;
    };
    let texto = texto.trim();
    if texto.is_empty() {
        return false;
    }
    itens[i + 1..]
        .iter()
        .take_while(|item| !matches!(item, Item::Usuario { .. }))
        .any(|item| match item {
            Item::Aviso { texto: aviso, erro: true, .. } => {
                aviso.strip_prefix("O turno terminou com erro:").unwrap_or(aviso).trim() == texto
            }
            _ => false,
        })
}

/// O estilo da resposta do agente: o das mensagens, com os links de arquivo abrindo no Vitral.
fn estilo_da_resposta(vista: TextView, cx: &App) -> TextView {
    estilo_das_mensagens(vista, cx).on_link_click(|url, _, window, cx| {
        if url.starts_with("vitral:") {
            if super::links_arquivo::link_permitido(url, true) {
                window.dispatch_action(Box::new(super::links_arquivo::AbrirLinkVitral(url.to_string())), cx);
            }
        } else {
            cx.open_url(url);
        }
    })
}

/// O estilo do seu balão: o da resposta (links de arquivo abrem no Vitral), com títulos que nunca
/// passam da fala e o código no poço escuro, que se vê no azul.
fn estilo_do_balao(vista: TextView, cx: &App) -> TextView {
    estilo_da_resposta(vista, cx).style(
        estilo_com_titulos(cx, [15., 14., 14.])
            .code_block(
                StyleRefinement::default()
                    .p(dentro_da_borda(12.))
                    .pr(COLUNA_DO_COPIAR)
                    .bg(Cores::preto())
                    .border_color(Cores::borda())
                    // 16 de fora menos 16 de padding: o mínimo, 10.
                    .rounded(raio::CONTROLE),
            )
            .inline_code(HighlightStyle {
                color: Some(Cores::codigo()),
                // O branco de 8% sumia no azul.
                background_color: Some(Cores::recuo()),
                ..Default::default()
            })
            .table(StyleRefinement::default().rounded(raio::CONTROLE).border_color(Cores::borda())),
    )
}

/// O texto cru de um erro: parágrafos juntos e só link da web, aberto no navegador.
fn estilo_do_erro(vista: TextView, cx: &App) -> TextView {
    estilo_das_mensagens(vista, cx).style(estilo_texto(cx).paragraph_gap(esp::E4)).on_link_click(|url, _, _, cx| {
        if super::links_arquivo::link_permitido(url, false) {
            cx.open_url(url);
        }
    })
}

/// A resposta em trechos, na ordem: `true` nas citações em bloco (linhas que começam com ">",
/// fora dos blocos de código), `false` no resto. As linhas em branco das pontas saem.
pub(super) fn trechos_com_citacoes(texto: &str) -> Vec<(bool, String)> {
    let mut saida: Vec<(bool, String)> = Vec::new();
    let mut cerca: Option<&str> = None;
    for linha in texto.split('\n') {
        let aparada = linha.trim_start();
        let recuo = linha.len() - aparada.len();
        let marca = ["```", "~~~"].into_iter().find(|m| aparada.starts_with(m));
        let citacao = cerca.is_none() && marca.is_none() && recuo <= 3 && aparada.starts_with('>');
        match (cerca, marca) {
            (None, Some(m)) => cerca = Some(m),
            (Some(aberta), Some(m)) if aberta == m => cerca = None,
            _ => {}
        }
        match saida.last_mut() {
            Some((c, t)) if *c == citacao => {
                t.push('\n');
                t.push_str(linha);
            }
            _ => saida.push((citacao, linha.to_string())),
        }
    }
    saida
        .into_iter()
        .filter_map(|(c, t)| {
            let t = t.trim_matches('\n').to_string();
            (!t.trim().is_empty()).then_some((c, t))
        })
        .collect()
}

/// O que o Copiar de uma citação leva: o texto sem os ">" e sem as marcas de markdown que não
/// fazem sentido colar numa conversa (negrito, itálico, código, título; o link vira "texto
/// (endereço)"), com as linhas como estão e um parágrafo em branco entre os parágrafos.
pub(super) fn texto_da_citacao(trecho: &str) -> String {
    let mut saida = String::new();
    let mut paragrafo_novo = false;
    for linha in trecho.lines() {
        let linha = linha.trim_start();
        let linha = linha.strip_prefix('>').unwrap_or(linha);
        let linha = linha.strip_prefix(' ').unwrap_or(linha).trim_end();
        if linha.trim().is_empty() {
            paragrafo_novo = !saida.is_empty();
            continue;
        }
        if paragrafo_novo {
            saida.push_str("\n\n");
            paragrafo_novo = false;
        } else if !saida.is_empty() {
            saida.push('\n');
        }
        saida.push_str(&sem_marcas_de_markdown(linha));
    }
    saida
}

/// Uma linha sem as marcas de markdown: o título ("## "), o código (`x`, que fica como está por
/// dentro), os links e a ênfase (**, __, * e _ só quando abrem ou fecham uma palavra, para não
/// mexer em "a*b" nem em nome_com_sublinhado).
fn sem_marcas_de_markdown(linha: &str) -> String {
    let semtitulo = linha.trim_start_matches('#');
    let linha = if semtitulo.len() < linha.len() && semtitulo.starts_with(' ') { semtitulo.trim_start() } else { linha };
    let mut saida = String::new();
    for (i, pedaco) in linha.split('`').enumerate() {
        // Os pedaços ímpares estavam entre crases: código, que vai como está.
        if i % 2 == 1 {
            saida.push_str(pedaco);
        } else {
            saida.push_str(&sem_enfase(&links_em_texto(pedaco)));
        }
    }
    saida
}

/// "[texto](endereço)" vira "texto (endereço)"; o link de arquivo do Vitral vira só o texto.
fn links_em_texto(texto: &str) -> String {
    let mut saida = String::new();
    let mut resto = texto;
    while let Some(abre) = resto.find('[') {
        let depois = &resto[abre + 1..];
        let Some(fecha) = depois.find("](") else { break };
        let Some(fim) = depois[fecha + 2..].find(')') else { break };
        let (rotulo, endereco) = (&depois[..fecha], &depois[fecha + 2..fecha + 2 + fim]);
        saida.push_str(&resto[..abre]);
        if endereco.starts_with("vitral:") || endereco == rotulo || endereco.is_empty() {
            saida.push_str(rotulo);
        } else {
            saida.push_str(&format!("{rotulo} ({endereco})"));
        }
        resto = &depois[fecha + 2 + fim + 1..];
    }
    saida.push_str(resto);
    saida
}

/// Tira as marcas de ênfase que abrem ou fecham uma palavra.
fn sem_enfase(texto: &str) -> String {
    let letras: Vec<char> = texto.chars().collect();
    let mut saida = String::with_capacity(texto.len());
    let mut i = 0;
    while i < letras.len() {
        let c = letras[i];
        if c == '*' || c == '_' {
            let tam = if letras.get(i + 1) == Some(&c) { 2 } else { 1 };
            let antes = i.checked_sub(1).map(|j| letras[j]);
            let depois = letras.get(i + tam).copied();
            let borda = |x: Option<char>| x.is_none_or(|x| !x.is_alphanumeric());
            let abre = borda(antes) && depois.is_some_and(|d| !d.is_whitespace());
            let fecha = antes.is_some_and(|a| !a.is_whitespace()) && borda(depois);
            if abre || fecha {
                i += tam;
                continue;
            }
        }
        saida.push(c);
        i += 1;
    }
    saida
}

/// O texto do seu balão com as quebras de linha que você digitou: no markdown, a quebra simples
/// vira espaço, e o balão juntava "linha um" e "linha dois" num parágrafo só (log colado,
/// endereço, lista sem marcador). Cada linha de texto seguida de outra ganha a quebra dura (dois
/// espaços no fim), menos dentro de blocos cercados (``` e ~~~), em linhas de tabela e antes de
/// linha em branco (teste de usabilidade, 16/11/2026). O Copiar continua com o texto original.
pub(super) fn com_quebras_duras(texto: &str) -> String {
    let linhas: Vec<&str> = texto.split('\n').collect();
    let mut saida = String::with_capacity(texto.len() + linhas.len() * 2);
    let mut cerca: Option<&str> = None;
    for (i, linha) in linhas.iter().enumerate() {
        let aparada = linha.trim_start();
        let marca = ["```", "~~~"].into_iter().find(|m| aparada.starts_with(m));
        let dentro = cerca.is_some();
        match (cerca, marca) {
            (None, Some(m)) => cerca = Some(m),
            (Some(aberta), Some(m)) if aberta == m => cerca = None,
            _ => {}
        }
        saida.push_str(linha);
        let seguinte = linhas.get(i + 1).map(|l| l.trim());
        let tabela = |l: &str| l.trim_start().starts_with('|');
        let quebra = !dentro
            && marca.is_none()
            && !linha.trim().is_empty()
            && !tabela(linha)
            && seguinte.is_some_and(|s| !s.is_empty() && !s.starts_with('|') && !s.starts_with("```") && !s.starts_with("~~~"));
        if quebra && !linha.ends_with("  ") {
            saida.push_str("  ");
        }
        if i + 1 < linhas.len() {
            saida.push('\n');
        }
    }
    saida
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
/// O hover da mensagem alcança a barra de ações inteira, que fica 14 acima dela (metade para
/// fora): 16 de folga em cima, devolvidos pela margem. Sem isso, chegando por cima, a barra sumia
/// antes do clique no lápis (teste de usabilidade, 16/11/2026).
fn com_alcance_da_barra<E: Styled>(elemento: E) -> E {
    elemento.pt(esp::E16).mt(r(-16.))
}

/// O mesmo para a barra no pé da resposta, que sai 8 abaixo dela: 8 de folga embaixo, devolvidos
/// pela margem.
fn com_alcance_da_barra_embaixo<E: Styled>(elemento: E) -> E {
    elemento.pb(esp::E8).mb(r(-8.))
}

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
pub(super) fn separadores_de_data(horas: &[u64], agora: u64) -> Vec<(usize, String)> {
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
/// Num chat com o Claude e o Codex juntos, quem trabalha daqui em diante: a marca e o nome, à
/// esquerda, na coluna do texto da resposta.
fn marca_da_vez(agente: Agente) -> AnyElement {
    let marca = match agente {
        Agente::Claude => Marca::Claude,
        Agente::Codex => Marca::Codex,
    };
    h_flex()
        .h(r(24.))
        .gap(r(8.))
        .child(marca.disco(18.))
        .child(div().metadado().font_semibold().text_color(Cores::forte()).child(agente.nome()))
        .into_any_element()
}

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

/// O separador "Novo": o desenho do separador de dia, em azul, antes do primeiro item que você não
/// viu (rodada de uso, 2.2).
fn separador_novo() -> AnyElement {
    let traco = || div().flex_1().min_w(r(16.)).h(px(1.)).bg(Cores::destaque());
    h_flex()
        .w_full()
        .min_h(r(24.))
        .gap(r(8.))
        .child(traco())
        .child(div().flex_none().metadado().font_semibold().text_color(Cores::destaque_texto()).child("Novo"))
        .child(traco())
        .into_any_element()
}

fn copiar(chave: &str, texto: &str) -> BotaoCopiar {
    BotaoCopiar::new(SharedString::from(format!("copiar-{chave}")), texto.to_string())
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
        if linha.starts_with("+++") || linha.starts_with("---") { continue; }
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

/// Quantos itens entram por página: começa pelos últimos e acrescenta os anteriores ao subir.
/// A conversa não é virtualizada: cada redesenho da janela monta e mede todos
/// os blocos de todos os painéis, e isso custa a cada passo da roda, a cada movimento do
/// ponteiro e a cada letra. Medido em 13/11/2026 no binário otimizado, com duas conversas de 400
/// itens lado a lado: 6,2 s de processador para 80 passos de rolagem (78 ms por passo).
pub(super) const ITENS_NA_TELA: usize = 60;

/// Quantas miniaturas a pilha do grupo recolhido mostra (abrir o grupo mostra todas, cada uma na
/// linha da chamada dela) e quantos nomes vão na dica dela.
const IMAGENS_NA_PILHA: usize = 4;
const NOMES_NA_DICA: usize = 10;

/// A linha das imagens de um grupo recolhido: a pilha de miniaturas quadradas de 22, sobrepostas
/// 6 com a borda do fundo entre elas (como avatares empilhados), e "9 imagens". O clique abre o
/// visor na primeira; a dica tem os nomes.
/// A dica da pilha: os nomes, até `NOMES_NA_DICA`, e "e mais N".
fn dica_da_pilha(nomes: &[String]) -> String {
    let mut linhas: Vec<String> = nomes.iter().take(NOMES_NA_DICA).cloned().collect();
    if nomes.len() > NOMES_NA_DICA {
        linhas.push(format!("e mais {}", nomes.len() - NOMES_NA_DICA));
    }
    linhas.join("\n")
}

fn pilha_de_imagens(chat: &str, lista: &[(usize, PathBuf)], cx: &Context<Janela>) -> AnyElement {
    let n = lista.len();
    let nomes: Vec<String> = lista.iter().map(|(_, c)| imagens::nome_do_arquivo(c)).collect();
    let dica = SharedString::from(dica_da_pilha(&nomes));
    let (primeiro, primeira) = lista[0].clone();
    let chat_id = chat.to_string();
    h_flex().pl(RECUO_DO_LOG_SOLTO).py(esp::E4).child(
        h_flex()
            .id(SharedString::from(format!("pilha-{chat}:{primeiro}")))
            .flex_none()
            .h(r(28.))
            .pl(esp::E4)
            .pr(esp::E8)
            .gap(esp::E8)
            .rounded(raio::de(28.))
            .cursor_pointer()
            .hover(|s| s.bg(Cores::hover()))
            .child(h_flex().flex_none().children(lista.iter().take(IMAGENS_NA_PILHA).enumerate().map(|(i, (_, caminho))| {
                div()
                    .flex_none()
                    .size(r(22.))
                    .when(i > 0, |s| s.ml(r(-6.)))
                    .rounded(raio::CONTROLE)
                    .border_2()
                    .border_color(Cores::cartao_opaco())
                    .bg(Cores::preto())
                    .overflow_hidden()
                    .child(img(caminho.clone()).size_full().object_fit(ObjectFit::Cover))
            })))
            .child(div().flex_none().metadado().text_color(Cores::tenue()).child(contagem(n, "imagem", "imagens")))
            .on_click(cx.listener(move |this, _, window, cx| this.abrir_visor(&chat_id, &primeira, window, cx)))
            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx)),
    )
    .into_any_element()
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

/// Um arquivo na conversa, como a extensão do Claude Code mostra (dono, 16/11/2026: "faz as
/// imagens que mando e a que você usa ficar igual fica aqui"): uma pílula com a miniatura (ou o
/// ícone), o nome e as medidas da imagem (ou o tamanho do arquivo). Serve aos anexos das suas
/// mensagens, às imagens que a resposta aponta e às que uma chamada leu ou gravou; o clique numa
/// imagem abre o visor. O que saiu do disco fica apagado, com "sumiu". Dentro do seu balão
/// (`no_balao`), a pílula vai no poço escuro, da família do código do balão: o cinza opaco parecia
/// colado de fora no azul (balão e cartão de erro, 16/11/2026).
fn pilula_de_arquivo(id: String, caminho: &Path, chat: &str, no_balao: bool, cx: &Context<Janela>) -> AnyElement {
    let lido = imagens::anexo(caminho);
    let nome = imagens::nome_do_arquivo(caminho);
    let detalhe = lido.detalhe();
    let imagem = crate::claude::tipo_imagem(caminho).is_some();
    let dica = SharedString::from(match (lido.existe, detalhe.is_empty()) {
        (false, _) => format!("Não está mais em {}", caminho.display()),
        (true, true) => format!("{nome}\n{}", caminho.display()),
        (true, false) => format!("{nome} · {detalhe}\n{}", caminho.display()),
    });
    let medida = match (lido.medidas, &lido.tamanho) {
        (Some((largura, altura)), _) => format!("{largura}×{altura}"),
        (None, Some(tamanho)) => tamanho.to_string(),
        (None, None) => "sumiu".into(),
    };
    let abre = imagem && lido.existe;
    let simbolo = if abre {
        let (largura, altura) = imagens::medida_da_miniatura(lido.medidas, imagens::ALTURA_CHIP, imagens::LARGURA_MAXIMA_CHIP);
        div()
            .flex_none()
            .w(r(largura.max(imagens::ALTURA_CHIP)))
            .h(r(altura.max(imagens::ALTURA_CHIP)))
            .rounded(raio::CONTROLE)
            .bg(Cores::preto())
            .overflow_hidden()
            .child(img(caminho.to_path_buf()).size_full().object_fit(ObjectFit::Cover))
            .into_any_element()
    } else {
        let (icone, cor) = match (imagem, lido.existe) {
            (true, false) => (Lucide::ImageOff, Cores::apagado()),
            (false, false) => (Lucide::FileX, Cores::apagado()),
            (true, true) => (Lucide::Image, Cores::tenue()),
            (false, true) => (Lucide::File, Cores::tenue()),
        };
        icone_de(icone, 14.).text_color(cor).into_any_element()
    };
    let (chat, alvo) = (chat.to_string(), caminho.to_path_buf());
    h_flex()
        .id(SharedString::from(id))
        .flex_none()
        .max_w_full()
        .h(r(28.))
        .map(|s| if abre { s.pl(esp::E4) } else { s.pl(esp::E8) })
        .pr(esp::E8)
        .gap(esp::E6)
        .rounded(raio::de(28.))
        .bg(if no_balao { Cores::recuo() } else { Cores::cartao_elevado() })
        .border_1()
        .border_color(if no_balao { Cores::borda() } else { Cores::borda_forte() })
        .metadado()
        .child(simbolo)
        .child(div().min_w_0().max_w(r(220.)).truncate().text_color(if lido.existe { Cores::texto() } else { Cores::tenue() }).child(nome))
        .child(div().flex_none().text_color(Cores::tenue()).child(medida))
        .when(abre, |s| {
            s.cursor_pointer()
                .hover(|s| s.bg(Cores::hover()))
                .on_click(cx.listener(move |this, _, window, cx| this.abrir_visor(&chat, &alvo, window, cx)))
        })
        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
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
    /// Desde que posição as mensagens do usuário dão para corrigir (ver `reenvio`); `None`, nenhuma.
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
            editavel: super::reenvio::editaveis_desde(motor, chat),
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
const ALTURA_DO_LOG: Rems = r(32.);

/// O resultado aberto de uma linha de log, embaixo do texto dela.
const RECUO_DO_LOG: Rems = r(56.);

/// O mesmo numa chamada solta (fora de grupo), que começa a 12 como o cabeçalho do grupo, sem
/// ramo: a coluna do texto dela (12 + 14 + 8).
const RECUO_DO_LOG_SOLTO: Rems = r(34.);

/// O resumo do grupo de chamadas: "Rodou 13 comandos · editou 1 arquivo · leu 3 arquivos" no
/// painel largo e, fora dele, a forma curta sem os verbos, "13 comandos · 1 editado · 3 lidos":
/// cortado, o fim levava o último número junto ("leu 3 arqu…") (relatório de design, 16/11/2026).
fn resumo_das_chamadas(chamadas: &[(usize, &Item)], curto: bool) -> String {
    let (mut comandos, mut leituras, mut buscas, mut outras, mut pensamentos) = (0, 0, 0, 0, 0);
    for (_, item) in chamadas {
        match item {
            Item::Raciocinio { .. } => pensamentos += 1,
            Item::Ferramenta { nome, .. } => match nome.as_str() {
                "Bash" => comandos += 1,
                "Read" => leituras += 1,
                "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {}
                "Grep" | "Glob" | "WebSearch" | "WebFetch" => buscas += 1,
                _ => outras += 1,
            },
            _ => {}
        }
    }
    // Até dois arquivos, o nome deles ("editou carrinho.py e motor.rs"); acima, a contagem (5.1).
    let editados = editados_nas_chamadas(chamadas.iter().map(|(_, item)| *item));
    let mut partes = Vec::new();
    if curto {
        if comandos > 0 { partes.push(contagem(comandos, "comando", "comandos")); }
        if (1..=2).contains(&editados.len()) { partes.push(editou(&editados)); }
        else if !editados.is_empty() { partes.push(contagem(editados.len(), "editado", "editados")); }
        if leituras > 0 { partes.push(contagem(leituras, "lido", "lidos")); }
        if buscas > 0 { partes.push(contagem(buscas, "busca", "buscas")); }
        if outras > 0 { partes.push(contagem(outras, "ferramenta", "ferramentas")); }
        if partes.is_empty() && pensamentos > 0 { partes.push("Pensou".into()); }
    } else {
        if comandos > 0 { partes.push(format!("rodou {}", contagem(comandos, "comando", "comandos"))); }
        if !editados.is_empty() { partes.push(editou(&editados)); }
        if leituras > 0 { partes.push(format!("leu {}", contagem(leituras, "arquivo", "arquivos"))); }
        if buscas > 0 { partes.push(format!("fez {}", contagem(buscas, "busca", "buscas"))); }
        if outras > 0 { partes.push(format!("usou {}", contagem(outras, "ferramenta", "ferramentas"))); }
        if partes.is_empty() && pensamentos > 0 { partes.push("Pensou sobre o pedido".into()); }
    }
    let mut resumo = partes.join(" · ");
    if let Some(primeira) = resumo.get_mut(..1) { primeira.make_ascii_uppercase(); }
    resumo
}

/// Um turno pronto que recolhe numa linha "Trabalhou…" (dono, 16/11/2026: rodada de uso, item
/// 2.1): de depois da `mensagem` (a sua, a entrega ou a vez da dupla que o abriu) até antes da
/// `resposta` final, tudo vira uma linha, com a resposta logo embaixo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TurnoRecolhivel {
    pub mensagem: usize,
    pub resposta: usize,
}

impl TurnoRecolhivel {
    /// O item `indice` está no que recolhe.
    pub fn contem(&self, indice: usize) -> bool {
        indice > self.mensagem && indice < self.resposta
    }

    /// A chave de abrir em `expandidos` (o Ctrl+F a abre ao achar algo lá dentro).
    pub fn chave(&self, chat: &str) -> String {
        format!("{chat}:{}:turno", self.mensagem)
    }
}

/// Os turnos que recolhem: os prontos (o último, com o chat `rodando`, não) com uma resposta final
/// e, antes dela, pelo menos uma trilha e um texto intermediário. Na dupla, a vez de cada agente
/// é um turno (um recolhido por agente).
pub(super) fn turnos_recolhiveis(itens: &[Item], rodando: bool) -> Vec<TurnoRecolhivel> {
    let marcos: Vec<usize> = itens
        .iter()
        .enumerate()
        .filter(|(_, i)| matches!(i, Item::Usuario { .. } | Item::Entrega { .. } | Item::Vez { .. }))
        .map(|(k, _)| k)
        .collect();
    let mut saida = Vec::new();
    for (n, &mensagem) in marcos.iter().enumerate() {
        let fim = marcos.get(n + 1).copied().unwrap_or(itens.len());
        // A revisão da memória tem a linha dela (item 6.1).
        if (rodando && fim == itens.len()) || crate::motor::e_revisao_da_memoria(&itens[mensagem]) {
            continue;
        }
        let Some(resposta) = itens[mensagem + 1..fim].iter().rposition(|i| matches!(i, Item::Assistente { .. })).map(|k| mensagem + 1 + k)
        else {
            continue;
        };
        let antes = &itens[mensagem + 1..resposta];
        let trilha = antes.iter().any(|i| matches!(i, Item::Ferramenta { .. } | Item::Raciocinio { .. }));
        let texto = antes.iter().any(|i| matches!(i, Item::Assistente { .. }));
        if trilha && texto {
            saida.push(TurnoRecolhivel { mensagem, resposta });
        }
    }
    saida
}

/// O que fica à vista com o turno recolhido: o texto intermediário que termina em pergunta, o
/// aviso de erro e o cartão de falha.
pub(super) fn fica_fora_do_recolhido(item: &Item) -> bool {
    match item {
        Item::Assistente { texto, .. } => texto.trim_end().ends_with('?'),
        Item::Aviso { texto, erro, .. } => *erro || tipo_aviso(texto, *erro) == ClasseAvisoChat::Alerta,
        _ => false,
    }
}

/// A linha recolhida da revisão da memória (item 6.1), pela ferramenta usada e não pelo texto:
/// "Memória revista · nada a guardar", "Memória: editou AGENTS.md". Também os arquivos que ela
/// editou, para abrir pelo clique.
pub(super) fn resumo_da_revisao(itens: &[Item]) -> (String, Vec<String>) {
    let escritas: Vec<&Item> =
        itens.iter().filter(|i| matches!(i, Item::Ferramenta { nome, .. } if crate::motor::ferramenta_de_escrita(nome))).collect();
    let nomes = editados_nas_chamadas(escritas.iter().copied());
    let caminhos = escritas
        .iter()
        .filter_map(|i| match i {
            Item::Ferramenta { arquivo: Some(a), .. } => Some(a.clone()),
            _ => None,
        })
        .collect();
    let texto = if !nomes.is_empty() {
        format!("Memória: {}", editou(&nomes))
    } else if !escritas.is_empty() {
        "Memória: guardou o que valia".to_string()
    } else {
        "Memória revista · nada a guardar".to_string()
    };
    (texto, caminhos)
}

/// O +N −M de uma chamada de edição: o contado quando ela chegou ou, nas de antes disso, o do
/// detalhe gravado (que é cortado: sem número quando ele foi cortado).
pub(super) fn linhas_da_edicao_gravada(item: &Item) -> Option<(usize, usize)> {
    let Item::Ferramenta { nome, detalhe, mudanca, .. } = item else {
        return None;
    };
    if !matches!(nome_curto(nome), "Edit" | "MultiEdit" | "Write" | "NotebookEdit") {
        return None;
    }
    match mudanca {
        Some(m) => Some((m.entram as usize, m.saem as usize)).filter(|(e, s)| e + s > 0),
        None => detalhe.as_deref().and_then(contagem_do_diff),
    }
}

/// Um arquivo que o chat editou ou criou, somado em todas as chamadas: para a seção Mudanças de
/// "Este chat" e para a faixa colada na caixa (rodada de uso, item 5.1).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ArquivoMudado {
    /// O caminho como a chamada o deu (relativo ou inteiro).
    pub caminho: String,
    pub entram: usize,
    pub saem: usize,
    pub novo: bool,
}

/// As mudanças do chat pelas chamadas de edição: as do último turno (desde a sua última mensagem)
/// e as de antes, cada arquivo uma vez (no turno em que mudou por último). O que um comando mudou
/// fica de fora: não há como saber pelas chamadas.
pub(super) fn mudancas_do_chat(itens: &[Item]) -> (Vec<ArquivoMudado>, Vec<ArquivoMudado>) {
    let turno = itens.iter().rposition(|i| matches!(i, Item::Usuario { .. })).unwrap_or(0);
    let mut neste: Vec<ArquivoMudado> = Vec::new();
    let mut antes: Vec<ArquivoMudado> = Vec::new();
    for (i, item) in itens.iter().enumerate() {
        let Item::Ferramenta { nome, arquivo, resumo, erro: false, mudanca, .. } = item else {
            continue;
        };
        if !matches!(nome_curto(nome), "Edit" | "MultiEdit" | "Write" | "NotebookEdit") {
            continue;
        }
        let caminho = arquivo.clone().unwrap_or_else(|| resumo.clone());
        let (entram, saem) = linhas_da_edicao_gravada(item).unwrap_or((0, 0));
        let novo = mudanca.is_some_and(|m| m.novo);
        let lista = if i > turno { &mut neste } else { &mut antes };
        match lista.iter_mut().find(|a| a.caminho == caminho) {
            Some(a) => {
                a.entram += entram;
                a.saem += saem;
                a.novo |= novo;
            }
            None => lista.push(ArquivoMudado { caminho, entram, saem, novo }),
        }
    }
    // O que mudou neste turno não se repete em "Antes".
    antes.retain(|a| !neste.iter().any(|n| n.caminho == a.caminho));
    (neste, antes)
}

/// Quanto o turno de agora mudou: arquivos e o total de linhas que entraram e saíram.
pub(super) fn total_das_mudancas(arquivos: &[ArquivoMudado]) -> (usize, usize, usize) {
    arquivos.iter().fold((0, 0, 0), |(n, e, s), a| (n + 1, e + a.entram, s + a.saem))
}

/// Quanto o turno levou: "45 s", "6 min 12 s", "1 h 3 min".
pub(super) fn duracao_do_turno(segundos: u64) -> String {
    match segundos {
        s if s < 60 => format!("{s} s"),
        s if s < 3600 && s % 60 == 0 => format!("{} min", s / 60),
        s if s < 3600 => format!("{} min {} s", s / 60, s % 60),
        s if (s % 3600) / 60 == 0 => format!("{} h", s / 3600),
        s => format!("{} h {} min", s / 3600, (s % 3600) / 60),
    }
}

/// Os arquivos editados ou gravados, pelo nome, na ordem em que apareceram (sem repetir).
fn editados_nas_chamadas<'a>(chamadas: impl IntoIterator<Item = &'a Item>) -> Vec<String> {
    let mut nomes: Vec<String> = Vec::new();
    for item in chamadas {
        if let Item::Ferramenta { nome, arquivo, resumo, .. } = item
            && matches!(nome_curto(nome), "Edit" | "Write" | "MultiEdit" | "NotebookEdit")
        {
            let caminho = arquivo.as_deref().unwrap_or(resumo);
            let nome = Path::new(caminho).file_name().map_or_else(|| caminho.to_string(), |n| n.to_string_lossy().into_owned());
            if !nomes.contains(&nome) {
                nomes.push(nome);
            }
        }
    }
    nomes
}

/// "editou carrinho.py e motor.rs" com até dois arquivos; acima disso, a contagem (5.1).
pub(super) fn editou(nomes: &[String]) -> String {
    match nomes {
        [] => String::new(),
        [um] => format!("editou {um}"),
        [um, dois] => format!("editou {um} e {dois}"),
        varios => format!("editou {}", contagem(varios.len(), "arquivo", "arquivos")),
    }
}

/// A linha do turno recolhido: "Trabalhou 6 min 12 s · 13 comandos · editou carrinho.py e
/// motor.rs". A duração é a hora do último item menos a da mensagem; sem hora (conversa de antes
/// da 0.4.0), sem número.
pub(super) fn resumo_do_turno(itens: &[Item], turno: TurnoRecolhivel) -> String {
    let inicio = itens[turno.mensagem].em();
    let fim = itens[turno.mensagem..=turno.resposta].iter().map(Item::em).max().unwrap_or(0);
    let mut partes = vec![if inicio > 0 && fim >= inicio {
        format!("Trabalhou {}", duracao_do_turno(fim - inicio))
    } else {
        "Trabalhou".to_string()
    }];
    let trecho = &itens[turno.mensagem + 1..turno.resposta];
    let comandos = trecho.iter().filter(|i| matches!(i, Item::Ferramenta { nome, .. } if nome == "Bash")).count();
    if comandos > 0 {
        partes.push(contagem(comandos, "comando", "comandos"));
    }
    let nomes = editados_nas_chamadas(trecho);
    if !nomes.is_empty() {
        partes.push(editou(&nomes));
    } else {
        let lidos = trecho.iter().filter(|i| matches!(i, Item::Ferramenta { nome, .. } if nome == "Read")).count();
        if lidos > 0 {
            partes.push(format!("leu {}", contagem(lidos, "arquivo", "arquivos")));
        }
    }
    partes.join(" · ")
}

/// Trilho de 1 px que liga os ícones de linhas de log seguidas: no centro do ícone (12 + 7),
/// de 2 px abaixo dele até 1 px antes do ícone da linha de baixo.
fn trilho_do_log(continua: bool) -> Div {
    div()
        .absolute()
        .inset_0()
        .child(canvas(|_, _, _| (), move |bounds, _, window, _| {
            let x = bounds.left() + r(19.).to_pixels(window.rem_size());
            let y = bounds.top();
            let passo = r(16.).to_pixels(window.rem_size());
            let curva = r(6.).to_pixels(window.rem_size());
            let mut linha = PathBuilder::stroke(px(1.));
            linha.move_to(point(x, y));
            linha.line_to(point(x, y + passo - curva));
            linha.curve_to(point(x + curva, y + passo), point(x, y + passo));
            linha.line_to(point(bounds.left() + r(28.).to_pixels(window.rem_size()), y + passo));
            if continua { linha.move_to(point(x, y + passo - curva)); linha.line_to(point(x, bounds.bottom())); }
            if let Ok(path) = linha.build() { window.paint_path(path, Cores::traco()); }
        }).size_full())
}

impl Janela {

    /// Chamadas ligadas por ramificações, com resumo do trabalho. O grupo abre durante o turno
    /// e recolhe quando termina; a escolha manual prevalece.
    fn trilha(
        &self,
        contexto: &Contexto,
        chamadas: &[(usize, &Item)],
        ao_vivo: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        if let [(indice, item)] = chamadas {
            return self.chamada(contexto, *indice, item, None, cx);
        }
        let recolhivel = chamadas.len() > 1;
        // Os itens só crescem no fim: o índice da primeira chamada identifica o grupo.
        let chave = format!("{}:{}:grupo", contexto.chat.id, chamadas[0].0);
        let aberto = !recolhivel || self.expandidos.contains(&chave)
            || (ao_vivo && !self.expandidos.contains(&format!("{chave}:fechado")));
        let mostradas = if aberto {
            chamadas
        } else if ao_vivo {
            &chamadas[chamadas.len() - 1..]
        } else {
            &[]
        };
        let cabecalho = recolhivel.then(|| {
            let resumo = resumo_das_chamadas(chamadas, !larga(contexto.nivel));
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
                // Na coluna dos ícones, o da trilha, de onde descem os ramos das chamadas; o
                // chevron vai no fim, como na chamada e no "Pensou" (relatório de design,
                // 16/11/2026: à esquerda só aqui, à direita no resto).
                .child(
                    div()
                        .flex_none()
                        .size(r(14.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(Cores::tenue())
                        .child(icone_de(Lucide::ListTree, 14.)),
                )
                .child(div().min_w_0().truncate().text_color(Cores::tenue()).child(if ao_vivo {
                    ShimmerText::new(resumo).duration(Duration::from_millis(3400)).highlight_color(Cores::forte()).into_any_element()
                } else { div().child(resumo).into_any_element() }))
                .when(falhas > 0, |s| {
                    s.child(Selo::cor(contagem(falhas, "falhou", "falharam"), Cores::vermelho()))
                })
                .when(rodando_escondida, |s| s.child(AnelGirando::new(14.).cor(Cores::destaque())))
                .child(div().flex_none().text_color(Cores::apagado()).child(icone_de(if aberto { Lucide::ChevronDown } else { Lucide::ChevronRight }, 12.)))
                .on_click(cx.listener({ let chave = chave.clone(); move |j, _, _, cx| {
                    if aberto { j.expandidos.remove(&chave); j.expandidos.insert(format!("{chave}:fechado")); }
                    else { j.expandidos.remove(&format!("{chave}:fechado")); j.expandidos.insert(chave.clone()); }
                    for p in j.paineis.iter_mut().filter(|p| p.seguindo) { p.manter_lugar = true; }
                    cx.notify();
                }}))
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
        // Uma imagem só fica na pílula dela. Mais de uma vira uma linha só: a pilha de até 4
        // miniaturas, "9 imagens" e, no clique, o visor na primeira (as setas passam pelas outras)
        // (dono, 16/11/2026: "tá meio ruim isso aqui, tinha que ser mais organizado": eram pílulas
        // de larguras diferentes quebrando em 8 linhas, com um "+1" solto no fim).
        let fileira = match imagens_do_grupo.as_slice() {
            [] => None,
            [(indice, imagem)] => Some(
                h_flex()
                    .pl(RECUO_DO_LOG_SOLTO)
                    .py(esp::E4)
                    .child(pilula_de_arquivo(format!("img-{}:{indice}:g", contexto.chat.id), imagem, &contexto.chat.id, false, cx))
                    .into_any_element(),
            ),
            varias => Some(pilha_de_imagens(&contexto.chat.id, varias, cx)),
        };
        let ultima = mostradas.len().saturating_sub(1);
        v_flex()
            .w_full()
            .children(cabecalho)
            .children(fileira)
            .children(
                mostradas
                    .iter()
                    .enumerate()
                    .map(|(k, (indice, item))| self.chamada(contexto, *indice, item, Some(k < ultima), cx)),
            )
            .into_any_element()
    }

    /// A linha de um turno pronto, "Trabalhou 6 min · 13 comandos · editou x e y", que abre e
    /// recolhe o que veio antes da resposta final (rodada de uso, 2.1). Recolhida, as imagens que as
    /// chamadas escondidas leram ou gravaram continuam à vista embaixo dela (dono, 13/11 e 16/11).
    fn linha_do_turno(
        &self,
        contexto: &Contexto,
        turno: TurnoRecolhivel,
        aberto: bool,
        escondidos: &[(usize, &Item)],
        cx: &Context<Self>,
    ) -> AnyElement {
        let chave = turno.chave(&contexto.chat.id);
        let resumo = resumo_do_turno(&contexto.chat.itens, turno);
        let dica = SharedString::from(if aberto { "Recolher o que o agente fez neste turno" } else { "Ver o que o agente fez neste turno" });
        let linha = h_flex()
            .id(SharedString::from(format!("turno-{chave}")))
            .h(ALTURA_DO_LOG)
            .px(esp::E12)
            .gap(esp::E8)
            .rounded(raio::CONTROLE)
            .metadado()
            .cursor_pointer()
            .hover(|s| s.bg(Cores::hover()))
            .tab_index(0)
            .focus_visible(|s| s.shadow(halo_foco()))
            .child(div().flex_none().size(r(14.)).flex().items_center().justify_center().text_color(Cores::tenue()).child(icone_de(Lucide::Clock, 14.)))
            .child(div().min_w_0().truncate().text_color(Cores::tenue()).child(resumo))
            .child(div().flex_none().text_color(Cores::apagado()).child(icone_de(if aberto { Lucide::ChevronDown } else { Lucide::ChevronRight }, 12.)))
            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            .on_click(alternar(chave.clone(), cx));
        let mut imagens: Vec<(usize, PathBuf)> = Vec::new();
        if !aberto {
            for (indice, item) in escondidos {
                if let Some(imagem) = imagem_da_chamada(item, &contexto.chat.cwd)
                    && !imagens.iter().any(|(_, outra)| *outra == imagem)
                {
                    imagens.push((*indice, imagem));
                }
            }
        }
        let fileira = match imagens.as_slice() {
            [] => None,
            [(indice, imagem)] => Some(
                h_flex()
                    .pl(RECUO_DO_LOG_SOLTO)
                    .py(esp::E4)
                    .child(pilula_de_arquivo(format!("img-{}:{indice}:t", contexto.chat.id), imagem, &contexto.chat.id, false, cx))
                    .into_any_element(),
            ),
            varias => Some(pilha_de_imagens(&contexto.chat.id, varias, cx)),
        };
        v_flex().w_full().child(linha).children(fileira).into_any_element()
    }

    /// A linha da revisão da memória (item 6.1): recolhida por padrão; o clique mostra o que ela
    /// fez. Com um arquivo editado, o ícone ao lado o abre à direita.
    #[allow(clippy::too_many_arguments)]
    fn linha_da_memoria(
        &self,
        contexto: &Contexto,
        chave: &str,
        itens: &[Item],
        rodando: bool,
        aberta: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let (texto, caminhos) = if rodando { ("Revendo a memória…".to_string(), Vec::new()) } else { resumo_da_revisao(itens) };
        let abrir = caminhos
            .first()
            .and_then(|a| super::links_arquivo::resolver(&contexto.chat.cwd, a))
            .map(|inteiro| super::links_arquivo::link_de(&inteiro, None));
        h_flex()
            .id(SharedString::from(format!("memoria-{chave}")))
            .h(ALTURA_DO_LOG)
            .px(esp::E12)
            .gap(esp::E8)
            .rounded(raio::CONTROLE)
            .metadado()
            .cursor_pointer()
            .hover(|s| s.bg(Cores::hover()))
            .child(div().flex_none().size(r(14.)).flex().items_center().justify_center().child(if rodando {
                AnelGirando::new(14.).cor(Cores::tenue()).into_any_element()
            } else {
                icone_de(Lucide::Brain, 14.).text_color(Cores::tenue()).into_any_element()
            }))
            .child(div().min_w_0().truncate().text_color(Cores::tenue()).child(texto))
            .child(div().flex_none().text_color(Cores::apagado()).child(icone_de(if aberta { Lucide::ChevronDown } else { Lucide::ChevronRight }, 12.)))
            .when_some(abrir, |s, alvo| {
                s.child(
                    botao_so_icone(
                        SharedString::from(format!("memoria-abrir-{chave}")),
                        Lucide::PanelRight,
                        "Abrir o arquivo ao lado",
                        TamanhoBotao::Linha,
                        move |_, window, cx| window.dispatch_action(Box::new(super::links_arquivo::AbrirLinkVitral(alvo.clone())), cx),
                        window,
                        cx,
                    ),
                )
            })
            .on_click(alternar(chave.to_string(), cx))
            .into_any_element()
    }

    /// Um bloco de raciocínio na trilha: "Pensou" e quanto durou; o clique mostra o texto, quando
    /// o Claude Code o entrega (referência do dono: 21st.dev, "AI Thinking and Tool calls").
    fn raciocinio(&self, contexto: &Contexto, indice: usize, texto: &str, milis: u64, ramo: Option<bool>, cx: &Context<Self>) -> AnyElement {
        let chave = format!("{}:{indice}", contexto.chat.id);
        let tem_texto = !texto.trim().is_empty();
        let aberto = tem_texto && self.expandidos.contains(&chave);
        let (recuo, recuo_aberto) = if ramo.is_some() { (esp::E32, RECUO_DO_LOG) } else { (esp::E12, RECUO_DO_LOG_SOLTO) };
        let linha = h_flex()
            .id(SharedString::from(format!("raciocinio-{chave}")))
            .h(ALTURA_DO_LOG)
            .pl(recuo)
            .pr(esp::E8)
            .gap(esp::E8)
            .rounded(raio::CONTROLE)
            .metadado()
            .child(div().flex_none().size(r(14.)).flex().items_center().justify_center().child(icone_de(Lucide::Brain, 14.).text_color(Cores::tenue())))
            .child(div().flex_none().font_weight(FontWeight::MEDIUM).text_color(Cores::texto()).child("Pensou"))
            // Sem `flex_1`: o chevron vem logo depois do texto, como na chamada.
            .child(
                div()
                    .flex_shrink(1.)
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
                    div().pl(recuo_aberto).py(esp::E4).child(
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
            .children(ramo.map(trilho_do_log))
            .into_any_element()
    }

    /// Uma chamada comum: ícone do tipo (cor só na falha), verbo, detalhe e a meta. O clique
    /// mostra o resultado. `ramo`: dentro de um grupo, o ramo que a liga ao cabeçalho (e se ele
    /// continua para a de baixo); solta, `None`, ela começa na coluna do cabeçalho, sem ramo
    /// (relatório de design, 16/11/2026: o "└" da chamada solta pendia do nada).
    fn chamada(
        &self,
        contexto: &Contexto,
        indice: usize,
        item: &Item,
        ramo: Option<bool>,
        cx: &Context<Self>,
    ) -> AnyElement {
        if let Item::Raciocinio { texto, milis, .. } = item {
            return self.raciocinio(contexto, indice, texto, *milis, ramo, cx);
        }
        let (recuo, recuo_aberto) = if ramo.is_some() { (esp::E32, RECUO_DO_LOG) } else { (esp::E12, RECUO_DO_LOG_SOLTO) };
        let Item::Ferramenta {
            nome,
            resumo,
            resultado,
            erro,
            detalhe: pedido,
            arquivo,
            sub,
            em,
            mudanca,
            ..
        } = item
        else {
            return div().into_any_element();
        };
        let chave = format!("{}:{indice}", contexto.chat.id);
        let aberto = self.expandidos.contains(&chave);
        let novo = mudanca.is_some_and(|m| m.novo);
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
                .pl(recuo_aberto)
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
            div().pl(recuo_aberto).pt(esp::E4).child(
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
        // O +N −M contado com o texto inteiro quando a chamada chegou (item 5.1); as chamadas de
        // antes disso contam pelo detalhe gravado.
        let linhas_do_diff = linhas_da_edicao_gravada(item);
        let verbo = verbo_da_ferramenta(nome, pendente);
        let detalhe = sem_marcacao(&detalhe_da_chamada(nome, resumo));
        let detalhe = if detalhe.is_empty() && nome == "Bash" {
            pedido.as_deref().and_then(|p| p.strip_prefix("$ ")).unwrap_or(resumo).to_string()
        } else { detalhe };
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
        // O detalhe: o que é código (comando, caminho, padrão de busca, endereço, ferramenta MCP)
        // sai em mono dentro do chip; a prosa (a tarefa do Agent, a busca na web, a lista de
        // tarefas) sai em Inter, sem chip. Antes, o "conferir cupons" do Agent saía em mono solto
        // e o mesmo texto do Codex num chip (relatório de design, 16/11/2026).
        let codigo = arquivo.is_some()
            || matches!(nome.as_str(), "Bash" | "BashOutput" | "KillShell" | "Grep" | "Glob" | "WebFetch")
            || nome.starts_with("mcp__");
        // A coluna do verbo (56) só alinha os detalhes dentro de um grupo; na solta, ela deixava
        // uns 35 px entre "Rodou" e o chip.
        let em_grupo = ramo.is_some();
        let linha = h_flex()
            .id(SharedString::from(format!("ferramenta-{chave}")))
            .h(ALTURA_DO_LOG)
            .pl(recuo)
            .pr(esp::E8)
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
                        .when(em_grupo, |s| s.min_w(r(56.)))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(Cores::texto())
                        .child(verbo.to_string()),
                )
            })
            .child(
                div()
                    .flex_shrink(1.)
                    .min_w_0()
                    .truncate()
                    .map(|s| {
                        if codigo {
                            s.font_family(FONTE_MONO)
                                .text_color(Cores::tenue())
                                .h(r(22.))
                                .px(esp::E6)
                                .rounded(raio::CONTROLE)
                                .bg(Cores::recuo())
                                .flex()
                                .items_center()
                        } else {
                            s.text_color(Cores::texto())
                        }
                    })
                    .child(busca_conversa::realcar(if let Some(arquivo) = arquivo {
                        Path::new(arquivo).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(detalhe)
                    } else { detalhe }, &no_resumo)),
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
            .when(novo && !minima && !*erro, |s| s.child(Selo::cor("novo", Cores::verde())))
            .when_some(meta.filter(|_| !estreita), |s, (meta, cor)| {
                s.child(div().flex_none().text_color(cor).child(meta))
            })
            .group(grupo.clone())
            .child(icone_de(if aberto { Lucide::ChevronDown } else { Lucide::ChevronRight }, 12.).text_color(Cores::apagado()))
            .when_some(abrir.clone(), |s, alvo| {
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
        let comando = (nome == "Bash").then(|| pedido.as_deref().and_then(|p| p.strip_prefix("$ ")).unwrap_or(resumo));
        let bloco = v_flex()
            .id(SharedString::from(format!("bloco-chamada-{chave}")))
            .relative()
            .w_full()
            .child(linha)
            .children(atividade_viva)
            .when_some(imagem.as_deref(), |s, imagem| {
                s.child(h_flex().pl(recuo_aberto).py(esp::E4).child(pilula_de_arquivo(format!("img-{chave}"), imagem, &contexto.chat.id, false, cx)))
            })
            .when(aberto, |s| {
                s.children(passos).when_some(pedido.as_deref().filter(|_| comando.is_none()), |s, pedido| {
                    s.child(div().pl(recuo_aberto).pt(esp::E4).child(cartao_de_edicao(&chave, arquivo.as_deref(), pedido)))
                })
                .when_some(comando, |s, comando| {
                    s.child(div().pl(recuo_aberto).py(esp::E4).child(
                        CaixaDeCodigo::new(SharedString::from(format!("terminal-{chave}")), resultado.as_deref()
                            .unwrap_or(if pendente { "A saída aparece quando terminar." } else { "(interrompido, sem saída)" }).to_string().into())
                            // A busca da conversa conta a saída do comando: o achado ganha o realce
                            // e a caixa rola até ele (conferência na bancada, 16/11/2026).
                            .realce(&no_resultado)
                            .comando(comando.to_string()).raio_chat().erro(*erro)
                            .tempo(if pendente { "Em execução" } else if *erro { "Falhou" } else { "Concluído" }),
                    ))
                })
                // Rodando ainda não há saída: em vez da caixa do resultado vazia, com o Copiar e
                // um "(rodando…)" dentro, uma linha só (dono, 13/11/2026: "melhora essa parte que
                // fica o rodando").
                .when(comando.is_none() && pendente && resultado.is_none(), |s| {
                    let texto = match rodando_ha {
                        Some(segundos) => format!("A saída aparece quando terminar · rodando há {}", relogio(segundos)),
                        None => "A saída aparece quando terminar".to_string(),
                    };
                    s.child(div().pl(recuo_aberto).pt(esp::E6).pb(esp::E4).child(
                        div().px(esp::E12).metadado().font_family(FONTE_MONO).text_color(Cores::apagado()).child(texto),
                    ))
                })
                .when(comando.is_none() && !so_o_pedido && !sem_texto && !(pendente && resultado.is_none()), |s| {
                    s.child(div().pl(recuo_aberto).py(esp::E4).child(
                        bloco_resultado(&chave, resultado.as_deref(), *erro, "(interrompida, sem resultado)").realce(&no_resultado),
                    ))
                })
                .when_some(abrir.filter(|_| pedido.is_none() && matches!(nome.as_str(), "Edit" | "Write")), |s, alvo| {
                    s.child(div().pl(recuo_aberto).py(esp::E8).child(
                        div().id(SharedString::from(format!("abrir-versao-{chave}"))).cursor_pointer().corpo_ui()
                            .text_color(Cores::destaque_texto()).child("Este registro não guardou o diff. Abrir o arquivo atual")
                            .on_click(move |_, window, cx| window.dispatch_action(Box::new(super::links_arquivo::AbrirLinkVitral(alvo.clone())), cx)),
                    ))
                })
                .when(so_o_pedido, |s| s.pb(esp::E4))
            })
            .children(ramo.map(trilho_do_log))
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
const LINHAS_DO_PEDIDO: usize = 600;

fn cartao_de_edicao(chave: &str, arquivo: Option<&str>, pedido: &str) -> Div {
    div().w_full().rounded(raio::CARTAO).border_1().border_color(Cores::borda()).bg(Cores::preto()).overflow_hidden()
        .child(h_flex().gap(esp::E8).px(esp::E8).h(r(28.))
            .child(icone_de(Lucide::FileDiff, 14.).text_color(Cores::tenue()))
            .child(div().flex_1().min_w_0().truncate().mono_pequeno().text_color(Cores::texto()).child(
                arquivo.and_then(|a| Path::new(a).file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Alteração".into())))
            .child(BotaoCopiar::new(SharedString::from(format!("copiar-diff-{chave}")), pedido.to_string())))
        .child(bloco_do_pedido(pedido))
}

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
        .max_h(r(300.))
        .overflow_y_scrollbar()
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
    /// Erro, no cartão de erro (`Janela::cartao_de_erro`).
    Alerta,
}

/// O aviso do turno cortado ao fechar o Vitral (`motor::marcar_turnos_cortados`). O texto gravado
/// não muda: o motor compara o texto exato para não avisar duas vezes.
const VITRAL_FECHADO: &str = "O Vitral foi fechado";

/// A troca da sessão do Claude Code por uma nova com resumo: as duas versões (a que "sumiu", gravada
/// como erro, e a que "expirou") são um marco, porque o turno continuou.
const SESSAO_ANTIGA: &str = "A sessão antiga do Claude Code";

/// Os textos vêm todos de `motor.rs`.
fn tipo_aviso(texto: &str, erro: bool) -> ClasseAvisoChat {
    if texto.starts_with(SESSAO_ANTIGA) {
        return ClasseAvisoChat::Marco;
    }
    const MARCOS: [&str; 6] = [
        "Sessão nova",
        "Interrompido",
        "Saiu do modo Planejar",
        "Retomando a sessão",
        crate::importar::MARCO,
        VITRAL_FECHADO,
    ];
    if erro {
        ClasseAvisoChat::Alerta
    } else if MARCOS.iter().any(|m| texto.starts_with(m)) {
        ClasseAvisoChat::Marco
    } else {
        ClasseAvisoChat::Info
    }
}

fn aviso_do_chat(texto: &str, erro: bool, realce: &Realce) -> (Bloco, AnyElement) {
    match tipo_aviso(texto, erro) {
        ClasseAvisoChat::Marco => {
            let (icone, cor, rotulo) = if texto.starts_with(SESSAO_ANTIGA) {
                (Lucide::RefreshCw, Cores::tenue(), "Sessão nova, com um resumo da anterior".to_string())
            } else if texto.starts_with("Interrompido") {
                (Lucide::CirclePause, Cores::tenue(), "Turno interrompido".to_string())
            } else if texto.starts_with(VITRAL_FECHADO) {
                // Um marco de uma linha, sem a instrução (a faixa logo abaixo já tem o Retomar, e
                // nos antigos não há mais o que retomar) e sem nome de agente (no chat do Codex o
                // texto mandava o Claude continuar). Vale também para os avisos já gravados
                // (relatório de design, 16/11/2026).
                (Lucide::CirclePause, Cores::tenue(), "O Vitral fechou no meio do turno".to_string())
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
        // O erro vai para o cartão (`Janela::cartao_de_erro`); aqui só chega sem ser erro.
        ClasseAvisoChat::Info | ClasseAvisoChat::Alerta => {
            // O ícone pelo assunto: a troca de conta ou de agente, o limite, a nova tentativa
            // (balão e cartão de erro, 16/11/2026).
            let icone = if texto.contains("chegou ao limite do plano; continuando na conta")
                || texto.contains("perdeu o login no Claude Code; continuando na conta")
                || texto.starts_with("Daqui em diante quem responde é o")
            {
                Lucide::Shuffle
            } else if texto.starts_with("Todas as contas chegaram ao limite do plano") {
                Lucide::Gauge
            } else if texto.to_lowercase().contains("tentando de novo") {
                Lucide::RotateCw
            } else {
                Lucide::Info
            };
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
    }
}

/// Onde o usuário está num painel: o deslocamento da última conferência (para saber quando foi
/// ele que rolou) e o tamanho da conversa da última vez que ele estava no fim.
#[derive(Default)]
pub struct Leitura {
    deslocamento: Pixels,
    vista: (usize, usize),
}

impl Leitura {
    /// O tamanho da conversa que conta como visto: a pílula do fim diz "Novas mensagens" quando a
    /// conversa passou dele.
    pub(super) fn marcar_vista(&mut self, vista: (usize, usize)) {
        self.vista = vista;
    }
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
    /// A conversa grande ficou parada: a próxima mensagem relê estes tokens por inteiro.
    Fria { tokens: u64 },
    /// O último turno mudou arquivos e nada roda (item 5.1): quantos e as linhas.
    Mudou { arquivos: usize, entram: usize, saem: usize },
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
        // A revisão da memória roda por baixo, depois do "Terminou": só uma linha discreta, sem o
        // Esc (item 6.1). Uma mensagem sua a corta (ou espera, se ela já escreveu).
        if motor.revendo_memoria(&chat.id) {
            let linha = h_flex()
                .flex_1()
                .min_w_0()
                .gap(esp::E8)
                .child(AnelGirando::new(12.).cor(Cores::tenue()))
                .child(div().min_w_0().truncate().font_family(cx.theme().font_family.clone()).metadado().child("Revendo a memória…"));
            return Some((Some(linha.into_any_element()), None));
        }
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
            // Por último, e só com o chat quieto: a conversa grande que esfriou e, depois, o que o
            // último turno mudou (o lugar onde o dono aprovou "N arquivos mudados", 12/11; item 5.1).
            None => situacao_do_chat(motor, chat)
                .or_else(|| chat.fria(crate::modelo::agora()).map(|tokens| Situacao::Fria { tokens }))
                .or_else(|| {
                    let (arquivos, entram, saem) = total_das_mudancas(&mudancas_do_chat(&chat.itens).0);
                    (arquivos > 0).then_some(Situacao::Mudou { arquivos, entram, saem })
                })?,
        };
        // O turno em andamento mora na linha de estado da caixa, dentro da moldura
        // (`linha_de_estado`): acima da caixa só fica o que pede um botão seu.
        if matches!(situacao, Situacao::Rodando { .. }) {
            return None;
        }
        let medida = self.paineis.iter().find(|p| p.id == painel_id).map(|p| p.medida).unwrap_or_default();
        let fila_aberta = self.paineis.iter().any(|p| p.id == painel_id && p.fila_aberta);
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
            // No formato da lista e do sino ("Parada · Vitral fechou"), com o motivo inteiro na
            // dica: o cru do motor saía cortado em todo painel parado (relatório de design,
            // 16/11/2026). Numa linha só, com reticências no fim quando aperta: em duas linhas a
            // faixa ficava mais alta que as outras (o dono viu "Parada · Vitral" / "fechou").
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
                    .child({
                        let curto = motivo_curto(&motivo);
                        let dica = SharedString::from(format!("Parada: {motivo}"));
                        div()
                            .id(("faixa-parada", painel_id as usize))
                            .min_w_0()
                            .truncate()
                            .text_size(r(13.))
                            .line_height(r(18.))
                            .child(format!("Parada · {curto}"))
                            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                    })
                    .into_any_element(),
                Some(
                    botao_fantasma("faixa-retomar", Some(Lucide::Play.into()), "Retomar", TamanhoBotao::Barra, window, cx)
                        .on_click(retomar)
                        .into_any_element(),
                ),
            ),
            // O que falhou, em poucas palavras (balão e cartão de erro, 16/11/2026): "Limite de uso
            // do Codex · volta 12:30" diz mais que "O último turno falhou". Só o limite e o login
            // ganham saída própria, porque o motor sabe essas duas causas com certeza; no resto, a
            // primeira linha do erro, com o texto inteiro na dica (rodada de uso, item 3.2).
            Situacao::Falhou => {
                let cru = match motor.execucao(&chat.id) {
                    Execucao::Erro(texto) => texto,
                    _ => String::new(),
                };
                let (mut icone, mut cor, mut rotulo) = erros::na_faixa(&cru, chat.agente);
                let (tipo, agente) = erros::tipo_da_execucao(&cru, chat.agente);
                let conta = motor.conta_do_ultimo_turno(&chat.id).cloned();
                let tentar_de_novo = || {
                    botao_fantasma("faixa-tentar", Some(Lucide::RotateCw.into()), "Tentar de novo", TamanhoBotao::Barra, window, cx)
                        .on_click(tentar)
                        .into_any_element()
                };
                let acao = match (tipo, agente) {
                    // No limite de uma conta escolhida à mão, com outra com login e folga: um clique
                    // troca a conta do chat e repete o turno (eram 4 passos por painel).
                    (erros::TipoDeErro::Limite, Agente::Claude) => match (conta.as_ref(), motor.outra_conta_com_folga(chat)) {
                        (Some(atual), Some(outra)) => {
                            let volta = erros::hora_de_volta(&cru).map(|h| format!(" · volta {h}")).unwrap_or_default();
                            rotulo = format!("Limite da {}{volta}", atual.nome);
                            let (motor_e, id, destino) = (self.motor.clone(), chat.id.clone(), outra.id.clone());
                            let dica = format!("A conta deste chat passa a ser a {}; volta-se pelo seletor de modelo.", outra.nome);
                            botao_fantasma("faixa-outra-conta", Some(Lucide::Shuffle.into()), format!("Continuar na {}", outra.nome), TamanhoBotao::Barra, window, cx)
                                .tooltip(dica)
                                .on_click(move |_, _, cx| {
                                    if let Some(m) = &motor_e {
                                        m.update(cx, |m, cx| m.continuar_em_outra_conta(&id, &destino, cx));
                                    }
                                })
                                .into_any_element()
                        }
                        _ => tentar_de_novo(),
                    },
                    // O login da conta caiu: "Entrar de novo" abre as Contas já no login dela (8.3).
                    (erros::TipoDeErro::Login, Agente::Claude) => {
                        let sem_login = conta.filter(|c| motor.conta_sem_login(&c.id));
                        if let Some(c) = &sem_login {
                            (icone, cor, rotulo) = (Lucide::KeyRound, Some(Cores::espera()), format!("Login da {} caiu", c.nome));
                        }
                        let id_conta = sem_login.map(|c| c.id);
                        botao_fantasma("faixa-entrar", Some(Lucide::KeyRound.into()), "Entrar de novo", TamanhoBotao::Barra, window, cx)
                            .on_click(cx.listener(move |this, _, window, cx| this.entrar_de_novo_na_conta(id_conta.clone(), window, cx)))
                            .into_any_element()
                    }
                    _ => tentar_de_novo(),
                };
                let dica = SharedString::from(if cru.trim().is_empty() { rotulo.clone() } else { cru.trim().to_string() });
                (
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(r(8.))
                        .child(icone_de(icone, 14.).text_color(cor.unwrap_or(Cores::tenue())))
                        .child(
                            div()
                                .id(("faixa-erro", painel_id as usize))
                                .min_w_0()
                                .truncate()
                                .text_size(r(13.))
                                .line_height(r(18.))
                                .child(rotulo)
                                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx)),
                        )
                        .into_any_element(),
                    Some(acao),
                )
            }
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
            Situacao::Fria { tokens } => (
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(r(8.))
                    .child(icone_de(Lucide::Clock, 14.).text_color(Cores::tenue()))
                    .child(texto(format!(
                        "Relê {} · parada há {}",
                        composer::tokens(tokens),
                        super::tempo_relativo(chat.atualizado_em)
                    )))
                    .into_any_element(),
                Some(
                    botao_fantasma("faixa-compactar", Some(Lucide::Minimize.into()), "Compactar", TamanhoBotao::Barra, window, cx)
                        .tooltip("A conversa ficou parada além da hora em que o contexto fica guardado com desconto: a próxima mensagem relê tudo por inteiro. Compactar resume antes, e os próximos turnos releem só o resumo.")
                        .on_click(cx.listener(move |this, _, window, cx| this.por_na_caixa(painel_id, "/compact ", window, cx)))
                        .into_any_element(),
                ),
            ),
            // "3 arquivos mudados · +40 −12": o clique abre a direita em "Este chat", na seção
            // Mudanças (item 5.1).
            Situacao::Mudou { arquivos, entram, saem } => (
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(r(8.))
                    .child(icone_de(Lucide::FileDiff, 14.).text_color(Cores::tenue()))
                    .child(texto(contagem(arquivos, "arquivo mudado", "arquivos mudados")))
                    .when(entram + saem > 0, |s| {
                        s.child(
                            h_flex()
                                .flex_none()
                                .gap(esp::E6)
                                .font_family(FONTE_MONO)
                                .metadado()
                                .when(entram > 0, |s| s.child(div().text_color(Cores::verde()).child(format!("+{entram}"))))
                                .when(saem > 0, |s| s.child(div().text_color(Cores::vermelho()).child(format!("−{saem}")))),
                        )
                    })
                    .into_any_element(),
                Some(
                    botao_fantasma("faixa-mudancas", Some(Lucide::PanelRight.into()), "Ver mudanças", TamanhoBotao::Barra, window, cx)
                        .tooltip("Abrir a lista do que este chat mudou, à direita. O que um comando mudou não aparece aqui.")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.aba_direita = super::abas_direita::AbaDireita::Chat;
                            this.mostrar_direita(window, cx);
                        }))
                        .into_any_element(),
                ),
            ),
            Situacao::Fila(n) => (
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(r(8.))
                    .child(icone_de(Lucide::ListChecks, 14.).text_color(Cores::tenue()))
                    .child(self.pilula_da_fila(&chat.id, painel_id, n, false, estreita, window, cx))
                    .into_any_element(),
                // Com a fila aberta, o "Entregar agora" fica só no rodapé dela: eram dois iguais,
                // a uns 60 px um do outro (teste de usabilidade, 16/11/2026).
                (!fila_aberta).then(|| {
                    botao_fantasma("faixa-entregar", None, "Entregar agora", TamanhoBotao::Barra, window, cx)
                        .on_click(entregar)
                        .into_any_element()
                }),
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
                // A ação fica sempre inteira; quem corta é o texto, à esquerda.
                .children(direita.map(|d| div().flex_none().child(d)))
                .into_any_element(),
        )
    }

    /// Confere os painéis que o usuário rolou (roda, barra, ou o conteúdo que encolheu) desde a
    /// última vez: o que saiu do fim deixa de segui-lo; o que voltou ao fim conta como visto.
    /// Qualquer subida solta o fim (o painel no fim volta a ele a cada desenho, e um passo da
    /// roda ou do touchpad é menor que a folga); descendo, a folga de 48 já prende de novo.
    pub(super) fn acompanhar_rolagem(&mut self, cx: &mut Context<Self>) {
        let mut anteriores = Vec::new();
        for painel in &mut self.paineis {
            let deslocamento = painel.rolagem.offset().y;
            if deslocamento == painel.leitura.deslocamento {
                continue;
            }
            let subiu = deslocamento > painel.leitura.deslocamento;
            if subiu && deslocamento >= -px(240.) { anteriores.push(painel.id); }
            painel.leitura.deslocamento = deslocamento;
            let folga = if subiu { px(1.) } else { px(48.) };
            painel.seguindo = distancia_do_fim(&painel.rolagem) <= folga;
            if painel.seguindo
                && let (Some(chat), Some(motor)) = (&painel.chat, &self.motor)
            {
                motor.update(cx, |m, _| m.marcar_lido(chat));
            }
        }
        for painel in anteriores { self.carregar_anteriores(painel, cx); }
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
            BotaoCopiar::new(id_copiar, plano).dica("Copiar o plano");
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
    #[test]
    fn mensagens_acrescentadas_em_cima_preservam_o_ponto_de_leitura() {
        let antes = gpui_kit::px(-120.);
        let depois = super::deslocamento_apos_prepend(antes, gpui_kit::px(1800.), gpui_kit::px(3100.));
        assert_eq!(gpui_kit::px(1600.) + antes, gpui_kit::px(2900.) + depois);
        assert_eq!(super::deslocamento_apos_prepend(antes, gpui_kit::px(1800.), gpui_kit::px(1800.)), antes);
    }
    // Sem `super::*`: o `gpui_kit::*` lá de cima traz um `test` que troca o `#[test]`.
    use super::{
        Bloco, ClasseAvisoChat, MarcaDeLinha, comando_de_barra, detalhe_da_chamada, entrada_do_bloco, espaco_antes,
        com_quebras_duras, dica_da_pilha, linhas_da_edicao, resposta_repete_o_erro, texto_da_citacao, trechos_com_citacoes, marco_da_conversa, nome_e_id, previa, resumo_da_trilha, erros::partes_do_erro,
        resumo_das_chamadas, sem_marcacao,
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
            mudanca: None,
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
        let px = |e: Option<Bloco>, a: Bloco| espaco_antes(e, a).0 * 15.;
        let perto = |v: f32, esperado: f32| (v - esperado).abs() < 1e-3;
        assert!(perto(px(None, Balao), 0.));
        // Entre turnos, 32 antes do balão; 12 dele até a resposta (mais de 2 vezes). Nada fica
        // reservado para as ações.
        assert!(perto(px(Some(Texto), Balao), 32.));
        assert!(perto(px(Some(Log), Balao), 32.));
        assert!(perto(px(Some(Balao), Texto), 12.));
        assert!(perto(px(Some(Balao), Log), 12.));
        assert!(perto(px(Some(Balao), Marco), 16.));
        // O dia começa um turno: 32 antes dele, 12 dele até o balão.
        assert!(perto(px(Some(Marco), Dia), 32.));
        assert!(perto(px(Some(Dia), Balao), 12.));
        // Linhas de log coladas, no passo de 28; recebida seguida de log, quase.
        assert!(perto(px(Some(Log), Log), 0.));
        assert!(perto(px(Some(Recebida), Log), 4.));
        assert!(perto(px(Some(Recebida), Recebida), 4.));
        assert!(perto(px(Some(Log), Recebida), 12.));
        // O marco pede ar dos dois lados.
        assert!(perto(px(Some(Texto), Marco), 16.));
        assert!(perto(px(Some(Marco), Log), 16.));
        // O resto, dentro do turno.
        assert!(perto(px(Some(Texto), Log), 12.));
        assert!(perto(px(Some(Log), Alerta), 12.));
        assert!(perto(px(Some(Alerta), Texto), 12.));
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
        assert_eq!(
            tipo_aviso("O Vitral foi fechado no meio deste turno. Use Retomar para o Claude continuar de onde parou.", false),
            ClasseAvisoChat::Marco
        );
        assert_eq!(tipo_aviso("Sessão nova: o Claude não lembra das mensagens acima.", false), ClasseAvisoChat::Marco);
        assert_eq!(tipo_aviso("Retomando a sessão anterior.", false), ClasseAvisoChat::Marco);
        assert_eq!(tipo_aviso("Tentando de novo.", false), ClasseAvisoChat::Info);
        assert_eq!(
            tipo_aviso("A conta x chegou ao limite do plano; continuando na conta y.", false),
            ClasseAvisoChat::Info
        );
        assert_eq!(tipo_aviso("Falhou: o processo caiu", true), ClasseAvisoChat::Alerta);
        // A sessão trocada por uma nova com resumo: o turno continuou, é marco nas duas versões.
        assert_eq!(
            tipo_aviso("A sessão antiga do Claude Code sumiu; continuei numa nova com um resumo.", true),
            ClasseAvisoChat::Marco
        );
        assert_eq!(
            tipo_aviso("A sessão antiga do Claude Code expirou; continuei numa nova com um resumo.", false),
            ClasseAvisoChat::Marco
        );
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
    fn a_resposta_igual_ao_erro_do_turno_nao_aparece_duas_vezes() {
        let erro = "Failed to authenticate: OAuth token revoked";
        let aviso = |t: &str| Item::Aviso { em: 0, texto: t.into(), erro: true };
        let itens = [usuario("oi"), assistente(erro), aviso(&format!("O turno terminou com erro: {erro}"))];
        assert!(resposta_repete_o_erro(&itens, 1));
        // Outra resposta, ou o erro de outro turno, não some.
        let outra = [usuario("oi"), assistente("Vou olhar."), aviso(&format!("O turno terminou com erro: {erro}"))];
        assert!(!resposta_repete_o_erro(&outra, 1));
        let outro_turno = [assistente(erro), usuario("de novo"), aviso(&format!("O turno terminou com erro: {erro}"))];
        assert!(!resposta_repete_o_erro(&outro_turno, 0));
        // Um aviso que não é erro, com o mesmo texto, também não.
        let info = [assistente(erro), Item::Aviso { em: 0, texto: erro.into(), erro: false }];
        assert!(!resposta_repete_o_erro(&info, 0));
    }

    #[test]
    fn a_dica_da_pilha_mostra_ate_dez_nomes() {
        let nomes: Vec<String> = (1..=12).map(|i| format!("q{i:02}.png")).collect();
        let dica = dica_da_pilha(&nomes);
        assert_eq!(dica.lines().count(), 11);
        assert!(dica.starts_with("q01.png\nq02.png"));
        assert!(dica.ends_with("q10.png\ne mais 2"));
        assert_eq!(dica_da_pilha(&nomes[..2]), "q01.png\nq02.png");
    }

    #[test]
    fn a_citacao_e_um_trecho_proprio_e_copia_sem_as_marcas() {
        let resposta = "Para a Carol:\n\n> **Oi, Carol!**\n>\n> O pedido *saiu* hoje.\n> Rastreio: [aqui](https://x.y/1)\n\nQualquer coisa me avise.";
        let trechos = trechos_com_citacoes(resposta);
        assert_eq!(trechos.len(), 3, "{trechos:?}");
        assert_eq!(trechos[0], (false, "Para a Carol:".to_string()));
        assert!(trechos[1].0 && trechos[1].1.starts_with("> **Oi"));
        assert_eq!(trechos[2], (false, "Qualquer coisa me avise.".to_string()));
        assert_eq!(
            texto_da_citacao(&trechos[1].1),
            "Oi, Carol!\n\nO pedido saiu hoje.\nRastreio: aqui (https://x.y/1)"
        );
        // O ">" dentro de um bloco de código não é citação.
        let codigo = "veja:\n```\n> não é citação\n```";
        assert!(trechos_com_citacoes(codigo).iter().all(|(c, _)| !*c));
        // Lista, código e nome com sublinhado ficam; o título e o link de arquivo perdem a marca.
        assert_eq!(texto_da_citacao("> - item 1\n> - item 2"), "- item 1\n- item 2");
        assert_eq!(texto_da_citacao("> rode `cargo test` em nome_com_sublinhado"), "rode cargo test em nome_com_sublinhado");
        assert_eq!(texto_da_citacao("> ## Aviso\n> veja [notas.md](vitral:arquivo/x)"), "Aviso\nveja notas.md");
        assert_eq!(texto_da_citacao("> 2*3 = 6"), "2*3 = 6");
    }

    #[test]
    fn o_balao_guarda_as_quebras_que_voce_digitou() {
        assert_eq!(com_quebras_duras("linha um\nlinha dois\nlinha três"), "linha um  \nlinha dois  \nlinha três");
        // Linha em branco separa parágrafos: nada muda ali.
        assert_eq!(com_quebras_duras("um\n\ndois"), "um\n\ndois");
        // Dentro do bloco cercado e nas linhas de tabela, o texto fica como veio.
        let codigo = "veja:\n```\nfn a() {}\nfn b() {}\n```\nfim";
        assert_eq!(com_quebras_duras(codigo), "veja:\n```\nfn a() {}\nfn b() {}\n```\nfim");
        let tabela = "| a | b |\n|---|---|\n| 1 | 2 |";
        assert_eq!(com_quebras_duras(tabela), tabela);
        // A linha que já termina com a quebra dura não ganha outra.
        assert_eq!(com_quebras_duras("um  \ndois"), "um  \ndois");
    }

    #[test]
    fn resumo_do_grupo_longo_e_curto() {
        let mut editar = ferramenta("Edit", "Edit · views.py", Some("ok"), false);
        if let crate::modelo::Item::Ferramenta { arquivo, .. } = &mut editar {
            *arquivo = Some("produtos/views.py".into());
        }
        let ler = ferramenta("Read", "Read · views.py", Some("ok"), false);
        let rodar = ferramenta("Bash", "Bash · cargo test", Some("ok"), false);
        let itens = [rodar.clone(), rodar, editar, ler.clone(), ler.clone(), ler];
        let chamadas: Vec<(usize, &crate::modelo::Item)> = itens.iter().enumerate().collect();
        // Até dois arquivos, o nome deles no lugar da contagem (rodada de uso, item 5.1).
        assert_eq!(resumo_das_chamadas(&chamadas, false), "Rodou 2 comandos · editou views.py · leu 3 arquivos");
        assert_eq!(resumo_das_chamadas(&chamadas, true), "2 comandos · editou views.py · 3 lidos");
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

    #[test]
    fn a_conversa_abre_no_comeco_da_resposta_que_nao_cabe() {
        use super::{AlvoDeAbertura, Abertura, onde_abrir};
        use gpui_kit::px;
        // Blocos: a mensagem (0), a linha do turno (1) e a resposta (5), numa lista de 900.
        let posicoes = [(0, px(0.), px(40.)), (1, px(72.), px(104.)), (5, px(116.), px(900.))];
        // A resposta (do 72 ao 900) não cabe em 400: abre no começo dela, com o respiro de 12.
        assert_eq!(onde_abrir(AlvoDeAbertura::InicioDaResposta, &posicoes, px(900.), px(400.), Some(1), 6), Abertura::Em(px(60.)));
        // Cabe: abre no fim, como sempre.
        assert_eq!(onde_abrir(AlvoDeAbertura::InicioDaResposta, &posicoes, px(900.), px(1000.), Some(1), 6), Abertura::Fim);
        // Sem resposta depois da sua mensagem: no fim.
        assert_eq!(onde_abrir(AlvoDeAbertura::InicioDaResposta, &posicoes, px(900.), px(400.), None, 6), Abertura::Fim);
    }

    #[test]
    fn a_ancora_volta_no_mesmo_item_e_carrega_mais_quando_falta() {
        use super::{AlvoDeAbertura, Abertura, Ancora, ancora_de, onde_abrir};
        use gpui_kit::px;
        let posicoes = [(10, px(0.), px(100.)), (12, px(112.), px(400.)), (20, px(412.), px(500.))];
        // O alto da tela no 150: o bloco 12 (que vai até o 400) estava 38 abaixo do alto dele.
        let ancora = ancora_de(&posicoes, px(150.)).unwrap();
        assert_eq!(ancora, Ancora { item: 12, desloc: px(38.) });
        assert_eq!(onde_abrir(AlvoDeAbertura::Ancora(ancora), &posicoes, px(500.), px(200.), None, 30), Abertura::Em(px(150.)));
        // O item ficou dentro de uma trilha que começa antes: abre no começo dela.
        let dentro = Ancora { item: 15, desloc: px(10.) };
        assert_eq!(onde_abrir(AlvoDeAbertura::Ancora(dentro), &posicoes, px(500.), px(200.), None, 30), Abertura::Em(px(112.)));
        // Mais para cima do que está desenhado: carrega mais itens antes.
        let antes = Ancora { item: 3, desloc: px(0.) };
        assert_eq!(onde_abrir(AlvoDeAbertura::Ancora(antes), &posicoes, px(500.), px(200.), None, 30), Abertura::CarregarMais);
        // O Corrigir tirou o item: abre no fim.
        let sumiu = Ancora { item: 40, desloc: px(0.) };
        assert_eq!(onde_abrir(AlvoDeAbertura::Ancora(sumiu), &posicoes, px(500.), px(200.), None, 30), Abertura::Fim);
    }

    #[test]
    fn separador_novo_e_comeco_da_resposta() {
        use super::{Item, inicio_da_ultima_resposta, item_do_separador_novo};
        let com_hora = |item: Item, em: u64| match item {
            Item::Usuario { texto, anexos, ultrathink, ultracode, .. } => Item::Usuario { texto, anexos, ultrathink, ultracode, em },
            Item::Assistente { texto, .. } => Item::Assistente { texto, em },
            outro => outro,
        };
        let itens = vec![
            com_hora(usuario("oi"), 100),
            com_hora(assistente("olá"), 110),
            com_hora(usuario("e agora?"), 200),
            com_hora(assistente("feito"), 260),
        ];
        // Lido até o 150: a sua mensagem das 200 não conta, o separador fica antes da resposta.
        assert_eq!(item_do_separador_novo(&itens, 150), Some(3));
        assert_eq!(item_do_separador_novo(&itens, 300), None);
        assert_eq!(inicio_da_ultima_resposta(&itens), Some(3));
        assert_eq!(inicio_da_ultima_resposta(&itens[..3]), None);
        assert_eq!(inicio_da_ultima_resposta(&[]), None);
    }

    #[test]
    fn turno_pronto_recolhe_ate_a_resposta_final() {
        use super::{Item, TurnoRecolhivel, fica_fora_do_recolhido, resumo_do_turno, turnos_recolhiveis};
        let em = |item: Item, t: u64| match item {
            Item::Usuario { texto, anexos, ultrathink, ultracode, .. } => Item::Usuario { texto, anexos, ultrathink, ultracode, em: t },
            Item::Assistente { texto, .. } => Item::Assistente { texto, em: t },
            outro => outro,
        };
        let editar = |arquivo: &str| Item::Ferramenta {
            id: "e".into(),
            nome: "Edit".into(),
            resumo: arquivo.into(),
            resultado: Some("ok".into()),
            erro: false,
            em: 0,
            detalhe: None,
            arquivo: Some(format!("src/{arquivo}")),
            sub: None,
            mudanca: None,
        };
        let itens = vec![
            em(usuario("faz"), 1_000),
            assistente("Vou olhar."),
            ferramenta("Bash", "cargo test", Some("ok"), false),
            editar("carrinho.py"),
            assistente("Quer que eu suba?"),
            editar("motor.rs"),
            ferramenta("Bash", "cargo check", Some("ok"), false),
            em(assistente("Pronto."), 1_372),
        ];
        let turnos = turnos_recolhiveis(&itens, false);
        assert_eq!(turnos, [TurnoRecolhivel { mensagem: 0, resposta: 7 }]);
        assert!(turnos[0].contem(1) && turnos[0].contem(6) && !turnos[0].contem(7) && !turnos[0].contem(0));
        assert_eq!(resumo_do_turno(&itens, turnos[0]), "Trabalhou 6 min 12 s · 2 comandos · editou carrinho.py e motor.rs");
        // Com o turno ainda rodando, o último não recolhe.
        assert!(turnos_recolhiveis(&itens, true).is_empty());
        // Sem texto intermediário (só a trilha e a resposta), não recolhe: a trilha já basta.
        let curto = vec![usuario("faz"), ferramenta("Bash", "ls", Some("ok"), false), assistente("Feito.")];
        assert!(turnos_recolhiveis(&curto, false).is_empty());
        // A pergunta do meio e o erro ficam à vista.
        assert!(fica_fora_do_recolhido(&itens[4]));
        assert!(!fica_fora_do_recolhido(&itens[1]));
        assert!(fica_fora_do_recolhido(&Item::Aviso { texto: "O turno terminou com erro: x".into(), erro: true, em: 0 }));
        // Sem hora, sem número.
        let sem_hora: Vec<Item> = itens.iter().map(|i| match i {
            Item::Usuario { texto, .. } => usuario(texto),
            Item::Assistente { texto, .. } => assistente(texto),
            outro => outro.clone(),
        }).collect();
        assert!(resumo_do_turno(&sem_hora, turnos[0]).starts_with("Trabalhou · 2 comandos"));
    }

    #[test]
    fn duracao_do_turno_em_segundos_minutos_e_horas() {
        use super::duracao_do_turno;
        assert_eq!(duracao_do_turno(45), "45 s");
        assert_eq!(duracao_do_turno(360), "6 min");
        assert_eq!(duracao_do_turno(372), "6 min 12 s");
        assert_eq!(duracao_do_turno(3600), "1 h");
        assert_eq!(duracao_do_turno(3780), "1 h 3 min");
    }

    #[test]
    fn na_dupla_cada_agente_tem_o_seu_recolhido() {
        use super::{Agente, Item, turnos_recolhiveis};
        let itens = vec![
            usuario("faz"),
            Item::Vez { agente: Agente::Claude, em: 0 },
            assistente("a"),
            ferramenta("Bash", "ls", Some("ok"), false),
            assistente("pronto"),
            Item::Vez { agente: Agente::Codex, em: 0 },
            assistente("b"),
            ferramenta("Bash", "ls", Some("ok"), false),
            assistente("concordo"),
        ];
        let turnos: Vec<(usize, usize)> = turnos_recolhiveis(&itens, false).iter().map(|t| (t.mensagem, t.resposta)).collect();
        assert_eq!(turnos, [(1, 4), (5, 8)]);
    }

    #[test]
    fn revisao_da_memoria_vira_uma_linha_pela_ferramenta() {
        use super::{Item, resumo_da_revisao, turnos_recolhiveis};
        let editar = |arquivo: &str| Item::Ferramenta {
            id: "e".into(),
            nome: "Edit".into(),
            resumo: arquivo.into(),
            resultado: Some("ok".into()),
            erro: false,
            em: 0,
            detalhe: None,
            arquivo: Some(arquivo.into()),
            sub: None,
            mudanca: None,
        };
        let pedido = Item::Entrega { de: "Vitral".into(), texto: crate::motor::PEDIDO_DE_REVISAO.into(), em: 0 };
        let nada = vec![pedido.clone(), ferramenta("Read", "AGENTS.md", Some("ok"), false), assistente("Não há nada a registrar.")];
        assert_eq!(resumo_da_revisao(&nada).0, "Memória revista · nada a guardar");
        let com = vec![pedido.clone(), ferramenta("Read", "AGENTS.md", Some("ok"), false), editar("/x/AGENTS.md"), assistente("Anotei.")];
        let (texto, caminhos) = resumo_da_revisao(&com);
        assert_eq!(texto, "Memória: editou AGENTS.md");
        assert_eq!(caminhos, ["/x/AGENTS.md"]);
        // A revisão não entra no "Trabalhou…" (ela tem a linha dela).
        let mut itens = vec![usuario("faz"), assistente("a"), ferramenta("Bash", "ls", Some("ok"), false), assistente("pronto")];
        itens.extend(vec![pedido, ferramenta("Read", "x", Some("ok"), false), assistente("olhei"), editar("AGENTS.md"), assistente("Anotei.")]);
        let turnos: Vec<usize> = turnos_recolhiveis(&itens, false).iter().map(|t| t.mensagem).collect();
        assert_eq!(turnos, [0]);
    }

    #[test]
    fn mudancas_do_chat_neste_turno_e_antes() {
        use super::{Item, mudancas_do_chat, total_das_mudancas};
        use crate::modelo::Mudanca;
        let editar = |arquivo: &str, entram: u32, saem: u32, novo: bool| Item::Ferramenta {
            id: "e".into(),
            nome: if novo { "Write".into() } else { "Edit".into() },
            resumo: arquivo.into(),
            resultado: Some("ok".into()),
            erro: false,
            em: 0,
            detalhe: None,
            arquivo: Some(arquivo.into()),
            sub: None,
            mudanca: Some(Mudanca { entram, saem, novo }),
        };
        let itens = vec![
            usuario("um"),
            editar("src/a.rs", 3, 1, false),
            editar("src/b.rs", 2, 0, false),
            assistente("ok"),
            usuario("dois"),
            editar("src/a.rs", 1, 1, false),
            editar("docs/c.md", 10, 0, true),
            editar("src/a.rs", 4, 0, false),
            assistente("pronto"),
        ];
        let (neste, antes) = mudancas_do_chat(&itens);
        let nomes = |l: &[super::ArquivoMudado]| l.iter().map(|a| (a.caminho.clone(), a.entram, a.saem, a.novo)).collect::<Vec<_>>();
        assert_eq!(nomes(&neste), [("src/a.rs".to_string(), 5, 1, false), ("docs/c.md".to_string(), 10, 0, true)]);
        // O que mudou de novo neste turno sai de "Antes".
        assert_eq!(nomes(&antes), [("src/b.rs".to_string(), 2, 0, false)]);
        assert_eq!(total_das_mudancas(&neste), (2, 15, 1));
    }

    #[test]
    fn resumo_da_trilha_diz_o_nome_ate_dois_arquivos() {
        use super::{Item, resumo_das_chamadas};
        let editar = |arquivo: &str| Item::Ferramenta {
            id: "e".into(),
            nome: "Edit".into(),
            resumo: arquivo.into(),
            resultado: Some("ok".into()),
            erro: false,
            em: 0,
            detalhe: None,
            arquivo: Some(format!("src/{arquivo}")),
            sub: None,
            mudanca: None,
        };
        let dois = [editar("carrinho.py"), editar("motor.rs"), ferramenta("Bash", "ls", Some("ok"), false)];
        let chamadas: Vec<(usize, &Item)> = dois.iter().enumerate().collect();
        assert_eq!(resumo_das_chamadas(&chamadas, false), "Rodou 1 comando · editou carrinho.py e motor.rs");
        assert_eq!(resumo_das_chamadas(&chamadas, true), "1 comando · editou carrinho.py e motor.rs");
        let tres = [editar("a.rs"), editar("b.rs"), editar("c.rs")];
        let chamadas: Vec<(usize, &Item)> = tres.iter().enumerate().collect();
        assert_eq!(resumo_das_chamadas(&chamadas, false), "Editou 3 arquivos");
        assert_eq!(resumo_das_chamadas(&chamadas, true), "3 editados");
    }
}
