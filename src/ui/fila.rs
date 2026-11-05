//! A fila de mensagens de uma conversa: o que escolheu esperar o turno acabar ("Mandar depois"),
//! o que chegou de outro chat com este ocupado e o que o turno não chegou a receber. A pílula da
//! faixa do turno abre esta lista, que mostra cada mensagem esperando e deixa editar uma, mudar a
//! ordem, tirar uma de volta para a caixa e, com o chat parado, entregar tudo agora.

use gpui_kit::component::{
    Disableable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Textarea, TextareaState},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        CabecalhoPopover, CorpoPopover, LarguraPopover, RodapeDialogo, Selo, TamanhoBotao, Tom, aviso, botao_fantasma,
        botao_icone, botao_primario, contagem, largura, popover, variante_fantasma,
    },
    dialogos::{cabecalho, moldura, topo},
};
use crate::{
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, esp, r, raio},
};

/// Quanto de cada mensagem a lista mostra numa linha só.
const LETRAS_DA_PREVIA: usize = 140;

/// Quanto do texto inteiro cabe na dica da linha.
const LETRAS_DA_DICA: usize = 600;

/// A dica de uma linha da lista: o texto inteiro (cortado), com quem mandou e os anexos.
fn dica_da_linha(e: &crate::modelo::Entrada) -> String {
    let mut partes = Vec::new();
    if let Some(de) = &e.de {
        partes.push(format!("De: {de}"));
    } else if let Some(rotina) = &e.rotina {
        partes.push(format!("Rotina: {rotina}"));
    }
    if !e.anexos.is_empty() {
        partes.push(contagem(e.anexos.len(), "anexo", "anexos"));
    }
    let texto = e.texto.trim();
    let texto = if texto.chars().count() > LETRAS_DA_DICA {
        format!("{}…", texto.chars().take(LETRAS_DA_DICA).collect::<String>())
    } else {
        texto.to_string()
    };
    partes.push(texto);
    partes.join("\n")
}

/// A primeira linha de uma mensagem da fila, cortada no limite.
pub(super) fn previa(texto: &str) -> String {
    let linha = texto.trim().lines().next().unwrap_or("").trim();
    if linha.chars().count() <= LETRAS_DA_PREVIA {
        return linha.to_string();
    }
    format!("{}…", linha.chars().take(LETRAS_DA_PREVIA).collect::<String>())
}

/// O que a pílula da faixa diz: com o turno rodando, só a contagem; parado, o que a fila é.
pub(super) fn rotulo(n: usize, rodando: bool, estreita: bool) -> String {
    match (rodando, estreita) {
        (true, _) => "na fila".to_string(),
        (false, true) => contagem(n, "na fila", "na fila"),
        (false, false) => contagem(n, "mensagem esperando na fila", "mensagens esperando na fila"),
    }
}

impl Janela {
    /// A pílula clicável da fila, com o número e o popover da lista. `rodando`: o turno está em
    /// andamento (a fila entra quando ele acabar); parado, o rodapé traz o "Entregar agora".
    pub(super) fn pilula_da_fila(
        &self,
        chat: &str,
        painel: u64,
        n: usize,
        rodando: bool,
        estreita: bool,
        _window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let dica: SharedString = if rodando {
            "Ver a fila: o que você mandou enquanto o turno roda, e tirar o que não quer mais".into()
        } else {
            "Ver a fila desta conversa e tirar o que não quer mais".into()
        };
        let gatilho = Button::new(("pilula-fila", painel as usize))
            .custom(variante_fantasma(cx))
            .h(r(22.))
            .px(r(6.))
            .rounded(raio::PILULA)
            .tooltip(dica)
            .child(
                h_flex()
                    .gap(r(4.))
                    .child(Selo::contagem(n))
                    .child(div().metadado().text_color(Cores::apagado()).child(rotulo(n, rodando, estreita))),
            );
        let (motor, janela, id) = (self.motor.clone(), cx.entity().downgrade(), chat.to_string());
        popover(("pop-fila", painel as usize))
            .anchor(Anchor::TopLeft)
            .offset(px(8.))
            .trigger(gatilho)
            .content(move |_, window, cx| {
                let Some(m) = motor.clone() else {
                    return div().into_any_element();
                };
                corpo_da_fila(&m, &janela, &id, painel, rodando, window, cx)
            })
            .into_any_element()
    }

    /// Tira uma mensagem da fila. O texto volta para a caixa quando ela está vazia; com algo
    /// escrito lá, ele iria no lugar do que você está escrevendo, então só sai da fila e o aviso
    /// diz qual era.
    pub(super) fn tirar_da_fila(&mut self, chat: String, indice: usize, painel: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let Some(texto) = motor.update(cx, |m, cx| m.tirar_da_fila(&chat, indice, cx)) else {
            return;
        };
        let caixa_vazia = self
            .paineis
            .iter()
            .find(|p| p.id == painel)
            .is_some_and(|p| p.composer.read(cx).value().trim().is_empty());
        if caixa_vazia {
            self.escrever_na_caixa(painel, &texto, window, cx);
            window.push_notification(aviso(Tom::Info, "Tirei da fila; o texto voltou para a caixa."), cx);
        } else {
            window.push_notification(aviso(Tom::Info, format!("Tirei da fila: “{}”", previa(&texto))), cx);
        }
        cx.notify();
    }
}

