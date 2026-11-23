//! O mapa dos agentes de uma conversa: a conversa à esquerda e, ligados a ela, os subagentes que
//! ela abriu (as chamadas Task e Agent), cada um com o estado, quanto tempo levou e quantas
//! ferramentas usou; o clique num agente abre os passos e o resultado dele. Referência do dono
//! (13/11/2026): o "Agent map" da extensão do Claude Code.

use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{AnelGirando, contagem, largura, relogio},
    composer::nome_curto_modelo,
    dialogos::{cabecalho, moldura, topo},
    mensagens::{detalhe_da_chamada, detalhe_do_passo, duracao_curta, nome_curto},
};
use crate::{
    modelo::{Agente as QuemResponde, Chat, Execucao, Item, agora},
    motor::Motor,
    tema::{Cores, FONTE_MONO, Tipo as _, esp, r, raio},
};

/// A altura da linha de um agente (o título e a linha de baixo), a 100%.
const ALTURA_DO_NO: f32 = 52.;
/// A largura da coluna da lista dos agentes, a 100%.
const LARGURA_DA_LISTA: f32 = 300.;
/// A altura da lista e do painel do agente escolhido, a 100%.
const ALTURA_DO_CORPO: f32 = 460.;
/// O diálogo é mais largo que o G: a lista e o agente escolhido lado a lado.
const LARGURA_DO_DIALOGO: f32 = 900.;

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
}

fn e_agente(item: &Item) -> bool {
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
    let ultimo_aviso = itens.iter().rposition(|i| matches!(i, Item::Aviso { .. }));
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
            })
        })
        .collect()
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
    format!("{:.1} mil", n as f64 / 1_000.).replace('.', ",")
}

/// A linha de baixo do nó de um agente: o que ele faz agora, ou quanto levou e quanto usou.
fn meta_do_agente(a: &Agente) -> String {
    let ferramentas = a.ferramentas.map(|n| contagem(n as usize, "ferramenta", "ferramentas"));
    let tempo = a.segundos.map(|s| duracao_curta(s * 1_000));
    let tokens = (a.tokens > 0).then(|| format!("{} tokens", em_mil(a.tokens)));
    let numeros = [tempo, tokens, ferramentas].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    match a.estado {
        Estado::Rodando if !a.atividade.is_empty() => detalhe_do_passo(&a.atividade),
        Estado::Rodando => "Começando…".to_string(),
        Estado::Terminou if numeros.is_empty() => "Terminou".to_string(),
        Estado::Terminou => numeros,
        Estado::Falhou if numeros.is_empty() => "Falhou".to_string(),
        Estado::Falhou => format!("Falhou · {numeros}"),
        Estado::Interrompido if numeros.is_empty() => "Interrompido".to_string(),
        Estado::Interrompido => format!("Interrompido · {numeros}"),
    }
}

/// O ponto do estado, na coluna da esquerda do nó.
fn ponto(estado: Estado) -> AnyElement {
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

/// O painel da direita: o agente escolhido, com o que ele está fazendo (ou quanto levou), os
/// passos dele e o resultado, cada parte com a sua rolagem. Passo vazio não entra (deixava um
/// buraco em branco em cima do resultado; dono, 23/11/2026).
fn painel_do_agente(i: usize, a: &Agente) -> Div {
    let passos: Vec<String> = a.passos.iter().map(|p| detalhe_do_passo(p)).map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    let titulo_de_secao = |texto: &'static str| super::componentes::etiqueta_de_secao(texto);
    let lista_de_passos = (!passos.is_empty()).then(|| {
        let n = passos.len();
        v_flex()
            .gap(esp::E4)
            .child(titulo_de_secao(if n == 1 { "1 passo" } else { "Passos" }))
            .child(
                v_flex()
                    .id(("passos-do-agente", i))
                    .max_h(r(200.))
                    .overflow_y_scroll()
                    .gap(esp::E2)
                    .children(passos.into_iter().enumerate().map(|(k, p)| {
                        h_flex()
                            .gap(esp::E8)
                            .items_start()
                            .child(div().flex_none().w(r(22.)).metadado().text_color(Cores::apagado()).child(format!("{}", k + 1)))
                            // O verbo na letra do texto, e só o objeto (o arquivo, o comando) em mono, como na
                            // trilha da conversa.
                            .child({
                                let (verbo, objeto) = p.split_once(' ').unwrap_or((p.as_str(), ""));
                                h_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(esp::E6)
                                    .child(div().flex_none().metadado().text_color(Cores::tenue()).child(verbo.to_string()))
                                    .child(div().min_w_0().truncate().metadado().font_family(FONTE_MONO).text_color(Cores::texto()).child(objeto.to_string()))
                            })
                    })),
            )
    });
    let resultado = a.resultado.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(|texto| {
        let curto: String = texto.chars().take(8_000).collect();
        v_flex().gap(esp::E4).min_h_0().child(titulo_de_secao("Resultado")).child(
            div()
                .id(("resultado-do-agente", i))
                .max_h(r(300.))
                .overflow_y_scroll()
                .p(esp::E12)
                .rounded(raio::CONTROLE)
                .bg(Cores::preto())
                .corpo_ui()
                .text_color(Cores::texto())
                .child(curto),
        )
    });
    let vazio = (lista_de_passos.is_none() && resultado.is_none()).then(|| {
        div().corpo_ui().text_color(Cores::tenue()).child(match a.estado {
            Estado::Rodando => "Ainda não usou nenhuma ferramenta.",
            _ => "A conversa não guardou os passos deste agente.",
        })
    });
    v_flex()
        .size_full()
        .gap(esp::E12)
        .child(
            h_flex().gap(esp::E8).items_start().child(div().flex_none().pt(esp::E4).child(ponto(a.estado))).child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(esp::E2)
                    .child(div().corpo_ui().font_weight(FontWeight::SEMIBOLD).text_color(Cores::forte()).child(a.titulo.clone()))
                    .child(div().metadado().text_color(Cores::tenue()).child(meta_do_agente(a))),
            ),
        )
        .children(lista_de_passos)
        .children(resultado)
        .children(vazio)
}

