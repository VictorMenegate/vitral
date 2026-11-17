//! O diálogo "Memória da equipe": o endereço do servidor ai-memory da empresa e a chave da pessoa
//! (`crate::memoria_equipe`). "Testar e ligar" confere os dois com o servidor antes de gravar;
//! "Desligar" apaga a chave e o endereço. Abre pelas Configurações, pelo menu do logo e pelas
//! boas-vindas.

use gpui_kit::component::{
    Disableable as _, WindowExt as _,
    input::{Input, InputState},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::componentes::{CabecalhoDialogo, RodapeDialogo, TamanhoBotao, botao_fantasma, botao_primario, largura};
use crate::motor::Motor;
use crate::tema::{Cores, Lucide, Tipo as _, esp, raio, r};

/// O que o diálogo mostra embaixo dos campos.
#[derive(Clone, Default)]
enum Situacao {
    #[default]
    Parada,
    Testando,
    Ligada(String),
    Falhou(String),
}

#[derive(Default)]
struct Estado {
    situacao: Situacao,
    fechada: bool,
}

/// O rótulo em cima de um campo.
fn campo(rotulo: &'static str, ajuda: &'static str, entrada: &Entity<InputState>) -> Div {
    v_flex()
        .gap(esp::E4)
        .child(div().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child(rotulo))
        .child(Input::new(entrada))
        .child(div().metadado().text_color(Cores::tenue()).child(ajuda))
}

pub fn abrir(motor: Entity<Motor>, window: &mut Window, cx: &mut App) {
    let (endereco_atual, config, tem_chave) = {
        let m = motor.read(cx);
        (m.config.memoria_da_equipe.clone(), m.dados.config.clone(), crate::memoria_equipe::ler_chave(&m.dados.config).is_some())
    };
    let ligada = !endereco_atual.is_empty() && tem_chave;
    let endereco = cx.new(|cx| InputState::new(window, cx).placeholder("https://memoria.suaempresa.com.br").default_value(endereco_atual));
    let chave = cx.new(|cx| {
        InputState::new(window, cx).masked(true).placeholder(if tem_chave { "(a chave guardada continua valendo)" } else { "aim_…" })
    });
    let estado = cx.new(|_| Estado::default());
    let foco = endereco.read(cx).focus_handle(cx);
    window.open_dialog(cx, move |dialogo, window, cx| {
        let situacao = estado.read(cx).situacao.clone();
        let testando = matches!(situacao, Situacao::Testando);
        let ligada_agora = !motor.read(cx).config.memoria_da_equipe.is_empty() && crate::memoria_equipe::ler_chave(&config).is_some();
        let linha = match &situacao {
            Situacao::Parada if ligada => Some((Cores::tenue(), "Ligada. Para trocar, preencha e teste de novo.".to_string())),
            Situacao::Parada => None,
            Situacao::Testando => Some((Cores::tenue(), "Conferindo com o servidor…".to_string())),
            Situacao::Ligada(servidor) => Some((Cores::verde(), format!("Ligada ao {servidor}. Vale a partir da próxima mensagem de cada chat."))),
            Situacao::Falhou(motivo) => Some((Cores::vermelho(), motivo.clone())),
        };
        let (m_ligar, m_desligar) = (motor.clone(), motor.clone());
        let (e_ligar, c_ligar, s_ligar, pasta_ligar) = (endereco.clone(), chave.clone(), estado.clone(), config.clone());
        let (e_desligar, c_desligar, s_desligar, pasta_desligar) = (endereco.clone(), chave.clone(), estado.clone(), config.clone());
        let (s_fechar, s_cancelar, m_consultar) = (estado.clone(), estado.clone(), motor.clone());
        super::dialogos::moldura(dialogo, largura::M, window)
            .on_close(move |_, _, cx| { s_fechar.update(cx, |s, _| s.fechada = true); })
            .title(super::dialogos::topo(CabecalhoDialogo::new("Memória da equipe").subtitulo(
                div().font_weight(FontWeight::NORMAL).child("O que um chat aprende ajuda os próximos."),
            )))
            .child(
                v_flex()
                    .gap(esp::E16)
                    .child(
                        h_flex().items_start().gap(esp::E12).p(esp::E12).rounded(raio::CARTAO).bg(Cores::preto())
                            .child(super::mensagens::icone_de(Lucide::Brain, 20.).text_color(Cores::destaque_texto()))
                            .child(v_flex().flex_1().min_w_0().gap(esp::E4)
                                .child(div().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child(if ligada_agora { "Conectada à equipe" } else { "Conecte a memória da sua equipe" }))
                                .child(div().corpo_ui().text_color(Cores::tenue()).child("Decisões, procedimentos e aprendizados compartilhados entre o Claude e o Codex. Não precisa instalar o ai-memory neste computador."))),
                    )
                    .child(campo("Endereço do servidor", "Quem cuida do servidor passa o endereço.", &endereco))
                    .child(campo(
                        "Sua chave de acesso",
                        "Peça sua chave a quem administra a memória. Ela fica guardada só neste computador.",
                        &chave,
                    ))
                    .child(div().metadado().text_color(Cores::tenue()).child(
                        "Só os aprendizados que o agente registra são compartilhados. O histórico do chat não é enviado automaticamente.",
                    ))
                    .when(ligada_agora, |s| s.child(
                        botao_fantasma("memoria-consultar", Some(Lucide::Search.into()), "Consultar memória", TamanhoBotao::Controle, window, cx)
                            .disabled(testando)
                            .on_click(move |_, window, cx| abrir_busca(m_consultar.clone(), window, cx)),
                    ))
                    .when_some(linha, |d, (cor, texto)| d.child(div().corpo_ui().text_color(cor).child(texto))),
            )
            .footer(
                RodapeDialogo::new()
                    .when(ligada_agora, |r| {
                        r.esquerda(
                            botao_fantasma("memoria-desligar", None, "Desligar", TamanhoBotao::Controle, window, cx)
                                .disabled(testando)
                                .on_click(move |_, window, cx| {
                                    if let Err(erro) = crate::memoria_equipe::gravar_chave(&pasta_desligar, "") {
                                        s_desligar.update(cx, |s, _| s.situacao = Situacao::Falhou(format!("Não consegui remover a chave: {erro}")));
                                        cx.refresh_windows();
                                        return;
                                    }
                                    let _ = std::fs::remove_file(m_desligar.read(cx).dados.cache.join("mcp-memoria-da-equipe.json"));
                                    m_desligar.update(cx, |m, cx| m.mudar_config(|c| c.memoria_da_equipe.clear(), cx));
                                    e_desligar.update(cx, |e, cx| e.set_value("", window, cx));
                                    c_desligar.update(cx, |e, cx| e.set_value("", window, cx));
                                    s_desligar.update(cx, |s, _| { s.situacao = Situacao::Parada; s.fechada = true; });
                                    window.close_dialog(cx);
                                }),
                        )
                    })
                    .cancelar(
                        botao_fantasma("memoria-fechar", None, "Fechar", TamanhoBotao::Controle, window, cx)
                            .on_click(move |_, window, cx| { s_cancelar.update(cx, |s, _| s.fechada = true); window.close_dialog(cx); }),
                    )
                    .principal(
                        botao_primario("memoria-ligar", None, "Testar e ligar", TamanhoBotao::Controle, window, cx)
                            .disabled(testando)
                            .on_click(move |_, _, cx| {
                                let digitado = e_ligar.read(cx).value().trim().to_string();
                                // Campo da chave vazio com uma já guardada: testa com a guardada.
                                let nova = c_ligar.read(cx).value().trim().to_string();
                                let chave = if nova.is_empty() { crate::memoria_equipe::ler_chave(&pasta_ligar).unwrap_or_default() } else { nova };
                                s_ligar.update(cx, |s, _| s.situacao = Situacao::Testando);
                                cx.refresh_windows();
                                let (motor, estado, pasta) = (m_ligar.clone(), s_ligar.clone(), pasta_ligar.clone());
                                cx.spawn(async move |cx| {
                                    let (teste, digitado, chave) = cx
                                        .background_spawn(async move { (crate::memoria_equipe::testar(&digitado, &chave), digitado, chave) })
                                        .await;
                                    cx.update(|cx| {
                                        if estado.read(cx).fechada { return; }
                                        let situacao = match teste {
                                            Ok(servidor) => match crate::memoria_equipe::gravar_chave(&pasta, &chave) {
                                                Ok(()) => {
                                                    motor.update(cx, |m, cx| m.mudar_config(|c| c.memoria_da_equipe = digitado, cx));
                                                    Situacao::Ligada(servidor)
                                                }
                                                Err(erro) => Situacao::Falhou(format!("Não consegui guardar a chave: {erro}")),
                                            },
                                            Err(motivo) => Situacao::Falhou(motivo),
                                        };
                                        estado.update(cx, |s, _| s.situacao = situacao);
                                        cx.refresh_windows();
                                    });
                                })
                                .detach();
                            }),
                    ),
            )
    });
    window.focus(&foco, cx);
}

#[derive(Clone, Default)]
struct Busca {
    carregando: bool,
    pesquisou: bool,
    paginas: Vec<crate::memoria_equipe::Pagina>,
    pagina: Option<(String, String)>,
    erro: Option<String>,
}

fn abrir_busca(motor: Entity<Motor>, window: &mut Window, cx: &mut App) {
    let m = motor.read(cx);
    let endereco = m.config.memoria_da_equipe.clone();
    let Some(chave) = crate::memoria_equipe::ler_chave(&m.dados.config) else { return; };
    let consulta = cx.new(|cx| InputState::new(window, cx).placeholder("Busque uma decisão, um procedimento, um projeto…"));
    let foco = consulta.read(cx).focus_handle(cx);
    let estado = cx.new(|_| Busca::default());
    window.open_dialog(cx, move |dialogo, window, cx| {
        let busca = estado.read(cx).clone();
        let carregando = busca.carregando;
        let (e_buscar, c_buscar, s_buscar, entrada) = (endereco.clone(), chave.clone(), estado.clone(), consulta.clone());
        let mut corpo = v_flex().gap(esp::E12)
            .child(Input::new(&consulta))
            .child(botao_primario("memoria-buscar", Some(Lucide::Search.into()), "Buscar", TamanhoBotao::Controle, window, cx)
                .disabled(carregando)
                .on_click(move |_, _, cx| {
                    let texto = entrada.read(cx).value().to_string();
                    s_buscar.update(cx, |s, _| { s.carregando = true; s.erro = None; s.pagina = None; });
                    cx.refresh_windows();
                    let (endereco, chave, estado) = (e_buscar.clone(), c_buscar.clone(), s_buscar.clone());
                    cx.spawn(async move |cx| {
                        let resultado = cx.background_spawn(async move { crate::memoria_equipe::buscar(&endereco, &chave, &texto) }).await;
                        cx.update(|cx| {
                            estado.update(cx, |s, _| {
                                s.carregando = false;
                                s.pesquisou = true;
                                match resultado { Ok(paginas) => s.paginas = paginas, Err(erro) => s.erro = Some(erro) }
                            });
                            cx.refresh_windows();
                        });
                    }).detach();
                }));
        if carregando {
            corpo = corpo.child(div().corpo_ui().text_color(Cores::tenue()).child("Consultando a memória…"));
        }
        if let Some(erro) = &busca.erro {
            corpo = corpo.child(div().corpo_ui().text_color(Cores::vermelho()).child(erro.clone()));
        } else if let Some((titulo, texto)) = &busca.pagina {
            let voltar = estado.clone();
            corpo = corpo
                .child(botao_fantasma("memoria-resultados", Some(Lucide::ArrowLeft.into()), "Voltar aos resultados", TamanhoBotao::Controle, window, cx)
                    .disabled(carregando).on_click(move |_, _, cx| { voltar.update(cx, |s, _| s.pagina = None); cx.refresh_windows(); }))
                .child(div().corpo_ui().font_weight(FontWeight::SEMIBOLD).text_color(Cores::forte()).child(titulo.clone()))
                .child(div().id("memoria-pagina").max_h(r(320.)).overflow_y_scroll().child(super::mensagens::markdown("memoria-pagina-texto".into(), texto.clone(), cx)));
        } else {
            if busca.pesquisou && busca.paginas.is_empty() && !carregando {
                corpo = corpo.child(div().corpo_ui().text_color(Cores::tenue()).child("Nada encontrado. Tente outras palavras; a memória cresce com o que a equipe registra."));
            }
            let resultados = v_flex().id("memoria-resultados-lista").max_h(r(320.)).overflow_y_scroll().gap(esp::E8)
                .children(busca.paginas.iter().enumerate().map(|(i, pagina)| {
                    let (pagina, estado, endereco, chave) = (pagina.clone(), estado.clone(), endereco.clone(), chave.clone());
                    div().id(("memoria-resultado", i)).p(esp::E12).rounded(raio::CARTAO).bg(Cores::preto())
                        .cursor_pointer().hover(|s| s.bg(Cores::cartao_opaco())).child(
                            v_flex().gap(esp::E4)
                                .child(div().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child(pagina.titulo.clone()))
                                .child(div().metadado().text_color(Cores::destaque_texto()).child(format!("{} / {}", pagina.workspace, pagina.projeto)))
                                .child(div().corpo_ui().text_color(Cores::tenue()).child(pagina.resumo.clone())),
                        ).on_click(move |_, _, cx| {
                            if estado.read(cx).carregando { return; }
                            estado.update(cx, |s, _| { s.carregando = true; s.erro = None; });
                            cx.refresh_windows();
                            let (pagina, estado, endereco, chave) = (pagina.clone(), estado.clone(), endereco.clone(), chave.clone());
                            cx.spawn(async move |cx| {
                                let titulo = pagina.titulo.clone();
                                let resultado = cx.background_spawn(async move { crate::memoria_equipe::ler_pagina(&endereco, &chave, &pagina) }).await;
                                cx.update(|cx| {
                                    estado.update(cx, |s, _| {
                                        s.carregando = false;
                                        match resultado { Ok(texto) => s.pagina = Some((titulo, texto)), Err(erro) => s.erro = Some(erro) }
                                    });
                                    cx.refresh_windows();
                                });
                            }).detach();
                        })
                }));
            corpo = corpo.child(resultados);
        }
        super::dialogos::moldura(dialogo, largura::M, window)
            .title(super::dialogos::topo(CabecalhoDialogo::new("Consultar a memória").subtitulo(
                div().font_weight(FontWeight::NORMAL).child("Busque o que a equipe já aprendeu. Abra um resultado para ler."),
            )))
            .child(corpo)
            .footer(RodapeDialogo::new().cancelar(botao_fantasma("memoria-busca-fechar", None, "Fechar", TamanhoBotao::Controle, window, cx)
                .on_click(|_, window, cx| window.close_dialog(cx))))
    });
    window.focus(&foco, cx);
}
