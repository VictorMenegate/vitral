//! A caixa de mensagem, com as opções da conversa como na extensão do Claude Code: anexos,
//! modo de permissão, modelo, esforço, conta, uso do contexto e o menu de comandos de barra.
//! Os chips da barra, com os menus e os popovers deles, ficam em `composer_seletores.rs`.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use gpui_kit::component::{
    ElementExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{InlineToken, InputContent, Textarea, TextareaState},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Janela;
use super::componentes::{
    BotaoRedondo, FaixaCaixa, Flutuante as _, TamanhoBotao, VarianteRedonda, botao_fantasma, cor_da_faixa,
    popover, variante_fantasma,
};
use super::movimento::{self, Curva, com_estado};
use crate::{
    modelo::Chat,
    motor::{self, Motor},
    tema::{Cores, Lucide, Tipo as _, esp, gradiente_quente, r, raio, tam},
};

#[path = "composer_seletores.rs"]
mod seletores;

use seletores::Barra;

/// (valor, nome, descrição) dos modos de permissão do Claude Code.
pub const MODOS: [(&str, &str, &str); 5] = [
    (
        "manual",
        "Manual",
        "Pede aprovação antes de cada edição ou comando",
    ),
    (
        "acceptEdits",
        "Aceitar edições",
        "Edita arquivos sem perguntar; comandos ainda pedem",
    ),
    (
        "plan",
        "Planejar",
        "Explora o código e apresenta um plano antes de mexer",
    ),
    (
        "auto",
        "Automático",
        "Um classificador decide o que precisa de aprovação",
    ),
    (
        "bypassPermissions",
        "Sem perguntar",
        "Faz tudo sem pedir permissão",
    ),
];

pub const MODELOS: [(&str, &str); 5] = [
    ("", "Padrão"),
    ("fable", "Fable"),
    ("opus", "Opus"),
    ("sonnet", "Sonnet"),
    ("haiku", "Haiku"),
];

/// Uma linha sobre cada família, na ordem de `MODELOS` (sem o Padrão): no seletor da caixa e nos
/// menus de modelo das Configurações.
pub const DESCRICOES_DE_MODELO: [&str; 4] = [
    "Para os desafios mais difíceis",
    "Para trabalho complexo e o dia a dia",
    "O mais eficiente para tarefas simples",
    "O mais rápido para respostas curtas",
];

/// As versões exatas que o seletor oferece embaixo das famílias, na ordem de `MODELOS` e, dentro
/// de cada família, da mais nova para a mais antiga. A família manda `--model opus` e quem decide
/// qual Opus é o Claude Code; estas fixam a versão.
///
/// É a lista do próprio seletor do Claude Code (as seções "main" e "overflow" da configuração
/// assinada que a 2.1.318 traz embutida), menos o Opus 4 e o Opus 4.1: esses dois ele troca
/// calado pelo Opus da família (o remapeamento que o `CLAUDE_CODE_DISABLE_LEGACY_MODEL_REMAP`
/// desliga), então pedir o 4.1 traria o 5.5 — melhor não oferecer que oferecer mentindo. As
/// outras foram conferidas uma por uma: o `init` do turno devolve o mesmo id que foi pedido.
///
/// O id completo, e não o apelido curto (`opus-5-5`), porque o Claude Code devolve no `init` o
/// que recebeu, sem normalizar: só com o id completo o Vitral sabe o nome legível
/// (`nome_curto_modelo`), a janela (`motor::janela_padrao`) e se há esforço Extra
/// (`motor::aceita_ultracode`).
///
/// Sem as variantes `[1m]`: da geração 5 em diante a janela de 1M já é a do modelo (pedir
/// `claude-opus-5[1m]` chega ao `init` como `claude-opus-5`), então seriam linhas repetidas.
///
/// Esta lista é só a RESERVA: assim que o Claude Code instalado responde quais modelos oferece
/// (`claude::atualizar_modelos_oferecidos`, ao abrir o app), quem vale é a resposta dele
/// (`versoes_de_modelo`). Foi por depender só desta lista que o Fable 5.2 ficou de fora do seletor
/// até alguém lembrar de acrescentá-lo aqui.
pub const VERSOES_DE_MODELO: [&str; 13] = [
    "claude-fable-5-2",
    "claude-fable-5-1",
    "claude-fable-5",
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-opus-4-8",
    "claude-opus-4-7",
    "claude-opus-4-6",
    "claude-sonnet-5-5",
    "claude-sonnet-5",
    "claude-sonnet-4-6",
    "claude-haiku-5-5",
    "claude-haiku-4-5",
];

/// As versões exatas que o seletor oferece agora: as que o Claude Code instalado disse que tem
/// (cada modelo que ele lista e o que cada apelido de família resolve), por família na ordem de
/// `MODELOS` e, dentro dela, da mais nova para a mais antiga; sem resposta dele, a lista fixa.
pub fn versoes_de_modelo() -> Vec<String> {
    versoes_dos_oferecidos(&crate::claude::modelos_oferecidos())
}

fn versoes_dos_oferecidos(oferecidos: &[crate::claude::ModeloOferecido]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for modelo in oferecidos {
        for id in [&modelo.valor, &modelo.resolvido] {
            if id.starts_with("claude-") && !ids.contains(id) {
                ids.push(id.clone());
            }
        }
    }
    if ids.is_empty() {
        return VERSOES_DE_MODELO.iter().map(|v| v.to_string()).collect();
    }
    // A mesma versão com e sem a data no fim (o `claude-haiku-4-5-20251001` e o `claude-haiku-4-5`)
    // é uma linha só: fica a que o Claude Code listou primeiro. A chave leva o nome da família, e
    // não o lugar dela: duas famílias que o Vitral ainda não conhece caem no mesmo lugar (o fim),
    // e uma esconderia a outra.
    let mut vistas: Vec<(&str, Vec<u32>)> = Vec::new();
    let mut unicas: Vec<String> = Vec::new();
    for id in &ids {
        let chave = (familia_da_versao(id), ordem_da_versao(id).1);
        if !vistas.contains(&chave) {
            vistas.push(chave);
            unicas.push(id.clone());
        }
    }
    unicas.sort_by(|a, b| {
        let ((fa, va), (fb, vb)) = (ordem_da_versao(a), ordem_da_versao(b));
        fa.cmp(&fb).then_with(|| familia_da_versao(a).cmp(familia_da_versao(b))).then(vb.cmp(&va))
    });
    unicas
}

/// A família de uma versão: `opus` no `claude-opus-5-5`.
fn familia_da_versao(id: &str) -> &str {
    let resto = sem_sufixo_de_janela(id).strip_prefix("claude-").unwrap_or(id);
    resto.split('-').next().unwrap_or_default()
}

/// A posição da família em `MODELOS` (as desconhecidas, no fim) e os números da versão, sem a
/// data: `claude-opus-5-5` é (2, [5, 5]).
fn ordem_da_versao(id: &str) -> (usize, Vec<u32>) {
    let resto = sem_sufixo_de_janela(id).strip_prefix("claude-").unwrap_or(id);
    let familia = familia_da_versao(id);
    let lugar = MODELOS.iter().position(|(m, _)| *m == familia).unwrap_or(MODELOS.len());
    let versao = resto.split('-').skip(1).filter(|p| p.len() < 6).filter_map(|p| p.parse().ok()).collect();
    (lugar, versao)
}

/// O nome da versão que um apelido de família resolve hoje no Claude Code instalado ("Fable 5.1"
/// para o `fable`), se ele já respondeu.
pub fn versao_da_familia(apelido: &str) -> Option<String> {
    crate::claude::mais_nova_da_familia(&crate::claude::modelos_oferecidos(), apelido).map(|id| nome_curto_modelo(&id))
}

/// O valor é uma das versões exatas que o seletor oferece agora (e não um apelido de família nem
/// o nome completo que uma conversa antiga guardou). Só a lista que está valendo: uma versão da
/// lista fixa que o Claude Code instalado deixou de oferecer aparece como "escolhido antes", em
/// vez de sumir do seletor.
pub fn versao_exata(valor: &str) -> bool {
    versoes_de_modelo().iter().any(|v| v == valor)
}

/// O modelo sem o sufixo da janela, para comparar com os ids da lista: o Claude Code 2.1.278
/// abria os turnos no `claude-opus-5[1m]`, que é o `claude-opus-5` da lista.
pub fn sem_sufixo_de_janela(id: &str) -> &str {
    id.split('[').next().unwrap_or(id)
}

/// Os nomes do seletor do Claude Code ("Extra" é o xhigh); os valores gravados não mudam.
pub const ESFORCOS: [(&str, &str); 6] = [
    ("", "Auto"),
    ("low", "Baixo"),
    ("medium", "Médio"),
    ("high", "Alto"),
    ("xhigh", "Extra"),
    ("max", "Máximo"),
];

/// Comandos de barra sugeridos antes de a conversa começar (depois vêm do Claude Code).
const COMANDOS_INICIAIS: [&str; 8] = [
    "clear",
    "compact",
    "context",
    "usage",
    "init",
    "review",
    "security-review",
    "simplify",
];

// Esc na caixa de mensagem: fecha o menu "/" ou para o turno do chat.
actions!(vitral, [PararTurno]);
// Shift+Tab na caixa de mensagem: passa para o próximo modo de permissão.
actions!(vitral, [AlternarModo]);
// Ctrl+Shift+M e Ctrl+E abrem os popovers de modelo e de esforço; dentro do de esforço, Espaço
// liga o Ultrathink e U, o Ultracode.
actions!(vitral, [AbrirModelo, AbrirEsforco, AlternarUltrathink, AlternarUltracode]);
// No menu "/" da caixa: as setas andam e o Tab completa o marcado (o Enter também, pelo enviar).
actions!(vitral, [MenuAcima, MenuAbaixo, MenuEscolher]);
// Tab na caixa vazia com a sugestão da próxima mensagem à vista: ela vira o texto da caixa.
actions!(vitral, [AceitarSugestao]);

/// Popover da caixa (um por vez em toda a janela, guardado em `Janela.pop_caixa`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PopCaixa {
    Modelo,
    Esforco,
}

/// Linhas da caixa antes de ela rolar por dentro, com a janela alta.
pub const LINHAS_CAIXA: usize = 12;

/// No máximo tantas sugestões no menu da caixa.
const MAX_SUGESTOES: usize = 8;

/// Uma sugestão do menu da caixa: um comando de barra.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Sugestao {
    /// O que entra no lugar do token ("/compact", "#ninee", "@12").
    pub valor: String,
    pub titulo: String,
    pub detalhe: Option<String>,
    /// O começo que casa primeiro (o nome do comando).
    pub chave: String,
    /// Onde a busca olha, sem acento.
    pub busca: String,
    /// A ordem entre as que casam igual (o tamanho do comando).
    pub peso: usize,
}

/// O que a última palavra da caixa pede: o sinal, o que veio depois dele e onde a palavra começa.
/// `/` só no começo e sem espaço depois; `#` e `@` no começo ou depois de um espaço.
pub(super) fn gatilho(texto: &str) -> Option<(char, &str, usize)> {
    if let Some(resto) = texto.strip_prefix('/') {
        return (!resto.contains(char::is_whitespace)).then_some(('/', resto, 0));
    }
    let inicio = texto.char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8());
    let palavra = &texto[inicio..];
    let sinal = palavra.chars().next().filter(|c| matches!(c, '#' | '@'))?;
    Some((sinal, &palavra[sinal.len_utf8()..], inicio))
}

/// As sugestões que casam com o digitado: quem começa com ele primeiro, depois quem só o contém,
/// cada grupo pelo peso; no máximo 8.
pub(super) fn menu_sugestoes(itens: Vec<Sugestao>, digitado: &str) -> Vec<Sugestao> {
    let digitado = crate::dados::chave(digitado);
    let mut lista: Vec<Sugestao> = itens
        .into_iter()
        .filter(|s| digitado.is_empty() || s.chave.contains(&digitado) || s.busca.contains(&digitado))
        .collect();
    lista.sort_by_key(|s| (!s.chave.starts_with(&digitado), s.peso));
    lista.truncate(MAX_SUGESTOES);
    lista
}

