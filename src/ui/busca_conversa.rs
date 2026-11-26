//! Buscar dentro da conversa aberta (Ctrl+F): a barra que flutua no alto do painel, o índice das
//! ocorrências e o realce delas no texto.
//!
//! A busca não liga para maiúsculas nem para acento ("codigo" acha "código"), e o índice conta
//! também o que está dentro de um bloco fechado (a saída de uma ferramenta, o relatório de um
//! ajudante): ao chegar numa dessas, o bloco abre.
//!
//! O custo é o ponto delicado — uma conversa tem centenas de itens e todo quadro desenha todos.
//! Por isso:
//! - o índice só é refeito quando o termo ou o tamanho da conversa muda (`Busca::atualizar`);
//! - cada texto recebe só o termo e qual ocorrência dele é a atual (`Realce`), e acha as faixas
//!   por conta própria uma vez por mudança, nunca a cada quadro (`TextoAchado`);
//! - nada anima: a barra entra uma vez e a rolagem até o achado acontece uma vez por salto.

use std::ops::Range;

use gpui_kit::component::{
    Disableable as _, ElementExt as _, h_flex,
    input::{Escape, InputState},
    text::{RangeHighlight, RenderedText, TextView, TextViewState},
    tooltip::{ManagedTooltipExt as _, Tooltip},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Flutuante as _, LarguraPainel, TamanhoBotao, botao_icone, campo_de_busca},
    movimento::{self, De},
};
use crate::{
    modelo::Item,
    tema::{Cores, Lucide, Tipo as _, esp, r, raio},
};

actions!(vitral, [BuscarNaConversa, AchadoSeguinte, AchadoAnterior, FecharBuscaConversa]);


// ------------------------------------------------------------------ achar no texto

// A peça que tira o acento mora em `acentos`, comum à lista e ao histórico do "+" (rodada de uso,
// item 4.3).
pub use super::acentos::{faixas, normalizar};

// ------------------------------------------------------------------ índice da conversa

/// Qual texto de um item casou. `Resumo` e `Resultado` são de uma chamada de ferramenta, e o
/// resultado só aparece com o bloco aberto.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Campo {
    Texto,
    Resumo,
    Resultado,
}

/// Uma ocorrência na conversa: em que item, em que texto dele e onde.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ocorrencia {
    pub item: usize,
    pub campo: Campo,
    pub faixa: Range<usize>,
}

/// A chamada de ferramenta aparece na conversa (as buscas de ferramenta nunca; no modo foco, dentro
/// da linha do turno, que a busca abre). Segue o filtro de `mensagens::mensagens`.
fn ferramenta_visivel(nome: &str, _foco: bool) -> bool {
    nome != "ToolSearch"
}

/// Todas as ocorrências de `termo` nos itens, na ordem da conversa. `foco` é o modo foco do
/// painel, que esconde as chamadas de ferramenta que não são ações sobre tarefas.
pub fn ocorrencias(itens: &[Item], termo: &str, foco: bool) -> Vec<Ocorrencia> {
    if termo.is_empty() {
        return Vec::new();
    }
    let mut saida = Vec::new();
    for (item, dado) in itens.iter().enumerate() {
        let mut achar = |campo: Campo, texto: &str| {
            saida.extend(faixas(texto, termo).into_iter().map(|faixa| Ocorrencia { item, campo, faixa }));
        };
        // A resposta que repete o erro do turno não vai para a tela: a busca acha no cartão.
        if super::mensagens::resposta_repete_o_erro(itens, item) {
            continue;
        }
        match dado {
            Item::Usuario { texto, .. }
            | Item::Assistente { texto, .. }
            | Item::Entrega { texto, .. }
            | Item::Aviso { texto, .. } => achar(Campo::Texto, texto),
            // O plano que ficou na conversa é um cartão com o texto dele, não uma linha da trilha.
            Item::Ferramenta { nome, .. } if ferramenta_visivel(nome, foco) && super::mensagens::plano_na_conversa(dado).is_some() => {
                if let Some(plano) = super::mensagens::plano_na_conversa(dado) {
                    achar(Campo::Texto, plano.texto);
                }
            }
            Item::Ferramenta { nome, resumo, resultado, .. } if ferramenta_visivel(nome, foco) => {
                achar(Campo::Resumo, resumo);
                if let Some(resultado) = resultado {
                    achar(Campo::Resultado, resultado);
                }
            }
            _ => {}
        }
    }
    saida
}

/// A ocorrência seguinte (`passo` 1) ou anterior (−1), dando a volta nas pontas.
pub fn andar(total: usize, atual: usize, passo: i32) -> usize {
    if total == 0 {
        return 0;
    }
    let atual = atual.min(total - 1) as i64;
    (atual + passo as i64).rem_euclid(total as i64) as usize
}

