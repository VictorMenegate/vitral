//! As partes Codex e Gemini das Configurações, no molde do cartão do Claude Code: a versão
//! instalada com o Atualizar (ou o Instalar), a conta com o Entrar (e o Sair do Codex), a lista
//! dos servidores MCP e o arquivo de configuração. A lógica que decide os textos é pura e tem
//! testes aqui; o que roda processo mora em `crate::cli_agentes` e no motor.

use gpui_kit::component::{Disableable as _, Size, Sizable as _, WindowExt as _, h_flex, spinner::Spinner, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Configuracoes;
use crate::{
    claude::{Atualizacao, Versao},
    cli_agentes::{self, ServidorFora},
    modelo::Agente,
    motor::Motor,
    tema::{Cores, FONTE_MONO, Lucide, Tipo as _, esp, r, raio, tam},
    ui::{
        componentes::{
            CabecalhoDialogo, Confirmacao, GrupoDeOpcoes, LinhaDeOpcao, RodapeDialogo, TamanhoBotao, TomConfirmacao, botao_fantasma,
            botao_primario, confirmar, largura,
        },
        dialogos, uso_plano,
    },
    versoes_agentes,
};

/// O que a linha da versão diz, e se o Atualizar dá para clicar.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct LinhaDoAgente {
    /// A primeira linha: a versão instalada, ou o que está acontecendo agora.
    pub texto: String,
    /// O spinner antes do texto.
    pub andando: bool,
    /// "Há a X. O Atualizar instala.", só quando ela cabe (não durante nem depois de um Atualizar).
    pub nova: Option<String>,
    /// O que o `update` respondeu, linha por linha.
    pub resposta: Vec<String>,
    /// A resposta foi de erro: ela vai em âmbar.
    pub falhou: bool,
    /// Por que o Atualizar não dá para clicar agora (`None`: dá).
    pub espera: Option<String>,
}

/// A linha da versão do Codex ou do Gemini. `programa` é o que o `update` chama ("codex", "agy");
/// `versao` é `None` até a leitura responder, e `Some(None)` quando ela não deu.
pub(super) fn linha_da_versao(
    programa: &str,
    instalado: bool,
    versao: Option<Option<Versao>>,
    publicada: Option<Versao>,
    a_caminho: usize,
    andando: bool,
    resposta: Option<&Atualizacao>,
) -> LinhaDoAgente {
    let texto = match (instalado, andando, versao) {
        (false, ..) => tr!("Não está instalado neste computador").to_string(),
        (true, true, _) => tr!("Rodando o {programa} update…", programa = programa).to_string(),
        (true, false, None) => tr!("Lendo a versão…").to_string(),
        // Sem a versão o Atualizar continua valendo: ele abre o mesmo programa dos turnos.
        (true, false, Some(None)) => tr!("Não deu para ler a versão instalada").to_string(),
        (true, false, Some(Some(v))) => versoes_agentes::texto(v),
    };
    let resposta_do_comando = resposta.filter(|_| instalado);
    let resposta_linhas = resposta_do_comando.map(|r| r.linhas.clone()).unwrap_or_default();
    // Depois de um Atualizar que respondeu, quem fala é a resposta dele.
    let nova = versao
        .flatten()
        .and_then(|v| versoes_agentes::frase_da_nova(Some(v), publicada))
        .filter(|_| instalado && !andando && resposta_linhas.is_empty());
    LinhaDoAgente {
        texto,
        andando: instalado && andando,
        nova,
        resposta: resposta_linhas,
        falhou: resposta_do_comando.is_some_and(|r| !r.ok),
        espera: if instalado { super::espera_da_atualizacao(andando, a_caminho) } else { None },
    }
}

/// O que a linha da conta do Gemini diz. O Vitral só sabe que há login quando o `agy` devolve a
/// cota (`agente_gemini::ler_uso`), então é isso que vale como "conectada".
pub(super) fn conta_do_gemini(lido: bool, conectado: bool) -> String {
    match (lido, conectado) {
        (false, _) => tr!("Lendo a conta…").to_string(),
        (true, true) => tr!("Conta Google conectada").to_string(),
        (true, false) => tr!("Sem login, ou o agy não respondeu").to_string(),
    }
}

