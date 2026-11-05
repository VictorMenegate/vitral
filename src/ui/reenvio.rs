//! Corrigir e mandar de novo a última mensagem: o "Editar" do balão e a seta para cima na caixa
//! vazia trazem o texto (com os anexos e o reforço) de volta para a caixa, e a conversa volta ao
//! ponto antes dela. O que o turno já fez nos arquivos não volta sozinho: o aviso de depois
//! oferece o mesmo "Voltar os arquivos" dos balões, quando há um ponto para voltar.

use gpui_kit::component::{WindowExt as _, button::Button};
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        Confirmacao, TamanhoBotao, Tom, TomConfirmacao, aviso, aviso_com_acao, botao_icone, confirmar, contagem,
    },
};
use crate::{
    modelo::{Chat, Item, Papel},
    motor::{self, Motor},
    tema::Lucide,
};

/// A última mensagem do usuário pode ser corrigida nesta conversa: o chat aceita mensagem (o do
/// Vitral antigo é só para leitura) e nada de outro chat entrou depois dela.
pub(super) fn indice_editavel(motor: &Motor, chat: &Chat) -> Option<usize> {
    (chat.papel != Papel::Coordenador && motor.somente_leitura().is_none())
        .then(|| motor::indice_editavel(&chat.itens))
        .flatten()
}

/// O que sai da conversa junto com a mensagem do índice `ix`: a resposta, as ferramentas e os
/// avisos daquele turno.
pub(super) fn blocos_depois(itens: &[Item], ix: usize) -> usize {
    itens.len().saturating_sub(ix + 1)
}

/// A pergunta antes de corrigir, montada com o que de fato vai acontecer nesta conversa.
pub(super) fn pergunta(depois: usize, rodando: bool, tem_issues: bool, tem_sessao: bool) -> Confirmacao {
    let mut detalhes = Vec::new();
    if rodando {
        detalhes.push((Lucide::Square.into(), "O turno que está rodando para agora".into(), true));
    }
    if depois > 0 {
        detalhes.push((
            Lucide::Eraser.into(),
            format!("{} saem da conversa", contagem(depois, "bloco da resposta", "blocos da resposta")).into(),
        false));
    }
    detalhes.push((
        Lucide::FolderOpen.into(),
        "Os arquivos da pasta não voltam sozinhos (o aviso oferece isso depois)".into(),
        false,
    ));
    if tem_sessao {
        detalhes.push((
            Lucide::RefreshCw.into(),
            "O Claude Code continua numa sessão nova, com um resumo do que ficou".into(),
            false,
        ));
    }
    if tem_issues {
        detalhes.push((Lucide::Paperclip.into(), "As issues anexadas não voltam para a caixa".into(), false));
    }
    Confirmacao {
        tom: TomConfirmacao::Atencao,
        icone: Lucide::Pencil.into(),
        titulo: "Corrigir a última mensagem?".into(),
        texto: "O texto volta para a caixa, com os anexos e o reforço, e a conversa volta ao ponto antes \
                dela. Ao enviar, o turno roda de novo."
            .into(),
        detalhes,
        ok: "Corrigir".into(),
        cancelar: "Deixar como está".into(),
        ok_perigo: false,
    }
}

/// O "Editar" do balão da última mensagem: pergunta e, com o turno rodando, para ele primeiro.
pub(super) fn botao_editar(chave: &str, painel: u64, window: &Window, cx: &Context<Janela>) -> Button {
    let janela = cx.entity().downgrade();
    botao_icone(
        SharedString::from(format!("editar-{chave}")),
        Lucide::Pencil,
        "Editar: o texto volta para a caixa e a conversa volta a antes dela",
        TamanhoBotao::Linha,
        window,
        cx,
    )
    .on_click(move |_, window, cx| {
        cx.stop_propagation();
        let _ = janela.update(cx, |this, cx| this.pedir_para_editar(painel, true, window, cx));
    })
}

impl Janela {
    /// A seta para cima na caixa vazia: igual ao "Editar", mas sem parar um turno em andamento —
    /// uma tecla sem querer não corta o trabalho do Claude.
    pub(super) fn editar_ultima(&mut self, painel: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.pedir_para_editar(painel, false, window, cx);
    }

