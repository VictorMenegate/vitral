//! "Servidores MCP": o `/mcp` do terminal dentro do Vitral (dono, 13/11/2026: "coloca os mcp"; no
//! `-p` o Claude Code só responde "use /mcp in the terminal"). Mostra os servidores que uma
//! conversa do chat teria, como estão e as ferramentas de cada um, e liga de novo o que caiu.
//!
//! Enquanto o diálogo está aberto há um Claude Code só dele (`claude::SessaoMcp`: sem mensagem,
//! sem hooks, não gasta do plano), numa thread própria que atende os pedidos da tela: reler a lista
//! e reconectar um servidor. Fechou o diálogo, a thread acaba e o Claude Code sai com os servidores
//! que abriu. O reconectar vale para essa sessão de consulta (serve para ver se o servidor volta e
//! qual é o erro): cada turno de um chat abre o Claude Code de novo e liga os servidores sozinho.

use std::{collections::HashSet, path::PathBuf, sync::mpsc};

use gpui_kit::component::{Disableable as _, StyledExt as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::componentes::{
    CabecalhoDialogo, RodapeDialogo, Selo, TamanhoBotao, botao_fantasma, botao_icone, botao_primario, icone, largura,
};
use crate::claude::{ServidorMcp, SessaoMcp};
use crate::tema::{Cores, FONTE_MONO, Lucide, Tipo as _, esp, r, raio};

/// O que a tela pede à thread da sessão.
enum Pedido {
    Atualizar,
    Reconectar(String),
}

/// O que a thread devolve: a lista de agora (`None` se o Claude Code não respondeu) e, depois de
/// um reconectar, o servidor que ele não conseguiu ligar.
struct Retorno {
    servidores: Option<Vec<ServidorMcp>>,
    recusado: Option<String>,
}

/// O estado do diálogo.
#[derive(Default)]
struct Consulta {
    pedidos: Option<mpsc::Sender<Pedido>>,
    /// `None` na primeira pergunta; `Some(None)` sem resposta.
    resultado: Option<Option<Vec<ServidorMcp>>>,
    /// O que está em andamento: o nome do servidor reconectando, ou vazio para a lista inteira.
    ocupado: Option<String>,
    /// Os servidores com as ferramentas à vista.
    abertos: HashSet<String>,
    /// Os que o último reconectar não conseguiu ligar.
    recusados: HashSet<String>,
}

impl Consulta {
    fn pedir(&mut self, pedido: Pedido) {
        let nome = match &pedido {
            Pedido::Atualizar => String::new(),
            Pedido::Reconectar(nome) => nome.clone(),
        };
        if self.ocupado.is_none() && self.pedidos.as_ref().is_some_and(|p| p.send(pedido).is_ok()) {
            self.recusados.remove(&nome);
            self.ocupado = Some(nome);
        }
    }
}

/// O estado de um servidor em português, com a cor do ponto.
fn estado_do_servidor(estado: &str) -> (&'static str, Hsla) {
    match estado {
        "connected" => ("ligado", Cores::verde()),
        "pending" => ("ligando", Cores::destaque()),
        "needs-auth" => ("precisa entrar", Cores::ambar()),
        "disabled" => ("desligado", Cores::apagado()),
        _ => ("falhou", Cores::vermelho()),
    }
}

/// De onde o servidor vem, para o grupo dele na lista.
fn origem_do_servidor(escopo: &str) -> &'static str {
    match escopo {
        "claudeai" => "Conectores da conta",
        "project" | "local" => "Desta pasta",
        "user" => "Do seu Claude Code",
        // O que o Vitral entrega ao chat pelo `--mcp-config`: a memória da equipe.
        "dynamic" => "Do Vitral",
        _ => "Outros",
    }
}

/// "8 ferramentas", "1 ferramenta", "sem ferramentas".
fn contagem_de_ferramentas(n: usize) -> String {
    match n {
        0 => "sem ferramentas".to_string(),
        1 => "1 ferramenta".to_string(),
        n => format!("{n} ferramentas"),
    }
}

