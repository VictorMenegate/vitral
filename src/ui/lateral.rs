//! Lista da esquerda, numa peça de vidro de leitura, conforme a vista: a visão geral ("Esperando
//! você", "Rodando agora", "Fixadas" e os 15 mais recentes, com o projeto de cada um), um projeto
//! (o cabeçalho dele, as abas Conversas · Arquivos e as faixas de data) ou os Avulsos. Embaixo dos
//! chats, os ajudantes; a árvore de arquivos por pasta ligada (ou da cópia do chat aberto) e o
//! rodapé com os agentes rodando.

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
    AbaLateral, AlternarLista, Buscar, Filtro, Gaveta, Janela, NovaConversa, Vista,
    componentes::{
        BotaoRedondo, CabecalhoDeSecao, CartaoDeLista, barra_de_rolagem, ControleSegmentado, EstadoVazio,
        PreviaArrasto, QuadradoDeProjeto, Segmento, Selo, Simbolo, Sinal, TamanhoBotao, Tom,
        VarianteRedonda, aviso, aviso_com_acao, botao_fantasma, botao_icone, botao_primario, campo_de_busca_de,
        contagem, estado_visual, icone, motivo_curto, numero_painel, sinal_do_chat, variante_fantasma,
    },
    duracao, icone_projeto, menus,
    paineis::ArrastoChat,
    sem_arrastar, tempo_relativo, visao_geral,
};
use crate::{
    modelo::{Chat, EstadoTarefa, Execucao, Item, Papel, Tarefa, agora},
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
    /// Abaixo de 288: o trabalhador fica só com o ponto (sem o selo de pedido ou erro à direita)
    /// e, no hover, só com o "…" (que também tem Abrir ao lado).
    sem_rotulos: bool,
    /// Abaixo de 276: abas só com ícone e a prévia com o rótulo curto do motivo (o sinal já diz
    /// qual é; de um trabalhador, fica o número).
    estreita: bool,
    /// Abaixo de 260: a busca de arquivos diz só "Buscar" e "Retomar tudo" e "Arquivar estas"
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

/// Onde uma conversa de coordenador aparece na lista.
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

/// Um coordenador, os trabalhadores dele e o que a família espera de você.
type Familia<'a> = (&'a Chat, Vec<&'a Chat>, Option<Pendencia<'a>>);

/// Por que uma conversa está em "Esperando você" (a regra única fica no motor).
pub(super) use crate::motor::Espera as Motivo;
use crate::motor::{Pendencia as PendenciaDoMotor, alvo_da_familia as escolher_alvo, espera as motivo};

/// O que a família de um coordenador espera de você.
pub(super) struct Pendencia<'a> {
    /// Quem pede: o próprio coordenador ou um trabalhador.
    pub alvo: &'a Chat,
    pub motivo: Motivo,
    /// Outros da família que também têm pedido ou erro.
    pub outros: usize,
}

/// Trabalhadores não arquivados de um coordenador, na ordem das tarefas.
pub(super) fn ajudantes_de<'a>(motor: &'a Motor, coordenador: &str) -> Vec<&'a Chat> {
    let mut lista: Vec<&Chat> = motor
        .chats
        .iter()
        .filter(|t| t.pai.as_deref() == Some(coordenador) && !t.arquivado)
        .collect();
    lista.sort_by_key(|t| t.tarefa);
    lista
}

/// A pendência da família (nenhuma se o coordenador está arquivado).
pub(super) fn pendencia<'a>(
    motor: &Motor,
    coordenador: &'a Chat,
    trabalhadores: &[&'a Chat],
) -> Option<Pendencia<'a>> {
    if motor.arquivada(coordenador) {
        return None;
    }
    let familia: Vec<&Chat> = std::iter::once(coordenador).chain(trabalhadores.iter().copied()).collect();
    let motivos: Vec<Option<Motivo>> = familia
        .iter()
        .map(|c| {
            motivo(
                &motor.execucao(&c.id),
                c.papel.na_lista(),
                motor.nao_lido(c),
                motor.precisa_retomar(&c.id),
            )
            .filter(|_| !motor.arquivada(c))
        })
        .collect();
    let ix = escolher_alvo(&motivos)?;
    Some(Pendencia {
        alvo: familia[ix],
        motivo: motivos[ix]?,
        outros: motivos
            .iter()
            .enumerate()
            .filter(|(i, m)| *i != ix && matches!(m, Some(Motivo::Pedido | Motivo::Parada | Motivo::Erro)))
            .count(),
    })
}