    /// Pergunta antes de corrigir. Com `parar`, um turno em andamento é interrompido e a correção
    /// espera ele sair; sem ele, a conversa rodando só avisa como parar.
    pub(super) fn pedir_para_editar(&mut self, painel: u64, parar: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let Some(chat) = self.paineis.iter().find(|p| p.id == painel).and_then(|p| p.chat.clone()) else {
            return;
        };
        let (ix, rodando, tem_sessao) = {
            let m = motor.read(cx);
            let Some(c) = m.chat(&chat) else {
                return;
            };
            (indice_editavel(m, c), m.execucao(&chat).rodando(), c.sessao_criada)
        };
        let Some(ix) = ix else {
            return;
        };
        if rodando && !parar {
            window.push_notification(
                aviso(Tom::Info, "A conversa está rodando: pare (Esc) para corrigir a última mensagem."),
                cx,
            );
            return;
        }
        let (depois, tem_issues) = {
            let m = motor.read(cx);
            let Some(c) = m.chat(&chat) else {
                return;
            };
            let issues = matches!(c.itens.get(ix), Some(Item::Usuario { referencias, .. }) if !referencias.is_empty());
            (blocos_depois(&c.itens, ix), issues)
        };
        // Nada a perder (a mensagem é o último bloco e nada roda): vai direto para a caixa.
        if depois == 0 && !rodando {
            self.aplicar_correcao(painel, chat, window, cx);
            return;
        }
        let janela = cx.entity().downgrade();
        let (motor_e, id) = (motor, chat);
        confirmar(
            pergunta(depois, rodando, tem_issues, tem_sessao),
            move |window, cx| {
                let (janela, motor_e, id) = (janela.clone(), motor_e.clone(), id.clone());
                if rodando {
                    motor_e.update(cx, |m, cx| m.parar(&id, cx));
                    esperar_e_corrigir(janela, motor_e, id, painel, window, cx);
                } else {
                    let _ = janela.update(cx, |this, cx| this.aplicar_correcao(painel, id.clone(), window, cx));
                }
            },
            None,
            window,
            cx,
        );
    }

    /// Tira a mensagem da conversa e a põe de volta na caixa, com os anexos e o reforço. Quando há
    /// um ponto de volta, o aviso oferece desfazer o que o turno fez nos arquivos.
    fn aplicar_correcao(&mut self, painel: u64, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let Some(retirada) = motor.update(cx, |m, cx| m.retirar_ultima_mensagem(&chat, cx)) else {
            window.push_notification(aviso(Tom::Atencao, "Não deu para corrigir esta mensagem agora."), cx);
            return;
        };
        if !retirada.anexos.is_empty() {
            let lista = self.anexos.entry(chat.clone()).or_default();
            for caminho in retirada.anexos {
                if !lista.contains(&caminho) {
                    lista.push(caminho);
                }
            }
        }
        if retirada.reforco.ultrathink {
            self.ultrathink.insert(chat.clone());
        }
        if retirada.reforco.ultracode {
            self.ultracode.insert(chat.clone());
        }
        self.escrever_na_caixa(painel, &retirada.texto, window, cx);
        let aviso_final = match retirada.ponto {
            Some(ponto) => {
                let (janela, chat_voltar) = (cx.entity().downgrade(), chat.clone());
                aviso_com_acao(
                    Tom::Info,
                    "A conversa voltou ao ponto antes da mensagem. Os arquivos da pasta ficaram como estão.",
                    "Voltar os arquivos",
                    move |window, cx| {
                        let (chat, ponto) = (chat_voltar.clone(), ponto.clone());
                        let _ = janela.update(cx, |this, cx| this.voltar_ao_ponto(chat, ponto, window, cx));
                    },
                )
            }
            None => aviso(Tom::Info, "A conversa voltou ao ponto antes da mensagem."),
        };
        window.push_notification(aviso_final, cx);
        cx.notify();
    }
}

