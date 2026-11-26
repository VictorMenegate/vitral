//! O mapa dos agentes de uma conversa: a conversa à esquerda e, ligados a ela, os subagentes que
//! ela abriu (as chamadas Task e Agent), cada um com o estado, quanto tempo levou e quantas
//! ferramentas usou; o clique num agente abre os passos e o resultado dele. Referência do dono
//! (13/11/2026): o "Agent map" da extensão do Claude Code.

use std::collections::{HashMap, HashSet};

use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{AnelGirando, contagem, largura, relogio},
    composer::nome_curto_modelo,
    dialogos::{cabecalho, moldura, topo},
    mensagens::{detalhe_da_chamada, detalhe_do_passo, duracao_curta, markdown, nome_curto, resumo_das_chamadas},
};
use crate::{
    importar::subagente::Sinal,
    modelo::{Agente as QuemResponde, Chat, Execucao, Item, agora},
    motor::{Motor, subconversa::id_da_subconversa},
    tema::{Cores, FONTE_MONO, Lucide, Tipo as _, esp, r, raio},
};

/// A altura da linha de um agente (o título e a linha de baixo), a 100%.
const ALTURA_DO_NO: f32 = 52.;
/// A largura da coluna da lista dos agentes, a 100%.
const LARGURA_DA_LISTA: f32 = 300.;
/// A altura da lista e do painel do agente escolhido, a 100%.
const ALTURA_DO_CORPO: f32 = 460.;
/// O diálogo é mais largo que o G: a lista e o agente escolhido lado a lado.
const LARGURA_DO_DIALOGO: f32 = 900.;
/// O pedido do agente fechado: 6 linhas inteiras do `corpo_ui` (20 de altura cada), a 100%.
const ALTURA_DO_PEDIDO: f32 = 120.;
/// A partir de quantas linhas ou letras o pedido vem fechado, com o "Mostrar tudo".
const LINHAS_DO_PEDIDO: usize = 6;
const LETRAS_DO_PEDIDO: usize = 420;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Estado {
    Rodando,
    Terminou,
    Falhou,
    /// Sem resultado e fora do turno que roda: o turno foi parado (ou caiu) com ele no meio.
    Interrompido,
}

/// Um subagente da conversa, pronto para desenhar.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Agente {
    pub id: String,
    pub titulo: String,
    pub estado: Estado,
    /// `None` quando a conversa não guardou o que ele fez (veio do histórico do Claude Code).
    pub ferramentas: Option<u32>,
    /// Os tokens que ele gastou (0: o Claude Code não disse).
    pub tokens: u64,
    pub segundos: Option<u64>,
    pub atividade: String,
    pub passos: Vec<String>,
    pub resultado: Option<String>,
    /// Quando a chamada que o abriu entrou na conversa (0: sem hora): ordena a lista.
    pub em: u64,
    /// Espera uma resposta sua a um pedido de permissão.
    pub esperando: bool,
}

pub(super) fn e_agente(item: &Item) -> bool {
    matches!(item, Item::Ferramenta { nome, .. } if matches!(nome_curto(nome), "Task" | "Agent"))
}

/// A conversa abriu algum subagente (o item do menu só aparece assim).
pub(super) fn tem_agentes(chat: &Chat) -> bool {
    chat.itens.iter().any(e_agente)
}

/// Os subagentes da conversa, na ordem em que foram abertos. `rodando`: há um turno em andamento
/// (sem ele, quem não tem resultado foi interrompido). Um agente sem resultado só conta como vivo
/// se nenhum aviso veio depois dele: o turno que para ou falha deixa um aviso na conversa.
pub(super) fn agentes_do_chat(itens: &[Item], rodando: bool, agora: u64) -> Vec<Agente> {
    agentes_do_chat_com(itens, rodando, agora, &HashMap::new(), &[])
}

/// Como `agentes_do_chat`, com o que o arquivo de cada subagente diz (`sinais`, pelo id da chamada)
/// e as chamadas que esperam uma resposta do usuário. Um agente sem resultado cujo arquivo termina
/// numa resposta só em texto terminou, e um cujo arquivo anda segue trabalhando mesmo com um aviso
/// de turno parado no meio: o turno da conversa caiu e foi retomado (com o `SendMessage`, por
/// exemplo), e o subagente não parou junto. Só sem arquivo vale a regra do aviso.
pub(super) fn agentes_do_chat_com(
    itens: &[Item],
    rodando: bool,
    agora: u64,
    sinais: &HashMap<String, Sinal>,
    esperando: &[String],
) -> Vec<Agente> {
    let ultimo_aviso = itens.iter().rposition(Item::aviso_que_fecha);
    itens
        .iter()
        .enumerate()
        .filter(|(_, item)| e_agente(item))
        .filter_map(|(posicao, item)| {
            let Item::Ferramenta { id, nome, resumo, resultado, erro, em, sub, .. } = item else {
                return None;
            };
            let vivo = resultado.is_none() && rodando && ultimo_aviso.is_none_or(|aviso| aviso < posicao);
            let estado = match (resultado.is_some(), *erro, vivo) {
                (_, _, true) => Estado::Rodando,
                (true, true, _) => Estado::Falhou,
                (true, false, _) => Estado::Terminou,
                (false, ..) => Estado::Interrompido,
            };
            // O que o arquivo do subagente diz vale mais que o aviso do turno da conversa.
            let estado = match (resultado.is_none(), sinais.get(id)) {
                (true, Some(Sinal::Trabalhando)) => Estado::Rodando,
                (true, Some(Sinal::Terminou)) => Estado::Terminou,
                (true, Some(Sinal::Parado)) => Estado::Interrompido,
                _ => estado,
            };
            let segundos = match (sub, estado) {
                (_, Estado::Rodando) if *em > 0 => Some(agora.saturating_sub(*em)),
                (Some(sub), _) if sub.fim >= *em && *em > 0 => Some(sub.fim - *em),
                _ => None,
            };
            Some(Agente {
                id: id.clone(),
                titulo: detalhe_da_chamada(nome, resumo),
                estado,
                ferramentas: sub.as_ref().map(|s| s.ferramentas),
                tokens: sub.as_ref().map_or(0, |s| s.tokens),
                segundos,
                atividade: sub.as_ref().map(|s| s.atividade.clone()).unwrap_or_default(),
                passos: sub.as_ref().map(|s| s.passos.clone()).unwrap_or_default(),
                resultado: resultado.clone(),
                em: *em,
                esperando: estado == Estado::Rodando && esperando.iter().any(|e| e == id),
            })
        })
        .collect()
}

