//! A primeira abertura do Vitral: uma apresentação curta, a escolha da pasta de trabalho (como o
//! "Abrir pasta" do VS Code: é nela que os chats nascem) e três preferências básicas. O dono
//! pediu (13/11/2026): "tem que ter uma apresentação e escolher a pasta que vai rodar tipo o
//! vscode… faz bem bonitinho e pergunta umas configurações básicas".
//!
//! A tela toma a janela inteira, por cima do metal, até o "Começar". Com `VITRAL_BOAS_VINDAS=1`
//! ela abre mesmo fora da primeira vez (para ver e ajustar o desenho).

use std::path::PathBuf;

use gpui_kit::component::{Sizable as _, Size, h_flex, switch::Switch, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{ControleSegmentado, TamanhoBotao, botao_fantasma, botao_primario, malha_viva, metal::FASE_INICIAL_DO_METAL},
    composer::MODELOS,
};
use crate::tema::{Cores, FONTE_MONO, Lucide, Tipo as _, aresta_de_tecla, esp, r, raio};

/// Os passos, na ordem.
const PASSOS: [&str; 4] = ["Apresentação", "Pasta de trabalho", "Preferências", "Memória da equipe"];

/// Os modos de permissão oferecidos aqui (os outros ficam na caixa de cada chat): o valor que vai
/// para a configuração, o nome e o que ele quer dizer.
const MODOS: [(&str, &str, &str); 3] = [
    ("manual", "Perguntar sempre", "Cada edição e cada comando pedem a sua aprovação na tela."),
    ("acceptEdits", "Aceitar edições", "Edita arquivos sem perguntar; os comandos ainda pedem."),
    ("bypassPermissions", "Sem perguntar", "Faz tudo sem pedir, como o modo livre do Claude Code."),
];

/// Quantas pastas a tela sugere (as em que o Claude Code já tem mais conversas).
const SUGESTOES: usize = 5;

/// O estado da tela de boas-vindas enquanto ela está aberta.
pub(super) struct BoasVindas {
    passo: usize,
    /// As pastas em que o Claude Code já tem conversas, com quantas (da que tem mais para menos).
    sugestoes: Vec<(PathBuf, usize)>,
    pasta: Option<PathBuf>,
    modo: usize,
    modelo: usize,
    /// Quem responde nos chats novos: 0 o Claude, 1 o Codex (só aparece com o Codex instalado).
    agente: usize,
    importar: bool,
}

/// A pasta com o `~` no lugar da pasta do usuário, para caber e ler melhor.
fn com_til(pasta: &std::path::Path) -> String {
    match dirs::home_dir().and_then(|casa| pasta.strip_prefix(&casa).ok().map(|resto| resto.to_path_buf())) {
        Some(resto) if resto.as_os_str().is_empty() => "~".into(),
        Some(resto) => format!("~/{}", resto.display()),
        None => pasta.display().to_string(),
    }
}

impl Janela {
    /// A tela de boas-vindas está aberta.
    pub(super) fn em_boas_vindas(&self) -> bool {
        self.boas_vindas.is_some()
    }

