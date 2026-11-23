//! Configurações (valem para os chats que não escolheram outra coisa), em seis seções com a
//! navegação à esquerda (no diálogo estreito, uma fileira no alto): Chats, Agentes, Perfis,
//! Aparência, Avisos e Vitral. Cada seção é feita de grupos de linhas como as preferências do
//! GNOME: o texto à esquerda e o controle à direita (embaixo do texto no diálogo estreito). Grava
//! na hora. O que mora em cada seção está em `Secao::grupos`: é a lista que o teste confere, e a
//! tela desenha a partir dela. As Animações, o zoom e as Atualizações valem para o app todo (ficam
//! em `interface.json` e em `atualizacoes.json`).

use std::rc::Rc;

use gpui_kit::component::{
    Disableable as _, ElementExt as _, Icon, Size, Sizable as _,
    menu::{DropdownMenu, PopupMenuItem},
    spinner::Spinner,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    composer::{DESCRICOES_DE_MODELO, MODELOS, MODOS},
    componentes::{
        Chip, Contador, ControleSegmentado, GatilhoChip, GrupoDeOpcoes, LinhaDeOpcao, Marca, Simbolo, TamanhoBotao,
        TamanhoChip, barra_de_rolagem, botao_fantasma, ficha_do_escolhido, icone_do_modo, item_descrito, largura,
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
    tema::{Cores, Lucide, r, raio, tam},
    versoes_agentes::{self, Publicadas},
};

mod instrucoes;
pub(in crate::ui) mod perfis;

/// As seções das Configurações, na ordem da navegação.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Secao {
    Chats,
    Agentes,
    Perfis,
    Aparencia,
    Avisos,
    Vitral,
}

/// Um grupo de linhas de uma seção: o rótulo e as linhas (pelo id), na ordem em que aparecem.
pub(super) struct Grupo {
    pub titulo: &'static str,
    pub linhas: &'static [&'static str],
}

impl Secao {
    pub const TODAS: [Secao; 6] = [Secao::Chats, Secao::Agentes, Secao::Perfis, Secao::Aparencia, Secao::Avisos, Secao::Vitral];

    pub fn nome(self) -> &'static str {
        match self {
            Secao::Chats => "Chats",
            Secao::Agentes => "Agentes",
            Secao::Perfis => "Perfis",
            Secao::Aparencia => "Aparência",
            Secao::Avisos => "Avisos",
            Secao::Vitral => "Vitral",
        }
    }

    fn icone(self) -> Lucide {
        match self {
            Secao::Chats => Lucide::MessageSquare,
            Secao::Agentes => Lucide::Bot,
            Secao::Perfis => Lucide::UsersRound,
            Secao::Aparencia => Lucide::Palette,
            Secao::Avisos => Lucide::Bell,
            Secao::Vitral => Lucide::Info,
        }
    }

    /// O que mora na seção. `cfg-perfis` é a lista dos perfis (uma linha por perfil e o Novo
    /// perfil) e `cfg-atualizacoes` é o grupo das Atualizações do Vitral, que vem pronto.
    pub fn grupos(self) -> &'static [Grupo] {
        match self {
            Secao::Chats => &[
                Grupo { titulo: "Chats novos", linhas: &["cfg-agente", "cfg-modelo-trab", "cfg-modo"] },
                Grupo { titulo: "Caixa de mensagem", linhas: &["cfg-sugerir"] },
                Grupo { titulo: "Lista de chats", linhas: &["cfg-arquivar"] },
            ],
            Secao::Agentes => &[
                Grupo { titulo: "Suas instruções", linhas: &["cfg-instrucoes"] },
                Grupo { titulo: "O que os agentes carregam", linhas: &["cfg-mcp", "cfg-memoria-equipe", "cfg-memoria"] },
                Grupo { titulo: "Instalados", linhas: &["cfg-claude", "cfg-codex"] },
            ],
            Secao::Perfis => &[Grupo { titulo: "Perfis de agente", linhas: &["cfg-perfis"] }],
            Secao::Aparencia => &[
                Grupo { titulo: "Tamanho", linhas: &["cfg-zoom", "cfg-letra"] },
                Grupo { titulo: "Janela", linhas: &["cfg-transparencia", "cfg-animacoes", "cfg-uso-no-canto"] },
            ],
            Secao::Avisos => &[
                Grupo { titulo: "Dentro do Vitral", linhas: &["cfg-avisos-no-app", "cfg-resumo"] },
                Grupo { titulo: "Do sistema", linhas: &["cfg-notificacao"] },
            ],
            Secao::Vitral => &[
                Grupo { titulo: "Atualizações", linhas: &["cfg-atualizacoes"] },
                Grupo { titulo: "Ajuda", linhas: &["cfg-diagnostico"] },
            ],
        }
    }

    #[cfg(test)]
    pub fn linhas(self) -> impl Iterator<Item = &'static str> {
        self.grupos().iter().flat_map(|g| g.linhas.iter().copied())
    }

    /// A seção em que a linha `id` mora.
    #[cfg(test)]
    pub fn de(id: &str) -> Option<Secao> {
        Secao::TODAS.into_iter().find(|s| s.linhas().any(|l| l == id))
    }
}

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

