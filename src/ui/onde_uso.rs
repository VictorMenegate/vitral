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
        Motor, Tokens,
        gasto::{Estatistica, Gasto, GastoDaPasta, Turno, cache_restante, comeco_do_dia, resumir},
    },
    tema::{Cores, Lucide, Tipo as _, esp, r, raio},
};

/// Quantas pastas e quantos chats por pasta aparecem; o resto vira uma linha de soma.
const PASTAS: usize = 8;
const CHATS: usize = 5;

/// "850", "1,8 mil", "8 mil", "182 mil", "1,2 mi": uma quantidade de tokens em poucas letras.
pub(super) fn quantia_curta(n: u64) -> String {
    // Uma casa, sem o ",0" de enfeite.
    let decimal = |valor: f64| format!("{valor:.1}").trim_end_matches(".0").replace('.', ",");
    match n {
        0..=999 => n.to_string(),
        1_000..=9_949 => format!("{} mil", decimal(n as f64 / 1e3)),
        9_950..=999_499 => format!("{} mil", (n as f64 / 1e3).round() as u64),
        999_500..=9_949_999 => format!("{} mi", decimal(n as f64 / 1e6)),
        _ => format!("{} mi", (n as f64 / 1e6).round() as u64),
    }
}

/// "850 MB", "3,1 GB": a memória de um chat (em GB a partir de 1 GB, com uma casa).
pub(super) fn memoria_curta(bytes: u64) -> String {
    const MB: f64 = 1024. * 1024.;
    let megas = bytes as f64 / MB;
    if megas < 1024. {
        format!("{} MB", megas.round() as u64)
    } else {
        format!("{:.1} GB", megas / 1024.).replace('.', ",")
    }
}

/// A dica do relógio do turno: "Memória deste chat: 3,1 GB", com o que o número conta.
pub(super) fn dica_da_memoria(bytes: u64) -> String {
    format!("Memória deste chat: {}\nO agente e tudo o que ele abriu (comandos, testes, servidores)", memoria_curta(bytes))
}

/// "850 tokens", "1,8 mil tokens", "182 mil tokens", "1,2 mi tokens".
pub(super) fn tokens_curtos(n: u64) -> String {
    format!("{} {}", quantia_curta(n), if n == 1 { "token" } else { "tokens" })
}

/// Os três números de um turno: a entrada nova (a sem desconto e a que foi gravada no cache), o
/// que foi lido do cache e a saída.
pub(super) fn partes_dos_tokens(tokens: &Tokens) -> [String; 3] {
    [
        format!("{} de entrada nova", quantia_curta(tokens.entrada + tokens.cache_criado)),
        format!("{} lidos do cache", quantia_curta(tokens.cache_lido)),
        format!("{} de saída", quantia_curta(tokens.saida)),
    ]
}

/// A dica da linha do turno, com o gasto e os três números dos tokens embaixo.
pub(super) fn dica_com_gasto(dica: &str, gasto: Option<&Gasto>) -> String {
    let Some(gasto) = gasto else {
        return dica.to_string();
    };
    let mut linhas = vec![dica.to_string(), gasto_curto(gasto)];
    if gasto.tokens.total() > 0 {
        linhas.push(partes_dos_tokens(&gasto.tokens).join(" · "));
    }
    linhas.join("\n")
}

/// "0,4 s", "1,8 s", "45 s", "6 min 12 s": um tempo medido em milissegundos. Abaixo de 10 s, com
/// uma casa; daí em diante, como a duração do turno.
pub(super) fn tempo_curto(milis: u64) -> String {
    if milis < 9_950 {
        return format!("{} s", format!("{:.1}", milis as f64 / 1e3).trim_end_matches(".0").replace('.', ","));
    }
    super::mensagens::duracao_do_turno((milis as f64 / 1e3).round() as u64)
}

