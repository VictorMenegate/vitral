//! O texto que o Vitral acrescenta ao prompt de sistema de cada chat.

use std::path::Path;

/// Onde o chat trabalha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Onde {
    /// Direto numa pasta escolhida pelo usuário.
    Direta,
    /// Numa pasta vazia só dele (o chat "Sem pasta").
    SemPasta,
    /// Direto na pasta de uma conversa trazida do Claude Code, que ele continua (não é só dele).
    Continuada,
}

/// O prompt de sistema de um chat. Vai por arquivo a cada turno: a pasta vale já no turno seguinte.
pub fn do_chat(cwd: &Path, onde: Onde, memoria_da_equipe: bool) -> String {
    let pasta = cwd.display();
    let mut texto = match onde {
        Onde::Direta => format!("Você é um chat do Vitral. Sua pasta é {pasta}: mexa direto nela e altere só o que for pedido."),
        Onde::SemPasta => format!("Você é um chat do Vitral. Sua pasta é {pasta}, só sua: os arquivos que você criar ficam lá."),
        Onde::Continuada => format!(
            "Você é um chat do Vitral que continua uma conversa começada no Claude Code. Sua pasta é {pasta}, a \
mesma da conversa original, e não é só sua: altere só o que for pedido."
        ),
    };
    texto.push_str(
        "\n\nQuem fala com você: mensagem sem prefixo é do usuário: converse e faça o que ele pedir. \
Mensagem que começa com [de: …] vem de outro chat do Vitral. Mensagens que começam com [Vitral] são avisos \
do Vitral.",
    );
    texto.push_str(
        "\n\nNão deixe comandos rodando em segundo plano: o seu processo termina no fim de cada vez. Responda em \
português do Brasil, sem emojis.",
    );
    if memoria_da_equipe {
        texto.push_str("\n\n");
        texto.push_str(crate::memoria_equipe::DICA_DO_PROMPT);
    }
    texto
}

/// Até quanto as instruções de quem usa podem ter (o diálogo não salva acima disso): elas vão em
/// todo turno de todo chat.
pub const LIMITE_DAS_INSTRUCOES: usize = 4000;

/// A frase que separa o que o Vitral manda do que quem usa escreveu.
const CABECALHO_DAS_INSTRUCOES: &str = "Instruções de quem usa o Vitral, para todos os chats:";

/// O prompt do chat com as instruções de quem usa (Configurações > Agentes) no fim: elas somam ao
/// que o Vitral manda, nunca entram no lugar. Sem instruções (ou só espaço), o prompt não muda.
pub fn com_instrucoes(prompt: String, instrucoes: &str) -> String {
    let instrucoes = instrucoes.trim();
    if instrucoes.is_empty() {
        return prompt;
    }
    format!("{prompt}\n\n{CABECALHO_DAS_INSTRUCOES}\n{instrucoes}")
}

/// "Nenhuma", ou o começo do texto entre aspas e o tamanho: o que a linha das Configurações diz.
pub fn resumo_das_instrucoes(instrucoes: &str) -> String {
    let texto = instrucoes.trim();
    let Some(primeira) = texto.lines().find(|l| !l.trim().is_empty()) else {
        return "Nenhuma".to_string();
    };
    let primeira = primeira.trim();
    let comeco: String = primeira.chars().take(60).collect();
    let cortou = comeco.len() < primeira.len() || texto.lines().filter(|l| !l.trim().is_empty()).count() > 1;
    let total = texto.chars().count();
    format!("“{}{}” · {total} {}", comeco.trim_end(), if cortou { "…" } else { "" }, if total == 1 { "caractere" } else { "caracteres" })
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn as_instrucoes_de_quem_usa_somam_ao_prompt_do_vitral() {
        let do_vitral = do_chat(Path::new("/p/loja"), Onde::Direta, true);
        let com = com_instrucoes(do_vitral.clone(), "  Rode os testes antes de dizer que terminou.\nCommits em português.  ");
        // O que o Vitral manda continua inteiro, na frente; as instruções vêm depois, aparadas.
        assert!(com.starts_with(&do_vitral), "{com}");
        assert!(com.ends_with(
            "\n\nInstruções de quem usa o Vitral, para todos os chats:\nRode os testes antes de dizer que terminou.\nCommits em português."
        ));
        // Vale para os três jeitos de chat.
        for onde in [Onde::SemPasta, Onde::Continuada] {
            let base = do_chat(Path::new("/x"), onde, false);
            assert!(com_instrucoes(base.clone(), "Seja breve.").starts_with(&base));
        }
    }

    #[test]
    fn sem_instrucoes_o_prompt_nao_muda() {
        let do_vitral = do_chat(Path::new("/p/loja"), Onde::Direta, false);
        assert_eq!(com_instrucoes(do_vitral.clone(), ""), do_vitral);
        assert_eq!(com_instrucoes(do_vitral.clone(), "  \n\t "), do_vitral);
    }

    #[test]
    fn a_linha_das_configuracoes_resume_as_instrucoes() {
        assert_eq!(resumo_das_instrucoes(""), "Nenhuma");
        assert_eq!(resumo_das_instrucoes("  \n "), "Nenhuma");
        assert_eq!(resumo_das_instrucoes("Seja breve."), "“Seja breve.” · 11 caracteres");
        assert_eq!(resumo_das_instrucoes("Seja breve.\nCommits em português."), "“Seja breve.…” · 33 caracteres");
        let longa = "a".repeat(200);
        assert_eq!(resumo_das_instrucoes(&longa), format!("“{}…” · 200 caracteres", "a".repeat(60)));
    }

    #[test]
    fn o_chat_diz_onde_trabalha() {
        let direto = do_chat(Path::new("/p/loja"), Onde::Direta, false);
        assert!(direto.starts_with("Você é um chat do Vitral. Sua pasta é /p/loja: mexa direto nela"), "{direto}");
        for saiu in ["projeto", "ajudante", "cópia", "Juntar", "VITRAL_PORT", "anotar_decisao", "informar_progresso"] {
            assert!(!direto.contains(saiu), "{saiu}");
        }
        // A mesma entrada dá o mesmo texto (o prompt vai por arquivo a cada turno).
        assert_eq!(direto, do_chat(Path::new("/p/loja"), Onde::Direta, false));

        let sem_pasta = do_chat(Path::new("/d/avulsos/abc"), Onde::SemPasta, false);
        assert!(sem_pasta.starts_with("Você é um chat do Vitral. Sua pasta é /d/avulsos/abc, só sua"));
        assert!(sem_pasta.contains("[de: …] vem de outro chat do Vitral"));
    }

    #[test]
    fn conversa_trazida_do_claude_code_na_pasta_dela() {
        let continuada = do_chat(Path::new("/home/v"), Onde::Continuada, false);
        assert!(continuada.starts_with("Você é um chat do Vitral que continua uma conversa começada no Claude Code. Sua pasta é /home/v"));
        assert!(continuada.contains("e não é só sua") && !continuada.contains("só sua: os arquivos"));
    }
}