/// A ordem da lista do mapa: quem está trabalhando em cima, depois quem espera uma resposta sua,
/// quem falhou e, por fim, quem terminou ou foi interrompido; em cada grupo, do mais recente ao
/// mais antigo (a ordem de abertura desempata).
pub(super) fn ordenar(agentes: &mut [Agente]) {
    let grupo = |a: &Agente| match (a.estado, a.esperando) {
        (Estado::Rodando, false) => 0,
        (Estado::Rodando, true) => 1,
        (Estado::Falhou, _) => 2,
        (Estado::Terminou | Estado::Interrompido, _) => 3,
    };
    // Estável: com a mesma hora, o aberto depois vem antes.
    agentes.reverse();
    agentes.sort_by_key(|a| (grupo(a), std::cmp::Reverse(a.em)));
}

/// Quantos subagentes desta conversa estão trabalhando agora (a pílula da linha de estado).
pub(super) fn agentes_vivos(motor: &Motor, chat: &Chat) -> usize {
    let rodando = matches!(motor.execucao(&chat.id), Execucao::Rodando { .. });
    if !rodando {
        return 0;
    }
    agentes_do_chat(&chat.itens, true, agora()).iter().filter(|a| a.estado == Estado::Rodando).count()
}

/// "437,7 mil" para os tokens do contexto.
pub(super) fn em_mil(n: u64) -> String {
    if n < 1_000 {
        return n.to_string();
    }
    let valor = format!("{:.1}", n as f64 / 1_000.);
    let valor = if crate::i18n::lingua() == crate::i18n::Lingua::Ingles { valor } else { valor.replace('.', ",") };
    tr!("{n} mil", n = valor).to_string()
}

/// A linha de baixo do nó de um agente: o que ele faz agora, ou quanto levou e quanto usou.
pub(super) fn meta_do_agente(a: &Agente) -> String {
    meta_com(a, true)
}

/// A linha de baixo na lista do mapa: sem os tokens, que o painel do escolhido mostra. Com eles,
/// a linha passava da coluna e cortava as ferramentas ("89 ferrame…"; dono, 26/11/2026).
fn meta_na_lista(a: &Agente) -> String {
    meta_com(a, false)
}

fn meta_com(a: &Agente, com_tokens: bool) -> String {
    let ferramentas = a.ferramentas.map(|n| contagem(n as usize, &tr!("ferramenta"), &tr!("ferramentas")));
    let tempo = a.segundos.map(|s| duracao_curta(s * 1_000));
    let tokens = (com_tokens && a.tokens > 0).then(|| format!("{} tokens", em_mil(a.tokens)));
    let numeros = [tempo, tokens, ferramentas].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    match a.estado {
        Estado::Rodando if a.esperando => tr!("Esperando você").to_string(),
        Estado::Rodando if !a.atividade.is_empty() => detalhe_do_passo(&a.atividade),
        Estado::Rodando => tr!("Começando…").to_string(),
        Estado::Terminou if numeros.is_empty() => tr!("Terminou").to_string(),
        Estado::Terminou => numeros,
        Estado::Falhou if numeros.is_empty() => tr!("Falhou").to_string(),
        Estado::Falhou => tr!("Falhou · {numeros}", numeros = numeros).to_string(),
        Estado::Interrompido if numeros.is_empty() => tr!("Interrompido").to_string(),
        Estado::Interrompido => tr!("Interrompido · {numeros}", numeros = numeros).to_string(),
    }
}

/// O ponto do estado, na coluna da esquerda do nó.
pub(super) fn ponto(estado: Estado) -> AnyElement {
    let disco = |cor: Hsla| div().size(r(8.)).rounded(raio::de(8.)).bg(cor).into_any_element();
    let dentro = match estado {
        Estado::Rodando => AnelGirando::new(12.).cor(Cores::destaque()).into_any_element(),
        Estado::Terminou => disco(Cores::verde()),
        Estado::Falhou => disco(Cores::vermelho()),
        Estado::Interrompido => disco(Cores::fantasma()),
    };
    div().flex_none().size(r(12.)).flex().items_center().justify_center().child(dentro).into_any_element()
}

