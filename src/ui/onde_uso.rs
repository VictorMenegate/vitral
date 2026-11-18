//! "Onde o uso foi", no diálogo das contas: o gasto dos turnos terminados por pasta e, dentro de
//! cada uma, os chats que mais gastaram, de hoje ou dos últimos 7 dias. Os números vêm do livro
//! do motor (`motor/gasto.rs`); aqui ficam o desenho e os textos ("182 mil tokens · US$ 0,84").

use std::{collections::HashSet, path::Path};

use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{ControleSegmentado, contagem, etiqueta_de_secao, formatar_custo, icone},
    dialogos::{com_til, nome_da_pasta},
};
use crate::{
    modelo::agora,
    motor::{
        Motor,
        gasto::{Gasto, GastoDaPasta, comeco_do_dia, resumir},
    },
    tema::{Cores, Lucide, Tipo as _, esp, r, raio},
};

/// Quantas pastas e quantos chats por pasta aparecem; o resto vira uma linha de soma.
const PASTAS: usize = 8;
const CHATS: usize = 5;

/// "850 tokens", "1,8 mil tokens", "182 mil tokens", "1,2 mi tokens".
pub(super) fn tokens_curtos(n: u64) -> String {
    let decimal = |valor: f64| format!("{valor:.1}").replace('.', ",");
    match n {
        0..=999 => contagem(n as usize, "token", "tokens"),
        1_000..=9_949 => format!("{} mil tokens", decimal(n as f64 / 1e3)),
        9_950..=999_499 => format!("{} mil tokens", (n as f64 / 1e3).round() as u64),
        999_500..=9_949_999 => format!("{} mi tokens", decimal(n as f64 / 1e6)),
        _ => format!("{} mi tokens", (n as f64 / 1e6).round() as u64),
    }
}

/// "182 mil tokens · US$ 0,84": o gasto numa linha. Sem custo (o Codex não diz), só os tokens;
/// sem tokens (um `result` de versão velha), só o custo.
pub(super) fn gasto_curto(gasto: &Gasto) -> String {
    let tokens = gasto.tokens.total();
    match (tokens > 0, gasto.custo > 0.0) {
        (true, true) => format!("{} · {}", tokens_curtos(tokens), formatar_custo(gasto.custo)),
        (true, false) => tokens_curtos(tokens),
        (false, _) => formatar_custo(gasto.custo),
    }
}

/// O começo do período: hoje, ou os 7 dias que terminam hoje (o dia de hoje e os 6 anteriores).
fn desde(periodo: usize, agora: u64) -> u64 {
    let hoje = comeco_do_dia(agora);
    if periodo == 0 { hoje } else { hoje.saturating_sub(6 * 86_400) }
}

pub struct OndeOUsoFoi {
    motor: Entity<Motor>,
    janela: WeakEntity<Janela>,
    /// 0: hoje; 1: 7 dias.
    periodo: usize,
    /// As pastas abertas (mostrando os chats).
    abertas: HashSet<String>,
    _assinatura: Subscription,
}

impl OndeOUsoFoi {
    pub fn new(motor: Entity<Motor>, janela: WeakEntity<Janela>, cx: &mut Context<Self>) -> Self {
        let assinatura = cx.observe(&motor, |_, _, cx| cx.notify());
        Self { motor, janela, periodo: 0, abertas: HashSet::new(), _assinatura: assinatura }
    }

    fn alternar(&mut self, pasta: &str, cx: &mut Context<Self>) {
        if !self.abertas.remove(pasta) {
            self.abertas.insert(pasta.to_string());
        }
        cx.notify();
    }

    /// O clique num chat: o diálogo fecha e a conversa abre no painel ativo.
    fn abrir_chat(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        window.close_dialog(cx);
        let _ = self.janela.update(cx, |j, cx| j.selecionar(Some(chat), window, cx));
    }