/// O agente que abre escolhido: o que está trabalhando, ou o último.
fn escolhido_padrao(agentes: &[Agente]) -> Option<String> {
    agentes.iter().find(|a| a.estado == Estado::Rodando).or_else(|| agentes.last()).map(|a| a.id.clone())
}

/// A tela dos agentes: em cima, a conversa (o modelo e o contexto) e a conta de quantos
/// trabalham, terminaram e falharam; embaixo, a lista de todos à esquerda, com a sua rolagem, e o
/// escolhido à direita, com os passos e o resultado. Era uma árvore com o agente aberto dentro
/// dela: com 10 agentes, só dois cabiam e o resto ficava escondido embaixo (dono, 23/11/2026:
/// "tem que melhorar bastante essa parte pra ver os agents").
fn mapa(motor: &Motor, chat: &Chat, agentes: &[Agente], aberto: Option<&str>, estado: &Entity<Option<String>>) -> AnyElement {
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
        Some(tokens) => format!("{modelo} · {} no contexto", em_mil(tokens)),
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
    .map(|(n, um, varios)| format!("{n} {}", if n == 1 { um } else { varios }))
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
            "Este turno ainda não abriu nenhum agente. Quando o Claude dividir o trabalho com subagentes, eles aparecem aqui."
        } else {
            "Esta conversa não abriu nenhum agente. Quando o Claude divide o trabalho com subagentes, eles aparecem aqui."
        };
        return v_flex().gap(esp::E12).child(conversa).child(div().px(esp::E4).corpo_ui().text_color(Cores::tenue()).child(aviso)).into_any_element();
    }
    let escolhido = aberto.filter(|id| agentes.iter().any(|a| a.id == *id)).map(str::to_string).or_else(|| escolhido_padrao(agentes));
    let linhas = agentes.iter().enumerate().map(|(i, a)| {
        let selecionado = escolhido.as_deref() == Some(a.id.as_str());
        let relogio_vivo = (a.estado == Estado::Rodando)
            .then(|| a.segundos.map(|s| div().flex_none().etiqueta().text_color(Cores::apagado()).child(relogio(s)).into_any_element()))
            .flatten();
        let (estado, id) = (estado.clone(), a.id.clone());
        div()
            .id(("agente", i))
            .rounded(raio::CARTAO)
            // O escolhido só com o fundo neutro, sem a borda azul (o "é aqui" do resto do app).
            .when(selecionado, |s| s.bg(Cores::pressionado()))
            .when(!selecionado, |s| s.hover(|s| s.bg(Cores::hover())))
            .cursor_pointer()
            .child(linha_do_no(ponto(a.estado), a.titulo.clone().into(), meta_do_agente(a).into(), false, relogio_vivo))
            .on_click(move |_, window, cx| {
                estado.update(cx, |aberto, cx| {
                    *aberto = Some(id.clone());
                    cx.notify();
                });
                window.refresh();
            })
    });
    let painel = agentes
        .iter()
        .enumerate()
        .find(|(_, a)| escolhido.as_deref() == Some(a.id.as_str()))
        .map(|(i, a)| painel_do_agente(i, a));
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

impl Janela {
    /// Abre o mapa dos agentes da conversa. O diálogo lê a conversa a cada quadro: os agentes que
    /// trabalham andam ao vivo, com o relógio.
    pub(super) fn abrir_mapa_de_agentes(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        // Qual agente está aberto (o id da chamada): um estado só do diálogo.
        let aberto: Entity<Option<String>> = cx.new(|_| None);
        self.foco_antes_do_dialogo(window, cx);
        window.open_dialog(cx, move |dialogo, window, cx| {
            let m = motor.read(cx);
            let Some(c) = m.chat(&chat) else {
                return moldura(dialogo, largura::G, window).title(topo(cabecalho("Agentes desta conversa", None)));
            };
            let rodando = matches!(m.execucao(&c.id), Execucao::Rodando { .. });
            let agentes = agentes_do_chat(&c.itens, rodando, agora());
            let vivos = agentes.iter().filter(|a| a.estado == Estado::Rodando).count();
            let subtitulo = match (agentes.len(), vivos) {
                (0, _) => "Os subagentes que a conversa abre".to_string(),
                (n, 0) => format!("{} · clique num para ver os passos", contagem(n, "agente", "agentes")),
                (n, v) => format!("{} · {v} trabalhando · clique num para ver os passos", contagem(n, "agente", "agentes")),
            };
            let qual = aberto.read(cx).clone();
            let corpo = mapa(m, c, &agentes, qual.as_deref(), &aberto);
            moldura(dialogo, LARGURA_DO_DIALOGO, window).title(topo(cabecalho("Agentes desta conversa", Some(subtitulo)))).child(corpo)
        });
    }
}

#[cfg(test)]
mod testes {
    use super::{Agente, Estado, agentes_do_chat, em_mil, meta_do_agente};
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
}