/// A linha de cima de um nó: o ponto, o título e a linha de baixo, com o que vai à direita.
fn linha_do_no(ponto: AnyElement, titulo: SharedString, meta: SharedString, forte: bool, direita: Option<AnyElement>) -> Div {
    h_flex()
        .h(r(ALTURA_DO_NO))
        .px(esp::E12)
        .gap(esp::E8)
        .child(ponto)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .corpo_ui()
                        .font_weight(if forte { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                        .text_color(Cores::forte())
                        .child(titulo),
                )
                .child(div().truncate().metadado().text_color(Cores::tenue()).child(meta)),
        )
        .children(direita)
}

/// O que só o diálogo sabe: qual agente está aberto, o que o arquivo de cada um diz (lido de
/// tempos em tempos) e quais grupos de chamadas e pedidos longos estão abertos na conversa dele.
#[derive(Default)]
struct EstadoDoMapa {
    aberto: Option<String>,
    sinais: HashMap<String, Sinal>,
    expandidos: HashSet<usize>,
    fechado: bool,
}

/// O que o desenho do mapa precisa e não vem do motor.
struct Ligacoes {
    estado: Entity<EstadoDoMapa>,
    motor: Entity<Motor>,
    janela: WeakEntity<Janela>,
    /// O painel onde a aba do subagente abre (o ativo quando o diálogo abriu).
    painel: u64,
    chat: String,
}

/// A conversa do agente como um chat normal, no painel da direita: o pedido num balão, o que ele
/// respondeu em Markdown, as chamadas agrupadas e recolhíveis como na trilha da conversa, as
/// imagens em miniatura (o clique abre o visor) e, por último, o relatório. Os itens vêm da
/// conversa do subagente que o motor mantém aberta para o mapa (`abrir_subconversa_no_mapa`).
fn conversa_do_agente(ligacoes: &Ligacoes, sub_id: &str, itens: &[Item], expandidos: &HashSet<usize>, cx: &App) -> Vec<AnyElement> {
    let mut saida: Vec<AnyElement> = Vec::new();
    let mut i = 0;
    while i < itens.len() {
        match &itens[i] {
            Item::Usuario { texto, .. } | Item::Entrega { texto, .. } => {
                saida.push(balao_do_pedido(ligacoes, sub_id, i, texto.trim(), expandidos.contains(&i)));
                i += 1;
            }
            Item::Assistente { texto, .. } if !texto.trim().is_empty() => {
                saida.push(div().corpo_ui().child(markdown(SharedString::from(format!("agente-{sub_id}-{i}")), texto.clone(), cx)).into_any_element());
                i += 1;
            }
            Item::Ferramenta { .. } | Item::Raciocinio { .. } => {
                // Todas as chamadas seguidas, num grupo só.
                let inicio = i;
                while i < itens.len() && matches!(itens[i], Item::Ferramenta { .. } | Item::Raciocinio { .. }) {
                    i += 1;
                }
                let grupo: Vec<(usize, &Item)> = (inicio..i).map(|k| (k, &itens[k])).collect();
                saida.extend(grupo_de_chamadas(ligacoes, sub_id, inicio, &grupo, expandidos.contains(&inicio)));
            }
            Item::Aviso { texto, .. } => {
                saida.push(div().metadado().text_color(Cores::tenue()).child(texto.clone()).into_any_element());
                i += 1;
            }
            _ => i += 1,
        }
    }
    saida
}

/// O pedido que abriu o agente (ou outra mensagem para ele), num balão. Longo, ele mostra só o
/// começo e abre inteiro no clique. O `flex_none` é o que segura a altura: com o `overflow_hidden`,
/// a coluna que rola espremia o balão numa faixa de uma linha cortada (dono, 26/11/2026).
fn balao_do_pedido(ligacoes: &Ligacoes, sub_id: &str, i: usize, texto: &str, aberto: bool) -> AnyElement {
    let longo = texto.lines().count() > LINHAS_DO_PEDIDO || texto.chars().count() > LETRAS_DO_PEDIDO;
    let estado = ligacoes.estado.clone();
    v_flex()
        .id(SharedString::from(format!("pedido-{sub_id}-{i}")))
        .flex_none()
        .gap(esp::E4)
        .px(esp::E12)
        .py(esp::E8)
        .rounded(raio::CARTAO)
        .bg(Cores::balao())
        .child(
            div()
                .corpo_ui()
                .text_color(Cores::forte())
                .when(longo && !aberto, |s| s.max_h(r(ALTURA_DO_PEDIDO)).overflow_hidden())
                .child(texto.to_string()),
        )
        .when(longo, |s| {
            s.cursor_pointer()
                .child(div().metadado().text_color(Cores::tenue()).child(if aberto { tr!("Mostrar menos") } else { tr!("Mostrar tudo") }))
                .on_click(move |_, window, cx| {
                    estado.update(cx, |e, cx| {
                        if !e.expandidos.remove(&i) {
                            e.expandidos.insert(i);
                        }
                        cx.notify();
                    });
                    window.refresh();
                })
        })
        .into_any_element()
}

