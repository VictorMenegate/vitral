//! O cartão da pergunta com opções (a ferramenta AskUserQuestion do Claude Code): as perguntas
//! lidas do pedido, o que foi escolhido em cada uma e as respostas no formato que o motor devolve
//! (`Motor::responder_pergunta`).

use std::collections::BTreeSet;
use std::rc::Rc;

use gpui_kit::component::{
    Disableable as _,
    button::Button,
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement as _,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{EstadoDaMoldura, LarguraPainel, TamanhoBotao, azulejo, botao_claro, botao_fantasma, miolo_da_moldura, moldura_dupla},
    mensagens::{icone_de, minima},
};
use crate::tema::{Cores, Lucide, Tipo as _, esp, halo_foco, r, raio};

/// Uma opção de uma pergunta: o rótulo (é ele que volta como resposta) e a descrição.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Opcao {
    pub rotulo: String,
    pub descricao: String,
}

/// Uma pergunta do pedido: o texto inteiro (a chave da resposta), o cabeçalho curto ("Cor"), as
/// opções e se aceita várias (`multiSelect`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pergunta {
    pub texto: String,
    pub cabecalho: String,
    pub opcoes: Vec<Opcao>,
    pub varias: bool,
}

/// As perguntas da entrada de uma AskUserQuestion (`questions`), na ordem. A que não tem texto
/// não entra (não haveria chave para a resposta dela).
pub(super) fn perguntas_de(entrada: &serde_json::Value) -> Vec<Pergunta> {
    let texto = |v: &serde_json::Value| v.as_str().map(str::trim).unwrap_or_default().to_string();
    entrada["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            // A chave da resposta é o texto como veio, sem aparar.
            let pergunta = p["question"].as_str().filter(|t| !t.trim().is_empty())?.to_string();
            let opcoes = p["options"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|o| Opcao { rotulo: texto(&o["label"]), descricao: texto(&o["description"]) })
                .filter(|o| !o.rotulo.is_empty())
                .collect();
            Some(Pergunta { texto: pergunta, cabecalho: texto(&p["header"]), opcoes, varias: p["multiSelect"] == true })
        })
        .collect()
}

/// O que está marcado numa pergunta: as opções (pelo índice) e a "Outra…".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Escolha {
    pub opcoes: BTreeSet<usize>,
    pub outra: bool,
}

impl Escolha {
    pub fn escolhida(&self, opcao: usize) -> bool {
        self.opcoes.contains(&opcao)
    }

    /// O clique numa opção: com `varias`, liga ou desliga; sem, ela passa a ser a única (e a
    /// "Outra…" sai).
    pub fn alternar(&mut self, opcao: usize, varias: bool) {
        if varias {
            if !self.opcoes.remove(&opcao) {
                self.opcoes.insert(opcao);
            }
        } else {
            self.opcoes = BTreeSet::from([opcao]);
            self.outra = false;
        }
    }

    /// O clique na "Outra…": com `varias`, soma às opções; sem, fica só ela.
    pub fn alternar_outra(&mut self, varias: bool) {
        if varias {
            self.outra = !self.outra;
        } else {
            self.outra = true;
            self.opcoes.clear();
        }
    }
}

/// A resposta de uma pergunta, como o Claude Code a espera: o rótulo da opção; com várias, os
/// rótulos na ordem das opções separados por ", " e o texto da "Outra…" no fim; só a "Outra…", o
/// texto. `None` enquanto falta responder (nada marcado, ou a "Outra…" sozinha e vazia). A
/// pergunta sem opções só tem o campo.
pub(super) fn resposta_de(pergunta: &Pergunta, escolha: &Escolha, outra: &str) -> Option<String> {
    let outra = Some(outra.trim()).filter(|t| !t.is_empty() && (escolha.outra || pergunta.opcoes.is_empty()));
    let mut partes: Vec<&str> =
        escolha.opcoes.iter().filter_map(|&i| pergunta.opcoes.get(i)).map(|o| o.rotulo.as_str()).collect();
    if !pergunta.varias {
        partes.truncate(1);
        if outra.is_some() {
            partes.clear();
        }
    }
    partes.extend(outra);
    (!partes.is_empty()).then(|| partes.join(", "))
}

