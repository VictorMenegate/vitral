//! "O que precisa de você" (B21, etapa 2): o popover do sino (no trilho e no logo do modo
//! recolhido) e o Ctrl+G. O que entra nele sai de `Motor::pendencias`, na ordem de lá: pedidos e
//! planos, paradas, erros e respostas novas.

use gpui_kit::component::{
    Icon, Selectable, StyledExt as _, WindowExt as _, h_flex,
    popover::{Popover, PopoverState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela, ProximaPendencia,
    componentes::{
        CabecalhoDeSecao, CartaoDeLista, ChaveAviso as _, CorpoPopover, EstadoVazio, LarguraPopover, Selo, Simbolo,
        TamanhoBotao, Tom, aviso, botao_fantasma, botao_icone, botao_primario, numero_painel, popover, simbolo_do_chat,
        teclas,
    },
    lateral::{Causa, MotivoDaPrevia, previa_elemento},
    mensagens::trechos_perigosos,
};
use crate::{
    motor::{Motor, Pendencia},
    tema::{Cores, Lucide, Tipo as _, esp, r, raio, tam},
};

/// O que cada linha pede, com o que os botões dela precisam.
enum Pedido {
    Permissao { pedido: String, ferramenta: String, comando: Option<String> },
    Plano,
    Pergunta,
    Parada,
    Erro { pode_tentar: bool },
    Nova,
}

/// Uma linha da caixa, já lida do motor e da janela.
struct Linha {
    /// Única entre as linhas: o pedido ou o tipo com a conversa.
    chave: String,
    chat: String,
    titulo: String,
    /// O sinal e o rótulo, os mesmos da prévia da lista (a resposta nova não tem).
    motivo: Option<MotivoDaPrevia>,
    detalhe: String,
    /// O que o último turno fez ("3 editados · 12 comandos"), na linha de baixo da resposta nova.
    feito: String,
    /// Número do painel em que a conversa está aberta (e se é o ativo).
    painel: Option<(usize, bool)>,
    quando: u64,
    pedido: Pedido,
}

/// As linhas da caixa em quatro seções, na ordem do motor.
#[derive(Default)]
struct Secoes {
    pedidos: Vec<Linha>,
    paradas: Vec<Linha>,
    erros: Vec<Linha>,
    novas: Vec<Linha>,
}

fn ler_secoes(janela: &Janela, motor: &Motor) -> Secoes {
    let mut secoes = Secoes::default();
    for p in motor.pendencias() {
        let Some(chat) = motor.chat(p.chat()) else {
            continue;
        };
        // A mesma conversa pode ter mais de um pedido, ou estar parada e com erro.
        let chave = match &p {
            Pendencia::Permissao { pedido, .. } | Pendencia::Plano { pedido, .. } | Pendencia::Pergunta { pedido, .. } => pedido.clone(),
            Pendencia::Parada { chat, .. } => format!("parada-{chat}"),
            Pendencia::Erro { chat, .. } => format!("erro-{chat}"),
            Pendencia::Nova { chat } => format!("nova-{chat}"),
        };
        // A frase da lista.
        let (motivo, detalhe) = match Causa::da_pendencia(&p) {
            Some(causa) => {
                let (motivo, detalhe) = MotivoDaPrevia::de(&causa);
                (Some(motivo), detalhe)
            }
            // A resposta nova diz o que o chat respondeu (a primeira frase, sem markdown) em vez de
            // "Resposta nova" em toda linha (rodada de uso, item 3.3).
            None => (None, super::falas::ultima_fala(chat).unwrap_or_else(|| tr!("Resposta nova").to_string())),
        };
        let feito = if matches!(p, Pendencia::Nova { .. }) { super::falas::o_que_o_turno_fez(&chat.itens) } else { String::new() };
        let pedido = match p {
            Pendencia::Permissao { pedido, ferramenta, .. } => {
                let comando = motor
                    .permissoes
                    .iter()
                    .find(|x| x.id == pedido)
                    .and_then(|x| x.entrada["command"].as_str().map(str::to_string));
                Pedido::Permissao { pedido, ferramenta, comando }
            }
            Pendencia::Plano { .. } => Pedido::Plano,
            Pendencia::Pergunta { .. } => Pedido::Pergunta,
            Pendencia::Parada { .. } => Pedido::Parada,
            Pendencia::Erro { pode_tentar, .. } => Pedido::Erro { pode_tentar },
            Pendencia::Nova { .. } => Pedido::Nova,
        };
        let linha = Linha {
            chave,
            chat: chat.id.clone(),
            titulo: chat.titulo.clone(),
            motivo,
            detalhe,
            feito,
            painel: janela.painel_da_conversa(&chat.id),
            quando: chat.atualizado_em,
            pedido,
        };
        match linha.pedido {
            Pedido::Permissao { .. } | Pedido::Plano | Pedido::Pergunta => secoes.pedidos.push(linha),
            Pedido::Parada => secoes.paradas.push(linha),
            Pedido::Erro { .. } => secoes.erros.push(linha),
            Pedido::Nova => secoes.novas.push(linha),
        }
    }
    secoes
}

impl Janela {
    /// O popover "O que precisa de você", controlado por `caixa_aberta`: o sino do trilho e o
    /// item do menu do modo recolhido abrem o mesmo.
    pub(super) fn popover_caixa(
        &self,
        id: &'static str,
        gatilho: impl Selectable + IntoElement + 'static,
        ancora: Anchor,
        cx: &Context<Self>,
    ) -> Popover {
        let janela = cx.entity().downgrade();
        let fechar = janela.clone();
        popover(id)
            .anchor(ancora)
            .offset(px(8.))
            .arrow(true)
            .open(self.caixa_aberta)
            .on_open_change(move |aberto, _, cx| {
                let _ = fechar.update(cx, |this, cx| {
                    if *aberto {
                        this.so_um_flutuante(super::QualFlutuante::Sino);
                    }
                    if this.caixa_aberta != *aberto {
                        this.caixa_aberta = *aberto;
                        cx.notify();
                    }
                });
            })
            .trigger(gatilho)
            .content(move |_, window, cx| {
                let popover = cx.entity();
                caixa_do_sino(&janela, popover, window, cx)
            })
    }

    /// Ctrl+G (e Ctrl+Shift+G, para trás): a pendência seguinte à do chat aberto, achada pelo chat
    /// e não pela posição, dando a volta depois da última (dono, 16/11/2026: rodada de uso, item
    /// 3.4; antes ele recomeçava do início e, com A, B e C pendentes, alternava entre A e B). Ao
    /// chegar, o foco vai para o lugar útil, e a pílula diz onde está ("2 de 3 · Parada · WMS").
    pub(super) fn andar_nas_pendencias(&mut self, passo: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let pendencias = motor.read(cx).pendencias();
        let chats = chats_das_pendencias(&pendencias);
        let Some(ix) = proxima_na_volta(&chats, self.selecionado.as_deref(), passo) else {
            // Sem pendência, o primeiro par travado no freio de ciclo; sem ele, a pílula avisa.
            match super::resumo::esperas_de_ciclo(motor.read(cx)).into_iter().next() {
                Some(e) => self.selecionar(Some(e.chat), window, cx),
                None => window.push_notification(aviso(Tom::Info, tr!("Nada esperando você")).chave("ctrl-g"), cx),
            }
            return;
        };
        let chat = chats[ix].clone();
        let Some(pendencia) = pendencias.iter().find(|p| p.chat() == chat).cloned() else {
            return;
        };
        self.selecionar(Some(chat.clone()), window, cx);
        let titulo = motor.read(cx).chat(&chat).map(|c| c.titulo.clone()).unwrap_or_default();
        let pilula = tr!("{n} de {total} · {tipo} · {titulo}", n = ix + 1, total = chats.len(), tipo = rotulo_da_pendencia(&pendencia), titulo = titulo);
        window.push_notification(aviso(Tom::Info, pilula).chave("ctrl-g"), cx);
        // O foco no lugar útil: o Retomar da parada, o Tentar de novo do erro, o comentário do
        // plano; nos outros casos, a caixa. (O pedido de permissão pelo teclado é o item 6.5.)
        let painel = self.paineis.get(self.ativo);
        let foco = match &pendencia {
            Pendencia::Parada { .. } | Pendencia::Erro { pode_tentar: true, .. } => painel.map(|p| p.foco_faixa.clone()),
            Pendencia::Plano { pedido, .. } => self.feedback_plano.get(pedido).map(|campo| campo.read(cx).focus_handle(cx)),
            _ => None,
        }
        .or_else(|| painel.map(|p| p.composer.read(cx).focus_handle(cx)));
        if let Some(foco) = foco {
            window.focus(&foco, cx);
        }
    }

    /// Ctrl+G: a próxima.
    pub(super) fn proxima_pendencia(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.andar_nas_pendencias(1, window, cx);
    }
}

/// Os chats das pendências, um por chat, na ordem do sino.
fn chats_das_pendencias(pendencias: &[Pendencia]) -> Vec<String> {
    let mut chats: Vec<String> = Vec::new();
    for p in pendencias {
        if !chats.iter().any(|c| c == p.chat()) {
            chats.push(p.chat().to_string());
        }
    }
    chats
}

/// A posição da pendência seguinte (ou anterior, com `passo` negativo) à do chat `atual`, dando a
/// volta; sem o chat aberto na lista, a primeira (ou a última, para trás).
fn proxima_na_volta(chats: &[String], atual: Option<&str>, passo: i32) -> Option<usize> {
    let n = chats.len() as i32;
    if n == 0 {
        return None;
    }
    Some(match atual.and_then(|a| chats.iter().position(|c| c == a)) {
        Some(k) => (k as i32 + passo).rem_euclid(n) as usize,
        None if passo < 0 => (n - 1) as usize,
        None => 0,
    })
}

/// O nome da pendência na pílula do Ctrl+G.
fn rotulo_da_pendencia(p: &Pendencia) -> SharedString {
    match p {
        Pendencia::Permissao { .. } => tr!("Permissão"),
        Pendencia::Plano { .. } => tr!("Plano para aprovar"),
        Pendencia::Pergunta { .. } => tr!("Pergunta"),
        Pendencia::Erro { .. } => tr!("Erro"),
        Pendencia::Parada { .. } => tr!("Parada"),
        Pendencia::Nova { .. } => tr!("Resposta nova"),
    }
}

/// Fecha a caixa e abre a conversa (Ctrl+clique: ao lado).
fn abrir_conversa(
    janela: &WeakEntity<Janela>,
    popover: &WeakEntity<PopoverState>,
    chat: &str,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let (janela, popover, chat) = (janela.clone(), popover.clone(), chat.to_string());
    move |evento, window, cx| {
        let _ = popover.update(cx, |p, cx| p.dismiss(window, cx));
        let chat = chat.clone();
        let _ = janela.update(cx, |this, cx| {
            if evento.modifiers().control {
                this.abrir_ao_lado(chat, None, window, cx);
            } else {
                this.selecionar(Some(chat), window, cx);
            }
        });
    }
}

/// O conteúdo da caixa, lido do motor a cada desenho.
fn caixa_do_sino(
    janela: &WeakEntity<Janela>,
    popover: Entity<PopoverState>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Some(j) = janela.upgrade() else {
        return div().into_any_element();
    };
    let Some(motor) = j.read(cx).motor.clone() else {
        return div().into_any_element();
    };
    let (secoes, precisa, de_ciclo) = {
        let m = motor.read(cx);
        (ler_secoes(j.read(cx), m), super::resumo::precisa_em_tudo(m), super::resumo::esperas_de_ciclo(m))
    };
    let popover = popover.downgrade();
    let numero = precisa.numero();
    let cor = if precisa.erro { Cores::vermelho() } else { Cores::espera() };
    let novas: Vec<String> = secoes.novas.iter().map(|l| l.chat.clone()).collect();
    let vazia = de_ciclo.is_empty()
        && secoes.pedidos.is_empty()
        && secoes.paradas.is_empty()
        && secoes.erros.is_empty()
        && secoes.novas.is_empty();

    let cabecalho = h_flex()
        .gap(esp::E8)
        .min_h(r(20.))
        .child(div().flex_none().text_color(Cores::tenue()).child(Icon::new(Lucide::Bell).size(r(14.))))
        .child(
            div()
                .text_size(r(14.))
                .line_height(r(20.))
                .font_semibold()
                .text_color(Cores::texto())
                .child(tr!("O que precisa de você")),
        )
        .when(numero > 0, |s| s.child(Selo::contagem(numero).tingida(cor)))
        .child(div().flex_1())
        .when_some(teclas(&ProximaPendencia, None, window), |s, t| {
            s.child(t).child(div().metadado().text_color(Cores::tenue()).child(tr!("próxima")))
        });

    // Como os itens de menu do kit: o realce do hover a 8 da borda, e o ícone e o texto na guia
    // de 16 do cabeçalho.
    let mut corpo = v_flex()
        .id("caixa-corpo")
        .mx(r(-8.))
        .gap(esp::E2)
        .max_h(window.viewport_size().height - tam(window, 200.))
        .overflow_y_scroll();
    if vazia {
        corpo = corpo.child(
            EstadoVazio::new(Lucide::BellOff, tr!("Nada esperando você"))
                .texto(tr!("Pedidos de permissão, planos e erros aparecem aqui.")),
        );
    }
    // A primeira seção encosta no título (4 em cima, e não os 16 de respiro).
    let primeira = if de_ciclo.is_empty() {
        [&secoes.pedidos, &secoes.paradas, &secoes.erros, &secoes.novas].iter().position(|l| !l.is_empty())
    } else {
        Some(usize::MAX)
    };
    if !de_ciclo.is_empty() {
        corpo = corpo.child(CabecalhoDeSecao::new(tr!("Mensagens seguidas")).contagem(de_ciclo.len()).tingida(Cores::espera()).primeira(true));
        for e in de_ciclo {
            let (chat, chave) = (e.chat.clone(), e.chave.clone());
            let (janela_v, popover_v) = (janela.clone(), popover.clone());
            let abrir = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                let _ = popover_v.update(cx, |p, cx| p.dismiss(window, cx));
                let _ = janela_v.update(cx, |this, cx| this.selecionar(Some(chat.clone()), window, cx));
            };
            let abrir = std::rc::Rc::new(abrir);
            let (a1, a2) = (abrir.clone(), abrir);
            let subtitulo = previa_elemento(None, e.texto, false);
            let titulo = motor.read(cx).chat(&e.chat).map(|c| c.titulo.clone()).unwrap_or_default();
            corpo = corpo.child(
                CartaoDeLista::new(id("caixa-ciclo", &chave), titulo)
                    .simbolo(Simbolo::icone_cor(Lucide::MessageSquare, Cores::espera()))
                    .subtitulo(subtitulo)
                    .acao_fixa(
                        botao_fantasma(id("caixa-ciclo-ver", &chave), None, tr!("Ver"), TamanhoBotao::Linha, window, cx)
                            .on_click(move |ev, window, cx| a1(ev, window, cx)),
                    )
                    .on_click(move |ev, window, cx| a2(ev, window, cx)),
            );
        }
    }
    let secao = |ordem: usize, titulo: SharedString, linhas: &[Linha], cor: Option<Hsla>| {
        (!linhas.is_empty()).then(|| {
            let cabecalho = CabecalhoDeSecao::new(titulo).contagem(linhas.len()).primeira(primeira == Some(ordem));
            match cor {
                Some(cor) => cabecalho.tingida(cor),
                None => cabecalho,
            }
        })
    };
    corpo = corpo
        .children(secao(0, tr!("Pedidos"), &secoes.pedidos, Some(Cores::espera())))
        .children(secoes.pedidos.into_iter().map(|l| linha_de_pedido(l, janela, &popover, &motor, window, cx)))
        .children(secao(1, tr!("Paradas"), &secoes.paradas, Some(Cores::espera())).map(|cabecalho| {
            // Com duas ou mais, "Retomar tudo" (o mesmo da lista; item 3.4): depois de "Vitral
            // fechou" eram N cliques.
            if secoes.paradas.len() < 2 {
                return cabecalho;
            }
            let (motor, chats) = (motor.clone(), secoes.paradas.iter().map(|l| l.chat.clone()).collect::<Vec<_>>());
            cabecalho.acao(
                botao_fantasma("caixa-retomar-tudo", Some(Lucide::Play.into()), tr!("Retomar tudo"), TamanhoBotao::Linha, window, cx)
                    .on_click(move |_, _, cx| motor.update(cx, |m, cx| for c in &chats { m.retomar(c, cx) })),
            )
        }))
        .children(secoes.paradas.into_iter().map(|l| linha(l, janela, &popover, &motor, window, cx)))
        .children(secao(2, tr!("Erros"), &secoes.erros, Some(Cores::vermelho())))
        .children(secoes.erros.into_iter().map(|l| linha(l, janela, &popover, &motor, window, cx)))
        .children(secao(3, tr!("Respostas novas"), &secoes.novas, None))
        .children(secoes.novas.into_iter().map(|l| linha(l, janela, &popover, &motor, window, cx)));

    let (ver, fechar) = (janela.clone(), popover.clone());
    let rodape = h_flex()
        .gap(esp::E8)
        .when(!novas.is_empty(), |s| {
            let motor = motor.clone();
            s.child(
                botao_fantasma("caixa-marcar-lidas", Some(Lucide::CheckCheck.into()), tr!("Marcar tudo como lido"), TamanhoBotao::Barra, window, cx)
                    .ml(r(-12.))
                    .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.marcar_lidos(&novas, cx))),
            )
        })
        .child(div().flex_1())
        .child(
            botao_fantasma("caixa-ver-na-lista", Some(Lucide::ArrowRight.into()), tr!("Ver na lista"), TamanhoBotao::Barra, window, cx)
                .on_click(move |_, window, cx| {
                    let _ = fechar.update(cx, |p, cx| p.dismiss(window, cx));
                    let _ = ver.update(cx, |this, cx| this.mostrar_esperando(window, cx));
                }),
        );

    CorpoPopover::new(LarguraPopover::P360)
        .child(cabecalho)
        .child(corpo)
        .rodape(rodape)
        .into_any_element()
}

