//! A casca de um painel de conversa: o cabeçalho de uma linha (a aba, as ações do painel e os
//! botões da janela), o menu "⋯", o popover de uso, a tela sem conversa e a soltura de arquivos.
//! As mensagens ficam em `mensagens.rs` e a caixa em `composer.rs`.

use std::{collections::HashMap, rc::Rc};

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
    AlternarPainel, Gaveta, Janela,
    NovaConversa, QualFlutuante,
    componentes::{
        Aba, Anel, CabecalhoPopover, CorpoPopover, EstadoVazio, EventoEscolha, GrupoDeEscolha, ItemDeEscolha,
        LarguraPainel, LarguraPopover, PreviaArrasto, Reflexo as _, Simbolo, TamanhoBotao, TomAba,
        botao_fantasma, botao_icone, botao_primario, cor_da_faixa,
        formatar_custo,
        metal::{Encaixe, abertura_lisa},
        icone_de_estado, popover, sinal_do_chat,
    },
    busca_conversa, composer, menus,
    dialogos::{com_til, nome_da_pasta},
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

/// O histórico do cabeçalho: os chats do mexido por último ao mais antigo, com o ícone do estado
/// (o da lista), a hora e a pasta; os arquivados num grupo no fim. Um chat à vista noutro painel
/// diz qual.
/// A chave da linha "Novo chat neste painel", no "+" do cabeçalho.
const NOVO_CHAT_NO_PAINEL: &str = "\u{0}novo-chat";
/// O grupo só da linha de criar, no alto do "+", sem título (os chats vão em "Recentes").
const GRUPO_DE_ACAO: &str = "\u{0}acao";

/// Uma linha do histórico, em dados simples: a busca própria (sem acento e por palavras, item 4.3)
/// monta as linhas de novo a cada letra, sem o motor.
#[derive(Clone)]
struct LinhaDoHistorico {
    grupo: Option<&'static str>,
    chave: String,
    titulo: String,
    descricao: String,
    sinal: Option<super::componentes::Sinal>,
    termos: String,
}

fn linhas_do_historico(motor: &Motor, noutros: &[(String, usize)], so: Option<&[String]>, a_vista: Option<&str>) -> Vec<LinhaDoHistorico> {
    // O "+N" da faixa das abas mostra só as que saíram dela.
    let mut chats: Vec<&Chat> = motor.chats_da_lista().filter(|c| so.is_none_or(|so| so.contains(&c.id))).collect();
    chats.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
    // A pasta só aparece quando é outra que a do chat deste painel: com todos os chats em ~/eita,
    // o "· eita" em toda linha não dizia nada (rodada de uso, item 3.3).
    let pasta_do_painel = a_vista.and_then(|id| motor.chat(id)).map(|c| c.cwd.clone());
    let linha = |grupo: Option<&'static str>, c: &Chat| {
        let quando = super::tempo_relativo(c.atualizado_em);
        let quando = if quando == "agora" { quando } else { format!("há {quando}") };
        let pasta = if motor.sem_pasta(c) {
            Some("sem pasta".to_string())
        } else {
            (pasta_do_painel.as_ref() != Some(&c.cwd)).then(|| nome_da_pasta(&c.cwd))
        };
        let onde = if a_vista == Some(c.id.as_str()) {
            "à vista neste painel · ".to_string()
        } else {
            noutros.iter().find(|(id, _)| *id == c.id).map(|(_, n)| format!("no painel {n} · ")).unwrap_or_default()
        };
        // O estado e o começo da última fala ("Terminou · Pronto, mandei as duas…"), com o quando.
        let fala = super::falas::estado_e_fala(motor, c);
        let mut partes: Vec<String> = Vec::new();
        if !fala.is_empty() {
            partes.push(fala);
        }
        partes.push(quando);
        partes.extend(pasta);
        LinhaDoHistorico {
            grupo,
            chave: c.id.clone(),
            titulo: c.titulo.clone(),
            descricao: format!("{onde}{}", partes.join(" · ")),
            sinal: sinal_do_chat(motor, c),
            termos: com_til(&c.cwd),
        }
    };
    let (arquivados, ativos): (Vec<&Chat>, Vec<&Chat>) = chats.into_iter().partition(|c| c.arquivado);
    // O que espera você (pedido, parada, erro) vem num grupo no alto, antes da ordem por data
    // (rodada de uso, item 4.1).
    let (esperando, ativos): (Vec<&Chat>, Vec<&Chat>) = ativos.into_iter().partition(|c| {
        a_vista != Some(c.id.as_str())
            && motor.espera_do_chat(c).is_some_and(|e| e != crate::motor::Espera::Nova)
    });
    let recentes = if esperando.is_empty() { None } else { Some("Recentes") };
    let mut linhas: Vec<LinhaDoHistorico> = Vec::new();
    for (grupo, chats) in [(Some("Esperando você"), esperando), (recentes, ativos), (Some("Arquivados"), arquivados)] {
        linhas.extend(chats.into_iter().map(|c| linha(grupo, c)));
    }
    linhas
}

