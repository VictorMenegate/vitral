//! A atualização na interface (`distribuicao.md`, seção 3.11): o aviso "Versão 0.3.0 pronta" com
//! Reiniciar (uma vez por versão), o ponto azul no logo e o item do menu (`trilho.rs`), o grupo
//! Atualizações das Configurações, o diálogo de Novidades e os avisos da abertura (atualizou,
//! voltou). As frases saem de funções puras do estado, com teste. Nada anda sozinho: a interface só
//! muda quando a `Global` `Atualizacoes` muda.

use gpui_kit::component::{
    Disableable as _, Icon, Size, Sizable as _, WindowExt as _, button::Button, h_flex, notification::Notification,
    spinner::Spinner, text::TextView,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        CabecalhoDialogo, ChaveAviso as _, Confirmacao, GrupoDeOpcoes, LinhaDeOpcao, RodapeDialogo, TamanhoBotao, Tom,
        TomConfirmacao, aviso, botao_do_aviso, botao_fantasma, botao_primario, confirmar, largura,
    },
    configuracoes::{Opcao, menu_escolha},
    dialogos::{moldura, topo},
    fechar::{conversas_trabalhando, reiniciar_pela_janela},
    uso_plano,
};
use crate::{
    atualizacao::{
        self, Atualizacoes, Erro, Estado, Novidade, Permissao, VERSAO,
        config::Modo,
        saude::{Aviso, VoltaPossivel},
    },
    modelo,
    tema::{Cores, Lucide, Tipo as _, esp, r, tam},
};

/// Os avisos da atualização se substituem (o pronto tira o "há versão nova").
const CHAVE: &str = "atualizacao";

/// O que o grupo das Configurações desenha: a `Global`, ou um estado de exemplo na vitrine.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Vista {
    pub versao: String,
    pub estado: Estado,
    pub modo: Modo,
    pub permissao: Permissao,
    pub ultima_procura: Option<u64>,
    pub proxima: Option<u64>,
    pub volta: Option<VoltaPossivel>,
    /// O "agora" das horas ("procurada hoje às 14:02").
    pub agora: u64,
}

impl Vista {
    pub(super) fn da_global(cx: &App) -> Option<Vista> {
        let a = cx.try_global::<Atualizacoes>()?;
        Some(Vista {
            versao: VERSAO.to_string(),
            estado: a.estado.clone(),
            modo: a.modo,
            permissao: a.permissao,
            ultima_procura: a.ultima_procura,
            proxima: a.proxima,
            volta: a.volta.clone(),
            agora: modelo::agora(),
        })
    }

    /// "hoje às 14:02" da última procura e "às 15:10" da próxima, no fuso local.
    fn horas(&self) -> (Option<String>, Option<String>) {
        let hoje = uso_plano::local(self.agora as i64).0;
        let procurada = self.ultima_procura.map(|t| {
            let quando = uso_plano::local(t as i64);
            let texto = uso_plano::quando(quando, hoje);
            if quando.0 == hoje { format!("hoje {texto}") } else { texto }
        });
        let proxima = self.proxima.map(|t| uso_plano::quando(uso_plano::local(t as i64), hoje));
        (procurada, proxima)
    }
}

/// A linha "Versão": de onde veio a cópia (só a que não se atualiza sozinha), o texto (com o
/// spinner quando algo anda) e o erro, se houver, com a cor (vermelho quando grave).
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Linha {
    pub origem: Option<&'static str>,
    pub texto: String,
    pub andando: bool,
    pub erro: Option<(String, bool)>,
}

pub(super) fn linha_da_versao(v: &Vista, procurada: Option<&str>, proxima: Option<&str>) -> Linha {
    let origem = match v.permissao {
        Permissao::Sozinha => None,
        Permissao::CompiladoNoPc => Some("Compilado no PC · sem atualização automática"),
        Permissao::ForaDaPasta => Some("Aberto fora da pasta do app · sem atualização automática"),
    };
    let quando = || match procurada {
        Some(p) => format!("procurada {p}"),
        None if v.modo == Modo::Desligadas => "não procura sozinho".to_string(),
        None => "ainda não procurou".to_string(),
    };
    let (texto, andando) = match &v.estado {
        Estado::Procurando => ("Procurando…".to_string(), true),
        Estado::Baixando { versao, feito, total } => (format!("Baixando {versao} · {}", megas(*feito, *total)), true),
        Estado::Instalando { versao } => (format!("Instalando a {versao}…"), true),
        Estado::EmDia => (format!("{} é a mais nova · {}", v.versao, quando()), false),
        Estado::Pronta(n) => (format!("A {} está pronta; vale quando reiniciar", n.versao), false),
        Estado::Disponivel(n) if n.pela_pagina => (format!("A {} precisa ser instalada pela página", n.versao), false),
        Estado::Disponivel(n) => (format!("Há a {} no GitHub", n.versao), false),
        // A procura que falhou não conta: sem nenhuma que deu certo, fica só a versão.
        Estado::Erro(_) if procurada.is_none() => (v.versao.clone(), false),
        Estado::Parado | Estado::Erro(_) => (format!("{} · {}", v.versao, quando()), false),
    };
    let erro = match &v.estado {
        Estado::Erro(erro) => Some((texto_do_erro(erro, proxima), erro.grave())),
        _ => None,
    };
    Linha { origem, texto, andando, erro }
}