/// Um grupo de chamadas: a linha de resumo (clique abre e fecha), as chamadas dele e as imagens que
/// elas trouxeram.
fn grupo_de_chamadas(ligacoes: &Ligacoes, sub_id: &str, inicio: usize, grupo: &[(usize, &Item)], aberto: bool) -> Vec<AnyElement> {
    let chamadas: Vec<&Item> = grupo.iter().map(|(_, i)| *i).filter(|i| matches!(i, Item::Ferramenta { .. })).collect();
    let imagens: Vec<std::path::PathBuf> = chamadas
        .iter()
        .filter_map(|i| match i {
            Item::Ferramenta { imagens, .. } => Some(imagens.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    let mut saida: Vec<AnyElement> = Vec::new();
    if !chamadas.is_empty() {
        let resumo = resumo_das_chamadas(grupo, false);
        let resumo = if resumo.is_empty() { contagem(chamadas.len(), &tr!("chamada"), &tr!("chamadas")) } else { resumo };
        let estado = ligacoes.estado.clone();
        let linhas = aberto.then(|| {
            v_flex().pl(esp::E16).gap(esp::E2).children(chamadas.iter().map(|item| {
                let Item::Ferramenta { nome, resumo, erro, .. } = item else {
                    return div().into_any_element();
                };
                let passo = detalhe_do_passo(&format!("{} · {}", nome_curto(nome), detalhe_da_chamada(nome, resumo)));
                let (verbo, objeto) = passo.split_once(' ').unwrap_or((passo.as_str(), ""));
                h_flex()
                    .gap(esp::E6)
                    .child(div().flex_none().metadado().text_color(if *erro { Cores::vermelho() } else { Cores::tenue() }).child(verbo.to_string()))
                    .child(div().min_w_0().truncate().metadado().font_family(FONTE_MONO).text_color(Cores::texto()).child(objeto.to_string()))
                    .into_any_element()
            }))
        });
        saida.push(
            v_flex()
                .gap(esp::E4)
                .child(
                    h_flex()
                        .id(SharedString::from(format!("grupo-{sub_id}-{inicio}")))
                        .gap(esp::E6)
                        .items_center()
                        .cursor_pointer()
                        .text_color(Cores::tenue())
                        .hover(|s| s.text_color(Cores::texto()))
                        .child(super::mensagens::icone_de(if aberto { Lucide::ChevronDown } else { Lucide::ChevronRight }, 12.))
                        .child(div().min_w_0().truncate().metadado().child(resumo))
                        .on_click(move |_, window, cx| {
                            estado.update(cx, |e, cx| {
                                if !e.expandidos.remove(&inicio) {
                                    e.expandidos.insert(inicio);
                                }
                                cx.notify();
                            });
                            window.refresh();
                        }),
                )
                .children(linhas)
                .into_any_element(),
        );
    }
    if !imagens.is_empty() {
        let miniaturas = imagens.into_iter().enumerate().map(|(k, caminho)| {
            let (janela, sub_id, abrir) = (ligacoes.janela.clone(), sub_id.to_string(), caminho.clone());
            div()
                .id(SharedString::from(format!("imagem-{sub_id}-{inicio}-{k}")))
                .flex_none()
                .size(r(72.))
                .rounded(raio::CONTROLE)
                .overflow_hidden()
                .bg(Cores::preto())
                .cursor_pointer()
                .hover(|s| s.opacity(0.85))
                .child(img(caminho).size_full().object_fit(ObjectFit::Cover))
                .on_click(move |_, window, cx| {
                    let (sub_id, abrir) = (sub_id.clone(), abrir.clone());
                    let _ = janela.update(cx, |j, cx| j.abrir_visor(&sub_id, &abrir, window, cx));
                })
        });
        saida.push(h_flex().flex_wrap().gap(esp::E8).children(miniaturas).into_any_element());
    }
    saida
}

/// O painel da direita: o agente escolhido, com o que ele está fazendo (ou quanto levou), a conversa
/// dele como a de um chat normal e o botão que a abre numa aba. Sem a conversa (o Claude Code já a
/// apagou, ou a leitura ainda não voltou), os passos e o relatório que o cartão guardou.
fn painel_do_agente(i: usize, a: &Agente, ligacoes: &Ligacoes, expandidos: &HashSet<usize>, motor: &Motor, window: &Window, cx: &App) -> Div {
    let sub_id = id_da_subconversa(&ligacoes.chat, &a.id);
    let itens: Option<Vec<Item>> = motor.subconversa(&sub_id).map(|s| s.chat.itens.clone()).filter(|itens| !itens.is_empty());
    let passos: Vec<String> = a.passos.iter().map(|p| detalhe_do_passo(p)).map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    let titulo_de_secao = |texto: SharedString| super::componentes::etiqueta_de_secao(texto);
    let corpo: Vec<AnyElement> = match &itens {
        Some(itens) => conversa_do_agente(ligacoes, &sub_id, itens, expandidos, cx),
        None => {
            let lista_de_passos = (!passos.is_empty()).then(|| {
                v_flex()
                    .gap(esp::E2)
                    .child(titulo_de_secao(if passos.len() == 1 { tr!("1 passo") } else { tr!("Passos") }))
                    .children(passos.iter().map(|p| div().truncate().metadado().font_family(FONTE_MONO).text_color(Cores::texto()).child(p.clone())))
                    .into_any_element()
            });
            let resultado = a.resultado.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(|texto| {
                let curto: String = texto.chars().take(8_000).collect();
                v_flex()
                    .gap(esp::E4)
                    .child(titulo_de_secao(tr!("Relatório")))
                    .child(div().corpo_ui().child(markdown(SharedString::from(format!("relatorio-{sub_id}")), curto, cx)))
                    .into_any_element()
            });
            let vazio = (lista_de_passos.is_none() && resultado.is_none()).then(|| {
                div()
                    .corpo_ui()
                    .text_color(Cores::tenue())
                    .child(match a.estado {
                        Estado::Rodando => tr!("Lendo a conversa do agente…"),
                        _ => tr!("A conversa não guardou os passos deste agente."),
                    })
                    .into_any_element()
            });
            lista_de_passos.into_iter().chain(resultado).chain(vazio).collect()
        }
    };
    let (janela, chat, chamada, painel) = (ligacoes.janela.clone(), ligacoes.chat.clone(), a.id.clone(), ligacoes.painel);
    let abrir_aba = super::componentes::botao_fantasma(
        ("agente-abrir-aba", i),
        Some(Lucide::PanelRight.into()),
        tr!("Abrir em aba"),
        super::componentes::TamanhoBotao::Controle,
        window,
        cx,
    )
    .bg(Cores::recuo())
    .on_click(move |_, window, cx| {
        window.close_dialog(cx);
        let (chat, chamada) = (chat.clone(), chamada.clone());
        let _ = janela.update(cx, |j, cx| j.abrir_subagente(painel, chat, chamada, window, cx));
    });
    v_flex()
        .size_full()
        .gap(esp::E12)
        .child(
            h_flex()
                .gap(esp::E8)
                .items_start()
                .child(div().flex_none().pt(esp::E4).child(ponto(a.estado)))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(esp::E2)
                        .child(div().corpo_ui().font_weight(FontWeight::SEMIBOLD).text_color(Cores::forte()).child(a.titulo.clone()))
                        .child(div().metadado().text_color(Cores::tenue()).child(meta_do_agente(a))),
                )
                .child(div().flex_none().child(abrir_aba)),
        )
        .child(
            v_flex()
                .id(("conversa-do-agente", i))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .gap(esp::E12)
                .children(corpo),
        )
}
/// O agente que abre escolhido: o primeiro da lista (o que trabalha, ou o mais recente).
fn escolhido_padrao(agentes: &[Agente]) -> Option<String> {
    agentes.first().map(|a| a.id.clone())
}

/// A tela dos agentes: em cima, a conversa (o modelo e o contexto) e a conta de quantos
/// trabalham, terminaram e falharam; embaixo, a lista de todos à esquerda, com a sua rolagem, e o
/// escolhido à direita, como uma conversa. Era uma árvore com o agente aberto dentro dela: com 10
/// agentes, só dois cabiam e o resto ficava escondido embaixo (dono, 23/11/2026: "tem que melhorar
/// bastante essa parte pra ver os agents").
fn mapa(motor: &Motor, chat: &Chat, agentes: &[Agente], ligacoes: &Ligacoes, window: &Window, cx: &App) -> AnyElement {
    let execucao = motor.execucao(&chat.id);
    let rodando = matches!(execucao, Execucao::Rodando { .. });
    let modelo = match (chat.agente, chat.modelo_atual.as_deref().filter(|m| !m.is_empty())) {
        (QuemResponde::Codex, _) => "Codex".to_string(),
        (QuemResponde::Gemini, Some(m)) => crate::agente_gemini::nome_do_modelo(m),
        (QuemResponde::Gemini, None) => "Gemini".to_string(),
        (_, Some(m)) => nome_curto_modelo(m),
        _ => "Claude".to_string(),
    };
    let meta_da_raiz = match chat.contexto {
        Some(tokens) => tr!("{modelo} · {tokens} no contexto", modelo = modelo, tokens = em_mil(tokens)).to_string(),
        None => modelo,
    };
    let estado_da_raiz = match execucao {
        Execucao::Rodando { .. } => Estado::Rodando,
        Execucao::Erro(_) => Estado::Falhou,
        _ => Estado::Terminou,
    };
    let conta = |e: Estado| agentes.iter().filter(|a| a.estado == e).count();
    let resumo: Vec<String> = [
        (conta(Estado::Rodando), "trabalhando", "trabalhando"),
        (conta(Estado::Terminou), "terminou", "terminaram"),
        (conta(Estado::Falhou), "falhou", "falharam"),
        (conta(Estado::Interrompido), "interrompido", "interrompidos"),
    ]
    .into_iter()
    .filter(|(n, ..)| *n > 0)
    .map(|(n, um, varios)| format!("{n} {}", crate::i18n::traduzir(if n == 1 { um } else { varios })))
    .collect();
    let conversa = h_flex()
        .gap(esp::E8)
        .px(esp::E12)
        .py(esp::E8)
        .rounded(raio::CARTAO)
        .bg(Cores::recuo())
        .child(ponto(estado_da_raiz))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().truncate().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child(chat.titulo.clone()))
                .child(div().truncate().metadado().text_color(Cores::tenue()).child(meta_da_raiz)),
        )
        .when(!resumo.is_empty(), |s| s.child(div().flex_none().metadado().text_color(Cores::tenue()).child(resumo.join(" · "))));
    if agentes.is_empty() {
        let aviso = if rodando {
            tr!("Este turno ainda não abriu nenhum agente. Quando o Claude dividir o trabalho com subagentes, eles aparecem aqui.")
        } else {
            tr!("Esta conversa não abriu nenhum agente. Quando o Claude divide o trabalho com subagentes, eles aparecem aqui.")
        };
        return v_flex().gap(esp::E12).child(conversa).child(div().px(esp::E4).corpo_ui().text_color(Cores::tenue()).child(aviso)).into_any_element();
    }
    let (aberto, expandidos) = {
        let e = ligacoes.estado.read(cx);
        (e.aberto.clone(), e.expandidos.clone())
    };
    let escolhido = aberto.filter(|id| agentes.iter().any(|a| a.id == *id)).or_else(|| escolhido_padrao(agentes));
    let linhas = agentes.iter().enumerate().map(|(i, a)| {
        let selecionado = escolhido.as_deref() == Some(a.id.as_str());
        let relogio_vivo = (a.estado == Estado::Rodando)
            .then(|| a.segundos.map(|s| div().flex_none().etiqueta().text_color(Cores::apagado()).child(relogio(s)).into_any_element()))
            .flatten();
        let (estado, motor_e, chat_id, id) = (ligacoes.estado.clone(), ligacoes.motor.clone(), ligacoes.chat.clone(), a.id.clone());
        div()
            .id(("agente", i))
            .rounded(raio::CARTAO)
            // O escolhido só com o fundo neutro, sem a borda azul (o "é aqui" do resto do app).
            .when(selecionado, |s| s.bg(Cores::pressionado()))
            .when(!selecionado, |s| s.hover(|s| s.bg(Cores::hover())))
            .cursor_pointer()
            .child(linha_do_no(ponto(a.estado), a.titulo.clone().into(), meta_na_lista(a).into(), false, relogio_vivo))
            .on_click(move |_, window, cx| {
                estado.update(cx, |e, cx| {
                    e.aberto = Some(id.clone());
                    e.expandidos.clear();
                    cx.notify();
                });
                // A conversa dele passa a ser lida para o painel da direita.
                motor_e.update(cx, |m, cx| m.abrir_subconversa_no_mapa(&chat_id, &id, cx));
                window.refresh();
            })
    });
    let painel = agentes
        .iter()
        .enumerate()
        .find(|(_, a)| escolhido.as_deref() == Some(a.id.as_str()))
        .map(|(i, a)| painel_do_agente(i, a, ligacoes, &expandidos, motor, window, cx));
    v_flex()
        .gap(esp::E12)
        .child(conversa)
        .child(
            h_flex()
                .items_start()
                .gap(esp::E12)
                .h(r(ALTURA_DO_CORPO))
                .child(
                    v_flex()
                        .id("lista-de-agentes")
                        .flex_none()
                        .w(r(LARGURA_DA_LISTA))
                        .h_full()
                        .overflow_y_scroll()
                        .gap(esp::E2)
                        .children(linhas),
                )
                .child(div().flex_none().w(px(1.)).h_full().bg(Cores::linha()))
                .child(div().flex_1().min_w_0().h_full().children(painel)),
        )
        .into_any_element()
}

