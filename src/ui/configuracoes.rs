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
        TamanhoChip, botao_fantasma, icone_do_modo, item_descrito, largura,
    },
    atualizacao::{Vista, grupo as grupo_de_atualizacoes},
    dialogos::estreito,
    movimento::{self, Animacoes},
};
use crate::{
    atualizacao::{self, Acompanhando, Atualizacoes},
    claude::{self, Atualizacao, Versao},
    modelo::Config,
    motor::{self, Motor},
    tema::{Cores, Lucide, r, tam},
};

const PRAZOS_ARQUIVAR: [(u32, &str); 4] = [(0, "Nunca"), (3, "3 dias"), (7, "7 dias"), (30, "30 dias")];

/// O que a linha do grupo Claude Code diz, e se o Atualizar dá para clicar.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct LinhaDoClaude {
    /// A primeira linha: a versão instalada, ou o que está acontecendo agora.
    pub texto: String,
    /// O spinner antes do texto.
    pub andando: bool,
    /// O que o `claude update` respondeu, linha por linha.
    pub resposta: Vec<String>,
    /// A resposta foi de erro: ela vai em âmbar.
    pub falhou: bool,
    /// Por que o Atualizar não dá para clicar agora (`None`: dá).
    pub espera: Option<String>,
}

/// A linha do Claude Code: a versão que os turnos abrem e o estado do `claude update`. O botão
/// espera os chats porque a atualização troca o binário no disco (e no Windows ela nem roda com o
/// `claude.exe` em uso); quem está no meio de um turno continua no processo que já abriu.
pub(super) fn linha_do_claude(
    versao: Option<Versao>,
    a_caminho: usize,
    andando: bool,
    resposta: Option<&Atualizacao>,
) -> LinhaDoClaude {
    let texto = match (andando, versao) {
        (true, _) => "Rodando o claude update…".to_string(),
        (false, Some(v)) => format!("{} · a versão que os turnos abrem", motor::texto_da_versao(v)),
        // Sem a versão o Atualizar continua valendo: ele abre o mesmo `claude` dos turnos.
        (false, None) => "Não deu para ler a versão instalada".to_string(),
    };
    let mut linhas: Vec<String> = resposta.map(|r| r.linhas.clone()).unwrap_or_default();
    if resposta.is_some_and(claude::instalou_versao_nova) {
        linhas.push("A versão nova vale no próximo turno; o Vitral não precisa reiniciar.".into());
    }
    let espera = if andando {
        Some("A atualização está rodando".to_string())
    } else if a_caminho > 0 {
        let (quantos, eles) = if a_caminho == 1 {
            ("1 chat está trabalhando".to_string(), "ele terminar")
        } else {
            (format!("{a_caminho} chats estão trabalhando"), "eles terminarem")
        };
        Some(format!("{quantos} ou com mensagem na fila: o Atualizar espera {eles}"))
    } else {
        None
    };
    LinhaDoClaude { texto, andando, resposta: linhas, falhou: resposta.is_some_and(|r| !r.ok), espera }
}

pub struct Configuracoes {
    motor: Entity<Motor>,
    _observa: Subscription,
    _atualizacoes: Subscription,
    /// Aberta, o download conta o progresso (no máximo 2 vezes por segundo).
    _acompanhando: Option<Acompanhando>,
}

impl Configuracoes {
    pub fn new(motor: Entity<Motor>, cx: &mut Context<Self>) -> Self {
        let _observa = cx.observe(&motor, |_, _, cx| cx.notify());
        let _atualizacoes = cx.observe_global::<Atualizacoes>(|_, cx| cx.notify());
        let _acompanhando = atualizacao::acompanhar_progresso(cx);
        // A linha do Claude Code mostra a versão instalada: ela pode ter mudado por fora (ou pelo
        // próprio Atualizar, numa abertura anterior) desde a leitura da abertura do app.
        motor.update(cx, |m, cx| m.reler_versao_claude(cx));
        Self { motor, _observa, _atualizacoes, _acompanhando }
    }

