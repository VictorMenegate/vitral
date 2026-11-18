//! "Nesta conversa", no Este chat: o sumário da conversa, com uma linha por mensagem sua (a hora e
//! o começo do texto) e, embaixo, o que o turno fez ("3 editados · 12 comandos", a conta de
//! `falas.rs`). O clique rola a conversa até a mensagem, trazendo antes as anteriores se ela está
//! fora das que o painel desenha, e a realça por um instante. A mensagem que está sendo lida
//! fica marcada no sumário enquanto a conversa rola.
//!
//! Tudo sai dos itens do chat, na hora. O que veio do histórico de uma conversa trazida do Claude
//! Code (que não está nos itens) fica de fora.

use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::Reflexo as _,
    mensagens::{AlvoDeAbertura, Ancora},
};
use crate::{
    modelo::{Chat, Item},
    tema::{Cores, FONTE_MONO, Tipo as _, dentro_da_borda, esp, r, raio},
};

/// Até quantas letras do começo da mensagem a linha guarda (a tela mostra duas linhas dela).
const MAX_DO_COMECO: usize = 160;

/// Uma mensagem conta como "à vista" quando começa até aqui abaixo do alto da conversa: a que o
/// sumário acabou de trazer para o alto (com o respiro dela) já é a marcada.
const FOLGA_DA_VISTA: Pixels = px(120.);

/// Quanto a mensagem fica abaixo do alto da conversa depois do clique: o lugar da faixa da
/// mensagem presa (a anterior gruda lá) e um respiro.
const RESPIRO_DO_ALTO: Pixels = px(84.);

/// Por quanto tempo a mensagem fica realçada depois do clique.
const REALCE: Duration = Duration::from_millis(1600);

/// Quantos itens antes da mensagem clicada o painel também desenha.
const ITENS_ANTES: usize = 6;

/// A altura da lista do sumário (a 100%): passando disso, ela rola por dentro.
const ALTURA_DA_LISTA: f32 = 232.;

/// Uma linha do sumário: a mensagem sua (o índice do item), quando foi, o começo do texto e o que
/// o turno dela fez.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LinhaDoSumario {
    pub item: usize,
    /// Unix, em segundos; 0 nas conversas de antes da hora nas mensagens.
    pub em: u64,
    pub texto: String,
    /// "3 editados · 12 comandos"; vazio quando o turno só respondeu.
    pub fez: String,
}

/// O começo de uma mensagem numa linha: as primeiras linhas com texto, juntas, em até 160 letras.
fn comeco(texto: &str) -> String {
    let junto = texto.lines().map(str::trim).filter(|l| !l.is_empty()).take(3).collect::<Vec<_>>().join(" ");
    if junto.chars().count() > MAX_DO_COMECO {
        format!("{}…", junto.chars().take(MAX_DO_COMECO - 1).collect::<String>().trim_end())
    } else {
        junto
    }
}

/// As linhas do sumário: uma por mensagem sua com texto (a que só tem anexos diz quantos são).
pub(super) fn linhas_do_sumario(itens: &[Item]) -> Vec<LinhaDoSumario> {
    let suas: Vec<usize> = itens.iter().enumerate().filter(|(_, i)| matches!(i, Item::Usuario { .. })).map(|(k, _)| k).collect();
    suas.iter()
        .enumerate()
        .filter_map(|(n, &item)| {
            let Item::Usuario { texto, anexos, em, .. } = &itens[item] else { return None };
            let texto = match (comeco(texto), anexos.len()) {
                (t, 0) if t.is_empty() => return None,
                (t, n) if t.is_empty() => super::componentes::contagem(n, "anexo", "anexos"),
                (t, _) => t,
            };
            // O turno vai até a sua mensagem seguinte: a conta é a das chamadas desse pedaço.
            let fim = suas.get(n + 1).copied().unwrap_or(itens.len());
            Some(LinhaDoSumario { item, em: *em, texto, fez: super::falas::o_que_o_turno_fez(&itens[..fim]) })
        })
        .collect()
}

/// A sua mensagem que está sendo lida: entre as desenhadas (`posicoes`: o índice do item, o alto
/// e o pé de cada bloco, a partir do alto da lista), a última que começa até a folga abaixo do
/// `topo` da vista; com o topo antes de todas, a primeira.
pub(super) fn mensagem_a_vista(posicoes: &[(usize, Pixels, Pixels)], sua: impl Fn(usize) -> bool, topo: Pixels) -> Option<usize> {
    let mut suas = posicoes.iter().filter(|(item, ..)| sua(*item));
    let primeira = suas.next()?;
    let lida = std::iter::once(primeira).chain(suas).take_while(|(_, alto, _)| *alto <= topo + FOLGA_DA_VISTA).last();
    Some(lida.unwrap_or(primeira).0)
}

