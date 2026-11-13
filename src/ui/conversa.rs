//! A casca de um painel de conversa: o cabeçalho de uma linha (a aba, as ações do painel e os
//! botões da janela), o menu "⋯", o popover de uso, a tela sem conversa e a soltura de arquivos.
//! As mensagens ficam em `mensagens.rs` e a caixa em `composer.rs`.

use gpui_kit::base::actions::SelectDown;
use gpui_kit::component::{
    ElementExt as _, Selectable, h_flex,
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    popover::Popover,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    AlternarPainel, Gaveta, IrParaPainel1, IrParaPainel2, IrParaPainel3, IrParaPainel4, Janela,
    NovaConversa,
    componentes::{
        Aba, Anel, CabecalhoPopover, CorpoPopover, EstadoVazio, LarguraPainel, LarguraPopover,
        PreviaArrasto, Reflexo as _, Simbolo, TamanhoBotao, TomAba,
        botao_fantasma, botao_icone, botao_primario, cor_da_faixa,
        formatar_custo,
        metal::{Encaixe, abertura_lisa},
        icone_de_estado, sinal_do_chat,
    },
    busca_conversa, composer, menus,
    dialogos::com_til,
    movimento::{self, Curva, De, com_estado, entrada},
    paineis::{ArrastoChat, FormaSoltura, Soltura, motivo_sem_lugar},
    sem_arrastar,
};
use crate::{
    contas,
    modelo::Chat,
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, altura, esp, r, tam},
};

/// O símbolo da conversa: o balão, o mesmo em todo chat.
fn simbolo_da_conversa() -> Simbolo {
    Simbolo::icone_cor(Lucide::MessageCircle, Cores::tenue())
}

/// O ícone da aba: o estado do chat (anel girando, disco âmbar, azul ou vermelho, balão apagado
/// quando parado e lido), com a pasta na dica.
fn simbolo_na_aba(motor: &Motor, chat: &Chat, painel_id: u64) -> AnyElement {
    let dica: SharedString = if motor.sem_pasta(chat) {
        "Sem pasta: um chat numa pasta vazia só dele".into()
    } else if chat.importada.is_some() {
        format!("Conversa do Claude Code, direto na pasta dela\n{}", com_til(&chat.cwd)).into()
    } else {
        format!("Direto em {}", com_til(&chat.cwd)).into()
    };
    div()
        .id(("simbolo-aba", painel_id as usize))
        .flex_none()
        .child(icone_de_estado(sinal_do_chat(motor, chat), 16.))
        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        .into_any_element()
}

/// Alt+1…4 de cada painel (para a dica da aba mostrar a tecla do keymap).
fn acao_do_painel(ix: usize) -> Option<Box<dyn Action>> {
    Some(match ix {
        0 => Box::new(IrParaPainel1),
        1 => Box::new(IrParaPainel2),
        2 => Box::new(IrParaPainel3),
        3 => Box::new(IrParaPainel4),
        _ => return None,
    })
}

/// O modelo em uso ("Opus 5.5") e, quando a escolha da caixa é outra, o nome dela.
fn modelo_da_conversa(chat: &Chat) -> (String, Option<String>) {
    let escolha = chat.modelo.clone().unwrap_or_default();
    let rotulo_escolha = composer::MODELOS
        .iter()
        .find(|(valor, _)| *valor == escolha)
        .map(|(_, nome)| nome.to_string())
        .unwrap_or_else(|| composer::nome_curto_modelo(&escolha));
    match chat.modelo_atual.as_deref().filter(|m| !m.is_empty()) {
        Some(atual) => {
            let curto = composer::nome_curto_modelo(atual);
            let outro = !escolha.is_empty()
                && curto != rotulo_escolha
                && !curto.to_lowercase().starts_with(&escolha.to_lowercase());
            (curto, outro.then_some(rotulo_escolha))
        }
        None => (rotulo_escolha, None),
    }
}

/// Onde a conversa está, em texto, para a dica da aba: a pasta ("~/Projetos/loja") ou "Sem pasta".
fn pasta_em_texto(motor: &Motor, chat: &Chat) -> String {
    if motor.sem_pasta(chat) { "Sem pasta".into() } else { com_til(&chat.cwd) }
}

/// Clique de item de menu que mexe na janela.
fn na_janela(
    janela: &WeakEntity<Janela>,
    f: impl Fn(&mut Janela, &mut Window, &mut Context<Janela>) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let janela = janela.clone();
    move |_, window, cx| {
        let _ = janela.update(cx, |this, cx| f(this, window, cx));
    }
}

/// Item de menu que pode estar desligado: aí ganha uma segunda linha com o motivo.
fn item_com_motivo(titulo: &'static str, icone: Lucide, motivo: Option<String>) -> PopupMenuItem {
    match motivo {
        None => PopupMenuItem::new(titulo).icon(icone),
        Some(motivo) => PopupMenuItem::element(move |_, _| {
            v_flex()
                .min_w_0()
                .py(r(4.))
                .child(div().truncate().child(titulo))
                .child(div().truncate().metadado().text_color(Cores::tenue()).child(motivo.clone()))
        })
        .icon(icone)
        .disabled(true),
    }
}

