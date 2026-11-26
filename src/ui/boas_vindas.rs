//! A primeira abertura do Vitral: uma apresentação curta, a escolha da pasta de trabalho (como o
//! "Abrir pasta" do VS Code: é nela que os chats nascem) e três preferências básicas. O dono
//! pediu (13/11/2026): "tem que ter uma apresentação e escolher a pasta que vai rodar tipo o
//! vscode… faz bem bonitinho e pergunta umas configurações básicas".
//!
//! A tela toma a janela inteira, por cima do metal, até o "Começar". Com `VITRAL_BOAS_VINDAS=1`
//! ela abre mesmo fora da primeira vez (para ver e ajustar o desenho).

use std::path::PathBuf;

use gpui_kit::component::{h_flex, switch::Switch, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{TamanhoBotao, botao_fantasma, botao_primario, malha_viva, metal::FASE_INICIAL_DO_METAL},
    composer::MODELOS,
};
use crate::tema::{Cores, FONTE_MONO, Lucide, esp, r, raio};

/// Os passos, na ordem. As IAs entraram em 26/11/2026 (dono: "tem que colocar pra dar pra baixar
/// todas as IA que vai colocar por aqui, até o Claude Code").
const PASSOS: [&str; 5] = ["Apresentação", "Pasta", "IAs", "Preferências", "Memória"];

/// Os modos de permissão oferecidos aqui (os outros ficam na caixa de cada chat): o valor que vai
/// para a configuração, o nome e o que ele quer dizer.
const MODOS: [(&str, &str, &str); 3] = [
    ("manual", "Perguntar sempre", "Cada edição e comando pedem aprovação."),
    ("acceptEdits", "Aceitar edições", "Edita sem perguntar; comandos pedem."),
    ("bypassPermissions", "Sem perguntar", "Faz tudo sem pedir."),
];

/// O passo das IAs (na ordem de `PASSOS`).
const PASSO_DAS_IAS: usize = 2;

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
    /// Quem responde nos chats novos: o índice em `Agente::instalados()` (0 é o Claude; a escolha
    /// só aparece com outro agente instalado).
    agente: usize,
    importar: bool,
    /// A revisão automática do turno pelo outro agente (`Config.revisar_edicoes`).
    revisar: bool,
    /// A sugestão da próxima mensagem na caixa (`Config.sugerir_resposta`).
    sugerir: bool,
    /// O aviso dentro do app quando um chat precisa de você (`Config.avisos_no_app`).
    avisos: bool,
    /// Para onde foi a última troca de passo (true: adiante). O passo novo entra deslizando
    /// desse lado (dono, 23/11/2026: "slide the next step in from the direction you're going").
    indo: Option<bool>,
    /// A busca das pastas sugeridas ainda não voltou.
    procurando: bool,
}