/// O primeiro item da trilha em que `indice` está, com as mesmas regras com que a conversa monta
/// as trilhas (`Janela::mensagens`): as chamadas visíveis e os raciocínios ficam juntos (a
/// escondida não vai para a tela, então não quebra), e a troca de dia parte a trilha. É a chave do
/// grupo em `expandidos`. Antes o raciocínio partia a trilha só aqui, e o Ctrl+F abria um grupo que
/// não existia: o contador andava e a tela não (teste de usabilidade, 16/11/2026).
fn primeira_da_trilha(itens: &[Item], indice: usize, foco: bool) -> usize {
    let visiveis: Vec<usize> = (0..itens.len())
        .filter(|i| match &itens[*i] {
            Item::Ferramenta { nome, .. } => ferramenta_visivel(nome, foco),
            Item::Assistente { .. } => !super::mensagens::resposta_repete_o_erro(itens, *i),
            _ => true,
        })
        .collect();
    let Some(mut p) = visiveis.iter().position(|i| *i == indice) else {
        return indice;
    };
    let horas: Vec<u64> = visiveis.iter().map(|i| itens[*i].em()).collect();
    let dias: Vec<usize> = super::mensagens::separadores_de_data(&horas, 0).into_iter().map(|(p, _)| p).collect();
    // O cartão do plano não é da trilha, e a parte.
    let da_trilha = |i: usize| {
        matches!(itens[i], Item::Ferramenta { .. } | Item::Raciocinio { .. }) && super::mensagens::plano_na_conversa(&itens[i]).is_none()
    };
    if !da_trilha(indice) {
        return indice;
    }
    while p > 0 && da_trilha(visiveis[p - 1]) && !dias.contains(&p) {
        p -= 1;
    }
    visiveis[p]
}

/// As chaves de `expandidos` que precisam entrar para a ocorrência do item `indice` ficar à
/// vista: o próprio bloco (a saída da chamada, o relatório, a mensagem recebida), o "Mostrar
/// tudo" de uma mensagem longa e, numa chamada, o grupo da trilha.
pub fn chaves_para_abrir(chat: &str, itens: &[Item], indice: usize, foco: bool) -> Vec<String> {
    let mut chaves = vec![format!("{chat}:{indice}"), format!("{chat}:{indice}:tudo")];
    if matches!(itens.get(indice), Some(Item::Ferramenta { .. } | Item::Raciocinio { .. })) {
        chaves.push(format!("{chat}:{}:grupo", primeira_da_trilha(itens, indice, foco)));
    }
    // Dentro de um turno recolhido na linha "Trabalhou…", a linha abre (rodada de uso, 2.1).
    if let Some(turno) = super::mensagens::turnos_da_conversa(itens, foco, false).into_iter().find(|t| t.contem(indice)) {
        chaves.push(turno.chave(chat));
    }
    chaves
}

// ------------------------------------------------------------------ estado da busca

/// O que o índice depende: refazê-lo só quando isso muda custa uma comparação por quadro.
#[derive(Clone, PartialEq, Eq, Default)]
struct Assinatura {
    chat: String,
    termo: String,
    itens: usize,
    foco: bool,
}

/// O índice da busca de uma conversa, sem nada de interface: o termo, as ocorrências, qual é a
/// atual e o salto. É a parte que os testes exercitam.
#[derive(Default)]
pub struct Indice {
    /// O termo normalizado (vazio: sem busca).
    termo: String,
    achados: Vec<Ocorrencia>,
    atual: usize,
    assinatura: Assinatura,
    /// Sobe a cada salto: cada texto rola até a ocorrência atual uma vez por número.
    salto: u64,
}

impl Indice {
    /// Refaz o índice quando o termo, a conversa ou o modo foco mudaram; senão, não faz nada.
    /// Chamado uma vez por quadro, de `conversa`.
    pub fn atualizar(&mut self, chat: &str, itens: &[Item], foco: bool, termo: &str) {
        let assinatura = Assinatura { chat: chat.to_string(), termo: termo.to_string(), itens: itens.len(), foco };
        if self.assinatura == assinatura {
            return;
        }
        let mesma_busca = self.assinatura.chat == assinatura.chat && self.assinatura.termo == assinatura.termo;
        self.assinatura = assinatura;
        self.termo = termo.to_string();
        self.achados = ocorrencias(itens, termo, foco);
        // Mudou o termo (ou a conversa): volta para a primeira. Só cresceu: fica onde está.
        if !mesma_busca {
            self.atual = 0;
            // Conta como salto: a conversa precisa rolar até o primeiro achado do termo novo,
            // mesmo quando ele cai no mesmo lugar do termo anterior (o número é o que faz a
            // rolagem acontecer uma vez).
            self.salto += 1;
        }
        self.atual = self.atual.min(self.achados.len().saturating_sub(1));
    }

    pub fn total(&self) -> usize {
        self.achados.len()
    }

    /// "3 de 12", ou `None` quando não há termo.
    pub fn contador(&self) -> Option<String> {
        if self.termo.is_empty() {
            return None;
        }
        if self.achados.is_empty() {
            return Some(tr!("nenhum achado").to_string());
        }
        Some(tr!("{n} de {total}", n = self.atual + 1, total = self.achados.len()).to_string())
    }

    /// O contador da barra estreita: "2/7" e, sem achado, "0" (em âmbar; a frase vai na dica).
    pub fn contador_curto(&self) -> Option<String> {
        if self.termo.is_empty() {
            return None;
        }
        if self.achados.is_empty() {
            return Some("0".into());
        }
        Some(format!("{}/{}", self.atual + 1, self.achados.len()))
    }

    pub fn ocorrencia_atual(&self) -> Option<&Ocorrencia> {
        self.achados.get(self.atual)
    }

    /// Anda para a ocorrência seguinte ou anterior e devolve o item dela.
    fn andar(&mut self, passo: i32) -> Option<usize> {
        if self.achados.is_empty() {
            return None;
        }
        self.atual = andar(self.achados.len(), self.atual, passo);
        self.salto += 1;
        self.ocorrencia_atual().map(|o| o.item)
    }

    /// Vai para a primeira ocorrência a partir do item `item` (a que a lista da esquerda mostrou).
    fn ir_ao_item(&mut self, item: usize) -> Option<usize> {
        self.atual = self.achados.iter().position(|o| o.item >= item).unwrap_or(0);
        self.salto += 1;
        self.ocorrencia_atual().map(|o| o.item)
    }