/// Onde o texto da linha começa (8 do cartão, o balão de 24 e 12): o comando e os botões do
/// pedido ficam nessa coluna.
const TEXTO_DA_LINHA: f32 = 44.;

/// O balão de 24 à esquerda da linha.
fn simbolo() -> Simbolo {
    Simbolo::Elemento(simbolo_do_chat(24.).into_any_element())
}

/// À direita: o número do painel em que a conversa está aberta, ou há quanto tempo mudou.
fn meta(l: &Linha) -> AnyElement {
    match l.painel {
        Some((n, ativo)) => numero_painel(n, ativo).into_any_element(),
        None => div().child(super::tempo_relativo_na_tela(l.quando)).into_any_element(),
    }
}

/// Parada, erro e resposta nova: o cartão da lista com a ação no hover.
fn linha(
    l: Linha,
    janela: &WeakEntity<Janela>,
    popover: &WeakEntity<PopoverState>,
    motor: &Entity<Motor>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let chat = l.chat.clone();
    let motor = motor.clone();
    let acao = match l.pedido {
        Pedido::Parada => Some(
            botao_fantasma(id("caixa-retomar", &l.chave), Some(Lucide::Play.into()), tr!("Retomar"), TamanhoBotao::Linha, window, cx)
                .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.retomar(&chat, cx))),
        ),
        Pedido::Erro { pode_tentar: true } => Some(
            botao_fantasma(id("caixa-tentar", &l.chave), Some(Lucide::RotateCw.into()), tr!("Tentar de novo"), TamanhoBotao::Linha, window, cx)
                .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.tentar_de_novo(&chat, cx))),
        ),
        Pedido::Nova => Some(
            botao_icone(id("caixa-lida", &l.chave), Lucide::Check, tr!("Marcar como lida"), TamanhoBotao::Linha, window, cx)
                .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.marcar_lidos(&[chat.clone()], cx))),
        ),
        _ => None,
    };
    // Embaixo da frase, o que o turno fez: dá para separar o chat que mexeu em arquivos do que só
    // respondeu sem abrir nenhum.
    let subtitulo = v_flex()
        .min_w_0()
        .child(previa_elemento(l.motivo.as_ref(), l.detalhe.clone(), false))
        .when(!l.feito.is_empty(), |s| s.child(div().min_w_0().truncate().metadado().text_color(Cores::apagado()).child(l.feito.clone())));
    let cartao = CartaoDeLista::new(id("caixa-linha", &l.chave), l.titulo.clone())
        .simbolo(simbolo())
        .subtitulo(subtitulo)
        .meta(meta(&l))
        .on_click(abrir_conversa(janela, popover, &l.chat));
    // O botão com texto ("Tentar de novo", "Retomar") fica no fluxo, sempre à vista, e o título
    // encolhe antes dele. Como ação de hover ele cairia no lugar de um botão de ícone (24 px) e
    // cobria o título (dono, 16/11/2026: "esse tentar de novo tá bugado"). Só o de ícone
    // ("Marcar como lida") é ação de hover.
    let com_texto = !matches!(l.pedido, Pedido::Nova);
    match acao {
        Some(acao) if com_texto => cartao.acao_fixa(div().flex_none().ml(esp::E8).child(acao)).into_any_element(),
        Some(acao) => cartao.acao(acao).into_any_element(),
        None => cartao.into_any_element(),
    }
}

