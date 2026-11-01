//! Configurações da pasta de trabalho (valem para as conversas que não escolheram outra coisa),
//! em grupos de linhas como as preferências do GNOME: o texto à esquerda e o controle à direita
//! (embaixo do texto no diálogo estreito). Grava na hora. As Animações e as Atualizações, no fim,
//! valem para o app todo (ficam em `interface.json` e em `atualizacoes.json`).

use std::rc::Rc;

use gpui_kit::component::{
    Disableable as _, Size, Sizable as _,
    menu::{DropdownMenu, PopupMenuItem},
    spinner::Spinner,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    composer::{DESCRICOES_DE_MODELO, MODELOS, MODOS},
    componentes::{
        Chip, Contador, ControleSegmentado, GatilhoChip, GrupoDeOpcoes, LinhaDeOpcao, Marca, Simbolo, TamanhoBotao,
        TamanhoChip, botao_perigo, icone_do_modo, item_descrito, largura,
    },
    atualizacao::{Vista, grupo as grupo_de_atualizacoes},
    dialogos::estreito,
    limpeza::{Pergunta, confirmar_limpeza},
    movimento::{self, Animacoes},
};
use crate::{
    atualizacao::{self, Acompanhando, Atualizacoes},
    git,
    modelo::Config,
    motor::Motor,
    tema::{Cores, Lucide, r, tam},
};

const PRAZOS_ARQUIVAR: [(u32, &str); 4] = [(0, "Nunca"), (3, "3 dias"), (7, "7 dias"), (30, "30 dias")];
const PRAZOS_COPIAS: [(u32, &str); 3] = [(0, "Nunca"), (7, "7 dias"), (30, "30 dias")];

pub struct Configuracoes {
    motor: Entity<Motor>,
    /// Espaço de .vitral/worktrees; None enquanto mede.
    espaco: Option<u64>,
    /// Quantas cópias existiam na última medida: quando muda, mede de novo.
    copias: usize,
    _medicao: Task<()>,
    _observa: Subscription,
    _atualizacoes: Subscription,
    /// Aberta, o download conta o progresso (no máximo 2 vezes por segundo).
    _acompanhando: Option<Acompanhando>,
}

impl Configuracoes {
    pub fn new(motor: Entity<Motor>, cx: &mut Context<Self>) -> Self {
        let _observa = cx.observe(&motor, |este: &mut Self, motor, cx| {
            if contar_copias(motor.read(cx)) != este.copias {
                este.medir(cx);
            }
            cx.notify()
        });
        let _atualizacoes = cx.observe_global::<Atualizacoes>(|_, cx| cx.notify());
        let _acompanhando = atualizacao::acompanhar_progresso(cx);
        let mut este = Self {
            motor,
            espaco: None,
            copias: 0,
            _medicao: Task::ready(()),
            _observa,
            _atualizacoes,
            _acompanhando,
        };
        este.medir(cx);
        este
    }

    /// Mede as cópias em segundo plano (node_modules tem centenas de milhares de arquivos).
    fn medir(&mut self, cx: &mut Context<Self>) {
        let motor = self.motor.read(cx);
        self.copias = contar_copias(motor);
        self.espaco = None;
        let pasta = motor.disco.pasta().join("worktrees");
        self._medicao = cx.spawn(async move |este, cx| {
            let bytes = cx.background_spawn(async move { git::espaco_ocupado(&pasta) }).await;
            let _ = este.update(cx, |este, cx| {
                este.espaco = Some(bytes);
                cx.notify();
            });
        });
    }
}

fn contar_copias(motor: &Motor) -> usize {
    motor.tarefas.iter().filter(|t| t.worktree.is_some()).count()
}

/// Uma opção de um menu de escolha: o valor gravado, o nome e, se houver, a descrição e o
/// símbolo (que também vai no chip).
pub(super) struct Opcao {
    pub valor: String,
    pub nome: SharedString,
    pub descricao: Option<SharedString>,
    pub simbolo: Option<Simbolo>,
}

impl Opcao {
    pub(super) fn new(valor: impl Into<String>, nome: impl Into<SharedString>) -> Self {
        Opcao { valor: valor.into(), nome: nome.into(), descricao: None, simbolo: None }
    }

    pub(super) fn descricao(mut self, d: impl Into<SharedString>) -> Self {
        self.descricao = Some(d.into());
        self
    }

    pub(super) fn simbolo(mut self, s: Simbolo) -> Self {
        self.simbolo = Some(s);
        self
    }
}