pub struct Configuracoes {
    motor: Entity<Motor>,
    secao: Secao,
    rolagem: ScrollHandle,
    /// A altura da fileira das seções no diálogo estreito, medida no último desenho.
    altura_da_fileira: Rc<std::cell::Cell<Pixels>>,
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
            secao: Secao::Chats,
            rolagem: ScrollHandle::new(),
            altura_da_fileira: Rc::new(std::cell::Cell::new(px(48.))),
            codex: None,
            publicadas: versoes_agentes::guardadas().unwrap_or_default(),
            teste_da_notificacao: None,
            diagnostico_copiado: false,
            _observa,
            _atualizacoes,
            _acompanhando,
        }
    }

    fn ir_para(&mut self, secao: Secao, cx: &mut Context<Self>) {
        self.secao = secao;
        self.rolagem.set_offset(point(px(0.), px(0.)));
        if secao == Secao::Agentes {
            self.ler_os_instalados(cx);
        }
        cx.notify();
    }

    /// A versão do Codex e as mais novas publicadas, em segundo plano, quando a seção Agentes
    /// abre: o `codex --version` leva um instante e a consulta vai à rede (guardada por uma hora).
    fn ler_os_instalados(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |vista, cx| {
            let (codex, publicadas) =
                cx.background_spawn(async move { (versoes_agentes::versao_do_codex(), versoes_agentes::consultar()) }).await;
            let _ = vista.update(cx, |vista, cx| {
                (vista.codex, vista.publicadas) = (Some(codex), publicadas);
                cx.notify();
            });
        })
        .detach();
    }

    /// A linha do Claude Code: a versão que os turnos abrem, a nova quando há, e o Atualizar.
    fn linha_do_claude_code(&self, estreito: bool, window: &Window, cx: &Context<Self>) -> LinhaDeOpcao {
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
        .when_some(linha.espera.clone(), |b, motivo| b.disabled(true).opacity(0.45).tooltip(motivo))
        .on_click(move |_, _, cx| motor_e.update(cx, |m, cx| m.atualizar_claude(cx)));
        let opcao = LinhaDeOpcao::new("cfg-claude", "Claude Code")
            .marca(Simbolo::Marca(Marca::Claude))
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
        if estreito { opcao.campo(botao) } else { opcao.controle(botao) }
    }

    /// A linha do Codex: a versão instalada e a nova quando há; sem ele, o Instalar.
    fn linha_do_codex(&self, estreito: bool, window: &Window, cx: &Context<Self>) -> LinhaDeOpcao {
        let instalado = crate::agente_codex::instalado();
        let texto = match self.codex {
            None if instalado => "Lendo a versão…".to_string(),
            lida => versoes_agentes::linha_do_codex(instalado, lida.flatten(), self.publicadas.codex),
        };
        let opcao = LinhaDeOpcao::new("cfg-codex", "Codex")
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

/// O que as linhas precisam saber do desenho de agora.
struct Desenho {
    estreito: bool,
    ancora: Anchor,
    config: Config,
}

impl Desenho {
    /// No diálogo estreito, o controle desce para baixo do texto.
    fn com(&self, linha: LinhaDeOpcao, controle: AnyElement) -> LinhaDeOpcao {
        if self.estreito { linha.campo(controle) } else { linha.controle(controle) }
    }
}

impl Configuracoes {
    /// A linha de id `id` (os ids são os de `Secao::grupos`).
    fn linha(&self, id: &'static str, d: &Desenho, window: &Window, cx: &Context<Self>) -> AnyElement {
        let config = &d.config;
        let padrao = Config::default();
        let m = || self.motor.clone();
        let (estreito, ancora) = (d.estreito, d.ancora);
        match id {
            "cfg-agente" => d
                .com(
                    LinhaDeOpcao::new("cfg-agente", "Quem responde")
                        .info("O Codex é o agente de código da OpenAI. Ele usa o login e o plano do próprio Codex, e precisa estar instalado neste computador.")
                        .descricao("Cada chat troca no seletor de modelo."),
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
                )
                .into_any_element(),
            "cfg-modelo-trab" => d
                .com(
                    LinhaDeOpcao::new("cfg-modelo-trab", "Modelo do Claude").descricao("Cada chat troca na caixa de mensagem."),
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
                )
                .into_any_element(),
            "cfg-modo" => {
                let modos = MODOS
                    .iter()
                    .map(|(valor, nome, descricao)| {
                        let (icone, cor) = icone_do_modo(valor);
                        Opcao::new(*valor, *nome).descricao(*descricao).simbolo(Simbolo::Icone(icone, Some(cor)))
                    })
                    .collect();
                d.com(
                    LinhaDeOpcao::new("cfg-modo", "Modo de permissão")
                        .info("“Sem perguntar” é o mesmo do seu VS Code. Nos outros, os pedidos de permissão aparecem na conversa.")
                        .descricao("Cada chat troca na caixa de mensagem, ou com Shift+Tab."),
                    menu_escolha("cfg-permissao", ancora, modos, &config.permissao, &padrao.permissao, gravar(m(), |c, v| c.permissao = v), window, cx),
                )
                .into_any_element()
            }
            "cfg-sugerir" => {
                let (sugerir, alternar) = (config.sugerir_resposta, m());
                LinhaDeOpcao::new("cfg-sugerir", "Sugerir a próxima mensagem")
                    .info(
                        "Depois de cada resposta do Claude, o Claude Code adivinha o que você escreveria em seguida \
                         (“roda os testes”, “commita”). Quando o próximo passo não é óbvio, ele não sugere nada. É uma \
                         chamada curta, que reaproveita o cache da conversa.",
                    )
                    .descricao("Aparece apagada na caixa vazia; o Tab aceita. Só no Claude.")
                    .interruptor(sugerir, move |ligado, _, cx| {
                        alternar.update(cx, |m, cx| m.mudar_config(|c| c.sugerir_resposta = ligado, cx))
                    })
                    .into_any_element()
            }
            "cfg-arquivar" => d
                .com(
                    LinhaDeOpcao::new("cfg-arquivar", "Arquivar chats parados depois de")
                        .info(
                            "Vale para os chats sem novidade há esse tempo, já lidos, sem fila e sem pedido aberto. Os \
                             fixados ficam.",
                        )
                        .descricao("Nada é apagado: eles vão para Arquivadas."),
                    prazo("cfg-arquivar", &PRAZOS_ARQUIVAR, config.arquivar_apos_dias, estreito, m(), |c, v| c.arquivar_apos_dias = v, window, cx),
                )
                .into_any_element(),
            "cfg-instrucoes" => {
                let abrir = m();
                let vazio = config.instrucoes.trim().is_empty();
                let botao = botao_fantasma("cfg-instrucoes-editar", None, if vazio { "Escrever…" } else { "Editar…" }, TamanhoBotao::Barra, window, cx)
                    .on_click(move |_, window, cx| instrucoes::abrir(abrir.clone(), window, cx));
                d.com(
                    LinhaDeOpcao::new("cfg-instrucoes", "Instruções para todos os chats")
                        .info(
                            "O texto entra no fim do prompt de sistema de cada chat, do Claude e do Codex, depois do que o \
                             Vitral já manda (a pasta do chat e as regras dele). Vale a partir da próxima mensagem.",
                        )
                        .descricao(div().line_clamp(2).child(crate::prompts::resumo_das_instrucoes(&config.instrucoes))),
                    botao.into_any_element(),
                )
                .into_any_element()
            }
            "cfg-mcp" => {
                let (mcp, alternar) = (config.mcp_trabalhadores, m());
                LinhaDeOpcao::new("cfg-mcp", "Servidores MCP do Claude Code")
                    .info("No Haiku, muitos servidores MCP estouram o contexto.")
                    // A descrição acompanha o estado, como a da Memória da equipe.
                    .descricao(if mcp { "Os chats usam também os servidores do seu Claude Code." } else { "Os chats usam só as ferramentas do Vitral." })
                    .interruptor(mcp, move |ligado, _, cx| {
                        alternar.update(cx, |m, cx| m.mudar_config(|c| c.mcp_trabalhadores = ligado, cx))
                    })
                    .into_any_element()
            }
            "cfg-memoria-equipe" => {
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
                        botao_fantasma("cfg-memoria-equipe-abrir", None, if ligada { "Trocar…" } else { "Ligar…" }, TamanhoBotao::Barra, window, cx)
                            .on_click(move |_, window, cx| super::memoria_equipe::abrir(abrir.clone(), window, cx)),
                    )
                    .into_any_element()
            }
            "cfg-memoria" => {
                let (memoria, alternar) = (config.memoria_ao_terminar, m());
                LinhaDeOpcao::new("cfg-memoria", "Rever a memória ao terminar")
                    .info(
                        "Quando um turno que mexeu em arquivo termina, o Vitral pede ao agente, uma vez por sessão, \
                         para ver se algo vale guardar na memória ou nas instruções do projeto. Quase sempre a \
                         resposta é que não há nada a registrar.",
                    )
                    .descricao("Uma vez por sessão, depois de um turno que mexeu em arquivo.")
                    .interruptor(memoria, move |ligado, _, cx| {
                        alternar.update(cx, |m, cx| m.mudar_config(|c| c.memoria_ao_terminar = ligado, cx))
                    })
                    .into_any_element()
            }
            "cfg-claude" => self.linha_do_claude_code(estreito, window, cx).into_any_element(),
            "cfg-codex" => self.linha_do_codex(estreito, window, cx).into_any_element(),
            "cfg-zoom" => {
                let atual = zoom::porcentagem(cx);
                d.com(
                    LinhaDeOpcao::new("cfg-zoom", "Zoom da interface")
                        .descricao("Tudo cresce junto, como no Ctrl+roda e no Ctrl+=."),
                    Contador::new("cfg-zoom", atual, zoom::FAIXA, move |novo, _, cx| zoom::mudar(if novo > atual { 1 } else { -1 }, cx))
                        .sufixo(SUFIXO_DE_PORCENTO)
                        .map(solto),
                )
                .into_any_element()
            }
            "cfg-letra" => d
                .com(
                    LinhaDeOpcao::new("cfg-letra", "Letra da conversa")
                        .info("Muda só as mensagens: o texto, o código e os cartões delas. A lista, a caixa de mensagem e os menus seguem o zoom.")
                        .descricao("O tamanho das mensagens em relação ao resto."),
                    contador("cfg-letra", &crate::tema::LETRAS_DA_CONVERSA, config.letra_da_conversa, m(), |c, v| c.letra_da_conversa = v)
                        .sufixo(SUFIXO_DE_PORCENTO)
                        .map(solto),
                )
                .into_any_element(),
            "cfg-transparencia" => {
                let mudar = m();
                d.com(
                    LinhaDeOpcao::new("cfg-transparencia", "Transparência dos fundos")
                        .info(
                            "Quanto o fundo azul da lista e o cinza da conversa deixam ver o que está atrás da janela. \
                             Com papel de parede claro, os níveis de cima deixam o texto mais fraco difícil de ler. O pé da \
                             conversa, onde fica a caixa de mensagem, continua fechado.",
                        )
                        .descricao("Quanto se vê do que está atrás da janela."),
                    ControleSegmentado::new(
                        "cfg-transparencia",
                        crate::tema::NOMES_DA_TRANSPARENCIA,
                        usize::from(config.transparencia.min(3)),
                        move |novo, _, cx| mudar.update(cx, |m, cx| m.mudar_config(|c| c.transparencia = novo as u8, cx)),
                    )
                    .with_size(Size::Small)
                    .largura_total(estreito)
                    .into_any_element(),
                )
                .into_any_element()
            }
            "cfg-animacoes" => {
                let (animacoes, sistema_sem) = movimento::animacoes(cx);
                let segmentos = [movimento::SEGUIR_O_SISTEMA, "Desligadas"];
                let atual = usize::from(animacoes == Animacoes::Desligadas);
                d.com(
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
                )
                .into_any_element()
            }
            "cfg-uso-no-canto" => {
                let (uso, alternar) = (config.uso_no_canto, m());
                LinhaDeOpcao::new("cfg-uso-no-canto", "Uso do plano no canto da janela")
                    // Os anéis voltaram para baixo do × no mesmo dia em que foram ao cabeçalho
                    // (dono, 16/11/2026).
                    .descricao("Os anéis embaixo do ×. Desligado, o uso fica só em Contas do Claude.")
                    .interruptor(uso, move |ligado, _, cx| alternar.update(cx, |m, cx| m.mudar_config(|c| c.uso_no_canto = ligado, cx)))
                    .into_any_element()
            }
            "cfg-avisos-no-app" => {
                let (avisos, alternar) = (config.avisos_no_app, m());
                // Rodada de uso, item 3.1 (dono, 16/11/2026).
                LinhaDeOpcao::new("cfg-avisos-no-app", "Avisar dentro do app")
                    .info("Só para chat fora de vista e só para o que trava trabalho: pedido de permissão, plano para aprovar e falha. O aviso fica até ser respondido e nunca pega o foco da caixa.")
                    .descricao("No canto de baixo, à direita, quando um chat fora de vista precisa de você.")
                    .interruptor(avisos, move |ligado, _, cx| alternar.update(cx, |m, cx| m.mudar_config(|c| c.avisos_no_app = ligado, cx)))
                    .into_any_element()
            }
            "cfg-resumo" => d
                .com(
                    LinhaDeOpcao::new("cfg-resumo", "Resumo ao voltar depois de")
                        .descricao("Minutos com a janela fora de foco antes do “Enquanto você esteve fora”."),
                    contador("cfg-resumo", &[5, 10, 15, 30, 60, 120], config.minutos_para_resumo as u32, m(), |c, v| c.minutos_para_resumo = v as u64)
                        .map(solto),
                )
                .into_any_element(),
            "cfg-notificacao" => {
                let botao = botao_fantasma("cfg-notificacao-testar", Some(Lucide::Bell.into()), "Testar", TamanhoBotao::Barra, window, cx).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.teste_da_notificacao = Some(this.motor.read(cx).testar_notificacao());
                        cx.notify();
                    }),
                );
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
                d.com(linha, botao.into_any_element()).into_any_element()
            }
            "cfg-diagnostico" => {
                let botao = botao_fantasma("cfg-diagnostico-copiar", Some(Lucide::Copy.into()), "Copiar", TamanhoBotao::Barra, window, cx).on_click(
                    cx.listener(|this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(this.diagnostico(cx)));
                        this.diagnostico_copiado = true;
                        cx.notify();
                    }),
                );
                d.com(
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
                .into_any_element()
            }
            _ => {
                debug_assert!(false, "linha sem desenho: {id}");
                div().into_any_element()
            }
        }
    }

    /// Os grupos da seção à vista, um embaixo do outro.
    fn grupos(&self, d: &Desenho, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        let mut grupos = Vec::new();
        for grupo in self.secao.grupos() {
            match grupo.linhas {
                ["cfg-perfis"] => grupos.push(perfis::grupo(&self.motor, window, cx).into_any_element()),
                ["cfg-atualizacoes"] => {
                    let trabalhando = self.motor.read(cx).trabalhando();
                    grupos.extend(
                        Vista::da_global(cx).map(|vista| grupo_de_atualizacoes(&vista, trabalhando, d.estreito, window, cx).into_any_element()),
                    );
                }
                linhas => {
                    let mut caixa = GrupoDeOpcoes::new(Some(grupo.titulo.into()));
                    for id in linhas {
                        caixa = caixa.linha(self.linha(id, d, window, cx));
                    }
                    grupos.push(caixa.into_any_element());
                }
            }
        }
        grupos
    }

    /// Um item da navegação. O escolhido vai na ficha de metal, com o mesmo peso dos outros: a
    /// largura do texto não muda, e a fileira do diálogo estreito não pula.
    fn item_da_navegacao(&self, secao: Secao, em_coluna: bool, window: &Window, cx: &Context<Self>) -> AnyElement {
        let escolhida = secao == self.secao;
        div()
            .id(("cfg-secao", secao as usize))
            .relative()
            .flex()
            .flex_none()
            .items_center()
            // Na fileira do diálogo estreito, mais junto: as seis cabem numa linha na janela mínima.
            .gap(if em_coluna { r(8.) } else { r(6.) })
            .h(r(32.))
            .px(if em_coluna { r(12.) } else { r(8.) })
            .rounded(raio::de(32.))
            .when(em_coluna, |d| d.w_full())
            .text_size(r(14.))
            .line_height(r(20.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(if escolhida { Cores::forte() } else { Cores::tenue() })
            .when(escolhida, |d| d.child(ficha_do_escolhido(raio::de(32.))))
            .when(!escolhida, |d| d.cursor_pointer().hover(|s| s.bg(Cores::hover()).text_color(Cores::texto())))
            .child(div().relative().flex_none().child(Icon::new(secao.icone()).with_size(tam(window, 16.))))
            .child(div().relative().child(secao.nome()))
            .on_click(cx.listener(move |this, _, _, cx| this.ir_para(secao, cx)))
            .into_any_element()
    }
}

/// A altura do conteúdo que cabe no diálogo sem ele rolar por fora: a da janela (sem a sombra),
/// menos o que o diálogo do kit gasta (16 px de cada borda da janela e 1 px de fio em cima e
/// embaixo; por dentro, 16 de margem em cima e embaixo, o título de 32 e o vão de 8) e menos
/// `fileira`. No máximo, os 520 do desenho.
fn altura_do_conteudo(window: &Window, fileira: Pixels) -> Pixels {
    let sombra = gpui_kit::component::window_paddings(window);
    let janela = window.viewport_size().height - sombra.top - sombra.bottom;
    let moldura = px(32. + 2.) + tam(window, 16. + 16. + 32. + 8.);
    tam(window, 520.).min(janela - moldura - fileira - px(4.)).max(px(160.))
}

impl Render for Configuracoes {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let estreito = estreito(window, largura::CONFIGURACOES);
        let desenho = Desenho {
            estreito,
            ancora: if estreito { Anchor::TopLeft } else { Anchor::TopRight },
            config: self.motor.read(cx).config.clone(),
        };
        let itens: Vec<AnyElement> = Secao::TODAS.into_iter().map(|s| self.item_da_navegacao(s, !estreito, window, cx)).collect();
        // A altura é uma só para todas as seções, para o diálogo não mudar de tamanho ao trocar: a
        // de desenho (520 a 100%) ou a que couber na janela. O que passar disso rola aqui dentro,
        // com a navegação parada. No diálogo estreito, a fileira das seções (medida no desenho
        // anterior: ela pode quebrar em duas linhas) e o vão embaixo dela saem da conta.
        let fileira = if estreito { self.altura_da_fileira.get() + tam(window, 12.) } else { px(0.) };
        let altura = altura_do_conteudo(window, fileira);
        // A barra fina só aparece quando a seção passa da altura; os 6 à direita são o lugar dela.
        // Em coluna (diálogo estreito), o `flex_1` valeria para a altura e passaria por cima dela.
        let conteudo = div()
            .relative()
            .map(|d| if estreito { d.w_full().flex_none() } else { d.flex_1().min_w_0() })
            .h(altura)
            .child(
                div()
                    .id("cfg-conteudo")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.rolagem)
                    .child(div().flex().flex_col().gap(r(16.)).pb(r(4.)).pr(r(6.)).children(self.grupos(&desenho, window, cx))),
            )
            .child(barra_de_rolagem("cfg-barra", &self.rolagem));
        if estreito {
            // A fileira quebra a linha quando não cabe: as seis seções ficam sempre à vista.
            div()
                .flex()
                .flex_col()
                .gap(r(12.))
                .child({
                    let (medida, vista) = (self.altura_da_fileira.clone(), cx.entity().downgrade());
                    div().relative().flex().flex_wrap().gap(r(2.)).children(itens).on_prepaint(move |limites, _, cx| {
                        if medida.get() != limites.size.height {
                            medida.set(limites.size.height);
                            // A conta da altura usa a medida no desenho seguinte (pedido depois deste).
                            cx.defer(move |cx| {
                                let _ = vista.update(cx, |_, cx| cx.notify());
                            });
                        }
                    })
                })
                .child(conteudo)
        } else {
            div()
                .flex()
                .items_start()
                .gap(r(16.))
                .child(div().flex().flex_col().flex_none().w(r(168.)).gap(r(2.)).children(itens))
                .child(conteudo)
        }
    }
}