/// Pedido de permissão ou plano: o cartão e, embaixo, o comando e os botões. O pedido se responde
/// aqui (com trecho arriscado no comando, "Permitir mesmo assim" em vermelho); o plano abre a
/// conversa para ler.
fn linha_de_pedido(
    l: Linha,
    janela: &WeakEntity<Janela>,
    popover: &WeakEntity<PopoverState>,
    motor: &Entity<Motor>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let chave = l.chave.as_str();
    let cartao = CartaoDeLista::new(id("caixa-pedido", chave), l.titulo.clone())
        .simbolo(simbolo())
        .subtitulo(previa_elemento(l.motivo.as_ref(), l.detalhe.clone(), false))
        .meta(meta(&l))
        .on_click(abrir_conversa(janela, popover, &l.chat));
    let ver = |rotulo: SharedString| {
        botao_fantasma(id("caixa-ver", chave), Some(Lucide::ArrowRight.into()), rotulo, TamanhoBotao::Barra, window, cx)
            .on_click(abrir_conversa(janela, popover, &l.chat))
    };
    let (comando, botoes): (Option<String>, Vec<AnyElement>) = match &l.pedido {
        Pedido::Permissao { pedido, ferramenta, comando } => {
            let (m1, m2, p1, p2) = (motor.clone(), motor.clone(), pedido.clone(), pedido.clone());
            let negar = botao_fantasma(id("caixa-negar", chave), None, tr!("Negar"), TamanhoBotao::Barra, window, cx)
                .on_click(move |_, _, cx| m1.update(cx, |m, cx| m.responder_permissao(&p1, false, None, cx)));
            // Comando com trecho que apaga, força ou roda algo difícil de desfazer, ou comprido
            // demais para a caixinha daqui (que corta em duas linhas): não se aprova pelo sino.
            // O botão leva à conversa, onde o cartão mostra o comando inteiro e o perigoso pede
            // o "segure para permitir". Aprovar daqui com um clique furava as duas garantias.
            let perigoso = ferramenta == "Bash"
                && comando.as_deref().is_some_and(|c| !trechos_perigosos(c).is_empty());
            let comprido = comando.as_deref().is_some_and(|c| c.lines().count() > 2 || c.chars().count() > 160);
            let segundo = if perigoso || comprido {
                ver(if perigoso { tr!("Ver o comando perigoso") } else { tr!("Ver o comando inteiro") }).into_any_element()
            } else {
                botao_primario(id("caixa-permitir", chave), None, tr!("Permitir"), TamanhoBotao::Barra, window, cx)
                    .on_click(move |_, _, cx| m2.update(cx, |m, cx| m.responder_permissao(&p2, true, None, cx)))
                    .into_any_element()
            };
            (comando.clone(), vec![negar.into_any_element(), segundo])
        }
        // A pergunta se responde no cartão dela, na conversa.
        Pedido::Pergunta => (None, vec![ver(tr!("Ver pergunta")).into_any_element()]),
        _ => (None, vec![ver(tr!("Ver plano")).into_any_element()]),
    };
    v_flex()
        .child(cartao)
        .child(
            v_flex()
                .gap(esp::E6)
                .pl(r(TEXTO_DA_LINHA))
                .pr(esp::E8)
                .pb(esp::E8)
                .when_some(comando, |s, comando| {
                    s.child(
                        div()
                            .px(esp::E8)
                            .py(esp::E4)
                            .max_h(r(56.))
                            .overflow_hidden()
                            .rounded(raio::CONTROLE)
                            .bg(Cores::preto())
                            .border_1()
                            .border_color(Cores::borda())
                            .mono_pequeno()
                            .text_color(Cores::codigo())
                            .child(format!("$ {comando}")),
                    )
                })
                .child(h_flex().justify_end().gap(esp::E6).children(botoes)),
        )
        .into_any_element()
}

fn id(prefixo: &str, chave: &str) -> ElementId {
    SharedString::from(format!("{prefixo}-{chave}")).into()
}

#[cfg(test)]
mod testes {
    use super::proxima_na_volta;

    #[test]
    fn ctrl_g_anda_pelas_pendencias_dando_a_volta() {
        let chats: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        // Do A vai ao B, do B ao C e do C volta ao A: o C não fica de fora.
        assert_eq!(proxima_na_volta(&chats, Some("a"), 1), Some(1));
        assert_eq!(proxima_na_volta(&chats, Some("b"), 1), Some(2));
        assert_eq!(proxima_na_volta(&chats, Some("c"), 1), Some(0));
        // Para trás (Ctrl+Shift+G).
        assert_eq!(proxima_na_volta(&chats, Some("a"), -1), Some(2));
        // Sem o chat aberto entre as pendências: a primeira, ou a última para trás.
        assert_eq!(proxima_na_volta(&chats, Some("x"), 1), Some(0));
        assert_eq!(proxima_na_volta(&chats, None, -1), Some(2));
        assert_eq!(proxima_na_volta(&[], Some("a"), 1), None);
    }
}