    /// O grupo Claude Code das Configurações: a versão que os turnos abrem e o Atualizar.
    fn grupo_do_claude(&self, estreito: bool, window: &Window, cx: &Context<Self>) -> GrupoDeOpcoes {
        let motor = self.motor.read(cx);
        let linha = linha_do_claude(
            motor.versao_claude,
            motor.turnos_a_caminho(),
            motor.atualizando_claude,
            motor.resposta_do_claude.as_ref(),
        );
        let falhou = linha.falhou;
        let descricao = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(r(6.))
                    .when(linha.andando, |d| {
                        d.child(Spinner::new().with_size(Size::Size(tam(window, 12.))).color(Cores::tenue()))
                    })
                    .child(linha.texto.clone()),
            )
            .children(linha.resposta.iter().map(|t| {
                div()
                    .when(falhou, |d| d.text_color(Cores::ambar()))
                    .child(t.clone())
            }));
        let motor_e = self.motor.clone();
        let botao = botao_fantasma(
            "cfg-claude-atualizar",
            Some(Lucide::RefreshCw.into()),
            "Atualizar",
            TamanhoBotao::Barra,
            window,
            cx,
        )
        .when_some(linha.espera.clone(), |b, motivo| b.disabled(true).opacity(0.45).tooltip(motivo))
        .on_click(move |_, _, cx| motor_e.update(cx, |m, cx| m.atualizar_claude(cx)));
        let opcao = LinhaDeOpcao::new("cfg-claude", "Versão instalada")
            .info(
                "O Vitral abre um processo do claude por turno; a versão é a do claude que ele encontra (o do PATH \
                 ou o de ~/.local/bin). O Atualizar roda `claude update` fora de qualquer turno e mostra o que ele \
                 responder; a versão nova vale no turno seguinte, sem reiniciar o Vitral.",
            )
            .descricao(descricao);
        // O motivo de o botão esperar também fica à vista, em âmbar, sem precisar da dica.
        let opcao = match linha.espera.filter(|_| !linha.andando) {
            Some(motivo) => opcao.aviso(motivo),
            None => opcao,
        };
        let opcao = if estreito { opcao.campo(botao) } else { opcao.controle(botao) };
        GrupoDeOpcoes::new(Some("Claude Code".into())).linha(opcao)
    }
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
                let (icone, cor) = icone_do_modo(valor);
                Opcao::new(*valor, *nome).descricao(*descricao).simbolo(Simbolo::Icone(icone, Some(cor)))
            })
            .collect();
        let (mcp, alternar_mcp) = (config.mcp_trabalhadores, m());
        let (memoria, alternar_memoria) = (config.memoria_ao_terminar, m());
        let (uso_no_canto, alternar_uso) = (config.uso_no_canto, m());
        let conversas = GrupoDeOpcoes::new(Some("Chats".into()))
            .linha(com(
                LinhaDeOpcao::new("cfg-resumo", "Resumo ao voltar depois de")
                    .descricao("Minutos com a janela fora de foco antes do “Enquanto você esteve fora”."),
                contador("cfg-resumo", &[5, 10, 15, 30, 60, 120], config.minutos_para_resumo as u32, m(), |c, v| {
                    c.minutos_para_resumo = v as u64
                })
                .into_any_element(),
            ))
            .linha(
                LinhaDeOpcao::new("cfg-uso-no-canto", "Mostrar o uso do plano no canto da janela")
                    .descricao("Os anéis de uso ficam flutuando embaixo, à direita. Desligado, somem; o uso continua em Contas do Claude.")
                    .interruptor(uso_no_canto, move |ligado, _, cx| {
                        alternar_uso.update(cx, |m, cx| m.mudar_config(|c| c.uso_no_canto = ligado, cx))
                    }),
            )
            .linha(com(
                LinhaDeOpcao::new("cfg-modo", "Modo de permissão padrão")
                    .info("“Sem perguntar” é o mesmo do seu VS Code. Nos outros, os pedidos de permissão aparecem na conversa.")
                    .descricao("Cada conversa pode trocar o seu na caixa de mensagem."),
                menu_escolha("cfg-permissao", ancora, modos, &config.permissao, &padrao.permissao, gravar(m(), |c, v| c.permissao = v), window, cx),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-agente", "Quem responde nos chats novos")
                    .info("O Codex é o agente de código da OpenAI. Ele usa o login e o plano do próprio Codex, e precisa estar instalado neste computador.")
                    .descricao("Cada chat pode trocar o seu no seletor de modelo."),
                menu_escolha(
                    "cfg-agente",
                    ancora,
                    vec![
                        Opcao::new("claude", "Claude").descricao("Claude Code").simbolo(Simbolo::Marca(Marca::Claude)),
                        Opcao::new("codex", "Codex")
                            .descricao(if crate::agente_codex::instalado() { "Codex, da OpenAI" } else { "Não achei o Codex neste computador" })
                            .simbolo(Simbolo::Marca(Marca::Codex)),
                    ],
                    if config.agente_padrao == crate::modelo::Agente::Codex { "codex" } else { "claude" },
                    "claude",
                    gravar(m(), |c, v| c.agente_padrao = if v == "codex" { crate::modelo::Agente::Codex } else { crate::modelo::Agente::Claude }),
                    window,
                    cx,
                ),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-modelo-trab", "Modelo dos chats do Claude").descricao("Cada chat pode trocar o seu na caixa de mensagem."),
                menu_escolha(
                    "cfg-modelo-trab",
                    ancora,
                    opcoes_de_modelo("O do seu Claude Code"),
                    &config.modelo_chats,
                    "",
                    gravar(m(), |c, v| c.modelo_chats = v),
                    window,
                    cx,
                ),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-arquivar", "Arquivar chats parados depois de")
                    .info(
                        "Vale para os chats sem novidade há esse tempo, já lidos, sem fila e sem pedido aberto. Os \
                         fixados ficam.",
                    )
                    .descricao("Nada é apagado: eles vão para Arquivadas."),
                prazo("cfg-arquivar", &PRAZOS_ARQUIVAR, config.arquivar_apos_dias, estreito, m(), |c, v| {
                    c.arquivar_apos_dias = v
                }, window, cx),
            ))
            .linha(
                LinhaDeOpcao::new("cfg-mcp", "Servidores MCP do Claude Code")
                    .info(
                        "Ligado, os chats carregam também os servidores MCP do seu Claude Code. No Haiku, muitos \
                         servidores estouram o contexto.",
                    )
                    .descricao("Desligado, eles só têm as ferramentas do Vitral.")
                    .interruptor(mcp, move |ligado, _, cx| {
                        alternar_mcp.update(cx, |m, cx| m.mudar_config(|c| c.mcp_trabalhadores = ligado, cx))
                    }),
            )
            .linha({
                let ligada = !config.memoria_da_equipe.is_empty();
                let abrir = m();
                LinhaDeOpcao::new("cfg-memoria-equipe", "Memória da equipe")
                    .info(
                        "Um servidor ai-memory da empresa. Ligado, os chats do Claude ganham as ferramentas de memória e \
                         passam a consultar e a registrar o que vale para todos. Só o que o agente escreve de caso pensado \
                         sobe; o que você digita e a saída das ferramentas ficam neste computador.",
                    )
                    .descricao(if ligada { format!("Ligada a {}", config.memoria_da_equipe) } else { "Desligada".to_string() })
                    .controle(
                        botao_fantasma("cfg-memoria-equipe-abrir", None, if ligada { "Trocar…" } else { "Ligar…" }, TamanhoBotao::Controle, window, cx)
                            .on_click(move |_, window, cx| super::memoria_equipe::abrir(abrir.clone(), window, cx)),
                    )
            })
            .linha(
                LinhaDeOpcao::new("cfg-memoria", "Rever a memória ao terminar")
                    .info(
                        "Quando um turno que mexeu em arquivo termina, o Vitral pede ao agente, uma vez por sessão, \
                         para ver se algo vale guardar na memória ou nas instruções do projeto. Quase sempre a \
                         resposta é que não há nada a registrar.",
                    )
                    .descricao("Vale para o Claude e para o Codex.")
                    .interruptor(memoria, move |ligado, _, cx| {
                        alternar_memoria.update(cx, |m, cx| m.mudar_config(|c| c.memoria_ao_terminar = ligado, cx))
                    }),
            );

        let (animacoes, sistema_sem) = movimento::animacoes(cx);
        let segmentos = [movimento::SEGUIR_O_SISTEMA, "Desligadas"];
        let atual = usize::from(animacoes == Animacoes::Desligadas);
        let interface = GrupoDeOpcoes::new(Some("Interface".into())).linha(com(
            LinhaDeOpcao::new("cfg-animacoes", "Animações")
                .info(
                    "Desligadas, os popovers, os menus, as mensagens novas, a pílula dos segmentados e os anéis \
                     aparecem direto no lugar, e o spinner fica parado.",
                )
                .descricao(if animacoes == Animacoes::SeguirGnome && sistema_sem {
                    movimento::SISTEMA_SEM_ANIMACOES
                } else {
                    "Vale para o app todo."
                }),
            ControleSegmentado::new("cfg-animacoes", segmentos, atual, |novo, _, cx| {
                let escolha = if novo == 1 { Animacoes::Desligadas } else { Animacoes::SeguirGnome };
                movimento::escolher_animacoes(escolha, cx);
            })
            .with_size(Size::Small)
            .largura_total(estreito)
            .into_any_element(),
        ));

        let mudar_transparencia = m();
        let interface = interface.linha(com(
            LinhaDeOpcao::new("cfg-transparencia", "Transparência dos fundos")
                .info(
                    "Quanto o fundo azul da lista e o cinza da conversa deixam ver o que está atrás da janela.                      Com papel de parede claro, os níveis de cima deixam o texto mais fraco difícil de ler.",
                )
                .descricao("O pé da conversa, onde fica a caixa de mensagem, continua fechado."),
            ControleSegmentado::new(
                "cfg-transparencia",
                crate::tema::NOMES_DA_TRANSPARENCIA,
                usize::from(config.transparencia.min(3)),
                move |novo, _, cx| mudar_transparencia.update(cx, |m, cx| m.mudar_config(|c| c.transparencia = novo as u8, cx)),
            )
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
            .child(interface)
            .child(self.grupo_do_claude(estreito, window, cx))
            .children(atualizacoes)
    }
}

