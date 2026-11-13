//! Lista da esquerda, numa peça de vidro de leitura: no alto, o logo e as abas Conversas ·
//! Arquivos; os chats numa lista corrida ("Esperando você", "Rodando agora", "Fixadas" e todos os
//! outros, do mais recente ao mais antigo, uma linha cada); a árvore de arquivos da pasta do chat
//! aberto; no pé, as arquivadas, os agentes rodando, o sino, a busca e o "+".

use std::{
    collections::{BTreeMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui_kit::component::{
    ElementExt as _, Selectable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Escape, InputState, MoveDown},
    list::ListItem,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
    tree::{TreeEntry, TreeItem, TreeState, tree},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    AbaLateral, Buscar, Filtro, Gaveta, Janela, NovaConversa,
    componentes::{
        BotaoRedondo, CabecalhoDeSecao, CartaoDeLista, barra_de_rolagem, ControleSegmentado, EstadoVazio,
        ALTURA_DA_LINHA_DE_CHAT, PreviaArrasto, Segmento, Selo, Simbolo, Sinal, TamanhoBotao, Tom,
        VarianteRedonda, aviso, aviso_com_acao, botao_fantasma, botao_icone, botao_primario, campo_de_busca_de,
        contagem, icone, motivo_curto, numero_painel, sinal_do_chat, variante_fantasma,
    },
    dialogos::{com_til, nome_da_pasta},
    duracao, menus,
    paineis::ArrastoChat,
    sem_arrastar, tempo_relativo,
};
use crate::{
    modelo::{Chat, Execucao, Item, agora},
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, altura, esp, r, raio},
};

/// Larguras da lista, a 100% (a conta é a mesma em qualquer zoom, porque o texto cresce junto).
/// Abaixo de cada uma, algo dá lugar ao título em vez de ser cortado no meio. As abas e as ações
/// das seções foram medidas com a peça de 6; a de 8 tira 4 da largura útil, e os limites sobem 4.
const LIMITE_ROTULOS: f32 = 288.;
const LIMITE_ESTREITA: f32 = 276.;
const LIMITE_APERTADA: f32 = 260.;

/// O que cabe na lista nesta largura.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Aperto {
    /// Abaixo de 288: no hover, o cartão fica só com o "…" (que também tem Abrir ao lado).
    sem_rotulos: bool,
    /// Abaixo de 276: abas só com ícone e a prévia com o rótulo curto do motivo (o sinal já diz
    /// qual é).
    estreita: bool,
    /// Abaixo de 260: a busca de arquivos diz só "Buscar" e "Retomar tudo"
    /// ficam só com o ícone.
    apertada: bool,
}

impl Aperto {
    fn de(largura_a_100: f32) -> Aperto {
        Aperto {
            sem_rotulos: largura_a_100 < LIMITE_ROTULOS,
            estreita: largura_a_100 < LIMITE_ESTREITA,
            apertada: largura_a_100 < LIMITE_APERTADA,
        }
    }
}

/// O texto da busca: "Buscar", e o resto na dica do campo (com o Ctrl+K).
fn dica_da_busca(aba: AbaLateral, apertada: bool) -> &'static str {
    match (aba, apertada) {
        (AbaLateral::Arquivos, false) => "Buscar arquivos",
        _ => "Buscar",
    }
}

/// Onde uma conversa aparece na lista.
#[derive(PartialEq)]
enum Secao {
    EsperandoVoce,
    Rodando,
    Fixadas,
    Conversas,
}

/// Faixas de data das conversas paradas, pela última atividade: o tempo de cada cartão já está
/// nele, então bastam duas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Faixa {
    Recentes,
    Antigas,
}

impl Faixa {
    const TODAS: [Faixa; 2] = [Faixa::Recentes, Faixa::Antigas];

    fn nome(self) -> &'static str {
        match self {
            Faixa::Recentes => "Recentes",
            Faixa::Antigas => "Mais antigas",
        }
    }

    /// `hoje` é a meia-noite local de hoje; os 7 dias contam a partir dela.
    fn de(instante: u64, hoje: u64) -> Faixa {
        const DIA: u64 = 86_400;
        if instante >= hoje.saturating_sub(7 * DIA) { Faixa::Recentes } else { Faixa::Antigas }
    }
}

/// Meia-noite, no fuso local, do dia de `instante`. Num dia em que o horário de verão começa à
/// meia-noite, ela não existe e vale a 1h (como no `mktime`).
fn meia_noite_local(instante: u64) -> u64 {
    use chrono::{Local, TimeDelta};
    let dia = crate::modelo::no_fuso_local(instante as i64).date_naive().and_time(chrono::NaiveTime::MIN);
    let inicio = [dia, dia + TimeDelta::hours(1)].into_iter().find_map(|hora| hora.and_local_timezone(Local).earliest());
    match inicio.map(|i| i.timestamp()) {
        Some(inicio) if inicio >= 0 => inicio as u64,
        _ => instante - instante % 86_400,
    }
}

/// Uma conversa da lista e o que ela espera de você.
type DaLista<'a> = (&'a Chat, Option<Motivo>);

/// Por que uma conversa está em "Esperando você" (a regra única fica no motor).
pub(super) use crate::motor::Espera as Motivo;
#[cfg(test)]
use crate::motor::espera as motivo;
use crate::motor::Pendencia as PendenciaDoMotor;

/// O que a conversa espera de você, como aparece na lista: a resposta nova da conversa aberta
/// não conta, porque você já está lendo.
fn pendencia_visivel(motor: &Motor, chat: &Chat, selecionado: Option<&str>) -> Option<Motivo> {
    let aberta = selecionado == Some(chat.id.as_str());
    motor.espera_do_chat(chat).filter(|m| !(aberta && *m == Motivo::Nova))
}

/// O que uma pendência pede, com o que a frase dela precisa. A mesma na lista e no sino.
pub(super) enum Causa {
    /// O resumo da ferramenta ("Bash · npm test").
    Permissao(String),
    Plano,
    /// O motivo já curto ("Vitral fechou").
    Parada(String),
    /// O texto do erro: vale a primeira linha.
    Erro(String),
}

impl Causa {
    /// A de uma linha do sino; a resposta nova não tem.
    pub(super) fn da_pendencia(p: &PendenciaDoMotor) -> Option<Causa> {
        Some(match p {
            PendenciaDoMotor::Permissao { resumo, .. } => Causa::Permissao(resumo.clone()),
            PendenciaDoMotor::Plano { .. } => Causa::Plano,
            PendenciaDoMotor::Parada { motivo, .. } => Causa::Parada(motivo_curto(motivo)),
            PendenciaDoMotor::Erro { texto, .. } => Causa::Erro(texto.clone()),
            PendenciaDoMotor::Nova { .. } => return None,
        })
    }

    /// O sinal, o rótulo e o detalhe que vem depois dele.
    fn partes(&self) -> (Sinal, &'static str, String) {
        match self {
            Causa::Permissao(resumo) => (Sinal::Pedido, "Permissão", resumo.clone()),
            Causa::Plano => (Sinal::Pedido, "Plano para aprovar", String::new()),
            Causa::Parada(porque) => (Sinal::Parada, "Parada", porque.clone()),
            Causa::Erro(texto) => (Sinal::Erro, "Erro", primeira_linha(texto)),
        }
    }
}

/// O motivo de um cartão em "Esperando você", antes do texto da prévia: o ponto do sinal e o
/// rótulo ("Parada"). A resposta nova não tem: o ponto azul e o título em forte já dizem.
pub(super) struct MotivoDaPrevia {
    sinal: Sinal,
    rotulo: &'static str,
}

impl MotivoDaPrevia {
    /// O motivo de uma causa e o detalhe que vai depois dele.
    pub(super) fn de(causa: &Causa) -> (MotivoDaPrevia, String) {
        let (sinal, rotulo, detalhe) = causa.partes();
        (MotivoDaPrevia { sinal, rotulo }, detalhe)
    }

    /// O curto (lista estreita) fica sem rótulo: o sinal já diz o quê.
    fn rotulo(&self, curto: bool) -> String {
        if curto { String::new() } else { self.rotulo.to_string() }
    }
}

/// O que a prévia mostra: o rótulo do motivo (o curto na lista estreita, quando há detalhe para
/// ele abrir mão) e o texto, com o " · " quando vem depois de um rótulo.
fn partes_da_previa(motivo: Option<&MotivoDaPrevia>, texto: &str, estreita: bool) -> (Option<String>, String) {
    let curto = estreita && !texto.is_empty();
    let rotulo = motivo.map(|m| m.rotulo(curto)).filter(|r| !r.is_empty());
    let texto = if rotulo.is_some() && !texto.is_empty() { format!("· {texto}") } else { texto.to_string() };
    (rotulo, texto)
}

fn primeira_linha(texto: &str) -> String {
    texto
        .lines()
        .map(|l| l.trim().trim_start_matches('#').trim())
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect()
}

/// Onde `busca` (já em minúsculas) aparece em `texto`, ignorando maiúsculas, em bytes de `texto`.
fn trecho(texto: &str, busca: &str) -> Option<Range<usize>> {
    let alvo: Vec<char> = busca.chars().flat_map(char::to_lowercase).collect();
    if alvo.is_empty() {
        return None;
    }
    let letras: Vec<(usize, char)> = texto.char_indices().collect();
    for inicio in 0..letras.len() {
        let (mut k, mut j) = (0, inicio);
        while j < letras.len() && k < alvo.len() {
            let baixa: Vec<char> = letras[j].1.to_lowercase().collect();
            if !alvo[k..].starts_with(&baixa) {
                break;
            }
            k += baixa.len();
            j += 1;
        }
        if k == alvo.len() {
            let fim = letras.get(j).map_or(texto.len(), |l| l.0);
            return Some(letras[inicio].0..fim);
        }
    }
    None
}

/// O termo achado, em forte: no título, no nome do arquivo e no recorte da prévia.
fn realce_da_busca() -> HighlightStyle {
    HighlightStyle { font_weight: Some(FontWeight::SEMIBOLD), color: Some(Cores::forte()), ..Default::default() }
}

/// Título com o peso do não lido ou com o trecho da busca em forte; `None` quando é o simples.
fn titulo_rico(titulo: &str, busca: &str, nao_lido: bool) -> Option<StyledText> {
    let faixa = if nao_lido {
        0..titulo.len()
    } else {
        trecho(titulo, busca)?
    };
    Some(StyledText::new(titulo.to_string()).with_highlights([(faixa, realce_da_busca())]))
}

/// Casa com a busca (já em minúsculas): o título ou o texto de alguma fala.
fn casa_com_a_busca(c: &Chat, busca: &str) -> bool {
    busca.is_empty()
        || c.titulo.to_lowercase().contains(busca)
        || c.cwd.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains(busca))
        || c.itens.iter().any(|i| match i {
            Item::Usuario { texto, .. } | Item::Assistente { texto, .. } | Item::Entrega { texto, .. } => {
                texto.to_lowercase().contains(busca)
            }
            _ => false,
        })
}