/// Chip médio com a escolha atual que abre o menu das opções. O rótulo fica tênue no padrão da
/// pasta e claro quando mudou; com descrição, os itens têm duas linhas e o menu 400 de largura.
/// A âncora segue o lado do chip (à direita na linha, à esquerda embaixo do texto).
pub(super) fn menu_escolha(
    id: impl Into<ElementId>,
    ancora: Anchor,
    opcoes: Vec<Opcao>,
    atual: &str,
    padrao: &str,
    escolher: impl Fn(String, &mut Window, &mut App) + 'static,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let id: ElementId = id.into();
    let escolhida = opcoes.iter().find(|o| o.valor == atual);
    let nome: SharedString = escolhida.map(|o| o.nome.clone()).unwrap_or_else(|| {
        if atual.is_empty() { "Escolher".into() } else { atual.to_string().into() }
    });
    let simbolo = escolhida.and_then(|o| o.simbolo.as_ref());
    let chip = |aberto: bool| {
        let chip = Chip::new(id.clone(), nome.clone())
            .tamanho(TamanhoChip::Medio)
            .mudado(atual != padrao)
            .seta(true)
            .aberto(aberto);
        match simbolo.and_then(Simbolo::copia) {
            Some(s) => chip.simbolo(s),
            None => chip,
        }
        .construir(window, cx)
    };
    let gatilho = GatilhoChip::new(chip(false), chip(true));
    let (opcoes, atual, escolher) = (Rc::new(opcoes), atual.to_string(), Rc::new(escolher));
    gatilho
        .dropdown_menu_with_anchor(ancora, move |mut menu, window, _| {
            let com_descricao = opcoes.iter().any(|o| o.descricao.is_some());
            for opcao in opcoes.iter() {
                let marcado = opcao.valor == atual;
                let simbolo = opcao.simbolo.as_ref().and_then(Simbolo::copia);
                let item = match (&opcao.descricao, simbolo) {
                    (Some(d), simbolo) => item_descrito(simbolo, opcao.nome.clone(), d.clone(), marcado),
                    (None, Some(simbolo)) => {
                        let nome = opcao.nome.clone();
                        PopupMenuItem::element(move |window, _| {
                            div()
                                .flex()
                                .items_center()
                                .gap(r(8.))
                                .children(simbolo.copia().map(|s| s.desenhar(16., window)))
                                .child(nome.clone())
                        })
                        .checked(marcado)
                    }
                    (None, None) => PopupMenuItem::new(opcao.nome.clone()).checked(marcado),
                };
                let (escolher, valor) = (escolher.clone(), opcao.valor.clone());
                menu = menu.item(item.on_click(move |_, window, cx| escolher(valor.clone(), window, cx)));
            }
            if com_descricao {
                menu.min_w(tam(window, 400.)).max_w(tam(window, 400.))
            } else {
                menu.min_w(tam(window, 220.))
            }
        })
        .into_any_element()
}

/// Valores vizinhos de `atual` na lista (o de baixo e o de cima), mesmo se `atual` não estiver nela.
fn vizinhos(lista: &[u32], atual: u32) -> (Option<u32>, Option<u32>) {
    (lista.iter().rev().find(|v| **v < atual).copied(), lista.iter().find(|v| **v > atual).copied())
}

/// − número +, andando pelos valores da lista; cada botão desliga na ponta.
fn contador(
    id: &'static str,
    lista: &'static [u32],
    atual: u32,
    motor: Entity<Motor>,
    aplicar: fn(&mut Config, u32),
) -> Contador {
    let (menor, maior) = (lista[0].min(atual), lista[lista.len() - 1].max(atual));
    Contador::new(id, atual, menor..=maior, move |novo, _, cx| {
        let (abaixo, acima) = vizinhos(lista, atual);
        if let Some(valor) = if novo < atual { abaixo } else { acima } {
            motor.update(cx, |m, cx| m.mudar_config(|c| aplicar(c, valor), cx));
        }
    })
}

/// Grava o valor escolhido num menu na configuração da pasta.
fn gravar(motor: Entity<Motor>, aplicar: fn(&mut Config, String)) -> impl Fn(String, &mut Window, &mut App) + 'static {
    move |valor, _, cx| motor.update(cx, |m, cx| m.mudar_config(|c| aplicar(c, valor), cx))
}

