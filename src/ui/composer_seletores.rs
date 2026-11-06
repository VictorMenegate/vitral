//! Os controles da barra da caixa: os chips de modo, modelo, esforço e conta, com os menus e os
//! popovers deles, o clipe de anexar e o "…".

use std::rc::Rc;

use gpui_kit::base::actions::{Confirm, SelectFirst, SelectLast, SelectLeft, SelectRight};
use gpui_kit::component::{
    Icon, Sizable as _,
    input::{Paste, TextareaState},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    AbrirEsforco, AbrirModelo, AlternarModo, AlternarUltracode, AlternarUltrathink, DESCRICOES_DE_MODELO, ESFORCOS,
    MODELOS, MODOS, PopCaixa, VERSOES_DE_MODELO, com_claude, descricao_do_esforco, descricao_do_modo, descricao_regra,
    indice_esforco, nome_curto_modelo, nome_do_modo, nomes_dos_niveis, rotulo_modelo, sem_sufixo_de_janela,
    usa_esforco, versao_exata,
};
use crate::ui::{Janela, composer};
use crate::ui::componentes::{
    AnelDeUso, BarrinhasDeSinal, CabecalhoPopover, Chip, ControleSegmentado, CorpoPopover, EventoEscolha,
    FaixaCaixa, GatilhoChip, GrupoDeEscolha, ItemDeEscolha, LarguraPopover, LinhaDeOpcao, ListaDeEscolha, Marca,
    Selo, OndeAtalho, Simbolo, TamanhoBotao, TamanhoChip, botao_icone, cabecalho_escolha, cor_da_faixa,
    desenho_teclas, icone_do_modo, item_descrito, popover, resumo_uso, rodape_menu,
};
use crate::{
    contas,
    modelo::{Chat, Papel},
    motor::{FLUXOS_DESLIGADOS, Motor, Uso, motivo_sem_versao_exata},
    tema::{Cores, Lucide, Tipo as _, esp, r, tam},
};

/// O que os controles da barra de um painel precisam saber.
pub(super) struct Barra {
    pub(super) painel: u64,
    pub(super) chat: String,
    pub(super) faixa: FaixaCaixa,
    /// Sem cópia isolada: o chat mexe direto na pasta do projeto.
    pub(super) sem_copia: bool,
    pub(super) composer: Entity<TextareaState>,
    /// Rodando ou na fila esperando vaga.
    pub(super) rodando: bool,
}

impl Barra {
    /// Ids por painel: o estado de menus e popovers é guardado pelo id.
    fn id(&self, nome: &'static str) -> ElementId {
        (nome, self.painel as usize).into()
    }

    fn estreita(&self) -> bool {
        self.faixa == FaixaCaixa::Estreita
    }

    fn larga(&self) -> bool {
        self.faixa == FaixaCaixa::Larga
    }

    fn tamanho(&self) -> TamanhoChip {
        if self.estreita() { TamanhoChip::Medio } else { TamanhoChip::Grande }
    }

    fn largura_popover(&self) -> LarguraPopover {
        if self.estreita() { LarguraPopover::P320 } else { LarguraPopover::P400 }
    }
}

fn mudar(motor: &Option<Entity<Motor>>, chat: &str, cx: &mut App, f: impl FnOnce(&mut Chat)) {
    if let Some(m) = motor {
        m.update(cx, |m, cx| m.mudar_chat(chat, f, cx));
    }
}

/// O popover da caixa abriu ou fechou. Ao fechar, só limpa se ainda é ele o aberto: o clique
/// fora de um e a abertura do outro chegam no mesmo evento.
fn abrir_ou_fechar(janela: &WeakEntity<Janela>, painel: u64, qual: PopCaixa, abriu: bool, cx: &mut App) {
    let _ = janela.update(cx, |this, cx| {
        if abriu {
            this.pop_caixa = Some((painel, qual));
        } else if this.pop_caixa == Some((painel, qual)) {
            this.pop_caixa = None;
        }
        cx.notify();
    });
}

/// Item de menu com o ícone dentro do conteúdo: num menu de itens descritos, o `.icon()` do kit
/// reservaria uma coluna vazia antes de todos. O texto cai na coluna dos títulos descritos (28).
fn item_com_icone(icone: Lucide, rotulo: &'static str) -> PopupMenuItem {
    PopupMenuItem::element(move |window, _| {
        div()
            .flex()
            .items_center()
            .gap(esp::E12)
            .min_w_0()
            .child(div().flex_none().text_color(Cores::tenue()).child(Icon::new(icone).with_size(tam(window, 16.))))
            .child(div().flex_1().min_w_0().truncate().child(rotulo))
    })
}

/// Rótulo com o gatilho digitado à direita ("@", "/"), em mono tênue: não é atalho do teclado.
fn rotulo_com_gatilho(rotulo: &'static str, gatilho: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(esp::E8)
        .min_w_0()
        .child(div().flex_1().min_w_0().truncate().child(rotulo))
        .child(div().flex_none().mono_pequeno().text_color(Cores::tenue()).child(gatilho))
}

/// Uma tecla (ou um par) e o que ela faz, no desenho das teclas, como no rodapé da paleta.
fn dica_de_tecla(teclas: &[&'static str], texto: &'static str) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(esp::E4)
        .child(desenho_teclas(teclas.iter().map(|t| SharedString::from(*t)).collect()))
        .child(texto)
}

/// O rodapé de teclas dos popovers da caixa (28, 12 tênue), com a linha em cima: as teclas que
/// o popover aceita. Se não couber numa linha, quebra em vez de cortar.
fn rodape_de_teclas(dicas: impl IntoIterator<Item = Div>) -> [AnyElement; 2] {
    [
        div().h(px(1.)).bg(Cores::linha()).into_any_element(),
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(esp::E8)
            .gap_y(esp::E4)
            .min_h(r(28.))
            .metadado()
            .text_color(Cores::tenue())
            .children(dicas)
            .into_any_element(),
    ]
}

/// Linha de duas (48) com o símbolo de 20: as contas e a Automática (o texto em 28, como nos
/// itens descritos).
fn linha_de_duas(simbolo: AnyElement, titulo: SharedString, descricao: SharedString) -> Div {
    div()
        .flex()
        .items_center()
        .gap(esp::E8)
        .h(r(48.))
        .min_w_0()
        .child(div().flex_none().size(r(20.)).flex().items_center().justify_center().child(simbolo))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(r(14.))
                        .line_height(r(20.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(titulo),
                )
                .child(div().truncate().metadado().text_color(Cores::tenue()).child(descricao)),
        )
}

// ── Modo ────────────────────────────────────────────────────────────────

struct DadosModo {
    motor: Option<Entity<Motor>>,
    chat: String,
    atual: String,
    sem_copia: bool,
    permitidas: Vec<String>,
    /// O campo da caixa: o Shift+Tab só vale nele ("Composer > Input").
    foco_caixa: FocusHandle,
}

