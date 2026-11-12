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
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Flutuante as _, TamanhoBotao, botao_icone, campo_de_busca},
    movimento::{self, De},
};
use crate::{
    modelo::Item,
    tema::{Cores, Lucide, Tipo as _, esp, r, raio},
};

actions!(vitral, [BuscarNaConversa, AchadoSeguinte, AchadoAnterior, FecharBuscaConversa]);


// ------------------------------------------------------------------ achar no texto

/// A letra sem acento, para a busca não ligar para ele. Só o latino que aparece em português e
/// nos nomes vizinhos; o resto passa igual (minúsculas vêm depois, em `dobrar`).
fn sem_acento(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'Á' | 'À' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'É' | 'È' | 'Ê' | 'Ë' => 'E',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'Í' | 'Ì' | 'Î' | 'Ï' => 'I',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'O',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'Ú' | 'Ù' | 'Û' | 'Ü' => 'U',
        'ç' => 'c',
        'Ç' => 'C',
        'ñ' => 'n',
        'Ñ' => 'N',
        _ => c,
    }
}

/// O texto dobrado para a busca (sem acento e em minúsculas) e, para cada byte dele, a faixa de
/// bytes da letra que o gerou: é assim que uma faixa achada volta a ser uma faixa do original,
/// mesmo quando dobrar muda o tamanho (o 'İ' vira duas letras).
struct Dobrado {
    texto: String,
    origem: Vec<(u32, u32)>,
}

fn dobrar(texto: &str) -> Dobrado {
    let mut d = Dobrado { texto: String::with_capacity(texto.len()), origem: Vec::with_capacity(texto.len()) };
    for (i, c) in texto.char_indices() {
        let fim = i + c.len_utf8();
        for letra in sem_acento(c).to_lowercase() {
            d.texto.push(letra);
            d.origem.resize(d.texto.len(), (i as u32, fim as u32));
        }
    }
    d
}

impl Dobrado {
    /// A faixa do original que o trecho `faixa` do dobrado cobre (`faixa` nunca é vazia).
    fn no_original(&self, faixa: Range<usize>) -> Range<usize> {
        self.origem[faixa.start].0 as usize..self.origem[faixa.end - 1].1 as usize
    }
}

/// O termo pronto para `faixas`: dobrado e sem os espaços das pontas. Vazio quer dizer sem busca.
pub fn normalizar(termo: &str) -> String {
    dobrar(termo.trim()).texto
}

/// Onde `termo` (já normalizado) aparece em `texto`, em bytes de `texto`, sem se sobrepor.
/// `limite` corta a conta: a lista da esquerda só quer as primeiras.
fn faixas_ate(texto: &str, termo: &str, limite: usize) -> Vec<Range<usize>> {
    if termo.is_empty() || limite == 0 || texto.len() < termo.len() {
        return Vec::new();
    }
    let d = dobrar(texto);
    let mut saida = Vec::new();
    let mut de = 0;
    while let Some(k) = d.texto[de..].find(termo) {
        let achado = de + k..de + k + termo.len();
        de = achado.end;
        saida.push(d.no_original(achado));
        if saida.len() == limite {
            break;
        }
    }
    saida
}

/// Todas as ocorrências de `termo` em `texto`.
pub fn faixas(texto: &str, termo: &str) -> Vec<Range<usize>> {
    faixas_ate(texto, termo, usize::MAX)
}

/// A primeira ocorrência de `termo` em `texto` (para quem só quer saber se casou e onde).
pub fn primeira(texto: &str, termo: &str) -> Option<Range<usize>> {
    faixas_ate(texto, termo, 1).pop()
}

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