/// A pasta com o `~` no lugar da pasta do usuário, para caber e ler melhor.
pub(super) fn com_til(pasta: &std::path::Path) -> String {
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
        let padrao = crate::modelo::Config::default();
        let (modo, modelo, revisar, sugerir, avisos) = self.motor.as_ref().map_or((2, 2, padrao.revisar_edicoes, padrao.sugerir_resposta, padrao.avisos_no_app), |m| {
            let config = &m.read(cx).config;
            (
                MODOS.iter().position(|(valor, ..)| *valor == config.permissao).unwrap_or(2),
                // Sem um dos três modelos da tela (o Padrão saiu do seletor), começa no Opus.
                MODELOS
                    .iter()
                    .position(|(valor, _)| *valor == config.modelo_chats && matches!(*valor, "opus" | "sonnet" | "haiku"))
                    .unwrap_or_else(|| MODELOS.iter().position(|(valor, _)| *valor == "opus").unwrap_or(0)),
                config.revisar_edicoes,
                config.sugerir_resposta,
                config.avisos_no_app,
            )
        });
        self.boas_vindas = Some(BoasVindas {
            passo: 0,
            sugestoes: Vec::new(),
            pasta: None,
            modo,
            modelo,
            agente: 0,
            importar: true,
            revisar,
            sugerir,
            avisos,
            indo: None,
            procurando: true,
        });
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
                    tela.procurando = false;
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
            prompt: Some(tr!("Usar esta pasta")),
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
                        c.agente_padrao = crate::modelo::Agente::instalados().get(tela.agente).copied().unwrap_or_default();
                        c.pasta_de_trabalho = pasta;
                        c.importar_da_pasta = tela.importar;
                        c.revisar_edicoes = tela.revisar;
                        c.sugerir_resposta = tela.sugerir;
                        c.avisos_no_app = tela.avisos;
                    },
                    cx,
                )
            });
        }
        // A tela some num fade por cima do app que aparece embaixo (o `.inicio.saindo`).
        self.saida_boas_vindas = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(200)).await;
            let _ = this.update(cx, |this, cx| {
                this.saida_boas_vindas = false;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// A tela inteira (protótipo Linear, 26/11/2026): a faixa de arrastar com os botões da janela
    /// e, no meio, um cartão só no preto, com o logo e os passos no alto, o passo de agora e o pé
    /// com Voltar e Continuar; atrás, o fundo da lista com um brilho do destaque vindo de baixo.
    pub(super) fn tela_de_boas_vindas(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        use crate::tema::inicio as m;
        let Some(tela) = &self.boas_vindas else {
            return div().into_any_element();
        };
        let passo = tela.passo.min(PASSOS.len() - 1);
        let ultimo = passo + 1 == PASSOS.len();
        // Sem pasta escolhida não se passa do passo dela.
        let pode_seguir = passo != 1 || tela.pasta.is_some();
        let sem_pasta = passo == 1 && tela.pasta.is_none();
        // Os passos no alto: o de agora no cinza do cartão, os feitos com a bolinha verde. Dá para
        // voltar a qualquer um e ir ao seguinte (o da pasta só com a pasta escolhida).
        let pilulas = h_flex().ml_auto().gap(esp::E4).children(PASSOS.iter().enumerate().map(|(i, nome)| {
            let (atual, feito) = (i == passo, i < passo);
            let pode = i <= passo || (i == passo + 1 && !sem_pasta);
            h_flex()
                .id(("bv-pilula", i))
                .flex_none()
                .items_center()
                .gap(esp::E6)
                .h(r(24.))
                .pl(m::PILULA_ESQUERDA)
                .pr(m::PILULA_DIREITA)
                .rounded(raio::de(24.))
                .text_size(r(11.5))
                .whitespace_nowrap()
                .text_color(if atual { Cores::texto() } else if feito { Cores::tenue() } else { Cores::apagado() })
                .when(atual, |s| s.bg(Cores::cartao_elevado()))
                .child(div().flex_none().size(r(6.)).rounded_full().bg(if atual {
                    Cores::destaque_texto()
                } else if feito {
                    Cores::verde()
                } else {
                    Cores::apagado().opacity(0.6)
                }))
                .child(SharedString::from(crate::i18n::traduzir(*nome)))
                .when(pode && !atual, |s| {
                    s.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| this.ir_ao_passo(i, cx)))
                })
        }));
        let logo = div()
            .relative()
            .flex_none()
            .size(r(36.))
            .child(div().absolute().top_0().left_0().size_full().child(malha_viva(FASE_INICIAL_DO_METAL, raio::de(36.), false)))
            .child(img(super::trilho::logo_v()).absolute().top_0().left_0().size_full());
        let corpo = match passo {
            0 => self.passo_da_apresentacao(window, cx).into_any_element(),
            1 => self.passo_da_pasta(tela, window, cx).into_any_element(),
            2 => self.passo_das_ias(window, cx).into_any_element(),
            3 => self.passo_das_preferencias(tela, cx).into_any_element(),
            _ => self.passo_da_memoria(window, cx).into_any_element(),
        };
        // O passo novo desliza do lado para onde se vai (sem crossfade): Continuar traz da direita,
        // Voltar da esquerda. O id leva o passo, para tocar a cada troca.
        let corpo = match tela.indo {
            Some(adiante) => {
                let de = if adiante { super::movimento::De::Direita } else { super::movimento::De::Esquerda };
                super::movimento::entrar(div().relative().child(corpo), ("bv-passo", passo), de).into_any_element()
            }
            None => corpo,
        };
        let botao_do_pe = |b: gpui_kit::component::button::Button| b.h(r(32.)).px(esp::E16).rounded(raio::de(32.)).text_size(r(13.));
        let pe = h_flex()
            .items_center()
            .gap(esp::E8)
            .pt(m::PE_Y)
            .px(m::PE_X)
            .pb(m::PE_X)
            .when(passo > 0, |s| {
                s.child(botao_do_pe(botao_fantasma("bv-voltar", None, tr!("Voltar"), TamanhoBotao::Controle, window, cx)).bg(Cores::cartao_elevado()).on_click(
                    cx.listener(move |this, _, _, cx| this.ir_ao_passo(passo.saturating_sub(1), cx)),
                ))
            })
            .child(div().flex_1())
            .when(sem_pasta && !tela.sugestoes.is_empty(), |s| s.child(div().text_size(r(12.)).text_color(Cores::apagado()).child(tr!("Escolha uma pasta"))))
            .child(
                botao_do_pe(botao_primario("bv-seguir", None, if ultimo { tr!("Começar") } else { tr!("Continuar") }, TamanhoBotao::Controle, window, cx))
                    .when(!pode_seguir, |b| b.opacity(0.45))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !pode_seguir {
                            return;
                        }
                        if ultimo {
                            this.concluir_boas_vindas(cx);
                        } else {
                            this.ir_ao_passo(passo + 1, cx);
                        }
                    })),
            );
        let cartao = v_flex()
            .id("bv-cartao")
            .w(r(m::LARGURA))
            .max_w_full()
            .max_h_full()
            .rounded(raio::de(44.))
            .bg(Cores::preto())
            .overflow_hidden()
            .child(h_flex().items_center().gap(esp::E12).pt(m::TOPO_Y).px(m::TOPO_X).child(logo).child(pilulas))
            .child(div().flex_1().min_h(r(340.)).pt(m::PALCO_Y).px(m::PALCO_X).pb(esp::E8).overflow_hidden().child(corpo))
            .child(pe);
        // O brilho de baixo (o `radial-gradient` do protótipo): o GPUI não tem degradê redondo, e uma
        // sombra grande e borrada de uma peça escondida embaixo da tela faz o mesmo efeito.
        let brilho = div()
            .absolute()
            .left(relative(0.15))
            .w(relative(0.7))
            .bottom(relative(-0.3))
            .h(relative(0.4))
            .rounded_full()
            .shadow(vec![BoxShadow::new(px(0.), px(0.), Cores::destaque().opacity(0.24)).blur_radius(px(240.)).spread_radius(px(40.))]);
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .bg(Cores::vidro())
            .child(brilho)
            .child(
                div()
                    .flex()
                    .w_full()
                    .p(esp::E8)
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
                    .child(super::movimento::entrar(cartao, "bv-cartao-entra", super::movimento::De::Baixo)),
            )
            .into_any_element()
    }

    /// Vai ao passo `i`, deslizando do lado para onde se vai.
    fn ir_ao_passo(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(tela) = &mut self.boas_vindas
            && i != tela.passo
            && i < PASSOS.len()
        {
            tela.indo = Some(i > tela.passo);
            tela.passo = i;
            if i == PASSO_DAS_IAS {
                self.conferir_as_ias(cx);
            }
            cx.notify();
        }
    }

    /// No passo das IAs, a cada 2 s, o que está instalado é lido de novo: o instalador roda no
    /// terminal, e a linha passa a "Instalado" sozinha quando ele termina.
    fn conferir_as_ias(&mut self, cx: &mut Context<Self>) {
        if let Some(motor) = &self.motor {
            motor.update(cx, |m, cx| m.reler_versao_claude(cx));
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_secs(2)).await;
                let seguir = this
                    .update(cx, |this, cx| {
                        let no_passo = this.boas_vindas.as_ref().is_some_and(|t| t.passo == PASSO_DAS_IAS);
                        if no_passo {
                            if let Some(motor) = &this.motor {
                                motor.update(cx, |m, cx| m.reler_versao_claude(cx));
                            }
                            cx.notify();
                        }
                        no_passo
                    })
                    .unwrap_or(false);
                if !seguir {
                    break;
                }
            }
        })
        .detach();
    }

    /// Passo 3: as IAs. Cada uma com a marca, se já está instalada e, quando falta, o Instalar (o
    /// instalador oficial no terminal). Nenhuma é obrigatória aqui: sem o Claude Code, o chat avisa
    /// e oferece o mesmo botão.
    fn passo_das_ias(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let versao_claude = self.motor.as_ref().and_then(|m| m.read(cx).versao_claude);
        let linhas = crate::modelo::Agente::TODOS.into_iter().enumerate().map(|(i, agente)| {
            let (instalada, texto) = match agente {
                crate::modelo::Agente::Claude => {
                    let instalada = versao_claude.is_some() || crate::claude::binario_claude().is_file();
                    let texto = match versao_claude {
                        Some((a, b, c)) => tr!("Instalado · {versao}", versao = format!("{a}.{b}.{c}")).to_string(),
                        None if instalada => tr!("Instalado").to_string(),
                        None => tr!("O principal: chats, planos e agentes").to_string(),
                    };
                    (instalada, texto)
                }
                crate::modelo::Agente::Codex => (agente.instalado(), if agente.instalado() { tr!("Instalado").to_string() } else { tr!("Da OpenAI, com a conta do ChatGPT").to_string() }),
                crate::modelo::Agente::Gemini => (agente.instalado(), if agente.instalado() { tr!("Instalado").to_string() } else { tr!("Do Google, com a conta Google").to_string() }),
                outro => (
                    outro.instalado(),
                    if outro.instalado() { tr!("Instalado").to_string() } else { crate::i18n::traduzir(crate::agente_acp::perfil(outro).map_or("", |p| p.descricao)).to_string() },
                ),
            };
            let acao = if instalada {
                h_flex()
                    .flex_none()
                    .gap(crate::tema::linear::CHAT_VAO)
                    .text_size(r(12.5))
                    .text_color(Cores::verde())
                    .child(super::componentes::icone(Lucide::Check, 14., window))
                    .child(tr!("Pronto"))
                    .into_any_element()
            } else {
                Self::botao_da_tela(("bv-instalar", i), Lucide::Download, tr!("Instalar"), window)
                    .on_click(move |_, _, _| super::uso_plano::instalar(agente))
                    .into_any_element()
            };
            let linha = h_flex()
                .items_center()
                .gap(esp::E12)
                .py(crate::tema::inicio::LINHA_Y)
                .px(crate::tema::inicio::LINHA_X)
                .rounded(raio::CARTAO)
                .bg(Cores::cartao_elevado())
                .child(div().flex_none().size(r(32.)).flex().items_center().justify_center().child(super::componentes::Marca::do_agente(agente).glifo(20.)))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_size(r(13.)).font_weight(FontWeight::SEMIBOLD).text_color(Cores::texto()).child(match agente {
                            crate::modelo::Agente::Claude => "Claude Code",
                            outro => outro.nome(),
                        }))
                        .child(div().text_size(r(12.5)).text_color(Cores::tenue()).child(texto)),
                )
                .child(acao);
            super::movimento::entrar_depois(linha, ("bv-ia", i), super::movimento::De::Baixo, std::time::Duration::from_millis(80 + 45 * i as u64))
        });
        v_flex()
            .gap(crate::tema::inicio::PASSO_VAO)
            .child(Self::cabeca_do_passo(false, tr!("Suas IAs"), tr!("Instale as que você quer usar. Dá para mudar depois em Configurações, Agentes.")))
            // Nove IAs desde 26/11/2026: a lista rola dentro do cartão.
            .child(div().id("bv-ias").max_h(r(320.)).overflow_y_scroll().child(v_flex().gap(esp::E4).children(linhas)))
    }

    /// O título do passo, a frase de baixo e o conteúdo.
    fn cabeca_do_passo(primeiro: bool, titulo: SharedString, frase: SharedString) -> Div {
        v_flex()
            .gap(esp::E6)
            .child(
                div()
                    .text_size(r(if primeiro { 27. } else { 17. }))
                    .line_height(r(if primeiro { 34. } else { 22. }))
                    .font_weight(FontWeight(if primeiro { 650. } else { 620. }))
                    .text_color(Cores::texto())
                    .child(titulo),
            )
            .child(div().mb(esp::E4).text_size(r(13.5)).text_color(Cores::tenue()).child(frase))
    }

    /// Uma linha da apresentação e da memória: o ícone no azul e o título com a frase embaixo. Entra
    /// um pouco depois da de cima.
    fn linha_de_apresentacao(i: usize, icone: Lucide, titulo: SharedString, frase: SharedString) -> impl IntoElement {
        let linha = h_flex()
            .items_center()
            .gap(esp::E12)
            .py(crate::tema::inicio::LINHA_Y)
            .px(crate::tema::inicio::LINHA_X)
            .rounded(raio::CARTAO)
            .bg(Cores::cartao_elevado())
            .child(
                div()
                    .flex_none()
                    .size(r(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(raio::de(28.))
                    .bg(Cores::destaque().opacity(0.16))
                    .child(super::mensagens::icone_de(icone, 16.).text_color(Cores::destaque_texto())),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .child(div().text_size(r(13.)).font_weight(FontWeight::SEMIBOLD).text_color(Cores::texto()).child(titulo))
                    .child(div().text_size(r(12.5)).text_color(Cores::tenue()).child(frase)),
            );
        super::movimento::entrar_depois(linha, ("bv-linha", i), super::movimento::De::Baixo, std::time::Duration::from_millis(80 + 45 * i as u64))
    }

    /// Passo 1: o que é o Vitral, em quatro linhas, e a língua da interface (que vale na hora).
    fn passo_da_apresentacao(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let atual = crate::i18n::lingua();
        let motor = self.motor.clone();
        let linguas = h_flex().gap(esp::E4).children(crate::i18n::Lingua::TODAS.into_iter().map(move |lingua| {
            let motor = motor.clone();
            let escolhida = lingua == atual;
            div()
                .id(("bv-lingua", lingua as usize))
                .h(r(26.))
                .px(crate::tema::inicio::BOTAO_X)
                .flex()
                .items_center()
                .rounded(raio::de(26.))
                .text_size(r(12.5))
                .when(escolhida, |d| d.bg(Cores::selecao()).text_color(Cores::texto()))
                .when(!escolhida, |d| d.bg(Cores::cartao_elevado()).text_color(Cores::tenue()).cursor_pointer())
                .child(lingua.nome())
                .on_click(move |_, _, cx| {
                    if let Some(motor) = &motor {
                        motor.update(cx, |m, cx| m.mudar_config(|c| c.lingua = lingua.codigo().to_string(), cx));
                    }
                })
        }));
        let _ = cx;
        v_flex()
            .gap(crate::tema::inicio::PASSO_VAO)
            .child(linguas)
            .child(Self::cabeca_do_passo(true, tr!("Bem-vindo ao Vitral"), tr!("Chats que trabalham direto nas suas pastas.")))
            .child(
                v_flex()
                    .gap(esp::E4)
                    .child(Self::linha_de_apresentacao(0, Lucide::MessagesSquare, tr!("Uma lista de chats"), tr!("Cada chat numa pasta. Claude, Codex ou Gemini.")))
                    .child(Self::linha_de_apresentacao(1, Lucide::Columns2, tr!("Vários lado a lado"), tr!("Abas e conversas lado a lado.")))
                    .child(Self::linha_de_apresentacao(2, Lucide::ShieldCheck, tr!("Você decide o que roda"), tr!("Cada permissão aparece na tela.")))
                    .child(Self::linha_de_apresentacao(3, Lucide::Brain, tr!("O que se aprende fica"), tr!("O agente guarda o que vale lembrar."))),
            )
    }

    /// O botão claro de uma linha só da tela (Outra pasta…, Ligar a memória…).
    fn botao_da_tela(id: impl Into<ElementId>, icone: Lucide, rotulo: SharedString, window: &Window) -> Stateful<Div> {
        h_flex()
            .id(id)
            .flex_none()
            .items_center()
            .gap(crate::tema::linear::CHAT_VAO)
            .h(r(30.))
            .px(crate::tema::inicio::BOTAO_X)
            .rounded(raio::de(30.))
            .bg(Cores::cartao_elevado())
            .text_size(r(12.5))
            .text_color(Cores::texto_da_fala())
            .cursor_pointer()
            .hover(|s| s.text_color(Cores::texto()))
            .child(super::componentes::icone(icone, 15., window))
            .child(rotulo)
    }

    /// Passo 4: a memória da equipe, que é opcional. O botão abre o mesmo diálogo das
    /// Configurações; ligada, o passo mostra onde.
    fn passo_da_memoria(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let endereco = self.motor.as_ref().map(|m| m.read(cx).config.memoria_da_equipe.clone()).unwrap_or_default();
        let ligada = self.motor.as_ref().is_some_and(|m| {
            !endereco.is_empty() && crate::memoria_equipe::ler_chave(&m.read(cx).dados.config).is_some()
        });
        let acao = if ligada {
            h_flex()
                .gap(crate::tema::linear::CHAT_VAO)
                .text_size(r(12.5))
                .text_color(Cores::verde())
                .child(super::componentes::icone(Lucide::Check, 14., window))
                .child(tr!("Ligada a {endereco}", endereco = endereco))
                .into_any_element()
        } else {
            Self::botao_da_tela("bv-memoria", Lucide::Brain, tr!("Ligar a memória…"), window)
                .on_click(cx.listener(|this, _, window, cx| {
                    if let Some(motor) = this.motor.clone() {
                        super::memoria_equipe::abrir(motor, window, cx);
                    }
                }))
                .into_any_element()
        };
        v_flex()
            .gap(crate::tema::inicio::PASSO_VAO)
            .items_start()
            .child(Self::cabeca_do_passo(false, tr!("Memória da equipe"), tr!("Opcional. Dá para ligar depois.")))
            .child(
                v_flex()
                    .w_full()
                    .gap(esp::E4)
                    .child(Self::linha_de_apresentacao(0, Lucide::MessagesSquare, tr!("Todos com o mesmo contexto"), tr!("Decisões e armadilhas num lugar só.")))
                    .child(Self::linha_de_apresentacao(1, Lucide::ShieldCheck, tr!("Só o que importa sobe"), tr!("O histórico do chat não vai."))),
            )
            .child(acao)
    }

    /// Passo 2: a pasta de trabalho. As sugestões são as pastas em que o Claude Code já tem
    /// conversas; a escolhida fica com a borda azul e o certinho, e o botão abre o seletor do
    /// sistema.
    fn passo_da_pasta(&self, tela: &BoasVindas, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let escolhida = tela.pasta.clone();
        // A escolhida que não está entre as sugestões (veio do seletor) entra no alto da lista.
        let mut linhas: Vec<(PathBuf, Option<usize>)> = tela.sugestoes.iter().map(|(p, n)| (p.clone(), Some(*n))).collect();
        if let Some(pasta) = &escolhida
            && !linhas.iter().any(|(p, _)| p == pasta)
        {
            linhas.insert(0, (pasta.clone(), None));
        }
        let procurando = tela.procurando && linhas.is_empty();
        let lista_vazia = linhas.is_empty();
        let lista = v_flex().gap(esp::E4).children(linhas.into_iter().enumerate().map(|(i, (pasta, conversas))| {
            let marcada = escolhida.as_ref() == Some(&pasta);
            let alvo = pasta.clone();
            let linha = h_flex()
                .id(("bv-pasta", i))
                .h(r(46.))
                .px(esp::E12)
                .gap(crate::tema::inicio::PASTA_VAO)
                .rounded(raio::CARTAO)
                .border(px(1.5))
                .cursor_pointer()
                .map(|s| {
                    if marcada {
                        s.border_color(Cores::destaque()).bg(Cores::cartao_elevado().blend(Cores::destaque().opacity(0.12)))
                    } else {
                        s.border_color(gpui_kit::transparent_black()).bg(Cores::cartao_elevado()).hover(|s| s.bg(Cores::cartao_elevado().blend(Cores::forte().opacity(0.04))))
                    }
                })
                .child(super::componentes::icone(Lucide::Folder, 16., window).text_color(Cores::tenue()))
                .child(div().min_w_0().truncate().font_family(FONTE_MONO).text_size(r(13.)).font_weight(FontWeight::MEDIUM).text_color(Cores::texto()).child(com_til(&pasta)))
                .child(div().flex_1())
                .children(conversas.map(|n| {
                    div().flex_none().whitespace_nowrap().text_size(r(12.)).text_color(Cores::apagado()).child(if n == 1 { tr!("1 chat") } else { tr!("{n} chats", n = n) })
                }))
                .when(marcada, |s| {
                    s.child(
                        div()
                            .flex_none()
                            .size(r(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(Cores::destaque())
                            .text_color(Cores::forte())
                            .child(super::componentes::icone(Lucide::Check, 11., window)),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(tela) = &mut this.boas_vindas {
                        tela.pasta = Some(alvo.clone());
                        cx.notify();
                    }
                }));
            super::movimento::entrar_depois(linha, ("bv-pasta-entra", i), super::movimento::De::Baixo, std::time::Duration::from_millis(80 + 45 * i as u64))
        }));
        v_flex()
            .gap(crate::tema::inicio::PASSO_VAO)
            .items_start()
            .child(Self::cabeca_do_passo(false, tr!("Em qual pasta você trabalha?"), tr!("Os chats novos nascem nela.")))
            .when(procurando, |s| {
                s.child(
                    h_flex()
                        .gap(esp::E8)
                        .text_size(r(12.5))
                        .text_color(Cores::apagado())
                        .child(super::componentes::AnelGirando::new(14.).cor(Cores::destaque()))
                        .child(tr!("Procurando suas pastas…")),
                )
            })
            .when(!lista_vazia, |s| s.child(lista.w_full()))
            .child(Self::botao_da_tela("bv-outra-pasta", Lucide::FolderSearch, tr!("Outra pasta…"), window).on_click(cx.listener(|this, _, window, cx| this.escolher_pasta_de_trabalho(window, cx))))
    }

    /// Passo 3: as preferências básicas (tudo muda depois, nas Configurações ou na caixa de cada
    /// chat), cada uma num segmentado no cinza do cartão.
    fn passo_das_preferencias(&self, tela: &BoasVindas, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let janela = cx.entity().downgrade();
        let segmentado = |id: &'static str, opcoes: Vec<(SharedString, Option<AnyElement>)>, escolhida: usize, escolher: Box<dyn Fn(usize, &mut App)>| {
            let escolher = std::rc::Rc::new(escolher);
            h_flex()
                .flex_none()
                .gap(esp::E2)
                .p(esp::E2)
                .h(r(32.))
                .rounded(raio::de(32.))
                .bg(Cores::cartao_elevado())
                .children(opcoes.into_iter().enumerate().map(move |(i, (nome, simbolo))| {
                    let escolher = escolher.clone();
                    h_flex()
                        .id((id, i))
                        .items_center()
                        .gap(crate::tema::linear::CHAT_VAO)
                        .h(r(28.))
                        .px(crate::tema::inicio::BOTAO_X)
                        .rounded(raio::de(28.))
                        .whitespace_nowrap()
                        .text_size(r(12.5))
                        .map(|s| if i == escolhida { s.bg(Cores::selecao()).text_color(Cores::texto()) } else { s.text_color(Cores::tenue()).cursor_pointer() })
                        .children(simbolo)
                        .child(nome)
                        .on_click(move |_, _, cx| escolher(i, cx))
                }))
        };
        let preferencia = |titulo: SharedString, controle: AnyElement, frase: Option<String>| {
            v_flex()
                // O segmentado na largura dele, não na do cartão.
                .items_start()
                .gap(esp::E6)
                .child(div().text_size(r(13.)).font_weight(FontWeight::SEMIBOLD).text_color(Cores::texto()).child(titulo))
                .child(controle)
                .children(frase.map(|f| div().min_h(r(17.)).text_size(r(12.5)).text_color(Cores::tenue()).child(f)))
        };
        let mudar = |f: fn(&mut BoasVindas, usize)| {
            let janela = janela.clone();
            Box::new(move |i: usize, cx: &mut App| {
                let _ = janela.update(cx, |this, cx| {
                    if let Some(tela) = &mut this.boas_vindas {
                        f(tela, i);
                        cx.notify();
                    }
                });
            }) as Box<dyn Fn(usize, &mut App)>
        };
        let modo = tela.modo.min(MODOS.len() - 1);
        let agentes = crate::modelo::Agente::instalados();
        let agente = tela.agente.min(agentes.len().saturating_sub(1));
        // Os modelos do protótipo: Opus, Sonnet e Haiku (o Padrão saiu do seletor em 25/11).
        let modelos: Vec<usize> = MODELOS.iter().enumerate().filter(|(_, (v, _))| matches!(*v, "opus" | "sonnet" | "haiku")).map(|(i, _)| i).collect();
        let modelo = modelos.iter().position(|i| *i == tela.modelo).unwrap_or(0);
        let importar = tela.importar;
        v_flex()
            .gap(esp::E16)
            .child(preferencia(
                tr!("Permissões"),
                segmentado("bv-modo", MODOS.iter().map(|(_, nome, _)| (SharedString::from(crate::i18n::traduzir(*nome)), None)).collect(), modo, mudar(|t, i| t.modo = i)).into_any_element(),
                Some(crate::i18n::traduzir(MODOS[modo].2).to_string()),
            ))
            .when(agentes.len() > 1, |s| {
                s.child(preferencia(
                    tr!("Quem responde"),
                    segmentado(
                        "bv-agente",
                        agentes.iter().map(|a| (SharedString::from(a.nome()), Some(super::componentes::Marca::do_agente(*a).glifo(15.)))).collect(),
                        agente,
                        mudar(|t, i| t.agente = i),
                    )
                    .into_any_element(),
                    Some(tr!("Cada chat pode trocar.").to_string()),
                ))
            })
            .child(preferencia(
                tr!("Modelo do Claude"),
                segmentado(
                    "bv-modelo",
                    modelos.iter().map(|i| (SharedString::from(MODELOS[*i].1), None)).collect(),
                    modelo,
                    {
                        let modelos = modelos.clone();
                        let janela = janela.clone();
                        Box::new(move |i: usize, cx: &mut App| {
                            let indice = modelos[i];
                            let _ = janela.update(cx, |this, cx| {
                                if let Some(tela) = &mut this.boas_vindas {
                                    tela.modelo = indice;
                                    cx.notify();
                                }
                            });
                        })
                    },
                )
                .into_any_element(),
                None,
            ))
            // As chaves que valem para todos os chats, num cartão só (26/11/2026, dono: "coloca mais
            // configurações importantes na parte de início").
            .child(
                v_flex()
                    .rounded(raio::CARTAO)
                    .bg(Cores::cartao_elevado())
                    .child(Self::linha_de_chave(&janela, "bv-importar", tr!("Trazer as conversas que já existem"), tr!("Do Claude Code, nesta pasta."), importar, |t, v| t.importar = v))
                    .child(Self::linha_de_chave(&janela, "bv-revisar", tr!("Revisão automática"), tr!("O outro agente confere o que mudou, só lendo."), tela.revisar, |t, v| t.revisar = v))
                    .child(Self::linha_de_chave(&janela, "bv-sugerir", tr!("Sugerir a próxima mensagem"), tr!("Aparece apagada na caixa; o Tab aceita."), tela.sugerir, |t, v| t.sugerir = v))
                    .child(Self::linha_de_chave(&janela, "bv-avisos", tr!("Avisar quando um chat precisa de você"), tr!("Um aviso no canto, dentro do app."), tela.avisos, |t, v| t.avisos = v)),
            )
    }
}


impl Janela {
    /// Uma chave do cartão de Preferências: o título, a frase e o interruptor, que muda a tela
    /// (a escolha só vai para a configuração no "Começar").
    fn linha_de_chave(
        janela: &WeakEntity<Janela>,
        id: &'static str,
        titulo: SharedString,
        frase: SharedString,
        ligado: bool,
        mudar: fn(&mut BoasVindas, bool),
    ) -> impl IntoElement + use<> {
        let janela = janela.clone();
        h_flex()
            .items_center()
            .gap(esp::E12)
            .py(esp::E8)
            .px(esp::E12)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_size(r(13.)).font_weight(FontWeight::SEMIBOLD).text_color(Cores::texto()).child(titulo))
                    .child(div().text_size(r(12.5)).text_color(Cores::tenue()).child(frase)),
            )
            .child(Switch::new(id).checked(ligado).on_click(move |novo, _, cx| {
                let novo = *novo;
                let _ = janela.update(cx, |this, cx| {
                    if let Some(tela) = &mut this.boas_vindas {
                        mudar(tela, novo);
                        cx.notify();
                    }
                });
            }))
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
