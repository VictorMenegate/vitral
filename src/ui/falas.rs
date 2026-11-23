//! O que um chat disse e fez, em poucas palavras, para as listas (dono, 16/11/2026: rodada de uso,
//! item 3.3). O sino mostrava "Resposta nova" em toda linha, o histórico do "+" repetia "· eita" em
//! todos os chats, e a dica da aba só tinha o título: para separar o chat que mexeu em arquivos do
//! que só respondeu era preciso abrir um por um. Tudo é tirado dos itens na hora, sem gravar nada
//! novo, e é a mesma montagem em todo lugar.

use crate::{
    modelo::{Chat, Execucao, Item},
    motor::Motor,
};

/// Até quantas letras a frase vai (a tela ainda corta com reticências).
const MAX_DA_FRASE: usize = 140;

/// A primeira frase de um texto, sem a marcação do markdown: a primeira linha com texto, cortada no
/// fim da primeira frase (". ", "! ", "? ", ": " no fim) e em até 140 letras.
pub(super) fn primeira_frase(texto: &str) -> String {
    let mut dentro_de_codigo = false;
    let linha = texto
        .lines()
        .map(str::trim)
        .filter(|l| {
            if l.starts_with("```") {
                dentro_de_codigo = !dentro_de_codigo;
                return false;
            }
            !dentro_de_codigo
        })
        .map(sem_marcas)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let fim = linha
        .char_indices()
        .find(|&(i, c)| matches!(c, '.' | '!' | '?') && linha[i + c.len_utf8()..].starts_with(' '))
        .map(|(i, c)| i + c.len_utf8());
    let frase = match fim {
        Some(fim) => &linha[..fim],
        None => linha.as_str(),
    };
    if frase.chars().count() > MAX_DA_FRASE {
        format!("{}…", frase.chars().take(MAX_DA_FRASE - 1).collect::<String>().trim_end())
    } else {
        frase.to_string()
    }
}

/// Uma linha sem as marcas do markdown que não fazem sentido numa lista: título, citação, item de
/// lista, negrito, código e link ("[texto](url)" vira "texto").
fn sem_marcas(linha: &str) -> String {
    let linha = linha.trim_start_matches(['#', '>']).trim_start();
    let linha = linha.strip_prefix("- ").or_else(|| linha.strip_prefix("* ")).unwrap_or(linha);
    let mut saida = String::with_capacity(linha.len());
    let mut resto = linha.replace("**", "").replace("__", "").replace('`', "");
    while let Some(abre) = resto.find('[') {
        let Some(meio) = resto[abre..].find("](").map(|m| abre + m) else { break };
        let Some(fecha) = resto[meio..].find(')').map(|f| meio + f) else { break };
        saida.push_str(&resto[..abre]);
        saida.push_str(&resto[abre + 1..meio]);
        resto = resto[fecha + 1..].to_string();
    }
    saida.push_str(&resto);
    saida.trim().to_string()
}

/// O começo da última fala do chat: a última resposta do agente ou, sem ela, a última mensagem
/// (sua ou entregue).
pub(super) fn ultima_fala(chat: &Chat) -> Option<String> {
    let texto = chat
        .itens
        .iter()
        .rev()
        .find_map(|i| match i {
            Item::Assistente { texto, .. } => Some(texto.as_str()),
            _ => None,
        })
        .or_else(|| {
            chat.itens.iter().rev().find_map(|i| match i {
                Item::Usuario { texto, .. } | Item::Entrega { texto, .. } => Some(texto.as_str()),
                _ => None,
            })
        })?;
    Some(primeira_frase(texto)).filter(|f| !f.is_empty())
}

/// O que o último turno fez, contado nas chamadas desde a sua última mensagem: "3 editados · 12
/// comandos". Vazio quando ele só respondeu.
pub(super) fn o_que_o_turno_fez(itens: &[Item]) -> String {
    let desde = itens.iter().rposition(|i| matches!(i, Item::Usuario { .. })).map_or(0, |i| i + 1);
    let mut editados: Vec<&str> = Vec::new();
    let mut comandos = 0;
    for item in &itens[desde..] {
        if let Item::Ferramenta { nome, arquivo, resumo, .. } = item {
            match super::mensagens::nome_curto(nome) {
                "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
                    let a = arquivo.as_deref().unwrap_or(resumo);
                    if !editados.contains(&a) {
                        editados.push(a);
                    }
                }
                "Bash" => comandos += 1,
                _ => {}
            }
        }
    }
    let mut partes = Vec::new();
    if !editados.is_empty() {
        partes.push(super::componentes::contagem(editados.len(), "editado", "editados"));
    }
    if comandos > 0 {
        partes.push(super::componentes::contagem(comandos, "comando", "comandos"));
    }
    partes.join(" · ")
}