/// O menu do painel, o mesmo no "⋯" e no clique direito da aba: o que saiu do cabeçalho na
/// largura estreita, abrir ao lado, mover e fechar, e o menu da conversa.
fn menu_do_painel(
    menu: PopupMenu,
    janela: WeakEntity<Janela>,
    motor: Entity<Motor>,
    painel_id: u64,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    struct Dados {
        chat: String,
        foco: bool,
        ultimo: bool,
        direita_aberta: bool,
        /// O que a direita mostra (o arquivo aberto ou Este chat).
        direita: Direita,
        cheio: Option<String>,
        rotulo: Option<String>,
    }
    let dados = (|| {
        let j = janela.upgrade()?;
        let j = j.read(cx);
        let ix = j.paineis.iter().position(|p| p.id == painel_id)?;
        let painel = &j.paineis[ix];
        let m = motor.read(cx);
        let chat = m.chat(painel.chat.as_deref()?)?;
        let nivel = painel.medida.nivel;
        let estreito = matches!(nivel, LarguraPainel::Estreito | LarguraPainel::Minimo);
        let rotulo = estreito.then(|| {
            let modelo = modelo_da_conversa(chat).0;
            if chat.custo_usd > 0.0 {
                format!("{} nesta conversa · {modelo}", formatar_custo(chat.custo_usd))
            } else {
                modelo
            }
        });
        let cheio = (!j.cabe_outro_painel()).then(|| motivo_sem_lugar(j.paineis.len()));
        Some(Dados {
            chat: chat.id.clone(),
            foco: painel.modo_foco,
            ultimo: ix + 1 == j.paineis.len(),
            direita_aberta: j.direita_visivel() || j.gaveta == Some(Gaveta::Direita),
            direita: j.o_que_a_direita_mostra(m),
            cheio,
            rotulo,
        })
    })();
    let Some(d) = dados else {
        return menu;
    };
    let mut menu = menu.min_w(tam(window, 300.)).max_w(tam(window, 360.));
    if let Some(rotulo) = d.rotulo {
        menu = menu.label(rotulo).separator();
    }
    menu = menu
        .item(
            PopupMenuItem::new("Modo foco")
                .icon(Lucide::Focus)
                .checked(d.foco)
                .on_click(na_janela(&janela, move |this, _, cx| {
                    if let Some(p) = this.paineis.iter_mut().find(|p| p.id == painel_id) {
                        p.modo_foco = !p.modo_foco;
                    }
                    cx.notify();
                })),
        );
    if d.ultimo {
        // O botão da direita saiu do cabeçalho (pedido do dono, 12/11/2026): abrir e fechar o
        // painel fica aqui.
        menu = menu.item(
            PopupMenuItem::new(d.direita.no_menu())
            .icon(Lucide::PanelRight)
            .checked(d.direita_aberta)
            .action(Box::new(AlternarPainel))
            .on_click(na_janela(&janela, |this, window, cx| this.alternar_painel(window, cx))),
        );
    }
    menu = menu
        .separator()
        .item(
            item_com_motivo("Nova conversa ao lado", Lucide::Columns2, d.cheio)
                .on_click(na_janela(&janela, |this, window, cx| this.nova_conversa_ao_lado(window, cx))),
        )
        .separator();
    // Mover o painel, fechar os outros e fechar este saíram do menu a pedido do dono
    // (13/11/2026): as abas se arrastam, o × fecha, e os atalhos continuam valendo
    // (Ctrl+Shift+PgUp/PgDn e Ctrl+W).
    menu = menus::menu_conversa(menu, janela.clone(), &motor, &d.chat, false, window, cx);
    menu
}

/// O popover do anel do contexto, na caixa: no cabeçalho o contexto e o custo ("Contexto 31% ·
/// US$ 1,84"); embaixo o modelo, a conta e os tokens; no rodapé a nota da estimativa e `acao` (o
/// Compactar).
pub(super) fn uso_da_conversa(motor: &Motor, chat: &Chat, acao: AnyElement) -> CorpoPopover {
    let linha = |rotulo: &'static str, valor: AnyElement| {
        h_flex()
            .h(r(28.))
            .gap(esp::E8)
            .child(div().flex_none().w(r(104.)).metadado().text_color(Cores::tenue()).child(rotulo))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(esp::E6)
                    .text_size(r(13.))
                    .line_height(r(18.))
                    .text_color(Cores::texto())
                    .child(valor),
            )
    };
    let texto = |t: String| div().truncate().child(t).into_any_element();
    let (modelo, escolhido) = modelo_da_conversa(chat);
    let modelo = h_flex()
        .min_w_0()
        .child(div().flex_none().child(modelo))
        .when_some(escolhido, |s, e| {
            s.child(div().truncate().text_color(Cores::tenue()).child(format!(" · escolhido: {e}")))
        })
        .into_any_element();
    let escolha = motor.escolha_de_conta(chat);
    let conta = if escolha == contas::AUTOMATICA {
        "Automática".to_string()
    } else {
        motor
            .contas
            .iter()
            .find(|c| c.id == escolha)
            .map(|c| c.nome.clone())
            .unwrap_or_else(|| "Principal".into())
    };
    let contexto = chat
        .contexto
        .map(|usados| (usados, composer::uso_do_contexto(usados, chat.janela, chat.modelo_atual.as_deref())));
    let custo = formatar_custo(chat.custo_usd);
    let (simbolo, valor) = match contexto {
        Some((_, (fracao, numero, _))) => (
            Some(Simbolo::Anel { fracao, cor: Some(cor_da_faixa(fracao, Cores::destaque())) }),
            format!("{numero}% · {custo}"),
        ),
        None => (None, custo),
    };
    let tokens = contexto.map(|(usados, (fracao, _, janela))| {
        h_flex()
            .gap(esp::E6)
            .child(Anel::new("uso-contexto", fracao).diametro(20.).cor(cor_da_faixa(fracao, Cores::destaque())).encher())
            .child(div().truncate().child(format!(
                "{} de {}",
                composer::milhares(usados),
                composer::milhares(janela)
            )))
            .into_any_element()
    });
    CorpoPopover::new(LarguraPopover::P320)
        .cabecalho(CabecalhoPopover::Escolha {
            simbolo,
            titulo: "Contexto".into(),
            valor: Some((valor.into(), Cores::forte())),
            dica: None,
        })
        .child(
            v_flex()
                .child(linha("Modelo", modelo))
                .child(linha("Conta", texto(format!("Claude · {conta}"))))
                .when_some(tokens, |s, t| s.child(linha("Tokens", t))),
        )
        .rodape(
            h_flex()
                .gap(esp::E8)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .metadado()
                        .text_color(Cores::tenue())
                        .child("Custo estimado pelo preço da API; no plano, o que conta é o limite."),
                )
                .child(div().flex_none().child(acao)),
        )
}