/// As respostas do cartão, na ordem das perguntas, cada uma com o texto inteiro da pergunta:
/// `None` enquanto alguma falta (o Enviar fica desligado).
pub(super) fn respostas_de(perguntas: &[Pergunta], escolhas: &[Escolha], outras: &[String]) -> Option<Vec<(String, String)>> {
    if perguntas.is_empty() {
        return None;
    }
    perguntas
        .iter()
        .enumerate()
        .map(|(i, pergunta)| {
            let outra = outras.get(i).map(String::as_str).unwrap_or_default();
            Some((pergunta.texto.clone(), resposta_de(pergunta, escolhas.get(i)?, outra)?))
        })
        .collect()
}

/// Quantas perguntas já têm resposta: com várias, o rodapé do cartão diz "1 de 3 respondidas" (as
/// de baixo podem estar fora da vista, e o Enviar só liga com todas).
pub(super) fn respondidas(perguntas: &[Pergunta], escolhas: &[Escolha], outras: &[String]) -> usize {
    perguntas
        .iter()
        .enumerate()
        .filter(|(i, pergunta)| {
            let outra = outras.get(*i).map(String::as_str).unwrap_or_default();
            escolhas.get(*i).is_some_and(|escolha| resposta_de(pergunta, escolha, outra).is_some())
        })
        .count()
}

/// O que a janela guarda de uma pergunta aberta: o que está marcado e o campo da "Outra…" de
/// cada pergunta. Some junto com o pedido (`Janela::acertar_perguntas`).
pub(super) struct EstadoDaPergunta {
    pub escolhas: Vec<Escolha>,
    pub campos: Vec<Entity<InputState>>,
    _assinaturas: Vec<Subscription>,
}

/// Até aqui o cartão encolhe quando falta altura (240 px): o cabeçalho, a pergunta com o começo
/// das opções (o resto rola) e os botões. Sem isto, num painel baixo, as perguntas sumiam e
/// sobravam só os botões.
const MINIMO_DA_PERGUNTA: Rems = r(240.);

