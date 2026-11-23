//! Os agentes em segundo plano na conversa: enquanto a resposta já veio e eles seguem
//! trabalhando, uma linha por agente no fim da conversa (a descrição, a última ação e há quanto
//! tempo ele roda; o clique abre a conversa dele numa aba); e, no ponto em que cada um termina,
//! a linha "Terminou: … · 25 min" (ou "Falhou: …"), que abre o relatório dele. De onde vêm os
//! dados: `motor::fundo`.

use super::*;
use crate::motor::fundo::Linha;

/// A última ação de um agente que ainda trabalha, como a tela a diz: "Bash · cargo test" vira
/// "Rodando cargo test…" e "Read · painel_arquivo.rs", "Lendo painel_arquivo.rs…"; o que ele
/// disse fica como está; sem ação nenhuma, "Começando…". É o mesmo texto na linha da conversa e
/// na lista da pílula.
pub(in crate::ui) fn acao_ao_vivo(atividade: &str) -> String {
    let atividade = atividade.trim();
    if atividade.is_empty() {
        return "Começando…".to_string();
    }
    let (nome, detalhe) = match atividade.split_once(" · ") {
        Some((nome, detalhe)) if !nome.contains(' ') => (nome, detalhe.trim().trim_end_matches('…').trim_end()),
        // Uma palavra só: uma ferramenta sem detalhe, ou o que ele disse.
        None if !atividade.contains(' ') => (atividade, ""),
        _ => return atividade.to_string(),
    };
    let verbo = verbo_da_ferramenta(nome, true);
    // O que não é uma ferramenta conhecida (o verbo volta o próprio nome) fica como veio.
    if verbo == nome_curto(nome) {
        return atividade.to_string();
    }
    // Os passos já dizem tudo no verbo ("Atualizando os passos").
    if detalhe.is_empty() || crate::motor::passos::e_de_passos(nome) { format!("{verbo}…") } else { format!("{verbo} {detalhe}…") }
}

/// Quanto um agente levou, por extenso e sem precisão que não importa: "45 s", "3 min 20 s",
/// "25 min", "1 h 05 min". Vazio quando não foi medido.
pub(in crate::ui) fn tempo_do_agente(segundos: u64) -> String {
    match segundos {
        0 => String::new(),
        1..60 => format!("{segundos} s"),
        60..600 if segundos % 60 == 0 => format!("{} min", segundos / 60),
        60..600 => format!("{} min {} s", segundos / 60, segundos % 60),
        600..3600 => format!("{} min", segundos / 60),
        _ => format!("{} h {:02} min", segundos / 3600, (segundos % 3600) / 60),
    }
}

/// As partes da linha do fim: a palavra ("Terminou", "Falhou"), a descrição e quanto levou.
fn partes_do_fim(descricao: &str, erro: bool, durou: u64) -> (&'static str, String, Option<String>) {
    let descricao = match descricao.trim() {
        "" => "o agente",
        descricao => descricao,
    };
    let tempo = tempo_do_agente(durou);
    (if erro { "Falhou" } else { "Terminou" }, descricao.to_string(), (!tempo.is_empty()).then_some(tempo))
}

/// A linha do fim de um agente: "Terminou: Mapear o carrinho · 25 min", "Falhou: …".
pub(in crate::ui) fn texto_do_fim(descricao: &str, erro: bool, durou: u64) -> String {
    let (palavra, descricao, tempo) = partes_do_fim(descricao, erro, durou);
    match tempo {
        Some(tempo) => format!("{palavra}: {descricao} · {tempo}"),
        None => format!("{palavra}: {descricao}"),
    }
}

/// O que a linha de estado da caixa diz enquanto os agentes seguem em segundo plano (ela só
/// aparece com a conversa rolada para cima): o agente e o que ele faz, ou quantos são.
pub(in crate::ui) fn atividade_no_fundo(linhas: &[Linha]) -> String {
    match linhas {
        [] => "Agentes trabalhando em segundo plano…".to_string(),
        [uma] => format!("{}: {}", uma.descricao, acao_ao_vivo(&uma.atividade)),
        varias => format!("{} agentes trabalhando em segundo plano…", varias.len()),
    }
}

