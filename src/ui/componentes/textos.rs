//! Formatadores de texto únicos: o mesmo número sai escrito igual em todo lugar.

/// "1 tarefa", "3 tarefas": o número com a palavra no singular ou no plural.
pub fn contagem(n: usize, singular: &str, plural: &str) -> String {
    format!("{n} {}", if n == 1 { singular } else { plural })
}

/// "US$ 0,42", "< US$ 0,01" (gasto que arredonda para zero) e "US$ 1.234,56".
pub fn formatar_custo(usd: f64) -> String {
    if usd > 0. && usd < 0.005 {
        return "< US$ 0,01".into();
    }
    let centavos = (usd.max(0.) * 100.).round() as u64;
    let inteiro = (centavos / 100).to_string();
    let mut milhar = String::new();
    for (i, c) in inteiro.chars().enumerate() {
        if i > 0 && (inteiro.len() - i) % 3 == 0 {
            milhar.push('.');
        }
        milhar.push(c);
    }
    format!("US$ {milhar},{:02}", centavos % 100)
}

/// "19 s", "2 min 13 s", "1 h 05 min": quanto algo levou (ou leva), com espaço como o resto do
/// app ("2 min", "1 h"). Os segundos e os minutos de dentro têm dois dígitos, para o tempo que
/// anda não mudar de largura a cada batida.
pub fn duracao_curta(s: u64) -> String {
    match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} min {:02} s", s / 60, s % 60),
        _ => format!("{} h {:02} min", s / 3600, (s % 3600) / 60),
    }
}

fn porcento(fracao: f32) -> u32 {
    (fracao * 100.).round().max(0.) as u32
}

/// "5 h 23% · semana 41%": as janelas que já foram lidas.
pub fn resumo_uso(cinco_h: Option<f32>, semana: Option<f32>) -> String {
    let partes: Vec<String> = [
        cinco_h.map(|f| format!("5 h {}%", porcento(f))),
        semana.map(|f| format!("semana {}%", porcento(f))),
    ]
    .into_iter()
    .flatten()
    .collect();
    if partes.is_empty() {
        "uso ainda não lido".into()
    } else {
        partes.join(" · ")
    }
}

/// O texto longo do uso de um serviço, com a hora em que cada janela renova:
/// "Claude: 23% do limite de 5 h (renova em 2 h 13 min · 15:59) · 41% da semana".
pub fn dica_uso(
    servico: &str,
    cinco_h: Option<f32>,
    renova_5h: Option<&str>,
    semana: Option<f32>,
    renova_semana: Option<&str>,
) -> String {
    let janela = |fracao: Option<f32>, nome: &str, renova: Option<&str>| {
        fracao.map(|f| {
            let mut texto = format!("{}% {nome}", porcento(f));
            if let Some(renova) = renova {
                texto.push_str(&format!(" ({renova})"));
            }
            texto
        })
    };
    let partes: Vec<String> = [
        janela(cinco_h, "do limite de 5 h", renova_5h),
        janela(semana, "da semana", renova_semana),
    ]
    .into_iter()
    .flatten()
    .collect();
    if partes.is_empty() {
        format!("{servico}: uso ainda não lido")
    } else {
        format!("{servico}: {}", partes.join(" · "))
    }
}

/// Para texto fora de layout (o `notify-send`): caminho perde o começo, inteiro por pasta
/// ("…/worktrees/loja-api/t2"); o resto perde o meio. Dentro da tela, use o
/// `text_ellipsis_middle` do GPUI.
pub fn truncar_no_meio(texto: &str, max: usize) -> String {
    let total = texto.chars().count();
    if total <= max {
        return texto.to_string();
    }
    if texto.contains('/') {
        let mut fim = String::new();
        for parte in texto.rsplit('/').filter(|p| !p.is_empty()) {
            let candidato = if fim.is_empty() { parte.to_string() } else { format!("{parte}/{fim}") };
            if candidato.chars().count() + 2 > max {
                break;
            }
            fim = candidato;
        }
        if !fim.is_empty() {
            return format!("…/{fim}");
        }
    }
    let sobra = max.saturating_sub(1);
    let inicio: String = texto.chars().take(sobra - sobra / 2).collect();
    let fim: String = texto.chars().skip(total - sobra / 2).collect();
    format!("{inicio}…{fim}")
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn contagem_no_singular_e_no_plural() {
        assert_eq!(contagem(1, "tarefa", "tarefas"), "1 tarefa");
        assert_eq!(contagem(0, "tarefa", "tarefas"), "0 tarefas");
        assert_eq!(contagem(3, "tarefa", "tarefas"), "3 tarefas");
    }

    #[test]
    fn duracao_com_espaco_como_o_resto_do_app() {
        assert_eq!(duracao_curta(0), "0 s");
        assert_eq!(duracao_curta(19), "19 s");
        assert_eq!(duracao_curta(60), "1 min 00 s");
        assert_eq!(duracao_curta(133), "2 min 13 s");
        assert_eq!(duracao_curta(3599), "59 min 59 s");
        assert_eq!(duracao_curta(3900), "1 h 05 min");
        assert_eq!(duracao_curta(26 * 3600), "26 h 00 min");
    }

    #[test]
    fn custo_em_reais_de_dolar() {
        assert_eq!(formatar_custo(0.42), "US$ 0,42");
        assert_eq!(formatar_custo(0.), "US$ 0,00");
        assert_eq!(formatar_custo(0.004), "< US$ 0,01");
        assert_eq!(formatar_custo(0.005), "US$ 0,01");
        assert_eq!(formatar_custo(12.5), "US$ 12,50");
        assert_eq!(formatar_custo(1234.56), "US$ 1.234,56");
        assert_eq!(formatar_custo(1234567.), "US$ 1.234.567,00");
    }

    #[test]
    fn resumo_do_uso_so_com_o_que_foi_lido() {
        assert_eq!(resumo_uso(Some(0.23), Some(0.41)), "5 h 23% · semana 41%");
        assert_eq!(resumo_uso(Some(0.07), None), "5 h 7%");
        assert_eq!(resumo_uso(None, Some(1.)), "semana 100%");
        assert_eq!(resumo_uso(None, None), "uso ainda não lido");
        assert_eq!(
            dica_uso("Claude", Some(0.23), Some("renova em 2 h 13 min · 15:59"), Some(0.41), None),
            "Claude: 23% do limite de 5 h (renova em 2 h 13 min · 15:59) · 41% da semana"
        );
    }

    #[test]
    fn caminho_perde_o_comeco_e_texto_perde_o_meio() {
        assert_eq!(
            truncar_no_meio("/home/victor/.cache/worktrees/loja-api/t2", 28),
            "…/worktrees/loja-api/t2"
        );
        assert_eq!(truncar_no_meio("curto", 28), "curto");
        assert_eq!(truncar_no_meio("Aplicar cupom no checkout da loja", 15), "Aplicar…da loja");
        assert_eq!(truncar_no_meio("Aplicar cupom no checkout da loja", 15).chars().count(), 15);
        // A última pasta sozinha já não cabe: corta no meio.
        assert_eq!(truncar_no_meio("/x/nome-de-pasta-comprido-demais", 12), "/x/nom…emais");
    }
}
