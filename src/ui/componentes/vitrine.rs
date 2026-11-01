//! `vitral --vitrine`: uma janela com cada componente em todos os estados, sobre as quatro
//! superfícies (vidro, painel, flutuante e caixa quente), com dados fixos e sem motor. Serve
//! para conferir a linguagem na bancada, inclusive com zoom.

use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, Size, StyledExt as _,
    TitleBar, WindowExt as _,
    button::Button,
    input::InputState,
    menu::{DropdownMenu as _, PopupMenuItem},
    notification::Notification,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::*;
use crate::tema::{
    CORES_PROJETO, Cores, Lucide, Tipo as _, altura, dentro_da_borda, esp, gradiente_destaque, gradiente_quente,
    halo_foco, halo_suave, linha_de_luz,
    r, raio, sombra_dialogo, sombra_elevado, sombra_flutuante, sombra_recuo, tam,
};
use crate::atualizacao::saude::Aviso;
use crate::ui::atualizacao as atualizacao_ui;
use crate::ui::fechar::{Parar, confirmacao_de_parar};
use crate::ui::icone_projeto::ICONES;
use crate::ui::movimento::{self, Curva, De};
use crate::ui::{NovaConversa, zoom};

/// Abre a janela da vitrine (no lugar da janela do app).
pub fn abrir(cx: &mut App) {
    let opcoes = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1440.), px(960.)), cx)),
        window_min_size: Some(size(px(960.), px(600.))),
        window_background: WindowBackgroundAppearance::Transparent,
        window_decorations: Some(WindowDecorations::Client),
        app_id: Some("vitral".into()),
        ..TitleBar::window_options()
    };
    gpui_kit::open_window(opcoes, cx, |window, cx| {
        window.set_window_title("Vitral: vitrine");
        cx.new(|cx| Vitrine::new(window, cx))
    })
    .expect("não abriu a janela da vitrine");
    crate::ui::iniciar_contador_de_quadros(cx);
    cx.on_window_closed(|cx, _| cx.quit()).detach();
    cx.activate(true);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fundo {
    Vidro,
    Painel,
    Flutuante,
    Quente,
}

const FUNDOS: [Fundo; 4] = [Fundo::Vidro, Fundo::Painel, Fundo::Flutuante, Fundo::Quente];

impl Fundo {
    fn nome(self) -> &'static str {
        match self {
            Fundo::Vidro => "vidro",
            Fundo::Painel => "painel",
            Fundo::Flutuante => "flutuante",
            Fundo::Quente => "caixa quente",
        }
    }
}

struct Vitrine {
    segmento: usize,
    esforco: usize,
    /// Movimento: quantas vezes "Entrar de novo" e "Enviar" foram clicados, e os alternadores.
    entrada: u64,
    pulso: u64,
    vazio: bool,
    fracao_alta: bool,
    focada: bool,
    girando: bool,
    contador: u32,
    ligado: bool,
    ultrathink: bool,
    dias: [bool; 7],
    busca: Entity<InputState>,
    campo: Entity<InputState>,
    lista: Entity<ListaDeEscolha>,
    previa: Entity<PreviaArrasto>,
    escolhido: SharedString,
    rolagem: ScrollHandle,
    _escolha: Subscription,
}

impl Vitrine {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let busca = cx.new(|cx| InputState::new(window, cx).placeholder("Buscar conversas"));
        let campo = cx.new(|cx| InputState::new(window, cx).placeholder("loja-api"));
        let lista = cx.new(|cx| {
            let mut lista = ListaDeEscolha::new("Buscar modelo…", window, cx);
            lista.definir(modelos(), Some("opus".into()), cx);
            // O logo só no rótulo do grupo; os itens, sem símbolo.
            lista.simbolo_do_grupo("Claude", Simbolo::Marca(Marca::Claude));
            lista.simbolo_do_grupo("Codex", Simbolo::Marca(Marca::Codex));
            // Busca própria: o que começa com o texto vem antes do que só o contém.
            lista.filtro_proprio(|busca| {
                let busca = busca.to_lowercase();
                modelos()
                    .into_iter()
                    .map(|mut grupo| {
                        grupo.itens.retain(|i| i.titulo.to_lowercase().contains(&busca));
                        grupo.itens.sort_by_key(|i| !i.titulo.to_lowercase().starts_with(&busca));
                        grupo
                    })
                    .filter(|grupo| !grupo.itens.is_empty())
                    .collect()
            });
            lista.rodape(|_, _| {
                div()
                    .px(r(16.))
                    .py(r(8.))
                    .metadado()
                    .text_color(Cores::tenue())
                    .child("Cada chat ainda pode trocar")
                    .into_any_element()
            });
            lista
        });
        let previa = cx.new(|_| PreviaArrasto {
            titulo: "Cupons de desconto na API".into(),
            simbolo: Simbolo::Papel(Papel::Coordenador),
        });
        let _escolha = cx.subscribe(&lista, |vitrine, _, evento, cx| {
            vitrine.escolhido = match evento {
                EventoEscolha::Escolheu(chave) => format!("escolheu: {chave}").into(),
                EventoEscolha::Fechou => "fechou".into(),
            };
            cx.notify();
        });
        Vitrine {
            segmento: 0,
            esforco: 2,
            entrada: 0,
            pulso: 0,
            vazio: true,
            fracao_alta: false,
            focada: false,
            girando: false,
            contador: 4,
            ligado: true,
            ultrathink: false,
            dias: [false, true, true, true, true, true, false],
            busca,
            campo,
            lista,
            previa,
            escolhido: "nada escolhido ainda".into(),
            rolagem: ScrollHandle::new(),
            _escolha,
        }
    }
}

fn modelos() -> Vec<GrupoDeEscolha> {
    let capacidades = || {
        Selo::capacidades(vec![
            (Lucide::Brain.into(), Cores::violeta(), "raciocínio"),
            (Lucide::Image.into(), Cores::ciano(), "lê imagens"),
        ])
    };
    let item = |chave: &str, titulo: &str, descricao: &str| ItemDeEscolha {
        descricao: Some(descricao.to_string().into()),
        ..ItemDeEscolha::new(chave.to_string(), titulo.to_string())
    };
    vec![
        GrupoDeEscolha {
            titulo: Some("Claude".into()),
            itens: vec![
                ItemDeEscolha { selos: vec![capacidades()], ..item("opus", "Opus 5.5", "1M de contexto") },
                ItemDeEscolha {
                    selos: vec![Selo::cor("rápido", Cores::verde()).icone(Lucide::Zap, Cores::verde())],
                    ..item("sonnet", "Sonnet 5", "O equilíbrio para o dia a dia")
                },
                item("haiku", "Haiku 4.5", "O mais rápido"),
            ],
        },
        GrupoDeEscolha {
            titulo: Some("Codex".into()),
            itens: vec![ItemDeEscolha {
                desabilitado: Some("Instale o Codex para usar".into()),
                ..ItemDeEscolha::new("codex", "GPT-5 Codex")
            }],
        },
    ]
}

/// Título de seção da vitrine.
fn secao(titulo: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(r(12.))
        .child(div().titulo_tela().text_color(Cores::forte()).child(titulo))
}

fn superficie(fundo: Fundo, cx: &App) -> Div {
    let base = div()
        .flex()
        .flex_col()
        .gap(r(12.))
        .p(r(16.))
        .min_w(r(320.))
        .flex_1()
        .rounded(raio::CARTAO);
    match fundo {
        Fundo::Vidro => base.border_1().border_color(Cores::linha()),
        Fundo::Painel => base.bg(Cores::cartao()),
        Fundo::Flutuante => base.flutuante(cx).rounded(raio::PECA),
        Fundo::Quente => base.bg(gradiente_quente()).border_1().border_color(Cores::borda()),
    }
}

/// O mesmo conteúdo sobre as quatro superfícies.
fn quatro(
    id: &'static str,
    titulo: &'static str,
    window: &mut Window,
    cx: &mut App,
    f: impl Fn(Fundo, &mut Window, &mut App) -> AnyElement,
) -> Div {
    let mut linha = div().flex().flex_wrap().gap(r(12.));
    for fundo in FUNDOS {
        let conteudo = f(fundo, window, cx);
        linha = linha.child(
            superficie(fundo, cx)
                .id((id, fundo as usize))
                .child(div().metadado().text_color(Cores::tenue()).child(fundo.nome()))
                .child(conteudo),
        );
    }
    secao(titulo).child(linha)
}

/// Só sobre o painel (peças grandes).
fn um(titulo: &'static str, cx: &App, conteudo: impl IntoElement) -> Div {
    secao(titulo).child(superficie(Fundo::Painel, cx).child(conteudo))
}

fn fileira() -> Div {
    div().flex().flex_wrap().items_center().gap(r(8.))
}

fn legenda(texto: impl Into<SharedString>) -> Div {
    div().metadado().text_color(Cores::tenue()).child(texto.into())
}

