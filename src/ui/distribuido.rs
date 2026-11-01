//! O painel da direita do Mestre, "Distribuído": o que as divisões dele mandaram aos chats, por
//! projeto (as que ainda andam e as que terminaram nas últimas 24 h; as de antes num link), cada
//! chat com o estado, o andamento e o que mudou, e embaixo a memória do Mestre com Editar.

use std::collections::BTreeMap;

use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        BarraSegmentada, CabecalhoDeSecao, CartaoDeLista, EstadoVazio, QuadradoDeProjeto, Selo, Simbolo, TamanhoBotao,
        botao_fantasma, estado_visual, pilula_da_tarefa,
    },
    equipe::{Bloco, Contagem},
    icone_projeto,
    paineis::Lado,
};
use crate::{
    modelo::{Cartao, EstadoCartao, ID_MESTRE, Parte, agora},
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, esp, r},
};

/// Uma divisão começada fica no Distribuído até 24 h depois de terminar.
const JANELA: u64 = 24 * 3600;
/// A chave de "Ver anteriores" em `expandidos`.
const ANTERIORES: &str = "distribuido-anteriores";

/// A divisão ainda anda (algum chat que começou sem resultado) ou terminou há menos de 24 h.
pub(super) fn divisao_recente(c: &Cartao, agora: u64) -> bool {
    let Some(d) = c.divisao().filter(|_| c.estado == EstadoCartao::Aceito) else {
        return false;
    };
    let andando = d.partes.iter().any(|p| p.criada.is_some() && p.resultado.is_none());
    andando || agora.saturating_sub(c.decidido_em) < JANELA
}