/// O estado do chat em poucas palavras, para o histórico e a dica da aba: "Permissão · Bash · npm
/// test", "Plano para aprovar", "Rodando", "Parada · Vitral fechou", "Erro · …", "Terminou" (com
/// resposta que você não viu). Vazio quando não há nada a dizer.
pub(super) fn estado_do_chat(motor: &Motor, chat: &Chat) -> String {
    match motor.execucao(&chat.id) {
        Execucao::EsperandoVoce => match motor.permissoes.iter().find(|p| p.chat == chat.id) {
            Some(p) if p.ferramenta == "ExitPlanMode" => "Plano para aprovar".into(),
            Some(p) => format!("Permissão · {}", crate::claude::resumo_ferramenta(&p.ferramenta, &p.entrada)),
            None => "Esperando você".into(),
        },
        Execucao::Rodando { .. } => "Rodando".into(),
        Execucao::Erro(texto) => format!("Erro · {}", primeira_frase(&texto)),
        _ => match motor.parada(&chat.id) {
            Some(motivo) => format!("Parada · {}", super::componentes::motivo_curto(&motivo)),
            None if motor.nao_lido(chat) => "Terminou".into(),
            None => String::new(),
        },
    }
}

/// O estado e o começo da última fala, numa linha: "Terminou · Pronto, mandei as duas…". O pedido
/// de permissão já diz o que pede: a fala não vai junto.
pub(super) fn estado_e_fala(motor: &Motor, chat: &Chat) -> String {
    let estado = estado_do_chat(motor, chat);
    let fala = ultima_fala(chat).unwrap_or_default();
    match (estado.is_empty(), fala.is_empty()) {
        (true, _) => fala,
        (false, true) => estado,
        (false, false) if estado.starts_with("Permissão") || estado.starts_with("Erro") => estado,
        (false, false) => format!("{estado} · {fala}"),
    }
}

#[cfg(test)]
mod testes {
    use super::{o_que_o_turno_fez, primeira_frase, ultima_fala};
    use crate::modelo::{Chat, Item};

    #[test]
    fn primeira_frase_sem_markdown_e_cortada_na_frase() {
        assert_eq!(primeira_frase("**Pronto**, mandei as duas. Depois conferi o resto."), "Pronto, mandei as duas.");
        assert_eq!(primeira_frase("## Resumo\n\nFeito"), "Resumo");
        assert_eq!(primeira_frase("```rust\nfn a() {}\n```\nMudei o `motor.rs`! E mais."), "Mudei o motor.rs!");
        assert_eq!(primeira_frase("Veja [o relatório](arquivo/x.md) agora"), "Veja o relatório agora");
        assert_eq!(primeira_frase("- item da lista"), "item da lista");
        // Ponto sem espaço depois (versão, arquivo) não corta.
        assert_eq!(primeira_frase("Atualizei para a 0.8.0 e o motor.rs"), "Atualizei para a 0.8.0 e o motor.rs");
        let longa = "palavra ".repeat(40);
        assert!(primeira_frase(&longa).chars().count() <= 140);
        assert_eq!(primeira_frase("   \n  "), "");
    }

    #[test]
    fn ultima_fala_e_o_que_o_turno_fez() {
        let mut chat = Chat::novo("c".into(), "t", std::path::PathBuf::from("/tmp"));
        assert_eq!(ultima_fala(&chat), None);
        chat.itens.push(Item::Usuario { texto: "faz o deploy".into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0, marca: None });
        assert_eq!(ultima_fala(&chat).as_deref(), Some("faz o deploy"));
        let ferramenta = |nome: &str, arquivo: Option<&str>| Item::Ferramenta {
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
            durou: 0,
        };
        chat.itens.push(ferramenta("Edit", Some("a.rs")));
        chat.itens.push(ferramenta("Edit", Some("a.rs")));
        chat.itens.push(ferramenta("Write", Some("b.md")));
        chat.itens.push(ferramenta("Bash", None));
        chat.itens.push(Item::Assistente { texto: "Pronto. Subi as duas.".into(), em: 0 });
        assert_eq!(ultima_fala(&chat).as_deref(), Some("Pronto."));
        assert_eq!(o_que_o_turno_fez(&chat.itens), "2 editados · 1 comando");
        // Um turno que só respondeu não diz nada.
        chat.itens.push(Item::Usuario { texto: "e aí?".into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0, marca: None });
        chat.itens.push(Item::Assistente { texto: "Tudo certo.".into(), em: 0 });
        assert_eq!(o_que_o_turno_fez(&chat.itens), "");
    }
}