/// O marcador de uma opção: redondo na escolha única, quadrado quando dá para marcar várias.
fn marcador(marcado: bool, varias: bool) -> Div {
    let raio_do_marcador = if varias { px(5.) } else { raio::PILULA };
    div()
        .flex_none()
        .size(r(16.))
        .mt(r(2.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(raio_do_marcador)
        .border_1()
        .map(|s| if marcado { s.bg(Cores::claro()).border_color(Cores::claro()) } else { s.border_color(Cores::borda_forte()) })
        .when(marcado && varias, |s| s.child(icone_de(Lucide::Check, 12.).text_color(Cores::sobre_cor())))
        .when(marcado && !varias, |s| s.child(div().size(r(6.)).rounded(raio::PILULA).bg(Cores::sobre_cor())))
}

impl Janela {
    /// Um estado por pergunta esperando resposta (no `render`, que tem a janela para criar os
    /// campos); o do pedido que saiu vai embora.
    pub(super) fn acertar_perguntas(&mut self, pedidos: Vec<(String, usize)>, window: &mut Window, cx: &mut Context<Self>) {
        self.perguntas.retain(|id, _| pedidos.iter().any(|(pedido, _)| pedido == id));
        for (id, quantas) in pedidos {
            if self.perguntas.contains_key(&id) {
                continue;
            }
            let campos: Vec<Entity<InputState>> =
                (0..quantas).map(|_| cx.new(|cx| InputState::new(window, cx).placeholder(tr!("Escreva a sua resposta")))).collect();
            let assinaturas = campos
                .iter()
                .map(|campo| {
                    let id = id.clone();
                    cx.subscribe_in(campo, window, move |this, _, evento: &InputEvent, _, cx| match evento {
                        InputEvent::PressEnter { .. } => this.enviar_pergunta(&id, cx),
                        // O Enviar liga e desliga conforme o que está escrito.
                        InputEvent::Change => cx.notify(),
                        _ => {}
                    })
                })
                .collect();
            self.perguntas.insert(id, EstadoDaPergunta { escolhas: vec![Escolha::default(); quantas], campos, _assinaturas: assinaturas });
        }
    }

    /// O que está escrito no campo da "Outra…" de cada pergunta do cartão.
    fn outras_da_pergunta(&self, id: &str, cx: &App) -> Vec<String> {
        self.perguntas.get(id).map(|e| e.campos.iter().map(|c| c.read(cx).value().to_string()).collect()).unwrap_or_default()
    }

    /// As respostas do cartão como estão agora; `None` enquanto falta alguma.
    fn respostas_da_pergunta(&self, id: &str, perguntas: &[Pergunta], cx: &App) -> Option<Vec<(String, String)>> {
        respostas_de(perguntas, &self.perguntas.get(id)?.escolhas, &self.outras_da_pergunta(id, cx))
    }

    /// O Enviar do cartão (e o Enter no campo da "Outra…"): só com todas respondidas.
    fn enviar_pergunta(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let perguntas = motor.read(cx).permissoes.iter().find(|p| p.id == id).map(|p| perguntas_de(&p.entrada)).unwrap_or_default();
        let Some(respostas) = self.respostas_da_pergunta(id, &perguntas, cx) else {
            return;
        };
        motor.update(cx, |m, cx| m.responder_pergunta(id, &respostas, cx));
    }

    /// A pergunta com opções que o agente faz (AskUserQuestion), no desenho dos pedidos (a
    /// moldura dupla): cada pergunta com as opções clicáveis (uma, ou várias com `multiSelect`),
    /// a descrição de cada uma embaixo do rótulo, a "Outra…" com o campo de texto livre e, no
    /// rodapé, "Não responder" e "Enviar" (que só liga com todas respondidas). Escrever na caixa
    /// de mensagem e dar Enter também responde (`Motor::responder_pedido_escrevendo`).
    pub(super) fn cartao_pergunta(
        &self,
        id: &str,
        perguntas: &[Pergunta],
        corpo_aberto: bool,
        posicao: Option<(usize, usize)>,
        nivel: LarguraPainel,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id_de = |nome: &str| SharedString::from(format!("{nome}-{id}"));
        let titulo = match perguntas {
            [_] => tr!("Pergunta").to_string(),
            varias => tr!("{n} perguntas", n = varias.len()).to_string(),
        };
        let cabecalho = h_flex()
            .id(id_de("cab-pergunta"))
            .w_full()
            .gap(esp::E8)
            .child(azulejo(Lucide::MessageCircleQuestionMark, Cores::texto()))
            .child(div().flex_1().min_w_0().truncate().titulo_cartao().text_color(Cores::forte()).child(titulo))
            .when_some(posicao, |s, (k, n)| {
                s.child(div().flex_none().etiqueta().text_color(Cores::tenue()).child(tr!("{k} de {n}", k = k, n = n)))
            });
        let nao_responder: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)> = {
            let id = id.to_string();
            Rc::new(cx.listener(move |this: &mut Janela, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                // Sem resposta nenhuma: o modelo lê que ninguém respondeu (negar marcaria a
                // chamada como falha).
                if let Some(m) = &this.motor {
                    m.update(cx, |m, cx| m.responder_pergunta(&id, &[], cx));
                }
            }))
        };
        let nao_responder = |t: TamanhoBotao| {
            let ao_clicar = nao_responder.clone();
            botao_fantasma(id_de("nao-responder"), None, tr!("Não responder"), t, window, cx).on_click(move |e, window, cx| ao_clicar(e, window, cx))
        };
        if !corpo_aberto {
            // Recolhida (há outro pedido aberto): o cabeçalho com a primeira pergunta; o clique abre.
            let abrir = {
                let id = id.to_string();
                cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.pedido_aberto = Some(id.clone());
                    cx.notify();
                })
            };
            return moldura_dupla(EstadoDaMoldura::Recolhida)
                .child(
                    miolo_da_moldura()
                        .py(esp::E8)
                        .px(esp::E12)
                        .child(cabecalho.cursor_pointer().on_click(abrir).child(nao_responder(TamanhoBotao::Barra))),
                )
                .into_any_element();
        }
        let estado = self.perguntas.get(id);
        let pronta = self.respostas_da_pergunta(id, perguntas, cx).is_some();
        // Com várias perguntas, quantas já têm resposta (as de baixo podem estar fora da vista).
        let andamento = (perguntas.len() > 1).then(|| {
            let feitas = respondidas(perguntas, estado.map(|e| e.escolhas.as_slice()).unwrap_or_default(), &self.outras_da_pergunta(id, cx));
            tr!("{feitas} de {total} respondidas", feitas = feitas, total = perguntas.len()).to_string()
        });
        let blocos = perguntas.iter().enumerate().map(|(n, pergunta)| {
            let escolha = estado.and_then(|e| e.escolhas.get(n)).cloned().unwrap_or_default();
            let campo = estado.and_then(|e| e.campos.get(n)).cloned();
            let varias = pergunta.varias;
            // Uma linha de opção: o marcador, o rótulo e, embaixo, a descrição (as duas quebram).
            let linha = |chave: SharedString, marcado: bool, rotulo: String, descricao: String| {
                h_flex()
                    .id(chave)
                    .w_full()
                    .items_start()
                    .gap(esp::E8)
                    .px(esp::E8)
                    .py(esp::E6)
                    .rounded(raio::CONTROLE)
                    .cursor_pointer()
                    // Só o marcador diz o que está escolhido: o fundo é do ponteiro.
                    .hover(|s| s.bg(Cores::hover()))
                    .tab_index(0)
                    .focus_visible(|s| s.shadow(halo_foco()))
                    .child(marcador(marcado, varias))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().rotulo().text_color(Cores::forte()).child(rotulo))
                            .when(!descricao.is_empty(), |s| s.child(div().metadado().text_color(Cores::tenue()).child(descricao))),
                    )
            };
            let opcoes = pergunta.opcoes.iter().enumerate().map(|(k, opcao)| {
                let id = id.to_string();
                linha(
                    SharedString::from(format!("opcao-{id}-{n}-{k}")),
                    escolha.escolhida(k),
                    opcao.rotulo.clone(),
                    opcao.descricao.clone(),
                )
                .on_click(cx.listener(move |this: &mut Janela, _: &ClickEvent, _, cx| {
                    if let Some(escolha) = this.perguntas.get_mut(&id).and_then(|e| e.escolhas.get_mut(n)) {
                        escolha.alternar(k, varias);
                    }
                    cx.notify();
                }))
            });
            // A "Outra…": marcada, o campo aparece embaixo e ganha o foco. A pergunta sem opções
            // só tem o campo.
            let sem_opcoes = pergunta.opcoes.is_empty();
            let outra = (!sem_opcoes).then(|| {
                let id = id.to_string();
                linha(SharedString::from(format!("outra-{id}-{n}")), escolha.outra, tr!("Outra…").to_string(), String::new()).on_click(
                    cx.listener(move |this: &mut Janela, _: &ClickEvent, window, cx| {
                        if let Some(estado) = this.perguntas.get_mut(&id)
                            && let Some(escolha) = estado.escolhas.get_mut(n)
                        {
                            escolha.alternar_outra(varias);
                            if escolha.outra
                                && let Some(campo) = estado.campos.get(n)
                            {
                                let foco = campo.read(cx).focus_handle(cx);
                                window.focus(&foco, cx);
                            }
                        }
                        cx.notify();
                    }),
                )
            });
            let campo = campo.filter(|_| escolha.outra || sem_opcoes).map(|campo| {
                // Alinhado com o texto das opções (o marcador de 16 e o vão de 8).
                div().when(!sem_opcoes, |s| s.pl(r(32.))).pr(esp::E8).pt(esp::E2).child(Input::new(&campo))
            });
            v_flex()
                .gap(esp::E6)
                .child(
                    v_flex()
                        .gap(esp::E2)
                        .when(!pergunta.cabecalho.is_empty(), |s| {
                            s.child(div().etiqueta().text_color(Cores::apagado()).child(pergunta.cabecalho.to_uppercase()))
                        })
                        .child(div().corpo_ui().text_color(Cores::forte()).child(pergunta.texto.trim().to_string()))
                        .when(varias, |s| s.child(div().metadado().text_color(Cores::tenue()).child(tr!("Dá para marcar mais de uma.")))),
                )
                .child(v_flex().gap(esp::E2).children(opcoes).children(outra).children(campo))
        });
        let enviar = {
            let id = id.to_string();
            cx.listener(move |this: &mut Janela, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                this.enviar_pergunta(&id, cx);
            })
        };
        let minima = minima(nivel);
        let enviar: Button = botao_claro(id_de("enviar-pergunta"), None, tr!("Enviar"), TamanhoBotao::Controle, window, cx)
            .disabled(!pronta)
            .when(!pronta, |b| b.tooltip(if perguntas.len() == 1 { tr!("Escolha uma opção ou escreva a sua resposta") } else { tr!("Responda todas as perguntas") }))
            .when(minima, |b| b.w_full())
            .on_click(enviar);
        let andamento = andamento.map(|texto| div().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(texto));
        let rodape = if minima {
            // Na largura mínima, o andamento em cima e os dois botões lado a lado, o Enviar com
            // o que sobra.
            v_flex()
                .gap(esp::E8)
                .children(andamento)
                .child(h_flex().gap(esp::E8).child(nao_responder(TamanhoBotao::Controle)).child(div().flex_1().min_w_0().child(enviar)))
                .into_any_element()
        } else {
            h_flex()
                .gap(esp::E8)
                .child(div().flex_1().min_w_0().children(andamento))
                .child(nao_responder(TamanhoBotao::Controle))
                .child(enviar)
                .into_any_element()
        };
        moldura_dupla(EstadoDaMoldura::Esperando)
            .min_h_0()
            .child(
                miolo_da_moldura()
                    .flex_1()
                    .min_h(MINIMO_DA_PERGUNTA)
                    .max_h(r(520.))
                    .p(esp::E12)
                    .gap(esp::E12)
                    .child(div().flex_none().child(cabecalho))
                    // Quando falta altura (várias perguntas, janela baixa), são as perguntas que
                    // rolam; o cabeçalho e os botões ficam.
                    .child(
                        div()
                            .id(id_de("rolagem-pergunta"))
                            .flex_1()
                            .min_h_0()
                            .child(v_flex().gap(esp::E16).children(blocos))
                            .overflow_y_scrollbar(),
                    )
                    .child(div().flex_none().child(rodape)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod testes {
    use serde_json::json;

    use super::{Escolha, Pergunta, perguntas_de, resposta_de, respostas_de};

    fn entrada() -> serde_json::Value {
        json!({ "questions": [
            { "question": "Qual cor você prefere?", "header": "Cor", "multiSelect": false,
              "options": [{ "label": "Azul", "description": "A do céu." }, { "label": "Verde", "description": "" }, { "label": "Vermelho" }] },
            { "question": "Quais frutas você gosta?", "header": "Frutas", "multiSelect": true,
              "options": [{ "label": "Maçã" }, { "label": "Banana" }, { "label": "Uva" }] },
        ] })
    }

    #[test]
    fn as_perguntas_saem_do_pedido() {
        let perguntas = perguntas_de(&entrada());
        assert_eq!(perguntas.len(), 2);
        let cor = &perguntas[0];
        assert_eq!((cor.texto.as_str(), cor.cabecalho.as_str(), cor.varias), ("Qual cor você prefere?", "Cor", false));
        let opcoes: Vec<(&str, &str)> = cor.opcoes.iter().map(|o| (o.rotulo.as_str(), o.descricao.as_str())).collect();
        assert_eq!(opcoes, [("Azul", "A do céu."), ("Verde", ""), ("Vermelho", "")]);
        assert!(perguntas[1].varias);
        // O que não é pergunta não quebra o cartão: sem texto, a pergunta não entra.
        assert!(perguntas_de(&json!({})).is_empty());
        assert!(perguntas_de(&json!({ "questions": "x" })).is_empty());
        let torta = json!({ "questions": [{ "header": "Sem texto" }, { "question": "Só o texto?" }] });
        let lidas = perguntas_de(&torta);
        assert_eq!(lidas.len(), 1);
        assert!(lidas[0].opcoes.is_empty() && lidas[0].cabecalho.is_empty() && !lidas[0].varias);
    }

    #[test]
    fn uma_escolha_ou_varias() {
        // Escolha única: a opção nova troca a anterior, e a "Outra…" tira as opções.
        let mut cor = Escolha::default();
        cor.alternar(0, false);
        cor.alternar(1, false);
        assert_eq!(cor.opcoes.iter().copied().collect::<Vec<_>>(), [1]);
        // Clicar de novo na escolhida não a desfaz (é um botão de rádio).
        cor.alternar(1, false);
        assert!(cor.escolhida(1) && !cor.outra);
        cor.alternar_outra(false);
        assert!(cor.outra && cor.opcoes.is_empty());
        cor.alternar(2, false);
        assert!(!cor.outra && cor.escolhida(2));
        // Várias: cada clique liga ou desliga, e a "Outra…" soma.
        let mut frutas = Escolha::default();
        frutas.alternar(2, true);
        frutas.alternar(0, true);
        frutas.alternar_outra(true);
        assert!(frutas.escolhida(0) && frutas.escolhida(2) && frutas.outra);
        frutas.alternar(2, true);
        frutas.alternar_outra(true);
        assert!(frutas.escolhida(0) && !frutas.escolhida(2) && !frutas.outra);
    }

    fn escolha(opcoes: &[usize], outra: bool) -> Escolha {
        Escolha { opcoes: opcoes.iter().copied().collect(), outra }
    }

    #[test]
    fn a_resposta_de_cada_pergunta() {
        let perguntas = perguntas_de(&entrada());
        let (cor, frutas) = (&perguntas[0], &perguntas[1]);
        assert_eq!(resposta_de(cor, &escolha(&[1], false), ""), Some("Verde".into()));
        assert_eq!(resposta_de(cor, &Escolha::default(), ""), None);
        // A "Outra…" responde com o texto; marcada e vazia, ainda falta responder.
        assert_eq!(resposta_de(cor, &escolha(&[], true), "  roxo com bolinhas "), Some("roxo com bolinhas".into()));
        assert_eq!(resposta_de(cor, &escolha(&[], true), "  "), None);
        // O texto que ficou no campo com a "Outra…" desmarcada não vai.
        assert_eq!(resposta_de(cor, &escolha(&[0], false), "roxo"), Some("Azul".into()));
        // Várias: na ordem das opções (não na do clique), separadas por vírgula, como a CLI
        // devolve; a "Outra…" vai no fim.
        assert_eq!(resposta_de(frutas, &escolha(&[2, 0], false), ""), Some("Maçã, Uva".into()));
        assert_eq!(resposta_de(frutas, &escolha(&[1], true), "caqui"), Some("Banana, caqui".into()));
        assert_eq!(resposta_de(frutas, &escolha(&[], true), "caqui"), Some("caqui".into()));
        assert_eq!(resposta_de(frutas, &escolha(&[1], true), " "), Some("Banana".into()));
        assert_eq!(resposta_de(frutas, &escolha(&[], true), ""), None);
        // Uma opção que não existe (o pedido mudou) não conta.
        assert_eq!(resposta_de(cor, &escolha(&[9], false), ""), None);
        // A pergunta sem opções só tem o campo.
        let aberta = Pergunta { texto: "Como chamar?".into(), cabecalho: String::new(), opcoes: Vec::new(), varias: false };
        assert_eq!(resposta_de(&aberta, &Escolha::default(), "Vitral"), Some("Vitral".into()));
        assert_eq!(resposta_de(&aberta, &Escolha::default(), ""), None);
    }

    /// O Enviar só vale com todas respondidas; as respostas saem com a pergunta inteira como
    /// chave, na ordem das perguntas.
    #[test]
    fn as_respostas_do_cartao() {
        let perguntas = perguntas_de(&entrada());
        let vazio = String::new();
        let faltando = respostas_de(&perguntas, &[escolha(&[1], false), Escolha::default()], &[vazio.clone(), vazio.clone()]);
        assert_eq!(faltando, None);
        let todas = respostas_de(&perguntas, &[escolha(&[1], false), escolha(&[0, 2], true)], &[vazio, "caqui".into()]);
        assert_eq!(
            todas,
            Some(vec![
                ("Qual cor você prefere?".to_string(), "Verde".to_string()),
                ("Quais frutas você gosta?".to_string(), "Maçã, Uva, caqui".to_string()),
            ])
        );
        // O andamento do rodapé conta as que já têm resposta.
        let vazio = String::new();
        assert_eq!(super::respondidas(&perguntas, &[escolha(&[1], false), Escolha::default()], &[vazio.clone(), vazio.clone()]), 1);
        assert_eq!(super::respondidas(&perguntas, &[escolha(&[1], false), escolha(&[], true)], &[vazio.clone(), "caqui".into()]), 2);
        assert_eq!(super::respondidas(&perguntas, &[], &[]), 0);
        // Sem pergunta nenhuma não há o que enviar.
        assert_eq!(respostas_de(&[], &[], &[]), None);
        // Estado mais curto que as perguntas (o pedido acabou de chegar): falta responder.
        assert_eq!(respostas_de(&perguntas, &[], &[]), None);
    }
}
