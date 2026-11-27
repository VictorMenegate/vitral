//! A revisão do turno pelo outro agente, na conversa (`motor/revisao_do_par.rs`): o cartão com o
//! que ele achou e a ação de mandá-lo ao agente do chat, e a ação "Revisar com o Codex" da linha
//! "Trabalhou…" do último turno pronto. Fica num módulo filho da conversa para mexer o mínimo
//! nela: só o braço do `Item::Revisao` e o gancho da linha do turno moram lá.

use super::*;

/// O rótulo da ação do cartão e o de depois de mandada.
pub(super) fn rotulo_de_mandar(principal: Agente, mandada: bool) -> String {
    let nome = principal.nome();
    if mandada { crate::tr!("Mandada ao {nome}", nome = nome) } else { crate::tr!("Mandar ao {nome}", nome = nome) }.to_string()
}

/// O turno é o último da conversa: depois da resposta dele não há outra mensagem.
pub(super) fn ultimo_turno(itens: &[Item], turno: TurnoRecolhivel) -> bool {
    !itens.get(turno.resposta + 1..).unwrap_or_default().iter().any(|i| matches!(i, Item::Usuario { .. } | Item::Entrega { .. }))
}

impl Janela {
    /// O cartão da revisão: quem revisou, o texto dele e "Mandar ao Claude" (nada vai sozinho). No
    /// desenho do plano respondido (o fundo de cartão, o cabeçalho de 36), sem recolher: é para ler.
    pub(super) fn cartao_da_revisao(
        &self,
        contexto: &Contexto,
        indice: usize,
        de: Agente,
        texto: &str,
        mandada: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let chave = format!("{}:{indice}", contexto.chat.id);
        let realce = contexto.busca.realce(indice, Campo::Texto);
        let marca = Marca::do_agente(de);
        let principal = de.outro();
        let cabecalho = h_flex()
            .h(r(36.))
            .px(esp::E12)
            .gap(esp::E8)
            .child(marca.disco(16.))
            .child(div().flex_1().min_w_0().truncate().rotulo().text_color(Cores::forte()).child(crate::tr!("Revisão do {nome}", nome = de.nome())))
            .child(BotaoCopiar::new(SharedString::from(format!("copiar-revisao-{chave}")), texto.to_string()).dica(crate::tr!("Copiar a revisão")));
        let rodape = if mandada {
            div().metadado().text_color(Cores::tenue()).child(rotulo_de_mandar(principal, true)).into_any_element()
        } else {
            let (motor, chat) = (self.motor.clone(), contexto.chat.id.clone());
            botao_fantasma(
                SharedString::from(format!("mandar-revisao-{chave}")),
                Some(Lucide::Send.into()),
                rotulo_de_mandar(principal, false),
                TamanhoBotao::Barra,
                window,
                cx,
            )
            .tooltip(crate::tr!("A revisão vai ao {nome} como uma mensagem deste chat", nome = principal.nome()))
            .on_click(move |_, _, cx| {
                if let Some(m) = &motor {
                    m.update(cx, |m, cx| m.mandar_revisao(&chat, indice, cx));
                }
            })
            .into_any_element()
        };
        let cartao = v_flex()
            .w_full()
            .rounded(raio::CARTAO)
            .bg(Cores::hover())
            .child(cabecalho)
            .child(
                v_flex()
                    .px(esp::E12)
                    .pb(esp::E12)
                    .gap(esp::E8)
                    .child(div().corpo().text_color(Cores::texto()).child(markdown_achado(
                        contexto,
                        format!("{chave}-revisao").into(),
                        texto.to_string(),
                        realce.clone(),
                    )))
                    .child(h_flex().child(rodape)),
            )
            .into_any_element();
        RolarAoAchado::novo(format!("rolar-{chave}").into(), &realce, contexto.rolagem, cartao).into_any_element()
    }