/// Põe `texto` na caixa, com o cursor no fim e a caixa rolada até ele. Pelo `replace_all`, e não
/// pelo `set_value`: ele guarda o desfazer (o Ctrl+Z traz de volta o que estava escrito) e avisa a
/// mudança, que agenda a gravação do rascunho (teste de usabilidade, 16/11/2026: o `set_value`
/// apagava o desfazer e não gravava o rascunho). O `replace_all` põe a seleção em 0..0 e a
/// rolagem no começo: o cursor vai para o fim e a rolagem vai até ele (o kit limita o valor).
pub(super) fn por_texto(c: &mut TextareaState, texto: String, window: &mut Window, cx: &mut Context<TextareaState>) {
    let fim = texto.len();
    c.replace_all(texto, window, cx);
    c.set_selected_range(fim..fim, cx);
    c.set_scroll_offset(point(px(0.), px(-1.0e6)), cx);
}

/// A pílula de uma menção na caixa (o `InlineToken` do campo do kit; dono, 16/11/2026: "faz as
/// menções ficarem de uma forma melhor"): o texto dela é o que vai ao agente ("@src/x.rs#L3-9");
/// o id, o endereço que o clique abre (`vitral:mencao/…`); o rótulo, o nome curto ("x.rs:3-9").
/// Apagar com Backspace leva a menção inteira.
pub(super) fn ficha_da_mencao(texto: &str, inteiro: &str, linhas: Option<(u32, u32)>) -> InlineToken {
    InlineToken::new(super::mencao::link_da_mencao(inteiro, linhas), texto.to_string())
        .with_label(super::mencao::rotulo_da_mencao(inteiro, linhas))
}

/// O texto com as menções que existem em `base` já como pílulas, para o `set_value` (o rascunho
/// que volta para a caixa ao trocar de chat).
pub(super) fn conteudo_com_mencoes(texto: &str, base: &Path) -> InputContent {
    let mut conteudo = InputContent::new(texto.to_string());
    for (faixa, mencao) in super::mencao::mencoes_no_texto(texto) {
        let Some(inteiro) = super::links_arquivo::resolver_mencao(base, &mencao.caminho) else { continue };
        let ficha = ficha_da_mencao(&texto[faixa.clone()], &inteiro, mencao.linhas);
        if let Ok(com) = conteudo.clone().with_token(faixa, ficha) {
            conteudo = com;
        }
    }
    conteudo
}

/// Troca o fim do texto, desde `inicio`, por `texto` (ou pela pílula `ficha`, que tem o mesmo
/// texto) e um espaço, com o cursor no fim e a caixa rolada até ele. Pela faixa, e não pelo
/// `replace_all` do `por_texto`: as pílulas que já estavam na caixa continuam pílulas.
pub(super) fn trocar_o_fim(
    c: &mut TextareaState,
    inicio: usize,
    texto: &str,
    ficha: Option<InlineToken>,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let total = c.value().len();
    let inicio = inicio.min(total);
    let feito = ficha.is_some_and(|f| c.replace_range_with_token(inicio..total, f, window, cx).is_ok());
    if !feito {
        c.set_selected_range(inicio..total, cx);
        c.replace(texto.to_string(), window, cx);
    }
    let fim = c.value().len();
    c.set_selected_range(fim..fim, cx);
    c.insert(" ", window, cx);
    c.set_scroll_offset(point(px(0.), px(-1.0e6)), cx);
}

/// O que o Enter faz com o menu "/" aberto: com o nome exato de um comando, nada (ele vai como
/// está); fora disso, completa o item marcado, o mesmo que o menu mostra. Antes ele procurava na
/// lista crua e completava outro ("/s" virava "/usage", e "/" virava "/clear") (teste de
/// usabilidade, 16/11/2026).
pub(super) fn escolha_do_enter(comandos: &[String], prefixo: &str, tamanho_do_menu: usize, marcado: usize) -> Option<usize> {
    if tamanho_do_menu == 0 || comandos.iter().any(|c| c == prefixo) {
        return None;
    }
    Some(marcado.min(tamanho_do_menu - 1))
}

/// O grupo de hover da caixa de um painel (a moldura), que acende os controles em repouso.
fn grupo_da_caixa(painel: u64) -> SharedString {
    SharedString::from(format!("caixa-{painel}"))
}

/// O texto com a última palavra trocada pela escolhida e um espaço depois.
#[cfg(test)]
pub(super) fn completar(texto: &str, inicio: usize, valor: &str) -> String {
    let inicio = if texto.is_char_boundary(inicio) { inicio } else { texto.len() };
    format!("{}{valor} ", &texto[..inicio])
}

pub fn nome_do_modo(valor: &str) -> &'static str {
    MODOS
        .iter()
        .find(|(v, _, _)| *v == valor)
        .map(|(_, n, _)| *n)
        .unwrap_or("Sem perguntar")
}

fn descricao_do_modo(valor: &str) -> &'static str {
    MODOS
        .iter()
        .find(|(v, _, _)| *v == valor)
        .map(|(_, _, d)| *d)
        .unwrap_or(MODOS[4].2)
}

/// Posição do esforço em `ESFORCOS` (valor desconhecido conta como Auto).
fn indice_esforco(valor: &str) -> usize {
    ESFORCOS.iter().position(|(v, _)| *v == valor).unwrap_or(0)
}

/// Nomes dos níveis no segmentado: na faixa estreita, "Máximo" não cabe no segmento.
fn nomes_dos_niveis(estreita: bool) -> [&'static str; 6] {
    let mut nomes = ESFORCOS.map(|(_, n)| n);
    if estreita {
        nomes[5] = "Máx";
    }
    nomes
}

fn descricao_do_esforco(indice: usize) -> &'static str {
    match indice {
        1 => "Rápido e direto, bom para tarefas simples",
        2 => "Equilíbrio entre rapidez e cuidado",
        3 => "Implementação completa, com testes",
        4 => "Raciocínio estendido e análise minuciosa",
        5 => "O máximo de raciocínio; gasta muito do plano",
        _ => "Usa o padrão do modelo.",
    }
}

/// A chave armada para a próxima mensagem desliga ao enviar; um comando de barra não a consome.
pub fn gastar_chave(armados: &mut HashSet<String>, chat: &str, texto: &str) -> bool {
    !texto.trim_start().starts_with('/') && armados.remove(chat)
}

/// O Haiku 4 não usa esforço: o Claude Code descarta o valor sozinho.
fn usa_esforco(em_uso: Option<&str>) -> bool {
    !em_uso.is_some_and(|m| m.starts_with("claude-haiku-4"))
}

/// Nome legível do modelo em uso: "claude-opus-5-5" vira "Opus 5.5" e
/// "claude-sonnet-4-5-20250929[1m]" vira "Sonnet 4.5 (1M)". O que não for do Claude fica igual.
pub fn nome_curto_modelo(id: &str) -> String {
    let (base, extra) = match id.split_once('[') {
        Some((base, resto)) => (base, Some(resto.trim_end_matches(']'))),
        None => (id, None),
    };
    let Some(resto) = base.strip_prefix("claude-") else {
        return id.to_string();
    };
    let mut familia = None;
    let mut versao = Vec::new();
    for parte in resto.split('-') {
        if !parte.is_empty() && parte.chars().all(|c| c.is_ascii_digit()) {
            // Datas (20250929) não entram no nome.
            if parte.len() < 6 {
                versao.push(parte);
            }
        } else if familia.is_none() && !parte.is_empty() {
            familia = Some(parte);
        } else {
            return id.to_string();
        }
    }
    let Some(familia) = familia else {
        return id.to_string();
    };
    let mut letras = familia.chars();
    let mut nome: String = letras.next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    nome.push_str(letras.as_str());
    if !versao.is_empty() {
        nome = format!("{nome} {}", versao.join("."));
    }
    match extra.filter(|e| !e.is_empty()) {
        Some(extra) => format!("{nome} ({})", extra.to_uppercase()),
        None => nome,
    }
}

/// "Opus 5.5" vira "Claude Opus 5.5"; "Padrão" e nomes de fora do Claude ficam como estão.
fn com_claude(rotulo: &str) -> String {
    if MODELOS[1..].iter().any(|(_, familia)| rotulo.starts_with(familia)) {
        format!("Claude {rotulo}")
    } else {
        rotulo.to_string()
    }
}

/// Uso do contexto: a fração (para o anel) e a porcentagem do meio, de 0 a 99. Antes do
/// primeiro `result`, a janela é a padrão do modelo em uso.
pub(super) fn uso_do_contexto(usados: u64, janela: Option<u64>, modelo: Option<&str>) -> (f32, u32, u64) {
    let janela = janela.unwrap_or_else(|| motor::janela_padrao(modelo)).max(1);
    let fracao = (usados as f64 / janela as f64) as f32;
    let numero = ((fracao * 100.).round().max(0.) as u32).min(99);
    (fracao, numero, janela)
}

/// "120 mil" (ou "1 milhão", para a janela de 1M).
pub(super) fn milhares(n: u64) -> String {
    match n {
        1_000_000 => "1 milhão".into(),
        n if n > 1_000_000 && n % 1_000_000 == 0 => format!("{} milhões", n / 1_000_000),
        n => format!("{} mil", n / 1000),
    }
}

/// "200 mil tokens", "1 milhão de tokens".
pub(super) fn tokens(n: u64) -> String {
    let quantia = milhares(n);
    if quantia.ends_with("mil") { format!("{quantia} tokens") } else { format!("{quantia} de tokens") }
}

/// Onde começa o texto da caixa, da borda de fora dela: a moldura (1 de borda e 2 de folga), a
/// borda do campo (1), a margem (8; na estreita, 6 com o campo 2 para a esquerda) e os 10 do
/// campo do kit, que crescem com o zoom como o resto. Cai na coluna do ícone do primeiro chip, e
/// a faixa do turno se alinha por aqui.
pub(super) fn recuo_do_texto(estreita: bool, window: &Window) -> Pixels {
    tam(window, if estreita { 18. } else { 22. })
}

/// Da borda de fora da caixa ao conteúdo (o enviar, à direita): a moldura, a borda do campo e a
/// margem.
pub(super) fn borda_ao_conteudo(estreita: bool, window: &Window) -> Pixels {
    tam(window, if estreita { 10. } else { 12. })
}

/// A dica dos botões de compactar: o clique compacta já; quem quer foco escreve a instrução.
pub(super) const DICA_DO_COMPACTAR: &str =
    "Resume a conversa agora, sem mexer no que está escrito na caixa (com o turno rodando, entra na fila). Para compactar com foco, escreva “/compact” e a instrução na caixa.";

/// O máximo de letras de um trecho citado (uma seleção enorme não vira a mensagem).
const MAX_DA_CITACAO: usize = 4_000;

/// A mensagem com o trecho citado na frente: cada linha do trecho com "> ", uma linha em branco
/// e a resposta.
pub(super) fn com_citacao(trecho: &str, resposta: &str) -> String {
    let cortado: String = trecho.trim().chars().take(MAX_DA_CITACAO).collect();
    let citado: Vec<String> =
        cortado.lines().map(|l| if l.trim().is_empty() { ">".to_string() } else { format!("> {}", l.trim_end()) }).collect();
    format!("{}\n\n{}", citado.join("\n"), resposta.trim_start())
}

/// O que o botão redondo da direita faz agora.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Envio {
    /// Caixa vazia e chat parado: o botão só mostra que existe.
    Vazio,
    Enviar,
    /// Chat rodando com a caixa vazia: o botão vira parar.
    Parar,
    /// Chat rodando com texto: parar ao lado e enviar, que dirige o turno.
    PararEEnviar,
}