/// As partes comecadas, por projeto (na ordem em que aparecem), das divisões que o filtro aceita.
fn por_projeto<'a>(cartoes: &[&'a Cartao]) -> Vec<(String, Vec<(u64, &'a Parte)>)> {
    let mut ordem: Vec<String> = Vec::new();
    let mut grupos: BTreeMap<String, Vec<(u64, &Parte)>> = BTreeMap::new();
    for c in cartoes {
        let Some(d) = c.divisao() else {
            continue;
        };
        for p in d.partes.iter().filter(|p| p.criada.is_some()) {
            if !grupos.contains_key(&p.projeto) {
                ordem.push(p.projeto.clone());
            }
            grupos.entry(p.projeto.clone()).or_default().push((c.id, p));
        }
    }
    ordem.into_iter().filter_map(|projeto| Some((projeto.clone(), grupos.remove(&projeto)?))).collect()
}

impl Janela {
    /// Os blocos do Distribuído, no ritmo da Equipe.
    pub(super) fn distribuido(&self, motor: &Motor, window: &Window, cx: &Context<Self>) -> Vec<(Bloco, AnyElement)> {
        let mut blocos = Vec::new();
        let hora = agora();
        let mut comecadas: Vec<&Cartao> = motor
            .cartoes()
            .iter()
            .filter(|c| c.chat == ID_MESTRE && c.estado == EstadoCartao::Aceito && c.divisao().is_some())
            .collect();
        comecadas.sort_by_key(|c| std::cmp::Reverse(c.decidido_em));
        let (recentes, antigas): (Vec<&Cartao>, Vec<&Cartao>) = comecadas.into_iter().partition(|c| divisao_recente(c, hora));
        let ver_antigas = self.expandidos.contains(ANTERIORES);
        let mostradas: Vec<&Cartao> = if ver_antigas { recentes.iter().chain(antigas.iter()).copied().collect() } else { recentes };
        let grupos = por_projeto(&mostradas);
        if grupos.is_empty() {
            blocos.push((
                Bloco::Caixa,
                EstadoVazio::new(Lucide::GitFork, "Nada distribuído ainda")
                    .texto("Peça algo ao Mestre e ele mostra como vai dividir antes de começar.")
                    .into_any_element(),
            ));
        }
        for (projeto, partes) in grupos {
            blocos.push((Bloco::Secao, self.cabecalho_do_grupo(motor, &projeto, &partes)));
            for (cartao, parte) in partes {
                blocos.push((Bloco::Caixa, self.linha_distribuida(motor, cartao, parte, window, cx)));
            }
        }
        if !antigas.is_empty() {
            let rotulo = if ver_antigas { "Esconder as anteriores".to_string() } else { format!("Ver anteriores ({})", antigas.len()) };
            let botao = botao_fantasma("distribuido-anteriores", Some(Lucide::Clock.into()), rotulo, TamanhoBotao::Barra, window, cx)
                .on_click(super::mensagens::alternar(ANTERIORES.to_string(), cx));
            blocos.push((Bloco::Resumo, div().px(esp::E4).child(botao).into_any_element()));
        }
        blocos.push((Bloco::Secao, self.memoria_no_distribuido(motor, window, cx)));
        blocos
    }

    /// O cabeçalho de um projeto: o quadrado de 20, o nome, quantos chats e a barra do estado deles.
    fn cabecalho_do_grupo(&self, motor: &Motor, projeto: &str, partes: &[(u64, &Parte)]) -> AnyElement {
        let mut conta = Contagem::default();
        for (_, p) in partes {
            if let Some(t) = p.criada.as_ref().and_then(|c| motor.tarefa(c.tarefa)) {
                conta.somar(t.id, &estado_visual(motor, t));
            }
        }
        let simbolo = if projeto.is_empty() {
            div()
                .flex_none()
                .size(r(20.))
                .flex()
                .items_center()
                .justify_center()
                .child(svg().path(Lucide::MessageCircle.path()).size(r(16.)).text_color(Cores::tenue()))
                .into_any_element()
        } else {
            let (icone, cor) = icone_projeto::visual_do(motor, projeto);
            QuadradoDeProjeto::new(icone, cor, 20.).into_any_element()
        };
        h_flex()
            .h(r(28.))
            .px(esp::E4)
            .gap(esp::E8)
            .child(simbolo)
            .child(div().flex_none().max_w(r(160.)).truncate().rotulo_secao().child(motor.nome_do_projeto(projeto)))
            .child(Selo::contagem(partes.len()))
            .child(div().flex_1().min_w_0().px(esp::E4).child(BarraSegmentada::new(conta.partes()).fina()))
            .into_any_element()
    }

    /// Um chat da divisão: o estado, "chat 12 · título", o andamento ou o que mudou, e Revisar
    /// quando ele entregou. O clique abre ao lado (Ctrl+clique, no lugar).
    fn linha_distribuida(&self, motor: &Motor, cartao: u64, p: &Parte, window: &Window, cx: &Context<Self>) -> AnyElement {
        let Some(criada) = p.criada.as_ref() else {
            return div().into_any_element();
        };
        let chat = motor.chat(&criada.chat);
        let tarefa = motor.tarefa(criada.tarefa);
        let titulo = match chat {
            Some(c) => format!("chat {} · {}", criada.tarefa, c.titulo),
            None => format!("chat {} · {}", criada.tarefa, p.titulo),
        };
        // O símbolo cabe no lugar de 30 do cartão, com o ícone de 16 (ou o ponto do estado) no meio.
        let lugar = |e: AnyElement| Simbolo::Elemento(div().size(r(30.)).flex().items_center().justify_center().child(e).into_any_element());
        let icone = |i: Lucide, cor: Hsla| Icon::new(i).size(r(16.)).text_color(cor).into_any_element();
        let (simbolo, meta): (Simbolo, AnyElement) = match (p.resultado.as_deref(), tarefa) {
            (Some("juntada"), _) => (
                lugar(icone(Lucide::CircleCheck, Cores::verde())),
                Selo::cor("juntado", Cores::verde()).into_any_element(),
            ),
            (Some(outro), _) => (lugar(icone(Lucide::Ban, Cores::tenue())), Selo::neutro(outro.to_string()).into_any_element()),
            (None, Some(t)) => {
                let visual = estado_visual(motor, t);
                let cor = visual.estado.cor().unwrap_or_else(Cores::tenue);
                (lugar(div().size(r(8.)).rounded_full().bg(cor).into_any_element()), pilula_da_tarefa(motor, t).compacta().into_any_element())
            }
            (None, None) => (lugar(icone(Lucide::CircleX, Cores::vermelho())), Selo::neutro("sumiu").into_any_element()),
        };
        let subtitulo = match tarefa {
            _ if p.resultado.is_some() => p.detalhe.clone().unwrap_or_default(),
            Some(t) if t.progresso.is_some() && p.resultado.is_none() => t.progresso.clone().unwrap_or_default(),
            Some(t) => match t.mudancas.as_ref().filter(|m| m.arquivos > 0) {
                Some(m) => format!("+{} −{} · divisão {cartao}", m.mais, m.menos),
                None => format!("divisão {cartao}"),
            },
            None => format!("divisão {cartao}"),
        };
        let mut linha = CartaoDeLista::new(SharedString::from(format!("distribuido-{}", criada.tarefa)), titulo)
            .simbolo(simbolo)
            .subtitulo(subtitulo)
            .meta(meta);
        if let Some(t) = tarefa.filter(|t| t.estado == crate::modelo::EstadoTarefa::EmRevisao) {
            let id = t.id;
            linha = linha.acao_fixa(
                botao_fantasma(SharedString::from(format!("distribuido-revisar-{id}")), Some(Lucide::Eye.into()), "Revisar", TamanhoBotao::Linha, window, cx)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.dialogo_diff(id, window, cx)
                    })),
            );
        }
        if chat.is_some() {
            let alvo = criada.chat.clone();
            linha = linha.on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                if ev.modifiers().control {
                    this.selecionar(Some(alvo.clone()), window, cx)
                } else {
                    let perto = this.paineis.get(this.ativo).map(|p| (p.id, Lado::Direita));
                    this.abrir_ao_lado(alvo.clone(), perto, window, cx)
                }
            }));
        }
        div().px(esp::E4).child(linha).into_any_element()
    }

    /// "Memória do Mestre": o que ele lembra (relido a cada 5 s, não a cada quadro) e Editar.
    fn memoria_no_distribuido(&self, motor: &Motor, window: &Window, cx: &Context<Self>) -> AnyElement {
        let texto = {
            let mut lida = self.memoria_lida.borrow_mut();
            let vencida = lida.as_ref().is_none_or(|(quando, _)| quando.elapsed() >= std::time::Duration::from_secs(5));
            if vencida {
                *lida = Some((std::time::Instant::now(), motor.memoria_do_mestre()));
            }
            lida.as_ref().map(|(_, t)| t.clone()).unwrap_or_default()
        };
        let editar = botao_fantasma("distribuido-memoria-editar", Some(Lucide::Pencil.into()), "Editar", TamanhoBotao::Linha, window, cx)
            .on_click(cx.listener(|this, _, window, cx| {
                this.memoria_lida.borrow_mut().take();
                this.dialogo_memoria(window, cx)
            }));
        let corpo = if texto.trim().is_empty() {
            div()
                .px(esp::E12)
                .metadado()
                .text_color(Cores::tenue())
                .child("Nada lembrado ainda. O Mestre guarda aqui as preferências que você disser (ex.: pagamentos vão para a Ninee).")
                .into_any_element()
        } else {
            div()
                .px(esp::E12)
                .corpo_ui()
                .text_color(Cores::texto())
                .child(super::mensagens::markdown(SharedString::from("distribuido-memoria"), texto, cx))
                .into_any_element()
        };
        v_flex()
            .child(CabecalhoDeSecao::new("Memória do Mestre").acao(editar))
            .child(corpo)
            .into_any_element()
    }
}

