//! O que um hook `PreToolUse` do usuário mandou perguntar (`permissionDecision: "ask"`).
//!
//! Medido no Claude Code 2.1.328 (23/11/2026): com o "ask" de um hook, ele chama a ferramenta do
//! `--permission-prompt-tool` em todos os modos, até em `bypassPermissions`, mas só com
//! `tool_name`, `input` e `tool_use_id`: o motivo que o hook deu não vai junto. Ele só aparece no
//! evento `hook_response` da saída (`--include-hook-events`), que não traz o id da chamada, só o
//! nome da ferramenta, e sai logo antes do pedido. O motor guarda aqui o motivo quando o evento
//! chega (`Evento::HookPediu`) e o tira quando chega o pedido da mesma ferramenta, no mesmo chat:
//! o cartão mostra o motivo, a regra de "Não perguntar de novo" não vale para ele e a caixa de
//! marcar some (quem pôs o hook quer ser perguntado toda vez).
//!
//! Sem o id, o par é pela ordem: duas chamadas da mesma ferramenta ao mesmo tempo, com motivos
//! diferentes, podem sair com o motivo trocado. O motivo que ninguém veio buscar (outro hook
//! negou a chamada, o turno parou) vence em `VALE_POR` e some quando o processo do chat sai.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use serde_json::Value;

use crate::modelo::Chat;

/// Por quanto tempo um motivo espera o pedido dele. Entre o hook que mandou perguntar e o pedido
/// só correm os outros hooks da mesma chamada.
pub const VALE_POR: Duration = Duration::from_secs(60);

struct Motivo {
    ferramenta: String,
    texto: String,
    quando: Instant,
}

/// Os motivos que ainda esperam o pedido de permissão, por chat, na ordem em que chegaram.
#[derive(Default)]
pub struct MotivosDeHook(HashMap<String, Vec<Motivo>>);

impl MotivosDeHook {
    /// Um hook mandou perguntar antes de uma chamada dessa ferramenta (o motivo pode vir vazio).
    pub fn guardar(&mut self, chat: &str, ferramenta: &str, motivo: &str, agora: Instant) {
        self.0.entry(chat.to_string()).or_default().push(Motivo {
            ferramenta: ferramenta.to_string(),
            texto: motivo.trim().to_string(),
            quando: agora,
        });
    }

    /// O motivo do pedido que acabou de chegar: o mais antigo ainda válido dessa ferramenta nesse
    /// chat, que sai da espera. `None` quando nenhum hook mandou perguntar.
    pub fn tirar(&mut self, chat: &str, ferramenta: &str, agora: Instant) -> Option<String> {
        let fila = self.0.get_mut(chat)?;
        fila.retain(|m| agora.saturating_duration_since(m.quando) < VALE_POR);
        let posicao = fila.iter().position(|m| m.ferramenta == ferramenta);
        let motivo = posicao.map(|i| fila.remove(i).texto);
        if fila.is_empty() {
            self.0.remove(chat);
        }
        motivo
    }

    /// O processo do chat saiu (ou outro vai começar): o que esperava não vale mais.
    pub fn esquecer(&mut self, chat: &str) {
        self.0.remove(chat);
    }
}

/// O pedido passa sem perguntar: só o que uma regra de "Não perguntar de novo" da conversa cobre,
/// e nunca o que um hook mandou perguntar.
pub fn passa_sem_perguntar(chat: &Chat, ferramenta: &str, entrada: &Value, do_hook: bool) -> bool {
    !do_hook && chat.permite(ferramenta, entrada)
}

#[cfg(test)]
mod testes {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    #[test]
    fn o_motivo_espera_o_pedido_da_mesma_ferramenta_no_mesmo_chat() {
        let agora = Instant::now();
        let mut motivos = MotivosDeHook::default();
        assert_eq!(motivos.tirar("c1", "Bash", agora), None);
        motivos.guardar("c1", "Bash", "primeiro", agora);
        motivos.guardar("c1", "Bash", "segundo", agora);
        motivos.guardar("c1", "Write", "  arquivo de produção \n", agora);
        motivos.guardar("c2", "Bash", "do outro chat", agora);
        // Outra ferramenta e outro chat não levam o motivo de ninguém.
        assert_eq!(motivos.tirar("c1", "Edit", agora), None);
        assert_eq!(motivos.tirar("c3", "Bash", agora), None);
        // Na ordem em que os hooks pediram, e cada motivo vale para um pedido só.
        assert_eq!(motivos.tirar("c1", "Bash", agora).as_deref(), Some("primeiro"));
        assert_eq!(motivos.tirar("c1", "Bash", agora).as_deref(), Some("segundo"));
        assert_eq!(motivos.tirar("c1", "Bash", agora), None);
        assert_eq!(motivos.tirar("c2", "Bash", agora).as_deref(), Some("do outro chat"));
        assert_eq!(motivos.tirar("c1", "Write", agora).as_deref(), Some("arquivo de produção"));
    }

    #[test]
    fn hook_sem_motivo_ainda_marca_o_pedido() {
        let agora = Instant::now();
        let mut motivos = MotivosDeHook::default();
        motivos.guardar("c1", "Bash", "", agora);
        assert_eq!(motivos.tirar("c1", "Bash", agora).as_deref(), Some(""));
    }

    #[test]
    fn motivo_que_ninguem_buscou_vence_e_some_com_o_processo() {
        let agora = Instant::now();
        let mut motivos = MotivosDeHook::default();
        motivos.guardar("c1", "Bash", "velho", agora);
        let quase = agora + VALE_POR - Duration::from_secs(1);
        motivos.guardar("c1", "Bash", "novo", quase);
        // O velho venceu: o pedido de agora leva o novo.
        assert_eq!(motivos.tirar("c1", "Bash", agora + VALE_POR).as_deref(), Some("novo"));
        motivos.guardar("c1", "Bash", "sozinho", agora);
        assert_eq!(motivos.tirar("c1", "Bash", agora + VALE_POR), None);
        // O processo saiu: nada fica para o turno seguinte.
        motivos.guardar("c1", "Bash", "do turno que acabou", agora);
        motivos.guardar("c2", "Bash", "do outro", agora);
        motivos.esquecer("c1");
        assert_eq!(motivos.tirar("c1", "Bash", agora), None);
        assert_eq!(motivos.tirar("c2", "Bash", agora).as_deref(), Some("do outro"));
    }

    /// A regra de "Não perguntar de novo" cobre o pedido comum, e nunca o que um hook mandou
    /// perguntar: quem pôs o hook quer ver o cartão toda vez.
    #[test]
    fn o_que_o_hook_mandou_perguntar_nunca_passa_pela_regra() {
        let mut chat = Chat::novo("c1".into(), "t", PathBuf::from("/p/loja"));
        chat.permitidas = vec!["bash:easypanel deploy central-worker".into(), "ferramenta:mcp__x__y".into()];
        let deploy = json!({ "command": "easypanel deploy central-worker" });
        assert!(passa_sem_perguntar(&chat, "Bash", &deploy, false));
        assert!(!passa_sem_perguntar(&chat, "Bash", &deploy, true));
        assert!(passa_sem_perguntar(&chat, "mcp__x__y", &json!({}), false));
        assert!(!passa_sem_perguntar(&chat, "mcp__x__y", &json!({}), true));
        // Sem regra, pergunta de qualquer jeito.
        assert!(!passa_sem_perguntar(&chat, "Bash", &json!({ "command": "ls" }), false));
    }
}
