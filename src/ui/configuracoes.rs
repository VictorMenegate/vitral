//! Configurações da pasta de trabalho (valem para as conversas que não escolheram outra coisa),
//! em grupos de linhas como as preferências do GNOME: o texto à esquerda e o controle à direita
//! (embaixo do texto no diálogo estreito). Grava na hora. As Animações, o zoom e as Atualizações
//! valem para o app todo (ficam em `interface.json` e em `atualizacoes.json`). A organização é a
//! das partes (`Parte`, a coluna da esquerda); as instruções, os perfis, a letra da conversa, o
//! teste da notificação, a versão do Codex e o diagnóstico entraram dentro delas.

use std::rc::Rc;

use gpui_kit::component::{
    Disableable as _, Icon, Size, Sizable as _,
    menu::{DropdownMenu, PopupMenuItem},
    spinner::Spinner,
    tooltip::{ManagedTooltipExt as _, Tooltip},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    composer::{DESCRICOES_DE_MODELO, MODELOS, MODOS},
    componentes::{
        Chip, Contador, ControleSegmentado, GatilhoChip, GrupoDeOpcoes, LinhaDeOpcao, Marca, Simbolo, TamanhoBotao,
        TamanhoChip, botao_fantasma, icone_do_modo, item_descrito,
    },
    atualizacao::{Vista, grupo as grupo_de_atualizacoes},
    dialogos::estreito,
    movimento::{self, Animacoes},
    zoom,
};
use crate::{
    atualizacao::{self, Acompanhando, Atualizacoes},
    claude::{self, Atualizacao, Versao},
    modelo::Config,
    motor::{self, Motor},
    tema::{Cores, Lucide, Tipo as _, esp, r, raio, tam},
    versoes_agentes::{self, Publicadas},
};

mod instrucoes;
pub(in crate::ui) mod perfis;

/// "100%": o rótulo de um tamanho em relação ao resto.
const SUFIXO_DE_PORCENTO: &str = "%";

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

/// As partes das Configurações, na coluna da esquerda (dono, 23/11/2026: "faz ser uma coluna na
/// esquerda com as configurações separadas em cada parte").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Parte {
    Chats,
    Agentes,
    Perfis,
    Memoria,
    Aparencia,
    ClaudeCode,
    Atualizacoes,
}

impl Parte {
    const TODAS: [Parte; 7] =
        [Parte::Chats, Parte::Agentes, Parte::Perfis, Parte::Memoria, Parte::Aparencia, Parte::ClaudeCode, Parte::Atualizacoes];

    fn nome(self) -> &'static str {
        match self {
            Parte::Chats => "Chats",
            Parte::Agentes => "Agentes",
            Parte::Perfis => "Perfis",
            Parte::Memoria => "Memória",
            Parte::Aparencia => "Aparência",
            Parte::ClaudeCode => "Claude Code",
            Parte::Atualizacoes => "Atualizações",
        }
    }

    fn icone(self) -> Lucide {
        match self {
            Parte::Chats => Lucide::MessagesSquare,
            Parte::Agentes => Lucide::Bot,
            Parte::Perfis => Lucide::UsersRound,
            Parte::Memoria => Lucide::Brain,
            Parte::Aparencia => Lucide::Palette,
            Parte::ClaudeCode => Lucide::Terminal,
            Parte::Atualizacoes => Lucide::RefreshCw,
        }
    }
}

/// A altura do corpo das Configurações, a 100%: a coluna das partes fica parada e só o lado
/// direito rola, sem o diálogo mudar de tamanho a cada parte.
const ALTURA_DAS_CONFIGURACOES: f32 = 520.;
/// A largura do diálogo, a 100%: mais largo que o G, com a coluna das partes e as opções lado a lado.
pub(super) const LARGURA_DAS_CONFIGURACOES: f32 = 860.;