/// A pendência que aparece para você, a mesma na lista, no sino e nos atalhos do trilho: a
/// resposta nova da conversa aberta não conta, porque você já está lendo.
pub(super) fn pendencia_visivel<'a>(
    motor: &Motor,
    coordenador: &'a Chat,
    trabalhadores: &[&'a Chat],
    selecionado: Option<&str>,
) -> Option<Pendencia<'a>> {
    let aberta = selecionado == Some(coordenador.id.as_str());
    pendencia(motor, coordenador, trabalhadores).filter(|p| !(aberta && p.motivo == Motivo::Nova))
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
            PendenciaDoMotor::Parada { motivo, .. } => Causa::Parada(motivo_curto(motivo, false)),
            PendenciaDoMotor::Erro { texto, .. } => Causa::Erro(texto.clone()),
            PendenciaDoMotor::Nova { .. } => return None,
        })
    }

    /// A de quem espera numa família da lista; a resposta nova não tem.
    fn da_espera(motor: &Motor, alvo: &Chat, motivo: Motivo) -> Option<Causa> {
        Some(match motivo {
            Motivo::Pedido => Causa::do_pedido(motor, &alvo.id),
            Motivo::Erro => Causa::Erro(match motor.execucao(&alvo.id) {
                Execucao::Erro(erro) => erro,
                _ => String::new(),
            }),
            Motivo::Parada => {
                Causa::Parada(motor.parada(&alvo.id).map(|m| motivo_curto(&m, false)).unwrap_or_default())
            }
            Motivo::Nova => return None,
        })
    }

    /// O pedido aberto de um chat: o plano ou a permissão.
    fn do_pedido(motor: &Motor, chat: &str) -> Causa {
        match motor.permissoes.iter().find(|x| x.chat == chat) {
            Some(x) if x.ferramenta == "ExitPlanMode" => Causa::Plano,
            Some(x) => Causa::Permissao(crate::claude::resumo_ferramenta(&x.ferramenta, &x.entrada)),
            None => Causa::Permissao(String::new()),
        }
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
/// rótulo ("Parada", "#5 parada"). A resposta nova não tem: o ponto azul e o título em forte já
/// dizem.
pub(super) struct MotivoDaPrevia {
    sinal: Sinal,
    rotulo: &'static str,
    /// Quem pede é um trabalhador: o rótulo leva o número dele.
    do_trabalhador: bool,
    tarefa: u64,
    /// Outros da família que também têm pedido ou erro.
    outros: usize,
}

impl MotivoDaPrevia {
    /// O motivo de uma causa e o detalhe que vai depois dele; `tarefa` quando quem pede é um
    /// trabalhador.
    pub(super) fn de(causa: &Causa, tarefa: Option<u64>, outros: usize) -> (MotivoDaPrevia, String) {
        let (sinal, rotulo, detalhe) = causa.partes();
        let motivo =
            MotivoDaPrevia { sinal, rotulo, do_trabalhador: tarefa.is_some(), tarefa: tarefa.unwrap_or_default(), outros };
        (motivo, detalhe)
    }

    fn rotulo(&self, curto: bool) -> String {
        rotulo_pendencia(self.rotulo, self.do_trabalhador.then_some(self.tarefa), self.outros, curto)
    }
}

/// A frase de uma pendência, a mesma na prévia da lista e no sino: "Parada · Vitral fechou",
/// "#5 parada · Vitral fechou" (de um trabalhador), "Permissão · Bash · npm test", "Plano para
/// aprovar", "Erro · a primeira linha".
pub(super) fn frase_da_pendencia(causa: &Causa, tarefa: Option<u64>) -> (Sinal, String) {
    let (motivo, detalhe) = MotivoDaPrevia::de(causa, tarefa, 0);
    let frase = match partes_da_previa(Some(&motivo), &detalhe, false) {
        (Some(rotulo), texto) if texto.is_empty() => rotulo,
        (Some(rotulo), texto) => format!("{rotulo} {texto}"),
        (None, texto) => texto,
    };
    (motivo.sinal, frase)
}

/// Motivo e detalhe da linha de baixo de um cartão em "Esperando você"; a resposta nova fica com
/// a última fala.
fn previa_pendencia(motor: &Motor, chat: &Chat, p: &Pendencia) -> (Option<MotivoDaPrevia>, String) {
    let Some(causa) = Causa::da_espera(motor, p.alvo, p.motivo) else {
        return (None, ultima_fala(chat));
    };
    let do_trabalhador = p.alvo.id != chat.id;
    let tarefa = do_trabalhador.then(|| p.alvo.tarefa.unwrap_or_default());
    let (motivo, detalhe) = MotivoDaPrevia::de(&causa, tarefa, p.outros);
    (Some(motivo), detalhe)
}

/// "Parada" no próprio coordenador, "#2 parada" quando é um trabalhador, e "(+N)" se mais gente
/// da família espera. O curto (lista estreita) fica só com o número do trabalhador e o "(+N)": o
/// sinal já diz o quê.
fn rotulo_pendencia(rotulo: &str, tarefa: Option<u64>, outros: usize, curto: bool) -> String {
    let quem = match (tarefa, curto) {
        (Some(n), true) => Some(format!("#{n}")),
        (Some(n), false) => Some(format!("#{n} {}", minuscula(rotulo))),
        (None, true) => None,
        (None, false) => Some(rotulo.to_string()),
    };
    let mais = (outros > 0).then(|| format!("(+{outros})"));
    quem.into_iter().chain(mais).collect::<Vec<_>>().join(" ")
}

/// O que a prévia mostra: o rótulo do motivo (o curto na lista estreita, quando há detalhe para
/// ele abrir mão) e o texto, com o " · " quando vem depois de um rótulo.
fn partes_da_previa(motivo: Option<&MotivoDaPrevia>, texto: &str, estreita: bool) -> (Option<String>, String) {
    let curto = estreita && !texto.is_empty();
    let rotulo = motivo.map(|m| m.rotulo(curto)).filter(|r| !r.is_empty());
    let texto = if rotulo.is_some() && !texto.is_empty() { format!("· {texto}") } else { texto.to_string() };
    (rotulo, texto)
}

fn minuscula(texto: &str) -> String {
    let mut letras = texto.chars();
    letras.next().map(|c| c.to_lowercase().chain(letras).collect()).unwrap_or_default()
}

fn maiuscula(texto: &str) -> String {
    let mut letras = texto.chars();
    letras.next().map(|c| c.to_uppercase().chain(letras).collect()).unwrap_or_default()
}

/// A última coisa dita na conversa.
fn ultima_fala(chat: &Chat) -> String {
    for item in chat.itens.iter().rev() {
        match item {
            Item::Assistente { texto, .. } => return primeira_linha(texto),
            Item::Usuario { texto, .. } => return format!("Você: {}", primeira_linha(texto)),
            Item::Entrega { de, texto, .. } => return format!("{de}: {}", primeira_linha(texto)),
            _ => {}
        }
    }
    match &chat.importada {
        Some(i) if !i.ultima_fala.is_empty() => i.ultima_fala.clone(),
        _ => "Nova conversa".into(),
    }
}

/// A linha de baixo de cada conversa: o que está acontecendo, ou a última coisa dita. Pedido,
/// erro e parada usam a frase das pendências.
fn previa(motor: &Motor, chat: &Chat) -> String {
    let frase = |causa: Causa, tarefa: Option<u64>| frase_da_pendencia(&causa, tarefa).1;
    match motor.execucao(&chat.id) {
        Execucao::EsperandoVoce => return frase(Causa::do_pedido(motor, &chat.id), None),
        Execucao::Erro(erro) => return frase(Causa::Erro(erro), None),
        Execucao::Rodando { .. } => {
            if let Some(p) = chat.tarefa.and_then(|t| motor.tarefa(t)).and_then(|t| t.progresso.as_deref()) {
                return maiuscula(p);
            }
            // As mensagens que o usuário dirigiu ao turno não mudam o que ele está fazendo: a
            // chamada de antes delas é a de agora.
            let mut itens = chat.itens.as_slice();
            let mut dirigidas = motor.dirigidas_no_turno(&chat.id);
            while dirigidas > 0 && matches!(itens.last(), Some(Item::Usuario { .. })) {
                itens = &itens[..itens.len() - 1];
                dirigidas -= 1;
            }
            if let Some(Item::Ferramenta { resumo, .. }) = itens.last() {
                return resumo.clone();
            }
            return "Pensando…".into();
        }
        Execucao::Parado => {}
    }
    if motor.esperando_vaga(&chat.id) {
        return "Na fila, esperando vaga".into();
    }
    if let Some(motivo) = motor.parada(&chat.id) {
        return frase(Causa::Parada(motivo_curto(&motivo, false)), None);
    }
    if let Some(t) = motor.tarefas_do(&chat.id).find(|t| motor.tarefa_parada(t)) {
        let em_revisao = t.estado == EstadoTarefa::EmRevisao;
        let porque = motivo_curto(t.parada.as_deref().unwrap_or_default(), em_revisao);
        return frase(Causa::Parada(porque), Some(t.id));
    }
    let ativas: Vec<&Tarefa> = motor
        .tarefas_do(&chat.id)
        .filter(|t| t.estado.ativa() && !motor.tarefa_parada(t))
        .collect();
    if !ativas.is_empty() {
        return atividade_das_tarefas(motor, &ativas);
    }
    ultima_fala(chat)
}

/// O que a prévia de um chat de projeto parado diz no lugar da última fala, com um ícone: a cópia
/// que não foi preparada, a fila do trabalho pesado, o pedido pronto para revisar, o que falta
/// juntar ou que acabou de ser juntado.
fn estado_do_chat(motor: &Motor, chat: &Chat) -> Option<(Lucide, Hsla, String)> {
    if chat.papel != Papel::Projeto || motor.execucao(&chat.id).rodando() || motor.esperando_vaga(&chat.id) {
        return None;
    }
    let t = chat.tarefa.and_then(|t| motor.tarefa(t))?;
    if t.falha_copia.is_some() {
        return Some((Lucide::TriangleAlert, Cores::ambar(), "A cópia não foi preparada".into()));
    }
    if motor.na_fila_pesada(t.id) {
        let texto = t.progresso.clone().unwrap_or_else(|| "na fila".into());
        return Some((Lucide::Clock, Cores::tenue(), maiuscula(&texto)));
    }
    match t.estado {
        EstadoTarefa::EmRevisao => Some((Lucide::Eye, Cores::ambar(), "Pronto para revisar".into())),
        EstadoTarefa::Conversa => match t.mudancas.as_ref().filter(|m| m.arquivos > 0) {
            // No modo PR as mudanças vão para um PR, não para a pasta.
            Some(m) if motor.config_da_pasta(&t.projeto, &motor.origem_da_tarefa(t)).github.entrega == crate::modelo::ModoEntrega::Pr => {
                Some((Lucide::GitPullRequest, Cores::tenue(), format!("Para o PR: +{} −{}", m.mais, m.menos)))
            }
            Some(m) => Some((Lucide::GitMerge, Cores::tenue(), format!("Para juntar: +{} −{}", m.mais, m.menos))),
            None if acabou_de_juntar(chat) => Some((Lucide::Check, Cores::tenue(), "Juntado".into())),
            None => None,
        },
        // O PR aberto, com os checks.
        EstadoTarefa::NoPr => t.pr.as_ref().map(|pr| {
            let checks = (pr.checks.total > 0).then(|| format!(" · {} checks", super::painel_pr::rotulo_checks(&pr.checks).curto));
            let cor = match pr.checks.estado {
                crate::github::EstadoChecks::Falhou => Cores::vermelho(),
                _ if t.pronto_para_atualizar => Cores::ambar(),
                _ => Cores::tenue(),
            };
            let texto = if t.pronto_para_atualizar {
                format!("Pronto para atualizar o PR #{}", pr.numero)
            } else {
                format!("PR #{} {}{}", pr.numero, pr.estado.nome(), checks.unwrap_or_default())
            };
            (Lucide::GitPullRequest, cor, texto)
        }),
        _ => None,
    }
}

/// A última coisa no chat é o aviso de que as mudanças entraram na pasta original.
fn acabou_de_juntar(chat: &Chat) -> bool {
    chat.itens
        .iter()
        .rev()
        .find(|i| !matches!(i, Item::Ferramenta { .. }))
        .is_some_and(|i| matches!(i, Item::Aviso { texto, .. } if texto.starts_with("As mudanças entraram em")))
}

/// O que as tarefas em andamento estão fazendo agora, pela última ferramenta de quem roda.
fn atividade_das_tarefas(motor: &Motor, ativas: &[&Tarefa]) -> String {
    let agora = ativas.iter().find_map(|t| {
        let trabalhador = motor.chat(&t.trabalhador)?;
        match trabalhador.itens.last() {
            Some(Item::Ferramenta { resumo, .. }) if motor.execucao(&t.trabalhador).rodando() => {
                Some(format!("#{} · {resumo}", t.id))
            }
            _ => t.progresso.as_ref().map(|p| format!("#{} · {p}", t.id)),
        }
    });
    let ajudantes = ativas.iter().any(|t| motor.chat(&t.trabalhador).is_some_and(|c| c.pai.is_some()))
        && motor.chat(&ativas[0].coordenador).is_some_and(|c| c.papel == Papel::Projeto);
    if ajudantes && agora.is_none() {
        return format!("{} rodando", contagem(ativas.len(), "ajudante", "ajudantes"));
    }
    texto_da_atividade(ativas.len(), agora)
}

/// "#3 · Edit · README.md", ou "2 rodando · #3 · Edit · README.md" com mais de um ajudante.
fn texto_da_atividade(n: usize, agora: Option<String>) -> String {
    match (n, agora) {
        (1, Some(a)) => a,
        (n, Some(a)) => format!("{n} rodando · {a}"),
        (n, None) => format!("{} rodando", contagem(n, "ajudante", "ajudantes")),
    }
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
        || c.itens.iter().any(|i| match i {
            Item::Usuario { texto, .. } | Item::Assistente { texto, .. } | Item::Entrega { texto, .. } => {
                texto.to_lowercase().contains(busca)
            }
            _ => false,
        })
}

/// Os coordenadores arquivados (eles ou o projeto) que casam com a busca (eles ou um
/// trabalhador), do mais recente para o mais antigo. Sem busca, nenhum: fora dela, as arquivadas
/// ficam em Arquivadas.
fn arquivadas_que_casam<'a>(
    chats: &'a [Chat],
    busca: &str,
    projeto: Option<&str>,
    arquivada: impl Fn(&Chat) -> bool,
) -> Vec<&'a Chat> {
    if busca.is_empty() {
        return Vec::new();
    }
    let mut lista: Vec<&Chat> = chats
        .iter()
        .filter(|c| c.papel.na_lista() && arquivada(c) && projeto.is_none_or(|p| c.projeto == p))
        .filter(|c| {
            casa_com_a_busca(c, busca)
                || chats
                    .iter()
                    .any(|t| t.pai.as_deref() == Some(c.id.as_str()) && !t.arquivado && casa_com_a_busca(t, busca))
        })
        .collect();
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