/// Quanto as ações do cabeçalho (foco, "⋯", painel da direita) ficam apagadas enquanto o
/// ponteiro não chega nelas. Acende inteiro no hover do grupo, e o teclado não passa por aqui.
const ACOES_APAGADAS: f32 = 0.55;

/// O grupo de hover das ações de um painel.
fn grupo_das_acoes(painel_id: u64) -> SharedString {
    SharedString::from(format!("acoes-cabecalho-{painel_id}"))
}

impl Janela {
    /// Um painel de conversa (`ix` em `self.paineis`); sem painel, a tela de começar.
    pub(super) fn conversa(
        &mut self,
        motor_entidade: &Entity<Motor>,
        ix: Option<usize>,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let motor = motor_entidade.read(cx);
        let painel_id = ix.map(|ix| self.paineis[ix].id).unwrap_or(0);
        let chat = ix
            .and_then(|ix| self.paineis[ix].chat.clone())
            .and_then(|id| motor.chat(&id));
        let ativo = ix == Some(self.ativo);
        let id_texto = |nome: &str| SharedString::from(format!("{nome}-{painel_id}"));

        let cabecalho = self.cabecalho_painel(motor_entidade, motor, chat, ix, window, cx);
        // Buscar na conversa (Ctrl+F): o índice é refeito aqui, uma vez por quadro e só quando o
        // termo, a conversa ou o modo foco mudaram (ver `busca_conversa::Indice::atualizar`). A
        // barra vai dentro das mensagens, por cima delas.
        if let (Some(chat), Some(ix)) = (chat, ix) {
            let termo = busca_conversa::normalizar(&self.paineis[ix].busca.campo.read(cx).value());
            let foco = self.paineis[ix].modo_foco;
            self.paineis[ix].busca.indice.atualizar(&chat.id, &chat.itens, foco, &termo);
        }
        let barra_busca = ix.and_then(|ix| self.barra_de_busca(ix, window, cx));
        let corpo = match (&chat, ix) {
            (Some(chat), Some(ix)) => self.mensagens(motor, chat, ix, barra_busca, window, cx).into_any_element(),
            _ => self.sem_conversa(window, cx).into_any_element(),
        };

        let arquivos_sobre = chat.is_some() && self.arquivos_sobre == Some(painel_id);
        let medida = ix.map(|ix| Self::medir_painel(cx.entity().downgrade(), painel_id, self.paineis[ix].medida));
        // A faixa do turno entra com fade e 8 px de baixo no quadro em que aparece; o painel que
        // acabou de abrir ao lado, só com o fade.
        let faixa = chat.and_then(|c| self.faixa_atividade(motor, c, painel_id, window, cx));
        let (faixa_nova, painel_novo) = match ix {
            Some(ix) => {
                let p = &mut self.paineis[ix];
                let nova = faixa.is_some() && !p.faixa_vista;
                p.faixa_vista = faixa.is_some();
                (nova, std::mem::take(&mut p.entra))
            }
            None => (false, false),
        };
        let faixa = faixa.map(|f| entrada(("entra-faixa", painel_id as usize), faixa_nova, De::Baixo, div().child(f)));
        // O pé de toda conversa é a doca: uma abertura no cartão por onde se vê o metal, com a
        // caixa de mensagem pousada nela. Alta na conversa ainda vazia, rente à caixa no resto do
        // tempo (com o chat trabalhando, quem mostra é o metal, que corre).
        let vazia = chat.is_none_or(|c| c.itens.is_empty());
        // Fora da conversa vazia a doca não tem folga em cima: a faixa "N arquivos mudados" fica
        // colada na caixa (com 24 de folga ela parecia solta, longe).
        let topo_da_doca: f32 = if vazia {
            if window.viewport_size().height >= px(640.) { 172. } else { 56. }
        } else {
            4.
        };
        let cartao = v_flex()
            .id(id_texto("cartao-conversa"))
            .relative()
            .size_full()
            .min_w_0()
            .rounded(crate::tema::raio_da_peca())
            .bg(Cores::cartao())
            // O cartão flutua sobre a base. Com os fundos transparentes a sombra sai: debaixo de
            // um cartão transparente ela só o escurecia (é pintada também por baixo da peça).
            .when(crate::tema::transparencia() == 0, |s| s.shadow(crate::tema::sombra_flutuante()))
            .overflow_hidden()
            // O cartão é meio transparente em cima e fecha até ficar opaco no pé, onde mora a
            // caixa de mensagem: 200 de degradê e 200 cheios (por baixo do conteúdo).
            // Só com os fundos transparentes (no opaco não há o que fechar). O pedaço cheio leva os
            // cantos de baixo do cartão: o `overflow_hidden` corta em retângulo, e sem isso o
            // fundo sobrava quadrado para fora da curva (dono, 13/11/2026).
            .when(crate::tema::transparencia() > 0, |s| {
                s.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(400.))
                        .child(div().absolute().top_0().left_0().right_0().h(px(200.)).bg(crate::tema::gradiente_do_pe_do_cartao()))
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(200.))
                                .rounded_b(crate::tema::raio_da_peca())
                                .bg(Cores::cartao_opaco()),
                        ),
                )
            })
            // Na captura, antes dos `sem_arrastar` do cabeçalho pararem o clique: qualquer clique
            // no cartão, até na aba, ativa o painel.
            .capture_any_mouse_down(
                cx.listener(move |this, _, _, cx| this.clique_no_painel(painel_id, cx)),
            )
            .when_some(medida, |s, medir| s.on_prepaint(medir))
            .when_some(chat.map(|c| c.id.clone()), |s, chat_id| {
                // Arquivos arrastados do gerenciador viram anexos da próxima mensagem.
                s.on_drag_move(cx.listener(
                    move |this, evento: &DragMoveEvent<ExternalPaths>, _, cx| {
                        let dentro = evento.bounds.contains(&evento.event.position);
                        if dentro && this.arquivos_sobre != Some(painel_id) {
                            this.arquivos_sobre = Some(painel_id);
                            cx.notify();
                        } else if !dentro && this.arquivos_sobre == Some(painel_id) {
                            this.arquivos_sobre = None;
                            cx.notify();
                        }
                    },
                ))
                .on_drop(cx.listener(move |this, caminhos: &ExternalPaths, window, cx| {
                    this.arquivos_sobre = None;
                    // Foco na caixa deste painel, para escrever a mensagem dos anexos.
                    if let Some(ix) = this.paineis.iter().position(|p| p.id == painel_id) {
                        this.focar_painel(ix, window, cx);
                    }
                    let lista = this.anexos.entry(chat_id.clone()).or_default();
                    for caminho in caminhos.paths() {
                        if !lista.contains(caminho) {
                            lista.push(caminho.clone());
                        }
                    }
                    cx.notify();
                }))
            })
            .child(cabecalho)
            .child(corpo)
            .children(faixa)
            .when_some(chat.zip(ix), |s, (chat, ix)| {
                let caixa = self.caixa_mensagem(motor, chat, ix, window, cx).into_any_element();
                // A doca está no fluxo: as mensagens terminam onde ela começa, e nenhum texto fica
                // por cima do metal. O `overflow_hidden` do cartão corta reto, então a abertura leva
                // o raio dos cantos de baixo.
                let raio_doca = crate::tema::raio_da_peca();
                let doca = com_estado(move |window, cx| {
                    let topo = movimento::valor(
                        ("doca", painel_id as usize),
                        topo_da_doca,
                        movimento::ENCHER,
                        Curva::Mover,
                        window,
                        cx,
                    );
                    // A luz de baixo é a marca do painel SELECIONADO (pedido do dono, 12/11/2026):
                    // só o painel ativo a tem; nos outros o pé fica escuro. Acende em 180 ms e
                    // apaga em 120.
                    let (alvo, duracao) = if ativo { (1f32, movimento::MOVER) } else { (0f32, movimento::COR) };
                    let luz = movimento::valor(("doca-ativa", painel_id as usize), alvo, duracao, Curva::Mover, window, cx);
                    div()
                        .relative()
                        .flex_none()
                        .w_full()
                        .pt(r(topo))
                        .when(luz > 0., |s| {
                            s.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .size_full()
                                    .opacity(luz)
                                    // O campo INTEIRO do metal encolhido para cobrir a doca (as
                                    // bolhas clara, azul e escura lado a lado), com um pouco do
                                    // azul do destaque por cima (a bolha clara fica azul-céu, não
                                    // cinza) e o grão leve.
                                    .child(abertura_lisa(
                                        Encaixe::Cobrir,
                                        Some(Cores::destaque().opacity(0.2)),
                                        Corners { bottom_left: raio_doca, bottom_right: raio_doca, ..Default::default() },
                                    ))
                                    // O escuro do cartão some devagar, da beirada de cima até o pé
                                    // (duas camadas de degradê dão a curva): o metal é uma luz que
                                    // vem de baixo, não uma faixa. Foi o desenho que o dono aprovou
                                    // (12/11/2026), depois de reprovar a faixa cheia.
                                    .child(div().absolute().top_0().left_0().size_full().bg(crate::tema::gradiente_da_doca()))
                                    .child(
                                        div()
                                            .absolute()
                                            .top_0()
                                            .left_0()
                                            .right_0()
                                            .h(relative(0.5))
                                            .bg(crate::tema::gradiente_da_doca()),
                                    )
                                    // Os lados também somem (dono, 13/11/2026: "menos nos lados
                                    // pra ficar mais no meio só"): de cada lado uma camada larga e
                                    // uma estreita, para a luz morrer em curva antes da beirada.
                                    .children([false, true].into_iter().flat_map(|direita| {
                                        [0.44, 0.2].map(|largura| {
                                            let lado = div()
                                                .absolute()
                                                .top_0()
                                                .bottom_0()
                                                .w(relative(largura))
                                                .bg(crate::tema::gradiente_do_lado_da_doca(direita));
                                            if direita {
                                                lado.right_0().rounded_br(raio_doca)
                                            } else {
                                                lado.left_0().rounded_bl(raio_doca)
                                            }
                                        })
                                    })),
                            )
                        })
                        .child(caixa)
                        .into_any_element()
                });
                // Com um trecho da conversa selecionado, o painel ativo oferece citá-lo na caixa
                // (dono, 13/11/2026: "selecionar uma parte da conversa pra responder").
                let responder = (ativo && self.trecho.is_some()).then(|| {
                    div().map(super::mensagens::coluna).flex().justify_end().pb(esp::E6).child(
                        h_flex()
                            .id(("responder-trecho", painel_id as usize))
                            .flex_none()
                            .h(r(28.))
                            .px(esp::E12)
                            .gap(esp::E6)
                            .rounded(crate::tema::raio::de(28.))
                            // Uma tecla azul em relevo: o azul do destaque bem leve, o fio do
                            // mesmo azul e a luz em cima.
                            .bg(Cores::tinta_solida(Cores::destaque(), 0.16))
                            .border_1()
                            .border_color(Cores::tinta_borda(Cores::destaque()))
                            .shadow([crate::tema::aresta_de_tecla(), crate::tema::sombra_elevado()].concat())
                            .metadado()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(Cores::forte())
                            .cursor_pointer()
                            .hover(|s| s.bg(Cores::tinta_solida(Cores::destaque(), 0.26)))
                            .child(super::mensagens::icone_de(Lucide::MessageSquareQuote, 13.).text_color(Cores::destaque_texto()))
                            .child("Responder a este trecho")
                            // No apertar, e não no clique: o apertar fora do texto desfaz a
                            // seleção, e o botão sumiria antes de o clique chegar.
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    this.responder_ao_trecho(painel_id, window, cx);
                                    cx.stop_propagation();
                                }),
                            ),
                    )
                });
                s.child(self.permissoes(motor, chat, window, cx)).children(responder).child(doca)
            })
            .when(arquivos_sobre, |s| {
                s.child(FormaSoltura::new(id_texto("soltura-arquivos"), Soltura::Arquivos))
            })
            // O vidro da peça (fio, luz na quina de cima, sombra na base), por cima de tudo.
            .aresta_de_vidro(crate::tema::raio_da_peca());
        // O painel ativo é marcado pela luz de baixo da doca (acima), não por borda nem cantoneira.
        // O que acabou de abrir ao lado entra só com o fade: a largura não anima.
        com_estado(move |window, cx| {
            let opacidade = movimento::valor_desde(
                ("entra-painel", painel_id as usize),
                if painel_novo { 0. } else { 1. },
                1f32,
                movimento::ENTRAR,
                Curva::Entrar,
                window,
                cx,
            );
            cartao.when(opacidade < 1., |s| s.opacity(opacidade)).into_any_element()
        })
    }

    /// O cabeçalho numa linha só (44), que arrasta a janela: a aba, as ações e os botões da janela.
    /// Com a lista escondida, o 1º painel começa pelo logo compacto e, no modo recolhido, os anéis
    /// de uso. A borda de baixo só aparece com mensagens roladas por baixo dele.
    fn cabecalho_painel(
        &self,
        motor_entidade: &Entity<Motor>,
        motor: &Motor,
        chat: Option<&Chat>,
        ix: Option<usize>,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let painel_id = ix.map(|ix| self.paineis[ix].id).unwrap_or(0);
        let nivel = ix.map(|ix| self.paineis[ix].medida.nivel).unwrap_or_default();
        // Os botões da janela ficam no canto de cima à direita: no último painel, se o painel da
        // direita estiver fechado ou em gaveta.
        let ultimo = ix.is_none_or(|ix| ix + 1 == self.paineis.len());
        let mostra_botoes_janela = ultimo && !self.direita_visivel();
        let conversa = chat.zip(ix);
        let primeiro = ix.unwrap_or(0) == 0;
        self.arrastavel_com_id(SharedString::from(format!("arrastar-conversa-{painel_id}")))
            .flex_none()
            .flex()
            .items_center()
            .h(altura::CABECALHO)
            .pl(esp::E8)
            .pr(esp::E8)
            .gap(esp::E8)
            // Sem a linha embaixo do cabeçalho com a conversa rolada (pedido do dono, 12/11/2026).
            // O recolher da lista mora no menu do logo (pedido do dono, 12/11/2026).
            // Com a lista fora da tela, o primeiro painel começa pelo logo compacto.
            .when(!self.orcamento.lista && primeiro, |s| {
                s.child(self.logo_compacto(if ix.is_some() { "mostrar-lista" } else { "mostrar-lista-vazio" }, window, cx))
            })
            .map(|s| {
                // As ações do painel são segundo plano: ficam apagadas enquanto o ponteiro não
                // chega perto, para o cabeçalho ser o título da conversa, e não uma fileira de
                // ícones.
                let acoes = h_flex().flex_none().gap(esp::E8).group(grupo_das_acoes(painel_id)).child(
                    h_flex()
                        .flex_none()
                        .gap(esp::E8)
                        .opacity(ACOES_APAGADAS)
                        .group_hover(grupo_das_acoes(painel_id), |s| s.opacity(1.))
                        .children(conversa.map(|(_, ix)| self.acoes_do_painel(motor_entidade, ix, nivel, window, cx))),
                );
                let abas = conversa.map(|(chat, ix)| self.abas_do_painel(motor_entidade, motor, chat, ix, nivel, window, cx));
                // Cada aba é uma peça; o vão depois delas continua arrastando a janela.
                s.child(div().flex().flex_1().min_w_0().items_center().gap(esp::E6).children(abas.into_iter().flatten()))
                    .child(acoes)
                    .when(mostra_botoes_janela, |s| s.child(self.botoes_janela(cx)))
            })
    }

    /// As abas do painel, na ordem em que abriram: a do chat à vista (a aba completa, abaixo) e as
    /// outras, compactas, que trocam o chat do painel no clique e fecham no × ou no clique do meio.
    #[allow(clippy::too_many_arguments)]
    fn abas_do_painel(
        &self,
        motor_entidade: &Entity<Motor>,
        motor: &Motor,
        chat: &Chat,
        ix: usize,
        nivel: LarguraPainel,
        window: &Window,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let painel = &self.paineis[ix];
        let painel_id = painel.id;
        // Na largura mínima só cabe a aba à vista.
        if nivel == LarguraPainel::Minimo || painel.abas.len() <= 1 {
            return vec![self.aba_a_vista(motor_entidade, motor, chat, ix, window, cx)];
        }
        painel
            .abas
            .iter()
            .filter_map(|id| {
                if *id == chat.id {
                    // Com outras abas ao lado, a que está à vista para em 280 de largura, para as
                    // outras ficarem com um pedaço do título.
                    let a_vista = self.aba_a_vista(motor_entidade, motor, chat, ix, window, cx);
                    return Some(div().flex().min_w_0().max_w(r(280.)).child(a_vista).into_any_element());
                }
                let outro = motor.chat(id)?;
                let fechar = cx.entity().downgrade();
                let (id_abrir, id_fechar) = (id.clone(), id.clone());
                let dica = SharedString::from(format!("{}\n{}", outro.titulo, pasta_em_texto(motor, outro)));
                // Toda aba arrasta: para outro painel (vira aba dele) ou para a borda de um painel
                // (vira um painel novo daquele lado).
                let arrasto = ArrastoChat { chat: id.clone(), titulo: outro.titulo.clone().into(), de_painel: Some(painel_id) };
                let previa = simbolo_da_conversa();
                let aba = Aba::new(SharedString::from(format!("aba-{painel_id}-{id}")), outro.titulo.clone())
                    // As outras abas do painel são peças lisas, só com o contorno.
                    .tom(TomAba::Inativa)
                    // O ícone da aba é o estado do chat (e o ponto ao lado do × sai).
                    .simbolo(Simbolo::Elemento(icone_de_estado(sinal_do_chat(motor, outro), 16.).into_any_element()))
                    .fechar("Fechar a aba (a conversa continua na lista)", false, move |window, cx| {
                        let _ = fechar.update(cx, |this, cx| this.fechar_aba(painel_id, &id_fechar, window, cx));
                    })
                    .construir(window, cx)
                    .max_w(r(180.))
                    .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                    .on_click(cx.listener(move |this, _, window, cx| this.abrir_aba(painel_id, id_abrir.clone(), window, cx)))
                    .on_drag(arrasto, move |arrasto, _, _, cx| {
                        let simbolo = previa.copia().unwrap_or_else(simbolo_da_conversa);
                        cx.new(|_| PreviaArrasto { titulo: arrasto.titulo.clone(), simbolo })
                    });
                // As outras abas cedem o espaço antes da que está à vista, até sobrar o símbolo: só
                // então ela encolhe (com o peso igual, "WMS" virava "W…" ao lado de títulos longos).
                Some(sem_arrastar().flex().min_w(r(40.)).flex_shrink(1000.).child(aba).into_any_element())
            })
            .collect()
    }

    /// A aba do chat à vista, pronta para o cabeçalho: a do painel com o foco leva a casca preta.
    fn aba_a_vista(&self, motor_entidade: &Entity<Motor>, motor: &Motor, chat: &Chat, ix: usize, window: &Window, cx: &Context<Self>) -> AnyElement {
        let aba = self.aba_da_conversa(motor_entidade, motor, chat, ix, window, cx);
        let com_foco = self.paineis.len() <= 1 || ix == self.ativo;
        if !com_foco {
            return aba;
        }
        super::componentes::casca_da_aba().child(aba).into_any_element()
    }

    /// A aba da conversa: símbolo por papel, número do painel com vários, o ponto do estado no
    /// lugar do × e o tom (única, ativa ou inativa). O clique põe o foco na caixa do painel; no
    /// Tab, Enter faz o mesmo, Delete fecha e Shift+F10 abre o menu.
    #[allow(clippy::too_many_arguments)]
    fn aba_da_conversa(
        &self,
        motor_entidade: &Entity<Motor>,
        motor: &Motor,
        chat: &Chat,
        ix: usize,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let painel = &self.paineis[ix];
        let painel_id = painel.id;
        let varios = self.paineis.len() > 1;
        let ativo = ix == self.ativo;
        let tom = match (varios, ativo) {
            // Com outras abas ao lado, a que está à vista leva o fundo mais claro.
            (false, _) if painel.abas.len() > 1 => TomAba::Ativa,
            (false, _) => TomAba::Unica,
            (true, true) => TomAba::Ativa,
            // Sem o foco e com outras abas ao lado: um degrau mais claro que elas.
            (true, false) if painel.abas.len() > 1 => TomAba::InativaAVista,
            (true, false) => TomAba::Inativa,
        };
        let arrastando = self.arrastando_aba == Some(painel_id);
        let previa = Some(simbolo_da_conversa());
        let janela = cx.entity().downgrade();
        let fechar = janela.clone();
        // O × aparece no hover, no foco pelo teclado e sempre no ativo quando há vários; o ponto
        // do estado fica no lugar dele.
        let mut aba = Aba::new(SharedString::from(format!("aba-{painel_id}")), chat.titulo.clone())
            .tom(tom)
            .fechar(
                if painel.abas.len() > 1 {
                    "Fechar a aba (a conversa continua na lista)"
                } else {
                    "Fechar este painel (a conversa continua na lista)"
                },
                varios && ativo,
                {
                    let id_do_chat = chat.id.clone();
                    move |window, cx| {
                        let _ = fechar.update(cx, |this, cx| this.fechar_aba(painel_id, &id_do_chat, window, cx));
                    }
                },
            )
            .foco(&painel.foco_aba)
            .arrastando(arrastando);
        // No Mínimo, com vários painéis, o número fica no lugar do símbolo (e o que ele dava vai
        // para o "⋯").
        // O número do painel liga a aba à linha do chat na lista: com a lista recolhida não há a
        // que ligar, e ele sai (dono, 13/11/2026).
        let numerada = varios && self.lista;
        aba = aba.simbolo(Simbolo::Elemento(simbolo_na_aba(motor, chat, painel_id)));
        if numerada {
            aba = aba.numero(ix + 1, ativo);
        }
        let dica = SharedString::from(format!("{}\n{}", chat.titulo, pasta_em_texto(motor, chat)));
        let acao = acao_do_painel(ix).filter(|_| varios);
        let arrasto = ArrastoChat { chat: chat.id.clone(), titulo: chat.titulo.clone().into(), de_painel: Some(painel_id) };
        let (menu_janela, menu_motor) = (janela.clone(), motor_entidade.clone());
        let aba = aba
            .construir(window, cx)
            .key_context("AbaPainel")
            .on_key_down(cx.listener(move |this, evento: &KeyDownEvent, window, cx| {
                let Some(ix) = this.paineis.iter().position(|p| p.id == painel_id) else {
                    return;
                };
                let teclas = &evento.keystroke;
                match teclas.key.as_str() {
                    "enter" if !teclas.modifiers.modified() => this.focar_painel(ix, window, cx),
                    "delete" if !teclas.modifiers.modified() => {
                        // Como o × e o Ctrl+W: fecha a aba à vista e, na última, o painel.
                        let _ = ix;
                        this.ativar_painel(painel_id, cx);
                        this.fechar_painel_ativo(window, cx);
                    }
                    "f10" if teclas.modifiers.shift => this.abrir_menu_do_painel(painel_id, cx),
                    "menu" => this.abrir_menu_do_painel(painel_id, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .tooltip(move |window, cx| {
                let mut dica = Tooltip::new(dica.clone());
                if let Some(acao) = &acao {
                    dica = dica.action(acao.as_ref(), None);
                }
                dica.build(window, cx)
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Some(ix) = this.paineis.iter().position(|p| p.id == painel_id) {
                    this.focar_painel(ix, window, cx);
                }
            }))
            .on_drag(arrasto, move |arrasto, _, _, cx| {
                let simbolo = previa.as_ref().and_then(Simbolo::copia).unwrap_or_else(simbolo_da_conversa);
                cx.new(|_| PreviaArrasto { titulo: arrasto.titulo.clone(), simbolo })
            })
            .context_menu(move |menu, window, cx| {
                menu_do_painel(menu, menu_janela.clone(), menu_motor.clone(), painel_id, window, cx)
            });
        // O clique na aba não move a janela; o vazio em volta dela, sim.
        sem_arrastar().flex().min_w_0().child(aba).into_any_element()
    }

    /// Shift+F10 ou a tecla Menu na aba: abre o "⋯" do painel.
    fn abrir_menu_do_painel(&mut self, painel_id: u64, cx: &mut Context<Self>) {
        if let Some(p) = self.paineis.iter_mut().find(|p| p.id == painel_id) {
            p.menu_aberto = true;
            cx.notify();
        }
    }

    /// "⋯" e painel da direita, em botões de 28. O modo foco mora no "⋯": o botão dele só aparece
    /// com ele ligado, no pressionado neutro, para desligar com um clique. Na largura estreita, ele
    /// e depois o painel da direita ficam só no "⋯".
    fn acoes_do_painel(
        &self,
        motor_entidade: &Entity<Motor>,
        ix: usize,
        nivel: LarguraPainel,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let painel = &self.paineis[ix];
        let (painel_id, foco) = (painel.id, painel.modo_foco);
        let mostra_foco = foco && matches!(nivel, LarguraPainel::Medio | LarguraPainel::Largo);
        h_flex()
            .flex_none()
            .gap(r(4.))
            .when(mostra_foco, |s| {
                let dica = if foco {
                    "Mostrar as ferramentas\nVolta a mostrar as ferramentas usadas neste painel"
                } else {
                    "Modo foco\nSó as mensagens, sem as ferramentas (neste painel)"
                };
                s.child(sem_arrastar().child(
                    botao_icone(("modo-foco", painel_id as usize), Lucide::Focus, dica, TamanhoBotao::Barra, window, cx)
                        .selected(foco)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(p) = this.paineis.iter_mut().find(|p| p.id == painel_id) {
                                p.modo_foco = !p.modo_foco;
                            }
                            cx.notify();
                        })),
                ))
            })
            // O "..." leva tudo do painel, inclusive abrir e fechar a direita.
            .child(sem_arrastar().relative().child(self.botao_opcoes(motor_entidade, painel_id, painel.menu_aberto, window, cx)))
    }

    /// O que a direita mostra: o arquivo aberto ou o painel do chat.
    fn o_que_a_direita_mostra(&self, motor: &Motor) -> Direita {
        match self.aba_visivel(motor) {
            super::abas_direita::AbaDireita::Arquivo(_) => Direita::Arquivo,
            super::abas_direita::AbaDireita::Chat => Direita::EsteChat,
        }
    }

    /// O "⋯" do painel, controlado por `Painel.menu_aberto` para o Shift+F10 da aba abrir o mesmo.
    fn botao_opcoes(
        &self,
        motor_entidade: &Entity<Motor>,
        painel_id: u64,
        aberto: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let janela = cx.entity().downgrade();
        let ao_mudar = janela.clone();
        let motor = motor_entidade.clone();
        Popover::new(("menu-painel", painel_id as usize))
            .appearance(false)
            .overlay_closable(false)
            .anchor(Anchor::TopRight)
            .offset(px(4.))
            .open(aberto)
            .on_open_change(move |aberto, _, cx| {
                let _ = ao_mudar.update(cx, |this, cx| {
                    if let Some(p) = this.paineis.iter_mut().find(|p| p.id == painel_id)
                        && p.menu_aberto != *aberto
                    {
                        p.menu_aberto = *aberto;
                        cx.notify();
                    }
                });
            })
            // O clique direito na aba abre o mesmo menu (`menu_do_painel`).
            .trigger(botao_icone(
                ("opcoes-painel", painel_id as usize),
                Lucide::Ellipsis,
                "Mais opções da conversa\nTambém no clique direito da aba",
                TamanhoBotao::Barra,
                window,
                cx,
            ))
            .content(move |_, window, cx| {
                // Feito uma vez por abertura (o conteúdo é pedido a cada quadro) e refeito
                // depois de fechar, como o dropdown do kit.
                let guardado = window.use_keyed_state(
                    SharedString::from(format!("menu-painel-itens-{painel_id}")),
                    cx,
                    |_, _| None::<Entity<PopupMenu>>,
                );
                if let Some(menu) = guardado.read(cx).clone() {
                    return menu;
                }
                let (janela, motor) = (janela.clone(), motor.clone());
                let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
                    menu_do_painel(menu, janela.clone(), motor.clone(), painel_id, window, cx)
                });
                menu.focus_handle(cx).focus(window, cx);
                // Pelo Shift+F10, o primeiro item já vem realçado.
                if window.last_input_was_keyboard() {
                    window.on_next_frame(|window, cx| window.dispatch_action(Box::new(SelectDown), cx));
                }
                let (popover, fraco) = (cx.entity().downgrade(), guardado.downgrade());
                window
                    .subscribe(&menu, cx, move |_, _: &DismissEvent, window, cx| {
                        if let Some(popover) = popover.upgrade() {
                            popover.update(cx, |p, cx| p.dismiss(window, cx));
                        }
                        let _ = fraco.update(cx, |g, _| *g = None);
                    })
                    .detach();
                guardado.update(cx, |g, _| *g = Some(menu.clone()));
                menu
            })
    }

    /// Painel sem conversa: o estado vazio amplo, com Novo chat (na última pasta usada; sem ela, a
    /// paleta pergunta) e Novo chat em outra pasta.
    fn sem_conversa(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let vazio = EstadoVazio::new(Lucide::MessageSquarePlus, "Comece um chat")
            .texto("Cada chat trabalha direto numa pasta do seu computador, como o Claude Code. Sem pasta, ele ganha uma vazia só dele.")
            .acao(
                botao_primario("comecar", Some(Lucide::Plus.into()), "Novo chat", TamanhoBotao::Controle, window, cx)
                    .tooltip_with_action("Novo chat", &NovaConversa, None)
                    .on_click(cx.listener(|this, _, window, cx| this.novo_chat_direto(window, cx))),
            )
            .acao(
                botao_fantasma("outra-pasta-vazio", Some(Lucide::FolderOpen.into()), "Em outra pasta…", TamanhoBotao::Controle, window, cx)
                    .on_click(cx.listener(|this, _, window, cx| this.dialogo_nova_conversa(window, cx))),
            );
        v_flex().flex_1().items_center().justify_center().p(esp::E32).child(vazio.amplo())
    }
}