#[cfg(test)]
mod testes {
    use super::{Atualizacao, linha_do_claude, vizinhos};

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

    fn resposta(ok: bool, linhas: &[&str]) -> Atualizacao {
        Atualizacao { ok, linhas: linhas.iter().map(|l| l.to_string()).collect() }
    }

    #[test]
    fn a_linha_do_claude_code_diz_a_versao_e_o_que_esta_acontecendo() {
        let parada = linha_do_claude(Some((2, 1, 318)), 0, false, None);
        assert_eq!(parada.texto, "2.1.318 · a versão que os turnos abrem");
        assert!(parada.espera.is_none() && parada.resposta.is_empty() && !parada.falhou);
        // Sem conseguir ler a versão, o Atualizar continua valendo.
        assert_eq!(linha_do_claude(None, 0, false, None).texto, "Não deu para ler a versão instalada");
        assert!(linha_do_claude(None, 0, false, None).espera.is_none());
        let rodando = linha_do_claude(Some((2, 1, 318)), 0, true, None);
        assert_eq!(rodando.texto, "Rodando o claude update…");
        assert!(rodando.andando && rodando.espera.is_some());
    }

    #[test]
    fn o_atualizar_espera_os_chats() {
        let um = linha_do_claude(Some((2, 1, 318)), 1, false, None).espera.unwrap();
        assert_eq!(um, "1 chat está trabalhando ou com mensagem na fila: o Atualizar espera ele terminar");
        let tres = linha_do_claude(Some((2, 1, 318)), 3, false, None).espera.unwrap();
        assert_eq!(tres, "3 chats estão trabalhando ou com mensagem na fila: o Atualizar espera eles terminarem");
    }