/// Os agentes desta conversa na ordem da lista, com o que o arquivo de cada um diz.
fn agentes_ordenados(c: &Chat, rodando: bool, m: &Motor, sinais: &HashMap<String, Sinal>) -> Vec<Agente> {
    let esperando: Vec<String> = m.subagentes_esperando(&c.id);
    let mut agentes = agentes_do_chat_com(&c.itens, rodando, agora(), sinais, &esperando);
    ordenar(&mut agentes);
    agentes
}

impl Janela {
    /// Abre o mapa dos agentes da conversa. O diálogo lê a conversa a cada quadro: os agentes que
    /// trabalham andam ao vivo, com o relógio. A cada 2 s, enquanto aberto, o fim do arquivo de
    /// cada agente sem resultado é lido: é ele que diz se um agente que o cartão deu por parado
    /// trabalha ou terminou (o turno da conversa pode ter caído e voltado com ele no meio).
    pub(super) fn abrir_mapa_de_agentes(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let painel = self.paineis.get(self.ativo).map_or(0, |p| p.id);
        let estado: Entity<EstadoDoMapa> = cx.new(|_| EstadoDoMapa::default());
        // O primeiro da lista abre já com a conversa dele lida.
        {
            let m = motor.read(cx);
            if let Some(c) = m.chat(&chat) {
                let rodando = matches!(m.execucao(&c.id), Execucao::Rodando { .. });
                if let Some(id) = escolhido_padrao(&agentes_ordenados(c, rodando, m, &HashMap::new())) {
                    estado.update(cx, |e, _| e.aberto = Some(id.clone()));
                    motor.update(cx, |m, cx| m.abrir_subconversa_no_mapa(&chat, &id, cx));
                }
            }
        }
        // A leitura do fim dos arquivos, em segundo plano.
        let (motor_l, estado_l, chat_l) = (motor.clone(), estado.clone(), chat.clone());
        cx.spawn(async move |_, cx| {
            loop {
                let pedido = motor_l.update(cx, |m, _| {
                    let c = m.chat(&chat_l)?;
                    let sem_resultado: Vec<String> = c
                        .itens
                        .iter()
                        .filter_map(|i| match i {
                            Item::Ferramenta { id, resultado: None, .. } if e_agente(i) => Some(id.clone()),
                            _ => None,
                        })
                        .collect();
                    let (projetos, sessao) = m.onde_ficam_os_subagentes(&chat_l)?;
                    Some((sem_resultado, projetos, sessao))
                });
                if estado_l.read_with(cx, |e, _| e.fechado) {
                    break;
                }
                if let Some((chamadas, projetos, sessao)) = pedido
                    && !chamadas.is_empty()
                {
                    let sinais = cx
                        .background_spawn(async move {
                            chamadas
                                .into_iter()
                                .filter_map(|chamada| {
                                    let arquivo = crate::importar::subagente::transcript(&projetos, &sessao, &chamada)?;
                                    Some((chamada, crate::importar::subagente::sinal_do_arquivo(&arquivo)?))
                                })
                                .collect::<HashMap<_, _>>()
                        })
                        .await;
                    let _ = estado_l.update(cx, |e, cx| {
                        if e.sinais != sinais {
                            e.sinais = sinais;
                            cx.notify();
                        }
                    });
                }
                cx.background_executor().timer(std::time::Duration::from_secs(2)).await;
            }
        })
        .detach();
        let ligacoes = Ligacoes { estado: estado.clone(), motor: motor.clone(), janela: cx.weak_entity(), painel, chat: chat.clone() };
        let (motor_f, estado_f) = (motor.clone(), estado.clone());
        self.foco_antes_do_dialogo(window, cx);
        window.open_dialog(cx, move |dialogo, window, cx| {
            let m = motor.read(cx);
            let Some(c) = m.chat(&chat) else {
                return moldura(dialogo, largura::G, window).title(topo(cabecalho(tr!("Agentes desta conversa"), None)));
            };
            let rodando = matches!(m.execucao(&c.id), Execucao::Rodando { .. });
            let agentes = agentes_ordenados(c, rodando, m, &ligacoes.estado.read(cx).sinais);
            let vivos = agentes.iter().filter(|a| a.estado == Estado::Rodando).count();
            let subtitulo = match (agentes.len(), vivos) {
                (0, _) => tr!("Os subagentes que a conversa abre").to_string(),
                (n, 0) => tr!("{quantos} · clique num para ver a conversa dele", quantos = contagem(n, &tr!("agente"), &tr!("agentes"))).to_string(),
                (n, v) => tr!("{quantos} · {v} trabalhando · clique num para ver a conversa dele", quantos = contagem(n, &tr!("agente"), &tr!("agentes")), v = v).to_string(),
            };
            let corpo = mapa(m, c, &agentes, &ligacoes, window, cx);
            moldura(dialogo, LARGURA_DO_DIALOGO, window)
                .title(topo(cabecalho(tr!("Agentes desta conversa"), Some(subtitulo))))
                .on_close({
                    let (motor_fechou, estado_fechou) = (motor_f.clone(), estado_f.clone());
                    move |_, _, cx| {
                        estado_fechou.update(cx, |e, _| e.fechado = true);
                        motor_fechou.update(cx, |m, _| m.mapa_de_agentes_fechou());
                    }
                })
                .child(corpo)
        });
    }
}

