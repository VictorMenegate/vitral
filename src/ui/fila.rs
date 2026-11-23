//! A fila de mensagens de uma conversa: o que escolheu esperar o turno acabar ("Mandar depois"),
//! o que chegou de outro chat com este ocupado e o que o turno não chegou a receber. A pílula da
//! faixa do turno abre esta lista acima da caixa de mensagem, dentro do painel. Deixa editar uma, mudar a
//! ordem, tirar uma de volta para a caixa, mandar uma agora para dentro do turno que está rodando
//! e, com o chat parado, entregar tudo agora.

use gpui_kit::component::{
    Disableable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Enter, Textarea, TextareaState},
    scroll::ScrollableElement as _,
    tooltip::{ManagedTooltipExt as _, Tooltip},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{LarguraPainel, Selo, TamanhoBotao, Tom, aviso, botao_fantasma, botao_icone, contagem, variante_fantasma},
};
use crate::{
    modelo::Entrada,
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, esp, r, raio},
};

/// Quanto de cada mensagem a lista mostra numa linha só.
const LETRAS_DA_PREVIA: usize = 140;

/// Quanto do texto inteiro cabe na dica da linha.
const LETRAS_DA_DICA: usize = 600;

/// A dica de uma linha da lista: o texto inteiro (cortado), com quem mandou e os anexos.
fn dica_da_linha(e: &Entrada) -> String {
    let mut partes = Vec::new();
    if let Some(de) = &e.de {
        partes.push(format!("De: {de}"));
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
        _chat: &str,
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
            .rounded(raio::de(22.))
            .tooltip(dica)
            .child(
                h_flex()
                    .gap(r(4.))
                    .child(Selo::contagem(n))
                    .child(div().metadado().text_color(Cores::apagado()).child(rotulo(n, rodando, estreita))),
            );
        gatilho.on_click(cx.listener(move |this, _, _, cx| {
            if let Some(p) = this.paineis.iter_mut().find(|p| p.id == painel) { p.fila_aberta = !p.fila_aberta; }
            cx.notify();
        })).into_any_element()
    }

    pub(super) fn fila_do_painel(&self, motor: &Entity<Motor>, chat: &str, ix: usize, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let p = &self.paineis[ix];
        if !p.fila_aberta || motor.read(cx).na_fila(chat) == 0 { return None; }
        let rodando = matches!(motor.read(cx).execucao(chat), crate::modelo::Execucao::Rodando { .. });
        // A linha em edição vai por parâmetro: ler a Janela daqui de dentro, no desenho dela,
        // derrubava o app (`cannot read Janela while it is already being updated`).
        let editando = self.editando_fila.as_ref().filter(|e| e.chat == chat).map(|e| (e.indice, e.campo.clone()));
        Some(div().map(super::mensagens::coluna).py(esp::E8)
            .child(corpo_da_fila(motor, &cx.entity().downgrade(), chat, p.id, rodando, p.medida.nivel == LarguraPainel::Largo, editando, window, cx)).into_any_element())
    }

    /// Tira uma mensagem da fila e a devolve inteira à caixa (rodada de uso, item 1.3): com a caixa
    /// vazia, o texto, os anexos e o reforço vão para ela; com algo escrito, o texto se junta ao fim,
    /// depois de uma linha em branco, e os anexos às pílulas (o Ctrl+Z desfaz a junção). Antes, o ×
    /// devolvia só o texto, e com a caixa ocupada nem isso.
    pub(super) fn tirar_da_fila(&mut self, chat: String, indice: usize, painel: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let Some(entrada) = motor.update(cx, |m, cx| m.tirar_da_fila(&chat, indice, cx)) else {
            return;
        };
        let juntou = self.devolver_a_caixa(painel, &chat, entrada, window, cx);
        let texto = if juntou {
            "Tirei da fila e juntei ao que estava na caixa (Ctrl+Z desfaz a junção)."
        } else {
            "Tirei da fila; a mensagem voltou para a caixa."
        };
        window.push_notification(aviso(Tom::Info, texto), cx);
        cx.notify();
    }

    /// Põe uma mensagem que saiu da fila na caixa do painel, com os anexos e o reforço. Devolve
    /// `true` quando juntou ao que já estava escrito.
    pub(super) fn devolver_a_caixa(&mut self, painel: u64, chat: &str, entrada: Entrada, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(composer) = self.paineis.iter().find(|p| p.id == painel).map(|p| p.composer.clone()) else {
            return false;
        };
        let atual = composer.read(cx).value().to_string();
        let juntou = !atual.trim().is_empty();
        let texto = juntar_na_caixa(&atual, &entrada.texto);
        if !entrada.anexos.is_empty() {
            let lista = self.anexos.entry(chat.to_string()).or_default();
            for caminho in entrada.anexos {
                if !lista.contains(&caminho) {
                    lista.push(caminho);
                }
            }
        }
        if entrada.ultrathink {
            self.ultrathink.insert(chat.to_string());
        }
        if entrada.ultracode {
            self.ultracode.insert(chat.to_string());
        }
        // Pelo `por_texto` (o `replace_all` do campo): o Ctrl+Z traz de volta o que estava escrito.
        self.escrever_na_caixa(painel, &texto, window, cx);
        juntou
    }

    /// A seta para cima com o turno rodando, a caixa vazia e algo na fila: a última da fila volta
    /// para a caixa, com os anexos e o reforço, sem tocar no turno; o Ctrl+Enter a devolve à fila
    /// (rodada de uso, item 1.3). Devolve `false` quando não há o que trazer.
    pub(super) fn trazer_da_fila(&mut self, painel: u64, chat: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(motor) = self.motor.clone() else {
            return false;
        };
        let Some(ultima) = motor.read(cx).fila_do_chat(chat).into_iter().filter(|(_, e)| e.do_usuario()).map(|(i, _)| i).last() else {
            return false;
        };
        let Some(entrada) = motor.update(cx, |m, cx| m.tirar_da_fila(chat, ultima, cx)) else {
            return false;
        };
        self.devolver_a_caixa(painel, chat, entrada, window, cx);
        window.push_notification(aviso(Tom::Info, "Trouxe da fila a última mensagem; Ctrl+Enter a devolve à fila."), cx);
        true
    }

    /// O lápis da fila: edita a mensagem na própria linha do cartão, sem diálogo (item 1.3).
    pub(super) fn editar_na_linha(&mut self, chat: String, indice: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let Some(antes) = motor.read(cx).fila_do_chat(&chat).into_iter().find(|(ix, _)| *ix == indice).map(|(_, e)| e.texto.clone()) else {
            return;
        };
        let campo = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 8).default_value(antes.clone()));
        // O cursor no fim do texto, como na caixa: o que se digita continua a mensagem.
        let fim = antes.len();
        campo.update(cx, |c, cx| c.set_selected_range(fim..fim, cx));
        let foco = campo.read(cx).focus_handle(cx);
        self.editando_fila = Some(EdicaoNaFila { chat, indice, antes, campo });
        window.focus(&foco, cx);
        cx.notify();
    }

    /// Ctrl+Enter na linha que se edita: troca o texto se a fila não andou; Esc desiste.
    pub(super) fn fechar_edicao_da_fila(&mut self, salvar: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edicao) = self.editando_fila.take() else {
            return;
        };
        if salvar && let Some(motor) = self.motor.clone() {
            let texto = edicao.campo.read(cx).value().to_string();
            if !texto.trim().is_empty() {
                let trocou = motor.update(cx, |m, cx| m.editar_na_fila(&edicao.chat, edicao.indice, &edicao.antes, &texto, cx));
                if !trocou {
                    window.push_notification(aviso(Tom::Atencao, "A fila andou enquanto você editava; nada foi mudado."), cx);
                }
            }
        }
        self.focar_composer(window, cx);
        cx.notify();
    }
}