/// As conversas arquivadas que casam com a busca, da mais recente para a mais antiga. Sem busca,
/// nenhuma: fora dela, as arquivadas ficam em Arquivadas.
fn arquivadas_que_casam<'a>(chats: impl Iterator<Item = &'a Chat>, busca: &str) -> Vec<&'a Chat> {
    if busca.is_empty() {
        return Vec::new();
    }
    let mut lista: Vec<&Chat> = chats.filter(|c| c.arquivado && casa_com_a_busca(c, busca)).collect();
    lista.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
    lista
}

/// Letras de contexto antes do termo, no recorte da prévia.
const ANTES_DO_TERMO: usize = 24;

/// Numa linha só: cada sequência de espaços e quebras vira um espaço.
fn uma_linha(texto: &str) -> String {
    let mut saida = String::with_capacity(texto.len());
    let mut espaco = false;
    for c in texto.chars() {
        if c.is_whitespace() {
            if !espaco {
                saida.push(' ');
            }
            espaco = true;
        } else {
            saida.push(c);
            espaco = false;
        }
    }
    saida
}

/// O pedaço de `texto` em volta de onde `busca` casou, numa linha: até 24 letras antes (a partir
/// do começo de uma palavra, com "…" quando corta) e o resto depois. A faixa é a do termo, já no
/// recorte.
fn recorte(texto: &str, busca: &str) -> Option<(String, Range<usize>)> {
    let faixa = trecho(texto, busca)?;
    let antes = &texto[..faixa.start];
    let letras: Vec<usize> = antes.char_indices().map(|(i, _)| i).collect();
    let mut inicio = letras.len().checked_sub(ANTES_DO_TERMO).map_or(0, |k| letras[k]);
    let no_meio_da_palavra = antes[..inicio].chars().next_back().is_some_and(|c| !c.is_whitespace());
    if no_meio_da_palavra && let Some(espaco) = antes[inicio..].find(char::is_whitespace) {
        inicio += espaco;
    }
    let mut saida = if inicio > 0 { "…".to_string() } else { String::new() };
    saida.push_str(&uma_linha(antes[inicio..].trim_start()));
    let comeco = saida.len();
    saida.push_str(&uma_linha(&texto[faixa.clone()]));
    let fim = saida.len();
    saida.extend(uma_linha(&texto[faixa.end..]).chars().take(160));
    Some((saida, comeco..fim))
}

/// Onde a busca casou numa conversa cujo título não casa: a fala mais recente com o termo ("Você:
/// …", "chat a1b2 “Cupons”: …" numa mensagem de outro chat).
fn achado_na_conversa(chat: &Chat, busca: &str) -> Option<(String, Range<usize>)> {
    chat.itens.iter().rev().find_map(|item| {
        let (quem, texto) = match item {
            Item::Usuario { texto, .. } => (Some("Você"), texto),
            Item::Entrega { de, texto, .. } => (Some(de.as_str()), texto),
            Item::Assistente { texto, .. } => (None, texto),
            _ => return None,
        };
        let (pedaco, faixa) = recorte(texto, busca)?;
        let prefixo = quem.map(|q| format!("{q}: ")).unwrap_or_default();
        let deslocar = prefixo.len();
        Some((prefixo + &pedaco, faixa.start + deslocar..faixa.end + deslocar))
    })
}

/// Para a próxima posição do realce com `tecla`, numa lista de `n` cartões.
fn posicao_do_realce(n: usize, atual: Option<usize>, tecla: &str) -> Option<usize> {
    if n == 0 {
        return None;
    }
    match tecla {
        "down" => Some(atual.map_or(0, |i| (i + 1).min(n - 1))),
        "up" => Some(atual.map_or(0, |i| i.saturating_sub(1))),
        "home" => Some(0),
        "end" => Some(n - 1),
        _ => None,
    }
}

/// "3 rodando": a pasta inteira, sem filtro; nada quando nada roda.
fn texto_agentes(rodando: usize) -> Option<String> {
    (rodando > 0).then(|| format!("{rodando} rodando"))
}

/// "1.234": o milhar separado por ponto.
pub(super) fn milhar(n: usize) -> String {
    let digitos = n.to_string();
    let mut texto = String::new();
    for (i, c) in digitos.chars().enumerate() {
        if i > 0 && (digitos.len() - i) % 3 == 0 {
            texto.push('.');
        }
        texto.push(c);
    }
    texto
}

/// Uma pasta ou um arquivo da árvore, com os filhos já em ordem (pastas antes, sem caixa).
#[derive(Debug, PartialEq)]
struct No {
    nome: String,
    caminho: String,
    pasta: bool,
    filhos: Vec<No>,
}

/// A árvore dos caminhos relativos ("src/ui/lateral.rs").
fn montar_arvore(caminhos: &[String]) -> Vec<No> {
    #[derive(Default)]
    struct Pasta {
        pastas: BTreeMap<String, Pasta>,
        arquivos: Vec<String>,
    }
    fn converter(pasta: Pasta, prefixo: &str) -> Vec<No> {
        let junta = |nome: &str| if prefixo.is_empty() { nome.to_string() } else { format!("{prefixo}/{nome}") };
        let mut pastas: Vec<No> = pasta
            .pastas
            .into_iter()
            .map(|(nome, p)| {
                let caminho = junta(&nome);
                No { filhos: converter(p, &caminho), nome, caminho, pasta: true }
            })
            .collect();
        let mut arquivos: Vec<No> = pasta
            .arquivos
            .into_iter()
            .map(|nome| No { caminho: junta(&nome), nome, pasta: false, filhos: Vec::new() })
            .collect();
        pastas.sort_by_key(|n| n.nome.to_lowercase());
        arquivos.sort_by_key(|n| n.nome.to_lowercase());
        pastas.extend(arquivos);
        pastas
    }
    let mut raiz = Pasta::default();
    for caminho in caminhos {
        let mut partes: Vec<&str> = caminho.split('/').filter(|p| !p.is_empty()).collect();
        let Some(arquivo) = partes.pop() else {
            continue;
        };
        let mut atual = &mut raiz;
        for parte in partes {
            atual = atual.pastas.entry(parte.to_string()).or_default();
        }
        atual.arquivos.push(arquivo.to_string());
    }
    converter(raiz, "")
}

/// Os itens do `Tree` do kit: abertas as pastas que já estavam e o caminho até o arquivo aberto.
fn itens_da_arvore(nos: Vec<No>, abertas: &HashSet<String>, aberto: Option<&str>) -> Vec<TreeItem> {
    nos.into_iter()
        .map(|no| {
            let abrir = no.pasta
                && (abertas.contains(&no.caminho)
                    || aberto.is_some_and(|a| a.starts_with(&format!("{}/", no.caminho))));
            TreeItem::new(no.caminho, no.nome)
                .children(itens_da_arvore(no.filhos, abertas, aberto))
                .expanded(abrir)
        })
        .collect()
}

/// As pastas abertas de uma árvore (para mantê-las ao reler os arquivos).
fn pastas_abertas(arvore: &TreeState) -> HashSet<String> {
    (0..)
        .map_while(|ix| arvore.entry(ix))
        .filter(|e| e.is_folder() && e.is_expanded())
        .map(|e| e.item().id.to_string())
        .collect()
}

/// O que a lista precisa saber em todo cartão.
struct Contexto {
    busca: String,
    aperto: Aperto,
    /// A lista tem o foco e a última entrada veio do teclado: o realce aparece.
    teclado: bool,
    motor: Entity<Motor>,
    janela: WeakEntity<Janela>,
}

/// Um cartão que o teclado alcança: o chat, a posição entre os filhos da lista (para rolar até
/// ele) e se o Enter também foca a caixa.
struct Navegavel {
    chat: String,
    indice: usize,
    pendencia: bool,
}

/// Os filhos da lista de conversas, numa sequência só (assim a rolagem chega a cada um).
#[derive(Default)]
struct Montagem {
    itens: Vec<AnyElement>,
    navegaveis: Vec<Navegavel>,
    /// A lista dos chats é corrida: o que espera você, o que roda, as fixadas e os outros, nessa
    /// ordem, sem o título de cada seção (o ponto do cartão e o alfinete já dizem qual é). As
    /// Arquivadas têm os títulos das faixas de data.
    corrida: bool,
    /// As posições (entre os filhos da lista) que estão perto da área à vista: fora delas, o cartão
    /// vira um espaço da mesma altura. `None` desenha tudo.
    a_vista: Option<Range<usize>>,
}

/// As posições da lista corrida que ficam perto da área à vista, com uma tela de folga para cada
/// lado. A lista redesenha a cada tecla e a cada evento de qualquer chat: com 130 chats, montar
/// todos os cartões (menu, arrasto, botões, dica) era o que mais pesava com a lista aberta. Sem
/// o tamanho da área (o primeiro quadro), tudo é desenhado.
fn linhas_a_vista(rolado: f32, visivel: f32, altura: f32, margem: f32) -> Option<Range<usize>> {
    if altura < 8. || visivel <= 0. {
        return None;
    }
    let passo = altura + margem;
    let folga = visivel.max(passo * 12.);
    let de = ((rolado - folga) / passo).floor().max(0.) as usize;
    let ate = ((rolado + visivel + folga) / passo).ceil().max(0.) as usize + 1;
    Some(de..ate)
}

impl Montagem {
    /// A primeira seção visível fica com 4 em cima.
    fn cabecalho(&mut self, c: CabecalhoDeSecao) {
        if self.corrida {
            if self.itens.is_empty() {
                self.itens.push(div().flex_none().h(esp::E4).into_any_element());
            }
            return;
        }
        let item = c.primeira(self.itens.is_empty()).into_any_element();
        self.itens.push(item);
    }

    fn navegavel(&mut self, e: AnyElement, chat: &str, pendencia: bool) {
        self.navegaveis.push(Navegavel { chat: chat.to_string(), indice: self.itens.len(), pendencia });
        self.itens.push(e);
    }
}

/// O que a aba Arquivos mostra: a pasta do chat aberto, os arquivos dela e, com busca, os que
/// casam.
struct Arquivos {
    pasta: Option<PathBuf>,
    lista: Rc<Vec<String>>,
    achados: Option<Rc<Vec<String>>>,
    arvore: Option<Entity<TreeState>>,
    rolagem: UniformListScrollHandle,
}

/// Linha de 20 (na altura do título) ou 16 (na da prévia) da coluna da meta.
fn linha_meta(altura: f32) -> Div {
    div().h(r(altura)).flex().items_center().justify_end().gap(esp::E4)
}

/// A hora de um cartão ("12 min", "3 d"): em mono de 11, para os números ficarem numa coluna só.
fn hora_do_cartao() -> Div {
    linha_meta(20.).font_family(crate::tema::FONTE_MONO).text_size(r(11.))
}