/// O Enviar manda para dentro do turno que está rodando (o steering do `Motor::dirigir_turno`) em
/// vez de pôr na fila: só com um turno de verdade em andamento, nem parado num pedido de
/// permissão, nem só esperando os agentes do fundo (aí a entrada já está fechada).
pub(super) fn dirige_o_turno(motor: &Motor, chat: &str) -> bool {
    motor.turno_recebe_mensagem(chat)
}

fn envio(rodando: bool, vazio: bool) -> Envio {
    match (rodando, vazio) {
        (false, true) => Envio::Vazio,
        (false, false) => Envio::Enviar,
        (true, true) => Envio::Parar,
        (true, false) => Envio::PararEEnviar,
    }
}

impl Janela {
    /// A caixa de mensagem do painel, em moldura dupla (a base AI Input With Loading): por fora,
    /// uma moldura preta de raio 20; dentro, com 4 de folga, o campo opaco de raio 16 com o texto
    /// (na coluna do ícone do primeiro chip) e os chips; embaixo do campo, ainda na moldura, a
    /// linha de estado em mono 11. O foco acende a moldura (180 ms; apaga em 120).
    pub(super) fn caixa_mensagem(
        &mut self,
        motor: &Motor,
        chat: &Chat,
        ix: usize,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let painel = &self.paineis[ix];
        let (painel_id, faixa, linhas) = (painel.id, painel.medida.faixa, painel.linhas_caixa);
        let composer = painel.composer.clone();
        let estreita = faixa == FaixaCaixa::Estreita;
        // A borda acende na caixa com foco (é para ela que vão Enter, Ctrl+V e @) ou com um
        // popover dela aberto.
        let focada = composer.read(cx).focus_handle(cx).contains_focused(window, cx)
            || self.pop_caixa.is_some_and(|(p, _)| p == painel_id);
        let anexos = self.anexos.get(&chat.id).cloned().unwrap_or_default();
        let janela = cx.entity().downgrade();
        let (colar, chat_colar, campo_colado) = (janela.clone(), chat.id.clone(), composer.downgrade());
        let pasta_anexos = motor.dados.pasta_anexos();
        let barra = Barra {
            painel: painel_id,
            chat: chat.id.clone(),
            faixa,
            composer: composer.clone(),
            rodando: motor.execucao(&chat.id).rodando(),
        };
        let barra = self.barra_da_caixa(motor, chat, ix, &barra, anexos.is_empty(), window, cx);
        // O mesmo canto na caixa larga e na estreita (dono, 13/11/2026: a estreita estava menos
        // redonda que o resto).
        let raio_caixa = raio::peca();
        let foco_da_caixa = composer.read(cx).focus_handle(cx);
        let caixa_vazia = composer.read(cx).value().trim().is_empty();
        let com_menu = self.menu_da_caixa(ix, chat, cx).is_some();
        // O miolo é concêntrico com a moldura de 20 e 2 de folga, não com o cartão da conversa.
        // A moldura preta tem 2 de folga (eram 4; dono, 16/11/2026: "diminui as bordas"); o miolo
        // continua concêntrico com ela.
        let raio_miolo = raio::peca() - px(2.);
        // A linha de estado: com o chat trabalhando, o que ele faz agora (M15); parada, as teclas.
        // A linha de baixo só existe com o chat trabalhando (o que ele faz agora e o Esc). Parada,
        // a caixa não tem mais a linha das teclas (dono, 13/11/2026: "tira essa parte de baixo").
        let estado = self.linha_de_estado(motor, chat, painel_id, window, cx);

        let citacao = self.citacoes.get(&chat.id).map(|trecho| self.cartao_da_citacao(&chat.id, trecho, window, cx).into_any_element());
        let tem_anexos = !anexos.is_empty();
        let pilulas = self.anexos_pendentes(&chat.id, anexos, window, cx);
        // O miolo: o campo opaco, um degrau mais claro que a moldura, com a linha de luz no topo.
        let miolo = div()
            .relative()
            .flex()
            .flex_col()
            .min_w_0()
            .rounded(raio_miolo)
            .border_1()
            .border_color(Cores::borda())
            .bg(gradiente_quente())
            .shadow(vec![BoxShadow::new(px(0.), px(1.), Cores::brilho_topo()).inset()])
            .map(|s| if estreita { s.pt(esp::E8).pb(esp::E6).px(esp::E6) } else { s.pt(esp::E12).pb(esp::E8).px(esp::E8) })
            .child(
                // Só o campo da mensagem fica em "Composer": o Shift+Tab troca o modo aqui, e não
                // na busca de um popover da caixa. Sem margem própria, o texto (com os 10 do campo
                // do kit) cai na coluna do ícone do primeiro chip.
                div()
                    .w_full()
                    // Com nada escrito, o campo também fica em "Vazia": só aí a seta para cima
                    // vale como corrigir a última mensagem, e não como mover o cursor. Com o menu
                    // "/" à vista, em "Menu": as setas andam nele e o Tab completa.
                    .key_context(if caixa_vazia {
                        "Composer Vazia"
                    } else if com_menu {
                        "Composer Menu"
                    } else {
                        "Composer"
                    })
                    .when(estreita, |s| s.ml(r(-2.)))
                    .child(
                        Textarea::new(&composer)
                            .appearance(false)
                            // As menções de arquivo são pílulas dentro do texto; o clique abre o
                            // arquivo à direita, nas linhas.
                            .token(|ficha, _, _| {
                                let id = ficha.token().id().clone();
                                match super::mencao::ler_link_da_mencao(&id) {
                                    Some((inteiro, linhas)) => super::mencao::pilula_de_mencao(
                                        SharedString::from(format!("ficha-{id}")),
                                        &inteiro,
                                        linhas,
                                        ficha.is_selected(),
                                        ficha.line_height(),
                                    )
                                    .into_any_element(),
                                    None => div().child(ficha.token().label().clone()).into_any_element(),
                                }
                            })
                            .on_token_click(|evento, window, cx| {
                                let link = evento.token().id().to_string();
                                window.dispatch_action(Box::new(super::links_arquivo::AbrirLinkVitral(link)), cx);
                            })
                            // Colar imagem ou arquivos copiados no gerenciador vira anexo.
                            .on_paste(move |item, window, cx| {
                                let caminhos = anexos_da_area_de_transferencia(item, &pasta_anexos);
                                if caminhos.is_empty() {
                                    // Texto: quem cola é o campo, que pede a rolagem até o cursor
                                    // antes do layout novo, com a caixa ainda do tamanho de antes;
                                    // com 200 linhas ela ficava parada no topo até a próxima tecla.
                                    // No quadro seguinte, já medido, o cursor vai para onde está e
                                    // a caixa rola até ele (conferência na bancada, 16/11/2026).
                                    let campo = campo_colado.clone();
                                    window.on_next_frame(move |_, cx| {
                                        let _ = campo.update(cx, |c, cx| {
                                            let cursor = c.cursor();
                                            c.set_selected_range(cursor..cursor, cx);
                                        });
                                    });
                                    return false;
                                }
                                let chat = chat_colar.clone();
                                let _ = colar.update(cx, |this, cx| {
                                    this.anexos.entry(chat).or_default().extend(caminhos);
                                    cx.notify();
                                });
                                true
                            }),
                    ),
            )
            .child(div().h(esp::E8))
            .child(barra);
        // Na altura, os 2 de cima igualam a folga de baixo da moldura.
        let linha_de_estado = estado.map(|(estado_esquerda, estado_direita)| {
            h_flex()
                .h(r(22.))
                .pt(esp::E2)
                .px(esp::E12)
                .gap(esp::E8)
                .justify_between()
                .etiqueta()
                .text_color(Cores::tenue())
                .child(h_flex().flex_1().min_w_0().gap(esp::E12).overflow_hidden().children(estado_esquerda))
                .child(h_flex().flex_none().gap(esp::E12).children(estado_direita))
        });
        // A moldura preta: o campo e a linha de estado. O vão entre as peças, a folga e a linha
        // são da caixa: o clique ali não leva o foco para a janela. O campo e os botões tratam o
        // deles antes.
        let moldura = v_flex()
            .min_w_0()
            // O hover da caixa acende os controles que recuam nos painéis de fundo.
            .group(grupo_da_caixa(painel_id))
            .rounded(raio_caixa)
            .bg(Cores::preto())
            .border_1()
            .p(esp::E2)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if !window.default_prevented() {
                    window.focus(&foco_da_caixa, cx);
                    window.prevent_default();
                }
            })
            // Os anexos moram na casca, em cima do campo (a referência do dono, 13/11/2026: a
            // "prompt box" da 21st.dev): pílulas com a miniatura, e a imagem abre na ilha.
            .children(citacao.map(|cartao| div().px(esp::E2).pt(esp::E2).pb(esp::E6).child(cartao)))
            .children(tem_anexos.then(|| div().px(esp::E4).pt(esp::E2).child(pilulas)))
            .child(miolo)
            .children(linha_de_estado);
        // O foco: `t` vai a 1 em 180 ms com a caixa focada e volta a 0 em 120. Sem halo, sem
        // cantoneira e sem azul.
        let caixa = com_estado(move |window, cx| {
            let t = movimento::valor(
                ("aro-caixa", painel_id as usize),
                if focada { 1f32 } else { 0. },
                if focada { movimento::MOVER } else { movimento::COR },
                Curva::Mover,
                window,
                cx,
            );
            // Com o foco, o aro de 2 px de metal em volta da moldura ("é aqui"), o mesmo da busca:
            // acende por `t`, e a moldura opaca por cima deixa só o aro à vista.
            div()
                .relative()
                .min_w_0()
                .child(super::componentes::metal::aro_de_metal(raio_caixa, px(2.), t))
                .child(moldura)
                .into_any_element()
        });

        div()
            .map(super::mensagens::coluna)
            .when(estreita, |s| s.px(esp::E16))
            .pb(esp::E12)
            .on_prepaint(ajustar_linhas(janela.clone(), painel_id, linhas))
            .key_context("Caixa")
            .on_action(cx.listener({
                let chat = chat.id.clone();
                move |this, _: &PararTurno, _, cx| this.esc_na_caixa(&chat, painel_id, cx)
            }))
            .on_action(cx.listener({
                let chat = chat.id.clone();
                move |this, _: &AlternarModo, window, cx| this.alternar_modo(&chat, painel_id, window, cx)
            }))
            .on_action(cx.listener(move |this, _: &MenuAcima, _, cx| this.andar_no_menu(painel_id, -1, cx)))
            .on_action(cx.listener(move |this, _: &MenuAbaixo, _, cx| this.andar_no_menu(painel_id, 1, cx)))
            .on_action(cx.listener(move |this, _: &MenuEscolher, window, cx| {
                this.escolher_no_menu(painel_id, window, cx);
            }))
            .on_action(cx.listener({
                let chat = chat.id.clone();
                move |this, _: &AceitarSugestao, window, cx| this.aceitar_sugestao(&chat, painel_id, window, cx)
            }))
            // Seta para cima na caixa vazia: o texto da última mensagem volta para ela, como num
            // terminal (o keymap só a manda para cá no contexto "Vazia").
            .on_action(cx.listener(move |this, _: &super::EditarUltima, window, cx| {
                this.editar_ultima(painel_id, window, cx)
            }))
            .child(self.menu_comandos(chat, ix, cx))
            .child(self.ilha_do_anexo(&chat.id, cx))
            .child(caixa)
    }

    /// "Modo · modelo · esforço · conta" à esquerda e, à direita, o anel do contexto, o clipe, o
    /// "…" e o enviar. O modo fica sempre à vista; os outros vêm num grupo que quebra linha com
    /// a segunda linha escondida, então o que não cabe some inteiro, a conta primeiro.
    #[allow(clippy::too_many_arguments)]
    fn barra_da_caixa(
        &self,
        motor: &Motor,
        chat: &Chat,
        ix: usize,
        b: &Barra,
        sem_anexos: bool,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let estreita = b.faixa == FaixaCaixa::Estreita;
        let altura = if estreita { 28. } else { 32. };
        let vazio = sem_anexos && b.composer.read(cx).value().trim().is_empty();
        let envios = self.paineis[ix].envios;
        // Nos painéis de fundo, a caixa recua: o modelo, o esforço, o clipe, o "⋯" e o modo (menos
        // o "Sem perguntar", que é aviso de segurança) ficam a 0,55, e acendem com o ponteiro na
        // caixa e ao ativar o painel. O campo, a moldura, o anel com a porcentagem e o parar ficam
        // sempre acesos. Com três caixas iguais e acesas no pé da janela, nada dizia qual painel
        // estava ativo além da luz azul (relatório de aparência, 16/11/2026, V2).
        let ativo = ix == self.ativo;
        let painel_id = b.painel;
        let grupo_da_caixa = grupo_da_caixa(painel_id);
        let em_repouso = move |nome: &'static str, e: AnyElement| -> AnyElement {
            let grupo = grupo_da_caixa.clone();
            com_estado(move |window, cx| {
                let (alvo, duracao) = if ativo { (1f32, movimento::MOVER) } else { (0f32, movimento::COR) };
                let luz = movimento::valor((nome, painel_id as usize), alvo, duracao, Curva::Mover, window, cx);
                let repouso = crate::tema::opacidade::EM_REPOUSO;
                div()
                    .flex_none()
                    .opacity(repouso + (1. - repouso) * luz)
                    .group_hover(grupo, |s| s.opacity(1.))
                    .child(e)
                    .into_any_element()
            })
            .into_any_element()
        };
        let grupo = |e: AnyElement| div().flex_none().ml(esp::E4).child(e);
        let sem_perguntar = motor.permissao_de(chat) == "bypassPermissions";
        let modo = self.chip_modo(motor, chat, b, window, cx);
        let modo = if sem_perguntar { modo } else { em_repouso("repouso-modo", modo) };
        div()
            .flex()
            .items_center()
            .h(r(altura))
            .child(div().flex_none().child(modo))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .h(r(altura))
                    .overflow_hidden()
                    // A linha de cima nunca fica vazia: sem este marcador, a primeira opção que
                    // não coubesse ficaria nela, cortada. Com a altura da linha, a segunda começa
                    // abaixo do recorte.
                    .child(div().h(r(altura)))
                    .child(grupo(em_repouso("repouso-modelo", self.chip_modelo(motor, chat, ix, b, window, cx))))
                    .child(grupo(em_repouso("repouso-esforco", self.chip_esforco(motor, chat, ix, b, window, cx)))),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(r(4.))
                    .ml(r(8.))
                    .children(self.anel_do_contexto(chat, b, cx))
                    .when(!estreita, |s| s.child(em_repouso("repouso-anexar", self.botao_anexar(b, window, cx).into_any_element())))
                    .when(b.faixa != FaixaCaixa::Media, |s| {
                        s.child(em_repouso("repouso-mais", self.botao_mais(motor, chat, b, window, cx).into_any_element()))
                    })
                    .children(self.botoes_de_envio(b, vazio, dirige_o_turno(motor, &chat.id), envios, cx)),
            )
    }

    /// O enviar claro (apagado com a caixa vazia) e, com o chat rodando, o parar. O botão grande
    /// guarda o mesmo id como enviar e como parar: o pulso do envio (`envios`) toca nele mesmo
    /// quando o chat já começou a rodar e ele virou o parar.
    fn botoes_de_envio(
        &self,
        b: &Barra,
        vazio: bool,
        dirige: bool,
        envios: u64,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let estreita = b.faixa == FaixaCaixa::Estreita;
        // O enviar na altura dos chips da linha (eram 36 ao lado de 32; rodada de concisão,
        // 17/11/2026: vizinhos na mesma altura).
        let (grande, pequeno) = if estreita { (28., 28.) } else { (32., 32.) };
        let painel = b.painel;
        let parar = || {
            let (motor, chat) = (self.motor.clone(), b.chat.clone());
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                if let Some(m) = &motor {
                    m.update(cx, |m, cx| m.parar(&chat, cx));
                }
            }
        };
        let dica_parar = "Parar esta conversa";
        let principal = |icone: Option<Lucide>, dica: &'static str| {
            // O enviar (e o parar) é uma tecla de metal, o par do "+" da lista.
            BotaoRedondo::new(("enviar", painel as usize), icone.map(Into::into), dica)
                .variante(VarianteRedonda::Metal)
                .diametro(grande)
                .pulso(envios)
        };
        let enviar = |dica: &'static str| {
            principal(Some(Lucide::ArrowUp), dica)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.ativar_painel(painel, cx);
                    this.enviar_do_painel(painel, window, cx);
                }))
                .into_any_element()
        };
        // Com o turno rodando, a mensagem entra nele sem pará-lo (o steering do Claude Code). O que
        // vale para o turno inteiro, e tudo o que não é um turno em andamento, vai pela fila.
        let dica_dirigir = if !dirige {
            "Enviar\nEntra na fila: este turno não recebe mensagem agora"
        } else if self.ultrathink.contains(&b.chat) || self.ultracode.contains(&b.chat) {
            "Enviar\nO Ultrathink e o Ultracode valem para o turno inteiro: esta espera a vez na fila"
        } else {
            "Enviar\nEntra no turno que está rodando, sem pará-lo\nCtrl+Enter manda depois, na fila"
        };
        match envio(b.rodando, vazio) {
            Envio::Vazio => vec![principal(Some(Lucide::ArrowUp), "Escreva uma mensagem").vazio(true).into_any_element()],
            Envio::Enviar => vec![enviar("Enviar")],
            Envio::Parar => vec![
                principal(None, dica_parar)
                    .dica_acao(Box::new(PararTurno), Some("Caixa"))
                    .on_click(parar())
                    .into_any_element(),
            ],
            Envio::PararEEnviar => vec![
                BotaoRedondo::new(("parar", painel as usize), None, dica_parar)
                    .variante(VarianteRedonda::Neutro)
                    .diametro(pequeno)
                    .dica_acao(Box::new(PararTurno), Some("Caixa"))
                    .on_click(parar())
                    .into_any_element(),
                enviar(dica_dirigir),
            ],
        }
    }

    /// Anel do uso do contexto (some antes do primeiro turno). O clique abre o uso da conversa:
    /// o contexto e o custo, o modelo, a conta e o Compactar.
    fn anel_do_contexto(&self, chat: &Chat, b: &Barra, cx: &Context<Self>) -> Option<AnyElement> {
        let usados = chat.contexto?;
        let (fracao, numero, janela) = uso_do_contexto(usados, chat.janela, chat.modelo_atual.as_deref());
        let estreita = b.faixa == FaixaCaixa::Estreita;
        // O anel com fenda, sem número dentro (lia como selo de notificação): a porcentagem vai
        // ao lado, em mono, e acompanha o arco quando ele sai do destaque (âmbar a 70%, vermelho
        // a 90%). Também na caixa estreita, mais justa: só o anel, com a fenda e o arco azul
        // parcial, ficava igual ao anel girando de "rodando", e com 3 painéis a porcentagem nunca
        // aparecia (relatório de design, 16/11/2026).
        // Um medidor deitado, e não um anel: o anel com um pedaço azul lia como o "carregando"
        // (dono, 18/11/2026: "melhora o design dessa parte de usos e contexto").
        let cor_arco = cor_da_faixa(fracao, Cores::destaque());
        let barra = div()
            .flex_none()
            .w(r(if estreita { 16. } else { 24. }))
            .h(r(4.))
            .rounded_full()
            .bg(Cores::traco())
            .child(div().h_full().w(relative(fracao.clamp(0.06, 1.))).rounded_full().bg(cor_arco));
        let cor_numero = if cor_arco == Cores::destaque() { Cores::tenue() } else { cor_arco };
        let dica: SharedString = format!(
            "Contexto: {} de {}\nO custo, o modelo e o Compactar",
            milhares(usados),
            tokens(janela)
        )
        .into();
        let lado = if estreita { 28. } else { 32. };
        let gatilho = Button::new(("contexto", b.painel as usize))
            .custom(variante_fantasma(cx))
            .h(r(lado))
            .px(if estreita { esp::E4 } else { esp::E8 })
            .rounded(raio::de(lado))
            .tooltip(dica)
            .child(
                h_flex()
                    .gap(if estreita { esp::E4 } else { esp::E6 })
                    .child(barra)
                    .child(div().etiqueta().text_color(cor_numero).child(format!("{numero}%"))),
            );
        let (motor, janela_e, chat_id, painel) = (self.motor.clone(), cx.entity().downgrade(), chat.id.clone(), b.painel);
        Some(
            popover(("pop-contexto", painel as usize))
                .anchor(Anchor::BottomRight)
                .offset(px(8.))
                .trigger(gatilho)
                .content(move |_, window, cx| {
                    let pop = cx.entity().downgrade();
                    let janela_e = janela_e.clone();
                    // A linha que vem depois dos tokens: o que o último turno gastou.
                    let extras: Vec<AnyElement> = motor
                        .as_ref()
                        .map(|m| super::onde_uso::ultimo_turno(m.read(cx), &chat_id).into_iter().collect())
                        .unwrap_or_default();
                    let compactar = botao_fantasma(
                        ("compactar", painel as usize),
                        Some(Lucide::Minimize.into()),
                        "Compactar a conversa",
                        TamanhoBotao::Barra,
                        window,
                        cx,
                    )
                    .tooltip(DICA_DO_COMPACTAR)
                    .on_click(move |_, window, cx| {
                        if let Some(pop) = pop.upgrade() {
                            pop.update(cx, |p, cx| p.dismiss(window, cx));
                        }
                        let _ = janela_e.update(cx, |this, cx| this.compactar_conversa(painel, cx));
                    });
                    let Some(m) = motor.as_ref().map(|m| m.read(cx)) else {
                        return div().into_any_element();
                    };
                    match m.chat(&chat_id) {
                        Some(c) => super::conversa::uso_da_conversa(m, c, extras, compactar.into_any_element()).into_any_element(),
                        None => div().into_any_element(),
                    }
                })
                .into_any_element(),
        )
    }

    /// Escreve na caixa do painel e põe o foco nela (quem envia é você, com Enter).
    /// Guarda o trecho selecionado como a citação da próxima mensagem do chat do painel (o cartão
    /// em cima do campo) e põe o cursor na caixa, pronto para a resposta.
    pub(super) fn responder_ao_trecho(&mut self, painel: u64, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(trecho), Some(p)) = (self.trecho.take(), self.paineis.iter().find(|p| p.id == painel)) else {
            return;
        };
        let (Some(chat), foco) = (p.chat.clone(), p.composer.read(cx).focus_handle(cx)) else {
            return;
        };
        self.citacoes.insert(chat, trecho.trim().to_string());
        gpui_kit::base::TextSelection::clear(window, cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    /// O cartão do trecho citado, na casca da caixa, em cima das pílulas dos anexos. É da família
    /// das pílulas (a mesma superfície, o mesmo fio, a mesma luz de tecla), e o azul fica só no
    /// que diz "isto é uma citação": a barra arredondada, as aspas e o rótulo. O trecho vai em
    /// até duas linhas, um degrau mais baixo que o texto que você escreve (dono, 13/11/2026:
    /// "melhora o de responder e coloca aí também" e "melhora um pouco o design").
    fn cartao_da_citacao(&self, chat: &str, trecho: &str, _: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let chat = chat.to_string();
        h_flex()
            .relative()
            .items_center()
            .gap(esp::E8)
            .pl(esp::E24)
            .pr(esp::E6)
            .py(esp::E8)
            .rounded(raio::CARTAO)
            .bg(Cores::cartao_elevado())
            .border_1()
            .border_color(Cores::borda_forte())
            .shadow(crate::tema::aresta_de_tecla())
            // A barra da citação: solta da beirada, arredondada, na altura do texto.
            .child(div().absolute().left(esp::E8).top(esp::E8).bottom(esp::E8).w(px(3.)).rounded_full().bg(Cores::destaque()))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(esp::E2)
                    .child(
                        h_flex()
                            .gap(esp::E4)
                            .child(super::mensagens::icone_de(Lucide::MessageSquareQuote, 11.).text_color(Cores::destaque_texto()))
                            .child(div().etiqueta().text_color(Cores::destaque_texto()).child("Respondendo a")),
                    )
                    .child(div().min_w_0().line_clamp(2).text_ellipsis().corpo_ui().text_color(Cores::tenue()).child(trecho.to_string())),
            )
            .child(
                BotaoRedondo::new(SharedString::from(format!("tirar-citacao-{chat}")), Some(Lucide::X.into()), "Tirar a citação")
                    .variante(VarianteRedonda::Plano)
                    .diametro(24.)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.citacoes.remove(&chat);
                        cx.notify();
                    })),
            )
    }

    /// "Compactar" (o do anel, o da faixa da conversa grande parada, o do "…" e o do cartão de
    /// erro): manda o /compact direto pelo envio normal, sem tocar na caixa; com o turno rodando,
    /// ele entra na fila. Antes ele escrevia "/compact " na caixa no lugar do rascunho, que sumia
    /// sem aviso, e ainda faltava o Enter (dono, 16/11/2026: rodada de uso, item 6.3).
    pub(super) fn compactar_conversa(&mut self, painel: u64, cx: &mut Context<Self>) {
        let (Some(motor), Some(chat)) = (self.motor.clone(), self.paineis.iter().find(|p| p.id == painel).and_then(|p| p.chat.clone()))
        else {
            return;
        };
        motor.update(cx, |m, cx| m.enviar_do_usuario(&chat, "/compact".into(), Vec::new(), crate::modelo::Reforco::default(), cx));
        if let Some(p) = self.paineis.iter_mut().find(|p| p.id == painel) {
            p.seguindo = true;
            p.rolagem.scroll_to_bottom();
        }
        cx.notify();
    }

    pub(super) fn por_na_caixa(&mut self, painel: u64, texto: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.paineis.iter().find(|p| p.id == painel).map(|p| p.composer.clone()) else {
            return;
        };
        composer.update(cx, |c, cx| por_texto(c, texto.to_string(), window, cx));
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    /// Ctrl+Shift+M e Ctrl+E: abre o popover no painel ativo (com conversa), ou fecha se é o que
    /// está aberto. O foco passa antes pela caixa, para voltar a ela quando o popover fechar.
    pub(super) fn alternar_pop(&mut self, qual: PopCaixa, window: &mut Window, cx: &mut Context<Self>) {
        let Some(painel) = self.paineis.get(self.ativo).filter(|p| p.chat.is_some()) else {
            return;
        };
        let (id, chat, composer, lista) = (
            painel.id,
            painel.chat.clone().unwrap_or_default(),
            painel.composer.clone(),
            painel.busca_modelo.clone(),
        );
        if self.pop_caixa == Some((id, qual)) {
            self.pop_caixa = None;
            cx.notify();
            return;
        }
        let Some(motor) = self.motor.clone() else {
            return;
        };
        if qual == PopCaixa::Esforco
            && !usa_esforco(motor.read(cx).chat(&chat).and_then(|c| c.modelo_atual.as_deref()))
        {
            return;
        }
        if qual == PopCaixa::Modelo {
            self.aba_do_modelo = None;
            seletores::preparar_modelos(&lista, &motor, &chat, None, window, cx);
        }
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        self.so_um_flutuante(super::QualFlutuante::Caixa);
        self.pop_caixa = Some((id, qual));
        cx.notify();
    }

    /// A ilha: a imagem anexada aberta em tamanho de ver, flutuando em cima da caixa, sem tirar
    /// a caixa do lugar. O clique na imagem abre o visor (com zoom); os dois botões do canto
    /// recolhem a ilha e tiram o anexo; o clique fora a fecha.
    fn ilha_do_anexo(&self, chat: &str, cx: &Context<Self>) -> impl IntoElement + use<> {
        let aberta = self.ilha.as_ref().filter(|(c, _)| c == chat).map(|(_, p)| p.clone());
        div().relative().when_some(aberta, |s, caminho| {
            let lido = super::imagens::anexo(&caminho);
            let (largura, altura) = super::imagens::medida_da_miniatura(lido.medidas, 300., 560.);
            let (chat_visor, alvo, fechado, tirado) = (chat.to_string(), caminho.clone(), caminho.clone(), caminho.clone());
            let chat_tirar = chat.to_string();
            let vidro = |id: &'static str, icone: Lucide| {
                div()
                    .id(id)
                    .size(r(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(gpui_kit::rgba(0x00000099))
                    .border_1()
                    .border_color(gpui_kit::rgba(0xffffff26))
                    .text_color(gpui_kit::white())
                    .cursor_pointer()
                    .hover(|s| s.bg(gpui_kit::rgba(0x000000cc)))
                    .child(super::mensagens::icone_de(icone, 14.))
            };
            // Por cima de tudo e fora do recorte do painel (`deferred` + `anchored`, como os menus):
            // dentro do cartão, que corta reto, a imagem larga saía cortada e passava por baixo do
            // painel vizinho (dono, 19/11/2026: "tá bugado a imagem aqui, tem que ficar por cima").
            // O canto de baixo dela fica no alto da caixa, e ela não sai da janela.
            s.child(deferred(anchored().anchor(Anchor::BottomLeft).snap_to_window_with_margin(px(8.)).child(
                div()
                    .id("ilha-do-anexo")
                    .mb(esp::E8)
                    .p(esp::E6)
                    .rounded(raio::PECA)
                    .bg(Cores::flutuante())
                    .border_1()
                    .border_color(Cores::borda_forte())
                    .shadow(crate::tema::sombra_flutuante())
                    .occlude()
                    .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                        this.ilha = None;
                        this.ilha_fechada = Some((fechado.clone(), std::time::Instant::now()));
                        cx.notify();
                    }))
                    .child(
                        div()
                            .id("ilha-imagem")
                            .relative()
                            .w(r(largura))
                            .h(r(altura))
                            .rounded(raio::CARTAO)
                            .bg(Cores::preto())
                            .overflow_hidden()
                            .cursor_pointer()
                            .child(img(caminho).size_full().object_fit(ObjectFit::Contain))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.ilha = None;
                                this.abrir_visor(&chat_visor, &alvo, window, cx);
                            }))
                            .child(
                                h_flex()
                                    .absolute()
                                    .top(esp::E8)
                                    .right(esp::E8)
                                    .gap(esp::E4)
                                    .child(vidro("ilha-recolher", Lucide::Minimize).on_click(cx.listener(|this, _, _, cx| {
                                        this.ilha = None;
                                        cx.notify();
                                        cx.stop_propagation();
                                    })))
                                    .child(vidro("ilha-tirar", Lucide::X).on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(lista) = this.anexos.get_mut(&chat_tirar) {
                                            lista.retain(|a| *a != tirado);
                                        }
                                        this.ilha = None;
                                        cx.notify();
                                        cx.stop_propagation();
                                    }))),
                            ),
                    ),
            )).with_priority(1))
        })
    }

    /// Os anexos da próxima mensagem, em pílulas na casca da caixa, acima do campo.
    fn anexos_pendentes(
        &self,
        chat: &str,
        anexos: Vec<PathBuf>,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        div()
            .flex()
            .flex_wrap()
            .gap(esp::E6)
            .pb(esp::E6)
            .children(anexos.into_iter().enumerate().map(|(i, caminho)| {
                let chat = chat.to_string();
                let nome = super::imagens::nome_do_arquivo(&caminho);
                let imagem = crate::claude::tipo_imagem(&caminho).is_some();
                // Três prints colados têm nomes iguais ("colado-…"): a miniatura, o tamanho e a
                // dica com o caminho são o que diz qual é qual antes de mandar.
                let lido = super::imagens::anexo(&caminho);
                let detalhe = lido.detalhe();
                let dica = SharedString::from(if detalhe.is_empty() {
                    caminho.display().to_string()
                } else {
                    format!("{detalhe}\n{}", caminho.display())
                });
                let (largura_mini, altura_mini) = super::imagens::medida_da_miniatura(
                    lido.medidas,
                    super::imagens::ALTURA_CHIP,
                    super::imagens::LARGURA_MAXIMA_CHIP,
                );
                let aberta = self.ilha.as_ref().is_some_and(|(c, p)| *c == chat && *p == caminho);
                let miniatura = (imagem && lido.existe).then(|| {
                    div()
                        .id(("mini-anexo", i))
                        .flex_none()
                        .w(r(largura_mini))
                        .h(r(altura_mini))
                        .rounded(raio::CONTROLE)
                        .bg(Cores::preto())
                        .overflow_hidden()
                        .child(img(caminho.clone()).size_full().object_fit(ObjectFit::Cover))
                });
                // O clique na pílula de uma imagem abre a ilha em cima da caixa (e fecha, se é a
                // dela que está aberta).
                let abre_ilha = imagem && lido.existe;
                let (chat_ilha, alvo_ilha) = (chat.clone(), caminho.clone());
                let icone = miniatura.is_none().then(|| {
                    let (lucide, cor) = match (imagem, lido.existe) {
                        (true, false) => (Lucide::ImageOff, Cores::apagado()),
                        (false, false) => (Lucide::FileX, Cores::apagado()),
                        (true, true) => (Lucide::Image, Cores::tenue()),
                        (false, true) => (Lucide::File, Cores::tenue()),
                    };
                    div().flex_none().text_color(cor).child(super::componentes::icone(lucide, 14., window))
                });
                div()
                    .id(("anexo-pendente", i))
                    .flex()
                    .items_center()
                    .gap(esp::E6)
                    .h(r(28.))
                    .map(|s| if miniatura.is_some() { s.pl(esp::E2) } else { s.pl(esp::E8) })
                    .pr(esp::E2)
                    .rounded(raio::de(28.))
                    .border_1()
                    // A pílula da imagem aberta inverte: é ela que está em cima.
                    .map(|s| {
                        if aberta {
                            s.bg(Cores::forte()).border_color(Cores::forte())
                        } else {
                            s.bg(Cores::cartao_elevado()).border_color(Cores::borda_forte())
                        }
                    })
                    .when(abre_ilha, |s| {
                        s.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
                            // O apertar fora da ilha já a fechou: o clique na própria pílula não
                            // a reabre.
                            let fechou_agora = this
                                .ilha_fechada
                                .take()
                                .is_some_and(|(p, quando)| p == alvo_ilha && quando.elapsed() < std::time::Duration::from_millis(500));
                            this.ilha = (!fechou_agora).then(|| (chat_ilha.clone(), alvo_ilha.clone()));
                            cx.notify();
                        }))
                    })
                    .children(icone)
                    .children(miniatura)
                    // A imagem fica só na miniatura (o nome e o tamanho vão na dica): uma bolinha.
                    .when(!(imagem && lido.existe), |s| s.child(
                        div()
                            .max_w(r(220.))
                            .truncate()
                            .metadado()
                            .text_color(if aberta {
                                Cores::preto()
                            } else if lido.existe {
                                Cores::texto()
                            } else {
                                Cores::tenue()
                            })
                            .child(nome),
                    )
                    .child(
                        div()
                            .flex_none()
                            .metadado()
                            .text_color(Cores::tenue())
                            .child(lido.tamanho.map(|t| t.to_string()).unwrap_or_else(|| "sumiu".into())),
                    ))
                    .child(
                        BotaoRedondo::new(("tirar-anexo", i), Some(Lucide::X.into()), "Remover o anexo")
                            .variante(VarianteRedonda::Plano)
                            .diametro(24.)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(lista) = this.anexos.get_mut(&chat)
                                    && i < lista.len()
                                {
                                    lista.remove(i);
                                }
                                this.ilha = None;
                                cx.notify();
                                // Não é um clique na pílula (que abriria a ilha).
                                cx.stop_propagation();
                            })),
                    )
                    .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            }))
    }

    /// As sugestões da última palavra da caixa: os comandos de barra (o texto começa com "/") e as
    /// menções do "@" (rodada de uso, item 1.2).
    pub(super) fn sugestoes_da_caixa(&self, chat: &Chat, texto: &str) -> Option<(char, usize, Vec<Sugestao>)> {
        let (sinal, digitado, inicio) = gatilho(texto)?;
        if sinal == '@' {
            let lista = self.mencoes_da_caixa(chat, digitado, inicio);
            return (!lista.is_empty()).then_some((sinal, inicio, lista));
        }
        let itens: Vec<Sugestao> = match sinal {
            '/' => self
                .comandos_de(chat)
                .into_iter()
                .map(|c| Sugestao {
                    valor: format!("/{c}"),
                    titulo: format!("/{c}"),
                    detalhe: descricao_comando(&c).map(str::to_string).or_else(|| crate::claude::descricao_do_comando(&c)),
                    busca: c.clone(),
                    peso: c.len(),
                    chave: c,
                })
                .collect(),
            _ => return None,
        };
        let lista = menu_sugestoes(itens, digitado);
        (!lista.is_empty()).then_some((sinal, inicio, lista))
    }

    /// O menu "@" na própria caixa (dono, 16/11/2026: rodada de uso, item 1.2), no lugar do diálogo
    /// "Mencionar arquivo", que tirava o cursor da caixa e engolia o "@claude" da dupla. Na ordem:
    /// o @claude e o @codex (só com a dupla ligada e só no começo da mensagem, a única posição que
    /// o motor entende) e os arquivos e as pastas do chat, com o arquivo aberto à direita no topo.
    /// Enquanto a pasta é lida, uma linha "Lendo a pasta…".
    fn mencoes_da_caixa(&self, chat: &Chat, digitado: &str, inicio: usize) -> Vec<Sugestao> {
        let mut lista: Vec<Sugestao> = Vec::new();
        if chat.parceiro.is_some() && inicio == 0 {
            for agente in [crate::modelo::Agente::Claude, crate::modelo::Agente::Codex] {
                let nome = agente.nome().to_lowercase();
                if nome.starts_with(&digitado.to_lowercase()) {
                    lista.push(Sugestao {
                        valor: format!("@{nome}"),
                        titulo: format!("@{nome}"),
                        detalhe: Some(format!("Só o {} responde a esta mensagem", agente.nome())),
                        chave: nome.clone(),
                        busca: nome,
                        peso: 0,
                    });
                }
            }
        }
        let Some((arquivos, pastas)) = self.arquivos_e_pastas_lidos(&chat.cwd).filter(|_| !self.lendo_arquivos(&chat.cwd)) else {
            lista.push(Sugestao {
                valor: String::new(),
                titulo: "Lendo a pasta…".into(),
                detalhe: None,
                chave: String::new(),
                busca: String::new(),
                peso: 0,
            });
            return lista;
        };
        // O arquivo aberto à direita, se é deste chat, vem antes dos outros.
        let aberto = self.arquivo_aberto.as_ref().and_then(|(pasta, caminho)| {
            let inteiro = pasta.join(caminho);
            inteiro.strip_prefix(&chat.cwd).ok().map(crate::git::caminho_em_texto)
        });
        let chave_memo = (chat.cwd.clone(), digitado.to_string(), std::rc::Rc::as_ptr(&arquivos) as usize, aberto.clone());
        let achados = {
            let memo = self.mencoes.borrow();
            memo.as_ref().filter(|(chave, _)| *chave == chave_memo).map(|(_, achados)| achados.clone())
        };
        let achados = achados.unwrap_or_else(|| {
            let termos: Vec<String> = digitado.to_lowercase().split_whitespace().map(str::to_string).collect();
            let mut achados: Vec<String> = aberto
                .clone()
                .filter(|a| termos.iter().all(|t| a.to_lowercase().contains(t)))
                .into_iter()
                .collect();
            for a in super::seletor_arquivos::filtrar_mencoes(&arquivos, &pastas, digitado, MAX_SUGESTOES) {
                if !achados.contains(a) {
                    achados.push(a.clone());
                }
            }
            *self.mencoes.borrow_mut() = Some((chave_memo, achados.clone()));
            achados
        });
        for caminho in achados {
            let sem_barra = caminho.trim_end_matches('/');
            let (pasta, nome) = sem_barra.rsplit_once('/').unwrap_or(("", sem_barra));
            let titulo = if caminho.ends_with('/') { format!("{nome}/") } else { nome.to_string() };
            lista.push(Sugestao {
                valor: format!("@{caminho}"),
                titulo,
                detalhe: (!pasta.is_empty()).then(|| pasta.to_string()),
                chave: nome.to_lowercase(),
                busca: caminho.to_lowercase(),
                peso: caminho.len(),
            });
        }
        lista.truncate(MAX_SUGESTOES);
        lista
    }

    /// O menu "/" do painel `ix`, se está à vista: há sugestões e o Esc não o fechou.
    pub(super) fn menu_da_caixa(&self, ix: usize, chat: &Chat, cx: &App) -> Option<(char, usize, Vec<Sugestao>)> {
        let painel = self.paineis.get(ix)?;
        if painel.menu_dispensado {
            return None;
        }
        let texto = painel.composer.read(cx).value().to_string();
        self.sugestoes_da_caixa(chat, &texto)
    }

    /// ↑/↓ com o menu "/" à vista: o item marcado anda, sem passar das pontas.
    fn andar_no_menu(&mut self, painel: u64, passo: isize, cx: &mut Context<Self>) {
        let Some(ix) = self.paineis.iter().position(|p| p.id == painel) else {
            return;
        };
        let Some(n) = self.menu_do_painel_ix(ix, cx).map(|(_, _, lista)| lista.len()) else {
            cx.propagate();
            return;
        };
        let atual = self.paineis[ix].indice_menu.min(n - 1);
        self.paineis[ix].indice_menu = atual.saturating_add_signed(passo).min(n - 1);
        cx.notify();
    }

    /// O menu "/" à vista no painel `ix`, lido do chat dele.
    fn menu_do_painel_ix(&self, ix: usize, cx: &App) -> Option<(char, usize, Vec<Sugestao>)> {
        let id = self.paineis.get(ix)?.chat.clone()?;
        let motor = self.motor.as_ref()?.read(cx);
        self.menu_da_caixa(ix, motor.chat(&id)?, cx)
    }

    /// Tab na caixa vazia: a sugestão da próxima mensagem vira o texto, com o cursor no fim, para
    /// mandar com Enter ou mudar antes. Sem sugestão, o Tab segue o caminho de sempre.
    fn aceitar_sugestao(&mut self, chat: &str, painel: u64, window: &mut Window, cx: &mut Context<Self>) {
        let sugestao = self.motor.as_ref().and_then(|m| m.read(cx).sugestao(chat).map(str::to_string));
        let (Some(sugestao), Some(p)) = (sugestao, self.paineis.iter().find(|p| p.id == painel)) else {
            cx.propagate();
            return;
        };
        let composer = p.composer.clone();
        composer.update(cx, |c, cx| {
            c.set_value(sugestao.clone(), window, cx);
            let fim = sugestao.len();
            c.set_selected_range(fim..fim, cx);
        });
        cx.notify();
    }

    /// Tab com o menu "/" à vista: completa o item marcado.
    fn escolher_no_menu(&mut self, painel: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.paineis.iter().position(|p| p.id == painel) else {
            return;
        };
        let Some((_, inicio, lista)) = self.menu_do_painel_ix(ix, cx) else {
            cx.propagate();
            return;
        };
        let marcado = self.paineis[ix].indice_menu.min(lista.len() - 1);
        let valor = lista[marcado].valor.clone();
        self.escolher_sugestao(painel, inicio, &valor, window, cx);
    }

    /// O menu das sugestões, acima da caixa. Entra como os popovers: fade e 6 px vindos de baixo,
    /// do lado da caixa. O marcado (o primeiro, até as setas andarem) é o que o Enter e o Tab
    /// escolhem.
    fn menu_comandos(&self, chat: &Chat, ix: usize, cx: &Context<Self>) -> impl IntoElement + use<> {
        let painel = self.paineis[ix].id;
        let sugestoes = self.menu_da_caixa(ix, chat, cx);
        let marcado = self.paineis[ix].indice_menu;
        div().relative().when_some(sugestoes, |s, (sinal, inicio, lista)| {
            let marcado = marcado.min(lista.len().saturating_sub(1));
            let menu = div()
                .flex()
                .flex_col()
                .gap(esp::E2)
                .w_full()
                .p(esp::E8)
                .flutuante(cx)
                .children(lista.into_iter().enumerate().map(|(i, sugestao)| {
                    let valor = sugestao.valor.clone();
                    div()
                        .id(("sugestao-caixa", i))
                        .flex()
                        .items_center()
                        .h(r(32.))
                        .px(esp::E8)
                        .gap(esp::E8)
                        .rounded(raio::CONTROLE)
                        .cursor_pointer()
                        .when(i == marcado, |s| s.bg(Cores::selecao()))
                        .when(i != marcado, |s| s.hover(|s| s.bg(Cores::hover())))
                        .child(
                            div()
                                .flex_shrink_0()
                                .max_w(relative(0.6))
                                .truncate()
                                .map(|d| if sinal == '/' { d.mono().text_color(Cores::codigo()) } else { d.corpo_ui().text_color(Cores::texto()) })
                                .child(sugestao.titulo.clone()),
                        )
                        .when_some(sugestao.detalhe.clone(), |s, d| {
                            s.child(div().flex_1().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(d))
                        })
                        .on_click(cx.listener(move |this, _, window, cx| this.escolher_sugestao(painel, inicio, &valor, window, cx)))
                }))
                .with_animation(
                    ("entra-comandos", painel as usize),
                    movimento::animacao(movimento::ENTRAR, Curva::Entrar),
                    |menu, t| menu.opacity(t).top(r(6. * (1. - t))),
                );
            // O absoluto fica por fora: o deslize mexe no `top` de quem está dentro.
            s.child(div().absolute().bottom(esp::E4).left_0().w_full().max_w(r(420.)).child(menu))
        })
    }

    /// Troca a última palavra da caixa pela sugestão escolhida (o comando, `#chave ` ou `@12 `).
    pub(super) fn escolher_sugestao(&mut self, painel: u64, inicio: usize, valor: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.paineis.iter().find(|p| p.id == painel).map(|p| p.composer.clone()) else {
            return;
        };
        // A linha "Lendo a pasta…" não completa nada.
        if valor.is_empty() {
            return;
        }
        // A palavra é medida de novo no texto de agora (o clique pode vir depois de outra tecla).
        let atual = composer.read(cx).value().to_string();
        let inicio = gatilho(&atual).map_or(inicio, |(_, _, i)| i);
        let inicio = if atual.is_char_boundary(inicio) { inicio } else { atual.len() };
        // Um arquivo ou uma pasta do "@" entra como pílula; o comando, o "@claude" e o chat, como
        // texto. Só a palavra que se completa muda: as pílulas de antes ficam.
        let cwd = self
            .paineis
            .iter()
            .find(|p| p.id == painel)
            .and_then(|p| p.chat.clone())
            .and_then(|id| self.motor.as_ref().and_then(|m| m.read(cx).chat(&id).map(|c| c.cwd.clone())));
        let ficha = valor
            .strip_prefix('@')
            .filter(|c| c.contains(['.', '/']))
            .zip(cwd)
            .and_then(|(caminho, cwd)| super::links_arquivo::resolver_mencao(&cwd, caminho))
            .map(|inteiro| ficha_da_mencao(valor, &inteiro, None));
        composer.update(cx, |c, cx| trocar_o_fim(c, inicio, valor, ficha, window, cx));
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    pub(super) fn comandos_de(&self, chat: &Chat) -> Vec<String> {
        // Os comandos de barra e as skills são do Claude Code; no Codex fica só o `/clear`, que é
        // do Vitral.
        if chat.agente == crate::modelo::Agente::Codex {
            return vec!["clear".into()];
        }
        // Antes do primeiro turno do chat no Vitral, vale o que o Claude Code disse que oferece
        // naquela pasta (comandos e skills, perguntados quando o chat abre); sem resposta dele,
        // os fixos.
        let mut lista: Vec<String> = if !chat.comandos.is_empty() {
            chat.comandos.clone()
        } else if let Some(da_pasta) = crate::claude::comandos_da_pasta(&chat.cwd) {
            da_pasta
        } else {
            COMANDOS_INICIAIS.iter().map(|c| c.to_string()).collect()
        };
        if !lista.iter().any(|c| c == "clear") {
            lista.insert(0, "clear".into());
        }
        lista
    }

    /// O texto digitado depois da barra, enquanto ainda é o nome de um comando.
    pub(super) fn prefixo_comando(&self, cx: &App) -> Option<String> {
        prefixo_de(&self.composer_ativo()?, cx)
    }

    /// Esc na caixa: com o menu "/" aberto, fecha o menu e deixa o que foi digitado (antes apagava
    /// a caixa, embora os atalhos prometessem só fechar o menu: teste de usabilidade, 16/11/2026);
    /// com o chat trabalhando, para. Fora disso, a tecla segue adiante.
    fn esc_na_caixa(&mut self, chat: &str, painel: u64, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            cx.propagate();
            return;
        };
        let Some(ix) = self.paineis.iter().position(|p| p.id == painel) else {
            cx.propagate();
            return;
        };
        let (menu_aberto, ocupado) = {
            let m = motor.read(cx);
            let menu = m.chat(chat).is_some_and(|c| self.menu_da_caixa(ix, c, cx).is_some());
            (menu, m.execucao(chat).rodando())
        };
        if menu_aberto {
            self.paineis[ix].menu_dispensado = true;
            cx.notify();
        } else if ocupado {
            self.esc_no_turno(chat, cx);
        } else {
            cx.propagate();
        }
    }

    /// Shift+Tab na caixa: o próximo modo de permissão, na ordem do menu. Vale a partir do
    /// próximo turno, como no menu.
    fn alternar_modo(&mut self, chat: &str, painel: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let atual = {
            let m = motor.read(cx);
            m.chat(chat).map(|c| m.permissao_de(c))
        };
        if let Some(atual) = atual {
            let proximo = proximo_modo(&atual);
            motor.update(cx, |m, cx| m.mudar_chat(chat, |c| c.permissao = Some(proximo.into()), cx));
            self.avisar_na_estreita(painel, format!("Modo: {}", nome_do_modo(proximo)), window, cx);
        }
    }

    /// Na caixa estreita os chips são só ícones: a troca de modo ou de modelo pelo teclado quase
    /// não dava sinal (o Fable escolhido continuava com o logo do Claude). Lá, um aviso curto diz o
    /// que ficou (teste de usabilidade, 16/11/2026).
    pub(super) fn avisar_na_estreita(&self, painel: u64, texto: String, window: &mut Window, cx: &mut App) {
        let estreita = self.paineis.iter().any(|p| p.id == painel && p.medida.faixa == FaixaCaixa::Estreita);
        if estreita {
            gpui_kit::component::WindowExt::push_notification(window, super::componentes::aviso(super::componentes::Tom::Info, texto), cx);
        }
    }

    fn escolher_anexos(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        let escolha = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Anexar".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(caminhos))) = escolha.await {
                let _ = this.update(cx, |this, cx| {
                    this.anexos.entry(chat).or_default().extend(caminhos);
                    cx.notify();
                });
            }
        })
        .detach();
    }
}