    /// "Revisar", com a IA da revisão, na ponta da linha "Trabalhou…" do último turno
    /// pronto: só quando o motor diz que dá (`Motor::pode_revisar`: chat parado, turno ainda não
    /// revisto, o outro agente à mão e sem revisão rodando). Discreta, no tom da linha; no painel
    /// estreito, só o ícone, com a dica.
    pub(super) fn acao_de_revisar(&self, contexto: &Contexto, turno: TurnoRecolhivel, cx: &Context<Self>) -> Option<AnyElement> {
        if !ultimo_turno(&contexto.chat.itens, turno) {
            return None;
        }
        let revisor = contexto.motor.pode_revisar(&contexto.chat.id)?;
        // Só "Revisar": quem revisa é a IA escolhida em Configurações › Revisão, e o nome dela vai
        // na dica (dono, 27/11/2026: "esse revisar com o Codex tinha que ser só revisar e funcionar
        // com a IA que tá configurada").
        let rotulo = crate::tr!("Revisar");
        let so_icone = matches!(contexto.nivel, LarguraPainel::Minimo | LarguraPainel::Estreito);
        let dica = crate::tr!("Revisar com o {nome}\nEle só lê o que este turno fez e diz o que achou; nada muda sozinho", nome = revisor.nome());
        let (motor, chat) = (self.motor.clone(), contexto.chat.id.clone());
        let _ = cx;
        let dica = dica.clone();
        Some(
            h_flex()
                .id(SharedString::from(format!("revisar-{}", turno.chave(&contexto.chat.id))))
                .flex_none()
                .h(ALTURA_DO_LOG)
                .px(esp::E8)
                .gap(esp::E6)
                .rounded(raio::CONTROLE)
                .metadado()
                .text_color(Cores::tenue())
                .cursor_pointer()
                .hover(|s| s.bg(Cores::hover()).text_color(Cores::texto()))
                .tab_index(0)
                .focus_visible(|s| s.shadow(halo_foco()))
                .child(icone_de(Lucide::TextSearch, 14.))
                .when(!so_icone, |s| s.child(rotulo))
                .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                .on_click(move |_, _, cx| {
                    if let Some(m) = &motor {
                        m.update(cx, |m, cx| {
                            m.revisar_com_o_outro(&chat, cx);
                        });
                    }
                })
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod testes {
    // Sem o `use super::*`: ele traria o `test` do GPUI no lugar do da biblioteca padrão.
    use super::{rotulo_de_mandar, ultimo_turno};
    use crate::modelo::{Agente, Item};
    use crate::ui::mensagens::TurnoRecolhivel;

    /// A ação do cartão diz a quem a revisão vai, e depois que foi.
    #[test]
    fn o_rotulo_de_mandar_a_revisao() {
        assert_eq!(rotulo_de_mandar(Agente::Claude, false), "Mandar ao Claude");
        assert_eq!(rotulo_de_mandar(Agente::Codex, false), "Mandar ao Codex");
        assert_eq!(rotulo_de_mandar(Agente::Claude, true), "Mandada ao Claude");
    }

    /// "Revisar com o Codex" só no último turno da conversa: o cartão da revisão e os avisos
    /// depois da resposta não contam como outro turno; a sua mensagem seguinte, sim.
    #[test]
    fn a_acao_de_revisar_so_no_ultimo_turno() {
        let usuario = |t: &str| Item::Usuario { texto: t.into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0, marca: None };
        let fala = |t: &str| Item::Assistente { texto: t.into(), em: 0 };
        let itens = vec![usuario("a"), fala("vou"), fala("pronto"), usuario("b"), fala("vou"), fala("feito"), Item::Aviso { texto: "x".into(), erro: false, em: 0 }];
        assert!(!ultimo_turno(&itens, TurnoRecolhivel { mensagem: 0, resposta: 2, ao_vivo: false }));
        assert!(ultimo_turno(&itens, TurnoRecolhivel { mensagem: 3, resposta: 5, ao_vivo: false }));
    }
}