/// As linhas que casam com a busca, nos grupos delas. Sem acento dos dois lados e por palavras
/// soltas, no título, na pasta ou numa fala; as que casam no título primeiro, e dentro disso a
/// ordem de antes (por data).
fn grupos_que_casam(
    linhas: &[LinhaDoHistorico],
    preparados: &HashMap<String, Rc<super::lateral::Preparado>>,
    busca: &str,
) -> Vec<GrupoDeEscolha> {
    let palavras = super::acentos::palavras(busca);
    let mut achadas: Vec<(super::lateral::Casou, &LinhaDoHistorico)> = linhas
        .iter()
        .filter_map(|l| {
            let casou = match preparados.get(&l.chave) {
                _ if palavras.is_empty() => Some(super::lateral::Casou::NoTitulo),
                Some(p) => p.casa(&palavras),
                None => super::acentos::tem_todas(&super::acentos::normalizar(&format!("{} {}", l.titulo, l.termos)), &palavras)
                    .then_some(super::lateral::Casou::NoTitulo),
            };
            Some((casou?, l))
        })
        .collect();
    achadas.sort_by_key(|(casou, _)| *casou);
    let mut grupos: Vec<GrupoDeEscolha> = Vec::new();
    // Os grupos na ordem em que aparecem nas linhas.
    let mut ordem: Vec<Option<&'static str>> = Vec::new();
    for l in linhas {
        if !ordem.contains(&l.grupo) {
            ordem.push(l.grupo);
        }
    }
    for grupo in ordem {
        let itens: Vec<ItemDeEscolha> = achadas
            .iter()
            .filter(|(_, l)| l.grupo == grupo)
            .map(|(_, l)| {
                if l.chave == NOVO_CHAT_NO_PAINEL {
                    return ItemDeEscolha {
                        simbolo: Some(Simbolo::icone_cor(Lucide::Plus, Cores::destaque_texto())),
                        termos: Some(l.termos.clone().into()),
                        ..ItemDeEscolha::new(NOVO_CHAT_NO_PAINEL, l.titulo.clone())
                    };
                }
                ItemDeEscolha {
                    simbolo: Some(Simbolo::Estado(l.sinal)),
                    descricao: Some(l.descricao.clone().into()),
                    termos: Some(l.termos.clone().into()),
                    ..ItemDeEscolha::new(l.chave.clone(), l.titulo.clone())
                }
            })
            .collect();
        if !itens.is_empty() {
            grupos.push(GrupoDeEscolha { titulo: grupo.filter(|g| *g != GRUPO_DE_ACAO).map(Into::into), itens });
        }
    }
    grupos
}

/// A dica da aba de fundo, em até duas linhas: o título; e o estado com quando ("Terminou há 3
/// min") e o começo da última fala, ou a pasta quando não há o que dizer.
fn dica_da_aba(motor: &Motor, chat: &Chat) -> String {
    let quando = super::tempo_relativo(chat.atualizado_em);
    let quando = if quando == "agora" { quando } else { format!("há {quando}") };
    let estado = super::falas::estado_do_chat(motor, chat);
    let fala = super::falas::ultima_fala(chat).unwrap_or_default();
    let segunda = match (estado.is_empty(), fala.is_empty()) {
        (true, true) => format!("{quando} · {}", pasta_em_texto(motor, chat)),
        (true, false) => format!("{quando} · {fala}"),
        (false, true) => format!("{estado} {quando}"),
        (false, false) => format!("{estado} {quando} · {fala}"),
    };
    format!("{}\n{segunda}", chat.titulo)
}

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
    // O ramo do git vai junto da pasta, lido quando a dica aparece.
    let pasta = (!motor.sem_pasta(chat)).then(|| chat.cwd.clone());
    div()
        .id(("simbolo-aba", painel_id as usize))
        .flex_none()
        .child(icone_de_estado(sinal_do_chat(motor, chat), 16.).vivo(format!("aba-{painel_id}-{}", chat.id)))
        .tooltip(move |window, cx| {
            let texto = match &pasta {
                Some(pasta) => super::ramo::com_ramo(&dica, pasta),
                None => dica.to_string(),
            };
            Tooltip::new(texto).build(window, cx)
        })
        .into_any_element()
}

/// A largura de um título de aba (o `rotulo()`: 13, médio), em px de agora.
fn largura_do_rotulo(window: &Window, texto: &str) -> Pixels {
    let mut fonte = window.text_style().font();
    fonte.weight = FontWeight::MEDIUM;
    let texto = texto.replace('\n', " ");
    let corrida = TextRun { len: texto.len(), font: fonte, color: Hsla::default(), background_color: None, underline: None, strikethrough: None };
    window.text_system().shape_line(texto.into(), tam(window, 13.), &[corrida], None).width
}

/// O que uma aba ocupa além do título (a 100%): a borda (2), a folga da esquerda (8), o ícone
/// (16), o vão até o título (6) e a folga da direita (12). O × aparece por cima, no hover, sem
/// guardar lugar: contado aqui (eram 74, com os 20 dele e mais um vão), a conta via a faixa
/// 26 px mais cheia por aba e mandava a aba de fundo para o ícone com espaço sobrando (dono,
/// 17/11/2026: "por que tá ficando só o logo aqui?").
const ABA_SEM_TITULO: f32 = 44.;
/// A aba de fundo só com o ícone: um quadrado da altura da aba.
const ABA_SO_ICONE: f32 = 32.;
/// O que a aba à vista ocupa além do título, com o número do painel (18 e o vão de 8).
const NUMERO_NA_ABA: f32 = 26.;
/// O "+N" do fim da faixa, com as abas que saíram dela (um botão de barra com "+2").
const BOTAO_MAIS: f32 = 40.;

/// Como cada aba de fundo entra na faixa.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum JeitoDaAba {
    Titulo,
    /// Só o ícone, num quadrado.
    Icone,
    /// Fora da faixa (continua no painel; volta quando houver lugar).
    Fora,
}

/// Como as abas de fundo cabem na faixa, sem o título da aba à vista sumir. As de fundo encolhem
/// antes dela, na proporção da largura, como o `flex_shrink` faz; a que receberia menos que o
/// `minimo` dela (4 letras e as reticências, ou o título inteiro, se for menor) fica só com o
/// ícone, a mais estreita primeiro, porque cada uma que vira ícone devolve espaço às outras. Se
/// mesmo com todas em ícone a aba à vista não chegar ao mínimo dela, as de fundo saem da faixa, a
/// mais antiga primeiro (a ordem de `naturais` é a das abas). Antes o título encolhia até sobrar
/// "..", e a aba à vista ficava só com o ícone e as reticências (o dono viu, 16/11/2026).
pub(super) fn jeito_das_abas(
    disponivel: f32,
    a_vista: (f32, f32),
    naturais: &[f32],
    minimos: &[f32],
    (so_icone, mais, vao): (f32, f32, f32),
) -> Vec<JeitoDaAba> {
    let n = naturais.len();
    let mut jeito = vec![JeitoDaAba::Titulo; n];
    if disponivel <= 0. {
        return jeito;
    }
    let (natural_a_vista, minima_a_vista) = a_vista;
    loop {
        let titulos: Vec<usize> = (0..n).filter(|i| jeito[*i] == JeitoDaAba::Titulo).collect();
        // Todas as com título (a à vista também) dividem o lugar por igual (`teto_por_igual`); a
        // de fundo que ficaria abaixo do mínimo dela vira ícone, a mais estreita primeiro.
        let teto = teto_por_igual(livre_para_titulos(disponivel, &jeito, (so_icone, mais, vao)), &com_a_vista(natural_a_vista, &titulos, naturais));
        if let Some(i) = titulos
            .iter()
            .copied()
            .filter(|i| naturais[*i].min(teto) < minimos[*i])
            .min_by(|a, b| naturais[*a].total_cmp(&naturais[*b]))
        {
            jeito[i] = JeitoDaAba::Icone;
            continue;
        }
        if natural_a_vista.min(teto) >= minima_a_vista {
            return jeito;
        }
        // A aba à vista abaixo do mínimo dela: primeiro a de fundo mais estreita vira ícone.
        if let Some(i) = titulos.iter().copied().min_by(|a, b| naturais[*a].total_cmp(&naturais[*b])) {
            jeito[i] = JeitoDaAba::Icone;
            continue;
        }
        // Nem com as de fundo em ícone a aba à vista chega ao mínimo dela: a mais antiga sai.
        match (0..n).find(|i| jeito[*i] != JeitoDaAba::Fora) {
            Some(i) => jeito[i] = JeitoDaAba::Fora,
            None => return jeito,
        }
    }
}