/// O que o painel da direita mostra: o arquivo aberto ou Este chat.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Direita {
    Arquivo,
    EsteChat,
}

impl Direita {
    /// O item do "⋯" quando o botão sai do cabeçalho.
    fn no_menu(self) -> &'static str {
        match self {
            Direita::Arquivo => "Mostrar o arquivo aberto",
            Direita::EsteChat => "Mostrar Este chat",
        }
    }
}

#[cfg(test)]
mod testes {
    use super::modelo_da_conversa;
    use crate::modelo::Chat;

    fn chat(modelo: Option<&str>, atual: Option<&str>) -> Chat {
        let mut c: Chat = serde_json::from_str(
            r#"{"id": "c1", "titulo": "Cupons", "sessao": "s", "sessao_criada": false, "cwd": "/tmp/loja", "modelo": null,
                "criado_em": 0, "atualizado_em": 0, "custo_usd": 0.0}"#,
        )
        .unwrap();
        c.modelo = modelo.map(Into::into);
        c.modelo_atual = atual.map(Into::into);
        c
    }

    fn ver(modelo: Option<&str>, atual: Option<&str>) -> (String, Option<String>) {
        modelo_da_conversa(&chat(modelo, atual))
    }

    #[test]
    fn modelo_em_uso_e_a_escolha_quando_difere() {
        assert_eq!(ver(None, None), ("Padrão".into(), None));
        assert_eq!(ver(Some("sonnet"), None), ("Sonnet".into(), None));
        assert_eq!(ver(None, Some("claude-opus-5-5")), ("Opus 5.5".into(), None));
        assert_eq!(ver(Some("opus"), Some("claude-opus-5-5")), ("Opus 5.5".into(), None));
        assert_eq!(ver(Some("sonnet"), Some("claude-opus-5-5")), ("Opus 5.5".into(), Some("Sonnet".into())));
        // Conversa antiga com o nome completo guardado na escolha.
        assert_eq!(ver(Some("claude-opus-4-1"), Some("claude-opus-4-1")), ("Opus 4.1".into(), None));
    }
}