/// A mensagem que está sendo editada na linha da fila.
pub(super) struct EdicaoNaFila {
    chat: String,
    indice: usize,
    /// O texto de quando a edição abriu: se a fila andou, nada é trocado.
    antes: String,
    campo: Entity<TextareaState>,
}

/// O texto da caixa com a mensagem da fila: no lugar do vazio, ou no fim, depois de uma linha em
/// branco.
pub(super) fn juntar_na_caixa(atual: &str, texto: &str) -> String {
    if atual.trim().is_empty() {
        texto.to_string()
    } else {
        format!("{}\n\n{}", atual.trim_end(), texto.trim_start())
    }
}

/// Uma linha da lista, já pronta para desenhar: o índice é o da `fila_do_chat` e a posição é o
/// lugar na ordem de entrada, de 0.
struct Linha {
    indice: usize,
    posicao: usize,
    previa: String,
    dica: String,
    /// "De: loja · 2 anexos": o que a mensagem leva além do texto.
    meta: String,
    do_usuario: bool,
    primeira: bool,
    ultima: bool,
    /// O turno que roda recebe mensagem: a linha ganha o "Mandar agora".
    agora: bool,
    /// O painel é largo: o "Mandar agora" vai com o rótulo.
    larga: bool,
    /// Esta linha está sendo editada no lugar: o campo dela.
    edicao: Option<Entity<TextareaState>>,
    /// A mensagem tem hora marcada: o selo dela ("sai às 15:10").
    agendada: Option<String>,
}