    #[test]
    fn a_resposta_do_claude_update_vai_inteira_na_linha() {
        // Em dia: o que ele imprimiu, sem a nota do turno seguinte (nada foi instalado).
        let em_dia = resposta(true, &["Current version: 2.1.318", "Claude Code is up to date (2.1.318)"]);
        let linha = linha_do_claude(Some((2, 1, 318)), 0, false, Some(&em_dia));
        assert_eq!(linha.resposta, em_dia.linhas);
        assert!(!linha.falhou);
        // Instalou: a nota entra, porque a versão nova só vale no turno seguinte.
        let nova = resposta(true, &["Successfully updated from 2.1.318 to version 2.1.320"]);
        let linha = linha_do_claude(Some((2, 1, 320)), 0, false, Some(&nova));
        assert_eq!(linha.resposta.len(), 2);
        assert!(linha.resposta[1].starts_with("A versão nova vale no próximo turno"));
        assert!(!linha.falhou);
        // Falhou: o texto dele é o que explica (pasta sem permissão, npm, sem rede), em âmbar.
        let erro = resposta(false, &["Error: Failed to install native update", "Try running \"claude doctor\""]);
        let linha = linha_do_claude(Some((2, 1, 318)), 0, false, Some(&erro));
        assert!(linha.falhou && linha.resposta == erro.linhas && linha.espera.is_none());
    }

}
