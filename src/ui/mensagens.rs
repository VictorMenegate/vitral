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
    tooltip::{ManagedTooltipExt as _, Tooltip},
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
    edicao::LinhaDoDiff,
    imagens,
    movimento::{self, De},
    paineis::tamanho_da_conversa,
};
use crate::{
    modelo::{Agente, Chat, Execucao, Item, Reforco},
    motor::Motor,
    tema::{
        Cores, FONTE_MONO, Lucide, Tipo as _, aresta_de_poco, dentro_da_borda, esp, halo_foco, linear, r, raio,
    },
};

pub(super) mod a_vista;
pub(super) mod historico;
pub(super) mod no_fundo;

use a_vista::{Assinatura, ChaveDoBloco, Medida, TipoDaChave, Vista};
mod erros;
mod revisao;

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
pub(super) fn larga(nivel: LarguraPainel) -> bool {
    nivel == LarguraPainel::Largo
}

pub(super) fn minima(nivel: LarguraPainel) -> bool {
    matches!(nivel, LarguraPainel::Estreito | LarguraPainel::Minimo)
}

/// A folga dos lados do balão do usuário: 12, e 10 no painel mínimo (eram 16; dono, 17/11/2026:
/// "diminui o padding disso"). O texto do balão e a barra de ações em cima dele medem daqui.
fn folga_do_balao(nivel: LarguraPainel) -> f32 {
    // 11 dos lados, a do protótipo Linear (25/11/2026); 10 no painel mínimo.
    if minima(nivel) { 10. } else { 11. }
}

/// Fundos da linha que sai e da que entra num trecho de código.
pub(super) fn fundo_sai() -> Hsla {
    rgba(0xf8717114).into()
}

pub(super) fn fundo_entra() -> Hsla {
    rgba(0x4ade8014).into()
}

/// O fundo do trecho trocado dentro da linha que sai e da que entra: o mesmo tom da linha, mais
/// forte, para a palavra se ver sem ler as duas linhas inteiras.
fn fundo_do_trecho(entra: bool) -> HighlightStyle {
    let cor: Hsla = if entra { rgba(0x4ade8047).into() } else { rgba(0xf8717147).into() };
    HighlightStyle { background_color: Some(cor), ..Default::default() }
}

/// O tipo de cada bloco da conversa, para o espaço entre eles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::ui) enum Bloco {
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

/// Um bloco da conversa antes de montar: o que já se sabe dele e como montá-lo. O bloco longe da
/// vista que já foi medido não é montado; entra um espaço da altura dele (ver `a_vista`).
pub(in crate::ui) struct Planejado<'a> {
    /// O tipo, quando se sabe sem montar (o de um item sai da montagem ou da altura guardada).
    bloco: Option<Bloco>,
    /// O índice do (primeiro) item dele: identifica a entrada, a posição e o realce do sumário.
    indice: Option<usize>,
    /// A identidade para a altura guardada; `None`: sempre montado, e sem altura.
    chave: Option<ChaveDoBloco>,
    /// O bloco de um texto longo, que o kit lê em segundo plano: a altura dele só vale depois de
    /// assentar, e ao voltar à vista a caixa fica nela até o texto chegar (`a_vista::Alturas`).
    fica_montado: bool,
    sem_entrada: bool,
    /// No separador "Novo": o índice do bloco que vem logo depois dele.
    antes_de: Option<usize>,
    /// Numa mensagem sua do histórico trazido: o texto dela, para a mensagem presa no alto.
    presa: Option<String>,
    montar: Box<dyn FnOnce() -> (Bloco, AnyElement) + 'a>,
}

impl<'a> Planejado<'a> {
    fn novo(indice: Option<usize>, chave: Option<ChaveDoBloco>, montar: impl FnOnce() -> (Bloco, AnyElement) + 'a) -> Self {
        Planejado { bloco: None, indice, chave, fica_montado: false, sem_entrada: false, antes_de: None, presa: None, montar: Box::new(montar) }
    }

    /// Um bloco de tipo sabido antes de montar.
    fn de_tipo(bloco: Bloco, indice: Option<usize>, chave: Option<ChaveDoBloco>, montar: impl FnOnce() -> AnyElement + 'a) -> Self {
        Planejado { bloco: Some(bloco), ..Self::novo(indice, chave, move || (bloco, montar())) }
    }

    /// Já montado, e sempre montado: o que não tem altura a guardar (a dica, o texto chegando).
    fn pronto(bloco: Bloco, indice: Option<usize>, elemento: AnyElement) -> Self {
        Self::de_tipo(bloco, indice, None, move || elemento)
    }

    fn sem_entrada(mut self) -> Self {
        self.sem_entrada = true;
        self
    }

    /// O bloco de um texto longo (`texto_que_chega_depois`).
    fn fica_montado(mut self, fica: bool) -> Self {
        self.fica_montado = fica;
        self
    }
}

/// O que um item mostra, em números: o que muda a altura do bloco dele.
fn partes_do_item(item: &Item, partes: &mut Vec<u64>) {
    let n = |texto: &str| texto.len() as u64;
    match item {
        Item::Usuario { texto, anexos, .. } => partes.extend([1, n(texto), anexos.len() as u64]),
        Item::Entrega { de, texto, .. } => partes.extend([2, n(de), n(texto)]),
        Item::Assistente { texto, .. } => partes.extend([3, n(texto)]),
        Item::Ferramenta { resumo, resultado, erro, detalhe, sub, mudanca, imagens, .. } => {
            partes.extend([4, n(resumo), resultado.as_deref().map_or(0, |r| 1 + n(r)), u64::from(*erro)]);
            // As miniaturas das imagens que o resultado trouxe chegam depois dele e mudam a altura.
            partes.extend([detalhe.as_deref().map_or(0, |d| 1 + n(d)), u64::from(mudanca.is_some()), imagens.len() as u64]);
            if let Some(sub) = sub {
                partes.extend([u64::from(sub.ferramentas), sub.passos.len() as u64, n(&sub.atividade), sub.fim]);
            }
        }
        Item::Raciocinio { texto, milis, .. } => partes.extend([5, n(texto), *milis]),
        Item::Aviso { texto, erro, .. } => partes.extend([6, n(texto), u64::from(*erro)]),
        Item::Cartao { id, .. } => partes.extend([7, *id]),
        Item::Vez { .. } => partes.push(8),
        Item::Desconhecido(_) => partes.push(9),
        Item::FimDoAgente { chamada, descricao, erro, durou, .. } => partes.extend([10, n(chamada), n(descricao), u64::from(*erro), *durou]),
        // Mandada, a ação do cartão vira uma linha de texto.
        Item::Revisao { texto, mandada, .. } => partes.extend([11, n(texto), u64::from(*mandada)]),
    }
    partes.push(item.em());
}

/// A marca do conteúdo de uma faixa de itens, com o que mais o bloco deles mostra (`extras`).
fn marca_da_faixa<'i>(itens: impl IntoIterator<Item = &'i Item>, extras: &[u64]) -> u64 {
    let mut partes = extras.to_vec();
    for item in itens {
        partes_do_item(item, &mut partes);
    }
    a_vista::marca(&partes)
}

/// Acima deste tamanho, o kit lê o markdown em segundo plano (4 KB; aqui com folga, porque o
/// texto ganha as quebras duras e os links antes de ir para ele): o primeiro desenho do bloco sai
/// sem o texto, e a altura medida nele não é a de verdade.
const TEXTO_LIDO_NA_HORA: usize = 3000;

/// O item tem um texto longo, que o kit lê em segundo plano: o primeiro desenho do bloco sai sem
/// o texto. Por isso a altura dele só vale depois de ficar igual por um tempo
/// (`a_vista::Alturas::assentada`), e, quando ele volta à vista depois de ter sido espaço, a caixa
/// fica na altura guardada até o texto relido chegar (`Alturas::segurar`): sem isso ele encolhia
/// e crescia na frente de quem lê.
pub(super) fn texto_que_chega_depois(item: &Item) -> bool {
    match item {
        Item::Usuario { texto, .. }
        | Item::Assistente { texto, .. }
        | Item::Entrega { texto, .. }
        | Item::Aviso { texto, .. }
        | Item::Raciocinio { texto, .. }
        | Item::Revisao { texto, .. } => texto.len() > TEXTO_LIDO_NA_HORA,
        // O relatório de um subagente abre em markdown.
        Item::Ferramenta { resultado, sub, .. } => sub.is_some() && resultado.as_deref().is_some_and(|r| r.len() > TEXTO_LIDO_NA_HORA),
        Item::Cartao { .. } | Item::Vez { .. } | Item::FimDoAgente { .. } | Item::Desconhecido(_) => false,
    }
}

/// A chave do bloco de um item sozinho. O cartão de erro e o cartão interativo não têm: o que
/// eles mostram muda com o estado do turno e das contas, e são poucos; ficam sempre montados e
/// sem altura guardada.
fn chave_do_item(tipo: TipoDaChave, indice: usize, item: &Item, extras: &[u64]) -> Option<ChaveDoBloco> {
    match item {
        Item::Aviso { texto, erro, .. } if *erro || tipo_aviso(texto, *erro) == ClasseAvisoChat::Alerta => None,
        Item::Cartao { .. } => None,
        _ => Some(ChaveDoBloco { tipo, inicio: indice, fim: indice + 1, marca: marca_da_faixa([item], extras) }),
    }
}

/// A chave de um bloco que junta vários itens (a trilha).
fn chave_da_faixa(tipo: TipoDaChave, itens: &[(usize, &Item)], extras: &[u64]) -> Option<ChaveDoBloco> {
    let (primeiro, ultimo) = (itens.first()?.0, itens.last()?.0);
    Some(ChaveDoBloco { tipo, inicio: primeiro, fim: ultimo + 1, marca: marca_da_faixa(itens.iter().map(|(_, item)| *item), extras) })
}

/// O bloco destes itens tem texto longo (ver `texto_que_chega_depois`).
fn fica_montado<'i>(itens: impl IntoIterator<Item = &'i Item>) -> bool {
    itens.into_iter().any(texto_que_chega_depois)
}

/// Espaço antes de um bloco, numa escala de três passos (18/11/2026; medidas do protótipo Linear
/// desde 25/11/2026): 10 entre as falas (o seu balão, a resposta, o dia), 7 dentro da resposta e
/// nada entre linhas da trilha, que ficam coladas no passo de 22. Com mais de 2 vezes de fora para dentro, o olho agrupa a pergunta com a resposta
/// (relatório de aparência, 16/11/2026, V3). Saíram os 16 em volta do marco e os 4 entre entregas:
/// eram dois passos a mais para o mesmo "dentro do turno". O fim da lista guarda os mesmos 32 de
/// entre turnos: a caixa de mensagem começa o próximo.
fn espaco_antes(anterior: Option<Bloco>, atual: Bloco) -> Rems {
    use Bloco::*;
    let Some(anterior) = anterior else {
        return r(0.);
    };
    match (anterior, atual) {
        // Linear (25/11/2026): 10 entre as falas (o dia, o seu balão e a resposta) e 7 dentro
        // da resposta, os vãos do protótipo.
        (_, Dia) | (Dia, _) | (_, Balao) | (Balao, _) => esp::ENTRE_TURNOS,
        (Log, Log) => r(0.),
        _ => esp::NO_TURNO,
    }
}

/// A entrada de um bloco da conversa (E5): nenhuma para o que já estava nela quando abriu no
/// painel (`base`) ou para o que não tem índice; para o resto, `Some(nova)`, com `nova` só no
/// quadro em que o bloco aparece (`vistos`). Trocar de conversa ou rolar não anima nada.
fn entrada_do_bloco(indice: Option<usize>, base: usize, vistos: usize) -> Option<bool> {
    indice.filter(|i| *i >= base).map(|i| i >= vistos)
}

/// A entrada de uma chamada dentro de uma trilha: a primeira entra com o bloco (que leva o índice
/// dela); cada uma que chega depois, numa trilha que já estava à vista, entra sozinha, com a mesma
/// regra dos blocos. Como no bloco, o `Some` fica enquanto ela está à vista (o estado da entrada
/// não pode sumir no meio dela), e o `true` só no desenho em que ela aparece.
fn entrada_da_chamada(primeira: bool, indice: usize, base: usize, vistos: usize) -> Option<bool> {
    if primeira { None } else { entrada_do_bloco(Some(indice), base, vistos) }
}

/// O cabeçalho de um grupo da trilha nasce com a segunda chamada: entra com ela. Se a trilha inteira
/// é nova neste desenho, quem entra é o bloco, e o cabeçalho vem junto.
fn entrada_do_cabecalho(indices: &[usize], base: usize, vistos: usize) -> Option<bool> {
    match indices {
        [primeira, segunda, ..] if *segunda >= base && *primeira < vistos => Some(*segunda >= vistos),
        _ => None,
    }
}