/// O que a mensagem leva além do texto, numa linha curta: quem mandou e quantos anexos.
fn meta_da_linha(e: &Entrada) -> String {
    let mut partes = Vec::new();
    if let Some(de) = &e.de {
        partes.push(format!("De: {de}"));
    }
    if !e.anexos.is_empty() {
        partes.push(contagem(e.anexos.len(), "anexo", "anexos"));
    }
    partes.join(" · ")
}

/// A lista da fila: uma linha por mensagem, na ordem em que vão entrar, com editar, mudar de lugar
/// e tirar em cada uma.
#[allow(clippy::too_many_arguments)]
fn corpo_da_fila(
    motor: &Entity<Motor>,
    janela: &WeakEntity<Janela>,
    chat: &str,
    painel: u64,
    rodando: bool,
    larga: bool,
    editando: Option<(usize, Entity<TextareaState>)>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let m = motor.read(cx);
    let fila = m.fila_do_chat(chat);
    let total = fila.len();
    // Com o turno rodando e recebendo mensagem, cada uma pode entrar nele agora.
    let agora = rodando && m.turno_recebe_mensagem(chat);
    let linhas: Vec<Linha> = fila
        .into_iter()
        .enumerate()
        .map(|(posicao, (indice, e))| Linha {
            indice,
            posicao,
            previa: previa(&e.texto),
            dica: dica_da_linha(e),
            meta: meta_da_linha(e),
            do_usuario: e.do_usuario(),
            primeira: posicao == 0,
            ultima: posicao + 1 == total,
            agora,
            larga,
            edicao: editando.as_ref().filter(|(i, _)| *i == indice).map(|(_, c)| c.clone()),
            agendada: e.agendada.map(super::agendar::selo_da_hora),
        })
        .collect();
    let fechar = janela.clone();
    let explicacao = if agora {
        "Esperam este turno terminar e entram juntas no próximo. “Mandar agora” põe a mensagem dentro do turno que está rodando, sem pará-lo."
    } else if rodando {
        "Entram juntas no próximo turno, quando este terminar."
    } else {
        "Entram juntas na próxima mensagem, ou agora, no botão abaixo."
    };
    // A explicação vai na dica do título: com 4 linhas embaixo, o cartão tomava quase metade do
    // painel e cortava a 3ª mensagem (teste de usabilidade, 16/11/2026).
    let cabecalho = h_flex().gap(esp::E8).items_center()
        .child(super::mensagens::icone_de(Lucide::ListChecks, 16.).text_color(Cores::tenue()))
        .child(
            div()
                .id(("titulo-da-fila", painel as usize))
                .flex_1()
                .corpo_ui()
                .text_color(Cores::texto())
                .child(format!("Fila · {}", contagem(total, "mensagem", "mensagens")))
                .dica_no_ponteiro(move |window, cx| Tooltip::new(explicacao).build(window, cx)),
        )
        .child(botao_icone(("fechar-fila", painel as usize), Lucide::X, "Fechar a fila", TamanhoBotao::Linha, window, cx)
            .on_click(move |_, _, cx| { let _ = fechar.update(cx, |j, cx| {
                if let Some(p) = j.paineis.iter_mut().find(|p| p.id == painel) { p.fila_aberta = false; }
                cx.notify();
            }); }));
    if linhas.is_empty() {
        return div().into_any_element();
    }
    let desenhadas: Vec<AnyElement> = linhas
        .into_iter()
        .map(|linha| linha_da_fila(motor, janela, chat, painel, linha, window, cx))
        .collect();
    let entregar = if agora && total > 1 {
        // Uma por uma, na ordem da fila: cada mandada tira a primeira, e a seguinte vira a primeira.
        let (motor, chat) = (motor.clone(), chat.to_string());
        Some(
            botao_fantasma("fila-mandar-todas", Some(Lucide::ArrowUp.into()), "Mandar todas agora", TamanhoBotao::Barra, window, cx).on_click(
                move |_, window, cx| {
                    let resultado = motor.update(cx, |m, cx| {
                        while let Some(indice) = m.fila_do_chat(&chat).first().map(|(i, _)| *i) {
                            m.mandar_da_fila_agora(&chat, indice, cx)?;
                        }
                        Ok::<(), &'static str>(())
                    });
                    if let Err(motivo) = resultado {
                        window.push_notification(aviso(Tom::Atencao, motivo), cx);
                    }
                },
            ),
        )
    } else if !rodando {
        let (motor, chat) = (motor.clone(), chat.to_string());
        Some(
            botao_fantasma("fila-entregar", Some(Lucide::Play.into()), "Entregar agora", TamanhoBotao::Barra, window, cx)
                .on_click(move |_, _, cx| {
                    // Tudo entra já, as mensagens com hora marcada também.
                    motor.update(cx, |m, cx| m.entregar_fila_agora(&chat, cx));
                }),
        )
    } else {
        None
    };
    v_flex().w_full().min_w_0().gap(esp::E8).p(esp::E12).rounded(raio::CARTAO)
        .border_1().border_color(Cores::borda()).bg(Cores::cartao_elevado())
        .child(cabecalho)
        // Até umas três mensagens inteiras antes de rolar. A barra do kit (16) fica por cima da
        // ponta direita e segura o clique: as linhas guardam o lugar dela, senão o × ficava
        // debaixo da barra e não tirava a mensagem da fila (teste de usabilidade, 16/11/2026).
        .child(
            v_flex()
                .gap(esp::E6)
                .pr(esp::E16)
                .children(desenhadas)
                .max_h(r(260.))
                .overflow_y_scrollbar()
                .id(("fila-mensagens", painel as usize)),
        )
        .when_some(entregar, |c, b| c.child(h_flex().justify_end().child(b)))
        .into_any_element()
}

