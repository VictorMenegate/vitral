//! Corrigir e mandar de novo a última mensagem: o "Editar" do balão e a seta para cima na caixa
//! vazia trazem o texto (com os anexos e o reforço) de volta para a caixa, e a conversa volta ao
//! ponto antes dela. O que o turno já fez nos arquivos não volta: a pergunta e o aviso dizem isso.

use gpui_kit::component::{WindowExt as _, button::Button};
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        Confirmacao, TamanhoBotao, Tom, TomConfirmacao, aviso, botao_icone, confirmar, contagem,
    },
};
use crate::{
    modelo::{Chat, Item},
    motor::{self, Motor},
    tema::Lucide,
};

/// A última mensagem do usuário que pode ser corrigida nesta conversa: o Vitral não está só para
/// leitura e nada de outro chat entrou depois dela.
pub(super) fn indice_editavel(motor: &Motor, chat: &Chat) -> Option<usize> {
    motor.somente_leitura().is_none()
        .then(|| motor::indice_editavel(&chat.itens))
        .flatten()
}

/// Desde que posição as mensagens do usuário podem ser corrigidas (`None` com o Vitral só para
/// leitura).
pub(super) fn editaveis_desde(motor: &Motor, chat: &Chat) -> Option<usize> {
    motor.somente_leitura().is_none().then(|| motor::editaveis_desde(&chat.itens))
}

/// O que sai da conversa junto com a mensagem do índice `ix`: a resposta, as ferramentas e os
/// avisos daquele turno.
pub(super) fn blocos_depois(itens: &[Item], ix: usize) -> usize {
    itens.len().saturating_sub(ix + 1)
}

/// Corrigir sem perguntar só quando não há nada a perder: a mensagem é o último bloco, nada roda
/// e a caixa está vazia. Com algo escrito na caixa, a correção troca o texto dela, e isso a pessoa
/// precisa saber antes (teste de usabilidade, 16/11/2026: o rascunho sumia sem aviso).
pub(super) fn precisa_perguntar(depois: usize, rodando: bool, caixa_com_texto: bool) -> bool {
    depois > 0 || rodando || caixa_com_texto
}

/// A pergunta antes de corrigir, montada com o que de fato vai acontecer nesta conversa.
pub(super) fn pergunta(depois: usize, rodando: bool, tem_sessao: bool, caixa_com_texto: bool) -> Confirmacao {
    let mut detalhes = Vec::new();
    if rodando {
        detalhes.push((Lucide::Square.into(), "O turno que está rodando para agora".into(), true));
    }
    if caixa_com_texto {
        detalhes.push((
            Lucide::TextCursorInput.into(),
            "O que está escrito na caixa sai dela (Ctrl+Z traz de volta)".into(),
            true,
        ));
    }
    if depois > 0 {
        let verbo = if depois == 1 { "sai" } else { "saem" };
        detalhes.push((
            Lucide::Eraser.into(),
            format!("{} {verbo} da conversa", contagem(depois, "bloco da resposta", "blocos da resposta")).into(),
        false));
    }
    // O aviso de depois não tem como voltar os arquivos: a pergunta não promete isso.
    detalhes.push((Lucide::FolderOpen.into(), "Os arquivos da pasta não voltam sozinhos".into(), false));
    if tem_sessao {
        detalhes.push((
            Lucide::RefreshCw.into(),
            "O Claude Code continua numa sessão nova, com um resumo do que ficou".into(),
            false,
        ));
    }
    Confirmacao {
        tom: TomConfirmacao::Atencao,
        icone: Lucide::Pencil.into(),
        titulo: "Corrigir esta mensagem?".into(),
        texto: "O texto volta para a caixa, com os anexos e o reforço, e a conversa volta ao ponto antes \
                dela. Ao enviar, o turno roda de novo."
            .into(),
        detalhes,
        ok: "Corrigir".into(),
        cancelar: "Deixar como está".into(),
        ok_perigo: false,
    }
}

/// O "Editar" do balão de uma mensagem sua (`ix` na conversa): pergunta e, com o turno rodando,
/// para ele primeiro.
pub(super) fn botao_editar(chave: &str, painel: u64, ix: usize, window: &Window, cx: &Context<Janela>) -> Button {
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
        let _ = janela.update(cx, |this, cx| this.pedir_para_editar(painel, Some(ix), true, window, cx));
    })
}