/// Os servidores por origem, na ordem em que os grupos aparecem.
fn por_origem(servidores: &[ServidorMcp]) -> Vec<(&'static str, Vec<&ServidorMcp>)> {
    let mut grupos: Vec<(&'static str, Vec<&ServidorMcp>)> = Vec::new();
    for ordem in ["Do Vitral", "Do seu Claude Code", "Desta pasta", "Conectores da conta", "Outros"] {
        let membros: Vec<&ServidorMcp> = servidores.iter().filter(|s| origem_do_servidor(&s.escopo) == ordem).collect();
        if !membros.is_empty() {
            grupos.push((ordem, membros));
        }
    }
    grupos
}

/// A linha de baixo de um servidor: o erro, o que ele oferece ou o que falta.
fn detalhe_do_servidor(servidor: &ServidorMcp, reconectando: bool, recusado: bool) -> String {
    if reconectando {
        return "Reconectando…".to_string();
    }
    let ferramentas = contagem_de_ferramentas(servidor.ferramentas.len());
    match (&servidor.erro, servidor.estado.as_str()) {
        (Some(erro), _) => erro.lines().next().unwrap_or_default().to_string(),
        (None, "connected") => match &servidor.versao {
            Some(versao) => format!("{ferramentas} · versão {versao}"),
            None => ferramentas,
        },
        (None, "needs-auth") => "Entre nele pelo /mcp do Claude Code no terminal".to_string(),
        (None, "pending") => "Ainda ligando".to_string(),
        (None, "disabled") => "Desligado no Claude Code".to_string(),
        (None, _) if recusado => "Não ligou de novo".to_string(),
        _ => "Não ligou".to_string(),
    }
}

/// O cartão de um servidor: o ponto do estado, o nome, o detalhe, o estado e o reconectar. Com
/// ferramentas, o clique abre a lista delas.
fn cartao(servidor: &ServidorMcp, consulta: &Entity<Consulta>, window: &Window, cx: &App) -> AnyElement {
    let c = consulta.read(cx);
    let (estado, cor) = estado_do_servidor(&servidor.estado);
    let nome = servidor.nome.strip_prefix("claude.ai ").unwrap_or(&servidor.nome).to_string();
    let reconectando = c.ocupado.as_deref() == Some(servidor.nome.as_str());
    let ocupado = c.ocupado.is_some();
    let aberto = c.abertos.contains(&servidor.nome) && !servidor.ferramentas.is_empty();
    let detalhe = detalhe_do_servidor(servidor, reconectando, c.recusados.contains(&servidor.nome));
    let com_erro = !matches!(servidor.estado.as_str(), "connected" | "pending" | "disabled" | "needs-auth");
    let tem_ferramentas = !servidor.ferramentas.is_empty();
    // Os conectores da conta são ligados pelo servidor da Anthropic: reconectar daqui não os refaz.
    let reconecta = servidor.escopo != "claudeai" && servidor.estado != "disabled";
    let (alternar, reconectar) = (consulta.clone(), consulta.clone());
    let (chave, alvo) = (servidor.nome.clone(), servidor.nome.clone());
    let cabeca = h_flex()
        .id(SharedString::from(format!("mcp-{}", servidor.nome)))
        .items_center()
        .gap(esp::E12)
        .min_h(r(52.))
        .px(esp::E12)
        .when(tem_ferramentas, |d| {
            d.cursor_pointer().on_click(move |_, _, cx| {
                alternar.update(cx, |c, cx| {
                    if !c.abertos.remove(&chave) {
                        c.abertos.insert(chave.clone());
                    }
                    cx.notify();
                });
                cx.refresh_windows();
            })
        })
        .child(div().flex_none().size(r(8.)).rounded_full().bg(if reconectando { Cores::destaque() } else { cor }))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().truncate().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child(nome))
                .child(
                    div()
                        .truncate()
                        .metadado()
                        .text_color(if com_erro && !reconectando { Cores::vermelho() } else { Cores::tenue() })
                        .child(detalhe),
                ),
        )
        .child(Selo::neutro(if reconectando { "reconectando" } else { estado }))
        .when(reconecta, |d| {
            d.child(
                botao_icone(
                    SharedString::from(format!("mcp-reconectar-{}", servidor.nome)),
                    Lucide::RefreshCw,
                    "Reconectar\nLiga o servidor de novo e mostra se ele voltou",
                    TamanhoBotao::Linha,
                    window,
                    cx,
                )
                .disabled(ocupado)
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    reconectar.update(cx, |c, cx| {
                        c.pedir(Pedido::Reconectar(alvo.clone()));
                        cx.notify();
                    });
                    cx.refresh_windows();
                }),
            )
        })
        .when(tem_ferramentas, |d| {
            let seta = if aberto { Lucide::ChevronDown } else { Lucide::ChevronRight };
            d.child(div().flex_none().text_color(Cores::apagado()).child(icone(seta, 14., window)))
        });
    v_flex()
        .w_full()
        .mb(esp::E6)
        .rounded(raio::CARTAO)
        .border_1()
        .border_color(if com_erro { Cores::vermelho().opacity(0.35) } else { Cores::borda() })
        .bg(Cores::cartao_elevado())
        .overflow_hidden()
        .child(cabeca)
        .when(aberto, |d| {
            d.child(
                h_flex()
                    .flex_wrap()
                    .gap(esp::E4)
                    .px(esp::E12)
                    .pb(esp::E12)
                    .children(servidor.ferramentas.iter().map(|ferramenta| {
                        div()
                            .px(esp::E6)
                            .rounded(raio::CONTROLE)
                            .bg(Cores::hover())
                            .font_family(FONTE_MONO)
                            .metadado()
                            .text_color(Cores::texto())
                            .child(ferramenta.clone())
                    })),
            )
        })
        .into_any_element()
}