/// O lugar da faixa para os títulos: o que sobra dos vãos, das abas em ícone e do "+N" das que
/// saíram.
pub(super) fn livre_para_titulos(disponivel: f32, jeito: &[JeitoDaAba], (so_icone, mais, vao): (f32, f32, f32)) -> f32 {
    let conta = |j: JeitoDaAba| jeito.iter().filter(|x| **x == j).count() as f32;
    let com_mais = if conta(JeitoDaAba::Fora) > 0. { mais + vao } else { 0. };
    (disponivel - vao * (jeito.len() as f32 - conta(JeitoDaAba::Fora)) - so_icone * conta(JeitoDaAba::Icone) - com_mais).max(0.)
}

fn com_a_vista(natural_a_vista: f32, titulos: &[usize], naturais: &[f32]) -> Vec<f32> {
    std::iter::once(natural_a_vista).chain(titulos.iter().map(|i| naturais[*i])).collect()
}

/// O teto que reparte `livre` por igual entre abas que pedem `naturais`: a que pede menos que a
/// parte dela fica com o que pede, e o que sobra vai para as outras (dono, 17/11/2026: "não
/// ficar só a bolinha, mostrar mais e deixar mais igual todos"). Sem aperto, sem teto.
pub(super) fn teto_por_igual(livre: f32, naturais: &[f32]) -> f32 {
    let mut ordem = naturais.to_vec();
    ordem.sort_by(f32::total_cmp);
    let (mut resto, mut faltam) = (livre, ordem.len());
    for natural in ordem {
        let parte = resto / faltam as f32;
        if natural > parte {
            return parte;
        }
        resto -= natural;
        faltam -= 1;
    }
    f32::INFINITY
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
/// O menu do painel: o botão direito no cabeçalho e nas abas, e o Shift+F10 (o "⋯" do cabeçalho
/// saiu a pedido do dono, 16/11/2026: "tira o de 3 pontinhos e deixa só no botão direito"). Com
/// `do_chat`, as ações da conversa valem para aquele chat (a aba de fundo clicada); sem ele, para
/// o chat à vista no painel.
fn menu_do_painel(
    menu: PopupMenu,
    janela: WeakEntity<Janela>,
    motor: Entity<Motor>,
    painel_id: u64,
    do_chat: Option<String>,
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
        let chat = m.chat(do_chat.as_deref().or(painel.chat.as_deref())?)?;
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
/// Compactar). `extras` são as linhas que outros módulos põem depois dos tokens (o peso da pasta).
pub(super) fn uso_da_conversa(motor: &Motor, chat: &Chat, extras: Vec<AnyElement>, acao: AnyElement) -> CorpoPopover {
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
                .when_some(tokens, |s, t| s.child(linha("Tokens", t)))
                .children(extras),
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
const ACOES_APAGADAS: f32 = crate::tema::opacidade::EM_REPOUSO;

/// O grupo de hover das ações de um painel.
fn grupo_das_acoes(painel_id: u64) -> SharedString {
    SharedString::from(format!("acoes-cabecalho-{painel_id}"))
}

impl Janela {
    /// Um painel de conversa (`ix` em `self.paineis`); sem painel, a tela de começar.
    /// O que há à esquerda do painel `ix` da fileira (`None`: a tela de começar, sozinha): a
    /// vizinha, o vão da lista ou a beirada da janela, com a lista recolhida.
    pub(super) fn esquerda_do_painel(&self, ix: Option<usize>) -> crate::tema::Esquerda {
        if ix.is_some_and(|ix| ix > 0) {
            crate::tema::Esquerda::Emendada
        } else if self.orcamento.lista {
            crate::tema::Esquerda::Solta
        } else {
            crate::tema::Esquerda::Beirada
        }
    }

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
        // As mensagens são uma vista em cache (`vista_mensagens`): a Janela redesenhando por outra
        // coisa (o spinner da caixa, a lista, outro painel) não refaz o layout nem a pintura delas.
        let corpo = match (&chat, ix) {
            (Some(_), Some(ix)) => AnyView::from(self.paineis[ix].vista.clone())
                .cached(StyleRefinement::default().flex_1().w_full().min_h(super::mensagens::MINIMO_MENSAGENS).flex().flex_col())
                .into_any_element(),
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
        // A pasta trocou de ramo desde o último turno do chat: o aviso, uma vez (`ramo.rs`).
        let aviso_do_ramo = chat.and_then(|c| self.aviso_do_ramo(motor, c, painel_id, window, cx));
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
        // A fileira das conversas encosta no topo, na direita e embaixo da janela, e cada conversa
        // depois da primeira emenda na da esquerda por uma divisória de 1 px (dono, 16/11/2026).
        // Só os cantos da esquerda da primeira fazem a curva, e só com a lista ao lado: sem ela,
        // a primeira encosta também na beirada da esquerda. Sem a sombra de cartão flutuante,
        // que vazaria por cima da vizinha na emenda.
        let esquerda = self.esquerda_do_painel(ix);
        let raio = crate::tema::raio_encostado(esquerda);
        let cartao = v_flex()
            .id(id_texto("cartao-conversa"))
            .relative()
            .size_full()
            .min_w_0()
            .rounded_l(raio)
            .bg(Cores::cartao())
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
                                .rounded_bl(raio)
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
            .children(chat.zip(ix).and_then(|(chat, ix)| self.fila_do_painel(motor_entidade, &chat.id, ix, window, cx)))
            .children(aviso_do_ramo)
            .children(faixa)
            .when_some(chat.zip(ix), |s, (chat, ix)| {
                let caixa = self.caixa_mensagem(motor, chat, ix, window, cx).into_any_element();
                // A doca está no fluxo: as mensagens terminam onde ela começa, e nenhum texto fica
                // por cima do metal. O `overflow_hidden` do cartão corta reto, então a abertura leva
                // o raio dos cantos de baixo.
                let raio_doca = raio;
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
                                        Corners { bottom_left: raio_doca, ..Default::default() },
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
                                                lado.right_0()
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
                    div()
                        .map(super::mensagens::coluna)
                        .flex()
                        .justify_end()
                        .pb(esp::E6)
                        .child(
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
                // O botão flutua em cima da doca, fora do fluxo: no fluxo, ele roubava altura da
                // conversa ao aparecer e a rolagem pulava (dono, 16/11/2026: "quando selecionar o
                // texto e aparecer o de responder não subir o chat inteiro").
                let doca = div()
                    .relative()
                    .flex_none()
                    .w_full()
                    .child(doca)
                    .children(responder.map(|r| div().absolute().left_0().right_0().bottom(relative(1.)).child(r)));
                s.child(self.permissoes(motor, chat, window, cx)).child(doca)
            })
            .when(arquivos_sobre, |s| {
                s.child(FormaSoltura::new(id_texto("soltura-arquivos"), Soltura::Arquivos).raio_esquerdo(raio))
            })
            // O vidro da peça (fio, luz na quina de cima, sombra na base), por cima de tudo.
            .aresta_encostada(esquerda);
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
        // O botão direito no cabeçalho (na faixa das abas e no vazio dele) abre o menu do painel.
        let menu_do_cabecalho = conversa.map(|_| (cx.entity().downgrade(), motor_entidade.clone()));
        let cabecalho = self
            .arrastavel_com_id(SharedString::from(format!("arrastar-conversa-{painel_id}")))
            .flex_none()
            .flex()
            .items_center()
            .h(altura::FILEIRA_DO_ALTO)
            // 12 dos lados: as abas não encostam na divisória nem na beirada da janela.
            .pl(esp::E12)
            .pr(esp::E12)
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
                // Cada aba é uma peça; o vão depois delas continua arrastando a janela. A faixa
                // corta o que não couber (nunca por cima do "⋯") e guarda a largura dela para o
                // quadro seguinte decidir quais abas ficam só com o ícone.
                let medir = ix.map(|ix| {
                    let (janela, antes) = (cx.entity().downgrade(), self.paineis[ix].largura_abas);
                    move |limites: Bounds<Pixels>, window: &mut Window, _: &mut App| {
                        let largura = limites.size.width;
                        if (largura - antes).abs() < px(1.) {
                            return;
                        }
                        window.on_next_frame(move |_, cx| {
                            let _ = janela.update(cx, |this, cx| {
                                if let Some(p) = this.paineis.iter_mut().find(|p| p.id == painel_id) {
                                    p.largura_abas = largura;
                                    cx.notify();
                                }
                            });
                        });
                    }
                });
                s.child(
                    div()
                        .flex()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .items_center()
                        .gap(esp::E6)
                        .when_some(medir, |s, medir| s.on_prepaint(medir))
                        .children(abas.into_iter().flatten()),
                )
                    .child(acoes)
                    .when(mostra_botoes_janela, |s| s.child(self.botoes_janela_separados(cx)))
            });
        match menu_do_cabecalho {
            Some((janela, motor)) => cabecalho
                .context_menu(move |menu, window, cx| menu_do_painel(menu, janela.clone(), motor.clone(), painel_id, None, window, cx))
                .into_any_element(),
            None => cabecalho.into_any_element(),
        }
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
        // Quais abas de fundo cabem com o título: a conta é feita com a largura da faixa no
        // quadro anterior e a largura natural de cada aba, que não depende da decisão.
        let fundo: Vec<&Chat> = painel.abas.iter().filter(|id| **id != chat.id).filter_map(|id| motor.chat(id)).collect();
        let quatro = largura_do_rotulo(window, "Abcd…");
        let titulo_a_vista = largura_do_rotulo(window, &chat.titulo);
        let resto_a_vista = tam(window, ABA_SEM_TITULO + if self.paineis.len() > 1 && self.lista { NUMERO_NA_ABA } else { 0. });
        // O título inteiro de cada aba, até 260: acima disso, as reticências (a à vista e as de
        // fundo com o mesmo teto, para elas ficarem iguais).
        let teto_natural = tam(window, 260.);
        let natural_a_vista = (resto_a_vista + titulo_a_vista).min(teto_natural);
        // O mínimo da aba à vista: o símbolo, o número, o lugar do × e o começo do título.
        let minima_a_vista = (resto_a_vista + titulo_a_vista.min(quatro)).min(natural_a_vista);
        let (naturais, minimos): (Vec<f32>, Vec<f32>) = fundo
            .iter()
            .map(|c| {
                let titulo = largura_do_rotulo(window, &c.titulo);
                let resto = tam(window, ABA_SEM_TITULO);
                ((resto + titulo).min(teto_natural).as_f32(), (resto + titulo.min(quatro)).as_f32())
            })
            .unzip();
        let jeitos = jeito_das_abas(
            painel.largura_abas.as_f32(),
            (natural_a_vista.as_f32(), minima_a_vista.as_f32()),
            &naturais,
            &minimos,
            (tam(window, ABA_SO_ICONE).as_f32(), tam(window, BOTAO_MAIS).as_f32(), tam(window, 6.).as_f32()),
        );
        let fora: Vec<String> =
            fundo.iter().zip(&jeitos).filter(|(_, j)| **j == JeitoDaAba::Fora).map(|(c, _)| c.id.clone()).collect();
        // O teto de cada aba com título, igual para todas: o lugar que sobra dividido por igual,
        // com quem pede menos ficando com o que pede (dono, 17/11/2026: "deixar mais igual
        // todos"). Sem aperto, cada uma com o título inteiro (até o teto natural).
        let teto = {
            let livre = livre_para_titulos(
                painel.largura_abas.as_f32(),
                &jeitos,
                (tam(window, ABA_SO_ICONE).as_f32(), tam(window, BOTAO_MAIS).as_f32(), tam(window, 6.).as_f32()),
            );
            let com_titulo: Vec<f32> = std::iter::once(natural_a_vista.as_f32())
                .chain(jeitos.iter().zip(&naturais).filter(|(j, _)| **j == JeitoDaAba::Titulo).map(|(_, n)| *n))
                .collect();
            let teto = teto_por_igual(livre, &com_titulo);
            px(if teto.is_finite() { teto } else { teto_natural.as_f32() }.floor())
        };
        let a_vista_maxima = teto.max(minima_a_vista);
        let jeito_de = |id: &str| fundo.iter().position(|c| c.id == id).map_or(JeitoDaAba::Titulo, |i| jeitos[i]);
        painel
            .abas
            .iter()
            .filter_map(|id| {
                if *id == chat.id {
                    // Com outras abas ao lado, a que está à vista para em 280 de largura, para as
                    // outras ficarem com um pedaço do título, e não desce do mínimo dela (o
                    // símbolo, o número, o × e o começo do título).
                    let a_vista = self.aba_a_vista(motor_entidade, motor, chat, ix, window, cx);
                    return Some(div().flex().min_w(minima_a_vista).max_w(a_vista_maxima).child(a_vista).into_any_element());
                }
                let jeito = jeito_de(id);
                if jeito == JeitoDaAba::Fora {
                    return None;
                }
                let icone = jeito == JeitoDaAba::Icone;
                let outro = motor.chat(id)?;
                let fechar = cx.entity().downgrade();
                let (id_abrir, id_fechar) = (id.clone(), id.clone());
                // O título e, na segunda linha, o estado com quando e o começo da última fala: dá
                // para decidir se vale trocar de aba sem trocar (rodada de uso, item 3.3).
                let dica = SharedString::from(dica_da_aba(motor, outro));
                // O ramo do git entra numa linha a mais, lido quando a dica aparece.
                let pasta_da_dica = (!motor.sem_pasta(outro)).then(|| outro.cwd.clone());
                // Toda aba arrasta: para outro painel (vira aba dele) ou para a borda de um painel
                // (vira um painel novo daquele lado).
                let arrasto = ArrastoChat { chat: id.clone(), titulo: outro.titulo.clone().into(), de_painel: Some(painel_id) };
                let previa = simbolo_da_conversa();
                let mut aba = Aba::new(SharedString::from(format!("aba-{painel_id}-{id}")), outro.titulo.clone())
                    // As outras abas do painel são peças lisas, só com o contorno.
                    .tom(TomAba::Inativa)
                    // O ícone da aba é o estado do chat (e o ponto ao lado do × sai).
                    .simbolo(Simbolo::Elemento(icone_de_estado(sinal_do_chat(motor, outro), 16.).vivo(format!("aba-{painel_id}-{id}")).into_any_element()))
                    // Sem lugar para 4 letras e as reticências, só o ícone num quadrado; o título
                    // vai na dica.
                    .quadrada(icone);
                // O lápis do rascunho, com o começo dele na dica (rodada de uso, item 1.4).
                if let Some(rascunho) = self.rascunho_fora_de_vista(id).filter(|_| !icone) {
                    let dica = SharedString::from(rascunho);
                    aba = aba.fim(
                        div()
                            .id(SharedString::from(format!("rascunho-aba-{painel_id}-{id}")))
                            .flex_none()
                            .text_color(Cores::apagado())
                            .child(super::componentes::icone(Lucide::Pencil, 11., window))
                            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx)),
                    );
                }
                let aba = aba
                    .fechar("Fechar a aba (a conversa continua na lista)", false, move |window, cx| {
                        let _ = fechar.update(cx, |this, cx| this.fechar_aba(painel_id, &id_fechar, window, cx));
                    })
                    .construir(window, cx)
                    .max_w(teto)
                    .tooltip(move |window, cx| {
                        let texto = match &pasta_da_dica {
                            Some(pasta) => super::ramo::com_ramo(&dica, pasta),
                            None => dica.to_string(),
                        };
                        Tooltip::new(texto).build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| this.abrir_aba(painel_id, id_abrir.clone(), window, cx)))
                    .on_drag(arrasto, move |arrasto, _, _, cx| {
                        let simbolo = previa.copia().unwrap_or_else(simbolo_da_conversa);
                        cx.new(|_| PreviaArrasto { titulo: arrasto.titulo.clone(), simbolo })
                    })
                    // O botão direito na aba de fundo: o menu do painel, com as ações daquele chat.
                    .context_menu({
                        let (janela, motor, chat) = (cx.entity().downgrade(), motor_entidade.clone(), id.clone());
                        move |menu, window, cx| menu_do_painel(menu, janela.clone(), motor.clone(), painel_id, Some(chat.clone()), window, cx)
                    });
                // Todas as abas com título têm o mesmo teto (o `teto` acima) e não descem do começo do
                // título delas: ninguém vira bolinha enquanto houver lugar para quatro letras.
                let minimo = fundo.iter().position(|c| c.id == *id).map_or(px(0.), |i| px(minimos[i]));
                let casca = super::componentes::casca_da_aba().child(aba);
                Some(if icone {
                    sem_arrastar().flex().flex_none().child(casca).into_any_element()
                } else {
                    sem_arrastar().flex().min_w(minimo).max_w(teto).child(casca).into_any_element()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .chain((!fora.is_empty()).then(|| self.botao_abas_fora(ix, fora.clone(), window, cx)))
            .collect()
    }

    /// A aba do chat à vista: a mesma casca preta em todos os painéis.
    fn aba_a_vista(&self, motor_entidade: &Entity<Motor>, motor: &Motor, chat: &Chat, ix: usize, window: &Window, cx: &Context<Self>) -> AnyElement {
        let aba = self.aba_da_conversa(motor_entidade, motor, chat, ix, window, cx);
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
            (true, false) => TomAba::Ativa,
        };
        let arrastando = self.arrastando_aba == Some(painel_id);
        let previa = Some(simbolo_da_conversa());
        let janela = cx.entity().downgrade();
        let fechar = janela.clone();
        // O × aparece no hover e no foco pelo teclado, também na aba selecionada (dono, 16/11/2026:
        // "quando tá selecionada tem que ficar sem o × também"); o ponto do estado fica no lugar dele.
        let mut aba = Aba::new(SharedString::from(format!("aba-{painel_id}")), chat.titulo.clone())
            .tom(tom)
            .fechar(
                if painel.abas.len() > 1 {
                    "Fechar a aba (a conversa continua na lista)"
                } else {
                    "Fechar este painel (a conversa continua na lista)"
                },
                false,
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
            // Sem dica na aba à vista: o título está ali mesmo, e a dica abria por cima da conversa
            // a cada passada do ponteiro (dono, 16/11/2026: "o de passar o mouse tá bem ruim").
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
                menu_do_painel(menu, menu_janela.clone(), menu_motor.clone(), painel_id, None, window, cx)
            });
        // O clique na aba não move a janela; o vazio em volta dela, sim.
        sem_arrastar().flex().min_w_0().child(aba).into_any_element()
    }

    /// Shift+F10 ou a tecla Menu na aba: abre o "⋯" do painel.
    pub(super) fn abrir_menu_do_painel(&mut self, painel_id: u64, cx: &mut Context<Self>) {
        self.so_um_flutuante(super::QualFlutuante::MenuPainel(painel_id));
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
            // O "+": novo chat neste painel ou um do histórico. O menu do painel, sem o "⋯" (dono,
            // 16/11/2026), abre no botão direito; pelo teclado (Shift+F10, tecla Menu), ancorado
            // aqui, num gatilho sem tamanho.
            .child(
                sem_arrastar()
                    .relative()
                    .child(self.botao_historico(ix, window, cx))
                    .child(div().absolute().right_0().bottom_0().child(self.menu_pelo_teclado(motor_entidade, painel_id, painel.menu_aberto, cx))),
            )
    }

    /// O histórico, o relógio da extensão do Claude Code (dono, 16/11/2026: "coloca essa parte de
    /// histórico também, pra dar pra escolher outro chat"): os chats numa lista com busca, e o
    /// escolhido aparece neste painel, como uma aba. Controlado por `historico_aberto`, para
    /// fechar os outros flutuantes ao abrir.
    fn botao_historico(&self, ix: usize, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let painel = &self.paineis[ix];
        let painel_id = painel.id;
        let lista = painel.busca_historico.clone();
        let foco = lista.read(cx).focus_handle(cx);
        let janela = cx.entity().downgrade();
        popover(("pop-historico", painel_id as usize))
            .anchor(Anchor::TopRight)
            .offset(px(4.))
            .p(esp::E6)
            .open(self.historico_aberto == Some((painel_id, false)))
            .on_open_change(move |abriu, window, cx| {
                let _ = janela.update(cx, |this, cx| {
                    if *abriu {
                        this.abrir_historico(painel_id, None, window, cx);
                    } else if this.historico_aberto == Some((painel_id, false)) {
                        this.historico_aberto = None;
                        cx.notify();
                    }
                });
            })
            .track_focus(&foco)
            .trigger(botao_icone(
                ("historico-painel", painel_id as usize),
                Lucide::Plus,
                "Novo chat ou histórico",
                TamanhoBotao::Barra,
                window,
                cx,
            ))
            .content(move |_, window, _| {
                let largura = (tam(window, LarguraPopover::P400.px()) - tam(window, 12.)).min(window.viewport_size().width - px(44.));
                div().w(largura).child(lista.clone())
            })
    }

    /// O "+N" no fim da faixa das abas, com as que saíram dela por falta de lugar: abre a lista do
    /// histórico só com elas (conferência na bancada, 16/11/2026: sem ele, sumiam da vista).
    fn botao_abas_fora(&self, ix: usize, fora: Vec<String>, window: &Window, cx: &Context<Self>) -> AnyElement {
        let painel = &self.paineis[ix];
        let painel_id = painel.id;
        let lista = painel.busca_historico.clone();
        let foco = lista.read(cx).focus_handle(cx);
        let janela = cx.entity().downgrade();
        let n = fora.len();
        let dica = if n == 1 { "1 aba que não coube aqui".to_string() } else { format!("{n} abas que não couberam aqui") };
        let gatilho = botao_fantasma(("abas-fora", painel_id as usize), None, format!("+{n}"), TamanhoBotao::Barra, window, cx).tooltip(dica);
        sem_arrastar()
            .flex_none()
            .child(
                popover(("pop-abas-fora", painel_id as usize))
                    .anchor(Anchor::TopLeft)
                    .offset(px(4.))
                    .p(esp::E6)
                    .open(self.historico_aberto == Some((painel_id, true)))
                    .on_open_change(move |abriu, window, cx| {
                        let _ = janela.update(cx, |this, cx| {
                            if *abriu {
                                this.abrir_historico(painel_id, Some(fora.clone()), window, cx);
                            } else if this.historico_aberto == Some((painel_id, true)) {
                                this.historico_aberto = None;
                                cx.notify();
                            }
                        });
                    })
                    .track_focus(&foco)
                    .trigger(gatilho)
                    .content(move |_, window, _| {
                        let largura = (tam(window, LarguraPopover::P360.px()) - tam(window, 12.)).min(window.viewport_size().width - px(44.));
                        div().w(largura).child(lista.clone())
                    }),
            )
            .into_any_element()
    }

    /// Abre o histórico do painel: a lista de agora, a busca limpa e o chat à vista marcado. Com
    /// `so`, só esses chats (o "+N" das abas que saíram da faixa).
    fn abrir_historico(&mut self, painel_id: u64, so: Option<Vec<String>>, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(motor), Some(painel)) = (self.motor.clone(), self.paineis.iter().find(|p| p.id == painel_id)) else {
            return;
        };
        let (lista, atual) = (painel.busca_historico.clone(), painel.chat.clone());
        // Os chats à vista nos outros painéis: escolher um deles ativa aquele painel (um chat só
        // aparece num lugar), e a linha avisa.
        let noutros: Vec<(String, usize)> = self
            .paineis
            .iter()
            .enumerate()
            .filter(|(_, p)| p.id != painel_id)
            .filter_map(|(i, p)| Some((p.chat.clone()?, i + 1)))
            .collect();
        let mut linhas = linhas_do_historico(motor.read(cx), &noutros, so.as_deref(), atual.as_deref());
        // No "+" do cabeçalho, a primeira linha é a ação de criar um chat neste painel, num grupo
        // só dela, com o atalho à direita; o realce do teclado começa nela (o Enter cria) e o
        // visto fica no chat à vista, como nas outras escolhas (dono, 16/11/2026: "o novo chat tem
        // um certinho do lado"). Os chats vão embaixo, em "Recentes". A busca é a da lista:
        // digitar filtra o histórico, e a ação só fica se casar com o texto dela.
        let novo = so.is_none();
        if novo {
            for l in linhas.iter_mut().filter(|l| l.grupo.is_none()) {
                l.grupo = Some("Recentes");
            }
            linhas.insert(
                0,
                LinhaDoHistorico {
                    grupo: Some(GRUPO_DE_ACAO),
                    chave: NOVO_CHAT_NO_PAINEL.into(),
                    titulo: "Novo chat neste painel".into(),
                    descricao: String::new(),
                    sinal: None,
                    termos: "novo chat criar conversa".into(),
                },
            );
        }
        let escolhido: Option<SharedString> = atual.map(Into::into);
        // As teclas do Ctrl+N (o mesmo chat novo no painel ativo, que é este) na linha de criar.
        let teclas = gpui_kit::component::kbd::Kbd::binding_for_action(&super::NovaConversa, None, window).map(|k| k.partes());
        let com_teclas = move |mut grupos: Vec<GrupoDeEscolha>| {
            for item in grupos.iter_mut().flat_map(|g| g.itens.iter_mut()) {
                if item.chave.as_ref() == NOVO_CHAT_NO_PAINEL {
                    item.teclas = teclas.clone();
                }
            }
            grupos
        };
        // O texto de cada chat, preparado para a busca (o mesmo guardado da lista).
        let preparados: HashMap<String, Rc<super::lateral::Preparado>> = {
            let m = motor.read(cx);
            linhas
                .iter()
                .filter_map(|l| m.chat(&l.chave))
                .map(|c| (c.id.clone(), super::lateral::preparado(&self.busca_preparada, c)))
                .collect()
        };
        let grupos = com_teclas(grupos_que_casam(&linhas, &preparados, ""));
        let linhas = Rc::new(linhas);
        self.so_um_flutuante(QualFlutuante::Historico(painel_id));
        self.historico_aberto = Some((painel_id, so.is_some()));
        lista.update(cx, |l, cx| {
            // Sem a ficha de metal: o visto basta para o chat à vista, e o realce do teclado
            // (na linha de criar) não briga com ela.
            l.ficha_no_escolhido(false);
            l.cursor_em(novo.then(|| NOVO_CHAT_NO_PAINEL.into()));
            // A busca do histórico ignora acento e aceita as palavras em qualquer ordem, como a da
            // lista (rodada de uso, item 4.3).
            l.filtro_proprio(move |busca| com_teclas(grupos_que_casam(&linhas, &preparados, busca)));
            l.definir(grupos, escolhido, cx);
            l.abrir(window, cx);
        });
        cx.notify();
    }

    /// Escolheu (ou fechou com Esc) no histórico do painel: o chat vai para este painel, como uma
    /// aba, e o foco volta à caixa.
    pub(super) fn escolheu_no_historico(&mut self, painel_id: u64, evento: &EventoEscolha, window: &mut Window, cx: &mut Context<Self>) {
        if self.historico_aberto.is_some_and(|(p, _)| p == painel_id) {
            self.historico_aberto = None;
        }
        // A linha do chat novo: cria neste painel, como aba (o "+" da lista, mas aqui).
        if matches!(evento, EventoEscolha::Escolheu(chave) if chave.as_ref() == NOVO_CHAT_NO_PAINEL) {
            self.ativar_painel(painel_id, cx);
            self.novo_chat_direto(window, cx);
            cx.notify();
            return;
        }
        if let EventoEscolha::Escolheu(chat) = evento
            && !self.paineis.iter().any(|p| p.id == painel_id && p.chat.as_deref() == Some(chat.as_ref()))
        {
            self.abrir_aba(painel_id, chat.to_string(), window, cx);
        }
        let ix = match evento {
            EventoEscolha::Escolheu(_) => Some(self.ativo),
            EventoEscolha::Fechou => self.paineis.iter().position(|p| p.id == painel_id),
        };
        if let Some(ix) = ix {
            self.focar_painel(ix, window, cx);
        }
        cx.notify();
    }

    /// O que a direita mostra: o arquivo aberto ou o painel do chat.
    fn o_que_a_direita_mostra(&self, motor: &Motor) -> Direita {
        match self.aba_visivel(motor) {
            super::abas_direita::AbaDireita::Arquivo(_) => Direita::Arquivo,
            super::abas_direita::AbaDireita::Chat => Direita::EsteChat,
        }
    }

    /// O menu do painel pelo teclado (Shift+F10, tecla Menu), controlado por `Painel.menu_aberto`,
    /// num gatilho sem tamanho no lugar onde ficava o "⋯"; o botão direito abre o mesmo menu onde
    /// o mouse está.
    fn menu_pelo_teclado(&self, motor_entidade: &Entity<Motor>, painel_id: u64, aberto: bool, cx: &Context<Self>) -> impl IntoElement + use<> {
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
                    if *aberto {
                        this.so_um_flutuante(super::QualFlutuante::MenuPainel(painel_id));
                    }
                    if let Some(p) = this.paineis.iter_mut().find(|p| p.id == painel_id)
                        && p.menu_aberto != *aberto
                    {
                        p.menu_aberto = *aberto;
                        cx.notify();
                    }
                });
            })
            // Sem botão à vista: o gatilho não tem tamanho, e o menu abre pelo teclado.
            .trigger(super::trilho::Gatilho(div().size_0().into_any_element()))
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
                    menu_do_painel(menu, janela.clone(), motor.clone(), painel_id, None, window, cx)
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
    /// O que a vista das mensagens de um painel desenha (`vista_mensagens`): a busca na conversa
    /// (Ctrl+F; o índice é refeito só quando o termo, a conversa ou o modo foco mudaram) e as
    /// mensagens, com a barra da busca por cima delas.
    pub(super) fn corpo_do_painel(&mut self, painel_id: u64, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (Some(ix), Some(motor_entidade)) = (self.paineis.iter().position(|p| p.id == painel_id), self.motor.clone()) else {
            return div().into_any_element();
        };
        let motor = motor_entidade.read(cx);
        let Some(chat) = self.paineis[ix].chat.clone().and_then(|id| motor.chat(&id)) else {
            return div().into_any_element();
        };
        let termo = busca_conversa::normalizar(&self.paineis[ix].busca.campo.read(cx).value());
        let foco = self.paineis[ix].modo_foco;
        self.paineis[ix].busca.indice.atualizar(&chat.id, &chat.itens, foco, &termo);
        let barra_busca = self.barra_de_busca(ix, window, cx);
        self.mensagens(motor, chat, ix, barra_busca, window, cx).into_any_element()
    }

    /// Avisa as vistas das mensagens de todos os painéis: o estado que elas desenham mora na
    /// Janela, e a Janela avisada pode ter mudado qualquer parte dele.
    pub(super) fn avisar_vistas(&mut self, cx: &mut Context<Self>) {
        for painel in &self.paineis {
            painel.vista.update(cx, |_, cx| cx.notify());
        }
        self.vista_da_lista.update(cx, |_, cx| cx.notify());
    }

    /// Avisa só a vista das mensagens de um painel (a rolagem dele, que não mexe no resto).
    pub(super) fn avisar_vista(&mut self, painel_id: u64, cx: &mut Context<Self>) {
        if let Some(painel) = self.paineis.iter().find(|p| p.id == painel_id) {
            painel.vista.update(cx, |_, cx| cx.notify());
        }
    }

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
    use super::{JeitoDaAba, LinhaDoHistorico, grupos_que_casam, jeito_das_abas, modelo_da_conversa, teto_por_igual};
    use crate::modelo::Chat;

    #[test]
    fn historico_busca_sem_acento_com_o_titulo_primeiro() {
        let linha = |chave: &str, titulo: &str, grupo: Option<&'static str>| LinhaDoHistorico {
            grupo,
            chave: chave.into(),
            titulo: titulo.into(),
            descricao: String::new(),
            sinal: None,
            termos: format!("~/eita/{chave}"),
        };
        let mut so_no_texto = Chat::novo("b".into(), "Ajustes", std::path::PathBuf::from("/x/eita"));
        so_no_texto.itens.push(crate::modelo::Item::Assistente { texto: "A integração ficou pronta".into(), em: 0 });
        let no_titulo = Chat::novo("a".into(), "Integração do Tiny", std::path::PathBuf::from("/x/eita"));
        let cache = Default::default();
        let preparados = [&so_no_texto, &no_titulo]
            .into_iter()
            .map(|c| (c.id.clone(), crate::ui::lateral::preparado(&cache, c)))
            .collect();
        let linhas = vec![linha("b", "Ajustes", None), linha("a", "Integração do Tiny", None), linha("c", "Outro", Some("Arquivados"))];
        let achados = grupos_que_casam(&linhas, &preparados, "integracao");
        let chaves: Vec<String> = achados.iter().flat_map(|g| g.itens.iter().map(|i| i.chave.to_string())).collect();
        // O que casa no título vem antes do que casa só numa fala, mesmo sendo mais antigo.
        assert_eq!(chaves, ["a", "b"]);
        // Sem busca, tudo, na ordem e nos grupos de antes.
        let todos = grupos_que_casam(&linhas, &preparados, "");
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[1].titulo.as_ref().map(|t| t.to_string()), Some("Arquivados".to_string()));
    }

    /// Com espaço, todas com título; apertando, todas dividem por igual e só vira ícone a de fundo
    /// que nem assim teria o começo do título (a mais estreita primeiro, devolvendo o espaço às
    /// outras); sem lugar para o começo do título da aba à vista, as de fundo saem da faixa, a
    /// mais antiga primeiro; sem medida (o primeiro quadro), nada muda.
    #[test]
    fn as_abas_dividem_por_igual_e_so_viram_icone_sem_lugar() {
        use JeitoDaAba::{Fora, Icone, Titulo};
        // À vista: 200 de natural e 140 de mínimo; vãos de 6; ícone de 36. De fundo: 150 e 110 de
        // natural, 114 e 108 de mínimo.
        let (naturais, minimos) = ([150., 110.], [114., 108.]);
        let jeito = |largura: f32| jeito_das_abas(largura, (200., 140.), &naturais, &minimos, (36., 40., 6.));
        assert_eq!(jeito(500.), [Titulo, Titulo]);
        // 400: as três dividem 388 com teto de 139: a à vista ficaria 1 abaixo dos 140 dela, e a
        // mais estreita de fundo vira ícone; as outras duas cabem inteiras.
        assert_eq!(jeito(400.), [Titulo, Icone]);
        // O teto: quem pede menos que a parte fica com o que pede, e o resto se divide.
        assert_eq!(teto_por_igual(300., &[50., 200., 200.]), 125.);
        assert!(teto_por_igual(500., &[50., 200., 200.]).is_infinite());
        // 340: com teto de 109, a de 150 não chega aos 114 dela e vira ícone; a de 110 fica com
        // título (antes as duas viravam bolinha).
        assert_eq!(jeito(340.), [Icone, Titulo]);
        // 200: com as duas em ícone sobram 116 para a aba à vista, menos que os 140 dela. A mais
        // antiga sai, mas o "+N" (40 e o vão) ocupa mais que ela: sai a outra também, e sobram 154.
        assert_eq!(jeito(200.), [Fora, Fora]);
        assert_eq!(jeito(0.), [Titulo, Titulo]);
        // Com três de fundo, sair uma já basta: 250 - (12 + 72 + 46) = 120, ainda pouco; saindo
        // as duas mais antigas, sobram 250 - (6 + 36 + 46) = 162.
        let (naturais, minimos) = ([150., 110., 120.], [114., 108., 110.]);
        let jeito = |largura: f32| jeito_das_abas(largura, (200., 140.), &naturais, &minimos, (36., 40., 6.));
        assert_eq!(jeito(250.), [Fora, Fora, Icone]);
        // 300: todas em ícone deixam 300 - 126 = 174 para a aba à vista.
        assert_eq!(jeito(300.), [Icone, Icone, Icone]);
    }

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