/// Uma regra permitida nesta conversa: o clique tira só ela.
fn item_de_regra(regra: &str, d: &Rc<DadosModo>) -> PopupMenuItem {
    let (icone, comando) = if regra.starts_with("bash:") {
        (Lucide::Terminal, true)
    } else if regra == "edicoes" {
        (Lucide::Pencil, false)
    } else {
        (Lucide::Wrench, false)
    };
    let texto: SharedString = descricao_regra(regra).into();
    let (regra, d) = (regra.to_string(), d.clone());
    PopupMenuItem::element(move |window, _| {
        div()
            .flex()
            .items_center()
            .gap(esp::E12)
            .min_w_0()
            .child(div().flex_none().text_color(Cores::tenue()).child(Icon::new(icone).with_size(tam(window, 16.))))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .map(|s| {
                        if comando {
                            s.mono_pequeno().text_color(Cores::codigo())
                        } else {
                            s.corpo_ui().text_color(Cores::texto())
                        }
                    })
                    .child(texto.clone()),
            )
            .child(div().flex_none().text_color(Cores::tenue()).child(Icon::new(Lucide::X).with_size(tam(window, 14.))))
    })
    .on_click(move |_, _, cx| {
        let regra = regra.clone();
        mudar(&d.motor, &d.chat, cx, move |c| c.permitidas.retain(|r| *r != regra));
    })
}

fn menu_do_modo(mut menu: PopupMenu, d: &Rc<DadosModo>, window: &mut Window, cx: &mut Context<PopupMenu>) -> PopupMenu {
    let largura = tam(window, 400.);
    menu = menu.min_w(largura).max_w(largura).item(cabecalho_escolha(
        "Modo de permissão",
        Some((Box::new(AlternarModo), OndeAtalho::Foco(d.foco_caixa.clone()), "troca")),
    ));
    for (valor, nome, descricao) in MODOS {
        let (icone, cor) = icone_do_modo(valor, d.sem_copia);
        let descricao = if valor == "bypassPermissions" && d.sem_copia {
            "Sem cópia isolada: mexe direto na pasta do projeto"
        } else {
            descricao
        };
        let d2 = d.clone();
        menu = menu.item(
            item_descrito(Some(Simbolo::Icone(icone, Some(cor))), nome, descricao, valor == d.atual)
                .on_click(move |_, _, cx| mudar(&d2.motor, &d2.chat, cx, |c| c.permissao = Some(valor.into()))),
        );
    }
    if !d.permitidas.is_empty() {
        menu = menu.separator().label("Permitido sempre nesta conversa");
        let n = d.permitidas.len();
        let mostradas = if n > 4 { 3 } else { n };
        for regra in &d.permitidas[..mostradas] {
            menu = menu.item(item_de_regra(regra, d));
        }
        if n > 4 {
            let d2 = d.clone();
            menu = menu.submenu(format!("Ver todas ({n})"), window, cx, move |mut sub, window, _| {
                let largura = tam(window, 360.);
                sub = sub.min_w(largura).max_w(largura).scrollable(true);
                for regra in &d2.permitidas {
                    sub = sub.item(item_de_regra(regra, &d2));
                }
                sub
            });
        }
        let d2 = d.clone();
        menu = menu.item(
            item_com_icone(Lucide::Undo2, "Voltar a perguntar sobre tudo")
                .on_click(move |_, _, cx| mudar(&d2.motor, &d2.chat, cx, |c| c.permitidas.clear())),
        );
    }
    menu
}

// ── Conta ───────────────────────────────────────────────────────────────

struct DadosConta {
    motor: Option<Entity<Motor>>,
    janela: WeakEntity<Janela>,
    chat: String,
    escolha: String,
    /// (id, nome, uso) de cada conta.
    contas: Vec<(String, String, Uso)>,
}

impl DadosConta {
    fn automatica(&self) -> bool {
        self.escolha == contas::AUTOMATICA
    }

    fn nome(&self) -> String {
        if self.automatica() {
            return "Automática".into();
        }
        self.contas
            .iter()
            .find(|(id, _, _)| *id == self.escolha)
            .map(|(_, nome, _)| nome.clone())
            .unwrap_or_else(|| "Principal".into())
    }

    fn uso(&self) -> Uso {
        self.contas
            .iter()
            .find(|(id, _, _)| *id == self.escolha)
            .map(|(_, _, uso)| *uso)
            .unwrap_or_default()
    }
}

fn cinco_h(uso: &Uso) -> Option<f32> {
    uso.cinco_horas.map(|f| f as f32)
}

fn descricao_da_conta(uso: &Uso) -> String {
    let resumo = resumo_uso(cinco_h(uso), uso.semana.map(|f| f as f32));
    if uso.bloqueado { format!("limite atingido · {resumo}") } else { resumo }
}

fn itens_da_conta(mut menu: PopupMenu, d: &Rc<DadosConta>, window: &mut Window) -> PopupMenu {
    let largura = tam(window, 320.);
    menu = menu.min_w(largura).max_w(largura).item(cabecalho_escolha("Conta do Claude", None));
    let trocar = |valor: String| {
        let d = d.clone();
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            let valor = valor.clone();
            mudar(&d.motor, &d.chat, cx, move |c| c.conta = Some(valor));
        }
    };
    for (id, nome, uso) in &d.contas {
        let (titulo, uso, chave): (SharedString, Uso, SharedString) =
            (nome.clone().into(), *uso, format!("uso-conta-{id}").into());
        menu = menu.item(
            PopupMenuItem::element(move |_, _| {
                let anel = AnelDeUso::new(chave.clone(), Marca::Claude, cinco_h(&uso))
                    .diametro(20.)
                    .bloqueado(uso.bloqueado);
                linha_de_duas(anel.into_any_element(), titulo.clone(), descricao_da_conta(&uso).into())
            })
            .checked(*id == d.escolha)
            .on_click(trocar(id.clone())),
        );
    }
    let janela = d.janela.clone();
    menu.item(
        PopupMenuItem::element(|window, _| {
            linha_de_duas(
                div()
                    .text_color(Cores::tenue())
                    .child(Icon::new(Lucide::Shuffle).with_size(tam(window, 16.)))
                    .into_any_element(),
                "Automática".into(),
                "A conta com mais folga no plano".into(),
            )
        })
        .checked(d.automatica())
        .on_click(trocar(contas::AUTOMATICA.to_string())),
    )
    .separator()
    .item(item_com_icone(Lucide::Settings, "Gerenciar contas…").on_click(move |_, window, cx| {
        let _ = janela.update(cx, |this, cx| this.dialogo_contas(window, cx));
    }))
}

// ── Anexar ──────────────────────────────────────────────────────────────

#[derive(Clone)]
struct DadosAnexar {
    janela: WeakEntity<Janela>,
    chat: String,
    painel: u64,
    mestre: bool,
}