/// Abre o terminal com o login do agente (o instalador, se faltar, é o "Instalar…"). O Vitral não
/// faz o login: ele acontece no terminal, e o motor olha depois se a conta entrou.
fn entrar(agente: Agente, motor: &Entity<Motor>, cx: &mut App) {
    let Some(casa) = dirs::home_dir() else { return };
    let binario = cli_agentes::binario(agente);
    let powershell = cfg!(windows);
    let comando = match agente {
        Agente::Gemini => {
            let aviso = if powershell { "Write-Host 'Entre com a sua conta Google:'; " } else { "echo 'Entre com a sua conta Google:'; " };
            format!("{aviso}{}", cli_agentes::comando_de_terminal(&binario, "acp --login", powershell))
        }
        _ => cli_agentes::comando_de_terminal(&binario, "login", powershell),
    };
    if powershell {
        crate::sistema::abrir_powershell(&casa, &comando);
    } else {
        crate::sistema::abrir_terminal(&casa, Some(&comando));
    }
    motor.update(cx, |m, cx| m.esperar_login(agente, cx));
}

/// Pergunta antes de sair da conta do Codex: o `codex logout` derruba os chats dele até entrar de
/// novo. O Enter só fecha.
fn confirmar_sair(motor: Entity<Motor>, window: &mut Window, cx: &mut App) {
    confirmar(
        Confirmacao {
            tom: TomConfirmacao::Perigo,
            icone: Lucide::Unplug.into(),
            titulo: tr!("Sair da conta do Codex?").into(),
            texto: tr!("O Vitral roda codex logout e apaga o login do Codex neste computador.").into(),
            detalhes: vec![(Lucide::MessagesSquare.into(), tr!("Os chats do Codex não respondem até você entrar de novo").into(), false)],
            ok: tr!("Sair").into(),
            cancelar: tr!("Cancelar").into(),
            ok_perigo: true,
        },
        move |_, cx| motor.update(cx, |m, cx| m.sair_do_codex(cx)),
        None,
        window,
        cx,
    );
}

/// O estado do diálogo da lista: `None` enquanto pergunta, `Some(None)` sem resposta.
#[derive(Default)]
struct Lista {
    resultado: Option<Option<Vec<ServidorFora>>>,
}

/// A lista dos servidores MCP do Codex ou do Gemini, só leitura (o `codex mcp list --json` e o
/// `agy mcp list`). Adicionar e remover fica no terminal.
fn abrir_servidores(agente: Agente, window: &mut Window, cx: &mut App) {
    let lista = cx.new(|_| Lista::default());
    cx.spawn({
        let lista = lista.downgrade();
        async move |cx| {
            let resultado = cx.background_spawn(async move { cli_agentes::ler_servidores(agente) }).await;
            let _ = cx.update(|cx| {
                let _ = lista.update(cx, |l, _| l.resultado = Some(resultado));
                cx.refresh_windows();
            });
        }
    })
    .detach();
    window.open_dialog(cx, move |dialogo, window, cx| {
        let resultado = lista.read(cx).resultado.clone();
        let aviso = |texto: &str| div().py(esp::E24).corpo_ui().text_color(Cores::tenue()).child(texto.to_string()).into_any_element();
        let (subtitulo, corpo) = match &resultado {
            None => (tr!("Perguntando…").to_string(), aviso(&tr!("Isto leva um instante."))),
            Some(None) => (tr!("Sem resposta").to_string(), aviso(&tr!("O {agente} não respondeu. Feche e abra de novo para tentar outra vez.", agente = agente.nome()))),
            Some(Some(servidores)) if servidores.is_empty() => {
                (tr!("Nenhum servidor").to_string(), aviso(&tr!("Não há servidor MCP configurado no {agente}.", agente = agente.nome())))
            }
            Some(Some(servidores)) => {
                let ligados = servidores.iter().filter(|s| s.ligado).count();
                let cartoes = servidores.iter().map(|s| {
                    h_flex()
                        .id(SharedString::from(format!("mcp-fora-{}", s.nome)))
                        .items_center()
                        .gap(esp::E12)
                        .min_h(r(52.))
                        .px(esp::E12)
                        .mb(esp::E6)
                        .rounded(raio::CARTAO)
                        .border_1()
                        .border_color(Cores::borda())
                        .bg(Cores::cartao_elevado())
                        .child(div().flex_none().size(r(8.)).rounded_full().bg(if s.ligado { Cores::verde() } else { Cores::apagado() }))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().truncate().corpo_ui().font_weight(FontWeight::MEDIUM).text_color(Cores::forte()).child(s.nome.clone()))
                                .when(!s.detalhe.is_empty(), |d| {
                                    d.child(div().truncate().font_family(FONTE_MONO).metadado().text_color(Cores::tenue()).child(s.detalhe.clone()))
                                }),
                        )
                        .child(div().flex_none().metadado().text_color(Cores::tenue()).child(if s.ligado { tr!("ligado") } else { tr!("desligado") }))
                });
                let corpo = div()
                    .id("mcp-fora-lista")
                    .max_h(window.viewport_size().height * 0.6)
                    .overflow_y_scroll()
                    .child(v_flex().w_full().children(cartoes))
                    .into_any_element();
                (tr!("{ligados} de {total} ligados", ligados = ligados, total = servidores.len()).to_string(), corpo)
            }
        };
        dialogos::moldura(dialogo, largura::M, window)
            .title(dialogos::topo(
                CabecalhoDialogo::new(tr!("Servidores MCP do {agente}", agente = agente.nome())).subtitulo(div().font_weight(FontWeight::NORMAL).child(subtitulo)),
            ))
            .child(corpo)
            .footer(RodapeDialogo::new().principal(
                botao_primario("mcp-fora-fechar", None, tr!("Fechar"), TamanhoBotao::Controle, window, cx)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            ))
    });
}