fn descricao_comando(comando: &str) -> Option<&'static str> {
    Some(match comando {
        "clear" => "Começa uma sessão nova (o Claude esquece a conversa)",
        "compact" => "Resume a conversa para liberar contexto",
        "context" => "Mostra o uso do contexto",
        "usage" => "Uso do plano",
        "init" => "Cria o CLAUDE.md do projeto",
        "review" => "Revisa as mudanças",
        "security-review" => "Revisão de segurança das mudanças",
        "simplify" => "Simplifica o código alterado",
        "model" => "Troca o modelo",
        "effort" => "Troca o esforço",
        "code-review" => "Revisão de código",
        "debug" => "Ajuda a depurar",
        "verify" => "Verifica o trabalho",
        _ => return None,
    })
}

/// Uma regra de "permitir sempre" como aparece no menu do modo.
fn descricao_regra(regra: &str) -> String {
    if regra == "edicoes" {
        return "Edições de arquivos neste chat".into();
    }
    if let Some(comando) = regra.strip_prefix("bash:") {
        let linha = comando.lines().next().unwrap_or_default();
        return if linha.chars().count() > 48 || comando.contains('\n') {
            format!("$ {}…", linha.chars().take(48).collect::<String>())
        } else {
            format!("$ {linha}")
        };
    }
    regra.strip_prefix("ferramenta:").unwrap_or(regra).to_string()
}

