//! Os controles da barra da caixa: os chips de modo, modelo, esforço e conta, com os menus e os
//! popovers deles, o clipe de anexar e o "…".

use std::rc::Rc;

use gpui_kit::base::actions::{Confirm, SelectFirst, SelectLast, SelectLeft, SelectRight};
use gpui_kit::component::{
    Icon, Sizable as _,
    input::TextareaState,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui_kit::component::tooltip::{ManagedTooltipExt as _, Tooltip};
use gpui_kit::component::{h_flex, v_flex};

use super::{
    AbrirEsforco, AbrirModelo, AlternarModo, AlternarUltracode, DESCRICOES_DE_MODELO, ESFORCOS,
    MODELOS, MODOS, PopCaixa, com_claude, descricao_do_esforco, descricao_do_modo, descricao_regra,
    indice_esforco, nome_curto_modelo, nome_do_modo, nomes_dos_niveis, rotulo_modelo, sem_sufixo_de_janela,
    usa_esforco, versao_da_familia, versao_exata, versoes_de_modelo,
};
use crate::ui::{Janela, composer};
use crate::ui::componentes::{
    BarrinhasDeSinal, CabecalhoPopover, Chip, ControleSegmentado, CorpoPopover, EventoEscolha,
    FaixaCaixa, GatilhoChip, GrupoDeEscolha, ItemDeEscolha, LarguraPopover, LinhaDeOpcao, ListaDeEscolha, Marca,
    Selo, Simbolo, TamanhoBotao, TamanhoChip, botao_icone,
    desenho_teclas, fio_de_pontos, icone_do_modo, item_descrito, popover, resumo_uso,
};
use crate::{
    agente_codex::{self, ModeloDoCodex},
    agente_gemini,
    contas,
    modelo::{Agente, Chat},
    motor::{FLUXOS_DESLIGADOS, Motor, Uso, motivo_sem_versao_exata},
    tema::{Cores, Lucide, Tipo as _, esp, r, raio, tam},
};

/// O que os controles da barra de um painel precisam saber.
pub(super) struct Barra {
    pub(super) painel: u64,
    pub(super) chat: String,
    pub(super) faixa: FaixaCaixa,
    pub(super) composer: Entity<TextareaState>,
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

    /// Os chips da caixa: 26 em qualquer largura (protótipo Linear, 25/11/2026).
    fn tamanho(&self) -> TamanhoChip {
        TamanhoChip::Caixa
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
            this.so_um_flutuante(crate::ui::QualFlutuante::Caixa);
            this.pop_caixa = Some((painel, qual));
        } else if this.pop_caixa == Some((painel, qual)) {
            this.pop_caixa = None;
        }
        cx.notify();
    });
}

/// Rótulo com o que fica à direita em tênue: o gatilho digitado ("@", "/") ou uma hora ("14:20"),
/// como no protótipo. Não é atalho do teclado.
pub(in crate::ui) fn rotulo_com_gatilho(rotulo: impl Into<SharedString>, gatilho: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(esp::E8)
        .min_w_0()
        .child(div().flex_1().min_w_0().truncate().child(rotulo.into()))
        .child(div().flex_none().metadado().text_color(Cores::tenue()).child(gatilho.into()))
}

/// Uma tecla (ou um par) e o que ela faz, no desenho das teclas, como no rodapé da paleta.
pub(super) fn dica_de_tecla(teclas: &[&'static str], texto: &'static str) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(esp::E4)
        .child(desenho_teclas(teclas.iter().map(|t| SharedString::from(*t)).collect()))
        .child(texto)
}