    /// Uma pasta: o nome, o gasto, a barra (do tamanho dela perto da que mais gastou) e, aberta,
    /// os chats.
    fn pasta(&self, ix: usize, pasta: &GastoDaPasta, maior: f64, por_custo: bool, window: &Window, cx: &Context<Self>) -> AnyElement {
        let aberta = self.abertas.contains(&pasta.pasta);
        let caminho = Path::new(&pasta.pasta);
        let chave = pasta.pasta.clone();
        let cabeca = v_flex()
            .id(("gasto-pasta", ix))
            .gap(esp::E6)
            .px(esp::E12)
            .py(esp::E8)
            .cursor_pointer()
            .hover(|s| s.bg(Cores::hover()))
            .child(
                h_flex()
                    .items_start()
                    .gap(esp::E6)
                    .child(
                        div()
                            .flex_none()
                            .h(r(18.))
                            .flex()
                            .items_center()
                            .text_color(Cores::tenue())
                            .child(icone(if aberta { Lucide::ChevronDown } else { Lucide::ChevronRight }, 12., window)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().rotulo().text_color(Cores::texto()).child(nome_da_pasta(caminho)))
                            .child(div().metadado().text_color(Cores::apagado()).child(com_til(caminho))),
                    )
                    .child(div().flex_none().metadado().text_color(Cores::forte()).child(gasto_curto(&pasta.gasto))),
            )
            .child(barra(pasta.gasto.peso(por_custo), maior, 6.))
            .on_click(cx.listener(move |this, _, _, cx| this.alternar(&chave, cx)));
        if !aberta {
            return cabeca.into_any_element();
        }
        let motor = self.motor.read(cx);
        let maior_chat = pasta.chats.first().map_or(0.0, |c| c.gasto.peso(por_custo));
        let chats = pasta.chats.iter().take(CHATS).enumerate().map(|(k, c)| {
            let titulo = motor.chat(&c.chat).map(|chat| chat.titulo.clone());
            let existe = titulo.is_some();
            let id = c.chat.clone();
            v_flex()
                .id(SharedString::from(format!("gasto-chat-{ix}-{k}")))
                .gap(esp::E4)
                .pl(esp::E32)
                .pr(esp::E12)
                .py(esp::E6)
                .child(
                    h_flex()
                        .items_start()
                        .gap(esp::E8)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(r(13.))
                                .line_height(r(18.))
                                .text_color(if existe { Cores::texto() } else { Cores::apagado() })
                                .child(titulo.unwrap_or_else(|| "Chat apagado".into())),
                        )
                        .child(div().flex_none().metadado().text_color(Cores::tenue()).child(gasto_curto(&c.gasto))),
                )
                .child(barra(c.gasto.peso(por_custo), maior_chat, 4.))
                .when(existe, |s| {
                    s.cursor_pointer()
                        .hover(|s| s.bg(Cores::hover()))
                        .on_click(cx.listener(move |this, _, window, cx| this.abrir_chat(id.clone(), window, cx)))
                })
        });
        let resto = pasta.chats.len().saturating_sub(CHATS);
        v_flex()
            .child(cabeca)
            .children(chats)
            .when(resto > 0, |s| {
                let mut soma = Gasto::default();
                for c in &pasta.chats[CHATS..] {
                    soma.custo += c.gasto.custo;
                    soma.tokens.somar(c.gasto.tokens);
                }
                s.child(
                    div()
                        .pl(esp::E32)
                        .pr(esp::E12)
                        .py(esp::E6)
                        .metadado()
                        .text_color(Cores::tenue())
                        .child(format!("e mais {} · {}", contagem(resto, "chat", "chats"), gasto_curto(&soma))),
                )
            })
            .into_any_element()
    }
}

/// A barra de um gasto perto do maior da lista, na trilha da borda.
fn barra(valor: f64, maior: f64, altura: f32) -> Div {
    let fracao = if maior > 0.0 { (valor / maior).clamp(0.0, 1.0) as f32 } else { 0.0 };
    div()
        .h(r(altura))
        .w_full()
        .rounded(raio::PILULA)
        .bg(Cores::traco())
        .child(div().h_full().w(relative(fracao)).rounded(raio::PILULA).bg(Cores::destaque()))
}

