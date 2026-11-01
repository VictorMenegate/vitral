//! A casca de um painel de conversa: o cabeçalho de uma linha (a aba, as ações do painel e os
//! botões da janela), o menu "⋯", o popover de uso, a tela sem conversa e a soltura de arquivos.
//! As mensagens ficam em `mensagens.rs` e a caixa em `composer.rs`.

use gpui_kit::base::actions::SelectDown;
use gpui_kit::component::{
    ElementExt as _, Selectable, h_flex,
    menu::{ContextMenuExt as _, DropdownMenu, PopupMenu, PopupMenuItem},
    popover::Popover,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    AlternarPainel, FecharPainel, Gaveta, IrParaPainel1, IrParaPainel2, IrParaPainel3, IrParaPainel4, Janela,
    MoverPainelDireita, MoverPainelEsquerda, NovaConversa,
    componentes::{
        Aba, Anel, CabecalhoPopover, CorpoPopover, EstadoVazio, LarguraPainel, LarguraPopover, Papel, PreviaArrasto,
        QuadradoDeProjeto, Reflexo as _, Selo, Simbolo, TamanhoBotao, TomAba, botao_icone, botao_primario,
        cabecalho_objeto, cor_da_faixa, formatar_custo, sinal_do_chat,
    },
    composer, equipe, icone_projeto, menus,
    movimento::{self, Curva, De, com_estado, entrada},
    paineis::{ArrastoChat, FormaSoltura, Soltura, motivo_sem_lugar},
    sem_arrastar,
};
use crate::{
    contas,
    modelo::{Chat, Papel as PapelDoChat},
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, altura, esp, gradiente_destaque, halo_suave, r, raio, tam},
};

/// Quem age na conversa, para o símbolo da aba e o fundo do cabeçalho. O Mestre e os avulsos
/// chegam com a onda 2.
fn papel_da_conversa(chat: &Chat) -> Papel {
    match chat.papel {
        PapelDoChat::Coordenador => Papel::Coordenador,
        PapelDoChat::Trabalhador => Papel::Trabalhador,
    }
}

/// O símbolo da conversa: o quadrado do projeto no coordenador, o círculo do papel nos outros.
fn simbolo_da_conversa(motor: &Motor, chat: &Chat) -> Simbolo {
    match papel_da_conversa(chat) {
        Papel::Coordenador => {
            let (icone, cor) = icone_projeto::visual(motor, &chat.projeto);
            Simbolo::Projeto(icone, cor)
        }
        papel => Simbolo::Papel(papel),
    }
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

/// Onde o trabalhador mexe: a cópia isolada da tarefa ou, num projeto sem git, a pasta original.
enum Pasta {
    Copia { tarefa: u64 },
    Original,
    Nenhuma,
}

fn pasta_do_chat(motor: &Motor, chat: &Chat) -> Pasta {
    if chat.papel != PapelDoChat::Trabalhador {
        return Pasta::Nenhuma;
    }
    let tarefa = chat.tarefa.and_then(|id| motor.tarefa(id)).filter(|t| !t.copia_limpa);
    match tarefa.filter(|t| t.worktree.is_some()) {
        Some(t) => Pasta::Copia { tarefa: t.id },
        None if chat.cwd == motor.caminho_projeto(&chat.projeto) => Pasta::Original,
        None => Pasta::Nenhuma,
    }
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

/// Onde a conversa está, em texto, para a dica da aba: "loja › Cupons… · cópia t2".
fn caminho_em_texto(motor: &Motor, chat: &Chat) -> String {
    let mut texto = chat.projeto.clone();
    if let Some(pai) = chat.pai.as_ref().and_then(|p| motor.chat(p)) {
        texto.push_str(&format!(" › {}", pai.titulo));
    }
    match pasta_do_chat(motor, chat) {
        Pasta::Copia { tarefa, .. } => texto.push_str(&format!(" · cópia t{tarefa}")),
        Pasta::Original => texto.push_str(" · na pasta original"),
        Pasta::Nenhuma => {}
    }
    texto
}

/// O que vai no lugar do símbolo da aba: o quadrado do projeto (com o menu dele) no coordenador,
/// o ↰ que abre o coordenador no trabalhador e, sem pai, o círculo do papel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SimboloDaAba {
    Projeto,
    Pai,
    Papel,
}

fn simbolo_da_aba(chat: &Chat, tem_pai: bool) -> SimboloDaAba {
    match papel_da_conversa(chat) {
        Papel::Coordenador => SimboloDaAba::Projeto,
        _ if tem_pai => SimboloDaAba::Pai,
        _ => SimboloDaAba::Papel,
    }
}

/// O quadrado do projeto na aba do coordenador, gatilho do menu do projeto: no hover e aberto,
/// um aro de 2 px em volta.
#[derive(IntoElement)]
struct QuadradoComMenu {
    base: Stateful<Div>,
    aberto: bool,
}

impl Selectable for QuadradoComMenu {
    fn selected(mut self, selected: bool) -> Self {
        self.aberto = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.aberto
    }
}

impl Styled for QuadradoComMenu {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for QuadradoComMenu {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl RenderOnce for QuadradoComMenu {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.base.when(self.aberto, |s| s.shadow(aro_do_simbolo()))
    }
}

impl DropdownMenu for QuadradoComMenu {}

fn aro_do_simbolo() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(0.), Cores::borda_forte()).spread_radius(px(2.))]
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

