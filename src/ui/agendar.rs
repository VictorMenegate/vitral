//! Mandar a mensagem numa hora marcada: os itens "Mandar quando a janela de 5 h renovar" e
//! "Mandar às…" do "…" da caixa, o diálogo da hora e o envio. A mensagem vai para a fila do chat
//! com a hora (`Entrada::agendada`); quem a segura até lá e a solta é o motor
//! (`motor/agendadas.rs`).

use std::rc::Rc;

use gpui_kit::component::{
    Disableable as _, WindowExt as _,
    input::{Input, InputState},
    menu::{PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{RodapeDialogo, TamanhoBotao, Tom, aviso, botao_fantasma, botao_primario, largura},
    dialogos::{cabecalho, moldura, topo},
};
use crate::{
    modelo::{agora, quando_local},
    tema::{Cores, Lucide, Tipo as _, esp},
};

/// O que os itens do menu precisam, lido uma vez ao desenhar a caixa.
#[derive(Clone)]
pub(super) struct DadosDoMenu {
    pub janela: WeakEntity<Janela>,
    pub painel: u64,
    /// Quando a janela de 5 h da conta do chat renova, já com a folga (`Motor::renovacao_do_chat`).
    pub renova: Option<u64>,
}

/// Os dois itens, no grupo de mandar do "…" (que só aparece com texto na caixa: sem ele, não há o
/// que agendar). Sem a hora da renovação lida (ou com ela já passada), fica só o "Mandar às…".
pub(super) fn itens_do_menu(menu: PopupMenu, dados: &DadosDoMenu) -> PopupMenu {
    let (janela, painel) = (dados.janela.clone(), dados.painel);
    let as_horas = janela.clone();
    menu.when_some(dados.renova, |menu, quando| {
        menu.item(PopupMenuItem::new("Mandar quando as 5 h renovarem").icon(Lucide::CalendarClock).on_click(move |_, window, cx| {
            let _ = janela.update(cx, |this, cx| this.enviar_agendada(painel, quando, window, cx));
        }))
    })
    .item(PopupMenuItem::new("Mandar às…").icon(Lucide::Clock).on_click(move |_, window, cx| {
        let _ = as_horas.update(cx, |this, cx| this.dialogo_mandar_as(painel, window, cx));
    }))
}

/// A hora e o minuto que a pessoa escreveu: "15:10", "15h10", "15h", "15", "1510", "8.30".
pub(super) fn hora_e_minuto(texto: &str) -> Option<(u32, u32)> {
    let texto = texto.trim().to_lowercase();
    let texto = texto.strip_prefix("às").or_else(|| texto.strip_prefix("as")).unwrap_or(&texto).trim();
    let texto = texto.strip_suffix("min").unwrap_or(texto).trim();
    let partes: Vec<&str> = texto.split([':', 'h', '.', ' ']).filter(|p| !p.is_empty()).collect();
    let numero = |p: &str| p.chars().all(|c| c.is_ascii_digit()).then(|| p.parse::<u32>().ok()).flatten();
    let (hora, minuto) = match partes.as_slice() {
        // "1510" e "830": os dois últimos dígitos são o minuto.
        [junto] if junto.len() >= 3 && junto.bytes().all(|b| b.is_ascii_digit()) => {
            let (h, m) = junto.split_at(junto.len() - 2);
            (numero(h)?, numero(m)?)
        }
        [hora] => (numero(hora)?, 0),
        [hora, minuto] => (numero(hora)?, numero(minuto)?),
        _ => return None,
    };
    (hora < 24 && minuto < 60).then_some((hora, minuto))
}

/// A próxima vez que o relógio local marca `hora:minuto` depois de `agora`: hoje, ou amanhã se já
/// passou.
pub(super) fn proxima_hora(hora: u32, minuto: u32, agora: u64) -> Option<u64> {
    use chrono::TimeZone as _;
    let hoje = crate::modelo::no_fuso_local(agora as i64).date_naive();
    [Some(hoje), hoje.succ_opt()].into_iter().flatten().find_map(|dia| {
        let instante = chrono::Local.from_local_datetime(&dia.and_hms_opt(hora, minuto, 0)?).earliest()?.timestamp();
        u64::try_from(instante).ok().filter(|quando| *quando > agora)
    })
}

/// A hora escrita no diálogo, como instante (ver `hora_e_minuto` e `proxima_hora`).
pub(super) fn hora_pedida(texto: &str, agora: u64) -> Option<u64> {
    let (hora, minuto) = hora_e_minuto(texto)?;
    proxima_hora(hora, minuto, agora)
}

/// O selo da mensagem agendada na fila: "sai às 15:10", "sai em 19/11 às 08:00".
pub(super) fn selo_da_hora(quando: u64) -> String {
    format!("sai {}", quando_local(quando))
}

impl Janela {
    /// Manda o que está na caixa do painel para a fila do chat, com a hora marcada. A fila abre,
    /// para o cartão com o selo ficar à vista.
    pub(super) fn enviar_agendada(&mut self, painel: u64, quando: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.mandar_agendada = Some(quando);
        self.ativar_painel(painel, cx);
        self.enviar_do_painel(painel, window, cx);
        // Um envio recusado (caixa vazia, o menu "@" aberto) não guarda a hora para o seguinte.
        if self.mandar_agendada.take().is_some() {
            return;
        }
        if let Some(p) = self.paineis.iter_mut().find(|p| p.id == painel) {
            p.fila_aberta = true;
        }
        let texto = format!("Mensagem agendada: {}. Ela espera na fila deste chat.", selo_da_hora(quando));
        window.push_notification(aviso(Tom::Info, texto), cx);
        cx.notify();
    }

    /// "Mandar às…": pergunta a hora e agenda o que está na caixa do painel.
    pub(super) fn dialogo_mandar_as(&mut self, painel: u64, window: &mut Window, cx: &mut Context<Self>) {
        let campo = cx.new(|cx| InputState::new(window, cx).placeholder("15:10"));
        let janela = cx.entity().downgrade();
        let agendar = {
            let campo = campo.clone();
            Rc::new(move |window: &mut Window, cx: &mut App| -> bool {
                let Some(quando) = hora_pedida(&campo.read(cx).value(), agora()) else {
                    return false;
                };
                let _ = janela.update(cx, |this, cx| this.enviar_agendada(painel, quando, window, cx));
                true
            })
        };
        let foco = campo.read(cx).focus_handle(cx);
        self.foco_antes_do_dialogo(window, cx);
        window.open_dialog(cx, move |dialogo, window, cx| {
            let lida = hora_pedida(&campo.read(cx).value(), agora());
            let vazio = campo.read(cx).value().trim().is_empty();
            let (no_botao, no_enter) = (agendar.clone(), agendar.clone());
            let nota = match lida {
                Some(quando) => format!("A mensagem {} e espera na fila até lá, com o Vitral aberto.", selo_da_hora(quando)),
                None if vazio => "Escreva a hora, como 15:10. Se ela já passou hoje, vale a de amanhã.".to_string(),
                None => "Não entendi a hora. Escreva como 15:10 ou 15h.".to_string(),
            };
            moldura(dialogo, largura::P, window)
                .title(topo(cabecalho("Mandar às…", None)))
                .child(
                    v_flex()
                        .gap(esp::E8)
                        .child(Input::new(&campo))
                        .child(div().metadado().text_color(Cores::tenue()).child(nota)),
                )
                .footer(
                    RodapeDialogo::new()
                        .cancelar(
                            botao_fantasma("mandar-as-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .principal(
                            botao_primario("mandar-as-agendar", None, "Agendar", TamanhoBotao::Controle, window, cx)
                                .when(lida.is_none(), |b| b.disabled(true).opacity(0.45).tooltip("Escreva a hora"))
                                .on_click(move |_, window, cx| {
                                    if no_botao(window, cx) {
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| no_enter(window, cx))
        });
        window.focus(&foco, cx);
    }
}

#[cfg(test)]
mod testes {
    use super::{hora_e_minuto, hora_pedida, proxima_hora, selo_da_hora};

    #[test]
    fn hora_escrita_de_varios_jeitos() {
        assert_eq!(hora_e_minuto("15:10"), Some((15, 10)));
        assert_eq!(hora_e_minuto(" 15h10 "), Some((15, 10)));
        assert_eq!(hora_e_minuto("15h10min"), Some((15, 10)));
        assert_eq!(hora_e_minuto("15h"), Some((15, 0)));
        assert_eq!(hora_e_minuto("8"), Some((8, 0)));
        assert_eq!(hora_e_minuto("1510"), Some((15, 10)));
        assert_eq!(hora_e_minuto("830"), Some((8, 30)));
        assert_eq!(hora_e_minuto("8.30"), Some((8, 30)));
        assert_eq!(hora_e_minuto("às 9:05"), Some((9, 5)));
        assert_eq!(hora_e_minuto("0:00"), Some((0, 0)));
        // O que não é hora não agenda nada.
        for torto in ["", "24:00", "12:60", "meio-dia", "não", "amanhã", "12:3o", "1:2:3", "-5", "15100"] {
            assert_eq!(hora_e_minuto(torto), None, "{torto}");
        }
    }

    #[test]
    fn a_hora_que_ja_passou_hoje_fica_para_amanha() {
        use chrono::{Duration, TimeZone as _, Timelike as _};
        // Meio-dia de um dia qualquer, no fuso desta máquina.
        let meio_dia = chrono::Local.with_ymd_and_hms(2026, 11, 18, 12, 0, 0).earliest().unwrap();
        let agora = meio_dia.timestamp() as u64;
        let as_15 = proxima_hora(15, 10, agora).unwrap();
        assert_eq!(as_15 - agora, 3 * 3600 + 600);
        // 09:00 já passou: é a de amanhã.
        let as_9 = proxima_hora(9, 0, agora).unwrap();
        let esperado = (meio_dia + Duration::days(1)).with_hour(9).unwrap().timestamp() as u64;
        assert_eq!(as_9, esperado);
        // A hora de agora, exata, também é a de amanhã (nunca agenda para o passado).
        assert!(proxima_hora(12, 0, agora).unwrap() > agora + 23 * 3600);
        assert_eq!(hora_pedida("15h10", agora), Some(as_15));
        assert_eq!(hora_pedida("depois", agora), None);
        assert!(selo_da_hora(as_15).starts_with("sai "));
    }
}