/// Em janela baixa, a caixa cresce menos antes de rolar por dentro, para não espremer as
/// mensagens e o plano; muda no quadro seguinte (avisar a janela durante o desenho não a
/// redesenha).
fn ajustar_linhas(
    janela: WeakEntity<Janela>,
    painel: u64,
    linhas: usize,
) -> impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static {
    move |_, window, _| {
        let agora = linhas_da_caixa(window.viewport_size().height / window.rem_size());
        if agora == linhas {
            return;
        }
        window.on_next_frame(move |_, cx| {
            let _ = janela.update(cx, |this, cx| {
                if let Some(p) = this.paineis.iter_mut().find(|p| p.id == painel) {
                    p.linhas_caixa = agora;
                    p.composer.update(cx, |c, cx| c.set_auto_grow(1, agora, cx));
                    cx.notify();
                }
            });
        });
    }
}

/// Quantas linhas a caixa pode ter com a janela desta altura (em rem): o resto do cartão, o
/// plano e umas linhas de mensagem levam uns 26 rem (420 px); cada linha, 1,5 rem.
fn linhas_da_caixa(altura: f32) -> usize {
    ((altura - 26.25) / 1.5).floor().clamp(4., LINHAS_CAIXA as f32) as usize
}

/// O modo seguinte na ordem do menu, voltando ao primeiro depois do último.
fn proximo_modo(atual: &str) -> &'static str {
    let i = MODOS
        .iter()
        .position(|(v, _, _)| *v == atual)
        .unwrap_or(MODOS.len() - 1);
    MODOS[(i + 1) % MODOS.len()].0
}