/// Quanto dos tokens de entrada do turno veio do cache, em porcento (a entrada é a nova, a
/// gravada no cache e a lida dele). Nada num turno sem entrada. Só diz 100 quando foi tudo.
pub(super) fn fatia_do_cache(tokens: &Tokens) -> Option<u32> {
    let entrada = tokens.entrada + tokens.cache_criado + tokens.cache_lido;
    if entrada == 0 {
        return None;
    }
    let porcento = (tokens.cache_lido as f64 * 100. / entrada as f64).round() as u32;
    Some(if tokens.cache_lido < entrada { porcento.min(99) } else { 100 })
}

/// A dica da linha do turno inteira: o gasto (`dica_com_gasto`) e, embaixo, quanto da entrada
/// veio do cache, o modelo com os passos, os tempos e quanto custou subir o Claude Code. Cada
/// linha só aparece com o dado dela: o turno de antes disto fica com o modelo e o cache.
pub(super) fn dica_do_turno(dica: &str, gasto: Option<&Gasto>, estatistica: Option<&Estatistica>) -> String {
    let mut linhas = vec![dica_com_gasto(dica, gasto)];
    let Some(Estatistica { modelo, tempos }) = estatistica else {
        return linhas.join("\n");
    };
    if let Some(porcento) = gasto.and_then(|g| fatia_do_cache(&g.tokens)) {
        linhas.push(format!("{porcento}% da entrada veio do cache"));
    }
    let mut do_modelo = Vec::new();
    if !modelo.is_empty() {
        do_modelo.push(nome_do_modelo(modelo));
    }
    if tempos.passos > 0 {
        do_modelo.push(contagem(tempos.passos as usize, "passo", "passos"));
    }
    let do_tempo: Vec<String> = [(tempos.total_ms, "no total"), (tempos.modelo_ms, "de modelo"), (tempos.ttft_ms, "até a primeira palavra")]
        .into_iter()
        .filter(|(milis, _)| *milis > 0)
        .map(|(milis, o_que)| format!("{} {o_que}", tempo_curto(milis)))
        .collect();
    linhas.extend([do_modelo, do_tempo].into_iter().filter(|partes| !partes.is_empty()).map(|partes| partes.join(" · ")));
    if let Some(subida) = tempos.subida_ms() {
        linhas.push(format!("{} para abrir o Claude Code", tempo_curto(subida)));
    }
    linhas.join("\n")
}

/// O nome de mostrar do modelo de um turno: "Opus 5.5" para os do Claude, o do catálogo para os
/// do Codex (ou o nome cru).
fn nome_do_modelo(modelo: &str) -> String {
    if modelo.starts_with("claude-") { super::composer::nome_curto_modelo(modelo) } else { crate::agente_codex::nome_do_modelo(modelo) }
}

/// "cache quente por mais 3 min" ou "cache frio": o que sobra da validade do cache do prompt, em
/// segundos. Os minutos arredondam para cima (com 2 min 10 s ainda são "3 min").
pub(super) fn estado_do_cache(restante: u64) -> String {
    match restante {
        0 => "cache frio".to_string(),
        1..60 => format!("cache quente por mais {restante} s"),
        60..=3540 => format!("cache quente por mais {} min", restante.div_ceil(60)),
        _ => format!("cache quente por mais {} h", restante.div_ceil(3600)),
    }
}

/// A linha do cache no popover do contexto, a partir do último turno anotado. Nada quando o chat
/// está trabalhando (o cache está em uso e o fim do último turno já não diz nada) e quando o
/// turno não diz a validade (o Codex, o turno de antes disto).
pub(super) fn linha_do_cache(turno: &Turno, rodando: bool, agora: u64) -> Option<String> {
    let validade = turno.tempos.cache_ttl;
    (!rodando && validade > 0).then(|| estado_do_cache(cache_restante(turno.em, validade, agora)))
}