fn itens_de_anexar(menu: PopupMenu, d: &DadosAnexar, com_comandos: bool) -> PopupMenu {
    let (j1, j2, j3, j4) = (d.janela.clone(), d.janela.clone(), d.janela.clone(), d.janela.clone());
    if d.mestre {
        // O Mestre não tem pasta: o que se anexa a ele é para onde vai e de que chat se fala.
        let (c1, painel) = (d.chat.clone(), d.painel);
        return menu
            .item(
                PopupMenuItem::element(|_, _| rotulo_com_gatilho("Mandar para um projeto…", "#"))
                    .icon(Lucide::FolderInput)
                    .on_click(move |_, window, cx| {
                        let _ = j2.update(cx, |this, cx| this.acrescentar_gatilho(painel, '#', window, cx));
                    }),
            )
            .item(
                PopupMenuItem::element(|_, _| rotulo_com_gatilho("Falar de um chat…", "@"))
                    .icon(Lucide::AtSign)
                    .on_click(move |_, window, cx| {
                        let _ = j3.update(cx, |this, cx| this.acrescentar_gatilho(painel, '@', window, cx));
                    }),
            )
            .item(
                PopupMenuItem::new("Arquivo do computador…").icon(Lucide::FolderOpen).on_click(move |_, window, cx| {
                    let chat = c1.clone();
                    let _ = j1.update(cx, |this, cx| this.escolher_anexos(chat, window, cx));
                }),
            );
    }
    let (c1, c2, c3, painel) = (d.chat.clone(), d.chat.clone(), d.chat.clone(), d.painel);
    let menu = menu
        .item(
            PopupMenuItem::new("Arquivo ou imagem do computador…")
                .icon(Lucide::FolderOpen)
                .on_click(move |_, window, cx| {
                    let chat = c1.clone();
                    let _ = j1.update(cx, |this, cx| this.escolher_anexos(chat, window, cx));
                }),
        )
        .item(
            PopupMenuItem::element(|_, _| rotulo_com_gatilho("Arquivo do projeto…", "@"))
                .icon(Lucide::FileText)
                .on_click(move |_, window, cx| {
                    let chat = c2.clone();
                    let _ = j2.update(cx, |this, cx| this.dialogo_arquivos_projeto(chat, window, cx));
                }),
        )
        .item(
            PopupMenuItem::new("Conversa anterior como contexto…")
                .icon(Lucide::MessageSquareQuote)
                .on_click(move |_, window, cx| {
                    let chat = c3.clone();
                    let _ = j3.update(cx, |this, cx| this.dialogo_conversa_contexto(chat, window, cx));
                }),
        );
    if com_comandos { menu.item(item_de_comandos(j4, painel)) } else { menu }
}

fn item_de_comandos(janela: WeakEntity<Janela>, painel: u64) -> PopupMenuItem {
    PopupMenuItem::element(|_, _| rotulo_com_gatilho("Comandos de barra", "/"))
        .icon(Lucide::SquareSlash)
        .on_click(move |_, window, cx| {
            let _ = janela.update(cx, |this, cx| this.por_na_caixa(painel, "/", window, cx));
        })
}

// ── Modelo ──────────────────────────────────────────────────────────────

/// A chave da linha que só explica por que as versões exatas não dão: ela vai desabilitada, e
/// escolher não muda modelo nenhum.
const SEM_VERSOES: &str = "+sem-versoes";

/// Uma linha do seletor de modelo.
#[derive(Clone, PartialEq, Debug)]
struct LinhaDeModelo {
    valor: String,
    titulo: String,
    descricao: String,
    em_uso: bool,
    /// Por que a linha não dá para escolher (o Claude Code velho não aceita as versões novas).
    desabilitada: Option<String>,
}

impl LinhaDeModelo {
    fn new(valor: impl Into<String>, titulo: impl Into<String>, descricao: impl Into<String>) -> Self {
        LinhaDeModelo {
            valor: valor.into(),
            titulo: titulo.into(),
            descricao: descricao.into(),
            em_uso: false,
            desabilitada: None,
        }
    }
}

/// As linhas do primeiro grupo: Padrão, as quatro famílias e, numa conversa antiga que guardou o
/// nome completo, uma sexta com ele. A versão só aparece quando o modelo em uso é daquela família
/// (nunca inventada) e a conversa segue um apelido. Seguindo o Padrão, é ele que diz qual está
/// em uso ("Padrão · Opus 5.5"), e a família não ganha o selo. Uma versão exata escolhida está no
/// segundo grupo, não aqui: as famílias ficam com o nome seco.
fn linhas_de_modelo(escolhido: Option<&str>, em_uso: Option<&str>, padrao_pasta: &str) -> Vec<LinhaDeModelo> {
    let em_uso = em_uso.filter(|m| !m.is_empty());
    let curto = em_uso.map(nome_curto_modelo);
    let apelido = |v: &str| MODELOS.iter().any(|(m, _)| *m == v);
    let segue_apelido = escolhido.is_none_or(apelido);
    let no_padrao = escolhido.is_none();
    let qual = curto.clone().filter(|_| no_padrao);
    let padrao = match (padrao_pasta.is_empty(), qual) {
        (true, Some(m)) => format!("O do Claude Code · {m}"),
        (true, None) => "O que o Claude Code escolher".to_string(),
        (false, Some(m)) => format!("Padrão · {m}"),
        (false, None) => format!("Padrão · {}", rotulo_modelo(padrao_pasta, None)),
    };
    let mut linhas = vec![LinhaDeModelo::new("", "Padrão", padrao)];
    for ((valor, familia), descricao) in MODELOS[1..].iter().zip(DESCRICOES_DE_MODELO) {
        let versao = curto.clone().filter(|m| segue_apelido && m.to_lowercase().starts_with(valor));
        linhas.push(LinhaDeModelo {
            em_uso: versao.is_some() && !no_padrao,
            ..LinhaDeModelo::new(
                *valor,
                format!("Claude {}", versao.clone().unwrap_or_else(|| familia.to_string())),
                descricao,
            )
        });
    }
    if let Some(antigo) = escolhido.filter(|e| !apelido(e) && !versao_exata(e)) {
        linhas.push(LinhaDeModelo {
            em_uso: em_uso == Some(antigo),
            ..LinhaDeModelo::new(antigo, com_claude(&nome_curto_modelo(antigo)), "escolhido antes")
        });
    }
    linhas
}

/// As linhas do segundo grupo: as versões exatas, por família, com a que está rodando marcada —
/// inclusive quando a conversa segue a família, que é como se vê qual Opus o apelido pega hoje.
/// Num Claude Code anterior ao `VERSAO_MODELOS_EXATOS` nenhuma dá para escolher: fica uma linha
/// só dizendo por quê, mais a que já estava escolhida, se havia (para sair dela, ou deixá-la).
fn linhas_de_versao(escolhido: Option<&str>, em_uso: Option<&str>, motivo: Option<&str>) -> Vec<LinhaDeModelo> {
    let rodando = em_uso.filter(|m| !m.is_empty()).map(sem_sufixo_de_janela);
    if let Some(motivo) = motivo {
        let mut linhas = vec![LinhaDeModelo {
            desabilitada: Some(motivo.to_string()),
            ..LinhaDeModelo::new(SEM_VERSOES, "Sem as versões exatas", com_maiuscula(motivo))
        }];
        if let Some(exata) = escolhido.filter(|e| versao_exata(e)) {
            linhas.push(LinhaDeModelo {
                em_uso: rodando == Some(exata),
                ..LinhaDeModelo::new(exata, nome_curto_modelo(exata), "escolhida nesta conversa")
            });
        }
        return linhas;
    }
    VERSOES_DE_MODELO
        .iter()
        .map(|id| LinhaDeModelo {
            em_uso: rodando == Some(*id),
            ..LinhaDeModelo::new(*id, nome_curto_modelo(id), "")
        })
        .collect()
}

fn padrao_da_pasta<'a>(motor: &'a Motor, chat: &Chat) -> &'a str {
    match chat.papel {
        Papel::Coordenador | Papel::Mestre => &motor.config.modelo_mestre,
        Papel::Trabalhador | Papel::Projeto => &motor.config.modelo_chats,
    }
}