/// Prazo em dias: segmentado; no diálogo estreito com mais de 3 opções, ou com um valor fora da
/// lista (editado à mão), o menu.
fn prazo(
    id: &'static str,
    prazos: &'static [(u32, &'static str)],
    atual: u32,
    estreito: bool,
    motor: Entity<Motor>,
    aplicar: fn(&mut Config, u32),
    window: &Window,
    cx: &App,
) -> AnyElement {
    match prazos.iter().position(|(v, _)| *v == atual) {
        Some(i) if !(estreito && prazos.len() > 3) => {
            ControleSegmentado::new(id, prazos.iter().map(|(_, nome)| *nome), i, move |novo, _, cx| {
                let valor = prazos[novo].0;
                motor.update(cx, |m, cx| m.mudar_config(|c| aplicar(c, valor), cx));
            })
            .with_size(Size::Small)
            .largura_total(estreito)
            .into_any_element()
        }
        _ => {
            let opcoes = prazos
                .iter()
                .map(|(v, nome)| Opcao::new(v.to_string(), if *v == 0 { nome.to_string() } else { format!("Depois de {nome}") }))
                .collect();
            menu_escolha(
                id,
                if estreito { Anchor::TopLeft } else { Anchor::TopRight },
                opcoes,
                &atual.to_string(),
                "0",
                move |valor, _, cx| {
                    let valor = valor.parse().unwrap_or(0);
                    motor.update(cx, |m, cx| m.mudar_config(|c| aplicar(c, valor), cx));
                },
                window,
                cx,
            )
        }
    }
}

/// O menu de modelo: a marca, o nome e a descrição de cada família; o Padrão diz de onde vem.
fn opcoes_de_modelo(padrao: &'static str) -> Vec<Opcao> {
    MODELOS
        .iter()
        .enumerate()
        .map(|(i, (valor, nome))| {
            let descricao = if i == 0 { padrao } else { DESCRICOES_DE_MODELO[i - 1] };
            Opcao::new(*valor, *nome).descricao(descricao).simbolo(Simbolo::Marca(Marca::Claude))
        })
        .collect()
}

/// "1,2 GB no total · 3 de tarefas encerradas".
fn resumo_das_copias(espaco: Option<u64>, encerradas: usize) -> String {
    let Some(espaco) = espaco.map(git::tamanho) else {
        return "Medindo…".into();
    };
    match encerradas {
        0 => format!("{espaco} no total · nenhuma de tarefa encerrada"),
        1 => format!("{espaco} no total · 1 de tarefa encerrada"),
        n => format!("{espaco} no total · {n} de tarefas encerradas"),
    }
}

impl Render for Configuracoes {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let estreito = estreito(window, largura::G);
        let ancora = if estreito { Anchor::TopLeft } else { Anchor::TopRight };
        let config = self.motor.read(cx).config.clone();
        let padrao = Config::default();
        let m = || self.motor.clone();
        // No diálogo estreito, o controle desce para baixo do texto.
        let com = |linha: LinhaDeOpcao, controle: AnyElement| {
            if estreito { linha.campo(controle) } else { linha.controle(controle) }
        };

        let modos = MODOS
            .iter()
            .map(|(valor, nome, descricao)| {
                let (icone, cor) = icone_do_modo(valor, false);
                Opcao::new(*valor, *nome).descricao(*descricao).simbolo(Simbolo::Icone(icone, Some(cor)))
            })
            .collect();
        let conversas = GrupoDeOpcoes::new(Some("Conversas".into()))
            .linha(com(
                LinhaDeOpcao::new("cfg-modo", "Modo de permissão padrão")
                    .info("“Sem perguntar” é o mesmo do seu VS Code. Nos outros, os pedidos de permissão aparecem na conversa.")
                    .descricao("Cada conversa pode trocar o seu na caixa de mensagem."),
                menu_escolha("cfg-permissao", ancora, modos, &config.permissao, &padrao.permissao, gravar(m(), |c, v| c.permissao = v), window, cx),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-modelo-coord", "Modelo dos coordenadores")
                    .descricao("“Padrão” usa o modelo do seu Claude Code."),
                menu_escolha(
                    "cfg-modelo-coord",
                    ancora,
                    opcoes_de_modelo("O do seu Claude Code"),
                    &config.modelo_coordenador,
                    "",
                    gravar(m(), |c, v| c.modelo_coordenador = v),
                    window,
                    cx,
                ),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-arquivar", "Arquivar conversas paradas")
                    .info(
                        "Vale para as conversas sem novidade há esse tempo, já lidas, sem fila e com todas as \
                         tarefas encerradas. As fixadas ficam.",
                    )
                    .descricao("Nada é apagado: elas vão para Arquivadas."),
                prazo("cfg-arquivar", &PRAZOS_ARQUIVAR, config.arquivar_apos_dias, estreito, m(), |c, v| {
                    c.arquivar_apos_dias = v
                }, window, cx),
            ));