/// Uma mensagem da fila, num cartão de dois andares: em cima o lugar na ordem e o começo do texto
/// (até duas linhas; o resto na dica); embaixo o que ela leva junto e as ações. Com as ações todas
/// ao lado do texto sobravam umas vinte letras dele (dono, 13/11/2026: "melhora essa parte").
#[allow(clippy::too_many_arguments)]
fn linha_da_fila(
    motor: &Entity<Motor>,
    janela: &WeakEntity<Janela>,
    chat: &str,
    painel: u64,
    linha: Linha,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let Linha { indice, posicao, previa, dica, meta, do_usuario, primeira, ultima, agora, larga, edicao, agendada } = linha;
    let dica: SharedString = dica.into();
    let mover = |para_baixo: bool| {
        let (motor, chat) = (motor.clone(), chat.to_string());
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            motor.update(cx, |m, cx| m.mover_na_fila(&chat, indice, para_baixo, cx));
        }
    };
    let mandar = agora.then(|| {
        let (motor, chat) = (motor.clone(), chat.to_string());
        // Fora do painel largo, só a seta: com o rótulo a fileira não cabia em ~340 e o × saía
        // cortado na borda.
        if larga {
            botao_fantasma(("mandar-da-fila", indice), Some(Lucide::ArrowUp.into()), "Mandar agora", TamanhoBotao::Linha, window, cx)
                .tooltip("Entra no turno que está rodando, sem pará-lo")
        } else {
            botao_icone(("mandar-da-fila", indice), Lucide::ArrowUp, "Mandar agora\nEntra no turno que está rodando, sem pará-lo", TamanhoBotao::Linha, window, cx)
        }
            .on_click(move |_, window, cx| {
                if let Err(motivo) = motor.update(cx, |m, cx| m.mandar_da_fila_agora(&chat, indice, cx)) {
                    window.push_notification(aviso(Tom::Atencao, motivo), cx);
                }
            })
    });
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
            let _ = janela.update(cx, |this, cx| this.editar_na_linha(chat.clone(), indice, window, cx));
        })
    };
    // Com uma mensagem só não há ordem para mudar.
    let ordem = (!(primeira && ultima)).then(|| {
        let subir = botao_icone(("subir-na-fila", indice), Lucide::MoveUp, "Subir na fila: entra antes da de cima", TamanhoBotao::Linha, window, cx)
            .when(primeira, |b| b.disabled(true).opacity(0.45))
            .on_click(mover(false));
        let descer = botao_icone(("descer-na-fila", indice), Lucide::MoveDown, "Descer na fila: entra depois da de baixo", TamanhoBotao::Linha, window, cx)
            .when(ultima, |b| b.disabled(true).opacity(0.45))
            .on_click(mover(true));
        h_flex().child(subir).child(descer)
    });
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
    v_flex()
        .id(("linha-da-fila", indice))
        .gap(esp::E4)
        .px(esp::E8)
        .pt(esp::E8)
        .pb(esp::E4)
        .rounded(raio::CONTROLE)
        .bg(Cores::recuo())
        .child(
            h_flex()
                .items_start()
                .gap(esp::E8)
                .child(
                    div()
                        .flex_none()
                        .size(r(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(raio::CONTROLE)
                        .bg(Cores::hover())
                        .micro()
                        .text_color(Cores::tenue())
                        .child((posicao + 1).to_string()),
                )
                .child(match edicao {
                    // Editando no lugar: Ctrl+Enter salva e Esc desiste (o Enter quebra a linha).
                    Some(campo) => {
                        let (salvar, desistir) = (janela.clone(), janela.clone());
                        div()
                            .flex_1()
                            .min_w_0()
                            // O Ctrl+Enter do campo vira a ação Enter{secondary} do kit, que é
                            // despachada antes das teclas e quebraria a linha: ela é pega aqui, na
                            // captura, antes do campo (conferência na bancada, 16/11/2026).
                            .capture_action(move |acao: &Enter, window, cx| {
                                if acao.secondary {
                                    cx.stop_propagation();
                                    let _ = salvar.update(cx, |this, cx| this.fechar_edicao_da_fila(true, window, cx));
                                }
                            })
                            .capture_key_down(move |ev: &KeyDownEvent, window, cx| {
                                if ev.keystroke.key == "escape" {
                                    cx.stop_propagation();
                                    let _ = desistir.update(cx, |this, cx| this.fechar_edicao_da_fila(false, window, cx));
                                }
                            })
                            .child(Textarea::new(&campo))
                            .child(div().pt(esp::E4).metadado().text_color(Cores::apagado()).child("Ctrl+Enter salva · Esc desiste"))
                            .into_any_element()
                    }
                    None => div()
                        .id(("texto-da-fila", indice))
                        .flex_1()
                        .min_w_0()
                        .line_clamp(2)
                        .corpo_ui()
                        .text_color(if do_usuario { Cores::texto() } else { Cores::tenue() })
                        .child(previa)
                        .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                        .into_any_element(),
                }),
        )
        .child(
            h_flex()
                .gap(esp::E4)
                // Na coluna do texto, depois do número.
                .pl(esp::E24)
                // A hora marcada, num selo com o relógio, antes do resto.
                .children(agendada.map(|hora| Selo::neutro(hora).icone(Lucide::Clock, Cores::tenue())))
                .child(div().flex_1().min_w_0().truncate().metadado().text_color(Cores::apagado()).child(meta))
                // Os botões não encolhem: quem corta é a meta.
                .child(h_flex().flex_none().gap(esp::E4).children(mandar).child(editar).children(ordem).child(tirar)),
        )
        .into_any_element()
}

#[cfg(test)]
mod testes {
    use super::{Entrada, dica_da_linha, juntar_na_caixa, previa, rotulo};

    #[test]
    fn a_mensagem_da_fila_se_junta_ao_que_esta_na_caixa() {
        assert_eq!(juntar_na_caixa("", "volta inteira"), "volta inteira");
        assert_eq!(juntar_na_caixa("  \n", "volta inteira"), "volta inteira");
        assert_eq!(juntar_na_caixa("estava escrevendo  \n", "\nda fila"), "estava escrevendo\n\nda fila");
    }

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