#[cfg(test)]
mod testes {
    use std::collections::HashMap;

    use super::{Agente, Estado, agentes_do_chat, agentes_do_chat_com, em_mil, meta_do_agente, ordenar};
    use crate::importar::subagente::Sinal;
    use crate::modelo::{Item, Subagente};

    fn agente(id: &str, resultado: Option<&str>, erro: bool, em: u64, sub: Option<Subagente>) -> Item {
        Item::Ferramenta {
            id: id.into(),
            nome: "Task".into(),
            resumo: "Task · Explorar o carrinho".into(),
            resultado: resultado.map(str::to_string),
            erro,
            em,
            detalhe: None,
            arquivo: None,
            sub,
            mudanca: None,
            imagens: Vec::new(),
            durou: 0,
        }
    }

    fn sub(ferramentas: u32, atividade: &str, fim: u64) -> Subagente {
        Subagente { ferramentas, atividade: atividade.into(), passos: vec!["Read · a.rs".into()], fim, ..Default::default() }
    }

    #[test]
    fn estados_dos_agentes_da_conversa() {
        let itens = vec![
            // De um turno que foi parado: sem resultado, e o aviso veio depois.
            agente("a", None, false, 100, None),
            Item::Aviso { texto: "Interrompido.".into(), erro: false, em: 110 },
            agente("b", Some("pronto"), false, 200, Some(sub(3, "Read · a.rs", 260))),
            agente("c", Some("deu ruim"), true, 200, Some(sub(1, "", 230))),
            agente("d", None, false, 300, Some(sub(2, "Grep · cupom", 0))),
            Item::Ferramenta {
                id: "x".into(),
                nome: "Read".into(),
                resumo: "Read · a.rs".into(),
                resultado: None,
                erro: false,
                em: 300,
                detalhe: None,
                arquivo: None,
                sub: None,
                mudanca: None,
                imagens: Vec::new(),
                durou: 0,
            },
        ];
        let agentes = agentes_do_chat(&itens, true, 345);
        let estados: Vec<(&str, Estado, Option<u64>)> = agentes.iter().map(|a| (a.id.as_str(), a.estado, a.segundos)).collect();
        assert_eq!(
            estados,
            [
                ("a", Estado::Interrompido, None),
                ("b", Estado::Terminou, Some(60)),
                ("c", Estado::Falhou, Some(30)),
                ("d", Estado::Rodando, Some(45)),
            ]
        );
        assert_eq!(agentes[1].titulo, "Explorar o carrinho");
        // Com o turno parado, quem não tem resultado foi interrompido.
        assert_eq!(agentes_do_chat(&itens, false, 345)[3].estado, Estado::Interrompido);
    }

