//! A caixa de mensagem, com as opções da conversa como na extensão do Claude Code: anexos,
//! modo de permissão, modelo, esforço, conta, uso do contexto e o menu de comandos de barra.
//! Os chips da barra, com os menus e os popovers deles, ficam em `composer_seletores.rs`.

use std::{
    cell::Cell,
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui_kit::component::{
    ElementExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{InlineToken, InputContent, Textarea, TextareaState},
    tooltip::{ManagedTooltipExt as _, Tooltip},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Janela;
use super::componentes::{
    Anel, BotaoRedondo, FaixaCaixa, Flutuante as _, TamanhoBotao, VarianteRedonda, botao_fantasma, cor_da_faixa,
    popover, variante_fantasma,
};
use super::movimento::{self, Curva, com_estado};
use crate::{
    modelo::Chat,
    motor::{self, Motor},
    tema::{Cores, Lucide, Tipo as _, esp, gradiente_quente, linear, r, raio, tam},
};

#[path = "composer_seletores.rs"]
mod seletores;

use seletores::Barra;
pub(super) use seletores::rotulo_com_gatilho;
pub(super) use seletores::Abertos as AbertosDoModelo;

/// (valor, nome, descrição) dos modos de permissão do Claude Code.
pub const MODOS: [(&str, &str, &str); 5] = [
    // Textos curtos (Linear, 25/11/2026: "tudo muito verboso").
    ("manual", "Manual", "Pede antes de tudo"),
    ("acceptEdits", "Aceitar edições", "Edita sem pedir"),
    ("plan", "Planejar", "Só planeja"),
    ("auto", "Automático", "Decide sozinho"),
    ("bypassPermissions", "Sem perguntar", "Nunca pede"),
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
pub const DESCRICOES_DE_MODELO: [&str; 4] = ["O mais forte", "Dia a dia", "Mais leve", "O mais rápido"];

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
// No menu "/" da caixa: as setas andam e o Tab completa o marcado (o Enter também, pelo enviar);
// o Ctrl+O abre o arquivo da skill marcada no painel da direita.
actions!(vitral, [MenuAcima, MenuAbaixo, MenuEscolher, AbrirSkill]);
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

/// Uma sugestão do menu da caixa: um comando de barra, uma skill, uma menção ou um chat.
#[derive(Clone, Debug, Default, PartialEq)]
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
    /// A dica de argumento do comando ("audit | google").
    pub dica: Option<String>,
    /// De onde vem: "projeto", "pessoal" ou "plugin <nome>".
    pub origem: Option<String>,
    /// É uma skill (tem `SKILL.md`), e não um comando de barra comum.
    pub skill: bool,
    /// O arquivo que o painel da direita abre ("ver a skill").
    pub arquivo: Option<PathBuf>,
}

/// O que a palavra sob o cursor pede: o sinal, o que veio depois dele até o cursor, e onde a palavra
/// começa e acaba (é essa faixa que a escolha troca).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Gatilho<'a> {
    pub sinal: char,
    pub digitado: &'a str,
    pub inicio: usize,
    pub fim: usize,
}

/// O gatilho da palavra sob o cursor (dono, 24/11/2026: o menu abria só para a última palavra do
/// texto). A palavra vai de um espaço ao outro em volta do cursor; `/` só vale na primeira palavra
/// do texto (o Claude Code só entende comando de barra ali), `#` e `@` em qualquer uma. O cursor
/// tem que estar depois do sinal. Um caminho com espaço vai entre aspas (`@"minha pasta/a b.rs"`):
/// com o cursor dentro das aspas, a palavra é o trecho todo.
pub(super) fn gatilho_em(texto: &str, cursor: usize) -> Option<Gatilho<'_>> {
    let mut cursor = cursor.min(texto.len());
    while !texto.is_char_boundary(cursor) {
        cursor -= 1;
    }
    // O `@"` aberto antes do cursor, na mesma linha e sem aspa de fechar entre ele e o cursor.
    let linha = texto[..cursor].rfind('\n').map_or(0, |i| i + 1);
    if let Some(i) = texto[linha..cursor].rfind("@\"").map(|i| linha + i)
        && texto[..i].chars().next_back().is_none_or(char::is_whitespace)
        && !texto[i + 2..cursor].contains('"')
    {
        let fim = match texto[cursor..].find(['"', '\n']) {
            Some(j) if texto[cursor + j..].starts_with('"') => cursor + j + 1,
            _ => cursor + texto[cursor..].find(char::is_whitespace).unwrap_or(texto.len() - cursor),
        };
        return Some(Gatilho { sinal: '@', digitado: &texto[i + 2..cursor], inicio: i, fim });
    }
    let inicio = texto[..cursor].char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8());
    let fim = cursor + texto[cursor..].find(char::is_whitespace).unwrap_or(texto.len() - cursor);
    let sinal = texto[inicio..fim].chars().next().filter(|c| matches!(c, '#' | '@' | '/'))?;
    if (sinal == '/' && inicio != 0) || cursor < inicio + sinal.len_utf8() {
        return None;
    }
    Some(Gatilho { sinal, digitado: &texto[inicio + sinal.len_utf8()..cursor], inicio, fim })
}

/// O gatilho da última palavra do texto (o cursor no fim): `(sinal, digitado, onde começa)`.
#[cfg(test)]
pub(super) fn gatilho(texto: &str) -> Option<(char, &str, usize)> {
    let g = gatilho_em(texto, texto.len())?;
    Some((g.sinal, g.digitado, g.inicio))
}

/// O valor de uma menção de arquivo na caixa: `@caminho`, ou `@"caminho"` se tem espaço.
pub(super) fn valor_da_mencao(caminho: &str) -> String {
    if caminho.contains(char::is_whitespace) { format!("@\"{caminho}\"") } else { format!("@{caminho}") }
}

/// Quão bem o `digitado` (já sem acento nem maiúscula) casa com a sugestão, de 0 (o melhor) a 4:
/// o nome exato, o nome começa com ele, uma parte do nome começa com ele (depois de `-`, `:`, `_`,
/// `.` ou `/`), o nome o contém, só a busca (a descrição) o contém. `None`: não casa.
fn qualidade_do_casamento(s: &Sugestao, digitado: &str) -> Option<u8> {
    if digitado.is_empty() || s.chave == digitado {
        return Some(0);
    }
    if s.chave.starts_with(digitado) {
        return Some(1);
    }
    let comeca_uma_parte = s.chave.match_indices(digitado).any(|(i, _)| s.chave[..i].ends_with(['-', ':', '_', '.', '/']));
    if comeca_uma_parte {
        return Some(2);
    }
    if s.chave.contains(digitado) {
        return Some(3);
    }
    s.busca.contains(digitado).then_some(4)
}

/// As sugestões que casam com o digitado, da que casa melhor para a que casa pior (ver
/// `qualidade_do_casamento`), cada grupo pelo peso; no máximo `limite`, ou todas com `None` (o "/"
/// mostra o catálogo inteiro).
pub(super) fn menu_sugestoes(itens: Vec<Sugestao>, digitado: &str, limite: Option<usize>) -> Vec<Sugestao> {
    let digitado = crate::dados::chave(digitado);
    let mut lista: Vec<(u8, Sugestao)> = itens.into_iter().filter_map(|s| Some((qualidade_do_casamento(&s, &digitado)?, s))).collect();
    lista.sort_by_key(|(qualidade, s)| (*qualidade, s.peso));
    let lista = lista.into_iter().map(|(_, s)| s);
    match limite {
        Some(n) => lista.take(n).collect(),
        None => lista.collect(),
    }
}

/// Como o menu mostra a origem de um comando: "projeto", "pessoal", "plugin <nome>". Os do próprio
/// Claude Code e os de origem desconhecida não mostram nada.
fn rotulo_da_origem(origem: &crate::claude::Origem) -> Option<String> {
    use crate::claude::Origem;
    match origem {
        Origem::Projeto => Some(tr!("projeto").to_string()),
        Origem::Pessoal => Some(tr!("pessoal").to_string()),
        Origem::Plugin(nome) => Some(tr!("plugin {nome}", nome = nome).to_string()),
        Origem::Sincronizada => Some("claude.ai".into()),
        Origem::Embutida | Origem::Desconhecida => None,
    }
}

/// A sugestão do menu "/" para um comando ou skill do catálogo.
pub(super) fn sugestao_do_comando(c: &crate::claude::ComandoOferecido) -> Sugestao {
    let detalhe = descricao_comando(&c.nome).map(|d| d.to_string()).or_else(|| Some(c.descricao.clone()).filter(|d| !d.is_empty()));
    Sugestao {
        valor: format!("/{}", c.nome),
        titulo: format!("/{}", c.nome),
        detalhe,
        // Sem acento e minúscula, como o digitado.
        chave: crate::dados::chave(&c.nome),
        busca: crate::dados::chave(&c.descricao),
        peso: c.nome.len(),
        dica: Some(c.dica.clone()).filter(|d| !d.is_empty()),
        origem: rotulo_da_origem(&c.origem),
        skill: c.skill,
        arquivo: c.arquivo.clone(),
    }
}

/// O nome da família do modelo, sem a versão: "Opus 5.5" vira "Opus". É o que a caixa estreita
/// mostra no chip do modelo.
pub(super) fn nome_da_familia(curto: &str) -> &str {
    curto.split_whitespace().next().unwrap_or(curto)
}

