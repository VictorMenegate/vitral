//! "Continuar daqui em outro chat": o botão nas ações de uma mensagem sua e de uma resposta. Cria
//! um chat novo na mesma pasta, com a conversa até aquele ponto (`motor/bifurcar.rs`), e o abre
//! num painel ao lado; o original fica como está.

use gpui_kit::component::{WindowExt as _, button::Button};
use gpui_kit::*;

use super::{
    Janela,
    componentes::{TamanhoBotao, Tom, aviso, botao_icone},
    paineis::Lado,
};
use crate::{modelo::Item, tema::Lucide};

/// O botão das ações da mensagem do índice `ix` da conversa do painel.
pub(super) fn botao(chave: &str, painel: u64, ix: usize, window: &Window, cx: &Context<Janela>) -> Button {
    let janela = cx.entity().downgrade();
    botao_icone(
        SharedString::from(format!("bifurcar-{chave}")),
        Lucide::GitFork,
        tr!("Continuar daqui em outro chat\nAbre ao lado um chat novo com a conversa até este ponto; este fica como está"),
        TamanhoBotao::Linha,
        window,
        cx,
    )
    .on_click(move |_, window, cx| {
        cx.stop_propagation();
        let _ = janela.update(cx, |this, cx| this.continuar_em_outro_chat(painel, ix, window, cx));
    })
}

/// A última resposta da conversa: de onde a paleta de comandos bifurca.
pub(super) fn ultima_resposta(itens: &[Item]) -> Option<usize> {
    itens.iter().rposition(|i| matches!(i, Item::Assistente { .. }))
}

impl Janela {
    /// Bifurca a conversa do painel no item `ix` e abre o chat novo ao lado (sem lugar para outro
    /// painel, numa aba do mesmo). A mensagem sua do ponto vai para a caixa do chat novo.
    pub(super) fn continuar_em_outro_chat(&mut self, painel: u64, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(motor), Some(chat)) = (self.motor.clone(), self.paineis.iter().find(|p| p.id == painel).and_then(|p| p.chat.clone())) else {
            return;
        };
        let novo = match motor.update(cx, |m, cx| m.bifurcar_chat(&chat, ix, cx)) {
            Ok(novo) => novo,
            Err(motivo) => {
                let mut frase = motivo;
                if let Some(primeira) = frase.get_mut(..1) {
                    primeira.make_ascii_uppercase();
                }
                window.push_notification(aviso(Tom::Atencao, format!("{frase}.")), cx);
                return;
            }
        };
        let ao_lado = self.cabe_outro_painel();
        if ao_lado {
            self.abrir_ao_lado(novo.chat.clone(), Some((painel, Lado::Direita)), window, cx);
        } else {
            self.selecionar(Some(novo.chat.clone()), window, cx);
        }
        // O painel em que o chat novo ficou: a sua mensagem do ponto volta para a caixa dele.
        if let Some(onde) = self.paineis.iter().find(|p| p.chat.as_deref() == Some(novo.chat.as_str())).map(|p| p.id) {
            if let Some((texto, anexos)) = novo.mensagem {
                if !anexos.is_empty() {
                    self.anexos.entry(novo.chat.clone()).or_default().extend(anexos);
                }
                self.escrever_na_caixa(onde, &texto, window, cx);
            } else if let Some(p) = self.paineis.iter().find(|p| p.id == onde) {
                let foco = p.composer.read(cx).focus_handle(cx);
                window.focus(&foco, cx);
            }
        }
        let como = if novo.com_sessao {
            tr!("o agente continua com o que viu até este ponto")
        } else {
            tr!("o agente recebe as últimas mensagens como resumo")
        };
        let onde = if ao_lado { tr!("ao lado") } else { tr!("numa aba (não cabia outro painel)") };
        window.push_notification(aviso(Tom::Info, tr!("Chat novo aberto {onde}: {como}.", onde = onde, como = como)), cx);
        cx.notify();
    }
}

#[cfg(test)]
mod testes {
    use super::ultima_resposta;
    use crate::modelo::Item;

    #[test]
    fn a_paleta_bifurca_na_ultima_resposta() {
        let resposta = |t: &str| Item::Assistente { texto: t.into(), em: 0 };
        let aviso = Item::Aviso { texto: "Interrompido.".into(), erro: false, em: 0 };
        assert_eq!(ultima_resposta(&[]), None);
        assert_eq!(ultima_resposta(&[aviso.clone()]), None);
        assert_eq!(ultima_resposta(&[resposta("a"), resposta("b"), aviso]), Some(1));
    }
}
