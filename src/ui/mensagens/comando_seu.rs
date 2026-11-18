//! O comando que você roda com "!" na caixa de mensagem (`motor/comando_seu.rs`), na conversa: a
//! linha "Comando seu", a caixa de saída da trilha com o comando no cabeçalho e o Copiar, o Parar
//! enquanto roda e o "Mandar ao agente", que cita a saída na próxima mensagem. O comando com trecho
//! arriscado aparece antes de rodar, com o botão de segurar dos pedidos de permissão.

use gpui_kit::component::WindowExt as _;

use super::*;
use crate::motor::{COMANDO_COMECOU as COMECOU, SituacaoDoComando, citacao_do_comando};

/// Quanto da saída o "Mandar ao agente" cita (o fim dela): a citação da caixa corta em 4000.
const MAX_CITADO: usize = 3_600;

/// "!comando" na caixa: o comando, sem o sinal. `None` para o que é mensagem (o "!" sozinho também).
pub(in crate::ui) fn comando_da_caixa(texto: &str) -> Option<&str> {
    let resto = texto.trim().strip_prefix('!')?.trim();
    (!resto.is_empty()).then_some(resto)
}

/// O que o cabeçalho da caixa diz do comando e se ele conta como erro.
fn como_ficou(situacao: SituacaoDoComando, codigo: Option<i32>, milis: u64) -> (String, bool) {
    match (situacao, codigo) {
        (SituacaoDoComando::EsperandoVoce, _) => ("Esperando você".into(), false),
        (SituacaoDoComando::Rodando, _) => ("Rodando (Esc para)".into(), false),
        (SituacaoDoComando::Fim, Some(0)) => (format!("Pronto em {}", duracao_curta(milis)), false),
        (SituacaoDoComando::Fim, Some(n)) => (format!("Saiu com {n} em {}", duracao_curta(milis)), true),
        (SituacaoDoComando::Fim, None) if milis == 0 => ("Não rodou".into(), false),
        // O Vitral fechou com ele rodando: não há duração a dizer.
        (SituacaoDoComando::Fim, None) if milis == COMECOU => ("Interrompido".into(), true),
        (SituacaoDoComando::Fim, None) => (format!("Interrompido em {}", duracao_curta(milis)), true),
    }
}

impl Janela {
    /// O Enviar com "!comando" na caixa: roda o comando na pasta do chat em vez de mandar a mensagem.
    /// Devolve `true` quando o texto era um comando (mesmo recusado: aí ele fica na caixa).
    pub(in crate::ui) fn rodar_comando_da_caixa(&mut self, chat: &str, texto: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (Some(comando), Some(motor)) = (comando_da_caixa(texto), self.motor.clone()) else {
            return false;
        };
        let arriscado = !trechos_perigosos(comando).is_empty();
        match motor.update(cx, |m, cx| m.pedir_comando_seu(chat, comando, arriscado, cx)) {
            Ok(()) => {
                if let Some(painel) = self.paineis.get_mut(self.ativo) {
                    painel.composer.update(cx, |c, cx| c.set_value("", window, cx));
                    painel.seguindo = true;
                    painel.rolagem.scroll_to_bottom();
                }
                // A caixa esvaziada vai para o disco na hora, como no envio de uma mensagem.
                self.gravar_agora(cx);
            }
            Err(erro) => {
                let mut frase = erro;
                if let Some(primeira) = frase.get_mut(..1) {
                    primeira.make_ascii_uppercase();
                }
                window.push_notification(crate::ui::componentes::aviso(crate::ui::componentes::Tom::Atencao, format!("{frase}.")), cx);
            }
        }
        cx.notify();
        true
    }