#[cfg(test)]
mod testes {
    use super::{Atualizacao, Secao, linha_do_claude, vizinhos};

    /// As linhas que a tela tinha antes das seções (0.9.5), pelo id.
    const DE_ANTES: [&str; 15] = [
        "cfg-resumo",
        "cfg-avisos-no-app",
        "cfg-uso-no-canto",
        "cfg-modo",
        "cfg-agente",
        "cfg-modelo-trab",
        "cfg-arquivar",
        "cfg-mcp",
        "cfg-memoria-equipe",
        "cfg-memoria",
        "cfg-sugerir",
        "cfg-animacoes",
        "cfg-transparencia",
        "cfg-claude",
        "cfg-atualizacoes",
    ];

    #[test]
    fn toda_opcao_de_antes_continua_na_tela_em_uma_secao_so() {
        for id in DE_ANTES {
            let onde: Vec<Secao> = Secao::TODAS.into_iter().filter(|s| s.linhas().any(|l| l == id)).collect();
            assert_eq!(onde.len(), 1, "{id} está em {onde:?}");
        }
        // Nenhum id repetido na tela inteira.
        let mut todas: Vec<&str> = Secao::TODAS.into_iter().flat_map(Secao::linhas).collect();
        let total = todas.len();
        todas.sort();
        todas.dedup();
        assert_eq!(todas.len(), total);
    }