/// Quantos itens o painel precisa desenhar (os últimos) para o item `item` estar entre eles, com
/// alguns antes dele (a mensagem não fica colada no alto da lista).
fn mostrar_ate(itens: usize, item: usize, mostrar: usize) -> usize {
    mostrar.max(itens.saturating_sub(item) + ITENS_ANTES)
}

/// O que o sumário guarda entre um desenho e outro.
#[derive(Default)]
pub(super) struct Sumario {
    /// A mensagem marcada no último desenho (o chat e o item): a rolagem só avisa a janela quando
    /// ela muda.
    a_vista: Option<(String, usize)>,
    /// A mensagem para a qual o clique levou, realçada até a hora guardada.
    realce: Option<(String, usize, Instant)>,
    rolagem: ScrollHandle,
    /// As linhas do último desenho, com o chat e o número de itens dele: a conversa que não
    /// cresceu não refaz a conta a cada quadro.
    linhas: Option<(String, usize, Rc<Vec<LinhaDoSumario>>)>,
}

impl Janela {
    /// O painel que mostra o chat: o ativo, se é ele; senão, o primeiro que o tem à vista.
    fn painel_do_sumario(&self, chat: &str) -> Option<usize> {
        let mostra = |ix: &usize| self.paineis[*ix].chat.as_deref() == Some(chat);
        Some(self.ativo).filter(|ix| *ix < self.paineis.len()).filter(mostra).or_else(|| (0..self.paineis.len()).find(mostra))
    }

    /// A mensagem do chat que está sendo lida no painel dele, pelas posições do último desenho.
    fn a_vista_no_painel(&self, chat: &Chat) -> Option<usize> {
        let p = &self.paineis[self.painel_do_sumario(&chat.id)?];
        let topo = -p.rolagem.offset().y;
        mensagem_a_vista(&p.posicoes.borrow(), |i| matches!(chat.itens.get(i), Some(Item::Usuario { .. })), topo)
    }

    /// A conversa de um painel rolou: se a mensagem lida passou a ser outra, a janela redesenha
    /// (o sumário muda a marca). Só com o Este chat do mesmo chat.
    pub(super) fn acompanhar_sumario(&mut self, painel_id: u64, cx: &mut Context<Self>) {
        let Some(chat_id) = self.paineis.iter().find(|p| p.id == painel_id).and_then(|p| p.chat.clone()) else { return };
        if self.selecionado.as_deref() != Some(chat_id.as_str()) {
            return;
        }
        let Some(motor) = self.motor.clone() else { return };
        let a_vista = motor.read(cx).chat(&chat_id).and_then(|chat| self.a_vista_no_painel(chat)).map(|item| (chat_id, item));
        if self.sumario.a_vista != a_vista {
            cx.notify();
        }
    }

    /// O clique numa linha do sumário: o painel passa a desenhar até a mensagem, rola até ela e a
    /// realça por um instante.
    fn ir_a_mensagem_do_sumario(&mut self, chat: &str, item: usize, cx: &mut Context<Self>) {
        let Some(ix) = self.painel_do_sumario(chat) else { return };
        let itens = self.motor.as_ref().and_then(|m| m.read(cx).chat(chat).map(|c| c.itens.len())).unwrap_or(0);
        let p = &mut self.paineis[ix];
        p.mostrar = mostrar_ate(itens, item, p.mostrar);
        // A abertura na âncora faz o resto no próximo desenho, já com os blocos medidos: põe o
        // item no alto, abaixo do lugar da mensagem presa.
        // A primeira da conversa não tem o que mostrar acima dela.
        let desloc = if item == 0 { px(0.) } else { -RESPIRO_DO_ALTO };
        p.alvo = Some(AlvoDeAbertura::Ancora(Ancora { item, desloc }));
        p.seguindo = false;
        let painel_id = p.id;
        self.sumario.realce = Some((chat.to_string(), item, Instant::now() + REALCE));
        cx.spawn(async move |janela, cx| {
            cx.background_executor().timer(REALCE).await;
            let _ = janela.update(cx, |this, cx| {
                if this.sumario.realce.as_ref().is_some_and(|(_, _, ate)| *ate <= Instant::now()) {
                    this.sumario.realce = None;
                    this.avisar_vista(painel_id, cx);
                }
            });
        })
        .detach();
        self.avisar_vista(painel_id, cx);
        cx.notify();
    }