    /// O realce de um texto da conversa: o termo e, quando a ocorrência atual está nele, qual
    /// das ocorrências desse texto é. Quem desenha acha as faixas por conta própria — o texto na
    /// tela não é exatamente o do item (as referências viram links, o markdown sai da linha de
    /// log), e refazer a conta no texto certo é mais simples que mapear deslocamentos.
    pub fn realce(&self, item: usize, campo: Campo) -> Realce {
        if self.termo.is_empty() {
            return Realce::default();
        }
        let atual = self.achados.get(self.atual).filter(|o| o.item == item && o.campo == campo).map(|atual| {
            self.achados.iter().filter(|o| o.item == item && o.campo == campo).position(|o| o == atual).unwrap_or(0)
        });
        Realce { termo: self.termo.clone(), atual, salto: self.salto }
    }
}

/// A busca de um painel: o índice mais o campo da barra. Vive em `Painel::busca` e dura enquanto
/// o painel existe — fechar a barra guarda o termo para o próximo Ctrl+F.
pub struct Busca {
    pub campo: Entity<InputState>,
    pub aberta: bool,
    /// A barra já estava na tela no quadro anterior (ela entra uma vez por abertura).
    pub vista: bool,
    pub indice: Indice,
}

impl Busca {
    pub fn nova(campo: Entity<InputState>) -> Self {
        Busca { campo, aberta: false, vista: false, indice: Indice::default() }
    }
}

/// O que realçar num texto: o termo normalizado e, quando a ocorrência atual está ali, a posição
/// dela entre as ocorrências do texto (na ordem do texto).
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Realce {
    pub termo: String,
    pub atual: Option<usize>,
    salto: u64,
}

impl Realce {
    pub fn vazio(&self) -> bool {
        self.termo.is_empty()
    }

    /// O número do salto, só quando a ocorrência atual está neste texto: é o que faz a vista
    /// rolar até ela uma vez, e não a cada quadro.
    pub(crate) fn salto_aqui(&self) -> Option<u64> {
        self.atual.map(|_| self.salto)
    }

    /// O realce de um pedaço do texto que tem `n` ocorrências, a partir da ocorrência `inicio`
    /// do texto todo: a resposta partida em trechos (as citações com o Copiar delas).
    pub fn trecho(&self, inicio: usize, n: usize) -> Realce {
        Realce {
            termo: self.termo.clone(),
            atual: self.atual.filter(|a| *a >= inicio && *a < inicio + n).map(|a| a - inicio),
            salto: self.salto,
        }
    }
}

// ------------------------------------------------------------------ realce na tela

/// Fundo de toda ocorrência e da atual, em âmbar: a atual é a mesma cor mais forte, e não outra,
/// para as duas lerem como "o termo".
fn cor_do_achado(atual: bool) -> Hsla {
    if atual { Cores::ambar().opacity(0.42) } else { Cores::ambar().opacity(0.16) }
}

/// Texto simples (a linha de uma chamada, um aviso, a saída de um comando) com as ocorrências
/// realçadas. Mantém o texto num só filho: a quebra de linha e o recorte continuam iguais.
pub fn realcar(texto: String, realce: &Realce) -> StyledText {
    realcar_trecho(texto, &realce.termo, realce.atual)
}

/// O mesmo para um pedaço do texto (uma linha da saída de um comando): `atual` é a posição da
/// ocorrência atual entre as do pedaço.
pub fn realcar_trecho(texto: String, termo: &str, atual: Option<usize>) -> StyledText {
    if termo.is_empty() {
        return StyledText::new(texto);
    }
    let destaques = destaques_do_trecho(&texto, termo, atual);
    StyledText::new(texto).with_highlights(destaques)
}

/// O fundo de cada ocorrência de `termo` no pedaço (o da `atual` mais forte), para quem junta o
/// realce da busca a outros destaques do mesmo texto (os links da saída de comando).
pub fn destaques_do_trecho(texto: &str, termo: &str, atual: Option<usize>) -> Vec<(Range<usize>, HighlightStyle)> {
    if termo.is_empty() {
        return Vec::new();
    }
    faixas(texto, termo)
        .into_iter()
        .enumerate()
        .map(|(i, faixa)| {
            let estilo = HighlightStyle {
                background_color: Some(cor_do_achado(atual == Some(i))),
                ..Default::default()
            };
            (faixa, estilo)
        })
        .collect()
}

/// O que já foi pedido à vista do texto: o realce e o desenho em que ele foi aplicado. Sem isso,
/// aplicar o realce a cada quadro pediria um redesenho a cada quadro, para sempre.
#[derive(Default)]
struct Aplicado {
    realce: Realce,
    texto: Option<RenderedText>,
}

/// Markdown de uma mensagem com o termo realçado atrás das letras.
///
/// O realce de verdade vem da vista do texto (`TextViewState`), que sabe pintar uma faixa sem
/// mexer no layout e rolar até a linha dela — inclusive dentro de um bloco de código. Por isso o
/// estado fica guardado aqui (`use_keyed_state`) em vez de dentro do elemento: é o mesmo que o
/// `TextView::markdown` faz, só que alcançável.
#[derive(IntoElement)]
pub struct TextoAchado {
    chave: SharedString,
    texto: String,
    realce: Realce,
    /// A rolagem do painel: a vista pede para aparecer e quem rola é ela.
    rolagem: Option<ScrollHandle>,
    /// Monta o `TextView` com o estilo do lugar (as mensagens, o texto compacto dos painéis).
    estilo: Box<dyn FnOnce(TextView, &App) -> TextView>,
    /// `max_lines` da mensagem longa recolhida.
    linhas: Option<usize>,
}

