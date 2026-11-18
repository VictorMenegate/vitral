//! O ramo do git de cada chat: o motor anota em que ramo a pasta estava quando o turno começou e
//! quando terminou (`Chat::ramo`), para a conversa avisar se a pasta trocou de ramo por fora
//! (`ui/ramo.rs`). A leitura é a dos arquivos do git (`git::ramo_no_disco`), sem abrir processo.

use super::*;

impl Motor {
    /// Anota no chat o ramo em que a pasta dele está agora (nada, fora de um repositório).
    pub(super) fn anotar_ramo(&mut self, chat_id: &str) {
        if let Some(c) = self.chat_mut(chat_id) {
            c.ramo = crate::git::ramo_no_disco(&c.cwd).map(|r| r.nome);
        }
    }

    /// O × do aviso de troca de ramo: o chat passa a valer no ramo de agora, e o aviso não volta.
    pub fn aceitar_ramo(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        self.anotar_ramo(chat_id);
        self.salvar();
        cx.notify();
    }
}