/// O texto com a palavra `inicio..fim` trocada por `valor`, mais o lugar do cursor depois da troca.
/// Com um espaço logo depois da palavra, o cursor passa por ele; sem, entra um espaço (no fim do
/// texto também). É a regra que `trocar_palavra` aplica no campo (lá, sem refazer o texto, para as
/// pílulas ficarem).
#[cfg(test)]
pub(super) fn substituir_palavra(texto: &str, inicio: usize, fim: usize, valor: &str) -> (String, usize) {
    let fim = fim.min(texto.len());
    let inicio = inicio.min(fim);
    let tem_espaco = texto[fim..].chars().next().is_some_and(char::is_whitespace);
    let mut novo = format!("{}{valor}", &texto[..inicio]);
    let mut cursor = novo.len();
    if tem_espaco {
        cursor += texto[fim..].chars().next().map_or(0, char::len_utf8);
        novo.push_str(&texto[fim..]);
    } else {
        novo.push(' ');
        cursor += 1;
        novo.push_str(&texto[fim..]);
    }
    (novo, cursor)
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
pub(super) fn ficha_da_mencao(texto: &str, inteiro: &str, linhas: Option<(u32, u32)>, trecho: Option<&str>) -> InlineToken {
    InlineToken::new(super::mencao::link_da_mencao_com(inteiro, linhas, trecho), texto.to_string())
        .with_label(super::mencao::rotulo_da_mencao(inteiro, linhas))
}

/// O texto com as menções que existem em `base` já como pílulas, para o `set_value` (o rascunho
/// que volta para a caixa ao trocar de chat).
pub(super) fn conteudo_com_mencoes(texto: &str, base: &Path) -> InputContent {
    let mut conteudo = InputContent::new(texto.to_string());
    for (faixa, mencao) in super::mencao::mencoes_no_texto(texto) {
        let Some(inteiro) = super::links_arquivo::resolver_mencao(base, &mencao.caminho) else { continue };
        let ficha = ficha_da_mencao(&texto[faixa.clone()], &inteiro, mencao.linhas, mencao.trecho.as_deref());
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
    trocar_palavra(c, inicio, total, texto, ficha, window, cx);
}

/// Troca a palavra `inicio..fim` por `texto` (ou pela pílula `ficha`): o cursor vai para depois
/// dela e do espaço que a segue, que já existia ou é posto agora (`substituir_palavra`). A caixa
/// rola até o cursor.
pub(super) fn trocar_palavra(
    c: &mut TextareaState,
    inicio: usize,
    fim: usize,
    texto: &str,
    ficha: Option<InlineToken>,
    window: &mut Window,
    cx: &mut Context<TextareaState>,
) {
    let total = c.value().len();
    let (inicio, fim) = (inicio.min(total), fim.min(total));
    let feito = ficha.is_some_and(|f| c.replace_range_with_token(inicio..fim, f, window, cx).is_ok());
    if !feito {
        c.set_selected_range(inicio..fim, cx);
        c.replace(texto.to_string(), window, cx);
    }
    let atual = c.value().to_string();
    let depois = (inicio + texto.len()).min(atual.len());
    if atual[depois..].chars().next().is_some_and(char::is_whitespace) {
        let passo = atual[depois..].chars().next().map_or(0, char::len_utf8);
        c.set_selected_range(depois + passo..depois + passo, cx);
    } else {
        c.set_selected_range(depois..depois, cx);
        c.insert(" ", window, cx);
    }
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
fn nomes_dos_niveis(estreita: bool) -> [String; 6] {
    let mut nomes = ESFORCOS.map(|(_, n)| crate::i18n::traduzir(n).into_owned());
    if estreita {
        nomes[5] = tr!("Máx").to_string();
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

/// O agente e o modelo do chat por extenso, para a linha do Este chat ("Claude · Opus 5.5").
pub(super) fn modelo_por_extenso(motor: &Motor, chat: &Chat) -> String {
    match chat.agente {
        crate::modelo::Agente::Codex => match chat.modelo.as_deref() {
            Some(m) => format!("Codex · {}", crate::agente_codex::nome_do_modelo(m)),
            None => "Codex".to_string(),
        },
        outro if outro.acp() => match chat.modelo.as_deref() {
            Some(m) => format!("{} · {}", outro.nome(), crate::agente_gemini::nome_do_modelo_de(outro, m)),
            None => outro.nome().to_string(),
        },
        _ => {
            let efetivo = chat.modelo.clone().unwrap_or_else(|| motor.config.modelo_chats.clone());
            let em_uso = chat.modelo_atual.as_deref().filter(|m| !m.is_empty());
            format!("Claude · {}", rotulo_modelo(&efetivo, em_uso))
        }
    }
}

/// O nível de esforço do chat (o índice em `ESFORCOS`), o que o automático usa agora e se o
/// modelo usa esforço.
pub(super) fn esforco_do_chat(motor: &Motor, chat: &Chat) -> (usize, Option<usize>, bool) {
    let atual = indice_esforco(chat.esforco.as_deref().unwrap_or(""));
    let padrao = motor.esforco_automatico(chat).map(|e| indice_esforco(&e)).filter(|i| *i != 0);
    (atual, padrao, usa_esforco(chat.modelo_atual.as_deref()))
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
        1_000_000 => tr!("1 milhão").to_string(),
        n if n > 1_000_000 && n % 1_000_000 == 0 => tr!("{n} milhões", n = n / 1_000_000).to_string(),
        n => tr!("{n} mil", n = n / 1000).to_string(),
    }
}

/// "200 mil tokens", "1 milhão de tokens".
pub(super) fn tokens(n: u64) -> String {
    let quantia = milhares(n);
    if n >= 1_000_000 && n % 1_000_000 == 0 {
        tr!("{quantia} de tokens", quantia = quantia).to_string()
    } else {
        tr!("{quantia} tokens", quantia = quantia).to_string()
    }
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

/// Da beirada da coluna à borda de fora da caixa: a margem da coluna de leitura (24) ou, na caixa
/// estreita, 16. A coluna de leitura inteira (`mensagens::coluna`) e a faixa de cima da caixa ("N
/// arquivos mudados") medem daqui, para o texto de tudo cair na mesma coluna. A faixa usava 16
/// fixo e, quando a coluna foi a 24 (17/11/2026), ficou 8 px à esquerda do texto da caixa e 8
/// antes do enviar (folha dimensional, 23/11/2026).
pub(super) fn margem_da_caixa(_estreita: bool) -> Rems {
    // 8 do cartão em qualquer largura (protótipo Linear, 25/11/2026).
    esp::E8
}

/// O enviar e o parar da caixa: 26,67, o do protótipo Linear (25/11/2026).
const ALTURA_DO_ENVIAR: f32 = 26.67;

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

/// O que o Enter (e o Enviar) faz com a mensagem escrita enquanto o chat trabalha.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Enter {
    /// Entra no turno que está rodando (`dirige_o_turno`).
    Dirige,
    /// Há tantos pedidos de permissão abertos: nega e manda o texto ao Claude como a resposta
    /// (`Motor::enviar_responde_pedido`).
    RespondePedido(usize),
    /// O que está aberto é uma pergunta com opções (AskUserQuestion): o texto é a resposta a ela.
    RespondePergunta,
    /// Espera a vez na fila.
    Fila,
}

pub(super) fn enter_no_turno(motor: &Motor, chat: &str) -> Enter {
    match motor.pedidos_abertos(chat) {
        0 if dirige_o_turno(motor, chat) => Enter::Dirige,
        0 => Enter::Fila,
        _ if motor.so_perguntas_abertas(chat) => Enter::RespondePergunta,
        pedidos => Enter::RespondePedido(pedidos),
    }
}

/// A dica do Enviar com o chat trabalhando: o que o Enter vai fazer com a mensagem. `reforco`: o
/// Ultrathink ou o Ultracode ligado na caixa, que valem para o turno inteiro.
fn dica_do_enviar(enter: Enter, reforco: bool) -> SharedString {
    match enter {
        Enter::Fila => tr!("Enviar\nEntra na fila: este turno não recebe mensagem agora"),
        _ if reforco => tr!("Enviar\nO Ultrathink e o Ultracode valem para o turno inteiro: esta espera a vez na fila"),
        Enter::Dirige => tr!("Enviar\nEntra no turno que está rodando, sem pará-lo\nCtrl+Enter manda depois, na fila"),
        Enter::RespondePedido(1) => tr!("Enviar\nNega o pedido aberto e manda isto ao Claude como resposta\nCtrl+Enter manda depois, na fila"),
        Enter::RespondePedido(_) => tr!("Enviar\nNega os pedidos abertos e manda isto ao Claude como resposta\nCtrl+Enter manda depois, na fila"),
        Enter::RespondePergunta => tr!("Enviar\nResponde à pergunta com isto, sem escolher opção\nCtrl+Enter manda depois, na fila"),
    }
}

/// A dica do Enviar com "Enter vai para a fila": o Enter espera a vez, o Ctrl+Enter entra no turno.
fn dica_do_enviar_na_fila(enter: Enter) -> Option<SharedString> {
    matches!(enter, Enter::Dirige).then_some(
        tr!("Enviar\nVai para a fila e espera este turno acabar\nCtrl+Enter entra no turno que está rodando, sem pará-lo"),
    )
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
    /// uma moldura preta de raio 20; dentro, com 2 de folga, o campo opaco com o texto
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
        let raio_caixa = raio::moldura();
        let foco_da_caixa = composer.read(cx).focus_handle(cx);
        let caixa_vazia = composer.read(cx).value().trim().is_empty();
        // "Letra da conversa também na caixa de mensagem".
        let fator_da_caixa = self.motor.as_ref().map_or(1., |m| {
            let c = &m.read(cx).config;
            crate::tema::fator_da_letra(crate::opcoes::letra_da_caixa(c.letra_na_caixa, c.letra_da_conversa))
        });
        // Com "Percorrer as enviadas", a caixa com o texto trazido: as setas voltam a percorrer.
        let recordando = self.paineis[ix].percurso.ativo(&composer.read(cx).value());
        let com_menu = self.menu_da_caixa(ix, chat, cx).is_some();
        // O miolo é concêntrico com a moldura de 20, não com o cartão da conversa: o raio de fora
        // menos a borda de 1 e a folga de 2 (eram 4; dono, 16/11/2026: "diminui as bordas"). A
        // conta tirava só a folga e o canto de dentro saía 1 px mais aberto (2 a 150%), visto na
        // folha dimensional de 23/11/2026. A borda é 1 px em qualquer zoom; a folga cresce com ele.
        // Protótipo Linear (25/11/2026): a moldura de 17 com 3 de folga, e o miolo de 14 dentro.
        let raio_miolo = raio::cartao();
        // A linha de estado: com o chat trabalhando, o que ele faz agora (M15); parada, as teclas.
        // A linha de baixo só existe com o chat trabalhando (o que ele faz agora e o Esc). Parada,
        // a caixa não tem mais a linha das teclas (dono, 13/11/2026: "tira essa parte de baixo").
        let estado = self.linha_de_estado(motor, chat, painel_id, window, cx);

        let citacao = self.citacoes.get(&chat.id).map(|trecho| self.cartao_da_citacao(&chat.id, trecho, window, cx).into_any_element());
        let tem_anexos = !anexos.is_empty();
        let pilulas = self.anexos_pendentes(&chat.id, anexos, window, cx);
        // O miolo: o campo opaco, um degrau mais claro que a moldura. Sem a linha de luz no topo
        // (dono, 23/11/2026: "tira esse brilho de cima das coisas").
        let miolo = div()
            .relative()
            .flex()
            .flex_col()
            .min_w_0()
            .rounded(raio_miolo)
            .bg(gradiente_quente())
            // 8 em cima, 6 na direita, 5 embaixo; na esquerda, só os 10 do campo do kit, e os
            // controles a 6 da borda (os 10 do protótipo menos os 4 que eles voltam).
            .pt(linear::CAIXA_ALTO)
            .pr(linear::CAIXA_DIREITA)
            .pb(linear::CAIXA_BAIXO)
            // A citação mora no miolo, quase na coluna do texto (os 10 do campo do kit; 8 na escala).
            .children(citacao.map(|cartao| div().ml(esp::E8).mb(esp::E6).child(cartao)))
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
                    } else if recordando {
                        "Composer Recordando"
                    } else {
                        "Composer"
                    })
                    .child(super::vista_mensagens::na_letra(
                        fator_da_caixa,
                        Textarea::new(&composer)
                            .appearance(false)
                            // 14/21, sem o respiro de cima e de baixo do kit: o campo começa nos 8
                            // do miolo (protótipo Linear, 25/11/2026).
                            .respiro_vertical(px(0.))
                            .text_size(r(14.))
                            .line_height(r(21.))
                            // As menções de arquivo são pílulas dentro do texto; o clique abre o
                            // arquivo à direita, nas linhas.
                            .token(|ficha, _, _| {
                                let id = ficha.token().id().clone();
                                match super::mencao::ler_link_da_mencao(&id) {
                                    Some((inteiro, linhas)) => super::mencao::pilula_de_mencao(
                                        SharedString::from(format!("ficha-{id}")),
                                        &inteiro,
                                        linhas,
                                        super::mencao::trecho_do_link(&id).as_deref(),
                                        false,
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
                                // HTML copiado (navegador, documento): vira Markdown, com o texto
                                // puro por baixo, para o Ctrl+Z voltar a ele.
                                if caminhos.is_empty()
                                    && let (Some(plano), Some(html)) = (item.text(), item.metadata())
                                    && let Some(markdown) = super::colar_html::markdown_da_colagem(&plano, html)
                                {
                                    let campo = campo_colado.clone();
                                    window.on_next_frame(move |window, cx| {
                                        let _ = campo.update(cx, |c, cx| colar_com_markdown(c, &plano, &markdown, window, cx));
                                    });
                                    return true;
                                }
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
                    )),
            )
            .child(div().h(esp::E4))
            .child(div().pl(esp::E6).child(barra));
        // Na altura, os 2 de cima igualam a folga de baixo da moldura. Quando o turno começa, ela
        // entra com fade (sem deslize: ela abre o lugar dela na moldura, e a caixa cresce 22 px).
        let estado_novo = estado.is_some() && self.paineis[ix].ja_vistos.borrow_mut().novo("linha-de-estado");
        let linha_de_estado = estado.map(|(estado_esquerda, estado_direita)| {
            // 22 de altura, 8 na esquerda e 9 na direita, em 12 (protótipo Linear, 25/11/2026).
            let linha = h_flex()
                .h(r(22.))
                .pl(linear::ESTADO_ESQUERDA)
                .pr(linear::ESTADO_DIREITA)
                .gap(esp::E8)
                .justify_between()
                .metadado()
                .text_color(Cores::tenue())
                .child(h_flex().flex_1().min_w_0().gap(esp::E12).overflow_hidden().children(estado_esquerda))
                .child(h_flex().flex_none().gap(esp::E12).children(estado_direita));
            movimento::entrada(SharedString::from(format!("entra-estado-{painel_id}")), estado_novo, movimento::De::Parado, linha)
        });
        // A moldura preta: o campo e a linha de estado. O vão entre as peças, a folga e a linha
        // são da caixa: o clique ali não leva o foco para a janela. O campo e os botões tratam o
        // deles antes.
        let tem_estado = linha_de_estado.is_some();
        // O foco: `t` vai a 1 em 180 ms com a caixa focada e volta a 0 em 120. Com ele, o aro de
        // 2 de metal em volta do miolo, por dentro da moldura preta (o `.aro` do protótipo, com
        // `inset: -2px` na caixa): o miolo opaco por cima deixa só o aro à vista, e a moldura
        // continua lisa por fora. Antes o aro ficava em volta da moldura inteira, um brilho por
        // fora do preto (dono, 26/11/2026: "tira o shader de volta da parte preta").
        let miolo = com_estado(move |window, cx| {
            let t = movimento::valor(
                ("aro-caixa", painel_id as usize),
                if focada { 1f32 } else { 0. },
                if focada { movimento::MOVER } else { movimento::COR },
                Curva::Mover,
                window,
                cx,
            );
            div()
                .relative()
                .flex()
                .flex_col()
                .min_w_0()
                .child(super::componentes::metal::aro_de_metal(raio_miolo, tam(window, 2.), t))
                .child(miolo)
                .into_any_element()
        });
        let moldura = v_flex()
            .min_w_0()
            // O hover da caixa acende os controles que recuam nos painéis de fundo.
            .group(grupo_da_caixa(painel_id))
            .rounded(raio_caixa)
            .bg(Cores::preto())
            // 3 de folga; embaixo, a linha de estado quando há (protótipo Linear, 25/11/2026).
            .p(linear::MOLDURA_FOLGA)
            .when(tem_estado, |s| s.pb_0())
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if !window.default_prevented() {
                    window.focus(&foco_da_caixa, cx);
                    window.prevent_default();
                }
            })
            // Os anexos moram na casca, em cima do campo (a referência do dono, 13/11/2026: a
            // "prompt box" da 21st.dev): pílulas com a miniatura, e a imagem abre na ilha.
            .children(tem_anexos.then(|| div().px(esp::E4).pt(esp::E2).child(pilulas)))
            .child(miolo)
            .children(linha_de_estado);
        let caixa = moldura;

        div()
            .map(|s| super::mensagens::coluna(s, estreita))
            .pb(esp::E6)
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
            .on_action(cx.listener(move |this, _: &AbrirSkill, window, cx| this.abrir_skill_marcada(painel_id, window, cx)))
            .on_action(cx.listener({
                let chat = chat.id.clone();
                move |this, _: &AceitarSugestao, window, cx| this.aceitar_sugestao(&chat, painel_id, window, cx)
            }))
            // Seta para cima na caixa vazia: o texto da última mensagem volta para ela, como num
            // terminal (o keymap só a manda para cá no contexto "Vazia").
            .on_action(cx.listener(move |this, _: &super::EditarUltima, window, cx| {
                // Com "Percorrer as enviadas", a seta na caixa vazia traz as enviadas; o corrigir
                // fica no lápis do balão e no Ctrl+↑.
                if this.motor.as_ref().is_some_and(|m| m.read(cx).config.seta_acima_percorre) {
                    this.percorrer_enviadas(painel_id, true, window, cx)
                } else {
                    this.editar_ultima(painel_id, window, cx)
                }
            }))
            .on_action(cx.listener(move |this, _: &super::CorrigirUltima, window, cx| {
                this.editar_ultima(painel_id, window, cx)
            }))
            .on_action(cx.listener(move |this, _: &super::RecordarAnterior, window, cx| {
                this.percorrer_enviadas(painel_id, true, window, cx)
            }))
            .on_action(cx.listener(move |this, _: &super::RecordarSeguinte, window, cx| {
                this.percorrer_enviadas(painel_id, false, window, cx)
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
        // A fileira dos controles: os chips de 26 e o enviar de 26,67 (protótipo Linear, 25/11/2026).
        let altura = ALTURA_DO_ENVIAR;
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
        let grupo = |e: AnyElement| div().flex_none().ml(linear::CONTROLES_VAO).child(e);
        // O esforço que não cabe inteiro ao lado do modelo fica só com o ícone, em vez de ir para a
        // segunda linha escondida e sumir (`decidir_esforco`).
        let compacto = self.paineis[ix].esforco_compacto;
        let medida_do_modelo: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::default();
        let medida_da_linha: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::default();
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
                    .on_prepaint({
                        let caixa = medida_da_linha.clone();
                        move |limites, _, _| caixa.set(Some(limites))
                    })
                    .child(grupo(em_repouso("repouso-modelo", self.chip_modelo(motor, chat, ix, b, window, cx))).on_prepaint({
                        let modelo = medida_do_modelo.clone();
                        move |limites, _, _| modelo.set(Some(limites))
                    }))
                    .child(
                        grupo(em_repouso("repouso-esforco", self.chip_esforco(motor, chat, ix, b, compacto.is_some(), window, cx)))
                            .on_prepaint(medir_esforco(
                                cx.entity().downgrade(),
                                painel_id,
                                medida_do_modelo,
                                medida_da_linha,
                                compacto,
                                estreita,
                                self.paineis[ix].modelo_so_icone,
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(linear::CONTROLES_VAO)
                    .ml(linear::CONTROLES_VAO)
                    .children(self.anel_do_contexto(chat, b, cx))
                    // O clipe mora no "…" (protótipo Linear, 25/11/2026).
                    .child(em_repouso("repouso-mais", self.botao_mais(motor, chat, b, window, cx).into_any_element()))
                    .children(self.botoes_de_envio(b, vazio, enter_no_turno(motor, &chat.id), envios, cx)),
            )
    }

    /// O enviar claro (apagado com a caixa vazia) e, com o chat rodando, o parar. O botão grande
    /// guarda o mesmo id como enviar e como parar: o pulso do envio (`envios`) toca nele mesmo
    /// quando o chat já começou a rodar e ele virou o parar.
    fn botoes_de_envio(
        &self,
        b: &Barra,
        vazio: bool,
        enter: Enter,
        envios: u64,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        // O enviar e o parar de 26,67, no azul-marinho do escolhido (protótipo Linear, 25/11/2026).
        let (grande, pequeno) = (ALTURA_DO_ENVIAR, 26.);
        let painel = b.painel;
        let parar = || {
            let (motor, chat) = (self.motor.clone(), b.chat.clone());
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                if let Some(m) = &motor {
                    m.update(cx, |m, cx| m.parar(&chat, cx));
                }
            }
        };
        let dica_parar = tr!("Parar esta conversa");
        let principal = |icone: Option<Lucide>, dica: SharedString| {
            // O enviar (e o parar), o par do "+" da lista.
            BotaoRedondo::new(("enviar", painel as usize), icone.map(Into::into), dica)
                .variante(VarianteRedonda::Primario)
                .diametro(grande)
                .pulso(envios)
        };
        let enviar = |dica: SharedString| {
            principal(Some(Lucide::ArrowUp), dica)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.ativar_painel(painel, cx);
                    if this.enter_manda_depois(painel, false, cx) {
                        this.enviar_depois(painel, window, cx);
                    } else {
                        this.enviar_do_painel(painel, window, cx);
                    }
                }))
                .into_any_element()
        };
        // Com o turno rodando, a mensagem entra nele sem pará-lo (o steering do Claude Code); com
        // um pedido de permissão aberto, ela é a resposta ao pedido. O que vale para o turno
        // inteiro, e tudo o que não é um turno em andamento, vai pela fila.
        let reforco = self.ultrathink.contains(&b.chat) || self.ultracode.contains(&b.chat);
        let na_fila = self.motor.as_ref().is_some_and(|m| m.read(cx).config.enter_na_fila);
        let dica_dirigir = match dica_do_enviar_na_fila(enter).filter(|_| na_fila && !reforco) {
            Some(dica) => dica,
            None => dica_do_enviar(enter, reforco),
        };
        match envio(b.rodando, vazio) {
            Envio::Vazio => vec![principal(Some(Lucide::ArrowUp), tr!("Escreva uma mensagem")).vazio(true).into_any_element()],
            Envio::Enviar => vec![enviar(tr!("Enviar"))],
            Envio::Parar => vec![
                principal(None, dica_parar.clone())
                    .dica_acao(Box::new(PararTurno), Some("Caixa"))
                    .on_click(parar())
                    .into_any_element(),
            ],
            Envio::PararEEnviar => vec![
                BotaoRedondo::new(("parar", painel as usize), None, dica_parar.clone())
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
        let cor_arco = cor_da_faixa(fracao, Cores::destaque_texto());
        // Linear (25/11/2026): de novo um anel, fino, com o número ao lado ("essa parte da
        // porcentagem de contexto tá feia", sobre o medidor deitado de 18/11). O anel inteiro e
        // na cor do da barra de baixo: com a fenda ele lia como o "carregando" (dono, 26/11/2026:
        // "trocar esse ícone do contexto pra igual o de baixo"). Âmbar e vermelho continuam
        // avisando perto do fim.
        let barra = Anel::new(("anel-contexto", b.painel as usize), fracao).diametro(14.).espessura(2.).inteiro().cor(cor_arco);
        let cor_numero = if cor_arco == Cores::destaque_texto() { Cores::tenue() } else { cor_arco };
        // O Codex e o Gemini não dizem o custo: o popover mostra os tokens.
        let o_que_abre = if chat.agente == crate::modelo::Agente::Claude { tr!("O custo, o modelo e o Compactar") } else { tr!("Os tokens, o modelo e o Compactar") };
        let dica: SharedString = tr!("Contexto: {usados} de {janela}\n{o_que_abre}", usados = milhares(usados), janela = tokens(janela), o_que_abre = o_que_abre);
        // O anel de 14 e a porcentagem em 12, com 5 entre eles e 4 dos lados (protótipo Linear).
        let _ = estreita;
        let gatilho = Button::new(("contexto", b.painel as usize))
            .custom(variante_fantasma(cx))
            .h(r(26.))
            .px(esp::E4)
            .rounded(raio::de(26.))
            .tooltip(dica)
            .child(
                h_flex()
                    .gap(linear::CONTEXTO_VAO)
                    .child(barra)
                    .child(div().text_size(r(12.)).line_height(r(16.)).tabular().text_color(cor_numero).child(format!("{numero}%"))),
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
                        tr!("Compactar a conversa"),
                        TamanhoBotao::Barra,
                        window,
                        cx,
                    )
                    .tooltip(SharedString::from(crate::i18n::traduzir(DICA_DO_COMPACTAR)))
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

    /// O trecho citado, dentro do miolo, em cima do campo (protótipo Linear, `.citacao`): um recuo
    /// no preto da moldura, sem borda nem sombra, com o fio azul de 2 solto da beirada, o trecho no
    /// tênue em até duas linhas (12,5/17) e o X de 20. Sem rótulo: o fio e o lugar já dizem que é
    /// uma resposta. Entra subindo, como no protótipo; outro trecho entra de novo (dono, 27/11/2026:
    /// "melhora o design disso", sobre o cartão com borda, sombra e o "Respondendo a").
    fn cartao_da_citacao(&self, chat: &str, trecho: &str, _: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let chat = chat.to_string();
        let marca = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            trecho.hash(&mut h);
            h.finish()
        };
        let cartao = h_flex()
            .relative()
            .items_start()
            .gap(esp::E6)
            .pl(esp::E12)
            .pr(esp::E4)
            .py(esp::E4)
            .rounded(raio::de(11.))
            .bg(Cores::preto())
            .text_size(r(12.5))
            .line_height(r(17.))
            .text_color(Cores::tenue())
            .child(div().absolute().left(r(5.)).top(esp::E6).bottom(esp::E6).w(px(2.)).rounded_full().bg(Cores::destaque_texto()))
            .child(div().flex_1().min_w_0().line_clamp(2).text_ellipsis().child(trecho.to_string()))
            .child(
                BotaoRedondo::new(SharedString::from(format!("tirar-citacao-{chat}")), Some(Lucide::X.into()), tr!("Tirar a citação"))
                    .variante(VarianteRedonda::Plano)
                    .diametro(20.)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.citacoes.remove(&chat);
                        cx.notify();
                    })),
            );
        movimento::subir(cartao, SharedString::from(format!("citacao-{marca}")), movimento::De::Baixo)
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
            (self.aba_do_modelo, self.abertos_do_modelo) = (None, None);
            seletores::preparar_modelos(&lista, &motor, &chat, None, None, None, window, cx);
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
            // Entra com fade e 8 px de baixo, saindo da caixa.
            s.child(deferred(anchored().anchor(Anchor::BottomLeft).snap_to_window_with_margin(px(8.)).child(movimento::entrar(
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
                            .child(img(caminho).size_full().rounded(raio::CARTAO).object_fit(ObjectFit::Contain))
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
                "entra-ilha",
                movimento::De::Baixo,
            ))).with_priority(1))
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
                        .child(img(caminho.clone()).size_full().rounded(raio::CONTROLE).object_fit(ObjectFit::Cover))
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
                            // Corta no meio: a extensão fica à vista.
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_middle()
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
                            .child(lido.tamanho.map(|t| t.to_string()).unwrap_or_else(|| tr!("sumiu").to_string())),
                    ))
                    .child(
                        BotaoRedondo::new(("tirar-anexo", i), Some(Lucide::X.into()), tr!("Remover o anexo"))
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
                    .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            }))
    }

    /// As sugestões da palavra sob o cursor: os comandos de barra e as skills (o texto começa com
    /// "/") e as menções do "@" (rodada de uso, item 1.2).
    pub(super) fn sugestoes_da_caixa(&self, chat: &Chat, texto: &str, cursor: usize) -> Option<(char, usize, Vec<Sugestao>)> {
        let Gatilho { sinal, digitado, inicio, .. } = gatilho_em(texto, cursor)?;
        if sinal == '@' {
            let lista = self.mencoes_da_caixa(chat, digitado, inicio);
            return (!lista.is_empty()).then_some((sinal, inicio, lista));
        }
        let itens: Vec<Sugestao> = match sinal {
            '/' => self.catalogo_de(chat).iter().map(sugestao_do_comando).collect(),
            _ => return None,
        };
        let lista = menu_sugestoes(itens, digitado, None);
        (!lista.is_empty()).then_some((sinal, inicio, lista))
    }

    /// O menu "@" na própria caixa (dono, 16/11/2026: rodada de uso, item 1.2), no lugar do diálogo
    /// "Mencionar arquivo", que tirava o cursor da caixa e engolia o "@claude" da dupla. Na ordem:
    /// o @claude e o @codex (só com a dupla ligada e só no começo da mensagem, a única posição que
    /// o motor entende) e os arquivos e as pastas do chat, com o arquivo aberto à direita no topo.
    /// Enquanto a pasta é lida, uma linha "Lendo a pasta…".
    fn mencoes_da_caixa(&self, chat: &Chat, digitado: &str, inicio: usize) -> Vec<Sugestao> {
        let mut lista: Vec<Sugestao> = Vec::new();
        if let Some(parceiro) = chat.parceiro.as_ref().map(|p| p.agente).filter(|_| inicio == 0) {
            for agente in [chat.agente, parceiro] {
                let nome = agente.nome().to_lowercase();
                if nome.starts_with(&digitado.to_lowercase()) {
                    lista.push(Sugestao {
                        valor: format!("@{nome}"),
                        titulo: format!("@{nome}"),
                        detalhe: Some(tr!("Só o {agente} responde a esta mensagem", agente = agente.nome()).to_string()),
                        chave: nome.clone(),
                        busca: nome,
                        ..Sugestao::default()
                    });
                }
            }
        }
        let Some((arquivos, pastas)) = self.arquivos_e_pastas_lidos(&chat.cwd).filter(|_| !self.lendo_arquivos(&chat.cwd)) else {
            lista.push(Sugestao { titulo: tr!("Lendo a pasta…").to_string(), ..Sugestao::default() });
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
                valor: valor_da_mencao(&caminho),
                titulo,
                detalhe: (!pasta.is_empty()).then(|| pasta.to_string()),
                chave: nome.to_lowercase(),
                busca: caminho.to_lowercase(),
                peso: caminho.len(),
                ..Sugestao::default()
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
        let campo = painel.composer.read(cx);
        let (texto, cursor) = (campo.value().to_string(), campo.cursor());
        self.sugestoes_da_caixa(chat, &texto, cursor)
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
        let novo = atual.saturating_add_signed(passo).min(n - 1);
        self.paineis[ix].indice_menu = novo;
        // A lista rola junto: o marcado nunca sai da vista.
        self.paineis[ix].rolagem_menu.scroll_to_item(novo);
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

    /// A seta ↑ (`para_tras`) ou ↓ no percurso das enviadas deste chat (`Config.seta_acima_percorre`):
    /// põe o texto na caixa, com o cursor no fim, sem enviar nem mexer na conversa.
    fn percorrer_enviadas(&mut self, painel: u64, para_tras: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.paineis.iter().position(|p| p.id == painel) else {
            return;
        };
        let (Some(chat), Some(motor)) = (self.paineis[ix].chat.clone(), self.motor.clone()) else {
            return;
        };
        let enviadas = motor.read(cx).chat(&chat).map(|c| super::recordar::enviadas_do_chat(&c.itens)).unwrap_or_default();
        let composer = self.paineis[ix].composer.clone();
        let caixa = composer.read(cx).value().to_string();
        let novo = if para_tras {
            self.paineis[ix].percurso.subir(&enviadas, &caixa)
        } else {
            self.paineis[ix].percurso.descer(&enviadas, &caixa)
        };
        let Some(texto) = novo else {
            cx.propagate();
            return;
        };
        composer.update(cx, |c, cx| {
            c.set_value(texto.clone(), window, cx);
            let fim = texto.len();
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

    /// Ctrl+O com o menu "/" à vista: abre no painel da direita o arquivo da skill marcada (o
    /// `SKILL.md`), para ler o que ela faz antes de usar.
    fn abrir_skill_marcada(&mut self, painel: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.paineis.iter().position(|p| p.id == painel) else {
            return;
        };
        let Some((_, _, lista)) = self.menu_do_painel_ix(ix, cx) else {
            cx.propagate();
            return;
        };
        let marcado = self.paineis[ix].indice_menu.min(lista.len().saturating_sub(1));
        match lista.get(marcado).and_then(|s| s.arquivo.clone()) {
            Some(arquivo) => self.abrir_arquivo_da_skill(&arquivo, window, cx),
            None => self.avisar_na_estreita(painel, tr!("Este comando não tem arquivo para abrir").to_string(), window, cx),
        }
    }

    /// Abre o arquivo de uma skill no painel da direita, mesmo fora da pasta do chat.
    fn abrir_arquivo_da_skill(&mut self, arquivo: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(pasta), Some(nome)) = (arquivo.parent(), arquivo.file_name()) else {
            return;
        };
        self.abrir_arquivo(pasta.to_path_buf(), nome.to_string_lossy().into_owned(), window, cx);
    }

    /// O menu das sugestões, acima da caixa. Entra como os popovers: fade e 6 px vindos de baixo,
    /// do lado da caixa. O marcado (o primeiro, até as setas andarem) é o que o Enter e o Tab
    /// escolhem. A lista rola: o "/" mostra o catálogo inteiro (cada linha com a descrição, a
    /// dica de argumento, o selo "skill" e a origem; o ícone à direita abre o `SKILL.md`).
    fn menu_comandos(&self, chat: &Chat, ix: usize, cx: &Context<Self>) -> impl IntoElement + use<> {
        let painel = self.paineis[ix].id;
        let sugestoes = self.menu_da_caixa(ix, chat, cx);
        let marcado = self.paineis[ix].indice_menu;
        let rolagem = self.paineis[ix].rolagem_menu.clone();
        div().relative().when_some(sugestoes, |s, (sinal, inicio, lista)| {
            let marcado = marcado.min(lista.len().saturating_sub(1));
            let linhas = lista.into_iter().enumerate().map(|(i, sugestao)| {
                let valor = sugestao.valor.clone();
                let arquivo = sugestao.arquivo.clone();
                div()
                    .id(("sugestao-caixa", i))
                    .flex()
                    .flex_none()
                    .flex_col()
                    .justify_center()
                    .min_h(r(32.))
                    .py(esp::E4)
                    .px(esp::E8)
                    .rounded(raio::CONTROLE)
                    .cursor_pointer()
                    .when(i == marcado, |s| s.bg(Cores::selecao()))
                    .when(i != marcado, |s| s.hover(|s| s.bg(Cores::hover())))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(esp::E8)
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .max_w(relative(0.6))
                                    .truncate()
                                    .map(|d| if sinal == '/' { d.mono().text_color(Cores::codigo()) } else { d.corpo_ui().text_color(Cores::texto()) })
                                    .child(sugestao.titulo.clone()),
                            )
                            .when_some(sugestao.dica.clone(), |s, dica| {
                                s.child(div().flex_1().min_w_0().truncate().mono_pequeno().text_color(Cores::tenue()).child(dica))
                            })
                            .when(sugestao.dica.is_none(), |s| s.child(div().flex_1()))
                            .when(sugestao.skill, |s| {
                                s.child(
                                    div()
                                        .flex_none()
                                        .px(esp::E4)
                                        .rounded(raio::de(4.))
                                        .border_1()
                                        .border_color(Cores::borda())
                                        .etiqueta()
                                        .text_color(Cores::tenue())
                                        .child("skill"),
                                )
                            })
                            .when_some(sugestao.origem.clone(), |s, origem| {
                                s.child(div().flex_none().max_w(relative(0.35)).truncate().etiqueta().text_color(Cores::tenue()).child(origem))
                            })
                            .when_some(arquivo, |s, arquivo| {
                                s.child(
                                    div()
                                        .id(("abrir-skill", i))
                                        .flex_none()
                                        .p(esp::E4)
                                        .rounded(raio::de(6.))
                                        .cursor_pointer()
                                        .text_color(Cores::tenue())
                                        .hover(|s| s.bg(Cores::hover()).text_color(Cores::texto()))
                                        .child(super::mensagens::icone_de(Lucide::ScrollText, 14.))
                                        .dica_no_ponteiro(|window, cx| Tooltip::new(tr!("Ver o arquivo da skill (Ctrl+O)")).build(window, cx))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.abrir_arquivo_da_skill(&arquivo, window, cx);
                                            cx.stop_propagation();
                                        })),
                                )
                            }),
                    )
                    .when_some(sugestao.detalhe.clone(), |s, d| {
                        s.child(div().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(d))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| this.escolher_sugestao(painel, inicio, &valor, window, cx)))
            });
            let menu = div()
                .flex()
                .flex_col()
                .w_full()
                .p(esp::E8)
                .flutuante(cx)
                .child(
                    div()
                        .id(("lista-comandos", painel as usize))
                        .flex()
                        .flex_col()
                        .gap(esp::E2)
                        .max_h(r(340.))
                        .overflow_y_scroll()
                        .track_scroll(&rolagem)
                        .children(linhas),
                )
                .with_animation(
                    ("entra-comandos", painel as usize),
                    movimento::animacao(movimento::ENTRAR, Curva::Entrar),
                    |menu, t| menu.opacity(t).top(r(6. * (1. - t))),
                );
            // O absoluto fica por fora: o deslize mexe no `top` de quem está dentro.
            s.child(div().absolute().bottom(esp::E4).left_0().w_full().max_w(r(460.)).child(menu))
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
        let (atual, cursor) = {
            let campo = composer.read(cx);
            (campo.value().to_string(), campo.cursor())
        };
        let (inicio, fim) = gatilho_em(&atual, cursor).map_or((inicio, atual.len()), |g| (g.inicio, g.fim));
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
            .map(|c| c.trim_matches('"'))
            .filter(|c| c.contains(['.', '/']))
            .zip(cwd)
            .and_then(|(caminho, cwd)| super::links_arquivo::resolver_mencao(&cwd, caminho))
            .map(|inteiro| ficha_da_mencao(valor, &inteiro, None, None));
        composer.update(cx, |c, cx| trocar_palavra(c, inicio, fim, valor, ficha, window, cx));
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    /// Os nomes dos comandos e skills que o chat oferece.
    pub(super) fn comandos_de(&self, chat: &Chat) -> Vec<String> {
        self.catalogo_de(chat).into_iter().map(|c| c.nome).collect()
    }

    /// Os comandos e skills do chat, cada um com o que se sabe dele (descrição, dica de argumento,
    /// origem, arquivo).
    pub(super) fn catalogo_de(&self, chat: &Chat) -> Vec<crate::claude::ComandoOferecido> {
        use crate::claude::ComandoOferecido;
        let so_nome = |nome: &str| ComandoOferecido { nome: nome.to_string(), ..Default::default() };
        // Fora do Claude, os que o Vitral conhece: o `/clear` e, no Codex, o `/compact`, o `/review`
        // e as skills da pasta (`comandos_fora_do_claude`).
        if let Some(lista) = comandos_fora_do_claude(chat) {
            return lista.iter().map(|n| so_nome(n)).collect();
        }
        // O que o Claude Code disse que oferece naquela pasta (perguntado com o chat aberto, e de
        // novo a cada 10 minutos), com os detalhes; sem resposta dele ainda, os fixos. Depois que
        // a sessão do chat abriu, os nomes valem os dela, que é quem manda no que existe.
        let da_pasta = crate::claude::catalogo_da_pasta(&chat.cwd).unwrap_or_default();
        let mut lista: Vec<ComandoOferecido> = if !chat.comandos.is_empty() {
            chat.comandos
                .iter()
                .map(|nome| {
                    da_pasta.iter().find(|c| c.nome == *nome).cloned().unwrap_or_else(|| ComandoOferecido {
                        descricao: crate::claude::descricao_do_comando(nome).unwrap_or_default(),
                        ..so_nome(nome)
                    })
                })
                .collect()
        } else if !da_pasta.is_empty() {
            da_pasta
        } else {
            COMANDOS_INICIAIS.iter().map(|c| so_nome(c)).collect()
        };
        if !lista.iter().any(|c| c.nome == "clear") {
            lista.insert(0, so_nome("clear"));
        }
        // O `/conta` é do Vitral: troca a conta do Claude deste chat (26/11/2026).
        if !lista.iter().any(|c| c.nome == "conta") {
            lista.insert(1.min(lista.len()), ComandoOferecido { descricao: "Troca a conta do Claude deste chat (/conta nome, automática ou padrão)".into(), ..so_nome("conta") });
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
        } else if self.maximizado.is_some() {
            // Painel maximizado e nada a parar: o Esc devolve o arranjo.
            self.sair_do_maximizado(cx);
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
            self.avisar_na_estreita(painel, tr!("Modo: {modo}", modo = crate::i18n::traduzir(nome_do_modo(proximo))).to_string(), window, cx);
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
            prompt: Some(tr!("Anexar")),
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

/// Os comandos de barra de um chat do Codex ou do Gemini (`None` no Claude, que tem os do Claude
/// Code). Sempre o `/clear`, que é do Vitral; no Codex, o que ele disse que oferece na pasta no
/// último turno (o `/compact`, o `/review` e as skills, `agente_codex::servidor`), ou os dois
/// primeiros antes de ele dizer.
fn comandos_fora_do_claude(chat: &Chat) -> Option<Vec<String>> {
    let do_agente: Vec<String> = match chat.agente {
        crate::modelo::Agente::Claude => return None,
        crate::modelo::Agente::Codex if chat.comandos.is_empty() => {
            crate::agente_codex::servidor::COMANDOS.iter().map(|c| c.to_string()).collect()
        }
        crate::modelo::Agente::Codex => chat.comandos.clone(),
        // O Gemini e os ACP novos: só os do Vitral.
        _ => Vec::new(),
    };
    let mut lista = vec!["clear".to_string()];
    // O `/conta` do Vitral também troca a conta do Codex (26/11/2026).
    if chat.agente == crate::modelo::Agente::Codex {
        lista.push("conta".to_string());
    }
    for comando in do_agente {
        if !lista.contains(&comando) {
            lista.push(comando);
        }
    }
    Some(lista)
}

fn descricao_comando(comando: &str) -> Option<SharedString> {
    Some(match comando {
        "clear" => tr!("Limpa a conversa e começa do zero"),
        "conta" => tr!("Troca a conta deste chat (/conta nome ou padrão)"),
        "compact" => tr!("Resume a conversa para liberar contexto"),
        "context" => tr!("Mostra o uso do contexto"),
        "usage" => tr!("Uso do plano"),
        "init" => tr!("Cria o CLAUDE.md do projeto"),
        "review" => tr!("Revisa as mudanças"),
        "security-review" => tr!("Revisão de segurança das mudanças"),
        "simplify" => tr!("Simplifica o código alterado"),
        "model" => tr!("Troca o modelo"),
        "effort" => tr!("Troca o esforço"),
        "code-review" => tr!("Revisão de código"),
        "debug" => tr!("Ajuda a depurar"),
        "verify" => tr!("Verifica o trabalho"),
        _ => return None,
    })
}

/// Uma regra de "permitir sempre" como aparece no menu do modo.
fn descricao_regra(regra: &str) -> String {
    if regra == "edicoes" {
        return tr!("Edições de arquivos neste chat").to_string();
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

/// Mede o chip do esforço depois do layout e liga ou desliga o modo só com o ícone; na caixa
/// estreita mede também se o nome da família ainda cabe no chip do modelo (`decidir_modelo`).
fn medir_esforco(
    janela: WeakEntity<Janela>,
    painel: u64,
    modelo: Rc<Cell<Option<Bounds<Pixels>>>>,
    linha: Rc<Cell<Option<Bounds<Pixels>>>>,
    compacto: Option<Pixels>,
    estreita: bool,
    modelo_so_icone: Option<Pixels>,
) -> impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static {
    move |esforco, window, _| {
        let Some(modelo) = modelo.get() else {
            return;
        };
        let novo = decidir_esforco(compacto, modelo, esforco, linha.get());
        let novo_modelo = if estreita { decidir_modelo(modelo_so_icone, modelo, esforco, linha.get()) } else { None };
        if novo == compacto && novo_modelo == modelo_so_icone {
            return;
        }
        window.on_next_frame(move |_, cx| {
            let _ = janela.update(cx, |this, cx| {
                if let Some(p) = this.paineis.iter_mut().find(|p| p.id == painel) {
                    p.esforco_compacto = novo;
                    p.modelo_so_icone = novo_modelo;
                    cx.notify();
                }
            });
        });
    }
}

/// O chip do modelo da caixa estreita com o nome da família (`None`) ou só com a marca
/// (`Some(largura que a linha pedia com o nome)`). Mesmo sinal do esforço: o chip seguinte caiu
/// para a linha escondida. Volta com o nome quando a linha tem a largura que ele pedia, mais 16 px
/// de folga para não ficar indo e voltando.
pub(super) fn decidir_modelo(
    so_icone: Option<Pixels>,
    modelo: Bounds<Pixels>,
    esforco: Bounds<Pixels>,
    linha: Option<Bounds<Pixels>>,
) -> Option<Pixels> {
    let quebrou = esforco.origin.y > modelo.origin.y + px(2.);
    match (so_icone, linha) {
        (None, Some(linha)) if quebrou => Some(modelo.right() - linha.left() + esforco.size.width + px(4.)),
        (None, _) => None,
        (Some(pedia), Some(linha)) if linha.size.width >= pedia + px(16.) => None,
        (Some(_), _) => so_icone,
    }
}

/// O esforço da caixa inteiro (`None`) ou só com o ícone (`Some(largura que ele pede inteiro)`).
/// O grupo do modelo e do esforço quebra linha com a segunda escondida: inteiro, o esforço que
/// não cabia ia para ela e sumia (dono, 24/11/2026: "o chat fica sumindo o esforço dele"). Abaixo
/// da linha do modelo é o sinal de que não coube; volta inteiro quando a sobra à direita do
/// modelo couber a largura que ele pedia, com 16 px de folga para não ficar indo e voltando.
pub(super) fn decidir_esforco(
    compacto: Option<Pixels>,
    modelo: Bounds<Pixels>,
    esforco: Bounds<Pixels>,
    linha: Option<Bounds<Pixels>>,
) -> Option<Pixels> {
    let quebrou = esforco.origin.y > modelo.origin.y + px(2.);
    match compacto {
        None if quebrou => Some(esforco.size.width),
        None => None,
        Some(inteiro) => {
            let Some(linha) = linha else {
                return compacto;
            };
            let sobra = linha.right() - modelo.right();
            if !quebrou && sobra >= inteiro + px(16.) { None } else { compacto }
        }
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
        return em_uso.unwrap_or_else(|| tr!("Padrão").to_string());
    }
    match MODELOS.iter().find(|(v, _)| *v == escolhido) {
        Some((_, nome)) => em_uso
            .filter(|m| m.to_lowercase().starts_with(escolhido))
            .unwrap_or_else(|| nome.to_string()),
        // Conversas antigas guardaram o nome completo do modelo, que não está na lista.
        None => nome_curto_modelo(escolhido),
    }
}

/// Cola o texto puro e, numa segunda edição, troca-o pelo Markdown: o Ctrl+Z desfaz só a troca e
/// deixa o texto puro (a colagem em duas edições é de propósito; o kit guarda cada uma).
fn colar_com_markdown(c: &mut TextareaState, plano: &str, markdown: &str, window: &mut Window, cx: &mut Context<TextareaState>) {
    c.insert(plano, window, cx);
    let fim = c.cursor();
    let inicio = fim.saturating_sub(plano.len());
    c.set_selected_range(inicio..fim, cx);
    c.replace(markdown.to_string(), window, cx);
    let cursor = c.cursor();
    c.set_selected_range(cursor..cursor, cx);
    c.set_scroll_offset(point(px(0.), px(-1.0e6)), cx);
}

/// Imagens coladas viram arquivos na pasta dos anexos (`anexos/` nos dados); arquivos copiados do
/// gerenciador entram pelo caminho. Com texto junto, quem cola é o texto: no Windows o GPUI entrega
/// o texto e a imagem no mesmo item (o Excel põe um desenho das células junto), e no Linux e no
/// macOS ele já entrega só o texto.
fn anexos_da_area_de_transferencia(item: &ClipboardItem, pasta: &Path) -> Vec<PathBuf> {
    let com_texto = item.entries().iter().any(|e| matches!(e, ClipboardEntry::String(s) if !s.text().trim().is_empty()));
    let mut caminhos = Vec::new();
    for entrada in item.entries() {
        match entrada {
            ClipboardEntry::ExternalPaths(lista) => caminhos.extend(lista.paths().iter().cloned()),
            ClipboardEntry::Image(_) if com_texto => {}
            ClipboardEntry::Image(imagem) => {
                let Some((extensao, bytes)) = imagem_para_anexo(imagem) else {
                    continue;
                };
                let arquivo = pasta.join(format!("colado-{}.{extensao}", crate::modelo::novo_id()));
                if std::fs::create_dir_all(pasta).is_ok() && std::fs::write(&arquivo, &*bytes).is_ok() {
                    caminhos.push(arquivo);
                }
            }
            ClipboardEntry::String(_) => {}
        }
    }
    caminhos
}

/// PNG, JPEG, GIF e WebP vão como vieram. BMP e TIFF viram PNG: o print do Windows (Win+Shift+S,
/// PrintScreen) chega como CF_DIB, que o GPUI entrega como BMP, e o Ctrl+V não fazia nada (dono,
/// 26/11/2026: "parece que tá bugado no Windows a parte de colar imagem").
fn imagem_para_anexo(imagem: &Image) -> Option<(&'static str, std::borrow::Cow<'_, [u8]>)> {
    use std::borrow::Cow;
    let direto = match imagem.format {
        ImageFormat::Png => Some("png"),
        ImageFormat::Jpeg => Some("jpg"),
        ImageFormat::Gif => Some("gif"),
        ImageFormat::Webp => Some("webp"),
        _ => None,
    };
    if let Some(extensao) = direto {
        return Some((extensao, Cow::Borrowed(imagem.bytes.as_slice())));
    }
    // `::image`: o `use gpui_kit::*` do arquivo traz outro `image` para o escopo.
    let formato = match imagem.format {
        ImageFormat::Bmp => ::image::ImageFormat::Bmp,
        ImageFormat::Tiff => ::image::ImageFormat::Tiff,
        _ => return None,
    };
    let decodificada = ::image::load_from_memory_with_format(&imagem.bytes, formato).ok()?;
    let mut png = Vec::new();
    decodificada.write_to(&mut std::io::Cursor::new(&mut png), ::image::ImageFormat::Png).ok()?;
    Some(("png", Cow::Owned(png)))
}

/// O texto depois da barra, enquanto ainda é o nome de um comando.
fn prefixo_de(composer: &Entity<TextareaState>, cx: &App) -> Option<String> {
    let campo = composer.read(cx);
    let texto = campo.value().to_string();
    let g = gatilho_em(&texto, campo.cursor())?;
    // Só enquanto o comando é o texto inteiro: com argumento depois, o Enter envia.
    (g.sinal == '/' && g.fim == texto.len()).then(|| g.digitado.to_string())
}

#[cfg(test)]
mod testes_sugestoes {
    use super::{Sugestao, completar, comandos_fora_do_claude, escolha_do_enter, gatilho, menu_sugestoes as menu_com_limite};

    /// O menu de antes: no máximo oito (o limite do "@" e do "#").
    fn menu_sugestoes(itens: Vec<Sugestao>, digitado: &str) -> Vec<Sugestao> {
        menu_com_limite(itens, digitado, Some(8))
    }

    #[test]
    fn o_codex_tem_o_compact_o_review_e_as_skills_no_menu() {
        use crate::modelo::{Agente, Chat};
        let mut chat = Chat::novo("c".into(), "t", std::path::PathBuf::from("/p"));
        assert_eq!(comandos_fora_do_claude(&chat), None, "o Claude tem os do Claude Code");
        chat.agente = Agente::Codex;
        assert_eq!(comandos_fora_do_claude(&chat).unwrap(), ["clear", "conta", "compact", "review"]);
        chat.comandos = vec!["compact".into(), "review".into(), "revisar-pr".into(), "clear".into()];
        assert_eq!(comandos_fora_do_claude(&chat).unwrap(), ["clear", "conta", "compact", "review", "revisar-pr"]);
        chat.agente = Agente::Gemini;
        chat.comandos.clear();
        assert_eq!(comandos_fora_do_claude(&chat).unwrap(), ["clear"]);
    }

    fn sugestao(valor: &str, chave: &str, busca: &str, peso: usize) -> Sugestao {
        Sugestao {
            valor: valor.into(),
            titulo: valor.into(),
            detalhe: None,
            chave: chave.into(),
            busca: busca.into(),
            peso,
            ..Sugestao::default()
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
        Enter, Envio, LINHAS_CAIXA, MODOS, com_claude, decidir_esforco, decidir_modelo, dica_do_enviar, envio, gastar_chave, indice_esforco, linhas_da_caixa,
        milhares, nomes_dos_niveis, proximo_modo, rotulo_modelo, tokens, usa_esforco, uso_do_contexto,
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
    fn o_esforco_que_nao_cabe_fica_so_com_o_icone_e_volta_com_lugar() {
        use gpui_kit::{Bounds, point, px, size};
        let caixa = |x: f32, y: f32, w: f32| Bounds { origin: point(px(x), px(y)), size: size(px(w), px(32.)) };
        let modelo = caixa(200., 10., 220.);
        let linha = Some(caixa(180., 10., 420.));
        // Inteiro e na mesma linha do modelo: fica inteiro.
        assert_eq!(decidir_esforco(None, modelo, caixa(424., 10., 150.), linha), None);
        // Inteiro e na linha de baixo (a escondida): só o ícone, lembrando que pedia 150.
        assert_eq!(decidir_esforco(None, modelo, caixa(180., 42., 150.), linha), Some(px(150.)));
        // Só o ícone, sobra 180 à direita do modelo (600 - 420): 150 + 16 cabe, volta inteiro.
        assert_eq!(decidir_esforco(Some(px(150.)), modelo, caixa(424., 10., 32.), linha), None);
        // Sobra pouca: continua só com o ícone, sem ficar indo e voltando.
        let estreita = Some(caixa(180., 10., 330.));
        assert_eq!(decidir_esforco(Some(px(150.)), modelo, caixa(424., 10., 32.), estreita), Some(px(150.)));
        // Sem a medida da linha ainda: não muda.
        assert_eq!(decidir_esforco(Some(px(150.)), modelo, caixa(424., 10., 32.), None), Some(px(150.)));
    }

    /// O nome da família no chip do modelo da caixa estreita: some quando o esforço cai para a linha
    /// escondida e volta quando a linha tem a largura que ele pedia (mais 16 de folga).
    #[test]
    fn o_nome_da_familia_some_quando_nao_cabe_e_volta_com_lugar() {
        use gpui_kit::{Bounds, point, px, size};
        let caixa = |x: f32, y: f32, w: f32| Bounds { origin: point(px(x), px(y)), size: size(px(w), px(32.)) };
        let modelo = caixa(200., 10., 110.);
        let linha = Some(caixa(180., 10., 200.));
        // O esforço (32) cabe ao lado do modelo (termina em 310; 310 + 4 + 32 < 380): fica o nome.
        assert_eq!(decidir_modelo(None, modelo, caixa(314., 10., 32.), linha), None);
        // O esforço caiu para a linha de baixo: só a marca, lembrando o que a linha pedia
        // (310 - 180 + 32 + 4 = 166).
        assert_eq!(decidir_modelo(None, modelo, caixa(180., 42., 32.), linha), Some(px(166.)));
        // Linha de 200 com o chip só com a marca: 166 + 16 cabe, o nome volta.
        assert_eq!(decidir_modelo(Some(px(166.)), caixa(200., 10., 60.), caixa(264., 10., 32.), linha), None);
        // Linha de 170: não cabe com folga, fica só com a marca.
        let curta = Some(caixa(180., 10., 170.));
        assert_eq!(decidir_modelo(Some(px(166.)), caixa(200., 10., 60.), caixa(264., 10., 32.), curta), Some(px(166.)));
        // Sem a medida da linha ainda: não muda.
        assert_eq!(decidir_modelo(Some(px(166.)), modelo, caixa(264., 10., 32.), None), Some(px(166.)));
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

    /// O critério do pedido: com um pedido de permissão aberto, a dica diz que o Enter nega e
    /// manda o texto como resposta (antes dizia "Entra na fila"); o Ctrl+Enter segue sendo a fila.
    #[test]
    fn a_dica_do_enviar_diz_o_que_o_enter_faz() {
        assert_eq!(
            dica_do_enviar(Enter::RespondePedido(1), false),
            "Enviar\nNega o pedido aberto e manda isto ao Claude como resposta\nCtrl+Enter manda depois, na fila"
        );
        assert_eq!(
            dica_do_enviar(Enter::RespondePedido(3), false),
            "Enviar\nNega os pedidos abertos e manda isto ao Claude como resposta\nCtrl+Enter manda depois, na fila"
        );
        assert_eq!(
            dica_do_enviar(Enter::Dirige, false),
            "Enviar\nEntra no turno que está rodando, sem pará-lo\nCtrl+Enter manda depois, na fila"
        );
        assert_eq!(dica_do_enviar(Enter::Fila, false), "Enviar\nEntra na fila: este turno não recebe mensagem agora");
        // O Ultrathink e o Ultracode valem para o turno inteiro: a mensagem espera a vez, e o
        // pedido continua aberto.
        let reforco = "Enviar\nO Ultrathink e o Ultracode valem para o turno inteiro: esta espera a vez na fila";
        assert_eq!(dica_do_enviar(Enter::Dirige, true), reforco);
        assert_eq!(dica_do_enviar(Enter::RespondePedido(1), true), reforco);
        assert_eq!(dica_do_enviar(Enter::Fila, true), "Enviar\nEntra na fila: este turno não recebe mensagem agora");
    }

    /// Com a pergunta com opções aberta, escrever é responder a ela: a dica não fala em negar.
    #[test]
    fn a_dica_do_enviar_com_a_pergunta_aberta() {
        assert_eq!(
            dica_do_enviar(Enter::RespondePergunta, false),
            "Enviar\nResponde à pergunta com isto, sem escolher opção\nCtrl+Enter manda depois, na fila"
        );
        let reforco = "Enviar\nO Ultrathink e o Ultracode valem para o turno inteiro: esta espera a vez na fila";
        assert_eq!(dica_do_enviar(Enter::RespondePergunta, true), reforco);
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

/// O menu na posição do cursor, o "/" completo e a caixa estreita (Paseo, rodada de 24/11/2026).
#[cfg(test)]
mod testes_do_cursor_e_do_catalogo {
    use super::{
        Gatilho, Sugestao, gatilho_em, menu_sugestoes, nome_da_familia, substituir_palavra, sugestao_do_comando, valor_da_mencao,
    };
    use crate::claude::{ComandoOferecido, Origem};

    fn g(sinal: char, digitado: &str, inicio: usize, fim: usize) -> Option<Gatilho<'_>> {
        Some(Gatilho { sinal, digitado, inicio, fim })
    }

    #[test]
    fn o_menu_abre_para_a_palavra_sob_o_cursor() {
        // No fim da palavra, como sempre.
        assert_eq!(gatilho_em("veja @cent", 10), g('@', "cent", 5, 10));
        // No meio dela: o que vale é o digitado até o cursor, e a palavra inteira se troca.
        assert_eq!(gatilho_em("veja @cent agora", 8), g('@', "ce", 5, 10));
        // Depois do espaço, ou antes do sinal, não é a palavra do gatilho.
        assert_eq!(gatilho_em("veja @cent agora", 16), None);
        assert_eq!(gatilho_em("veja @cent", 5), None);
        // O "/" só vale na primeira palavra, mesmo com o cursor nela, no meio.
        assert_eq!(gatilho_em("/compact agora", 5), g('/', "comp", 0, 8));
        assert_eq!(gatilho_em("foo /ba", 7), None);
        // Texto com mais de uma linha: a palavra acaba na quebra.
        assert_eq!(gatilho_em("a\n@b\nc", 4), g('@', "b", 2, 4));
        // Cursor no meio de um caractere de dois bytes cai na borda de antes.
        assert_eq!(gatilho_em("#lój", 3), g('#', "l", 0, 5));
    }

    #[test]
    fn caminho_com_espaco_entre_aspas_tambem_abre_o_menu() {
        let texto = "ver @\"minha pasta/a b";
        assert_eq!(gatilho_em(texto, texto.len()), g('@', "minha pasta/a b", 4, texto.len()));
        // Com as aspas fechadas e o cursor dentro, a palavra vai até a aspa de fechar.
        let fechado = "ver @\"a b\" e";
        assert_eq!(gatilho_em(fechado, 9), g('@', "a b", 4, 10));
        // Depois do fechamento já é outra palavra.
        assert_eq!(gatilho_em(fechado, fechado.len()), None);
        assert_eq!(valor_da_mencao("minha pasta/a b.rs"), "@\"minha pasta/a b.rs\"");
        assert_eq!(valor_da_mencao("src/a.rs"), "@src/a.rs");
    }

    #[test]
    fn completar_no_meio_da_frase_troca_so_a_palavra() {
        // Com espaço depois da palavra, o cursor passa por ele sem pôr outro.
        assert_eq!(substituir_palavra("veja @ce agora", 5, 8, "@src/x.rs"), ("veja @src/x.rs agora".to_string(), 15));
        // No fim do texto, entra o espaço.
        assert_eq!(substituir_palavra("veja @ce", 5, 8, "@src/x.rs"), ("veja @src/x.rs ".to_string(), 15));
        // Colado em pontuação ou em outra palavra, entra o espaço também.
        assert_eq!(substituir_palavra("(@ce)", 1, 4, "@a.rs"), ("(@a.rs )".to_string(), 7));
        assert_eq!(substituir_palavra("/comp", 0, 5, "/compact"), ("/compact ".to_string(), 9));
    }

    fn comando(nome: &str, descricao: &str) -> ComandoOferecido {
        ComandoOferecido { nome: nome.into(), descricao: descricao.into(), ..Default::default() }
    }

    fn itens(comandos: &[ComandoOferecido]) -> Vec<Sugestao> {
        comandos.iter().map(sugestao_do_comando).collect()
    }

    #[test]
    fn o_menu_da_barra_mostra_todos_os_comandos() {
        let todos: Vec<ComandoOferecido> = (0..40).map(|i| comando(&format!("skill-{i:02}"), "faz algo")).collect();
        assert_eq!(menu_sugestoes(itens(&todos), "", None).len(), 40);
        assert_eq!(menu_sugestoes(itens(&todos), "skill", None).len(), 40);
        // Com limite (o "@" e o "#"), corta.
        assert_eq!(menu_sugestoes(itens(&todos), "", Some(8)).len(), 8);
    }

    #[test]
    fn a_ordem_segue_a_qualidade_do_casamento() {
        let todos = [
            comando("outro", "Revisa o código antes do merge"),
            comando("code-review", "Revisão em dois eixos"),
            comando("revisar", "Revisa o diff"),
            comando("rev", "O nome é exato"),
            comando("pre-revisao", "meio"),
        ];
        let nomes = |digitado: &str| -> Vec<String> { menu_sugestoes(itens(&todos), digitado, None).into_iter().map(|s| s.valor).collect() };
        // Exato, depois começa com, depois começa uma parte do nome (depois de "-"), depois só
        // contém no nome; quem casa só pela descrição vai por último.
        assert_eq!(nomes("rev"), ["/rev", "/revisar", "/code-review", "/pre-revisao", "/outro"]);
        // Sem acento nem maiúscula; o nome que contém vem antes de quem só tem na descrição.
        assert_eq!(nomes("REVISÃO"), ["/pre-revisao", "/code-review"]);
    }

    #[test]
    fn cada_comando_traz_selo_origem_e_dica() {
        let skill = ComandoOferecido {
            nome: "easypanel".into(),
            descricao: "Operar o EasyPanel".into(),
            dica: "deploy | env".into(),
            origem: Origem::Pessoal,
            skill: true,
            arquivo: Some("/x/SKILL.md".into()),
            ..Default::default()
        };
        let s = sugestao_do_comando(&skill);
        assert_eq!((s.valor.as_str(), s.titulo.as_str()), ("/easypanel", "/easypanel"));
        assert!(s.skill);
        assert_eq!(s.origem.as_deref(), Some("pessoal"));
        assert_eq!(s.dica.as_deref(), Some("deploy | env"));
        assert_eq!(s.detalhe.as_deref(), Some("Operar o EasyPanel"));
        assert_eq!(s.arquivo.as_deref(), Some(std::path::Path::new("/x/SKILL.md")));
        let plugin = ComandoOferecido { origem: Origem::Plugin("mattpocock-skills".into()), ..comando("mattpocock-skills:tdd", "TDD") };
        assert_eq!(sugestao_do_comando(&plugin).origem.as_deref(), Some("plugin mattpocock-skills"));
        let projeto = ComandoOferecido { origem: Origem::Projeto, ..comando("x", "") };
        assert_eq!(sugestao_do_comando(&projeto).origem.as_deref(), Some("projeto"));
        // Os do Claude Code e os de origem desconhecida não mostram origem.
        assert_eq!(sugestao_do_comando(&comando("clear", "")).origem, None);
        assert!(!sugestao_do_comando(&comando("clear", "")).skill);
    }

    #[test]
    fn a_caixa_estreita_guarda_o_nome_da_familia() {
        assert_eq!(nome_da_familia("Opus 5.5"), "Opus");
        assert_eq!(nome_da_familia("Fable 5.2"), "Fable");
        assert_eq!(nome_da_familia("Sonnet 4.5 (1M)"), "Sonnet");
        assert_eq!(nome_da_familia("Padrão"), "Padrão");
        assert_eq!(nome_da_familia("Codex"), "Codex");
        assert_eq!(nome_da_familia(""), "");
    }
}

#[cfg(test)]
mod testes_do_colar {
    use gpui_kit::{ClipboardEntry, ClipboardItem, ClipboardString, Image, ImageFormat};

    use super::anexos_da_area_de_transferencia;

    fn pasta() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("vitral-colar-{}", crate::modelo::novo_id()))
    }

    /// O print do Windows chega como BMP (CF_DIB) e vira um PNG anexado (26/11/2026: o Ctrl+V não
    /// fazia nada).
    #[test]
    fn print_do_windows_em_bmp_vira_png() {
        let bmp = crate::ui::componentes::metal::bmp_de_bgra(2, 2, &[0x40; 16]);
        let item = ClipboardItem::new_image(&Image::from_bytes(ImageFormat::Bmp, bmp));
        let anexos = anexos_da_area_de_transferencia(&item, &pasta());
        assert_eq!(anexos.len(), 1);
        assert_eq!(anexos[0].extension().and_then(|e| e.to_str()), Some("png"));
        assert!(std::fs::read(&anexos[0]).unwrap().starts_with(b"\x89PNG"));
    }

    /// Texto e imagem no mesmo item (células do Excel no Windows): cola o texto, sem anexo.
    #[test]
    fn texto_com_imagem_junto_cola_o_texto() {
        let png = Image::from_bytes(ImageFormat::Png, b"\x89PNG\r\n\x1a\n".to_vec());
        let item = ClipboardItem { entries: vec![ClipboardEntry::String(ClipboardString::new("a\tb".into())), ClipboardEntry::Image(png)] };
        assert!(anexos_da_area_de_transferencia(&item, &pasta()).is_empty());
    }
}