/// O "Voltar os arquivos" do balão de uma mensagem sua que levou marca ao Claude Code.
pub(super) fn botao_voltar_arquivos(chave: &str, chat: &str, marca: &str, window: &Window, cx: &Context<Janela>) -> Button {
    let janela = cx.entity().downgrade();
    let (chat, marca) = (chat.to_string(), marca.to_string());
    botao_icone(
        SharedString::from(format!("voltar-arquivos-{chave}")),
        Lucide::RotateCcw,
        "Voltar os arquivos a como estavam antes desta mensagem (a conversa fica)",
        TamanhoBotao::Linha,
        window,
        cx,
    )
    .on_click(move |_, window, cx| {
        cx.stop_propagation();
        let _ = janela.update(cx, |this, cx| this.pedir_volta_dos_arquivos(chat.clone(), marca.clone(), window, cx));
    })
}

/// "a.rs, b.rs e mais 3": os nomes dos arquivos, sem a pasta.
fn nomes_dos_arquivos(arquivos: &[String]) -> String {
    let nomes: Vec<String> = arquivos
        .iter()
        .map(|a| std::path::Path::new(a).file_name().map_or_else(|| a.clone(), |n| n.to_string_lossy().into_owned()))
        .collect();
    match nomes.len() {
        0..=3 => nomes.join(", "),
        n => format!("{} e mais {}", nomes[..3].join(", "), n - 3),
    }
}

impl Janela {
    /// Pergunta ao Claude Code o que voltaria (sem mexer em nada), mostra e, no ok, volta os
    /// arquivos a antes da mensagem `marca`. A conversa fica como está; um aviso nela diz o que
    /// voltou. Com o chat rodando, não: o turno ainda mexe nos arquivos.
    pub(super) fn pedir_volta_dos_arquivos(&mut self, chat: String, marca: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let Some((pasta, conta, sessao)) = motor.read(cx).onde_voltar_arquivos(&chat) else {
            window.push_notification(aviso(Tom::Info, "Pare o turno (Esc) antes de voltar os arquivos."), cx);
            return;
        };
        cx.spawn_in(window, async move |_, cx| {
            let (p, c, s, m) = (pasta.clone(), conta.clone(), sessao.clone(), marca.clone());
            let seco = cx
                .background_spawn(async move { crate::claude::voltar_arquivos(&p, c.as_deref(), &s, &m, true) })
                .await;
            let _ = cx.update(|window, cx| {
                let volta = match seco {
                    Ok(volta) if volta.arquivos.is_empty() => {
                        window.push_notification(aviso(Tom::Info, "Nada a voltar: os arquivos já estão como antes desta mensagem."), cx);
                        return;
                    }
                    Ok(volta) => volta,
                    Err(erro) => {
                        window.push_notification(aviso(Tom::Atencao, format!("Não dá para voltar os arquivos: {erro}")), cx);
                        return;
                    }
                };
                let linhas = format!("+{} −{}", volta.entram, volta.saem);
                let detalhes = vec![
                    (Lucide::FileDiff.into(), format!("{}: {}", contagem(volta.arquivos.len(), "arquivo", "arquivos"), nomes_dos_arquivos(&volta.arquivos)).into(), true),
                    (Lucide::MessagesSquare.into(), "A conversa fica como está; o agente vê os arquivos de novo no próximo turno".into(), false),
                    (Lucide::TriangleAlert.into(), "O que você mudou à mão nesses arquivos depois também volta".into(), false),
                ];
                let (motor, chat) = (motor.clone(), chat.clone());
                let texto_do_aviso = format!(
                    "Voltei {} a como estavam antes de uma mensagem sua: {}.",
                    contagem(volta.arquivos.len(), "arquivo", "arquivos"),
                    nomes_dos_arquivos(&volta.arquivos)
                );
                confirmar(
                    Confirmacao {
                        tom: TomConfirmacao::Atencao,
                        icone: Lucide::RotateCcw.into(),
                        titulo: "Voltar os arquivos?".into(),
                        texto: format!("Os arquivos que o Claude mudou desde esta mensagem voltam a como estavam ({linhas}).").into(),
                        detalhes,
                        ok: "Voltar os arquivos".into(),
                        cancelar: "Deixar como está".into(),
                        ok_perigo: true,
                    },
                    move |window, cx| {
                        let (pasta, conta, sessao, marca, motor, chat, texto_do_aviso) = (
                            pasta.clone(),
                            conta.clone(),
                            sessao.clone(),
                            marca.clone(),
                            motor.clone(),
                            chat.clone(),
                            texto_do_aviso.clone(),
                        );
                        window
                            .spawn(cx, async move |cx| {
                                let feito = cx
                                    .background_spawn(async move {
                                        crate::claude::voltar_arquivos(&pasta, conta.as_deref(), &sessao, &marca, false)
                                    })
                                    .await;
                                let _ = cx.update(|window, cx| match feito {
                                    Ok(_) => {
                                        motor.update(cx, |m, cx| m.avisar_na_conversa(&chat, &texto_do_aviso, cx));
                                        window.push_notification(aviso(Tom::Pronto, "Arquivos de volta"), cx);
                                    }
                                    Err(erro) => {
                                        window.push_notification(aviso(Tom::Atencao, format!("Não voltei os arquivos: {erro}")), cx);
                                    }
                                });
                            })
                            .detach();
                    },
                    None,
                    window,
                    cx,
                );
            });
        })
        .detach();
    }