/// Itens do chip do projeto: o objeto em cima, "Mostrar só este projeto" e os do projeto.
fn itens_do_chip_projeto(
    menu: PopupMenu,
    janela: WeakEntity<Janela>,
    motor: &Entity<Motor>,
    projeto: &str,
    cabecalho: bool,
    window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    let filtrado = janela
        .upgrade()
        .is_some_and(|j| j.read(cx).projeto_filtro.as_deref() == Some(projeto));
    let (icone, cor) = icone_projeto::visual(motor.read(cx), projeto);
    let caminho = motor.read(cx).caminho_projeto(projeto).display().to_string();
    let nome = projeto.to_string();
    let menu = menu
        .when(cabecalho, |menu| {
            menu.item(cabecalho_objeto(Simbolo::Projeto(icone, cor), nome.clone(), Some(caminho.into())))
                .separator()
        })
        .item(
            PopupMenuItem::new("Mostrar só este projeto")
                .checked(filtrado)
                .on_click(na_janela(&janela, move |this, window, cx| {
                    this.projeto_filtro = if this.projeto_filtro.as_deref() == Some(nome.as_str()) {
                        None
                    } else {
                        Some(nome.clone())
                    };
                    this.mostrar_lista(window, cx);
                })),
        )
        .separator();
    menus::itens_do_projeto(menu, janela, motor, projeto, false, window, cx)
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
        projeto: String,
        nivel: LarguraPainel,
        foco: bool,
        primeiro: bool,
        ultimo: bool,
        varios: bool,
        direita_aberta: bool,
        /// A direita mostra um arquivo (e não a Equipe) e, com a Equipe fora da vista, quantas
        /// tarefas pedem você.
        direita: (bool, Option<(usize, bool, String)>),
        cheio: Option<String>,
        rotulo: Option<String>,
        pasta: Pasta,
        /// A aba não mostra o quadrado do projeto (trabalhador, ou o número no lugar dele): o
        /// menu do projeto vem para cá.
        projeto_no_menu: bool,
        /// O ↰ não está à vista na aba: "Abrir o coordenador" vem para cá.
        pai_no_menu: Option<(String, String)>,
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
        let simbolo_a_vista = !(nivel == LarguraPainel::Minimo && j.paineis.len() > 1);
        let pai = chat.pai.as_ref().and_then(|p| m.chat(p));
        let simbolo = simbolo_da_aba(chat, pai.is_some());
        Some(Dados {
            projeto_no_menu: !(simbolo_a_vista && simbolo == SimboloDaAba::Projeto),
            pai_no_menu: pai.filter(|_| !simbolo_a_vista).map(|p| (p.id.clone(), p.titulo.clone())),
            chat: chat.id.clone(),
            projeto: chat.projeto.clone(),
            nivel,
            foco: painel.modo_foco,
            primeiro: ix == 0,
            ultimo: ix + 1 == j.paineis.len(),
            varios: j.paineis.len() > 1,
            direita_aberta: j.direita_visivel() || j.gaveta == Some(Gaveta::Direita),
            direita: j.o_que_a_direita_mostra(m),
            cheio,
            rotulo,
            pasta: pasta_do_chat(m, chat),
        })
    })();
    let Some(d) = dados else {
        return menu;
    };
    let estreito = matches!(d.nivel, LarguraPainel::Estreito | LarguraPainel::Minimo);
    let minimo = d.nivel == LarguraPainel::Minimo;
    let mut menu = menu.min_w(tam(window, 300.)).max_w(tam(window, 360.));
    if let Some(rotulo) = d.rotulo {
        menu = menu.label(rotulo).separator();
    }
    if estreito {
        menu = menu.item(
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
    }
    if minimo && d.ultimo {
        // O botão da direita saiu do cabeçalho: o número do que a Equipe espera vem para cá,
        // tingido porque fica na linha.
        let (arquivo, pedidos) = d.direita;
        let rotulo = if arquivo { "Mostrar o arquivo aberto" } else { "Mostrar a Equipe" };
        menu = menu.item(
            PopupMenuItem::element(move |_, _| {
                h_flex()
                    .gap(esp::E8)
                    .min_w_0()
                    .child(div().flex_1().min_w_0().truncate().child(rotulo))
                    .children(pedidos.as_ref().map(|(n, erro, _)| {
                        Selo::contagem(*n).tingida(if *erro { Cores::vermelho() } else { Cores::ambar() })
                    }))
            })
            .icon(Lucide::PanelRight)
            .checked(d.direita_aberta)
            .action(Box::new(AlternarPainel))
            .on_click(na_janela(&janela, |this, window, cx| this.alternar_painel(window, cx))),
        );
    }
    menu = menu
        .item(
            item_com_motivo("Nova conversa ao lado", Lucide::Columns2, d.cheio)
                .on_click(na_janela(&janela, |this, window, cx| this.nova_conversa_ao_lado(window, cx))),
        )
        .separator();
    if d.varios {
        let primeiro = d.primeiro.then(|| "Já é o primeiro painel".to_string());
        let ultimo = d.ultimo.then(|| "Já é o último painel".to_string());
        menu = menu
            .item(
                item_com_motivo("Mover para a esquerda", Lucide::ArrowLeft, primeiro)
                    .action(Box::new(MoverPainelEsquerda))
                    .on_click(na_janela(&janela, move |this, _, cx| this.mover_este_painel(painel_id, -1, cx))),
            )
            .item(
                item_com_motivo("Mover para a direita", Lucide::ArrowRight, ultimo)
                    .action(Box::new(MoverPainelDireita))
                    .on_click(na_janela(&janela, move |this, _, cx| this.mover_este_painel(painel_id, 1, cx))),
            )
            .item(
                PopupMenuItem::new("Fechar os outros painéis")
                    .on_click(na_janela(&janela, move |this, _, cx| this.fechar_outros(painel_id, cx))),
            );
    }
    menu = menu
        .item(
            PopupMenuItem::new("Fechar este painel")
                .icon(Lucide::X)
                .action(Box::new(FecharPainel))
                .on_click(na_janela(&janela, move |this, window, cx| this.fechar_e_focar(painel_id, window, cx))),
        )
        .separator();
    menu = menus::menu_conversa(menu, janela.clone(), &motor, &d.chat, false, window, cx);
    // O que a aba não mostra vem para cá: o coordenador, o projeto e a cópia da tarefa.
    let copia = match d.pasta {
        Pasta::Copia { tarefa, .. } => Some(tarefa),
        _ => None,
    };
    if d.pai_no_menu.is_some() || d.projeto_no_menu || copia.is_some() {
        menu = menu.separator();
    }
    if let Some((pai, titulo)) = d.pai_no_menu {
        menu = menu.item(
            PopupMenuItem::new(format!("Abrir o coordenador “{titulo}”"))
                .icon(Lucide::CornerLeftUp)
                .on_click(na_janela(&janela, move |this, window, cx| this.selecionar(Some(pai.clone()), window, cx))),
        );
    }
    if d.projeto_no_menu {
        let (icone, cor) = icone_projeto::visual(motor.read(cx), &d.projeto);
        let (j, m, projeto) = (janela.clone(), motor.clone(), d.projeto.clone());
        menu = menu.submenu_with_icon(
            Some(icone_projeto::desenho(icone, 16., rgb(cor))),
            format!("Projeto {}", d.projeto),
            window,
            cx,
            move |sub, window, cx| itens_do_chip_projeto(sub, j.clone(), &m, &projeto, false, window, cx),
        );
    }
    if let Some(tarefa) = copia {
        let (j, m) = (janela.clone(), motor.clone());
        menu = menu.submenu_with_icon(
            Some(Lucide::GitBranch.into()),
            format!("Cópia t{tarefa}"),
            window,
            cx,
            move |sub, window, cx| menus::itens_da_copia(sub, j.clone(), &m, tarefa, false, window, cx),
        );
    }
    menu
}