impl Vitrine {
    fn tipografia(&self, window: &mut Window, cx: &mut App) -> Div {
        quatro("tipo", "Texto e cores", window, cx, |_, _, _| {
            let niveis = [
                ("forte", Cores::forte()),
                ("texto", Cores::texto()),
                ("tenue", Cores::tenue()),
                ("apagado", Cores::apagado()),
                ("fantasma (decoração)", Cores::fantasma()),
            ];
            let cores = [
                ("destaque_texto", Cores::destaque_texto()),
                ("ciano", Cores::ciano()),
                ("verde", Cores::verde()),
                ("âmbar", Cores::ambar()),
                ("vermelho", Cores::vermelho()),
                ("violeta", Cores::violeta()),
                ("codigo", Cores::codigo()),
            ];
            div()
                .flex()
                .flex_col()
                .gap(r(4.))
                .child(div().titulo_tela().child("Título de tela 18/600"))
                .child(div().titulo_cartao().child("Título de cartão 15/600"))
                .child(div().rotulo_secao().child("Rótulo de seção 12/600"))
                .child(div().corpo().child("Corpo 15/23 da conversa"))
                .child(div().corpo_ui().child("Corpo da interface 14/20"))
                .child(div().rotulo().child("Rótulo 13/500"))
                .child(div().metadado().child("Metadado 12/16"))
                .child(div().micro().child("Micro 11/600"))
                .child(div().mono().child("mono 13/20: cargo test"))
                .child(div().numero().child("42%"))
                .children(niveis.map(|(nome, cor)| div().metadado().text_color(cor).child(nome)))
                .child(fileira().children(cores.map(|(nome, cor)| div().metadado().text_color(cor).child(nome))))
                .into_any_element()
        })
    }

    fn chips(&self, window: &mut Window, cx: &mut App) -> Div {
        quatro("chips", "Chip", window, cx, |fundo, window, cx| {
            let id = |nome: &str| SharedString::from(format!("{nome}-{}", fundo as usize));
            let modo = |valor: &str, sem_copia: bool| {
                let (icone, cor) = icone_do_modo(valor, sem_copia);
                Simbolo::Icone(icone, Some(cor))
            };
            div()
                .flex()
                .flex_col()
                .gap(r(8.))
                .child(fileira()
                    .child(Chip::new(id("padrao"), "Opus 5.5").simbolo(Simbolo::Marca(Marca::Claude)).mudado(false).seta(true).construir(window, cx))
                    .child(Chip::new(id("mudado"), "Opus 5.5").simbolo(Simbolo::Marca(Marca::Claude)).seta(true).construir(window, cx))
                    .child(Chip::new(id("hover"), "hover").seta(true).construir(window, cx).bg(Cores::hover()))
                    .child(Chip::new(id("aberto"), "aberto").seta(true).aberto(true).construir(window, cx))
                    .child(Chip::new(id("foco"), "foco").seta(true).construir(window, cx).border_color(Cores::destaque()).shadow(halo_foco())))
                .child(fileira()
                    .child(Chip::new(id("esforco"), "Médio").simbolo(Simbolo::Sinal { nivel: 3, total: 5 }).cor_rotulo(Cores::ciano()).seta(true).construir(window, cx))
                    .child(Chip::new(id("modo"), "Sem perguntar").simbolo(modo("bypassPermissions", false)).seta(true).construir(window, cx))
                    .child(Chip::new(id("alerta"), "Sem perguntar").simbolo(modo("bypassPermissions", true)).alerta(Cores::vermelho()).seta(true).construir(window, cx))
                    .child(Chip::new(id("icone"), "Modelo").simbolo(Simbolo::Marca(Marca::Claude)).so_icone(true).dica("Modelo: Opus 5.5").construir(window, cx))
                    .child(Chip::new(id("desab"), "Conta").seta(true).desabilitado("Pare a conversa para trocar de conta").construir(window, cx)))
                .child(fileira()
                    .child(Chip::new(id("pequeno"), "Pequeno").tamanho(TamanhoChip::Pequeno).construir(window, cx))
                    .child(Chip::new(id("medio"), "Médio").tamanho(TamanhoChip::Medio).construir(window, cx))
                    .child(Chip::new(id("contexto"), "30%").simbolo(Simbolo::Anel { fracao: 0.3, cor: None }).tamanho(TamanhoChip::Medio).construir(window, cx))
                    .child(Chip::new(id("atalho"), "Nova").simbolo(Simbolo::icone_cor(Lucide::MessageSquarePlus, Cores::destaque_texto())).dica_acao("Nova conversa", Box::new(NovaConversa), None).construir(window, cx))
                    .child(Chip::new(id("seg"), "Seg").alternavel(true).tamanho(TamanhoChip::Medio).construir(window, cx))
                    .child(Chip::new(id("dom"), "Dom").alternavel(false).tamanho(TamanhoChip::Medio).construir(window, cx)))
                .into_any_element()
        })
    }

    fn botoes(&self, window: &mut Window, cx: &mut App) -> Div {
        quatro("botoes", "Botões", window, cx, |fundo, window, cx| {
            let id = |nome: &str| SharedString::from(format!("{nome}-{}", fundo as usize));
            div()
                .flex()
                .flex_col()
                .gap(r(8.))
                .child(fileira()
                    .child(botao_fantasma(id("f24"), Some(Lucide::Eye.into()), "Revisar", TamanhoBotao::Linha, window, cx))
                    .child(botao_fantasma(id("f28"), Some(Lucide::CodeXml.into()), "Abrir no VS Code", TamanhoBotao::Barra, window, cx))
                    .child(botao_fantasma(id("f32"), None, "Mestre", TamanhoBotao::Controle, window, cx))
                    .child(botao_fantasma(id("fsel"), Some(Lucide::Focus.into()), "Modo foco", TamanhoBotao::Barra, window, cx).selected(true))
                    .child(botao_fantasma(id("fdes"), Some(Lucide::Play.into()), "Retomar", TamanhoBotao::Barra, window, cx).disabled(true).tooltip("Já está rodando")))
                .child(fileira()
                    .child(botao_icone(id("i24"), Lucide::Columns2, "Abrir ao lado", TamanhoBotao::Linha, window, cx))
                    .child(botao_icone(id("i28"), Lucide::PanelRight, "Painel da direita", TamanhoBotao::Barra, window, cx))
                    .child(botao_icone(id("i28h"), Lucide::Ellipsis, "Mais", TamanhoBotao::Barra, window, cx).bg(Cores::hover()))
                    .child(botao_icone(id("i28s"), Lucide::PanelRight, "Painel da direita", TamanhoBotao::Barra, window, cx).selected(true))
                    .child(botao_perigo(id("perigo"), Some(Lucide::Trash.into()), "Apagar a cópia…", TamanhoBotao::Barra, window, cx))
                    .child(botao_primario(id("prim"), Some(Lucide::GitMerge.into()), "Juntar", TamanhoBotao::Controle, window, cx)))
                .child(fileira()
                    .child(BotaoRedondo::new(id("r-prim"), Some(Lucide::Plus.into()), "Nova conversa").variante(VarianteRedonda::Primario).diametro(28.))
                    .child(BotaoRedondo::new(id("r-claro"), Some(Lucide::ArrowUp.into()), "Enviar").variante(VarianteRedonda::Claro))
                    .child(BotaoRedondo::new(id("r-vazio"), Some(Lucide::ArrowUp.into()), "Escreva algo para enviar").variante(VarianteRedonda::Claro).vazio(true))
                    .child(BotaoRedondo::new(id("r-parar"), None, "Parar").variante(VarianteRedonda::Claro))
                    .child(BotaoRedondo::new(id("r-neutro"), Some(Lucide::Bell.into()), "O que precisa de você").diametro(36.).selo(Selo::contagem(3).solida(Cores::ambar())))
                    .child(BotaoRedondo::new(id("r-plano"), Some(Lucide::Paperclip.into()), "Anexar").variante(VarianteRedonda::Plano).diametro(28.))
                    .child(BotaoRedondo::new(id("r-sel"), Some(Lucide::Paperclip.into()), "Anexar").variante(VarianteRedonda::Neutro).diametro(28.).selected(true))
                    .child(BotaoRedondo::new(id("r-perigo"), Some(Lucide::Unplug.into()), "Desconectar").variante(VarianteRedonda::Perigo).diametro(28.))
                    .child(BotaoRedondo::new(id("r-desab"), Some(Lucide::ArrowUp.into()), "Sem conexão").variante(VarianteRedonda::Claro).disabled(true)))
                .into_any_element()
        })
    }