/// O erro numa linha; o de rede diz quando tenta de novo.
pub(super) fn texto_do_erro(erro: &Erro, proxima: Option<&str>) -> String {
    match (erro, proxima) {
        (Erro::Rede(_), Some(quando)) => format!("Sem conexão com o GitHub; tento de novo {quando}"),
        (Erro::Rede(_), None) => "Sem conexão com o GitHub.".to_string(),
        (erro, _) => erro.texto(),
    }
}

/// "12 de 21 MB" (com uma casa abaixo de 10 MB). O feito arredonda para baixo: só chega ao total
/// quando chegou.
pub(super) fn megas(feito: u64, total: u64) -> String {
    const MB: f64 = 1024. * 1024.;
    let casas = total < 10 * 1024 * 1024;
    let em_mb = |bytes: u64, baixo: bool| {
        let valor = bytes as f64 / MB;
        if casas {
            let valor = if baixo { (valor * 10.).floor() / 10. } else { valor };
            format!("{valor:.1}").replace('.', ",")
        } else {
            format!("{}", if baixo { valor.floor() } else { valor.round() })
        }
    };
    format!("{} de {} MB", em_mb(feito, true), em_mb(total, false))
}

/// O que a linha "Versão" oferece à direita.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Botao {
    Procurar { ocupado: bool },
    /// Só avisar: instalar a que foi avisada.
    Instalar,
    /// Compilado no PC (ou fora da pasta): baixar e instalar como app.
    InstalarComoApp(String),
    /// No Windows, a cópia que não é gerenciada (o zip portátil, o build do PC): a página do release,
    /// com o instalador.
    Baixar(String),
    Pagina(String),
    Reiniciar,
    Nenhum,
}

/// O botão e se vai junto o das Novidades.
pub(super) fn botoes(v: &Vista) -> (bool, Botao) {
    match &v.estado {
        Estado::Procurando => (false, Botao::Procurar { ocupado: true }),
        Estado::Baixando { .. } | Estado::Instalando { .. } => (false, Botao::Nenhum),
        Estado::Pronta(_) => (true, Botao::Reiniciar),
        Estado::Disponivel(n) if n.pela_pagina => (false, Botao::Pagina(pagina(n))),
        Estado::Disponivel(_) if v.permissao == Permissao::Sozinha => (true, Botao::Instalar),
        Estado::Disponivel(n) if !atualizacao::INSTALA_COMO_APP => (true, Botao::Baixar(pagina(n))),
        Estado::Disponivel(n) => (true, Botao::InstalarComoApp(n.versao.clone())),
        Estado::Parado | Estado::EmDia | Estado::Erro(_) => (false, Botao::Procurar { ocupado: false }),
    }
}

fn pagina(n: &Novidade) -> String {
    if n.pagina.is_empty() { atualizacao::pagina(&n.versao) } else { n.pagina.clone() }
}

/// As novidades de `n` (a do manifesto, a que veio com a versão ou a do próprio binário) e a página.
fn notas_e_pagina(n: &Novidade, cx: &App) -> (String, String) {
    let notas = match cx.try_global::<Atualizacoes>() {
        Some(a) => a.notas(n),
        None => n.notas.clone(),
    };
    (notas, pagina(n))
}

fn valor_do_modo(modo: Modo) -> &'static str {
    match modo {
        Modo::Automaticas => "automaticas",
        Modo::SoAvisar => "so_avisar",
        Modo::Desligadas => "desligadas",
    }
}

fn modo_do_valor(valor: &str) -> Modo {
    match valor {
        "so_avisar" => Modo::SoAvisar,
        "desligadas" => Modo::Desligadas,
        _ => Modo::Automaticas,
    }
}