impl Configuracoes {
    /// A parte do Codex ou do Gemini: no molde do cartão do Claude Code. Sem o programa só aparece
    /// a versão com o Instalar; com ele, a conta, a lista dos servidores MCP e o arquivo de
    /// configuração (quando existe).
    pub(super) fn grupo_do_agente(&self, agente: Agente, estreito: bool, window: &Window, cx: &Context<Self>) -> GrupoDeOpcoes {
        let motor = self.motor.read(cx);
        let cli = motor.cli(agente);
        let instalado = agente.instalado();
        let (id, programa, publicada) = match agente {
            Agente::Gemini => ("cfg-gemini", "agy", None),
            _ => ("cfg-codex", "codex", self.publicadas.codex),
        };
        let linha = linha_da_versao(programa, instalado, cli.versao, publicada, motor.turnos_a_caminho_de(agente), cli.atualizando, cli.resposta.as_ref());
        let falhou = linha.falhou;
        let descricao = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(r(6.))
                    .when(linha.andando, |d| d.child(Spinner::new().with_size(Size::Size(tam(window, 12.))).color(Cores::tenue())))
                    .child(linha.texto.clone()),
            )
            .children(linha.nova.clone().map(|frase| div().text_color(Cores::texto()).child(frase)))
            .children(linha.resposta.iter().map(|t| div().when(falhou, |d| d.text_color(Cores::ambar())).child(t.clone())));
        let com = |opcao: LinhaDeOpcao, controle: AnyElement| if estreito { opcao.campo(controle) } else { opcao.controle(controle) };
        let motor_e = self.motor.clone();
        let botao = if instalado {
            botao_fantasma(format!("{id}-atualizar"), Some(Lucide::RefreshCw.into()), tr!("Atualizar"), TamanhoBotao::Barra, window, cx)
                // Na caixa escura dos outros controles: solto, parecia texto.
                .bg(Cores::recuo())
                .when_some(linha.espera.clone(), |b, motivo| b.disabled(true).opacity(0.45).tooltip(motivo))
                .on_click(move |_, _, cx| motor_e.update(cx, |m, cx| m.atualizar_cli(agente, cx)))
        } else {
            // O instalador oficial e o login, no terminal (`uso_plano::instalar_*`).
            botao_fantasma(format!("{id}-instalar"), Some(Lucide::Download.into()), tr!("Instalar…"), TamanhoBotao::Barra, window, cx)
                .bg(Cores::recuo())
                .on_click(move |_, _, _| match agente {
                    Agente::Gemini => uso_plano::instalar_gemini(),
                    _ => uso_plano::instalar_codex(),
                })
        };
        let info = match agente {
            Agente::Gemini => {
                tr!("O Gemini roda pelo Antigravity CLI (agy), do Google. O Vitral abre o agy que encontra no PATH ou em \
                 ~/.local/bin. O Atualizar roda `agy update` fora de qualquer turno e mostra o que ele responder; a \
                 versão nova vale no turno seguinte. Sem consulta de versão nova: o agy não publica uma que o Vitral \
                 leia sem instalar.")
            }
            _ => {
                tr!("O agente de código da OpenAI. O Vitral abre o codex que encontra no PATH ou em ~/.local/bin. O Atualizar \
                 roda `codex update` fora de qualquer turno e mostra o que ele responder; a versão nova vale no turno \
                 seguinte, sem reiniciar o Vitral.")
            }
        };
        let opcao = LinhaDeOpcao::new(id, tr!("Versão instalada")).info(info).descricao(descricao);
        // O motivo de o botão esperar também fica à vista, em âmbar, sem precisar da dica.
        let opcao = match linha.espera.filter(|_| !linha.andando) {
            Some(motivo) => opcao.aviso(motivo),
            None => opcao,
        };
        let mut grupo = GrupoDeOpcoes::new(Some(agente.nome().into())).linha(com(opcao, botao.into_any_element()));
        if !instalado {
            return grupo;
        }