    /// Item 2: o agente que segue em segundo plano depois da resposta continua rodando no mapa (o
    /// aviso de que os agentes seguem não é o de um turno que parou).
    #[test]
    fn o_agente_que_segue_em_segundo_plano_continua_rodando() {
        let itens = vec![
            agente("a", None, false, 100, Some(sub(2, "Bash · cargo test", 0))),
            Item::Assistente { texto: "Lancei.".into(), em: 105 },
            Item::Aviso { texto: crate::modelo::AGENTES_SEGUEM.into(), erro: false, em: 106 },
        ];
        let agentes = agentes_do_chat(&itens, true, 160);
        assert_eq!((agentes[0].estado, agentes[0].segundos), (Estado::Rodando, Some(60)));
    }

    #[test]
    fn a_linha_de_baixo_diz_o_que_faz_ou_quanto_levou() {
        let base = Agente {
            id: "a".into(),
            titulo: "t".into(),
            estado: Estado::Rodando,
            ferramentas: Some(3),
            tokens: 0,
            segundos: Some(493),
            atividade: "Read · admin.py".into(),
            passos: Vec::new(),
            resultado: None,
            em: 0,
            esperando: false,
        };
        assert_eq!(meta_do_agente(&base), "Leu admin.py");
        assert_eq!(meta_do_agente(&Agente { atividade: String::new(), ..base.clone() }), "Começando…");
        assert_eq!(meta_do_agente(&Agente { estado: Estado::Terminou, ..base.clone() }), "8 min 13 s · 3 ferramentas");
        assert_eq!(meta_do_agente(&Agente { estado: Estado::Falhou, ferramentas: Some(1), ..base.clone() }), "Falhou · 8 min 13 s · 1 ferramenta");
        // Do histórico do Claude Code não vem o que ele fez.
        assert_eq!(meta_do_agente(&Agente { estado: Estado::Terminou, ferramentas: None, segundos: None, ..base }), "Terminou");
        assert_eq!(em_mil(437_700), "437,7 mil");
        assert_eq!(em_mil(900), "900");
    }