impl TextoAchado {
    pub fn novo(chave: SharedString, texto: String, realce: Realce) -> Self {
        TextoAchado {
            chave,
            texto,
            realce,
            rolagem: None,
            estilo: Box::new(|tv, _| tv),
            linhas: None,
        }
    }

    pub fn rolagem(mut self, rolagem: &ScrollHandle) -> Self {
        self.rolagem = Some(rolagem.clone());
        self
    }

    pub fn estilo(mut self, f: impl FnOnce(TextView, &App) -> TextView + 'static) -> Self {
        self.estilo = Box::new(f);
        self
    }

    pub fn linhas(mut self, linhas: Option<usize>) -> Self {
        self.linhas = linhas;
        self
    }
}

impl RenderOnce for TextoAchado {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let chave = ElementId::Name(self.chave.clone());
        let texto = self.texto;
        let estado = window.use_keyed_state(chave.clone(), cx, {
            let texto = texto.clone();
            move |_, cx| TextViewState::markdown(&texto, cx)
        });
        estado.update(cx, |estado, cx| estado.set_text(&texto, cx));
        let aplicado = window.use_keyed_state(movimento::chave(&chave, "realce"), cx, |_, _| Aplicado::default());
        // Sem busca e sem realce de antes não há nada a fazer: é o caso de quase todo quadro.
        let tem_realce = !self.realce.vazio() || !aplicado.read(cx).realce.vazio();
        if tem_realce {
            let desenhado = estado.read(cx).rendered_text();
            let mudou = {
                let antes = aplicado.read(cx);
                antes.realce != self.realce || antes.texto.as_ref() != Some(&desenhado)
            };
            if mudou {
                let (pintar, atual) = pedidos(&desenhado, &self.realce);
                estado.update(cx, |estado, cx| {
                    let _ = estado.set_range_highlights(pintar, cx);
                    // Rolar até a atual só no salto: senão a conversa voltaria a ela a cada
                    // quadro e o usuário não conseguiria rolar.
                    if let (Some(faixa), Some(_)) = (atual, self.realce.salto_aqui()) {
                        let _ = estado.reveal_range(faixa, cx);
                    }
                });
                aplicado.update(cx, |a, _| {
                    a.realce = self.realce.clone();
                    a.texto = Some(desenhado);
                });
            }
        }
        let vista = (self.estilo)(TextView::new(&estado), cx).when_some(self.linhas, TextView::max_lines);
        match self.rolagem {
            Some(rolagem) => vista
                .on_reveal(move |limites, _, _| rolar_ate(&rolagem, limites.origin.y, limites.size.height))
                .into_any_element(),
            None => vista.into_any_element(),
        }
    }
}

/// As faixas a pintar no texto desenhado e qual delas é a atual. As faixas vêm da fonte (o
/// markdown) e a vista pinta no texto que ela desenha, então cada uma é convertida; o que não
/// virar nada (dentro de um pedaço que não vai à tela) só não é pintado.
fn pedidos(desenhado: &RenderedText, realce: &Realce) -> (Vec<RangeHighlight>, Option<Range<usize>>) {
    let mut pintar = Vec::new();
    let mut atual = None;
    for (i, faixa) in faixas(desenhado.source(), &realce.termo).into_iter().enumerate() {
        let Some(convertida) = desenhado.range_for_source(faixa) else {
            continue;
        };
        let esta = realce.atual == Some(i);
        if esta {
            atual = Some(convertida.clone());
        }
        pintar.push(RangeHighlight::new(convertida, cor_do_achado(esta)));
    }
    (pintar, atual)
}

/// Rola o painel até deixar à vista a faixa que começa em `topo` (em coordenadas da janela), com
/// um respiro de 48 em cima (o achado nunca cola no cabeçalho, onde fica a barra) e de 88 embaixo:
/// o achado não para na última linha, cortado, debaixo da pílula de voltar ao fim (teste de
/// usabilidade, 16/11/2026).
fn rolar_ate(rolagem: &ScrollHandle, topo: Pixels, altura_do_alvo: Pixels) {
    const RESPIRO: Pixels = px(48.);
    const RESPIRO_BAIXO: Pixels = px(88.);
    let vista = rolagem.bounds();
    if vista.size.height <= px(0.) {
        return;
    }
    let atual = rolagem.offset();
    let acima = topo - vista.origin.y;
    let abaixo = (topo + altura_do_alvo) - (vista.origin.y + vista.size.height);
    // Já dentro da vista, com os respiros: não mexe (rolar de novo brigaria com a roda).
    let ajuste = if acima < RESPIRO {
        acima - RESPIRO
    } else if abaixo > -RESPIRO_BAIXO {
        (abaixo + RESPIRO_BAIXO).min(acima - RESPIRO)
    } else {
        return;
    };
    let alvo = (atual.y - ajuste).clamp(-rolagem.max_offset().y, px(0.));
    if alvo != atual.y {
        rolagem.set_offset(point(atual.x, alvo));
    }
}

/// Envolve um bloco que não é markdown (a linha de uma chamada, um aviso) e rola até ele quando
/// ele tem a ocorrência atual, uma vez por salto.
#[derive(IntoElement)]
pub struct RolarAoAchado {
    chave: SharedString,
    salto: Option<u64>,
    rolagem: ScrollHandle,
    filho: AnyElement,
}

impl RolarAoAchado {
    /// `None` em `realce.salto_aqui()` devolve o filho inteiro, sem nada em volta.
    pub fn novo(chave: SharedString, realce: &Realce, rolagem: &ScrollHandle, filho: AnyElement) -> Self {
        RolarAoAchado { chave, salto: realce.salto_aqui(), rolagem: rolagem.clone(), filho }
    }
}