        let mcp = config.mcp_trabalhadores;
        let alternar_mcp = m();
        let trabalhadores = GrupoDeOpcoes::new(Some("Trabalhadores".into()))
            .linha(com(
                LinhaDeOpcao::new("cfg-modelo-trab", "Modelo").descricao("O coordenador ainda pode escolher outro por tarefa."),
                menu_escolha(
                    "cfg-modelo-trab",
                    ancora,
                    opcoes_de_modelo("O do seu Claude Code"),
                    &config.modelo_trabalhador,
                    "",
                    gravar(m(), |c, v| c.modelo_trabalhador = v),
                    window,
                    cx,
                ),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-max", "Ao mesmo tempo").descricao("Os outros esperam na fila; cada um usa uns 300 MB."),
                contador("cfg-max", &[1, 2, 3, 4, 6, 8], config.max_trabalhadores as u32, m(), |c, v| {
                    c.max_trabalhadores = v as usize
                })
                .into_any_element(),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-rodadas", "Rodadas de ajuste antes de chamar você")
                    .descricao("Depois disso o coordenador para e explica o impasse."),
                contador("cfg-rodadas", &[1, 2, 3, 5, 8], config.max_rodadas, m(), |c, v| c.max_rodadas = v)
                    .into_any_element(),
            ))
            .linha(
                LinhaDeOpcao::new("cfg-mcp", "Servidores MCP do Claude Code")
                    .info(
                        "Ligado, os trabalhadores carregam também os servidores MCP do seu Claude Code. No Haiku, \
                         muitos servidores estouram o contexto.",
                    )
                    .descricao("Desligado, eles só têm as ferramentas do Vitral.")
                    .interruptor(mcp, move |ligado, _, cx| {
                        alternar_mcp.update(cx, |m, cx| m.mudar_config(|c| c.mcp_trabalhadores = ligado, cx))
                    }),
            );

        let revisor = config.revisor;
        let alternar_revisor = m();
        // Com o revisor desligado o modelo dele não vale: a linha fica apagada, sem menu.
        let modelo_revisor = LinhaDeOpcao::new("cfg-modelo-rev", "Modelo do revisor");
        let modelo_revisor = if revisor {
            com(
                modelo_revisor.descricao("“Padrão” usa o dos trabalhadores; outro dá uma segunda opinião."),
                menu_escolha(
                    "cfg-modelo-rev",
                    ancora,
                    opcoes_de_modelo("O dos trabalhadores"),
                    &config.modelo_revisor,
                    "",
                    gravar(m(), |c, v| c.modelo_revisor = v),
                    window,
                    cx,
                ),
            )
        } else {
            let nome = MODELOS
                .iter()
                .find(|(v, _)| *v == config.modelo_revisor)
                .map_or(config.modelo_revisor.clone(), |(_, n)| n.to_string());
            let chip = Chip::new("cfg-modelo-rev", nome)
                .tamanho(TamanhoChip::Medio)
                .simbolo(Simbolo::Marca(Marca::Claude))
                .mudado(!config.modelo_revisor.is_empty())
                .seta(true)
                .construir(window, cx)
                .disabled(true);
            com(modelo_revisor.desabilitada("Ligue o revisor para escolher."), chip.into_any_element())
        };
        let revisao = GrupoDeOpcoes::new(Some("Revisão".into()))
            .linha(
                LinhaDeOpcao::new("cfg-revisor", "Revisor independente")
                    .info(
                        "Cada entrega passa por uma sessão nova, só de leitura, que confere os critérios e \
                         procura problemas antes do coordenador decidir. Gasta um pouco mais do plano e pega o \
                         que o coordenador deixaria passar.",
                    )
                    .descricao("Uma sessão nova, só de leitura, confere cada entrega.")
                    .interruptor(revisor, move |ligado, _, cx| {
                        alternar_revisor.update(cx, |m, cx| m.mudar_config(|c| c.revisor = ligado, cx))
                    }),
            )
            .linha(modelo_revisor);