    #[test]
    fn cada_assunto_mora_na_secao_dele() {
        // O que estava solto em "Chats" foi para a seção do assunto.
        for (id, secao) in [
            ("cfg-agente", Secao::Chats),
            ("cfg-modelo-trab", Secao::Chats),
            ("cfg-modo", Secao::Chats),
            ("cfg-arquivar", Secao::Chats),
            ("cfg-uso-no-canto", Secao::Aparencia),
            ("cfg-transparencia", Secao::Aparencia),
            ("cfg-animacoes", Secao::Aparencia),
            ("cfg-avisos-no-app", Secao::Avisos),
            ("cfg-resumo", Secao::Avisos),
            ("cfg-mcp", Secao::Agentes),
            ("cfg-memoria-equipe", Secao::Agentes),
            ("cfg-memoria", Secao::Agentes),
            ("cfg-claude", Secao::Agentes),
            ("cfg-atualizacoes", Secao::Vitral),
        ] {
            assert_eq!(Secao::de(id), Some(secao), "{id}");
        }
        // As três primeiras linhas da tela são as que mais se mexe: quem responde, modelo e modo.
        assert_eq!(Secao::TODAS[0].linhas().take(3).collect::<Vec<_>>(), ["cfg-agente", "cfg-modelo-trab", "cfg-modo"]);
    }