    fn com_estado(id: &str, estado: Estado, em: u64, esperando: bool) -> Agente {
        Agente {
            id: id.into(),
            titulo: id.into(),
            estado,
            ferramentas: None,
            tokens: 0,
            segundos: None,
            atividade: String::new(),
            passos: Vec::new(),
            resultado: None,
            em,
            esperando,
        }
    }

    /// Pedido 4: rodando em cima; depois quem espera resposta, quem falhou, e por fim os terminados
    /// e interrompidos, do mais recente ao mais antigo em cada grupo.
    #[test]
    fn a_lista_poe_quem_roda_em_cima_e_ordena_cada_grupo_do_mais_recente() {
        let mut lista = vec![
            com_estado("terminou-velho", Estado::Terminou, 10, false),
            com_estado("rodando-velho", Estado::Rodando, 20, false),
            com_estado("falhou", Estado::Falhou, 70, false),
            com_estado("interrompido-novo", Estado::Interrompido, 60, false),
            com_estado("esperando", Estado::Rodando, 30, true),
            com_estado("rodando-novo", Estado::Rodando, 50, false),
            com_estado("terminou-novo", Estado::Terminou, 80, false),
        ];
        ordenar(&mut lista);
        let ids: Vec<&str> = lista.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(
            ids,
            ["rodando-novo", "rodando-velho", "esperando", "falhou", "terminou-novo", "interrompido-novo", "terminou-velho"]
        );
        // Sem hora, o aberto depois vem antes.
        let mut sem_hora = vec![com_estado("a", Estado::Terminou, 0, false), com_estado("b", Estado::Terminou, 0, false)];
        ordenar(&mut sem_hora);
        assert_eq!(sem_hora.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
    }

    /// Complemento do pedido 4: o agente que o aviso do turno deu por interrompido, mas cujo arquivo
    /// anda (o turno da conversa caiu e voltou, o subagente seguiu), trabalha; se o arquivo termina
    /// na resposta dele, terminou; se parou de andar, segue interrompido.
    #[test]
    fn o_arquivo_do_agente_vale_mais_que_o_aviso_do_turno_parado() {
        let itens = vec![
            agente("a", None, false, 100, None),
            Item::Aviso { texto: "Interrompido.".into(), erro: false, em: 110 },
            agente("b", None, false, 120, None),
            Item::Aviso { texto: "Interrompido.".into(), erro: false, em: 130 },
        ];
        // O turno retomado roda, mas o aviso veio depois dos dois: sem o arquivo, interrompidos.
        let sem = agentes_do_chat_com(&itens, true, 200, &HashMap::new(), &[]);
        assert!(sem.iter().all(|a| a.estado == Estado::Interrompido));
        let sinais = HashMap::from([("a".to_string(), Sinal::Trabalhando), ("b".to_string(), Sinal::Terminou)]);
        let com = agentes_do_chat_com(&itens, true, 200, &sinais, &[]);
        assert_eq!((com[0].estado, com[1].estado), (Estado::Rodando, Estado::Terminou));
        let parado = HashMap::from([("a".to_string(), Sinal::Parado)]);
        assert_eq!(agentes_do_chat_com(&itens, true, 200, &parado, &[])[0].estado, Estado::Interrompido);
        // Com resultado gravado, o arquivo não muda nada.
        let pronto = vec![agente("c", Some("ok"), false, 100, None)];
        let engana = HashMap::from([("c".to_string(), Sinal::Trabalhando)]);
        assert_eq!(agentes_do_chat_com(&pronto, true, 200, &engana, &[])[0].estado, Estado::Terminou);
    }

    #[test]
    fn o_agente_que_pede_permissao_espera_voce() {
        let itens = vec![agente("a", None, false, 100, Some(sub(1, "Bash · ls", 0)))];
        let agentes = agentes_do_chat_com(&itens, true, 120, &HashMap::new(), &["a".to_string()]);
        assert!(agentes[0].esperando);
        assert_eq!(meta_do_agente(&agentes[0]), "Esperando você");
        assert!(!agentes_do_chat_com(&itens, true, 120, &HashMap::new(), &[])[0].esperando);
    }
}