/// Rótulo do seletor de modelo: o modelo que vale de verdade. No padrão, o que o Claude Code usou
/// no último turno; num apelido ("opus"), a versão em uso, se for da mesma família.
fn rotulo_modelo(escolhido: &str, em_uso: Option<&str>) -> String {
    let em_uso = em_uso.filter(|m| !m.is_empty()).map(nome_curto_modelo);
    if escolhido.is_empty() {
        return em_uso.unwrap_or_else(|| "Padrão".into());
    }
    match MODELOS.iter().find(|(v, _)| *v == escolhido) {
        Some((_, nome)) => em_uso
            .filter(|m| m.to_lowercase().starts_with(escolhido))
            .unwrap_or_else(|| nome.to_string()),
        // Conversas antigas guardaram o nome completo do modelo, que não está na lista.
        None => nome_curto_modelo(escolhido),
    }
}

/// Imagens coladas viram arquivos na pasta dos anexos (`anexos/` nos dados); arquivos copiados do
/// gerenciador entram pelo caminho.
fn anexos_da_area_de_transferencia(item: &ClipboardItem, pasta: &Path) -> Vec<PathBuf> {
    let mut caminhos = Vec::new();
    for entrada in item.entries() {
        match entrada {
            ClipboardEntry::ExternalPaths(lista) => caminhos.extend(lista.paths().iter().cloned()),
            ClipboardEntry::Image(imagem) => {
                let extensao = match imagem.format {
                    ImageFormat::Png => "png",
                    ImageFormat::Jpeg => "jpg",
                    ImageFormat::Gif => "gif",
                    ImageFormat::Webp => "webp",
                    _ => continue,
                };
                let arquivo = pasta.join(format!(
                    "colado-{}.{extensao}",
                    crate::modelo::novo_id()
                ));
                if std::fs::create_dir_all(pasta).is_ok()
                    && std::fs::write(&arquivo, &imagem.bytes).is_ok()
                {
                    caminhos.push(arquivo);
                }
            }
            ClipboardEntry::String(_) => {}
        }
    }
    caminhos
}