/// Onde a busca casou numa família cujo título não casa: a fala mais recente com o termo, do
/// coordenador ("Você: …", "Coordenador: …") ou, depois, de um trabalhador ("#2: …").
fn achado_na_familia(chat: &Chat, trabalhadores: &[&Chat], busca: &str) -> Option<(String, Range<usize>)> {
    let na_conversa = |c: &Chat, trabalhador: Option<u64>| {
        c.itens.iter().rev().find_map(|item| {
            let (quem, texto) = match item {
                Item::Usuario { texto, .. } => (Some("Você"), texto),
                Item::Entrega { de, texto, .. } => (Some(de.as_str()), texto),
                Item::Assistente { texto, .. } => (None, texto),
                _ => return None,
            };
            let (pedaco, faixa) = recorte(texto, busca)?;
            let prefixo = match (trabalhador, quem) {
                (Some(n), _) => format!("#{n}: "),
                (None, Some(quem)) => format!("{quem}: "),
                (None, None) => String::new(),
            };
            let deslocar = prefixo.len();
            Some((prefixo + &pedaco, faixa.start + deslocar..faixa.end + deslocar))
        })
    };
    na_conversa(chat, None)
        .or_else(|| trabalhadores.iter().find_map(|t| na_conversa(t, Some(t.tarefa.unwrap_or_default()))))
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

/// "3 rodando · 1 na fila": a pasta inteira, sem filtro; nada quando nada roda.
fn texto_agentes(rodando: usize, fila: usize) -> Option<String> {
    match (rodando, fila) {
        (0, 0) => None,
        (0, f) => Some(format!("{f} na fila")),
        (n, 0) => Some(format!("{n} rodando")),
        (n, f) => Some(format!("{n} rodando · {f} na fila")),
    }
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

/// "site", "site e loja", "site, loja e api".
fn lista_com_e(nomes: &[String]) -> String {
    match nomes {
        [] => String::new(),
        [um] => um.clone(),
        [inicio @ .., ultimo] => format!("{} e {ultimo}", inicio.join(", ")),
    }
}

/// Dica do "+N" do cabeçalho: o que espera você fora do projeto aberto.
fn dica_de_fora(n: usize, projetos: &[String]) -> String {
    let quantas = contagem(n, "chat esperando", "chats esperando");
    if projetos.is_empty() {
        format!("{quantas} você em outros projetos")
    } else {
        format!("{quantas} você em {}", lista_com_e(projetos))
    }
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

/// Um cartão que o teclado alcança: o chat, o coordenador (nos trabalhadores), a posição entre
/// os filhos da lista (para rolar até ele) e se o Enter também foca a caixa.
struct Navegavel {
    chat: String,
    coordenador: Option<String>,
    indice: usize,
    pendencia: bool,
}

/// Os filhos da lista de conversas, numa sequência só (assim a rolagem chega a cada um).
#[derive(Default)]
struct Montagem {
    itens: Vec<AnyElement>,
    navegaveis: Vec<Navegavel>,
    /// A visão geral é uma lista corrida: o que espera você, o que roda, as fixadas e as recentes,
    /// nessa ordem, sem o título de cada seção (o ponto do cartão e o alfinete já dizem qual é).
    corrida: bool,
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

    fn navegavel(&mut self, e: AnyElement, chat: &str, coordenador: Option<&str>, pendencia: bool) {
        self.navegaveis.push(Navegavel {
            chat: chat.to_string(),
            coordenador: coordenador.map(str::to_string),
            indice: self.itens.len(),
            pendencia,
        });
        self.itens.push(e);
    }
}

/// O que a aba Arquivos mostra: a pasta escolhida entre as do projeto (e a cópia do chat aberto),
/// os arquivos dela e, com busca, os que casam.
struct Arquivos {
    /// As pastas que dá para ver (rótulo, caminho), na ordem do seletor, e qual está à vista.
    fontes: Vec<(SharedString, PathBuf)>,
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
/// por " · ". Na lista estreita o rótulo fica curto: o número do trabalhador e o "(+N)"; o do
/// próprio coordenador sai, porque o sinal basta. O sino usa a mesma.
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
                    Tooltip::new("Buscar nos arquivos do projeto").build(window, cx)
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

/// Botão pequeno de texto no cabeçalho de uma seção ("Arquivar estas", "Retomar tudo"); na
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
    pasta_projeto: PathBuf,
    caminho: String,
    pasta: bool,
) -> PopupMenu {
    let completo = pasta_projeto.join(&caminho);
    let onde = if pasta {
        completo.clone()
    } else {
        completo.parent().map(Path::to_path_buf).unwrap_or_else(|| pasta_projeto.clone())
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
        // A aba Arquivos existe na vista de um projeto e na visão geral (onde mostra a pasta do chat
        // aberto); nos Avulsos, não.
        let sem_arquivos = self.vista == Vista::Avulsos;
        let conversas = self.aba == AbaLateral::Conversas || sem_arquivos;
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

        let cabecalho = self.cabecalho_da_lista(motor, &ctx, window, cx);
        let topo = match &self.vista {
            Vista::Geral => None,
            Vista::Projeto(p) => Some(self.cabecalho_projeto(motor_entidade, p, window, cx)),
            Vista::Avulsos => Some(self.cabecalho_avulsos(window)),
        };
        let abas = (!sem_arquivos).then(|| self.abas_da_lista(motor, &ctx, conversas));
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

        let enxuta = self.lista_enxuta(motor);
        let (linha_busca, busca_no_pe) = if enxuta { (None, Some(linha_busca)) } else { (Some(linha_busca), None) };
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
            // O trilho deitado no alto da lista: as duas peças viraram uma só, e o que era a
            // coluna de ícones à esquerda é a primeira fileira daqui. Também na gaveta, que é o
            // único lugar do logo (e do menu do Vitral) quando a lista não cabe ao lado.
            .map(|s| {
                if enxuta {
                    // Uma linha só no alto: o logo e as abas. A busca e o "+" moram no pé da
                    // lista (pedido do dono, 12/11/2026).
                    s.child(self.topo_enxuto(motor_entidade, abas, cx)).child(div().flex_none().h(esp::E4))
                } else {
                    s.child(self.trilho_deitado(motor_entidade, window, cx))
                        // Na visão geral não há título: o recolher fica na ponta da fileira de cima.
                        .when(self.vista != Vista::Geral, |s| s.child(cabecalho))
                        .children(topo)
                        .children(abas)
                        .children(linha_busca)
                }
            })
            .children(escopo)
            .child(conteudo)
            .children(rodape)
            // Na lista enxuta, a busca e o "+" fecham a lista, embaixo de tudo.
            .children(busca_no_pe.map(|b| b.mt(esp::E4).mb(esp::E8)))
            // Sem fundo nem aresta: a lista é a própria base da janela (duas camadas: a base com
            // a lista e a borda, e os cartões por cima). Na gaveta, a superfície é a da gaveta.
    }

    /// Cabeçalho de 44, no topo da peça (o centro fica em 22, como o logo e as abas): o título
    /// da visão geral ou, num projeto e nos Avulsos, o caminho de volta a ela; e o botão de recolher.
    fn cabecalho_da_lista(&self, motor: &Motor, ctx: &Contexto, window: &Window, cx: &Context<Self>) -> Stateful<Div> {
        let titulo = if self.vista == Vista::Geral {
            div()
                .flex_1()
                .min_w_0()
                .pl(esp::E8)
                .truncate()
                .titulo_cartao()
                .text_color(Cores::forte())
                .child("Visão geral")
                .into_any_element()
        } else {
            let janela = ctx.janela.clone();
            sem_arrastar()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .justify_start()
                .child(
                    botao_fantasma("voltar-a-visao-geral", Some(Lucide::ArrowLeft.into()), "Visão geral", TamanhoBotao::Barra, window, cx)
                        .tooltip("Voltar à visão geral, com todos os projetos")
                        .on_click(move |_, window, cx| {
                            let _ = janela.update(cx, |this, cx| this.abrir_vista(Vista::Geral, window, cx));
                        }),
                )
                .into_any_element()
        };
        // Fora da visão geral, o que espera você nos outros projetos: o "+N" leva de volta a ela.
        let fora = self.vista.filtro().and_then(|filtro| {
            let n = super::cartoes::precisa_com_o_mestre(motor).numero().saturating_sub(motor.precisa_de_voce(Some(filtro)).numero());
            (n > 0).then(|| {
                let outros: Vec<String> = motor
                    .projetos_visiveis()
                    .into_iter()
                    .filter(|p| p.id != filtro && motor.precisa_de_voce(Some(&p.id)).numero() > 0)
                    .map(|p| p.nome.clone())
                    .collect();
                let dica = SharedString::from(dica_de_fora(n, &outros));
                let janela = ctx.janela.clone();
                sem_arrastar()
                    .id("escopo-fora")
                    .flex_none()
                    .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                    .child(Selo::cor(format!("+{n}"), Cores::ambar()).clicavel("escopo-fora-selo", move |window, cx| {
                        let _ = janela.update(cx, |this, cx| this.abrir_vista(Vista::Geral, window, cx));
                    }))
            })
        });
        let recolher = botao_icone("recolher-lista", Lucide::PanelLeftClose, "Recolher a lista", TamanhoBotao::Barra, window, cx)
            .tooltip_with_action("Recolher a lista", &AlternarLista, None)
            .on_click(cx.listener(|this, _, window, cx| this.alternar_lista(window, cx)));
        self.arrastavel("arrastar-lateral")
            .flex_none()
            .h(altura::CABECALHO)
            .flex()
            .items_center()
            .gap(esp::E4)
            .child(titulo)
            .children(fora)
            .child(sem_arrastar().flex_none().child(recolher))
    }

    /// As abas Conversas · Arquivos do projeto, num segmentado (o ponto âmbar na de Conversas
    /// quando algo do projeto espera você e você está nos arquivos).
    fn abas_da_lista(&self, motor: &Motor, ctx: &Contexto, conversas: bool) -> Div {
        let pendentes = motor.precisa_de_voce(self.vista.filtro()).numero();
        let mut aba_conversas = Segmento::new("Conversas").icone(Lucide::MessagesSquare);
        if !conversas && pendentes > 0 {
            let dica = format!("{} você", contagem(pendentes, "chat esperando", "chats esperando"));
            aba_conversas = aba_conversas.ponto(Cores::ambar()).dica(dica);
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
        // O + abre um chat novo na hora, na pasta do chat aberto (como no VS Code); num projeto e
        // nos Avulsos, cria direto ali (com duas pastas, pergunta qual).
        let nova = conversas.then(|| match self.vista.filtro().map(str::to_string) {
            None => BotaoRedondo::new("nova-conversa", Some(Lucide::Plus.into()), "Novo chat")
                .dica_acao(Box::new(NovaConversa), None)
                .variante(VarianteRedonda::Metal)
                .diametro(40.)
                .on_click(cx.listener(|this, _, window, cx| this.novo_chat_direto(window, cx)))
                .into_any_element(),
            Some(projeto) if ctx.aperto.estreita => {
                BotaoRedondo::new("nova-conversa", Some(Lucide::Plus.into()), "Novo chat")
                    .dica_acao(Box::new(NovaConversa), None)
                    .variante(VarianteRedonda::Metal)
                    .diametro(40.)
                    .on_click(cx.listener(move |this, _, window, cx| this.novo_chat_em(&projeto, window, cx)))
                    .into_any_element()
            }
            Some(projeto) => botao_primario("nova-conversa", Some(Lucide::Plus.into()), "Novo chat", TamanhoBotao::Controle, window, cx)
                .flex_none()
                .tooltip_with_action("Novo chat", &NovaConversa, None)
                .on_click(cx.listener(move |this, _, window, cx| this.novo_chat_em(&projeto, window, cx)))
                .into_any_element(),
        });
        h_flex().flex_none().gap(esp::E8).mb(esp::E8).child(campo).children(nova)
    }

    /// Linha de escopo (36): nos Arquivos, de qual pasta (ou da cópia do chat aberto) e o ↻; nas
    /// Arquivadas, como sair.
    fn linha_escopo(
        &self,
        motor: &Motor,
        arquivos: Option<&Arquivos>,
        ctx: &Contexto,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        if let Some(a) = arquivos {
            let pasta = a.pasta.clone()?;
            let projeto = self.projeto_dos_arquivos(motor)?;
            let recarregar = {
                let pasta = pasta.clone();
                botao_icone("escopo-recarregar", Lucide::RefreshCw, "Recarregar a lista de arquivos", TamanhoBotao::Linha, window, cx)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.reler_arquivos(&pasta);
                        cx.notify();
                    }))
            };
            let escolha = if a.fontes.len() > 1 {
                let ix = a.fontes.iter().position(|(_, p)| *p == pasta).unwrap_or(0);
                let fontes: Vec<PathBuf> = a.fontes.iter().map(|(_, p)| p.clone()).collect();
                let segmentos: Vec<Segmento> = a
                    .fontes
                    .iter()
                    .map(|(rotulo, p)| Segmento::new(rotulo.clone()).dica(super::dialogos::com_til(p)))
                    .collect();
                let janela = ctx.janela.clone();
                ControleSegmentado::new("fonte-dos-arquivos", segmentos, ix, move |i, _, cx| {
                    let Some(p) = fontes.get(i).cloned() else { return };
                    let projeto = projeto.clone();
                    let _ = janela.update(cx, |this, cx| {
                        this.pasta_arquivos.insert(projeto, p);
                        cx.notify();
                    });
                })
                .largura_total(true)
                .compacto(true)
                .into_any_element()
            } else {
                div()
                    .flex_1()
                    .min_w_0()
                    .pl(esp::E8)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_start()
                    .metadado()
                    .text_color(Cores::tenue())
                    .child(super::dialogos::com_til(&pasta))
                    .into_any_element()
            };
            return Some(
                h_flex()
                    .flex_none()
                    .h(r(36.))
                    .gap(esp::E8)
                    .child(div().flex_1().min_w_0().child(escolha))
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
        let nome = match self.vista.filtro() {
            Some(p) => format!("{} · arquivadas", motor.nome_do_projeto(p)),
            None => "Arquivadas".into(),
        };
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
    /// os agentes rodando na pasta toda ("2 rodando · 1 na fila"); em Arquivos, quantos arquivos
    /// o projeto tem.
    fn rodape(&self, motor: &Motor, arquivos: Option<&Arquivos>, window: &Window, cx: &Context<Self>) -> Option<Div> {
        let rodando = motor.chats.iter().filter(|c| motor.execucao(&c.id).rodando()).count();
        let fila = motor.chats.iter().filter(|c| motor.esperando_vaga(&c.id)).count();
        let agentes = texto_agentes(rodando, fila).map(|texto| {
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
                .tooltip("Em todos os projetos")
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.aba != AbaLateral::Conversas {
                        this.trocar_aba(AbaLateral::Conversas, window, cx);
                    }
                    this.rolagem_lista.set_offset(point(px(0.), px(0.)));
                    cx.notify();
                }))
        });
        let arquivadas = arquivos.is_none().then(|| {
            let filtro = self.vista.filtro();
            let n = motor
                .chats
                .iter()
                .filter(|c| c.papel.na_lista() && motor.arquivada(c) && filtro.is_none_or(|p| c.projeto == p))
                .count();
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
        let sino = self.botao_sino(super::cartoes::precisa_com_o_mestre(motor), cx);
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
        let filtro = self.vista.filtro();
        let do_filtro = |c: &Chat| filtro.is_none_or(|p| p == c.projeto);
        let rodando = |c: &Chat| {
            motor.execucao(&c.id).rodando()
                || motor.esperando_vaga(&c.id)
                || motor.tarefas_do(&c.id).any(|t| t.estado.ativa() && !motor.tarefa_parada(t))
        };

        let mostrar_arquivados = self.filtro == Filtro::Arquivados;
        // Na visão geral, o Mestre entra quando roda ou espera você (o resto está no trilho).
        let geral = self.vista == Vista::Geral && !mostrar_arquivados;
        let mestre_a_vista = |c: &Chat| {
            geral && c.papel == Papel::Mestre && (rodando(c) || motor.espera_do_chat(c).is_some_and(|e| e != Motivo::Nova))
        };
        // Na lista enxuta o Mestre é a primeira linha, sempre: ele saiu da fileira de cima.
        let mestre_fixo = geral && busca.is_empty() && self.lista_enxuta(motor);
        let mut coordenadores: Vec<&Chat> = motor
            .chats
            .iter()
            .filter(|c| c.papel.na_lista() || mestre_a_vista(c) || (mestre_fixo && c.papel == Papel::Mestre))
            .filter(|c| motor.arquivada(c) == mostrar_arquivados)
            .filter(|c| do_filtro(c))
            .filter(|c| combina(c) || ajudantes_de(motor, &c.id).iter().any(|t| combina(t)))
            .collect();
        coordenadores.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
        let familias: Vec<Familia> = coordenadores
            .into_iter()
            .map(|c| {
                let trabalhadores = ajudantes_de(motor, &c.id);
                let pendencia = pendencia_visivel(motor, c, &trabalhadores, self.selecionado.as_deref());
                (c, trabalhadores, pendencia)
            })
            .collect();
        let secao_de = |(c, trabalhadores, pendencia): &Familia| {
            if pendencia.is_some() {
                Secao::EsperandoVoce
            } else if rodando(c) || trabalhadores.iter().any(|t| rodando(t)) {
                Secao::Rodando
            } else if c.fixado {
                Secao::Fixadas
            } else {
                Secao::Conversas
            }
        };
        let e_o_mestre = |f: &Familia| mestre_fixo && f.0.papel == Papel::Mestre;
        let da_secao = |secao: Secao| familias.iter().filter(|f| !e_o_mestre(f) && secao_de(f) == secao).collect::<Vec<_>>();
        let mut m = Montagem { corrida: geral, ..Montagem::default() };
        if let Some(f) = familias.iter().find(|f| e_o_mestre(f)) {
            m.itens.push(div().flex_none().h(esp::E4).into_any_element());
            self.familia(&mut m, motor, f, false, ctx, window, cx);
        }

        // "Enquanto você esteve fora", no topo da visão geral.
        if geral && busca.is_empty()
            && let Some(faixa) = self.faixa_do_resumo(motor, "lista", window, cx)
        {
            m.itens.push(div().pt(esp::E4).px(esp::E2).child(faixa).into_any_element());
        }
        // Pedidos, paradas e erros primeiro; as respostas novas depois (cada grupo por data).
        let mut esperando = da_secao(Secao::EsperandoVoce);
        esperando.sort_by_key(|f| f.2.as_ref().is_some_and(|p| p.motivo == Motivo::Nova));
        // Os cartões abertos do Mestre e os chats parados no freio de ciclo vêm primeiro.
        let do_mestre = if geral && busca.is_empty() { super::cartoes::esperas_do_mestre(motor) } else { Vec::new() };
        // O que os PRs esperam de você (falhou no Watch, pronto para atualizar, commit de fora…).
        let do_pr = if busca.is_empty() && !mostrar_arquivados { super::painel_pr::esperas_do_pr(motor, filtro) } else { Vec::new() };
        if !esperando.is_empty() || !do_mestre.is_empty() || !do_pr.is_empty() {
            let numero = if busca.is_empty() && geral {
                super::cartoes::precisa_com_o_mestre(motor).numero() + do_pr.len()
            } else if busca.is_empty() {
                motor.precisa_de_voce(filtro).numero() + do_pr.len()
            } else {
                esperando.iter().filter(|f| f.2.as_ref().is_some_and(|p| p.motivo != Motivo::Nova)).count()
            };
            let paradas: Vec<&&Familia> =
                esperando.iter().filter(|f| f.2.as_ref().is_some_and(|p| p.motivo == Motivo::Parada)).collect();
            let mut cabecalho = CabecalhoDeSecao::new("Esperando você");
            if numero > 0 {
                cabecalho = cabecalho.contagem(numero).tingida(Cores::ambar());
            }
            if paradas.len() >= 2 {
                let retomar: Vec<String> = paradas
                    .iter()
                    .flat_map(|f| motor.familia(f.0))
                    .filter(|c| motor.precisa_retomar(&c.id))
                    .map(|c| c.id.clone())
                    .collect();
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
            m.itens.extend(self.linhas_do_mestre(do_mestre, window, cx));
            m.itens.extend(self.linhas_do_pr(motor, do_pr, window, cx));
            for f in esperando {
                self.familia(&mut m, motor, f, true, ctx, window, cx);
            }
        }

        let rodando_agora = da_secao(Secao::Rodando);
        if !rodando_agora.is_empty() {
            let cabecalho = CabecalhoDeSecao::new("Rodando agora")
                .contagem(rodando_agora.len())
                .tingida(Cores::destaque_texto());
            m.cabecalho(cabecalho);
            for f in rodando_agora {
                self.familia(&mut m, motor, f, true, ctx, window, cx);
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
                    self.familia(&mut m, motor, f, false, ctx, window, cx);
                }
            }
        }

        // Na visão geral, os 15 mais recentes de todos os projetos (com o nome do projeto no
        // cartão); a lista inteira fica na vista de cada projeto.
        let paradas = da_secao(Secao::Conversas);
        if self.vista == Vista::Geral && !mostrar_arquivados {
            let acima: HashSet<&str> = familias
                .iter()
                .filter(|f| secao_de(f) != Secao::Conversas)
                .map(|f| f.0.id.as_str())
                .collect();
            let candidatos: Vec<&Chat> = paradas.iter().map(|f| f.0).collect();
            let limite = if busca.is_empty() { visao_geral::RECENTES } else { usize::MAX };
            let ids: Vec<&str> = visao_geral::recentes(&candidatos, &acima, limite).iter().map(|c| c.id.as_str()).collect();
            if !ids.is_empty() {
                let titulo = if busca.is_empty() { "Recentes" } else { "Conversas" };
                m.cabecalho(CabecalhoDeSecao::new(titulo));
                for id in ids {
                    if let Some(f) = paradas.iter().find(|f| f.0.id == id) {
                        self.familia(&mut m, motor, f, false, ctx, window, cx);
                    }
                }
            }
        }
        // As paradas por data. "Mais antigas" começa recolhida, menos na busca, nas Arquivadas,
        // quando é a única faixa ou quando tem uma conversa aberta.
        let paradas = if self.vista == Vista::Geral && !mostrar_arquivados { Vec::new() } else { paradas };
        let hoje = meia_noite_local(agora());
        let faixas: Vec<(Faixa, Vec<&Familia>)> = Faixa::TODAS
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
            let com_aberta = membros.iter().any(|f| self.familia_aberta(f));
            let pode_recolher = busca.is_empty() && (!antigas || (!mostrar_arquivados && !so_uma && !com_aberta));
            let recolhida = pode_recolher && self.secoes_recolhidas.contains(faixa.nome());
            let mut cabecalho = CabecalhoDeSecao::new(faixa.nome());
            if antigas {
                cabecalho = cabecalho.contagem(membros.len());
                if !mostrar_arquivados {
                    let ids: Vec<String> = membros.iter().map(|f| f.0.id.clone()).collect();
                    let botao = botao_de_secao("arquivar-antigas", "Arquivar estas", Lucide::Archive, ctx.aperto.apertada, window, cx);
                    cabecalho = cabecalho.acao(botao.on_click(
                        cx.listener(move |this, _, window, cx| this.arquivar_estas(ids.clone(), window, cx)),
                    ));
                }
            }
            if pode_recolher {
                cabecalho = cabecalho.recolhivel(!recolhida, alternar_secao(ctx.janela.clone(), faixa.nome()));
            }
            m.cabecalho(cabecalho);
            if !recolhida {
                for f in membros {
                    self.familia(&mut m, motor, f, false, ctx, window, cx);
                }
            }
        }

        // Na busca, as arquivadas que casam vêm no fim, tênues; fora dela, ficam em Arquivadas.
        if !mostrar_arquivados {
            let arquivadas =
                arquivadas_que_casam(&motor.chats, busca, filtro, |c| motor.arquivada(c));
            if !arquivadas.is_empty() {
                m.cabecalho(CabecalhoDeSecao::new("Arquivadas").contagem(arquivadas.len()));
                for c in arquivadas {
                    let aberta = self.selecionado.as_deref() == Some(c.id.as_str());
                    let cartao = self.cartao_conversa(motor, c, None, ctx, window, cx);
                    let tenue = div().when(!aberta, |d| d.opacity(0.6).hover(|s| s.opacity(1.))).child(cartao);
                    m.navegavel(tenue.into_any_element(), &c.id, None, false);
                }
            }
        }

        if m.itens.is_empty() {
            m.itens.push(self.lista_vazia(motor, ctx, window, cx));
        }
        m
    }

    /// O estado vazio certo para o que está sendo visto.
    fn lista_vazia(&self, motor: &Motor, ctx: &Contexto, window: &Window, cx: &Context<Self>) -> AnyElement {
        let vazio = if !ctx.busca.is_empty() {
            let digitado = self.busca.read(cx).value().trim().to_string();
            EstadoVazio::new(Lucide::Search, format!("Nada encontrado para “{digitado}”"))
                .texto("A busca olha títulos e mensagens, também das arquivadas.")
                .acao(self.botao_limpar_busca(window, cx))
        } else if self.filtro == Filtro::Arquivados {
            EstadoVazio::new(Lucide::Archive, "Nada arquivado").acao(
                botao_fantasma("vazio-voltar", None, "Voltar às conversas", TamanhoBotao::Controle, window, cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.filtro = Filtro::Todos;
                        cx.notify();
                    })),
            )
        } else if let Some(projeto) = self.vista.projeto().map(str::to_string) {
            let (icone_p, cor) = icone_projeto::visual_do(motor, &projeto);
            let chave = motor.projeto(&projeto).map(|p| p.chave()).unwrap_or_else(|| projeto.clone());
            let citar = format!("#{chave} ");
            EstadoVazio::new(icone_projeto::desenho(icone_p, 20., rgb(cor)), "Nenhum chat ainda")
                .texto(format!("Comece um chat aqui, ou peça ao Mestre com #{chave}: ele divide o pedido e abre os chats."))
                .acao(
                    botao_primario("vazio-nova-no-projeto", Some(Lucide::Plus.into()), "Novo chat", TamanhoBotao::Controle, window, cx)
                        .on_click(cx.listener(move |this, _, window, cx| this.novo_chat_em(&projeto, window, cx))),
                )
                .acao(
                    botao_fantasma("vazio-pedir-ao-mestre", Some(Lucide::Orbit.into()), "Pedir ao Mestre", TamanhoBotao::Controle, window, cx)
                        .on_click(cx.listener(move |this, _, window, cx| this.pedir_ao_mestre(&citar, window, cx))),
                )
        } else if self.vista == Vista::Avulsos {
            EstadoVazio::new(Lucide::MessageCircle, "Nenhum chat avulso")
                .texto("Um chat avulso tem uma pasta só dele, fora dos projetos.")
                .acao(
                    botao_primario("vazio-novo-avulso", Some(Lucide::Plus.into()), "Novo chat", TamanhoBotao::Controle, window, cx)
                        .on_click(cx.listener(|this, _, window, cx| this.nova_conversa("", window, cx))),
                )
        } else if motor.projetos_visiveis().is_empty() {
            EstadoVazio::new(Lucide::FolderPlus, "Nenhum projeto ainda")
                .texto("Crie um projeto (com ou sem pastas) ou comece um chat avulso.")
                .acao(
                    botao_primario("vazio-novo-projeto", None, "Novo projeto…", TamanhoBotao::Controle, window, cx)
                        .on_click(cx.listener(|this, _, window, cx| this.dialogo_criar_projeto(window, cx))),
                )
        } else {
            EstadoVazio::new(Lucide::MessagesSquare, "Nenhum chat ainda")
                .texto("Abra um projeto no trilho ou comece um chat novo.")
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

    /// A família tem alguém aberto em algum painel (não só no ativo).
    fn familia_aberta(&self, (coordenador, trabalhadores, _): &Familia) -> bool {
        self.aberta_em_painel(&coordenador.id) || trabalhadores.iter().any(|t| self.aberta_em_painel(&t.id))
    }

    /// O cartão do coordenador e, embaixo, os trabalhadores: sempre em "Esperando você" e
    /// "Rodando agora"; nas outras seções, com a família aberta num painel ou pela seta. Fora da
    /// conversa aberta, as tarefas aprovadas e canceladas ficam em "Mais N concluídas".
    #[allow(clippy::too_many_arguments)]
    fn familia(
        &self,
        m: &mut Montagem,
        motor: &Motor,
        familia: &Familia,
        sempre: bool,
        ctx: &Contexto,
        window: &Window,
        cx: &Context<Self>,
    ) {
        let (coordenador, trabalhadores, pendencia) = familia;
        let cartao = self.cartao_conversa(motor, coordenador, pendencia.as_ref(), ctx, window, cx);
        m.navegavel(cartao, &coordenador.id, None, pendencia.is_some());
        let aberta_por_voce = self.familias_abertas.contains(&coordenador.id);
        if !(sempre || aberta_por_voce || self.familia_aberta(familia)) {
            return;
        }
        let na_conversa = self
            .selecionado
            .as_deref()
            .is_some_and(|s| s == coordenador.id || trabalhadores.iter().any(|t| t.id == s));
        let mostrar_concluidas = na_conversa || aberta_por_voce;
        let concluida = |t: &Chat| {
            motor.espera_do_chat(t).is_none()
                && t.tarefa
                    .and_then(|id| motor.tarefa(id))
                    .is_some_and(|t| matches!(t.estado, EstadoTarefa::Aprovada | EstadoTarefa::Cancelada))
        };
        let concluidas = trabalhadores.iter().filter(|t| concluida(t)).count();
        let com_concluidas = concluidas > 0 && !na_conversa;
        let visiveis: Vec<&Chat> =
            trabalhadores.iter().copied().filter(|t| mostrar_concluidas || !concluida(t)).collect();
        for (i, t) in visiveis.iter().enumerate() {
            let ramo = (i == 0, i + 1 == visiveis.len() && !com_concluidas);
            let linha = self.linha_ajudante(motor, t, ramo, ctx, window, cx);
            m.navegavel(linha, &t.id, Some(&coordenador.id), false);
        }
        if com_concluidas {
            let primeira = visiveis.is_empty();
            m.itens.push(self.linha_concluidas(&coordenador.id, concluidas, aberta_por_voce, primeira, window, cx));
        }
    }

    /// "Mais 2 concluídas" (ou "Esconder as concluídas"), nas colunas dos trabalhadores: o traço da
    /// árvore em 31 termina na seta, e o texto fica na coluna dos títulos (58). A primeira linha do
    /// ramo (sem trabalhadores à vista) leva o traço até o avatar do pai, como o `CartaoDeLista`.
    fn linha_concluidas(
        &self,
        coordenador: &str,
        n: usize,
        abertas: bool,
        primeira: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id = coordenador.to_string();
        let texto = if abertas { "Esconder as concluídas".to_string() } else { format!("Mais {}", contagem(n, "concluída", "concluídas")) };
        let traco = div()
            .absolute()
            .left(relative(0.5))
            .w(px(1.))
            .map(|d| if primeira { d.top(r(-13.)).h(r(23.)) } else { d.top(px(-1.)).h(r(10.)) })
            .bg(Cores::traco());
        let coluna = div()
            .relative()
            .flex_none()
            .w(r(30.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .child(traco)
            .child(icone(if abertas { Lucide::ChevronUp } else { Lucide::ChevronDown }, 12., window));
        div()
            .id(SharedString::from(format!("concluidas-{coordenador}")))
            .flex()
            .items_center()
            .gap(esp::E12)
            .h(r(28.))
            .mt(esp::E2)
            .px(esp::E8)
            .rounded(raio::CONTROLE)
            .metadado()
            .text_color(Cores::tenue())
            .cursor_pointer()
            .hover(|s| s.bg(Cores::hover()).text_color(Cores::texto()))
            .child(coluna)
            .child(div().min_w_0().truncate().child(texto))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.familias_abertas.remove(&id) {
                    this.familias_abertas.insert(id.clone());
                }
                cx.notify();
            }))
            .into_any_element()
    }

    /// Arrastar para um painel, clique do meio e clique direito, iguais no coordenador e no
    /// trabalhador. Todo cartão leva 2 em cima: o traço da árvore conta com eles.
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

    /// A ação 1 do hover: Retomar a parada, ir ao trabalhador que pede, ou abrir ao lado.
    fn acao_do_cartao(
        &self,
        motor: &Motor,
        chat: &Chat,
        pendencia: Option<&Pendencia>,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id = SharedString::from(format!("acao-{}", chat.id));
        match pendencia {
            Some(p) if p.motivo == Motivo::Parada => {
                let retomar: Vec<String> = motor
                    .familia(chat)
                    .into_iter()
                    .filter(|c| motor.precisa_retomar(&c.id))
                    .map(|c| c.id.clone())
                    .collect();
                let dica = match p.alvo.tarefa.filter(|_| p.alvo.id != chat.id) {
                    Some(n) => format!("Retomar o ajudante #{n}"),
                    None => "Retomar o que parou no meio".into(),
                };
                botao_icone(id, Lucide::Play, dica, TamanhoBotao::Linha, window, cx)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(entidade) = &this.motor {
                            entidade.update(cx, |motor, cx| {
                                for id in &retomar {
                                    motor.retomar(id, cx);
                                }
                            });
                        }
                    }))
                    .into_any_element()
            }
            Some(p) if p.alvo.id != chat.id => {
                let n = p.alvo.tarefa.unwrap_or_default();
                let dica = if p.motivo == Motivo::Erro {
                    format!("Ver o erro do ajudante #{n}")
                } else {
                    format!("Ver o pedido do ajudante #{n}")
                };
                let alvo = p.alvo.id.clone();
                // No trabalhador a resposta é o cartão Permitir/Negar, não a caixa.
                botao_icone(id, Lucide::ArrowRight, dica, TamanhoBotao::Linha, window, cx)
                    .on_click(cx.listener(move |this, _, window, cx| this.selecionar(Some(alvo.clone()), window, cx)))
                    .into_any_element()
            }
            _ => self.botao_ao_lado(&chat.id, window, cx),
        }
    }

    /// Cartão de conversa (52): o quadrado do projeto com o sinal da família, o título, a prévia
    /// com o motivo e a coluna da meta (horário em cima; número do painel e não lido embaixo).
    fn cartao_conversa(
        &self,
        motor: &Motor,
        chat: &Chat,
        pendencia: Option<&Pendencia>,
        ctx: &Contexto,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let selecionado = self.selecionado.as_deref() == Some(chat.id.as_str());
        let em_painel = self.painel_da_conversa(&chat.id);
        let ao_lado = !selecionado && self.aberta_em_painel(&chat.id);
        let nao_lido = !selecionado && motor.nao_lido(chat);
        let realce = ctx.teclado && self.realce_lista.as_deref() == Some(chat.id.as_str());
        let (icone_p, cor_p) = icone_projeto::visual_do(motor, &chat.projeto);
        let (motivo, texto) = match pendencia {
            Some(p) => previa_pendencia(motor, chat, p),
            None => (None, previa(motor, chat)),
        };
        // Na busca, quando o termo não está no título, a prévia mostra onde ele casou.
        let achado = if !ctx.busca.is_empty() && trecho(&chat.titulo, &ctx.busca).is_none() {
            achado_na_familia(chat, &ajudantes_de(motor, &chat.id), &ctx.busca)
        } else {
            None
        };
        let estado = pendencia.is_none().then(|| estado_do_chat(motor, chat)).flatten();
        // Na visão geral o cartão é uma linha só (o quadrado da pasta, o título e a hora); a de
        // baixo volta na busca que achou no texto, para mostrar onde.
        let enxuta = self.vista == Vista::Geral && achado.is_none();
        let subtitulo = match (achado, estado) {
            (Some((achado, faixa)), _) => div()
                .min_w_0()
                .truncate()
                .child(StyledText::new(achado).with_highlights([(faixa, realce_da_busca())])),
            (None, Some((icone_e, cor, texto))) => h_flex()
                .gap(esp::E4)
                .min_w_0()
                .child(div().flex_none().text_color(cor).child(icone(icone_e, 12., window)))
                .child(div().flex_1().min_w_0().truncate().child(texto)),
            (None, None) => previa_elemento(motivo.as_ref(), texto, ctx.aperto.estreita),
        };
        // Na visão geral, o nome do projeto vem antes da prévia ("Ninee · …", "Avulso · …").
        let mestre = chat.papel == Papel::Mestre;
        let subtitulo = if self.vista == Vista::Geral && !mestre {
            let prefixo = if chat.projeto.is_empty() { "Avulso".to_string() } else { motor.nome_do_projeto(&chat.projeto) };
            h_flex()
                .gap(esp::E4)
                .min_w_0()
                .child(div().flex_none().max_w(r(96.)).truncate().font_weight(FontWeight::MEDIUM).child(prefixo))
                .child(div().flex_none().child("·"))
                .child(div().flex_1().min_w_0().child(subtitulo))
        } else {
            subtitulo
        };
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
        let do_mestre = super::cartoes::do_mestre(motor, chat)
            .map(|dica| super::cartoes::selo_do_mestre(SharedString::from(format!("do-mestre-{}", chat.id)), dica));
        let indicadores = linha_meta(16.)
            .children(do_mestre)
            .children(em_painel.map(|(n, ativo)| numero_painel(n, ativo)))
            .when(nao_lido && sinal != Some(Sinal::Nova), |d| d.child(Sinal::Nova.ponto(8.)))
            // Sem o título "Fixadas", é o alfinete que diz por que ela está em cima.
            .when(enxuta && chat.fixado, |d| d.child(div().text_color(Cores::apagado()).child(icone(Lucide::Pin, 11., window))));
        // Na visão geral o quadrado é sempre o pequeno, também no resultado da busca (que tem duas
        // linhas): com uns de 18 e outros de 30 a coluna dos ícones ficava torta.
        let lado_avatar = if self.vista == Vista::Geral { 18. } else { 30. };
        let projeto = match chat.tarefa.filter(|_| chat.papel == Papel::Projeto) {
            Some(n) => SharedString::from(format!("{} · chat {n}", motor.nome_do_projeto(&chat.projeto))),
            None => SharedString::from(motor.nome_do_projeto(&chat.projeto)),
        };
        let avatar = div()
            .id(SharedString::from(format!("avatar-{}", chat.id)))
            .map(|d| {
                if mestre {
                    d.child(super::componentes::Papel::Mestre.circulo(lado_avatar))
                } else {
                    // Na lista de uma linha o quadrado é neutro: a cor de cada projeto, repetida em
                    // quinze linhas, virava um arco-íris ao lado dos títulos.
                    d.child(QuadradoDeProjeto::new(icone_p, if enxuta { 0x9ba8cf } else { cor_p }, lado_avatar))
                }
            })
            .tooltip(move |window, cx| Tooltip::new(if mestre { SharedString::from("Mestre") } else { projeto.clone() }).build(window, cx));
        let id = chat.id.clone();
        let tem_pendencia = pendencia.is_some();
        let titulo_do_cartao: SharedString = if mestre { "Mestre".into() } else { chat.titulo.clone().into() };
        let mut cartao = CartaoDeLista::new(SharedString::from(format!("cartao-{}", chat.id)), titulo_do_cartao)
            .simbolo(Simbolo::Elemento(avatar.into_any_element()))
            .sinal(sinal)
            .map(|c| {
                if enxuta {
                    c.uma_linha().meta(h_flex().gap(esp::E4).child(indicadores).child(quando))
                } else {
                    c.subtitulo(subtitulo).meta(quando).meta_inferior(indicadores)
                }
            })
            .acao(self.acao_do_cartao(motor, chat, pendencia, window, cx))
            .acao(self.botao_menu(&chat.id, ctx, window, cx))
            .selecionado(selecionado)
            .aberto_em_outro(ao_lado)
            .nao_lido(nao_lido)
            .realce(realce)
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                if ev.click_count() >= 2 && !mestre {
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
        self.envolver(cartao, chat, Simbolo::Projeto(icone_p, cor_p), ctx, cx)
    }

    /// Linha de trabalhador (32, nível 1): o ponto do estado sobre o traço da árvore, "#2 título"
    /// na coluna do título do pai e, à direita, só o que pede ação (o selo do pedido, do plano ou
    /// do erro), a duração de quem roda e o número do painel. A palavra do estado fica na dica do
    /// ponto.
    #[allow(clippy::too_many_arguments)]
    fn linha_ajudante(
        &self,
        motor: &Motor,
        chat: &Chat,
        (primeiro, ultimo): (bool, bool),
        ctx: &Contexto,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let selecionado = self.selecionado.as_deref() == Some(chat.id.as_str());
        let em_painel = self.painel_da_conversa(&chat.id);
        let ao_lado = !selecionado && self.aberta_em_painel(&chat.id);
        let realce = ctx.teclado && self.realce_lista.as_deref() == Some(chat.id.as_str());
        let visual = chat.tarefa.and_then(|t| motor.tarefa(t)).map(|t| estado_visual(motor, t));
        let rotulo = rotulo_do_trabalhador(motor, chat, visual.as_ref().map(|v| v.rotulo.as_ref()))
            .or_else(|| visual.as_ref().map(|v| v.rotulo.to_string()));
        let dica = match &visual {
            Some(v) => dica_com_rotulo(rotulo, &v.dica),
            None => dica_com_rotulo(rotulo, "Ajudante"),
        };
        let cor = visual.as_ref().and_then(|v| v.estado.cor()).unwrap_or_else(Cores::tenue);
        // Sem sinal (pronto, em revisão), o ponto solto de 8 na cor do estado.
        let ponto = div().flex_none().size(r(8.)).rounded_full().bg(cor);
        let detalhe = detalhe_do_trabalhador(motor, chat, selecionado, ctx.aperto.sem_rotulos);
        let meta = (detalhe.is_some() || em_painel.is_some()).then(|| {
            linha_meta(20.).children(detalhe).children(em_painel.map(|(n, ativo)| numero_painel(n, ativo)))
        });
        let titulo = match chat.tarefa {
            Some(n) => {
                let numero = format!("#{n}");
                let estilo = HighlightStyle { color: Some(Cores::tenue()), ..Default::default() };
                // O título do trabalhador já costuma vir como "#2 título".
                let titulo = chat.titulo.strip_prefix(&format!("{numero} ")).unwrap_or(&chat.titulo);
                StyledText::new(format!("{numero} {titulo}")).with_highlights([(0..numero.len(), estilo)])
            }
            None => StyledText::new(chat.titulo.clone()),
        };
        let id = chat.id.clone();
        let cartao = CartaoDeLista::new(SharedString::from(format!("trab-{}", chat.id)), chat.titulo.clone())
            .titulo_rico(titulo)
            .nivel(1)
            .ramo(primeiro, ultimo)
            .simbolo(Simbolo::Elemento(ponto.into_any_element()))
            .sinal(sinal_do_chat(motor, chat))
            .dica_do_ponto(dica)
            .when_some(meta, |c, meta| c.meta(meta))
            .when(!ctx.aperto.sem_rotulos, |c| c.acao(self.botao_ao_lado(&chat.id, window, cx)))
            .acao(self.botao_menu(&chat.id, ctx, window, cx))
            .selecionado(selecionado)
            .aberto_em_outro(ao_lado)
            .realce(realce)
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                if ev.modifiers().control {
                    this.abrir_ao_lado(id.clone(), None, window, cx);
                } else {
                    this.selecionar(Some(id.clone()), window, cx);
                }
            }));
        let simbolo = Simbolo::Papel(super::componentes::Papel::Trabalhador);
        self.envolver(cartao, chat, simbolo, ctx, cx)
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

    /// Teclado da lista: ↑/↓, Home/End movem o realce; →/← mostram ou escondem as concluídas
    /// (← num trabalhador volta ao coordenador); Enter abre, Ctrl+Enter abre ao lado, F2
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
        match (tecla, &n.coordenador) {
            ("enter", _) if ev.keystroke.modifiers.control => self.abrir_ao_lado(n.chat.clone(), None, window, cx),
            ("enter", _) => {
                self.selecionar(Some(n.chat.clone()), window, cx);
                if n.pendencia {
                    self.focar_composer(window, cx);
                }
            }
            ("f2", _) => self.dialogo_renomear(n.chat.clone(), window, cx),
            ("right", None) => {
                self.familias_abertas.insert(n.chat.clone());
            }
            ("left", None) => {
                self.familias_abertas.remove(&n.chat);
            }
            ("left", Some(pai)) => {
                if let Some(i) = nav.iter().position(|x| x.chat == *pai) {
                    self.realce_lista = Some(pai.clone());
                    self.rolagem_lista.scroll_to_item(nav[i].indice);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// "Arquivar estas": arquiva na hora (nada se apaga), e o aviso tem Desfazer.
    fn arquivar_estas(&self, ids: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        // Só as que ainda não estavam: o Desfazer devolve estas e deixa as outras como estavam.
        let ids: Vec<String> = {
            let m = motor.read(cx);
            ids.into_iter().filter(|id| m.chat(id).is_some_and(|c| !c.arquivado)).collect()
        };
        if ids.is_empty() {
            return;
        }
        motor.update(cx, |m, cx| m.arquivar_varios(&ids, cx));
        let texto = contagem(ids.len(), "conversa arquivada", "conversas arquivadas");
        avisar_arquivadas(motor, ids, texto, window, cx);
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

    /// As pastas do projeto que a aba Arquivos oferece: as ligadas e, se o chat aberto é do
    /// projeto e trabalha numa cópia, a "Cópia deste chat".
    fn fontes_dos_arquivos(&self, motor: &Motor, projeto: &str) -> Vec<(SharedString, PathBuf)> {
        let mut fontes: Vec<(SharedString, PathBuf)> = motor
            .pastas_do(projeto)
            .iter()
            .filter(|p| p.caminho.is_dir())
            .map(|p| {
                let nome = if p.propria {
                    "Pasta própria".to_string()
                } else {
                    p.caminho.file_name().map_or_else(|| p.caminho.display().to_string(), |n| n.to_string_lossy().to_string())
                };
                (SharedString::from(nome), p.caminho.clone())
            })
            .collect();
        if fontes.len() == 1 {
            fontes[0].0 = "Original".into();
        }
        let copia = self
            .selecionado
            .as_ref()
            .and_then(|id| motor.chat(id))
            .filter(|c| c.projeto == projeto)
            .and_then(|c| c.tarefa.and_then(|t| motor.tarefa(t)))
            .and_then(|t| t.worktree.clone())
            .filter(|w| w.is_dir());
        if let Some(copia) = copia {
            fontes.push(("Cópia deste chat".into(), copia));
        }
        fontes
    }

    /// De qual projeto a aba Arquivos mostra as pastas: o da vista ou, na visão geral, o do chat
    /// aberto no painel ativo.
    fn projeto_dos_arquivos(&self, motor: &Motor) -> Option<String> {
        if let Some(p) = self.vista.projeto() {
            return Some(p.to_string());
        }
        if self.vista != Vista::Geral {
            return None;
        }
        let chat = self.selecionado.as_ref().and_then(|id| motor.chat(id))?;
        (!chat.projeto.is_empty()).then(|| chat.projeto.clone())
    }

    /// A pasta da aba Arquivos (a escolhida, ou a primeira), os arquivos dela e a árvore, ou os
    /// que casam com a busca.
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
        let projeto = self.projeto_dos_arquivos(motor);
        let fontes = match &projeto {
            Some(p) => self.fontes_dos_arquivos(motor, p),
            // Na visão geral, o chat sem projeto mostra a pasta dele.
            None => self
                .selecionado
                .as_ref()
                .and_then(|id| motor.chat(id))
                .filter(|c| self.vista == Vista::Geral && c.papel != Papel::Mestre && c.cwd.is_dir())
                .map(|c| vec![(SharedString::from("Pasta do chat"), c.cwd.clone())])
                .unwrap_or_default(),
        };
        let escolhida = projeto.as_ref().and_then(|p| self.pasta_arquivos.get(p));
        let pasta = escolhida
            .filter(|e| fontes.iter().any(|(_, p)| p == *e))
            .cloned()
            .or_else(|| fontes.first().map(|(_, p)| p.clone()));
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
        Arquivos { fontes, pasta, lista, achados, arvore, rolagem }
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
            let texto = if self.vista == Vista::Geral {
                "Abra um chat para ver os arquivos da pasta dele."
            } else {
                "As pastas deste projeto não foram encontradas. Localize-as no cabeçalho, ou em Editar projeto."
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

/// A palavra do estado na dica do ponto: "permissão", "plano", "erro", "parada" ou o estado da
/// tarefa; de quem roda, nenhuma (fica a da tarefa).
fn rotulo_do_trabalhador(motor: &Motor, chat: &Chat, rotulo: Option<&str>) -> Option<String> {
    match motor.execucao(&chat.id) {
        Execucao::Rodando { .. } => None,
        Execucao::EsperandoVoce => {
            let plano = motor.permissoes.iter().any(|p| p.chat == chat.id && p.ferramenta == "ExitPlanMode");
            Some(if plano { "plano" } else { "permissão" }.into())
        }
        Execucao::Erro(_) => Some("erro".into()),
        Execucao::Parado if motor.parada(&chat.id).is_some() => Some("parada".into()),
        Execucao::Parado => rotulo.map(str::to_string),
    }
}

/// A dica com o rótulo em cima ("Em revisão\nO coordenador decide…"), a não ser que ela já
/// comece por ele.
fn dica_com_rotulo(rotulo: Option<String>, dica: &str) -> String {
    match rotulo {
        Some(r) if !r.is_empty() && !dica.to_lowercase().starts_with(&r.to_lowercase()) => {
            let mut letras = r.chars();
            let titulo: String = letras.next().into_iter().flat_map(char::to_uppercase).chain(letras).collect();
            format!("{titulo}\n{dica}")
        }
        _ => dica.to_string(),
    }
}

/// O que fica à direita do trabalhador: a duração de quem roda e o selo do que pede ação (o
/// pedido, o plano ou o erro). Na lista apertada só a duração: o ponto já diz o resto.
fn detalhe_do_trabalhador(motor: &Motor, chat: &Chat, selecionado: bool, sem_rotulos: bool) -> Option<AnyElement> {
    match motor.execucao(&chat.id) {
        Execucao::Rodando { desde } => Some(
            div()
                .whitespace_nowrap()
                .text_color(if selecionado { Cores::texto() } else { Cores::destaque_texto() })
                .child(duracao(desde))
                .into_any_element(),
        ),
        _ if sem_rotulos => None,
        Execucao::EsperandoVoce => {
            let plano = motor
                .permissoes
                .iter()
                .any(|p| p.chat == chat.id && p.ferramenta == "ExitPlanMode");
            Some(Selo::cor(if plano { "plano" } else { "permissão" }, Cores::ambar()).into_any_element())
        }
        Execucao::Erro(_) => Some(Selo::cor("erro", Cores::vermelho()).into_any_element()),
        Execucao::Parado => None,
    }
}

#[cfg(test)]
mod testes {
    use super::{
        AbaLateral, Aperto, Causa, Faixa, Motivo, MotivoDaPrevia, No, PendenciaDoMotor, Sinal, achado_na_familia,
        arquivadas_que_casam, dica_com_rotulo, dica_da_busca, dica_de_fora, escolher_alvo, frase_da_pendencia,
        meia_noite_local, milhar, montar_arvore, motivo, partes_da_previa, posicao_do_realce, rotulo_pendencia,
        texto_agentes, texto_da_atividade, trecho,
    };
    use crate::modelo::{Chat, Execucao, Item, Papel};

    fn conversa(id: &str, titulo: &str, falas: &[&str]) -> Chat {
        let json = format!(
            r#"{{"id": "{id}", "projeto": "loja", "titulo": "{titulo}", "papel": "Coordenador", "pai": null,
                "tarefa": null, "sessao": "s", "sessao_criada": false, "cwd": "/tmp/loja", "modelo": null,
                "criado_em": 0, "atualizado_em": 0, "custo_usd": 0.0}}"#
        );
        let mut c: Chat = serde_json::from_str(&json).unwrap();
        c.itens = falas.iter().map(|t| Item::Assistente { texto: t.to_string(), em: 0 }).collect();
        c
    }

    #[test]
    fn regra_do_que_precisa_de_voce() {
        let erro = Execucao::Erro("caiu".into());
        assert_eq!(motivo(&Execucao::EsperandoVoce, false, false, false), Some(Motivo::Pedido));
        assert_eq!(motivo(&erro, false, false, false), Some(Motivo::Erro));
        assert_eq!(motivo(&Execucao::Parado, true, true, false), Some(Motivo::Nova));
        // Resposta nova só conta no coordenador, e o erro vence ela.
        assert_eq!(motivo(&Execucao::Parado, false, true, false), None);
        assert_eq!(motivo(&erro, true, true, false), Some(Motivo::Erro));
        assert_eq!(motivo(&Execucao::Rodando { desde: 0 }, true, false, false), None);
        // O que parou no meio pede Retomar: vem antes do erro e da resposta nova, depois do pedido.
        assert_eq!(motivo(&Execucao::Parado, true, true, true), Some(Motivo::Parada));
        assert_eq!(motivo(&erro, false, false, true), Some(Motivo::Parada));
        assert_eq!(motivo(&Execucao::EsperandoVoce, false, false, true), Some(Motivo::Pedido));
    }

    #[test]
    fn alvo_da_familia() {
        use Motivo::{Erro, Nova, Parada, Pedido};
        assert_eq!(escolher_alvo(&[None, None]), None);
        assert_eq!(escolher_alvo(&[Some(Nova)]), Some(0));
        // O pedido de um trabalhador vem antes da resposta nova e do erro do coordenador.
        assert_eq!(escolher_alvo(&[Some(Nova), None, Some(Pedido)]), Some(2));
        assert_eq!(escolher_alvo(&[Some(Erro), Some(Pedido)]), Some(1));
        assert_eq!(escolher_alvo(&[Some(Nova), Some(Erro), Some(Parada)]), Some(2));
        // Empate: o coordenador, ou o primeiro trabalhador.
        assert_eq!(escolher_alvo(&[Some(Pedido), Some(Pedido)]), Some(0));
        assert_eq!(escolher_alvo(&[None, Some(Erro), Some(Erro)]), Some(1));
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
    fn rotulo_do_motivo() {
        assert_eq!(rotulo_pendencia("Permissão", None, 0, false), "Permissão");
        assert_eq!(rotulo_pendencia("Erro", None, 2, false), "Erro (+2)");
        assert_eq!(rotulo_pendencia("Parada", Some(5), 0, false), "#5 parada");
        assert_eq!(rotulo_pendencia("Permissão", Some(3), 1, false), "#3 permissão (+1)");
    }

    #[test]
    fn a_mesma_pendencia_diz_o_mesmo_na_lista_e_no_sino() {
        let frase = |causa: Causa, tarefa| frase_da_pendencia(&causa, tarefa);
        assert_eq!(frase(Causa::Parada("Vitral fechou".into()), None), (Sinal::Parada, "Parada · Vitral fechou".into()));
        assert_eq!(frase(Causa::Parada("Vitral fechou".into()), Some(5)).1, "#5 parada · Vitral fechou");
        assert_eq!(frase(Causa::Permissao("Bash · npm test".into()), None), (Sinal::Pedido, "Permissão · Bash · npm test".into()));
        assert_eq!(frase(Causa::Plano, None), (Sinal::Pedido, "Plano para aprovar".into()));
        assert_eq!(frase(Causa::Plano, Some(2)).1, "#2 plano para aprovar");
        assert_eq!(frase(Causa::Erro("\n# deu ruim\nresto".into()), None), (Sinal::Erro, "Erro · deu ruim".into()));
        // O sino lê a pendência do motor; a lista, o motivo da família. As duas viram a mesma
        // causa, e a prévia monta o mesmo texto que a frase.
        let do_motor = PendenciaDoMotor::Parada { chat: "c1".into(), motivo: "o Vitral fechou no meio do turno".into() };
        let causa = Causa::da_pendencia(&do_motor).unwrap();
        let (motivo, detalhe) = MotivoDaPrevia::de(&causa, None, 0);
        let (rotulo, texto) = partes_da_previa(Some(&motivo), &detalhe, false);
        assert_eq!(format!("{} {texto}", rotulo.unwrap()), frase_da_pendencia(&causa, None).1);
        assert_eq!(frase_da_pendencia(&causa, None).1, "Parada · Vitral fechou");
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
                ponto: None,
                ultrathink: false,
                ultracode: false,
                referencias: Vec::new(),
                em: 0,
            },
        );
        // O termo só numa mensagem: a prévia vira o trecho dela, numa linha, com o termo marcado.
        let (texto, faixa) = achado_na_familia(&cupons, &[], "frete").unwrap();
        assert_eq!(texto, "Você: Valide o frete grátis no carrinho");
        assert_eq!(&texto[faixa], "frete");
        assert_eq!(achado_na_familia(&cupons, &[], "boleto"), None);
        // Fala longa: o recorte começa numa palavra, pouco antes do termo.
        let longa = conversa("c2", "Loja", &["Conferi o layout inteiro da página de produtos e o Frete ficou certo"]);
        let (texto, faixa) = achado_na_familia(&longa, &[], "frete").unwrap();
        assert_eq!(texto, "…página de produtos e o Frete ficou certo");
        assert_eq!(&texto[faixa], "Frete");
        // Só um trabalhador falou: o trecho leva o número dele.
        let mut trabalhador = conversa("t1", "#2 Entrega", &["calculei o frete por CEP"]);
        (trabalhador.papel, trabalhador.pai, trabalhador.tarefa) = (Papel::Trabalhador, Some("c3".into()), Some(2));
        let antiga = conversa("c3", "Antiga", &["nada aqui"]);
        let (texto, faixa) = achado_na_familia(&antiga, &[&trabalhador], "frete").unwrap();
        assert_eq!((texto.as_str(), &texto[faixa]), ("#2: calculei o frete por CEP", "frete"));
        // As arquivadas que casam (por elas ou por um trabalhador) aparecem só com busca.
        let mut antiga = antiga;
        antiga.arquivado = true;
        let mut outra = conversa("c4", "Frete antigo", &[]);
        (outra.arquivado, outra.atualizado_em) = (true, 10);
        let chats = vec![cupons, antiga, trabalhador, outra];
        let ids = |busca: &str, projeto: Option<&str>| -> Vec<String> {
            arquivadas_que_casam(&chats, busca, projeto, |c| c.arquivado).into_iter().map(|c| c.id.clone()).collect()
        };
        assert_eq!(ids("frete", None), ["c4", "c3"]);
        assert!(ids("", None).is_empty());
        assert!(ids("frete", Some("site")).is_empty());
        // Com o projeto arquivado, as conversas dele contam como arquivadas sem a marca própria.
        let projetos = vec![crate::modelo::Projeto { id: "velho".into(), arquivado: true, ..Default::default() }];
        let mut do_velho = conversa("c5", "Frete do velho", &[]);
        do_velho.projeto = "velho".into();
        let chats = vec![do_velho, chats[0].clone(), chats[3].clone()];
        let arquivada = |c: &Chat| crate::motor::conversa_arquivada(&projetos, c);
        let achadas: Vec<&str> =
            arquivadas_que_casam(&chats, "frete", None, arquivada).into_iter().map(|c| c.id.as_str()).collect();
        assert_eq!(achadas, ["c4", "c5"]);
        assert!(!arquivada(&chats[1]));
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
        assert_eq!(texto_agentes(0, 0), None);
        assert_eq!(texto_agentes(1, 0).as_deref(), Some("1 rodando"));
        assert_eq!(texto_agentes(2, 1).as_deref(), Some("2 rodando · 1 na fila"));
        assert_eq!(texto_agentes(0, 2).as_deref(), Some("2 na fila"));
        assert_eq!(milhar(7), "7");
        assert_eq!(milhar(1234), "1.234");
        assert_eq!(milhar(1_234_567), "1.234.567");
    }

    #[test]
    fn atividade_das_tarefas_em_andamento() {
        assert_eq!(texto_da_atividade(1, Some("#3 · Edit · README.md".into())), "#3 · Edit · README.md");
        assert_eq!(texto_da_atividade(2, Some("#3 · Bash · cargo test".into())), "2 rodando · #3 · Bash · cargo test");
        assert_eq!(texto_da_atividade(1, None), "1 ajudante rodando");
        assert_eq!(texto_da_atividade(3, None), "3 ajudantes rodando");
    }

    #[test]
    fn dica_do_que_espera_fora_do_filtro() {
        assert_eq!(dica_de_fora(1, &["site".into()]), "1 chat esperando você em site");
        assert_eq!(
            dica_de_fora(3, &["site".into(), "loja".into(), "api".into()]),
            "3 chats esperando você em site, loja e api"
        );
        assert_eq!(dica_de_fora(2, &[]), "2 chats esperando você em outros projetos");
    }

    #[test]
    fn a_lista_estreita_abre_mao_do_que_e_menos_importante_primeiro() {
        // Na largura normal (312) cabe tudo.
        for largura in [288., 312., 560.] {
            let a = Aperto::de(largura);
            assert!(!a.sem_rotulos && !a.estreita && !a.apertada);
        }
        // Encolhendo: os selos e o Abrir ao lado dos trabalhadores saem, depois os rótulos das
        // abas e da prévia e, por último, os das ações das seções.
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
    fn na_lista_estreita_a_parada_do_trabalhador_continua_dizendo_de_quem_e() {
        // O curto fica com o número e o "(+N)"; só o rótulo do próprio coordenador sai.
        assert_eq!(rotulo_pendencia("Parada", Some(5), 0, true), "#5");
        assert_eq!(rotulo_pendencia("Parada", Some(5), 2, true), "#5 (+2)");
        assert_eq!(rotulo_pendencia("Erro", None, 1, true), "(+1)");
        assert_eq!(rotulo_pendencia("Erro", None, 0, true), "");
        let parada = Causa::Parada("Vitral fechou".into());
        let (do_trabalhador, detalhe) = MotivoDaPrevia::de(&parada, Some(5), 1);
        assert_eq!(partes_da_previa(Some(&do_trabalhador), &detalhe, true), (Some("#5 (+1)".into()), "· Vitral fechou".into()));
        let (do_coordenador, detalhe) = MotivoDaPrevia::de(&parada, None, 0);
        assert_eq!(partes_da_previa(Some(&do_coordenador), &detalhe, true), (None, "Vitral fechou".into()));
        // Sem detalhe, o rótulo é a frase inteira e fica.
        let (plano, detalhe) = MotivoDaPrevia::de(&Causa::Plano, None, 0);
        assert_eq!(partes_da_previa(Some(&plano), &detalhe, true), (Some("Plano para aprovar".into()), String::new()));
    }

    #[test]
    fn sem_o_rotulo_a_direita_ele_vai_para_a_dica() {
        let dica = "O coordenador decide: aprovar ou pedir ajuste.";
        assert_eq!(dica_com_rotulo(Some("em revisão".into()), dica), format!("Em revisão\n{dica}"));
        // A dica que já começa pelo estado fica como está.
        assert_eq!(dica_com_rotulo(Some("parada".into()), "Parada: você interrompeu."), "Parada: você interrompeu.");
        assert_eq!(dica_com_rotulo(None, dica), dica);
        assert_eq!(dica_com_rotulo(Some(String::new()), dica), dica);
    }

    #[test]
    fn a_busca_diz_so_buscar() {
        assert_eq!(dica_da_busca(AbaLateral::Conversas, false), "Buscar");
        assert_eq!(dica_da_busca(AbaLateral::Conversas, true), "Buscar");
        assert_eq!(dica_da_busca(AbaLateral::Arquivos, false), "Buscar arquivos");
        assert_eq!(dica_da_busca(AbaLateral::Arquivos, true), "Buscar");
    }
}