/// Abre o diálogo e a sessão de consulta de uma conversa em `pasta`, na conta `pasta_conta`
/// (`None` é o login normal).
pub fn abrir(pasta: PathBuf, pasta_conta: Option<PathBuf>, mcp_da_equipe: Option<PathBuf>, window: &mut Window, cx: &mut App) {
    let (pedidos, pedidos_rx) = mpsc::channel::<Pedido>();
    let (retornos, retornos_rx) = smol::channel::unbounded::<Retorno>();
    // A thread da sessão: abre o Claude Code, manda a primeira lista e atende até a tela fechar
    // (o `Sender` dos pedidos cai junto com o diálogo, e o `recv` devolve erro).
    std::thread::spawn(move || {
        let Some(mut sessao) = SessaoMcp::abrir(&pasta, pasta_conta.as_deref(), mcp_da_equipe.as_deref()) else {
            let _ = retornos.send_blocking(Retorno { servidores: None, recusado: None });
            return;
        };
        if retornos.send_blocking(Retorno { servidores: sessao.servidores(), recusado: None }).is_err() {
            return;
        }
        while let Ok(pedido) = pedidos_rx.recv() {
            let recusado = match pedido {
                Pedido::Atualizar => None,
                Pedido::Reconectar(nome) => (!sessao.reconectar(&nome)).then_some(nome),
            };
            if retornos.send_blocking(Retorno { servidores: sessao.servidores(), recusado }).is_err() {
                return;
            }
        }
    });
    let consulta = cx.new(|_| Consulta { pedidos: Some(pedidos), ocupado: Some(String::new()), ..Consulta::default() });
    cx.spawn({
        let consulta = consulta.downgrade();
        async move |cx| {
            while let Ok(retorno) = retornos_rx.recv().await {
                let vivo = cx.update(|cx| {
                    let vivo = consulta
                        .update(cx, |c, _| {
                            // Um reconectar que falhou não apaga a lista que já estava na tela.
                            if retorno.servidores.is_some() || c.resultado.is_none() {
                                c.resultado = Some(retorno.servidores);
                            }
                            c.recusados.extend(retorno.recusado);
                            c.ocupado = None;
                        })
                        .is_ok();
                    cx.refresh_windows();
                    vivo
                });
                if !vivo {
                    break;
                }
            }
        }
    })
    .detach();
    window.open_dialog(cx, move |dialogo, window, cx| {
        let (resultado, ocupado) = {
            let c = consulta.read(cx);
            (c.resultado.clone(), c.ocupado.is_some())
        };
        let aviso = |texto: &str| div().py(esp::E24).corpo_ui().text_color(Cores::tenue()).child(texto.to_string()).into_any_element();
        let (subtitulo, corpo) = match &resultado {
            None => ("Perguntando ao Claude Code…".to_string(), aviso("Os servidores levam alguns segundos para ligar.")),
            Some(None) => ("Sem resposta".to_string(), aviso("O Claude Code não respondeu. Feche e abra de novo para tentar outra vez.")),
            Some(Some(lista)) if lista.is_empty() => {
                ("Nenhum servidor".to_string(), aviso("Não há servidor MCP configurado no seu Claude Code para esta pasta."))
            }
            Some(Some(lista)) => {
                let ligados = lista.iter().filter(|s| s.estado == "connected").count();
                let ferramentas: usize = lista.iter().map(|s| s.ferramentas.len()).sum();
                let grupos = por_origem(lista).into_iter().map(|(titulo, membros)| {
                    let quantos = membros.len();
                    v_flex()
                        .w_full()
                        .pb(esp::E8)
                        .child(
                            h_flex()
                                .h(r(28.))
                                .items_center()
                                .gap(esp::E6)
                                .text_size(r(12.))
                                .font_semibold()
                                .text_color(Cores::tenue())
                                .child(titulo)
                                .child(div().font_weight(FontWeight::NORMAL).text_color(Cores::apagado()).child(quantos.to_string())),
                        )
                        .children(membros.into_iter().map(|s| cartao(s, &consulta, window, cx)))
                });
                let corpo = div()
                    .id("mcp-lista")
                    .max_h(window.viewport_size().height * 0.6)
                    .overflow_y_scroll()
                    .child(v_flex().w_full().children(grupos))
                    .into_any_element();
                (format!("{ligados} de {} ligados · {}", lista.len(), contagem_de_ferramentas(ferramentas)), corpo)
            }
        };
        let atualizar = consulta.clone();
        super::dialogos::moldura(dialogo, largura::M, window)
            .title(super::dialogos::topo(
                CabecalhoDialogo::new("Servidores MCP").subtitulo(div().font_weight(FontWeight::NORMAL).child(subtitulo)),
            ))
            .child(corpo)
            .footer(
                RodapeDialogo::new()
                    .esquerda(
                        botao_fantasma("mcp-atualizar", Some(Lucide::RefreshCw.into()), "Atualizar", TamanhoBotao::Controle, window, cx)
                            .disabled(ocupado)
                            .on_click(move |_, _, cx| {
                                atualizar.update(cx, |c, cx| {
                                    c.pedir(Pedido::Atualizar);
                                    cx.notify();
                                });
                                cx.refresh_windows();
                            }),
                    )
                    .principal(
                        botao_primario("mcp-fechar", None, "Fechar", TamanhoBotao::Controle, window, cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    });
}

impl super::Janela {
    /// O `/mcp` da caixa e o "Servidores MCP" do menu do logo: os servidores do chat do painel
    /// ativo (a pasta e a conta dele); sem chat aberto, os da pasta pessoal na conta principal.
    pub(super) fn abrir_mcp(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let do_chat = self.motor.as_ref().and_then(|motor| {
            let m = motor.read(cx);
            let chat = m.chat(self.selecionado.as_deref()?)?;
            Some((chat.cwd.clone(), m.conta_para(chat).pasta()))
        });
        let (pasta, conta) = do_chat.filter(|(pasta, _)| pasta.is_dir()).unwrap_or_else(|| (dirs::home_dir().unwrap_or_default(), None));
        let mcp_da_equipe = self.motor.as_ref().and_then(|motor| {
            let m = motor.read(cx);
            crate::memoria_equipe::arquivo_do_mcp(&m.dados.cache, &m.dados.config, &m.config.memoria_da_equipe)
        });
        abrir(pasta, conta, mcp_da_equipe, window, cx);
    }
}

#[cfg(test)]
mod testes {
    use super::{contagem_de_ferramentas, detalhe_do_servidor, origem_do_servidor, por_origem};
    use crate::claude::ServidorMcp;

    fn servidor(nome: &str, escopo: &str) -> ServidorMcp {
        ServidorMcp { nome: nome.into(), estado: "connected".into(), versao: None, escopo: escopo.into(), ferramentas: Vec::new(), erro: None }
    }

    #[test]
    fn servidores_agrupados_pela_origem() {
        let lista = [servidor("claude.ai Gmail", "claudeai"), servidor("central", "user"), servidor("loja", "project"), servidor("x", "dynamic")];
        let grupos = por_origem(&lista);
        let nomes: Vec<(&str, Vec<&str>)> = grupos.iter().map(|(t, m)| (*t, m.iter().map(|s| s.nome.as_str()).collect())).collect();
        assert_eq!(
            nomes,
            [("Do Vitral", vec!["x"]), ("Do seu Claude Code", vec!["central"]), ("Desta pasta", vec!["loja"]), ("Conectores da conta", vec!["claude.ai Gmail"])]
        );
        assert_eq!(origem_do_servidor("local"), "Desta pasta");
        // A linha de baixo: o erro vence, o reconectar em andamento vence o erro.
        let mut caido = servidor("ai-memory", "user");
        (caido.estado, caido.erro) = ("failed".into(), Some("Connection closed\nresto".into()));
        assert_eq!(detalhe_do_servidor(&caido, false, false), "Connection closed");
        assert_eq!(detalhe_do_servidor(&caido, true, false), "Reconectando…");
        caido.erro = None;
        assert_eq!(detalhe_do_servidor(&caido, false, true), "Não ligou de novo");
        assert_eq!((contagem_de_ferramentas(0), contagem_de_ferramentas(1), contagem_de_ferramentas(8)), ("sem ferramentas".into(), "1 ferramenta".into(), "8 ferramentas".into()));
    }
}