    /// O bloco da mensagem para a qual o sumário acabou de levar ganha o realce por cima, que
    /// some sozinho (chamado ao montar cada bloco da conversa).
    pub(super) fn com_realce_do_sumario(&self, caixa: Div, chat: &str, indice: Option<usize>) -> Div {
        let realcado = self
            .sumario
            .realce
            .as_ref()
            .is_some_and(|(c, item, ate)| c == chat && Some(*item) == indice && *ate > Instant::now());
        if !realcado {
            return caixa;
        }
        caixa.relative().child(
            div()
                .absolute()
                .top(-esp::E4)
                .bottom(-esp::E4)
                .left(-esp::E8)
                .right(-esp::E8)
                .rounded(raio::CARTAO)
                .border_1()
                .border_color(Cores::destaque_borda())
                .bg(Cores::destaque().opacity(0.10)),
        )
    }

    /// A seção "Nesta conversa" do Este chat. Nada com menos de duas mensagens suas: não há para
    /// onde ir.
    pub(super) fn secao_do_sumario(&mut self, chat: &Chat, cx: &Context<Self>) -> Option<AnyElement> {
        let linhas = match &self.sumario.linhas {
            Some((de, itens, linhas)) if *de == chat.id && *itens == chat.itens.len() => linhas.clone(),
            _ => {
                let linhas = Rc::new(linhas_do_sumario(&chat.itens));
                self.sumario.linhas = Some((chat.id.clone(), chat.itens.len(), linhas.clone()));
                linhas
            }
        };
        if linhas.len() < 2 {
            return None;
        }
        let a_vista = self.a_vista_no_painel(chat);
        // O dia entra como um título pequeno onde ele muda, quando a conversa não é só de hoje.
        let horas: Vec<u64> = linhas.iter().map(|l| l.em).collect();
        let dias = super::mensagens::separadores_de_data(&horas, crate::modelo::agora());
        let com_dias = dias.len() > 1 || dias.first().is_some_and(|(_, dia)| dia != "Hoje");
        let mut filhos: Vec<AnyElement> = Vec::with_capacity(linhas.len() + dias.len());
        // Onde a linha marcada ficou entre os filhos (os títulos de dia também contam).
        let mut filho_da_marcada = None;
        for (n, linha) in linhas.iter().enumerate() {
            if com_dias && let Some((_, dia)) = dias.iter().find(|(posicao, _)| *posicao == n) {
                filhos.push(div().flex_none().px(esp::E6).pt(esp::E6).metadado().text_color(Cores::apagado()).child(dia.clone()).into_any_element());
            }
            let lida = a_vista == Some(linha.item);
            if lida {
                filho_da_marcada = Some(filhos.len());
            }
            let quando = if linha.em == 0 { String::new() } else { crate::modelo::hora_local(linha.em) };
            let (chat_id, item) = (chat.id.clone(), linha.item);
            let filho = div()
                .id(("linha-do-sumario", n))
                .relative()
                .flex()
                .flex_none()
                .items_start()
                .gap(esp::E8)
                .px(esp::E6)
                .py(esp::E4)
                .rounded(raio::CONTROLE)
                .cursor_pointer()
                .when(lida, |s| s.bg(Cores::selecao()))
                .when(!lida, |s| s.hover(|s| s.bg(Cores::hover())))
                .child(
                    div()
                        .flex_none()
                        .w(r(44.))
                        .font_family(FONTE_MONO)
                        .metadado()
                        .text_color(if lida { Cores::texto() } else { Cores::tenue() })
                        .child(quando),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().corpo_ui().line_clamp(2).text_color(if lida { Cores::forte() } else { Cores::texto() }).child(linha.texto.clone()))
                        .when(!linha.fez.is_empty(), |s| s.child(div().metadado().text_color(Cores::tenue()).child(linha.fez.clone()))),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.ir_a_mensagem_do_sumario(&chat_id, item, cx)));
            filhos.push(filho.into_any_element());
        }
        let marcada = a_vista.map(|item| (chat.id.clone(), item));
        if self.sumario.a_vista != marcada {
            // A marca mudou: a lista do sumário rola até a linha dela.
            if let Some(filho) = filho_da_marcada {
                self.sumario.rolagem.scroll_to_item(filho);
            }
            self.sumario.a_vista = marcada;
        }
        let titulo = div()
            .flex()
            .items_center()
            .gap(esp::E6)
            .child(div().rotulo().text_color(Cores::forte()).child("Nesta conversa"))
            .child(div().font_family(FONTE_MONO).metadado().text_color(Cores::apagado()).child(linhas.len().to_string()));
        Some(
            div()
                .flex()
                .flex_col()
                .gap(esp::E8)
                .p(dentro_da_borda(12.))
                .rounded(raio::CARTAO)
                .bg(Cores::cartao_elevado())
                .border_1()
                .border_color(Cores::linha())
                .reflexo(raio::cartao())
                .child(titulo)
                .child(
                    div().relative().child(
                        div()
                            .id("lista-do-sumario")
                            .max_h(r(ALTURA_DA_LISTA))
                            .overflow_y_scroll()
                            .track_scroll(&self.sumario.rolagem)
                            .flex()
                            .flex_col()
                            .gap(esp::E2)
                            .children(filhos)
                            .vertical_scrollbar(&self.sumario.rolagem),
                    ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: ele traria o `test` do gpui, e o #[test] viraria recursão.
    use gpui_kit::px;

    use super::{LinhaDoSumario, comeco, linhas_do_sumario, mensagem_a_vista, mostrar_ate};
    use crate::modelo::Item;

    fn usuario(texto: &str, em: u64) -> Item {
        Item::Usuario { texto: texto.into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em, marca: None }
    }

    fn ferramenta(nome: &str, arquivo: Option<&str>) -> Item {
        Item::Ferramenta {
            id: "f".into(),
            nome: nome.into(),
            resumo: String::new(),
            resultado: Some("ok".into()),
            erro: false,
            em: 0,
            detalhe: None,
            arquivo: arquivo.map(str::to_string),
            sub: None,
            mudanca: None,
        }
    }

    #[test]
    fn uma_linha_por_mensagem_sua_com_o_que_o_turno_fez() {
        let itens = vec![
            usuario("arruma o frete\n\ne roda os testes", 100),
            ferramenta("Edit", Some("src/frete.rs")),
            ferramenta("Edit", Some("src/cep.rs")),
            ferramenta("Bash", None),
            Item::Assistente { texto: "Pronto.".into(), em: 110 },
            usuario("   ", 120),
            usuario("e o deploy?", 130),
            Item::Assistente { texto: "Ainda não.".into(), em: 131 },
            Item::Usuario { texto: String::new(), anexos: vec!["/tmp/a.png".into(), "/tmp/b.png".into()], ultrathink: false, ultracode: false, em: 140, marca: None },
            ferramenta("Bash", None),
        ];
        let linhas = linhas_do_sumario(&itens);
        let linha = |item: usize, em: u64, texto: &str, fez: &str| LinhaDoSumario { item, em, texto: texto.into(), fez: fez.into() };
        assert_eq!(
            linhas,
            [
                // O que o turno fez é só o do turno dela, não o do seguinte.
                linha(0, 100, "arruma o frete e roda os testes", "2 editados · 1 comando"),
                // A mensagem vazia não entra; a que só respondeu não diz nada embaixo.
                linha(6, 130, "e o deploy?", ""),
                linha(8, 140, "2 anexos", "1 comando"),
            ]
        );
        assert!(linhas_do_sumario(&[]).is_empty());
        let longa = "palavra ".repeat(60);
        assert!(comeco(&longa).chars().count() <= 160);
        assert!(comeco(&longa).ends_with('…'));
    }

    #[test]
    fn a_mensagem_a_vista_acompanha_a_rolagem() {
        // Blocos: a mensagem 0, a resposta dela (1), a mensagem 5, a resposta (6), a mensagem 9.
        let posicoes = [(0, px(12.), px(60.)), (1, px(92.), px(900.)), (5, px(932.), px(980.)), (6, px(1012.), px(2000.)), (9, px(2032.), px(2080.))];
        let sua = |i: usize| matches!(i, 0 | 5 | 9);
        let a_vista = |topo: f32| mensagem_a_vista(&posicoes, sua, px(topo));
        assert_eq!(a_vista(0.), Some(0));
        // Lendo a resposta da primeira, ainda é ela; a segunda passa a valer quando chega perto do alto.
        assert_eq!(a_vista(500.), Some(0));
        assert_eq!(a_vista(811.), Some(0));
        assert_eq!(a_vista(812.), Some(5));
        // Logo depois do clique (a mensagem a 84 do alto), é a clicada.
        assert_eq!(a_vista(932. - 84.), Some(5));
        assert_eq!(a_vista(1500.), Some(5));
        assert_eq!(a_vista(3000.), Some(9));
        // As primeiras não estão desenhadas: a primeira desenhada é a que vale no alto.
        assert_eq!(mensagem_a_vista(&posicoes[2..], sua, px(0.)), Some(5));
        assert_eq!(mensagem_a_vista(&posicoes[1..2], sua, px(0.)), None);
        assert_eq!(mensagem_a_vista(&[], sua, px(0.)), None);
    }

    #[test]
    fn o_painel_desenha_ate_a_mensagem_clicada() {
        // 200 itens, 60 desenhados: a primeira mensagem (item 0) pede todos.
        assert!(mostrar_ate(200, 0, 60) >= 200);
        assert_eq!(mostrar_ate(200, 150, 60), 60);
        // Com alguns antes dela, para ela não ficar colada no alto.
        assert_eq!(mostrar_ate(200, 100, 60), 106);
        assert_eq!(mostrar_ate(10, 3, 60), 60);
    }
}