/// O grupo Atualizações das Configurações, que vale para o app todo: o modo, a versão (com o que
/// está acontecendo e o botão do momento) e, na versão instalada, a volta para a anterior.
pub(super) fn grupo(v: &Vista, trabalhando: usize, estreito: bool, window: &Window, cx: &App) -> GrupoDeOpcoes {
    let ancora = if estreito { Anchor::TopLeft } else { Anchor::TopRight };
    let com = |linha: LinhaDeOpcao, controle: AnyElement| if estreito { linha.campo(controle) } else { linha.controle(controle) };
    let modos = vec![
        Opcao::new("automaticas", "Automáticas").descricao("Baixa e instala sozinho; vale quando você reiniciar"),
        Opcao::new("so_avisar", "Só avisar").descricao("Avisa da versão nova; você escolhe quando instalar"),
        Opcao::new("desligadas", "Desligadas").descricao("Não procura versões novas"),
    ];
    let modo = menu_escolha(
        "cfg-atualizacoes",
        ancora,
        modos,
        valor_do_modo(v.modo),
        "automaticas",
        |valor, _, cx| atualizacao::escolher_modo(modo_do_valor(&valor), cx),
        window,
        cx,
    );
    let mut grupo = GrupoDeOpcoes::new(Some("Atualizações".into())).linha(com(
        LinhaDeOpcao::new("cfg-atualizacoes", "Atualizações")
            .info(
                "O Vitral procura no GitHub a cada 6 h, confere a assinatura antes de instalar e nunca reinicia \
                 sozinho: a versão nova vale quando você reiniciar.",
            )
            .descricao("Vale para o app todo."),
        modo,
    ));
    let (procurada, proxima) = v.horas();
    let linha = linha_da_versao(v, procurada.as_deref(), proxima.as_deref());
    let descricao = div()
        .flex()
        .flex_col()
        .when_some(linha.origem, |d, origem| d.child(origem))
        .child(
            div()
                .flex()
                .items_center()
                .gap(esp::E6)
                .when(linha.andando, |d| {
                    d.child(Spinner::new().with_size(Size::Size(tam(window, 12.))).color(Cores::tenue()))
                })
                .child(linha.texto),
        )
        .when_some(linha.erro, |d, (texto, grave)| {
            d.child(div().text_color(if grave { Cores::vermelho() } else { Cores::ambar() }).child(texto))
        });
    grupo = grupo.linha(com(LinhaDeOpcao::new("cfg-versao", "Versão").descricao(descricao), controles(v, window, cx)));
    let mexendo = matches!(v.estado, Estado::Baixando { .. } | Estado::Instalando { .. } | Estado::Pronta(_));
    if let Some(volta) = v.volta.as_ref().filter(|_| !mexendo) {
        let rotulo = format!("Voltar para a {}…", volta.para);
        let botao = botao_fantasma("cfg-voltar", Some(Lucide::RotateCcw.into()), rotulo, TamanhoBotao::Barra, window, cx);
        let linha = LinhaDeOpcao::new("cfg-voltar", "Versão anterior");
        grupo = grupo.linha(if volta.le_o_disco {
            let (de, para) = (v.versao.clone(), volta.para.clone());
            com(
                linha.descricao(format!("A {para} continua instalada; voltar não instala a {de} de novo sozinho.")),
                botao
                    .on_click(move |_, window, cx| confirmar_volta(de.clone(), para.clone(), trabalhando, window, cx))
                    .into_any_element(),
            )
        } else {
            com(
                linha.desabilitada(format!("A {} não lê as conversas do formato novo.", volta.para)),
                botao.disabled(true).into_any_element(),
            )
        });
    }
    grupo
}