/// A prévia: o motivo (o ponto do sinal e o rótulo em texto) e o texto tênue cortado, separados
/// por " · ". Na lista estreita o rótulo sai quando há texto, porque o sinal basta. O sino usa a
/// mesma.
pub(super) fn previa_elemento(motivo: Option<&MotivoDaPrevia>, texto: String, estreita: bool) -> Div {
    let (rotulo, texto) = partes_da_previa(motivo, &texto, estreita);
    div()
        .flex()
        .items_center()
        .gap(esp::E4)
        .min_w_0()
        .when_some(motivo, |d, m| d.child(m.sinal.ponto(6.)))
        .when_some(rotulo, |d, rotulo| {
            // O rótulo vem antes do texto e, sem lugar, encolhe com reticências; o curto não.
            d.child(
                div()
                    .when(estreita, |d| d.flex_none())
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(Cores::texto())
                    .child(rotulo),
            )
        })
        .child(div().flex_1().min_w_0().truncate().child(texto))
}

/// A busca da lista: o `campo_de_busca` sem o atalho e com o texto curto; o que ela acha e o
/// Ctrl+K ficam na dica.
fn campo_da_lista(estado: &Entity<InputState>, aba: AbaLateral, estreita: bool, window: &Window, cx: &App) -> Stateful<Div> {
    let focado = estado.read(cx).focus_handle(cx).is_focused(window);
    let conversas = aba == AbaLateral::Conversas;
    // O atalho ("Ctrl" "K") à direita, com o campo vazio e sem foco; abaixo de 276 de lista, nada.
    // Só na aba Conversas: o Ctrl+K leva a ela, então na busca de arquivos ele enganaria.
    let atalho: Option<Box<dyn Action>> = (!estreita && conversas).then(|| Box::new(Buscar) as Box<dyn Action>);
    // No alto da lista, o logo, a busca e o "+" têm a mesma altura (40).
    campo_de_busca_de("busca-lista", estado, atalho, 40., window, cx)
        // Com o cursor no campo, a dica sairia por cima dos resultados.
        .when(!focado, |s| {
            s.tooltip(move |window, cx| {
                if conversas {
                    Tooltip::new("Buscar nas conversas\nNos títulos e nas mensagens, também das arquivadas").action(&Buscar, None).build(window, cx)
                } else {
                    Tooltip::new("Buscar nos arquivos da pasta").build(window, cx)
                }
            })
        })
}

/// Barra de 3 px da linha escolhida.
fn barra_de_selecao() -> Div {
    div()
        .absolute()
        .left(r(2.))
        .top(r(6.))
        .bottom(r(6.))
        .w(px(3.))
        .rounded_full()
        .bg(Cores::destaque())
}

/// Botão pequeno de texto no cabeçalho de uma seção ("Retomar tudo"); na
/// lista apertada, só o ícone, com o rótulo na dica.
fn botao_de_secao(
    id: &'static str,
    rotulo: &'static str,
    icone_: Lucide,
    apertada: bool,
    window: &Window,
    cx: &App,
) -> Button {
    if apertada {
        return botao_icone(id, icone_, rotulo, TamanhoBotao::Linha, window, cx).h(r(20.));
    }
    Button::new(id)
        .custom(variante_fantasma(cx).foreground(Cores::tenue()))
        .h(r(20.))
        .px(esp::E6)
        .rounded(raio::de(20.))
        .child(div().metadado().font_weight(FontWeight::MEDIUM).child(rotulo))
}

/// Recolhe ou mostra a seção `nome`.
fn alternar_secao(janela: WeakEntity<Janela>, nome: &'static str) -> impl Fn(&mut Window, &mut App) + 'static {
    move |_, cx| {
        let _ = janela.update(cx, |this, cx| {
            if !this.secoes_recolhidas.remove(nome) {
                this.secoes_recolhidas.insert(nome);
            }
            cx.notify();
        });
    }
}

/// Menu de um arquivo ou pasta da aba Arquivos (o "…" do hover e o clique direito).
fn menu_do_arquivo(
    menu: PopupMenu,
    janela: WeakEntity<Janela>,
    pasta_do_chat: PathBuf,
    caminho: String,
    pasta: bool,
) -> PopupMenu {
    let completo = pasta_do_chat.join(&caminho);
    let onde = if pasta {
        completo.clone()
    } else {
        completo.parent().map(Path::to_path_buf).unwrap_or_else(|| pasta_do_chat.clone())
    };
    let (mencao, copia) = (caminho.clone(), caminho);
    menu.when(!pasta, |menu| {
        menu.item(PopupMenuItem::new("Mencionar na conversa").icon(Lucide::AtSign).on_click(
            move |_, window, cx| {
                let _ = janela.update(cx, |this, cx| this.mencionar(&mencao, window, cx));
            },
        ))
        .separator()
    })
    .item(
        PopupMenuItem::new("Abrir no VS Code")
            .icon(Lucide::CodeXml)
            .external()
            .on_click(move |_, _, _| crate::sistema::abrir_no_editor(&completo)),
    )
    .item(PopupMenuItem::new("Copiar o caminho").icon(Lucide::Copy).on_click(move |_, window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(copia.clone()));
        window.push_notification(aviso(Tom::Pronto, "Caminho copiado."), cx);
    }))
    .item(
        PopupMenuItem::new("Mostrar na pasta")
            .icon(Lucide::FolderOpen)
            .external()
            .on_click(move |_, _, cx| crate::sistema::abrir_pasta(&onde, cx)),
    )
}

/// Mencionar e "…" de um arquivo, no hover da linha do `grupo`.
fn acoes_do_arquivo(
    grupo: &'static str,
    ix: usize,
    janela: WeakEntity<Janela>,
    pasta: PathBuf,
    caminho: String,
    window: &Window,
    cx: &App,
) -> Div {
    let (j, c) = (janela.clone(), caminho.clone());
    div()
        .flex()
        .flex_none()
        .gap(esp::E2)
        .opacity(0.)
        .group_hover(grupo, |s| s.opacity(1.))
        .child(
            botao_icone(("mencionar-arquivo", ix), Lucide::AtSign, "Mencionar na conversa", TamanhoBotao::Linha, window, cx)
                .on_click(move |_, window, cx| {
                    let _ = j.update(cx, |this, cx| this.mencionar(&c, window, cx));
                }),
        )
        .child(
            botao_icone(("mais-do-arquivo", ix), Lucide::Ellipsis, "Mais opções do arquivo\nTambém no clique direito", TamanhoBotao::Linha, window, cx)
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                    menu_do_arquivo(menu, janela.clone(), pasta.clone(), caminho.clone(), false)
                }),
        )
}

/// Uma linha da árvore de arquivos: 28 de altura e 16 de recuo por nível.
#[allow(clippy::too_many_arguments)]
fn linha_da_arvore(
    ix: usize,
    entrada: &TreeEntry,
    pasta: &Path,
    aberto: Option<&str>,
    janela: &WeakEntity<Janela>,
    window: &Window,
    cx: &App,
) -> ListItem {
    const GRUPO: &str = "linha-da-arvore";
    let caminho = entrada.item().id.to_string();
    let e_pasta = entrada.is_folder();
    let expandida = entrada.is_expanded();
    let escolhido = aberto == Some(caminho.as_str());
    let simbolo = match (e_pasta, expandida) {
        (true, true) => Lucide::FolderOpen,
        (true, false) => Lucide::Folder,
        // Uma imagem se acha na árvore pelo ícone, antes de abrir.
        _ if crate::claude::tipo_imagem(Path::new(&caminho)).is_some() => Lucide::FileImage,
        _ => Lucide::FileText,
    };
    let seta = e_pasta.then(|| icone(if expandida { Lucide::ChevronDown } else { Lucide::ChevronRight }, 12., window));
    let item = ListItem::new(ix)
        .group(GRUPO)
        .h(r(28.))
        .py_0()
        .pl(r(8. + 16. * entrada.depth() as f32))
        .pr(esp::E4)
        .rounded(raio::CONTROLE)
        .when(escolhido, |l| l.bg(Cores::selecao()).child(barra_de_selecao()))
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap(esp::E6)
                .child(div().flex_none().w(r(12.)).text_color(Cores::tenue()).children(seta))
                .child(div().flex_none().text_color(Cores::tenue()).child(icone(simbolo, 16., window)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(r(13.))
                        .line_height(r(18.))
                        .font_weight(if e_pasta { FontWeight::MEDIUM } else { FontWeight::NORMAL })
                        .text_color(Cores::texto())
                        .child(entrada.item().label.clone()),
                )
                .when(!e_pasta, |d| {
                    d.child(acoes_do_arquivo(GRUPO, ix, janela.clone(), pasta.to_path_buf(), caminho.clone(), window, cx))
                }),
        );
    if e_pasta {
        return item;
    }
    let (janela, raiz) = (janela.clone(), pasta.to_path_buf());
    item.on_click(move |_, window, cx| {
        let _ = janela.update(cx, |this, cx| this.abrir_arquivo(raiz.clone(), caminho.clone(), window, cx));
    })
}

/// Um resultado da busca de arquivos: o nome com o trecho em forte e a pasta cortada no meio.
#[allow(clippy::too_many_arguments)]
fn linha_achada(
    ix: usize,
    caminho: &str,
    termo: &str,
    pasta: &Path,
    aberto: Option<&str>,
    janela: &WeakEntity<Janela>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    const GRUPO: &str = "linha-achada";
    let (dir, nome) = match caminho.rsplit_once('/') {
        Some((d, n)) => (format!("{d}/"), n.to_string()),
        None => (String::new(), caminho.to_string()),
    };
    let escolhido = aberto == Some(caminho);
    let nome_rico = match trecho(&nome, termo) {
        Some(faixa) => StyledText::new(nome.clone()).with_highlights([(faixa, realce_da_busca())]),
        None => StyledText::new(nome.clone()),
    };
    let dica = SharedString::from(caminho.to_string());
    let (j_clique, j_menu) = (janela.clone(), janela.clone());
    let (p_clique, c_clique, c_menu) = (pasta.to_path_buf(), caminho.to_string(), caminho.to_string());
    let pasta_menu = pasta.to_path_buf();
    div()
        .id(("arquivo-achado", ix))
        .group(GRUPO)
        .relative()
        .flex()
        .items_center()
        .gap(esp::E8)
        .h(r(32.))
        .px(esp::E8)
        .rounded(raio::CONTROLE)
        .cursor_pointer()
        .when(escolhido, |d| d.bg(Cores::selecao()).child(barra_de_selecao()))
        .when(!escolhido, |d| d.hover(|s| s.bg(Cores::hover())))
        .child(div().flex_none().text_color(Cores::tenue()).child(icone(Lucide::FileText, 16., window)))
        // O nome corta no meio para a extensão continuar à vista; a pasta some antes.
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_middle()
                .corpo_ui()
                .text_color(Cores::texto())
                .child(nome_rico),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_middle()
                .metadado()
                .text_color(Cores::apagado())
                .child(dir),
        )
        .child(acoes_do_arquivo(GRUPO, ix, janela.clone(), pasta.to_path_buf(), caminho.to_string(), window, cx))
        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        .on_click(move |_, window, cx| {
            let _ = j_clique.update(cx, |this, cx| this.abrir_arquivo(p_clique.clone(), c_clique.clone(), window, cx));
        })
        .context_menu(move |menu, _, _| menu_do_arquivo(menu, j_menu.clone(), pasta_menu.clone(), c_menu.clone(), false))
        .into_any_element()
}