        let encerradas: Vec<u64> = self.motor.read(cx).copias_paradas(false).map(|t| t.id).collect();
        let n = encerradas.len();
        let apagar = m();
        let limpar = botao_perigo("cfg-limpar-copias", Some(Lucide::Trash.into()), format!("Limpar {n}"), TamanhoBotao::Barra, window, cx)
            .when(n == 0, |b| b.disabled(true).opacity(0.45).tooltip("Nenhuma cópia de tarefa encerrada"))
            .on_click(move |_, window, cx| {
                let pergunta = move |_: &str| Pergunta {
                    titulo: if n == 1 {
                        "Apagar a cópia da tarefa encerrada?".into()
                    } else {
                        format!("Apagar as {n} cópias das tarefas encerradas?")
                    },
                    texto: "As conversas dos trabalhadores ficam para leitura, mas não rodam mais.".into(),
                    botao: if n == 1 { "Apagar a cópia".into() } else { "Apagar as cópias".into() },
                };
                confirmar_limpeza(apagar.clone(), encerradas.clone(), pergunta, window, cx);
            });
        let copias = GrupoDeOpcoes::new(Some("Cópias das tarefas".into()))
            .linha(com(
                LinhaDeOpcao::new("cfg-copias", "Espaço ocupado")
                    .info(
                        "Cada tarefa trabalha numa cópia do projeto em .vitral/worktrees, com o que a preparação \
                         instala (node_modules, .venv). A cópia some quando a tarefa é aprovada; as das canceladas \
                         ficam até você limpar.",
                    )
                    .descricao(match self.espaco {
                        Some(_) => resumo_das_copias(self.espaco, n).into_any_element(),
                        None => div()
                            .flex()
                            .items_center()
                            .gap(r(6.))
                            .child(Spinner::new().with_size(Size::Size(tam(window, 12.))).color(Cores::tenue()))
                            .child(resumo_das_copias(None, n))
                            .into_any_element(),
                    }),
                limpar.into_any_element(),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-limpar-dias", "Apagar sozinho")
                    .descricao("Ao abrir a pasta, as das tarefas encerradas há mais tempo."),
                prazo("cfg-limpar-dias", &PRAZOS_COPIAS, config.limpar_copias_dias, estreito, m(), |c, v| {
                    c.limpar_copias_dias = v
                }, window, cx),
            ));

        let (animacoes, sistema_sem) = movimento::animacoes(cx);
        let segmentos = ["Seguir o GNOME", "Desligadas"];
        let atual = usize::from(animacoes == Animacoes::Desligadas);
        let interface = GrupoDeOpcoes::new(Some("Interface".into())).linha(com(
            LinhaDeOpcao::new("cfg-animacoes", "Animações")
                .info(
                    "Desligadas, os popovers, os menus, as mensagens novas, a pílula dos segmentados e os anéis \
                     aparecem direto no lugar, e o spinner fica parado.",
                )
                .descricao(if animacoes == Animacoes::SeguirGnome && sistema_sem {
                    "O GNOME está sem animações: tudo aparece pronto."
                } else {
                    "Vale para o app todo, em todas as pastas."
                }),
            ControleSegmentado::new("cfg-animacoes", segmentos, atual, |novo, _, cx| {
                let escolha = if novo == 1 { Animacoes::Desligadas } else { Animacoes::SeguirGnome };
                movimento::escolher_animacoes(escolha, cx);
            })
            .with_size(Size::Small)
            .largura_total(estreito)
            .into_any_element(),
        ));

        let trabalhando = self.motor.read(cx).trabalhando();
        let atualizacoes = Vista::da_global(cx).map(|vista| grupo_de_atualizacoes(&vista, trabalhando, estreito, window, cx));

        div()
            .flex()
            .flex_col()
            .gap(r(24.))
            .child(conversas)
            .child(trabalhadores)
            .child(revisao)
            .child(copias)
            .child(interface)
            .children(atualizacoes)
    }
}

#[cfg(test)]
mod testes {
    use super::{resumo_das_copias, vizinhos};

    #[test]
    fn contador_anda_pela_lista_e_para_nas_pontas() {
        let lista = [1, 2, 3, 4, 6, 8];
        assert_eq!(vizinhos(&lista, 4), (Some(3), Some(6)));
        assert_eq!(vizinhos(&lista, 1), (None, Some(2)));
        assert_eq!(vizinhos(&lista, 8), (Some(6), None));
        // Valor que não está na lista (editado à mão): anda para o vizinho mais perto.
        assert_eq!(vizinhos(&lista, 5), (Some(4), Some(6)));
        assert_eq!(vizinhos(&lista, 20), (Some(8), None));
    }

    #[test]
    fn espaco_das_copias_sem_plural_errado() {
        assert_eq!(resumo_das_copias(None, 3), "Medindo…");
        assert!(resumo_das_copias(Some(0), 0).ends_with(" no total · nenhuma de tarefa encerrada"));
        assert!(resumo_das_copias(Some(0), 1).ends_with(" · 1 de tarefa encerrada"));
        assert!(resumo_das_copias(Some(0), 3).ends_with(" · 3 de tarefas encerradas"));
    }
}