/// Os botões da linha "Versão".
fn controles(v: &Vista, window: &Window, cx: &App) -> AnyElement {
    let (com_novidades, botao) = botoes(v);
    let novidade = match &v.estado {
        Estado::Pronta(n) | Estado::Disponivel(n) => Some(n.clone()),
        _ => None,
    };
    let novidades = novidade.filter(|_| com_novidades).map(|n| {
        botao_fantasma("cfg-novidades", None, "Novidades", TamanhoBotao::Barra, window, cx).on_click(move |_, window, cx| {
            let (notas, pagina) = notas_e_pagina(&n, cx);
            abrir_novidades(&n.versao, notas, pagina, window, cx)
        })
    });
    let principal: Option<Button> = match botao {
        Botao::Procurar { ocupado } => Some(
            botao_fantasma("cfg-procurar", Some(Lucide::RefreshCw.into()), "Procurar agora", TamanhoBotao::Barra, window, cx)
                .when(ocupado, |b| b.disabled(true))
                .on_click(|_, _, cx| atualizacao::procurar_agora(cx)),
        ),
        Botao::Instalar => Some(
            botao_primario("cfg-instalar", Some(Lucide::Download.into()), "Instalar", TamanhoBotao::Barra, window, cx)
                .on_click(|_, _, cx| atualizacao::instalar_agora(cx)),
        ),
        Botao::InstalarComoApp(versao) => Some(
            botao_primario("cfg-instalar", Some(Lucide::Download.into()), format!("Instalar a {versao} como app"), TamanhoBotao::Barra, window, cx)
                .on_click(|_, _, cx| atualizacao::instalar_agora(cx)),
        ),
        Botao::Baixar(url) => Some(
            botao_primario("cfg-baixar", Some(Lucide::Download.into()), "Baixar", TamanhoBotao::Barra, window, cx)
                .on_click(move |_, _, cx| cx.open_url(&url)),
        ),
        Botao::Pagina(url) => Some(
            botao_fantasma("cfg-pagina", Some(Lucide::ExternalLink.into()), "Abrir a página", TamanhoBotao::Barra, window, cx)
                .on_click(move |_, _, cx| cx.open_url(&url)),
        ),
        Botao::Reiniciar => Some(
            botao_primario("cfg-reiniciar", Some(Lucide::RotateCw.into()), "Reiniciar", TamanhoBotao::Barra, window, cx)
                .on_click(|_, window, cx| reiniciar_pela_janela(window, cx)),
        ),
        Botao::Nenhum => None,
    };
    h_flex().flex_none().gap(esp::E8).children(novidades).children(principal).into_any_element()
}

/// Voltar à mão pede o clique (o Enter só fecha) e conta o que para no meio.
fn confirmar_volta(de: String, para: String, trabalhando: usize, window: &mut Window, cx: &mut App) {
    let mut detalhes: Vec<(Icon, SharedString, bool)> =
        vec![(Lucide::ShieldOff.into(), format!("A {de} não é instalada de novo sozinha").into(), false)];
    if trabalhando > 0 {
        let param = if trabalhando == 1 { "ela para" } else { "elas param" };
        let texto = format!("{}: {param} no meio, com Retomar", conversas_trabalhando(trabalhando));
        detalhes.push((Lucide::TriangleAlert.into(), texto.into(), true));
    }
    confirmar(
        Confirmacao {
            tom: TomConfirmacao::Atencao,
            icone: Lucide::RotateCcw.into(),
            titulo: format!("Voltar para a {para}?").into(),
            texto: format!("O Vitral reinicia na {para}. Os chats e os projetos ficam como estão.").into(),
            detalhes,
            ok: format!("Voltar para a {para}").into(),
            cancelar: "Cancelar".into(),
            ok_perigo: true,
        },
        |window, cx| match atualizacao::voltar_a_mao(cx) {
            Ok(_) => atualizacao::reiniciar(cx),
            Err(erro) => window.push_notification(aviso(Tom::Erro, erro).chave(CHAVE), cx),
        },
        None,
        window,
        cx,
    );
}