/// O chevron de abrir e recolher: para a direita fechado, para baixo aberto, e gira 90° em 120 ms
/// quando o clique troca (montado, já está no lugar; parado, é o ícone de sempre, sem giro).
pub(super) fn chevron(chave: &str, aberto: bool) -> movimento::ComEstado {
    let id = SharedString::from(format!("gira-{chave}"));
    movimento::com_estado(move |window, cx| {
        let quarto = movimento::valor(id, if aberto { 1f32 } else { 0. }, movimento::COR, movimento::Curva::Mover, window, cx);
        if quarto >= 1. {
            icone_de(Lucide::ChevronDown, 12.).into_any_element()
        } else if quarto <= 0. {
            icone_de(Lucide::ChevronRight, 12.).into_any_element()
        } else {
            icone_de(Lucide::ChevronRight, 12.).rotate(radians(quarto * std::f32::consts::FRAC_PI_2)).into_any_element()
        }
    })
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

/// O contexto de teclado da área da conversa: uma letra digitada com o foco aqui dentro (fora de um
/// campo) vai para a caixa (`Janela::tecla_na_janela`, item 1.1).
pub(super) const CONTEXTO_DA_CONVERSA: &str = "Conversa";

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
    novo: Option<(usize, Pixels)>,
    altura: Pixels,
    vista: Pixels,
    inicio: Option<usize>,
    itens: usize,
) -> Abertura {
    match alvo {
        AlvoDeAbertura::InicioDaResposta => {
            let Some(&(primeiro, alto, _)) = inicio.and_then(|inicio| posicoes.iter().find(|(i, ..)| *i >= inicio)) else {
                return Abertura::Fim;
            };
            // O separador "Novo" logo antes do começo da resposta fica à vista no alto: a abertura
            // conta a partir dele (`novo`: o índice do bloco que vem depois dele e o alto dele).
            let alto = match novo {
                Some((seguinte, alto_do_novo)) if seguinte == primeiro => alto_do_novo.min(alto),
                _ => alto,
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
        let foco = motor.config.modo_foco;
        let nivel = self.paineis[ix].medida.nivel;
        let mut contexto = Contexto::novo(motor, chat, nivel, painel_id, &self.paineis[ix].busca.indice, &rolagem);
        contexto.entrada = (self.paineis[ix].base_animacao, self.paineis[ix].vistos);
        let agora = crate::modelo::agora();
        // Conversas antigas guardaram as buscas de ferramenta; não interessam. No modo foco as
        // ferramentas ficam, recolhidas na linha do turno (`turnos_do_foco`).
        // O que o outro agente faz enquanto revê o turno fica fora da conversa: no fim vira o
        // cartão da revisão (ou a linha "revisou: nada a acrescentar").
        let fim_a_vista = motor.revisao_em_curso_desde(&chat.id).unwrap_or(usize::MAX);
        let mut visiveis: Vec<(usize, &Item)> = chat
            .itens
            .iter()
            .enumerate()
            .take_while(|(i, _)| *i < fim_a_vista)
            .filter(|(i, item)| match item {
                Item::Ferramenta { nome, .. } => nome != "ToolSearch",
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
        // As mensagens do histórico que podem ficar presas no alto: só as deste quadro. Sem isto, o
        // painel que trocava de uma conversa trazida para outra guardava as da anterior, e uma
        // mensagem de outro chat aparecia presa no alto (visto na bancada, 18/11/2026).
        self.paineis[ix].presa_do_historico.borrow_mut().clear();
        if escondidos > 0 {
            visiveis.drain(..escondidos);
        }
        // A conversa trazida do Claude Code: o que foi dito lá vem antes, lido do arquivo dela. O
        // que foi lido, a busca vazia e o contexto dela ficam aqui, à frente dos blocos, que só
        // são montados depois de decidido quais estão à vista.
        let lido = chat.importada.as_ref().filter(|_| escondidos == 0).map(|importada| (importada, self.historico_de(chat, importada, cx)));
        let sem_busca = busca_conversa::Indice::default();
        let contexto_do_historico = Contexto {
            motor,
            chat,
            rodando: false,
            nivel,
            // O histórico trazido do Claude Code é só para ler: nada dele se corrige aqui.
            painel: painel_id,
            editavel: None,
            marco: None,
            // A busca da conversa não varre o histórico do Claude Code: ele é lido do arquivo, sob
            // demanda, e nem todo ele está em memória.
            busca: &sem_busca,
            rolagem: &rolagem,
            entrada: (usize::MAX, usize::MAX),
        };
        // Daqui até a montagem, a Janela só é lida: os blocos guardam como se montar.
        let (eu, ctx) = (&*self, &contexto);
        // Cada bloco com o índice do (primeiro) item dele, que identifica a entrada.
        let mut blocos: Vec<Planejado> = Vec::new();
        if chat.itens.is_empty() && chat.importada.is_none() {
            blocos.push(Planejado::pronto(Bloco::Texto, None, eu.dica_inicial(motor, chat, window, cx)));
        }
        if let Some((importada, lido)) = &lido {
            blocos.extend(eu.blocos_do_historico(&contexto_do_historico, importada, lido, ix, window, cx));
        }
        // O que mais muda a cara de um bloco além dos itens dele: o turno em curso.
        let n_itens_da_conversa = chat.itens.len();
        let extras_de = |indice: usize| [u64::from(indice + 1 == n_itens_da_conversa), u64::from(contexto.rodando)];
        // O dia de cada turno: o separador entra onde a data muda (pela posição em `visiveis`).
        let horas: Vec<u64> = visiveis.iter().map(|(_, item)| item.em()).collect();
        let datas: HashMap<usize, String> = separadores_de_data(&horas, agora).into_iter().collect();
        // O separador "Novo", antes do primeiro bloco que tem o primeiro item que você não viu
        // (quando ele está carregado; numa trilha, antes dela).
        let mut novo = self.paineis[ix]
            .novo_desde
            .and_then(|lido| item_do_separador_novo(&chat.itens, lido))
            .filter(|n| escondidos == 0 || visiveis.first().is_some_and(|(i, _)| i <= n));
        // Os turnos prontos recolhem numa linha "Trabalhou…" (rodada de uso, 2.1). No modo foco,
        // todo turno, também o que roda, vira uma linha só (`turnos_do_foco`).
        let turnos = turnos_da_conversa(&chat.itens, foco, contexto.rodando);
        let mut turno_posto: Option<usize> = None;
        // Os recolhidos ("Trabalhou…", a revisão da memória) que um clique acabou de abrir: os blocos
        // de dentro entram com fade e 6 px de cima, uma vez (o primeiro índice e o fim, sem eles).
        let agora_mesmo_aberto = std::time::Instant::now();
        let mut abrindo: Vec<(usize, usize)> = Vec::new();
        // O separador "Novo" leva o índice do bloco logo depois dele: a abertura no começo da
        // resposta o deixa à vista no alto (conferência, 16/11/2026).
        let separador_do_novo = |seguinte: usize| {
            let chave = ChaveDoBloco { tipo: TipoDaChave::Novo, inicio: seguinte, fim: seguinte, marca: 0 };
            Planejado { antes_de: Some(seguinte), ..Planejado::de_tipo(Bloco::Dia, None, Some(chave), separador_novo) }
        };
        let mut i = 0;
        while i < visiveis.len() {
            let (indice, item) = visiveis[i];
            if let Some(dia) = datas.get(&i) {
                let chave = ChaveDoBloco { tipo: TipoDaChave::Dia, inicio: indice, fim: indice, marca: dia.len() as u64 };
                let dia = dia.clone();
                blocos.push(Planejado::de_tipo(Bloco::Dia, None, Some(chave), move || separador_de_dia(&dia)));
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
                if aberta && self.abertos_agora.recente(&chave, agora_mesmo_aberto) {
                    abrindo.push((indice, fim_item));
                }
                let rodando = fim_item == chat.itens.len() && motor.revendo_memoria(&chat.id);
                let itens_da_revisao = &chat.itens[indice..fim_item];
                let chave_do_bloco = ChaveDoBloco {
                    tipo: TipoDaChave::Memoria,
                    inicio: indice,
                    fim: fim_item,
                    marca: marca_da_faixa(itens_da_revisao, &[u64::from(rodando), u64::from(aberta)]),
                };
                blocos.push(
                    Planejado::de_tipo(Bloco::Log, Some(indice), Some(chave_do_bloco), move || {
                        eu.linha_da_memoria(ctx, &chave, itens_da_revisao, rodando, aberta, window, cx)
                    })
                    // A linha não entra com fade: o primeiro item dela, aberta, já usa o índice.
                    .sem_entrada(),
                );
                if !aberta {
                    i = visiveis[i..].iter().position(|(k, _)| *k >= fim_item).map_or(visiveis.len(), |p| i + p);
                    continue;
                }
            }
            if let Some(turno) = turnos.iter().copied().find(|t| t.contem(indice)).filter(|t| turno_posto != Some(t.mensagem)) {
                turno_posto = Some(turno.mensagem);
                let aberto = self.expandidos.contains(&turno.chave(&chat.id));
                if aberto && self.abertos_agora.recente(&turno.chave(&chat.id), agora_mesmo_aberto) {
                    abrindo.push((turno.mensagem, turno.resposta));
                }
                let fim = visiveis[i..].iter().position(|(k, _)| *k >= turno.resposta).map_or(visiveis.len(), |p| i + p);
                // O separador "Novo" de um item lá dentro fica antes da linha.
                if novo.is_some_and(|n| n < turno.resposta) {
                    novo = None;
                    blocos.push(separador_do_novo(indice));
                }
                let do_turno = &visiveis[i..fim];
                let chave_do_bloco = ChaveDoBloco {
                    tipo: TipoDaChave::Turno,
                    inicio: turno.mensagem,
                    fim: turno.resposta,
                    marca: marca_da_faixa(do_turno.iter().map(|(_, item)| *item), &[u64::from(aberto), do_turno.len() as u64]),
                };
                blocos.push(
                    Planejado::de_tipo(Bloco::Log, Some(indice), Some(chave_do_bloco), move || {
                        eu.linha_do_turno(ctx, turno, aberto, do_turno, cx)
                    })
                    // A linha do turno não entra com fade: o primeiro item dele, aberto, já usa o índice.
                    .sem_entrada(),
                );
                if !aberto {
                    // Recolhido, só o que não pode sumir: a pergunta, o aviso de erro e o cartão.
                    for &(k, escondido) in do_turno {
                        if let Some(plano) = plano_na_conversa(escondido) {
                            // O cartão do plano abre e fecha: sempre montado, sem altura guardada.
                            blocos.push(Planejado::pronto(Bloco::Texto, Some(k), eu.plano_respondido(ctx, k, plano, cx)));
                        } else if fica_fora_do_recolhido(escondido) {
                            let chave = chave_do_item(TipoDaChave::Item, k, escondido, &extras_de(k));
                            blocos.push(
                                Planejado::novo(Some(k), chave, move || eu.item(ctx, k, escondido, window, cx)).fica_montado(fica_montado([escondido])),
                            );
                        }
                    }
                    i = fim;
                    continue;
                }
            }
            if novo.is_some_and(|n| indice >= n) {
                novo = None;
                blocos.push(separador_do_novo(indice));
            }
            // O plano fica na conversa, no ponto em que foi proposto: um cartão recolhido que diz
            // o que você respondeu e abre o texto dele. Enquanto espera a resposta, quem aparece é
            // o pedido, logo abaixo das mensagens.
            if let Some(plano) = plano_na_conversa(item) {
                i += 1;
                if !plano_esperando(motor, &chat.id, item) {
                    blocos.push(Planejado::pronto(Bloco::Texto, Some(indice), eu.plano_respondido(ctx, indice, plano, cx)));
                }
                continue;
            }
            // O raciocínio faz parte da trilha: fica no meio das chamadas, sem parti-la.
            if matches!(item, Item::Ferramenta { .. } | Item::Raciocinio { .. }) {
                let mut fim = i + 1;
                // Uma trilha que passa da meia-noite se parte: o separador do dia novo entra nela.
                // O cartão do plano também a parte.
                while fim < visiveis.len()
                    && matches!(visiveis[fim].1, Item::Ferramenta { .. } | Item::Raciocinio { .. })
                    && plano_na_conversa(visiveis[fim].1).is_none()
                    && !datas.contains_key(&fim)
                {
                    fim += 1;
                }
                let ao_vivo = contexto.rodando && fim == visiveis.len();
                let chamadas = &visiveis[i..fim];
                let chave_do_bloco =
                    chave_da_faixa(TipoDaChave::Trilha, chamadas, &[u64::from(ao_vivo), chamadas.len() as u64, u64::from(contexto.rodando)]);
                blocos.push(
                    Planejado::de_tipo(Bloco::Log, Some(indice), chave_do_bloco, move || eu.trilha(ctx, chamadas, ao_vivo, cx))
                        .fica_montado(fica_montado(chamadas.iter().map(|(_, item)| *item))),
                );
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
                // Sem altura guardada: o cartão de erro muda com o turno e as contas (`chave_do_item`).
                blocos.push(Planejado::de_tipo(Bloco::Alerta, Some(indice), None, move || {
                    eu.cartao_de_erro(ctx, &chave, indice_erro, texto, Some((indice, antes.as_str())), window, cx)
                }));
                i += 2;
                continue;
            }
            // A marca que só fecha a vez de um agente (a dupla que nasceu pela troca de agente ou
            // pelo @) não tem nada dele depois: não aparece, senão sobrava um "Claude" solto antes
            // do aviso da dupla.
            if let Item::Vez { .. } = item
                && !chat.itens.get(indice + 1..).unwrap_or_default().iter()
                    .take_while(|i| !matches!(i, Item::Usuario { .. } | Item::Vez { .. }))
                    .any(|i| matches!(i, Item::Assistente { .. } | Item::Ferramenta { .. } | Item::Raciocinio { .. }))
                && chat.itens.get(indice + 1..).unwrap_or_default().iter().any(|i| matches!(i, Item::Usuario { .. } | Item::Vez { .. }))
            {
                i += 1;
                continue;
            }
            i += 1;
            let chave = chave_do_item(TipoDaChave::Item, indice, item, &extras_de(indice));
            blocos.push(Planejado::novo(Some(indice), chave, move || eu.item(ctx, indice, item, window, cx)).fica_montado(fica_montado([item])));
        }
        let parcial = motor.parcial(&chat.id);
        if let Some(parcial) = parcial {
            // O mesmo id do texto que vai pousar neste índice: o documento já lido continua, e a
            // entrada não se repete quando ele pousa.
            let indice = chat.itens.len();
            let id = SharedString::from(format!("{}:{indice}", chat.id));
            blocos.push(Planejado::pronto(
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
        super::componentes::limpar_aneis(painel_id);
        if matches!(motor.execucao(&chat.id), Execucao::Rodando { .. }) {
            // A resposta já veio e os agentes seguem em segundo plano: uma linha por agente, com o
            // que cada um faz agora (dono, 23/11/2026: "aparece isso mas não vejo em lugar nenhum
            // o que eles estão fazendo", da frase "Agentes trabalhando em segundo plano…"). Ficam
            // também enquanto o Claude comenta o agente que acabou de terminar: o texto chega
            // acima delas, e elas não piscam.
            let no_fundo = if motor.esperando_agentes(&chat.id) { motor.linhas_do_segundo_plano(&chat.id) } else { Vec::new() };
            if !no_fundo.is_empty() {
                // Um bloco só: as linhas ficam coladas, com o passo da trilha.
                let linhas: Vec<AnyElement> = no_fundo.iter().enumerate().map(|(n, linha)| eu.linha_do_agente(ctx, n, linha, cx)).collect();
                blocos.push(Planejado::pronto(Bloco::Log, None, v_flex().w_full().children(linhas).into_any_element()));
            }
            // O turno calado: o Claude Code resumindo a conversa (também com agentes no fundo), os
            // agentes do fluxo de que ainda não há o que mostrar, ou o modelo pensando.
            let compactando = motor.compactando(&chat.id);
            if (no_fundo.is_empty() || compactando)
                && parcial.is_none()
                && texto_atividade(&chat.itens, false, motor.dirigidas_no_turno(&chat.id)) == "Pensando…"
            {
                let rotulo = rotulo_do_turno_calado(compactando, motor.esperando_agentes(&chat.id));
                blocos.push(Planejado::pronto(Bloco::Log, None, linha_pensando(painel_id, rotulo, cx.theme().font_family.clone())));
            }
        }
        // Só o que está à vista (e a sobra em volta) é montado; o resto entra como um espaço da
        // altura que mediu da última vez (ver `a_vista`). Com a busca aberta, uma seleção de
        // texto na janela ou a abertura do chat por decidir, tudo é montado, como antes.
        let alturas = eu.paineis[ix].alturas.clone();
        // O rem das mensagens é o da janela vezes a letra da conversa (`na_letra_da_conversa`):
        // os espaços entre os blocos saem nele, e trocar a letra invalida as alturas guardadas.
        let rem = window.rem_size() * crate::tema::fator_da_conversa();
        alturas.borrow_mut().comecar(Assinatura { chat: chat.id.clone(), rem, foco, estreito: minima(nivel) });
        let altura_da_vista = rolagem.bounds().size.height;
        let vista = {
            let p = &eu.paineis[ix];
            if p.busca.aberta || eu.selecionando || p.alvo.is_some() {
                Vista::Tudo
            } else if p.seguindo && !p.manter_lugar {
                Vista::NoFim { altura: altura_da_vista }
            } else {
                Vista::Em { topo: -rolagem.offset().y, altura: altura_da_vista }
            }
        };
        let agora_mesmo = std::time::Instant::now();
        let guardadas: Vec<Option<(Pixels, Bloco)>> = {
            let alturas = alturas.borrow();
            blocos
                .iter()
                .map(|b| {
                    let chave = b.chave.as_ref()?;
                    // O texto longo só entra na conta com a altura assentada.
                    if b.fica_montado { alturas.assentada(chave, agora_mesmo) } else { alturas.de(chave) }
                })
                .collect()
        };
        let medidas: Vec<Medida> = {
            // O tipo de cada bloco, quando já se sabe; sem ele, o espaço antes conta zero (a conta
            // é sempre para o lado seguro).
            let tipos: Vec<Option<Bloco>> = blocos.iter().zip(&guardadas).map(|(b, g)| b.bloco.or(g.map(|(_, bloco)| bloco))).collect();
            (0..blocos.len())
                .map(|k| {
                    let margem = match (k.checked_sub(1).map(|a| tipos[a]), tipos[k]) {
                        (None, Some(_)) => px(0.),
                        (Some(Some(anterior)), Some(atual)) => espaco_antes(Some(anterior), atual).to_pixels(rem),
                        _ => px(0.),
                    };
                    Medida { altura: guardadas[k].map(|(altura, _)| altura), margem }
                })
                .collect()
        };
        let (sobra, antes, depois) = (a_vista::sobra_da_vista(altura_da_vista), esp::E12.to_pixels(rem), esp::NO_TURNO.to_pixels(rem));
        let mut a_montar = a_vista::blocos_a_montar(&medidas, vista, sobra, antes, depois);
        // No fim, monta também o que está onde a rolagem está agora: se ela ainda não chegou lá
        // (ou saiu de lá pela barra neste instante), não aparece um vazio no lugar.
        if matches!(vista, Vista::NoFim { .. }) {
            let aqui = Vista::Em { topo: -rolagem.offset().y, altura: altura_da_vista };
            for (montar, tambem) in a_montar.iter_mut().zip(a_vista::blocos_a_montar(&medidas, aqui, sobra, antes, depois)) {
                *montar |= tambem;
            }
        }
        let (base, vistos) = (self.paineis[ix].base_animacao, self.paineis[ix].vistos);
        let mut lista = v_flex()
            .flex_none()
            // Numa conversa curta, ela encosta embaixo, perto da caixa, como num mensageiro (dono,
            // 18/11/2026: o "Pensando…" lá em cima ficava "muito longe de baixo"). A margem automática
            // só ocupa a sobra: com a conversa maior que a tela, ela é zero e a rolagem segue igual.
            .mt_auto()
            // Na caixa estreita, 16 dos lados (eram 12; dono, 17/11/2026: "aumenta um pouco o
            // padding de dentro da parte do texto").
            .map(coluna_de_leitura)
            // 6 em cima e 20 no fim, os da rolagem do protótipo Linear (25/11/2026).
            .pt(esp::E6)
            .pb(linear::ROLAGEM_BAIXO);
        let mut anterior = None;
        // Onde cada bloco ficou neste quadro (o índice do item, o alto e o pé, na tela): o
        // `on_prepaint` da lista, que vem depois dos blocos, os passa para a régua do painel.
        let medidos: Rc<std::cell::RefCell<Vec<(usize, Pixels, Pixels)>>> = Default::default();
        let alto_do_novo: Rc<std::cell::Cell<Option<(usize, Pixels)>>> = Default::default();
        let presa_do_historico = eu.paineis[ix].presa_do_historico.clone();
        // Um bloco montado mediu outra altura que a usada na conta deste desenho.
        let outra_altura: Rc<std::cell::Cell<bool>> = Default::default();
        // Quantos blocos entraram como espaço: se a largura mudou, eles usaram altura velha.
        let mut espacos = 0usize;
        let (n_blocos, inicio_do_desenho) = (blocos.len(), std::time::Instant::now());
        for (k, planejado) in blocos.into_iter().enumerate() {
            let Planejado { indice, chave, fica_montado, sem_entrada, antes_de, presa: sua, montar, .. } = planejado;
            // Longe da vista e já medido: um espaço da altura dele. Senão, o bloco de verdade.
            let (bloco, montado) = match guardadas[k].filter(|_| !a_montar[k]) {
                Some((altura, bloco)) => (bloco, Err(altura)),
                None => {
                    let (bloco, elemento) = montar();
                    (bloco, Ok(elemento))
                }
            };
            let margem = espaco_antes(anterior, bloco);
            let mut caixa = match montado {
                // A mensagem para a qual o sumário do Este chat acabou de levar fica realçada um instante.
                Ok(elemento) => {
                    // O texto longo que voltou à vista: a caixa fica na altura guardada enquanto
                    // o kit relê o markdown dele (`Alturas::segurar`).
                    let segura = chave.filter(|_| fica_montado).and_then(|chave| alturas.borrow_mut().segurar(&chave, agora_mesmo));
                    let caixa = div().mt(margem).when_some(segura, |caixa, altura| caixa.min_h(altura)).child(elemento);
                    let mut caixa = eu.com_realce_do_sumario(caixa, &chat.id, indice);
                    if let Some(chave) = chave {
                        let (alturas, contada, outra_altura) = (alturas.clone(), guardadas[k].map(|(altura, _)| altura), outra_altura.clone());
                        caixa = caixa.on_prepaint(move |limites, _, _| {
                            // O bloco mediu outra coisa que a conta usou (abriu, recolheu, o texto
                            // chegou): os de depois dele não estão onde a conta os pôs.
                            if contada.is_some_and(|contada| contada != limites.size.height) {
                                outra_altura.set(true);
                            }
                            let mut alturas = alturas.borrow_mut();
                            if fica_montado {
                                alturas.anotar_assentando(chave, limites.size.height, bloco, std::time::Instant::now());
                            } else {
                                alturas.anotar(chave, limites.size.height, bloco);
                            }
                        });
                    }
                    caixa
                }
                Err(altura) => {
                    espacos += 1;
                    if let Some(chave) = chave.filter(|_| fica_montado) {
                        alturas.borrow_mut().virou_espaco(chave);
                    }
                    div().mt(margem).h(altura).flex_none()
                }
            };
            let montado = a_montar[k] || guardadas[k].is_none();
            if let Some(i) = indice {
                let medidos = medidos.clone();
                caixa = caixa.on_prepaint(move |limites, _, _| medidos.borrow_mut().push((i, limites.top(), limites.bottom())));
            }
            if let Some(seguinte) = antes_de {
                let alto_do_novo = alto_do_novo.clone();
                caixa = caixa.on_prepaint(move |limites, _, _| alto_do_novo.set(Some((seguinte, limites.top()))));
            }
            // As mensagens suas do histórico trazido: onde cada uma ficou, para a presa no alto
            // (montada ou não: a de cima da vista é justamente a que prende).
            if let Some(texto) = sua {
                let presa = presa_do_historico.clone();
                caixa = caixa.on_prepaint(move |limites, _, _| presa.borrow_mut().push((texto.clone(), limites.top(), limites.bottom())));
            }
            let entrada = if sem_entrada || !montado { None } else { entrada_do_bloco(indice, base, vistos) };
            // O que já ficava à vista com o turno recolhido (a pergunta, o erro, o plano) não pisca.
            let abre = !sem_entrada
                && montado
                && indice.is_some_and(|i| {
                    abrindo.iter().any(|(de, ate)| i > *de && i < *ate)
                        && chat.itens.get(i).is_some_and(|item| !fica_fora_do_recolhido(item) && plano_na_conversa(item).is_none())
                });
            lista = match (indice, entrada) {
                (Some(i), Some(nova)) => lista.child(movimento::entrada(("entra", i), nova, De::Baixo, caixa)),
                (Some(i), None) if abre => lista.child(movimento::entrada(("abre", i), true, De::Cima, caixa)),
                _ => lista.child(caixa),
            };
            anterior = Some(bloco);
        }
        // Embaixo, os 32 só depois de uma resposta, onde a barra de copiar e da hora aparece; depois
        // da trilha ("Pensou por 5 s") ou de um aviso, 12, colado na caixa (dono, 18/11/2026: "tá
        // muito longe o pensou da parte das mensagens em baixo").
        if let Some(pe) = pe_da_lista(matches!(anterior, Some(Bloco::Texto))) {
            lista = lista.pb(pe);
        }
        let janela = cx.entity().downgrade();
        let inicio_da_resposta = inicio_da_ultima_resposta(&chat.itens);
        let (chat_id, n_itens) = (chat.id.clone(), chat.itens.len());
        let suas: Rc<Vec<(usize, String)>> = Rc::new(mensagens_do_usuario(&chat.itens));
        let busca_aberta = self.paineis[ix].busca.aberta;
        lista = lista.on_prepaint(move |bounds, window, cx| {
            super::medir_conversa(n_blocos, n_blocos - espacos, inicio_do_desenho.elapsed());
            // `redesenhar`: as mensagens deste painel precisam de mais um desenho (a mensagem
            // presa mudou, a leitura foi reposta). `tudo`: a Janela inteira (a abertura do chat
            // mexe no que a lista e a caixa mostram).
            let (mut redesenhar, mut tudo) = (false, false);
            let _ = janela.update(cx, |j, cx| {
                let Some(p) = j.paineis.iter_mut().find(|p| p.id == painel_id) else { return; };
                p.altura_conteudo = bounds.size.height;
                p.topo_da_lista.set(bounds.top());
                // A coluna mudou de largura: as alturas guardadas eram da outra. Se algum espaço
                // deste desenho usou uma delas, desenha de novo, com tudo o que falta montado.
                if alturas.borrow_mut().conferir_largura(bounds.size.width, espacos) {
                    redesenhar = true;
                }
                // Um bloco montado mudou de altura (recolheu uma saída de 2 mil pixels, por
                // exemplo): os de depois dele subiram, e algum que a conta deixou como espaço
                // pode ter entrado na vista. Mais um desenho, já com a altura nova.
                if outra_altura.get() && espacos > 0 {
                    redesenhar = true;
                }
                // A mensagem presa, como o "sticky" do VS Code: a sua mensagem mais recente que já
                // subiu além do alto da vista gruda lá (a de verdade fica por baixo da faixa).
                // Subindo além dela, gruda a anterior. Com a rolagem e as posições deste quadro.
                let topo = -p.rolagem.offset().y - px(2.);
                let mut presa: Option<(String, Pixels, usize)> = None;
                // A sua mensagem seguinte, a primeira que ainda não subiu além do alto: chegando
                // perto, ela empurra a faixa presa para cima.
                let mut seguinte: Option<Pixels> = None;
                let mut considerar = |texto: &str, alto: Pixels, i: usize| {
                    if alto < topo && presa.as_ref().is_none_or(|(_, outro, _)| alto >= *outro) {
                        presa = Some((texto.to_string(), alto, i));
                    }
                    if alto >= topo && seguinte.is_none_or(|s| alto < s) {
                        seguinte = Some(alto);
                    }
                };
                for (texto, alto, _) in p.presa_do_historico.borrow().iter() {
                    considerar(texto, *alto - bounds.top(), usize::MAX);
                }
                {
                    let medidos = medidos.borrow();
                    for (i, texto) in suas.iter() {
                        if let Some(&(_, alto, _)) = medidos.iter().find(|(k, ..)| k == i) {
                            considerar(texto, alto - bounds.top(), *i);
                        }
                    }
                }
                let presa = presa.filter(|_| !busca_aberta);
                // Da seguinte até o alto da vista: o que faltar para caber a faixa inteira é o
                // quanto ela sobe.
                let empurrao = match (&presa, seguinte) {
                    (Some(_), Some(s)) => (p.altura_da_presa.get() - (s - (topo + px(2.)))).max(px(0.)),
                    _ => px(0.),
                };
                if p.presa != presa || p.empurrao_da_presa != empurrao {
                    p.presa = presa;
                    p.empurrao_da_presa = empurrao;
                    redesenhar = true;
                }
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
                    let novo = alto_do_novo.get().map(|(seguinte, alto)| (seguinte, alto - bounds.top()));
                    let decisao = onde_abrir(alvo, &p.posicoes.borrow(), novo, bounds.size.height, vista, inicio_da_resposta, n_itens);
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
                    tudo = true;
                    // Coube na tela: o fim já está à vista, e o chat conta como lido.
                    if lido && let Some(motor) = j.motor.clone() {
                        motor.update(cx, |m, _| m.marcar_lido(&chat_id));
                    }
                }
            });
            if redesenhar {
                // Só as mensagens deste painel: a mensagem presa troca a cada passo da roda, e
                // avisar a Janela refazia a lista e as conversas ao lado junto.
                window.on_next_frame(move |_, cx| {
                    let _ = janela.update(cx, |j, cx| if tudo { cx.notify() } else { j.avisar_vista(painel_id, cx) });
                });
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

        // A mensagem sua que você está lendo fica presa no alto (dono, 17 e 18/11/2026: "faz a
        // última mensagem que mando grudar em cima" e "não ser só a última, ser também a última
        // onde tô olhando"), em duas linhas; o clique leva até ela. Escolhida no prepaint da lista
        // (acima); sai com a busca da conversa aberta, que mora no mesmo lugar.
        let (empurrao, altura_da_presa) = (self.paineis[ix].empurrao_da_presa, self.paineis[ix].altura_da_presa.clone());
        // Ela entra com fade quando aparece (a sua mensagem subiu além do topo); trocar de texto
        // rolando segue a rolagem, sem fade, para não piscar a cada passo da roda.
        let presa_a_vista = self.paineis[ix].presa.is_some() && !self.paineis[ix].busca.aberta;
        let presa_nova = presa_a_vista && !std::mem::replace(&mut self.paineis[ix].presa_vista, presa_a_vista);
        let presa = self.paineis[ix]
            .presa
            .clone()
            .filter(|_| !self.paineis[ix].busca.aberta)
            .map(|(texto, alto, i)| {
                let janela = cx.entity().downgrade();
                let raio = crate::tema::raio_da_peca();
                // A faixa do topo até a mensagem não tem fundo: a conversa é que não é desenhada
                // embaixo dela (`recortada_no_alto`, na lista abaixo). Com as camadas do cartão
                // repetidas por cima, ela tapava o texto mas ficava mais escura que o painel com a
                // transparência ligada (dono, 18/11 e 24/11/2026). Sem sombra embaixo: a de 8 px
                // escura ficava "bem ruim" (dono, 18/11/2026).
                // A seguinte, chegando no alto, empurra a faixa para cima (`empurrao_da_presa`); a
                // altura dela é medida aqui, para o prepaint da lista saber quanto empurrar.
                div()
                    .absolute()
                    .top(-empurrao)
                    .left_0()
                    .right_0()
                    .pt(esp::E8)
                    .pb(esp::E4)
                    .on_prepaint(move |limites, window, _| {
                        // A conversa é recortada por esta altura (`recortada_no_alto`): mudou, desenha
                        // de novo para o recorte acompanhar.
                        if altura_da_presa.replace(limites.size.height) != limites.size.height {
                            window.refresh();
                        }
                    })
                    .child(
                    div().map(coluna_de_leitura).child(
                        div()
                            .id(("mensagem-presa", painel_id as usize))
                            .relative()
                            .w_full()
                            // A folga dos balões, também no painel mínimo (lá ficava 2 px fora).
                            .px(r(folga_do_balao(nivel)))
                            .py(linear::BALAO_ALTO)
                            .rounded(raio)
                            .cursor_pointer()
                            // A cor cheia dos balões da conversa (Linear, 25/11/2026).
                            .bg(Cores::balao())
                            .child(div().relative().fala().text_color(Cores::texto_do_balao()).line_clamp(2).child(texto))
                            .dica_no_ponteiro(|window, cx| Tooltip::new("Ir até esta mensagem").build(window, cx))
                            .on_click(move |_, _, cx| {
                                let _ = janela.update(cx, |j, cx| j.ir_para_a_mensagem(painel_id, i, alto, cx));
                            }),
                    ),
                )
            });
        // Com a faixa presa à vista, a conversa some embaixo dela (ver `recortada_no_alto`).
        let recorte = presa.is_some().then(|| (self.paineis[ix].altura_da_presa.clone(), empurrao));
        let alto_do_recorte = move || recorte.as_ref().map_or(px(0.), |(altura, empurrao)| super::vista_mensagens::alto_do_recorte(altura.get(), *empurrao));
        div()
            .key_context(CONTEXTO_DA_CONVERSA)
            .relative()
            .flex_1()
            // Em janela baixa, o plano e os pedidos de permissão encolhem e rolam antes: as
            // mensagens nunca somem de todo.
            .min_h(MINIMO_MENSAGENS)
            .w_full()
            .child(super::vista_mensagens::recortada_no_alto(
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
                        // para voltar ao fim. Só esta vista se redesenha: a lista e os outros
                        // painéis ficam no cache.
                        this.acompanhar_rolagem(cx);
                        this.carregar_anteriores(painel_id, cx);
                        this.avisar_vista(painel_id, cx);
                        this.acompanhar_sumario(painel_id, cx);
                    }))
                    // Só as mensagens crescem com a letra da conversa (Configurações > Aparência):
                    // a barra de rolagem, a pílula do fim e a busca ficam no tamanho da interface.
                    .child(super::vista_mensagens::na_letra_da_conversa(lista)),
                alto_do_recorte,
            ))
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
            // Empurrada para cima, a faixa sai pelo alto da lista, cortada nele: solta, ela subia por
            // cima das abas do cabeçalho (dono, 19/11/2026: "a mensagem ficou em cima das abas").
            .children(presa.map(|faixa| {
                let faixa = movimento::entrada(("entra-presa", painel_id as usize), presa_nova, De::Parado, faixa);
                div().absolute().inset_0().overflow_hidden().child(super::vista_mensagens::na_letra_da_conversa(faixa))
            }))
            // A barra do Ctrl+F, por cima das mensagens (montada em `conversa`).
            .children(barra_busca)
    }

    /// Rola o painel até a mensagem `item`, que começa em `alto` no conteúdo, com um respiro em cima.
    fn ir_para_a_mensagem(&mut self, painel_id: u64, item: usize, alto: Pixels, cx: &mut Context<Self>) {
        let Some(p) = self.paineis.iter_mut().find(|p| p.id == painel_id) else { return };
        let alto = p.posicoes.borrow().iter().find(|(k, ..)| *k == item).map_or(alto, |&(_, alto, _)| alto);
        let novo = point(px(0.), -(alto - px(12.)).max(px(0.)));
        p.rolagem.set_offset(novo);
        p.leitura.deslocamento = novo.y;
        p.seguindo = false;
        cx.notify();
    }

    fn carregar_anteriores(&mut self, painel_id: u64, cx: &mut Context<Self>) {
        let Some(ix) = self.paineis.iter().position(|p| p.id == painel_id) else { return; };
        let p = &self.paineis[ix];
        if p.rolagem.offset().y < -px(240.) || p.preservar_topo.is_some() || p.busca.aberta { return; }
        let nativos = self.motor.as_ref().and_then(|m| m.read(cx).chat(p.chat.as_deref()?)).map_or(0, |chat| {
            chat.itens.iter().filter(|item| !matches!(item, Item::Ferramenta { nome, .. } if nome == "ToolSearch")).count()
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
        // A conversa de um subagente que ainda não tem nada: a leitura do arquivo dele não voltou,
        // ou ele acabou de abrir.
        if let Some(sub) = motor.subconversa(&chat.id) {
            let (titulo, texto) = if motor.execucao(&chat.id).rodando() {
                ("O subagente está começando", "O que ele ler, rodar e responder aparece aqui.")
            } else if sub.fonte == crate::motor::subconversa::Fonte::Lendo {
                ("Lendo a conversa do subagente…", "Ela vem do arquivo que o Claude Code gravou.")
            } else {
                ("Sem conversa guardada", "O Claude Code não tem mais o arquivo deste subagente, e o cartão dele não guardou nada.")
            };
            return EstadoVazio::new(Lucide::Workflow, titulo).texto(texto).amplo().into_any_element();
        }
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
                marca,
                ..
            } => {
                let reforco = Reforco { ultrathink: *ultrathink, ultracode: *ultracode, depois: false };
                let realce = contexto.busca.realce(indice, Campo::Texto);
                let editavel = contexto.editavel.is_some_and(|desde| indice >= desde);
                // Só o Claude Code guarda o antes dos arquivos por mensagem (a marca de um turno
                // do Codex, numa dupla, não é dele).
                let marca = marca
                    .as_deref()
                    .filter(|m| contexto.chat.agente == crate::modelo::Agente::Claude && crate::modelo::turno_do_codex(m).is_none());
                (Bloco::Balao, self.balao_usuario(contexto, &chave, texto, anexos, reforco, item.em(), realce, editavel, marca, window, cx))
            }
            Item::Assistente { texto, .. } => {
                (Bloco::Texto, self.texto_assistente(contexto, indice, &chave, texto, item.em(), window, cx))
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
            Item::FimDoAgente { .. } => (Bloco::Log, self.fim_do_agente(contexto, indice, item, cx)),
            Item::Revisao { de, texto, mandada, .. } => {
                (Bloco::Recebida, self.cartao_da_revisao(contexto, indice, *de, texto, *mandada, window, cx))
            }
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
        marca: Option<&str>,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let grupo = SharedString::from(format!("msg-{chave}"));
        // Fechada em duas linhas, a mensagem abre e fecha com um clique (dono, 17/11/2026: "no
        // máximo duas linhas e ter que clicar pra aparecer o resto"). Mais de duas linhas no texto
        // ou mais caracteres do que cabem em duas na coluna (uns 170 na larga, 100 na estreita).
        let linhas = texto.lines().count();
        let cabem = if larga(contexto.nivel) { 170 } else { 100 };
        let longo = linhas > 2 || texto.chars().count() > cabem;
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
                        .relative()
                        .bg(Cores::balao())
                        .mono()
                        .text_color(Cores::codigo())
                        .child(comando.to_string()),
                )
                .into_any_element(),
            None => {
                // Longa: o balão inteiro abre e fecha no clique, e um chevron discreto no canto
                // diz que há mais (ou que dá para fechar).
                let mostrar = longo.then(|| {
                    let icone = if tudo { Lucide::ChevronUp } else { Lucide::ChevronDown };
                    div()
                        .absolute()
                        .right(esp::E8)
                        .bottom(esp::E6)
                        .text_color(Cores::tenue())
                        .child(icone_de(icone, 14.))
                });
                let alternar_tudo = longo.then(|| alternar(chave_tudo.clone(), cx));
                // Os anexos entram no balão, em pílulas em cima do texto, como na extensão do
                // Claude Code (dono, 16/11/2026), no poço escuro do balão.
                // Mais justo: 12 dos lados e 8 em cima e embaixo (eram 16 e 12; dono, 17/11/2026:
                // "diminui o padding disso").
                let folga = folga_do_balao(contexto.nivel);
                // As pílulas chegam a 5 da borda de fora do balão, e não aos 11 do texto: em volta
                // delas sobrava muito (dono, 17/11/2026).
                let pilulas = (!anexos.is_empty()).then(|| {
                    h_flex().flex_wrap().gap(esp::E6).ml(r(-(folga - 6.))).children(
                        anexos.iter().enumerate().map(|(i, a)| pilula_de_arquivo(format!("anexo-{chave}-{i}"), a, &contexto.chat.id, true, cx)),
                    )
                });
                let com_texto = !texto.trim().is_empty();
                // O balão é azul bem baixo (o destaque misturado à base, opaco): diz de quem é a
                // fala sem a moldura de cartão, e o canto de baixo à direita é o da ponta.
                // Na largura toda da coluna (dono, 16/11/2026: "faz o campo das minhas mensagens
                // ficar na largura toda"). Raio 16, o das peças: com 20 numa faixa de 46 a
                // mensagem de uma linha virava pílula (balão e cartão de erro, 16/11/2026).
                let tem_pilulas = pilulas.is_some();
                let balao = (com_texto || pilulas.is_some() || mostrar.is_some() || reforco != Reforco::default()).then(|| {
                    v_flex()
                        .id(SharedString::from(format!("balao-{chave}")))
                        .w_full()
                        .when_some(alternar_tudo, |s, f| s.cursor_pointer().on_click(f))
                        .px(r(folga))
                        // 6 em cima e embaixo (protótipo Linear, 25/11/2026).
                        .pt(linear::BALAO_ALTO)
                        .pb(linear::BALAO_ALTO)
                        .gap(if tem_pilulas { esp::E6 } else { esp::E8 })
                        // O fundo da lista (dono, 17/11/2026: "era pra ficar igual o fundo da
                        // lista"): o pedaço da chapa de metal que está atrás, com o véu azul e o
                        // fosco da base por cima, sem fio e sem a ponta.
                        .relative()
                        // Chapado (Linear, 25/11/2026): a cor cheia do escolhido, sem o buraco no
                        // metal de antes.
                        .rounded(linear::RAIO_DO_BALAO)
                        .bg(Cores::balao())
                        .children(pilulas)
                        .when(com_texto, |s| {
                            // A fala um degrau abaixo da resposta (14/22 no texto, não no branco),
                            // e os caminhos que existem viram link, como na resposta. O Copiar e o
                            // Editar levam o texto original.
                            let cwd = &contexto.chat.cwd;
                            // As menções que existem viram pílula (o texto que foi ao agente segue
                            // com o caminho inteiro); depois, os caminhos soltos viram link.
                            let mencoes = super::mencao::ligar_mencoes(texto, &mut |c| super::links_arquivo::resolver_mencao(cwd, c));
                            let ligador = self.ligador(cwd, cx);
                            let ligado = super::links_arquivo::ligar_arquivos(&mencoes, &mut |c| ligador.resolver(c));
                            let cortado = longo && !tudo;
                            s.child(
                                div()
                                    .relative()
                                    .fala()
                                    .text_color(Cores::texto_do_balao())
                                    .when(longo, |s| s.pr(esp::E16))
                                    .child(
                                        markdown_achado(contexto, chave.to_string().into(), com_quebras_duras(&ligado), realce)
                                            .estilo(estilo_do_balao)
                                            .linhas(cortado.then_some(2)),
                                    ),
                            )
                        })
                        .children(mostrar)
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
        // "Continuar daqui em outro chat": só nas mensagens da própria conversa (as do histórico
        // trazido do Claude Code não são itens dela).
        if let Some(ix) = chave.rsplit(':').next().and_then(|i| i.parse::<usize>().ok())
            && matches!(contexto.chat.itens.get(ix), Some(Item::Usuario { texto: t, .. }) if t == texto)
        {
            acoes.push(super::bifurcar::botao(chave, contexto.painel, ix, window, cx).into_any_element());
        }
        // Os arquivos voltam a como estavam antes desta mensagem (o checkpoint do Claude Code).
        if let Some(marca) = marca {
            acoes.push(super::reenvio::botao_voltar_arquivos(chave, &contexto.chat.id, marca, window, cx).into_any_element());
        }
        acoes.push(copiar(chave, texto).into_any_element());
        // O balão ocupa a coluna inteira: a barra de ações flutua no canto de cima, com a beirada
        // da direita na coluna do fim do texto (a mesma folga do balão). Era 16 fixo,
        // a folga de antes de 17/11, e ficava 5 px antes do fim do texto (folha dimensional,
        // 23/11/2026). Sobe 22, e não 14: com 8 de respiro no balão, ela entrava 6 px na primeira
        // linha do texto (revisão de design, 23/11/2026).
        // Linear (25/11/2026): embaixo do balão, à direita, por cima do que vem depois; o alcance
        // do ponteiro desce junto, para a barra não sumir no caminho até ela.
        let barra = div().absolute().bottom(r(-32.)).right(r(folga_do_balao(contexto.nivel)));
        v_flex()
            .w_full()
            .map(com_alcance_da_barra_embaixo)
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
                cartao = cartao.acao(fantasma(id("detalhes"), Some(icone), "Detalhes", TamanhoBotao::Linha, alternar(chave_detalhes.clone(), cx), window, cx));
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
                // Aberto pelo "Detalhes", o texto do agente entra com fade e 6 px de cima.
                cartao = cartao.original(self.ao_abrir(&chave_detalhes, original), !lido.recolhido);
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
            && outro.instalado();
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
        // O instalador do Gemini também faz o login (`agy acp --login`), e sem instalar só entra.
        let gemini = |rotulo: &str, icone: Lucide, nome: &str| {
            fantasma(id(nome), Some(icone), rotulo.to_string(), TamanhoBotao::Linha, |_, _, _| super::uso_plano::instalar_gemini(), window, cx)
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
            (TipoDeErro::Limite, Agente::Gemini) => continuar(&mut acoes),
            (TipoDeErro::Login, Agente::Gemini) => acoes.push(gemini("Entrar no Gemini", Lucide::KeyRound, "entrar")),
            (TipoDeErro::SemAgente, Agente::Gemini) => acoes.push(gemini("Instalar o Gemini", Lucide::Download, "instalar")),
            // As Contas já no login da conta que caiu, e não na lista (rodada de uso, item 8.3).
            (TipoDeErro::Login, Agente::Claude) => {
                let conta = conta_que_caiu(contexto.motor, chat);
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
            (TipoDeErro::SemAgente, _) => acoes.push(instalar("Instalar o Codex", Lucide::Download, "instalar")),
            (TipoDeErro::Contexto, Agente::Claude) => {
                let painel = contexto.painel;
                acoes.push(
                    fantasma(
                        id("compactar"),
                        Some(Lucide::Minimize),
                        "Compactar",
                        TamanhoBotao::Linha,
                        cx.listener(move |this, _, _, cx| this.compactar_conversa(painel, cx)),
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
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let grupo = SharedString::from(format!("msg-{chave}"));
        // Os arquivos citados que estão no disco viram links: abrem no painel da direita. O
        // comando de barra citado em código em linha (`/revisar`) vai para a caixa.
        let ligador = self.ligador(&contexto.chat.cwd, cx);
        let ligado = super::links_arquivo::ligar_resposta(texto, &mut |c| ligador.resolver(c), &mut |nome| {
            let existe = self.comandos_de(contexto.chat).iter().any(|c| c == nome);
            existe.then(|| super::links_arquivo::link_do_comando(&contexto.chat.id, nome))
        });
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
                    let texto = div().corpo().text_color(Cores::texto_da_fala()).child(
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
                .text_color(Cores::texto_da_fala())
                .child(markdown_achado(contexto, chave.to_string().into(), ligado, realce).estilo(estilo_da_resposta))
                .into_any_element()
        };
        // O texto fecha o turno quando depois dele vem o seu balão (ou nada): só aí há o vão de 32.
        let cabe_a_barra = matches!(contexto.chat.itens.get(indice + 1), None | Some(Item::Usuario { .. } | Item::Vez { .. } | Item::Entrega { .. }));
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
            // A barra fica embaixo da resposta, no vão até o bloco seguinte (32 antes do seu balão
            // e no fim da lista): em cima ela cobria metade da primeira linha (relatório de
            // aparência, 16/11/2026, V7) e, no pé do texto, o fim da última, que na coluna estreita
            // era um terço da linha. O seu balão continua com ela em cima. Só no texto que fecha o
            // turno: no do meio, a trilha vem a 12 e a barra ficava em cima dela (dono, 18/11/2026:
            // "o hover tá em cima da parte de copiar e hora").
            // Pintada depois de tudo (`deferred`): a linha logo abaixo (a revisão da memória) não
            // passa por cima dela, e a barra não parece transparente (dono, 18/11/2026: "faz ser
            // menos transparente isso").
            .when(cabe_a_barra, |s| s.child(deferred(div().absolute().bottom(r(-30.)).right_0().child(barra_de_acoes(
                grupo.clone(),
                hora_na_barra(em)
                    .into_iter()
                    // "Continuar daqui em outro chat", só nas respostas da própria conversa.
                    .chain(
                        (matches!(contexto.chat.itens.get(indice), Some(Item::Assistente { texto: t, .. }) if t == texto)
                            && crate::motor::subconversa::partes_do_id(&contexto.chat.id).is_none())
                        .then(|| super::bifurcar::botao(chave, contexto.painel, indice, window, cx).into_any_element()),
                    )
                    .chain([copiar(chave, texto).into_any_element()])
                    .collect(),
                cx,
            ))).with_priority(1)));
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
        let realce = contexto.busca.realce(indice, Campo::Texto);
        let aberto = self.expandidos.contains(chave);
        let grupo = SharedString::from(format!("msg-{chave}"));
        let (quem, nome) = remetente(contexto.motor, de);
        // O chat que mandou: "Abrir o chat" nas ações.
        let de_chat = chat_que_mandou(contexto.motor, de);
        let avatar = match quem {
            Some(papel) => papel.circulo(16.).into_any_element(),
            None => img(LOGO.clone()).flex_none().size(r(16.)).into_any_element(),
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
            // O avatar na coluna de 16 da primeira borda e o nome na segunda, como as linhas da trilha.
            .child(coluna_do_icone().child(avatar))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .corpo_ui()
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
            div().pl(RECUO_DO_LOG).corpo().text_color(Cores::texto()).child(markdown_achado(
                contexto,
                format!("{chave}-t").into(),
                texto.to_string(),
                realce.clone(),
            ))
        } else {
            // A prévia termina com reticências na coluna da seta, nunca cortada no meio da letra.
            div()
                .min_w_0()
                .pl(RECUO_DO_LOG)
                .pr(esp::E32)
                .corpo_ui()
                .text_color(Cores::tenue())
                // Duas linhas em qualquer largura: na coluna estreita, uma só mostrava três
                // palavras.
                .line_clamp(2)
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
            // Opaco, no preto do poço: o código não quebra mais a linha e passa por baixo do botão
            // (o fundo que o kit põe em volta dele é translúcido, e a letra aparecia através). A
            // tampa vai até 2 da borda do bloco e sobra 6 à esquerda, para não ficar um pedaço de
            // letra de cada lado do ícone.
            div().bg(Cores::preto()).px(esp::E6).mr(r(-6.)).child(BotaoCopiar::new("copiar", bloco.code()).dica(dica))
        })
        // O Copiar de cada tabela (rodada do Paseo, item 4), com o desenho do Copiar do código. O
        // kit põe o que este gancho devolve numa fileira logo abaixo da tabela, e não por cima
        // dela: o botão fica no canto direito dessa fileira. O clique copia a tabela em Markdown;
        // com Shift, separada por tabulação, que é o que uma planilha cola em colunas.
        .table_actions(|tabela, _, _| {
            h_flex().w_full().justify_end().child(
                // Sem os links internos que os caminhos de arquivo ganharam na resposta.
                BotaoCopiar::new("copiar-tabela", super::links_arquivo::sem_links_do_vitral(&tabela.markdown))
                    .com_shift(tabela_para_planilha(&tabela.headers, &tabela.rows))
                    .dica("Copiar a tabela (com Shift, para colar em planilha)"),
            )
        })
}

/// A tabela separada por tabulação, uma linha por fileira, com o cabeçalho na primeira: é o que
/// uma planilha cola em colunas. A tabulação e a quebra de linha dentro de uma célula viram
/// espaço, para a célula não se partir em duas.
pub(super) fn tabela_para_planilha(cabecalho: &[String], linhas: &[Vec<String>]) -> String {
    let fileira = |celulas: &[String]| celulas.iter().map(|c| c.replace(['\t', '\n', '\r'], " ")).collect::<Vec<_>>().join("\t");
    std::iter::once(cabecalho).chain(linhas.iter().map(Vec::as_slice)).map(fileira).collect::<Vec<_>>().join("\n")
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
    // A menção de arquivo ("@src/x.rs#L3-9") é uma pílula no meio do texto (dono, 16/11/2026).
    estilo_da_resposta(vista, cx).plugin(super::mencao::PilulaDeMencao).style(
        estilo_com_titulos(cx, [15., 14., 14.])
            // 16 de fora menos 16 de padding: o mínimo, 10.
            .code_block(poco_de_codigo(raio::CONTROLE))
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
    // O rem do tema já vem com o zoom: os títulos crescem junto. Eles são dados em px, então a
    // letra da conversa entra aqui (o resto do texto a recebe pelo rem, em `na_letra_da_conversa`).
    let rem = cx.theme().font_size * crate::tema::fator_da_conversa();
    TextViewStyle::default()
        .paragraph_gap(linear::ENTRE_FALAS)
        .heading_font_size(move |nivel, _| {
            rem * match nivel {
                1 => titulos[0] / 15.,
                2 => titulos[1] / 15.,
                _ => titulos[2] / 15.,
            }
        })
        .code_block(poco_de_codigo(raio::CARTAO))
        // O código no meio do texto no fundo da caixa, na cor do texto (protótipo Linear).
        .inline_code(HighlightStyle {
            color: Some(Cores::texto()),
            background_color: Some(Cores::caixa_preta()),
            ..Default::default()
        })
        .table(StyleRefinement::default().rounded(raio::CARTAO).border_color(Cores::borda()))
        // O kit mede as colunas contando 24 px de padding por célula, mas desenha o padding em rem
        // (0,75): com o zoom, a célula ficava mais estreita que o texto medido e a última palavra
        // caía para a linha de baixo, com a tabela sobrando de largura. Com o padding dito aqui, a
        // medida e o desenho usam o mesmo número em qualquer zoom.
        .table_cell(StyleRefinement::default().px(esp::E12))
}

/// O bloco de código das mensagens: o poço preto com o conteúdo a 12 da borda e a coluna do Copiar
/// à direita. As linhas não quebram, como na saída de comando e no diff: quebrada no meio, a linha
/// voltava à coluna zero e a indentação se perdia (numa coluna estreita, seis linhas de código
/// viravam catorze). O bloco rola de lado; a roda em pé continua levando a conversa, porque ele
/// nunca tem o que rolar para baixo.
fn poco_de_codigo(raio: Rems) -> StyleRefinement {
    let mut poco = StyleRefinement::default()
        // Em linha (flex), o texto fica com a largura da linha mais longa, e é essa largura que o
        // bloco rola; em bloco, ele seria esticado até a borda e o resto ficaria cortado.
        .flex()
        .p(dentro_da_borda(12.))
        .pr(COLUNA_DO_COPIAR)
        .bg(Cores::preto())
        .border_color(Cores::borda())
        .rounded(raio)
        .whitespace_nowrap();
    // Os dois eixos: só com o de pé o kit põe o Copiar fora do que rola (ele fica no canto), e só
    // com os dois a roda em pé não é desviada para o lado.
    poco.overflow.x = Some(Overflow::Scroll);
    poco.overflow.y = Some(Overflow::Scroll);
    poco
}

/// As ações de uma mensagem numa barra flutuante (28 em pílula, botões de 24): aparece seca no
/// hover da mensagem, sem reservar altura. Quem chama a posiciona. Um fade aqui viraria animação
/// em cadeia ao passar o ponteiro pela conversa.
/// O mesmo para a barra embaixo da resposta, que ocupa os 30 depois do texto: 32 de folga
/// embaixo, devolvidos pela margem.
fn com_alcance_da_barra_embaixo<E: Styled>(elemento: E) -> E {
    elemento.pb(esp::E32).mb(r(-32.))
}

/// As mensagens suas na conversa, com o índice: o texto (as primeiras linhas, que a faixa presa
/// no alto mostra em duas) ou, só com anexos, a contagem deles.
fn mensagens_do_usuario(itens: &[Item]) -> Vec<(usize, String)> {
    itens.iter().enumerate().filter_map(|(i, item)| match item {
        Item::Usuario { texto, .. } if !texto.trim().is_empty() => Some((i, texto_da_presa(texto))),
        Item::Usuario { anexos, .. } if !anexos.is_empty() => Some((i, contagem(anexos.len(), "anexo", "anexos"))),
        _ => None,
    })
    .collect()
}

/// As primeiras linhas de uma mensagem numa linha só (a faixa presa mostra duas).
pub(super) fn texto_da_presa(texto: &str) -> String {
    texto.lines().filter(|l| !l.trim().is_empty()).take(3).collect::<Vec<_>>().join(" ")
}

/// O fundo da lista (a base da janela) enchendo a caixa onde está: o recorte da chapa de metal
/// que fica atrás dela, o véu azul da base e o fosco por cima. O pai precisa ser `relative`.
pub(super) fn fundo_da_lista(raio: Pixels) -> Div {
    super::componentes::metal::fundo_da_lista(raio)
}

fn barra_de_acoes(grupo: SharedString, acoes: Vec<AnyElement>, _: &App) -> Div {
    h_flex()
        .h(r(28.))
        .p(esp::E2)
        .gap(esp::E2)
        // Chapada (Linear, 25/11/2026): o quase preto dos menus, sem fio nem sombra.
        .bg(Cores::flutuante())
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

/// Num chat com o Claude e o Codex juntos, quem trabalha daqui em diante: a marca na coluna de 16
/// da primeira borda e o nome na segunda, como as linhas da trilha.
fn marca_da_vez(agente: Agente) -> AnyElement {
    let marca = Marca::do_agente(agente);
    h_flex()
        .h(r(24.))
        .gap(r(8.))
        .child(marca.disco(16.))
        .child(div().metadado().font_semibold().text_color(Cores::texto()).child(agente.nome()))
        .into_any_element()
}

/// A nota da conversa: o que não é fala nem trabalho (o dia, "Trazida do Claude Code", "Turno
/// interrompido", a troca de conta). Centrada, na letra dos metadados, tênue e sem peso, sem fio e
/// sem ícone; a frase longa quebra centrada. Só o "Novo", que pede atenção, tem desenho próprio.
fn nota_da_conversa(conteudo: impl IntoElement) -> Div {
    h_flex().w_full().min_h(r(24.)).justify_center().child(
        div().min_w_0().max_w(relative(0.9)).metadado().text_color(Cores::tenue()).text_center().child(conteudo),
    )
}

/// O dia entre dois turnos, na nota da conversa.
fn separador_de_dia(rotulo: &str) -> AnyElement {
    nota_da_conversa(rotulo.to_string()).into_any_element()
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
/// Dos itens que entraram, só os blocos à vista são montados (ver `a_vista`, 23/11/2026); a
/// página continua porque um bloco só vira espaço depois de medido uma vez, e medir a conversa
/// inteira ao abrir custaria o que a página evita. Medido em 13/11/2026 no binário otimizado,
/// antes das duas coisas, com duas conversas de 400 itens lado a lado: 6,2 s de processador para
/// 80 passos de rolagem (78 ms por passo).
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
    h_flex().pl(RECUO_DO_LOG).py(esp::E4).child(
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
                    .child(img(caminho.clone()).size_full().rounded(raio::CONTROLE).object_fit(ObjectFit::Cover))
            })))
            .child(div().flex_none().metadado().text_color(Cores::tenue()).child(contagem(n, "imagem", "imagens")))
            .on_click(cx.listener(move |this, _, window, cx| this.abrir_visor(&chat_id, &primeira, window, cx)))
            .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx)),
    )
    .into_any_element()
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
            // O canto vai na própria imagem: o `overflow_hidden` corta reto, e a miniatura saía
            // quadrada dentro da caixa redonda (dono, 19/11/2026: "faz as mensagens ficar redonda").
            .child(img(caminho.to_path_buf()).size_full().rounded(raio::CONTROLE).object_fit(ObjectFit::Cover))
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
    // A imagem é só a miniatura, uma bolinha (dono, 17/11/2026: "a imagem tem que ficar uma
    // bolinha só"): o nome e o tamanho vão na dica, e o clique abre o visor.
    if abre {
        return div()
            .id(SharedString::from(id))
            .flex_none()
            .p(px(1.))
            .rounded(raio::CONTROLE)
            .border_1()
            .border_color(if no_balao { Cores::borda() } else { Cores::borda_forte() })
            .cursor_pointer()
            .hover(|s| s.border_color(Cores::destaque()))
            .child(simbolo)
            .on_click(cx.listener(move |this, _, window, cx| this.abrir_visor(&chat, &alvo, window, cx)))
            .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            .into_any_element();
    }
    h_flex()
        .id(SharedString::from(id))
        .flex_none()
        .max_w_full()
        .h(r(28.))
        .map(|s| if abre { s.pl(esp::E2) } else { s.pl(esp::E8) })
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
        .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
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
    /// `Painel.base_animacao` e `Painel.vistos` deste desenho: de onde as chamadas que chegam numa
    /// trilha já à vista entram com fade (`entrada_da_chamada`). No histórico, nada entra.
    entrada: (usize, usize),
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
            entrada: (usize::MAX, usize::MAX),
        }
    }

    /// Chamada ainda sem resultado no turno que está rodando.
    fn pendente(&self, indice: usize, item: &Item) -> bool {
        matches!(item, Item::Ferramenta { resultado: None, .. })
            && self.rodando
            && self.marco.is_none_or(|m| indice > m)
    }
}

/// Linha de log (chamada, evento, cabeçalho de trilha): 28 de altura, o ícone de 14 numa coluna de
/// 16 e o texto a 8 dela. Coladas, dão o passo de 28 (eram 32: sete linhas comiam mais da metade
/// da conversa; rodada de concisão, 17/11/2026).
const ALTURA_DO_LOG: Rems = r(22.);

/// As duas bordas da conversa (18/11/2026). A primeira é a do texto da resposta, e nela ficam
/// também o ícone de toda linha da trilha, o do erro, o avatar de quem entregou e a marca da vez.
/// A segunda fica a 24 dela (a coluna de 16 do ícone mais 8), e nela ficam o texto dessas linhas e
/// tudo o que elas abrem: a saída, o diff, o raciocínio, o relatório, a imagem. Eram cinco começos
/// diferentes (0, 12, 32, 34 e 56).
const RECUO_DO_LOG: Rems = r(21.);

/// A linha da trilha sangra 8 de cada lado da coluna de leitura: o fundo do hover precisa de folga
/// em volta do ícone, e sem a sangria o ícone ficava 12 para dentro da borda do texto.
fn linha_de_log<E: Styled>(linha: E) -> E {
    // O passo do protótipo Linear (25/11/2026): 22 de altura, 12,5 e 7 entre o ícone e o texto.
    linha.h(ALTURA_DO_LOG).mx(r(-8.)).px(esp::E8).gap(linear::PASSO_VAO).rounded(raio::CONTROLE).metadado().text_size(r(12.5))
}

/// A coluna de 16 do ícone de uma linha da trilha, na primeira borda.
fn coluna_do_icone() -> Div {
    div().flex_none().size(r(14.)).flex().items_center().justify_center()
}

/// O resumo do grupo de chamadas: "Rodou 13 comandos · editou 1 arquivo · leu 3 arquivos" no
/// painel largo e, fora dele, a forma curta sem os verbos, "13 comandos · 1 editado · 3 lidos":
/// cortado, o fim levava o último número junto ("leu 3 arqu…") (relatório de design, 16/11/2026).
pub(super) fn resumo_das_chamadas(chamadas: &[(usize, &Item)], curto: bool) -> String {
    let (mut comandos, mut leituras, mut buscas, mut outras, mut pensamentos) = (0, 0, 0, 0, 0);
    // As chamadas que mexem nos passos do agente não contam como "N ferramentas": com as
    // ferramentas de tarefa, cada passo criado, começado e concluído é uma chamada.
    let mut passos = false;
    for (_, item) in chamadas {
        match item {
            Item::Raciocinio { .. } => pensamentos += 1,
            Item::Ferramenta { nome, .. } => match nome.as_str() {
                nome if crate::motor::passos::e_de_passos(nome) => passos = true,
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
        if passos { partes.push("passos".into()); }
        if partes.is_empty() && pensamentos > 0 { partes.push("Pensou".into()); }
    } else {
        if comandos > 0 { partes.push(format!("rodou {}", contagem(comandos, "comando", "comandos"))); }
        if !editados.is_empty() { partes.push(editou(&editados)); }
        if leituras > 0 { partes.push(format!("leu {}", contagem(leituras, "arquivo", "arquivos"))); }
        if buscas > 0 { partes.push(format!("fez {}", contagem(buscas, "busca", "buscas"))); }
        if outras > 0 { partes.push(format!("usou {}", contagem(outras, "ferramenta", "ferramentas"))); }
        if passos { partes.push("atualizou os passos".into()); }
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
    /// A resposta final, que fica embaixo da linha; no turno `ao_vivo`, o fim da conversa.
    pub resposta: usize,
    /// O turno que roda agora (só no modo foco): a linha diz o que ele faz e quantas chamadas fez.
    pub ao_vivo: bool,
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
            saida.push(TurnoRecolhivel { mensagem, resposta, ao_vivo: false });
        }
    }
    saida
}

/// Os turnos que recolhem nesta conversa: os do modo foco, ou os de sempre.
pub(super) fn turnos_da_conversa(itens: &[Item], foco: bool, rodando: bool) -> Vec<TurnoRecolhivel> {
    if foco { turnos_do_foco(itens, rodando) } else { turnos_recolhiveis(itens, rodando) }
}

/// Os turnos do modo foco, como a extensão do Claude Code no VS Code (dono, 23/11/2026: "tem que
/// ser igual aqui no VS Code"): de depois da mensagem até a resposta final, tudo o que o agente fez
/// (as chamadas, o raciocínio, as falas do meio) vira uma linha só, que abre no clique, e a
/// resposta fica embaixo. O turno que roda também, ao vivo: "Rodando cargo test · 18 chamadas".
pub(super) fn turnos_do_foco(itens: &[Item], rodando: bool) -> Vec<TurnoRecolhivel> {
    let marcos: Vec<usize> = itens
        .iter()
        .enumerate()
        .filter(|(_, i)| matches!(i, Item::Usuario { .. } | Item::Entrega { .. } | Item::Vez { .. }))
        .map(|(k, _)| k)
        .collect();
    let trabalho = |i: &Item| matches!(i, Item::Ferramenta { .. } | Item::Raciocinio { .. } | Item::Assistente { .. });
    let mut saida = Vec::new();
    for (n, &mensagem) in marcos.iter().enumerate() {
        let fim = marcos.get(n + 1).copied().unwrap_or(itens.len());
        if crate::motor::e_revisao_da_memoria(&itens[mensagem]) {
            continue;
        }
        if rodando && fim == itens.len() {
            if itens[mensagem + 1..].iter().any(|i| matches!(i, Item::Ferramenta { .. } | Item::Raciocinio { .. })) {
                saida.push(TurnoRecolhivel { mensagem, resposta: fim, ao_vivo: true });
            }
            continue;
        }
        let Some(resposta) = itens[mensagem + 1..fim].iter().rposition(|i| matches!(i, Item::Assistente { .. })).map(|k| mensagem + 1 + k)
        else {
            continue;
        };
        if itens[mensagem + 1..resposta].iter().any(trabalho) {
            saida.push(TurnoRecolhivel { mensagem, resposta, ao_vivo: false });
        }
    }
    saida
}

/// A linha do turno que roda, no modo foco: o que ele faz agora e quantas chamadas fez.
fn resumo_ao_vivo(itens: &[Item], turno: TurnoRecolhivel) -> String {
    let trecho = &itens[turno.mensagem + 1..turno.resposta.min(itens.len())];
    let chamadas = trecho.iter().filter(|i| matches!(i, Item::Ferramenta { .. })).count();
    let agora = trecho.iter().rev().find_map(|i| match i {
        Item::Ferramenta { nome, resumo, .. } => {
            let detalhe = detalhe_da_chamada(nome, resumo);
            let verbo = verbo_da_ferramenta(nome, true);
            Some(if detalhe.is_empty() { verbo.to_string() } else { format!("{verbo} {detalhe}") })
        }
        Item::Raciocinio { .. } => Some("Pensando".to_string()),
        _ => None,
    });
    let mut partes = vec![agora.unwrap_or_else(|| "Trabalhando".to_string())];
    if chamadas > 0 {
        partes.push(contagem(chamadas, "chamada", "chamadas"));
    }
    partes.join(" · ")
}

/// O que fica à vista com o turno recolhido: o texto intermediário que termina em pergunta, o
/// aviso de erro e o cartão de falha.
pub(super) fn fica_fora_do_recolhido(item: &Item) -> bool {
    match item {
        Item::Assistente { texto, .. } => texto.trim_end().ends_with('?'),
        // O marco de compactação diz onde o contexto foi cortado: fica à vista.
        Item::Aviso { texto, erro, .. } => {
            *erro || tipo_aviso(texto, *erro) == ClasseAvisoChat::Alerta || texto.starts_with(crate::motor::CONTEXTO_COMPACTADO)
        }
        Item::Ferramenta { .. } => plano_na_conversa(item).is_some(),
        // O fim de cada agente em segundo plano: é por ele que se abre o relatório, e são poucos.
        Item::FimDoAgente { .. } => true,
        _ => false,
    }
}

/// O plano de uma chamada ExitPlanMode como fica na conversa depois de respondido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PlanoNaConversa<'a> {
    /// "Plano aprovado", "Plano recusado" ou, sem resposta (o turno parou antes), "Plano proposto".
    pub titulo: &'static str,
    pub texto: &'a str,
    pub aprovado: bool,
    /// O que você pediu para mudar ao recusar, se escreveu.
    pub mudancas: Option<&'a str>,
}

/// O cartão do plano de um item da conversa: só a chamada ExitPlanMode que guardou o texto do
/// plano (as de antes disto seguem como linha da trilha). É também o que parte a trilha: a busca
/// (`busca_conversa`) usa a mesma regra.
pub(super) fn plano_na_conversa(item: &Item) -> Option<PlanoNaConversa<'_>> {
    let Item::Ferramenta { nome, detalhe: Some(texto), resultado, erro, .. } = item else {
        return None;
    };
    if nome != "ExitPlanMode" || texto.trim().is_empty() {
        return None;
    }
    let (titulo, aprovado) = match (resultado, erro) {
        (None, _) => ("Plano proposto", false),
        (Some(_), false) => ("Plano aprovado", true),
        (Some(_), true) => ("Plano recusado", false),
    };
    let mudancas = resultado.as_deref().filter(|_| *erro).and_then(pedido_de_mudancas);
    Some(PlanoNaConversa { titulo, texto, aprovado, mudancas })
}

/// O comentário com que o plano foi devolvido, tirado da recusa que o modelo leu
/// (`motor::recusa_explicada`); `None` na recusa sem comentário.
pub(super) fn pedido_de_mudancas(resultado: &str) -> Option<&str> {
    resultado.trim().strip_prefix("O usuário pediu mudanças no plano:").map(str::trim).filter(|t| !t.is_empty())
}

/// O plano ainda espera a sua resposta: o pedido dele está aberto, logo abaixo das mensagens.
fn plano_esperando(motor: &Motor, chat: &str, item: &Item) -> bool {
    matches!(item, Item::Ferramenta { resultado: None, .. })
        && motor.permissoes.iter().any(|p| p.chat == chat && p.ferramenta == "ExitPlanMode")
}

/// A linha viva do turno que não tem chamada rodando nem texto chegando: o Claude Code resumindo
/// a conversa, os agentes do fluxo trabalhando depois da resposta, ou o modelo pensando.
pub(super) fn rotulo_do_turno_calado(compactando: bool, esperando_agentes: bool) -> &'static str {
    if compactando {
        "Compactando…"
    } else if esperando_agentes {
        "Agentes trabalhando em segundo plano…"
    } else {
        "Pensando…"
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
    /// As chamadas que mudaram o arquivo (para abrir a edição delas à direita).
    pub itens: Vec<usize>,
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
                a.itens.push(i);
            }
            None => lista.push(ArquivoMudado { caminho, entram, saem, novo, itens: vec![i] }),
        }
    }
    // O que mudou neste turno não se repete em "Antes".
    antes.retain(|a| !neste.iter().any(|n| n.caminho == a.caminho));
    (neste, antes)
}

/// A lista de passos do agente em texto, um por linha: ✓ feito, → fazendo (no gerúndio dele),
/// ○ falta.
pub(super) fn texto_dos_passos(passos: &[crate::motor::Passo]) -> String {
    use crate::motor::EstadoDoPasso::*;
    passos
        .iter()
        .map(|p| match p.estado {
            Feito => format!("✓ {}", p.texto),
            Fazendo => format!("→ {}", p.fazendo),
            Falta => format!("○ {}", p.texto),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Os arquivos de um registro antigo do Codex, de antes de guardar o diff: o resultado da chamada
/// era a lista "mudou <caminho>" (ou "criou", "apagou"), um por linha, e a conversa mostrava uma
/// caixa com ela (dono, 16/11/2026: "uma linha por arquivo no lugar da caixa"). Vazio quando o
/// resultado não é essa lista.
pub(super) fn arquivos_do_registro_antigo(resultado: &str) -> Vec<(&str, &str)> {
    let lista: Vec<(&str, &str)> = resultado
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map_while(|l| {
            let (verbo, caminho) = l.split_once(' ')?;
            (matches!(verbo, "mudou" | "criou" | "apagou") && !caminho.trim().is_empty()).then_some((verbo, caminho.trim()))
        })
        .collect();
    if lista.len() == resultado.lines().filter(|l| !l.trim().is_empty()).count() { lista } else { Vec::new() }
}

/// Um arquivo que as chamadas de um turno (ou de um grupo) editaram, para a linha dele: o caminho
/// como a chamada deu, o +N −M somado, se é novo, as chamadas (para abrir a edição) e se é de um
/// registro antigo, sem o diff (o caminho então vem do resultado).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Editado {
    pub caminho: String,
    pub entram: usize,
    pub saem: usize,
    pub novo: bool,
    pub itens: Vec<usize>,
    pub antigo: bool,
}

/// Os arquivos editados nas chamadas, um por arquivo, na ordem em que apareceram.
pub(super) fn editados(chamadas: &[(usize, &Item)]) -> Vec<Editado> {
    let mut lista: Vec<Editado> = Vec::new();
    let mut juntar = |novo: Editado| match lista.iter_mut().find(|e| e.caminho == novo.caminho) {
        Some(e) => {
            e.entram += novo.entram;
            e.saem += novo.saem;
            e.novo |= novo.novo;
            e.antigo |= novo.antigo;
            e.itens.extend(novo.itens);
        }
        None => lista.push(novo),
    };
    for (i, item) in chamadas {
        let Item::Ferramenta { nome, arquivo, resumo, resultado, erro: false, detalhe, mudanca, .. } = item else {
            continue;
        };
        if !matches!(nome_curto(nome), "Edit" | "MultiEdit" | "Write" | "NotebookEdit") {
            continue;
        }
        let antigos = match (detalhe, resultado) {
            (None, Some(r)) => arquivos_do_registro_antigo(r),
            _ => Vec::new(),
        };
        if antigos.len() > 1 {
            for (verbo, caminho) in antigos {
                juntar(Editado { caminho: caminho.to_string(), entram: 0, saem: 0, novo: verbo == "criou", itens: vec![*i], antigo: true });
            }
            continue;
        }
        let (entram, saem) = linhas_da_edicao_gravada(item).unwrap_or((0, 0));
        juntar(Editado {
            caminho: arquivo.clone().unwrap_or_else(|| resumo.clone()),
            entram,
            saem,
            novo: mudanca.is_some_and(|m| m.novo),
            itens: vec![*i],
            antigo: detalhe.is_none(),
        });
    }
    lista
}

/// Quantos arquivos editados aparecem embaixo de um recolhido; o resto vira "e mais N arquivos".
const EDITADOS_NO_RECOLHIDO: usize = 5;

/// Quanto o turno de agora mudou: arquivos e o total de linhas que entraram e saíram. Quem mostra
/// o total é a pílula das mudanças, que faz a própria conta sem copiar a conversa
/// (`pilulas::montar`, a cada quadro); esta é a régua com que os testes a conferem.
#[cfg(test)]
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

/// A partir de quantos segundos a linha de uma chamada terminada diz quanto ela levou.
pub(super) const CHAMADA_DEMORADA: u64 = 10;

/// "2 min 14 s": quanto a chamada levou, para a ponta da linha dela, quando foi demorada. Nada
/// para a que não terminou, para a rápida e para o subagente, que já mostra a conta dele.
pub(super) fn demora_da_chamada(item: &Item) -> Option<String> {
    match item {
        Item::Ferramenta { durou, resultado: Some(_), sub: None, .. } if *durou >= CHAMADA_DEMORADA => Some(duracao_do_turno(*durou)),
        _ => None,
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

/// A linha do turno recolhido: "Trabalhou 6 min 12 s · 13 comandos · cargo test falhou 2×, passou
/// · editou carrinho.py e motor.rs" (as checagens, em `checagens.rs`). A duração é a hora do último item menos a da mensagem; sem hora (conversa de antes
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
    // As checagens que o turno rodou e como cada uma terminou, ao lado dos comandos.
    partes.extend(super::checagens::checagens_do_turno(trecho).iter().map(super::checagens::Checagem::texto));
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

/// O resumo de uma linha da trilha ("Rodou 1 comando · editou x.rs · leu 3 arquivos") com as partes
/// inteiras: a parte que não cabe some inteira, com as que vêm depois dela, em vez de o texto
/// acabar em reticências no meio de uma palavra ("fez 1 bus…"). As partes quebram para uma segunda
/// linha, que a altura de uma linha só esconde. Uma parte sozinha maior que a linha é a única que
/// ainda se corta.
fn partes_inteiras(resumo: &str) -> Div {
    partes_com_falhas(resumo, false)
}

/// As partes inteiras e, com `falhas`, o "falhou" (ou "falhou 2×") de uma checagem do turno no
/// vermelho que a trilha já usa na chamada que falhou; o resto da parte fica no tom da linha.
fn partes_com_falhas(resumo: &str, falhas: bool) -> Div {
    div()
        .flex()
        .flex_wrap()
        .flex_shrink(1.)
        .min_w_0()
        .h(r(16.))
        .overflow_hidden()
        .gap_x(esp::E4)
        .children(resumo.split(" · ").enumerate().map(|(i, parte)| {
            let texto = if i == 0 { parte.to_string() } else { format!("· {parte}") };
            let vermelho = falhas.then(|| super::checagens::trecho_da_falha(&texto)).flatten();
            let marcas = vermelho.map(|trecho| (trecho, HighlightStyle { color: Some(Cores::vermelho()), ..Default::default() }));
            div().flex_none().max_w_full().truncate().child(StyledText::new(texto).with_highlights(marcas))
        }))
}

/// O que o subagente gastou ("18,4 mil tokens · 7 ferramentas · 00:48"), na ponta da linha dele
/// (numa chamada comum que demora, só o relógio). A tarefa vale mais que a conta: sem lugar para os dois, quem cede é a conta, uma parte inteira
/// por vez, dos tokens para o relógio, e a tarefa só começa a ser cortada depois que a conta saiu
/// toda. Em ordem invertida, a última parte (o relógio) é a primeira a ganhar lugar na linha, e as
/// que não cabem vão para a linha de baixo, escondida.
fn conta_do_subagente(partes: &[String]) -> Div {
    let ultima = partes.len().saturating_sub(1);
    div()
        .flex()
        .flex_row_reverse()
        .flex_wrap()
        // Encolhe muito antes da tarefa, que tem fator 1.
        .flex_shrink(1000.)
        .min_w_0()
        .h(r(16.))
        .overflow_hidden()
        .gap_x(esp::E6)
        .font_family(FONTE_MONO)
        .text_color(Cores::apagado())
        .children(partes.iter().enumerate().rev().map(|(i, parte)| {
            let texto = if i == ultima { parte.clone() } else { format!("{parte} ·") };
            div().flex_none().child(texto)
        }))
}

/// O trilho de um grupo aberto: um fio de 1 px pelo centro da coluna dos ícones, que liga o ícone
/// de cada linha ao da linha de cima e, se o grupo `continua`, desce até o fim do bloco (passando
/// ao lado da caixa aberta). Uma linha do tempo, como na referência do dono (21st.dev, "AI
/// Thinking and Tool calls"), no lugar dos ramos em curva, que pediam o recuo das linhas filhas.
/// O pai precisa ser `relative` e começar na primeira borda.
fn trilho_do_log(continua: bool) -> Div {
    // No fio forte: com 1 px de largura, o traço comum (8%) sumia no cartão.
    let fio = || div().absolute().left_0().w(px(1.)).bg(Cores::borda_forte());
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(r(7.))
        .w(px(1.))
        // Do topo da linha até 2 antes do ícone (que começa em 7).
        .child(fio().top_0().h(r(5.)))
        // De 2 depois do ícone (que acaba em 21) até o fim do bloco.
        .when(continua, |s| s.child(fio().top(r(23.)).bottom_0()))
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
                .map(linha_de_log)
                // Aberto, o trilho sai de baixo do ícone do grupo e desce pelas chamadas.
                .relative()
                .when(aberto, |s| s.child(div().absolute().left(r(15.)).top(r(23.)).bottom_0().w(px(1.)).bg(Cores::borda_forte())))
                .cursor_pointer()
                .hover(|s| s.bg(Cores::hover()))
                // O único ponto de Tab da trilha; Enter ou Espaço abre e fecha.
                .tab_index(0)
                .focus_visible(|s| s.shadow(halo_foco()))
                // Na coluna dos ícones, o da trilha, de onde descem os ramos das chamadas; o
                // chevron vai no fim, como na chamada e no "Pensou" (relatório de design,
                // 16/11/2026: à esquerda só aqui, à direita no resto).
                .child(coluna_do_icone().text_color(Cores::tenue()).child(icone_de(Lucide::ListTree, 14.)))
                // Sem o brilho andando: dentro das mensagens, cada batida dele redesenhava a conversa
                // inteira (ver `vista_mensagens`); o que anda é o anel da linha do turno. Parado, o
                // resumo corta em partes inteiras.
                .child(if ao_vivo {
                    div().min_w_0().truncate().text_color(Cores::texto()).child(resumo).into_any_element()
                } else {
                    partes_inteiras(&resumo).text_color(Cores::tenue()).into_any_element()
                })
                .when(falhas > 0, |s| {
                    s.child(Selo::cor(contagem(falhas, "falhou", "falharam"), Cores::vermelho()))
                })
                .when(rodando_escondida, |s| s.child(super::componentes::lugar_do_anel(contexto.painel, Cores::destaque(), 14.)))
                .child(div().flex_none().text_color(Cores::apagado()).child(chevron(&chave, aberto)))
                .on_click(cx.listener({ let chave = chave.clone(); move |j, _, _, cx| {
                    if aberto { j.expandidos.remove(&chave); j.expandidos.insert(format!("{chave}:fechado")); }
                    else {
                        j.expandidos.remove(&format!("{chave}:fechado"));
                        j.expandidos.insert(chave.clone());
                        j.abertos_agora.marcar(chave.clone(), std::time::Instant::now());
                    }
                    for p in j.paineis.iter_mut().filter(|p| p.seguindo) { p.manter_lugar = true; }
                    cx.notify();
                }}))
        });
        // Recolhido, o grupo ainda mostra as imagens que as chamadas escondidas leram ou gravaram.
        let escondidas = &chamadas[..chamadas.len() - mostradas.len()];
        let mut imagens_do_grupo: Vec<(usize, PathBuf)> = Vec::new();
        for (indice, item) in escondidas {
            for imagem in imagens::da_chamada(item, &contexto.chat.cwd) {
                if !imagens_do_grupo.iter().any(|(_, outra)| *outra == imagem) {
                    imagens_do_grupo.push((*indice, imagem));
                }
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
                    .pl(RECUO_DO_LOG)
                    .py(esp::E4)
                    .child(pilula_de_arquivo(format!("img-{}:{indice}:g", contexto.chat.id), imagem, &contexto.chat.id, false, cx))
                    .into_any_element(),
            ),
            varias => Some(pilha_de_imagens(&contexto.chat.id, varias, cx)),
        };
        // E os arquivos que elas editaram, uma linha cada, que abre a edição à direita.
        let editados_do_grupo = (recolhivel && !aberto).then(|| {
            let mais = cx.listener({
                let chave = chave.clone();
                move |j, _, _, cx| {
                    j.expandidos.remove(&format!("{chave}:fechado"));
                    j.expandidos.insert(chave.clone());
                    j.abertos_agora.marcar(chave.clone(), std::time::Instant::now());
                    cx.notify();
                }
            });
            self.editados_do_recolhido(contexto, &format!("{chave}:ed"), escondidas, mais)
        });
        let ultima = mostradas.len().saturating_sub(1);
        let (base, vistos) = contexto.entrada;
        let primeira = chamadas[0].0;
        // A chamada que chega numa trilha já à vista entra com fade e 8 px de baixo (a primeira
        // entra com o bloco); o cabeçalho, que nasce com a segunda, só com o fade.
        let indices: Vec<usize> = chamadas.iter().map(|(i, _)| *i).collect();
        let cabecalho = cabecalho.map(|c| match entrada_do_cabecalho(&indices, base, vistos) {
            Some(nova) => movimento::entrada(("entra-grupo", primeira), nova, De::Parado, c).into_any_element(),
            None => c.into_any_element(),
        });
        let linhas = v_flex().w_full().children(
            mostradas
                .iter()
                .enumerate()
                // Recolhido e ao vivo, a chamada à vista é uma linha solta, sem trilho.
                .map(|(k, (indice, item))| {
                    let linha = self.chamada(contexto, *indice, item, aberto.then_some(k < ultima), cx);
                    match entrada_da_chamada(*indice == primeira, *indice, base, vistos) {
                        Some(nova) => movimento::entrada(("entra-chamada", *indice), nova, De::Baixo, div().child(linha)).into_any_element(),
                        None => linha,
                    }
                }),
        );
        // Aberto pelo clique, o que estava escondido entra com fade e 6 px de cima.
        let linhas = if recolhivel && aberto { self.ao_abrir(&chave, linhas) } else { linhas.into_any_element() };
        v_flex()
            .w_full()
            .children(cabecalho)
            .children(editados_do_grupo.flatten())
            .children(fileira)
            .child(linhas)
            .into_any_element()
    }

    /// O que um clique acabou de abrir (`Janela::abertos_agora`) entra com fade e 6 px de cima, uma
    /// vez; remontado depois (rolar para longe e voltar, trocar de aba), aparece pronto.
    fn ao_abrir(&self, chave: &str, conteudo: impl IntoElement + Styled + 'static) -> AnyElement {
        if !self.abertos_agora.recente(chave, std::time::Instant::now()) {
            return conteudo.into_any_element();
        }
        movimento::entrada(SharedString::from(format!("abre-{chave}")), true, De::Cima, conteudo).into_any_element()
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
        // Os tokens e o custo do turno, quando o livro dos gastos os tem (`motor/gasto.rs`): só na
        // dica, para a linha continuar curta (dono, 18/11/2026).
        // Junto vão o modelo, os tempos e quanto da entrada veio do cache (`Livro::estatistica`).
        let (livro, quando) = (contexto.motor.gastos(), contexto.chat.itens[turno.mensagem].em());
        let gasto = livro.do_turno(&contexto.chat.id, turno.mensagem, quando);
        let estatistica = gasto.and_then(|_| livro.estatistica(&contexto.chat.id, turno.mensagem, quando));
        let resumo = if turno.ao_vivo { resumo_ao_vivo(&contexto.chat.itens, turno) } else { resumo_do_turno(&contexto.chat.itens, turno) };
        let dica = if aberto { "Recolher o que o agente fez neste turno" } else { "Ver o que o agente fez neste turno" };
        // As checagens também na dica: no painel estreito, a parte que não cabe na linha some inteira.
        let checagens = super::checagens::checagens_do_turno(&contexto.chat.itens[turno.mensagem + 1..turno.resposta]);
        let checagens = checagens.iter().map(super::checagens::Checagem::texto).collect::<Vec<_>>().join(" · ");
        let dica = if checagens.is_empty() { dica.to_string() } else { format!("{dica}\n{checagens}") };
        let dica = SharedString::from(super::onde_uso::dica_do_turno(&dica, gasto.as_ref(), estatistica.as_ref()));
        let linha = h_flex()
            .id(SharedString::from(format!("turno-{chave}")))
            .map(linha_de_log)
            .cursor_pointer()
            .hover(|s| s.bg(Cores::hover()))
            .tab_index(0)
            .focus_visible(|s| s.shadow(halo_foco()))
            .child(coluna_do_icone().text_color(Cores::tenue()).child(if turno.ao_vivo {
                super::componentes::lugar_do_anel(contexto.painel, Cores::destaque(), 14.).into_any_element()
            } else {
                icone_de(Lucide::Clock, 14.).into_any_element()
            }))
            .child(partes_com_falhas(&resumo, true).text_color(if turno.ao_vivo { Cores::texto() } else { Cores::tenue() }))
            .child(div().flex_none().text_color(Cores::apagado()).child(chevron(&chave, aberto)))
            .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            .on_click(alternar(chave.clone(), cx));
        let mut da_linha: Vec<(usize, PathBuf)> = Vec::new();
        if !aberto {
            for (indice, item) in escondidos {
                for imagem in imagens::da_chamada(item, &contexto.chat.cwd) {
                    if !da_linha.iter().any(|(_, outra)| *outra == imagem) {
                        da_linha.push((*indice, imagem));
                    }
                }
            }
        }
        let fileira = match da_linha.as_slice() {
            [] => None,
            [(indice, imagem)] => Some(
                h_flex()
                    .pl(RECUO_DO_LOG)
                    .py(esp::E4)
                    .child(pilula_de_arquivo(format!("img-{}:{indice}:t", contexto.chat.id), imagem, &contexto.chat.id, false, cx))
                    .into_any_element(),
            ),
            varias => Some(pilha_de_imagens(&contexto.chat.id, varias, cx)),
        };
        // Recolhido, cada arquivo que o turno editou fica numa linha embaixo dele, com o +N −M; o
        // clique abre o arquivo à direita já na edição (dono, 16/11/2026: "tinha que ter como abrir
        // pra ver o arquivo que editou e ver a edição").
        let editados = (!aberto)
            .then(|| self.editados_do_recolhido(contexto, &format!("{chave}:ed"), escondidos, alternar(chave.clone(), cx)))
            .flatten();
        // No último turno pronto, a ação discreta "Revisar com o Codex" na ponta da linha
        // (`mensagens/revisao.rs`); fora dele, a linha como sempre.
        let linha = match self.acao_de_revisar(contexto, turno, cx) {
            Some(acao) => h_flex().w_full().gap(esp::E8).child(div().flex_1().min_w_0().child(linha)).child(acao).into_any_element(),
            None => linha.into_any_element(),
        };
        v_flex().w_full().child(linha).children(editados).children(fileira).into_any_element()
    }

    /// Os arquivos editados nas chamadas escondidas de um recolhido (o turno ou o grupo), uma linha
    /// por arquivo; acima de cinco, "e mais N arquivos", que abre o recolhido (`mais`).
    fn editados_do_recolhido(
        &self,
        contexto: &Contexto,
        chave: &str,
        chamadas: &[(usize, &Item)],
        mais: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Option<AnyElement> {
        let lista = editados(chamadas);
        if lista.is_empty() {
            return None;
        }
        let sobra = lista.len().saturating_sub(EDITADOS_NO_RECOLHIDO);
        Some(
            v_flex()
                .w_full()
                .children(
                    lista
                        .iter()
                        .take(EDITADOS_NO_RECOLHIDO)
                        .enumerate()
                        .map(|(k, editado)| self.linha_de_editado(contexto, format!("{chave}:{k}"), editado, None)),
                )
                .when(sobra > 0, |s| {
                    s.child(
                        div()
                            .id(SharedString::from(format!("editados-mais-{chave}")))
                            .flex()
                            .items_center()
                            .h(r(28.))
                            .pl(RECUO_DO_LOG)
                            .metadado()
                            .text_color(Cores::tenue())
                            .cursor_pointer()
                            .hover(|s| s.text_color(Cores::texto()))
                            .child(format!("e mais {}", contagem(sobra, "arquivo", "arquivos")))
                            .on_click(mais),
                    )
                })
                .into_any_element(),
        )
    }

    /// A linha de um arquivo editado: o ícone (criou ou editou), o verbo quando é uma linha da
    /// trilha (tênue e sem peso, como o das chamadas; com `true`, na coluna de 56 do grupo), o nome
    /// em mono, o +N −M e o "novo". O clique abre o arquivo no painel da direita já
    /// na edição, rolado até a primeira mudança; no registro antigo, sem o diff, o arquivo como
    /// está agora, com um aviso. Um arquivo que saiu do disco fica apagado e sem clique.
    fn linha_de_editado(
        &self,
        contexto: &Contexto,
        chave: String,
        editado: &Editado,
        verbo: Option<(&'static str, bool)>,
    ) -> AnyElement {
        let existe = super::links_arquivo::resolver(&contexto.chat.cwd, &editado.caminho).is_some();
        let nome = Path::new(&editado.caminho).file_name().map_or_else(|| editado.caminho.clone(), |n| n.to_string_lossy().into_owned());
        let icone = if editado.novo { Lucide::FilePlus } else { Lucide::FilePenLine };
        let pedido = super::edicao::AbrirEdicao {
            chat: contexto.chat.id.clone(),
            itens: editado.itens.clone(),
            caminho: editado.antigo.then(|| editado.caminho.clone()),
        };
        let dica = SharedString::from(match (existe, editado.antigo) {
            (false, _) => format!("{} (não está mais no disco)", editado.caminho),
            (true, false) => format!("Ver a edição em {}", editado.caminho),
            (true, true) => format!("Abrir {} (este registro não guardou a edição)", editado.caminho),
        });
        h_flex()
            .id(SharedString::from(format!("editado-{chave}")))
            .map(linha_de_log)
            .child(coluna_do_icone().text_color(Cores::tenue()).child(icone_de(icone, 14.)))
            .when_some(verbo, |s, (verbo, em_grupo)| {
                s.child(div().flex_none().when(em_grupo, |s| s.min_w(r(56.))).text_color(Cores::tenue()).child(verbo))
            })
            .child(
                div()
                    .flex_shrink(1.)
                    .min_w_0()
                    // Corta no meio: a extensão fica à vista.
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_middle()
                    // Mono solto na cor do código, como o detalhe das chamadas (sem o chip).
                    .font_family(FONTE_MONO)
                    .text_color(if existe { Cores::codigo() } else { Cores::apagado() })
                    .child(nome),
            )
            .when(editado.entram + editado.saem > 0, |s| {
                s.child(
                    h_flex()
                        .flex_none()
                        .gap(esp::E6)
                        .font_family(FONTE_MONO)
                        .when(editado.entram > 0, |s| s.child(div().text_color(Cores::verde()).child(format!("+{}", editado.entram))))
                        .when(editado.saem > 0, |s| s.child(div().text_color(Cores::vermelho()).child(format!("−{}", editado.saem)))),
                )
            })
            .when(editado.novo, |s| s.child(Selo::cor("novo", Cores::verde())))
            .when(existe, |s| {
                s.cursor_pointer()
                    .hover(|s| s.bg(Cores::hover()))
                    .on_click(move |_, window, cx| window.dispatch_action(Box::new(pedido.clone()), cx))
            })
            .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            .into_any_element()
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
            .map(linha_de_log)
            .cursor_pointer()
            .hover(|s| s.bg(Cores::hover()))
            .child(coluna_do_icone().child(if rodando {
                super::componentes::lugar_do_anel(contexto.painel, Cores::tenue(), 14.).into_any_element()
            } else {
                icone_de(Lucide::Brain, 14.).text_color(Cores::tenue()).into_any_element()
            }))
            .child(div().min_w_0().truncate().text_color(Cores::tenue()).child(texto))
            .child(div().flex_none().text_color(Cores::apagado()).child(chevron(chave, aberta)))
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
    /// O plano depois de respondido, no ponto da conversa em que foi proposto: um cartão recolhido
    /// com o que você respondeu ("Plano aprovado", "Plano recusado") e o título do plano; o clique
    /// abre o texto inteiro e, no recusado, o que você pediu para mudar. Antes disto o plano só
    /// existia no pedido: respondido, sobrava uma chamada "ExitPlanMode" sem texto, escondida no
    /// "Trabalhou…".
    fn plano_respondido(&self, contexto: &Contexto, indice: usize, plano: PlanoNaConversa, cx: &Context<Self>) -> AnyElement {
        let chave = format!("{}:{indice}", contexto.chat.id);
        let realce = contexto.busca.realce(indice, Campo::Texto);
        let aberto = self.expandidos.contains(&chave);
        let (icone, cor) = match (plano.aprovado, plano.titulo) {
            (true, _) => (Lucide::CircleCheck, Cores::verde()),
            (false, "Plano recusado") => (Lucide::CircleX, Cores::tenue()),
            _ => (Lucide::Map, Cores::tenue()),
        };
        // O título do plano (a primeira linha dele, sem a marcação), para saber qual é sem abrir.
        let previa = sem_marcacao(plano.texto.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().trim_start_matches('#').trim());
        let cabecalho = h_flex()
            .id(SharedString::from(format!("plano-{chave}")))
            .h(r(36.))
            .px(esp::E12)
            .gap(esp::E8)
            .rounded(raio::CARTAO)
            .cursor_pointer()
            .hover(|s| s.bg(Cores::hover()))
            .tab_index(0)
            .focus_visible(|s| s.shadow(halo_foco()))
            .child(coluna_do_icone().child(icone_de(icone, 14.).text_color(cor)))
            .child(div().flex_none().rotulo().text_color(Cores::forte()).child(plano.titulo))
            .child(div().flex_1().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(if aberto { String::new() } else { previa }))
            .when(aberto, |s| {
                s.child(div().flex_none().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(
                    BotaoCopiar::new(SharedString::from(format!("copiar-plano-{chave}")), plano.texto.to_string()).dica("Copiar o plano"),
                ))
            })
            .child(div().flex_none().text_color(Cores::apagado()).child(chevron(&chave, aberto)))
            .on_click(alternar(chave.clone(), cx));
        let cartao = v_flex()
            .w_full()
            .rounded(raio::CARTAO)
            .bg(Cores::hover())
            .child(cabecalho)
            .when(aberto, |s| {
                s.child(self.ao_abrir(&chave,
                    v_flex()
                        .px(esp::E12)
                        .pb(esp::E12)
                        .gap(esp::E8)
                        .child(div().corpo().text_color(Cores::texto()).child(markdown_achado(
                            contexto,
                            format!("{chave}-plano").into(),
                            plano.texto.to_string(),
                            realce.clone(),
                        )))
                        .when_some(plano.mudancas, |s, mudancas| {
                            s.child(
                                h_flex()
                                    .items_start()
                                    .gap(esp::E6)
                                    .child(div().flex_none().pt(esp::E2).child(icone_de(Lucide::MessageSquareQuote, 14.).text_color(Cores::tenue())))
                                    .child(div().flex_1().min_w_0().metadado().text_color(Cores::texto()).child(format!("Você pediu: {}", sem_marcacao(mudancas)))),
                            )
                        }),
                ))
            })
            .into_any_element();
        // Aberto, o próprio texto pede para aparecer; fechado, quem rola até o cartão é o bloco.
        let realce = if aberto { Realce::default() } else { realce };
        RolarAoAchado::novo(format!("rolar-{chave}").into(), &realce, contexto.rolagem, cartao).into_any_element()
    }

    fn raciocinio(&self, contexto: &Contexto, indice: usize, texto: &str, milis: u64, ramo: Option<bool>, cx: &Context<Self>) -> AnyElement {
        let chave = format!("{}:{indice}", contexto.chat.id);
        let tem_texto = !texto.trim().is_empty();
        let aberto = tem_texto && self.expandidos.contains(&chave);
        let linha = h_flex()
            .id(SharedString::from(format!("raciocinio-{chave}")))
            .map(linha_de_log)
            .child(coluna_do_icone().child(icone_de(Lucide::Brain, 14.).text_color(Cores::tenue())))
            // O verbo como o das chamadas: tênue e sem peso, na coluna de 56 dentro de um grupo, com
            // a duração na coluna dos detalhes. Em negrito branco, o "Pensou" era a linha mais
            // forte da trilha, e é a que menos importa.
            .child(div().flex_none().when(ramo.is_some(), |s| s.min_w(r(56.))).text_color(Cores::tenue()).child("Pensou"))
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
                    .child(div().flex_none().text_color(Cores::apagado()).child(chevron(&chave, aberto)))
                    .on_click(alternar(chave.clone(), cx))
            });
        v_flex()
            .relative()
            .w_full()
            .child(linha)
            .when(aberto, |s| {
                s.child(self.ao_abrir(&chave,
                    div().pl(RECUO_DO_LOG).py(esp::E4).child(
                        div()
                            .id(SharedString::from(format!("raciocinio-texto-{chave}")))
                            .w_full()
                            .max_h(r(280.))
                            .overflow_y_scroll()
                            .py(esp::E8)
                            .px(esp::E12)
                            // O raio das caixas da conversa, como o relatório e o código.
                            .rounded(raio::CARTAO)
                            .bg(Cores::hover())
                            .metadado()
                            .text_color(Cores::tenue())
                            .child(texto.trim().to_string()),
                    ),
                ))
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
        let Item::Ferramenta {
            id: id_da_chamada,
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
        // O registro antigo do Codex com vários arquivos numa chamada só: uma linha por arquivo,
        // no lugar da caixa com a lista de "mudou …" (dono, 16/11/2026).
        let antigos = match (pedido, resultado) {
            (None, Some(r)) if !*erro && matches!(nome.as_str(), "Edit" | "Write") => arquivos_do_registro_antigo(r),
            _ => Vec::new(),
        };
        if antigos.len() > 1 {
            let n = antigos.len();
            return v_flex()
                .w_full()
                .children(antigos.into_iter().enumerate().map(|(k, (verbo, caminho))| {
                    let editado =
                        Editado { caminho: caminho.to_string(), entram: 0, saem: 0, novo: verbo == "criou", itens: vec![indice], antigo: true };
                    let verbo = match verbo {
                        "criou" => "Criou",
                        "apagou" => "Apagou",
                        _ => "Editou",
                    };
                    v_flex()
                        .relative()
                        .w_full()
                        .child(self.linha_de_editado(contexto, format!("{chave}:{k}"), &editado, Some((verbo, ramo.is_some()))))
                        .children(ramo.map(|continua| trilho_do_log(continua || k + 1 < n)))
                }))
                .into_any_element();
        }
        let aberto = self.expandidos.contains(&chave);
        let novo = mudanca.is_some_and(|m| m.novo);
        let pendente = contexto.pendente(indice, item);
        // O subagente (Task/Agent): quantas ferramentas usou e há quanto tempo roda (ou quanto
        // levou), na linha; o que ele faz agora, embaixo dela enquanto roda; e os passos dele,
        // ao abrir. É o cartão de subagente da referência do dono (21st.dev, 13/11/2026).
        // Como na extensão do Claude Code: os tokens, as ferramentas e o tempo (dono, 17/11/2026:
        // "testa essa parte de agents").
        let do_sub = sub.as_ref().map(|s| {
            let segundos = if s.fim > 0 { s.fim.saturating_sub(*em) } else { crate::modelo::agora().saturating_sub(*em) };
            let mut partes = Vec::new();
            if s.tokens > 0 {
                partes.push(format!("{} tokens", super::mapa_agentes::em_mil(s.tokens)));
            }
            partes.push(contagem(s.ferramentas as usize, "ferramenta", "ferramentas"));
            partes.push(relogio(segundos));
            (partes, s)
        });
        let atividade_viva = do_sub.as_ref().filter(|(_, s)| pendente && !s.atividade.is_empty()).map(|(_, s)| {
            h_flex()
                .pl(RECUO_DO_LOG)
                .h(r(22.))
                .gap(esp::E6)
                .metadado()
                .child(div().min_w_0().truncate().text_color(Cores::texto()).child(sem_marcacao(&no_fundo::acao_ao_vivo(&s.atividade))))
        });
        let passos = do_sub.as_ref().filter(|(_, s)| !s.passos.is_empty()).map(|(_, s)| {
            div().pl(RECUO_DO_LOG).pt(esp::E4).child(
                v_flex()
                    .w_full()
                    .py(esp::E8)
                    .px(esp::E12)
                    .gap(esp::E2)
                    .rounded(raio::CARTAO)
                    .bg(Cores::hover())
                    .mono_pequeno()
                    .text_color(Cores::tenue())
                    .children(s.passos.iter().map(|p| div().min_w_0().truncate().child(detalhe_do_passo(p)))),
            )
        });
        let meta_do_sub = do_sub.map(|(partes, _)| partes);
        // Há quanto tempo a chamada roda, na ponta da linha (depois de 3 s, para as rápidas não
        // piscarem um relógio): num comando longo é o que diz que ela segue viva.
        let rodando_ha = (pendente && item.em() > 0).then(|| crate::modelo::agora().saturating_sub(item.em())).filter(|s| *s >= 3);
        let meta_do_sub = meta_do_sub.or_else(|| rodando_ha.map(|s| vec![relogio(s)]));
        // Terminada, a que demorou diz quanto levou ("· 2 min 14 s") no lugar do relógio. Em Inter
        // e com o ponto: em mono, colado no comando, o "14 s" parecia um argumento dele.
        let demorou = demora_da_chamada(item);
        // O arquivo que a chamada leu ou mudou, se ainda está no disco: abre ao lado pelo ícone
        // que aparece no hover da linha.
        let abrir = arquivo
            .as_deref()
            .and_then(|a| super::links_arquivo::resolver(&contexto.chat.cwd, a))
            .map(|inteiro| super::links_arquivo::link_de(&inteiro, None));
        // Uma edição que deu certo, num arquivo que ainda está no disco: o clique na linha abre o
        // arquivo à direita já na edição (dono, 16/11/2026: "ver a edição"); o chevron, sozinho,
        // mostra o diff aqui mesmo.
        let edicao = (!*erro && !pendente && abrir.is_some() && matches!(nome_curto(nome), "Edit" | "MultiEdit" | "Write" | "NotebookEdit"))
            .then(|| super::edicao::AbrirEdicao { chat: contexto.chat.id.clone(), itens: vec![indice], caminho: None });
        // O subagente (Task/Agent): o clique na linha abre a conversa inteira dele numa aba, só para
        // ler, e o cartão fica aqui como o marco de quando ele saiu; o chevron, sozinho, mostra os
        // passos e o relatório aqui mesmo (como na edição). Dentro da conversa de um subagente não
        // há o que abrir: o Claude Code não grava a de um subagente de subagente à parte.
        let subagente = (matches!(nome_curto(nome), "Task" | "Agent")
            && contexto.chat.agente == crate::modelo::Agente::Claude
            && crate::motor::subconversa::partes_do_id(&contexto.chat.id).is_none())
        .then(|| (contexto.painel, contexto.chat.id.clone(), id_da_chamada.clone()));
        let grupo = SharedString::from(format!("chamada-{chave}"));
        // Sem resultado e fora do turno que está rodando: foi interrompida.
        let interrompida = resultado.is_none() && !pendente;
        let icone = if pendente {
            super::componentes::lugar_do_anel(contexto.painel, Cores::destaque(), 14.).into_any_element()
        } else {
            let cor = if *erro {
                Cores::vermelho()
            } else if interrompida {
                Cores::apagado()
            } else {
                Cores::tenue()
            };
            icone_de(icone_da_ferramenta(nome), 14.).text_color(cor).into_any_element()
        };
        // O +N −M contado com o texto inteiro quando a chamada chegou (item 5.1); as chamadas de
        // antes disso contam pelo detalhe gravado.
        let linhas_do_diff = linhas_da_edicao_gravada(item);
        let verbo = verbo_da_ferramenta(nome, pendente).to_string();
        let detalhe = sem_marcacao(&detalhe_da_chamada(nome, resumo));
        let detalhe = if detalhe.is_empty() && nome == "Bash" {
            pedido.as_deref().and_then(|p| p.strip_prefix("$ ")).unwrap_or(resumo).to_string()
        } else { detalhe };
        // Os passos do agente dizem o que mudou ("Começou: rodar os testes"). Na coluna mínima,
        // sem o verbo, a frase inteira vai no lugar do detalhe.
        let (verbo, detalhe) = match linha_dos_passos(nome, resumo, pendente) {
            Some((verbo, detalhe)) if minima(contexto.nivel) => (verbo.clone(), format!("{verbo} {detalhe}").trim().to_string()),
            Some(dos_passos) => dos_passos,
            None => (verbo, detalhe),
        };
        // Ctrl+F: o resumo da chamada fica na linha e o resultado dentro do bloco que ela abre.
        let (no_resumo, no_resultado) =
            (contexto.busca.realce(indice, Campo::Resumo), contexto.busca.realce(indice, Campo::Resultado));
        // A falha fica no ícone e na palavra, em vermelho, em qualquer largura. A linha inteira com
        // fundo vermelho pesava mais que a resposta e, com três falhas seguidas, virava uma parede.
        let meta = if *erro {
            Some(("falhou", Cores::vermelho()))
        } else if interrompida {
            Some(("interrompida", Cores::apagado()))
        } else {
            None
        };
        let (estreita, minima) = (!larga(contexto.nivel), minima(contexto.nivel));
        let dica = SharedString::from(format!(
            "{verbo} ({})\n{detalhe}{}{}",
            nome_curto(nome),
            meta.map(|(m, _)| format!(" · {m}")).unwrap_or_default(),
            if edicao.is_some() {
                "\nClique para ver a edição no arquivo"
            } else if subagente.is_some() {
                "\nClique para abrir a conversa dele numa aba"
            } else {
                ""
            }
        ));
        // O detalhe: o que é código (comando, caminho, padrão de busca, endereço, ferramenta MCP)
        // sai em mono dentro do chip; a prosa (a tarefa do Agent, a busca na web, a lista de
        // tarefas) sai em Inter, sem chip. Antes, o "conferir cupons" do Agent saía em mono solto
        // e o mesmo texto do Codex num chip (relatório de design, 16/11/2026).
        let codigo = arquivo.is_some()
            || matches!(nome.as_str(), "Bash" | "BashOutput" | "KillShell" | "Grep" | "Glob" | "WebFetch")
            || nome.starts_with("mcp__");
        // A coluna do verbo (56) só alinha os detalhes dentro de um grupo; na solta, ela deixava
        // uns 35 px entre "Rodou" e o chip. A linha dos passos é uma frase ("Criou 5 passos",
        // "Começou: rodar os testes"): sem a coluna, que abria um vão no meio dela.
        let em_grupo = ramo.is_some() && !crate::motor::passos::e_de_passos(nome);
        let linha = h_flex()
            .id(SharedString::from(format!("ferramenta-{chave}")))
            .map(linha_de_log)
            .cursor_pointer()
            .hover(|s| s.bg(Cores::hover()))
            .child(coluna_do_icone().child(icone))
            .when(!minima, |s| {
                s.child(
                    // O verbo é o rótulo (tênue, sem peso); o que importa é o detalhe ao lado.
                    div()
                        .flex_none()
                        .when(em_grupo, |s| s.min_w(r(56.)))
                        .text_color(Cores::tenue())
                        .child(verbo.clone()),
                )
            })
            .child(
                div()
                    .flex_shrink(1.)
                    .min_w_0()
                    .truncate()
                    .map(|s| {
                        // O que é código sai em mono solto, na cor do código; o chip escuro em toda
                        // linha virava uma tabela de pílulas (rodada de concisão, 17/11/2026).
                        if codigo {
                            s.font_family(FONTE_MONO).text_color(Cores::codigo())
                        } else {
                            s.text_color(Cores::texto())
                        }
                    })
                    .child(busca_conversa::realcar(if let Some(arquivo) = arquivo {
                        Path::new(arquivo).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(detalhe)
                    } else { detalhe }, &no_resumo)),
            )
            .when_some(meta_do_sub.filter(|_| !minima), |s, partes| s.child(conta_do_subagente(&partes)))
            .when_some(demorou.filter(|_| !minima), |s, durou| {
                s.child(div().flex_none().text_color(Cores::apagado()).child(format!("· {durou}")))
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
            .when_some(meta.filter(|_| *erro || !estreita), |s, (meta, cor)| {
                s.child(div().flex_none().text_color(cor).child(meta))
            })
            .group(grupo.clone())
            .map(|s| {
                let seta = div().flex_none().text_color(Cores::apagado()).child(chevron(&chave, aberto));
                if edicao.is_none() && subagente.is_none() {
                    return s.child(seta);
                }
                // Na edição, o chevron é um botão à parte: a linha abre o arquivo; ele, o diff aqui.
                // No subagente também: a linha abre a conversa dele; ele, os passos e o relatório.
                let alterna = alternar(chave.clone(), cx);
                let dica_chevron = match (edicao.is_some(), aberto) {
                    (true, true) => "Recolher o diff",
                    (true, false) => "Mostrar o diff aqui",
                    (false, true) => "Recolher os passos e o relatório",
                    (false, false) => "Mostrar os passos e o relatório aqui",
                };
                s.child(
                    div()
                        .id(SharedString::from(format!("diff-aqui-{chave}")))
                        .flex_none()
                        .size(r(20.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(raio::CONTROLE)
                        .hover(|s| s.bg(Cores::hover()))
                        .child(seta)
                        .dica_no_ponteiro(move |window, cx| Tooltip::new(dica_chevron).build(window, cx))
                        .on_click(move |evento, window, cx| {
                            cx.stop_propagation();
                            alterna(evento, window, cx);
                        }),
                )
            })
            .when_some(abrir.clone().filter(|_| edicao.is_none()), |s, alvo| {
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
                        .dica_no_ponteiro(|window, cx| Tooltip::new("Abrir o arquivo ao lado").build(window, cx))
                        .on_click(move |_, window, cx| {
                            // Não abre nem fecha o detalhe da linha.
                            cx.stop_propagation();
                            window.dispatch_action(Box::new(super::links_arquivo::AbrirLinkVitral(alvo.clone())), cx);
                        }),
                )
            })
            .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            .map(|s| match (edicao.clone(), subagente.clone()) {
                (Some(pedido), _) => s.on_click(move |_, window, cx| window.dispatch_action(Box::new(pedido.clone()), cx)),
                (None, Some((painel, chat, chamada))) => s.on_click(cx.listener(move |this, _, window, cx| {
                    this.abrir_subagente(painel, chat.clone(), chamada.clone(), window, cx);
                })),
                (None, None) => s.on_click(alternar(chave.clone(), cx)),
            });
        // Aberta, a linha mostra primeiro o que a chamada pediu (o diff, o que gravou). O resultado
        // de uma edição que deu certo é só "o arquivo foi atualizado": fica de fora.
        let so_o_pedido = pedido.is_some() && !*erro && resultado.is_some() && matches!(nome.as_str(), "Edit" | "MultiEdit" | "Write" | "NotebookEdit");
        // A imagem que a chamada leu ou gravou fica à vista embaixo da linha, sem precisar abrir
        // (dono, 13/11/2026: "dar pra ver as imagens que ele tá acessando… isso é o progresso
        // dele"). Entram também as que o resultado trouxe (o print do navegador por MCP, a imagem
        // gerada), que o Vitral guardou: mais de uma, lado a lado. O resultado de ler uma imagem
        // não tem texto: a caixa vazia não aparece.
        let da_chamada = imagens::da_chamada(item, &contexto.chat.cwd);
        let sem_texto = !da_chamada.is_empty() && resultado.as_deref().is_some_and(|r| r.trim().is_empty());
        let comando = (nome == "Bash").then(|| pedido.as_deref().and_then(|p| p.strip_prefix("$ ")).unwrap_or(resumo));
        let bloco = v_flex()
            .id(SharedString::from(format!("bloco-chamada-{chave}")))
            .relative()
            .w_full()
            .child(linha)
            .children(atividade_viva)
            .when(!da_chamada.is_empty(), |s| {
                s.child(h_flex().flex_wrap().gap(esp::E4).pl(RECUO_DO_LOG).py(esp::E4).children(da_chamada.iter().enumerate().map(|(k, imagem)| {
                    let id = if k == 0 { format!("img-{chave}") } else { format!("img-{chave}:{k}") };
                    pilula_de_arquivo(id, imagem, &contexto.chat.id, false, cx)
                })))
            })
            .when(aberto, |s| {
                // Aberto pelo clique, o que aparece entra com fade e 6 px de cima.
                s.child(self.ao_abrir(&chave, v_flex().w_full().children(passos).when_some(pedido.as_deref().filter(|_| comando.is_none()), |s, pedido| {
                    s.child(div().pl(RECUO_DO_LOG).pt(esp::E4).child(cartao_de_edicao(&chave, pedido)))
                })
                .when_some(comando, |s, comando| {
                    s.child(div().pl(RECUO_DO_LOG).py(esp::E4).child(
                        CaixaDeCodigo::new(SharedString::from(format!("terminal-{chave}")), resultado.as_deref()
                            .unwrap_or(if pendente { "A saída aparece quando terminar." } else { "(interrompido, sem saída)" }).to_string().into())
                            // A busca da conversa conta a saída do comando: o achado ganha o realce
                            // e a caixa rola até ele (conferência na bancada, 16/11/2026).
                            .realce(&no_resultado)
                            // "src/x.rs:120:5" na saída abre o arquivo na linha.
                            .arquivos(self.ligador(&contexto.chat.cwd, cx))
                            .comando(comando.to_string()).raio_chat().erro(*erro)
                            // O "Concluído" em toda saída não dizia nada: o comando que terminou bem
                            // é o caso comum, e a linha acima já marca o que falhou.
                            .when(pendente || *erro, |c| c.tempo(if pendente { "Em execução" } else { "Falhou" })),
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
                    s.child(div().pl(RECUO_DO_LOG).pt(esp::E6).pb(esp::E4).child(
                        div().px(esp::E12).metadado().font_family(FONTE_MONO).text_color(Cores::apagado()).child(texto),
                    ))
                })
                // O relatório do subagente é prosa: vai em texto, num cartão claro, e não na caixa
                // preta de saída de comando, já sem a moldura do Claude Code (dono, 17/11/2026:
                // "melhora o estilo dessas caixas pretas").
                .when_some(
                    (comando.is_none() && matches!(nome.as_str(), "Agent" | "Task") && !*erro)
                        .then(|| resultado.as_deref())
                        .flatten()
                        .map(crate::claude::relatorio_do_subagente)
                        .filter(|r| !r.is_empty()),
                    |s, relatorio| {
                        s.child(div().pl(RECUO_DO_LOG).py(esp::E4).child(
                            div()
                                .relative()
                                .group("relatorio")
                                .px(esp::E12)
                                .py(esp::E8)
                                .rounded(raio::CARTAO)
                                .bg(Cores::hover())
                                .text_size(r(14.))
                                .line_height(r(22.))
                                .text_color(Cores::texto())
                                .child(markdown(SharedString::from(format!("relatorio-{chave}")), relatorio.clone(), cx))
                                .child(
                                    div()
                                        .absolute()
                                        .top(esp::E6)
                                        .right(esp::E6)
                                        .invisible()
                                        .group_hover("relatorio", |s| s.visible())
                                        .child(BotaoCopiar::new(SharedString::from(format!("copiar-relatorio-{chave}")), relatorio).dica("Copiar o relatório")),
                                ),
                        ))
                    },
                )
                .when(
                    comando.is_none()
                        && !so_o_pedido
                        && !sem_texto
                        && !(pendente && resultado.is_none())
                        && !(matches!(nome.as_str(), "Agent" | "Task") && !*erro && resultado.as_deref().is_some_and(|r| !crate::claude::relatorio_do_subagente(r).is_empty())),
                    |s| {
                        s.child(div().pl(RECUO_DO_LOG).py(esp::E4).child(
                            bloco_resultado(&chave, resultado.as_deref(), *erro, "(interrompida, sem resultado)")
                                .realce(&no_resultado)
                                .arquivos(self.ligador(&contexto.chat.cwd, cx)),
                        ))
                    },
                )
                .when(so_o_pedido, |s| s.pb(esp::E4))))
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
            this.abertos_agora.marcar(chave.clone(), std::time::Instant::now());
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

/// O diff de uma edição aberto na trilha: o poço preto das caixas de código, com o Copiar no canto
/// só com o ponteiro em cima. Sem cabeçalho: ele repetia o nome do arquivo, que já está na linha
/// logo acima, e deixava o Copiar sempre à vista (nas outras caixas ele só aparece no hover).
fn cartao_de_edicao(chave: &str, pedido: &str) -> Div {
    let grupo = SharedString::from(format!("diff-{chave}"));
    // Só o que mudou, com contexto e a palavra trocada marcada; o Copiar leva o que a tela mostra.
    let diff = super::edicao::diff_da_trilha(pedido);
    let copiar = if diff.is_empty() { pedido.to_string() } else { super::edicao::texto_do_diff(&diff) };
    div()
        .relative()
        .group(grupo.clone())
        .w_full()
        .rounded(raio::CARTAO)
        .border_1()
        .border_color(Cores::borda())
        .bg(Cores::preto())
        .overflow_hidden()
        .child(bloco_do_pedido(pedido, &diff))
        .child(
            // Opaco: fica por cima do começo da primeira linha longa.
            div()
                .absolute()
                .top(esp::E4)
                .right(esp::E4)
                .rounded(raio::CONTROLE)
                .bg(Cores::preto())
                .invisible()
                .group_hover(grupo, |s| s.visible())
                .child(BotaoCopiar::new(SharedString::from(format!("copiar-diff-{chave}")), copiar).dica("Copiar o diff")),
        )
}

/// O que a chamada pediu (`Item::Ferramenta::detalhe`), em mono, numa caixa que rola de lado. Uma
/// edição sai como diff (`edicao::diff_da_trilha`): só as linhas que mudaram, com 3 de contexto,
/// o que saiu em vermelho, o que entrou em verde e a palavra trocada com o fundo mais forte.
/// Antes o lado antigo vinha inteiro em vermelho e o novo inteiro em verde (rodada do Paseo, item
/// 1). O que não é diff sai linha a linha, como veio.
fn bloco_do_pedido(pedido: &str, diff: &[LinhaDoDiff]) -> impl IntoElement + use<> {
    let linha = |texto: String, cor: Hsla, fundo: Option<Hsla>, trechos: Vec<(std::ops::Range<usize>, HighlightStyle)>| {
        div()
            .px(esp::E12)
            .whitespace_nowrap()
            .text_color(cor)
            .when_some(fundo, |s, fundo| s.bg(fundo))
            // A linha vazia mantém a altura.
            .child(StyledText::new(if texto.is_empty() { SharedString::from(" ") } else { SharedString::from(texto) }).with_highlights(trechos))
    };
    let (total, linhas): (usize, Vec<Div>) = if diff.is_empty() {
        let linhas = pedido.lines().take(LINHAS_DO_PEDIDO).map(|texto| {
            let (cor, fundo) = match texto.as_bytes().first() {
                Some(b'+') => (Cores::verde(), Some(fundo_entra())),
                Some(b'-') => (Cores::vermelho(), Some(fundo_sai())),
                Some(b'$') => (Cores::texto(), None),
                _ => (Cores::tenue(), None),
            };
            linha(texto.to_string(), cor, fundo, Vec::new())
        });
        (pedido.lines().count(), linhas.collect())
    } else {
        // O sinal e um espaço vão na frente do texto: os trechos andam 2 bytes.
        let marcados = |trechos: &[std::ops::Range<usize>], entra: bool| {
            trechos.iter().map(|t| (t.start + 2..t.end + 2, fundo_do_trecho(entra))).collect::<Vec<_>>()
        };
        let linhas = diff.iter().take(LINHAS_DO_PEDIDO).map(|l| match l {
            LinhaDoDiff::Igual(texto) => linha(format!("  {texto}"), Cores::tenue(), None, Vec::new()),
            LinhaDoDiff::Sai(texto, trechos) => linha(format!("- {texto}"), Cores::vermelho(), Some(fundo_sai()), marcados(trechos, false)),
            LinhaDoDiff::Entra(texto, trechos) => linha(format!("+ {texto}"), Cores::verde(), Some(fundo_entra()), marcados(trechos, true)),
            LinhaDoDiff::Pulo(0) => linha("…".into(), Cores::apagado(), None, Vec::new()),
            LinhaDoDiff::Pulo(n) => linha(format!("… {}", contagem(*n, "linha igual", "linhas iguais")), Cores::apagado(), None, Vec::new()),
            LinhaDoDiff::Nota(texto) => linha(texto.clone(), Cores::apagado(), None, Vec::new()),
        });
        (diff.len(), linhas.collect())
    };
    // O miolo tem a largura da linha mais longa (e ao menos a da caixa), e o fundo verde ou
    // vermelho de cada linha vai até o fim dele: antes a linha era cortada na beirada, sem como
    // ver o resto (dono, 17/11/2026: "tá bem zuado essas partes").
    let miolo = div()
        .flex_none()
        .min_w_full()
        .flex()
        .flex_col()
        .py(esp::E8)
        .children(linhas)
        .when(total > LINHAS_DO_PEDIDO, |s| {
            s.child(div().px(esp::E12).text_color(Cores::apagado()).child(format!("… mais {} linhas", total - LINHAS_DO_PEDIDO)))
        });
    // Sem fundo nem raio próprios: quem desenha a caixa é o cartão em volta (era uma caixa cinza
    // dentro da preta).
    div()
        .id("pedido-da-chamada")
        .w_full()
        .max_h(r(300.))
        .mono_pequeno()
        .flex()
        .child(miolo)
        .overflow_scrollbar()
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
        "TodoWrite" | "TaskCreate" | "TaskUpdate" | "TaskList" | "TaskGet" => Lucide::ListChecks,
        "AskUserQuestion" => Lucide::MessageCircleQuestionMark,
        "ExitPlanMode" => Lucide::Map,
        "ImageGeneration" => Lucide::Image,
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
        "TodoWrite" | "TaskUpdate" => ("Atualizou os passos", "Atualizando os passos"),
        "TaskCreate" => ("Criou um passo", "Criando um passo"),
        "TaskList" | "TaskGet" => ("Conferiu os passos", "Conferindo os passos"),
        "AskUserQuestion" => ("Perguntou", "Perguntando"),
        "ExitPlanMode" => ("Propôs um plano", "Propondo um plano"),
        // A imagem que o Codex gera (`imageGeneration`).
        "ImageGeneration" => ("Gerou uma imagem", "Gerando uma imagem"),
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

/// O verbo e o detalhe da linha de uma chamada que mexe nos passos do agente (o `TodoWrite` e as
/// ferramentas de tarefa): o que ela mudou, tirado da frase que o motor guardou no resumo
/// (`motor::passos`). "Começou: rodar os testes" vira o verbo "Começou:" e o detalhe "rodar os
/// testes"; "Criou 5 passos", "Criou" e "5 passos". Sem frase (as chamadas de antes disto, a que
/// não mudou nada, o `TaskList`), o verbo de sempre, sem detalhe: o "lista de afazeres" ao lado
/// de "Atualizou os passos" não dizia nada. `None` nas outras ferramentas.
fn linha_dos_passos(nome: &str, resumo: &str, ao_vivo: bool) -> Option<(String, String)> {
    if !crate::motor::passos::e_de_passos(nome) {
        return None;
    }
    Some(match crate::motor::passos::frase_do_resumo(nome, resumo) {
        Some(frase) => match frase.split_once(' ') {
            Some((verbo, detalhe)) => (verbo.to_string(), detalhe.to_string()),
            None => (frase.to_string(), String::new()),
        },
        None => (verbo_da_ferramenta(nome, ao_vivo).to_string(), String::new()),
    })
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
    const MARCOS: [&str; 7] = [
        "Sessão nova",
        "Interrompido",
        "Saiu do modo Planejar",
        "Retomando a sessão",
        crate::importar::MARCO,
        VITRAL_FECHADO,
        crate::motor::CONTEXTO_COMPACTADO,
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
    // Um desenho só para o que não é fala nem trabalho (18/11/2026): a nota da conversa, a mesma
    // do dia. Eram três: o dia centrado em negrito, o marco centrado entre dois fios com um ícone
    // e o aviso à esquerda com outro ícone e outro tamanho de letra.
    let rotulo = match tipo_aviso(texto, erro) {
        ClasseAvisoChat::Marco if texto.starts_with(SESSAO_ANTIGA) => "Sessão nova, com um resumo da anterior".to_string(),
        ClasseAvisoChat::Marco if texto.starts_with("Interrompido") => "Turno interrompido".to_string(),
        // Sem a instrução (a faixa logo abaixo já tem o Retomar, e nos antigos não há mais o que
        // retomar) e sem nome de agente (no chat do Codex o texto mandava o Claude continuar).
        // Vale também para os avisos já gravados (relatório de design, 16/11/2026).
        ClasseAvisoChat::Marco if texto.starts_with(VITRAL_FECHADO) => "O Vitral fechou no meio do turno".to_string(),
        // O erro vai para o cartão (`Janela::cartao_de_erro`); aqui só chega sem ser erro.
        _ => sem_marcacao(texto),
    };
    (Bloco::Marco, nota_da_conversa(busca_conversa::realcar(rotulo, realce)).into_any_element())
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
    /// Você parou o turno há pouco: "Você parou · Continuar" (item 6.2).
    VocePorou,
}

impl Situacao {
    /// O nome do estado, para a faixa entrar de novo quando ele troca (e não a cada segundo do
    /// relógio, nem a cada mudança de número dentro do mesmo estado).
    fn nome(&self) -> &'static str {
        match self {
            Situacao::Rodando { .. } => "rodando",
            Situacao::Parada { .. } => "parada",
            Situacao::Falhou => "falhou",
            Situacao::OrigemSumiu { .. } => "sumiu",
            Situacao::Fila(_) => "fila",
            Situacao::Fria { .. } => "fria",
            Situacao::VocePorou => "voce-parou",
        }
    }
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
        .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        .into_any_element()
}

/// A linha viva do turno que pensa, com o desenho de uma chamada em andamento na trilha: o anel
/// girando e o rótulo com o brilho passando.
fn linha_pensando(painel: u64, rotulo: &'static str, fonte: SharedString) -> AnyElement {
    h_flex()
        .h(ALTURA_DO_LOG)
        .gap(esp::E8)
        // Só o lugar do anel: ele gira por cima, numa vista própria (`AneisDaConversa`).
        .child(coluna_do_icone().child(super::componentes::lugar_do_anel(painel, Cores::destaque(), 14.)))
        .child(div().font_family(fonte).metadado().text_color(Cores::texto()).child(rotulo))
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
        // As pílulas do chat (os comandos no fundo, os subagentes, os passos, as mudanças do turno)
        // moram aqui, depois do que o turno faz; a linha existe com o turno vivo ou com alguma pílula.
        let pilulas = super::pilulas::pilulas_do_chat(motor, chat);
        let painel = self.paineis.iter().find(|p| p.id == painel_id);
        let medida = painel.map(|p| p.medida).unwrap_or_default();
        let (estreita, minima) = (!larga(medida.nivel), minima(medida.nivel));
        // Só pergunta se a linha é nova depois de saber que ela existe (quem não existe não conta como
        // vista, e a que aparece entra com fade).
        let na_linha = |cx: &Context<Self>| {
            let nova = painel.is_some_and(|p| p.ja_vistos.borrow_mut().novo("linha-de-estado"));
            self.pilulas_da_linha(motor, chat, &pilulas, painel_id, nova, estreita, cx)
        };
        // A revisão da memória roda por baixo, depois do "Terminou": só uma linha discreta, sem o
        // Esc (item 6.1). Uma mensagem sua a corta (ou espera, se ela já escreveu).
        // A revisão pelo outro agente, igual: por baixo, só lendo, e a sua mensagem a corta.
        let por_baixo = if motor.revendo_memoria(&chat.id) {
            Some("Revendo a memória…".to_string())
        } else {
            motor.revisando_o_par(&chat.id).map(|revisor| format!("{} revisando o turno…", revisor.nome()))
        };
        if let Some(texto) = por_baixo {
            let linha = h_flex()
                .flex_1()
                .min_w_0()
                .gap(esp::E8)
                .child(AnelGirando::new(12.).cor(Cores::tenue()).vivo(format!("memoria-{painel_id}")))
                .child(div().min_w_0().truncate().font_family(cx.theme().font_family.clone()).metadado().child(texto))
                .children(na_linha(cx));
            return Some((Some(linha.into_any_element()), None));
        }
        let Some(situacao @ Situacao::Rodando { .. }) = situacao_do_turno(motor, &chat.id) else {
            // Sem turno em andamento, a linha é só das pílulas (um comando que segue no fundo, o
            // que o turno mudou). Parada, a caixa não tem mais a linha das teclas.
            if !super::pilulas::linha_existe(false, &pilulas) {
                return None;
            }
            let linha = h_flex().flex_1().min_w_0().gap(esp::E8).children(na_linha(cx));
            return Some((Some(linha.into_any_element()), None));
        };
        // Com a conversa no fim, o que o turno faz já está à vista nela (a chamada em andamento, o
        // "Pensando…"): a linha da caixa não repete, fica com o relógio, a contagem e a fila
        // (dono, 13/11/2026: "por que aparece o rodando em cima e embaixo?"). Rolada para cima, a
        // conversa não mostra mais o fim, e aí a linha diz.
        let no_fim = painel.is_none_or(|p| p.seguindo);
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
                // O Claude Code resume a conversa; ou a resposta já veio e os agentes seguem
                // trabalhando: o agente e o que ele faz (ou quantos são), como nas linhas do fim
                // da conversa.
                let calado = !escrevendo && atividade == "Pensando…";
                let no_fundo = if !escrevendo && motor.esperando_agentes(&chat.id) { motor.linhas_do_segundo_plano(&chat.id) } else { Vec::new() };
                let atividade = if calado && motor.compactando(&chat.id) {
                    rotulo_do_turno_calado(true, false).to_string()
                } else if !no_fundo.is_empty() {
                    sem_marcacao(&no_fundo::atividade_no_fundo(&no_fundo))
                } else if calado {
                    rotulo_do_turno_calado(false, motor.esperando_agentes(&chat.id)).to_string()
                } else {
                    sem_marcacao(&atividade)
                };
                let agentes = super::mapa_agentes::agentes_vivos(motor, chat);
                (
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(esp::E8)
                        .child(AnelGirando::new(12.).cor(Cores::destaque_texto()).vivo(format!("estado-{painel_id}")))
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
                            // A dica do relógio diz a memória do chat (só no Linux com systemd).
                            let memoria = motor.memoria_do_chat(&chat.id).map(super::onde_uso::dica_da_memoria);
                            s.child(
                                div()
                                    .id(("relogio-do-turno", painel_id as usize))
                                    .flex_none()
                                    .etiqueta()
                                    .text_color(Cores::apagado())
                                    .child(relogio(crate::modelo::agora().saturating_sub(desde)))
                                    .when_some(memoria, |s, dica| s.dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))),
                            )
                        })
                        // Muito tempo sem evento: um comando longo, ou o processo travou. Só avisa;
                        // o Esc continua sendo o jeito de parar.
                        .children(motor.silencio(&chat.id).map(|calado| {
                            let minutos = calado.as_secs() / 60;
                            let dica: SharedString = format!(
                                "O agente não manda nada há {minutos} min. Se ele roda um comando longo, é normal; se travou, Esc para parar e mande de novo."
                            )
                            .into();
                            div()
                                .id(("sem-sinal", painel_id as usize))
                                .flex_none()
                                .etiqueta()
                                .text_color(Cores::espera())
                                .child(if estreita { format!("calado {minutos} min") } else { format!("sem sinal há {minutos} min") })
                                .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                        }))
                        // A mensagem que você mandou entrou neste turno, sem reiniciá-lo.
                        .when(dirigidas > 0, |s| s.child(selo_dirigido(painel_id, dirigidas, estreita)))
                        // A fila é um controle: o clique abre a lista do que espera, para tirar o
                        // que você não quer mais.
                        .when(fila > 0, |s| {
                            s.child(div().flex_none().child(self.pilula_da_fila(&chat.id, painel_id, fila, true, estreita, window, cx)))
                        })
                        // Por último: o que aparece depois (uma pílula nova) não move o que está à
                        // esquerda; o que cede é o texto do turno, com reticências.
                        .children(na_linha(cx))
                        .into_any_element(),
                    Some(match self.esc_armado.as_ref().filter(|(c, q)| *c == chat.id && q.elapsed() < std::time::Duration::from_secs(3)) {
                        // O Esc só armou: parar agora derruba os agentes (item 6.2).
                        Some(_) => {
                            let n = motor.agentes_no_fundo(&chat.id).max(agentes);
                            let rotulo = match n {
                                0 => "Esc de novo para parar o Ultracode".to_string(),
                                1 => "Esc de novo para parar o agente".to_string(),
                                n => format!("Esc de novo para parar os {n} agentes"),
                            };
                            h_flex()
                                .flex_none()
                                .gap(esp::E6)
                                .text_color(Cores::espera())
                                .children(teclas(&PararTurno, Some("Caixa"), window))
                                .child(rotulo)
                                .into_any_element()
                        }
                        None => esc("para parar", window),
                    }),
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
    ) -> Option<(AnyElement, &'static str)> {
        // O que roda (ou parou) vale mais; parado, a pasta que sumiu, e só então
        // a fila que espera.
        let situacao = match situacao_do_turno(motor, &chat.id) {
            Some(Situacao::Fila(n)) => situacao_do_chat(motor, chat).unwrap_or(Situacao::Fila(n)),
            Some(s) => s,
            // Por último, e só com o chat quieto: a conversa grande que esfriou. O que o último
            // turno mudou ("N arquivos mudados", que morava aqui desde 12/11) é agora a pílula das
            // mudanças, uma pílula na linha de estado da caixa (`pilulas.rs`), que não empurra a conversa.
            None => situacao_do_chat(motor, chat)
                // Logo depois de um Esc: o Continuar (não entra no sino; item 6.2).
                .or_else(|| motor.parou_por_voce(&chat.id).then_some(Situacao::VocePorou))
                .or_else(|| chat.fria(crate::modelo::agora()).map(|tokens| Situacao::Fria { tokens }))?,
        };
        // O turno em andamento mora na linha de estado da caixa, dentro da moldura
        // (`linha_de_estado`): acima da caixa só fica o que pede um botão seu.
        if matches!(situacao, Situacao::Rodando { .. }) {
            return None;
        }
        let estado = situacao.nome();
        let medida = self.paineis.iter().find(|p| p.id == painel_id).map(|p| p.medida).unwrap_or_default();
        let fila_aberta = self.paineis.iter().any(|p| p.id == painel_id && p.fila_aberta);
        // O botão da faixa (Retomar, Tentar de novo e as saídas da falha) leva este foco: o Ctrl+G
        // o põe nele ao chegar a uma parada ou a um erro (rodada de uso, item 3.4).
        let foco_faixa = self.paineis.iter().find(|p| p.id == painel_id).map(|p| p.foco_faixa.clone()).unwrap_or_else(|| cx.focus_handle());
        let estreita = !larga(medida.nivel);
        // Alinha com o texto da caixa (a margem da coluna mais o recuo do texto nela) e, à direita,
        // com o enviar.
        let caixa_estreita = medida.faixa == FaixaCaixa::Estreita;
        let margem = composer::margem_da_caixa(caixa_estreita).to_pixels(window.rem_size());
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
            // O "Entregar agora" manda tudo, as mensagens com hora marcada também.
            acao(|m, id, cx| m.entregar_fila_agora(id, cx)),
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
                            .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                    })
                    .into_any_element(),
                Some(
                    botao_fantasma("faixa-retomar", Some(Lucide::Play.into()), "Retomar", TamanhoBotao::Barra, window, cx)
                        .track_focus(&foco_faixa)
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
                        .track_focus(&foco_faixa)
                        .on_click(tentar)
                        .into_any_element()
                };
                // No limite: o instante em que ele volta, para "Continuar às 14:20".
                let volta = (tipo == erros::TipoDeErro::Limite)
                    .then(|| {
                        conta
                            .as_ref()
                            .filter(|_| agente == Agente::Claude)
                            .and_then(|c| motor.limite_vence(&c.id))
                            .or_else(|| erros::segundos_de_volta(&cru, crate::modelo::agora()))
                    })
                    .flatten();
                let agendada = motor.retomada_agendada(&chat.id);
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
                                .track_focus(&foco_faixa)
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
                        let id_conta = conta_que_caiu(motor, chat);
                        if let Some(c) = id_conta.as_ref().and_then(|id| motor.contas.iter().find(|c| &c.id == id)) {
                            (icone, cor, rotulo) = (Lucide::KeyRound, Some(Cores::espera()), format!("Login da {} caiu", c.nome));
                        }
                        botao_fantasma("faixa-entrar", Some(Lucide::KeyRound.into()), "Entrar de novo", TamanhoBotao::Barra, window, cx)
                            .track_focus(&foco_faixa)
                            .on_click(cx.listener(move |this, _, window, cx| this.entrar_de_novo_na_conta(id_conta.clone(), window, cx)))
                            .into_any_element()
                    }
                    _ => tentar_de_novo(),
                };
                // A volta sozinha: com ela agendada, a faixa diz a hora e o botão a cancela; sem
                // ela, o botão "Às 14:20" vai ao lado da outra saída.
                let acao = match (agendada, volta) {
                    (Some(quando), _) => {
                        rotulo = format!("Continua sozinho às {}", crate::modelo::hora_local(quando));
                        let (motor_e, id) = (self.motor.clone(), chat.id.clone());
                        botao_fantasma("faixa-cancelar-volta", Some(Lucide::X.into()), "Cancelar", TamanhoBotao::Barra, window, cx)
                            .track_focus(&foco_faixa)
                            .tooltip("Não continuar sozinho quando o limite voltar")
                            .on_click(move |_, _, cx| {
                                if let Some(m) = &motor_e {
                                    m.update(cx, |m, cx| m.cancelar_retomada(&id, cx));
                                }
                            })
                            .into_any_element()
                    }
                    (None, Some(quando)) => {
                        let hora = crate::modelo::hora_local(quando);
                        let (motor_e, id) = (self.motor.clone(), chat.id.clone());
                        h_flex()
                            .flex_none()
                            .gap(esp::E4)
                            .child(
                                botao_fantasma("faixa-agendar-volta", Some(Lucide::Clock.into()), format!("Às {hora}"), TamanhoBotao::Barra, window, cx)
                                    .tooltip(format!("Continuar este turno sozinho quando o limite voltar, às {hora}"))
                                    .on_click(move |_, _, cx| {
                                        if let Some(m) = &motor_e {
                                            m.update(cx, |m, cx| m.agendar_retomada(&id, quando, cx));
                                        }
                                    }),
                            )
                            .child(acao)
                            .into_any_element()
                    }
                    (None, None) => acao,
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
                                .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx)),
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
                        .tooltip("A conversa ficou parada além da hora em que o contexto fica guardado com desconto: a próxima mensagem relê tudo por inteiro. Compactar resume agora, sem mexer no que está escrito na caixa, e os próximos turnos releem só o resumo. Para compactar com foco, escreva “/compact” e a instrução na caixa.")
                        .on_click(cx.listener(move |this, _, _, cx| this.compactar_conversa(painel_id, cx)))
                        .into_any_element(),
                ),
            ),
            // Depois de um Esc: "Você parou", com o Continuar, que manda a mesma frase do Retomar.
            // O Enter com a caixa vazia faz o mesmo, e o fundo da caixa diz isso (item 6.2).
            Situacao::VocePorou => (
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(r(8.))
                    .child(icone_de(Lucide::CirclePause, 14.).text_color(Cores::tenue()))
                    .child(texto("Você parou".to_string()))
                    .into_any_element(),
                Some({
                    let (motor_e, id) = (self.motor.clone(), chat.id.clone());
                    botao_fantasma("faixa-continuar", Some(Lucide::Play.into()), "Continuar", TamanhoBotao::Barra, window, cx)
                        .tooltip("Pede ao agente para continuar de onde parou (o mesmo que o Enter com a caixa vazia)")
                        .on_click(move |_, _, cx| {
                            if let Some(m) = &motor_e {
                                m.update(cx, |m, cx| m.continuar_depois_do_esc(&id, cx));
                            }
                        })
                        .into_any_element()
                }),
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
        Some((
            h_flex()
                .map(|s| coluna(s, caixa_estreita))
                .pl(recuo)
                .pr(fim)
                .h(r(28.))
                .gap(esp::E8)
                .text_color(Cores::tenue())
                .child(esquerda)
                // A ação fica sempre inteira; quem corta é o texto, à esquerda.
                .children(direita.map(|d| div().flex_none().child(d)))
                .into_any_element(),
            estado,
        ))
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
    pub(super) fn ir_para_o_fim(&mut self, painel_id: u64, cx: &mut Context<Self>) {
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
        let pedidos: Vec<(String, String, serde_json::Value, Option<String>)> = motor
            .permissoes
            .iter()
            .filter(|p| p.chat == chat.id)
            .map(|p| (p.id.clone(), p.ferramenta.clone(), p.entrada.clone(), p.motivo_do_hook.clone()))
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
        // Cada pedido entra com fade e 8 px de baixo no desenho em que aparece (`ja_vistos` da casca
        // do painel); responder um e o próximo aparecer, ou trocar de conversa, faz o mesmo.
        let vistos = self.paineis.iter().find(|p| p.chat.as_deref() == Some(chat.id.as_str())).map(|p| &p.ja_vistos);
        let entra = move |id: &str, cartao: AnyElement| -> AnyElement {
            let nova = vistos.is_some_and(|v| v.borrow_mut().novo(format!("pedido:{id}")));
            movimento::entrada(SharedString::from(format!("entra-pedido-{id}")), nova, De::Baixo, div().child(cartao)).into_any_element()
        };
        // Os pedidos encolhem e rolam quando falta altura, para a caixa e as mensagens não saírem
        // da tela; vários de uma vez (chamadas em paralelo) nunca passam de 400 px.
        v_flex()
            .id(SharedString::from(format!("permissoes-{}", chat.id)))
            .map(|s| coluna(s, faixa == FaixaCaixa::Estreita))
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
            .children(pedidos.into_iter().enumerate().map(|(k, (id, ferramenta, entrada, motivo_do_hook))| {
                let corpo = aberto.as_deref() == Some(id.as_str());
                if ferramenta == "ExitPlanMode" {
                    return entra(&id, self.cartao_plano(motor, chat, &id, &entrada, nivel, corpo, window, cx));
                }
                let posicao = (quantos > 1).then_some((k + 1, quantos));
                // A pergunta com opções tem o cartão dela (sem pergunta legível, o pedido comum).
                let perguntas = if ferramenta == "AskUserQuestion" { super::pergunta::perguntas_de(&entrada) } else { Vec::new() };
                if !perguntas.is_empty() {
                    return entra(&id, self.cartao_pergunta(&id, &perguntas, corpo, posicao, nivel, window, cx));
                }
                entra(&id, self.cartao_permissao(chat, manual, &id, &ferramenta, &entrada, motivo_do_hook, corpo, posicao, nivel, window, cx))
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
        motivo_do_hook: Option<String>,
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
        // A regra só existe fora do perigoso e do que um hook mandou perguntar: o mesmo filtro do
        // desenho.
        let com_regra = regra_do_cartao(ferramenta, entrada, &chat.cwd, manual, motivo_do_hook.is_some()).is_some();
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
            motivo_do_hook,
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
        // A mesma frase do Enter na caixa com o pedido aberto (`Motor::responder_pedido_escrevendo`).
        let mensagem = (!texto.is_empty()).then(|| crate::motor::recusa_explicada("", &texto));
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
        // Tudo no cartão a 12 cheios, como os pedidos de permissão: o miolo da moldura não tem
        // borda para descontar (era dentro_da_borda, 11; folha dimensional, 23/11/2026).
        let cabecalho = h_flex()
            .id(id_cab)
            .flex_none()
            .h(r(44.))
            .px(esp::E12)
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
                    .px(esp::E12)
                    .py(esp::E12)
                    .corpo()
                    .text_color(Cores::texto())
                    .child(texto)
                    .overflow_y_scrollbar()
                    .id(id_rolagem),
            )
            .when_some(campo, |s, campo| {
                s.child(
                    div().flex_none().px(esp::E12).child(
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
            .child(div().flex_none().px(esp::E12).pt(esp::E12).pb(esp::E12).child(rodape));
        moldura_dupla(EstadoDaMoldura::Esperando).min_h_0().child(cartao.flex_1().min_h_0()).into_any_element()
    }
}

/// Largura máxima da coluna de leitura: 675 px a 100%, uns 80 caracteres por linha de resposta
/// (eram 806 px e perto de 100, linha longa demais para ler no painel único). Em rem, para o zoom
/// não mudar quantas letras cabem na linha.
const COLUNA_DE_LEITURA: Rems = r(760. + 2. * 18.);
/// A da caixa: os 768 da moldura com 8 de cada lado.
const COLUNA_DA_CAIXA: Rems = r(768. + 2. * 8.);
/// Altura que as mensagens guardam mesmo em janela baixa (120 px).
pub(super) const MINIMO_MENSAGENS: Rems = rems(7.5);
/// Até aqui o plano encolhe: título, umas linhas do texto, o campo e os botões (200 px).
const MINIMO_PLANO: Rems = rems(12.5);

/// A coluna da caixa e do que mora junto dela (pedidos de permissão, plano, fila, a faixa dos
/// arquivos mudados): alinham pelas mesmas bordas em qualquer largura de painel, a 8 do cartão
/// (protótipo Linear, 25/11/2026).
pub(super) fn coluna<E: Styled>(elemento: E, caixa_estreita: bool) -> E {
    elemento.w_full().max_w(COLUNA_DA_CAIXA).mx_auto().px(composer::margem_da_caixa(caixa_estreita))
}

/// A coluna das mensagens no protótipo Linear (25/11/2026): até 760 de texto, com 18 de cada lado.
/// A caixa e o que fica em volta dela (`coluna`) vão 4 além de cada lado, com 8 até o cartão.
pub(super) fn coluna_de_leitura<E: Styled>(elemento: E) -> E {
    elemento.w_full().max_w(COLUNA_DE_LEITURA).mx_auto().px(linear::ROLAGEM_LADO)
}

/// O painel está no fim da conversa (ou a menos de 48 px dele).
/// Quanto falta rolar até o fim. O deslocamento da roda só é limitado no prepaint: aqui ele
/// conta já limitado, para uma conversa curta não parecer fora do fim.
fn distancia_do_fim(rolagem: &ScrollHandle) -> Pixels {
    let maximo = rolagem.max_offset().y;
    maximo + rolagem.offset().y.clamp(-maximo, px(0.))
}

/// O que a lista guarda no fim, depois do último bloco: nada depois de uma resposta (os 32 vêm da
/// barra de copiar e da hora) e o respiro do turno depois da trilha ou de um aviso. As pílulas
/// moram na linha de estado da caixa, e não pedem lugar aqui.
fn pe_da_lista(ultimo_e_texto: bool) -> Option<Rems> {
    (!ultimo_e_texto).then_some(esp::NO_TURNO)
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
        .dica_no_ponteiro(|window, cx| Tooltip::new("Ir para o fim").action(&IrAoFim, None).build(window, cx))
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
        Item::Usuario { .. } | Item::Entrega { .. } => true,
        // O aviso de que os agentes seguem em segundo plano não fecha nada: eles continuam.
        aviso => aviso.aviso_que_fecha(),
    })
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
            Item::Usuario { .. } | Item::Entrega { .. } => false,
            outro => !outro.aviso_que_fecha(),
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
        "AskUserQuestion" => "Esperando a sua resposta…".into(),
        "ExitPlanMode" => "Esperando a sua aprovação…".into(),
        "mensagem_para_chat" => "Mandando mensagem a outra conversa…".into(),
        "TodoWrite" | "TaskCreate" | "TaskUpdate" | "TaskList" | "TaskGet" | "listar_tarefas" | "listar_chats" | "anotar_decisao"
        | "informar_progresso" => {
            format!("{verbo}…")
        }
        outro => match outro.strip_prefix("mcp__") {
            Some(resto) => format!("Usando {}…", resto.rsplit("__").next().unwrap_or(resto)),
            None => format!("{}…", resumo.trim_end_matches('…')),
        },
    }
}

/// Quantas linhas iguais ficam em volta de cada mudança no pedido de permissão de uma edição.
const CONTEXTO_DO_PEDIDO: usize = 2;

/// As linhas de uma edição para o pedido de permissão: o que sai e o que entra, com até duas
/// linhas iguais de cada lado como contexto e a palavra trocada marcada (o mesmo diff da trilha).
fn linhas_da_edicao(antigo: &str, novo: &str) -> Vec<LinhaDoDiff> {
    super::edicao::diff_da_troca(antigo, novo, CONTEXTO_DO_PEDIDO)
}

/// As linhas do pedido de um Edit: a troca (`old_string`, `new_string`) do Claude Code ou o diff
/// unificado (`diff`) que o Codex manda para cada arquivo.
fn linhas_do_pedido_de_edicao(entrada: &serde_json::Value) -> Vec<LinhaDoDiff> {
    let campo = |chave: &str| entrada[chave].as_str().unwrap_or_default();
    match entrada["diff"].as_str() {
        Some(diff) => super::edicao::diff_dos_trechos(&super::edicao::trechos(diff), CONTEXTO_DO_PEDIDO),
        None => linhas_da_edicao(campo("old_string"), campo("new_string")),
    }
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
    /// Um hook `PreToolUse` do usuário mandou perguntar, com este motivo (vazio se ele não deu):
    /// o cartão mostra a linha do hook e não oferece a caixa "Não perguntar de novo".
    pub motivo_do_hook: Option<String>,
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

/// A regra que a caixa do cartão oferece: a do pedido, menos no que um hook mandou perguntar (quem
/// pôs o hook quer ser perguntado toda vez; o motor também não deixa a regra valer para ele).
fn regra_do_cartao(ferramenta: &str, entrada: &serde_json::Value, pasta: &Path, manual: bool, do_hook: bool) -> Option<(String, String)> {
    if do_hook {
        return None;
    }
    regra_do_pedido(ferramenta, entrada, pasta, manual)
}

/// O maior motivo de hook que o cartão mostra inteiro.
const MAX_MOTIVO_DO_HOOK: usize = 600;

/// A linha do cartão que diz por que um pedido apareceu: um hook do usuário mandou perguntar, com
/// o motivo que ele deu, numa linha só e sem a marcação.
fn linha_do_hook(motivo: &str) -> String {
    let motivo = sem_marcacao(&motivo.split_whitespace().collect::<Vec<_>>().join(" "));
    if motivo.is_empty() {
        return "Um hook pediu a sua confirmação.".into();
    }
    let cortado: String = motivo.chars().take(MAX_MOTIVO_DO_HOOK).collect();
    let reticencias = if cortado.len() < motivo.len() { "…" } else { "" };
    format!("Um hook pediu a sua confirmação: {cortado}{reticencias}")
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
    let regra = regra_do_cartao(&p.ferramenta, entrada, &p.pasta, p.manual, p.motivo_do_hook.is_some());
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
                // Por que o pedido apareceu, quando foi um hook do usuário: a linha inteira, com
                // quebra (o motivo é o que ele precisa ler para decidir).
                .when_some(p.motivo_do_hook.as_deref().map(linha_do_hook), |s, linha| {
                    s.child(
                        h_flex()
                            .items_start()
                            .gap(esp::E6)
                            .child(div().flex_none().pt(esp::E2).child(icone_de(Lucide::ShieldAlert, 14.).text_color(Cores::ambar())))
                            .child(div().flex_1().min_w_0().metadado().text_color(Cores::texto()).child(linha)),
                    )
                })
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
            .child(bloco_de_linhas(id, &linhas_do_pedido_de_edicao(entrada), 80))
            .when(entrada["replace_all"].as_bool() == Some(true), |s| {
                s.child(nota("Troca todas as ocorrências no arquivo."))
            })
            .into_any_element(),
        "MultiEdit" => {
            let mut linhas = Vec::new();
            for (i, edicao) in entrada["edits"].as_array().into_iter().flatten().enumerate() {
                if i > 0 {
                    linhas.push(LinhaDoDiff::Pulo(0));
                }
                linhas.extend(linhas_da_edicao(
                    edicao["old_string"].as_str().unwrap_or_default(),
                    edicao["new_string"].as_str().unwrap_or_default(),
                ));
            }
            bloco_de_linhas(id, &linhas, 80).into_any_element()
        }
        "Write" => {
            let linhas: Vec<_> = campo("content").lines().map(|l| LinhaDoDiff::Entra(l.to_string(), Vec::new())).collect();
            bloco_de_linhas(id, &linhas, 20).into_any_element()
        }
        "NotebookEdit" if campo("edit_mode") == "delete" => nota("Apaga a célula.").into_any_element(),
        "NotebookEdit" => {
            let linhas: Vec<_> = campo("new_source").lines().map(|l| LinhaDoDiff::Entra(l.to_string(), Vec::new())).collect();
            bloco_de_linhas(id, &linhas, 20).into_any_element()
        }
        _ => {
            let json = serde_json::to_string_pretty(entrada).unwrap_or_default();
            let linhas: Vec<LinhaDoDiff> = json.lines().map(|l| LinhaDoDiff::Igual(l.to_string())).collect();
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
/// `limite`, diz quantas ficaram de fora. O sinal vai na cor, o texto em `texto` e o trecho
/// trocado dentro da linha com o fundo mais forte.
fn bloco_de_linhas(id: &str, linhas: &[LinhaDoDiff], limite: usize) -> impl IntoElement + use<> {
    let resto = linhas.len().saturating_sub(limite);
    caixa_de_codigo(id)
        .children(linhas.iter().take(limite).map(|linha| {
            let (sinal, cor, fundo, texto, trechos): (&str, Hsla, Option<Hsla>, &str, &[std::ops::Range<usize>]) = match linha {
                LinhaDoDiff::Pulo(0) => return div().my(r(4.)).h(px(1.)).bg(Cores::traco()).into_any_element(),
                LinhaDoDiff::Pulo(n) => {
                    return div().px(r(4.)).text_color(Cores::apagado()).child(format!("… {}", contagem(*n, "linha igual", "linhas iguais"))).into_any_element();
                }
                LinhaDoDiff::Nota(texto) => return div().px(r(4.)).text_color(Cores::apagado()).child(texto.clone()).into_any_element(),
                LinhaDoDiff::Sai(texto, trechos) => ("−", Cores::vermelho(), Some(fundo_sai()), texto, trechos),
                LinhaDoDiff::Entra(texto, trechos) => ("+", Cores::verde(), Some(fundo_entra()), texto, trechos),
                LinhaDoDiff::Igual(texto) => (" ", Cores::tenue(), None, texto, &[]),
            };
            let entra = matches!(linha, LinhaDoDiff::Entra(..));
            // A tabulação vira 4 espaços aqui (o diff de uma troca já vem assim; o Write e o JSON,
            // não), e os trechos só valem para o texto que não mudou de tamanho.
            let mostrado = if texto.is_empty() { " ".to_string() } else { texto.replace('\t', "    ") };
            let marcados: Vec<_> =
                if mostrado.len() == texto.len() { trechos.iter().map(|t| (t.clone(), fundo_do_trecho(entra))).collect() } else { Vec::new() };
            h_flex()
                .items_start()
                .gap(r(8.))
                .px(r(4.))
                .text_color(if fundo.is_some() { Cores::texto() } else { Cores::tenue() })
                .when_some(fundo, |s, f| s.bg(f))
                .child(div().flex_none().w(r(8.)).text_color(cor).child(sinal))
                .child(div().flex_1().min_w_0().child(StyledText::new(mostrado).with_highlights(marcados)))
                .into_any_element()
        }))
        .when(linhas.is_empty(), |s| s.child(div().text_color(Cores::tenue()).child("(vazio)")))
        .when(resto > 0, |s| {
            s.child(div().pt(r(4.)).text_color(Cores::tenue()).child(format!("+{resto} linhas")))
        })
}


/// A conta cujo login caiu num erro de login do chat: a do último turno, se ela está marcada
/// como desconectada; senão, a única desconectada; senão, a escolhida à mão no chat (depois de
/// reabrir o Vitral a do último turno se perde). `None` quando não dá para saber: aí as Contas
/// abrem com o "Entrar de novo" em cada conta.
fn conta_que_caiu(m: &crate::motor::Motor, chat: &Chat) -> Option<String> {
    let desconectadas: Vec<&crate::contas::Conta> = m.contas.iter().filter(|c| m.conta_desconectada(&c.id)).collect();
    m.conta_do_ultimo_turno(&chat.id)
        .filter(|c| m.conta_desconectada(&c.id))
        .or_else(|| (desconectadas.len() == 1).then(|| desconectadas[0]))
        .map(|c| c.id.clone())
        .or_else(|| chat.conta.clone().filter(|id| id != crate::contas::AUTOMATICA && m.contas.iter().any(|c| &c.id == id)))
}

#[cfg(test)]
mod testes {
    /// As pílulas moram na caixa (24/11/2026): o pé da lista não guarda lugar para elas. Depois de
    /// uma resposta ele fica como sempre (o espaço vem da barra de copiar), e depois da trilha ou
    /// de um aviso, 12.
    #[test]
    fn o_pe_da_lista_nao_guarda_lugar_para_pilulas() {
        assert_eq!(super::pe_da_lista(true), None);
        assert_eq!(super::pe_da_lista(false), Some(crate::tema::esp::NO_TURNO));
    }

    #[test]
    fn mensagens_acrescentadas_em_cima_preservam_o_ponto_de_leitura() {
        let antes = gpui_kit::px(-120.);
        let depois = super::deslocamento_apos_prepend(antes, gpui_kit::px(1800.), gpui_kit::px(3100.));
        assert_eq!(gpui_kit::px(1600.) + antes, gpui_kit::px(2900.) + depois);
        assert_eq!(super::deslocamento_apos_prepend(antes, gpui_kit::px(1800.), gpui_kit::px(1800.)), antes);
    }
    // Sem `super::*`: o `gpui_kit::*` lá de cima traz um `test` que troca o `#[test]`.
    use super::{
        Bloco, ClasseAvisoChat, comando_de_barra, detalhe_da_chamada, entrada_da_chamada, entrada_do_bloco, entrada_do_cabecalho, espaco_antes,
        com_quebras_duras, dica_da_pilha, linhas_da_edicao, resposta_repete_o_erro, texto_da_citacao, trechos_com_citacoes, marco_da_conversa, nome_e_id, previa, resumo_da_trilha, erros::partes_do_erro,
        resumo_das_chamadas, sem_marcacao,
        separadores_de_data, etiqueta_da_ferramenta, regra_do_pedido, texto_atividade, tipo_aviso,
        MAX_MOTIVO_DO_HOOK, linha_do_hook, regra_do_cartao,
        titulo_do_pedido, trechos_perigosos, verbo_da_ferramenta, fica_fora_do_recolhido,
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
            imagens: Vec::new(),
            durou: 0,
        }
    }

    fn usuario(texto: &str) -> Item {
        Item::Usuario { texto: texto.into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0, marca: None }
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
        // Protótipo Linear (25/11/2026): 10 entre as falas (o dia, o seu balão e a resposta).
        assert!(perto(px(Some(Texto), Balao), 10.));
        assert!(perto(px(Some(Log), Balao), 10.));
        assert!(perto(px(Some(Balao), Texto), 10.));
        assert!(perto(px(Some(Balao), Log), 10.));
        assert!(perto(px(Some(Balao), Marco), 10.));
        assert!(perto(px(Some(Marco), Dia), 10.));
        assert!(perto(px(Some(Dia), Balao), 10.));
        assert!(perto(px(Some(Marco), Balao), 10.));
        // Linhas de log coladas, no passo de 22.
        assert!(perto(px(Some(Log), Log), 0.));
        // Dentro da resposta, 7.
        assert!(perto(px(Some(Recebida), Log), 7.));
        assert!(perto(px(Some(Recebida), Recebida), 7.));
        assert!(perto(px(Some(Log), Recebida), 7.));
        assert!(perto(px(Some(Texto), Marco), 7.));
        assert!(perto(px(Some(Marco), Log), 7.));
        assert!(perto(px(Some(Texto), Log), 7.));
        assert!(perto(px(Some(Log), Alerta), 7.));
        assert!(perto(px(Some(Alerta), Texto), 7.));
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

    /// A chamada que chega numa trilha já à vista entra sozinha; a primeira entra com o bloco. O
    /// cabeçalho do grupo entra com a segunda chamada, e não quando a trilha inteira é nova.
    #[test]
    fn chamada_nova_entra_na_trilha_que_ja_estava_a_vista() {
        // Abriu com 5 itens; a trilha começou no 5 e já foi vista (vistos 6); chega a 6.
        assert_eq!(entrada_da_chamada(true, 5, 5, 6), None, "a primeira é o bloco");
        assert_eq!(entrada_da_chamada(false, 6, 5, 6), Some(true));
        assert_eq!(entrada_da_chamada(false, 6, 5, 7), Some(false), "no desenho seguinte, guardada sem recomeçar");
        assert_eq!(entrada_da_chamada(false, 3, 5, 7), None, "o que já estava na conversa não entra");
        // O cabeçalho nasce com a segunda.
        assert_eq!(entrada_do_cabecalho(&[5, 6], 5, 6), Some(true));
        assert_eq!(entrada_do_cabecalho(&[5, 6], 5, 7), Some(false));
        assert_eq!(entrada_do_cabecalho(&[5, 6, 7], 5, 7), Some(false), "a terceira não refaz o cabeçalho");
        // A trilha inteira nova (as duas no mesmo desenho): quem entra é o bloco.
        assert_eq!(entrada_do_cabecalho(&[5, 6], 5, 5), None);
        assert_eq!(entrada_do_cabecalho(&[5], 5, 5), None);
        // Trilha antiga (aberta com a conversa): nada.
        assert_eq!(entrada_do_cabecalho(&[1, 2], 5, 5), None);
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

    /// O que um hook mandou perguntar não oferece "Não perguntar de novo"; o resto, como antes.
    #[test]
    fn pedido_de_hook_nunca_vira_regra() {
        let pasta = Path::new("/home/v/loja");
        let deploy = json!({"command": "easypanel deploy central-worker"});
        assert_eq!(regra_do_cartao("Bash", &deploy, pasta, false, true), None);
        assert_eq!(regra_do_cartao("mcp__x__y", &json!({}), pasta, true, true), None);
        assert_eq!(regra_do_cartao("Bash", &deploy, pasta, false, false), regra_do_pedido("Bash", &deploy, pasta, false));
        assert_eq!(
            regra_do_cartao("Bash", &deploy, pasta, false, false).map(|(objeto, _)| objeto),
            Some("este comando".to_string())
        );
        // O perigoso continua sem regra, com hook ou sem.
        assert_eq!(regra_do_cartao("Bash", &json!({"command": "rm -rf build"}), pasta, true, false), None);
    }

    #[test]
    fn o_cartao_diz_o_motivo_que_o_hook_deu() {
        assert_eq!(
            linha_do_hook("Deploy do `central-worker` no expediente do galpão.\n  Confirme com o **dono**. "),
            "Um hook pediu a sua confirmação: Deploy do central-worker no expediente do galpão. Confirme com o dono."
        );
        // O hook que não disse por quê.
        assert_eq!(linha_do_hook(""), "Um hook pediu a sua confirmação.");
        assert_eq!(linha_do_hook(" \n "), "Um hook pediu a sua confirmação.");
        // Um motivo enorme não toma o cartão.
        let enorme = linha_do_hook(&"palavra ".repeat(500));
        assert!(enorme.ends_with('…'), "{enorme}");
        assert_eq!(enorme.chars().count(), "Um hook pediu a sua confirmação: ".chars().count() + MAX_MOTIVO_DO_HOOK + 1);
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

    /// Item 2: depois da resposta, com os agentes ainda em segundo plano, a conversa ganha o aviso
    /// de que eles seguem. Ele não fecha o que veio antes: o cartão do agente continua vivo (não
    /// "interrompida"), e a linha de estado continua dizendo dele.
    #[test]
    fn o_aviso_de_que_os_agentes_seguem_nao_interrompe_o_cartao_deles() {
        let agente = ferramenta("Agent", "Agent · Mapear o carrinho", None, false);
        let segue = Item::Aviso { texto: crate::modelo::AGENTES_SEGUEM.into(), erro: false, em: 0 };
        let itens = [usuario("lance o agente"), agente.clone(), Item::Assistente { texto: "Lancei.".into(), em: 0 }, segue];
        assert_eq!(marco_da_conversa(&itens, 0), Some(0));
        assert_eq!(texto_atividade(&itens, false, 0), "Subagente: Mapear o carrinho…");
        // O aviso de que o turno parou continua fechando.
        let parou = [usuario("lance o agente"), agente, Item::Aviso { texto: "Interrompido.".into(), erro: false, em: 0 }];
        assert_eq!(marco_da_conversa(&parou, 0), Some(2));
        assert_eq!(texto_atividade(&parou, false, 0), "Pensando…");
    }

    /// Item 3: no turno recolhido, a linha do fim de cada agente continua à vista, o que falhou e
    /// o que terminou bem. (A primeira versão deste teste recolhia o que terminou bem; na bancada,
    /// a linha "Terminou: …" sumia assim que o turno acabava, que é justo quando se quer abrir o
    /// relatório.)
    #[test]
    fn o_fim_do_agente_fica_fora_do_turno_recolhido() {
        use super::fica_fora_do_recolhido;
        let fim = |erro: bool| Item::FimDoAgente { chamada: "t".into(), descricao: "Mapear o carrinho".into(), erro, durou: 60, em: 0 };
        assert!(fica_fora_do_recolhido(&fim(true)));
        assert!(fica_fora_do_recolhido(&fim(false)));
    }

    #[test]
    fn edicao_mostra_o_que_sai_e_o_que_entra_com_pouco_contexto() {
        let antigo = "a\nb\nc\nvelho\nd\ne\nf";
        let novo = "a\nb\nc\nnovo\noutro\nd\ne\nf";
        // As mesmas linhas de antes do diff da trilha valer aqui (rodada do Paseo, item 1); o que
        // mudou foi o tipo, que agora leva o trecho trocado dentro da linha.
        use crate::ui::edicao::LinhaDoDiff::{Entra, Igual, Sai};
        let t = |texto: &str| texto.to_string();
        assert_eq!(
            linhas_da_edicao(antigo, novo),
            vec![
                Igual(t("b")),
                Igual(t("c")),
                Sai(t("velho"), vec![]),
                Entra(t("novo"), vec![]),
                Entra(t("outro"), vec![]),
                Igual(t("d")),
                Igual(t("e")),
            ]
        );
        assert_eq!(linhas_da_edicao("", "x"), vec![Entra(t("x"), vec![])]);
        assert_eq!(linhas_da_edicao("x\ny", "x"), vec![Igual(t("x")), Sai(t("y"), vec![])]);
        // Linhas repetidas no começo e no fim não se cruzam.
        assert_eq!(linhas_da_edicao("a\na", "a"), vec![Igual(t("a")), Sai(t("a"), vec![])]);
        // Uma palavra trocada no meio de um trecho: uma linha sai, uma entra, com o trecho marcado.
        assert_eq!(
            linhas_da_edicao("a\nb\ntotal = 1\nc\nd", "a\nb\ntotal = 2\nc\nd"),
            vec![Igual(t("a")), Igual(t("b")), Sai(t("total = 1"), vec![8..9]), Entra(t("total = 2"), vec![8..9]), Igual(t("c")), Igual(t("d"))]
        );
    }

    /// O pedido de edição do Codex traz o diff unificado do arquivo, e não a troca: o cartão mostra
    /// as linhas dele (antes ficava vazio).
    #[test]
    fn o_pedido_de_edicao_do_codex_mostra_o_diff() {
        use super::linhas_do_pedido_de_edicao;
        use crate::ui::edicao::LinhaDoDiff::{Entra, Igual, Sai};
        let t = |texto: &str| texto.to_string();
        let do_codex = json!({"file_path": "/p/a.rs", "diff": "@@ -1,3 +1,3 @@\n a\n-total = 1\n+total = 2\n c\n"});
        assert_eq!(
            linhas_do_pedido_de_edicao(&do_codex),
            vec![Igual(t("a")), Sai(t("total = 1"), vec![8..9]), Entra(t("total = 2"), vec![8..9]), Igual(t("c"))]
        );
        let do_claude = json!({"file_path": "/p/a.rs", "old_string": "x\ny", "new_string": "x"});
        assert_eq!(linhas_do_pedido_de_edicao(&do_claude), linhas_da_edicao("x\ny", "x"));
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
        assert_eq!(onde_abrir(AlvoDeAbertura::InicioDaResposta, &posicoes, None, px(900.), px(400.), Some(1), 6), Abertura::Em(px(60.)));
        // Com o separador "Novo" logo antes (do 40 ao 64, depois da mensagem), ele fica à vista no
        // alto: a abertura conta a partir dele (conferência na bancada, 16/11/2026).
        let novo = Some((1, px(40.)));
        assert_eq!(onde_abrir(AlvoDeAbertura::InicioDaResposta, &posicoes, novo, px(900.), px(400.), Some(1), 6), Abertura::Em(px(28.)));
        // Um "Novo" mais acima (antes de outro bloco) não muda a abertura.
        let antes = Some((0, px(0.)));
        assert_eq!(onde_abrir(AlvoDeAbertura::InicioDaResposta, &posicoes, antes, px(900.), px(400.), Some(1), 6), Abertura::Em(px(60.)));
        // Cabe: abre no fim, como sempre.
        assert_eq!(onde_abrir(AlvoDeAbertura::InicioDaResposta, &posicoes, novo, px(900.), px(1000.), Some(1), 6), Abertura::Fim);
        // Sem resposta depois da sua mensagem: no fim.
        assert_eq!(onde_abrir(AlvoDeAbertura::InicioDaResposta, &posicoes, None, px(900.), px(400.), None, 6), Abertura::Fim);
    }

    #[test]
    fn a_ancora_volta_no_mesmo_item_e_carrega_mais_quando_falta() {
        use super::{AlvoDeAbertura, Abertura, Ancora, ancora_de, onde_abrir};
        use gpui_kit::px;
        let posicoes = [(10, px(0.), px(100.)), (12, px(112.), px(400.)), (20, px(412.), px(500.))];
        // O alto da tela no 150: o bloco 12 (que vai até o 400) estava 38 abaixo do alto dele.
        let ancora = ancora_de(&posicoes, px(150.)).unwrap();
        assert_eq!(ancora, Ancora { item: 12, desloc: px(38.) });
        assert_eq!(onde_abrir(AlvoDeAbertura::Ancora(ancora), &posicoes, None, px(500.), px(200.), None, 30), Abertura::Em(px(150.)));
        // O item ficou dentro de uma trilha que começa antes: abre no começo dela.
        let dentro = Ancora { item: 15, desloc: px(10.) };
        assert_eq!(onde_abrir(AlvoDeAbertura::Ancora(dentro), &posicoes, None, px(500.), px(200.), None, 30), Abertura::Em(px(112.)));
        // Mais para cima do que está desenhado: carrega mais itens antes.
        let antes = Ancora { item: 3, desloc: px(0.) };
        assert_eq!(onde_abrir(AlvoDeAbertura::Ancora(antes), &posicoes, None, px(500.), px(200.), None, 30), Abertura::CarregarMais);
        // O Corrigir tirou o item: abre no fim.
        let sumiu = Ancora { item: 40, desloc: px(0.) };
        assert_eq!(onde_abrir(AlvoDeAbertura::Ancora(sumiu), &posicoes, None, px(500.), px(200.), None, 30), Abertura::Fim);
    }

    #[test]
    fn separador_novo_e_comeco_da_resposta() {
        use super::{Item, inicio_da_ultima_resposta, item_do_separador_novo};
        let com_hora = |item: Item, em: u64| match item {
            Item::Usuario { texto, anexos, ultrathink, ultracode, .. } => Item::Usuario { texto, anexos, ultrathink, ultracode, em, marca: None },
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
            Item::Usuario { texto, anexos, ultrathink, ultracode, .. } => Item::Usuario { texto, anexos, ultrathink, ultracode, em: t, marca: None },
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
            imagens: Vec::new(),
            durou: 0,
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
        assert_eq!(turnos, [TurnoRecolhivel { mensagem: 0, resposta: 7, ao_vivo: false }]);
        assert!(turnos[0].contem(1) && turnos[0].contem(6) && !turnos[0].contem(7) && !turnos[0].contem(0));
        // Os dois comandos deste turno são checagens (cargo test e cargo check), e desde a rodada
        // do Paseo (item 3) a linha diz como cada uma terminou, ao lado dos comandos. Antes disso
        // o esperado aqui era "Trabalhou 6 min 12 s · 2 comandos · editou carrinho.py e motor.rs".
        assert_eq!(
            resumo_do_turno(&itens, turnos[0]),
            "Trabalhou 6 min 12 s · 2 comandos · cargo test passou · cargo check passou · editou carrinho.py e motor.rs"
        );
        // O exemplo do pedido: a última execução de cada checagem, e as falhas de antes em "falhou N×".
        let falha = |comando: &str| ferramenta("Bash", comando, Some("Exit code 1"), true);
        let passa = |comando: &str| ferramenta("Bash", comando, Some("ok"), false);
        let checado = vec![
            em(usuario("faz"), 1_000),
            assistente("Vou rodar."),
            passa("npx tsc --noEmit"),
            falha("npx vitest run"),
            falha("npx vitest run"),
            passa("npx vitest run"),
            passa("ls"),
            passa("cargo test"),
            em(assistente("Pronto."), 1_062),
        ];
        let turno = turnos_recolhiveis(&checado, false)[0];
        assert_eq!(resumo_do_turno(&checado, turno), "Trabalhou 1 min 2 s · 6 comandos · tsc passou · vitest falhou 2×, passou · cargo test passou");
        // Um turno sem checagem fica como era.
        let sem = vec![em(usuario("faz"), 1_000), assistente("Vou olhar."), passa("ls"), em(assistente("Pronto."), 1_062)];
        assert_eq!(resumo_do_turno(&sem, turnos_recolhiveis(&sem, false)[0]), "Trabalhou 1 min 2 s · 1 comando");
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
    fn a_chamada_demorada_diz_quanto_levou() {
        use super::{CHAMADA_DEMORADA, Item, demora_da_chamada};
        use crate::modelo::Subagente;
        let chamada = |durou: u64, resultado: Option<&str>, erro: bool| match ferramenta("Bash", "cargo test", resultado, erro) {
            Item::Ferramenta { id, nome, resumo, resultado, erro, em, detalhe, arquivo, sub, mudanca, .. } => {
                Item::Ferramenta { id, nome, resumo, resultado, erro, em, detalhe, arquivo, sub, mudanca, durou, imagens: Vec::new() }
            }
            outro => outro,
        };
        assert_eq!(CHAMADA_DEMORADA, 10);
        assert_eq!(demora_da_chamada(&chamada(134, Some("ok"), false)).as_deref(), Some("2 min 14 s"));
        assert_eq!(demora_da_chamada(&chamada(10, Some("ok"), false)).as_deref(), Some("10 s"));
        assert_eq!(demora_da_chamada(&chamada(3_780, Some("ok"), false)).as_deref(), Some("1 h 3 min"));
        // A que falhou depois de muito tempo também diz.
        assert_eq!(demora_da_chamada(&chamada(45, Some("erro"), true)).as_deref(), Some("45 s"));
        // A rápida, a não medida (a que foi para o segundo plano, a de antes disto) e a que ainda
        // não tem resultado ficam como eram.
        assert_eq!(demora_da_chamada(&chamada(9, Some("ok"), false)), None);
        assert_eq!(demora_da_chamada(&chamada(0, Some("ok"), false)), None);
        assert_eq!(demora_da_chamada(&chamada(134, None, false)), None);
        // O subagente já mostra as ferramentas e o relógio dele na linha.
        let sub = match chamada(134, Some("ok"), false) {
            Item::Ferramenta { id, nome, resumo, resultado, erro, em, detalhe, arquivo, mudanca, durou, .. } => {
                Item::Ferramenta { id, nome, resumo, resultado, erro, em, detalhe, arquivo, sub: Some(Subagente::default()), mudanca, durou, imagens: Vec::new() }
            }
            outro => outro,
        };
        assert_eq!(demora_da_chamada(&sub), None);
        assert_eq!(demora_da_chamada(&usuario("oi")), None);
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
            imagens: Vec::new(),
            durou: 0,
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
            imagens: Vec::new(),
            durou: 0,
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

    fn chamada_do_plano(resultado: Option<&str>, erro: bool, detalhe: Option<&str>) -> Item {
        Item::Ferramenta {
            id: "p1".into(),
            em: 0,
            nome: "ExitPlanMode".into(),
            resumo: "ExitPlanMode".into(),
            resultado: resultado.map(str::to_string),
            erro,
            detalhe: detalhe.map(str::to_string),
            arquivo: None,
            sub: None,
            mudanca: None,
            imagens: Vec::new(),
            durou: 0,
        }
    }

    /// O plano continua na conversa depois de respondido: um cartão recolhido no ponto em que
    /// foi proposto, com o título dizendo o que você respondeu, que abre o texto do plano.
    #[test]
    fn o_plano_respondido_fica_na_conversa_como_cartao() {
        use super::{PlanoNaConversa, pedido_de_mudancas, plano_na_conversa};
        let texto = "# Trocar o cupom\n\n1. Ler.\n2. Aplicar.";
        let aprovado = chamada_do_plano(Some("User has approved your plan."), false, Some(texto));
        assert_eq!(
            plano_na_conversa(&aprovado),
            Some(PlanoNaConversa { titulo: "Plano aprovado", texto, aprovado: true, mudancas: None })
        );
        let recusado = chamada_do_plano(Some("O usuário pediu mudanças no plano: começa pelos testes"), true, Some(texto));
        assert_eq!(
            plano_na_conversa(&recusado),
            Some(PlanoNaConversa { titulo: "Plano recusado", texto, aprovado: false, mudancas: Some("começa pelos testes") })
        );
        // Recusado sem comentário (o "Pedir mudanças" vazio, o Negar do sino).
        assert_eq!(plano_na_conversa(&chamada_do_plano(Some("O usuário pediu para revisar o plano."), true, Some(texto))).unwrap().mudancas, None);
        assert_eq!(pedido_de_mudancas("O usuário negou esta ação."), None);
        // O turno parou antes da resposta: o plano fica, sem dizer que foi aprovado.
        assert_eq!(plano_na_conversa(&chamada_do_plano(None, false, Some(texto))).unwrap().titulo, "Plano proposto");
        // Sem o texto do plano (conversa de antes disto) e nas outras chamadas, a linha da trilha.
        assert_eq!(plano_na_conversa(&chamada_do_plano(Some("ok"), false, None)), None);
        assert_eq!(plano_na_conversa(&ferramenta("Bash", "Bash · ls", Some("ok"), false)), None);
        assert_eq!(plano_na_conversa(&usuario("oi")), None);
        // Com o turno recolhido em "Trabalhou…", o cartão fica à vista.
        assert!(fica_fora_do_recolhido(&aprovado) && fica_fora_do_recolhido(&recusado));
        assert!(!fica_fora_do_recolhido(&chamada_do_plano(Some("ok"), false, None)));
        assert!(!fica_fora_do_recolhido(&ferramenta("Bash", "Bash · ls", Some("ok"), false)));
        // A linha de quando não há o texto, e o que o turno espera enquanto o pedido está aberto.
        assert_eq!(verbo_da_ferramenta("ExitPlanMode", false), "Propôs um plano");
        assert_eq!(texto_atividade(&[usuario("oi"), chamada_do_plano(None, false, None)], false, 0), "Esperando a sua aprovação…");
    }

    #[test]
    fn a_pergunta_respondida_e_uma_linha_da_trilha() {
        assert_eq!(verbo_da_ferramenta("AskUserQuestion", false), "Perguntou");
        assert_eq!(verbo_da_ferramenta("AskUserQuestion", true), "Perguntando");
        let pendente = ferramenta("AskUserQuestion", "AskUserQuestion · Qual cor você prefere?", None, false);
        assert_eq!(texto_atividade(&[usuario("oi"), pendente], false, 0), "Esperando a sua resposta…");
        assert_eq!(super::detalhe_da_chamada("AskUserQuestion", "AskUserQuestion · Qual cor você prefere?"), "Qual cor você prefere?");
    }

    /// O marco de compactação: uma nota da conversa, que fica à vista com o turno recolhido, e a
    /// linha viva do turno dizendo "Compactando…" enquanto dura.
    #[test]
    fn o_marco_de_compactacao_e_a_linha_compactando() {
        use super::rotulo_do_turno_calado;
        let texto = crate::motor::marco_da_compactacao(false, Some(168_432));
        assert_eq!(tipo_aviso(&texto, false), ClasseAvisoChat::Marco);
        assert!(fica_fora_do_recolhido(&Item::Aviso { em: 0, texto, erro: false }));
        assert!(!fica_fora_do_recolhido(&Item::Aviso { em: 0, texto: "Nova tentativa".into(), erro: false }));
        assert_eq!(rotulo_do_turno_calado(true, false), "Compactando…");
        assert_eq!(rotulo_do_turno_calado(true, true), "Compactando…");
        assert_eq!(rotulo_do_turno_calado(false, true), "Agentes trabalhando em segundo plano…");
        assert_eq!(rotulo_do_turno_calado(false, false), "Pensando…");
    }

    /// Rodada do Paseo, item 4: a tabela para colar em planilha.
    #[test]
    fn a_tabela_separada_por_tabulacao_para_a_planilha() {
        use super::tabela_para_planilha;
        let t = |celulas: &[&str]| celulas.iter().map(|c| c.to_string()).collect::<Vec<_>>();
        let cabecalho = t(&["Arquivo", "Linhas", "O que mudou"]);
        let linhas = vec![t(&["produtos/views.py", "12", "O total desconta o cupom"]), t(&["produtos/tests.py", "48", "Um teste para cada caso"])];
        assert_eq!(
            tabela_para_planilha(&cabecalho, &linhas),
            "Arquivo\tLinhas\tO que mudou\nprodutos/views.py\t12\tO total desconta o cupom\nprodutos/tests.py\t48\tUm teste para cada caso"
        );
        // A tabulação e a quebra dentro da célula não partem a célula; a fileira incompleta (a
        // tabela ainda chegando) sai com as células que tem.
        assert_eq!(tabela_para_planilha(&t(&["a\tb", "c"]), &[t(&["um\ndois"])]), "a b\tc\num dois");
        assert_eq!(tabela_para_planilha(&t(&["só"]), &[]), "só");
    }

    /// Rodada do Paseo, item 2: a linha da trilha diz o que a chamada mudou nos passos.
    #[test]
    fn a_linha_dos_passos_diz_o_que_mudou() {
        use super::{Item, linha_dos_passos, resumo_das_chamadas};
        let par = |v: &str, d: &str| Some((v.to_string(), d.to_string()));
        assert_eq!(linha_dos_passos("TodoWrite", "TodoWrite · Criou 5 passos", false), par("Criou", "5 passos"));
        assert_eq!(linha_dos_passos("TodoWrite", "TodoWrite · Começou: rodar os testes", false), par("Começou:", "rodar os testes"));
        assert_eq!(linha_dos_passos("TaskUpdate", "TaskUpdate · Concluiu: rodar os testes", false), par("Concluiu:", "rodar os testes"));
        assert_eq!(linha_dos_passos("TodoWrite", "TodoWrite · Concluiu 2 passos", false), par("Concluiu", "2 passos"));
        assert_eq!(linha_dos_passos("TaskCreate", "TaskCreate · Criou: criar a.txt", false), par("Criou:", "criar a.txt"));
        // As chamadas de antes disto e a que não mudou nada: o verbo de sempre, sem o "lista de afazeres".
        assert_eq!(linha_dos_passos("TodoWrite", "TodoWrite · lista de afazeres", false), par("Atualizou os passos", ""));
        assert_eq!(linha_dos_passos("TodoWrite", "TodoWrite · lista de afazeres", true), par("Atualizando os passos", ""));
        assert_eq!(linha_dos_passos("TaskList", "TaskList", false), par("Conferiu os passos", ""));
        assert_eq!(linha_dos_passos("Bash", "Bash · ls", false), None);
        // No cabeçalho do grupo elas não viram "usou 9 ferramentas".
        let itens = [
            ferramenta("TaskCreate", "TaskCreate · Criou: a", Some("ok"), false),
            ferramenta("TaskUpdate", "TaskUpdate · Começou: a", Some("ok"), false),
            ferramenta("Bash", "ls", Some("ok"), false),
            ferramenta("TaskUpdate", "TaskUpdate · Concluiu: a", Some("ok"), false),
        ];
        let chamadas: Vec<(usize, &Item)> = itens.iter().enumerate().collect();
        assert_eq!(resumo_das_chamadas(&chamadas, false), "Rodou 1 comando · atualizou os passos");
        assert_eq!(resumo_das_chamadas(&chamadas, true), "1 comando · passos");
        let so_passos: Vec<(usize, &Item)> = itens[..2].iter().enumerate().collect();
        assert_eq!(resumo_das_chamadas(&so_passos, false), "Atualizou os passos");
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
            imagens: Vec::new(),
            durou: 0,
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

    // A altura guardada de um bloco só vale enquanto ele mostra a mesma coisa: a chave muda
    // quando o resultado de uma chamada chega, quando a mensagem é corrigida e quando a conversa
    // segue adiante dele.
    #[test]
    fn a_chave_do_bloco_muda_com_o_que_ele_mostra() {
        use super::{Item, TipoDaChave, chave_da_faixa, chave_do_item};
        let rodando = ferramenta("Read", "Read · a.rs", None, false);
        let pronta = ferramenta("Read", "Read · a.rs", Some("fn a() {}"), false);
        let trilha = |item: &Item, extras: &[u64]| chave_da_faixa(TipoDaChave::Trilha, &[(3, item)], extras).unwrap();
        assert_eq!(trilha(&pronta, &[0, 1]), trilha(&pronta.clone(), &[0, 1]));
        assert_ne!(trilha(&rodando, &[0, 1]), trilha(&pronta, &[0, 1]));
        // A trilha ao vivo e a mesma depois de pronta.
        assert_ne!(trilha(&pronta, &[1, 1]), trilha(&pronta, &[0, 1]));
        // Mais uma chamada na trilha: outro fim, outra chave.
        let duas = [(3, &pronta), (4, &rodando)];
        assert_ne!(chave_da_faixa(TipoDaChave::Trilha, &duas, &[0, 2]).unwrap(), trilha(&pronta, &[0, 1]));
        // A mensagem corrigida fica no mesmo índice com outro texto.
        let item = |item: &Item, extras: &[u64]| chave_do_item(TipoDaChave::Item, 7, item, extras).unwrap();
        assert_ne!(item(&usuario("deu erro"), &[0, 0]), item(&usuario("deu erro no carrinho"), &[0, 0]));
        // A última resposta ganha a barra embaixo; quando a conversa segue, é outro bloco.
        assert_ne!(item(&assistente("pronto"), &[1, 0]), item(&assistente("pronto"), &[0, 0]));
        // O mesmo item no histórico trazido e na conversa não dividem a altura.
        assert_ne!(chave_do_item(TipoDaChave::Historico, 7, &assistente("pronto"), &[]), chave_do_item(TipoDaChave::Item, 7, &assistente("pronto"), &[]));
    }

    // O que não tem altura confiável: o texto longo (o kit o lê em segundo plano, e o primeiro
    // desenho sai sem ele; a altura dele passa pelo assento) e os cartões que mudam com o turno
    // (sem chave: sempre montados).
    #[test]
    fn texto_longo_e_marcado_e_cartao_de_erro_fica_sempre_montado() {
        use super::{Item, TipoDaChave, chave_da_faixa, chave_do_item, fica_montado, texto_que_chega_depois};
        let longo = "linha de um relatório comprido\n".repeat(200);
        assert!(longo.len() > 4096);
        assert!(texto_que_chega_depois(&assistente(&longo)));
        assert!(texto_que_chega_depois(&usuario(&longo)));
        assert!(!texto_que_chega_depois(&assistente("curto")));
        // A saída longa de uma ferramenta vai numa caixa de código, lida na hora.
        assert!(!texto_que_chega_depois(&ferramenta("Read", "Read · a.rs", Some(&longo), false)));
        // O texto longo é marcado, e tem chave: a altura assentada dele vale como a dos outros.
        assert!(fica_montado([&assistente(&longo)]));
        assert!(!fica_montado([&assistente("curto")]));
        assert!(chave_do_item(TipoDaChave::Item, 2, &assistente(&longo), &[]).is_some());
        assert!(chave_do_item(TipoDaChave::Item, 2, &assistente("curto"), &[]).is_some());
        let raciocinio = Item::Raciocinio { texto: longo.clone(), milis: 0, em: 0 };
        let pronta = ferramenta("Read", "Read · a.rs", Some("ok"), false);
        assert!(fica_montado([&pronta, &raciocinio]));
        assert!(!fica_montado([&pronta]));
        assert!(chave_da_faixa(TipoDaChave::Trilha, &[(1, &pronta), (2, &raciocinio)], &[]).is_some());
        assert!(chave_da_faixa(TipoDaChave::Trilha, &[], &[]).is_none());
        let erro = Item::Aviso { texto: "O turno falhou: sem conexão".into(), erro: true, em: 0 };
        assert!(chave_do_item(TipoDaChave::Item, 2, &erro, &[]).is_none());
        assert!(chave_do_item(TipoDaChave::Item, 2, &Item::Cartao { id: 1, em: 0 }, &[]).is_none());
    }
}