fn itens_de_modelo(linhas: Vec<LinhaDeModelo>) -> Vec<ItemDeEscolha> {
    linhas
        .into_iter()
        .map(|l| {
            let termos = format!("{} {}", l.valor, l.descricao);
            ItemDeEscolha {
                descricao: (!l.descricao.is_empty()).then(|| l.descricao.into()),
                selos: if l.em_uso { vec![Selo::neutro("em uso")] } else { Vec::new() },
                termos: Some(termos.into()),
                desabilitado: l.desabilitada.map(Into::into),
                ..ItemDeEscolha::new(l.valor, l.titulo)
            }
        })
        .collect()
}

/// Os dois grupos do seletor: as famílias, que é o que se usa no dia a dia, e embaixo as versões
/// exatas, em linhas de uma altura só.
fn grupos_de_modelo(motor: &Motor, chat: &Chat) -> Vec<GrupoDeEscolha> {
    let (escolhido, em_uso) = (chat.modelo.as_deref(), chat.modelo_atual.as_deref());
    let motivo = motivo_sem_versao_exata(motor.versao_claude);
    vec![
        GrupoDeEscolha {
            titulo: Some("Claude".into()),
            itens: itens_de_modelo(linhas_de_modelo(escolhido, em_uso, padrao_da_pasta(motor, chat))),
        },
        GrupoDeEscolha {
            titulo: Some("Versões exatas".into()),
            itens: itens_de_modelo(linhas_de_versao(escolhido, em_uso, motivo.as_deref())),
        },
    ]
}

/// Ao abrir o seletor (clique ou Ctrl+Shift+M): as linhas de agora, a busca limpa e o realce no
/// escolhido.
pub(super) fn preparar_modelos(
    lista: &Entity<ListaDeEscolha>,
    motor: &Entity<Motor>,
    chat: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let dados = {
        let m = motor.read(cx);
        m.chat(chat).map(|c| (grupos_de_modelo(m, c), c.modelo.clone().unwrap_or_default()))
    };
    let Some((grupos, escolhido)) = dados else {
        return;
    };
    lista.update(cx, |l, cx| {
        // O logo fica só no rótulo do grupo; os itens vão sem símbolo.
        l.simbolo_do_grupo("Claude", Simbolo::Marca(Marca::Claude));
        l.definir(grupos, Some(escolhido.into()), cx);
        l.abrir(window, cx);
    });
}

// ── Esforço ─────────────────────────────────────────────────────────────

/// As barrinhas do esforço nas cores do arco-íris: o Ultrathink armado.
fn barras_arco_iris() -> Simbolo {
    Simbolo::Elemento(BarrinhasDeSinal::new(5, 5).arco_iris().into_any_element())
}

/// O Ultracode no chip e no popover: o ícone de fluxo em violeta (a cor da automação).
fn simbolo_ultracode() -> Simbolo {
    Simbolo::Icone(Lucide::Workflow.into(), Some(Cores::violeta()))
}

/// O motivo vem do motor em minúscula (ele também vai no meio de um aviso).
fn com_maiuscula(texto: &str) -> String {
    let mut letras = texto.chars();
    letras
        .next()
        .map(|p| p.to_uppercase().chain(letras).collect())
        .unwrap_or_default()
}

struct DadosEsforco {
    painel: u64,
    chat: String,
    atual: usize,
    armado: bool,
    /// Ultracode ligado para a próxima mensagem.
    ultracode: bool,
    /// Por que o Ultracode não vale nesta conversa.
    sem_ultracode: Option<SharedString>,
    /// Outra conversa roda um Ultracode: este espera a vez.
    outro_ultracode: bool,
    /// Um turno com Ultracode nesta conta mostrou os fluxos de agentes desligados.
    fluxos_desligados: bool,
    /// O modo de permissão pergunta antes de agir: os agentes do fluxo não conseguem perguntar.
    modo_pergunta: bool,
    /// A conversa é a do Mestre: o porquê de o Ultracode não valer nela vai na dica da linha.
    mestre: bool,
    estreita: bool,
    largura: LarguraPopover,
    foco: FocusHandle,
    motor: Option<Entity<Motor>>,
    janela: WeakEntity<Janela>,
}

impl DadosEsforco {
    /// Grava na hora (vale no próximo turno); o popover continua aberto.
    fn nivel(&self, i: usize, cx: &mut App) {
        let valor = ESFORCOS[i.min(ESFORCOS.len() - 1)].0;
        mudar(&self.motor, &self.chat, cx, move |c| {
            c.esforco = Some(valor.to_string()).filter(|v| !v.is_empty())
        });
    }

    fn armar(&self, ligado: bool, cx: &mut App) {
        let chat = self.chat.clone();
        let _ = self.janela.update(cx, |this, cx| {
            if ligado {
                this.ultrathink.insert(chat);
            } else {
                this.ultrathink.remove(&chat);
            }
            cx.notify();
        });
    }

    fn armar_ultracode(&self, ligado: bool, cx: &mut App) {
        if ligado && self.sem_ultracode.is_some() {
            return;
        }
        let chat = self.chat.clone();
        let _ = self.janela.update(cx, |this, cx| {
            if ligado {
                this.ultracode.insert(chat);
            } else {
                this.ultracode.remove(&chat);
            }
            cx.notify();
        });
    }
}

/// A linha do Ultracode: vale só para a próxima mensagem, com o custo sempre à vista e, embaixo,
/// o que pode atrapalhar (a vez de outra conversa, os fluxos desligados, o modo que pergunta).
fn linha_ultracode(d: &Rc<DadosEsforco>) -> AnyElement {
    let mut avisos = vec![String::from("Gasta bem mais do plano e roda no esforço Extra")];
    if d.outro_ultracode {
        avisos.push("Ligado, espera o Ultracode da outra conversa terminar".into());
    }
    if d.fluxos_desligados {
        avisos.push(com_maiuscula(FLUXOS_DESLIGADOS));
    }
    if d.modo_pergunta {
        avisos.push(
            "Neste modo, comandos e edições dos agentes podem ser recusados: eles não conseguem pedir \
             permissão (use Sem perguntar ou Automático)"
                .into(),
        );
    }
    let aviso = avisos.join("\n");
    let d1 = d.clone();
    let linha = LinhaDeOpcao::new(("ultracode", d.painel as usize), "Ultracode")
        .simbolo(Lucide::Workflow, Cores::violeta())
        .descricao("Vários agentes em paralelo, só na próxima mensagem")
        .aviso(aviso)
        .interruptor(d.ultracode, move |ligado, _, cx| d1.armar_ultracode(ligado, cx));
    let Some(motivo) = &d.sem_ultracode else {
        return linha.into_any_element();
    };
    // Desabilitada, a linha mostra o motivo curto; no Mestre, a explicação vai na dica.
    let linha = linha.desabilitada(motivo.clone());
    if d.mestre {
        div()
            .id(("ultracode-porque", d.painel as usize))
            .child(linha)
            .tooltip(|window, cx| {
                Tooltip::new("O Mestre não edita nada: o trabalho vai para os chats de projeto")
                    .build(window, cx)
            })
            .into_any_element()
    } else {
        linha.into_any_element()
    }
}