/// Novidades de uma versão: o Markdown do NOVIDADES e o link do release.
pub(super) fn abrir_novidades(versao: &str, notas: String, pagina: String, window: &mut Window, cx: &mut App) {
    let titulo = format!("Novidades da {versao}");
    let id = SharedString::from(format!("novidades-{versao}"));
    window.open_dialog(cx, move |dialogo, window, cx| {
        let corpo = if notas.trim().is_empty() {
            div()
                .corpo_ui()
                .text_color(Cores::tenue())
                .child("Esta versão não trouxe notas; a página do release tem o que mudou.")
                .into_any_element()
        } else {
            div()
                .id("novidades-texto")
                .max_h(r(440.))
                .overflow_y_scroll()
                .corpo_ui()
                .text_color(Cores::texto())
                .child(TextView::markdown(id.clone(), notas.clone()))
                .into_any_element()
        };
        let url = pagina.clone();
        moldura(dialogo, largura::M, window)
            .title(topo(CabecalhoDialogo::new(titulo.clone())))
            .child(corpo)
            .footer(
                RodapeDialogo::new()
                    .esquerda(
                        botao_fantasma("novidades-github", Some(Lucide::ExternalLink.into()), "Ver no GitHub", TamanhoBotao::Controle, window, cx)
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    )
                    .principal(
                        botao_primario("novidades-fechar", None, "Fechar", TamanhoBotao::Controle, window, cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    });
}

fn icone_do_aviso() -> Icon {
    Icon::new(Lucide::CircleArrowUp).text_color(Cores::destaque_texto())
}

/// A segunda linha dos avisos de versão nova: o que vale e o link das Novidades.
fn linha_com_novidades(texto: &'static str, versao: SharedString, notas: SharedString, pagina: SharedString) -> AnyElement {
    h_flex()
        .gap(esp::E4)
        .text_size(r(13.))
        .line_height(r(18.))
        .text_color(Cores::tenue())
        .child(texto)
        .child("·")
        .child(
            div()
                .id("aviso-novidades")
                .cursor_pointer()
                .text_color(Cores::destaque_texto())
                .hover(|s| s.underline())
                .child("Novidades")
                .on_click(move |_, window, cx| abrir_novidades(&versao, notas.to_string(), pagina.to_string(), window, cx)),
        )
        .into_any_element()
}

/// "Versão 0.3.0 pronta", com Reiniciar e as Novidades: fica até o Reiniciar ou o ×. Fechar não
/// perde nada (o ponto no logo e o item do menu continuam).
pub(super) fn aviso_pronta(versao: &str, notas: String, pagina: String) -> Notification {
    let (v, n, p) = (SharedString::from(versao.to_string()), SharedString::from(notas), SharedString::from(pagina));
    Notification::new()
        .icon(icone_do_aviso())
        .title(format!("Versão {versao} pronta"))
        .content(move |_, _, _| linha_com_novidades("Vale quando reiniciar", v.clone(), n.clone(), p.clone()))
        .action(|_, _, cx| {
            botao_do_aviso("Reiniciar", cx).on_click(cx.listener(|aviso, _, window, cx| {
                aviso.dismiss(window, cx);
                reiniciar_pela_janela(window, cx);
            }))
        })
        .chave(CHAVE)
}

/// Só avisar: "Há a 0.3.0 no GitHub", com Instalar e as Novidades.
pub(super) fn aviso_disponivel(versao: &str, notas: String, pagina: String) -> Notification {
    let (v, n, p) = (SharedString::from(versao.to_string()), SharedString::from(notas), SharedString::from(pagina));
    Notification::new()
        .icon(icone_do_aviso())
        .title(format!("Há a {versao} no GitHub"))
        .content(move |_, _, _| linha_com_novidades("Você escolhe quando instalar", v.clone(), n.clone(), p.clone()))
        .action(|_, _, cx| {
            botao_do_aviso("Instalar", cx).on_click(cx.listener(|aviso, _, window, cx| {
                aviso.dismiss(window, cx);
                atualizacao::instalar_agora(cx);
            }))
        })
        .chave(CHAVE)
}

/// O que a saúde contou ao abrir, numa linha.
pub(super) fn texto_do_aviso(aviso: &Aviso) -> String {
    match aviso {
        Aviso::Atualizada { versao } => format!("Atualizado para a {versao}"),
        Aviso::Voltou { de, para, automatica: true } => format!("A {de} não abriu duas vezes; voltei para a {para}"),
        Aviso::Voltou { de, para, automatica: false } => format!("De volta à {para}; a {de} não é instalada de novo sozinha"),
    }
}

/// O aviso da abertura: "Atualizado para a 0.3.0 · Ver novidades", ou a volta (a automática fica
/// até fechar).
pub(super) fn aviso_ao_abrir(aviso_ao_abrir: &Aviso, cx: &App) -> Notification {
    let texto = texto_do_aviso(aviso_ao_abrir);
    match aviso_ao_abrir {
        Aviso::Atualizada { versao } => {
            let novidade = Novidade { versao: versao.clone(), notas: String::new(), pagina: String::new(), pela_pagina: false };
            let (notas, pagina) = notas_e_pagina(&novidade, cx);
            let versao = versao.clone();
            Notification::new()
                .icon(icone_do_aviso())
                .message(texto)
                .action(move |_, _, cx| {
                    let (versao, notas, pagina) = (versao.clone(), notas.clone(), pagina.clone());
                    botao_do_aviso("Ver novidades", cx).on_click(cx.listener(move |aviso, _, window, cx| {
                        aviso.dismiss(window, cx);
                        abrir_novidades(&versao, notas.clone(), pagina.clone(), window, cx);
                    }))
                })
                .autohide(true)
                .chave(CHAVE)
        }
        Aviso::Voltou { automatica: true, .. } => aviso(Tom::Atencao, texto).autohide(false).chave(CHAVE),
        Aviso::Voltou { .. } => aviso(Tom::Info, texto).chave(CHAVE),
    }
}

impl Janela {
    /// No primeiro quadro: o que a saúde contou ao abrir (uma vez) e a versão que outra abertura já
    /// deixou pronta.
    pub(super) fn atualizacao_ao_abrir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let contado = cx.try_global::<Atualizacoes>().and_then(|a| a.aviso_ao_abrir.clone());
        if let Some(contado) = contado {
            cx.global_mut::<Atualizacoes>().aviso_ao_abrir = None;
            let nota = aviso_ao_abrir(&contado, cx);
            window.push_notification(nota, cx);
        }
        self.atualizacao_mudou(window, cx);
    }

    /// A `Global` mudou: redesenha só se a versão pronta mudou, e avisa uma vez por versão (a
    /// pronta e, no Só avisar, a que saiu).
    pub(super) fn atualizacao_mudou(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(a) = cx.try_global::<Atualizacoes>() else { return };
        let pronta = a.pronta().map(|n| n.versao.clone());
        let avisar = match &a.estado {
            Estado::Pronta(n) => Some((true, n.clone())),
            Estado::Disponivel(n) if a.modo == Modo::SoAvisar && a.permissao == Permissao::Sozinha && !n.pela_pagina => {
                Some((false, n.clone()))
            }
            _ => None,
        };
        if pronta != self.atualizacao_pronta {
            self.atualizacao_pronta = pronta;
            cx.notify();
        }
        let Some((esta_pronta, novidade)) = avisar else { return };
        let chave = format!("{} {}", if esta_pronta { "pronta" } else { "disponivel" }, novidade.versao);
        if self.atualizacao_avisada.as_deref() == Some(chave.as_str()) {
            return;
        }
        self.atualizacao_avisada = Some(chave);
        let (notas, pagina) = notas_e_pagina(&novidade, cx);
        let nota = if esta_pronta {
            aviso_pronta(&novidade.versao, notas, pagina)
        } else {
            aviso_disponivel(&novidade.versao, notas, pagina)
        };
        window.push_notification(nota, cx);
    }
}

/// Estados de exemplo para a vitrine e a bancada: os da procura, do download, os erros e o build do PC.
pub(super) fn vistas_de_exemplo(agora: u64) -> Vec<(&'static str, Vista)> {
    let base = Vista {
        versao: "0.2.0".into(),
        estado: Estado::Parado,
        modo: Modo::Automaticas,
        permissao: Permissao::Sozinha,
        ultima_procura: Some(agora.saturating_sub(40 * 60)),
        proxima: Some(agora + 50 * 60),
        volta: None,
        agora,
    };
    let nova = Novidade {
        versao: "0.3.0".into(),
        notas: "- **Conversas que se lembram.** O Mestre guarda o que importa entre as conversas.\n\
                - **Mais rápido ao abrir.** A lista aparece antes do motor terminar de ler os chats."
            .into(),
        pagina: atualizacao::pagina("0.3.0"),
        pela_pagina: false,
    };
    let com = |estado: Estado| Vista { estado, ..base.clone() };
    vec![
        ("Parado, com a anterior para voltar", Vista { volta: Some(VoltaPossivel { para: "0.1.0".into(), le_o_disco: true }), ..base.clone() }),
        ("Procurando", com(Estado::Procurando)),
        ("Em dia", com(Estado::EmDia)),
        ("Baixando", com(Estado::Baixando { versao: "0.3.0".into(), feito: 12 * 1024 * 1024 + 300_000, total: 21 * 1024 * 1024 })),
        ("Pronta", com(Estado::Pronta(nova.clone()))),
        ("Só avisar", Vista { modo: Modo::SoAvisar, ..com(Estado::Disponivel(nova.clone())) }),
        ("Sem conexão", com(Estado::Erro(Erro::Rede("recusada".into())))),
        ("Assinatura recusada", com(Estado::Erro(Erro::Assinatura))),
        ("Compilado no PC", Vista { permissao: Permissao::CompiladoNoPc, ..com(Estado::Disponivel(nova.clone())) }),
        (
            "Formato novo na pasta",
            Vista { volta: Some(VoltaPossivel { para: "0.1.0".into(), le_o_disco: false }), ..base.clone() },
        ),
    ]
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: o `gpui_kit::*` traz um `test` próprio que encobriria o da biblioteca.
    use super::{
        Aviso, Botao, Erro, Estado, Modo, Novidade, Permissao, Vista, botoes, linha_da_versao, megas, modo_do_valor,
        texto_do_aviso, texto_do_erro, valor_do_modo, vistas_de_exemplo,
    };

    fn vista(estado: Estado) -> Vista {
        Vista {
            versao: "0.2.0".into(),
            estado,
            modo: Modo::Automaticas,
            permissao: Permissao::Sozinha,
            ultima_procura: None,
            proxima: None,
            volta: None,
            agora: 0,
        }
    }

    fn nova(versao: &str) -> Novidade {
        Novidade { versao: versao.into(), notas: String::new(), pagina: String::new(), pela_pagina: false }
    }

    fn texto(v: &Vista) -> String {
        linha_da_versao(v, Some("hoje às 14:02"), Some("às 15:10")).texto
    }

    #[test]
    fn cada_estado_diz_o_que_esta_acontecendo() {
        let mb = 1024 * 1024;
        assert_eq!(texto(&vista(Estado::Parado)), "0.2.0 · procurada hoje às 14:02");
        assert_eq!(linha_da_versao(&vista(Estado::Parado), None, None).texto, "0.2.0 · ainda não procurou");
        let desligadas = Vista { modo: Modo::Desligadas, ..vista(Estado::Parado) };
        assert_eq!(linha_da_versao(&desligadas, None, None).texto, "0.2.0 · não procura sozinho");
        assert_eq!(texto(&vista(Estado::EmDia)), "0.2.0 é a mais nova · procurada hoje às 14:02");
        let procurando = linha_da_versao(&vista(Estado::Procurando), None, None);
        assert_eq!((procurando.texto.as_str(), procurando.andando), ("Procurando…", true));
        let baixando = Estado::Baixando { versao: "0.3.0".into(), feito: 12 * mb + 900_000, total: 21 * mb };
        assert_eq!(texto(&vista(baixando)), "Baixando 0.3.0 · 12 de 21 MB");
        assert_eq!(texto(&vista(Estado::Instalando { versao: "0.3.0".into() })), "Instalando a 0.3.0…");
        assert_eq!(texto(&vista(Estado::Pronta(nova("0.3.0")))), "A 0.3.0 está pronta; vale quando reiniciar");
        assert_eq!(texto(&vista(Estado::Disponivel(nova("0.3.0")))), "Há a 0.3.0 no GitHub");
        let pagina = Novidade { pela_pagina: true, ..nova("0.4.0") };
        assert_eq!(texto(&vista(Estado::Disponivel(pagina))), "A 0.4.0 precisa ser instalada pela página");
        for estado in [Estado::Parado, Estado::EmDia, Estado::Pronta(nova("0.3.0"))] {
            let linha = linha_da_versao(&vista(estado), None, None);
            assert!(!linha.andando && linha.erro.is_none() && linha.origem.is_none(), "{linha:?}");
        }
    }

    #[test]
    fn erros_numa_linha_ambar_para_rede_vermelho_para_assinatura() {
        let rede = linha_da_versao(&vista(Estado::Erro(Erro::Rede("x".into()))), Some("hoje às 14:02"), Some("às 15:10"));
        assert_eq!(rede.texto, "0.2.0 · procurada hoje às 14:02");
        assert_eq!(rede.erro, Some(("Sem conexão com o GitHub; tento de novo às 15:10".into(), false)));
        assert_eq!(texto_do_erro(&Erro::Rede("x".into()), None), "Sem conexão com o GitHub.");
        let nunca = linha_da_versao(&vista(Estado::Erro(Erro::Rede("x".into()))), None, None);
        assert_eq!(nunca.texto, "0.2.0", "a procura que falhou não vira \"ainda não procurou\"");
        let assinatura = linha_da_versao(&vista(Estado::Erro(Erro::Assinatura)), None, None);
        assert_eq!(
            assinatura.erro,
            Some(("A atualização foi recusada: a assinatura não confere. Nada foi instalado.".into(), true))
        );
        let soma = linha_da_versao(&vista(Estado::Erro(Erro::Arquivo { versao: "0.3.0".into() })), None, None);
        assert!(soma.erro.is_some_and(|(texto, grave)| grave && texto.contains("diferente do publicado")));
    }

    #[test]
    fn build_do_pc_so_instala_como_app() {
        let pc = Vista { permissao: Permissao::CompiladoNoPc, ..vista(Estado::Disponivel(nova("0.3.0"))) };
        let linha = linha_da_versao(&pc, None, None);
        assert_eq!(linha.origem, Some("Compilado no PC · sem atualização automática"));
        assert_eq!(linha.texto, "Há a 0.3.0 no GitHub");
        let esperado = if crate::atualizacao::INSTALA_COMO_APP {
            Botao::InstalarComoApp("0.3.0".into())
        } else {
            Botao::Baixar("https://github.com/VictorMenegate/vitral/releases/tag/v0.3.0".into())
        };
        assert_eq!(botoes(&pc), (true, esperado));
        let fora = Vista { permissao: Permissao::ForaDaPasta, ..vista(Estado::EmDia) };
        assert!(linha_da_versao(&fora, None, None).origem.is_some_and(|o| o.starts_with("Aberto fora da pasta")));
    }

    #[test]
    fn o_botao_de_cada_estado() {
        assert_eq!(botoes(&vista(Estado::Parado)), (false, Botao::Procurar { ocupado: false }));
        assert_eq!(botoes(&vista(Estado::Procurando)), (false, Botao::Procurar { ocupado: true }));
        assert_eq!(botoes(&vista(Estado::Erro(Erro::Assinatura))), (false, Botao::Procurar { ocupado: false }));
        assert_eq!(botoes(&vista(Estado::Baixando { versao: "0.3.0".into(), feito: 0, total: 1 })), (false, Botao::Nenhum));
        assert_eq!(botoes(&vista(Estado::Pronta(nova("0.3.0")))), (true, Botao::Reiniciar));
        assert_eq!(botoes(&vista(Estado::Disponivel(nova("0.3.0")))), (true, Botao::Instalar));
        let pagina = Novidade { pela_pagina: true, ..nova("0.4.0") };
        assert_eq!(
            botoes(&vista(Estado::Disponivel(pagina))),
            (false, Botao::Pagina("https://github.com/VictorMenegate/vitral/releases/tag/v0.4.0".into()))
        );
    }

    #[test]
    fn megas_sem_chegar_ao_total_antes_da_hora() {
        let mb = 1024 * 1024;
        assert_eq!(megas(0, 21 * mb), "0 de 21 MB");
        assert_eq!(megas(21 * mb - 1, 21 * mb), "20 de 21 MB");
        assert_eq!(megas(21 * mb, 21 * mb), "21 de 21 MB");
        assert_eq!(megas(mb + mb / 2, 3 * mb + 400_000), "1,5 de 3,4 MB");
        assert_eq!(megas(328, 328), "0,0 de 0,0 MB");
    }

    #[test]
    fn os_avisos_da_abertura() {
        assert_eq!(texto_do_aviso(&Aviso::Atualizada { versao: "0.3.0".into() }), "Atualizado para a 0.3.0");
        let automatica = Aviso::Voltou { de: "0.3.0".into(), para: "0.2.0".into(), automatica: true };
        assert_eq!(texto_do_aviso(&automatica), "A 0.3.0 não abriu duas vezes; voltei para a 0.2.0");
        let mao = Aviso::Voltou { de: "0.3.0".into(), para: "0.2.0".into(), automatica: false };
        assert_eq!(texto_do_aviso(&mao), "De volta à 0.2.0; a 0.3.0 não é instalada de novo sozinha");
    }

    #[test]
    fn modo_ida_e_volta() {
        for modo in [Modo::Automaticas, Modo::SoAvisar, Modo::Desligadas] {
            assert_eq!(modo_do_valor(valor_do_modo(modo)), modo);
            let gravado = serde_json::to_value(modo).unwrap();
            assert_eq!(gravado, valor_do_modo(modo), "o mesmo nome do atualizacoes.json");
        }
    }

    #[test]
    fn a_vitrine_mostra_os_estados_do_lote() {
        let vistas = vistas_de_exemplo(1_800_000_000);
        let tem = |f: &dyn Fn(&Vista) -> bool| vistas.iter().any(|(_, v)| f(v));
        assert!(tem(&|v| v.estado == Estado::Procurando));
        assert!(tem(&|v| matches!(v.estado, Estado::Baixando { .. })));
        assert!(tem(&|v| matches!(v.estado, Estado::Pronta(_))));
        assert!(tem(&|v| matches!(v.estado, Estado::Erro(Erro::Rede(_)))));
        assert!(tem(&|v| v.estado == Estado::Erro(Erro::Assinatura)));
        assert!(tem(&|v| v.permissao == Permissao::CompiladoNoPc));
    }
}