/// Depois do "parar": espera o turno sair (ele cai em menos de um segundo) e então corrige. Em 6 s
/// sem sair, avisa em vez de mexer na conversa com o Claude ainda escrevendo nela.
fn esperar_e_corrigir(
    janela: WeakEntity<Janela>,
    motor: Entity<Motor>,
    chat: String,
    painel: u64,
    window: &mut Window,
    cx: &mut App,
) {
    window
        .spawn(cx, async move |cx| {
            for _ in 0..40 {
                let parou = cx
                    .update(|_, cx| !motor.read(cx).execucao(&chat).rodando())
                    .unwrap_or(true);
                if parou {
                    let _ = cx.update(|window, cx| {
                        let _ = janela.update(cx, |this, cx| this.aplicar_correcao(painel, chat.clone(), window, cx));
                    });
                    return;
                }
                cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
            }
            let _ = cx.update(|window, cx| {
                window.push_notification(aviso(Tom::Atencao, "O turno não parou; tente corrigir de novo."), cx);
            });
        })
        .detach();
}

#[cfg(test)]
mod testes {
    use super::{blocos_depois, pergunta};
    use crate::modelo::Item;
    use crate::motor::indice_editavel;

    fn usuario(texto: &str) -> Item {
        Item::Usuario {
            texto: texto.into(),
            anexos: Vec::new(),
            ponto: None,
            ultrathink: false,
            ultracode: false,
            referencias: Vec::new(),
        }
    }

    #[test]
    fn a_ultima_mensagem_e_a_editavel_e_o_que_veio_dos_outros_a_tranca() {
        assert_eq!(indice_editavel(&[]), None);
        let so_aviso = [Item::Aviso { texto: "oi".into(), erro: false }];
        assert_eq!(indice_editavel(&so_aviso), None);
        let turno = [
            usuario("primeira"),
            Item::Assistente { texto: "resposta".into() },
            usuario("segunda"),
            Item::Ferramenta { id: "1".into(), nome: "Read".into(), resumo: "x".into(), resultado: None, erro: false },
            Item::Assistente { texto: "pronto".into() },
        ];
        assert_eq!(indice_editavel(&turno), Some(2));
        assert_eq!(blocos_depois(&turno, 2), 2);
        // Uma entrega de outro chat depois dela: tirar a mensagem levaria o relatório embora.
        let com_entrega = [
            usuario("primeira"),
            Item::Entrega { de: "ajudante".into(), texto: "relatório".into(), ponto: None },
        ];
        assert_eq!(indice_editavel(&com_entrega), None);
        let com_cartao = [usuario("primeira"), Item::Cartao { id: 3 }];
        assert_eq!(indice_editavel(&com_cartao), None);
        // Mensagem sozinha no fim: nada sai junto com ela.
        let sozinha = [Item::Assistente { texto: "antes".into() }, usuario("última")];
        assert_eq!(indice_editavel(&sozinha), Some(1));
        assert_eq!(blocos_depois(&sozinha, 1), 0);
    }

    #[test]
    fn a_pergunta_so_fala_do_que_vai_acontecer() {
        let nada = pergunta(0, false, false, false);
        let textos = |c: &crate::ui::componentes::Confirmacao| {
            c.detalhes.iter().map(|(_, t, _)| t.to_string()).collect::<Vec<_>>()
        };
        assert_eq!(textos(&nada).len(), 1, "sobrou detalhe: {:?}", textos(&nada));
        assert!(textos(&nada)[0].contains("não voltam sozinhos"));
        let tudo = pergunta(3, true, true, true);
        let d = textos(&tudo);
        assert_eq!(d.len(), 5, "{d:?}");
        assert!(d[0].contains("para agora"));
        assert!(d[1].contains("3 blocos da resposta"), "{:?}", d[1]);
        assert!(d[3].contains("sessão nova"));
        assert!(d[4].contains("issues"));
        // O Enter confirma: o texto volta para a caixa, nada se perde de vez.
        assert!(!tudo.ok_perigo);
        let uma = pergunta(1, false, false, false);
        assert!(textos(&uma)[0].contains("1 bloco da resposta"), "{:?}", textos(&uma));
    }
}