/// A chamada de ferramenta aparece na conversa (as buscas de ferramenta nunca; no modo foco, só
/// as ações sobre tarefas). Segue o filtro de `mensagens::mensagens`.
fn ferramenta_visivel(nome: &str, foco: bool) -> bool {
    nome != "ToolSearch" && (!foco || super::mensagens::acao_de_tarefa(nome).is_some())
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
        match dado {
            Item::Usuario { texto, .. }
            | Item::Assistente { texto, .. }
            | Item::Entrega { texto, .. }
            | Item::Aviso { texto, .. } => achar(Campo::Texto, texto),
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

/// A primeira chamada da sequência de chamadas em que `indice` está, do jeito que a conversa as
/// agrupa: é a chave do grupo em `expandidos`, que a trilha de 4 em diante usa para abrir.
fn primeira_da_trilha(itens: &[Item], indice: usize, foco: bool) -> usize {
    let visivel = |i: usize| match &itens[i] {
        Item::Ferramenta { nome, .. } => ferramenta_visivel(nome, foco).then_some(true),
        _ => Some(false),
    };
    let mut primeiro = indice;
    let mut i = indice;
    while i > 0 {
        i -= 1;
        match visivel(i) {
            // Outra chamada visível: o grupo continua para cima.
            Some(true) => primeiro = i,
            // Escondida: não quebra o grupo (ela não vai para a tela).
            None => {}
            Some(false) => break,
        }
    }
    primeiro
}

/// As chaves de `expandidos` que precisam entrar para a ocorrência do item `indice` ficar à
/// vista: o próprio bloco (a saída da chamada, o relatório, a mensagem recebida), o "Mostrar
/// tudo" de uma mensagem longa e, numa chamada, o grupo da trilha.
pub fn chaves_para_abrir(chat: &str, itens: &[Item], indice: usize, foco: bool) -> Vec<String> {
    let mut chaves = vec![format!("{chat}:{indice}"), format!("{chat}:{indice}:tudo")];
    if matches!(itens.get(indice), Some(Item::Ferramenta { .. })) {
        chaves.push(format!("{chat}:{}:grupo", primeira_da_trilha(itens, indice, foco)));
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
            return Some("nenhum achado".into());
        }
        Some(format!("{} de {}", self.atual + 1, self.achados.len()))
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
    fn salto_aqui(&self) -> Option<u64> {
        self.atual.map(|_| self.salto)
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
    if realce.vazio() {
        return StyledText::new(texto);
    }
    let destaques: Vec<(Range<usize>, HighlightStyle)> = faixas(&texto, &realce.termo)
        .into_iter()
        .enumerate()
        .map(|(i, faixa)| {
            let estilo = HighlightStyle {
                background_color: Some(cor_do_achado(realce.atual == Some(i))),
                ..Default::default()
            };
            (faixa, estilo)
        })
        .collect();
    StyledText::new(texto).with_highlights(destaques)
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
/// um respiro de 48 em cima: o achado nunca cola no cabeçalho.
fn rolar_ate(rolagem: &ScrollHandle, topo: Pixels, altura_do_alvo: Pixels) {
    const RESPIRO: Pixels = px(48.);
    let vista = rolagem.bounds();
    if vista.size.height <= px(0.) {
        return;
    }
    let atual = rolagem.offset();
    let acima = topo - vista.origin.y;
    let abaixo = (topo + altura_do_alvo) - (vista.origin.y + vista.size.height);
    // Já dentro da vista, com o respiro: não mexe (rolar de novo brigaria com a roda).
    let ajuste = if acima < RESPIRO {
        acima - RESPIRO
    } else if abaixo > px(0.) {
        abaixo.min(acima - RESPIRO)
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
        let foco = self.paineis[ix].modo_foco;
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
        let foco = self.paineis[ix].modo_foco;
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
        let contador = painel.busca.indice.contador();
        let sem_achado = total == 0 && contador.is_some();
        let seta = |nome: &'static str, icone: Lucide, dica: &'static str, acao: &dyn Action, passo: i32| {
            botao_icone(SharedString::from(format!("{nome}-{id}")), icone, dica, TamanhoBotao::Barra, window, cx)
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
                    .min_w_0()
                    .on_action(cx.listener(|this, _: &Escape, window, cx| this.fechar_busca_na_conversa(window, cx)))
                    .child(
                        campo_de_busca(SharedString::from(format!("campo-busca-{id}")), &painel.busca.campo, None, window, cx)
                            .bg(Cores::recuo_no_flutuante()),
                    ),
            )
            .children(contador.map(|texto| {
                div()
                    .flex_none()
                    .px(esp::E4)
                    .metadado()
                    .text_color(if sem_achado { Cores::ambar() } else { Cores::tenue() })
                    .child(texto)
            }))
            .child(seta("achado-anterior", Lucide::ChevronUp, "Achado anterior", &AchadoAnterior, -1))
            .child(seta("achado-seguinte", Lucide::ChevronDown, "Próximo achado", &AchadoSeguinte, 1))
            .child(
                botao_icone(SharedString::from(format!("fechar-busca-{id}")), Lucide::X, "Fechar a busca", TamanhoBotao::Barra, window, cx)
                    .tooltip_with_action("Fechar a busca", &FecharBuscaConversa, Some("BuscaConversa"))
                    .on_click(cx.listener(|this, _, window, cx| this.fechar_busca_na_conversa(window, cx))),
            );
        // A barra fica por cima da conversa, encostada no alto e à direita, e nunca mais larga
        // que o painel (no painel mínimo ela ocupa tudo menos as margens).
        Some(
            div()
                .absolute()
                .top(esp::E8)
                .right(esp::E8)
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
    use super::{Campo, Indice, Realce, andar, chaves_para_abrir, faixas, normalizar, ocorrencias, primeira};
    use crate::modelo::Item;

    fn item_usuario(texto: &str) -> Item {
        Item::Usuario {
            em: 0,
            texto: texto.into(),
            anexos: Vec::new(),
            ponto: None,
            ultrathink: false,
            ultracode: false,
            referencias: Vec::new(),
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
        // No modo foco as chamadas comuns saem da tela e da conta.
        assert_eq!(ocorrencias(&itens, "cupom", true).len(), 4);
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

    #[test]
    fn contador_e_realce_seguem_a_ocorrencia_atual() {
        let itens = vec![item_usuario("cupom e cupom"), Item::Assistente { em: 0, texto: "cupom".into() }];
        let mut indice = Indice::default();
        indice.atualizar("c1", &itens, false, "cupom");
        assert_eq!(indice.total(), 3);
        assert_eq!(indice.contador().as_deref(), Some("1 de 3"));
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