fn corpo_do_esforco(d: &Rc<DadosEsforco>) -> impl IntoElement + use<> {
    let atual = d.atual;
    let simbolo = if d.armado {
        barras_arco_iris()
    } else {
        Simbolo::Sinal { nivel: atual as u8, total: 5 }
    };
    let tecla = |f: fn(usize) -> usize| {
        let d = d.clone();
        move |cx: &mut App| d.nivel(f(d.atual), cx)
    };
    let (anterior, seguinte, primeiro, ultimo) = (
        tecla(|a| a.saturating_sub(1)),
        tecla(|a| (a + 1).min(ESFORCOS.len() - 1)),
        tecla(|_| 0),
        tecla(|_| ESFORCOS.len() - 1),
    );
    let (d1, d2, d3, d4) = (d.clone(), d.clone(), d.clone(), d.clone());
    div()
        .flex()
        .flex_col()
        .track_focus(&d.foco)
        .key_context("Esforco")
        .on_action(move |_: &SelectLeft, _, cx| anterior(cx))
        .on_action(move |_: &SelectRight, _, cx| seguinte(cx))
        .on_action(move |_: &SelectFirst, _, cx| primeiro(cx))
        .on_action(move |_: &SelectLast, _, cx| ultimo(cx))
        .on_action(move |_: &AlternarUltrathink, _, cx| d1.armar(!d1.armado, cx))
        .on_action(move |_: &AlternarUltracode, _, cx| d4.armar_ultracode(!d4.ultracode, cx))
        .on_action(move |_: &Confirm, _, cx| abrir_ou_fechar(&d2.janela, d2.painel, PopCaixa::Esforco, false, cx))
        .child(
            CorpoPopover::new(d.largura)
                // O nível já está na pílula do segmentado, logo abaixo.
                .cabecalho(CabecalhoPopover::Escolha {
                    simbolo: Some(simbolo),
                    titulo: "Esforço".into(),
                    valor: None,
                    dica: None,
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(esp::E8)
                        .child({
                            let d = d.clone();
                            ControleSegmentado::new(
                                ("segmentado-esforco", d.painel as usize),
                                nomes_dos_niveis(d.estreita),
                                atual,
                                move |i, _, cx| d.nivel(i, cx),
                            )
                            .cor(Cores::ciano())
                            .largura_total(true)
                            .compacto(true)
                            .no_flutuante(true)
                            .large()
                        })
                        .child(div().metadado().text_color(Cores::tenue()).child(descricao_do_esforco(atual))),
                )
                .rodape(
                    // A linha de opção tem margem própria; aqui ela alinha com o resto do popover.
                    div()
                        .flex()
                        .flex_col()
                        .mx(r(-12.))
                        .my(r(-6.))
                        .child(
                            LinhaDeOpcao::new(("ultrathink", d.painel as usize), "Ultrathink")
                                .arco_iris()
                                .descricao("Só na próxima mensagem")
                                .interruptor(d.armado, move |ligado, _, cx| d3.armar(ligado, cx)),
                        )
                        .child(linha_ultracode(d)),
                )
                .when(!d.estreita, |c| {
                    let [linha, teclas] = rodape_de_teclas([
                        dica_de_tecla(&["←", "→"], "nível"),
                        dica_de_tecla(&["Espaço"], "Ultrathink"),
                        dica_de_tecla(&["U"], "Ultracode"),
                        dica_de_tecla(&["Enter"], "fecha"),
                    ]);
                    c.rodape(linha).rodape(teclas)
                }),
        )
}

impl Janela {
    pub(super) fn chip_modo(&self, motor: &Motor, chat: &Chat, b: &Barra, window: &Window, cx: &Context<Self>) -> AnyElement {
        let atual = motor.permissao_de(chat);
        let alerta = b.sem_copia && atual == "bypassPermissions";
        let mudado = chat.permissao.as_ref().is_some_and(|p| *p != motor.config.permissao);
        let dica: SharedString = if alerta {
            "Sem perguntar, direto na pasta do projeto\nNão há cópia isolada: o Claude mexe nos arquivos sem pedir".into()
        } else {
            format!("Modo de permissão: {}\n{}", nome_do_modo(&atual), descricao_do_modo(&atual)).into()
        };
        let chip = |aberto: bool| {
            let (icone, cor) = icone_do_modo(&atual, b.sem_copia);
            let mut chip = Chip::new(b.id("chip-modo"), nome_do_modo(&atual))
                .simbolo(Simbolo::Icone(icone, Some(cor)))
                .tamanho(b.tamanho())
                .mudado(mudado)
                .seta(b.larga())
                .so_icone(b.estreita())
                .aberto(aberto);
            if alerta {
                chip = chip.alerta(Cores::vermelho());
            }
            if !aberto {
                chip = chip.dica_acao(dica.clone(), Box::new(AlternarModo), None);
            }
            chip.construir(window, cx)
        };
        let dados = Rc::new(DadosModo {
            motor: self.motor.clone(),
            chat: chat.id.clone(),
            atual: atual.clone(),
            sem_copia: b.sem_copia,
            permitidas: chat.permitidas.clone(),
            foco_caixa: b.composer.read(cx).focus_handle(cx),
        });
        GatilhoChip::new(chip(false), chip(true))
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, window, cx| menu_do_modo(menu, &dados, window, cx))
            .into_any_element()
    }

    /// No Mestre, no lugar do modo: se a divisão é mostrada antes (o cartão "Vou dividir assim")
    /// ou começa direto. É a opção "Mostrar a divisão antes de começar" das Configurações.
    pub(super) fn chip_divisao(&self, motor: &Motor, b: &Barra, window: &Window, cx: &Context<Self>) -> AnyElement {
        let direto = motor.config.comecar_sem_perguntar;
        let (icone, nome, cor) =
            if direto { (Lucide::Zap, "Começar direto", Cores::ambar()) } else { (Lucide::GitFork, "Mostrar antes", Cores::destaque_texto()) };
        let dica: SharedString = if direto {
            "Divisão: começar direto\nO Mestre cria os chats e manda o pedido sem mostrar o cartão antes".into()
        } else {
            "Divisão: mostrar antes\nO Mestre mostra como vai dividir e espera o seu Começar".into()
        };
        let chip = |aberto: bool| {
            let mut chip = Chip::new(b.id("chip-divisao"), if b.larga() { format!("Divisão: {nome}") } else { nome.to_string() })
                .simbolo(Simbolo::Icone(Icon::new(icone), Some(cor)))
                .tamanho(b.tamanho())
                .mudado(direto)
                .seta(b.larga())
                .so_icone(b.estreita())
                .aberto(aberto);
            if !aberto {
                chip = chip.dica(dica.clone());
            }
            chip.construir(window, cx)
        };
        let motor_e = self.motor.clone();
        GatilhoChip::new(chip(false), chip(true))
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, window, _| {
                let largura = tam(window, 380.);
                let opcao = |valor: bool, icone: Lucide, cor: Hsla, titulo: &'static str, descricao: &'static str| {
                    let m = motor_e.clone();
                    item_descrito(Some(Simbolo::Icone(Icon::new(icone), Some(cor))), titulo, descricao, valor == direto).on_click(
                        move |_, _, cx| {
                            if let Some(m) = &m {
                                m.update(cx, |m, cx| m.mudar_config(|c| c.comecar_sem_perguntar = valor, cx));
                            }
                        },
                    )
                };
                menu.min_w(largura)
                    .max_w(largura)
                    .item(cabecalho_escolha("Divisão do Mestre", None))
                    .item(opcao(false, Lucide::GitFork, Cores::destaque_texto(), "Mostrar antes", "Você vê o cartão “Vou dividir assim” e decide"))
                    .item(opcao(true, Lucide::Zap, Cores::ambar(), "Começar direto", "Os chats começam sem perguntar; o cartão fica para ver"))
            })
            .into_any_element()
    }

    pub(super) fn chip_modelo(
        &self,
        motor: &Motor,
        chat: &Chat,
        ix: usize,
        b: &Barra,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let padrao = padrao_da_pasta(motor, chat).to_string();
        let efetivo = chat.modelo.clone().unwrap_or(padrao);
        let em_uso = chat.modelo_atual.clone().filter(|m| !m.is_empty());
        let curto = rotulo_modelo(&efetivo, em_uso.as_deref());
        let nome = com_claude(&curto);
        let detalhe = match (&chat.modelo, &em_uso) {
            (None, Some(m)) => format!("padrão · em uso: {m}"),
            (None, None) => "padrão".to_string(),
            (Some(_), Some(m)) => format!("em uso: {m}"),
            (Some(_), None) => "escolhido nesta conversa".to_string(),
        };
        let aberto = self.pop_caixa == Some((b.painel, PopCaixa::Modelo));
        let mut chip = Chip::new(b.id("chip-modelo"), if b.larga() { nome.clone() } else { curto })
            .simbolo(Simbolo::Marca(Marca::Claude))
            .tamanho(b.tamanho())
            .mudado(chat.modelo.is_some())
            .seta(b.larga())
            .so_icone(b.estreita())
            .aberto(aberto);
        if !aberto {
            chip = chip.dica_acao(format!("Modelo: {nome}\n{detalhe}"), Box::new(AbrirModelo), Some("Vitral"));
        }
        let lista = self.paineis[ix].busca_modelo.clone();
        let foco = lista.read(cx).focus_handle(cx);
        let (motor_e, janela, chat_id, painel) = (self.motor.clone(), cx.entity().downgrade(), chat.id.clone(), b.painel);
        let (largura, estreita) = (b.largura_popover().px(), b.estreita());
        popover(b.id("pop-modelo"))
            .anchor(Anchor::BottomLeft)
            .offset(px(8.))
            .p(r(6.))
            .open(aberto)
            .on_open_change({
                let (lista, motor_e, janela, chat_id) = (lista.clone(), motor_e.clone(), janela.clone(), chat_id.clone());
                move |abriu, window, cx| {
                    if *abriu && let Some(m) = &motor_e {
                        preparar_modelos(&lista, m, &chat_id, window, cx);
                    }
                    abrir_ou_fechar(&janela, painel, PopCaixa::Modelo, *abriu, cx);
                }
            })
            .track_focus(&foco)
            .trigger(chip.construir(window, cx))
            .content(move |_, window, cx| {
                // A escolha chega por evento; a assinatura vive só enquanto o popover está aberto.
                let assinar = {
                    let (lista, motor_e, janela, chat_id) = (lista.clone(), motor_e.clone(), janela.clone(), chat_id.clone());
                    move |_: &mut Window, cx: &mut Context<Subscription>| {
                        let app: &mut App = cx;
                        app.subscribe(&lista, move |_, evento: &EventoEscolha, cx| {
                            if let EventoEscolha::Escolheu(chave) = evento {
                                let valor = chave.to_string();
                                // A linha que só explica não escolhe nada (e nem fecha o popover).
                                if valor == SEM_VERSOES {
                                    return;
                                }
                                mudar(&motor_e, &chat_id, cx, move |c| c.modelo = Some(valor).filter(|v| !v.is_empty()));
                            }
                            abrir_ou_fechar(&janela, painel, PopCaixa::Modelo, false, cx);
                        })
                    }
                };
                let _assinatura = window.use_keyed_state(("assinatura-modelo", painel as usize), cx, assinar);
                let largura = (tam(window, largura) - tam(window, 12.)).min(window.viewport_size().width - px(44.));
                // As teclas na coluna do texto das linhas (8 do item).
                div().w(largura).flex().flex_col().child(lista.clone()).when(!estreita, |s| {
                    let [linha, teclas] = rodape_de_teclas([
                        dica_de_tecla(&["↑", "↓"], "escolher"),
                        dica_de_tecla(&["Enter"], "usa"),
                    ]);
                    s.child(div().mt(esp::E4).child(linha)).child(div().px(esp::E8).child(teclas))
                })
            })
            .into_any_element()
    }

    pub(super) fn chip_esforco(
        &self,
        motor: &Motor,
        chat: &Chat,
        ix: usize,
        b: &Barra,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let atual = indice_esforco(chat.esforco.as_deref().unwrap_or(""));
        let nome = ESFORCOS[atual].1;
        let armado = self.ultrathink.contains(&chat.id);
        let sem_ultracode = motor.motivo_sem_ultracode(chat).map(|m| SharedString::from(com_maiuscula(&m)));
        let ultracode = self.ultracode.contains(&chat.id) && sem_ultracode.is_none();
        let tem_esforco = usa_esforco(chat.modelo_atual.as_deref());
        let aberto = tem_esforco && self.pop_caixa == Some((b.painel, PopCaixa::Esforco));
        let dica: SharedString = if ultracode {
            let junto = if armado { "\nCom o Ultrathink junto" } else { "" };
            format!("Ultracode na próxima mensagem\nVários agentes em paralelo, em Extra, só nela{junto}").into()
        } else if armado {
            "Ultrathink na próxima mensagem\nPensa mais a fundo e sobe o esforço só nela".into()
        } else if atual == 0 {
            "Esforço: automático\nO que o Claude Code usa para este modelo".into()
        } else {
            format!("Esforço: {nome}").into()
        };
        let base = if ultracode {
            // Com o Ultrathink junto, as barrinhas em arco-íris; só o ícone, o do fluxo.
            let simbolo = if armado && !b.estreita() { barras_arco_iris() } else { simbolo_ultracode() };
            Chip::new(b.id("chip-esforco"), "Ultracode")
                .simbolo(simbolo)
                .cor_rotulo(Cores::violeta())
                .seta(b.larga())
                .so_icone(b.estreita())
        } else if armado {
            Chip::new(b.id("chip-esforco"), "Ultrathink")
                .simbolo(barras_arco_iris())
                .arco_iris()
                .seta(b.larga())
                .so_icone(b.estreita())
        } else {
            let chip = Chip::new(b.id("chip-esforco"), nome)
                .simbolo(Simbolo::Sinal { nivel: atual as u8, total: 5 })
                .seta(b.larga())
                .so_icone(b.estreita());
            if atual == 0 { chip.mudado(false) } else { chip.cor_rotulo(Cores::ciano()) }
        };
        let mut chip = base.tamanho(b.tamanho()).aberto(aberto);
        if !tem_esforco {
            chip = chip.desabilitado("Este modelo não usa esforço");
        } else if !aberto {
            chip = chip.dica_acao(dica, Box::new(AbrirEsforco), Some("Vitral"));
        }
        let botao = chip.construir(window, cx);
        if !tem_esforco {
            return botao.into_any_element();
        }
        let dados = Rc::new(DadosEsforco {
            painel: b.painel,
            chat: chat.id.clone(),
            atual,
            armado,
            ultracode,
            outro_ultracode: motor.ultracode_rodando().is_some_and(|c| c != chat.id),
            fluxos_desligados: motor.fluxos_desligados(chat),
            modo_pergunta: !matches!(motor.permissao_de(chat).as_str(), "bypassPermissions" | "auto"),
            mestre: chat.papel == Papel::Mestre,
            sem_ultracode,
            estreita: b.estreita(),
            largura: b.largura_popover(),
            foco: self.paineis[ix].foco_esforco.clone(),
            motor: self.motor.clone(),
            janela: cx.entity().downgrade(),
        });
        let (janela, painel) = (dados.janela.clone(), b.painel);
        popover(b.id("pop-esforco"))
            .anchor(Anchor::BottomLeft)
            .offset(px(8.))
            .open(aberto)
            .on_open_change(move |abriu, _, cx| abrir_ou_fechar(&janela, painel, PopCaixa::Esforco, *abriu, cx))
            .track_focus(&dados.foco)
            .trigger(botao)
            .content(move |_, _, _| corpo_do_esforco(&dados))
            .into_any_element()
    }

    pub(super) fn chip_conta(&self, motor: &Motor, chat: &Chat, b: &Barra, window: &Window, cx: &Context<Self>) -> AnyElement {
        let dados = Rc::new(self.dados_da_conta(motor, chat, cx));
        let padrao = if motor.config.conta.is_empty() { contas::PRINCIPAL } else { motor.config.conta.as_str() };
        let mudado = dados.escolha != padrao;
        let (nome, uso) = (dados.nome(), dados.uso());
        let cor = if dados.automatica() {
            Cores::tenue()
        } else if uso.bloqueado {
            Cores::vermelho()
        } else {
            cor_da_faixa(cinco_h(&uso).unwrap_or(0.), Cores::tenue())
        };
        let detalhe = if dados.automatica() {
            "a com mais folga no plano, a cada turno".to_string()
        } else {
            descricao_da_conta(&uso)
        };
        let onde = match chat.papel {
            Papel::Trabalhador | Papel::Projeto if b.sem_copia => "roda na pasta do projeto",
            Papel::Trabalhador | Papel::Projeto => "roda na cópia isolada",
            Papel::Coordenador | Papel::Mestre => "os chats de projeto usam a mesma",
        };
        let dica: SharedString = format!("Conta: {nome}\n{detalhe} · {onde}").into();
        let chip = |aberto: bool| {
            let mut chip = Chip::new(b.id("chip-conta"), nome.clone())
                .simbolo(Simbolo::Icone(Lucide::CircleUserRound.into(), Some(cor)))
                .tamanho(b.tamanho())
                .mudado(mudado)
                .so_icone(!b.larga())
                .aberto(aberto);
            if !aberto {
                chip = chip.dica(dica.clone());
            }
            // Nome comprido corta com reticências.
            chip.construir(window, cx).max_w(r(160.))
        };
        GatilhoChip::new(chip(false), chip(true))
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, window, _| itens_da_conta(menu, &dados, window))
            .into_any_element()
    }

    fn dados_da_conta(&self, motor: &Motor, chat: &Chat, cx: &Context<Self>) -> DadosConta {
        DadosConta {
            motor: self.motor.clone(),
            janela: cx.entity().downgrade(),
            chat: chat.id.clone(),
            escolha: motor.escolha_de_conta(chat),
            contas: motor
                .contas
                .iter()
                .map(|c| (c.id.clone(), c.nome.clone(), motor.uso_da_conta(&c.id)))
                .collect(),
        }
    }

    pub(super) fn botao_anexar(&self, b: &Barra, window: &Window, cx: &Context<Self>) -> AnyElement {
        let mestre = b.chat == crate::modelo::ID_MESTRE;
        let dados = DadosAnexar { janela: cx.entity().downgrade(), chat: b.chat.clone(), painel: b.painel, mestre };
        let dica = if mestre { "Citar um projeto ou um chat, ou anexar um arquivo" } else { "Anexar arquivo ou contexto" };
        botao_icone(b.id("anexar"), Lucide::Paperclip, dica, TamanhoBotao::Controle, window, cx)
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, window, _| {
                let largura = tam(window, 300.);
                let menu = menu.min_w(largura).max_w(largura).item(cabecalho_escolha("Adicionar à mensagem", None));
                itens_de_anexar(menu, &dados, true)
                    .separator()
                    .item(rodape_menu("Cole imagens ou arquivos direto na caixa", Some((Box::new(Paste), Some("Input")))))
            })
            .into_any_element()
    }

    /// "…": comandos, compactar, sessão nova e fixar; na faixa estreita, também o anexar e a
    /// conta, que saem da barra.
    pub(super) fn botao_mais(&self, motor: &Motor, chat: &Chat, b: &Barra, window: &Window, cx: &Context<Self>) -> AnyElement {
        let estreita = b.estreita();
        let mestre = chat.papel == Papel::Mestre;
        let anexar = DadosAnexar { janela: cx.entity().downgrade(), chat: b.chat.clone(), painel: b.painel, mestre };
        let conta = estreita.then(|| Rc::new(self.dados_da_conta(motor, chat, cx)));
        let (motor_e, fixado, rodando, painel) = (self.motor.clone(), chat.fixado, b.rodando, b.painel);
        // Com o turno rodando, o Enviar entra nele; a escolha de esperar a vez na fila fica aqui.
        // Quem já ia pela fila de qualquer jeito não precisa da opção.
        let depois = composer::dirige_o_turno(motor, &chat.id)
            .then(|| (cx.entity().downgrade(), b.composer.read(cx).value().trim().is_empty()));
        let tamanho = if estreita { TamanhoBotao::Barra } else { TamanhoBotao::Controle };
        botao_icone(b.id("mais-caixa"), Lucide::Ellipsis, "Mais opções", tamanho, window, cx)
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, window, cx| {
                let largura = tam(window, 300.);
                let (j1, j2) = (anexar.janela.clone(), anexar.janela.clone());
                let (m1, m2, c1, c2) = (motor_e.clone(), motor_e.clone(), anexar.chat.clone(), anexar.chat.clone());
                let mut menu = menu
                    .min_w(largura)
                    .max_w(largura)
                    .when_some(depois.clone(), |menu, (janela, vazio)| {
                        menu.item(
                            PopupMenuItem::new("Mandar depois, na fila")
                                .icon(Lucide::ListChecks)
                                .disabled(vazio)
                                .on_click(move |_, window, cx| {
                                    let _ = janela.update(cx, |this, cx| this.enviar_depois(painel, window, cx));
                                }),
                        )
                        .separator()
                    })
                    .item(item_de_comandos(j1, painel))
                    .item(PopupMenuItem::new("Compactar a conversa").icon(Lucide::Minimize).on_click(move |_, window, cx| {
                        let _ = j2.update(cx, |this, cx| this.por_na_caixa(painel, "/compact ", window, cx));
                    }))
                    .item(
                        PopupMenuItem::new("Sessão nova")
                            .icon(Lucide::RefreshCw)
                            .disabled(rodando)
                            .on_click(move |_, _, cx| {
                                if let Some(m) = &m1 {
                                    m.update(cx, |m, cx| m.nova_sessao(&c1, cx));
                                }
                            }),
                    )
                    // O Mestre não se fixa: ele já tem o lugar dele no trilho.
                    .when(!mestre, |menu| {
                        menu.separator().item(
                            PopupMenuItem::new(if fixado { "Desafixar conversa" } else { "Fixar conversa" })
                                .icon(if fixado { Lucide::PinOff } else { Lucide::Pin })
                                .on_click(move |_, _, cx| {
                                    if let Some(m) = &m2 {
                                        m.update(cx, |m, cx| m.alternar_fixado(&c2, cx));
                                    }
                                }),
                        )
                    });
                if estreita {
                    menu = itens_de_anexar(menu.separator(), &anexar, false);
                    if let Some(conta) = &conta {
                        let c = conta.clone();
                        menu = menu.submenu_with_icon(
                            Some(Icon::new(Lucide::CircleUserRound)),
                            format!("Conta: {}", conta.nome()),
                            window,
                            cx,
                            move |sub, window, _| itens_da_conta(sub, &c, window),
                        );
                    }
                }
                menu
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod testes {
    use super::{LinhaDeModelo, SEM_VERSOES, linhas_de_modelo, linhas_de_versao};
    use crate::claude::VERSAO_MODELOS_EXATOS;
    use crate::motor::motivo_sem_versao_exata;

    fn titulos(linhas: &[LinhaDeModelo]) -> Vec<&str> {
        linhas.iter().map(|l| l.titulo.as_str()).collect()
    }

    #[test]
    fn seletor_de_modelo_com_a_versao_em_uso() {
        let linhas = linhas_de_modelo(None, Some("claude-opus-5-5"), "");
        assert_eq!(titulos(&linhas), ["Padrão", "Claude Fable", "Claude Opus 5.5", "Claude Sonnet", "Claude Haiku"]);
        assert_eq!(linhas[0].descricao, "O do Claude Code · Opus 5.5");
        // Seguindo o Padrão, é ele que diz qual está em uso: nenhum selo.
        assert!(linhas.iter().all(|l| !l.em_uso));
        assert_eq!(linhas[2].valor, "opus");
        let linhas = linhas_de_modelo(None, Some("claude-opus-5-5"), "opus");
        assert_eq!(linhas[0].descricao, "Padrão · Opus 5.5");
        // Escolhido um apelido, o selo vai na família em uso (um só).
        let linhas = linhas_de_modelo(Some("opus"), Some("claude-opus-5-5"), "sonnet");
        assert_eq!(linhas.iter().filter(|l| l.em_uso).count(), 1);
        assert!(linhas[2].em_uso);
    }

    #[test]
    fn seletor_de_modelo_sem_dado_nao_inventa_versao() {
        let linhas = linhas_de_modelo(None, None, "");
        assert_eq!(titulos(&linhas), ["Padrão", "Claude Fable", "Claude Opus", "Claude Sonnet", "Claude Haiku"]);
        assert!(linhas.iter().all(|l| !l.em_uso));
        // Escolhido um apelido, o Padrão diz o da pasta e não o que está em uso.
        let linhas = linhas_de_modelo(Some("sonnet"), Some("claude-opus-5-5"), "opus");
        assert_eq!(linhas[0].descricao, "Padrão · Opus");
        assert_eq!(linhas[2].titulo, "Claude Opus 5.5");
        assert_eq!(linhas[3].titulo, "Claude Sonnet");
    }

    #[test]
    fn conversa_antiga_ganha_a_sexta_linha() {
        let id = "claude-sonnet-4-5-20250929";
        let linhas = linhas_de_modelo(Some(id), Some(id), "");
        assert_eq!(linhas.len(), 6);
        let ultima = &linhas[5];
        assert_eq!((ultima.valor.as_str(), ultima.titulo.as_str()), (id, "Claude Sonnet 4.5"));
        assert_eq!(ultima.descricao, "escolhido antes");
        assert!(ultima.em_uso);
        // O apelido não toma a versão do nome completo.
        assert_eq!(linhas[3].titulo, "Claude Sonnet");
        assert!(!linhas[3].em_uso);
    }

    #[test]
    fn versao_exata_escolhida_nao_vira_a_sexta_linha() {
        // Ela vive no grupo das versões, e as famílias ficam com o nome seco.
        let linhas = linhas_de_modelo(Some("claude-opus-5-5"), Some("claude-opus-5-5"), "");
        assert_eq!(titulos(&linhas), ["Padrão", "Claude Fable", "Claude Opus", "Claude Sonnet", "Claude Haiku"]);
        assert!(linhas.iter().all(|l| !l.em_uso));
    }

    #[test]
    fn as_versoes_marcam_a_que_esta_rodando() {
        let linhas = linhas_de_versao(Some("opus"), Some("claude-opus-5-5"), None);
        assert_eq!(linhas[0].titulo, "Fable 5.1");
        assert_eq!(linhas.iter().filter(|l| l.em_uso).map(|l| l.titulo.as_str()).collect::<Vec<_>>(), ["Opus 5.5"]);
        // Seguindo a família é justamente quando interessa saber qual versão o apelido pega.
        assert!(linhas.iter().all(|l| l.desabilitada.is_none()));
        assert!(linhas.iter().all(|l| l.descricao.is_empty()), "linhas de uma altura só");
        // O sufixo da janela não muda a versão: o 2.1.278 abria os turnos no claude-opus-5[1m].
        let linhas = linhas_de_versao(None, Some("claude-opus-5[1m]"), None);
        assert_eq!(linhas.iter().filter(|l| l.em_uso).map(|l| l.titulo.as_str()).collect::<Vec<_>>(), ["Opus 5"]);
    }

    #[test]
    fn claude_code_velho_explica_em_vez_de_oferecer() {
        let antigo = motivo_sem_versao_exata(Some((2, 1, 278)));
        let linhas = linhas_de_versao(None, Some("claude-opus-5[1m]"), antigo.as_deref());
        assert_eq!(linhas.len(), 1);
        assert_eq!(linhas[0].valor, SEM_VERSOES);
        assert!(linhas[0].descricao.starts_with("Precisa do Claude Code 2.1.280"), "{}", linhas[0].descricao);
        assert!(linhas[0].desabilitada.is_some());
        // Com uma versão exata já escolhida, ela continua à vista para dar para sair dela.
        let linhas = linhas_de_versao(Some("claude-opus-5-5"), Some("claude-opus-5-5"), antigo.as_deref());
        assert_eq!(titulos(&linhas), ["Sem as versões exatas", "Opus 5.5"]);
        assert!(linhas[1].em_uso && linhas[1].desabilitada.is_none());
        // Na versão em que foram conferidas, e em qualquer uma mais nova, não há motivo.
        assert!(motivo_sem_versao_exata(Some(VERSAO_MODELOS_EXATOS)).is_none());
        assert!(motivo_sem_versao_exata(Some((2, 1, 318))).is_none());
        // Sem saber a versão (a leitura ao abrir falhou), o seletor oferece: quem recusa é o
        // servidor, e o erro do turno diz o que fazer.
        assert!(motivo_sem_versao_exata(None).is_none());
    }
}