impl Render for OndeOUsoFoi {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let resumo = resumir(self.motor.read(cx).gastos().turnos(), desde(self.periodo, agora()));
        let entidade = cx.entity().downgrade();
        let seletor = ControleSegmentado::new("gasto-periodo", vec!["Hoje", "7 dias"], self.periodo, move |novo, _, cx| {
            let _ = entidade.update(cx, |this, cx| {
                this.periodo = novo;
                cx.notify();
            });
        })
        .small();
        let maior = resumo.pastas.first().map_or(0.0, |p| p.gasto.peso(resumo.por_custo));
        let resto = resumo.pastas.len().saturating_sub(PASTAS);
        let corpo = if resumo.pastas.is_empty() {
            div()
                .px(esp::E12)
                .py(esp::E12)
                .metadado()
                .text_color(Cores::tenue())
                .child(if self.periodo == 0 { "Nenhum turno terminou hoje." } else { "Nenhum turno terminou nos últimos 7 dias." })
                .into_any_element()
        } else {
            v_flex()
                .children(
                    resumo
                        .pastas
                        .iter()
                        .take(PASTAS)
                        .enumerate()
                        .map(|(ix, pasta)| self.pasta(ix, pasta, maior, resumo.por_custo, window, cx)),
                )
                .when(resto > 0, |s| {
                    s.child(
                        div()
                            .px(esp::E12)
                            .py(esp::E8)
                            .metadado()
                            .text_color(Cores::tenue())
                            .child(format!("e mais {}", contagem(resto, "pasta", "pastas"))),
                    )
                })
                .into_any_element()
        };
        let total = (resumo.total.turnos > 0)
            .then(|| format!("{} · {}", contagem(resumo.total.turnos, "turno", "turnos"), gasto_curto(&resumo.total)));
        v_flex()
            .pt(esp::E16)
            .gap(esp::E6)
            .child(
                h_flex()
                    .gap(esp::E8)
                    .child(div().flex_1().min_w_0().child(etiqueta_de_secao("Onde o uso foi").px(esp::E4)))
                    .children(total.map(|t| div().flex_none().metadado().text_color(Cores::tenue()).child(t)))
                    .child(div().flex_none().child(seletor)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .rounded(raio::CARTAO)
                    .bg(Cores::grupo())
                    .border_1()
                    .border_color(Cores::borda())
                    .child(corpo),
            )
            .child(
                div()
                    .px(esp::E4)
                    .metadado()
                    .text_color(Cores::tenue())
                    .child("Custo estimado pelo preço da API, contado desde que esta versão foi instalada."),
            )
    }
}

#[cfg(test)]
mod testes {
    use super::{desde, gasto_curto, tokens_curtos};
    use crate::motor::{Tokens, gasto::Gasto};

    #[test]
    fn tokens_em_poucas_letras() {
        assert_eq!(tokens_curtos(0), "0 tokens");
        assert_eq!(tokens_curtos(1), "1 token");
        assert_eq!(tokens_curtos(850), "850 tokens");
        assert_eq!(tokens_curtos(1_849), "1,8 mil tokens");
        assert_eq!(tokens_curtos(9_960), "10 mil tokens");
        assert_eq!(tokens_curtos(182_400), "182 mil tokens");
        assert_eq!(tokens_curtos(999_700), "1,0 mi tokens");
        assert_eq!(tokens_curtos(1_240_000), "1,2 mi tokens");
        assert_eq!(tokens_curtos(48_600_000), "49 mi tokens");
    }

    #[test]
    fn gasto_numa_linha() {
        let tokens = Tokens { entrada: 2_000, cache_lido: 170_000, cache_criado: 6_000, saida: 4_000 };
        assert_eq!(gasto_curto(&Gasto { custo: 0.84, tokens, turnos: 1 }), "182 mil tokens · US$ 0,84");
        // O Codex não diz custo; um `result` antigo não diz tokens.
        assert_eq!(gasto_curto(&Gasto { custo: 0.0, tokens, turnos: 1 }), "182 mil tokens");
        assert_eq!(gasto_curto(&Gasto { custo: 1.5, tokens: Tokens::default(), turnos: 1 }), "US$ 1,50");
    }

    #[test]
    fn periodo_de_hoje_e_de_sete_dias() {
        let agora = 1_800_000_000;
        let hoje = desde(0, agora);
        assert!(hoje <= agora && agora - hoje < 90_000);
        assert_eq!(hoje - desde(1, agora), 6 * 86_400);
    }
}