/// O rodapé de teclas dos popovers da caixa (28, 12 tênue), com o fio de pontos em cima: as
/// teclas que o popover aceita. Se não couber numa linha, quebra em vez de cortar.
fn rodape_de_teclas(dicas: impl IntoIterator<Item = Div>, window: &Window) -> [AnyElement; 2] {
    [
        fio_de_pontos(Cores::fio(), window).into_any_element(),
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

// ── Modo ────────────────────────────────────────────────────────────────

struct DadosModo {
    motor: Option<Entity<Motor>>,
    chat: String,
    atual: String,
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
    // Linear (26/11/2026): como no protótipo, a regra em mono e o × no fim, sem o ícone.
    let _ = icone;
    PopupMenuItem::element(move |window, _| {
        div()
            .flex()
            .items_center()
            .gap(esp::E8)
            .min_w_0()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .map(|s| {
                        if comando {
                            s.mono_pequeno().text_color(Cores::texto())
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
    // Linear (26/11/2026): o menu de modo do protótipo, sem cabeçalho, uma linha por modo (o
    // ícone na cor dele, o nome e a descrição lado a lado), o escolhido na cor da seleção. O
    // Shift+Tab continua trocando o modo, como diz a dica do chip.
    let largura = tam(window, 300.);
    menu = menu.min_w(largura).max_w(largura);
    for (valor, nome, descricao) in MODOS {
        let (icone, cor) = icone_do_modo(valor);
        let d2 = d.clone();
        menu = menu.item(
            item_descrito(Some(Simbolo::Icone(icone, Some(cor))), nome, descricao, valor == d.atual)
                // O foco volta para a caixa, como no mouse e nos outros seletores: pelo teclado
                // ele ficava no chip, e a letra seguinte se perdia (teste de usabilidade, 16/11/2026).
                .on_click(move |_, window, cx| {
                    mudar(&d2.motor, &d2.chat, cx, |c| c.permissao = Some(valor.into()));
                    window.focus(&d2.foco_caixa, cx);
                }),
        );
    }
    if !d.permitidas.is_empty() {
        // "Sempre permitido" e, na mesma linha, o Limpar (volta a perguntar sobre tudo).
        let d2 = d.clone();
        menu = menu.separator().item(
            PopupMenuItem::element(|_, _| {
                div()
                    .flex()
                    .items_center()
                    .w_full()
                    .text_size(r(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(div().flex_1().text_color(Cores::apagado()).child("Sempre permitido"))
                    .child(div().font_weight(FontWeight::MEDIUM).text_color(Cores::tenue()).child("Limpar"))
            })
            .on_click(move |_, _, cx| mudar(&d2.motor, &d2.chat, cx, |c| c.permitidas.clear())),
        );
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
    }
    menu
}

// ── Conta ───────────────────────────────────────────────────────────────

fn cinco_h(uso: &Uso) -> Option<f32> {
    uso.cinco_horas.map(|f| f as f32)
}

fn descricao_da_conta(uso: &Uso) -> String {
    let resumo = resumo_uso(cinco_h(uso), uso.semana.map(|f| f as f32));
    if uso.bloqueado { format!("limite atingido · {resumo}") } else { resumo }
}

// ── Anexar ──────────────────────────────────────────────────────────────

#[derive(Clone)]
struct DadosAnexar {
    janela: WeakEntity<Janela>,
    chat: String,
    painel: u64,
}

fn itens_de_anexar(menu: PopupMenu, d: &DadosAnexar, com_comandos: bool) -> PopupMenu {
    let (j1, j2, j3, j4) = (d.janela.clone(), d.janela.clone(), d.janela.clone(), d.janela.clone());
    let (c1, c2, c3, painel) = (d.chat.clone(), d.chat.clone(), d.chat.clone(), d.painel);
    let menu = menu
        .item(
            // Os nomes curtos do protótipo (26/11/2026): a seção "Anexar" já diz o resto.
            PopupMenuItem::new("Arquivo…")
                .icon(Lucide::Paperclip)
                .on_click(move |_, window, cx| {
                    let chat = c1.clone();
                    let _ = j1.update(cx, |this, cx| this.escolher_anexos(chat, window, cx));
                }),
        )
        .item(
            PopupMenuItem::element(|_, _| rotulo_com_gatilho("Da pasta…", "@"))
                .icon(Lucide::Folder)
                .on_click(move |_, window, cx| {
                    let chat = c2.clone();
                    let _ = j2.update(cx, |this, cx| this.dialogo_mencionar_arquivo(chat, window, cx));
                }),
        )
        .item(
            PopupMenuItem::new("Outra conversa…")
                .icon(Lucide::MessagesSquare)
                .on_click(move |_, window, cx| {
                    let chat = c3.clone();
                    let _ = j3.update(cx, |this, cx| this.dialogo_conversa_contexto(chat, window, cx));
                }),
        );
    if com_comandos { menu.item(item_de_comandos(j4, painel)) } else { menu }
}

fn item_de_comandos(janela: WeakEntity<Janela>, painel: u64) -> PopupMenuItem {
    PopupMenuItem::element(|_, _| rotulo_com_gatilho("Comandos", "/"))
        .icon(Lucide::Slash)
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
    /// A versão da família, no selo à direita ("5.2"): assim a descrição não é cortada.
    versao: Option<String>,
    em_uso: bool,
    /// Por que a linha não dá para escolher (o Claude Code velho não aceita as versões novas).
    desabilitada: Option<String>,
    /// A linha abre outras no lugar ("Outras versões", a conta): aberta ou fechada.
    seta: Option<bool>,
}

impl LinhaDeModelo {
    fn new(valor: impl Into<String>, titulo: impl Into<String>, descricao: impl Into<String>) -> Self {
        LinhaDeModelo {
            valor: valor.into(),
            titulo: titulo.into(),
            descricao: descricao.into(),
            versao: None,
            em_uso: false,
            desabilitada: None,
            seta: None,
        }
    }
}

/// Só os números de um nome de modelo: "Opus 5.5" da família "Opus" é "5.5".
fn so_a_versao(nome: &str, familia: &str) -> Option<String> {
    let resto = nome.strip_prefix(familia)?.trim();
    (!resto.is_empty()).then(|| resto.to_string())
}

/// As linhas do primeiro grupo: Padrão, as quatro famílias e, numa conversa antiga que guardou o
/// nome completo, uma sexta com ele. O título é o nome seco da família, e a versão vai no selo:
/// a que está rodando, quando a conversa segue aquela família (nunca inventada), ou a que o
/// apelido pega hoje no Claude Code instalado, quando ele já disse. Seguindo o Padrão, é ele que
/// diz qual está em uso ("Padrão · Opus 5.5"), e a família não ganha o "em uso". Uma versão exata
/// escolhida está no segundo grupo, não aqui.
fn linhas_de_modelo(escolhido: Option<&str>, em_uso: Option<&str>, padrao_pasta: &str) -> Vec<LinhaDeModelo> {
    let em_uso = em_uso.filter(|m| !m.is_empty());
    let curto = em_uso.map(nome_curto_modelo);
    let apelido = |v: &str| MODELOS.iter().any(|(m, _)| *m == v);
    let segue_apelido = escolhido.is_none_or(apelido);
    let no_padrao = escolhido.is_none();
    // Sem a linha "Padrão" (Linear, 25/11/2026: "tira esses padrão e deixa igual funcionava no
    // vscode"): a lista é só dos modelos, e o que está valendo leva o visto (`chave_do_modelo`).
    let _ = padrao_pasta;
    let mut linhas = Vec::new();
    for ((valor, familia), descricao) in MODELOS[1..].iter().zip(DESCRICOES_DE_MODELO) {
        let rodando = curto.clone().filter(|m| segue_apelido && m.to_lowercase().starts_with(valor));
        let versao = rodando.clone().or_else(|| versao_da_familia(valor)).and_then(|nome| so_a_versao(&nome, familia));
        linhas.push(LinhaDeModelo {
            em_uso: rodando.is_some() && !no_padrao,
            versao,
            ..LinhaDeModelo::new(*valor, *familia, descricao)
        });
    }
    if let Some(antigo) = escolhido.filter(|e| !apelido(e) && !versao_exata(e)) {
        linhas.push(LinhaDeModelo {
            em_uso: em_uso == Some(antigo),
            ..LinhaDeModelo::new(antigo, nome_curto_modelo(antigo), "escolhido antes")
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
    versoes_de_modelo()
        .iter()
        .map(|id| LinhaDeModelo {
            em_uso: rodando == Some(id.as_str()),
            ..LinhaDeModelo::new(id.as_str(), nome_curto_modelo(id), "")
        })
        .collect()
}

 fn padrao_da_pasta<'a>(motor: &'a Motor, _chat: &Chat) -> &'a str {
    &motor.config.modelo_chats
}

/// No jeito do protótipo (26/11/2026): cada modelo com o glifo da marca e a versão no nome
/// ("Opus 5.5  Dia a dia"); "Outras versões" com a seta na coluna do símbolo; a conta com a
/// pessoa. As contas e a linha sem versões vão com o símbolo vazio, para o texto alinhar.
fn itens_de_modelo(linhas: Vec<LinhaDeModelo>, marca: Marca) -> Vec<ItemDeEscolha> {
    linhas
        .into_iter()
        .map(|l| {
            let termos = format!("{} {}", l.valor, l.descricao);
            let (simbolo, seta) = match l.valor.as_str() {
                OUTRAS_VERSOES => {
                    let aberta = l.seta == Some(true);
                    (Simbolo::icone(if aberta { Lucide::ChevronDown } else { Lucide::ChevronRight }), None)
                }
                A_CONTA => (Simbolo::icone(Lucide::CircleUserRound), l.seta),
                v if v.starts_with(DA_CONTA) || v.starts_with(DA_CONTA_CODEX) || v == SEM_VERSOES => (Simbolo::Icone(Icon::empty(), None), l.seta),
                _ => (Simbolo::Glifo(marca), l.seta),
            };
            let titulo = match &l.versao {
                Some(versao) => format!("{} {versao}", l.titulo),
                None => l.titulo,
            };
            ItemDeEscolha {
                simbolo: Some(simbolo),
                descricao: (!l.descricao.is_empty()).then(|| l.descricao.into()),
                selos: l.em_uso.then(|| Selo::neutro("em uso")).into_iter().collect(),
                termos: Some(termos.into()),
                desabilitado: l.desabilitada.map(Into::into),
                seta,
                ..ItemDeEscolha::new(l.valor, titulo)
            }
        })
        .collect()
}

/// O prefixo das chaves do grupo do Codex no seletor: `codex:` sozinho é o padrão dele, e
/// `codex:<slug>` um modelo do catálogo. Escolher uma delas passa o chat para o Codex.
const DO_CODEX: &str = "codex:";

/// O prefixo das chaves do grupo do Gemini no seletor: `gemini:` é o padrão dele e
/// `gemini:<valor>` um modelo da conta. Escolher uma delas passa o chat para o Gemini.
const DO_GEMINI: &str = "gemini:";

/// A chave do seletor que corresponde ao que o chat usa hoje.
fn chave_do_modelo(chat: &Chat, padrao_pasta: &str) -> String {
    let modelo = chat.modelo.clone().unwrap_or_default();
    match chat.agente {
        // Seguindo o padrão (sem linha "Padrão" desde o Linear), o visto vai na família que vale:
        // a do padrão das Configurações ou, sem ele, a do modelo que está rodando.
        Agente::Claude if modelo.is_empty() => {
            let base = if padrao_pasta.is_empty() { chat.modelo_atual.clone().unwrap_or_default() } else { padrao_pasta.to_string() };
            let base = base.to_lowercase();
            MODELOS[1..].iter().map(|(v, _)| *v).find(|v| base.contains(v)).unwrap_or_default().to_string()
        }
        Agente::Claude => modelo,
        Agente::Codex => format!("{DO_CODEX}{modelo}"),
        outro => format!("{}{modelo}", prefixo_acp(outro)),
    }
}

/// O prefixo das chaves de um agente ACP no seletor: `gemini:` no Gemini (o de antes) e
/// `<nome>:` nos novos ("copilot:", "kimi:").
fn prefixo_acp(agente: Agente) -> String {
    if agente == Agente::Gemini { DO_GEMINI.to_string() } else { format!("{}:", agente.nome().to_lowercase()) }
}

/// O agente ACP e o modelo de uma chave do seletor ("kimi:k2" é o Kimi com o k2).
fn agente_acp_da_chave(valor: &str) -> Option<(Agente, String)> {
    Agente::TODOS.into_iter().filter(|a| a.acp()).find_map(|a| valor.strip_prefix(&prefixo_acp(a)).map(|resto| (a, resto.to_string())))
}

/// As linhas de um agente ACP novo: os modelos que a última sessão dele ofereceu (vazio antes da
/// primeira conversa: fica o Padrão, que passa o chat para ele) e o escolhido que saiu da lista.
fn linhas_acp(agente: Agente, escolhido: Option<&str>, oferecidos: &[agente_gemini::ModeloDoGemini]) -> Vec<LinhaDeModelo> {
    let prefixo = prefixo_acp(agente);
    let mut linhas: Vec<LinhaDeModelo> = oferecidos.iter().map(|m| LinhaDeModelo::new(format!("{prefixo}{}", m.valor), m.nome.clone(), "")).collect();
    if linhas.is_empty() {
        linhas.push(LinhaDeModelo::new(prefixo.clone(), "Padrão", format!("O modelo que o {} usa", agente.nome())));
    }
    if let Some(antigo) = escolhido.filter(|e| !e.is_empty() && !oferecidos.iter().any(|m| m.valor == *e)) {
        linhas.push(LinhaDeModelo::new(format!("{prefixo}{antigo}"), antigo, "escolhido antes"));
    }
    linhas
}

/// As linhas do Gemini: o padrão dele e os modelos que a conta tem no Antigravity (o Gemini, e
/// também os de outras casas que o Antigravity oferece). Um escolhido que saiu da lista continua
/// nela, para dar para ver e sair dele.
fn linhas_do_gemini(escolhido: Option<&str>, oferecidos: &[agente_gemini::ModeloDoGemini]) -> Vec<LinhaDeModelo> {
    let mut linhas = Vec::new();
    linhas.extend(oferecidos.iter().map(|m| {
        let descricao = if m.valor.starts_with("gemini") { "do Google" } else { "pelo Antigravity" };
        LinhaDeModelo::new(format!("{DO_GEMINI}{}", m.valor), m.nome.clone(), descricao)
    }));
    if let Some(antigo) = escolhido.filter(|e| !oferecidos.iter().any(|m| m.valor == *e)) {
        linhas.push(LinhaDeModelo::new(format!("{DO_GEMINI}{antigo}"), antigo, "escolhido antes"));
    }
    linhas
}

/// A descrição de um modelo do Codex em português. O catálogo dele vem em inglês e muda a cada
/// versão: só as frases reconhecidas pelas palavras viram descrição; o resto fica numa linha só.
fn descricao_do_codex(original: &str) -> &'static str {
    let o = original.to_lowercase();
    let tem = |palavras: &[&str]| palavras.iter().any(|p| o.contains(p));
    if tem(&["older", "previous generation"]) {
        "Geração anterior"
    } else if tem(&["frontier", "most demanding", "most capable"]) {
        "O mais capaz, para o trabalho mais difícil"
    } else if tem(&["workhorse"]) {
        "Para código e o trabalho do dia a dia"
    } else if tem(&["fast", "affordable", "efficient"]) {
        "Rápido e econômico, para tarefas fáceis"
    } else {
        ""
    }
}

/// As linhas do Codex: o padrão dele e os modelos que o Codex instalado oferece. Um modelo
/// escolhido que saiu do catálogo continua na lista, para dar para ver e sair dele.
fn linhas_do_codex(escolhido: Option<&str>, oferecidos: &[ModeloDoCodex]) -> Vec<LinhaDeModelo> {
    let mut linhas = Vec::new();
    linhas.extend(
        oferecidos.iter().map(|m| LinhaDeModelo::new(format!("{DO_CODEX}{}", m.slug), m.nome.clone(), descricao_do_codex(&m.descricao))),
    );
    if let Some(antigo) = escolhido.filter(|e| !oferecidos.iter().any(|m| m.slug == *e)) {
        linhas.push(LinhaDeModelo::new(format!("{DO_CODEX}{antigo}"), antigo, "escolhido antes"));
    }
    linhas
}

/// O seletor tem as abas dos agentes: com o Codex ou o Gemini instalado, ou num chat que já é de
/// um deles.
fn com_abas_de_agente(chat: &Chat) -> bool {
    abas_de_agente(chat).len() > 1
}

/// As abas do seletor: o Claude sempre, e os outros instalados (ou o do chat).
fn abas_de_agente(chat: &Chat) -> Vec<Agente> {
    Agente::TODOS.into_iter().filter(|a| *a == Agente::Claude || *a == chat.agente || a.instalado()).collect()
}

/// As chaves das linhas que abrem outras no lugar: escolher uma não muda modelo nem conta.
const OUTRAS_VERSOES: &str = "+outras-versoes";
const A_CONTA: &str = "+conta";

/// O que está aberto no seletor de modelo do painel: as versões exatas e a escolha da conta. Os
/// dois começam recolhidos, para o seletor caber sem rolagem (dono, 24/11/2026: "melhora esse menu
/// aqui", com o print do menu que misturava modelo, conta, versões, aviso e a chave da dupla).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Abertos {
    versoes: bool,
    conta: bool,
}

impl Abertos {
    /// Ao abrir: tudo recolhido, menos as versões quando a conversa escolheu uma delas (o visto
    /// do escolhido fica à vista).
    fn de_partida(chat: &Chat) -> Self {
        Abertos { versoes: chat.agente == Agente::Claude && chat.modelo.as_deref().is_some_and(versao_exata), conta: false }
    }

    /// A linha de abrir `chave` foi escolhida: abre o que estava fechado e fecha o que estava aberto.
    fn alternar(mut self, chave: &str) -> Self {
        match chave {
            OUTRAS_VERSOES => self.versoes = !self.versoes,
            A_CONTA => self.conta = !self.conta,
            _ => {}
        }
        self
    }
}

/// A conta no pé do seletor do Claude: a linha que diz qual o chat usa e quanto ela gastou, e as
/// linhas que ela abre (cada conta e a Automática).
#[derive(Clone, Debug, PartialEq)]
struct PeDaConta {
    rotulo: String,
    contas: Vec<LinhaDeModelo>,
}

/// Tudo o que uma aba do seletor tem, antes de decidir o que fica à vista (`grupos_a_vista`).
#[derive(Clone, Debug, Default, PartialEq)]
struct PartesDoSeletor {
    /// Os grupos sempre abertos: os perfis e as famílias do Claude, ou os modelos do Codex e do Gemini.
    grupos: Vec<(Option<String>, Vec<LinhaDeModelo>)>,
    /// As versões exatas, atrás da linha "Outras versões" (só na aba do Claude).
    versoes: Vec<LinhaDeModelo>,
    /// A conta (só na aba do Claude e com mais de uma conta).
    conta: Option<PeDaConta>,
}

/// A linha casa com a busca: o título, a chave ou a descrição, sem caixa (como o `Command` do kit
/// fazia com o rótulo e os termos).
fn casa_com_a_busca(linha: &LinhaDeModelo, busca: &str) -> bool {
    let busca = busca.to_lowercase();
    [&linha.titulo, &linha.valor, &linha.descricao].into_iter().any(|t| t.to_lowercase().contains(&busca))
}

/// O que o seletor mostra. Sem busca: os grupos, com "Outras versões" no fim das famílias (as
/// versões logo embaixo dela, se aberta) e a conta numa linha no pé (as contas embaixo dela, se
/// aberta). Com busca: tudo o que casa, as versões e as contas inclusive, sem as linhas de abrir.
fn grupos_a_vista(partes: &PartesDoSeletor, abertos: Abertos, busca: &str) -> Vec<(Option<String>, Vec<LinhaDeModelo>)> {
    let busca = busca.trim();
    let mut grupos = partes.grupos.clone();
    let (versoes, contas) = if busca.is_empty() {
        let abrir = |chave: &str, rotulo: &str, aberta: bool, de_dentro: &[LinhaDeModelo]| {
            let linha = LinhaDeModelo { seta: Some(aberta), ..LinhaDeModelo::new(chave, rotulo, "") };
            std::iter::once(linha).chain(de_dentro.iter().filter(|_| aberta).cloned()).collect::<Vec<_>>()
        };
        let versoes = if partes.versoes.is_empty() { Vec::new() } else { abrir(OUTRAS_VERSOES, "Outras versões", abertos.versoes, &partes.versoes) };
        let contas = partes.conta.as_ref().map(|pe| abrir(A_CONTA, &pe.rotulo, abertos.conta, &pe.contas)).unwrap_or_default();
        (versoes, contas)
    } else {
        for (_, linhas) in &mut grupos {
            linhas.retain(|l| casa_com_a_busca(l, busca));
        }
        let casam = |linhas: &[LinhaDeModelo]| linhas.iter().filter(|l| casa_com_a_busca(l, busca)).cloned().collect::<Vec<_>>();
        (casam(&partes.versoes), partes.conta.as_ref().map(|pe| casam(&pe.contas)).unwrap_or_default())
    };
    match grupos.last_mut() {
        Some((_, linhas)) => linhas.extend(versoes),
        None => grupos.push((None, versoes)),
    }
    grupos.push((None, contas));
    grupos.retain(|(_, linhas)| !linhas.is_empty());
    grupos
}

/// As linhas viram os itens da lista.
fn em_grupos(grupos: Vec<(Option<String>, Vec<LinhaDeModelo>)>, marca: Marca) -> Vec<GrupoDeEscolha> {
    grupos.into_iter().map(|(titulo, linhas)| GrupoDeEscolha { titulo: titulo.map(Into::into), itens: itens_de_modelo(linhas, marca) }).collect()
}

/// As partes da aba `aba` do seletor. Claude: as famílias, que é o que se usa no dia a dia, as
/// versões exatas e a conta. Codex e Gemini: o padrão e o catálogo de cada um. Na aba que não é a
/// do chat, nada aparece como escolhido nem em uso.
fn partes_do_seletor(motor: &Motor, chat: &Chat, aba: Agente) -> PartesDoSeletor {
    let do_chat = chat.agente == aba;
    let escolhido = chat.modelo.as_deref().filter(|_| do_chat);
    // Os perfis do agente da aba vão no alto: são o atalho de quem os criou. Com eles, a lista de
    // modelos embaixo ganha o rótulo, para não parecer mais um perfil.
    // Sem os Perfis (Linear, 25/11/2026: "pode tirar os perfis").
    let mut partes = PartesDoSeletor::default();
    let titulo_dos_modelos = || None::<String>;
    match aba {
        Agente::Codex => {
            partes.grupos.push((titulo_dos_modelos(), linhas_do_codex(escolhido, &agente_codex::modelos())));
            partes.conta = pe_da_conta_codex(motor, chat);
        }
        Agente::Gemini => partes.grupos.push((titulo_dos_modelos(), linhas_do_gemini(escolhido, &agente_gemini::modelos()))),
        Agente::Claude => {
            let em_uso = chat.modelo_atual.as_deref().filter(|_| do_chat);
            // Com as abas, o nome do agente já está nelas.
            let titulo = if com_abas_de_agente(chat) { titulo_dos_modelos() } else { Some("Claude".into()) };
            partes.grupos.push((titulo, linhas_de_modelo(escolhido, em_uso, padrao_da_pasta(motor, chat))));
            partes.versoes = linhas_de_versao(escolhido, em_uso, motivo_sem_versao_exata(motor.versao_claude).as_deref());
            // A conta é do Claude, então mora na aba dele, e não solta na caixa (dono, 16/11/2026:
            // "por que tem essa conta aqui se agora tem o Codex também?"). Só com mais de uma conta.
            partes.conta = pe_da_conta(motor, chat);
        }
        outro => partes.grupos.push((titulo_dos_modelos(), linhas_acp(outro, escolhido, &agente_gemini::modelos_de(outro)))),
    }
    partes
}

/// O prefixo das chaves do grupo "Conta do Claude" no seletor: `conta:<id>`, ou `conta:auto`.
const DA_CONTA: &str = "conta:";
/// O prefixo das chaves das contas do Codex no seletor: `conta-codex:<id>`.
const DA_CONTA_CODEX: &str = "conta-codex:";

/// A linha da conta no pé do seletor: qual o chat usa agora e quanto ela gastou ("tecnologia ·
/// semana 44%"). Na Automática, a conta que ela escolheu.
fn rotulo_da_conta(nome: &str, automatica: bool, uso: &str) -> String {
    // Como no protótipo ("principal · 18%"): o ícone da pessoa já diz que é a conta.
    let conta = if automatica { format!("Automática: {nome}") } else { nome.to_string() };
    format!("{conta} · {uso}")
}

/// A conta do Claude no pé do seletor: a linha que a resume e, embaixo dela quando aberta, cada
/// conta com o uso do plano dela e a Automática. A que o chat escolheu leva o selo "em uso" (o
/// visto do seletor é do modelo); a sem login diz isso. Só com mais de uma conta.
fn pe_da_conta(motor: &Motor, chat: &Chat) -> Option<PeDaConta> {
    if motor.contas.len() < 2 {
        return None;
    }
    let escolha = motor.escolha_de_conta(chat);
    let uso_de = |id: &str| {
        if motor.conta_sem_login(id) {
            "Sem login: entre de novo em Contas do Claude".to_string()
        } else {
            com_maiuscula(&descricao_da_conta(&motor.uso_da_conta(id)))
        }
    };
    let mut linhas: Vec<LinhaDeModelo> = motor
        .contas
        .iter()
        .map(|c| LinhaDeModelo { em_uso: c.id == escolha, ..LinhaDeModelo::new(format!("{DA_CONTA}{}", c.id), c.nome.clone(), uso_de(&c.id)) })
        .collect();
    linhas.push(LinhaDeModelo {
        em_uso: escolha == contas::AUTOMATICA,
        ..LinhaDeModelo::new(format!("{DA_CONTA}{}", contas::AUTOMATICA), "Automática", "A conta com mais folga no plano")
    });
    let usada = motor.conta_para(chat);
    let uso = if motor.conta_sem_login(&usada.id) { "sem login".to_string() } else { descricao_da_conta(&motor.uso_da_conta(&usada.id)) };
    Some(PeDaConta { rotulo: rotulo_da_conta(&usada.nome, escolha == contas::AUTOMATICA, &uso), contas: linhas })
}

/// A conta do Codex no pé do seletor, como a do Claude: a em uso e, aberta, cada conta com o
/// login dela. Só com mais de uma conta do Codex.
fn pe_da_conta_codex(motor: &Motor, chat: &Chat) -> Option<PeDaConta> {
    if motor.contas_codex.len() < 2 {
        return None;
    }
    let usada = motor.conta_codex_para(chat);
    let contas = motor
        .contas_codex
        .iter()
        .map(|c| LinhaDeModelo {
            em_uso: c.id == usada.id,
            ..LinhaDeModelo::new(format!("{DA_CONTA_CODEX}{}", c.id), c.nome.clone(), if c.logada() { "Com login" } else { "Sem login: entre em Configurações, Agentes, Codex" })
        })
        .collect();
    Some(PeDaConta { rotulo: format!("{} · {}", usada.nome, if usada.logada() { "com login" } else { "sem login" }), contas })
}

/// A linha discreta embaixo da lista: trocar o modelo de uma conversa do Claude já grande
/// (`Chat::contexto_grande`, de 100 mil tokens para cima) faz a próxima mensagem reler tudo, sem
/// o desconto do que estava guardado. Na aba de outro agente, ou com a conversa pequena, nada.
fn aviso_de_troca(chat: &Chat, aba: Agente) -> Option<String> {
    chat.contexto_grande().filter(|_| aba == Agente::Claude).map(|t| format!("Trocar agora relê {}", super::tokens(t)))
}

/// Ao abrir o seletor (clique ou Ctrl+Shift+M), ao trocar de aba e ao abrir ou fechar uma das
/// linhas de abrir: as linhas de agora, a busca limpa e o realce no escolhido (ou em `cursor`, a
/// linha de abrir que foi clicada). `aba` é a aba à vista, sem ela a do agente do chat; `abertos`,
/// o que está aberto, sem ele o de partida.
#[allow(clippy::too_many_arguments)]
pub(super) fn preparar_modelos(
    lista: &Entity<ListaDeEscolha>,
    motor: &Entity<Motor>,
    chat: &str,
    aba: Option<Agente>,
    abertos: Option<Abertos>,
    cursor: Option<SharedString>,
    window: &mut Window,
    cx: &mut App,
) {
    let dados = {
        let m = motor.read(cx);
        m.chat(chat).map(|c| {
            let abertos = abertos.unwrap_or_else(|| Abertos::de_partida(c));
            let aba = aba.unwrap_or(c.agente);
            (partes_do_seletor(m, c, aba), chave_do_modelo(c, padrao_da_pasta(m, c)), abertos, Marca::do_agente(aba))
        })
    };
    let Some((partes, escolhido, abertos, marca)) = dados else {
        return;
    };
    let grupos = em_grupos(grupos_a_vista(&partes, abertos, ""), marca);
    lista.update(cx, |l, cx| {
        // Como no protótipo (26/11/2026): sem a busca (a lista é curta), uma linha por modelo,
        // com o glifo da marca em cada um.
        l.sem_busca(true);
        l.uma_linha(true);
        l.ficha_no_escolhido(true);
        l.cursor_em(cursor);
        l.filtro_proprio(move |busca| em_grupos(grupos_a_vista(&partes, abertos, busca), marca));
        l.definir(grupos, Some(escolhido.into()), cx);
        l.abrir(window, cx);
    });
}

/// Uma das linhas de abrir foi escolhida: abre ou fecha o que ela guarda, no lugar, com o popover
/// aberto e o realce nela.
#[allow(clippy::too_many_arguments)]
fn alternar_aberto(
    chave: &str,
    lista: &Entity<ListaDeEscolha>,
    motor: &Entity<Motor>,
    janela: &WeakEntity<Janela>,
    chat: &str,
    painel: u64,
    window: &mut Window,
    cx: &mut App,
) {
    let Ok((aba, guardados)) = janela.update(cx, |this, _| {
        (
            this.aba_do_modelo.filter(|(p, _)| *p == painel).map(|(_, a)| a),
            this.abertos_do_modelo.filter(|(p, _)| *p == painel).map(|(_, a)| a),
        )
    }) else {
        return;
    };
    let Some(antes) = guardados.or_else(|| motor.read(cx).chat(chat).map(Abertos::de_partida)) else {
        return;
    };
    let abertos = antes.alternar(chave);
    let _ = janela.update(cx, |this, _| this.abertos_do_modelo = Some((painel, abertos)));
    preparar_modelos(lista, motor, chat, aba, Some(abertos), Some(chave.to_string().into()), window, cx);
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
    /// O nível que o automático usa neste chat (o índice em `ESFORCOS`), se já se sabe.
    padrao: Option<usize>,
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

/// O que a linha do Ultracode diz embaixo do título, numa linha só: o detalhe vai na dica do (i)
/// (dono, 24/11/2026: "diminui essa parte escrita do Ultracode").
const ULTRACODE_CURTO: &str = "Agentes em paralelo";
const ULTRACODE_DICA: &str = "Vários agentes trabalham em paralelo, só na próxima mensagem. Gasta bem mais do plano e roda \
                              no esforço Extra.";

/// O que atrapalha o Ultracode agora (a vez de outra conversa, os fluxos desligados, o modo que
/// pergunta): cada um com a frase curta, em âmbar embaixo da linha, e o porquê, que vai na dica.
/// O custo de sempre está na linha curta e na dica.
fn avisos_do_ultracode(outro_ultracode: bool, fluxos_desligados: bool, modo_pergunta: bool) -> Vec<(&'static str, String)> {
    let mut avisos = Vec::new();
    if outro_ultracode {
        avisos.push((
            "Roda em outra conversa: esta iria sem ele",
            "Um Ultracode por vez no Vitral: com outro rodando, esta mensagem vai sem ele, com o motivo na conversa.".into(),
        ));
    }
    if fluxos_desligados {
        avisos.push(("Fluxos de agentes desligados nesta conta", format!("{}.", com_maiuscula(FLUXOS_DESLIGADOS))));
    }
    if modo_pergunta {
        avisos.push((
            "Neste modo, os agentes podem ter ações recusadas",
            "Os agentes do fluxo não conseguem pedir permissão: comandos e edições deles podem ser recusados. Use Sem \
             perguntar ou Automático."
                .into(),
        ));
    }
    avisos
}

/// A linha do Ultracode: vale só para a próxima mensagem, com o custo numa linha curta e, embaixo,
/// só o que pode atrapalhar, também curto (o porquê vai na dica).
fn linha_ultracode(d: &Rc<DadosEsforco>) -> AnyElement {
    let avisos = avisos_do_ultracode(d.outro_ultracode, d.fluxos_desligados, d.modo_pergunta);
    let dica = std::iter::once(ULTRACODE_DICA.to_string()).chain(avisos.iter().map(|(_, porque)| porque.clone())).collect::<Vec<_>>().join("\n\n");
    let d1 = d.clone();
    let linha = LinhaDeOpcao::new(("ultracode", d.painel as usize), "Ultracode")
        .simbolo(Lucide::Workflow, Cores::violeta())
        .info(dica)
        .descricao(ULTRACODE_CURTO)
        .interruptor(d.ultracode, move |ligado, _, cx| d1.armar_ultracode(ligado, cx));
    let linha = if avisos.is_empty() { linha } else { linha.aviso(avisos.iter().map(|(curto, _)| *curto).collect::<Vec<_>>().join("\n")) };
    let Some(motivo) = &d.sem_ultracode else {
        return linha.into_any_element();
    };
    // Desabilitada, a linha mostra o motivo curto.
    linha.desabilitada(motivo.clone()).into_any_element()
}

fn corpo_do_esforco(d: &Rc<DadosEsforco>, window: &Window) -> impl IntoElement + use<> {
    let atual = d.atual;
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
    let (d2, d4) = (d.clone(), d.clone());
    div()
        .flex()
        .flex_col()
        .track_focus(&d.foco)
        .key_context("Esforco")
        .on_action(move |_: &SelectLeft, _, cx| anterior(cx))
        .on_action(move |_: &SelectRight, _, cx| seguinte(cx))
        .on_action(move |_: &SelectFirst, _, cx| primeiro(cx))
        .on_action(move |_: &SelectLast, _, cx| ultimo(cx))
        .on_action(move |_: &AlternarUltracode, _, cx| d4.armar_ultracode(!d4.ultracode, cx))
        .on_action(move |_: &Confirm, _, cx| abrir_ou_fechar(&d2.janela, d2.painel, PopCaixa::Esforco, false, cx))
        .child(
            CorpoPopover::new(d.largura)
                // O nível já está na pílula do segmentado, logo abaixo.
                // Linear (26/11/2026): "Esforço" e o atalho à direita, como no protótipo.
                .cabecalho(CabecalhoPopover::Escolha {
                    simbolo: None,
                    titulo: "Esforço".into(),
                    valor: None,
                    dica: gpui_kit::component::kbd::Kbd::binding_for_action(&AbrirEsforco, Some("Vitral"), window)
                        .map(|k| SharedString::from(k.partes().join(" "))),
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
                            // O nível escolhido é a ficha de metal, como o escolhido dos menus.
                            .solto(true)
                            .large()
                        })
                        // No automático, qual nível o modelo usa (dono, 13/11/2026: "faz no auto
                        // mostrar qual tá usando").
                        .child(div().metadado().text_color(Cores::tenue()).child(match d.padrao.filter(|_| atual == 0) {
                            Some(p) => SharedString::from(format!("Usa o padrão do modelo, que agora é o {}.", ESFORCOS[p].1)),
                            None => descricao_do_esforco(atual).into(),
                        })),
                )
                .rodape(
                    // A linha de opção tem margem própria; aqui ela alinha com o resto do popover.
                    div()
                        .flex()
                        .flex_col()
                        .mx(r(-12.))
                        .my(r(-6.))
                        // Sem o Ultrathink: no Claude Code de agora a palavra não faz nada (o
                        // `ultrathink_available` do init vem falso) e o que sobrava era só subir o
                        // esforço desta mensagem para Extra, o mesmo que escolher o Extra aqui em
                        // cima (dono, 17/11/2026: "esse funciona mesmo? se não funcionar tira").
                        .child(linha_ultracode(d)),
                )
                .when(!d.estreita, |c| {
                    let [linha, teclas] = rodape_de_teclas([
                        dica_de_tecla(&["←", "→"], "nível"),
                        dica_de_tecla(&["U"], "Ultracode"),
                        dica_de_tecla(&["Enter"], "fecha"),
                    ], window);
                    c.rodape(linha).rodape(teclas)
                }),
        )
}

impl Janela {
    pub(super) fn chip_modo(&self, motor: &Motor, chat: &Chat, b: &Barra, window: &Window, cx: &Context<Self>) -> AnyElement {
        let atual = motor.permissao_de(chat);
        let mudado = chat.permissao.as_ref().is_some_and(|p| *p != motor.config.permissao);
        // No Codex o modo vira o sandbox dele, e é isso que a dica explica; no Gemini, o modo dele.
        let descricao = match chat.agente {
            Agente::Codex => agente_codex::dica_da_permissao(&atual),
            Agente::Gemini => agente_gemini::dica_da_permissao(&atual),
            Agente::Claude => descricao_do_modo(&atual),
            // Os ACP novos: o modo vai pela sessão quando o agente o tem; o Sem perguntar, o
            // Vitral aprova sozinho.
            _ => descricao_do_modo(&atual),
        };
        let dica: SharedString = format!("Modo de permissão: {}\n{}", nome_do_modo(&atual), descricao).into();
        let chip = |aberto: bool| {
            let (icone, cor) = icone_do_modo(&atual);
            let mut chip = Chip::new(b.id("chip-modo"), nome_do_modo(&atual))
                .simbolo(Simbolo::Icone(icone, Some(cor)))
                .tamanho(b.tamanho())
                .mudado(mudado)
                // Só o ícone, na cor do modo (protótipo Linear, 25/11/2026); o nome vai na dica.
                .so_icone(true)
                .aberto(aberto);
            if !aberto {
                chip = chip.dica_acao(dica.clone(), Box::new(AlternarModo), None);
            }
            chip.construir(window, cx)
        };
        let dados = Rc::new(DadosModo {
            motor: self.motor.clone(),
            chat: chat.id.clone(),
            atual: atual.clone(),
            permitidas: chat.permitidas.clone(),
            foco_caixa: b.composer.read(cx).focus_handle(cx),
        });
        GatilhoChip::new(chip(false), chip(true))
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, window, cx| menu_do_modo(menu, &dados, window, cx))
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
        let (curto, nome, detalhe) = if chat.agente == Agente::Codex {
            let curto = chat.modelo.as_deref().map_or_else(|| "Codex".to_string(), agente_codex::nome_do_modelo);
            let detalhe = if chat.modelo.is_some() { "do Codex, escolhido nesta conversa" } else { "o modelo que o Codex escolher" };
            (curto.clone(), curto, detalhe.to_string())
        } else if chat.agente == Agente::Gemini {
            let curto = chat.modelo.as_deref().map_or_else(
                || em_uso.clone().unwrap_or_else(|| "Gemini".to_string()),
                agente_gemini::nome_do_modelo,
            );
            let detalhe = if chat.modelo.is_some() { "pelo Antigravity, escolhido nesta conversa" } else { "o modelo que o Antigravity escolher" };
            (curto.clone(), curto, detalhe.to_string())
        } else if chat.agente.acp() {
            let agente = chat.agente;
            let curto = chat.modelo.as_deref().map_or_else(
                || em_uso.clone().unwrap_or_else(|| agente.nome().to_string()),
                |m| agente_gemini::nome_do_modelo_de(agente, m),
            );
            let detalhe = if chat.modelo.is_some() { format!("do {}, escolhido nesta conversa", agente.nome()) } else { format!("o modelo que o {} escolher", agente.nome()) };
            (curto.clone(), curto, detalhe)
        } else {
            let curto = rotulo_modelo(&efetivo, em_uso.as_deref());
            let detalhe = match (&chat.modelo, &em_uso) {
                (None, Some(m)) => format!("padrão · em uso: {m}"),
                (None, None) => "padrão".to_string(),
                (Some(_), Some(m)) => format!("em uso: {m}"),
                (Some(_), None) => "escolhido nesta conversa".to_string(),
            };
            (curto.clone(), com_claude(&curto), detalhe)
        };
        // Na dupla, o chip mostra só quem responde a uma mensagem sem "@" (`AlvoDaDupla::OsDois`
        // vai a `chat.agente`). As duas marcas juntas liam como "a revisão do outro ainda está
        // ligada", mesmo com o outro parado no limite do plano (dono, 25/11/2026: "deixa só o que
        // vai sair se eu mandar uma mensagem normal"). O @ da caixa continua oferecendo os dois.
        let aberto = self.pop_caixa == Some((b.painel, PopCaixa::Modelo));
        let simbolo = Simbolo::Marca(Marca::do_agente(chat.agente));
        // Na caixa estreita (3 painéis a 150%) o modo e o esforço viram só o ícone, e o modelo
        // guarda o nome da família ("Opus", "Fable"): é o que diz com quem se fala. Só quando a
        // largura ainda dá (`modelo_so_icone`, medido como o esforço: `decidir_esforco`).
        // Linear (25/11/2026): a marca e o nome da família ("Opus") em qualquer largura, sem a
        // seta; o nome inteiro vai na dica.
        let familia = self.paineis[ix].modelo_so_icone.is_none();
        let rotulo_do_chip = if chat.agente == Agente::Claude { super::nome_da_familia(&curto).to_string() } else { curto };
        let mut chip = Chip::new(b.id("chip-modelo"), rotulo_do_chip)
            .simbolo(simbolo)
            .tamanho(b.tamanho())
            .cor_rotulo(Cores::texto_da_fala())
            .so_icone(!familia)
            .aberto(aberto);
        if !aberto {
            chip = chip.dica_acao(format!("Modelo: {nome}\n{detalhe}"), Box::new(AbrirModelo), Some("Vitral"));
        }
        let lista = self.paineis[ix].busca_modelo.clone();
        let foco = lista.read(cx).focus_handle(cx);
        let (motor_e, janela, chat_id, painel) = (self.motor.clone(), cx.entity().downgrade(), chat.id.clone(), b.painel);
        // As abas Claude e Codex em cima da lista: a que está à vista e se elas existem.
        let aba = self.aba_do_modelo.filter(|(p, _)| *p == painel).map_or(chat.agente, |(_, a)| a);
        let com_abas = com_abas_de_agente(chat);
        let agentes_das_abas = abas_de_agente(chat);
        let (largura, estreita) = (b.largura_popover().px(), b.estreita());
        // Numa conversa já grande, trocar o modelo do Claude custa uma releitura inteira.
        let aviso_de_troca: Option<SharedString> = aviso_de_troca(chat, aba).map(Into::into);
        popover(b.id("pop-modelo"))
            .anchor(Anchor::BottomLeft)
            .offset(px(8.))
            .p(r(6.))
            .open(aberto)
            .on_open_change({
                let (lista, motor_e, janela, chat_id) = (lista.clone(), motor_e.clone(), janela.clone(), chat_id.clone());
                move |abriu, window, cx| {
                    if *abriu && let Some(m) = &motor_e {
                        // Abre sempre na aba de quem responde no chat, com tudo recolhido.
                        let _ = janela.update(cx, |this, _| (this.aba_do_modelo, this.abertos_do_modelo) = (None, None));
                        preparar_modelos(&lista, m, &chat_id, None, None, None, window, cx);
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
                    move |window: &mut Window, cx: &mut Context<Subscription>| {
                        let app: &mut App = cx;
                        let lista_aberta = lista.clone();
                        window.subscribe(&lista, app, move |_, evento: &EventoEscolha, window, cx| {
                            if let EventoEscolha::Escolheu(chave) = evento {
                                let valor = chave.to_string();
                                // A linha que só explica não escolhe nada (e nem fecha o popover).
                                if valor == SEM_VERSOES {
                                    return;
                                }
                                // "Outras versões" e a conta abrem e fecham no lugar, sem fechar o popover.
                                if valor == OUTRAS_VERSOES || valor == A_CONTA {
                                    if let Some(m) = &motor_e {
                                        alternar_aberto(&valor, &lista_aberta, m, &janela, &chat_id, painel, window, cx);
                                    }
                                    return;
                                }
                                // Uma linha do grupo da conta troca só a conta do chat.
                                // A conta do Codex: no motor, que abre a sessão nova quando precisa.
                                if let Some(conta) = valor.strip_prefix(DA_CONTA_CODEX) {
                                    let conta = conta.to_string();
                                    if let Some(m) = &motor_e {
                                        m.update(cx, |m, cx| m.definir_conta_codex(&chat_id, Some(conta), cx));
                                    }
                                    abrir_ou_fechar(&janela, painel, PopCaixa::Modelo, false, cx);
                                    return;
                                }
                                if let Some(conta) = valor.strip_prefix(DA_CONTA) {
                                    let conta = conta.to_string();
                                    mudar(&motor_e, &chat_id, cx, move |c| c.conta = Some(conta));
                                    abrir_ou_fechar(&janela, painel, PopCaixa::Modelo, false, cx);
                                    return;
                                }
                                // A linha escolhida diz também quem responde: as do grupo do Codex
                                // e do Gemini passam o chat para eles, as outras para o Claude.
                                let (agente, modelo) = match (valor.strip_prefix(DO_CODEX), agente_acp_da_chave(&valor)) {
                                    (Some(slug), _) => (Agente::Codex, slug.to_string()),
                                    (_, Some((agente, valor))) => (agente, valor),
                                    _ => (Agente::Claude, valor),
                                };
                                let nome = match agente {
                                    Agente::Codex if modelo.is_empty() => "Codex".to_string(),
                                    Agente::Codex => agente_codex::nome_do_modelo(&modelo),
                                    Agente::Claude => com_claude(&rotulo_modelo(&modelo, None)),
                                    outro if modelo.is_empty() => outro.nome().to_string(),
                                    outro => agente_gemini::nome_do_modelo_de(outro, &modelo),
                                };
                                if let Some(m) = &motor_e {
                                    m.update(cx, |m, cx| m.definir_agente(&chat_id, agente, Some(modelo).filter(|v| !v.is_empty()), cx));
                                }
                                let _ = janela.update(cx, |this, cx| this.avisar_na_estreita(painel, format!("Modelo: {nome}"), window, cx));
                            }
                            abrir_ou_fechar(&janela, painel, PopCaixa::Modelo, false, cx);
                        })
                    }
                };
                let _assinatura = window.use_keyed_state(("assinatura-modelo", painel as usize), cx, assinar);
                let largura = (tam(window, largura) - tam(window, 12.)).min(window.viewport_size().width - px(44.));
                // Trocar de aba só mostra os modelos do outro: quem responde muda ao escolher um.
                // Linear (25/11/2026): os agentes só pelo ícone, numa coluna à esquerda com os
                // ícones embaixo ("deixa só o icone e lá em baixo"); era um segmentado em cima.
                let abas = com_abas.then(|| {
                    let botoes = agentes_das_abas.iter().map(|&agente| {
                        let (lista, motor_e, janela, chat_id) = (lista.clone(), motor_e.clone(), janela.clone(), chat_id.clone());
                        let escolhido = agente == aba;
                        div()
                            .id(("aba-modelo", painel as usize * 8 + agente as usize))
                            .flex_none()
                            .size(r(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(raio::de(28.))
                            .cursor_pointer()
                            .when(escolhido, |d| d.bg(Cores::selecao()))
                            .when(!escolhido, |d| d.hover(|s| s.bg(Cores::hover())))
                            .child(Simbolo::Marca(Marca::do_agente(agente)).desenhar(16., window))
                            .dica_no_ponteiro(move |window, cx| Tooltip::new(agente.nome()).build(window, cx))
                            .on_click(move |_, window, cx| {
                                let _ = janela.update(cx, |this, cx| {
                                    (this.aba_do_modelo, this.abertos_do_modelo) = (Some((painel, agente)), None);
                                    cx.notify();
                                });
                                if let Some(m) = &motor_e {
                                    preparar_modelos(&lista, m, &chat_id, Some(agente), None, None, window, cx);
                                }
                            })
                    });
                    v_flex()
                        .flex_none()
                        .justify_end()
                        .gap(esp::E2)
                        .p(esp::E2)
                        .rounded(raio::CARTAO)
                        .bg(Cores::recuo_no_flutuante())
                        .children(botoes)
                });
                // O aviso e as teclas na coluna do texto das linhas: os 8 do item mais os ~4 da lista
                // do kit (eram 8 e ficavam 4 px fora; folha dimensional, 23/11/2026).
                // Curto e numa linha só: é o custo da troca, não um alerta (dono, 24/11/2026).
                let aviso = aviso_de_troca
                    .clone()
                    .map(|a| div().px(esp::E12).pt(esp::E4).min_w_0().truncate().metadado().text_color(Cores::tenue()).child(a));
                let corpo = h_flex().items_stretch().gap(esp::E6).children(abas).child(div().flex_1().min_w_0().child(lista.clone()));
                div().w(largura).flex().flex_col().child(corpo).children(aviso).when(!estreita, |s| {
                    let [linha, teclas] = rodape_de_teclas([
                        dica_de_tecla(&["↑", "↓"], "escolhe"),
                        dica_de_tecla(&["Enter"], "usa"),
                    ], window);
                    s.child(div().mt(esp::E4).child(linha)).child(div().px(esp::E12).child(teclas))
                })
            })
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn chip_esforco(
        &self,
        motor: &Motor,
        chat: &Chat,
        ix: usize,
        b: &Barra,
        compacto: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let atual = indice_esforco(chat.esforco.as_deref().unwrap_or(""));
        let nome = ESFORCOS[atual].1;
        // O nível que o automático usa (o que o Claude Code ou o catálogo do Codex disse).
        let padrao = motor.esforco_automatico(chat).map(|e| indice_esforco(&e)).filter(|i| *i != 0);
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
        } else if let Some(p) = padrao.filter(|_| atual == 0) {
            format!("Esforço: automático, que neste modelo é o {}", ESFORCOS[p].1).into()
        } else if atual == 0 {
            "Esforço: automático\nO que o Claude Code usa para este modelo".into()
        } else {
            format!("Esforço: {nome}").into()
        };
        let base = if ultracode {
            // Com o Ultrathink junto, as barrinhas em arco-íris; só o ícone, o do fluxo.
            let simbolo = if armado && !b.estreita() && !compacto { barras_arco_iris() } else { simbolo_ultracode() };
            Chip::new(b.id("chip-esforco"), "Ultracode")
                .simbolo(simbolo)
                .cor_rotulo(Cores::violeta())
                .so_icone(true)
        } else if armado {
            Chip::new(b.id("chip-esforco"), "Ultrathink")
                .simbolo(barras_arco_iris())
                .arco_iris()
                .so_icone(true)
        } else {
            // No automático, as barrinhas e o rótulo dizem o nível que ele usa ("Auto · Extra").
            let no_auto = padrao.filter(|_| atual == 0);
            let rotulo: SharedString = match no_auto.filter(|_| b.larga()) {
                Some(p) => format!("Auto · {}", ESFORCOS[p].1).into(),
                None => nome.into(),
            };
            // Só as barrinhas (protótipo Linear, 25/11/2026); o nível vai na dica.
            let chip = Chip::new(b.id("chip-esforco"), rotulo)
                .simbolo(Simbolo::Sinal { nivel: no_auto.unwrap_or(atual) as u8, total: 5 })
                .so_icone(true);
            chip.mudado(atual != 0)
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
            padrao,
            ultracode,
            outro_ultracode: motor.ultracode_rodando().is_some_and(|c| c != chat.id),
            fluxos_desligados: motor.fluxos_desligados(chat),
            modo_pergunta: !matches!(motor.permissao_de(chat).as_str(), "bypassPermissions" | "auto"),
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
            .content(move |_, window, _| corpo_do_esforco(&dados, window))
            .into_any_element()
    }

    /// "…": no máximo três grupos, com uma divisória entre eles (dono, 24/11/2026: "esse daqui
    /// também"; eram 11 itens e 5 divisórias, um cinza sozinho entre duas e um rótulo cortado). Na
    /// faixa estreita, **anexar**, que sai da barra; com texto na caixa, **mandar** (depois, numa
    /// hora, aos dois); e **a conversa**. O que não se aplica agora sai, em vez de ficar cinza
    /// (`grupos_do_mais`).
    pub(super) fn botao_mais(&self, motor: &Motor, chat: &Chat, b: &Barra, window: &Window, cx: &Context<Self>) -> AnyElement {
        let anexar = DadosAnexar { janela: cx.entity().downgrade(), chat: b.chat.clone(), painel: b.painel };
        let (motor_e, fixado, painel) = (self.motor.clone(), chat.fixado, b.painel);
        let caixa = b.composer.read(cx).value();
        let outro = chat.agente.outro();
        let agora = AgoraNoMais {
            // O clipe não fica na barra em largura nenhuma (protótipo Linear, 25/11/2026): o
            // anexar mora sempre aqui.
            estreita: true,
            com_texto: !caixa.trim().is_empty(),
            comando: caixa.trim_start().starts_with('/'),
            // Com o turno rodando, o Enviar entra nele (ou responde ao pedido aberto); quem já ia
            // pela fila de qualquer jeito não precisa da opção.
            fila_a_parte: composer::enter_no_turno(motor, &chat.id) != composer::Enter::Fila,
            outro_instalado: outro.instalado(),
            // O `/compact` é do Claude Code e do Codex, e precisa de uma conversa para resumir.
            compactar: !chat.agente.acp() && chat.sessao_criada,
            tem_agentes: crate::ui::mapa_agentes::tem_agentes(chat),
            rodando: b.rodando,
        };
        let grupos = grupos_do_mais(&agora);
        // Mandar numa hora marcada (`agendar.rs`): quando as 5 h renovarem, ou na hora escrita.
        let agendar = crate::ui::agendar::DadosDoMenu { janela: cx.entity().downgrade(), painel, renova: motor.renovacao_do_chat(&chat.id) };
        let rotulo_do_outro = SharedString::from(format!("Também ao {}", outro.nome()));
        botao_icone(b.id("mais-caixa"), Lucide::Ellipsis, "Mais opções", TamanhoBotao::Caixa, window, cx)
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, window, _| {
                let largura = tam(window, LARGURA_DO_MAIS);
                let mut menu = menu.min_w(largura).max_w(largura);
                for grupo in &grupos {
                    menu = menu.label(titulo_do_grupo(grupo));
                    for item in grupo {
                        let janela = anexar.janela.clone();
                        let (motor_e, chat) = (motor_e.clone(), anexar.chat.clone());
                        menu = match item {
                            ItemDoMais::Anexar => itens_de_anexar(menu, &anexar, false),
                            ItemDoMais::MandarDepois => menu.item(PopupMenuItem::new("Na fila").icon(Lucide::TextAlignStart).on_click(
                                move |_, window, cx| {
                                    let _ = janela.update(cx, |this, cx| this.enviar_depois(painel, window, cx));
                                },
                            )),
                            ItemDoMais::Agendar => crate::ui::agendar::itens_do_menu(menu, &agendar),
                            // A marca colorida do outro agente na coluna do ícone, como no protótipo.
                            ItemDoMais::PerguntarAoOutro => {
                                let rotulo = rotulo_do_outro.clone();
                                menu.item(
                                    PopupMenuItem::element(move |_, _| div().child(rotulo.clone()))
                                        .icone_proprio(move |_, _| Marca::do_agente(outro).glifo(14.))
                                        .on_click(move |_, window, cx| {
                                            let _ = janela.update(cx, |this, cx| this.enviar_aos_dois(painel, window, cx));
                                        }),
                                )
                            }
                            ItemDoMais::Comandos => menu.item(item_de_comandos(janela, painel)),
                            ItemDoMais::Compactar => menu.item(PopupMenuItem::new("Compactar").icon(Lucide::FoldVertical).on_click(move |_, _, cx| {
                                let _ = janela.update(cx, |this, cx| this.compactar_conversa(painel, cx));
                            })),
                            // O mapa dos subagentes, quando a conversa abriu algum.
                            ItemDoMais::Agentes => menu.item(PopupMenuItem::new("Agentes").icon(Lucide::UsersRound).on_click(
                                move |_, window, cx| {
                                    let _ = janela.update(cx, |this, cx| this.abrir_mapa_de_agentes(chat.clone(), window, cx));
                                },
                            )),
                            ItemDoMais::SessaoNova => menu.item(PopupMenuItem::new("Sessão nova").icon(Lucide::RefreshCw).on_click(move |_, _, cx| {
                                if let Some(m) = &motor_e {
                                    m.update(cx, |m, cx| m.nova_sessao(&chat, cx));
                                }
                            })),
                            ItemDoMais::Fixar => menu.item(
                                PopupMenuItem::new(if fixado { "Desafixar" } else { "Fixar" })
                                    .icon(if fixado { Lucide::PinOff } else { Lucide::Pin })
                                    .on_click(move |_, _, cx| {
                                        if let Some(m) = &motor_e {
                                            m.update(cx, |m, cx| m.alternar_fixado(&chat, cx));
                                        }
                                    }),
                            ),
                        };
                    }
                }
                menu
            })
            .into_any_element()
    }
}

/// A largura do "…", a 100%: a do protótipo (o `menu-justo` dele, de 268).
const LARGURA_DO_MAIS: f32 = 268.;

/// O rótulo de cada seção do "…", como no protótipo: "Anexar", "Mandar" e "Conversa". O grupo
/// é reconhecido pelo primeiro item (`grupos_do_mais` nunca mistura os três).
fn titulo_do_grupo(grupo: &[ItemDoMais]) -> &'static str {
    match grupo.first() {
        Some(ItemDoMais::Anexar) => "Anexar",
        Some(ItemDoMais::MandarDepois | ItemDoMais::Agendar | ItemDoMais::PerguntarAoOutro) => "Mandar",
        _ => "Conversa",
    }
}

/// O que o "…" da caixa precisa saber para decidir o que mostra.
#[derive(Clone, Copy, Debug, Default)]
struct AgoraNoMais {
    /// A faixa estreita: o clipe sai da barra e o anexar vem para cá.
    estreita: bool,
    com_texto: bool,
    /// O texto da caixa é um comando de barra.
    comando: bool,
    /// O Enter não vai para a fila: a fila é uma escolha à parte.
    fila_a_parte: bool,
    outro_instalado: bool,
    compactar: bool,
    tem_agentes: bool,
    rodando: bool,
}

/// Uma linha (ou um bloco de linhas) do "…".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ItemDoMais {
    /// Arquivo ou imagem, arquivo da pasta e conversa anterior.
    Anexar,
    MandarDepois,
    /// "Mandar quando as 5 h renovarem" (com a hora lida) e "Mandar às…".
    Agendar,
    PerguntarAoOutro,
    Comandos,
    Compactar,
    Agentes,
    SessaoNova,
    Fixar,
}

/// Os grupos do "…", na ordem: anexar (só na faixa estreita, e primeiro, porque é o que serve com
/// a caixa vazia), mandar (só com texto na caixa: todos precisam dele) e a conversa. O que não se
/// aplica agora sai, em vez de ficar cinza: a fila com o Enter já indo para ela, o outro agente
/// sem ele instalado ou com um comando de barra na caixa, o compactar sem conversa do Claude, a
/// sessão nova com o turno rodando. O "Perguntar também ao Codex" fica com os de mandar: ele manda
/// o texto da caixa, como eles.
fn grupos_do_mais(agora: &AgoraNoMais) -> Vec<Vec<ItemDoMais>> {
    use ItemDoMais::*;
    let anexar = if agora.estreita { vec![Anexar] } else { Vec::new() };
    let mandar = if agora.com_texto {
        [agora.fila_a_parte.then_some(MandarDepois), Some(Agendar), (agora.outro_instalado && !agora.comando).then_some(PerguntarAoOutro)]
            .into_iter()
            .flatten()
            .collect()
    } else {
        Vec::new()
    };
    let conversa = [Some(Comandos), agora.compactar.then_some(Compactar), agora.tem_agentes.then_some(Agentes), (!agora.rodando).then_some(SessaoNova), Some(Fixar)]
        .into_iter()
        .flatten()
        .collect();
    [anexar, mandar, conversa].into_iter().filter(|g: &Vec<ItemDoMais>| !g.is_empty()).collect()
}

#[cfg(test)]
mod testes {
    use super::{LinhaDeModelo, SEM_VERSOES, linhas_de_modelo, linhas_de_versao};
    use crate::claude::VERSAO_MODELOS_EXATOS;
    use crate::motor::motivo_sem_versao_exata;

    fn titulos(linhas: &[LinhaDeModelo]) -> Vec<&str> {
        linhas.iter().map(|l| l.titulo.as_str()).collect()
    }

    /// O "…" do pedido de 24/11/2026: no máximo três grupos (duas divisórias), anexar primeiro na
    /// faixa estreita, e o que não se aplica sai em vez de ficar cinza.
    #[test]
    fn o_mais_tem_tres_grupos_e_so_o_que_se_aplica() {
        use super::{AgoraNoMais, ItemDoMais::*, grupos_do_mais};
        let base = AgoraNoMais { estreita: true, fila_a_parte: true, outro_instalado: true, compactar: true, ..AgoraNoMais::default() };
        // Caixa vazia: anexar e a conversa; nada de mandar (era tudo cinza).
        assert_eq!(grupos_do_mais(&base), [vec![Anexar], vec![Comandos, Compactar, SessaoNova, Fixar]]);
        // Com texto: os de mandar no meio, o "Perguntar também" junto deles.
        let com_texto = AgoraNoMais { com_texto: true, ..base };
        assert_eq!(grupos_do_mais(&com_texto), [vec![Anexar], vec![MandarDepois, Agendar, PerguntarAoOutro], vec![Comandos, Compactar, SessaoNova, Fixar]]);
        // Um comando de barra não vai ao outro agente; o Enter que já vai à fila dispensa a fila.
        assert_eq!(grupos_do_mais(&AgoraNoMais { comando: true, fila_a_parte: false, ..com_texto })[1], [Agendar]);
        // Fora da faixa estreita o clipe está na barra; rodando, sem sessão nova; sem conversa do
        // Claude, sem compactar; com subagentes, o mapa deles.
        let larga = AgoraNoMais { estreita: false, rodando: true, compactar: false, tem_agentes: true, ..base };
        assert_eq!(grupos_do_mais(&larga), [vec![Comandos, Agentes, Fixar]]);
        for agora in [base, com_texto, larga, AgoraNoMais::default()] {
            let grupos = grupos_do_mais(&agora);
            assert!(grupos.len() <= 3 && grupos.iter().all(|g| !g.is_empty()), "{grupos:?}");
        }
    }

    fn linhas_com_seta(linhas: &[LinhaDeModelo]) -> Vec<(&str, Option<bool>)> {
        linhas.iter().map(|l| (l.titulo.as_str(), l.seta)).collect()
    }

    /// O seletor do Claude como o dono o viu em 24/11/2026: as famílias, "Outras versões", e a
    /// conta no pé, com as versões e as contas de verdade.
    fn partes_do_claude() -> super::PartesDoSeletor {
        use super::{DA_CONTA, PartesDoSeletor, PeDaConta, rotulo_da_conta};
        let versoes = ["claude-fable-5-2", "claude-fable-5-1", "claude-fable-5", "claude-opus-5-5", "claude-sonnet-5", "claude-haiku-5"]
            .into_iter()
            .map(|id| LinhaDeModelo::new(id, super::nome_curto_modelo(id), ""))
            .collect();
        let conta = |id: &str, nome: &str, uso: &str| LinhaDeModelo::new(format!("{DA_CONTA}{id}"), nome, uso);
        PartesDoSeletor {
            grupos: vec![(None, linhas_de_modelo(None, Some("claude-opus-5-5"), ""))],
            versoes,
            conta: Some(PeDaConta {
                rotulo: rotulo_da_conta("tecnologia", false, "semana 44%"),
                contas: vec![
                    conta("principal", "Principal", "Semana 68%"),
                    LinhaDeModelo { em_uso: true, ..conta("tec", "tecnologia", "Semana 44%") },
                    conta("auto", "Automática", "A conta com mais folga no plano"),
                ],
            }),
        }
    }

    /// Recolhido (como abre): as famílias com "Outras versões" no fim e a conta numa linha só, no
    /// pé. Sete linhas, que cabem sem rolagem; nada de versão exata nem de conta solta.
    #[test]
    fn o_seletor_abre_recolhido_com_as_familias_e_a_conta_numa_linha() {
        use super::{A_CONTA, Abertos, OUTRAS_VERSOES, grupos_a_vista};
        let grupos = grupos_a_vista(&partes_do_claude(), Abertos::default(), "");
        assert_eq!(grupos.len(), 2, "{grupos:?}");
        assert_eq!(
            linhas_com_seta(&grupos[0].1),
            [("Fable", None), ("Opus", None), ("Sonnet", None), ("Haiku", None), ("Outras versões", Some(false))]
        );
        assert_eq!(grupos[0].1[4].valor, OUTRAS_VERSOES);
        assert_eq!(linhas_com_seta(&grupos[1].1), [("tecnologia · semana 44%", Some(false))]);
        assert_eq!(grupos[1].1[0].valor, A_CONTA);
        assert_eq!(grupos[1].0, None, "a conta não tem mais o rótulo de grupo");
        // As linhas de abrir têm uma altura só (sem descrição).
        assert!(grupos[0].1[4].descricao.is_empty() && grupos[1].1[0].descricao.is_empty());
    }

    /// "Outras versões" abre a lista no lugar, logo embaixo dela; a conta abre as contas embaixo
    /// dela, com a escolhida em uso. Um não abre o outro.
    #[test]
    fn outras_versoes_e_a_conta_abrem_no_lugar() {
        use super::{Abertos, grupos_a_vista};
        let partes = partes_do_claude();
        let so_versoes = grupos_a_vista(&partes, Abertos { versoes: true, conta: false }, "");
        assert_eq!(
            titulos(&so_versoes[0].1),
            ["Fable", "Opus", "Sonnet", "Haiku", "Outras versões", "Fable 5.2", "Fable 5.1", "Fable 5", "Opus 5.5", "Sonnet 5", "Haiku 5"]
        );
        assert_eq!(so_versoes[0].1[4].seta, Some(true));
        assert_eq!(titulos(&so_versoes[1].1), ["tecnologia · semana 44%"]);
        let so_conta = grupos_a_vista(&partes, Abertos { versoes: false, conta: true }, "");
        assert_eq!(so_conta[0].1.len(), 5);
        assert_eq!(titulos(&so_conta[1].1), ["tecnologia · semana 44%", "Principal", "tecnologia", "Automática"]);
        assert_eq!(so_conta[1].1[0].seta, Some(true));
        assert!(so_conta[1].1[2].em_uso);
    }

    /// Clicar numa linha de abrir alterna só o que ela guarda; ao abrir o seletor, tudo vem
    /// recolhido, menos as versões quando a conversa escolheu uma delas (o visto fica à vista).
    #[test]
    fn as_linhas_de_abrir_alternam_e_o_seletor_comeca_recolhido() {
        use super::{A_CONTA, Abertos, OUTRAS_VERSOES};
        use crate::modelo::{Agente, Chat};
        let fechado = Abertos::default();
        assert_eq!(fechado.alternar(OUTRAS_VERSOES), Abertos { versoes: true, conta: false });
        assert_eq!(fechado.alternar(OUTRAS_VERSOES).alternar(OUTRAS_VERSOES), fechado);
        assert_eq!(fechado.alternar(A_CONTA), Abertos { versoes: false, conta: true });
        assert_eq!(fechado.alternar("opus"), fechado, "escolher um modelo não abre nada");
        let mut chat = Chat::novo("c".into(), "t", std::path::PathBuf::from("/p"));
        assert_eq!(Abertos::de_partida(&chat), fechado);
        chat.modelo = Some("opus".into());
        assert_eq!(Abertos::de_partida(&chat), fechado);
        chat.modelo = Some("claude-fable-5-1".into());
        assert_eq!(Abertos::de_partida(&chat), Abertos { versoes: true, conta: false });
        chat.agente = Agente::Codex;
        assert_eq!(Abertos::de_partida(&chat), fechado);
    }

    /// Com algo na busca, o que casa aparece direto, versões e contas inclusive, recolhidas ou
    /// não, e as linhas de abrir saem.
    #[test]
    fn a_busca_mostra_as_versoes_e_as_contas_que_casam() {
        use super::{Abertos, grupos_a_vista};
        let partes = partes_do_claude();
        for abertos in [Abertos::default(), Abertos { versoes: true, conta: true }] {
            let fable = grupos_a_vista(&partes, abertos, "fable");
            assert_eq!(fable.len(), 1, "{fable:?}");
            assert_eq!(titulos(&fable[0].1), ["Fable", "Fable 5.2", "Fable 5.1", "Fable 5"]);
            assert!(fable[0].1.iter().all(|l| l.seta.is_none()));
            // Pela chave também: "5-5" é a versão exata do Opus.
            assert_eq!(titulos(&grupos_a_vista(&partes, abertos, " 5-5 ")[0].1), ["Opus 5.5"]);
            // A conta pelo nome, sem a linha da conta de agora.
            let tec = grupos_a_vista(&partes, abertos, "TECNO");
            assert_eq!(tec.len(), 1);
            assert_eq!(titulos(&tec[0].1), ["tecnologia"]);
            assert!(grupos_a_vista(&partes, abertos, "gpt").is_empty());
        }
        // Sem nada escrito (só espaços), volta o recolhido.
        assert_eq!(grupos_a_vista(&partes, Abertos::default(), "  "), grupos_a_vista(&partes, Abertos::default(), ""));
    }

    /// Sem conta extra e sem versões (as abas do Codex e do Gemini), só os grupos de sempre.
    #[test]
    fn sem_versoes_nem_conta_o_seletor_fica_so_com_os_modelos() {
        use super::{Abertos, PartesDoSeletor, grupos_a_vista};
        let partes = PartesDoSeletor { grupos: vec![(Some("Perfis".into()), vec![LinhaDeModelo::new("perfil:a", "Rápido", "")]), (Some("Modelos".into()), linhas_de_modelo(None, None, ""))], ..PartesDoSeletor::default() };
        let grupos = grupos_a_vista(&partes, Abertos::default(), "");
        assert_eq!(grupos.iter().map(|(t, l)| (t.as_deref(), l.len())).collect::<Vec<_>>(), [(Some("Perfis"), 1), (Some("Modelos"), 4)]);
        // Na busca, o grupo que não casa some.
        assert_eq!(grupos_a_vista(&partes, Abertos::default(), "haiku").iter().map(|(t, _)| t.as_deref()).collect::<Vec<_>>(), [Some("Modelos")]);
    }

    /// O aviso de reler só aparece numa conversa do Claude que já passou de 100 mil tokens, na aba
    /// dele, curto e numa linha.
    #[test]
    fn o_aviso_de_reler_so_com_o_contexto_grande() {
        use super::aviso_de_troca;
        use crate::modelo::{Agente, CONTEXTO_GRANDE, Chat};
        let mut chat = Chat::novo("c".into(), "t", std::path::PathBuf::from("/p"));
        chat.sessao_criada = true;
        chat.contexto = Some(751_000);
        assert_eq!(aviso_de_troca(&chat, Agente::Claude).as_deref(), Some("Trocar agora relê 751 mil tokens"));
        assert_eq!(aviso_de_troca(&chat, Agente::Codex), None, "na aba do Codex a troca é de agente");
        chat.contexto = Some(CONTEXTO_GRANDE);
        assert!(aviso_de_troca(&chat, Agente::Claude).is_some());
        assert_eq!(CONTEXTO_GRANDE, 100_000);
        chat.contexto = Some(CONTEXTO_GRANDE - 1);
        assert_eq!(aviso_de_troca(&chat, Agente::Claude), None);
        chat.contexto = Some(751_000);
        chat.sessao_criada = false;
        assert_eq!(aviso_de_troca(&chat, Agente::Claude), None, "sem sessão ainda não há o que reler");
        chat.sessao_criada = true;
        chat.agente = Agente::Codex;
        assert_eq!(aviso_de_troca(&chat, Agente::Claude), None);
    }

    /// A linha da conta diz qual o chat usa e o uso dela; na Automática, a que ela escolheu.
    #[test]
    fn a_linha_da_conta_no_pe() {
        use super::rotulo_da_conta;
        assert_eq!(rotulo_da_conta("tecnologia", false, "semana 44%"), "tecnologia · semana 44%");
        assert_eq!(rotulo_da_conta("Principal", true, "5 h 12% · semana 68%"), "Automática: Principal · 5 h 12% · semana 68%");
    }

    /// O Ultracode diz o custo numa linha curta embaixo do título; o detalhe vai na dica, e o
    /// âmbar fica só para o que atrapalha agora.
    #[test]
    fn o_ultracode_numa_linha_curta() {
        use super::{ULTRACODE_CURTO, ULTRACODE_DICA, avisos_do_ultracode};
        assert!(!ULTRACODE_CURTO.contains('\n') && ULTRACODE_CURTO.chars().count() <= 52, "{ULTRACODE_CURTO}");
        assert!(ULTRACODE_DICA.contains("esforço Extra") && ULTRACODE_DICA.contains("bem mais do plano"));
        assert!(avisos_do_ultracode(false, false, false).is_empty());
        let avisos = avisos_do_ultracode(true, true, true);
        assert_eq!(avisos.len(), 3);
        // O que atrapalha também cabe numa linha; o porquê vai na dica.
        assert!(avisos.iter().all(|(curto, porque)| curto.chars().count() <= 52 && porque.len() > curto.len()), "{avisos:?}");
    }

    #[test]
    fn grupo_do_codex_com_o_padrao_o_catalogo_e_o_que_saiu_dele() {
        use super::{DO_CODEX, linhas_do_codex};
        use crate::agente_codex::ModeloDoCodex;
        let catalogo = [ModeloDoCodex { slug: "gpt-6.1-sol".into(), nome: "GPT-6.1-Sol".into(), descricao: "Latest workhorse.".into(), esforco: "low".into() }];
        let linhas = linhas_do_codex(None, &catalogo);
        // Sem a linha "Padrão" desde o Linear (25/11/2026): só o catálogo.
        let _ = DO_CODEX;
        assert_eq!(linhas.iter().map(|l| (l.valor.as_str(), l.titulo.as_str())).collect::<Vec<_>>(), [("codex:gpt-6.1-sol", "GPT-6.1-Sol")]);
        // A descrição vem em português quando a frase do catálogo é reconhecida; senão, nenhuma.
        assert_eq!(linhas[0].descricao, "Para código e o trabalho do dia a dia");
        use super::descricao_do_codex;
        assert_eq!(descricao_do_codex("Frontier intelligence for the most demanding work."), "O mais capaz, para o trabalho mais difícil");
        assert_eq!(descricao_do_codex("Fast and affordable model for easier tasks."), "Rápido e econômico, para tarefas fáceis");
        assert_eq!(descricao_do_codex("Older fast and efficient model."), "Geração anterior");
        assert_eq!(descricao_do_codex("GPT-6.1 Terra Codex model."), "");
        // O escolhido que saiu do catálogo segue na lista; o que está nele não repete.
        assert_eq!(linhas_do_codex(Some("gpt-5-velho"), &catalogo).last().map(|l| l.valor.as_str()), Some("codex:gpt-5-velho"));
        assert_eq!(linhas_do_codex(Some("gpt-6.1-sol"), &catalogo).len(), 1);
    }

    #[test]
    fn seletor_de_modelo_com_a_versao_em_uso() {
        let linhas = linhas_de_modelo(None, Some("claude-opus-5-5"), "");
        // O título é o nome seco da família, e a versão que está rodando vem à parte; o item da
        // lista junta os dois ("Opus 5.5", como no protótipo de 26/11/2026). Sem a linha "Padrão"
        // (Linear, 25/11/2026).
        assert_eq!(titulos(&linhas), ["Fable", "Opus", "Sonnet", "Haiku"]);
        assert_eq!(linhas[1].versao.as_deref(), Some("5.5"));
        assert_eq!(linhas[1].descricao, "Dia a dia");
        let itens = super::itens_de_modelo(linhas.clone(), crate::ui::componentes::Marca::Claude);
        assert_eq!(itens[1].titulo.as_ref(), "Opus 5.5");
        assert!(itens[1].selos.is_empty());
        // Seguindo o padrão, o visto (a chave do chat) diz qual vale: nenhum selo.
        assert!(linhas.iter().all(|l| !l.em_uso));
        assert_eq!(linhas[1].valor, "opus");
        // Escolhido um apelido, o selo vai na família em uso (um só).
        let linhas = linhas_de_modelo(Some("opus"), Some("claude-opus-5-5"), "sonnet");
        assert_eq!(linhas.iter().filter(|l| l.em_uso).count(), 1);
        assert!(linhas[1].em_uso);
    }

    #[test]
    fn seletor_de_modelo_sem_dado_nao_inventa_versao() {
        let linhas = linhas_de_modelo(None, None, "");
        assert_eq!(titulos(&linhas), ["Fable", "Opus", "Sonnet", "Haiku"]);
        assert!(linhas.iter().all(|l| !l.em_uso && l.versao.is_none()));
        let linhas = linhas_de_modelo(Some("sonnet"), Some("claude-opus-5-5"), "opus");
        assert_eq!(linhas[1].versao.as_deref(), Some("5.5"));
        assert_eq!(linhas[2].versao, None);
    }

    #[test]
    fn conversa_antiga_ganha_a_sexta_linha() {
        let id = "claude-sonnet-4-5-20250929";
        let linhas = linhas_de_modelo(Some(id), Some(id), "");
        assert_eq!(linhas.len(), 5);
        let ultima = &linhas[4];
        assert_eq!((ultima.valor.as_str(), ultima.titulo.as_str()), (id, "Sonnet 4.5"));
        assert_eq!(ultima.descricao, "escolhido antes");
        assert!(ultima.em_uso);
        // O apelido não toma a versão do nome completo.
        assert_eq!(linhas[2].versao, None);
        assert!(!linhas[2].em_uso);
    }

    #[test]
    fn versao_exata_escolhida_nao_vira_a_sexta_linha() {
        // Ela vive no grupo das versões, e as famílias ficam com o nome seco.
        let linhas = linhas_de_modelo(Some("claude-opus-5-5"), Some("claude-opus-5-5"), "");
        assert_eq!(titulos(&linhas), ["Fable", "Opus", "Sonnet", "Haiku"]);
        assert!(linhas.iter().all(|l| !l.em_uso && l.versao.is_none()));
    }

    /// Sem a linha "Padrão", o visto de quem segue o padrão vai na família que vale: a do padrão
    /// das Configurações ou, sem ele, a do modelo que roda.
    #[test]
    fn sem_modelo_escolhido_o_visto_vai_na_familia_que_vale() {
        use super::chave_do_modelo;
        use crate::modelo::Chat;
        let mut chat = Chat::novo("c".into(), "t", std::path::PathBuf::from("/p"));
        assert_eq!(chave_do_modelo(&chat, "sonnet"), "sonnet");
        assert_eq!(chave_do_modelo(&chat, ""), "");
        chat.modelo_atual = Some("claude-opus-5-5".into());
        assert_eq!(chave_do_modelo(&chat, ""), "opus");
        chat.modelo = Some("haiku".into());
        assert_eq!(chave_do_modelo(&chat, "sonnet"), "haiku");
    }

    #[test]
    fn as_versoes_marcam_a_que_esta_rodando() {
        let linhas = linhas_de_versao(Some("opus"), Some("claude-opus-5-5"), None);
        assert_eq!(linhas[0].titulo, "Fable 5.2");
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