/// Uma linha da lista, já pronta para desenhar (o índice é o da `fila_do_chat`).
struct Linha {
    indice: usize,
    previa: String,
    dica: String,
    do_usuario: bool,
    primeira: bool,
    ultima: bool,
}

/// A lista da fila: uma linha por mensagem, na ordem em que vão entrar, com editar, mudar de lugar
/// e tirar em cada uma.
fn corpo_da_fila(
    motor: &Entity<Motor>,
    janela: &WeakEntity<Janela>,
    chat: &str,
    painel: u64,
    rodando: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let m = motor.read(cx);
    let fila = m.fila_do_chat(chat);
    let total = fila.len();
    let linhas: Vec<Linha> = fila
        .into_iter()
        .enumerate()
        .map(|(posicao, (indice, e))| Linha {
            indice,
            previa: previa(&e.texto),
            dica: dica_da_linha(e),
            do_usuario: e.do_usuario(),
            primeira: posicao == 0,
            ultima: posicao + 1 == total,
        })
        .collect();
    let cabecalho = |valor: Option<SharedString>| CabecalhoPopover::Escolha {
        simbolo: Some(super::componentes::Simbolo::Icone(Lucide::ListChecks.into(), Some(Cores::tenue()))),
        titulo: "Fila desta conversa".into(),
        valor: valor.map(|v| (v, Cores::texto())),
        dica: None,
    };
    if linhas.is_empty() {
        return CorpoPopover::new(LarguraPopover::P400)
            .cabecalho(cabecalho(None))
            .child(div().corpo_ui().text_color(Cores::tenue()).child("A fila está vazia."))
            .into_any_element();
    }
    let desenhadas: Vec<AnyElement> = linhas
        .into_iter()
        .enumerate()
        .map(|(posicao, linha)| linha_da_fila(motor, janela, chat, painel, posicao, linha, window, cx))
        .collect();
    let explicacao = if rodando {
        "Entram juntas no próximo turno, quando este terminar."
    } else {
        "Entram juntas na próxima mensagem, ou agora, no botão abaixo."
    };
    let entregar = (!rodando).then(|| {
        let (motor, chat) = (motor.clone(), chat.to_string());
        botao_fantasma("fila-entregar", Some(Lucide::Play.into()), "Entregar agora", TamanhoBotao::Barra, window, cx)
            .on_click(move |_, _, cx| {
                motor.update(cx, |m, cx| m.entregar_fila(&chat, cx));
            })
    });
    CorpoPopover::new(LarguraPopover::P400)
        .cabecalho(cabecalho(Some(contagem(total, "mensagem", "mensagens").into())))
        .child(v_flex().gap(esp::E4).children(desenhadas))
        .child(div().metadado().text_color(Cores::tenue()).child(explicacao))
        .when_some(entregar, |c, b| c.rodape(h_flex().justify_end().child(b)))
        .into_any_element()
}