/// O texto depois da barra, enquanto ainda é o nome de um comando.
fn prefixo_de(composer: &Entity<TextareaState>, cx: &App) -> Option<String> {
    let texto = composer.read(cx).value().to_string();
    let resto = texto.strip_prefix('/')?;
    (!resto.contains(char::is_whitespace)).then(|| resto.to_string())
}

#[cfg(test)]
mod testes_sugestoes {
    use super::{Sugestao, completar, escolha_do_enter, gatilho, menu_sugestoes};

    fn sugestao(valor: &str, chave: &str, busca: &str, peso: usize) -> Sugestao {
        Sugestao {
            valor: valor.into(),
            titulo: valor.into(),
            detalhe: None,
            chave: chave.into(),
            busca: busca.into(),
            peso,
        }
    }

    /// O Enter completa o que o menu marca, e não o primeiro da lista crua que contém o digitado:
    /// "/s" virava "/usage" com "/simplify" marcado, e "/" virava "/clear" (teste de usabilidade,
    /// 16/11/2026).
    #[test]
    fn arroba_so_no_comeco_ou_depois_de_espaco() {
        assert_eq!(gatilho("@"), Some(('@', "", 0)));
        assert_eq!(gatilho("veja @cent"), Some(('@', "cent", 5)));
        // O "@" de um e-mail não abre o menu.
        assert_eq!(gatilho("victor@"), None);
        assert_eq!(gatilho("fala com victor@eita"), None);
    }

    #[test]
    fn o_enter_completa_o_item_marcado_do_menu() {
        let comandos: Vec<String> = ["clear", "usage", "init", "simplify", "compact"].map(String::from).to_vec();
        let itens = |c: &[String]| -> Vec<Sugestao> { c.iter().map(|c| sugestao(&format!("/{c}"), c, c, c.len())).collect() };
        // "/s": o menu põe primeiro quem começa com "s".
        let menu = menu_sugestoes(itens(&comandos), "s");
        let i = escolha_do_enter(&comandos, "s", menu.len(), 0).unwrap();
        assert_eq!(menu[i].valor, "/simplify");
        // Só "/": o marcado é o mais curto, nunca o /clear por estar primeiro na lista.
        let menu = menu_sugestoes(itens(&comandos), "");
        let i = escolha_do_enter(&comandos, "", menu.len(), 0).unwrap();
        assert_eq!(menu[i].valor, "/init");
        // As setas andaram: vale o marcado, sem passar do fim.
        assert_eq!(escolha_do_enter(&comandos, "", menu.len(), 2), Some(2));
        assert_eq!(escolha_do_enter(&comandos, "", menu.len(), 99), Some(menu.len() - 1));
        // O nome exato de um comando vai como está; sem menu, nada a completar.
        assert_eq!(escolha_do_enter(&comandos, "clear", 1, 0), None);
        assert_eq!(escolha_do_enter(&comandos, "xyz", 0, 0), None);
    }

    #[test]
    fn o_gatilho_e_a_ultima_palavra() {
        assert_eq!(gatilho("/comp"), Some(('/', "comp", 0)));
        assert_eq!(gatilho("/compact agora"), None);
        assert_eq!(gatilho("veja a /barra"), None);
        assert_eq!(gatilho("#nin"), Some(('#', "nin", 0)));
        assert_eq!(gatilho("mande para #nin"), Some(('#', "nin", 11)));
        assert_eq!(gatilho("fale do @"), Some(('@', "", 8)));
        assert_eq!(gatilho("fale do @12 e"), None);
        // No meio da palavra não é citação.
        assert_eq!(gatilho("C#"), None);
        assert_eq!(gatilho("eu@x.com"), None);
        assert_eq!(gatilho("ação #lój"), Some(('#', "lój", 7)));
    }

