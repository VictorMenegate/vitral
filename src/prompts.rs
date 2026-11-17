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

#[cfg(test)]
mod testes {
    use super::*;

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