/// "Último turno", no popover do contexto: a entrada nova, o cache lido e a saída do último turno
/// anotado da conversa, um embaixo do outro, e a linha "Cache", com por quanto tempo o cache dele
/// ainda vale (`linha_do_cache`, calculada quando o popover abre). Nada sem turno anotado ou sem
/// os tokens dele.
pub(super) fn ultimo_turno(motor: &Motor, chat: &str) -> Option<AnyElement> {
    let turno = motor.gastos().ultimo(chat).filter(|t| t.tokens.total() > 0)?;
    let linha = |rotulo: &'static str, valores: Vec<String>| {
        h_flex()
            .items_start()
            .gap(esp::E8)
            .py(esp::E4)
            .child(div().flex_none().w(r(104.)).metadado().line_height(r(18.)).text_color(Cores::tenue()).child(rotulo))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .text_size(r(13.))
                    .line_height(r(18.))
                    .text_color(Cores::texto())
                    .children(valores.into_iter().map(|valor| div().child(valor))),
            )
    };
    // O cache tem a linha dele, com o rótulo na coluna dos rótulos: a frase inteira na coluna dos
    // valores não cabia ("cache quente por mais 57 min" quebrava em duas linhas).
    let cache = linha_do_cache(turno, motor.execucao(chat).rodando(), agora())
        .map(|estado| linha("Cache", vec![estado.trim_start_matches("cache ").to_string()]));
    Some(v_flex().child(linha("Último turno", partes_dos_tokens(&turno.tokens).to_vec())).children(cache).into_any_element())
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
    use super::{
        desde, dica_com_gasto, dica_da_memoria, dica_do_turno, estado_do_cache, fatia_do_cache, gasto_curto, linha_do_cache, memoria_curta,
        partes_dos_tokens, tempo_curto, tokens_curtos,
    };
    use crate::motor::{
        Tokens,
        gasto::{Estatistica, Gasto, Tempos, Turno},
    };

    #[test]
    fn tokens_em_poucas_letras() {
        assert_eq!(tokens_curtos(0), "0 tokens");
        assert_eq!(tokens_curtos(1), "1 token");
        assert_eq!(tokens_curtos(850), "850 tokens");
        assert_eq!(tokens_curtos(1_849), "1,8 mil tokens");
        assert_eq!(tokens_curtos(9_960), "10 mil tokens");
        assert_eq!(tokens_curtos(182_400), "182 mil tokens");
        assert_eq!(tokens_curtos(999_700), "1 mi tokens");
        assert_eq!(tokens_curtos(8_000), "8 mil tokens");
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
    fn dica_do_turno_com_o_gasto() {
        let tokens = Tokens { entrada: 2_000, cache_lido: 170_000, cache_criado: 6_000, saida: 4_000 };
        let gasto = Gasto { custo: 0.84, tokens, turnos: 1 };
        // A entrada nova soma a sem desconto e a que foi para o cache.
        assert_eq!(partes_dos_tokens(&tokens), ["8 mil de entrada nova", "170 mil lidos do cache", "4 mil de saída"]);
        assert_eq!(
            dica_com_gasto("Ver o que o agente fez neste turno", Some(&gasto)),
            "Ver o que o agente fez neste turno\n182 mil tokens · US$ 0,84\n8 mil de entrada nova · 170 mil lidos do cache · 4 mil de saída"
        );
        // Conversa antiga, sem o dado: tudo como hoje.
        assert_eq!(dica_com_gasto("Ver o que o agente fez neste turno", None), "Ver o que o agente fez neste turno");
        // Só o custo (um `result` sem os tokens): a dica não inventa os três números.
        let so_custo = Gasto { custo: 0.5, tokens: Tokens::default(), turnos: 1 };
        assert_eq!(dica_com_gasto("Ver", Some(&so_custo)), "Ver\nUS$ 0,50");
    }

    #[test]
    fn tempo_em_poucas_letras() {
        assert_eq!(tempo_curto(0), "0 s");
        assert_eq!(tempo_curto(430), "0,4 s");
        assert_eq!(tempo_curto(1_807), "1,8 s");
        assert_eq!(tempo_curto(2_000), "2 s");
        assert_eq!(tempo_curto(9_940), "9,9 s");
        assert_eq!(tempo_curto(9_960), "10 s");
        assert_eq!(tempo_curto(45_400), "45 s");
        assert_eq!(tempo_curto(372_000), "6 min 12 s");
        assert_eq!(tempo_curto(3_780_000), "1 h 3 min");
    }

    #[test]
    fn quanto_da_entrada_veio_do_cache() {
        let tokens = Tokens { entrada: 2_000, cache_lido: 170_000, cache_criado: 6_000, saida: 4_000 };
        // 170 mil de 178 mil de entrada (a saída não entra na conta).
        assert_eq!(fatia_do_cache(&tokens), Some(96));
        assert_eq!(fatia_do_cache(&Tokens { entrada: 500, saida: 90, ..Tokens::default() }), Some(0));
        assert_eq!(fatia_do_cache(&Tokens { cache_lido: 40_000, ..Tokens::default() }), Some(100));
        // Quase tudo não é tudo.
        assert_eq!(fatia_do_cache(&Tokens { entrada: 1, cache_lido: 999_999, ..Tokens::default() }), Some(99));
        // Sem entrada (um `result` sem os tokens), não há do que tirar a porcentagem.
        assert_eq!(fatia_do_cache(&Tokens { saida: 90, ..Tokens::default() }), None);
        assert_eq!(fatia_do_cache(&Tokens::default()), None);
    }

    #[test]
    fn dica_do_turno_com_o_modelo_os_tempos_e_o_cache() {
        let tokens = Tokens { entrada: 2_000, cache_lido: 170_000, cache_criado: 6_000, saida: 4_000 };
        let gasto = Gasto { custo: 0.84, tokens, turnos: 1 };
        let tempos = Tempos { total_ms: 372_000, modelo_ms: 243_400, ttft_ms: 1_807, espera_ms: 3_369, passos: 12, cache_ttl: 3600 };
        let estatistica = Estatistica { modelo: "claude-opus-5-5".into(), tempos };
        // O que já havia fica igual, nas mesmas linhas; o novo vem embaixo.
        assert_eq!(
            dica_do_turno("Ver o que o agente fez neste turno", Some(&gasto), Some(&estatistica)),
            "Ver o que o agente fez neste turno\n\
             182 mil tokens · US$ 0,84\n\
             8 mil de entrada nova · 170 mil lidos do cache · 4 mil de saída\n\
             96% da entrada veio do cache\n\
             Opus 5.5 · 12 passos\n\
             6 min 12 s no total · 4 min 3 s de modelo · 1,8 s até a primeira palavra\n\
             1,6 s para abrir o Claude Code"
        );
        // Um passo só, e a primeira palavra sem a espera medida daqui (ou com ela menor): sem a
        // linha do Claude Code abrindo.
        let curto = Estatistica { modelo: "claude-haiku-5-5".into(), tempos: Tempos { total_ms: 2_526, modelo_ms: 1_501, ttft_ms: 1_807, espera_ms: 0, passos: 1, cache_ttl: 300 } };
        assert_eq!(
            dica_do_turno("Ver", Some(&gasto), Some(&curto)),
            "Ver\n182 mil tokens · US$ 0,84\n8 mil de entrada nova · 170 mil lidos do cache · 4 mil de saída\n\
             96% da entrada veio do cache\nHaiku 5.5 · 1 passo\n2,5 s no total · 1,5 s de modelo · 1,8 s até a primeira palavra"
        );
        // O turno anotado antes disto (e o do Codex): o livro só tem o modelo e os tokens.
        let antigo = Estatistica { modelo: "gpt-5".into(), tempos: Tempos::default() };
        assert_eq!(
            dica_do_turno("Ver", Some(&gasto), Some(&antigo)),
            "Ver\n182 mil tokens · US$ 0,84\n8 mil de entrada nova · 170 mil lidos do cache · 4 mil de saída\n96% da entrada veio do cache\ngpt-5"
        );
        // Só o custo, sem modelo e sem tempo: nada de linha vazia.
        let so_custo = Gasto { custo: 0.5, tokens: Tokens::default(), turnos: 1 };
        assert_eq!(dica_do_turno("Ver", Some(&so_custo), Some(&Estatistica::default())), "Ver\nUS$ 0,50");
        // Sem o tempo de modelo, a linha dos tempos fica com o que tem.
        let so_total = Estatistica { modelo: String::new(), tempos: Tempos { total_ms: 45_000, ..Tempos::default() } };
        assert_eq!(dica_do_turno("Ver", Some(&so_custo), Some(&so_total)), "Ver\nUS$ 0,50\n45 s no total");
        // Conversa antiga, sem o turno no livro: tudo como antes.
        assert_eq!(dica_do_turno("Ver o que o agente fez neste turno", None, None), "Ver o que o agente fez neste turno");
        assert_eq!(dica_do_turno("Ver", Some(&gasto), None), dica_com_gasto("Ver", Some(&gasto)));
    }

    #[test]
    fn cache_quente_ou_frio() {
        assert_eq!(estado_do_cache(0), "cache frio");
        assert_eq!(estado_do_cache(180), "cache quente por mais 3 min");
        // Os minutos arredondam para cima: com 2 min 1 s ainda são "3 min", e nunca "0 min".
        assert_eq!(estado_do_cache(121), "cache quente por mais 3 min");
        assert_eq!(estado_do_cache(120), "cache quente por mais 2 min");
        assert_eq!(estado_do_cache(60), "cache quente por mais 1 min");
        assert_eq!(estado_do_cache(59), "cache quente por mais 59 s");
        assert_eq!(estado_do_cache(1), "cache quente por mais 1 s");
        // O cache longo, de 1 h.
        assert_eq!(estado_do_cache(3_420), "cache quente por mais 57 min");
        assert_eq!(estado_do_cache(3_600), "cache quente por mais 1 h");

        let fim = 1_800_000_000;
        let turno = |ttl: u64| Turno { em: fim, chat: "a".into(), tempos: Tempos { cache_ttl: ttl, ..Tempos::default() }, ..Turno::default() };
        // 5 min de validade, contados do fim do último turno.
        assert_eq!(linha_do_cache(&turno(300), false, fim + 120).as_deref(), Some("cache quente por mais 3 min"));
        assert_eq!(linha_do_cache(&turno(300), false, fim + 300).as_deref(), Some("cache frio"));
        assert_eq!(linha_do_cache(&turno(300), false, fim + 9_000).as_deref(), Some("cache frio"));
        // A conta com o cache longo: aos 5 min ainda está quente.
        assert_eq!(linha_do_cache(&turno(3_600), false, fim + 300).as_deref(), Some("cache quente por mais 55 min"));
        // Com o chat trabalhando, e no turno que não diz a validade (o Codex, o de antes disto), nada.
        assert_eq!(linha_do_cache(&turno(300), true, fim + 120), None);
        assert_eq!(linha_do_cache(&turno(0), false, fim + 120), None);
    }

    #[test]
    fn memoria_do_chat_em_mb_e_gb() {
        assert_eq!(memoria_curta(0), "0 MB");
        assert_eq!(memoria_curta(850 * 1024 * 1024), "850 MB");
        assert_eq!(memoria_curta(1024 * 1024 * 1024), "1,0 GB");
        assert_eq!(memoria_curta(3_328_599_654), "3,1 GB");
        assert!(dica_da_memoria(3_328_599_654).starts_with("Memória deste chat: 3,1 GB\n"));
    }

    #[test]
    fn periodo_de_hoje_e_de_sete_dias() {
        let agora = 1_800_000_000;
        let hoje = desde(0, agora);
        assert!(hoje <= agora && agora - hoje < 90_000);
        assert_eq!(hoje - desde(1, agora), 6 * 86_400);
    }
}