    /// Cada efeito do orçamento de movimento, com o botão que o dispara.
    fn movimento(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let eu = cx.entity().downgrade();
        let mudar = move |f: fn(&mut Vitrine)| {
            let eu = eu.clone();
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                let _ = eu.update(cx, |v, cx| {
                    f(v);
                    cx.notify();
                });
            }
        };
        let botao = |id: &'static str, rotulo: &'static str, window: &Window, cx: &App| {
            botao_fantasma(id, None, rotulo, TamanhoBotao::Barra, window, cx)
        };
        let caixa = |nome: &'static str| {
            div()
                .h(r(40.))
                .px(esp::CAIXA)
                .flex()
                .items_center()
                .rounded(raio::CARTAO)
                .bg(Cores::cartao_elevado())
                .border_1()
                .border_color(Cores::linha())
                .reflexo(raio::CARTAO)
                .corpo_ui()
                .child(nome)
        };
        let n = self.entrada as usize;
        let entradas = fileira()
            .child(botao("mov-entrar", "Entrar de novo", window, cx).on_click(mudar(|v| v.entrada += 1)))
            .child(movimento::entrar(caixa("de baixo: mensagem nova"), ("mov-baixo", n), De::Baixo))
            .child(movimento::entrar(caixa("de cima: o que abre embaixo"), ("mov-cima", n), De::Cima))
            .child(movimento::entrar(caixa("parado: painel ao lado"), ("mov-parado", n), De::Parado))
            .child(
                div().with_animation(("mov-barra", n), movimento::animacao(movimento::ENTRAR, Curva::Entrar), |d, t| {
                    d.w(r(120. * t)).h(r(4.)).rounded_full().bg(Cores::destaque())
                }),
            );
        let ponteiro = movimento::Hover::de("mov-quadrado-hover", window, cx);
        let lugar = div()
            .id("mov-quadrado")
            .size(r(40.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(raio::CARTAO)
            .cursor_pointer()
            .on_hover(ponteiro.ouvinte())
            .child(
                QuadradoDeProjeto::new(&ICONES[3], CORES_PROJETO[2].0, 32.)
                    .realcado(ponteiro.ativo(cx) || self.focada)
                    .ativo(self.focada)
                    .suave("mov-quadrado-suave"),
            );
        let controles = fileira()
            .child(
                BotaoRedondo::new("mov-enviar", Some(Lucide::ArrowUp.into()), "Enviar (pulsa)")
                    .variante(VarianteRedonda::Claro)
                    .vazio(self.vazio)
                    .pulso(self.pulso)
                    .on_click(mudar(|v| v.pulso += 1)),
            )
            .child(botao("mov-vazio", "Com texto / vazia", window, cx).on_click(mudar(|v| v.vazio = !v.vazio)))
            .child(BotaoRedondo::new("mov-mais", Some(Lucide::Plus.into()), "Hover suave").variante(VarianteRedonda::Primario))
            .child(BotaoRedondo::new("mov-sino", Some(Lucide::Bell.into()), "Hover suave"))
            .child(lugar)
            .child(legenda("hover suave só em controle solto (120 ms); nunca em lista, menu ou cartão"));
        let fracao = if self.fracao_alta { 0.86 } else { 0.32 };
        let aneis = fileira()
            .child(Anel::new("mov-anel", fracao).diametro(36.).centro(div().micro().child(format!("{:.0}", fracao * 100.))))
            .child(Anel::new(("mov-anel-enche", n), 0.64).diametro(36.).encher())
            .child(AnelDeUso::new(("mov-uso-enche", n), Marca::Claude, Some(0.42)).semana(Some(0.7)).encher())
            .child(botao("mov-fracao", "Mudar a fração", window, cx).on_click(mudar(|v| v.fracao_alta = !v.fracao_alta)))
            .child(legenda("o arco desliza em 240 ms; com .encher(), enche de 0 ao aparecer (Entrar de novo)"));
        // O halo do foco (E3), como a caixa e o painel ativo vão fazer: borda e halo juntos, 180 ms
        // para entrar e 120 para sair.
        let (borda, forca) = movimento::valor(
            "mov-halo",
            (if self.focada { Cores::destaque_borda() } else { Cores::borda() }, if self.focada { 1. } else { 0. }),
            if self.focada { movimento::MOVER } else { movimento::COR },
            Curva::Mover,
            window,
            cx,
        );
        let realce = movimento::disparo("mov-realce", self.pulso, movimento::ENCHER, Curva::Sair, window, cx)
            .map_or(0., |t| 1. - (2. * t - 1.).abs());
        let halo = fileira()
            .child(
                div()
                    .w(r(320.))
                    .h(r(56.))
                    .px(esp::FLUTUANTE)
                    .flex()
                    .items_center()
                    .rounded(raio::PECA)
                    .bg(gradiente_quente())
                    .border_1()
                    .border_color(borda)
                    .shadow(vec![linha_de_luz(1.), halo_suave(forca)])
                    .corpo_ui()
                    .text_color(Cores::tenue())
                    .child("Mensagem para este chat…"),
            )
            .child(botao("mov-foco", "Focar / tirar o foco", window, cx).on_click(mudar(|v| v.focada = !v.focada)))
            .child(
                div()
                    .h(r(22.))
                    .px(esp::E8)
                    .flex()
                    .items_center()
                    .rounded_full()
                    .bg(Cores::ambar().opacity(0.12 + 0.12 * realce))
                    .metadado()
                    .text_color(Cores::ambar())
                    .child("em revisão"),
            )
            .child(legenda("a pílula realça a cada Enviar (movimento::disparo)"));
        let compasso = fileira()
            .child(botao("mov-girar", if self.girando { "Parar" } else { "Girar" }, window, cx).on_click(mudar(|v| v.girando = !v.girando)))
            .when(self.girando, |d| {
                d.child(gpui_kit::component::spinner::Spinner::new().with_size(Size::Small))
                    .child(Sinal::Rodando.ponto(8.))
                    .child(Sinal::Rodando.ponto(6.))
            })
            .child(legenda("o ponto de rodando respira só no compasso de 12 qps que o spinner já pede"));
        let estado = legenda(format!(
            "Movimento reduzido: {}. Cada efeito responde a uma ação, dura de 120 a 240 ms e redesenha no máximo {} vezes por segundo; VITRAL_QUADROS=1 conta os redesenhos.",
            if cx.reduce_motion() { "ligado, tudo aparece pronto" } else { "desligado" },
            movimento::QPS
        ));
        um(
            "Movimento",
            cx,
            div()
                .flex()
                .flex_col()
                .gap(esp::E16)
                .child(estado)
                .child(entradas)
                .child(controles)
                .child(aneis)
                .child(halo)
                .child(compasso),
        )
    }

    /// A escala e as guias: a lista (16 e 58, o traço em 31), a Equipe (24) e o ritmo da conversa.
    fn espacamento(&self, cx: &mut Context<Self>) -> Div {
        let degraus = [
            (esp::E2, "2 óptico"),
            (esp::E4, "4 botões vizinhos"),
            (esp::E6, "6 ícone e rótulo"),
            (esp::E8, "8 peça, frestas"),
            (esp::E12, "12 caixa, no turno"),
            (esp::E16, "16 flutuante, seção"),
            (esp::E24, "24 entre turnos"),
            (esp::E32, "32 vazio"),
        ];
        let escala = fileira().items_end().gap(esp::E16).children(degraus.map(|(e, nome)| {
            div()
                .flex()
                .flex_col()
                .gap(esp::E4)
                .child(div().w(e).h(r(24.)).bg(Cores::destaque()))
                .child(legenda(nome))
        }));
        let guia = |x: Rems, cor: Hsla| div().absolute().top_0().bottom_0().left(x).w(px(1.)).bg(cor);
        let lista = div()
            .relative()
            .w(r(320.))
            .flex()
            .flex_col()
            .gap(esp::E2)
            .px(esp::PECA)
            .pb(esp::E8)
            .rounded(raio::PECA)
            .bg(Cores::vidro_leitura())
            .child(CabecalhoDeSecao::new("Recentes").recolhivel(true, |_, _| {}).primeira(true))
            .child(
                CartaoDeLista::new("esp-pai", "Cupons de desconto na API")
                    .simbolo(Simbolo::Projeto(&ICONES[1], CORES_PROJETO[0].0))
                    .subtitulo("loja · 2 chats")
                    .meta("4 h"),
            )
            .child(
                CartaoDeLista::new("esp-filho", "#2 Aplicar cupom no checkout")
                    .nivel(1)
                    .ramo(true, true)
                    .simbolo(Simbolo::Elemento(
                        div().flex_none().size(r(6.)).rounded_full().bg(Cores::ambar()).into_any_element(),
                    ))
                    .dica_do_ponto("em revisão"),
            )
            .child(guia(esp::GUIA_LISTA, Cores::destaque().opacity(0.6)))
            .child(guia(r(31.), Cores::verde().opacity(0.6)))
            .child(guia(r(58.), Cores::destaque().opacity(0.6)));
        // A Equipe mede o ritmo na tinta (o `respiro` de equipe.rs): 8 entre cartões, 16 antes de
        // cada seção e, no cartão, 8 entre as linhas e 12 da borda ao título.
        let cartao_equipe = |titulo: &'static str, linha: &'static str| {
            div()
                .flex()
                .flex_col()
                .gap(esp::E8)
                .pt(dentro_da_borda(8.))
                .pb(dentro_da_borda(12.))
                .px(dentro_da_borda(12.))
                .rounded(raio::CARTAO)
                .bg(Cores::cartao_elevado())
                .border_1()
                .border_color(Cores::linha())
                .reflexo(raio::CARTAO)
                .child(div().line_height(r(18.)).child(titulo))
                .child(legenda(linha))
        };
        let equipe = div()
            .relative()
            .w(r(320.))
            .flex()
            .flex_col()
            .p(esp::E12)
            .rounded(raio::PECA)
            .bg(Cores::cartao())
            .reflexo(raio::PECA)
            .child(cartao_equipe("#4 Mostrar o desconto", "12 da borda ao título, 8 entre as linhas"))
            .child(div().mt(esp::E8).child(cartao_equipe("#5 Validar o cupom", "8 entre os cartões")))
            .child(
                div().mt(esp::E4).child(
                    CabecalhoDeSecao::new("Concluídas")
                        .recolhivel(true, |_, _| {})
                        .contagem(1)
                        .recuo(12.)
                        .respiro(4.),
                ),
            )
            .child(
                CartaoDeLista::new("esp-concluida", "#1 Aplicar cupom no checkout")
                    .uma_linha()
                    .simbolo(Simbolo::icone_cor(Lucide::CircleCheck, Cores::verde())),
            )
            .child(
                div()
                    .mt(r(-6.))
                    .child(CabecalhoDeSecao::new("Decisões").recolhivel(false, |_, _| {}).recuo(12.).respiro(4.)),
            )
            .child(guia(esp::GUIA_EQUIPE, Cores::destaque().opacity(0.6)));
        let bloco = |nome: &'static str, fundo: bool| {
            div()
                .p(dentro_da_borda(12.))
                .rounded(raio::CARTAO)
                .when(fundo, |d| d.bg(Cores::cartao_elevado()).border_1().border_color(Cores::linha()).reflexo(raio::CARTAO))
                .when(!fundo, |d| d.border_1().border_color(transparent_black()))
                .corpo_ui()
                .child(nome)
        };
        let ritmo = div()
            .w(r(360.))
            .flex()
            .flex_col()
            .child(bloco("balão: o conteúdo a 12 da borda", true))
            .child(div().h(esp::NO_TURNO))
            .child(bloco("texto do turno (12 dentro do turno)", false))
            .child(div().h(esp::ENTRE_TURNOS))
            .child(bloco("próximo turno (24 entre turnos)", true));
        let flutuante = div()
            .w(r(280.))
            .p(esp::FLUTUANTE)
            .flutuante(cx)
            .rounded(raio::PECA)
            .flex()
            .flex_col()
            .gap(esp::E12)
            .child(div().rotulo_secao().child("Popover: margem 16, blocos a 12"))
            .child(legenda("a linha de luz de 1 px é do popover_style"));
        um(
            "Espaçamento",
            cx,
            div()
                .flex()
                .flex_col()
                .gap(esp::E16)
                .child(escala)
                .child(legenda(
                    "guias: lista em 16 e 58 (o traço da árvore em 31); direita em 24, com 16 na tinta antes de cada seção",
                ))
                .child(fileira().items_start().gap(esp::E24).child(lista).child(equipe).child(ritmo).child(flutuante)),
        )
    }

    fn segmentados(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let eu = cx.entity().downgrade();
        let (segmento, esforco) = (self.segmento, self.esforco);
        quatro("segmentados", "Controle segmentado", window, cx, move |fundo, _, _| {
            let id = |nome: &str| SharedString::from(format!("{nome}-{}", fundo as usize));
            let mudar_segmento = {
                let eu = eu.clone();
                move |i, _: &mut Window, cx: &mut App| {
                    let _ = eu.update(cx, |v, cx| {
                        v.segmento = i;
                        cx.notify();
                    });
                }
            };
            let mudar_esforco = {
                let eu = eu.clone();
                move |i, _: &mut Window, cx: &mut App| {
                    let _ = eu.update(cx, |v, cx| {
                        v.esforco = i;
                        cx.notify();
                    });
                }
            };
            div()
                .flex()
                .flex_col()
                .gap(esp::E12)
                .child(ControleSegmentado::new(
                    id("abas"),
                    [Segmento::new("Conversas"), Segmento::new("Arquivos").ponto(Cores::destaque())],
                    segmento.min(1),
                    mudar_segmento.clone(),
                ))
                .child(
                    ControleSegmentado::new(
                        id("esforco"),
                        ["Baixo", "Médio", "Alto", "Extra", "Máximo"],
                        esforco,
                        mudar_esforco,
                    )
                    .cor(Cores::ciano())
                    .with_size(Size::Large),
                )
                .child(
                    ControleSegmentado::new(
                        id("revisao"),
                        [
                            Segmento::new("Unificado").icone(Lucide::Rows3).dica("Unificado"),
                            Segmento::new("Lado a lado").icone(Lucide::Columns2).dica("Lado a lado"),
                        ],
                        segmento.min(1),
                        mudar_segmento.clone(),
                    )
                    .so_icone(true)
                    .with_size(Size::Small),
                )
                .child(
                    ControleSegmentado::new(id("largo"), ["5 h", "Semana"], segmento.min(1), mudar_segmento.clone())
                        .largura_total(true),
                )
                .child(
                    ControleSegmentado::new(id("desab"), ["Ativas", "Arquivadas"], 0, mudar_segmento)
                        .disabled(true),
                )
                .into_any_element()
        })
    }

    fn campos(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let eu = cx.entity().downgrade();
        let contador = self.contador;
        let dias = self.dias;
        let conteudo = div()
            .flex()
            .flex_col()
            .gap(r(16.))
            .w(r(420.))
            .child(campo_de_busca("busca", &self.busca, Some(Box::new(NovaConversa)), window, cx))
            .child(
                div()
                    .rounded(raio::PECA)
                    .bg(Cores::flutuante())
                    .child(campo_de_busca_plano("busca-plana", &self.busca, window, cx)),
            )
            .child(
                Campo::new("Nome do projeto", gpui_kit::component::input::Input::new(&self.campo))
                    .ajuda("Vira a pasta própria do projeto"),
            )
            .child(
                Campo::new("Comando de verificação", gpui_kit::component::input::Input::new(&self.campo))
                    .opcional()
                    .erro(Some("Já existe um projeto com esse nome".into())),
            )
            .child(fileira().child(legenda("Ao mesmo tempo")).child(Contador::new(
                "contador",
                contador,
                1..=8,
                {
                    let eu = eu.clone();
                    move |n, _, cx| {
                        let _ = eu.update(cx, |v, cx| {
                            v.contador = n;
                            cx.notify();
                        });
                    }
                },
            )))
            .child(fileira().children(["D", "S", "T", "Q", "Q", "S", "S"].iter().enumerate().map(|(i, _)| {
                let nomes = ["Dom", "Seg", "Ter", "Qua", "Qui", "Sex", "Sáb"];
                let eu = eu.clone();
                Chip::new(("dia", i), nomes[i])
                    .tamanho(TamanhoChip::Medio)
                    .alternavel(dias[i])
                    .on_click(move |_, _, cx| {
                        let _ = eu.update(cx, |v, cx| {
                            v.dias[i] = !v.dias[i];
                            cx.notify();
                        });
                    })
                    .construir(window, cx)
            })));
        um("Campos, busca e contador", cx, conteudo)
    }

    fn popovers(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let esforco = self.esforco;
        let ultrathink = self.ultrathink;
        let eu = cx.entity().downgrade();
        let corpo_esforco = move |eu: WeakEntity<Vitrine>| {
            let mudar = {
                let eu = eu.clone();
                move |i, _: &mut Window, cx: &mut App| {
                    let _ = eu.update(cx, |v, cx| {
                        v.esforco = i;
                        cx.notify();
                    });
                }
            };
            CorpoPopover::new(LarguraPopover::P400)
                .cabecalho(CabecalhoPopover::Escolha {
                    simbolo: Some(Simbolo::Sinal { nivel: esforco as u8 + 1, total: 5 }),
                    titulo: "Esforço".into(),
                    valor: Some((["Baixo", "Médio", "Alto", "Extra", "Máximo"][esforco].into(), Cores::ciano())),
                    dica: Some("mais rápido → mais esperto".into()),
                })
                .child(
                    ControleSegmentado::new("pop-esforco", ["Baixo", "Médio", "Alto", "Extra", "Máximo"], esforco, mudar)
                        .cor(Cores::ciano())
                        .with_size(Size::Large)
                        .largura_total(true)
                        .no_flutuante(true),
                )
                .rodape(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            LinhaDeOpcao::new("ultrathink", "Ultrathink")
                                .arco_iris()
                                .descricao("Pede ao Claude para pensar mais só nesta mensagem")
                                .interruptor(ultrathink, move |ligado, _, cx| {
                                    let _ = eu.update(cx, |v, cx| {
                                        v.ultrathink = ligado;
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(
                            LinhaDeOpcao::new("ultracode", "Ultracode")
                                .simbolo(Lucide::Workflow, Cores::violeta())
                                .descricao("Vários agentes em paralelo, só na próxima mensagem")
                                .aviso("Gasta bem mais do plano e roda no esforço Extra")
                                .interruptor(false, |_, _, _| {}),
                        ),
                )
        };
        let estatico = corpo_esforco(eu.clone());
        let conta = CorpoPopover::new(LarguraPopover::P320)
            .cabecalho(CabecalhoPopover::Objeto {
                simbolo: Simbolo::Marca(Marca::Claude),
                nome: "Claude Max".into(),
                detalhe: Some("victor@exemplo.com".into()),
                selo: Some(Selo::neutro("padrão")),
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(r(12.))
                    .child(AnelDeUso::new("pop-uso", Marca::Claude, Some(0.23)).semana(Some(0.41)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().corpo_ui().child(resumo_uso(Some(0.23), Some(0.41))))
                            .child(legenda("renova em 2 h 13 min · 15:59")),
                    ),
            )
            .rodape(botao_fantasma("pop-atualizar", Some(Lucide::RotateCw.into()), "Atualizar", TamanhoBotao::Barra, window, cx));
        let gatilho = Chip::new("pop-gatilho", "Médio")
            .simbolo(Simbolo::Sinal { nivel: esforco as u8 + 1, total: 5 })
            .cor_rotulo(Cores::ciano())
            .seta(true)
            .construir(window, cx);
        let real = popover("pop-real")
            .anchor(Anchor::TopLeft)
            .trigger(gatilho)
            .content(move |_, _, _| corpo_esforco(eu.clone()));
        let conteudo = div()
            .flex()
            .flex_wrap()
            .items_start()
            .gap(r(16.))
            .child(div().p(r(16.)).flutuante(cx).child(estatico))
            .child(div().p(r(16.)).flutuante(cx).child(conta))
            .child(div().flex().flex_col().gap(r(8.)).child(legenda("Popover de verdade:")).child(real));
        um("Popover", cx, conteudo)
    }

    fn menus(&self, window: &mut Window, cx: &mut App) -> Div {
        let menu = Chip::new("menu-gatilho", "Menu do Vitral")
            .simbolo(Simbolo::icone(Lucide::Compass))
            .seta(true)
            .construir(window, cx)
            .dropdown_menu_with_anchor(Anchor::TopLeft, |menu, window, _| {
                menu.min_w(tam(window, 300.))
                    .item(cabecalho_objeto(Simbolo::icone(Lucide::Folder), "Vitral", Some("~/Projetos".into())))
                    .separator()
                    .item(PopupMenuItem::new("Nova conversa").icon(Lucide::MessageSquarePlus).action(Box::new(NovaConversa)))
                    .item(PopupMenuItem::new("Novo projeto…").icon(Lucide::FolderPlus))
                    .separator()
                    .label("Projetos")
                    .item(PopupMenuItem::new("Rotinas").icon(Lucide::CalendarClock).checked(true))
                    .item(PopupMenuItem::new("Desabilitado").icon(Lucide::Activity).disabled(true))
                    .separator()
                    .item(PopupMenuItem::new("Abrir no gerenciador de arquivos").icon(Lucide::FolderOpen).external())
                    .separator()
                    .item(PopupMenuItem::new("Apagar a cópia…").icon(Lucide::Trash).danger(true))
                    .separator()
                    .item(rodape_menu("Cole imagens direto na caixa", Some((Box::new(NovaConversa), None))))
            });
        let modo = Chip::new("menu-modo", "Manual")
            .simbolo({
                let (icone, cor) = icone_do_modo("manual", false);
                Simbolo::Icone(icone, Some(cor))
            })
            .seta(true)
            .construir(window, cx)
            .dropdown_menu_with_anchor(Anchor::TopLeft, |menu, window, _| {
                let modos = [
                    ("manual", "Manual", "Pede aprovação antes de cada edição ou comando"),
                    ("acceptEdits", "Aceitar edições", "Edita arquivos sem perguntar; comandos ainda pedem"),
                    ("plan", "Planejar", "Explora o código e apresenta um plano antes de mexer"),
                    ("auto", "Automático", "Um classificador decide o que precisa de aprovação"),
                    ("bypassPermissions", "Sem perguntar", "Faz tudo sem pedir permissão"),
                ];
                let mut menu = menu
                    .min_w(tam(window, 400.))
                    .item(cabecalho_escolha("Modo de permissão", Some((Box::new(NovaConversa), OndeAtalho::Contexto(None), "troca"))));
                for (valor, nome, descricao) in modos {
                    let (icone, cor) = icone_do_modo(valor, false);
                    menu = menu.item(item_descrito(Some(Simbolo::Icone(icone, Some(cor))), nome, descricao, valor == "manual"));
                }
                menu
            });
        let modos = fileira().children(["manual", "acceptEdits", "plan", "auto", "bypassPermissions"].map(|valor| {
            let (icone, cor) = icone_do_modo(valor, false);
            div().flex().items_center().gap(r(4.)).child(icone.with_size(tam(window, 16.)).text_color(cor)).child(legenda(valor))
        }))
        .child({
            let (icone, cor) = icone_do_modo("bypassPermissions", true);
            div().flex().items_center().gap(r(4.)).child(icone.with_size(tam(window, 16.)).text_color(cor)).child(legenda("sem cópia"))
        });
        um(
            "Menus e modos",
            cx,
            div().flex().flex_col().gap(r(12.)).child(fileira().child(menu).child(modo)).child(modos),
        )
    }

    fn alertas(&self, window: &mut Window, cx: &mut App) -> Div {
        let tons = [Tom::Info, Tom::Atencao, Tom::Erro, Tom::Pronto];
        let caixa = div()
            .flex()
            .flex_col()
            .gap(r(8.))
            .w(r(520.))
            .child(
                Alerta::new(Tom::Atencao, "Sem git, os chats mexem direto na pasta.")
                    .titulo("Projeto sem git")
                    .acao(botao_fantasma("alerta-git", Some(Lucide::GitBranch.into()), "Ativar git neste projeto", TamanhoBotao::Barra, window, cx)),
            )
            .children(tons.map(|tom| Alerta::new(tom, format!("Alerta no tom {tom:?}")).fechar(|_, _| {})))
            .children(tons.map(|tom| Alerta::new(tom, "Aprovada com ressalva: verificação vermelha").compacto()));
        let avisos = fileira()
            .children(tons.map(|tom| {
                Button::new(("aviso", tom as usize))
                    .outline()
                    .label(format!("Aviso {tom:?}"))
                    .on_click(move |_, window, cx| window.push_notification(aviso(tom, "Chat 2 juntado"), cx))
            }))
            .child(Button::new("aviso-acao").outline().label("Com Desfazer").on_click(|_, window, cx| {
                window.push_notification(
                    aviso_com_acao(Tom::Info, "Rotina apagada", "Desfazer", |_, _| {}).chave("rotina"),
                    cx,
                )
            }))
            .child(Button::new("aviso-erro").outline().label("Erro com Detalhes").on_click(|_, window, cx| {
                let erro = anyhow::anyhow!("fatal: a pasta já tem um .git\ncaminho: ~/Projetos/loja/.git");
                window.push_notification(aviso_erro("Não deu para ativar o git", &erro), cx)
            }))
            .child(Button::new("aviso-texto").outline().label("Texto simples").on_click(|_, window, cx| {
                window.push_notification(Notification::new().message("Caminho copiado").chave("copiado"), cx)
            }));
        um("Alertas e avisos", cx, div().flex().flex_col().gap(r(16.)).child(caixa).child(avisos))
    }

    fn vazios(&self, cx: &mut App) -> Div {
        let conteudo = div()
            .flex()
            .flex_wrap()
            .gap(r(16.))
            .child(
                div().w(r(360.)).child(
                    EstadoVazio::new(Lucide::ListChecks, "Nada aberto aqui")
                        .texto("Os pedidos que esta conversa abriu viraram chats do projeto.")
                        .acao(Button::new("vazio-acao").outline().label("Pedir ao Mestre")),
                ),
            )
            .child(
                div().flex_1().child(
                    EstadoVazio::new(Lucide::MessageSquarePlus, "Comece uma conversa")
                        .texto("Diga o que quer fazer; o Mestre manda para o projeto certo.")
                        .amplo(),
                ),
            );
        um("Estado vazio", cx, conteudo)
    }

    fn listas(&self, window: &mut Window, cx: &mut App) -> Div {
        let acoes = |id: &'static str, window: &Window, cx: &App| {
            botao_icone(id, Lucide::Columns2, "Abrir ao lado", TamanhoBotao::Linha, window, cx)
        };
        let projeto = Simbolo::Projeto(&ICONES[1], CORES_PROJETO[0].0);
        // Como na lista: em "Esperando você" o cartão é neutro e o âmbar fica no ícone do motivo.
        let previa = |nome: Lucide, rotulo: &'static str, texto: &'static str, window: &Window| {
            div()
                .flex()
                .items_center()
                .gap(esp::E4)
                .min_w_0()
                .child(div().flex_none().text_color(Cores::ambar()).child(icone(nome, 12., window)))
                .child(div().flex_none().font_weight(FontWeight::MEDIUM).text_color(Cores::texto()).child(rotulo))
                .child(div().flex_1().min_w_0().truncate().child(texto))
        };
        let conteudo = div()
            .flex()
            .flex_col()
            .gap(r(2.))
            .w(r(380.))
            .p(r(8.))
            .rounded(raio::PECA)
            .bg(Cores::vidro_leitura())
            .child(CabecalhoDeSecao::new("Esperando você").contagem(2).tingida(Cores::ambar()))
            .child(
                CartaoDeLista::new("c-ambar", "Rodar os testes da loja")
                    .simbolo(Simbolo::Projeto(&ICONES[2], CORES_PROJETO[3].0))
                    .sinal(Some(Sinal::Pedido))
                    .subtitulo(previa(Lucide::ShieldQuestionMark, "Permissão", "· Bash · cargo test", window))
                    .meta("1 min")
                    .acao(acoes("c-ambar-a", window, cx)),
            )
            .child(
                CartaoDeLista::new("c-ambar-sel", "Cupons de desconto na API")
                    .simbolo(Simbolo::Projeto(&ICONES[4], CORES_PROJETO[5].0))
                    .sinal(Some(Sinal::Pedido))
                    .subtitulo(previa(Lucide::Map, "#2 pede", "· aprovar o plano", window))
                    .meta("3 min")
                    .selecionado(true),
            )
            .child(CabecalhoDeSecao::new("Rodando agora").contagem(1).tingida(Cores::destaque()))
            .child(
                CartaoDeLista::new("c-sel", "Cupons de desconto na API")
                    .simbolo(projeto.copia().unwrap())
                    .sinal(Some(Sinal::Rodando))
                    .subtitulo("loja-api · 1 ajudante rodando")
                    .meta("agora")
                    .meta_inferior(PilulaDeEstado::new(Estado::Rodando).compacta())
                    .selecionado(true)
                    .acao(acoes("c-sel-a", window, cx)),
            )
            .children([
                ("c-trab-1", "#1 Validar o cupom na API", Estado::Pronto, None, (true, false)),
                ("c-trab-2", "#2 Aplicar cupom no checkout", Estado::Rodando, Some(Sinal::Rodando), (false, false)),
                ("c-trab-3", "#3 Mostrar o desconto no carrinho", Estado::EmRevisao, None, (false, true)),
            ]
            .map(|(id, titulo, estado, sinal, (primeiro, ultimo))| {
                // Como na lista: o ponto de 8 na cor do estado (ou o sinal, quando há) e a palavra
                // na dica dele.
                let cor = estado.cor().unwrap_or_else(Cores::tenue);
                CartaoDeLista::new(id, titulo)
                    .nivel(1)
                    .ramo(primeiro, ultimo)
                    .simbolo(Simbolo::Elemento(div().flex_none().size(r(8.)).rounded_full().bg(cor).into_any_element()))
                    .sinal(sinal)
                    .dica_do_ponto(estado.rotulo())
                    .meta(if estado == Estado::Rodando { "2 min" } else { "" })
            }))
            .child(CabecalhoDeSecao::new("Conversas").recolhivel(true, |_, _| {}).acao(acoes("secao-a", window, cx)))
            .child(CabecalhoDeSecao::new("Mais antigas").recolhivel(false, |_, _| {}).contagem(12))
            .child(CabecalhoDeSecao::new("Recentes"))
            .child(
                CartaoDeLista::new("c-nl", "Resposta nova esperando leitura")
                    .simbolo(Simbolo::Projeto(&ICONES[10], CORES_PROJETO[1].0))
                    .sinal(Some(Sinal::Nova))
                    .subtitulo("site · terminou: 3 ajudantes aprovados")
                    .meta("10:42")
                    .nao_lido(true)
                    .acao(acoes("c-nl-a", window, cx)),
            )
            .child(
                CartaoDeLista::new("c-realce", "Realce do teclado mostra as ações")
                    .simbolo(Simbolo::Projeto(&ICONES[20], CORES_PROJETO[7].0))
                    .subtitulo("loja · ontem")
                    .meta("ontem")
                    .realce(true)
                    .acao(acoes("c-realce-a", window, cx))
                    .acao(botao_icone("c-realce-b", Lucide::Pin, "Fixar", TamanhoBotao::Linha, window, cx)),
            )
            .child(
                CartaoDeLista::new("c-busca", "Cupons de desconto na API")
                    .titulo_rico(StyledText::new("Cupons de desconto na API").with_highlights([(
                        0..6,
                        HighlightStyle { font_weight: Some(FontWeight::BOLD), ..Default::default() },
                    )]))
                    .simbolo(Simbolo::Elemento(Selo::contagem(2).solida(Cores::destaque()).into_any_element()))
                    .subtitulo("trecho da busca em forte")
                    .meta("clique")
                    .on_click(|_, window, cx| window.push_notification(aviso(Tom::Pronto, "Cartão clicado"), cx)),
            )
            .child(
                CartaoDeLista::new("c-outro", "Aberta em outro painel")
                    .simbolo(Simbolo::Papel(Papel::Coordenador))
                    .subtitulo("vitral · chat 12")
                    .meta("seg.")
                    .aberto_em_outro(true),
            )
            .child(CabecalhoDeSecao::new("Arquivos"))
            .child(
                CartaoDeLista::new("c-um", "src/ui/componentes.rs")
                    .simbolo(Simbolo::icone(Lucide::FileCode))
                    .uma_linha()
                    .meta("+42 −3"),
            )
            .child(
                CartaoDeLista::new("c-rotina", "Revisar dependências")
                    .simbolo(Simbolo::icone(Lucide::CalendarClock))
                    .uma_linha()
                    .acao_fixa(gpui_kit::component::switch::Switch::new("rotina-switch").checked(true)),
            )
            .child(div().pt(r(8.)).child(rotulo_tarefa(2, "Aplicar cupom no checkout", true)));
        um("Listas", cx, conteudo)
    }

    fn opcoes(&self, cx: &mut Context<Self>) -> Div {
        let eu = cx.entity().downgrade();
        let (ligado, segmento) = (self.ligado, self.segmento);
        let mudar_ligado = {
            let eu = eu.clone();
            move |novo, _: &mut Window, cx: &mut App| {
                let _ = eu.update(cx, |v, cx| {
                    v.ligado = novo;
                    cx.notify();
                });
            }
        };
        let grupo = GrupoDeOpcoes::new(Some("Chats".into()))
            .linha(
                LinhaDeOpcao::new("op-revisor", "Revisor independente")
                    .descricao("Confere os critérios antes de juntar")
                    .info("Roda numa sessão nova, só de leitura, e pode ser outro modelo.")
                    .interruptor(ligado, mudar_ligado.clone()),
            )
            .linha(
                LinhaDeOpcao::new("op-modelo", "Modelo do revisor")
                    .descricao("Padrão: o mesmo dos chats")
                    .controle(ControleSegmentado::new("op-seg", ["Padrão", "Opus", "Sonnet"], segmento.min(2), {
                        let eu = eu.clone();
                        move |i, _, cx| {
                            let _ = eu.update(cx, |v, cx| {
                                v.segmento = i;
                                cx.notify();
                            });
                        }
                    }).with_size(Size::Small)),
            )
            .linha(
                LinhaDeOpcao::new("op-campo", "Comando de verificação")
                    .descricao("Roda quando o chat para")
                    .campo(div().h(r(32.)).px(esp::E12).flex().items_center().rounded(raio::CONTROLE).bg(Cores::recuo()).mono_pequeno().child("cargo test")),
            )
            .linha(
                LinhaDeOpcao::new("op-desab", "Arquivar conversas paradas")
                    .interruptor(false, |_, _, _| {})
                    .desabilitada("Ligue o revisor para escolher"),
            )
            .linha(
                LinhaDeOpcao::new("op-ultra", "Ultrathink")
                    .arco_iris()
                    .descricao("Pede ao Claude para pensar mais só nesta mensagem")
                    .interruptor(!ligado, move |novo, window, cx| mudar_ligado(!novo, window, cx)),
            )
            .linha(
                LinhaDeOpcao::new("op-ultracode", "Ultracode")
                    .simbolo(Lucide::Workflow, Cores::violeta())
                    .descricao("Vários agentes em paralelo, só na próxima mensagem")
                    .aviso("Gasta bem mais do plano e roda no esforço Extra")
                    .interruptor(false, |_, _, _| {})
                    .desabilitada("Só nos chats de projeto"),
            );
        um("Opções", cx, div().w(r(560.)).child(grupo))
    }

    fn escolha(&self, cx: &mut Context<Self>) -> Div {
        let lista = self.lista.clone();
        let abrir = Button::new("abrir-lista").outline().label("Abrir (limpa e realça o escolhido)").on_click(
            move |_, window, cx| lista.update(cx, |lista, cx| lista.abrir(window, cx)),
        );
        let larguras = [LarguraPopover::P320, LarguraPopover::P360, LarguraPopover::P400, LarguraPopover::P560]
            .map(|l| l.px().to_string())
            .join(", ");
        um(
            "Lista de escolha",
            cx,
            div()
                .flex()
                .flex_col()
                .gap(r(8.))
                .child(fileira().child(abrir).child(legenda(self.escolhido.clone())))
                .child(legenda(format!("Larguras de popover: {larguras}")))
                .child(div().w(r(560.)).flutuante(cx).overflow_hidden().child(self.lista.clone())),
        )
    }

    fn estados(&self, window: &mut Window, cx: &mut App) -> Div {
        quatro("estados", "Sinais, pílulas e selos", window, cx, |_, window, _| {
            let sinais = [Sinal::Erro, Sinal::Pedido, Sinal::Parada, Sinal::Nova, Sinal::Rodando];
            div()
                .flex()
                .flex_col()
                .gap(r(8.))
                .child(fileira().children(sinais.map(|s| s.ponto(6.))).children(sinais.map(|s| s.ponto(8.))))
                .child(fileira().children(Estado::TODOS.map(PilulaDeEstado::new)))
                .child(
                    fileira()
                        .child(PilulaDeEstado::new(Estado::Rodando).detalhe("2 min 13 s"))
                        .child(PilulaDeEstado::new(Estado::Pronto).rotulo("aprovada"))
                        .child(PilulaDeEstado::new(Estado::EmRevisao).detalhe("rodada 2"))
                        .children(Estado::TODOS.map(|e| PilulaDeEstado::new(e).compacta())),
                )
                .child(
                    fileira()
                        .child(Selo::neutro("1M de contexto"))
                        .child(Selo::cor("beta", Cores::violeta()))
                        .child(Selo::novo())
                        .child(Selo::contagem(3))
                        .child(Selo::contagem(2).tingida(Cores::ambar()))
                        .child(Selo::contagem(5).solida(Cores::destaque()))
                        .child(Selo::contagem(120).solida(Cores::vermelho()))
                        .child(Selo::ponto(Cores::ambar()))
                        .child(Selo::ponto(Cores::destaque()).vazado())
                        .child(Selo::ponto(Cores::vermelho()).lado(6.))
                        .child(Selo::neutro("clicável").clicavel("selo-clicavel", |window, cx| {
                            window.push_notification(aviso(Tom::Info, "Selo clicado"), cx)
                        }))
                        .child(Selo::capacidades(vec![
                            (Lucide::Brain.into(), Cores::violeta(), "raciocínio"),
                            (Lucide::Image.into(), Cores::ciano(), "lê imagens"),
                            (Lucide::Zap.into(), Cores::tenue(), "modo rápido"),
                        ])),
                )
                .child(
                    fileira()
                        .child(selo_gravidade("alta"))
                        .child(selo_gravidade("média"))
                        .child(selo_gravidade("baixa"))
                        .child(numero_painel(1, true))
                        .child(numero_painel(2, false))
                        .child(Icon::new(Lucide::Orbit).with_size(tam(window, 16.)).text_color(Cores::destaque_texto())),
                )
                .into_any_element()
        })
    }

    fn graficos(&self, window: &mut Window, cx: &mut App) -> Div {
        quatro("graficos", "Anéis e barras", window, cx, |fundo, _, _| {
            let id = |nome: &str, i: usize| SharedString::from(format!("{nome}-{}-{i}", fundo as usize));
            let fracoes = [0.3, 0.69, 0.70, 0.89, 0.90, 1.0];
            div()
                .flex()
                .flex_col()
                .gap(esp::E12)
                .child(fileira().children(fracoes.iter().enumerate().map(|(i, f)| {
                    Anel::new(id("anel", i), *f).dica(format!("{:.0}% do contexto", f * 100.))
                })))
                .child(
                    fileira()
                        .child(Anel::new(id("anel-cor", 0), 0.6).cor(Cores::verde()).diametro(28.).espessura(4.))
                        .child(Anel::new(id("anel-cor", 1), 0.6).cor(Marca::Codex.cor()).diametro(28.).espessura(2.))
                        .child(legenda(format!("{} e {}", Marca::Claude.nome(), Marca::Codex.nome()))),
                )
                .child(fileira().children([20., 22., 28., 36.].iter().enumerate().map(|(i, d)| {
                    Anel::new(id("anel-d", i), 0.42)
                        .diametro(*d)
                        .when(*d >= 28., |a| a.centro(div().micro().child("42")))
                })))
                .child(
                    fileira()
                        .child(AnelDeUso::new(id("uso", 0), Marca::Claude, Some(0.23)).semana(Some(0.41)))
                        .child(AnelDeUso::new(id("uso", 1), Marca::Claude, Some(0.75)).semana(Some(0.5)))
                        .child(AnelDeUso::new(id("uso", 2), Marca::Claude, Some(0.95)).semana(Some(0.92)))
                        .child(AnelDeUso::new(id("uso", 3), Marca::Claude, Some(1.)).bloqueado(true))
                        .child(AnelDeUso::new(id("uso", 4), Marca::Codex, Some(0.12)).semana(Some(0.3)))
                        .child(AnelDeUso::new(id("uso", 5), Marca::Codex, None).apagado(true))
                        .child(AnelDeUso::new(id("uso", 6), Marca::Claude, Some(0.4)).unico())
                        .child(AnelDeUso::new(id("uso", 7), Marca::Claude, Some(0.4)).diametro(20.)),
                )
                .child(
                    fileira()
                        .children((1..=5).map(|n| BarrinhasDeSinal::new(n, 5)))
                        .child(BarrinhasDeSinal::new(3, 5).sugerido())
                        .child(BarrinhasDeSinal::new(4, 5).altura(12.))
                        .child(BarrinhasDeSinal::new(2, 5).cor(Cores::destaque())),
                )
                .child(BarraSegmentada::new(vec![
                    (1, Cores::destaque(), "1 rodando".into()),
                    (1, Cores::ambar(), "1 em revisão".into()),
                    (2, Cores::verde(), "2 aprovadas".into()),
                    (1, Cores::tenue(), "1 parada".into()),
                ]))
                .child(BarraSegmentada::new(vec![(3, Cores::verde(), "3 aprovadas".into()), (1, Cores::vermelho(), "1 falhou".into())]).fina())
                .into_any_element()
        })
    }

    fn base(&self, window: &mut Window, cx: &mut App) -> Div {
        let quadrados = div().flex().flex_col().gap(esp::E12).children(CORES_PROJETO.iter().enumerate().map(|(i, (cor, nome))| {
            fileira()
                .child(div().w(r(80.)).child(legenda(*nome)))
                .children(LADOS_DO_QUADRADO.map(|lado| QuadradoDeProjeto::new(&ICONES[i * 7 % ICONES.len()], *cor, lado)))
                .child(QuadradoDeProjeto::new(&ICONES[i], *cor, 32.).ativo(true))
                .child(QuadradoDeProjeto::new(&ICONES[i], *cor, 32.).realcado(true))
                .child(QuadradoDeProjeto::new(&ICONES[i], *cor, 32.).sinal(Some([Sinal::Erro, Sinal::Pedido, Sinal::Nova, Sinal::Rodando][i % 4])))
        }));
        let papeis = fileira()
            .children([Papel::Coordenador, Papel::Trabalhador, Papel::Mestre, Papel::Avulso].map(|p| p.circulo(30.)))
            .child(Marca::Claude.disco(24.))
            .child(Marca::Codex.disco(24.))
            .child(Marca::Codex.disco(24.).apagada())
            .child(div().text_size(r(14.)).font_semibold().child(texto_arco_iris("Ultrathink")))
            .child(icone(Lucide::Sparkles, 16., window));
        um("Projetos, papéis e marcas", cx, div().flex().flex_col().gap(r(16.)).child(quadrados).child(papeis))
    }

    fn paineis(&self, window: &mut Window, cx: &mut App) -> Div {
        let abas = fileira()
            .child(Aba::new("aba-unica", "Cupons de desconto").tom(TomAba::Unica).simbolo(Simbolo::Papel(Papel::Coordenador)).construir(window, cx))
            .child(Aba::new("aba-ativa", "Cupons de desconto").tom(TomAba::Ativa).numero(1, true).estado(Some(Sinal::Rodando)).fechar("Fechar o painel", true, |_, _| {}).construir(window, cx))
            .child(Aba::new("aba-inativa", "#2 Aplicar cupom").tom(TomAba::Inativa).numero(2, false).estado(Some(Sinal::Pedido)).fechar("Fechar o painel", false, |_, _| {}).construir(window, cx))
            .child(Aba::new("aba-sel", "motor.rs").tom(TomAba::Selecionada).simbolo(Simbolo::icone(Lucide::FileCode)).detalhe("src/", true).construir(window, cx))
            .child(Aba::new("aba-normal", "LEIA-ME.md").simbolo(Simbolo::icone(Lucide::FileText)).construir(window, cx))
            .child(Aba::new("aba-arrasto", "Arrastando").tom(TomAba::Ativa).arrastando(true).construir(window, cx))
            .child(self.previa.clone());
        let faixas = [300., 360., 460., 800.].map(|px| {
            let rem = px / 15.;
            legenda(format!(
                "{px} px: {:?} · caixa {:?}",
                LarguraPainel::de(rem, LarguraPainel::default()),
                FaixaCaixa::de(rem.min(860. / 15.) - 32. / 15., FaixaCaixa::default())
            ))
        });
        let mut medida = MedidaPainel::default();
        let mudou = medida.atualizar(px(900.), window.rem_size());
        let medida = legenda(format!(
            "900 px neste zoom: {:.1} rem, {:?}, caixa {:?} (mudou: {mudou})",
            medida.rem, medida.nivel, medida.faixa
        ));
        um(
            "Abas e painéis",
            cx,
            div().flex().flex_col().gap(r(12.)).child(abas).child(fileira().children(faixas)).child(medida),
        )
    }

    /// O grupo das Configurações em cada estado, os avisos da versão nova e da abertura, as
    /// Novidades e a pergunta do Reiniciar com conversa trabalhando (o Enter fica no Continuar
    /// aberto; o "mesmo assim" deixa um aviso). Os botões têm 200 de largura: o roteiro da bancada
    /// clica pela posição.
    fn atualizacoes(&self, window: &mut Window, cx: &mut App) -> Div {
        const NOTAS: &str = "- **Conversas que se lembram.** O Mestre guarda o que importa entre as conversas.\n\
                             - **Mais rápido ao abrir.** A lista aparece antes do motor terminar de ler os chats.";
        let pagina = || crate::atualizacao::pagina("0.3.0");
        let botoes = fileira()
            .child(Button::new("at-pronta").outline().w(r(200.)).label("Aviso: pronta").on_click(move |_, window, cx| {
                window.push_notification(atualizacao_ui::aviso_pronta("0.3.0", NOTAS.into(), pagina()), cx)
            }))
            .child(Button::new("at-disponivel").outline().w(r(200.)).label("Aviso: só avisar").on_click(move |_, window, cx| {
                window.push_notification(atualizacao_ui::aviso_disponivel("0.3.0", NOTAS.into(), pagina()), cx)
            }))
            .child(Button::new("at-atualizado").outline().w(r(200.)).label("Aviso: atualizado").on_click(|_, window, cx| {
                let aviso = Aviso::Atualizada { versao: crate::atualizacao::VERSAO.into() };
                let nota = atualizacao_ui::aviso_ao_abrir(&aviso, cx);
                window.push_notification(nota, cx)
            }))
            .child(Button::new("at-voltou").outline().w(r(200.)).label("Aviso: voltou").on_click(|_, window, cx| {
                let aviso = Aviso::Voltou { de: "0.3.0".into(), para: "0.2.0".into(), automatica: true };
                let nota = atualizacao_ui::aviso_ao_abrir(&aviso, cx);
                window.push_notification(nota, cx)
            }))
            .child(Button::new("at-novidades").outline().w(r(200.)).label("Novidades").on_click(move |_, window, cx| {
                atualizacao_ui::abrir_novidades("0.3.0", NOTAS.into(), pagina(), window, cx)
            }))
            .child(Button::new("at-reiniciar").outline().w(r(200.)).label("Reiniciar com 2 conversas").on_click(|_, window, cx| {
                let mesmo_assim = |window: &mut Window, cx: &mut App| {
                    window.push_notification(aviso(Tom::Atencao, "Reiniciar mesmo assim (na vitrine, nada reinicia)"), cx)
                };
                confirmar(confirmacao_de_parar(2, Parar::Reiniciar), mesmo_assim, None, window, cx)
            }));
        let grupos = atualizacao_ui::vistas_de_exemplo(crate::modelo::agora()).into_iter().map(|(nome, vista)| {
            div()
                .flex()
                .flex_col()
                .gap(r(8.))
                .w(r(640.))
                .child(legenda(nome))
                .child(atualizacao_ui::grupo(&vista, 2, false, window, cx))
        });
        um(
            "Atualizações",
            cx,
            div().flex().flex_col().gap(r(16.)).child(botoes).child(div().flex().flex_wrap().gap(r(16.)).children(grupos)),
        )
    }

    fn dialogos(&self, window: &mut Window, cx: &mut App) -> Div {
        let estatico = div()
            .flex()
            .flex_col()
            .gap(r(16.))
            .w(r(560.))
            .p(r(24.))
            .rounded(raio::PECA)
            .bg(Cores::dialogo())
            .border_1()
            .border_color(Cores::borda_forte())
            .shadow(sombra_elevado())
            .child(CabecalhoDialogo::new("Novo projeto").subtitulo("Cria uma pasta em ~/Projetos").voltar(|_, _| {}))
            .child(
                CabecalhoDialogo::new("Contas do Claude")
                    .simbolo(Marca::Claude.disco(32.))
                    .subtitulo(format!(
                        "Larguras: alerta {}, P {}, M {}, G {}",
                        largura::ALERTA, largura::P, largura::M, largura::G
                    ))
                    .sem_fechar(),
            )
            .child(Campo::new("Nome", legenda("(campo)")))
            .child(
                RodapeDialogo::new()
                    .esquerda(botao_fantasma("d-git", Some(Lucide::GitBranch.into()), "Ativar git", TamanhoBotao::Controle, window, cx))
                    .cancelar(botao_fantasma("d-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx))
                    .principal(botao_primario("d-ok", None, "Criar projeto", TamanhoBotao::Controle, window, cx)),
            );
        let tons = [
            (TomConfirmacao::Neutro, Lucide::GitBranch, "Ativar git em “loja”?", false),
            (TomConfirmacao::Atencao, Lucide::TriangleAlert, "2 conversas estão trabalhando", false),
            (TomConfirmacao::Perigo, Lucide::Trash, "Apagar a cópia do chat 2?", true),
        ];
        let botoes = fileira().children(tons.map(|(tom, icone, titulo, perigo)| {
            Button::new(SharedString::from(format!("conf-{titulo}")))
                .outline()
                .label(format!("Confirmar {tom:?}"))
                .on_click(move |_, window, cx| {
                    confirmar(
                        Confirmacao {
                            tom,
                            icone: icone.into(),
                            titulo: titulo.into(),
                            texto: "O que já estiver na cópia fica no branch até você apagar.".into(),
                            detalhes: vec![
                                (Lucide::GitBranch.into(), "vitral/t2-cupom".into(), false),
                                (Lucide::TriangleAlert.into(), "1,2 GB em node_modules".into(), true),
                            ],
                            ok: if perigo { "Apagar a cópia".into() } else { "Continuar".into() },
                            cancelar: "Cancelar".into(),
                            ok_perigo: perigo,
                        },
                        |_, _| {},
                        None,
                        window,
                        cx,
                    )
                })
        }));
        let tela = div()
            .h(r(320.))
            .flex()
            .rounded(raio::PECA)
            .border_1()
            .border_color(Cores::linha())
            .child(
                TelaCheia::new("Vitral")
                    .simbolo(Marca::Claude.disco(40.))
                    .texto("Ligue uma pasta ao projeto ou deixe o Vitral criar uma.")
                    .child(botao_primario("tela-ok", Some(Lucide::FolderOpen.into()), "Escolher pasta…", TamanhoBotao::Controle, window, cx)),
            );
        um("Diálogos", cx, div().flex().flex_col().gap(r(16.)).child(estatico).child(botoes).child(tela))
    }

    fn medidas(&self, cx: &mut App) -> Div {
        let alturas = [
            ("mini", altura::MINI),
            ("ação de linha", altura::ACAO_LINHA),
            ("barra", altura::BARRA),
            ("controle", altura::CONTROLE),
            ("grande", altura::GRANDE),
            ("cabeçalho", altura::CABECALHO),
            ("escolha 2", altura::ESCOLHA_2),
            ("linha 2", altura::LINHA_2),
        ];
        let espacos = [
            esp::E2, esp::E4, esp::E6, esp::E8, esp::E12, esp::E16, esp::E24, esp::E32,
        ];
        let amostra = |nome: &'static str| {
            div()
                .size(r(96.))
                .p(r(8.))
                .rounded(raio::CARTAO)
                .flex()
                .items_end()
                .metadado()
                .child(nome)
        };
        let superficies = fileira()
            .gap(r(16.))
            .child(amostra("vidro").bg(Cores::vidro()))
            .child(amostra("vidro de leitura").bg(Cores::vidro_leitura()))
            .child(amostra("cartão").bg(Cores::cartao()))
            .child(amostra("elevado").bg(Cores::cartao_elevado()).shadow(sombra_elevado()))
            .child(amostra("flutuante").bg(Cores::flutuante()).shadow(sombra_flutuante()))
            .child(amostra("diálogo").bg(Cores::dialogo()).shadow(sombra_dialogo()))
            .child(amostra("preto").bg(Cores::preto()).border_1().border_color(Cores::borda()))
            .child(amostra("recuo").bg(Cores::recuo()).shadow(sombra_recuo()))
            .child(amostra("grupo").bg(Cores::grupo()).border_1().border_color(Cores::borda()))
            .child(amostra("véu").bg(Cores::veu()))
            .child(
                amostra("caixa")
                    .relative()
                    .rounded(raio::PECA)
                    .bg(gradiente_quente())
                    .border_1()
                    .border_color(Cores::brilho_topo())
                    .shadow(vec![BoxShadow::new(px(0.), px(6.), Cores::sombra_caixa()).blur_radius(px(10.))])
                    .child(div().absolute().top_0().left_0().size_full().rounded(raio::PECA - px(1.)).bg(Cores::caixa_brilho())),
            );
        let conteudo = div()
            .flex()
            .flex_col()
            .gap(r(16.))
            .child(fileira().items_end().children(alturas.map(|(nome, h)| {
                div().flex().flex_col().items_center().gap(r(4.)).child(div().w(r(24.)).h(h).rounded(raio::PILULA).bg(Cores::pressionado())).child(legenda(nome))
            })))
            .child(fileira().children(espacos.map(|e| div().w(e).h(r(16.)).bg(Cores::destaque()))))
            .child(superficies)
            .child(legenda(format!(
                "{} · {} · {} · {} · {}",
                formatar_custo(0.42),
                formatar_custo(0.004),
                formatar_custo(1234.56),
                contagem(3, "chat", "chats"),
                truncar_no_meio("/home/victor/.cache/worktrees/loja-api/t2", 28)
            )))
            .child(legenda(dica_uso("Claude", Some(0.23), Some("renova em 2 h 13 min · 15:59"), Some(0.41), None)));
        um("Medidas, superfícies e textos", cx, conteudo)
    }

    fn codigo(&self, cx: &mut App) -> Div {
        let saida: String = (1..=50).map(|i| format!("test motor::testes::caso_{i:02} ... ok\n")).collect();
        let conteudo = div()
            .flex()
            .flex_col()
            .gap(r(12.))
            .w(r(640.))
            .child(CaixaDeCodigo::new("cod-ok", saida.into()).comando("cargo test").tempo("PASSOU em 12 s"))
            .child(
                CaixaDeCodigo::new("cod-erro", "error[E0308]: mismatched types\n  --> src/motor.rs:42:9".into())
                    .comando("cargo check")
                    .tempo("FALHOU em 3 s")
                    .erro(true)
                    .raio_chat(),
            )
            .child(div().h(r(40.)).rounded(raio::CARTAO).bg(gradiente_destaque()));
        um("Código e gradientes", cx, conteudo)
    }
}

impl Render for Vitrine {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::ui::contar_quadro();
        let tema = cx.theme();
        let fonte = tema.font_family.clone();
        let cor = tema.foreground;
        let topo = div()
            .flex()
            .items_center()
            .gap(r(12.))
            .h(r(44.))
            .px(r(16.))
            .child(
                div()
                    .id("arrastar")
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .titulo_tela()
                    .child("Vitrine da linguagem visual")
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
            )
            .child(legenda("Ctrl + roda ou Ctrl+= / Ctrl+- mudam o zoom"))
            .child(
                botao_icone("fechar-vitrine", Lucide::X, "Fechar", TamanhoBotao::Barra, window, cx)
                    .on_click(|_, _, cx| cx.quit()),
            );
        let corpo = div()
            .id("vitrine")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.rolagem)
            .p(r(16.))
            .flex()
            .flex_col()
            .gap(r(32.))
            .child(self.atualizacoes(window, cx))
            .child(self.tipografia(window, cx))
            .child(self.chips(window, cx))
            .child(self.botoes(window, cx))
            .child(self.movimento(window, cx))
            .child(self.espacamento(cx))
            .child(self.segmentados(window, cx))
            .child(self.campos(window, cx))
            .child(self.popovers(window, cx))
            .child(self.menus(window, cx))
            .child(self.alertas(window, cx))
            .child(self.vazios(cx))
            .child(self.listas(window, cx))
            .child(self.opcoes(cx))
            .child(self.escolha(cx))
            .child(self.estados(window, cx))
            .child(self.graficos(window, cx))
            .child(self.base(window, cx))
            .child(self.paineis(window, cx))
            .child(self.dialogos(window, cx))
            .child(self.medidas(cx))
            .child(self.codigo(cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family(fonte)
            .text_color(cor)
            .child(zoom::captura_roda())
            .children(zoom::pilula(cx))
            .child(topo)
            .child(corpo)
    }
}