        // A conta.
        let (frase, entrou) = match agente {
            Agente::Gemini => (conta_do_gemini(motor.gemini_lido, motor.gemini.is_some()).to_string(), motor.gemini.is_some()),
            _ => match &cli.conta {
                None => (tr!("Lendo a conta…").to_string(), false),
                Some(conta) => (conta.frase(), conta.entrou()),
            },
        };
        let motor_c = self.motor.clone();
        let (rotulo, icone) = match (agente, entrou) {
            (Agente::Codex, true) => (tr!("Sair"), Lucide::Unplug),
            (Agente::Gemini, true) => (tr!("Entrar de novo…"), Lucide::KeyRound),
            _ => (tr!("Entrar…"), Lucide::KeyRound),
        };
        let sair = agente == Agente::Codex && entrou;
        let botao = botao_fantasma(format!("{id}-conta"), Some(icone.into()), rotulo, TamanhoBotao::Controle, window, cx)
            .bg(Cores::recuo())
            .on_click(move |_, window, cx| {
                if sair {
                    confirmar_sair(motor_c.clone(), window, cx);
                } else {
                    entrar(agente, &motor_c, cx);
                }
            });
        let info = match agente {
            Agente::Gemini => tr!("O login é o do agy, com a conta Google, e vale para os chats do Gemini. “Entrar” abre o terminal com `agy acp --login`."),
            _ => tr!("O login é o do próprio Codex, com a conta do ChatGPT ou uma chave de API. “Entrar” abre o terminal com `codex login`; “Sair” roda `codex logout`."),
        };
        grupo = grupo.linha(com(LinhaDeOpcao::new(format!("{id}-conta"), tr!("Conta")).info(info).descricao(frase), botao.into_any_element()));

        // Os servidores MCP.
        let botao = botao_fantasma(format!("{id}-mcp-ver"), Some(Lucide::Container.into()), tr!("Ver…"), TamanhoBotao::Controle, window, cx)
            .bg(Cores::recuo())
            .on_click(move |_, window, cx| abrir_servidores(agente, window, cx));
        grupo = grupo.linha(com(
            LinhaDeOpcao::new(format!("{id}-mcp"), tr!("Lista dos servidores MCP"))
                .descricao(tr!("Os servidores do {agente}, e se estão ligados. Só leitura.", agente = agente.nome())),
            botao.into_any_element(),
        ));

        // O arquivo de configuração, quando existe.
        if let Some(arquivo) = cli_agentes::arquivo_de_configuracao(agente) {
            let descricao = dialogos::com_til(&arquivo);
            let botao = botao_fantasma(format!("{id}-config-abrir"), Some(Lucide::FilePen.into()), tr!("Abrir"), TamanhoBotao::Controle, window, cx)
                .bg(Cores::recuo())
                .on_click(move |_, _, cx| crate::sistema::abrir_fora(&arquivo, cx));
            grupo = grupo.linha(com(
                LinhaDeOpcao::new(format!("{id}-config"), tr!("Configuração do {agente}", agente = agente.nome())).descricao(descricao),
                botao.into_any_element(),
            ));
        }
        grupo
    }
}