/// Uma linha: o lugar na ordem, a primeira linha do texto (o resto na dica) e, à direita, editar,
/// subir, descer e tirar.
#[allow(clippy::too_many_arguments)]
fn linha_da_fila(
    motor: &Entity<Motor>,
    janela: &WeakEntity<Janela>,
    chat: &str,
    painel: u64,
    posicao: usize,
    linha: Linha,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Linha { indice, previa, dica, do_usuario, primeira, ultima } = linha;
    let dica: SharedString = dica.into();
    let mover = |para_baixo: bool| {
        let (motor, chat) = (motor.clone(), chat.to_string());
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            motor.update(cx, |m, cx| m.mover_na_fila(&chat, indice, para_baixo, cx));
        }
    };
    let editar = {
        let (janela, chat) = (janela.clone(), chat.to_string());
        botao_icone(
            ("editar-da-fila", indice),
            Lucide::Pencil,
            if do_usuario { "Editar esta mensagem" } else { "Só as suas mensagens podem ser editadas" },
            TamanhoBotao::Linha,
            window,
            cx,
        )
        .when(!do_usuario, |b| b.disabled(true).opacity(0.45))
        .on_click(move |_, window, cx| {
            let _ = janela.update(cx, |this, cx| this.dialogo_editar_da_fila(chat.clone(), indice, window, cx));
        })
    };
    let subir = botao_icone(("subir-na-fila", indice), Lucide::MoveUp, "Mandar antes", TamanhoBotao::Linha, window, cx)
        .when(primeira, |b| b.disabled(true).opacity(0.45))
        .on_click(mover(false));
    let descer = botao_icone(("descer-na-fila", indice), Lucide::MoveDown, "Mandar depois", TamanhoBotao::Linha, window, cx)
        .when(ultima, |b| b.disabled(true).opacity(0.45))
        .on_click(mover(true));
    let tirar = {
        let (janela, chat) = (janela.clone(), chat.to_string());
        botao_icone(
            ("tirar-da-fila", indice),
            Lucide::X,
            "Tirar da fila (o texto volta para a caixa, se ela estiver vazia)",
            TamanhoBotao::Linha,
            window,
            cx,
        )
        .on_click(move |_, window, cx| {
            let _ = janela.update(cx, |this, cx| this.tirar_da_fila(chat.clone(), indice, painel, window, cx));
        })
    };
    h_flex()
        .id(("linha-da-fila", indice))
        .gap(esp::E8)
        .min_h(r(28.))
        .child(
            div()
                .flex_none()
                .size(r(18.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(raio::CONTROLE)
                .bg(Cores::recuo())
                .micro()
                .text_color(Cores::tenue())
                .child((posicao + 1).to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .corpo_ui()
                .text_color(if do_usuario { Cores::texto() } else { Cores::tenue() })
                .child(previa),
        )
        .child(h_flex().flex_none().child(editar).child(subir).child(descer).child(tirar))
        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        .into_any_element()
}

impl Janela {
    /// Edita uma mensagem que está na fila: o texto inteiro num campo, para corrigir antes de ela
    /// entrar. Salvar troca o texto no lugar dela (a ordem não muda); o diálogo recusa se a fila
    /// andou enquanto ele estava aberto.
    pub(super) fn dialogo_editar_da_fila(
        &mut self,
        chat: String,
        indice: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let Some(inicial) = motor
            .read(cx)
            .fila_do_chat(&chat)
            .into_iter()
            .find(|(ix, _)| *ix == indice)
            .map(|(_, e)| e.texto.clone())
        else {
            return;
        };
        let campo = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(6, 16)
                .placeholder("A mensagem que vai entrar no próximo turno")
                .default_value(inicial)
        });
        let foco = campo.read(cx).focus_handle(cx);
        self.foco_antes_do_dialogo(window, cx);
        window.open_dialog(cx, move |dialogo, window, cx| {
            let (motor, chat) = (motor.clone(), chat.clone());
            let (campo, no_botao) = (campo.clone(), campo.clone());
            let vazio = campo.read(cx).value().trim().is_empty();
            let salvar = botao_primario("fila-editar-salvar", None, "Salvar", TamanhoBotao::Controle, window, cx)
                .when(vazio, |b| b.disabled(true).opacity(0.45).tooltip("Uma mensagem vazia não vai; tire ela da fila"))
                .on_click(move |_, window, cx| {
                    let texto = no_botao.read(cx).value().to_string();
                    let trocou = motor.update(cx, |m, cx| m.editar_na_fila(&chat, indice, &texto, cx));
                    if !trocou {
                        window.push_notification(aviso(Tom::Atencao, "Esta mensagem já saiu da fila; nada foi mudado."), cx);
                    }
                    window.close_dialog(cx);
                });
            moldura(dialogo, largura::M, window)
                .title(topo(cabecalho("Editar a mensagem da fila", Some("Ela entra no próximo turno, com o texto que ficar aqui".into()))))
                .child(Textarea::new(&campo))
                .footer(
                    RodapeDialogo::new()
                        .cancelar(
                            botao_fantasma("fila-editar-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .principal(salvar),
                )
                // O Enter quebra a linha no campo; quem salva é o botão.
                .on_ok(|_, _, _| false)
        });
        window.focus(&foco, cx);
    }
}

#[cfg(test)]
mod testes {
    use super::{dica_da_linha, previa, rotulo};
    use crate::modelo::Entrada;

    #[test]
    fn a_previa_e_a_primeira_linha_cortada() {
        assert_eq!(previa("  uma linha só \n e outra"), "uma linha só");
        assert_eq!(previa(""), "");
        let longa = "á".repeat(200);
        let cortada = previa(&longa);
        assert_eq!(cortada.chars().count(), 141);
        assert!(cortada.ends_with('…'));
    }

    #[test]
    fn a_dica_tem_o_texto_inteiro_com_quem_mandou_e_os_anexos() {
        let entrada = Entrada {
            texto: "primeira\nsegunda".into(),
            de: Some("chat 3".into()),
            anexos: vec!["/tmp/a.png".into()],
            ..Default::default()
        };
        assert_eq!(dica_da_linha(&entrada), "De: chat 3\n1 anexo\nprimeira\nsegunda");
        let minha = Entrada { texto: "  só minha  ".into(), ..Default::default() };
        assert_eq!(dica_da_linha(&minha), "só minha");
        let longa = Entrada { texto: "á".repeat(700), ..Default::default() };
        assert_eq!(dica_da_linha(&longa).chars().count(), 601);
    }

    #[test]
    fn a_pilula_diz_o_que_cabe_na_largura() {
        assert_eq!(rotulo(2, true, false), "na fila");
        assert_eq!(rotulo(1, false, false), "1 mensagem esperando na fila");
        assert_eq!(rotulo(3, false, false), "3 mensagens esperando na fila");
        assert_eq!(rotulo(3, false, true), "3 na fila");
    }
}