    /// Abre a tela de boas-vindas e procura, em segundo plano, as pastas em que o Claude Code já
    /// tem conversas (a primeira vira a sugestão marcada).
    pub(super) fn abrir_boas_vindas(&mut self, cx: &mut Context<Self>) {
        let (modo, modelo) = self.motor.as_ref().map_or((2, 0), |m| {
            let config = &m.read(cx).config;
            (
                MODOS.iter().position(|(valor, ..)| *valor == config.permissao).unwrap_or(2),
                MODELOS.iter().position(|(valor, _)| *valor == config.modelo_chats).unwrap_or(0),
            )
        });
        self.boas_vindas = Some(BoasVindas { passo: 0, sugestoes: Vec::new(), pasta: None, modo, modelo, agente: 0, importar: true });
        let raizes = crate::importar::raizes(std::iter::empty());
        cx.spawn(async move |this, cx| {
            let achadas = cx
                .background_spawn(async move {
                    let pastas = crate::importar::pastas_de_projects(&raizes);
                    crate::importar::pastas_usadas(&pastas)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(tela) = &mut this.boas_vindas {
                    if tela.pasta.is_none() {
                        tela.pasta = achadas.first().map(|(pasta, _)| pasta.clone());
                    }
                    tela.sugestoes = achadas.into_iter().take(SUGESTOES).collect();
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// "Escolher outra pasta…": o seletor do sistema.
    fn escolher_pasta_de_trabalho(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let escolha = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Usar esta pasta".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(caminhos))) = escolha.await else {
                return;
            };
            let Some(pasta) = caminhos.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(tela) = &mut this.boas_vindas {
                    tela.pasta = Some(pasta);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// "Começar": grava as escolhas e fecha a tela.
    fn concluir_boas_vindas(&mut self, cx: &mut Context<Self>) {
        let Some(tela) = self.boas_vindas.take() else {
            return;
        };
        if let Some(motor) = self.motor.clone() {
            let (modo, modelo) = (MODOS[tela.modo.min(MODOS.len() - 1)].0, MODELOS[tela.modelo.min(MODELOS.len() - 1)].0);
            let pasta = tela.pasta.clone();
            motor.update(cx, |m, cx| {
                m.mudar_config(
                    |c| {
                        c.permissao = modo.to_string();
                        c.modelo_chats = modelo.to_string();
                        c.agente_padrao = if tela.agente == 1 { crate::modelo::Agente::Codex } else { crate::modelo::Agente::Claude };
                        c.pasta_de_trabalho = pasta;
                        c.importar_da_pasta = tela.importar;
                    },
                    cx,
                )
            });
        }
        cx.notify();
    }

    /// A tela inteira: a faixa de arrastar com os botões da janela, e no meio o medalhão do logo,
    /// os passos e o cartão do passo de agora.
    pub(super) fn tela_de_boas_vindas(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(tela) = &self.boas_vindas else {
            return div().into_any_element();
        };
        let passo = tela.passo.min(PASSOS.len() - 1);
        let ultimo = passo + 1 == PASSOS.len();
        // Sem pasta escolhida não se passa do passo dela.
        let pode_seguir = passo != 1 || tela.pasta.is_some();
        // Duas colunas (dono, 13/11/2026): à esquerda o logo e o que está escrito (o título, a
        // frase e os passos, em lista); à direita, o cartão do passo de agora.
        let medalhao = 88.;
        let passos = v_flex().gap(esp::E12).children((0..PASSOS.len()).map(|i| {
            let (agora, feito) = (i == passo, i < passo);
            h_flex()
                .items_center()
                .gap(esp::E12)
                .child(
                    div()
                        .flex_none()
                        .size(r(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(raio::de(28.))
                        .border_1()
                        .map(|s| {
                            if agora {
                                s.bg(Cores::destaque()).border_color(Cores::destaque()).shadow(aresta_de_tecla())
                            } else if feito {
                                s.bg(Cores::tinta_solida(Cores::destaque(), 0.3)).border_color(Cores::tinta_borda(Cores::destaque()))
                            } else {
                                s.bg(gpui_kit::rgba(0x00000033)).border_color(Cores::forte().opacity(0.25))
                            }
                        })
                        .child(if feito {
                            super::mensagens::icone_de(Lucide::Check, 13.).text_color(Cores::forte()).into_any_element()
                        } else {
                            div()
                                .etiqueta()
                                .text_color(if agora { Cores::sobre_destaque() } else { Cores::forte().opacity(0.7) })
                                .child(format!("{}", i + 1))
                                .into_any_element()
                        }),
                )
                .child(
                    div()
                        .corpo_ui()
                        .font_weight(if agora { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                        .text_color(if agora { Cores::forte() } else { Cores::forte().opacity(0.7) })
                        .child(PASSOS[i]),
                )
        }));
        let esquerda = v_flex()
            .flex_none()
            .w(r(340.))
            .items_start()
            .gap(esp::E24)
            .child(
                div()
                    .relative()
                    .flex_none()
                    .size(r(medalhao))
                    .child(div().absolute().top_0().left_0().size_full().child(malha_viva(FASE_INICIAL_DO_METAL, raio::de(medalhao), false)))
                    .child(img(super::trilho::logo_v()).absolute().top_0().left_0().size_full())
                    .child(div().absolute().inset_0().rounded(raio::de(medalhao)).shadow(aresta_de_tecla())),
            )
            .child(
                v_flex()
                    .gap(esp::E8)
                    .child(div().text_size(r(32.)).line_height(r(38.)).font_weight(FontWeight::SEMIBOLD).text_color(Cores::forte()).child("Bem-vindo ao Vitral"))
                    .child(
                        div()
                            .corpo_ui()
                            // Sobre o metal à vista, o texto vai um degrau mais claro que o tênue.
                            .text_color(Cores::texto())
                            .child("Um lugar só para trabalhar com o Claude Code e o Codex: vários chats, cada um mexendo direto na sua pasta."),
                    ),
            )
            .child(passos);
        let corpo = match passo {
            0 => self.passo_da_apresentacao().into_any_element(),
            1 => self.passo_da_pasta(tela, window, cx).into_any_element(),
            2 => self.passo_das_preferencias(tela, cx).into_any_element(),
            _ => self.passo_da_memoria(window, cx).into_any_element(),
        };
        let rodape = h_flex()
            .w_full()
            .justify_between()
            .child(if passo == 0 {
                div().into_any_element()
            } else {
                botao_fantasma("bv-voltar", Some(Lucide::ArrowLeft.into()), "Voltar", TamanhoBotao::Controle, window, cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(tela) = &mut this.boas_vindas {
                            tela.passo = tela.passo.saturating_sub(1);
                            cx.notify();
                        }
                    }))
                    .into_any_element()
            })
            .child(
                botao_primario(
                    "bv-seguir",
                    Some(if ultimo { Lucide::Check } else { Lucide::ArrowRight }.into()),
                    if ultimo { "Começar" } else { "Continuar" },
                    TamanhoBotao::Controle,
                    window,
                    cx,
                )
                .when(!pode_seguir, |b| b.opacity(0.45).tooltip("Escolha a pasta de trabalho"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !pode_seguir {
                        return;
                    }
                    if ultimo {
                        this.concluir_boas_vindas(cx);
                    } else if let Some(tela) = &mut this.boas_vindas {
                        tela.passo += 1;
                        cx.notify();
                    }
                })),
            );
        let cartao = v_flex()
            .w_full()
            .gap(esp::E24)
            .p(esp::E24)
            .rounded(crate::tema::raio_da_peca())
            .bg(Cores::cartao_opaco())
            .border_1()
            .border_color(Cores::borda())
            .shadow(crate::tema::sombra_flutuante())
            .child(corpo)
            .child(rodape);
        div()
            .flex()
            .flex_col()
            .size_full()
            .p(esp::E8)
            .child(
                div()
                    .flex()
                    .w_full()
                    .child(self.arrastavel("arrastar-boas-vindas").flex_1().h(px(36.)))
                    .child(self.botoes_janela(cx)),
            )
            .child(
                div()
                    .id("boas-vindas")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(esp::E24)
                    .pb(esp::E32)
                    .overflow_y_scroll()
                    // Lado a lado; na janela estreita a coluna da direita desce.
                    .child(
                        h_flex()
                            .w_full()
                            .max_w(r(1000.))
                            .flex_wrap()
                            .items_center()
                            .justify_center()
                            .gap_x(esp::E32)
                            .gap_y(esp::E32)
                            .child(esquerda)
                            .child(div().flex_1().min_w(r(420.)).max_w(r(600.)).child(cartao)),
                    ),
            )
            .into_any_element()
    }

    /// Passo 1: o que é o Vitral, em três linhas.
    fn passo_da_apresentacao(&self) -> impl IntoElement + use<> {
        let linha = |icone: Lucide, titulo: &'static str, texto: &'static str| {
            h_flex()
                .items_start()
                .gap(esp::E12)
                .child(
                    div()
                        .flex_none()
                        .size(r(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(raio::de(36.))
                        .bg(Cores::tinta_solida(Cores::destaque(), 0.14))
                        .border_1()
                        .border_color(Cores::tinta_borda(Cores::destaque()))
                        .child(super::mensagens::icone_de(icone, 16.).text_color(Cores::destaque_texto())),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(esp::E2)
                        .child(div().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child(titulo))
                        .child(div().corpo_ui().text_color(Cores::tenue()).child(texto)),
                )
        };
        v_flex()
            .gap(esp::E16)
            .child(linha(
                Lucide::MessagesSquare,
                "Uma lista de chats",
                "Cada chat é uma conversa trabalhando direto na sua pasta, como no VS Code. Em cada um você escolhe quem responde: o Claude ou o Codex.",
            ))
            .child(linha(
                Lucide::Columns2,
                "Vários ao mesmo tempo",
                "Abas e painéis lado a lado: arraste uma aba para abrir outra conversa do lado e acompanhe as duas.",
            ))
            .child(linha(
                Lucide::ShieldCheck,
                "Você decide o que roda",
                "Cada pedido de permissão aparece na tela, com o comando inteiro. Os perigosos pedem para segurar o botão.",
            ))
            .child(linha(
                Lucide::Brain,
                "O que se aprende fica",
                "Ao fim de uma sessão que mexeu em arquivo, o agente revê o que vale guardar. Com a memória da equipe ligada, isso chega a todos.",
            ))
    }

    /// Passo 4: a memória da equipe, que é opcional. O botão abre o mesmo diálogo das
    /// Configurações; ligada, o passo mostra onde.
    fn passo_da_memoria(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let endereco = self.motor.as_ref().map(|m| m.read(cx).config.memoria_da_equipe.clone()).unwrap_or_default();
        let ligada = self.motor.as_ref().is_some_and(|m| {
            !endereco.is_empty() && crate::memoria_equipe::ler_chave(&m.read(cx).dados.config).is_some()
        });
        let ponto = |titulo: &'static str, texto: &'static str| {
            v_flex()
                .gap(esp::E2)
                .child(div().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child(titulo))
                .child(div().corpo_ui().text_color(Cores::tenue()).child(texto))
        };
        let botao = super::componentes::botao_fantasma(
            "bv-memoria",
            Some(Lucide::Brain.into()),
            if ligada { "Trocar o servidor…" } else { "Ligar a memória da equipe…" },
            super::componentes::TamanhoBotao::Controle,
            window,
            cx,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            if let Some(motor) = this.motor.clone() {
                super::memoria_equipe::abrir(motor, window, cx);
            }
        }));
        v_flex()
            .gap(esp::E16)
            .child(ponto(
                "Todos com o mesmo contexto",
                "Se a sua empresa tem um servidor de memória (ai-memory), os chats consultam e registram nele o que vale para a equipe: decisões, armadilhas, procedimentos.",
            ))
            .child(ponto(
                "Só o que importa sobe",
                "A equipe compartilha o que o agente registra como aprendizado. O histórico do chat não é enviado automaticamente à memória.",
            ))
            .child(
                v_flex()
                    .items_start()
                    .gap(esp::E12)
                    .child(botao)
                    .when(ligada, |s| s.child(div().metadado().text_color(Cores::verde()).child(format!("Ligada a {endereco}")))),
            )
            .child(
                div()
                    .metadado()
                    .text_color(Cores::tenue())
                    .child("Não tem servidor ou não tem a chave agora? Pode seguir: dá para ligar depois em Memória da equipe, no menu do logo."),
            )
    }

    /// Passo 2: a pasta de trabalho. As sugestões são as pastas em que o Claude Code já tem
    /// conversas; a escolhida fica marcada, e o botão abre o seletor do sistema.
    fn passo_da_pasta(&self, tela: &BoasVindas, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let escolhida = tela.pasta.clone();
        // A escolhida que não está entre as sugestões (veio do seletor) entra no alto da lista.
        let mut linhas: Vec<(PathBuf, Option<usize>)> = tela.sugestoes.iter().map(|(p, n)| (p.clone(), Some(*n))).collect();
        if let Some(pasta) = &escolhida
            && !linhas.iter().any(|(p, _)| p == pasta)
        {
            linhas.insert(0, (pasta.clone(), None));
        }
        let lista = v_flex().gap(esp::E6).children(linhas.into_iter().enumerate().map(|(i, (pasta, conversas))| {
            let marcada = escolhida.as_ref() == Some(&pasta);
            let nome = pasta.file_name().map_or_else(|| pasta.display().to_string(), |n| n.to_string_lossy().into_owned());
            let alvo = pasta.clone();
            h_flex()
                .id(("bv-pasta", i))
                .h(r(52.))
                .px(esp::E12)
                .gap(esp::E12)
                .rounded(raio::CONTROLE)
                .border_1()
                .cursor_pointer()
                .map(|s| {
                    if marcada {
                        s.bg(Cores::tinta_solida(Cores::destaque(), 0.14)).border_color(Cores::tinta_borda(Cores::destaque()))
                    } else {
                        s.border_color(Cores::borda()).hover(|s| s.bg(Cores::hover()))
                    }
                })
                .child(
                    super::mensagens::icone_de(if marcada { Lucide::FolderOpen } else { Lucide::Folder }, 16.)
                        .text_color(if marcada { Cores::destaque_texto() } else { Cores::tenue() }),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child(nome))
                        .child(div().truncate().metadado().font_family(FONTE_MONO).text_color(Cores::tenue()).child(com_til(&pasta))),
                )
                .children(conversas.map(|n| {
                    div().flex_none().metadado().text_color(Cores::tenue()).child(if n == 1 {
                        "1 conversa".to_string()
                    } else {
                        format!("{n} conversas")
                    })
                }))
                .when(marcada, |s| s.child(super::mensagens::icone_de(Lucide::Check, 14.).text_color(Cores::destaque_texto())))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(tela) = &mut this.boas_vindas {
                        tela.pasta = Some(alvo.clone());
                        cx.notify();
                    }
                }))
        }));
        v_flex()
            .gap(esp::E16)
            .child(
                v_flex()
                    .gap(esp::E4)
                    .child(div().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child("Em qual pasta você trabalha?"))
                    .child(
                        div()
                            .corpo_ui()
                            .text_color(Cores::tenue())
                            .child("É como o Abrir pasta do VS Code: todo chat novo nasce nela, e a lista mostra os chats dela. Dá para trocar depois, no menu do logo."),
                    ),
            )
            .when(tela.sugestoes.is_empty() && escolhida.is_none(), |s| {
                s.child(div().metadado().text_color(Cores::apagado()).child("Procurando as pastas em que você já usa o Claude Code…"))
            })
            .child(lista)
            .child(
                h_flex().child(
                    botao_fantasma("bv-outra-pasta", Some(Lucide::FolderSearch.into()), "Escolher outra pasta…", TamanhoBotao::Controle, window, cx)
                        .on_click(cx.listener(|this, _, window, cx| this.escolher_pasta_de_trabalho(window, cx))),
                ),
            )
    }

    /// Passo 3: as três preferências básicas (tudo muda depois, nas Configurações ou na caixa de
    /// cada chat).
    fn passo_das_preferencias(&self, tela: &BoasVindas, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let grupo = |titulo: &'static str, texto: String, controle: AnyElement| {
            v_flex()
                .gap(esp::E8)
                .child(
                    v_flex()
                        .gap(esp::E2)
                        .child(div().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child(titulo))
                        .child(div().metadado().text_color(Cores::tenue()).child(texto)),
                )
                .child(controle)
        };
        let janela = cx.entity().downgrade();
        let (j1, j2, j3, j4) = (janela.clone(), janela.clone(), janela.clone(), janela);
        let (agente, tem_codex) = (tela.agente.min(1), crate::agente_codex::instalado());
        let modo = tela.modo.min(MODOS.len() - 1);
        let modelo = tela.modelo.min(MODELOS.len() - 1);
        let nomes_dos_modos: Vec<&'static str> = MODOS.iter().map(|(_, nome, _)| *nome).collect();
        let nomes_dos_modelos: Vec<&'static str> = MODELOS.iter().map(|(_, nome)| *nome).collect();
        let importar = tela.importar;
        let pasta = tela.pasta.as_deref().map(com_til).unwrap_or_default();
        v_flex()
            .gap(esp::E24)
            .child(grupo(
                "Permissões",
                MODOS[modo].2.to_string(),
                ControleSegmentado::new("bv-modo", nomes_dos_modos, modo, move |novo, _, cx| {
                    let _ = j1.update(cx, |this, cx| {
                        if let Some(tela) = &mut this.boas_vindas {
                            tela.modo = novo;
                            cx.notify();
                        }
                    });
                })
                .with_size(Size::Medium)
                .largura_total(true)
                .into_any_element(),
            ))
            .when(tem_codex, |s| {
                s.child(grupo(
                    "Quem responde",
                    if agente == 1 {
                        "Os chats novos começam no Codex, com o login e o plano dele. Cada chat pode trocar no seletor de modelo.".to_string()
                    } else {
                        "Os chats novos começam no Claude. Cada chat pode passar para o Codex no seletor de modelo.".to_string()
                    },
                    ControleSegmentado::new("bv-agente", vec!["Claude", "Codex"], agente, move |novo, _, cx| {
                        let _ = j4.update(cx, |this, cx| {
                            if let Some(tela) = &mut this.boas_vindas {
                                tela.agente = novo;
                                cx.notify();
                            }
                        });
                    })
                    .with_size(Size::Medium)
                    .largura_total(true)
                    .into_any_element(),
                ))
            })
            .child(grupo(
                "Modelo do Claude",
                if modelo == 0 {
                    "O padrão do seu Claude Code. Cada chat pode trocar o dele na caixa de mensagem.".to_string()
                } else {
                    format!("Os chats novos começam no {}. Cada chat pode trocar o dele na caixa de mensagem.", MODELOS[modelo].1)
                },
                ControleSegmentado::new("bv-modelo", nomes_dos_modelos, modelo, move |novo, _, cx| {
                    let _ = j2.update(cx, |this, cx| {
                        if let Some(tela) = &mut this.boas_vindas {
                            tela.modelo = novo;
                            cx.notify();
                        }
                    });
                })
                .with_size(Size::Medium)
                .largura_total(true)
                .into_any_element(),
            ))
            .child(
                h_flex()
                    .items_center()
                    .gap(esp::E16)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(esp::E2)
                            .child(div().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child("Trazer as conversas que já existem"))
                            .child(div().metadado().text_color(Cores::tenue()).child(format!(
                                "As conversas do Claude Code de {pasta} (do terminal e do VS Code) entram na lista. Nada lá é mudado."
                            ))),
                    )
                    .child(Switch::new("bv-importar").checked(importar).on_click(move |novo, _, cx| {
                        let novo = *novo;
                        let _ = j3.update(cx, |this, cx| {
                            if let Some(tela) = &mut this.boas_vindas {
                                tela.importar = novo;
                                cx.notify();
                            }
                        });
                    })),
            )
    }
}

#[cfg(test)]
mod testes {
    use super::{MODOS, com_til};

    #[test]
    fn a_pasta_do_usuario_vira_til() {
        let casa = dirs::home_dir().unwrap();
        assert_eq!(com_til(&casa), "~");
        assert_eq!(com_til(&casa.join("eita")), "~/eita");
        assert_eq!(com_til(std::path::Path::new("/opt/x")), "/opt/x");
    }

    /// Os modos oferecidos na primeira abertura existem na lista de modos da caixa.
    #[test]
    fn os_modos_oferecidos_existem() {
        for (valor, ..) in MODOS {
            assert!(crate::ui::composer::MODOS.iter().any(|(v, ..)| v == &valor), "{valor}");
        }
    }
}
