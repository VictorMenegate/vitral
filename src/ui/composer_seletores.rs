//! Os controles da barra da caixa: os chips de modo, modelo, esforço e conta, com os menus e os
//! popovers deles, o clipe de anexar e o "…".

use std::rc::Rc;

use gpui_kit::base::actions::{Confirm, SelectFirst, SelectLast, SelectLeft, SelectRight};
use gpui_kit::component::{
    Icon, Sizable as _,
    input::{Paste, TextareaState},
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

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
    Selo, OndeAtalho, Simbolo, TamanhoBotao, TamanhoChip, botao_icone, cabecalho_escolha,
    desenho_teclas, fio_de_pontos, icone_do_modo, item_descrito, popover, resumo_uso, rodape_menu,
};
use crate::{
    agente_codex::{self, ModeloDoCodex},
    agente_gemini,
    contas,
    modelo::{Agente, Chat},
    motor::{FLUXOS_DESLIGADOS, Motor, Uso, motivo_sem_versao_exata},
    tema::{Cores, Lucide, Tipo as _, esp, r, tam},
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
            this.so_um_flutuante(crate::ui::QualFlutuante::Caixa);
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
    // 420: com o azulejo e o visto, a descrição do escolhido ainda cabe inteira.
    let largura = tam(window, 420.);
    menu = menu.min_w(largura).max_w(largura).item(cabecalho_escolha(
        "Modo de permissão",
        Some((Box::new(AlternarModo), OndeAtalho::Foco(d.foco_caixa.clone()), "troca")),
    ));
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
            PopupMenuItem::new("Arquivo ou imagem do computador…")
                .icon(Lucide::FolderOpen)
                .on_click(move |_, window, cx| {
                    let chat = c1.clone();
                    let _ = j1.update(cx, |this, cx| this.escolher_anexos(chat, window, cx));
                }),
        )
        .item(
            PopupMenuItem::element(|_, _| rotulo_com_gatilho("Arquivo da pasta…", "@"))
                .icon(Lucide::FileText)
                .on_click(move |_, window, cx| {
                    let chat = c2.clone();
                    let _ = j2.update(cx, |this, cx| this.dialogo_mencionar_arquivo(chat, window, cx));
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
    /// A versão da família, no selo à direita ("5.2"): assim a descrição não é cortada.
    versao: Option<String>,
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
            versao: None,
            em_uso: false,
            desabilitada: None,
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
    let qual = curto.clone().filter(|_| no_padrao);
    let padrao = match (padrao_pasta.is_empty(), qual) {
        (true, Some(m)) => format!("O do Claude Code · {m}"),
        (true, None) => "O que o Claude Code escolher".to_string(),
        (false, Some(m)) => format!("Padrão · {m}"),
        (false, None) => format!("Padrão · {}", rotulo_modelo(padrao_pasta, None)),
    };
    let mut linhas = vec![LinhaDeModelo::new("", "Padrão", padrao)];
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

fn itens_de_modelo(linhas: Vec<LinhaDeModelo>) -> Vec<ItemDeEscolha> {
    linhas
        .into_iter()
        .map(|l| {
            let termos = format!("{} {}", l.valor, l.descricao);
            ItemDeEscolha {
                descricao: (!l.descricao.is_empty()).then(|| l.descricao.into()),
                selos: l.versao.map(Selo::neutro).into_iter().chain(l.em_uso.then(|| Selo::neutro("em uso"))).collect(),
                termos: Some(termos.into()),
                desabilitado: l.desabilitada.map(Into::into),
                ..ItemDeEscolha::new(l.valor, l.titulo)
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
fn chave_do_modelo(chat: &Chat) -> String {
    let modelo = chat.modelo.clone().unwrap_or_default();
    match chat.agente {
        Agente::Claude => modelo,
        Agente::Codex => format!("{DO_CODEX}{modelo}"),
        Agente::Gemini => format!("{DO_GEMINI}{modelo}"),
    }
}

/// As linhas do Gemini: o padrão dele e os modelos que a conta tem no Antigravity (o Gemini, e
/// também os de outras casas que o Antigravity oferece). Um escolhido que saiu da lista continua
/// nela, para dar para ver e sair dele.
fn linhas_do_gemini(escolhido: Option<&str>, oferecidos: &[agente_gemini::ModeloDoGemini]) -> Vec<LinhaDeModelo> {
    let mut linhas = vec![LinhaDeModelo::new(DO_GEMINI, "Padrão", "O modelo que o Antigravity escolher")];
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
    let mut linhas = vec![LinhaDeModelo::new(DO_CODEX, "Padrão", "O modelo que o Codex escolher")];
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

/// Os grupos da aba `aba` do seletor. Claude: as famílias, que é o que se usa no dia a dia, e
/// embaixo as versões exatas, em linhas de uma altura só. Codex: o padrão e o catálogo dele. Na
/// aba que não é a do chat, nada aparece como escolhido nem em uso.
fn grupos_de_modelo(motor: &Motor, chat: &Chat, aba: Agente) -> Vec<GrupoDeEscolha> {
    let do_chat = chat.agente == aba;
    let escolhido = chat.modelo.as_deref().filter(|_| do_chat);
    if aba == Agente::Codex {
        return vec![GrupoDeEscolha { titulo: None, itens: itens_de_modelo(linhas_do_codex(escolhido, &agente_codex::modelos())) }];
    }
    if aba == Agente::Gemini {
        return vec![GrupoDeEscolha { titulo: None, itens: itens_de_modelo(linhas_do_gemini(escolhido, &agente_gemini::modelos())) }];
    }
    let em_uso = chat.modelo_atual.as_deref().filter(|_| do_chat);
    let motivo = motivo_sem_versao_exata(motor.versao_claude);
    let mut grupos = vec![GrupoDeEscolha {
        // Com as abas, o nome do agente já está nelas.
        titulo: (!com_abas_de_agente(chat)).then(|| "Claude".into()),
        itens: itens_de_modelo(linhas_de_modelo(escolhido, em_uso, padrao_da_pasta(motor, chat))),
    }];
    // A conta é do Claude, então mora na aba dele, e não solta na caixa (dono, 16/11/2026: "por
    // que tem essa conta aqui se agora tem o Codex também?"). Só com mais de uma conta.
    if motor.contas.len() > 1 {
        grupos.push(GrupoDeEscolha { titulo: Some("Conta do Claude".into()), itens: itens_de_conta(motor, chat) });
    }
    grupos.push(GrupoDeEscolha {
        titulo: Some("Versões exatas".into()),
        itens: itens_de_modelo(linhas_de_versao(escolhido, em_uso, motivo.as_deref())),
    });
    grupos
}

/// O prefixo das chaves do grupo "Conta do Claude" no seletor: `conta:<id>`, ou `conta:auto`.
const DA_CONTA: &str = "conta:";

/// As linhas do grupo "Conta do Claude": cada conta com o uso do plano dela e a automática. A que
/// o chat usa leva o selo "em uso" (o visto do seletor é do modelo); a sem login diz isso.
fn itens_de_conta(motor: &Motor, chat: &Chat) -> Vec<ItemDeEscolha> {
    let escolha = motor.escolha_de_conta(chat);
    let selo = |sim: bool| if sim { vec![Selo::neutro("em uso")] } else { Vec::new() };
    let mut itens: Vec<ItemDeEscolha> = motor
        .contas
        .iter()
        .map(|c| {
            let uso = motor.uso_da_conta(&c.id);
            let descricao = if motor.conta_sem_login(&c.id) { "Sem login: entre de novo em Contas do Claude".to_string() } else { com_maiuscula(&descricao_da_conta(&uso)) };
            ItemDeEscolha {
                descricao: Some(descricao.into()),
                selos: selo(c.id == escolha),
                termos: Some(format!("conta {}", c.nome).into()),
                ..ItemDeEscolha::new(format!("{DA_CONTA}{}", c.id), c.nome.clone())
            }
        })
        .collect();
    itens.push(ItemDeEscolha {
        descricao: Some("A conta com mais folga no plano".into()),
        selos: selo(escolha == contas::AUTOMATICA),
        termos: Some("conta automática".into()),
        ..ItemDeEscolha::new(format!("{DA_CONTA}{}", contas::AUTOMATICA), "Automática")
    });
    itens
}

/// Ao abrir o seletor (clique ou Ctrl+Shift+M) e ao trocar de aba: as linhas de agora, a busca
/// limpa e o realce no escolhido. `aba` é a aba à vista; sem ela, a do agente do chat.
pub(super) fn preparar_modelos(
    lista: &Entity<ListaDeEscolha>,
    motor: &Entity<Motor>,
    chat: &str,
    aba: Option<Agente>,
    window: &mut Window,
    cx: &mut App,
) {
    let dados = {
        let m = motor.read(cx);
        m.chat(chat).map(|c| (grupos_de_modelo(m, c, aba.unwrap_or(c.agente)), chave_do_modelo(c)))
    };
    let Some((grupos, escolhido)) = dados else {
        return;
    };
    lista.update(cx, |l, cx| {
        // O logo fica só no rótulo do grupo; os itens vão sem símbolo.
        l.simbolo_do_grupo("Claude", Simbolo::Marca(Marca::Claude));
        l.ficha_no_escolhido(true);
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
    /// O nível que o automático usa neste chat (o índice em `ESFORCOS`), se já se sabe.
    padrao: Option<usize>,
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

/// A linha do Ultracode: vale só para a próxima mensagem, com o custo sempre à vista e, embaixo,
/// o que pode atrapalhar (a vez de outra conversa, os fluxos desligados, o modo que pergunta).
fn linha_ultracode(d: &Rc<DadosEsforco>) -> AnyElement {
    let mut avisos = vec![String::from("Gasta bem mais do plano e roda no esforço Extra")];
    if d.outro_ultracode {
        avisos.push("O Ultracode roda em outra conversa: esta mensagem iria sem ele".into());
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
    // Desabilitada, a linha mostra o motivo curto.
    linha.desabilitada(motivo.clone()).into_any_element()
}

fn corpo_do_esforco(d: &Rc<DadosEsforco>, window: &Window) -> impl IntoElement + use<> {
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
        };
        let dica: SharedString = format!("Modo de permissão: {}\n{}", nome_do_modo(&atual), descricao).into();
        let chip = |aberto: bool| {
            let (icone, cor) = icone_do_modo(&atual);
            let mut chip = Chip::new(b.id("chip-modo"), nome_do_modo(&atual))
                .simbolo(Simbolo::Icone(icone, Some(cor)))
                .tamanho(b.tamanho())
                .mudado(mudado)
                .seta(b.larga())
                .so_icone(b.estreita())
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
        // Na dupla, o chip diz os dois: primeiro quem trabalhou por último.
        let nome = match &chat.parceiro {
            Some(p) => format!("{} + {}", chat.agente.nome(), p.agente.nome()),
            None => nome,
        };
        let aberto = self.pop_caixa == Some((b.painel, PopCaixa::Modelo));
        let marca_de = Marca::do_agente;
        // Na dupla, as duas marcas, em toda largura (a de quem trabalhou por último na frente).
        let simbolo = match &chat.parceiro {
            Some(p) => Simbolo::Dupla(marca_de(chat.agente), marca_de(p.agente)),
            None => Simbolo::Marca(marca_de(chat.agente)),
        };
        let mut chip = Chip::new(b.id("chip-modelo"), if b.larga() { nome.clone() } else { curto })
            .simbolo(simbolo)
            .tamanho(b.tamanho())
            .mudado(chat.modelo.is_some() || chat.agente != motor.config.agente_padrao)
            .seta(b.larga())
            .so_icone(b.estreita())
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
        let juntos = chat.revisao_em_dupla;
        // Numa conversa já grande, trocar o modelo do Claude custa uma releitura inteira.
        let aviso_de_troca: Option<SharedString> = chat.contexto_grande().filter(|_| aba == Agente::Claude).map(|t| {
            format!("Trocar o modelo agora faz a próxima mensagem reler {} por inteiro.", super::tokens(t)).into()
        });
        popover(b.id("pop-modelo"))
            .anchor(Anchor::BottomLeft)
            .offset(px(8.))
            .p(r(6.))
            .open(aberto)
            .on_open_change({
                let (lista, motor_e, janela, chat_id) = (lista.clone(), motor_e.clone(), janela.clone(), chat_id.clone());
                move |abriu, window, cx| {
                    if *abriu && let Some(m) = &motor_e {
                        // Abre sempre na aba de quem responde no chat.
                        let _ = janela.update(cx, |this, _| this.aba_do_modelo = None);
                        preparar_modelos(&lista, m, &chat_id, None, window, cx);
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
                        window.subscribe(&lista, app, move |_, evento: &EventoEscolha, window, cx| {
                            if let EventoEscolha::Escolheu(chave) = evento {
                                let valor = chave.to_string();
                                // A linha que só explica não escolhe nada (e nem fecha o popover).
                                if valor == SEM_VERSOES {
                                    return;
                                }
                                // Uma linha do grupo da conta troca só a conta do chat.
                                if let Some(conta) = valor.strip_prefix(DA_CONTA) {
                                    let conta = conta.to_string();
                                    mudar(&motor_e, &chat_id, cx, move |c| c.conta = Some(conta));
                                    abrir_ou_fechar(&janela, painel, PopCaixa::Modelo, false, cx);
                                    return;
                                }
                                // A linha escolhida diz também quem responde: as do grupo do Codex
                                // e do Gemini passam o chat para eles, as outras para o Claude.
                                let (agente, modelo) = match (valor.strip_prefix(DO_CODEX), valor.strip_prefix(DO_GEMINI)) {
                                    (Some(slug), _) => (Agente::Codex, slug.to_string()),
                                    (_, Some(valor)) => (Agente::Gemini, valor.to_string()),
                                    _ => (Agente::Claude, valor),
                                };
                                let nome = match agente {
                                    Agente::Codex if modelo.is_empty() => "Codex".to_string(),
                                    Agente::Codex => agente_codex::nome_do_modelo(&modelo),
                                    Agente::Gemini if modelo.is_empty() => "Gemini".to_string(),
                                    Agente::Gemini => agente_gemini::nome_do_modelo(&modelo),
                                    Agente::Claude => com_claude(&rotulo_modelo(&modelo, None)),
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
                let abas = com_abas.then(|| {
                    let (lista, motor_e, janela, chat_id) = (lista.clone(), motor_e.clone(), janela.clone(), chat_id.clone());
                    let agentes = agentes_das_abas.clone();
                    let atual = agentes.iter().position(|a| *a == aba).unwrap_or(0);
                    let nomes: Vec<&'static str> = agentes.iter().map(|a| a.nome()).collect();
                    let controle = ControleSegmentado::new(("aba-modelo", painel as usize), nomes, atual, move |nova, window, cx| {
                        let agente = agentes.get(nova).copied().unwrap_or(Agente::Claude);
                        let _ = janela.update(cx, |this, cx| {
                            this.aba_do_modelo = Some((painel, agente));
                            cx.notify();
                        });
                        if let Some(m) = &motor_e {
                            preparar_modelos(&lista, m, &chat_id, Some(agente), window, cx);
                        }
                    })
                    .largura_total(true);
                    div().px(esp::E2).pb(esp::E6).child(controle)
                });
                // Os dois agentes no mesmo chat (dono, 16/11/2026: "dar pra usar o Codex e o Claude no
                // mesmo chat e eles conseguirem conversar entre si e verem o que cada um tá fazendo").
                // A dupla nasce sozinha ao trocar de agente ou chamar o outro com @ (17/11/2026); a
                // chave só liga a revisão, os dois em toda mensagem.
                let dupla = com_abas.then(|| {
                    let (motor_e, chat_id) = (motor_e.clone(), chat_id.clone());
                    div().child(
                        LinhaDeOpcao::new(("dupla", painel as usize), "Os dois em toda mensagem")
                            .descricao(
                                "Um responde e o outro revisa. Sem isso, trocar de modelo ou começar com @claude, @codex ou \
                                 @gemini já passa a conversa de um para o outro.",
                            )
                            .interruptor(juntos, move |ligar, _, cx| {
                                if let Some(m) = &motor_e {
                                    m.update(cx, |m, cx| m.revisao_em_dupla(&chat_id, ligar, cx));
                                }
                            }),
                    )
                });
                // As teclas na coluna do texto das linhas (8 do item).
                let aviso = aviso_de_troca
                    .clone()
                    .map(|a| div().px(esp::E8).pt(esp::E6).metadado().text_color(Cores::tenue()).child(a));
                div().w(largura).flex().flex_col().children(abas).child(lista.clone()).children(aviso).children(dupla).when(!estreita, |s| {
                    let [linha, teclas] = rodape_de_teclas([
                        dica_de_tecla(&["↑", "↓"], "escolhe"),
                        dica_de_tecla(&["Enter"], "usa"),
                    ], window);
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
            // No automático, as barrinhas e o rótulo dizem o nível que ele usa ("Auto · Extra").
            let no_auto = padrao.filter(|_| atual == 0);
            let rotulo: SharedString = match no_auto.filter(|_| b.larga()) {
                Some(p) => format!("Auto · {}", ESFORCOS[p].1).into(),
                None => nome.into(),
            };
            let chip = Chip::new(b.id("chip-esforco"), rotulo)
                .simbolo(Simbolo::Sinal { nivel: no_auto.unwrap_or(atual) as u8, total: 5 })
                .seta(b.larga())
                .so_icone(b.estreita());
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
            armado,
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

    pub(super) fn botao_anexar(&self, b: &Barra, window: &Window, cx: &Context<Self>) -> AnyElement {
        let dados = DadosAnexar { janela: cx.entity().downgrade(), chat: b.chat.clone(), painel: b.painel };
        let dica = "Anexar arquivo ou contexto";
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
        let anexar = DadosAnexar { janela: cx.entity().downgrade(), chat: b.chat.clone(), painel: b.painel };
        let (motor_e, fixado, rodando, painel) = (self.motor.clone(), chat.fixado, b.rodando, b.painel);
        // Com o turno rodando, o Enviar entra nele; a escolha de esperar a vez na fila fica aqui.
        // Quem já ia pela fila de qualquer jeito não precisa da opção.
        let depois = composer::dirige_o_turno(motor, &chat.id)
            .then(|| (cx.entity().downgrade(), b.composer.read(cx).value().trim().is_empty()));
        // Mandar numa hora marcada (`agendar.rs`): quando as 5 h renovarem, ou na hora escrita.
        let agendar = crate::ui::agendar::DadosDoMenu {
            janela: cx.entity().downgrade(),
            painel: b.painel,
            vazio: b.composer.read(cx).value().trim().is_empty(),
            renova: motor.renovacao_do_chat(&chat.id),
        };
        let claude = chat.agente == Agente::Claude;
        let tem_agentes = crate::ui::mapa_agentes::tem_agentes(chat);
        // A segunda resposta: o mesmo pedido para o outro agente, num painel ao lado.
        let outro = chat.agente.outro();
        let raia = outro.instalado().then(|| {
            let rotulo = format!("Perguntar também ao {}", outro.nome());
            let caixa = b.composer.read(cx).value();
            (cx.entity().downgrade(), rotulo, caixa.trim().is_empty() || caixa.trim_start().starts_with('/'))
        });
        let tamanho = if estreita { TamanhoBotao::Barra } else { TamanhoBotao::Controle };
        botao_icone(b.id("mais-caixa"), Lucide::Ellipsis, "Mais opções", tamanho, window, cx)
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, window, _| {
                let largura = tam(window, 300.);
                let (j1, j2, j3) = (anexar.janela.clone(), anexar.janela.clone(), anexar.janela.clone());
                let (m1, m2, c1, c2) = (motor_e.clone(), motor_e.clone(), anexar.chat.clone(), anexar.chat.clone());
                let c3 = anexar.chat.clone();
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
                    .map(|menu| crate::ui::agendar::itens_do_menu(menu, &agendar))
                    .when_some(raia.clone(), |menu, (janela, rotulo, vazio)| {
                        menu.item(PopupMenuItem::new(rotulo).icon(Lucide::Columns2).disabled(vazio).on_click(move |_, window, cx| {
                            let _ = janela.update(cx, |this, cx| this.enviar_aos_dois(painel, window, cx));
                        }))
                        .separator()
                    })
                    .item(item_de_comandos(j1, painel))
                    // O `/compact` é do Claude Code.
                    .when(claude, |menu| {
                        menu.item(PopupMenuItem::new("Compactar a conversa").icon(Lucide::Minimize).on_click(move |_, _, cx| {
                            let _ = j2.update(cx, |this, cx| this.compactar_conversa(painel, cx));
                        }))
                    })
                    // O mapa dos subagentes, quando a conversa abriu algum.
                    .when(tem_agentes, |menu| {
                        let chat = c3.clone();
                        menu.item(PopupMenuItem::new("Agentes desta conversa").icon(Lucide::Workflow).on_click(move |_, window, cx| {
                            let _ = j3.update(cx, |this, cx| this.abrir_mapa_de_agentes(chat.clone(), window, cx));
                        }))
                    })
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
                    .separator()
                    .item(
                        PopupMenuItem::new(if fixado { "Desafixar conversa" } else { "Fixar conversa" })
                            .icon(if fixado { Lucide::PinOff } else { Lucide::Pin })
                            .on_click(move |_, _, cx| {
                                if let Some(m) = &m2 {
                                    m.update(cx, |m, cx| m.alternar_fixado(&c2, cx));
                                }
                            }),
                    );
                if estreita {
                    menu = itens_de_anexar(menu.separator(), &anexar, false);
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
    fn grupo_do_codex_com_o_padrao_o_catalogo_e_o_que_saiu_dele() {
        use super::{DO_CODEX, linhas_do_codex};
        use crate::agente_codex::ModeloDoCodex;
        let catalogo = [ModeloDoCodex { slug: "gpt-6.1-sol".into(), nome: "GPT-6.1-Sol".into(), descricao: "Latest workhorse.".into(), esforco: "low".into() }];
        let linhas = linhas_do_codex(None, &catalogo);
        assert_eq!(linhas.iter().map(|l| (l.valor.as_str(), l.titulo.as_str())).collect::<Vec<_>>(), [(DO_CODEX, "Padrão"), ("codex:gpt-6.1-sol", "GPT-6.1-Sol")]);
        // A descrição vem em português quando a frase do catálogo é reconhecida; senão, nenhuma.
        assert_eq!(linhas[1].descricao, "Para código e o trabalho do dia a dia");
        use super::descricao_do_codex;
        assert_eq!(descricao_do_codex("Frontier intelligence for the most demanding work."), "O mais capaz, para o trabalho mais difícil");
        assert_eq!(descricao_do_codex("Fast and affordable model for easier tasks."), "Rápido e econômico, para tarefas fáceis");
        assert_eq!(descricao_do_codex("Older fast and efficient model."), "Geração anterior");
        assert_eq!(descricao_do_codex("GPT-6.1 Terra Codex model."), "");
        // O escolhido que saiu do catálogo segue na lista; o que está nele não repete.
        assert_eq!(linhas_do_codex(Some("gpt-5-velho"), &catalogo).last().map(|l| l.valor.as_str()), Some("codex:gpt-5-velho"));
        assert_eq!(linhas_do_codex(Some("gpt-6.1-sol"), &catalogo).len(), 2);
    }

    #[test]
    fn seletor_de_modelo_com_a_versao_em_uso() {
        let linhas = linhas_de_modelo(None, Some("claude-opus-5-5"), "");
        // O título é o nome seco da família; a versão que está rodando vai no selo.
        assert_eq!(titulos(&linhas), ["Padrão", "Fable", "Opus", "Sonnet", "Haiku"]);
        assert_eq!(linhas[2].versao.as_deref(), Some("5.5"));
        assert_eq!(linhas[2].descricao, "Para trabalho complexo e o dia a dia");
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
        assert_eq!(titulos(&linhas), ["Padrão", "Fable", "Opus", "Sonnet", "Haiku"]);
        assert!(linhas.iter().all(|l| !l.em_uso && l.versao.is_none()));
        // Escolhido um apelido, o Padrão diz o da pasta e não o que está em uso.
        let linhas = linhas_de_modelo(Some("sonnet"), Some("claude-opus-5-5"), "opus");
        assert_eq!(linhas[0].descricao, "Padrão · Opus");
        assert_eq!(linhas[2].versao.as_deref(), Some("5.5"));
        assert_eq!(linhas[3].versao, None);
    }

    #[test]
    fn conversa_antiga_ganha_a_sexta_linha() {
        let id = "claude-sonnet-4-5-20250929";
        let linhas = linhas_de_modelo(Some(id), Some(id), "");
        assert_eq!(linhas.len(), 6);
        let ultima = &linhas[5];
        assert_eq!((ultima.valor.as_str(), ultima.titulo.as_str()), (id, "Sonnet 4.5"));
        assert_eq!(ultima.descricao, "escolhido antes");
        assert!(ultima.em_uso);
        // O apelido não toma a versão do nome completo.
        assert_eq!(linhas[3].versao, None);
        assert!(!linhas[3].em_uso);
    }

    #[test]
    fn versao_exata_escolhida_nao_vira_a_sexta_linha() {
        // Ela vive no grupo das versões, e as famílias ficam com o nome seco.
        let linhas = linhas_de_modelo(Some("claude-opus-5-5"), Some("claude-opus-5-5"), "");
        assert_eq!(titulos(&linhas), ["Padrão", "Fable", "Opus", "Sonnet", "Haiku"]);
        assert!(linhas.iter().all(|l| !l.em_uso && l.versao.is_none()));
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