impl Janela {
    pub(super) fn lateral(
        &mut self,
        motor_entidade: &Entity<Motor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        // A largura vem do quadro anterior (a divisória redimensiona a lista), guardada a 100%:
        // com o zoom mudando, a lista cresce junto e a conta já sai certa no primeiro quadro.
        let inicial = self.larguras.lista;
        let largura = window.use_keyed_state("largura-da-lista", cx, move |_, _| inicial);
        let largura_atual = *largura.read(cx);
        let aperto = Aperto::de(largura_atual);
        let dica = dica_da_busca(self.aba, aperto.apertada);
        if self.dica_busca != dica {
            self.dica_busca = dica;
            self.busca.update(cx, |b, cx| b.set_placeholder(dica, window, cx));
        }
        let busca = self.busca.read(cx).value().to_lowercase();
        // A aba Arquivos mostra a pasta do chat aberto.
        let conversas = self.aba == AbaLateral::Conversas;
        let arquivos = (!conversas).then(|| self.preparar_arquivos(motor_entidade, &busca, window, cx));
        let teclado = self.foco_lista.is_focused(window) && window.last_input_was_keyboard();
        let motor = motor_entidade.read(cx);
        let ctx = Contexto {
            busca: busca.clone(),
            aperto,
            teclado,
            motor: motor_entidade.clone(),
            janela: cx.entity().downgrade(),
        };

        let abas = self.abas_da_lista(motor, &ctx, conversas);
        let montagem = if conversas { self.lista_conversas(motor, &ctx, window, cx) } else { Montagem::default() };
        let linha_busca = self.linha_da_busca(&montagem, &ctx, window, cx);
        let escopo = self.linha_escopo(motor, arquivos.as_ref(), &ctx, window, cx);
        let rodape = self.rodape(motor, arquivos.as_ref(), window, cx);
        let conteudo = match &arquivos {
            Some(a) => self.conteudo_arquivos(a, &ctx, window, cx),
            None => {
                let navegaveis = Rc::new(montagem.navegaveis);
                // A área vai até a borda da peça (a barra fina fica ali) e devolve os 8 de margem.
                v_flex()
                    .id("lista-lateral")
                    .flex_1()
                    .min_h_0()
                    .mr(r(-8.))
                    .pr(esp::PECA)
                    .overflow_y_scroll()
                    .track_scroll(&self.rolagem_lista)
                    .track_focus(&self.foco_lista)
                    .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                        this.tecla_na_lista(&navegaveis, ev, window, cx)
                    }))
                    .children(montagem.itens)
                    .child(div().flex_none().h(esp::E8))
                    .child(barra_de_rolagem("barra-lista", &self.rolagem_lista))
                    .into_any_element()
            }
        };

        v_flex()
            .size_full()
            .min_w_0()
            .px(esp::PECA)
            .rounded(crate::tema::raio_da_peca())
            .on_prepaint(move |limites, window, _| {
                let nova = limites.size.width.as_f32() / super::escala(window);
                if (nova - largura_atual).abs() > 0.5 {
                    window.on_next_frame(move |_, cx| {
                        largura.update(cx, |l, cx| {
                            *l = nova;
                            cx.notify();
                        })
                    });
                }
            })
            // Uma linha só no alto: o logo (o menu do Vitral, também na gaveta) e as abas. A busca e
            // o "+" moram no pé da lista (pedido do dono, 12/11/2026).
            .child(self.topo_enxuto(motor_entidade, Some(abas), cx))
            .child(div().flex_none().h(esp::E4))
            .children(escopo)
            .child(conteudo)
            .children(rodape)
            .child(linha_busca.mt(esp::E4).mb(esp::E8))
            // Sem fundo nem aresta: a lista é a própria base da janela (duas camadas: a base com
            // a lista e a borda, e os cartões por cima). Na gaveta, a superfície é a da gaveta.
    }

    /// As abas Conversas · Arquivos, num segmentado (o ponto âmbar na de Conversas quando algo
    /// espera você e você está nos arquivos).
    fn abas_da_lista(&self, motor: &Motor, ctx: &Contexto, conversas: bool) -> Div {
        let pendentes = motor.precisa_de_voce().numero();
        let mut aba_conversas = Segmento::new("Conversas").icone(Lucide::MessagesSquare);
        if !conversas && pendentes > 0 {
            let dica = format!("{} você", contagem(pendentes, "chat esperando", "chats esperando"));
            aba_conversas = aba_conversas.ponto(Cores::espera()).dica(dica);
        }
        let aba_arquivos = Segmento::new("Arquivos").icone(Lucide::Folder);
        let janela = ctx.janela.clone();
        let abas = ControleSegmentado::new(
            "abas-da-lista",
            [aba_conversas, aba_arquivos],
            if conversas { 0 } else { 1 },
            move |i, window, cx| {
                let _ = janela.update(cx, |this, cx| {
                    this.rolagem_lista.set_offset(point(px(0.), px(0.)));
                    this.realce_lista = None;
                    let aba = if i == 0 { AbaLateral::Conversas } else { AbaLateral::Arquivos };
                    this.trocar_aba(aba, window, cx);
                });
            },
        )
        // Largura total, com as duas abas do mesmo tamanho e o rótulo no meio de cada uma.
        .solto(true)
        .largura_total(true)
        .so_icone(ctx.aperto.estreita);
        // 40 de altura, a do logo: os dois dividem a fileira do alto.
        let abas = gpui_kit::component::Sizable::with_size(abas, gpui_kit::component::Size::Size(px(40.)));
        div().flex_none().mb(esp::E8).child(abas)
    }

    /// Busca (↓ entra na lista, Esc limpa e depois volta à caixa) e o + de nova conversa.
    fn linha_da_busca(&self, montagem: &Montagem, ctx: &Contexto, window: &Window, cx: &Context<Self>) -> Div {
        let conversas = self.aba == AbaLateral::Conversas;
        let primeiro = montagem.navegaveis.first().map(|n| (n.chat.clone(), n.indice));
        let campo = div()
            .flex_1()
            .min_w_0()
            .capture_action(cx.listener(move |this, _: &MoveDown, window, cx| {
                let Some((chat, indice)) = primeiro.clone().filter(|_| this.aba == AbaLateral::Conversas) else {
                    return;
                };
                cx.stop_propagation();
                this.realce_lista = Some(chat);
                this.rolagem_lista.scroll_to_item(indice);
                window.focus(&this.foco_lista, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Escape, window, cx| this.esc_na_busca(window, cx)))
            .child(campo_da_lista(&self.busca, self.aba, ctx.aperto.estreita, window, cx));
        // O + abre um chat novo na hora, na pasta do chat aberto (como no VS Code).
        let nova = conversas.then(|| {
            BotaoRedondo::new("nova-conversa", Some(Lucide::Plus.into()), "Novo chat")
                .dica_acao(Box::new(NovaConversa), None)
                .variante(VarianteRedonda::Metal)
                .diametro(40.)
                .on_click(cx.listener(|this, _, window, cx| this.novo_chat_direto(window, cx)))
        });
        h_flex().flex_none().gap(esp::E8).mb(esp::E8).child(campo).children(nova)
    }

    /// Linha de escopo (36): nos Arquivos, de qual pasta (ou da cópia do chat aberto) e o ↻; nas
    /// Arquivadas, como sair.
    fn linha_escopo(
        &self,
        _motor: &Motor,
        arquivos: Option<&Arquivos>,
        ctx: &Contexto,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        if let Some(a) = arquivos {
            let pasta = a.pasta.clone()?;
            let recarregar = {
                let pasta = pasta.clone();
                botao_icone("escopo-recarregar", Lucide::RefreshCw, "Recarregar a lista de arquivos", TamanhoBotao::Linha, window, cx)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.reler_arquivos(&pasta);
                        cx.notify();
                    }))
            };
            let caminho = div()
                .flex_1()
                .min_w_0()
                .pl(esp::E8)
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_start()
                .metadado()
                .text_color(Cores::tenue())
                .child(com_til(&pasta));
            return Some(
                h_flex()
                    .flex_none()
                    .h(r(36.))
                    .gap(esp::E8)
                    .child(caminho)
                    .when(!ctx.aperto.estreita, |s| s.child(sem_arrastar().flex_none().child(recarregar)))
                    .into_any_element(),
            );
        }
        if self.filtro != Filtro::Arquivados {
            return None;
        }
        let sair = botao_icone("escopo-sair-arquivadas", Lucide::X, "Voltar às conversas", TamanhoBotao::Linha, window, cx)
            .on_click(cx.listener(|this, _, _, cx| {
                this.filtro = Filtro::Todos;
                cx.notify();
            }));
        let nome = "Arquivadas";
        let simbolo = div()
            .flex_none()
            .size(r(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(raio::de(24.))
            .bg(Cores::recuo())
            .text_color(Cores::tenue())
            .child(icone(Lucide::Archive, 14., window));
        Some(
            h_flex()
                .flex_none()
                .h(r(36.))
                .px(esp::E8)
                .gap(esp::E8)
                .child(simbolo)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(r(14.))
                        .line_height(r(20.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(Cores::forte())
                        .child(nome),
                )
                .child(sem_arrastar().flex().flex_none().items_center().gap(esp::E2).child(sair))
                .into_any_element(),
        )
    }

    /// Rodapé (36), só quando há o que mostrar: as Arquivadas (o ícone com o número, na guia) e
    /// os agentes rodando no Vitral inteiro ("2 rodando"); em Arquivos, quantos arquivos a pasta
    /// tem.
    fn rodape(&self, motor: &Motor, arquivos: Option<&Arquivos>, window: &Window, cx: &Context<Self>) -> Option<Div> {
        let rodando = motor.chats.iter().filter(|c| motor.execucao(&c.id).rodando()).count();
        let agentes = texto_agentes(rodando).map(|texto| {
            Button::new("rodape-agentes")
                .custom(variante_fantasma(cx))
                .h(altura::BARRA)
                .min_w_0()
                .px(esp::E8)
                .gap(esp::E6)
                .rounded(raio::PILULA)
                .child(div().flex_none().size(r(8.)).rounded_full().bg(Cores::destaque()))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .metadado()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(Cores::tenue())
                        .child(texto),
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.aba != AbaLateral::Conversas {
                        this.trocar_aba(AbaLateral::Conversas, window, cx);
                    }
                    this.rolagem_lista.set_offset(point(px(0.), px(0.)));
                    cx.notify();
                }))
        });
        let arquivadas = arquivos.is_none().then(|| {
            let n = motor.chats_da_lista().filter(|c| c.arquivado).count();
            let vendo = self.filtro == Filtro::Arquivados;
            (n > 0 || vendo).then(|| {
                Button::new("rodape-arquivadas")
                    .custom(variante_fantasma(cx))
                    .h(altura::BARRA)
                    .flex_none()
                    .px(esp::E8)
                    .gap(esp::E6)
                    .rounded(raio::de(28.))
                    .selected(vendo)
                    .child(div().flex_none().text_color(Cores::tenue()).child(icone(Lucide::Archive, 14., window)))
                    .when(n > 0, |b| b.child(Selo::contagem(n)))
                    .tooltip(if vendo { "Voltar às conversas" } else { "Arquivadas" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.filtro = if vendo { Filtro::Todos } else { Filtro::Arquivados };
                        this.rolagem_lista.set_offset(point(px(0.), px(0.)));
                        this.realce_lista = None;
                        cx.notify();
                    }))
            })
        });
        let total = arquivos.filter(|a| !a.lista.is_empty()).map(|a| {
            let palavra = if a.lista.len() == 1 { "arquivo" } else { "arquivos" };
            div()
                .flex_none()
                .pr(esp::E8)
                .metadado()
                .text_color(Cores::apagado())
                .child(format!("{} {palavra}", milhar(a.lista.len())))
        });
        let arquivadas = arquivadas.flatten();
        // O sino desceu do trilho para cá; os anéis de uso ficam no cabeçalho da conversa.
        let sino = self.botao_sino(super::resumo::precisa_em_tudo(motor), cx);
        Some(
            h_flex()
                .flex_none()
                .h(r(36.))
                .mx(r(-8.))
                .px(esp::PECA)
                .gap(esp::E4)
                .children(arquivadas)
                .children(agentes)
                .child(div().flex_1())
                .children(total)
                .child(sino),
        )
    }

    fn lista_conversas(&self, motor: &Motor, ctx: &Contexto, window: &Window, cx: &Context<Self>) -> Montagem {
        let busca = ctx.busca.as_str();
        let combina = |c: &Chat| casa_com_a_busca(c, busca);
        let rodando = |c: &Chat| motor.execucao(&c.id).rodando();

        let mostrar_arquivados = self.filtro == Filtro::Arquivados;
        let geral = !mostrar_arquivados;
        let mut chats: Vec<&Chat> =
            motor.chats_da_lista().filter(|c| c.arquivado == mostrar_arquivados).filter(|c| combina(c)).collect();
        chats.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
        let linhas: Vec<DaLista> = chats
            .into_iter()
            .map(|c| (c, pendencia_visivel(motor, c, self.selecionado.as_deref())))
            .collect();
        let secao_de = |(c, pendencia): &DaLista| {
            if pendencia.is_some() {
                Secao::EsperandoVoce
            } else if rodando(c) {
                Secao::Rodando
            } else if c.fixado {
                Secao::Fixadas
            } else {
                Secao::Conversas
            }
        };
        let da_secao = |secao: Secao| linhas.iter().filter(|f| secao_de(f) == secao).collect::<Vec<_>>();
        let mut m = Montagem { corrida: geral, ..Montagem::default() };
        // Só na lista corrida e sem busca: é onde todos os cartões têm uma linha, da mesma altura.
        if geral && busca.is_empty() {
            let px_de = |rems: Rems| rems.to_pixels(window.rem_size()).as_f32();
            let rolado = -self.rolagem_lista.offset().y.as_f32();
            let visivel = self.rolagem_lista.bounds().size.height.as_f32();
            m.a_vista = linhas_a_vista(rolado, visivel, px_de(r(ALTURA_DA_LINHA_DE_CHAT)), px_de(esp::E2));
        }

        // "Enquanto você esteve fora", no topo da lista.
        if geral && busca.is_empty()
            && let Some(faixa) = self.faixa_do_resumo(motor, "lista", window, cx)
        {
            m.itens.push(div().pt(esp::E4).px(esp::E2).child(faixa).into_any_element());
        }
        // Pedidos, paradas e erros primeiro; as respostas novas depois (cada grupo por data).
        let mut esperando = da_secao(Secao::EsperandoVoce);
        esperando.sort_by_key(|f| f.1 == Some(Motivo::Nova));
        // Os chats parados no freio de ciclo vêm primeiro.
        let de_ciclo = if geral && busca.is_empty() { super::resumo::esperas_de_ciclo(motor) } else { Vec::new() };
        if !esperando.is_empty() || !de_ciclo.is_empty() {
            let numero = if busca.is_empty() {
                super::resumo::precisa_em_tudo(motor).numero()
            } else {
                esperando.iter().filter(|f| f.1.is_some_and(|m| m != Motivo::Nova)).count()
            };
            let paradas: Vec<&&DaLista> = esperando.iter().filter(|f| f.1 == Some(Motivo::Parada)).collect();
            let mut cabecalho = CabecalhoDeSecao::new("Esperando você");
            if numero > 0 {
                cabecalho = cabecalho.contagem(numero).tingida(Cores::espera());
            }
            if paradas.len() >= 2 {
                let retomar: Vec<String> =
                    paradas.iter().map(|f| f.0).filter(|c| motor.precisa_retomar(&c.id)).map(|c| c.id.clone()).collect();
                let botao = botao_de_secao("retomar-tudo", "Retomar tudo", Lucide::Play, ctx.aperto.apertada, window, cx);
                cabecalho = cabecalho.acao(botao.on_click(cx.listener(
                    move |this, _, _, cx| {
                        if let Some(entidade) = &this.motor {
                            entidade.update(cx, |motor, cx| {
                                for id in &retomar {
                                    motor.retomar(id, cx);
                                }
                            });
                        }
                    },
                )));
            }
            m.cabecalho(cabecalho);
            m.itens.extend(self.linhas_de_ciclo(motor, de_ciclo, window, cx));
            for f in esperando {
                self.linha_da_lista(&mut m, motor, f, ctx, window, cx);
            }
        }

        let rodando_agora = da_secao(Secao::Rodando);
        if !rodando_agora.is_empty() {
            let cabecalho = CabecalhoDeSecao::new("Rodando agora")
                .contagem(rodando_agora.len())
                .tingida(Cores::destaque_texto());
            m.cabecalho(cabecalho);
            for f in rodando_agora {
                self.linha_da_lista(&mut m, motor, f, ctx, window, cx);
            }
        }

        let fixadas = da_secao(Secao::Fixadas);
        if !fixadas.is_empty() {
            // Na lista corrida não há título para abrir a seção de novo: as fixadas ficam à vista.
            let recolhida = !m.corrida && busca.is_empty() && self.secoes_recolhidas.contains("Fixadas");
            let mut cabecalho = CabecalhoDeSecao::new("Fixadas").contagem(fixadas.len());
            if busca.is_empty() {
                cabecalho = cabecalho.recolhivel(!recolhida, alternar_secao(ctx.janela.clone(), "Fixadas"));
            }
            m.cabecalho(cabecalho);
            if !recolhida {
                for f in fixadas {
                    self.linha_da_lista(&mut m, motor, f, ctx, window, cx);
                }
            }
        }

        // Os outros, do mais recente ao mais antigo, todos; nas Arquivadas, por faixa de data.
        let paradas = da_secao(Secao::Conversas);
        if geral {
            if !paradas.is_empty() {
                m.cabecalho(CabecalhoDeSecao::new(if busca.is_empty() { "Recentes" } else { "Conversas" }));
                for f in &paradas {
                    self.linha_da_lista(&mut m, motor, f, ctx, window, cx);
                }
            }
        }
        // As arquivadas por data. "Mais antigas" começa recolhida, menos na busca, quando é a
        // única faixa ou quando tem uma conversa aberta.
        let paradas = if geral { Vec::new() } else { paradas };
        let hoje = meia_noite_local(agora());
        let faixas: Vec<(Faixa, Vec<&DaLista>)> = Faixa::TODAS
            .into_iter()
            .map(|faixa| {
                let membros = paradas.iter().copied().filter(|f| Faixa::de(f.0.atualizado_em, hoje) == faixa);
                (faixa, membros.collect::<Vec<_>>())
            })
            .filter(|(_, membros)| !membros.is_empty())
            .collect();
        let so_uma = faixas.len() == 1;
        for (faixa, membros) in faixas {
            let antigas = faixa == Faixa::Antigas;
            let com_aberta = membros.iter().any(|f| self.aberta_em_painel(&f.0.id));
            let pode_recolher = busca.is_empty() && (!antigas || (!mostrar_arquivados && !so_uma && !com_aberta));
            let recolhida = pode_recolher && self.secoes_recolhidas.contains(faixa.nome());
            let mut cabecalho = CabecalhoDeSecao::new(faixa.nome());
            if antigas {
                cabecalho = cabecalho.contagem(membros.len());
            }
            if pode_recolher {
                cabecalho = cabecalho.recolhivel(!recolhida, alternar_secao(ctx.janela.clone(), faixa.nome()));
            }
            m.cabecalho(cabecalho);
            if !recolhida {
                for f in membros {
                    self.linha_da_lista(&mut m, motor, f, ctx, window, cx);
                }
            }
        }

        // Na busca, as arquivadas que casam vêm no fim, tênues; fora dela, ficam em Arquivadas.
        if !mostrar_arquivados {
            let arquivadas = arquivadas_que_casam(motor.chats_da_lista(), busca);
            if !arquivadas.is_empty() {
                m.cabecalho(CabecalhoDeSecao::new("Arquivadas").contagem(arquivadas.len()));
                for c in arquivadas {
                    let aberta = self.selecionado.as_deref() == Some(c.id.as_str());
                    let cartao = self.cartao_conversa(motor, c, None, ctx, window, cx);
                    let tenue = div().when(!aberta, |d| d.opacity(0.6).hover(|s| s.opacity(1.))).child(cartao);
                    m.navegavel(tenue.into_any_element(), &c.id, false);
                }
            }
        }

        if m.itens.is_empty() {
            m.itens.push(self.lista_vazia(ctx, window, cx));
        }
        m
    }

    /// O estado vazio certo para o que está sendo visto.
    fn lista_vazia(&self, ctx: &Contexto, window: &Window, cx: &Context<Self>) -> AnyElement {
        let vazio = if !ctx.busca.is_empty() {
            let digitado = self.busca.read(cx).value().trim().to_string();
            EstadoVazio::new(Lucide::Search, format!("Nada encontrado para “{digitado}”"))
                .texto("A busca olha títulos, mensagens e pastas, também das arquivadas.")
                .acao(self.botao_limpar_busca(window, cx))
        } else if self.filtro == Filtro::Arquivados {
            EstadoVazio::new(Lucide::Archive, "Nada arquivado").acao(
                botao_fantasma("vazio-voltar", None, "Voltar às conversas", TamanhoBotao::Controle, window, cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.filtro = Filtro::Todos;
                        cx.notify();
                    })),
            )
        } else {
            EstadoVazio::new(Lucide::MessagesSquare, "Nenhum chat ainda")
                .texto("Cada chat trabalha direto numa pasta, como o Claude Code.")
                .acao(
                    botao_primario("vazio-nova", None, "Novo chat", TamanhoBotao::Controle, window, cx)
                        .on_click(cx.listener(|this, _, window, cx| this.novo_chat_direto(window, cx))),
                )
        };
        div().px(esp::E8).child(vazio).into_any_element()
    }

    fn botao_limpar_busca(&self, window: &Window, cx: &Context<Self>) -> Button {
        botao_fantasma("vazio-limpar-busca", None, "Limpar busca", TamanhoBotao::Controle, window, cx)
            .on_click(cx.listener(|this, _, window, cx| this.busca.update(cx, |b, cx| b.set_value("", window, cx))))
    }

    /// O cartão de uma conversa da lista, que o teclado alcança.
    fn linha_da_lista(
        &self,
        m: &mut Montagem,
        motor: &Motor,
        (chat, pendencia): &DaLista,
        ctx: &Contexto,
        window: &Window,
        cx: &Context<Self>,
    ) {
        // Longe da área à vista, só o espaço do cartão (a posição continua valendo para o teclado
        // rolar até ele; chegando perto, o cartão volta no quadro seguinte).
        // O espaço tem as medidas do cartão (`envolver` e `CartaoDeLista`), nas mesmas unidades:
        // assim o arredondamento do layout é o mesmo e a lista não pula quando o cartão volta.
        if let Some(faixa) = &m.a_vista
            && !faixa.contains(&m.itens.len())
        {
            let espaco = div().flex_none().mt(esp::E2).h(r(ALTURA_DA_LINHA_DE_CHAT)).into_any_element();
            m.navegavel(espaco, &chat.id, pendencia.is_some());
            return;
        }
        let cartao = self.cartao_conversa(motor, chat, *pendencia, ctx, window, cx);
        m.navegavel(cartao, &chat.id, pendencia.is_some());
    }

    /// Arrastar para um painel, clique do meio e clique direito. Todo cartão leva 2 em cima.
    fn envolver(&self, cartao: CartaoDeLista, chat: &Chat, simbolo: Simbolo, ctx: &Contexto, cx: &Context<Self>) -> AnyElement {
        let arrasto = ArrastoChat { chat: chat.id.clone(), titulo: chat.titulo.clone().into(), de_painel: None };
        let id_meio = chat.id.clone();
        let (janela, entidade, id_menu) = (ctx.janela.clone(), ctx.motor.clone(), chat.id.clone());
        div()
            .id(SharedString::from(format!("linha-{}", chat.id)))
            .mt(esp::E2)
            .child(cartao)
            .on_drag(arrasto, move |a, _, _, cx| {
                let simbolo = simbolo.copia().unwrap_or_else(|| Simbolo::icone(Lucide::MessagesSquare));
                cx.new(|_| PreviaArrasto { titulo: a.titulo.clone(), simbolo })
            })
            .on_aux_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                if ev.is_middle_click() {
                    this.abrir_ao_lado(id_meio.clone(), None, window, cx);
                }
            }))
            .context_menu(move |menu, window, cx| menus::menu_conversa(menu, janela.clone(), &entidade, &id_menu, true, window, cx))
            .into_any_element()
    }

    fn botao_ao_lado(&self, chat: &str, window: &Window, cx: &Context<Self>) -> AnyElement {
        let id = chat.to_string();
        botao_icone(
            SharedString::from(format!("ao-lado-{chat}")),
            Lucide::Columns2,
            "Abrir ao lado\nOu Ctrl+clique, ou arraste para um painel",
            TamanhoBotao::Linha,
            window,
            cx,
        )
        .on_click(cx.listener(move |this, _, window, cx| this.abrir_ao_lado(id.clone(), None, window, cx)))
        .into_any_element()
    }

    fn botao_menu(&self, chat: &str, ctx: &Contexto, window: &Window, cx: &Context<Self>) -> AnyElement {
        let (janela, entidade, id) = (ctx.janela.clone(), ctx.motor.clone(), chat.to_string());
        botao_icone(
            SharedString::from(format!("mais-{chat}")),
            Lucide::Ellipsis,
            "Mais opções da conversa\nTambém no clique direito",
            TamanhoBotao::Linha,
            window,
            cx,
        )
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
            menus::menu_conversa(menu, janela.clone(), &entidade, &id, true, window, cx)
        })
        .into_any_element()
    }

    /// A ação 1 do hover: Retomar a parada, ou abrir ao lado.
    fn acao_do_cartao(&self, chat: &Chat, pendencia: Option<Motivo>, window: &Window, cx: &Context<Self>) -> AnyElement {
        if pendencia != Some(Motivo::Parada) {
            return self.botao_ao_lado(&chat.id, window, cx);
        }
        let id = chat.id.clone();
        botao_icone(SharedString::from(format!("acao-{}", chat.id)), Lucide::Play, "Retomar o que parou no meio", TamanhoBotao::Linha, window, cx)
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(entidade) = &this.motor {
                    entidade.update(cx, |motor, cx| motor.retomar(&id, cx));
                }
            }))
            .into_any_element()
    }

    /// Cartão de conversa: uma linha (o balão com o sinal da conversa, o título e a meta: número do
    /// painel, não lido e horário). Na busca que achou no texto, a segunda linha mostra onde.
    fn cartao_conversa(
        &self,
        motor: &Motor,
        chat: &Chat,
        pendencia: Option<Motivo>,
        ctx: &Contexto,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let selecionado = self.selecionado.as_deref() == Some(chat.id.as_str());
        let em_painel = self.painel_da_conversa(&chat.id);
        let ao_lado = !selecionado && self.aberta_em_painel(&chat.id);
        let nao_lido = !selecionado && motor.nao_lido(chat);
        let realce = ctx.teclado && self.realce_lista.as_deref() == Some(chat.id.as_str());
        // Na busca, quando o termo não está no título, a prévia mostra onde ele casou.
        let achado = if !ctx.busca.is_empty() && trecho(&chat.titulo, &ctx.busca).is_none() {
            achado_na_conversa(chat, &ctx.busca)
        } else {
            None
        };
        // O cartão é uma linha só (o balão, o título e a hora); a de baixo volta na busca que achou
        // no texto, para mostrar onde.
        let enxuta = achado.is_none();
        // A pasta vem antes do trecho achado ("loja · …", "Sem pasta · …").
        let subtitulo = achado.map(|(achado, faixa)| {
            let pasta = if motor.sem_pasta(chat) { "Sem pasta".to_string() } else { nome_da_pasta(&chat.cwd) };
            let onde = div().min_w_0().truncate().child(StyledText::new(achado).with_highlights([(faixa, realce_da_busca())]));
            h_flex()
                .gap(esp::E4)
                .min_w_0()
                .child(div().flex_none().max_w(r(96.)).truncate().font_weight(FontWeight::MEDIUM).child(pasta))
                .child(div().flex_none().child("·"))
                .child(div().flex_1().min_w_0().child(onde))
        });
        let quando = match motor.execucao(&chat.id) {
            Execucao::Rodando { desde } => hora_do_cartao()
                .text_color(if selecionado { Cores::texto() } else { Cores::destaque_texto() })
                .child(duracao(desde)),
            _ => hora_do_cartao()
                .text_color(if selecionado { Cores::tenue() } else { Cores::apagado() })
                .child(tempo_relativo(chat.atualizado_em)),
        };
        let sinal = sinal_do_chat(motor, chat);
        // A resposta nova já está no ponto do avatar: aqui o azul só entra quando lá vale outro.
        let indicadores = linha_meta(16.)
            .children(em_painel.map(|(n, ativo)| numero_painel(n, ativo)))
            .when(nao_lido && sinal != Some(Sinal::Nova), |d| d.child(Sinal::Nova.ponto(8.)))
            // Sem o título "Fixadas", é o alfinete que diz por que ela está em cima.
            .when(enxuta && chat.fixado, |d| d.child(div().text_color(Cores::apagado()).child(icone(Lucide::Pin, 11., window))));
        // O balão é sempre o pequeno, também no resultado da busca (que tem duas linhas): com uns de
        // 18 e outros de 30 a coluna dos ícones ficava torta. A pasta vai na dica.
        let onde = SharedString::from(if motor.sem_pasta(chat) { "Sem pasta".to_string() } else { com_til(&chat.cwd) });
        let avatar = div()
            .id(SharedString::from(format!("avatar-{}", chat.id)))
            // O ícone do chat é o estado dele (anel girando, disco âmbar, azul ou vermelho, balão
            // apagado quando parado e lido).
            .child(super::componentes::icone_de_estado(sinal, 20.))
            .tooltip(move |window, cx| Tooltip::new(onde.clone()).build(window, cx));
        let id = chat.id.clone();
        let tem_pendencia = pendencia.is_some();
        let titulo_do_cartao: SharedString = chat.titulo.clone().into();
        let mut cartao = CartaoDeLista::new(SharedString::from(format!("cartao-{}", chat.id)), titulo_do_cartao)
            .simbolo(Simbolo::Elemento(avatar.into_any_element()))
            // O ícone já é o estado: sem o ponto no canto.
            .sinal(sinal.filter(|_| !enxuta))
            .map(|c| match subtitulo {
                None => c.uma_linha().meta(h_flex().gap(esp::E4).child(indicadores).child(quando)),
                Some(subtitulo) => c.subtitulo(subtitulo).meta(quando).meta_inferior(indicadores),
            })
            .acao(self.acao_do_cartao(chat, pendencia, window, cx))
            .acao(self.botao_menu(&chat.id, ctx, window, cx))
            .selecionado(selecionado)
            .aberto_em_outro(ao_lado)
            .nao_lido(nao_lido)
            .realce(realce)
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                if ev.click_count() >= 2 {
                    this.dialogo_renomear(id.clone(), window, cx);
                } else if ev.modifiers().control {
                    this.abrir_ao_lado(id.clone(), None, window, cx);
                } else {
                    this.selecionar(Some(id.clone()), window, cx);
                    if tem_pendencia {
                        this.focar_composer(window, cx);
                    }
                }
            }));
        if let Some(titulo) = titulo_rico(&chat.titulo, &ctx.busca, nao_lido) {
            cartao = cartao.titulo_rico(titulo);
        }
        self.envolver(cartao, chat, Simbolo::icone(Lucide::MessageCircle), ctx, cx)
    }

    /// Põe o cursor na caixa de mensagem do painel ativo.
    fn focar_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(composer) = self.composer_ativo() {
            let foco = composer.read(cx).focus_handle(cx);
            window.focus(&foco, cx);
        }
    }

    /// A busca das conversas, com a lista à vista (em gaveta, se não couber ao lado). Também é
    /// o Esc da lista.
    pub(super) fn focar_busca(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.orcamento.lista && self.gaveta != Some(Gaveta::Lista) {
            self.mostrar_lista(window, cx);
        }
        if self.aba != AbaLateral::Conversas {
            self.trocar_aba(AbaLateral::Conversas, window, cx);
        }
        let foco = self.busca.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
    }

    /// Esc na busca: limpa; vazia, fecha a gaveta ou volta à caixa do painel ativo.
    fn esc_na_busca(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.busca.read(cx).value().is_empty() {
            self.busca.update(cx, |b, cx| b.set_value("", window, cx));
        } else if self.gaveta == Some(Gaveta::Lista) {
            self.fechar_gaveta(window, cx);
        } else {
            self.focar_composer(window, cx);
        }
        cx.notify();
    }

    /// Teclado da lista: ↑/↓, Home/End movem o realce; Enter abre, Ctrl+Enter abre ao lado, F2
    /// renomeia e Esc volta à busca.
    fn tecla_na_lista(&mut self, nav: &[Navegavel], ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let tecla = ev.keystroke.key.as_str();
        if tecla == "escape" {
            cx.stop_propagation();
            self.focar_busca(window, cx);
            return;
        }
        let atual = self.realce_lista.as_ref().and_then(|id| nav.iter().position(|n| n.chat == *id));
        if let Some(ix) = posicao_do_realce(nav.len(), atual, tecla) {
            self.realce_lista = Some(nav[ix].chat.clone());
            self.rolagem_lista.scroll_to_item(nav[ix].indice);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let Some(n) = atual.map(|ix| &nav[ix]) else {
            return;
        };
        match tecla {
            "enter" if ev.keystroke.modifiers.control => self.abrir_ao_lado(n.chat.clone(), None, window, cx),
            "enter" => {
                self.selecionar(Some(n.chat.clone()), window, cx);
                if n.pendencia {
                    self.focar_composer(window, cx);
                }
            }
            "f2" => self.dialogo_renomear(n.chat.clone(), window, cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn abrir_arquivo(&mut self, pasta: PathBuf, caminho: String, window: &mut Window, cx: &mut Context<Self>) {
        // A marca de linha é do link que a pediu: outro arquivo (ou o mesmo, pela árvore) abre sem ela.
        if self.arquivo_linha.get().is_none() {
            self.arquivo_marcada.set(None);
        }
        self.arquivo_aberto = Some((pasta, caminho));
        self.aba_direita = super::abas_direita::AbaDireita::Arquivo(0);
        self.mostrar_direita(window, cx);
    }

    /// A pasta da aba Arquivos (a do chat aberto), os arquivos dela e a árvore, ou os que casam com
    /// a busca.
    fn preparar_arquivos(
        &mut self,
        motor_entidade: &Entity<Motor>,
        busca: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Arquivos {
        let rolagem = window
            .use_keyed_state("rolagem-dos-arquivos", cx, |_, _| UniformListScrollHandle::new())
            .read(cx)
            .clone();
        let motor = motor_entidade.read(cx);
        let pasta = self.selecionado.as_ref().and_then(|id| motor.chat(id)).map(|c| c.cwd.clone()).filter(|p| p.is_dir());
        let lista = match &pasta {
            Some(p) => self.arquivos_da_pasta(p, cx),
            None => Rc::new(Vec::new()),
        };
        let termos: Vec<&str> = busca.split_whitespace().collect();
        let achados = (!termos.is_empty()).then(|| {
            let mut achados: Vec<String> = lista
                .iter()
                .filter(|a| {
                    let a = a.to_lowercase();
                    termos.iter().all(|t| a.contains(t))
                })
                .cloned()
                .collect();
            achados.sort_by_key(|a| a.len());
            Rc::new(achados)
        });
        let arvore = match (&pasta, &achados) {
            (Some(p), None) if !lista.is_empty() => Some(self.arvore(p, &lista, window, cx)),
            _ => None,
        };
        Arquivos { pasta, lista, achados, arvore, rolagem }
    }

    /// A árvore da pasta, refeita quando a lista de arquivos é relida (as pastas abertas ficam).
    fn arvore(
        &mut self,
        pasta: &Path,
        lista: &Rc<Vec<String>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TreeState> {
        let chave = pasta.display().to_string();
        let fonte = window.use_keyed_state(
            SharedString::from(format!("fonte-da-arvore-{chave}")),
            cx,
            |_, _| None::<Rc<Vec<String>>>,
        );
        let em_dia = fonte.read(cx).as_ref().is_some_and(|f| Rc::ptr_eq(f, lista));
        if em_dia && let Some(arvore) = self.arvores.get(&chave) {
            return arvore.clone();
        }
        let abertas = self.arvores.get(&chave).map(|a| pastas_abertas(a.read(cx))).unwrap_or_default();
        let aberto = self.arquivo_aberto.as_ref().filter(|(p, _)| p == pasta).map(|(_, c)| c.clone());
        let itens = itens_da_arvore(montar_arvore(lista), &abertas, aberto.as_deref());
        let arvore = match self.arvores.get(&chave) {
            Some(a) => {
                a.update(cx, |a, cx| a.set_items(itens, cx));
                a.clone()
            }
            None => {
                let a = cx.new(|cx| TreeState::new(cx).items(itens));
                self.arvores.insert(chave, a.clone());
                a
            }
        };
        fonte.update(cx, |f, _| *f = Some(lista.clone()));
        arvore
    }

    /// Aba Arquivos: a árvore da pasta escolhida ou os resultados da busca.
    fn conteudo_arquivos(&self, a: &Arquivos, ctx: &Contexto, window: &Window, cx: &Context<Self>) -> AnyElement {
        let Some(pasta) = a.pasta.clone() else {
            let texto = if self.selecionado.is_some() {
                "A pasta deste chat não foi encontrada no disco."
            } else {
                "Abra um chat para ver os arquivos da pasta dele."
            };
            let vazio = EstadoVazio::new(Lucide::FolderX, "Nenhuma pasta à mão").texto(texto);
            return div().flex_1().px(esp::E8).child(vazio).into_any_element();
        };
        if a.lista.is_empty() {
            let destino = pasta.clone();
            let abrir = botao_fantasma(
                "vazio-abrir-pasta",
                Some(Lucide::ExternalLink.into()),
                "Abrir no gerenciador de arquivos",
                TamanhoBotao::Controle,
                window,
                cx,
            )
            .on_click(move |_, _, cx| crate::sistema::abrir_pasta(&destino, cx));
            let vazio = EstadoVazio::new(Lucide::FolderOpen, "Nenhum arquivo nesta pasta")
                .texto(pasta.display().to_string())
                .acao(abrir);
            return div().flex_1().px(esp::E8).child(vazio).into_any_element();
        }
        let aberto = self.arquivo_aberto.clone().filter(|(p, _)| *p == pasta).map(|(_, c)| c);
        if let Some(achados) = &a.achados {
            if achados.is_empty() {
                let digitado = self.busca.read(cx).value().trim().to_string();
                let vazio = EstadoVazio::new(Lucide::Search, format!("Nenhum arquivo com “{digitado}”"))
                    .acao(self.botao_limpar_busca(window, cx));
                return div().flex_1().px(esp::E8).child(vazio).into_any_element();
            }
            let termo = ctx.busca.split_whitespace().next().unwrap_or_default().to_string();
            let (achados, janela) = (achados.clone(), ctx.janela.clone());
            let n = achados.len();
            let lista = uniform_list("arquivos-achados", n, move |faixa, window, cx| {
                faixa
                    .map(|ix| linha_achada(ix, &achados[ix], &termo, &pasta, aberto.as_deref(), &janela, window, cx))
                    .collect::<Vec<_>>()
            })
            .track_scroll(&a.rolagem)
            .size_full();
            return v_flex()
                .flex_1()
                .min_h_0()
                .child(CabecalhoDeSecao::new(contagem(n, "arquivo", "arquivos")).primeira(true))
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .mr(r(-8.))
                        .pr(esp::PECA)
                        .child(lista)
                        .child(barra_de_rolagem("barra-achados", &a.rolagem)),
                )
                .into_any_element();
        }
        let Some(arvore) = &a.arvore else {
            return div().flex_1().into_any_element();
        };
        let (janela, janela_menu) = (ctx.janela.clone(), ctx.janela.clone());
        let pasta_menu = pasta.clone();
        let arvore = tree(arvore, move |ix, entrada, _, window, cx| {
            linha_da_arvore(ix, entrada, &pasta, aberto.as_deref(), &janela, window, cx)
        })
        .context_menu(move |_, entrada, menu, _, _| {
            menu_do_arquivo(menu, janela_menu.clone(), pasta_menu.clone(), entrada.item().id.to_string(), entrada.is_folder())
        });
        div().flex_1().min_h_0().pb(esp::E8).child(arvore).into_any_element()
    }
}

/// O aviso de quem acabou de arquivar, com Desfazer: devolve as `ids` que ainda estão
/// arquivadas (a que você já desarquivou fica como está).
pub(super) fn avisar_arquivadas(motor: Entity<Motor>, ids: Vec<String>, texto: String, window: &mut Window, cx: &mut App) {
    let nota = aviso_com_acao(Tom::Info, texto, "Desfazer", move |_, cx| {
        motor.update(cx, |m, cx| {
            let voltar: Vec<String> = ids.iter().filter(|id| m.chat(id).is_some_and(|c| c.arquivado)).cloned().collect();
            for id in &voltar {
                m.alternar_arquivado(id, cx);
            }
        })
    });
    window.push_notification(nota, cx);
}

#[cfg(test)]
mod testes {
    use super::{
        AbaLateral, Aperto, Causa, Faixa, Motivo, MotivoDaPrevia, No, PendenciaDoMotor, achado_na_conversa,
        arquivadas_que_casam, dica_da_busca, meia_noite_local, milhar, montar_arvore,
        motivo, partes_da_previa, posicao_do_realce, texto_agentes, trecho,
    };
    use crate::modelo::{Chat, Execucao, Item};

    fn conversa(id: &str, titulo: &str, falas: &[&str]) -> Chat {
        let json = format!(
            r#"{{"id": "{id}", "titulo": "{titulo}",
                "sessao": "s", "sessao_criada": false, "cwd": "/tmp/loja", "modelo": null,
                "criado_em": 0, "atualizado_em": 0, "custo_usd": 0.0}}"#
        );
        let mut c: Chat = serde_json::from_str(&json).unwrap();
        c.itens = falas.iter().map(|t| Item::Assistente { texto: t.to_string(), em: 0 }).collect();
        c
    }

    #[test]
    fn regra_do_que_precisa_de_voce() {
        let erro = Execucao::Erro("caiu".into());
        assert_eq!(motivo(&Execucao::EsperandoVoce, false, false), Some(Motivo::Pedido));
        assert_eq!(motivo(&erro, false, false), Some(Motivo::Erro));
        assert_eq!(motivo(&Execucao::Parado, true, false), Some(Motivo::Nova));
        assert_eq!(motivo(&Execucao::Parado, false, false), None);
        // O erro vence a resposta nova.
        assert_eq!(motivo(&erro, true, false), Some(Motivo::Erro));
        assert_eq!(motivo(&Execucao::Rodando { desde: 0 }, false, false), None);
        // O que parou no meio pede Retomar: vem antes do erro e da resposta nova, depois do pedido.
        assert_eq!(motivo(&Execucao::Parado, true, true), Some(Motivo::Parada));
        assert_eq!(motivo(&erro, false, true), Some(Motivo::Parada));
        assert_eq!(motivo(&Execucao::EsperandoVoce, false, true), Some(Motivo::Pedido));
    }

    #[test]
    fn faixas_de_data() {
        const DIA: u64 = 86_400;
        let hoje = 1_000 * DIA;
        // Até 7 dias contados da meia-noite de hoje: "Recentes"; antes disso, "Mais antigas".
        assert_eq!(Faixa::de(hoje + 5, hoje), Faixa::Recentes);
        assert_eq!(Faixa::de(hoje - DIA, hoje), Faixa::Recentes);
        assert_eq!(Faixa::de(hoje - 7 * DIA, hoje), Faixa::Recentes);
        assert_eq!(Faixa::de(hoje - 7 * DIA - 1, hoje), Faixa::Antigas);
        assert_eq!(Faixa::de(hoje - 30 * DIA - 1, hoje), Faixa::Antigas);
        // Conversa antiga gravada sem data não quebra a conta.
        assert_eq!(Faixa::de(0, hoje), Faixa::Antigas);
        assert_eq!(Faixa::TODAS.map(Faixa::nome), ["Recentes", "Mais antigas"]);
    }

    #[test]
    fn meia_noite_do_dia() {
        let agora = 1_793_450_000; // fim de outubro de 2026
        let inicio = meia_noite_local(agora);
        assert!(inicio <= agora && agora - inicio < 86_400 + 3_600);
        assert_eq!(meia_noite_local(inicio), inicio);
        assert_eq!(meia_noite_local(inicio + 86_399 - 3_600), inicio);
    }

    #[test]
    fn so_as_linhas_perto_da_vista_sao_desenhadas() {
        use super::linhas_a_vista;
        // Sem medida (o primeiro quadro) ou sem área, tudo é desenhado.
        assert_eq!(linhas_a_vista(0., 800., 0., 2.), None);
        assert_eq!(linhas_a_vista(0., 0., 36., 2.), None);
        // No topo: a tela (800 px, 21 linhas de 38) e mais uma tela de folga embaixo.
        assert_eq!(linhas_a_vista(0., 800., 36., 2.), Some(0..44));
        // Rolado 3.000 px: uma tela de folga para cada lado.
        assert_eq!(linhas_a_vista(3_000., 800., 36., 2.), Some(57..123));
        // Lista baixa: a folga nunca é menor que 12 linhas.
        assert_eq!(linhas_a_vista(0., 100., 36., 2.), Some(0..16));
    }

    #[test]
    fn a_pendencia_do_sino_vira_rotulo_e_detalhe() {
        let partes = |causa: Causa| {
            let (motivo, detalhe) = MotivoDaPrevia::de(&causa);
            partes_da_previa(Some(&motivo), &detalhe, false)
        };
        assert_eq!(partes(Causa::Parada("Vitral fechou".into())), (Some("Parada".into()), "· Vitral fechou".into()));
        assert_eq!(partes(Causa::Permissao("Bash · npm test".into())), (Some("Permissão".into()), "· Bash · npm test".into()));
        assert_eq!(partes(Causa::Plano), (Some("Plano para aprovar".into()), String::new()));
        assert_eq!(partes(Causa::Erro("\n# deu ruim\nresto".into())), (Some("Erro".into()), "· deu ruim".into()));
        let do_motor = PendenciaDoMotor::Parada { chat: "c1".into(), motivo: "o Vitral foi fechado no meio do turno".into() };
        let causa = Causa::da_pendencia(&do_motor).unwrap();
        let (motivo, detalhe) = MotivoDaPrevia::de(&causa);
        assert_eq!(partes_da_previa(Some(&motivo), &detalhe, false), (Some("Parada".into()), "· Vitral fechou".into()));
        // Na lista estreita, o rótulo sai quando há detalhe (o sinal já diz o quê); sem detalhe, fica.
        assert_eq!(partes_da_previa(Some(&motivo), &detalhe, true), (None, "Vitral fechou".into()));
        let (plano, detalhe) = MotivoDaPrevia::de(&Causa::Plano);
        assert_eq!(partes_da_previa(Some(&plano), &detalhe, true), (Some("Plano para aprovar".into()), String::new()));
        assert!(Causa::da_pendencia(&PendenciaDoMotor::Nova { chat: "c1".into() }).is_none());
    }

    #[test]
    fn trecho_da_busca_sem_caixa_e_com_acento() {
        assert_eq!(trecho("Cupons de desconto", "desc"), Some(10..14));
        assert_eq!(trecho("Página de contato", "página"), Some(0..7));
        assert_eq!(trecho("ÁRVORE de arquivos", "árvore"), Some(0..7));
        assert_eq!(trecho("Validar cupom", "frete"), None);
        assert_eq!(trecho("Validar cupom", ""), None);
        // O fim fica numa fronteira de letra, mesmo com acento depois.
        let texto = "ação";
        let faixa = trecho(texto, "a").unwrap();
        assert_eq!(&texto[faixa], "a");
    }

    #[test]
    fn a_busca_mostra_onde_casou_e_acha_as_arquivadas() {
        let mut cupons = conversa("c1", "Cupons", &["Pronto."]);
        cupons.itens.insert(
            0,
            Item::Usuario {
                texto: "Valide o frete grátis\nno carrinho".into(),
                anexos: Vec::new(),
                ultrathink: false,
                ultracode: false,
                em: 0,
            },
        );
        // O termo só numa mensagem: a prévia vira o trecho dela, numa linha, com o termo marcado.
        let (texto, faixa) = achado_na_conversa(&cupons, "frete").unwrap();
        assert_eq!(texto, "Você: Valide o frete grátis no carrinho");
        assert_eq!(&texto[faixa], "frete");
        assert_eq!(achado_na_conversa(&cupons, "boleto"), None);
        // Fala longa: o recorte começa numa palavra, pouco antes do termo.
        let longa = conversa("c2", "Loja", &["Conferi o layout inteiro da página de produtos e o Frete ficou certo"]);
        let (texto, faixa) = achado_na_conversa(&longa, "frete").unwrap();
        assert_eq!(texto, "…página de produtos e o Frete ficou certo");
        assert_eq!(&texto[faixa], "Frete");
        // As arquivadas que casam aparecem só com busca.
        let mut antiga = conversa("c3", "Antiga", &["calculei o frete por CEP"]);
        antiga.arquivado = true;
        let mut outra = conversa("c4", "Frete antigo", &[]);
        (outra.arquivado, outra.atualizado_em) = (true, 10);
        // A busca também olha o nome da pasta do chat.
        let mut na_pasta = conversa("c5", "Ajustes", &[]);
        (na_pasta.arquivado, na_pasta.cwd) = (true, "/p/frete-correios".into());
        let chats = vec![cupons, antiga, outra, na_pasta];
        let ids = |busca: &str| -> Vec<String> {
            arquivadas_que_casam(chats.iter(), busca).into_iter().map(|c| c.id.clone()).collect()
        };
        assert_eq!(ids("frete"), ["c4", "c3", "c5"]);
        assert!(ids("").is_empty());
        // O caminho acima da pasta não conta.
        assert!(ids("/p/").is_empty());
    }

    fn achatar(nos: &[No]) -> Vec<(String, bool)> {
        nos.iter()
            .flat_map(|n| std::iter::once((n.caminho.clone(), n.pasta)).chain(achatar(&n.filhos)))
            .collect()
    }

    #[test]
    fn arvore_com_pastas_antes_dos_arquivos() {
        let caminhos: Vec<String> =
            ["README.md", "src/ui/lateral.rs", "src/main.rs", "Cargo.toml", "assets/logo.svg", "src/ui/mod.rs"]
                .map(String::from)
                .to_vec();
        let arvore = montar_arvore(&caminhos);
        let ordem: Vec<(String, bool)> = achatar(&arvore);
        let esperado = [
            ("assets", true),
            ("assets/logo.svg", false),
            ("src", true),
            ("src/ui", true),
            ("src/ui/lateral.rs", false),
            ("src/ui/mod.rs", false),
            ("src/main.rs", false),
            ("Cargo.toml", false),
            ("README.md", false),
        ]
        .map(|(c, p)| (c.to_string(), p));
        assert_eq!(ordem, esperado.to_vec());
        assert_eq!(arvore[1].filhos[0].nome, "ui");
    }

    #[test]
    fn realce_anda_pelas_teclas() {
        assert_eq!(posicao_do_realce(0, None, "down"), None);
        assert_eq!(posicao_do_realce(3, None, "down"), Some(0));
        assert_eq!(posicao_do_realce(3, Some(0), "down"), Some(1));
        assert_eq!(posicao_do_realce(3, Some(2), "down"), Some(2));
        assert_eq!(posicao_do_realce(3, Some(1), "up"), Some(0));
        assert_eq!(posicao_do_realce(3, Some(0), "up"), Some(0));
        assert_eq!(posicao_do_realce(3, Some(1), "home"), Some(0));
        assert_eq!(posicao_do_realce(3, Some(1), "end"), Some(2));
        assert_eq!(posicao_do_realce(3, Some(1), "enter"), None);
    }

    #[test]
    fn texto_do_rodape() {
        // Sem nada rodando, a linha some.
        assert_eq!(texto_agentes(0), None);
        assert_eq!(texto_agentes(1).as_deref(), Some("1 rodando"));
        assert_eq!(texto_agentes(2).as_deref(), Some("2 rodando"));
        assert_eq!(milhar(7), "7");
        assert_eq!(milhar(1234), "1.234");
        assert_eq!(milhar(1_234_567), "1.234.567");
    }

    #[test]
    fn a_lista_estreita_abre_mao_do_que_e_menos_importante_primeiro() {
        // Na largura normal (312) cabe tudo.
        for largura in [288., 312., 560.] {
            let a = Aperto::de(largura);
            assert!(!a.sem_rotulos && !a.estreita && !a.apertada);
        }
        // Encolhendo: o Abrir ao lado do hover sai, depois os rótulos das abas e da prévia e, por
        // último, os das ações das seções.
        let ordem = [(287., (true, false, false)), (275., (true, true, false)), (259., (true, true, true))];
        for (largura, esperado) in ordem {
            let a = Aperto::de(largura);
            assert_eq!((a.sem_rotulos, a.estreita, a.apertada), esperado, "{largura}");
        }
        // A conta é em px a 100%: a lista de 468 px a 150% é a mesma de 312 a 100%, e a de 300
        // px a 150% (a do z01, com a Equipe ao lado) é a de 200, apertada.
        assert_eq!(Aperto::de(468. / 1.5), Aperto::de(312.));
        assert_eq!(Aperto::de(300. / 1.5), Aperto { sem_rotulos: true, estreita: true, apertada: true });
    }

    #[test]
    fn a_busca_diz_so_buscar() {
        assert_eq!(dica_da_busca(AbaLateral::Conversas, false), "Buscar");
        assert_eq!(dica_da_busca(AbaLateral::Conversas, true), "Buscar");
        assert_eq!(dica_da_busca(AbaLateral::Arquivos, false), "Buscar arquivos");
        assert_eq!(dica_da_busca(AbaLateral::Arquivos, true), "Buscar");
    }
}