/// O popover do anel do contexto, na caixa: no cabeçalho o contexto e o custo ("Contexto 31% ·
/// US$ 1,84"); embaixo o modelo, a conta, os tokens e, no coordenador, o custo com os
/// trabalhadores; no rodapé a nota da estimativa e `acao` (o Compactar).
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
    let trabalhadores: Vec<f64> = motor
        .chats
        .iter()
        .filter(|c| c.pai.as_deref() == Some(chat.id.as_str()))
        .map(|c| c.custo_usd)
        .collect();
    let com_trabalhadores = (chat.papel == PapelDoChat::Coordenador && !trabalhadores.is_empty())
        .then(|| formatar_custo(chat.custo_usd + trabalhadores.iter().sum::<f64>()));
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
                .when_some(tokens, |s, t| s.child(linha("Tokens", t)))
                .when_some(com_trabalhadores, |s, total| s.child(linha("Com os trabalhadores", texto(total)))),
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
        let varios = self.paineis.len() > 1;
        let ativo = ix == Some(self.ativo);
        let id_texto = |nome: &str| SharedString::from(format!("{nome}-{painel_id}"));

        let cabecalho = self.cabecalho_painel(motor_entidade, motor, chat, ix, window, cx);
        let corpo = match (&chat, ix) {
            (Some(chat), Some(ix)) => self.mensagens(motor, chat, ix, window, cx).into_any_element(),
            _ => self.sem_conversa(motor, window, cx).into_any_element(),
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
        let cartao = v_flex()
            .id(id_texto("cartao-conversa"))
            .relative()
            .size_full()
            .min_w_0()
            .rounded(raio::PECA)
            .bg(Cores::cartao())
            .overflow_hidden()
            .border(px(1.5))
            .reflexo(raio::PECA)
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
                s.child(self.permissoes(motor, chat, window, cx))
                    .child(self.caixa_mensagem(motor, chat, ix, window, cx))
            })
            .when(arquivos_sobre, |s| {
                s.child(FormaSoltura::new(id_texto("soltura-arquivos"), Soltura::Arquivos))
            });
        // Com vários painéis, o ativo acende a borda e o halo azul (180 ms; apaga em 120). O que
        // acabou de abrir ao lado entra só com o fade: a largura não anima.
        let halo = varios && ativo;
        com_estado(move |window, cx| {
            let (alvo, duracao) = if halo {
                ((Cores::destaque_borda(), 1f32), movimento::MOVER)
            } else {
                ((Cores::destaque_borda().opacity(0.), 0.), movimento::COR)
            };
            let (borda, forca) =
                movimento::valor(("halo-painel", painel_id as usize), alvo, duracao, Curva::Mover, window, cx);
            let opacidade = movimento::valor_desde(
                ("entra-painel", painel_id as usize),
                if painel_novo { 0. } else { 1. },
                1f32,
                movimento::ENTRAR,
                Curva::Entrar,
                window,
                cx,
            );
            cartao
                .border_color(borda)
                .when(forca > 0., |s| s.shadow(vec![halo_suave(forca)]))
                .when(opacidade < 1., |s| s.opacity(opacidade))
                .into_any_element()
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
        let rolado = chat.is_some() && ix.is_some_and(|ix| self.paineis[ix].rolagem.offset().y < px(0.));
        let mestre = chat.is_some_and(|c| papel_da_conversa(c) == Papel::Mestre);
        let conversa = chat.zip(ix);
        let primeiro = ix.unwrap_or(0) == 0;
        let aneis = (!self.lista && primeiro).then(|| self.uso_compacto(cx)).flatten();
        self.arrastavel_com_id(SharedString::from(format!("arrastar-conversa-{painel_id}")))
            .flex_none()
            .flex()
            .items_center()
            .h(altura::CABECALHO)
            .pl(esp::E8)
            .pr(esp::E8)
            .gap(esp::E8)
            .border_b_1()
            .border_color(if rolado { Cores::borda() } else { gpui_kit::transparent_black().into() })
            // O gradiente do Mestre leva o próprio raio: o `overflow_hidden` do cartão não corta
            // canto arredondado.
            .when(mestre, |s| s.bg(gradiente_destaque()).rounded_t(raio::PECA - px(1.5)))
            // Com a lista fora da tela, o primeiro painel começa pelo logo compacto.
            .when(!self.orcamento.lista && primeiro, |s| {
                s.child(self.logo_compacto(if ix.is_some() { "mostrar-lista" } else { "mostrar-lista-vazio" }, window, cx))
            })
            .when_some(aneis, |s, aneis| s.child(sem_arrastar().flex_none().child(aneis)))
            .child(
                div().flex().flex_1().min_w_0().children(
                    conversa.map(|(chat, ix)| self.aba_da_conversa(motor_entidade, motor, chat, ix, nivel, window, cx)),
                ),
            )
            .children(conversa.map(|(_, ix)| self.acoes_do_painel(motor_entidade, ix, nivel, window, cx)))
            .when(mostra_botoes_janela, |s| {
                // Com ações do painel à esquerda, um traço separa os botões da janela.
                s.child(if chat.is_some() { self.botoes_janela_separados(cx) } else { self.botoes_janela(cx) })
            })
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
        nivel: LarguraPainel,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let painel = &self.paineis[ix];
        let painel_id = painel.id;
        let varios = self.paineis.len() > 1;
        let ativo = ix == self.ativo;
        let minimo = nivel == LarguraPainel::Minimo;
        let tom = match (varios, ativo) {
            (false, _) => TomAba::Unica,
            (true, true) => TomAba::Ativa,
            (true, false) => TomAba::Inativa,
        };
        let arrastando = self.arrastando_aba == Some(painel_id);
        let previa = Some(simbolo_da_conversa(motor, chat));
        let janela = cx.entity().downgrade();
        let fechar = janela.clone();
        // O × aparece no hover, no foco pelo teclado e sempre no ativo quando há vários; o ponto
        // do estado fica no lugar dele.
        let mut aba = Aba::new(SharedString::from(format!("aba-{painel_id}")), chat.titulo.clone())
            .tom(tom)
            .estado(sinal_do_chat(motor, chat))
            .fechar("Fechar este painel (a conversa continua na lista)", varios && ativo, move |window, cx| {
                let _ = fechar.update(cx, |this, cx| this.fechar_painel_clicado(painel_id, window, cx));
            })
            .foco(&painel.foco_aba)
            .arrastando(arrastando);
        // No Mínimo, com vários painéis, o número fica no lugar do símbolo (e o que ele dava vai
        // para o "⋯").
        if !(minimo && varios) {
            aba = aba.simbolo(Simbolo::Elemento(self.simbolo_na_aba(motor_entidade, motor, chat, painel_id, window, cx)));
        }
        if varios {
            aba = aba.numero(ix + 1, ativo);
        }
        let papel = match (chat.papel, chat.tarefa) {
            (PapelDoChat::Trabalhador, Some(n)) => format!("Trabalhador da tarefa #{n}"),
            (PapelDoChat::Trabalhador, None) => "Trabalhador".to_string(),
            (PapelDoChat::Coordenador, _) => "Coordenador".to_string(),
        };
        let dica = SharedString::from(format!("{}\n{papel} · {}", chat.titulo, caminho_em_texto(motor, chat)));
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
                    "delete" if !teclas.modifiers.modified() => this.fechar_e_focar(painel_id, window, cx),
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
                let simbolo = previa.as_ref().and_then(Simbolo::copia).unwrap_or(Simbolo::Papel(Papel::Coordenador));
                cx.new(|_| PreviaArrasto { titulo: arrasto.titulo.clone(), simbolo })
            })
            .context_menu(move |menu, window, cx| {
                menu_do_painel(menu, menu_janela.clone(), menu_motor.clone(), painel_id, window, cx)
            });
        // O clique na aba não move a janela; o vazio em volta dela, sim.
        sem_arrastar().flex().min_w_0().child(aba).into_any_element()
    }

    /// O símbolo da aba, que também é um botão: no coordenador, o quadrado do projeto com o menu
    /// dele; no trabalhador, o ↰ que abre o coordenador (Ctrl+clique, ao lado), com um ponto âmbar
    /// quando ele mexe direto na pasta original. O clique nele não ativa a aba nem a arrasta.
    fn simbolo_na_aba(
        &self,
        motor_entidade: &Entity<Motor>,
        motor: &Motor,
        chat: &Chat,
        painel_id: u64,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let janela = cx.entity().downgrade();
        let pai = chat.pai.as_ref().and_then(|p| motor.chat(p));
        let parar = || {
            div()
                .flex()
                .flex_none()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        match simbolo_da_aba(chat, pai.is_some()) {
            SimboloDaAba::Projeto => {
                let (icone, cor) = icone_projeto::visual(motor, &chat.projeto);
                let caminho = motor.caminho_projeto(&chat.projeto).display().to_string();
                let dica = SharedString::from(format!("Projeto {}\n{caminho}", chat.projeto));
                let (m, projeto) = (motor_entidade.clone(), chat.projeto.clone());
                let lado = 18.;
                let base = div()
                    .id(("projeto-aba", painel_id as usize))
                    .flex_none()
                    .rounded(raio::do_icone(lado))
                    .cursor_pointer()
                    .hover(|s| s.shadow(aro_do_simbolo()))
                    .child(QuadradoDeProjeto::new(icone, cor, lado))
                    .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx));
                parar()
                    .child(QuadradoComMenu { base, aberto: false }.dropdown_menu_with_anchor(
                        Anchor::TopLeft,
                        move |menu, window, cx| itens_do_chip_projeto(menu, janela.clone(), &m, &projeto, true, window, cx),
                    ))
                    .into_any_element()
            }
            SimboloDaAba::Pai => {
                let pai = pai.map(|p| (p.id.clone(), p.titulo.clone())).unwrap_or_default();
                let original = matches!(pasta_do_chat(motor, chat), Pasta::Original);
                let dica = format!("Abrir o coordenador “{}”\nCtrl+clique abre ao lado", pai.1);
                let id_pai = pai.0;
                let botao = botao_icone(("pai-aba", painel_id as usize), Lucide::CornerLeftUp, dica, TamanhoBotao::Linha, window, cx)
                    .on_click(move |evento, window, cx| {
                        let _ = janela.update(cx, |this, cx| {
                            if evento.modifiers().control {
                                this.abrir_ao_lado(id_pai.clone(), None, window, cx)
                            } else {
                                this.selecionar(Some(id_pai.clone()), window, cx)
                            }
                        });
                    });
                parar()
                    .relative()
                    .child(botao)
                    // Sem git, ele mexe direto na pasta do projeto: o ponto âmbar avisa (o porquê
                    // está na dica da aba).
                    .when(original, |s| {
                        s.child(
                            div()
                                .absolute()
                                .top(r(2.))
                                .right(r(2.))
                                .size(r(6.))
                                .rounded_full()
                                .bg(Cores::ambar()),
                        )
                    })
                    .into_any_element()
            }
            SimboloDaAba::Papel => papel_da_conversa(chat).circulo(18.).into_any_element(),
        }
    }

    /// Shift+F10 ou a tecla Menu na aba: abre o "⋯" do painel.
    fn abrir_menu_do_painel(&mut self, painel_id: u64, cx: &mut Context<Self>) {
        if let Some(p) = self.paineis.iter_mut().find(|p| p.id == painel_id) {
            p.menu_aberto = true;
            cx.notify();
        }
    }

    /// Modo foco, "⋯" e painel da direita, em botões de 28. Os interruptores ligados ficam no
    /// pressionado neutro; na largura estreita, o modo foco e depois o painel da direita vão para
    /// o "⋯".
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
        let ultimo = ix + 1 == self.paineis.len();
        let direita_aberta = self.direita_visivel() || self.gaveta == Some(Gaveta::Direita);
        let mostra_foco = matches!(nivel, LarguraPainel::Medio | LarguraPainel::Largo);
        let mostra_direita = ultimo && nivel != LarguraPainel::Minimo;
        h_flex()
            .flex_none()
            .gap(r(4.))
            .when(mostra_foco, |s| {
                let dica = if foco {
                    "Mostrar as ferramentas\nVolta a mostrar as ferramentas usadas neste painel"
                } else {
                    "Modo foco\nSó as mensagens e as tarefas, sem as ferramentas (neste painel)"
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
            .child(sem_arrastar().child(self.botao_opcoes(motor_entidade, painel_id, painel.menu_aberto, window, cx)))
            .when(mostra_direita, |s| {
                // Com a Equipe fora da vista, o número do que ela espera fica no canto do botão
                // (vermelho se uma tarefa caiu), e o resumo, na dica.
                let (arquivo, pedidos) = self.o_que_a_direita_mostra(motor_entidade.read(cx));
                let nome = if arquivo { "Arquivo aberto" } else { "Equipe" };
                let dica: SharedString = match &pedidos {
                    Some((_, _, resumo)) => format!("{nome}\n{resumo}").into(),
                    None => nome.into(),
                };
                let selo = pedidos.map(|(n, erro, _)| {
                    Selo::contagem(n).solida(if erro { Cores::vermelho() } else { Cores::ambar() })
                });
                s.child(
                    sem_arrastar()
                        .relative()
                        .child(
                            botao_icone(
                                ("alternar-painel", painel_id as usize),
                                Lucide::PanelRight,
                                dica.clone(),
                                TamanhoBotao::Barra,
                                window,
                                cx,
                            )
                            .tooltip_with_action(dica, &AlternarPainel, None)
                            .selected(direita_aberta)
                            .on_click(cx.listener(|this, _, window, cx| this.alternar_painel(window, cx))),
                        )
                        .when_some(selo, |s, selo| s.child(div().absolute().top(r(-3.)).right(r(-3.)).child(selo))),
                )
            })
    }

    /// Se a direita mostra um arquivo aberto (e não a Equipe) e, com a Equipe fora da vista (a
    /// direita fechada, ou a gaveta dela fechada), quantas tarefas pedem você, se alguma caiu e o
    /// resumo. Com o arquivo na direita, o botão não leva à Equipe: nada de número.
    fn o_que_a_direita_mostra(&self, motor: &Motor) -> (bool, Option<(usize, bool, String)>) {
        if self.arquivo_aberto.is_some() {
            return (true, None);
        }
        let a_vista = self.direita_visivel() || self.gaveta == Some(Gaveta::Direita);
        let pedidos = self
            .selecionado
            .as_ref()
            .filter(|_| !a_vista)
            .and_then(|id| motor.chat(id))
            .and_then(|chat| equipe::coordenador_da_equipe(motor, chat))
            .map(|coordenador| equipe::pedidos_da_equipe(motor, &coordenador))
            .filter(|(n, ..)| *n > 0);
        (false, pedidos)
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

    /// Painel sem conversa: o estado vazio amplo, com a ação de começar (ou de criar o primeiro
    /// projeto).
    fn sem_conversa(&self, motor: &Motor, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let vazio = if motor.projetos().is_empty() {
            EstadoVazio::new(Lucide::FolderPlus, "Nenhum projeto ainda")
                .texto(
                    "Cada subpasta da pasta de trabalho é um projeto. Crie um para começar a conversar \
                     com o coordenador.",
                )
                .acao(
                    botao_primario(
                        "criar-projeto-vazio",
                        Some(Lucide::FolderPlus.into()),
                        "Novo projeto…",
                        TamanhoBotao::Controle,
                        window,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.dialogo_criar_projeto(window, cx))),
                )
        } else {
            EstadoVazio::new(Lucide::MessageSquarePlus, "Comece uma conversa")
                .texto(
                    "Você fala com o coordenador do projeto. Ele divide o trabalho em tarefas, abre uma \
                     conversa de trabalhador para cada uma, revisa as entregas e te devolve um resumo.",
                )
                .acao(
                    botao_primario("comecar", Some(Lucide::Plus.into()), "Nova conversa", TamanhoBotao::Controle, window, cx)
                        .tooltip_with_action("Nova conversa", &NovaConversa, None)
                        .on_click(cx.listener(|this, _, window, cx| this.dialogo_nova_conversa(window, cx))),
                )
        };
        v_flex().flex_1().items_center().justify_center().p(esp::E32).child(vazio.amplo())
    }
}

#[cfg(test)]
mod testes {
    use super::modelo_da_conversa;
    use crate::modelo::Chat;

    fn chat(modelo: Option<&str>, atual: Option<&str>) -> Chat {
        let mut c: Chat = serde_json::from_str(
            r#"{"id": "c1", "projeto": "loja", "titulo": "Cupons", "papel": "Coordenador", "pai": null,
                "tarefa": null, "sessao": "s", "sessao_criada": false, "cwd": "/tmp/loja", "modelo": null,
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