    /// Um comando seu na conversa.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn cartao_do_comando(
        &self,
        contexto: &Contexto,
        chave: &str,
        id: &str,
        comando: &str,
        saida: &str,
        codigo: Option<i32>,
        milis: u64,
        em: u64,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let chat = contexto.chat.id.clone();
        let situacao = contexto.motor.situacao_do_comando(&chat, id);
        let (estado, erro) = como_ficou(situacao, codigo, milis);
        let cabecalho = h_flex()
            .h(r(24.))
            .gap(r(8.))
            .child(coluna_do_icone().child(icone_de(Lucide::Terminal, 14.).text_color(Cores::tenue())))
            .child(div().flex_none().metadado().font_semibold().text_color(Cores::texto()).child("Comando seu"))
            .child(div().min_w_0().truncate().metadado().text_color(Cores::tenue()).child("fora da sessão do agente"))
            .child(div().flex_1())
            .children(hora_na_barra(em));
        let motor = self.motor.clone();
        let acao = |rotulo: &'static str, icone: Lucide, f: Box<dyn Fn(&mut Motor, &str, &mut gpui_kit::Context<Motor>)>| {
            let (motor, chat) = (motor.clone(), chat.clone());
            fantasma(
                SharedString::from(format!("{rotulo}-{chave}")),
                Some(icone),
                rotulo,
                TamanhoBotao::Linha,
                move |_, _, cx| {
                    if let Some(motor) = &motor {
                        motor.update(cx, |m, cx| f(m, &chat, cx));
                    }
                },
                window,
                cx,
            )
        };
        let corpo = match situacao {
            SituacaoDoComando::EsperandoVoce => {
                let (motor_segurar, chat_segurar) = (motor.clone(), chat.clone());
                let segurar = BotaoDeSegurar::new(SharedString::from(format!("segurar-{chave}")), "Segure para rodar")
                    .tamanho(TamanhoBotao::Linha)
                    .ao_completar(move |cx| {
                        if let Some(motor) = &motor_segurar {
                            motor.update(cx, |m, cx| m.confirmar_comando_seu(&chat_segurar, cx));
                        }
                    });
                v_flex()
                    .gap(esp::E8)
                    .child(corpo_permissao(&format!("comando-{chave}"), "Bash", &serde_json::json!({ "command": comando })))
                    .child(
                        h_flex()
                            .gap(esp::E8)
                            .justify_end()
                            .child(acao("Descartar", Lucide::X, Box::new(|m, chat, cx| m.parar_comando_seu(chat, cx))))
                            .child(segurar),
                    )
                    .into_any_element()
            }
            _ => {
                let rodando = situacao == SituacaoDoComando::Rodando;
                let vazio = if rodando { "(ainda sem saída)" } else { "(sem saída)" };
                let caixa = CaixaDeCodigo::new(SharedString::from(format!("comando-{chave}")), SharedString::from(if saida.is_empty() { vazio.to_string() } else { saida.to_string() }))
                    .comando(comando.to_string())
                    .tempo(estado)
                    .erro(erro)
                    .raio_chat();
                let botao = if rodando {
                    acao("Parar", Lucide::CircleX, Box::new(|m, chat, cx| m.parar_comando_seu(chat, cx))).into_any_element()
                } else {
                    let (painel, citado) = (contexto.painel, citacao_do_comando(comando, saida, codigo, MAX_CITADO));
                    fantasma(
                        SharedString::from(format!("mandar-{chave}")),
                        Some(Lucide::Send),
                        "Mandar ao agente",
                        TamanhoBotao::Linha,
                        cx.listener(move |this: &mut Janela, _: &ClickEvent, window, cx| this.citar_na_caixa(painel, &citado, window, cx)),
                        window,
                        cx,
                    )
                    .tooltip("Cita o comando e a saída na sua próxima mensagem")
                    .into_any_element()
                };
                v_flex().gap(esp::E4).child(caixa).child(h_flex().justify_end().child(botao)).into_any_element()
            }
        };
        v_flex().w_full().gap(esp::E4).child(cabecalho).child(div().pl(RECUO_DO_LOG).child(corpo)).into_any_element()
    }

    /// Põe o trecho como a citação da próxima mensagem do chat do painel (o cartão em cima do
    /// campo, o mesmo do "Responder a este trecho") e leva o foco à caixa.
    fn citar_na_caixa(&mut self, painel: u64, trecho: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.paineis.iter().find(|p| p.id == painel) else {
            return;
        };
        let (Some(chat), foco) = (p.chat.clone(), p.composer.read(cx).focus_handle(cx)) else {
            return;
        };
        self.citacoes.insert(chat, trecho.to_string());
        window.focus(&foco, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod testes {
    use super::{COMECOU, SituacaoDoComando, comando_da_caixa, como_ficou};

    #[test]
    fn so_o_ponto_de_exclamacao_com_algo_depois_e_comando() {
        assert_eq!(comando_da_caixa("!git status"), Some("git status"));
        assert_eq!(comando_da_caixa("  ! cargo test -q \n"), Some("cargo test -q"));
        assert_eq!(comando_da_caixa("!"), None);
        assert_eq!(comando_da_caixa("! "), None);
        assert_eq!(comando_da_caixa("veja isto!"), None);
        assert_eq!(comando_da_caixa("/compact"), None);
    }

    #[test]
    fn o_cabecalho_diz_como_o_comando_ficou() {
        use SituacaoDoComando::*;
        assert_eq!(como_ficou(Fim, Some(0), 1200), ("Pronto em 1,2 s".to_string(), false));
        assert_eq!(como_ficou(Fim, Some(2), 300), ("Saiu com 2 em 0,3 s".to_string(), true));
        assert_eq!(como_ficou(Fim, None, 5000), ("Interrompido em 5,0 s".to_string(), true));
        // Fechado o Vitral antes de você segurar o botão, o comando arriscado nunca rodou.
        assert_eq!(como_ficou(Fim, None, 0), ("Não rodou".to_string(), false));
        assert_eq!(como_ficou(Fim, None, COMECOU), ("Interrompido".to_string(), true));
        assert!(!como_ficou(Rodando, None, 0).1);
        assert!(!como_ficou(EsperandoVoce, None, 0).1);
    }
}