    #[test]
    fn completar_troca_so_o_ultimo_token() {
        assert_eq!(completar("fale do @1", 8, "@12"), "fale do @12 ");
        assert_eq!(completar("#nin", 0, "#ninee"), "#ninee ");
        assert_eq!(completar("/comp", 0, "/compact"), "/compact ");
    }

    #[test]
    fn quem_comeca_com_o_digitado_vem_antes_e_no_maximo_oito() {
        let itens = vec![
            sugestao("#site-loja", "site-loja", "site-loja loja do site", 0),
            sugestao("#loja", "loja", "loja loja", 1),
            sugestao("#vitral", "vitral", "vitral", 2),
        ];
        let achadas: Vec<String> = menu_sugestoes(itens.clone(), "loj").into_iter().map(|s| s.valor).collect();
        assert_eq!(achadas, ["#loja", "#site-loja"]);
        // Sem acento nem maiúscula; vazio mostra todas, pelo peso.
        assert_eq!(menu_sugestoes(itens.clone(), "LÓJ").len(), 2);
        let todas: Vec<String> = menu_sugestoes(itens, "").into_iter().map(|s| s.valor).collect();
        assert_eq!(todas, ["#site-loja", "#loja", "#vitral"]);
        let muitas: Vec<Sugestao> = (0..12).map(|i| sugestao(&format!("@{i}"), &i.to_string(), "", i)).collect();
        assert_eq!(menu_sugestoes(muitas.clone(), "").len(), 8);
        // O número do chat: "@1" traz o 1 e os que começam com 1 antes dos que só o contêm.
        let achados: Vec<String> = menu_sugestoes(muitas, "1").into_iter().map(|s| s.valor).collect();
        assert_eq!(achados, ["@1", "@10", "@11"]);
    }
}

#[cfg(test)]
mod testes_caixa {
    use super::{
        Envio, LINHAS_CAIXA, MODOS, com_claude, envio, gastar_chave, indice_esforco, linhas_da_caixa, milhares,
        nomes_dos_niveis, proximo_modo, rotulo_modelo, tokens, usa_esforco, uso_do_contexto,
    };

    #[test]
    fn chave_da_proxima_mensagem_desliga_depois_de_usar() {
        let mut armados = std::collections::HashSet::from(["c1".to_string()]);
        // Comando de barra não gasta.
        assert!(!gastar_chave(&mut armados, "c1", "  /compact"));
        assert!(armados.contains("c1"));
        // Outra conversa não mexe nesta.
        assert!(!gastar_chave(&mut armados, "c2", "oi"));
        assert!(gastar_chave(&mut armados, "c1", "refatore o motor"));
        assert!(armados.is_empty());
        // Só uma vez: a mensagem seguinte vai sem.
        assert!(!gastar_chave(&mut armados, "c1", "e os testes"));
    }

    #[test]
    fn caixa_cresce_menos_em_janela_baixa() {
        // Altura da janela em rem (16 px): 920 px, 750, 700, 600 e uma de 400.
        assert_eq!(linhas_da_caixa(57.5), LINHAS_CAIXA);
        assert_eq!(linhas_da_caixa(46.875), LINHAS_CAIXA);
        assert_eq!(linhas_da_caixa(43.75), 11);
        assert_eq!(linhas_da_caixa(37.5), 7);
        assert_eq!(linhas_da_caixa(25.), 4);
    }

    #[test]
    fn modelo_mostra_o_que_vale() {
        assert_eq!(rotulo_modelo("", None), "Padrão");
        assert_eq!(rotulo_modelo("", Some("")), "Padrão");
        assert_eq!(rotulo_modelo("", Some("claude-opus-5-5")), "Opus 5.5");
        assert_eq!(rotulo_modelo("opus", Some("claude-opus-5-5")), "Opus 5.5");
        // Trocou para outro modelo e o turno ainda não rodou: o nome da lista.
        assert_eq!(rotulo_modelo("sonnet", Some("claude-opus-5-5")), "Sonnet");
        assert_eq!(rotulo_modelo("haiku", None), "Haiku");
        assert_eq!(rotulo_modelo("claude-sonnet-4-5-20250929", None), "Sonnet 4.5");
    }

    #[test]
    fn prefixo_claude_so_nos_modelos_do_claude() {
        assert_eq!(com_claude("Opus 5.5"), "Claude Opus 5.5");
        assert_eq!(com_claude("Opus 5"), "Claude Opus 5");
        assert_eq!(com_claude("Haiku"), "Claude Haiku");
        assert_eq!(com_claude("Padrão"), "Padrão");
        assert_eq!(com_claude("gpt-5"), "gpt-5");
    }

    #[test]
    fn shift_tab_percorre_os_modos_em_ciclo() {
        assert_eq!(proximo_modo("manual"), "acceptEdits");
        assert_eq!(proximo_modo("auto"), "bypassPermissions");
        assert_eq!(proximo_modo("bypassPermissions"), "manual");
        // Valor desconhecido conta como "Sem perguntar", como em nome_do_modo.
        assert_eq!(proximo_modo("default"), "manual");
        let mut modo = "manual";
        for _ in 0..MODOS.len() {
            modo = proximo_modo(modo);
        }
        assert_eq!(modo, "manual");
    }

    #[test]
    fn niveis_de_esforco() {
        assert_eq!(indice_esforco(""), 0);
        assert_eq!(indice_esforco("medium"), 2);
        assert_eq!(indice_esforco("max"), 5);
        assert_eq!(indice_esforco("turbo"), 0);
        assert_eq!(nomes_dos_niveis(false)[5], "Máximo");
        assert_eq!(nomes_dos_niveis(true)[5], "Máx");
        assert_eq!(nomes_dos_niveis(true)[4], "Extra");
        assert!(usa_esforco(Some("claude-opus-5-5")));
        assert!(usa_esforco(None));
        assert!(!usa_esforco(Some("claude-haiku-4-5")));
    }

    #[test]
    fn contexto_usa_a_janela_padrao_antes_do_primeiro_result() {
        // Opus 5 tem 1M: 300 mil são 30%, não 150%.
        let (fracao, numero, janela) = uso_do_contexto(300_000, None, Some("claude-opus-5-5"));
        assert_eq!(janela, 1_000_000);
        assert_eq!(numero, 30);
        assert!((fracao - 0.3).abs() < 1e-6);
        let (_, numero, janela) = uso_do_contexto(150_000, None, Some("claude-haiku-4-5"));
        assert_eq!((numero, janela), (75, 200_000));
        // A janela informada pelo Claude Code vale mais que o palpite.
        assert_eq!(uso_do_contexto(100_000, Some(200_000), Some("claude-opus-5-5")).1, 50);
        // O número do meio para em 99.
        assert_eq!(uso_do_contexto(400_000, Some(200_000), None).1, 99);
        assert_eq!(milhares(123_456), "123 mil");
        assert_eq!(tokens(200_000), "200 mil tokens");
        assert_eq!(tokens(1_000_000), "1 milhão de tokens");
        assert_eq!(tokens(2_000_000), "2 milhões de tokens");
    }

    #[test]
    fn enviar_vira_parar_com_o_chat_rodando() {
        assert_eq!(envio(false, true), Envio::Vazio);
        assert_eq!(envio(false, false), Envio::Enviar);
        assert_eq!(envio(true, true), Envio::Parar);
        assert_eq!(envio(true, false), Envio::PararEEnviar);
    }
}

#[cfg(test)]
mod testes_nome_modelo {
    use super::nome_curto_modelo;

    #[test]
    fn encurta_os_nomes_do_claude() {
        assert_eq!(nome_curto_modelo("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(nome_curto_modelo("claude-sonnet-4-5-20250929"), "Sonnet 4.5");
        assert_eq!(nome_curto_modelo("claude-haiku-4-5[1m]"), "Haiku 4.5 (1M)");
        assert_eq!(nome_curto_modelo("claude-3-5-sonnet-20241022"), "Sonnet 3.5");
        assert_eq!(nome_curto_modelo("claude-fable"), "Fable");
    }

    #[test]
    fn deixa_o_resto_como_veio() {
        assert_eq!(nome_curto_modelo(""), "");
        assert_eq!(nome_curto_modelo("opus"), "opus");
        assert_eq!(nome_curto_modelo("gpt-5"), "gpt-5");
        assert_eq!(nome_curto_modelo("claude-"), "claude-");
        assert_eq!(nome_curto_modelo("claude-opus-mais-5"), "claude-opus-mais-5");
    }
}

#[cfg(test)]
mod testes_da_citacao {
    use super::com_citacao;

    #[test]
    fn o_trecho_vai_citado_na_frente_da_resposta() {
        assert_eq!(com_citacao("uma linha", "concordo"), "> uma linha\n\nconcordo");
        assert_eq!(com_citacao("  a\n\nb  \n", "\nmuda o b"), "> a\n>\n> b\n\nmuda o b");
    }
}

#[cfg(test)]
mod testes_das_versoes {
    use super::{VERSOES_DE_MODELO, versoes_dos_oferecidos};
    use crate::claude::ModeloOferecido;

    fn oferecido(valor: &str, resolvido: &str) -> ModeloOferecido {
        ModeloOferecido { valor: valor.into(), resolvido: resolvido.into(), nome: String::new() }
    }

    /// As versões exatas vêm do que o Claude Code instalado oferece: cada modelo que ele lista e o
    /// que cada apelido resolve, uma vez só, por família (Fable, Opus, Sonnet, Haiku) e da mais nova
    /// para a mais antiga. É o que faz um modelo novo aparecer sem versão nova do Vitral.
    #[test]
    fn versoes_seguem_o_claude_code_instalado() {
        let oferecidos = [
            oferecido("default", "claude-opus-5-5"),
            oferecido("opus", "claude-opus-5-5"),
            oferecido("claude-fable-5-2", "claude-fable-5-2"),
            oferecido("sonnet", "claude-sonnet-5-5"),
            oferecido("haiku", "claude-haiku-5-5"),
            oferecido("fable", "claude-fable-5-1"),
            oferecido("claude-haiku-4-5-20251001", "claude-haiku-4-5-20251001"),
            oferecido("claude-opus-5", "claude-opus-5"),
            oferecido("claude-nova-9", "claude-nova-9"),
        ];
        assert_eq!(
            versoes_dos_oferecidos(&oferecidos),
            [
                "claude-fable-5-2",
                "claude-fable-5-1",
                "claude-opus-5-5",
                "claude-opus-5",
                "claude-sonnet-5-5",
                "claude-haiku-5-5",
                "claude-haiku-4-5-20251001",
                // Uma família que o Vitral ainda não conhece vai no fim, mas aparece.
                "claude-nova-9",
            ]
        );
    }

    /// Sem resposta do Claude Code (ainda não respondeu, não está instalado), vale a lista fixa.
    #[test]
    fn sem_resposta_vale_a_lista_fixa() {
        assert_eq!(versoes_dos_oferecidos(&[]), VERSOES_DE_MODELO);
        // Só apelidos sem versão resolvida também não dão lista.
        assert_eq!(versoes_dos_oferecidos(&[oferecido("opus", "opus")]), VERSOES_DE_MODELO);
    }

    /// Duas famílias que o Vitral ainda não conhece, na mesma versão, são duas linhas (uma não
    /// esconde a outra), no fim da lista e cada família junta.
    #[test]
    fn familias_desconhecidas_nao_se_escondem() {
        let oferecidos = [
            oferecido("claude-zeta-6", "claude-zeta-6"),
            oferecido("claude-alfa-6", "claude-alfa-6"),
            oferecido("claude-opus-5-5", "claude-opus-5-5"),
            oferecido("claude-alfa-6-1", "claude-alfa-6-1"),
        ];
        assert_eq!(
            versoes_dos_oferecidos(&oferecidos),
            ["claude-opus-5-5", "claude-alfa-6-1", "claude-alfa-6", "claude-zeta-6"]
        );
    }
}