    /// A seta para cima na caixa vazia: igual ao "Editar", mas sem parar um turno em andamento —
    /// uma tecla sem querer não corta o trabalho do Claude.
    pub(super) fn editar_ultima(&mut self, painel: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.pedir_para_editar(painel, None, false, window, cx);
    }

    /// Pergunta antes de corrigir. Com `parar`, um turno em andamento é interrompido e a correção
    /// espera ele sair; sem ele, a conversa rodando só avisa como parar.
    /// `ix`: a mensagem; `None` é a última que dá para corrigir.
    pub(super) fn pedir_para_editar(&mut self, painel: u64, ix: Option<usize>, parar: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let Some(chat) = self.paineis.iter().find(|p| p.id == painel).and_then(|p| p.chat.clone()) else {
            return;
        };
        // O que está escrito na caixa deste painel: a correção vai trocá-lo.
        let caixa_com_texto = self
            .paineis
            .iter()
            .find(|p| p.id == painel)
            .is_some_and(|p| !p.composer.read(cx).value().trim().is_empty());
        // Com o turno rodando, a caixa vazia e algo seu na fila, a seta para cima traz a última da
        // fila de volta, sem tocar no turno (rodada de uso, item 1.3).
        if !parar
            && !caixa_com_texto
            && motor.read(cx).execucao(&chat).rodando()
            && self.trazer_da_fila(painel, &chat, window, cx)
        {
            return;
        }
        let (ix, rodando, tem_sessao) = {
            let m = motor.read(cx);
            let Some(c) = m.chat(&chat) else {
                return;
            };
            let ix = match ix {
                Some(ix) => motor::editavel(&c.itens, ix).then_some(ix).filter(|_| m.somente_leitura().is_none()),
                None => indice_editavel(m, c),
            };
            (ix, m.execucao(&chat).rodando(), c.sessao_criada)
        };
        let Some(ix) = ix else {
            return;
        };
        // Quando a sessão volta ao ponto antes da mensagem, não há sessão nova com resumo a avisar.
        let tem_sessao = tem_sessao && !motor.read(cx).correcao_mantem_a_sessao(&chat, ix);
        if rodando && !parar {
            window.push_notification(
                aviso(Tom::Info, "A conversa está rodando: pare (Esc) para corrigir uma mensagem."),
                cx,
            );
            return;
        }
        let depois = {
            let m = motor.read(cx);
            let Some(c) = m.chat(&chat) else {
                return;
            };
            blocos_depois(&c.itens, ix)
        };
        // Nada a perder (a mensagem é o último bloco, nada roda e a caixa está vazia): vai direto.
        if !precisa_perguntar(depois, rodando, caixa_com_texto) {
            self.aplicar_correcao(painel, chat, ix, window, cx);
            return;
        }
        let janela = cx.entity().downgrade();
        let (motor_e, id) = (motor, chat);
        confirmar(
            pergunta(depois, rodando, tem_sessao, caixa_com_texto),
            move |window, cx| {
                let (janela, motor_e, id) = (janela.clone(), motor_e.clone(), id.clone());
                if rodando {
                    motor_e.update(cx, |m, cx| m.parar(&id, cx));
                    esperar_e_corrigir(janela, motor_e, id, painel, ix, window, cx);
                } else {
                    let _ = janela.update(cx, |this, cx| this.aplicar_correcao(painel, id.clone(), ix, window, cx));
                }
            },
            None,
            window,
            cx,
        );
    }