impl Janela {
    /// A linha de um agente que segue trabalhando em segundo plano, no fim da conversa: o anel
    /// girando, a descrição dele, o que ele faz agora e há quanto tempo foi lançado. O clique abre
    /// a conversa dele numa aba (quando o cartão dele está nesta conversa).
    pub(super) fn linha_do_agente(&self, contexto: &Contexto, n: usize, linha: &Linha, cx: &Context<Self>) -> AnyElement {
        let estreita = minima(contexto.nivel);
        let acao = if linha.fluxo && linha.atividade.trim().is_empty() { "Fluxo de agentes".to_string() } else { sem_marcacao(&acao_ao_vivo(&linha.atividade)) };
        let abre = linha
            .chamada
            .clone()
            .filter(|_| contexto.chat.agente == crate::modelo::Agente::Claude)
            .map(|chamada| (contexto.painel, contexto.chat.id.clone(), chamada));
        let modelo = linha.modelo.as_deref().map(super::super::composer::nome_curto_modelo);
        let dica = SharedString::from(
            [
                Some(format!("{} · há {}", linha.descricao, tempo_do_agente(linha.segundos.max(1)))),
                Some(acao.clone()),
                modelo.map(|m| format!("Modelo: {m}")),
                abre.is_some().then(|| "Clique para abrir a conversa dele numa aba".to_string()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n"),
        );
        h_flex()
            .id(SharedString::from(format!("agente-no-fundo-{}-{n}", contexto.chat.id)))
            .map(linha_de_log)
            // Só o lugar do anel: ele gira por cima, numa vista própria (`AneisDaConversa`).
            .child(coluna_do_icone().child(super::super::componentes::lugar_do_anel(contexto.painel, Cores::destaque(), 14.)))
            // A descrição é quem ele é: fica com até metade da linha, e a ação com o que sobra.
            .child(div().flex_shrink(1.).min_w_0().max_w(relative(if estreita { 0.7 } else { 0.5 })).truncate().text_color(Cores::texto()).child(linha.descricao.clone()))
            .child(div().flex_1().min_w_0().truncate().text_color(Cores::tenue()).child(acao))
            // O relógio em mono: os dígitos têm a mesma largura, e a linha não treme a cada segundo.
            .child(div().flex_none().font_family(FONTE_MONO).text_color(Cores::apagado()).child(relogio(linha.segundos)))
            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            .when_some(abre, |s, (painel, chat, chamada)| {
                s.cursor_pointer()
                    .hover(|s| s.bg(Cores::hover()))
                    .child(div().flex_none().text_color(Cores::apagado()).child(icone_de(Lucide::ChevronRight, 12.)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.abrir_subagente(painel, chat.clone(), chamada.clone(), window, cx);
                    }))
            })
            .into_any_element()
    }

    /// O fim de um agente em segundo plano, no ponto da conversa em que ele terminou: "Terminou:
    /// Mapear o carrinho · 25 min" (ou "Falhou: …", com a palavra em vermelho). O relatório dele,
    /// que mora no cartão da chamada, fica recolhido e abre no clique.
    pub(super) fn fim_do_agente(&self, contexto: &Contexto, indice: usize, item: &Item, cx: &Context<Self>) -> AnyElement {
        let Item::FimDoAgente { chamada, descricao, erro, durou, .. } = item else {
            return div().into_any_element();
        };
        let chave = format!("{}:{indice}", contexto.chat.id);
        let relatorio = contexto
            .chat
            .itens
            .iter()
            .rev()
            .find_map(|item| match item {
                Item::Ferramenta { id, resultado: Some(resultado), .. } if id == chamada => Some(crate::claude::relatorio_do_subagente(resultado)),
                _ => None,
            })
            .filter(|relatorio| !relatorio.trim().is_empty());
        let aberto = relatorio.is_some() && self.expandidos.contains(&chave);
        let descricao_inteira = descricao.clone();
        let (palavra, descricao, tempo) = partes_do_fim(descricao, *erro, *durou);
        let cor = if *erro { Cores::vermelho() } else { Cores::tenue() };
        let linha = h_flex()
            .id(SharedString::from(format!("fim-do-agente-{chave}")))
            .map(linha_de_log)
            .child(coluna_do_icone().child(icone_de(if *erro { Lucide::CircleX } else { Lucide::CircleCheck }, 14.).text_color(cor)))
            .child(div().flex_none().text_color(cor).child(format!("{palavra}:")))
            .child(div().flex_shrink(1.).min_w_0().truncate().text_color(Cores::texto()).child(descricao))
            .when_some(tempo, |s, tempo| s.child(div().flex_none().text_color(Cores::apagado()).child(format!("· {tempo}"))))
            .when(relatorio.is_some(), |s| {
                // A linha inteira na dica: numa coluna estreita a descrição sai cortada.
                let dica = SharedString::from(format!(
                    "{}\n{}",
                    texto_do_fim(&descricao_inteira, *erro, *durou),
                    if aberto { "Clique para recolher o relatório" } else { "Clique para ver o relatório dele" }
                ));
                s.cursor_pointer()
                    .hover(|s| s.bg(Cores::hover()))
                    .child(div().flex_none().text_color(Cores::apagado()).child(icone_de(if aberto { Lucide::ChevronDown } else { Lucide::ChevronRight }, 12.)))
                    .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                    .on_click(alternar(chave.clone(), cx))
            });
        v_flex()
            .w_full()
            .child(linha)
            .when_some(relatorio.filter(|_| aberto), |s, relatorio| {
                // O mesmo cartão claro do relatório no cartão do subagente.
                s.child(
                    div().pl(RECUO_DO_LOG).py(esp::E4).child(
                        div()
                            .relative()
                            .group("relatorio")
                            .px(esp::E12)
                            .py(esp::E8)
                            .rounded(raio::CARTAO)
                            .bg(Cores::hover())
                            .text_size(r(14.))
                            .line_height(r(22.))
                            .text_color(Cores::texto())
                            .child(markdown(SharedString::from(format!("relatorio-do-fim-{chave}")), relatorio.clone(), cx))
                            .child(
                                div()
                                    .absolute()
                                    .top(esp::E6)
                                    .right(esp::E6)
                                    .invisible()
                                    .group_hover("relatorio", |s| s.visible())
                                    .child(BotaoCopiar::new(SharedString::from(format!("copiar-relatorio-do-fim-{chave}")), relatorio).dica("Copiar o relatório")),
                            ),
                    ),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod testes {
    // Sem o `use super::*`: ele traria o `test` do GPUI, e o `#[test]` daqui deixaria de ser o do Rust.
    use super::{Linha, acao_ao_vivo, atividade_no_fundo, tempo_do_agente, texto_do_fim};

    /// Item 1: a última ação ("Rodando cargo test…", "Lendo painel_arquivo.rs").
    #[test]
    fn a_ultima_acao_do_agente_no_gerundio() {
        assert_eq!(acao_ao_vivo("Bash · cargo test"), "Rodando cargo test…");
        assert_eq!(acao_ao_vivo("Read · painel_arquivo.rs"), "Lendo painel_arquivo.rs…");
        assert_eq!(acao_ao_vivo("Grep · cupom"), "Buscando cupom…");
        assert_eq!(acao_ao_vivo("Edit · views.py"), "Editando views.py…");
        // O resumo que já veio cortado com reticências não ganha outras.
        assert_eq!(acao_ao_vivo("Bash · python3 manage.py test produtos…"), "Rodando python3 manage.py test produtos…");
        // Os passos dizem o verbo, sem a "lista de afazeres"; a ferramenta sem detalhe, só o verbo.
        assert_eq!(acao_ao_vivo("TodoWrite · lista de afazeres"), "Atualizando os passos…");
        assert_eq!(acao_ao_vivo("TaskList"), "Conferindo os passos…");
        // O que ele disse fica como está, também quando é uma palavra só.
        assert_eq!(acao_ao_vivo("Conferindo os testes do carrinho."), "Conferindo os testes do carrinho.");
        assert_eq!(acao_ao_vivo("contei"), "contei");
        // Ainda sem ação nenhuma.
        assert_eq!(acao_ao_vivo(""), "Começando…");
        assert_eq!(acao_ao_vivo("  "), "Começando…");
    }

    /// Item 3: "Terminou: <descrição> · 25 min"; a linha de falha diz "Falhou: …".
    #[test]
    fn a_linha_do_fim_do_agente() {
        assert_eq!(texto_do_fim("Mapear o carrinho", false, 1_500), "Terminou: Mapear o carrinho · 25 min");
        assert_eq!(texto_do_fim("Conferir os cupons", true, 185), "Falhou: Conferir os cupons · 3 min 5 s");
        // Sem a medida, só o que terminou; sem descrição (o cartão não a tinha), "o agente".
        assert_eq!(texto_do_fim("Mapear o carrinho", false, 0), "Terminou: Mapear o carrinho");
        assert_eq!(texto_do_fim("  ", true, 40), "Falhou: o agente · 40 s");
        assert_eq!(tempo_do_agente(45), "45 s");
        assert_eq!(tempo_do_agente(120), "2 min");
        assert_eq!(tempo_do_agente(200), "3 min 20 s");
        // De dez minutos em diante os segundos não importam; de uma hora em diante, horas e minutos.
        assert_eq!(tempo_do_agente(1_529), "25 min");
        assert_eq!(tempo_do_agente(3_900), "1 h 05 min");
        assert_eq!(tempo_do_agente(0), "");
    }

    /// Com a conversa rolada para cima, a linha da caixa diz o agente e o que ele faz, ou quantos são.
    #[test]
    fn a_linha_da_caixa_diz_o_agente_ou_quantos_sao() {
        let linha = |descricao: &str, atividade: &str| Linha {
            id: "a".into(),
            chamada: None,
            descricao: descricao.into(),
            segundos: 10,
            atividade: atividade.into(),
            modelo: None,
            fluxo: false,
        };
        assert_eq!(atividade_no_fundo(&[linha("Mapear o carrinho", "Bash · cargo test")]), "Mapear o carrinho: Rodando cargo test…");
        assert_eq!(
            atividade_no_fundo(&[linha("Mapear o carrinho", "Bash · cargo test"), linha("Conferir os cupons", "")]),
            "2 agentes trabalhando em segundo plano…"
        );
        assert_eq!(atividade_no_fundo(&[]), "Agentes trabalhando em segundo plano…");
    }
}