    #[test]
    fn o_que_entrou_nesta_rodada_tem_lugar_na_tela() {
        for (id, secao) in [
            ("cfg-letra", Secao::Aparencia),
            ("cfg-zoom", Secao::Aparencia),
            ("cfg-instrucoes", Secao::Agentes),
            ("cfg-perfis", Secao::Perfis),
            ("cfg-codex", Secao::Agentes),
            ("cfg-notificacao", Secao::Avisos),
            ("cfg-diagnostico", Secao::Vitral),
        ] {
            assert_eq!(Secao::de(id), Some(secao), "{id}");
        }
        assert_eq!(Secao::de("cfg-que-nao-existe"), None);
    }

    #[test]
    fn nenhuma_secao_volta_a_ser_lista_corrida() {
        for secao in Secao::TODAS {
            assert!(secao.linhas().count() <= 7, "{secao:?} tem linhas demais");
            assert!(secao.grupos().iter().all(|g| !g.titulo.is_empty() && !g.linhas.is_empty() && g.linhas.len() <= 3), "{secao:?}");
            // Nome curto, de uma palavra: cabe na coluna e na fileira do diálogo estreito.
            assert!(secao.nome().chars().count() <= 9 && !secao.nome().contains(' '), "{}", secao.nome());
        }
        let mut nomes: Vec<&str> = Secao::TODAS.into_iter().map(Secao::nome).collect();
        nomes.dedup();
        assert_eq!(nomes.len(), 6);
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