    /// Tira a mensagem da conversa e a põe de volta na caixa, com os anexos e o reforço. Os
    /// arquivos da pasta ficam como estão.
    fn aplicar_correcao(&mut self, painel: u64, chat: String, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let Some(retirada) = motor.update(cx, |m, cx| m.retirar_mensagem(&chat, ix, cx)) else {
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
        // A mensagem já saiu do disco da conversa: o rascunho com ela vai para o disco agora, e
        // não daqui a pouco, para uma queda logo depois não levar as duas (teste de usabilidade,
        // 16/11/2026).
        self.gravar_agora(cx);
        // Curto, para caber nas duas linhas do aviso (o mais longo saía cortado no meio).
        let aviso_final = aviso(Tom::Info, "A conversa voltou a antes da mensagem. Os arquivos não mudaram.");
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
    ix: usize,
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
                        let _ = janela.update(cx, |this, cx| this.aplicar_correcao(painel, chat.clone(), ix, window, cx));
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
    use super::{blocos_depois, pergunta, precisa_perguntar};
    use crate::modelo::Item;
    use crate::motor::{editavel, indice_editavel};

    fn usuario(texto: &str) -> Item {
        Item::Usuario {
            em: 0,
            texto: texto.into(),
            anexos: Vec::new(),
            ultrathink: false,
            ultracode: false,
            marca: None,
        }
    }

    #[test]
    fn a_ultima_mensagem_e_a_editavel_e_o_que_veio_dos_outros_a_tranca() {
        assert_eq!(indice_editavel(&[]), None);
        let so_aviso = [Item::Aviso { em: 0, texto: "oi".into(), erro: false }];
        assert_eq!(indice_editavel(&so_aviso), None);
        let turno = [
            usuario("primeira"),
            Item::Assistente { em: 0, texto: "resposta".into() },
            usuario("segunda"),
            Item::Ferramenta { em: 0, id: "1".into(), nome: "Read".into(), resumo: "x".into(), resultado: None, erro: false, detalhe: None, arquivo: None, sub: None, mudanca: None, imagens: Vec::new(), durou: 0 },
            Item::Assistente { em: 0, texto: "pronto".into() },
        ];
        assert_eq!(indice_editavel(&turno), Some(2));
        assert_eq!(blocos_depois(&turno, 2), 2);
        // Uma entrega de outro chat depois dela: tirar a mensagem levaria o relatório embora.
        let com_entrega = [
            usuario("primeira"),
            Item::Entrega { em: 0, de: "ajudante".into(), texto: "relatório".into() },
        ];
        assert_eq!(indice_editavel(&com_entrega), None);
        let com_cartao = [usuario("primeira"), Item::Cartao { em: 0, id: 3 }];
        assert_eq!(indice_editavel(&com_cartao), None);
        // Mensagem sozinha no fim: nada sai junto com ela.
        let sozinha = [Item::Assistente { em: 0, texto: "antes".into() }, usuario("última")];
        assert_eq!(indice_editavel(&sozinha), Some(1));
        assert_eq!(blocos_depois(&sozinha, 1), 0);
        // Qualquer mensagem sua dá para corrigir, não só a última; o raciocínio e o pedido de
        // revisar a memória no meio não trancam nada.
        let longa = [
            usuario("primeira"),
            Item::Raciocinio { em: 0, texto: String::new(), milis: 900 },
            Item::Assistente { em: 0, texto: "resposta".into() },
            Item::Entrega { em: 0, de: "Vitral".into(), texto: "reveja a memória".into() },
            usuario("segunda"),
        ];
        assert!(editavel(&longa, 0) && editavel(&longa, 4));
        assert!(!editavel(&longa, 2), "só mensagem do usuário");
        assert_eq!(indice_editavel(&longa), Some(4));
        // O que outro chat entregou tranca o que veio antes dele.
        assert!(!editavel(&com_entrega, 0));
    }

    #[test]
    fn a_pergunta_so_fala_do_que_vai_acontecer() {
        let nada = pergunta(0, false, false, false);
        let textos = |c: &crate::ui::componentes::Confirmacao| {
            c.detalhes.iter().map(|(_, t, _)| t.to_string()).collect::<Vec<_>>()
        };
        assert_eq!(textos(&nada), ["Os arquivos da pasta não voltam sozinhos"]);
        let tudo = pergunta(3, true, true, true);
        let d = textos(&tudo);
        assert_eq!(d.len(), 5, "{d:?}");
        assert!(d[0].contains("para agora"));
        assert_eq!(d[1], "O que está escrito na caixa sai dela (Ctrl+Z traz de volta)");
        assert_eq!(d[2], "3 blocos da resposta saem da conversa");
        assert!(d[4].contains("sessão nova"));
        // O Enter confirma: o texto volta para a caixa, nada se perde de vez.
        assert!(!tudo.ok_perigo);
        // A concordância: um bloco sai, vários saem.
        let uma = pergunta(1, false, false, false);
        assert_eq!(textos(&uma)[0], "1 bloco da resposta sai da conversa");
    }

    /// A caixa com rascunho sempre pergunta, mesmo sem nada depois da mensagem e sem turno: era aí
    /// que o rascunho sumia sem aviso.
    #[test]
    fn com_rascunho_na_caixa_sempre_pergunta() {
        assert!(!precisa_perguntar(0, false, false));
        assert!(precisa_perguntar(0, false, true));
        assert!(precisa_perguntar(2, false, false));
        assert!(precisa_perguntar(0, true, false));
    }
}