pub struct Configuracoes {
    motor: Entity<Motor>,
    parte: Parte,
    /// A versão do Codex instalado (`None` até a leitura em segundo plano responder).
    codex: Option<Option<Versao>>,
    /// As versões mais novas publicadas, quando a consulta responde.
    publicadas: Publicadas,
    /// O que o Testar da notificação deu, depois do clique.
    teste_da_notificacao: Option<Result<(), String>>,
    /// O diagnóstico acabou de ser copiado.
    diagnostico_copiado: bool,
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
        Self {
            motor,
            parte: Parte::Chats,
            codex: None,
            publicadas: versoes_agentes::guardadas().unwrap_or_default(),
            teste_da_notificacao: None,
            diagnostico_copiado: false,
            _observa,
            _atualizacoes,
            _acompanhando,
        }
    }

    fn ir_para(&mut self, parte: Parte, cx: &mut Context<Self>) {
        self.parte = parte;
        // As linhas do Claude Code e do Codex dizem a versão instalada e a nova, quando há.
        if matches!(parte, Parte::Agentes | Parte::ClaudeCode) {
            self.ler_os_instalados(false, cx);
        }
        cx.notify();
    }

    /// A versão do Codex e as mais novas publicadas, em segundo plano, quando a parte Agentes ou a
    /// do Claude Code abre: o `codex --version` leva um instante e a consulta vai à rede (guardada
    /// por uma hora).
    /// Com "Só pelo botão", abrir a parte lê só o que está instalado e a última consulta guardada;
    /// a rede só é consultada com `pelo_botao` (`opcoes::consulta_a_rede`).
    fn ler_os_instalados(&mut self, pelo_botao: bool, cx: &mut Context<Self>) {
        let consultar = crate::opcoes::consulta_a_rede(self.motor.read(cx).config.consultar_versoes, pelo_botao);
        cx.spawn(async move |vista, cx| {
            let (codex, publicadas) = cx
                .background_spawn(async move {
                    let publicadas =
                        if consultar { versoes_agentes::consultar() } else { versoes_agentes::guardadas().unwrap_or_default() };
                    (versoes_agentes::versao_do_codex(), publicadas)
                })
                .await;
            let _ = vista.update(cx, |vista, cx| {
                (vista.codex, vista.publicadas) = (Some(codex), publicadas);
                cx.notify();
            });
        })
        .detach();
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
        // Depois de um Atualizar que respondeu, quem fala é a resposta dele.
        let nova = versoes_agentes::frase_da_nova_do_claude(motor.versao_claude, self.publicadas.claude)
            .filter(|_| !linha.andando && linha.resposta.is_empty());
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
            .children(nova.map(|frase| div().text_color(Cores::texto()).child(frase)))
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
        // Na caixa escura dos outros controles: solto, parecia texto (revisão de design, 23/11/2026).
        .bg(Cores::recuo())
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

    /// A linha do Codex: a versão instalada e a nova quando há; sem ele, o Instalar.
    fn linha_do_codex(&self, estreito: bool, window: &Window, cx: &Context<Self>) -> LinhaDeOpcao {
        let instalado = crate::agente_codex::instalado();
        let texto = match self.codex {
            None if instalado => "Lendo a versão…".to_string(),
            lida => versoes_agentes::linha_do_codex(instalado, lida.flatten(), self.publicadas.codex),
        };
        let opcao = LinhaDeOpcao::new("cfg-codex", "Versão instalada")
            .marca(Simbolo::Marca(Marca::Codex))
            .info(
                "O agente de código da OpenAI. Ele usa o login e o plano do próprio Codex. A versão nova é a que o \
                 pacote dele anuncia; quem atualiza é o instalador do Codex, fora do Vitral.",
            )
            .descricao(texto);
        // O instalador oficial do Codex é um script do sh: no Windows não há o botão.
        if instalado || cfg!(windows) {
            return opcao;
        }
        let botao = botao_fantasma("cfg-codex-instalar", Some(Lucide::Download.into()), "Instalar…", TamanhoBotao::Barra, window, cx)
            .bg(Cores::recuo())
            .on_click(|_, _, _| super::uso_plano::instalar_codex());
        if estreito { opcao.campo(botao) } else { opcao.controle(botao) }
    }

    /// O texto do diagnóstico de agora, já coberto (`crate::diagnostico`).
    fn diagnostico(&self, cx: &App) -> String {
        let motor = self.motor.read(cx);
        let c = &motor.config;
        let ligado = |sim: bool| if sim { "ligado" } else { "desligado" }.to_string();
        let (animacoes, _) = movimento::animacoes(cx);
        let opcoes = vec![
            ("Quem responde".to_string(), c.agente_padrao.nome().to_string()),
            ("Modelo do Claude".to_string(), if c.modelo_chats.is_empty() { "padrão".to_string() } else { c.modelo_chats.clone() }),
            ("Modo de permissão".to_string(), c.permissao.clone()),
            ("Servidores MCP do Claude Code".to_string(), ligado(c.mcp_trabalhadores)),
            ("Memória da equipe".to_string(), if c.memoria_da_equipe.is_empty() { "desligada" } else { "ligada" }.to_string()),
            ("Rever a memória ao terminar".to_string(), ligado(c.memoria_ao_terminar)),
            ("Sugerir a próxima mensagem".to_string(), ligado(c.sugerir_resposta)),
            ("Chat chamado abre numa aba".to_string(), ligado(c.chat_chamado_abre_aba)),
            ("Fechar a aba para o chat".to_string(), ligado(c.fechar_aba_para_o_chat)),
            ("Seta ↑ na caixa vazia".to_string(), if c.seta_acima_percorre { "percorre as enviadas" } else { "corrige a última" }.to_string()),
            ("Enter com o turno rodando".to_string(), if c.enter_na_fila { "vai para a fila" } else { "entra no turno" }.to_string()),
            ("Números nas abas".to_string(), if c.numeros_so_com_ctrl { "só com Ctrl" } else { "sempre" }.to_string()),
            ("Lembrar a aba Arquivos".to_string(), ligado(c.lembrar_arquivos)),
            ("Título pelo Claude Code".to_string(), ligado(c.titulo_pelo_claude)),
            ("Anéis de uso".to_string(), format!("Claude {}, Codex {}, Gemini {}", ligado(c.anel_claude), ligado(c.anel_codex), ligado(c.anel_gemini))),
            ("Versão nova dos agentes".to_string(), if c.consultar_versoes { "consulta sozinho" } else { "só pelo botão" }.to_string()),
            ("Letra da conversa na caixa".to_string(), ligado(c.letra_na_caixa)),
            ("Suas instruções".to_string(), format!("{} caracteres", c.instrucoes.trim().chars().count())),
            ("Perfis".to_string(), c.perfis.len().to_string()),
            ("Zoom".to_string(), format!("{}%", zoom::porcentagem(cx))),
            ("Letra da conversa".to_string(), format!("{}%", c.letra_da_conversa)),
            ("Transparência".to_string(), crate::tema::NOMES_DA_TRANSPARENCIA[usize::from(c.transparencia.min(3))].to_string()),
            ("Animações".to_string(), if animacoes == Animacoes::Desligadas { "desligadas" } else { "seguem o sistema" }.to_string()),
            ("Arquivar parados".to_string(), if c.arquivar_apos_dias == 0 { "nunca".to_string() } else { format!("{} dias", c.arquivar_apos_dias) }),
            ("Pasta de trabalho".to_string(), c.pasta_de_trabalho.as_ref().map_or_else(|| "nenhuma".to_string(), |p| p.display().to_string())),
        ];
        let pasta = motor.dados.pasta.clone();
        crate::diagnostico::texto(&crate::diagnostico::Entrada {
            vitral: format!("{} · {} · {}", atualizacao::VERSAO, atualizacao::origem(), atualizacao::plataforma()),
            sistema: crate::diagnostico::sistema(),
            claude: motor.versao_claude.map(motor::texto_da_versao),
            codex: self.codex.flatten().map(motor::texto_da_versao).or_else(|| {
                crate::agente_codex::instalado().then(|| "instalado".to_string())
            }),
            quedas: std::fs::read_to_string(pasta.join("queda.log")).ok(),
            dados: pasta,
            config: motor.dados.config.clone(),
            cache: motor.dados.cache.clone(),
            chats: motor.chats.len(),
            opcoes,
            casa: dirs::home_dir(),
        })
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
        // Na mesma caixa escura do contador e da escolha segmentada da coluna: solto, o seletor
        // parecia o valor, e não um controle (revisão de design, 23/11/2026).
        .bg(Cores::recuo())
        .text_color(Cores::texto())
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

/// O contador no tamanho dele: embaixo do texto (diálogo estreito), a linha o esticaria de ponta a
/// ponta.
fn solto(contador: Contador) -> AnyElement {
    div().flex().child(contador).into_any_element()
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
        let estreito = estreito(window, LARGURA_DAS_CONFIGURACOES);
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
        let (sugerir, alternar_sugerir) = (config.sugerir_resposta, m());
        let (uso_no_canto, alternar_uso) = (config.uso_no_canto, m());
        let (anel_claude, anel_codex, anel_gemini) = (m(), m(), m());
        let (avisos_no_app, alternar_avisos) = (config.avisos_no_app, m());
        let (titulo_claude, alternar_titulo) = (config.titulo_pelo_claude, m());
        let conversas = GrupoDeOpcoes::new(Some("Chats".into()))
            .linha(com(
                LinhaDeOpcao::new("cfg-resumo", "Resumo ao voltar depois de")
                    .info("O cartão “Enquanto você esteve fora” junta o que terminou, o que precisa de você e o que falhou desde que a janela saiu de foco. Ele some no × e volta na próxima saída.")
                    .descricao("Minutos com a janela fora de foco antes do “Enquanto você esteve fora”."),
                contador("cfg-resumo", &[5, 10, 15, 30, 60, 120], config.minutos_para_resumo as u32, m(), |c, v| {
                    c.minutos_para_resumo = v as u64
                })
                .into_any_element(),
            ))
            .linha(
                // Rodada de uso, item 3.1 (dono, 16/11/2026).
                LinhaDeOpcao::new("cfg-avisos-no-app", "Avisar dentro do app")
                    .info("Só para chat fora de vista e só para o que trava trabalho: pedido de permissão, plano para aprovar e falha. O aviso fica até ser respondido e nunca pega o foco da caixa.")
                    .descricao("No canto de baixo, à direita, quando um chat que não está à vista precisa de você.")
                    .interruptor(avisos_no_app, move |ligado, _, cx| {
                        alternar_avisos.update(cx, |m, cx| m.mudar_config(|c| c.avisos_no_app = ligado, cx))
                    }),
            )
            .linha({
                let botao = botao_fantasma("cfg-notificacao-testar", Some(Lucide::Bell.into()), "Testar", TamanhoBotao::Controle, window, cx)
                    .bg(Cores::recuo())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.teste_da_notificacao = Some(this.motor.read(cx).testar_notificacao());
                        cx.notify();
                    }));
                let linha = LinhaDeOpcao::new("cfg-notificacao", "Notificação do sistema")
                    .info(
                        "Clicar na notificação abre a conversa. A permissão se responde dentro do app, nunca na \
                         notificação. O pedido de permissão e a queda com erro ficam na tela até o clique.",
                    )
                    .descricao(match &self.teste_da_notificacao {
                        Some(Ok(())) => "Enviada. Não apareceu? Veja o Não perturbe do sistema.",
                        _ => "Com a janela atrás, quando um chat termina, pede permissão ou cai.",
                    });
                let linha = match &self.teste_da_notificacao {
                    Some(Err(erro)) => linha.aviso(erro.clone()),
                    _ => linha,
                };
                com(linha, botao.into_any_element())
            })
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
                LinhaDeOpcao::new("cfg-sugerir", "Sugerir a próxima mensagem")
                    .info(
                        "Depois de cada resposta do Claude, o Claude Code adivinha o que você escreveria em seguida \
                         (“roda os testes”, “commita”). A sugestão aparece apagada na caixa vazia, e o Tab a põe na \
                         caixa. Quando o próximo passo não é óbvio, ele não sugere nada. É uma chamada curta, que \
                         reaproveita o cache da conversa.",
                    )
                    .descricao("Só no Claude. O Tab aceita.")
                    .interruptor(sugerir, move |ligado, _, cx| {
                        alternar_sugerir.update(cx, |m, cx| m.mudar_config(|c| c.sugerir_resposta = ligado, cx))
                    }),
            )
            .linha(
                LinhaDeOpcao::new("cfg-titulo-claude", "Título do chat pelo Claude Code")
                    .info("O Claude Code dá um título à conversa depois da primeira mensagem, e o Vitral o põe no lugar das primeiras palavras. Nunca troca um nome que você deu. Desligado, o título fica o das primeiras palavras. É uma chamada curta ao modelo, uma vez por chat.")
                    .descricao("Só no Claude. Desligado, vale o começo da primeira mensagem.")
                    .interruptor(titulo_claude, move |ligado, _, cx| {
                        alternar_titulo.update(cx, |m, cx| m.mudar_config(|c| c.titulo_pelo_claude = ligado, cx))
                    }),
            );
        // As abas: o chat que outro chat põe para trabalhar e o fechar (dono, 24/11/2026).
        let (abre_aba, alternar_abre_aba) = (config.chat_chamado_abre_aba, m());
        let (fecha_para, alternar_fecha_para) = (config.fechar_aba_para_o_chat, m());
        let abas_grupo = GrupoDeOpcoes::new(Some("Abas".into()))
            .linha(
                LinhaDeOpcao::new("cfg-chat-chamado", "Chat chamado por outro abre numa aba")
                    .info("Quando um chat manda mensagem a outro, ou abre um, o destino aparece numa aba do painel ativo. A aba entra de fundo: não troca o que você está lendo nem traz a janela para a frente, e o contorno dela mostra que está rodando. Se o chat já está à vista ou já é aba, nada muda.")
                    .descricao("Uma aba de fundo, sem trocar o que está à vista.")
                    .interruptor(abre_aba, move |ligado, _, cx| {
                        alternar_abre_aba.update(cx, |m, cx| m.mudar_config(|c| c.chat_chamado_abre_aba = ligado, cx))
                    }),
            )
            .linha(
                LinhaDeOpcao::new("cfg-fechar-aba", "Fechar a aba para o chat")
                    .info("Fechar a aba de um chat que está trabalhando para o turno dele, como o botão Parar. Se ele continua à vista em outro painel ou aba, só a aba fecha. Se parar derrubaria agentes em segundo plano, o Vitral pergunta antes. Chat parado ou esperando você só fecha a aba.")
                    .descricao("O turno para junto, a menos que o chat siga à vista noutro lugar.")
                    .interruptor(fecha_para, move |ligado, _, cx| {
                        alternar_fecha_para.update(cx, |m, cx| m.mudar_config(|c| c.fechar_aba_para_o_chat = ligado, cx))
                    }),
            );
        // A caixa de mensagem: a seta ↑ e o Enter.
        let seta_percorre = config.seta_acima_percorre;
        let enter_fila = config.enter_na_fila;
        let (mudar_seta, mudar_enter) = (m(), m());
        let caixa_grupo = GrupoDeOpcoes::new(Some("Caixa de mensagem".into()))
            .linha(com(
                LinhaDeOpcao::new("cfg-seta-acima", "Seta ↑ na caixa vazia")
                    .info("“Percorrer as enviadas” traz o texto das mensagens que você já enviou neste chat, uma por vez, sem desfazer nada; ↓ volta, e editar o texto devolve as setas ao cursor. O corrigir continua no lápis da mensagem e no Ctrl+↑.")
                    .descricao("O que a seta faz com nada escrito."),
                ControleSegmentado::new(
                    "cfg-seta-acima",
                    ["Corrigir a última", "Percorrer as enviadas"],
                    usize::from(seta_percorre),
                    move |novo, _, cx| mudar_seta.update(cx, |m, cx| m.mudar_config(|c| c.seta_acima_percorre = novo == 1, cx)),
                )
                .with_size(Size::Small)
                .largura_total(estreito)
                .into_any_element(),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-enter-turno", "Enter com o turno rodando")
                    .info("“Entra no turno” manda a mensagem ao agente sem pará-lo. “Vai para a fila” espera o turno acabar. O Ctrl+Enter faz o outro. Respondendo a um pedido de permissão ou a uma pergunta, o Enter continua sendo a resposta.")
                    .descricao("O Ctrl+Enter faz o contrário."),
                ControleSegmentado::new(
                    "cfg-enter-turno",
                    ["Entra no turno", "Vai para a fila"],
                    usize::from(enter_fila),
                    move |novo, _, cx| mudar_enter.update(cx, |m, cx| m.mudar_config(|c| c.enter_na_fila = novo == 1, cx)),
                )
                .with_size(Size::Small)
                .largura_total(estreito)
                .into_any_element(),
            ));
        let (lembrar, alternar_lembrar) = (config.lembrar_arquivos, m());
        let arquivos_grupo = GrupoDeOpcoes::new(Some("Arquivos".into())).linha(
            LinhaDeOpcao::new("cfg-lembrar-arquivos", "Lembrar o que a aba Arquivos mostra")
                .info("Guarda “Mostrar ignorados” (a aba Arquivos) e “Ignorar espaço em branco” (o “…” do arquivo com o diff) entre uma abertura e outra. Desligado, os dois abrem desligados, como sempre.")
                .descricao("Os ignorados pelo git e o espaço em branco do diff.")
                .interruptor(lembrar, move |ligado, _, cx| {
                    alternar_lembrar.update(cx, |m, cx| m.mudar_config(|c| c.lembrar_arquivos = ligado, cx))
                }),
        );
        let agentes = GrupoDeOpcoes::new(Some("Agentes".into()))
            .linha(com(
                LinhaDeOpcao::new("cfg-modo", "Modo de permissão padrão")
                    .info("“Sem perguntar” é o mesmo do seu VS Code. Nos outros, os pedidos de permissão aparecem na conversa.")
                    .descricao("Cada chat pode trocar o modo na caixa de mensagem."),
                menu_escolha("cfg-permissao", ancora, modos, &config.permissao, &padrao.permissao, gravar(m(), |c, v| c.permissao = v), window, cx),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-agente", "Quem responde nos chats novos")
                    .info("O Codex é o agente de código da OpenAI, e o Gemini, o do Google (pelo Antigravity CLI). Cada um usa o login e o plano dele, e precisa estar instalado neste computador.")
                    .descricao("Cada chat pode trocar o agente no seletor de modelo."),
                menu_escolha(
                    "cfg-agente",
                    ancora,
                    vec![
                        Opcao::new("claude", "Claude").descricao("Claude Code").simbolo(Simbolo::Marca(Marca::Claude)),
                        Opcao::new("codex", "Codex")
                            .descricao(if crate::agente_codex::instalado() { "Codex, da OpenAI" } else { "Não achei o Codex neste computador" })
                            .simbolo(Simbolo::Marca(Marca::Codex)),
                        Opcao::new("gemini", "Gemini")
                            .descricao(if crate::agente_gemini::instalado() { "Gemini, do Google, pelo Antigravity CLI" } else { "Não achei o Antigravity CLI (agy) neste computador" })
                            .simbolo(Simbolo::Marca(Marca::Gemini)),
                    ],
                    match config.agente_padrao {
                        crate::modelo::Agente::Claude => "claude",
                        crate::modelo::Agente::Codex => "codex",
                        crate::modelo::Agente::Gemini => "gemini",
                    },
                    "claude",
                    gravar(m(), |c, v| {
                        c.agente_padrao = match v.as_str() {
                            "codex" => crate::modelo::Agente::Codex,
                            "gemini" => crate::modelo::Agente::Gemini,
                            _ => crate::modelo::Agente::Claude,
                        }
                    }),
                    window,
                    cx,
                ),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-modelo-trab", "Modelo dos chats do Claude")
                    .info("Padrão é o modelo que o seu Claude Code escolher. As versões exatas e as contas ficam no seletor de modelo da caixa.").descricao("Cada chat pode trocar o modelo na caixa de mensagem."),
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
            ));
        let instrucoes_grupo = GrupoDeOpcoes::new(Some("Suas instruções".into())).linha({
            let abrir = m();
            let vazio = config.instrucoes.trim().is_empty();
            let botao = botao_fantasma("cfg-instrucoes-editar", None, if vazio { "Escrever…" } else { "Editar…" }, TamanhoBotao::Controle, window, cx)
                .bg(Cores::recuo())
                .on_click(move |_, window, cx| instrucoes::abrir(abrir.clone(), window, cx));
            com(
                LinhaDeOpcao::new("cfg-instrucoes", "Instruções para todos os chats")
                    .info(
                        "O texto entra no fim do prompt de sistema de cada chat, do Claude e do Codex, depois do que o \
                         Vitral já manda (a pasta do chat e as regras dele). Vale a partir da próxima mensagem.",
                    )
                    .descricao(div().line_clamp(2).child(crate::prompts::resumo_das_instrucoes(&config.instrucoes))),
                botao.into_any_element(),
            )
        });
        // A versão nova dos agentes: consulta sozinha ao abrir a parte, ou só pelo botão.
        let consulta_sozinha = config.consultar_versoes;
        let mudar_consulta = m();
        let versoes_grupo = GrupoDeOpcoes::new(Some("Versão nova".into()))
            .linha(com(
                LinhaDeOpcao::new("cfg-consultar-versoes", "Consultar a versão nova dos agentes")
                    .info("O Vitral pergunta ao npm qual é a versão mais nova do Claude Code e do Codex e diz “há a X” nas linhas de versão. “Só pelo botão” não consulta nada ao abrir: a consulta sai quando você clica em Consultar agora.")
                    .descricao("Ao abrir esta parte, ou só quando você pedir."),
                ControleSegmentado::new(
                    "cfg-consultar-versoes",
                    ["Sozinho", "Só pelo botão"],
                    usize::from(!consulta_sozinha),
                    move |novo, _, cx| mudar_consulta.update(cx, |m, cx| m.mudar_config(|c| c.consultar_versoes = novo == 0, cx)),
                )
                .with_size(Size::Small)
                .largura_total(estreito)
                .into_any_element(),
            ))
            .when(!consulta_sozinha, |g| {
                let botao = botao_fantasma("cfg-consultar-agora", Some(Lucide::RefreshCw.into()), "Consultar agora", TamanhoBotao::Controle, window, cx)
                    .bg(Cores::recuo())
                    .on_click(cx.listener(|this, _, _, cx| this.ler_os_instalados(true, cx)));
                g.linha(com(
                    LinhaDeOpcao::new("cfg-consultar-agora", "Ver se há versão nova")
                        .descricao("Consulta o Claude Code e o Codex agora."),
                    botao.into_any_element(),
                ))
            });
        let codex_grupo = GrupoDeOpcoes::new(Some("Codex".into())).linha(self.linha_do_codex(estreito, window, cx));
        let memoria_grupo = GrupoDeOpcoes::new(Some("Memória".into()))
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
                            .bg(Cores::recuo())
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
        // A versão do Claude Code e os servidores MCP dele num cartão só, como as outras partes.
        let claude_grupo = self.grupo_do_claude(estreito, window, cx)
            .linha(
                LinhaDeOpcao::new("cfg-mcp", "Servidores MCP do Claude Code")
                    .info("No Haiku, muitos servidores MCP estouram o contexto.")
                    // A descrição acompanha o estado, como a da Memória da equipe: ligado, ela só
                    // falava do desligado.
                    .descricao(if mcp {
                        "Ligado, os chats usam também os servidores do seu Claude Code."
                    } else {
                        "Desligado, só as ferramentas do Vitral."
                    })
                    .interruptor(mcp, move |ligado, _, cx| {
                        alternar_mcp.update(cx, |m, cx| m.mudar_config(|c| c.mcp_trabalhadores = ligado, cx))
                    }),
            );

        let (animacoes, sistema_sem) = movimento::animacoes(cx);
        let segmentos = [movimento::SEGUIR_O_SISTEMA, "Desligadas"];
        let atual = usize::from(animacoes == Animacoes::Desligadas);
        let interface = GrupoDeOpcoes::new(Some("Aparência".into()))
            .linha(
                LinhaDeOpcao::new("cfg-uso-no-canto", "Mostrar o uso do plano no canto da janela")
                    .info("Cada anel é a janela de 5 horas por fora e a da semana por dentro. O clique abre os detalhes e as contas.")
                    // Os anéis voltaram para baixo do × no mesmo dia em que foram ao cabeçalho
                    // (dono, 16/11/2026).
                    .descricao("Os anéis ficam no canto de cima, à direita, embaixo do ×. Desligado, o uso fica só em Contas do Claude.")
                    .interruptor(uso_no_canto, move |ligado, _, cx| {
                        alternar_uso.update(cx, |m, cx| m.mudar_config(|c| c.uso_no_canto = ligado, cx))
                    }),
            )
            .linha(
                LinhaDeOpcao::new("cfg-anel-claude", "Anel do Claude")
                    .descricao("Só aparece com a conta do Claude conectada.")
                    .interruptor(config.anel_claude, move |ligado, _, cx| {
                        anel_claude.update(cx, |m, cx| m.mudar_config(|c| c.anel_claude = ligado, cx))
                    }),
            )
            .linha(
                LinhaDeOpcao::new("cfg-anel-codex", "Anel do Codex")
                    .descricao("Só aparece com a conta do Codex conectada.")
                    .interruptor(config.anel_codex, move |ligado, _, cx| {
                        anel_codex.update(cx, |m, cx| m.mudar_config(|c| c.anel_codex = ligado, cx))
                    }),
            )
            .linha(
                LinhaDeOpcao::new("cfg-anel-gemini", "Anel do Gemini")
                    .descricao("Só aparece com o Antigravity CLI e a cota lida.")
                    .interruptor(config.anel_gemini, move |ligado, _, cx| {
                        anel_gemini.update(cx, |m, cx| m.mudar_config(|c| c.anel_gemini = ligado, cx))
                    }),
            )
            .linha(com(
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

        let mudar_numeros = m();
        let interface = interface.linha(com(
            LinhaDeOpcao::new("cfg-numeros", "Números nas abas e na lista")
                .info("Os números ligam a aba ao chat na lista (e valem para o Alt+1 a 4). “Só segurando Ctrl” esconde os dois e mostra depois de uns 150 ms com o Ctrl apertado; somem ao soltar.")
                .descricao("Quando o número do painel aparece."),
            ControleSegmentado::new(
                "cfg-numeros",
                ["Sempre", "Só segurando Ctrl"],
                usize::from(config.numeros_so_com_ctrl),
                move |novo, _, cx| mudar_numeros.update(cx, |m, cx| m.mudar_config(|c| c.numeros_so_com_ctrl = novo == 1, cx)),
            )
            .with_size(Size::Small)
            .largura_total(estreito)
            .into_any_element(),
        ));

        let zoom_de_agora = zoom::porcentagem(cx);
        let tamanho = GrupoDeOpcoes::new(Some("Tamanho".into()))
            .linha(com(
                LinhaDeOpcao::new("cfg-zoom", "Zoom da interface")
                    .info("O mesmo zoom do Ctrl+roda, do Ctrl+= e do Ctrl+-: a lista, a caixa de mensagem, os menus e as conversas.")
                    .descricao("Tudo cresce junto, como no Ctrl+roda e no Ctrl+=."),
                Contador::new("cfg-zoom", zoom_de_agora, zoom::FAIXA, move |novo, _, cx| zoom::mudar(if novo > zoom_de_agora { 1 } else { -1 }, cx))
                    .sufixo(SUFIXO_DE_PORCENTO)
                    .map(solto),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-letra", "Letra da conversa")
                    .info("Muda só as mensagens: o texto, o código e os cartões delas. A lista, a caixa de mensagem e os menus seguem o zoom.")
                    .descricao("O tamanho das mensagens em relação ao resto."),
                contador("cfg-letra", &crate::tema::LETRAS_DA_CONVERSA, config.letra_da_conversa, m(), |c, v| c.letra_da_conversa = v)
                    .sufixo(SUFIXO_DE_PORCENTO)
                    .map(solto),
            ))
            .linha({
                let alternar = m();
                LinhaDeOpcao::new("cfg-letra-caixa", "Letra da conversa também na caixa")
                    .info("A caixa de mensagem passa a escrever no mesmo tamanho das mensagens. Desligado, ela segue o zoom da interface.")
                    .descricao("A caixa de mensagem acompanha a letra da conversa.")
                    .interruptor(config.letra_na_caixa, move |ligado, _, cx| {
                        alternar.update(cx, |m, cx| m.mudar_config(|c| c.letra_na_caixa = ligado, cx))
                    })
            });

        // O diagnóstico para pedir ajuda, na parte das Atualizações (é do Vitral, não dos chats).
        let ajuda = GrupoDeOpcoes::new(Some("Ajuda".into())).linha({
            let botao = botao_fantasma("cfg-diagnostico-copiar", Some(Lucide::Copy.into()), "Copiar", TamanhoBotao::Controle, window, cx)
                .bg(Cores::recuo())
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(this.diagnostico(cx)));
                    this.diagnostico_copiado = true;
                    cx.notify();
                }));
            com(
                LinhaDeOpcao::new("cfg-diagnostico", "Diagnóstico para pedir ajuda")
                    .info(
                        "Um texto com a versão do Vitral, do Claude Code e do Codex, o sistema, onde ficam os dados, as \
                         opções ligadas e as últimas 40 linhas do registro de quedas. O que tem cara de chave ou token é \
                         coberto, a pasta pessoal vira ~, e nenhuma conversa entra.",
                    )
                    .descricao(if self.diagnostico_copiado {
                        "Copiado. Cole no pedido de ajuda."
                    } else {
                        "Versões, sistema, caminhos e quedas. Sem chaves nem conversas."
                    }),
                botao.into_any_element(),
            )
        });

        let trabalhando = self.motor.read(cx).trabalhando();
        let atualizacoes = Vista::da_global(cx).map(|vista| grupo_de_atualizacoes(&vista, trabalhando, estreito, window, cx));

        let grupos: Vec<AnyElement> = match self.parte {
            Parte::Chats => vec![
                conversas.into_any_element(),
                abas_grupo.into_any_element(),
                caixa_grupo.into_any_element(),
                arquivos_grupo.into_any_element(),
            ],
            Parte::Agentes => vec![
                agentes.into_any_element(),
                instrucoes_grupo.into_any_element(),
                codex_grupo.into_any_element(),
                versoes_grupo.into_any_element(),
            ],
            Parte::Perfis => vec![perfis::grupo(&self.motor, window, cx).into_any_element()],
            Parte::Memoria => vec![memoria_grupo.into_any_element()],
            Parte::Aparencia => vec![tamanho.into_any_element(), interface.into_any_element()],
            Parte::ClaudeCode => vec![claude_grupo.into_any_element()],
            Parte::Atualizacoes => {
                atualizacoes.into_iter().map(IntoElement::into_any_element).chain([ajuda.into_any_element()]).collect()
            }
        };
        // A coluna das partes: o ícone e o nome (no diálogo estreito, só o ícone, com o nome na
        // dica); a escolhida com o fundo da seleção, como as linhas da lista.
        let atual = self.parte;
        let partes = Parte::TODAS.into_iter().map(|parte| {
            let escolhida = parte == atual;
            div()
                .id(SharedString::from(format!("cfg-parte-{}", parte.nome())))
                .flex()
                .items_center()
                .gap(esp::E8)
                .h(r(32.))
                .px(esp::E8)
                .rounded(raio::CONTROLE)
                .cursor_pointer()
                .text_color(if escolhida { Cores::forte() } else { Cores::tenue() })
                .when(escolhida, |s| s.bg(Cores::selecao()))
                .when(!escolhida, |s| s.hover(|s| s.bg(Cores::hover()).text_color(Cores::texto())))
                .child(Icon::new(parte.icone()).with_size(tam(window, 16.)))
                .when(!estreito, |s| s.child(div().rotulo().child(parte.nome())))
                .when(estreito, |s| s.dica_no_ponteiro(move |window, cx| Tooltip::new(parte.nome()).build(window, cx)))
                .on_click(cx.listener(move |this, _, _, cx| this.ir_para(parte, cx)))
        });
        div()
            .flex()
            .gap(esp::E16)
            .h(r(ALTURA_DAS_CONFIGURACOES))
            .child(v_flex().flex_none().w(if estreito { r(40.) } else { r(176.) }).gap(esp::E2).children(partes))
            .child(div().flex_none().w(px(1.)).h_full().bg(Cores::linha()))
            .child(
                v_flex()
                    .id("cfg-parte")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .gap(esp::E24)
                    .pb(esp::E16)
                    .children(grupos),
            )
    }
}

#[cfg(test)]
mod testes {
    use super::{Atualizacao, Parte, linha_do_claude, vizinhos};

    /// As partes da 0.9.6 continuam as mesmas, na mesma ordem, e os Perfis entraram depois dos
    /// Agentes; cada nome é um só, e cabe na coluna.
    #[test]
    fn as_partes_da_coluna_com_os_perfis() {
        let nomes: Vec<&str> = Parte::TODAS.into_iter().map(Parte::nome).collect();
        assert_eq!(nomes, ["Chats", "Agentes", "Perfis", "Memória", "Aparência", "Claude Code", "Atualizações"]);
    }

    #[test]
    fn o_zoom_das_configuracoes_anda_na_mesma_faixa_dos_atalhos() {
        assert_eq!(super::zoom::FAIXA, 70..=200);
    }

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
