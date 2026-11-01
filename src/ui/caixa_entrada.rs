//! "O que precisa de você" (B21, etapa 2): o popover do sino (no trilho e no logo do modo
//! recolhido) e o Ctrl+G. O que entra nele sai de `Motor::pendencias`, na ordem de lá: pedidos e
//! planos, paradas, erros e respostas novas.

use gpui_kit::component::{
    Icon, Selectable, StyledExt as _, h_flex,
    popover::{Popover, PopoverState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela, ProximaPendencia,
    componentes::{
        CabecalhoDeSecao, CartaoDeLista, CorpoPopover, EstadoVazio, LarguraPopover, QuadradoDeProjeto,
        Selo, Simbolo, TamanhoBotao, botao_fantasma, botao_icone, botao_perigo, botao_primario, numero_painel,
        popover, teclas,
    },
    icone_projeto,
    lateral::{Causa, MotivoDaPrevia, previa_elemento},
    mensagens::trechos_perigosos,
    tempo_relativo,
};
use crate::{
    motor::{Motor, Pendencia},
    tema::{Cores, Lucide, Tipo as _, esp, r, raio, tam},
};

/// O que cada linha pede, com o que os botões dela precisam.
enum Pedido {
    Permissao { pedido: String, ferramenta: String, comando: Option<String> },
    Plano,
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
    icone: &'static icone_projeto::Icone,
    cor: u32,
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
        let (icone, cor) = icone_projeto::visual(motor, &chat.projeto);
        // A mesma conversa pode ter mais de um pedido, ou estar parada e com erro.
        let chave = match &p {
            Pendencia::Permissao { pedido, .. } | Pendencia::Plano { pedido, .. } => pedido.clone(),
            Pendencia::Parada { chat, .. } => format!("parada-{chat}"),
            Pendencia::Erro { chat, .. } => format!("erro-{chat}"),
            Pendencia::Nova { chat } => format!("nova-{chat}"),
        };
        // A frase da lista: a linha já é da conversa que pede, então sem o número da tarefa.
        let (motivo, detalhe) = match Causa::da_pendencia(&p) {
            Some(causa) => {
                let (motivo, detalhe) = MotivoDaPrevia::de(&causa, None, 0);
                (Some(motivo), detalhe)
            }
            None => (None, format!("Resposta nova em {}", chat.projeto)),
        };
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
            icone,
            cor,
            painel: janela.painel_da_conversa(&chat.id),
            quando: chat.atualizado_em,
            pedido,
        };
        match linha.pedido {
            Pedido::Permissao { .. } | Pedido::Plano => secoes.pedidos.push(linha),
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

    /// Ctrl+G: abre a próxima pendência, a mais antiga que não é a conversa já aberta.
    pub(super) fn proxima_pendencia(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let pendencias = motor.read(cx).pendencias();
        let aberta = self.selecionado.as_deref();
        let Some(chat) = pendencias
            .iter()
            .find(|p| Some(p.chat()) != aberta)
            .or(pendencias.first())
            .map(|p| p.chat().to_string())
        else {
            return;
        };
        self.selecionar(Some(chat), window, cx);
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
    let (secoes, precisa) = {
        let m = motor.read(cx);
        (ler_secoes(j.read(cx), m), m.precisa_de_voce(None))
    };
    let popover = popover.downgrade();
    let numero = precisa.numero();
    let cor = if precisa.erro { Cores::vermelho() } else { Cores::ambar() };
    let novas: Vec<String> = secoes.novas.iter().map(|l| l.chat.clone()).collect();
    let vazia = secoes.pedidos.is_empty()
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
                .child("O que precisa de você"),
        )
        .when(numero > 0, |s| s.child(Selo::contagem(numero).tingida(cor)))
        .child(div().flex_1())
        .when_some(teclas(&ProximaPendencia, None, window), |s, t| {
            s.child(t).child(div().metadado().text_color(Cores::tenue()).child("próxima"))
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
            EstadoVazio::new(Lucide::BellOff, "Nada esperando você")
                .texto("Pedidos de permissão, planos e erros aparecem aqui."),
        );
    }
    // A primeira seção encosta no título (4 em cima, e não os 16 de respiro).
    let primeira = [&secoes.pedidos, &secoes.paradas, &secoes.erros, &secoes.novas].iter().position(|l| !l.is_empty());
    let secao = |ordem: usize, titulo: &'static str, linhas: &[Linha], cor: Option<Hsla>| {
        (!linhas.is_empty()).then(|| {
            let cabecalho = CabecalhoDeSecao::new(titulo).contagem(linhas.len()).primeira(primeira == Some(ordem));
            match cor {
                Some(cor) => cabecalho.tingida(cor),
                None => cabecalho,
            }
        })
    };
    corpo = corpo
        .children(secao(0, "Pedidos", &secoes.pedidos, Some(Cores::ambar())))
        .children(secoes.pedidos.into_iter().map(|l| linha_de_pedido(l, janela, &popover, &motor, window, cx)))
        .children(secao(1, "Paradas", &secoes.paradas, Some(Cores::ambar())))
        .children(secoes.paradas.into_iter().map(|l| linha(l, janela, &popover, &motor, window, cx)))
        .children(secao(2, "Erros", &secoes.erros, Some(Cores::vermelho())))
        .children(secoes.erros.into_iter().map(|l| linha(l, janela, &popover, &motor, window, cx)))
        .children(secao(3, "Respostas novas", &secoes.novas, None))
        .children(secoes.novas.into_iter().map(|l| linha(l, janela, &popover, &motor, window, cx)));

    let (ver, fechar) = (janela.clone(), popover.clone());
    let rodape = h_flex()
        .gap(esp::E8)
        .when(!novas.is_empty(), |s| {
            let motor = motor.clone();
            s.child(
                botao_fantasma("caixa-marcar-lidas", Some(Lucide::CheckCheck.into()), "Marcar tudo como lido", TamanhoBotao::Barra, window, cx)
                    .ml(r(-12.))
                    .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.marcar_lidos(&novas, cx))),
            )
        })
        .child(div().flex_1())
        .child(
            botao_fantasma("caixa-ver-na-lista", Some(Lucide::ArrowRight.into()), "Ver na lista", TamanhoBotao::Barra, window, cx)
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

/// Onde o texto da linha começa (8 do cartão, o quadrado de 24 e 12): o comando e os botões do
/// pedido ficam nessa coluna.
const TEXTO_DA_LINHA: f32 = 44.;

/// O quadrado do projeto de 24 à esquerda da linha.
fn simbolo(l: &Linha) -> Simbolo {
    Simbolo::Elemento(QuadradoDeProjeto::new(l.icone, l.cor, 24.).into_any_element())
}

/// À direita: o número do painel em que a conversa está aberta, ou há quanto tempo mudou.
fn meta(l: &Linha) -> AnyElement {
    match l.painel {
        Some((n, ativo)) => numero_painel(n, ativo).into_any_element(),
        None => div().child(tempo_relativo(l.quando)).into_any_element(),
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
            botao_fantasma(id("caixa-retomar", &l.chave), Some(Lucide::Play.into()), "Retomar", TamanhoBotao::Linha, window, cx)
                .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.retomar(&chat, cx))),
        ),
        Pedido::Erro { pode_tentar: true } => Some(
            botao_fantasma(id("caixa-tentar", &l.chave), Some(Lucide::RotateCw.into()), "Tentar de novo", TamanhoBotao::Linha, window, cx)
                .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.tentar_de_novo(&chat, cx))),
        ),
        Pedido::Nova => Some(
            botao_icone(id("caixa-lida", &l.chave), Lucide::Check, "Marcar como lida", TamanhoBotao::Linha, window, cx)
                .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.marcar_lidos(&[chat.clone()], cx))),
        ),
        _ => None,
    };
    let cartao = CartaoDeLista::new(id("caixa-linha", &l.chave), l.titulo.clone())
        .simbolo(simbolo(&l))
        .subtitulo(previa_elemento(l.motivo.as_ref(), l.detalhe.clone(), false))
        .meta(meta(&l))
        .on_click(abrir_conversa(janela, popover, &l.chat));
    match acao {
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
        .simbolo(simbolo(&l))
        .subtitulo(previa_elemento(l.motivo.as_ref(), l.detalhe.clone(), false))
        .meta(meta(&l))
        .on_click(abrir_conversa(janela, popover, &l.chat));
    let ver = |rotulo: &'static str| {
        botao_fantasma(id("caixa-ver", chave), Some(Lucide::ArrowRight.into()), rotulo, TamanhoBotao::Barra, window, cx)
            .on_click(abrir_conversa(janela, popover, &l.chat))
    };
    let (comando, botoes): (Option<String>, Vec<AnyElement>) = match &l.pedido {
        Pedido::Permissao { pedido, ferramenta, comando } => {
            let (m1, m2, p1, p2) = (motor.clone(), motor.clone(), pedido.clone(), pedido.clone());
            let negar = botao_fantasma(id("caixa-negar", chave), None, "Negar", TamanhoBotao::Barra, window, cx)
                .on_click(move |_, _, cx| m1.update(cx, |m, cx| m.responder_permissao(&p1, false, None, cx)));
            // Comando com trecho que apaga, força ou roda algo difícil de desfazer: o botão avisa.
            let perigoso = ferramenta == "Bash"
                && comando.as_deref().is_some_and(|c| !trechos_perigosos(c).is_empty());
            let segundo = if perigoso {
                botao_perigo(id("caixa-permitir", chave), None, "Permitir mesmo assim", TamanhoBotao::Barra, window, cx)
            } else {
                botao_primario(id("caixa-permitir", chave), None, "Permitir", TamanhoBotao::Barra, window, cx)
            }
            .on_click(move |_, _, cx| m2.update(cx, |m, cx| m.responder_permissao(&p2, true, None, cx)))
            .into_any_element();
            (comando.clone(), vec![negar.into_any_element(), segundo])
        }
        _ => (None, vec![ver("Ver plano").into_any_element()]),
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