impl RenderOnce for RolarAoAchado {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(salto) = self.salto else {
            return self.filho;
        };
        let feito = window.use_keyed_state(ElementId::Name(self.chave), cx, |_, _| u64::MAX);
        if *feito.read(cx) == salto {
            return self.filho;
        }
        feito.update(cx, |f, _| *f = salto);
        let rolagem = self.rolagem;
        div()
            .on_prepaint(move |limites, _, _| rolar_ate(&rolagem, limites.origin.y, limites.size.height))
            .child(self.filho)
            .into_any_element()
    }
}

// ------------------------------------------------------------------ a barra e as ações

/// A distância da barra da busca até a borda da direita do painel: o respiro de sempre, ou, no
/// painel que encosta na direita da janela e com anéis de uso à vista, a reserva deles (que conta
/// da borda da janela, e ali painel e janela têm a mesma borda). Nunca menos que o respiro.
pub(super) fn recuo_da_direita(painel_na_borda: bool, reserva: Option<Pixels>, respiro: Pixels) -> Pixels {
    match reserva {
        Some(reserva) if painel_na_borda => reserva.max(respiro),
        _ => respiro,
    }
}

impl Janela {
    /// Ctrl+F: abre (ou refoca) a busca do painel ativo, com o termo de antes selecionado.
    pub(super) fn abrir_busca_na_conversa(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(painel) = self.paineis.get_mut(self.ativo) else {
            return;
        };
        painel.busca.aberta = true;
        let campo = painel.busca.campo.clone();
        campo.update(cx, |campo, cx| campo.select_all(window, cx));
        window.focus(&campo.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    /// Esc: fecha a barra e devolve o foco à caixa de mensagem (o termo fica guardado).
    pub(super) fn fechar_busca_na_conversa(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(painel) = self.paineis.get(self.ativo).filter(|p| p.busca.aberta) else {
            // Sem barra aberta, o Esc é de quem vem depois (parar o turno, fechar a gaveta).
            cx.propagate();
            return;
        };
        let ativo = self.ativo;
        let composer = painel.composer.read(cx).focus_handle(cx);
        self.paineis[ativo].busca.aberta = false;
        window.focus(&composer, cx);
        cx.notify();
    }

    /// Enter, F3 (e Shift neles): vai para a ocorrência seguinte ou anterior do painel ativo,
    /// abrindo o bloco dela se estiver fechado.
    pub(super) fn andar_na_busca(&mut self, passo: i32, cx: &mut Context<Self>) {
        let ix = self.ativo;
        if !self.paineis.get(ix).is_some_and(|p| p.busca.aberta) {
            return;
        }
        if let Some(item) = self.paineis[ix].busca.indice.andar(passo) {
            self.ir_a_ocorrencia(ix, item, cx);
        }
    }

    /// Prepara o painel para mostrar a ocorrência do item `item`: abre o bloco dela e solta o fim
    /// da conversa (senão todo quadro voltaria para lá). Quem rola é o salto, já somado.
    fn ir_a_ocorrencia(&mut self, ix: usize, item: usize, cx: &mut Context<Self>) {
        let Some(chat) = self.paineis[ix].chat.clone() else {
            return;
        };
        let foco = self.modo_foco(cx);
        let chaves = self
            .motor
            .as_ref()
            .and_then(|m| m.read(cx).chat(&chat))
            .map(|c| chaves_para_abrir(&chat, &c.itens, item, foco))
            .unwrap_or_default();
        self.expandidos.extend(chaves);
        // Sem isso o painel que estava no fim voltaria para lá a cada desenho.
        self.paineis[ix].seguindo = false;
        cx.notify();
    }

    /// Um achado da busca da lista (Ctrl+K): abre a conversa já na mensagem que casou, com o
    /// mesmo termo na busca da conversa — o realce, o contador e o F3 vêm de graça.
    pub(super) fn abrir_no_achado(
        &mut self,
        chat: String,
        item: usize,
        termo: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selecionar(Some(chat.clone()), window, cx);
        let Some(ix) = self.paineis.iter().position(|p| p.chat.as_deref() == Some(chat.as_str())) else {
            return;
        };
        let campo = self.paineis[ix].busca.campo.clone();
        campo.update(cx, |campo, cx| campo.set_value(termo.clone(), window, cx));
        let itens = self.motor.as_ref().and_then(|m| m.read(cx).chat(&chat)).map(|c| c.itens.clone()).unwrap_or_default();
        let foco = self.modo_foco(cx);
        let busca = &mut self.paineis[ix].busca;
        busca.aberta = true;
        busca.indice.atualizar(&chat, &itens, foco, &normalizar(&termo));
        if let Some(item) = busca.indice.ir_ao_item(item) {
            self.ir_a_ocorrencia(ix, item, cx);
        }
    }

    /// A barra da busca, flutuando no alto da conversa: o campo, o contador, as setas e o fechar.
    /// Entra uma vez, quando abre.
    pub(super) fn barra_de_busca(&mut self, ix: usize, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let painel = self.paineis.get_mut(ix)?;
        if !painel.busca.aberta {
            painel.busca.vista = false;
            return None;
        }
        let nova = !std::mem::replace(&mut painel.busca.vista, true);
        let painel = self.paineis.get(ix)?;
        let id = painel.id as usize;
        let total = painel.busca.indice.total();
        // Na barra estreita, o contador curto ("2/7"), e o campo nunca abaixo de 128: com 4
        // painéis ele mostrava uma letra só (teste de usabilidade, 16/11/2026).
        let estreita = painel.medida.nivel != LarguraPainel::Largo;
        let contador = painel.busca.indice.contador();
        let sem_achado = total == 0 && contador.is_some();
        let contador_longo = contador.clone().map(SharedString::from);
        let contador = if estreita { painel.busca.indice.contador_curto() } else { contador };
        let seta = |nome: &'static str, icone: Lucide, dica: SharedString, acao: &dyn Action, passo: i32| {
            botao_icone(SharedString::from(format!("{nome}-{id}")), icone, dica.clone(), TamanhoBotao::Barra, window, cx)
                .tooltip_with_action(dica, acao, None)
                .disabled(total == 0)
                .on_click(cx.listener(move |this, _, _, cx| this.andar_na_busca(passo, cx)))
        };
        let barra = h_flex()
            .id(SharedString::from(format!("busca-conversa-{id}")))
            .key_context("BuscaConversa")
            .relative()
            .w_full()
            .p(esp::E8)
            .gap(esp::E4)
            .rounded(raio::PECA)
            .flutuante(cx)
            .on_action(cx.listener(|this, _: &FecharBuscaConversa, window, cx| this.fechar_busca_na_conversa(window, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w(r(128.))
                    .on_action(cx.listener(|this, _: &Escape, window, cx| this.fechar_busca_na_conversa(window, cx)))
                    .child(
                        campo_de_busca(SharedString::from(format!("campo-busca-{id}")), &painel.busca.campo, None, window, cx)
                            .bg(Cores::recuo_no_flutuante()),
                    ),
            )
            .children(contador.map(|texto| {
                div()
                    .id(SharedString::from(format!("contador-busca-{id}")))
                    .flex_none()
                    .px(esp::E4)
                    .metadado()
                    .text_color(if sem_achado { Cores::ambar() } else { Cores::tenue() })
                    .child(texto)
                    .when_some(contador_longo.filter(|_| estreita), |s, longo| {
                        s.dica_no_ponteiro(move |window, cx| Tooltip::new(longo.clone()).build(window, cx))
                    })
            }))
            .child(seta("achado-anterior", Lucide::ChevronUp, tr!("Achado anterior"), &AchadoAnterior, -1))
            .child(seta("achado-seguinte", Lucide::ChevronDown, tr!("Próximo achado"), &AchadoSeguinte, 1))
            .child(
                botao_icone(SharedString::from(format!("fechar-busca-{id}")), Lucide::X, tr!("Fechar a busca"), TamanhoBotao::Barra, window, cx)
                    .tooltip_with_action(tr!("Fechar a busca"), &FecharBuscaConversa, Some("BuscaConversa"))
                    .on_click(cx.listener(|this, _, window, cx| this.fechar_busca_na_conversa(window, cx))),
            );
        // A barra fica por cima da conversa, encostada no alto e à direita, e nunca mais larga
        // que o painel (no painel mínimo ela ocupa tudo menos as margens). No painel que encosta
        // na direita da janela ela guarda o lugar dos anéis de uso, que flutuam ali por cima
        // (dono, 24/11/2026: a barra ficava por baixo deles). A reserva vem de
        // `reserva_dos_aneis`, a mesma do cabeçalho do arquivo: sem anel nenhum não há reserva.
        let recuo = recuo_da_direita(
            self.painel_na_borda_da_janela(ix),
            self.reserva_dos_aneis(window, cx),
            crate::tema::tam(window, 8.),
        );
        Some(
            div()
                .absolute()
                .top(esp::E8)
                .right(recuo)
                .left(esp::E8)
                .flex()
                .justify_end()
                .child(
                    div()
                        .w(r(380.))
                        .max_w(relative(1.))
                        .child(movimento::entrada(("entra-busca", id), nova, De::Cima, barra)),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: o `gpui_kit::*` lá de cima traz um `test` que esconde o `#[test]`
    // (o mesmo cuidado do ui/mensagens.rs).
    use super::{Campo, Indice, Realce, andar, chaves_para_abrir, faixas, normalizar, ocorrencias, recuo_da_direita};
    use super::super::acentos::primeira;
    use crate::modelo::Item;

    #[test]
    fn a_barra_respeita_a_reserva_dos_aneis_so_na_borda_da_janela() {
        use gpui_kit::px;
        let respiro = px(8.);
        // Sem anéis (sem reserva), ou num painel que não encosta na direita da janela: o respiro.
        assert_eq!(recuo_da_direita(true, None, respiro), respiro);
        assert_eq!(recuo_da_direita(false, Some(px(60.)), respiro), respiro);
        // No painel da borda, com anel (um, dois ou três: a reserva é a mesma, eles se empilham),
        // a barra recua o que a reserva manda, sem número fixo.
        assert_eq!(recuo_da_direita(true, Some(px(60.)), respiro), px(60.));
        assert_eq!(recuo_da_direita(true, Some(px(90.)), respiro), px(90.));
        // Reserva menor que o respiro nunca aproxima a barra da borda.
        assert_eq!(recuo_da_direita(true, Some(px(4.)), respiro), respiro);
    }

    fn item_usuario(texto: &str) -> Item {
        Item::Usuario {
            em: 0,
            texto: texto.into(),
            anexos: Vec::new(),
            ultrathink: false,
            ultracode: false,
            marca: None,
        }
    }

    fn ferramenta(nome: &str, resumo: &str, resultado: Option<&str>) -> Item {
        Item::Ferramenta {
            em: 0,
            id: "f1".into(),
            nome: nome.into(),
            resumo: resumo.into(),
            resultado: resultado.map(str::to_string),
            erro: false,
            detalhe: None,
            arquivo: None,
            sub: None,
            mudanca: None,
            imagens: Vec::new(),
            durou: 0,
        }
    }

    #[test]
    fn busca_sem_maiuscula_e_sem_acento() {
        assert_eq!(normalizar("  Código  "), "codigo");
        // "codigo" acha "código", e "cupom" acha "Cupom".
        let texto = "O código do Cupom";
        assert_eq!(primeira(texto, "codigo").map(|f| texto[f].to_string()), Some("código".into()));
        assert_eq!(primeira(texto, "cupom").map(|f| texto[f].to_string()), Some("Cupom".into()));
        assert_eq!(primeira("ÁRVORE de arquivos", "arvore"), Some(0..7));
        assert_eq!(primeira("cupons", "frete"), None);
        assert_eq!(primeira("cupons", ""), None);
    }

    #[test]
    fn faixas_em_bytes_do_original() {
        // Cada faixa recorta a letra inteira, com acento e tudo.
        let texto = "ação e ações";
        let achadas = faixas(texto, "aca");
        assert_eq!(achadas.iter().map(|f| &texto[f.clone()]).collect::<Vec<_>>(), ["açã"]);
        // "acoes" acha "ações" inteiro, com os dois acentos no meio.
        assert_eq!(faixas(texto, "acoes").iter().map(|f| &texto[f.clone()]).collect::<Vec<_>>(), ["ações"]);
        // Ocorrências seguidas não se sobrepõem.
        assert_eq!(faixas("aaaa", "aa"), vec![0..2, 2..4]);
        assert_eq!(faixas("abc", "abcd"), Vec::<std::ops::Range<usize>>::new());
    }

    #[test]
    fn indice_conta_mensagens_avisos_e_ferramentas() {
        let itens = vec![
            item_usuario("Valide o cupom e o CUPOM de novo"),
            Item::Assistente { em: 0, texto: "```python\ncupom = 1\n```".into() },
            ferramenta("Grep", "cupom", Some("models.py:42:class Cupom")),
            // As buscas de ferramenta não aparecem na conversa: não contam.
            ferramenta("ToolSearch", "cupom", Some("cupom")),
            Item::Aviso { em: 0, texto: "Cupom vencido".into(), erro: true },
        ];
        let achados = ocorrencias(&itens, "cupom", false);
        assert_eq!(achados.len(), 6);
        assert_eq!(achados[0].item, 0);
        assert_eq!(achados[1].item, 0);
        assert_eq!(achados[2].item, 1);
        assert_eq!((achados[3].item, achados[3].campo), (2, Campo::Resumo));
        assert_eq!((achados[4].item, achados[4].campo), (2, Campo::Resultado));
        assert_eq!(achados[5].item, 4);
        // No modo foco as chamadas ficam dentro da linha do turno, que a busca abre: contam.
        assert_eq!(ocorrencias(&itens, "cupom", true).len(), 6);
        assert!(ocorrencias(&itens, "", false).is_empty());
    }

    #[test]
    fn andar_da_a_volta_nas_pontas() {
        assert_eq!(andar(12, 0, 1), 1);
        assert_eq!(andar(12, 11, 1), 0);
        assert_eq!(andar(12, 0, -1), 11);
        assert_eq!(andar(1, 0, 1), 0);
        assert_eq!(andar(0, 0, 1), 0);
        // Índice velho depois de a lista encolher.
        assert_eq!(andar(3, 9, 1), 0);
    }

    #[test]
    fn chaves_abrem_o_bloco_e_a_trilha_dele() {
        let itens = vec![
            item_usuario("oi"),
            ferramenta("Read", "a", None),
            ferramenta("ToolSearch", "b", None),
            ferramenta("Edit", "c", None),
            Item::Assistente { em: 0, texto: "fim".into() },
        ];
        // A chamada do índice 3 está na mesma trilha da 1 (a escondida no meio não quebra).
        let chaves = chaves_para_abrir("c1", &itens, 3, false);
        assert!(chaves.contains(&"c1:3".to_string()));
        assert!(chaves.contains(&"c1:1:grupo".to_string()));
        // Uma mensagem não tem grupo, só o próprio bloco e o "Mostrar tudo".
        assert_eq!(chaves_para_abrir("c1", &itens, 0, false), ["c1:0", "c1:0:tudo"]);
    }

    /// O raciocínio no meio da trilha não a parte: o achado depois dele abre o grupo que começa no
    /// primeiro raciocínio, o mesmo que a conversa desenha.
    #[test]
    fn o_raciocinio_faz_parte_da_trilha_que_o_ctrl_f_abre() {
        let raciocinio = || Item::Raciocinio { em: 0, texto: String::new(), milis: 500 };
        let itens = vec![
            Item::Assistente { em: 0, texto: "antes".into() },
            raciocinio(),
            ferramenta("Bash", "parar", None),
            raciocinio(),
            ferramenta("Bash", "parar de novo", None),
        ];
        for indice in [2, 4] {
            let chaves = chaves_para_abrir("c1", &itens, indice, false);
            assert!(chaves.contains(&"c1:1:grupo".to_string()), "{indice}: {chaves:?}");
        }
        // Com a troca de dia no meio, a trilha se parte ali, como na conversa.
        let dia = 86_400;
        let com_dia = vec![
            Item::Assistente { em: 1_773_316_800, texto: "antes".into() },
            Item::Raciocinio { em: 1_773_316_800, texto: String::new(), milis: 1 },
            Item::Raciocinio { em: 1_773_316_800 + dia, texto: String::new(), milis: 1 },
            ferramenta("Bash", "parar", None),
        ];
        assert!(chaves_para_abrir("c1", &com_dia, 3, false).contains(&"c1:2:grupo".to_string()));
    }

    /// O plano que ficou na conversa é um cartão: a busca acha no texto dele (e abre o cartão,
    /// pela chave do bloco), ele parte a trilha, e o nome da ferramenta não é achado em lugar
    /// nenhum da tela.
    #[test]
    fn o_plano_na_conversa_e_achado_pelo_texto_e_parte_a_trilha() {
        let plano = Item::Ferramenta {
            em: 0,
            id: "p1".into(),
            nome: "ExitPlanMode".into(),
            resumo: "ExitPlanMode".into(),
            resultado: Some("User has approved your plan.".into()),
            erro: false,
            detalhe: Some("# Trocar o cupom\n\n1. Ler o carrinho.".into()),
            arquivo: None,
            sub: None,
            mudanca: None,
            imagens: Vec::new(),
            durou: 0,
        };
        let itens = vec![item_usuario("oi"), ferramenta("Read", "a", None), plano, ferramenta("Edit", "c", None), ferramenta("Bash", "d", None)];
        let achados = ocorrencias(&itens, "carrinho", false);
        assert_eq!(achados.iter().map(|o| (o.item, o.campo)).collect::<Vec<_>>(), [(2, Campo::Texto)]);
        assert!(ocorrencias(&itens, "exitplanmode", false).is_empty());
        assert!(ocorrencias(&itens, "approved", false).is_empty());
        assert!(chaves_para_abrir("c1", &itens, 2, false).contains(&"c1:2".to_string()));
        // A trilha de depois do cartão começa depois dele, e a de antes não passa por ele.
        assert!(chaves_para_abrir("c1", &itens, 4, false).contains(&"c1:3:grupo".to_string()));
        assert!(chaves_para_abrir("c1", &itens, 1, false).contains(&"c1:1:grupo".to_string()));
    }

    #[test]
    fn contador_e_realce_seguem_a_ocorrencia_atual() {
        let itens = vec![item_usuario("cupom e cupom"), Item::Assistente { em: 0, texto: "cupom".into() }];
        let mut indice = Indice::default();
        indice.atualizar("c1", &itens, false, "cupom");
        assert_eq!(indice.total(), 3);
        assert_eq!(indice.contador().as_deref(), Some("1 de 3"));
        // Na barra estreita, o curto.
        assert_eq!(indice.contador_curto().as_deref(), Some("1/3"));
        // A atual é a primeira da mensagem 0; a mensagem 1 só leva o termo, sem a atual.
        assert_eq!(indice.realce(0, Campo::Texto).atual, Some(0));
        assert_eq!(indice.realce(1, Campo::Texto).atual, None);
        assert_eq!(indice.realce(1, Campo::Texto).termo, "cupom");
        assert_eq!(indice.andar(1), Some(0));
        assert_eq!(indice.contador().as_deref(), Some("2 de 3"));
        assert_eq!(indice.realce(0, Campo::Texto).atual, Some(1));
        assert_eq!(indice.andar(1), Some(1));
        assert_eq!(indice.realce(0, Campo::Texto).atual, None);
        assert_eq!(indice.realce(1, Campo::Texto).atual, Some(0));
        // Dá a volta e o salto sobe a cada passo.
        assert_eq!(indice.andar(1), Some(0));
        assert_eq!(indice.contador().as_deref(), Some("1 de 3"));
        assert_eq!(indice.realce(0, Campo::Texto).salto, 4);
        // Sem termo, nada é realçado.
        indice.atualizar("c1", &itens, false, "");
        assert_eq!(indice.contador(), None);
        assert!(indice.realce(0, Campo::Texto).vazio());
    }

    #[test]
    fn indice_so_e_refeito_quando_algo_muda() {
        let mut itens = vec![item_usuario("cupom")];
        let mut indice = Indice::default();
        indice.atualizar("c1", &itens, false, "cupom");
        assert_eq!(indice.andar(1), Some(0));
        let salto = indice.salto;
        // Mesma assinatura: nem o índice nem a posição mexem (é o caso de quase todo quadro).
        indice.atualizar("c1", &itens, false, "cupom");
        assert_eq!((indice.atual, indice.salto), (0, salto));
        // A conversa cresceu: o índice cresce e a posição fica onde estava.
        itens.push(Item::Assistente { em: 0, texto: "cupom e cupom".into() });
        indice.atualizar("c1", &itens, false, "cupom");
        assert_eq!((indice.total(), indice.atual), (3, 0));
        indice.andar(-1);
        assert_eq!(indice.atual, 2);
        itens.push(Item::Aviso { em: 0, texto: "cupom".into(), erro: false });
        indice.atualizar("c1", &itens, false, "cupom");
        assert_eq!((indice.total(), indice.atual), (4, 2));
        // Outro termo recomeça do primeiro achado.
        indice.atualizar("c1", &itens, false, "aviso");
        assert_eq!(indice.atual, 0);
    }

    #[test]
    fn ir_ao_item_cai_na_primeira_ocorrencia_dele() {
        let itens = vec![
            item_usuario("cupom"),
            Item::Assistente { em: 0, texto: "nada".into() },
            Item::Assistente { em: 0, texto: "cupom e cupom".into() },
        ];
        let mut indice = Indice::default();
        indice.atualizar("c1", &itens, false, "cupom");
        assert_eq!(indice.ir_ao_item(2), Some(2));
        assert_eq!(indice.contador().as_deref(), Some("2 de 3"));
        // Item sem ocorrência depois dele: volta para a primeira.
        assert_eq!(indice.ir_ao_item(9), Some(0));
        assert_eq!(indice.contador().as_deref(), Some("1 de 3"));
    }
}