#[cfg(test)]
mod testes {
    use super::{conta_do_gemini, linha_da_versao};
    use crate::claude::Atualizacao;

    fn resposta(ok: bool, linhas: &[&str]) -> Atualizacao {
        Atualizacao { ok, linhas: linhas.iter().map(|l| l.to_string()).collect() }
    }

    #[test]
    fn sem_o_programa_nao_ha_o_que_atualizar() {
        let l = linha_da_versao("codex", false, Some(None), Some((0, 171, 0)), 0, false, None);
        assert_eq!(l.texto, "Não está instalado neste computador");
        assert!(l.nova.is_none() && l.espera.is_none() && l.resposta.is_empty() && !l.andando);
    }

    #[test]
    fn a_linha_diz_a_versao_e_a_nova_so_quando_ha() {
        assert_eq!(linha_da_versao("codex", true, None, None, 0, false, None).texto, "Lendo a versão…");
        assert_eq!(linha_da_versao("codex", true, Some(None), None, 0, false, None).texto, "Não deu para ler a versão instalada");
        let em_dia = linha_da_versao("codex", true, Some(Some((0, 171, 0))), Some((0, 171, 0)), 0, false, None);
        assert_eq!(em_dia.texto, "0.171.0");
        assert!(em_dia.nova.is_none() && em_dia.espera.is_none() && !em_dia.falhou);
        let velha = linha_da_versao("codex", true, Some(Some((0, 169, 0))), Some((0, 171, 0)), 0, false, None);
        assert_eq!(velha.texto, "0.169.0");
        assert_eq!(velha.nova.as_deref(), Some("Há a 0.171.0. O Atualizar instala."));
        // Sem versão publicada conhecida (o Gemini): nada de "há a X".
        let gemini = linha_da_versao("agy", true, Some(Some((1, 5, 16))), None, 0, false, None);
        assert_eq!((gemini.texto.as_str(), gemini.nova), ("1.5.16", None));
    }

    #[test]
    fn atualizando_a_linha_diz_o_comando_e_o_botao_espera() {
        let l = linha_da_versao("agy", true, Some(Some((1, 5, 16))), None, 0, true, None);
        assert_eq!(l.texto, "Rodando o agy update…");
        assert!(l.andando && l.espera.as_deref() == Some("A atualização está rodando"));
        // Rodando, a frase da versão nova se cala.
        let l = linha_da_versao("codex", true, Some(Some((0, 169, 0))), Some((0, 171, 0)), 0, true, None);
        assert!(l.nova.is_none());
    }

    #[test]
    fn o_atualizar_espera_os_chats_do_agente() {
        let um = linha_da_versao("codex", true, Some(Some((0, 169, 0))), None, 1, false, None).espera.unwrap();
        assert_eq!(um, "1 chat está trabalhando ou com mensagem na fila: o Atualizar espera ele terminar");
        let dois = linha_da_versao("codex", true, Some(Some((0, 169, 0))), None, 2, false, None).espera.unwrap();
        assert_eq!(dois, "2 chats estão trabalhando ou com mensagem na fila: o Atualizar espera eles terminarem");
    }

    #[test]
    fn a_resposta_do_update_vai_inteira_e_a_falha_vai_em_ambar() {
        let em_dia = resposta(true, &["Already on the latest version."]);
        let l = linha_da_versao("agy", true, Some(Some((1, 5, 16))), None, 0, false, Some(&em_dia));
        assert_eq!(l.resposta, em_dia.linhas);
        assert!(!l.falhou && l.espera.is_none());
        let erro = resposta(false, &["Error: could not reach the update server"]);
        let l = linha_da_versao("codex", true, Some(Some((0, 169, 0))), Some((0, 171, 0)), 0, false, Some(&erro));
        assert!(l.falhou && l.resposta == erro.linhas && l.espera.is_none());
        // Depois de uma resposta, quem fala é ela, não a frase da versão nova.
        assert!(l.nova.is_none());
    }

    #[test]
    fn a_conta_do_gemini_so_vale_com_a_cota_lida() {
        assert_eq!(conta_do_gemini(false, false), "Lendo a conta…");
        assert_eq!(conta_do_gemini(true, true), "Conta Google conectada");
        assert_eq!(conta_do_gemini(true, false), "Sem login, ou o agy não respondeu");
    }
}