#[cfg(test)]
mod testes {
    use super::{JANELA, divisao_recente, por_projeto};
    use crate::modelo::{Cartao, Divisao, EstadoCartao, Parte, ParteCriada, TipoCartao};

    fn parte(projeto: &str, tarefa: Option<u64>, resultado: Option<&str>) -> Parte {
        Parte {
            projeto: projeto.into(),
            titulo: format!("t{tarefa:?}"),
            criada: tarefa.map(|t| ParteCriada { chat: format!("c{t}"), tarefa: t }),
            resultado: resultado.map(Into::into),
            ..Default::default()
        }
    }

    fn cartao(id: u64, estado: EstadoCartao, decidido_em: u64, partes: Vec<Parte>) -> Cartao {
        Cartao {
            id,
            chat: "mestre".into(),
            criado_em: 0,
            decidido_em,
            estado,
            tipo: TipoCartao::Divisao(Divisao { partes, ..Default::default() }),
            resto: Default::default(),
        }
    }

    #[test]
    fn distribuido_mostra_o_que_anda_e_o_que_terminou_ha_menos_de_um_dia() {
        let agora = 10 * JANELA;
        let andando = cartao(1, EstadoCartao::Aceito, 0, vec![parte("loja", Some(3), None)]);
        let terminou_ontem = cartao(2, EstadoCartao::Aceito, agora - JANELA - 1, vec![parte("loja", Some(4), Some("juntada"))]);
        let terminou_agora = cartao(3, EstadoCartao::Aceito, agora - 60, vec![parte("site", Some(5), Some("juntada"))]);
        let aberto = cartao(4, EstadoCartao::Aberto, 0, vec![parte("loja", None, None)]);
        assert!(divisao_recente(&andando, agora));
        assert!(!divisao_recente(&terminou_ontem, agora));
        assert!(divisao_recente(&terminou_agora, agora));
        assert!(!divisao_recente(&aberto, agora));
        // Por projeto, na ordem em que aparecem, só as partes que começaram.
        let misto = cartao(5, EstadoCartao::Aceito, agora, vec![parte("site", Some(6), None), parte("loja", None, None), parte("loja", Some(7), None)]);
        let grupos = por_projeto(&[&andando, &misto]);
        let resumo: Vec<(String, Vec<u64>)> =
            grupos.into_iter().map(|(p, partes)| (p, partes.iter().map(|(_, p)| p.criada.as_ref().unwrap().tarefa).collect())).collect();
        assert_eq!(resumo, vec![("loja".into(), vec![3, 7]), ("site".into(), vec![6])]);
    }
}
