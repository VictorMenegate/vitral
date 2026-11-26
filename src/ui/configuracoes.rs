//! Configurações da pasta de trabalho (valem para as conversas que não escolheram outra coisa),
//! em grupos de linhas como as preferências do GNOME: o texto à esquerda e o controle à direita
//! (embaixo do texto no diálogo estreito). Grava na hora. As Animações, o zoom e as Atualizações
//! valem para o app todo (ficam em `interface.json` e em `atualizacoes.json`). A organização é a
//! das partes (`Parte`, a coluna da esquerda); as instruções, os perfis, a letra da conversa, o
//! teste da notificação, a versão do Codex e o diagnóstico entraram dentro delas.

use std::rc::Rc;

use gpui_kit::component::{
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    slider::{Slider, SliderEvent, SliderState},
    Disableable as _, Icon, Size, Sizable as _, WindowExt as _, h_flex,
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
        TamanhoChip, botao_fantasma, botao_icone, icone_do_modo, item_descrito,
    },
    atualizacao::{Vista, grupo as grupo_de_atualizacoes},
    dialogos::estreito,
    movimento::{self, Animacoes},
    zoom,
};
use crate::{
    atualizacao::{self, Acompanhando, Atualizacoes},
    claude::{self, Atualizacao, Versao},
    modelo::{Agente, Config},
    motor::{self, Motor},
    paleta::{Cor, CoresDoFundo, FundoVivo, PERSONALIZADO, Paleta, TEMAS},
    tema::{Cores, Lucide, esp, linear, r, raio, tam},
    versoes_agentes::{self, Publicadas},
};

mod agentes_fora;
mod instrucoes;
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

/// Por que o Atualizar não dá para clicar agora (`None`: dá): ele já está rodando, ou há chats do
/// agente trabalhando (a atualização troca o binário no disco).
pub(super) fn espera_da_atualizacao(andando: bool, a_caminho: usize) -> Option<String> {
    if andando {
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
    }
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
    let espera = espera_da_atualizacao(andando, a_caminho);
    LinhaDoClaude { texto, andando, resposta: linhas, falhou: resposta.is_some_and(|r| !r.ok), espera }
}

/// As partes das Configurações, na coluna da esquerda (dono, 23/11/2026: "faz ser uma coluna na
/// esquerda com as configurações separadas em cada parte").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Parte {
    Chats,
    Agentes,
    Revisao,
    Notificacoes,
    Memoria,
    Aparencia,
    Atalhos,
    ClaudeCode,
    Codex,
    Gemini,
    Atualizacoes,
    /// Um agente ACP novo (`agente_acp`), aberto pela lista das IAs.
    OutraIa(Agente),
}

impl Parte {
    /// As partes da coluna (protótipo Linear, 25/11/2026; os Perfis saíram). O Claude Code, o Codex
    /// e o Gemini saíram da coluna para dentro de Agentes, que junta todas as IAs, com a versão, o
    /// Instalar e o Atualizar de cada uma (dono, 26/11/2026: "melhora nas configurações pra ter uma
    /// aba que junte todas"); a parte de cada uma abre por lá.
    const TODAS: [Parte; 8] = [
        Parte::Chats,
        Parte::Agentes,
        Parte::Revisao,
        Parte::Notificacoes,
        Parte::Memoria,
        Parte::Aparencia,
        Parte::Atalhos,
        Parte::Atualizacoes,
    ];

    /// A parte da coluna que fica acesa: a de cada IA mora dentro de Agentes.
    fn na_coluna(self) -> Parte {
        match self {
            Parte::ClaudeCode | Parte::Codex | Parte::Gemini | Parte::OutraIa(_) => Parte::Agentes,
            outra => outra,
        }
    }

    /// A parte de uma IA, aberta pela lista de Agentes.
    fn da_ia(agente: Agente) -> Parte {
        match agente {
            Agente::Claude => Parte::ClaudeCode,
            Agente::Codex => Parte::Codex,
            Agente::Gemini => Parte::Gemini,
            outro => Parte::OutraIa(outro),
        }
    }

    fn nome(self) -> &'static str {
        match self {
            Parte::Chats => "Chats",
            Parte::Agentes => "Agentes",
            Parte::Revisao => "Revisão",
            Parte::Notificacoes => "Notificações",
            Parte::Memoria => "Memória",
            Parte::Aparencia => "Aparência",
            Parte::Atalhos => "Atalhos",
            Parte::ClaudeCode => "Claude Code",
            Parte::Codex => "Codex",
            Parte::Gemini => "Gemini",
            Parte::Atualizacoes => "Atualizações",
            Parte::OutraIa(a) => a.programa(),
        }
    }

    fn icone(self) -> Lucide {
        match self {
            Parte::Chats => Lucide::MessagesSquare,
            Parte::Agentes => Lucide::UsersRound,
            Parte::Revisao => Lucide::Eye,
            Parte::Notificacoes => Lucide::Bell,
            Parte::Memoria => Lucide::Brain,
            Parte::Aparencia => Lucide::Sun,
            Parte::Atalhos => Lucide::Keyboard,
            // O Claude Code, o Codex e o Gemini levam a marca deles na coluna (`simbolo`).
            Parte::ClaudeCode | Parte::Codex | Parte::Gemini | Parte::OutraIa(_) => Lucide::Bot,
            Parte::Atualizacoes => Lucide::RefreshCw,
        }
    }

    /// O que vai à esquerda do nome na coluna: a marca do agente, o ícone do resto.
    fn simbolo(self) -> Simbolo {
        match self {
            Parte::ClaudeCode => Simbolo::Marca(Marca::Claude),
            Parte::Codex => Simbolo::Marca(Marca::Codex),
            Parte::Gemini => Simbolo::Marca(Marca::Gemini),
            Parte::OutraIa(a) => Simbolo::Marca(Marca::do_agente(a)),
            outra => Simbolo::icone(outra.icone()),
        }
    }
}

/// A altura do corpo das Configurações, a 100%: a coluna das partes fica parada e só o lado
/// direito rola, sem o diálogo mudar de tamanho a cada parte.
const ALTURA_DAS_CONFIGURACOES: f32 = 560.;
/// O que o diálogo gasta fora do corpo (o cabeçalho, o respiro de baixo e a margem até a borda da
/// janela), a 100%: o corpo nunca passa da janela menos isto.
const FOLGA_DO_DIALOGO: f32 = 80.;
/// O corpo não fica menor que isto (a 100%), por menor que seja a janela.
const MENOR_CORPO: f32 = 240.;

/// A altura do corpo das Configurações, em pixels: a de sempre (`cheia`), ou, com a janela baixa
/// (a 150% o corpo de 520 passava da altura dela, e a roda rolava o diálogo inteiro), o que sobra
/// da janela depois da `folga`; nunca menos que `minimo`. Com o corpo menor que o conteúdo, só a
/// coluna da parte rola.
fn altura_do_corpo(janela: f32, cheia: f32, folga: f32, minimo: f32) -> f32 {
    cheia.min(janela - folga).max(minimo)
}
/// A largura do diálogo, a 100%: mais largo que o G, com a coluna das partes e as opções lado a lado.
pub(super) const LARGURA_DAS_CONFIGURACOES: f32 = 760.;

pub struct Configuracoes {
    motor: Entity<Motor>,
    /// A janela, para o que abre fora daqui: o seletor da pasta dos chats novos e a lista dos
    /// servidores MCP (os dois saíram do menu do logo em 24/11/2026 e moram aqui).
    janela: WeakEntity<super::Janela>,
    parte: Parte,
    /// As versões mais novas publicadas, quando a consulta responde.
    publicadas: Publicadas,
    /// O que o Testar da notificação deu, depois do clique.
    teste_da_notificacao: Option<Result<(), String>>,
    /// O diagnóstico acabou de ser copiado.
    diagnostico_copiado: bool,
    /// A parte acabou de trocar: ela entra deslizando do lado para onde se foi (o desenho seguinte
    /// leva, e zera).
    entra_de: Option<movimento::De>,
    _observa: Subscription,
    _atualizacoes: Subscription,
    /// Aberta, o download conta o progresso (no máximo 2 vezes por segundo).
    _acompanhando: Option<Acompanhando>,
    /// Os seletores de cor e as faixas de contraste das Cores, feitos na primeira vez que a
    /// Aparência abre (o seletor precisa da janela).
    cores: Option<ControlesDeCor>,
    /// O que o Copiar ou o Importar do tema acabou de fazer (some em 2,5 s).
    aviso_do_tema: Option<SharedString>,
    geracao_do_aviso: u32,
}

/// Os controles das Cores que têm estado.
struct ControlesDeCor {
    destaque: Entity<ColorPickerState>,
    fundo: Entity<ColorPickerState>,
    lista_destaque: Entity<ColorPickerState>,
    lista_fundo: Entity<ColorPickerState>,
    contraste: Entity<SliderState>,
    lista_contraste: Entity<SliderState>,
    _assinaturas: Vec<Subscription>,
}

/// Qual das quatro cores um seletor muda.
#[derive(Clone, Copy)]
enum CampoDeCor {
    Destaque,
    Fundo,
    ListaDestaque,
    ListaFundo,
}

impl CampoDeCor {
    fn em(self, p: &mut Paleta) -> &mut Cor {
        match self {
            CampoDeCor::Destaque => &mut p.destaque,
            CampoDeCor::Fundo => &mut p.fundo,
            CampoDeCor::ListaDestaque => &mut p.lista.destaque,
            CampoDeCor::ListaFundo => &mut p.lista.fundo,
        }
    }
}

fn hsla_de(cor: Cor) -> Hsla {
    rgb(cor.0).into()
}

fn cor_de(cor: Hsla) -> Cor {
    let c = Rgba::from(cor);
    let canal = |v: f32| (v.clamp(0., 1.) * 255.).round() as u32;
    Cor((canal(c.r) << 16) | (canal(c.g) << 8) | canal(c.b))
}

/// A gravação do tema que espera o arrasto parar: só a última grava.
static GRAVACAO_DO_TEMA: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

impl Configuracoes {
    pub fn new(motor: Entity<Motor>, janela: WeakEntity<super::Janela>, cx: &mut Context<Self>) -> Self {
        let _observa = cx.observe(&motor, |_, _, cx| cx.notify());
        let _atualizacoes = cx.observe_global::<Atualizacoes>(|_, cx| cx.notify());
        let _acompanhando = atualizacao::acompanhar_progresso(cx);
        // A linha do Claude Code mostra a versão instalada: ela pode ter mudado por fora (ou pelo
        // próprio Atualizar, numa abertura anterior) desde a leitura da abertura do app.
        motor.update(cx, |m, cx| m.reler_versao_claude(cx));
        Self {
            motor,
            janela,
            parte: Parte::Chats,
            publicadas: versoes_agentes::guardadas().unwrap_or_default(),
            teste_da_notificacao: None,
            diagnostico_copiado: false,
            entra_de: None,
            _observa,
            _atualizacoes,
            _acompanhando,
            cores: None,
            aviso_do_tema: None,
            geracao_do_aviso: 0,
        }
    }

    /// Os seletores e as faixas das Cores, com a paleta de agora.
    fn preparar_cores(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cores.is_some() {
            return;
        }
        let p = self.motor.read(cx).config.paleta.clone();
        let mut assinaturas = Vec::new();
        let mut seletor = |campo: CampoDeCor, cor: Cor, window: &mut Window, cx: &mut Context<Self>| {
            let estado = cx.new(|cx| ColorPickerState::new(window, cx).default_value(hsla_de(cor)));
            assinaturas.push(cx.subscribe(&estado, move |este, _, evento: &ColorPickerEvent, cx| {
                let ColorPickerEvent::Change(Some(nova)) = evento else { return };
                let nova = cor_de(*nova);
                este.mudar_paleta(false, move |p| {
                    *campo.em(p) = nova;
                    p.nome = PERSONALIZADO.into();
                }, cx);
            }));
            estado
        };
        let destaque = seletor(CampoDeCor::Destaque, p.destaque, window, cx);
        let fundo = seletor(CampoDeCor::Fundo, p.fundo, window, cx);
        let lista_destaque = seletor(CampoDeCor::ListaDestaque, p.lista.destaque, window, cx);
        let lista_fundo = seletor(CampoDeCor::ListaFundo, p.lista.fundo, window, cx);
        let mut faixa = |da_lista: bool, valor: u8, cx: &mut Context<Self>| {
            let estado = cx.new(|_| SliderState::new().min(0.).max(100.).step(1.).default_value(f32::from(valor)));
            assinaturas.push(cx.subscribe(&estado, move |este, _, evento: &SliderEvent, cx| {
                let (valor, soltou) = match evento {
                    SliderEvent::Change(v) => (v.end(), false),
                    SliderEvent::Release(v) => (v.end(), true),
                };
                let valor = valor.round().clamp(0., 100.) as u8;
                este.mudar_paleta(soltou, move |p| {
                    if da_lista { p.lista.contraste = valor } else { p.contraste = valor }
                    p.nome = PERSONALIZADO.into();
                }, cx);
            }));
            estado
        };
        let contraste = faixa(false, p.contraste, cx);
        let lista_contraste = faixa(true, p.lista.contraste, cx);
        self.cores = Some(ControlesDeCor {
            destaque,
            fundo,
            lista_destaque,
            lista_fundo,
            contraste,
            lista_contraste,
            _assinaturas: assinaturas,
        });
    }

    /// Muda a paleta da configuração. Um arrasto (cor, contraste) muda a cada passo e grava quando
    /// para (`gravar_ja` falso: 400 ms depois do último passo); o resto grava na hora.
    fn mudar_paleta(&mut self, gravar_ja: bool, f: impl FnOnce(&mut Paleta), cx: &mut Context<Self>) {
        let geracao = GRAVACAO_DO_TEMA.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        self.motor.update(cx, |m, cx| {
            if gravar_ja { m.mudar_config(|c| f(&mut c.paleta), cx) } else { m.mudar_config_sem_gravar(|c| f(&mut c.paleta), cx) }
        });
        if !gravar_ja {
            let motor = self.motor.clone();
            cx.spawn(async move |_, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(400)).await;
                if GRAVACAO_DO_TEMA.load(std::sync::atomic::Ordering::Relaxed) == geracao {
                    let _ = motor.read_with(cx, |m, _| m.gravar_a_config());
                }
            })
            .detach();
        }
    }

    /// Uma paleta inteira nova (tema pronto, Importar): grava e acerta os seletores e as faixas.
    fn trocar_paleta(&mut self, nova: Paleta, window: &mut Window, cx: &mut Context<Self>) {
        let p = nova.clone();
        self.mudar_paleta(true, move |atual| *atual = nova, cx);
        let Some(c) = &self.cores else { return };
        for (estado, cor) in [(&c.destaque, p.destaque), (&c.fundo, p.fundo), (&c.lista_destaque, p.lista.destaque), (&c.lista_fundo, p.lista.fundo)] {
            estado.update(cx, |e, cx| e.set_value(hsla_de(cor), window, cx));
        }
        for (estado, valor) in [(&c.contraste, p.contraste), (&c.lista_contraste, p.lista.contraste)] {
            estado.update(cx, |e, cx| e.set_value(f32::from(valor), window, cx));
        }
    }

    fn escolher_tema(&mut self, nome: &str, window: &mut Window, cx: &mut Context<Self>) {
        let atual = self.motor.read(cx).config.paleta.clone();
        match atual.com_tema(nome) {
            Some(nova) => self.trocar_paleta(nova, window, cx),
            None => self.mudar_paleta(true, |p| p.nome = PERSONALIZADO.into(), cx),
        }
    }

    fn copiar_tema(&mut self, cx: &mut Context<Self>) {
        let texto = self.motor.read(cx).config.paleta.para_compartilhar();
        cx.write_to_clipboard(ClipboardItem::new_string(texto));
        self.avisar_do_tema("Tema copiado.", cx);
    }

    /// O Importar lê o tema copiado (daqui ou do protótipo) da área de transferência.
    fn importar_tema(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let texto = cx.read_from_clipboard().and_then(|c| c.text()).unwrap_or_default();
        let atual = self.motor.read(cx).config.paleta.clone();
        match atual.importada(&texto) {
            Some(nova) => {
                self.trocar_paleta(nova, window, cx);
                self.avisar_do_tema("Tema importado.", cx);
            }
            None => self.avisar_do_tema("Copie um tema do Vitral antes.", cx),
        }
    }

    fn avisar_do_tema(&mut self, texto: &'static str, cx: &mut Context<Self>) {
        self.aviso_do_tema = Some(texto.into());
        self.geracao_do_aviso += 1;
        let geracao = self.geracao_do_aviso;
        cx.notify();
        cx.spawn(async move |vista, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(2500)).await;
            let _ = vista.update(cx, |vista, cx| {
                if vista.geracao_do_aviso == geracao {
                    vista.aviso_do_tema = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// As Cores da Aparência, como a janela de Aparência do protótipo: o tema e as três cores dele;
    /// as cores próprias da lista; o fundo animado; as bordas finas; o Importar e o Copiar.
    fn grupos_das_cores(&self, p: &Paleta, estreito: bool, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        let Some(c) = &self.cores else { return Vec::new() };
        let com = |linha: LinhaDeOpcao, controle: AnyElement| {
            if estreito { linha.campo(controle) } else { linha.controle(controle) }
        };
        let ancora = if estreito { Anchor::TopLeft } else { Anchor::TopRight };
        let vista = cx.entity();

        let mut temas: Vec<Opcao> = TEMAS.iter().map(|(valor, nome)| Opcao::new(*valor, *nome)).collect();
        temas.push(Opcao::new(PERSONALIZADO, "Personalizado"));
        let v = vista.clone();
        let tema = GrupoDeOpcoes::new(Some("Cores".into()))
            .linha(com(
                LinhaDeOpcao::new("cfg-tema", "Tema").cabeca(),
                menu_escolha("cfg-tema", ancora, temas, &p.nome, &p.nome, move |valor, window, cx| {
                    v.update(cx, |este, cx| este.escolher_tema(&valor, window, cx));
                }, window, cx),
            ))
            .linha(com(LinhaDeOpcao::new("cfg-destaque", "Destaque").secundaria(), pilula_de_cor(&c.destaque, ancora, window)))
            .linha(com(LinhaDeOpcao::new("cfg-fundo", "Fundo").secundaria(), pilula_de_cor(&c.fundo, ancora, window)))
            .linha(com(LinhaDeOpcao::new("cfg-contraste", "Contraste").secundaria(), faixa_de_contraste(&c.contraste, p.contraste)));

        let v = vista.clone();
        let mut lista = GrupoDeOpcoes::new(None).linha(
            LinhaDeOpcao::new("cfg-cores-lista", "Cores da lista").cabeca().interruptor(p.lista.ligada, move |ligado, _, cx| {
                v.update(cx, |este, cx| {
                    este.mudar_paleta(true, move |p| {
                        p.lista.ligada = ligado;
                        p.nome = PERSONALIZADO.into();
                    }, cx)
                });
            }),
        );
        if p.lista.ligada {
            lista = lista
                .linha(com(LinhaDeOpcao::new("cfg-lista-destaque", "Destaque").secundaria(), pilula_de_cor(&c.lista_destaque, ancora, window)))
                .linha(com(LinhaDeOpcao::new("cfg-lista-fundo", "Fundo").secundaria(), pilula_de_cor(&c.lista_fundo, ancora, window)))
                .linha(com(
                    LinhaDeOpcao::new("cfg-lista-contraste", "Contraste").secundaria(),
                    faixa_de_contraste(&c.lista_contraste, p.lista.contraste),
                ));
        }

        let vivo = match p.fundo_vivo {
            FundoVivo::Smoke => "smoke",
            FundoVivo::Mesh => "mesh",
            FundoVivo::Desligado => "desligado",
        };
        let cores_do_fundo = if p.cores_do_fundo == CoresDoFundo::Receita { "receita" } else { "tema" };
        let (v1, v2) = (vista.clone(), vista.clone());
        let fundo = GrupoDeOpcoes::new(None)
            .linha(com(
                LinhaDeOpcao::new("cfg-fundo-vivo", "Fundo animado").cabeca(),
                menu_escolha(
                    "cfg-fundo-vivo",
                    ancora,
                    vec![Opcao::new("smoke", "Smoke"), Opcao::new("mesh", "Mesh drift"), Opcao::new("desligado", "Desligado")],
                    vivo,
                    vivo,
                    move |valor, _, cx| {
                        let novo = match valor.as_str() {
                            "mesh" => FundoVivo::Mesh,
                            "desligado" => FundoVivo::Desligado,
                            _ => FundoVivo::Smoke,
                        };
                        v1.update(cx, |este, cx| este.mudar_paleta(true, move |p| p.fundo_vivo = novo, cx));
                    },
                    window,
                    cx,
                ),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-cores-do-fundo", "Cores do fundo").cabeca(),
                menu_escolha(
                    "cfg-cores-do-fundo",
                    ancora,
                    vec![Opcao::new("tema", "Do tema"), Opcao::new("receita", "Da receita")],
                    cores_do_fundo,
                    cores_do_fundo,
                    move |valor, _, cx| {
                        let novo = if valor == "receita" { CoresDoFundo::Receita } else { CoresDoFundo::Tema };
                        v2.update(cx, |este, cx| este.mudar_paleta(true, move |p| p.cores_do_fundo = novo, cx));
                    },
                    window,
                    cx,
                ),
            ));

        let v = vista.clone();
        let bordas = GrupoDeOpcoes::new(None).linha(
            LinhaDeOpcao::new("cfg-bordas", "Bordas finas").cabeca().interruptor(p.bordas, move |ligado, _, cx| {
                v.update(cx, |este, cx| este.mudar_paleta(true, move |p| p.bordas = ligado, cx));
            }),
        );

        let compartilhar = GrupoDeOpcoes::new(None).linha(com(
            LinhaDeOpcao::new("cfg-compartilhar", "Compartilhar").cabeca(),
            h_flex()
                .gap(esp::E4)
                .child(botao_de_texto("cfg-tema-importar", "Importar", window, cx).on_click(cx.listener(|este, _, window, cx| este.importar_tema(window, cx))))
                .child(botao_de_texto("cfg-tema-copiar", "Copiar", window, cx).on_click(cx.listener(|este, _, _, cx| este.copiar_tema(cx))))
                .into_any_element(),
        ));

        let mut grupos: Vec<AnyElement> = vec![
            tema.into_any_element(),
            lista.into_any_element(),
            fundo.into_any_element(),
            bordas.into_any_element(),
            compartilhar.into_any_element(),
        ];
        if let Some(aviso) = self.aviso_do_tema.clone() {
            grupos.push(div().px(linear::AVISO_LADO).text_size(r(12.)).text_color(Cores::destaque_texto()).child(aviso).into_any_element());
        }
        grupos
    }

    /// A parte de um agente ACP novo: onde está o programa (com o Instalar, ou o Atualizar no
    /// terminal), a conta (o Entrar, que abre o login dele) e o que ele é.
    fn grupo_da_ia_acp(&self, agente: Agente, window: &Window, cx: &Context<Self>) -> GrupoDeOpcoes {
        let Some(perfil) = crate::agente_acp::perfil(agente) else {
            return GrupoDeOpcoes::new(None);
        };
        let id = agente.nome().to_lowercase();
        let instalado = perfil.instalado();
        let programa = if instalado {
            let caminho = crate::ui::dialogos::com_til(&perfil.binario());
            LinhaDeOpcao::new(SharedString::from(format!("cfg-acp-programa-{id}")), "Programa").descricao(caminho).controle(
                botao_fantasma(SharedString::from(format!("cfg-acp-atualizar-{id}")), Some(Lucide::RefreshCw.into()), "Atualizar", TamanhoBotao::Barra, window, cx)
                    .bg(Cores::recuo())
                    .on_click(move |_, _, _| {
                        if let Some(perfil) = crate::agente_acp::perfil(agente) {
                            perfil.atualizar_no_terminal();
                        }
                    }),
            )
        } else {
            LinhaDeOpcao::new(SharedString::from(format!("cfg-acp-programa-{id}")), "Programa").descricao("Não está instalado neste computador").controle(
                botao_fantasma(SharedString::from(format!("cfg-acp-instalar-{id}")), Some(Lucide::Download.into()), "Instalar…", TamanhoBotao::Barra, window, cx)
                    .bg(Cores::recuo())
                    .on_click(move |_, _, _| super::uso_plano::instalar(agente)),
            )
        };
        let entrar = format!("Entrar ({})", perfil.login_no_tui.map_or_else(|| perfil.login.to_string(), |dentro| format!("{dentro} no {}", agente.nome())));
        let conta = LinhaDeOpcao::new(SharedString::from(format!("cfg-acp-conta-{id}")), "Conta")
            .descricao(entrar)
            .when(instalado, |l| {
                l.controle(
                    botao_fantasma(SharedString::from(format!("cfg-acp-entrar-{id}")), Some(Lucide::LogIn.into()), "Entrar…", TamanhoBotao::Barra, window, cx)
                        .bg(Cores::recuo())
                        .on_click(move |_, _, _| {
                            if let Some(perfil) = crate::agente_acp::perfil(agente) {
                                perfil.entrar();
                            }
                        }),
                )
            });
        GrupoDeOpcoes::new(Some(agente.programa().into()))
            .linha(programa)
            .linha(conta)
            .linha(
                LinhaDeOpcao::new(SharedString::from(format!("cfg-acp-sobre-{id}")), "Sobre")
                    .descricao(perfil.descricao)
                    .info("O Vitral conversa com ele pelo ACP (Agent Client Protocol): o modelo e o esforço vêm da própria sessão dele, e os pedidos de permissão aparecem na conversa. O uso do plano ainda não aparece."),
            )
    }

    /// A lista das IAs, no alto de Agentes: cada uma com a marca, a versão instalada (ou "Não
    /// instalada"), o Instalar quando falta e a seta que abre a parte dela (a conta, o Atualizar,
    /// os servidores MCP). As próximas IAs entram aqui como mais uma linha.
    fn grupo_das_ias(&self, estreito: bool, window: &Window, cx: &Context<Self>) -> GrupoDeOpcoes {
        let motor = self.motor.read(cx);
        let mut grupo = GrupoDeOpcoes::new(Some("IAs".into()));
        for agente in Agente::TODOS {
            let (instalada, texto) = match agente {
                Agente::Claude => {
                    let instalada = motor.versao_claude.is_some() || crate::claude::binario_claude().is_file();
                    let texto = match motor.versao_claude {
                        Some((a, b, c)) => format!("Versão {a}.{b}.{c}"),
                        None if instalada => "Instalado".to_string(),
                        None => "Não instalado".to_string(),
                    };
                    (instalada, texto)
                }
                Agente::Codex | Agente::Gemini => {
                    let instalada = agente.instalado();
                    let texto = match motor.cli(agente).versao {
                        Some(Some((a, b, c))) => format!("Versão {a}.{b}.{c}"),
                        _ if instalada => "Instalado".to_string(),
                        _ => "Não instalado".to_string(),
                    };
                    (instalada, texto)
                }
                // Os ACP novos: instalado ou não (a versão o Vitral não lê deles).
                outro => {
                    let instalada = outro.instalado();
                    (instalada, if instalada { "Instalado".to_string() } else { "Não instalado".to_string() })
                }
            };
            let id = agente.nome().to_lowercase();
            let instalar = (!instalada).then(|| {
                botao_fantasma(SharedString::from(format!("cfg-ia-instalar-{id}")), Some(Lucide::Download.into()), "Instalar…", TamanhoBotao::Barra, window, cx)
                    .bg(Cores::recuo())
                    .on_click(move |_, _, _| super::uso_plano::instalar(agente))
            });
            let abrir = botao_icone(
                SharedString::from(format!("cfg-ia-abrir-{id}")),
                Lucide::ChevronRight,
                format!("Abrir o {}: conta, versão e servidores MCP", agente.nome()),
                TamanhoBotao::Barra,
                window,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.ir_para(Parte::da_ia(agente), cx)));
            let controle = h_flex().gap(esp::E4).children(instalar).child(abrir).into_any_element();
            let linha = LinhaDeOpcao::new(SharedString::from(format!("cfg-ia-{id}")), Parte::da_ia(agente).nome())
                .marca(Marca::do_agente(agente))
                .descricao(div().text_color(if instalada { Cores::tenue() } else { Cores::apagado() }).child(texto));
            grupo = grupo.linha(if estreito { linha.campo(controle) } else { linha.controle(controle) });
        }
        grupo
    }

    fn ir_para(&mut self, parte: Parte, cx: &mut Context<Self>) {
        if parte != self.parte {
            // A parte de uma IA fica logo depois de Agentes: entrar nela vem da direita, voltar,
            // da esquerda.
            let posicao = |p: Parte| {
                let na_coluna = Parte::TODAS.iter().position(|q| *q == p.na_coluna()).unwrap_or(0) * 2;
                if p.na_coluna() == p { na_coluna } else { na_coluna + 1 }
            };
            self.entra_de = Some(movimento::lado_da_navegacao(posicao(self.parte), posicao(parte)));
        }
        self.parte = parte;
        // As partes dos agentes dizem a versão instalada e a nova, quando há.
        if matches!(parte, Parte::Agentes | Parte::ClaudeCode | Parte::Codex | Parte::Gemini) {
            self.ler_os_instalados(false, cx);
        }
        match parte {
            // A lista das IAs mostra a versão de cada uma.
            Parte::Agentes => self.motor.update(cx, |m, cx| {
                m.reler_versao_claude(cx);
                m.reler_cli(Agente::Codex, cx);
                m.reler_cli(Agente::Gemini, cx);
            }),
            Parte::Codex => self.motor.update(cx, |m, cx| m.reler_cli(Agente::Codex, cx)),
            Parte::Gemini => self.motor.update(cx, |m, cx| {
                m.reler_cli(Agente::Gemini, cx);
                m.reler_uso_gemini(cx);
            }),
            _ => {}
        }
        cx.notify();
    }

    /// As versões mais novas publicadas, em segundo plano, quando uma parte que as mostra abre: a
    /// consulta vai à rede (guardada por uma hora). Com "Só pelo botão", abrir a parte lê só a
    /// última consulta guardada; a rede só é consultada com `pelo_botao` (`opcoes::consulta_a_rede`).
    fn ler_os_instalados(&mut self, pelo_botao: bool, cx: &mut Context<Self>) {
        let consultar = crate::opcoes::consulta_a_rede(self.motor.read(cx).config.consultar_versoes, pelo_botao);
        cx.spawn(async move |vista, cx| {
            let publicadas = cx
                .background_spawn(async move {
                    if consultar { versoes_agentes::consultar() } else { versoes_agentes::guardadas().unwrap_or_default() }
                })
                .await;
            let _ = vista.update(cx, |vista, cx| {
                vista.publicadas = publicadas;
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
        let botao = botao_cfg("cfg-claude-atualizar", "Atualizar", window, cx)
        // Na caixa escura dos outros controles: solto, parecia texto (revisão de design, 23/11/2026).
        .when_some(linha.espera.clone(), |b, motivo| b.disabled(true).opacity(0.45).tooltip(motivo))
        .on_click(move |_, _, cx| motor_e.update(cx, |m, cx| m.atualizar_claude(cx)));
        let opcao = LinhaDeOpcao::new("cfg-claude", "Versão")
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
            ("Rever ao terminar".to_string(), ligado(c.memoria_ao_terminar)),
            ("Revisão automática pelo outro agente".to_string(), ligado(c.revisar_edicoes)),
            ("Sugerir a próxima mensagem".to_string(), ligado(c.sugerir_resposta)),
            ("Chat chamado abre numa aba".to_string(), ligado(c.chat_chamado_abre_aba)),
            ("Fechar a aba para o chat".to_string(), ligado(c.fechar_aba_para_o_chat)),
            ("Seta ↑".to_string(), if c.seta_acima_percorre { "percorre as enviadas" } else { "corrige a última" }.to_string()),
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
            codex: motor.cli_codex.versao.flatten().map(motor::texto_da_versao).or_else(|| {
                crate::agente_codex::instalado().then(|| "instalado".to_string())
            }),
            gemini: motor.cli_gemini.versao.flatten().map(motor::texto_da_versao).or_else(|| {
                crate::agente_gemini::instalado().then(|| "instalado".to_string())
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
            .tamanho(TamanhoChip::Caixa)
            .mudado(atual != padrao)
            .seta(true)
            .aberto(aberto);
        match simbolo.and_then(Simbolo::copia) {
            Some(s) => chip.simbolo(s),
            None => chip,
        }
        .construir(window, cx)
        // No preto, como os outros controles das Configurações (protótipo Linear, 25/11/2026).
        .bg(Cores::preto())
        .min_w(r(170.))
        .justify_between()
        .text_size(r(12.))
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

/// O botão de uma linha das Configurações no jeito do protótipo Linear (25/11/2026): 26 de altura,
/// no preto, só o texto em 11,5.
fn botao_cfg(id: &'static str, rotulo: &'static str, window: &Window, cx: &App) -> gpui_kit::component::button::Button {
    botao_fantasma(id, None, rotulo, TamanhoBotao::Caixa, window, cx)
        .h(r(26.))
        .px(linear::SECAO_LADO)
        .rounded(raio::de(26.))
        .bg(Cores::preto())
        .text_size(r(11.5))
        .text_color(Cores::texto_da_fala())
}

/// A cor de um seletor, como a pílula do protótipo: a própria cor de fundo, o conta-gotas e o hex
/// em mono (o texto escuro sobre uma cor clara). O clique abre o seletor do kit.
fn pilula_de_cor(estado: &Entity<ColorPickerState>, ancora: Anchor, window: &Window) -> AnyElement {
    let icone = tam(window, 16.);
    ColorPicker::new(estado)
        .anchor(ancora)
        .w(r(220.))
        .gatilho(move |cor, _| {
            let cor = cor.unwrap_or_else(Cores::preto);
            let c = Rgba::from(cor);
            let luz = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
            let texto: Hsla = if luz > 0.6 { rgb(0x0d0f13).into() } else { Cores::forte() };
            let hex = cor_de(cor).hex().to_uppercase();
            h_flex()
                .h(r(26.))
                .px(esp::E12)
                .gap(esp::E8)
                .rounded(raio::de(26.))
                .bg(cor)
                .cursor_pointer()
                .text_color(texto)
                .font_family(crate::tema::FONTE_MONO)
                .text_size(r(12.))
                .child(Icon::new(Lucide::Pipette).with_size(icone).text_color(texto))
                .child(div().flex_1().child(hex))
                .into_any_element()
        })
        .into_any_element()
}

/// O contraste: a faixa no azul, com o trilho claro do range do protótipo, e o número.
fn faixa_de_contraste(estado: &Entity<SliderState>, valor: u8) -> AnyElement {
    h_flex()
        .w(r(220.))
        .gap(esp::E12)
        .items_center()
        .child(Slider::new(estado).flex_1().bg(Cores::destaque()).text_color(Cores::destaque()).trilho(rgb(0xededed).into()))
        .child(div().flex_none().w(r(26.)).text_right().text_size(r(12.5)).text_color(Cores::texto()).child(valor.to_string()))
        .into_any_element()
}

/// O botão só de texto do protótipo (Importar, Copiar): sem fundo, 28 de altura, texto em 13.
fn botao_de_texto(id: &'static str, rotulo: &'static str, window: &Window, cx: &App) -> gpui_kit::component::button::Button {
    botao_fantasma(id, None, rotulo, TamanhoBotao::Caixa, window, cx)
        .h(r(28.))
        .pl(esp::E8)
        .pr(linear::BOTAO_TEXTO_DIREITA)
        .rounded(raio::de(28.))
        .text_size(r(13.))
        .text_color(Cores::texto_da_fala())
}

/// As chips do foco do revisor: ligadas no azul do escolhido, as outras no preto.
const FOCOS_DA_REVISAO: [&str; 5] = ["Bugs", "Segurança", "Testes", "Desempenho", "Estilo"];

/// Os atalhos do teclado, por parte do app, com as teclas em texto (a mesma tabela do keymap).
fn grupos_dos_atalhos() -> Vec<AnyElement> {
    super::atalhos::Grupo::TODOS
        .into_iter()
        .map(|grupo| {
            let linhas = super::atalhos::do_grupo(grupo).into_iter().enumerate().map(|(i, (teclas, rotulo))| {
                LinhaDeOpcao::new(SharedString::from(format!("cfg-atalho-{}-{i}", grupo.titulo())), rotulo).controle(
                    div()
                        .flex_none()
                        .h(r(26.))
                        .px(linear::SECAO_LADO)
                        .flex()
                        .items_center()
                        .rounded(raio::de(26.))
                        .bg(Cores::preto())
                        .text_size(r(11.5))
                        .text_color(Cores::texto_da_fala())
                        .child(teclas.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(" ")),
                )
            });
            linhas.fold(GrupoDeOpcoes::new(Some(grupo.titulo().into())), |g, l| g.linha(l)).into_any_element()
        })
        .collect()
}

impl Configuracoes {
    /// Revisão (protótipo Linear, 25/11/2026): a automática (ligada, quando, o mínimo de linhas e
    /// o Planejar), o revisor (o foco e as suas instruções) e o resultado.
    fn grupos_da_revisao(&self, config: &Config, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        let m = || self.motor.clone();
        let mudar = |f: fn(&mut Config, bool)| {
            let motor = m();
            move |ligado: bool, _: &mut Window, cx: &mut App| motor.update(cx, |m, cx| m.mudar_config(|c| f(c, ligado), cx))
        };
        let quando = match config.revisao_quando.as_str() {
            "todo" => 1,
            "botao" => 2,
            _ => 0,
        };
        let mudar_quando = m();
        let automatica = GrupoDeOpcoes::new(Some("Revisão automática".into()))
            .linha(
                LinhaDeOpcao::new("cfg-revisar-edicoes", "Ligada")
                    .info(
                        "O revisor (de fábrica, o outro agente: o Codex num chat do Claude, o Claude nos outros; \
                         troque em Revisor) confere o que mudou, só lendo. Sem nada a dizer, fica uma linha discreta; com um achado, um cartão que você manda ao \
                         agente do chat. Desligada, a revisão continua no “Revisar com…” da linha “Trabalhou…”. \
                         Gasta o plano do outro agente a cada turno revisto.",
                    )
                    .interruptor(config.revisar_edicoes, mudar(|c, v| c.revisar_edicoes = v)),
            )
            .linha(
                LinhaDeOpcao::new("cfg-revisao-quando", "Quando")
                    .info("“Com edições”: o turno que mudou o mínimo de linhas. “Todo turno”: qualquer turno que terminou bem. “Só no botão”: só pelo “Revisar com…”.")
                    .controle(
                        ControleSegmentado::new("cfg-revisao-quando", ["Com edições", "Todo turno", "Só no botão"], quando, move |novo, _, cx| {
                            let valor = ["edicoes", "todo", "botao"][novo];
                            mudar_quando.update(cx, |m, cx| m.mudar_config(|c| c.revisao_quando = valor.to_string(), cx))
                        })
                        .with_size(Size::Small)
                        .chapado(true),
                    ),
            )
            .linha(LinhaDeOpcao::new("cfg-revisao-minimo", "Mínimo de linhas").controle(contador(
                "cfg-revisao-minimo",
                &[5, 10, 20, 40, 80, 150],
                config.revisao_minimo,
                m(),
                |c, v| c.revisao_minimo = v,
            )))
            .linha(
                LinhaDeOpcao::new("cfg-revisao-planejar", "Pular em Planejar")
                    .interruptor(config.revisao_pula_planejar, mudar(|c, v| c.revisao_pula_planejar = v)),
            );
        // O foco: cada chip liga e desliga um assunto no pedido do revisor.
        let chips = FOCOS_DA_REVISAO.iter().map(|foco| {
            let ligado = config.revisao_foco.iter().any(|f| f == foco);
            let motor = m();
            div()
                .id(SharedString::from(format!("cfg-foco-{foco}")))
                .flex()
                .flex_none()
                .items_center()
                .h(r(24.))
                .px(linear::ITEM_LADO)
                .rounded(raio::de(24.))
                .text_size(r(11.5))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .text_color(if ligado { Cores::forte() } else { Cores::texto_da_fala() })
                .bg(if ligado { Cores::selecao() } else { Cores::preto() })
                .child(*foco)
                .on_click(move |_, _, cx| {
                    motor.update(cx, |m, cx| {
                        m.mudar_config(
                            |c| {
                                if ligado {
                                    c.revisao_foco.retain(|f| f != foco);
                                } else {
                                    c.revisao_foco.push(foco.to_string());
                                }
                            },
                            cx,
                        )
                    })
                })
        });
        let editar_instrucoes = {
            let motor = m();
            let vazio = config.revisao_instrucoes.trim().is_empty();
            botao_cfg("cfg-revisao-instrucoes", if vazio { "Escrever…" } else { "Editar…" }, window, cx)
                .on_click(move |_, window, cx| instrucoes::abrir_da_revisao(motor.clone(), window, cx))
        };
        // Quem revisa e com que modelo (dono, 26/11/2026: "faz dar pra escolher que IA uso na
        // revisão e qual modelo"). Num chat do escolhido, quem revisa é o outro agente.
        let quem = {
            let motor = m();
            let mut opcoes = vec![Opcao::new("", "O outro agente").descricao("O Codex num chat do Claude, o Claude nos outros")];
            opcoes.extend(Agente::TODOS.into_iter().map(|a| {
                let descricao = if a.instalado() { format!("Num chat do {}, revisa o {}", a.nome(), a.outro().nome()) } else { "Não instalado".to_string() };
                Opcao::new(a.nome().to_lowercase(), a.nome()).descricao(descricao)
            }));
            menu_escolha(
                "cfg-revisao-agente",
                Anchor::TopRight,
                opcoes,
                &config.revisao_agente,
                "",
                move |valor, _, cx| motor.update(cx, |m, cx| m.mudar_config(|c| c.revisao_agente = valor, cx)),
                window,
                cx,
            )
        };
        // Um modelo por revisor que pode entrar: o escolhido e o outro (que revisa os chats dele),
        // ou, sem escolha, o Claude e o Codex.
        let fixo = Agente::TODOS.into_iter().find(|a| a.nome().eq_ignore_ascii_case(config.revisao_agente.trim()));
        let mut revisores: Vec<Agente> = match fixo {
            Some(a) => vec![a, a.outro()],
            None => vec![Agente::Claude, Agente::Codex],
        };
        revisores.dedup();
        let linhas_de_modelo: Vec<LinhaDeOpcao> = revisores
            .into_iter()
            .map(|agente| {
                let chave = agente.nome().to_lowercase();
                let mut opcoes = vec![Opcao::new("", "Padrão").descricao("O modelo padrão do agente")];
                match agente {
                    Agente::Claude => opcoes.extend(
                        MODELOS[1..].iter().zip(DESCRICOES_DE_MODELO).map(|((valor, nome), descricao)| Opcao::new(*valor, *nome).descricao(descricao)),
                    ),
                    Agente::Codex => opcoes.extend(crate::agente_codex::modelos().into_iter().map(|m| Opcao::new(m.slug, m.nome))),
                    outro => opcoes.extend(crate::agente_gemini::modelos_de(outro).into_iter().map(|m| Opcao::new(m.valor, m.nome))),
                }
                let atual = config.revisao_modelos.get(&chave).cloned().unwrap_or_default();
                // O modelo escolhido antes, que o catálogo de agora não traz, continua na lista.
                if !atual.is_empty() && !opcoes.iter().any(|o| o.valor == atual) {
                    opcoes.push(Opcao::new(atual.clone(), atual.clone()));
                }
                let motor = m();
                let chave_salva = chave.clone();
                LinhaDeOpcao::new(SharedString::from(format!("cfg-revisao-modelo-{chave}")), format!("Modelo do {}", agente.nome())).controle(menu_escolha(
                    SharedString::from(format!("cfg-revisao-modelo-{chave}")),
                    Anchor::TopRight,
                    opcoes,
                    &atual,
                    "",
                    move |valor, _, cx| {
                        let chave = chave_salva.clone();
                        motor.update(cx, |m, cx| {
                            m.mudar_config(
                                |c| {
                                    if valor.is_empty() {
                                        c.revisao_modelos.remove(&chave);
                                    } else {
                                        c.revisao_modelos.insert(chave, valor);
                                    }
                                },
                                cx,
                            )
                        })
                    },
                    window,
                    cx,
                ))
            })
            .collect();
        let mut revisor = GrupoDeOpcoes::new(Some("Revisor".into())).linha(
            LinhaDeOpcao::new("cfg-revisao-agente", "Quem revisa")
                .info("Num chat que já tem dois agentes, revisa o parceiro dele.")
                .controle(quem),
        );
        for linha in linhas_de_modelo {
            revisor = revisor.linha(linha);
        }
        let revisor = revisor
            .linha(LinhaDeOpcao::new("cfg-revisao-foco", "Foco").controle(h_flex().flex_wrap().justify_end().gap(esp::E4).children(chips)))
            .linha(
                LinhaDeOpcao::new("cfg-revisao-instrucoes", "Instruções")
                    .info("O que o revisor recebe a mais, no fim do pedido de revisão.")
                    .controle(editar_instrucoes),
            );
        let resultado = GrupoDeOpcoes::new(Some("Resultado".into()))
            .linha(
                LinhaDeOpcao::new("cfg-revisao-sozinha", "Mandar ao agente sozinho")
                    .info("O achado vai ao agente do chat na hora, sem o “Mandar ao…” do cartão.")
                    .interruptor(config.revisao_manda_sozinha, mudar(|c, v| c.revisao_manda_sozinha = v)),
            )
            .linha(
                LinhaDeOpcao::new("cfg-revisao-nada", "Mostrar \"nada a acrescentar\"")
                    .interruptor(config.revisao_mostra_nada, mudar(|c, v| c.revisao_mostra_nada = v)),
            );
        vec![automatica.into_any_element(), revisor.into_any_element(), resultado.into_any_element()]
    }

    /// Contas: a padrão dos chats (a automática, a com mais folga, ou uma das contas) e o entrar
    /// com outra.
    /// As contas do Codex (26/11/2026, no jeito do Zerosub): cada uma com o login (ou o Entrar), a
    /// escolha da padrão dos chats novos, o Tirar das extras e o Adicionar conta, que abre o login
    /// da nova no terminal.
    fn grupo_das_contas_codex(&self, window: &Window, cx: &Context<Self>) -> GrupoDeOpcoes {
        let motor = self.motor.read(cx);
        let padrao = if motor.config.conta_codex.is_empty() { crate::contas_codex::PRINCIPAL.to_string() } else { motor.config.conta_codex.clone() };
        let mut grupo = GrupoDeOpcoes::new(Some("Contas do Codex".into()));
        for conta in &motor.contas_codex {
            let (id, logada) = (conta.id.clone(), conta.logada());
            let principal = id == crate::contas_codex::PRINCIPAL;
            let mut botoes: Vec<AnyElement> = Vec::new();
            if padrao != id {
                let (motor_e, valor) = (self.motor.clone(), if principal { String::new() } else { id.clone() });
                botoes.push(
                    botao_fantasma(SharedString::from(format!("cfg-codex-padrao-{id}")), None, "Usar nos novos", TamanhoBotao::Barra, window, cx)
                        .bg(Cores::recuo())
                        .on_click(move |_, _, cx| {
                            let valor = valor.clone();
                            motor_e.update(cx, |m, cx| m.mudar_config(|c| c.conta_codex = valor, cx))
                        })
                        .into_any_element(),
                );
            }
            if !logada || !principal {
                let (motor_e, conta_id) = (self.motor.clone(), id.clone());
                botoes.push(
                    botao_fantasma(SharedString::from(format!("cfg-codex-entrar-{id}")), Some(Lucide::LogIn.into()), if logada { "Entrar de novo" } else { "Entrar…" }, TamanhoBotao::Barra, window, cx)
                        .bg(Cores::recuo())
                        .on_click(move |_, _, cx| motor_e.read(cx).entrar_conta_codex(&conta_id))
                        .into_any_element(),
                );
            }
            if !principal {
                let (motor_e, conta_id) = (self.motor.clone(), id.clone());
                botoes.push(
                    botao_icone(SharedString::from(format!("cfg-codex-tirar-{id}")), Lucide::Trash, "Tirar esta conta (o login dela some)", TamanhoBotao::Barra, window, cx)
                        .on_click(move |_, _, cx| motor_e.update(cx, |m, cx| m.remover_conta_codex(&conta_id, cx)))
                        .into_any_element(),
                );
            }
            let descricao = match (logada, padrao == id) {
                (true, true) => "Com login · padrão dos chats novos",
                (true, false) => "Com login",
                (false, _) => "Sem login",
            };
            grupo = grupo.linha(
                LinhaDeOpcao::new(SharedString::from(format!("cfg-codex-conta-{id}")), conta.nome.clone())
                    .descricao(div().text_color(if logada { Cores::tenue() } else { Cores::ambar() }).child(descricao))
                    .controle(h_flex().gap(esp::E4).children(botoes)),
            );
        }
        let motor_e = self.motor.clone();
        grupo.linha(
            LinhaDeOpcao::new("cfg-codex-adicionar", "Outra conta do Codex")
                .info(
                    "Cada conta tem uma pasta própria, em que só o login é dela: a configuração e as conversas são as da \
                     principal. Trocar a conta de um chat que já conversou abre uma sessão nova, com a conversa até ali \
                     junto da próxima mensagem. Não funciona no Windows por enquanto.",
                )
                .controle(botao_cfg("cfg-codex-adicionar", "Adicionar…", window, cx).on_click(move |_, window, cx| {
                    if let Err(erro) = motor_e.update(cx, |m, cx| m.adicionar_conta_codex(cx)) {
                        window.push_notification(super::componentes::aviso(super::componentes::Tom::Erro, format!("Não deu para criar a conta: {erro}")), cx);
                    }
                })),
        )
    }

    fn grupos_das_contas(&self, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        let motor = self.motor.read(cx);
        let escolha = if motor.config.conta.is_empty() { crate::contas::PRINCIPAL.to_string() } else { motor.config.conta.clone() };
        let radio = |marcado: bool| {
            div()
                .flex_none()
                .size(r(16.))
                .rounded_full()
                .border_2()
                .border_color(if marcado { Cores::destaque() } else { Cores::tenue() })
                .flex()
                .items_center()
                .justify_center()
                .when(marcado, |d| d.child(div().size(r(7.)).rounded_full().bg(Cores::destaque())))
        };
        let mut linhas: Vec<(String, String, Option<String>)> = vec![(crate::contas::AUTOMATICA.into(), "Automática".into(), Some("A com mais folga".into()))];
        for conta in &motor.contas {
            let uso = motor.uso_da_conta(&conta.id);
            let porcento = |f: Option<f64>| f.map_or("–".to_string(), |f| format!("{}%", (f * 100.).round() as u32));
            linhas.push((conta.id.clone(), conta.nome.clone(), Some(format!("5 h {} · semana {}", porcento(uso.cinco_horas), porcento(uso.semana)))));
        }
        let padrao = linhas.into_iter().fold(GrupoDeOpcoes::new(Some("Conta padrão".into())), |g, (id, nome, detalhe)| {
            let marcado = escolha == id;
            let motor_e = self.motor.clone();
            g.linha(
                div()
                    .id(SharedString::from(format!("cfg-conta-{id}")))
                    .cursor_pointer()
                    .child(LinhaDeOpcao::new(SharedString::from(format!("cfg-conta-linha-{id}")), nome).descricao(detalhe.unwrap_or_default()).controle(radio(marcado)))
                    .on_click(move |_, _, cx| super::painel_contas::escolher_conta(&motor_e, &id, cx)),
            )
        });
        let janela = self.janela.clone();
        let outra = GrupoDeOpcoes::new(None).linha(LinhaDeOpcao::new("cfg-outra-conta", "Outra conta").controle(
            botao_cfg("cfg-outra-conta", "Entrar…", window, cx).on_click(move |_, window, cx| {
                window.close_dialog(cx);
                let _ = janela.update(cx, |this, cx| this.abrir_contas(true, window, cx));
            }),
        ));
        // A troca sozinha no limite, também na conta escolhida à mão (26/11/2026).
        let (ligada, motor) = (self.motor.read(cx).config.trocar_conta_no_limite, self.motor.clone());
        let outra = outra.linha(
            LinhaDeOpcao::new("cfg-trocar-no-limite", "Trocar sozinho no limite")
                .info(
                    "Um chat com uma conta escolhida à mão que bate no limite do plano passa para a conta com mais \
                     folga, repete o turno nela e fica nela. Desligado, só a Automática troca; nas outras aparece o \
                     “Continuar na…”.",
                )
                .interruptor(ligada, move |v, _, cx| motor.update(cx, |m, cx| m.mudar_config(|c| c.trocar_conta_no_limite = v, cx))),
        );
        vec![padrao.into_any_element(), outra.into_any_element()]
    }
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
                .chapado(true)
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
        if self.parte == Parte::Aparencia {
            self.preparar_cores(window, cx);
        }
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
        // A pasta em que o "+" cria os chats (era "Escolher a pasta de trabalho…" no menu do logo).
        let pasta_de_trabalho = {
            let janela = self.janela.clone();
            let escolhida = config.pasta_de_trabalho.as_deref();
            let botao = botao_cfg("cfg-pasta-trabalho-trocar", if escolhida.is_some() { "Trocar…" } else { "Escolher…" }, window, cx)
                .on_click(move |_, window, cx| {
                    let _ = janela.update(cx, |this, cx| this.trocar_pasta_de_trabalho(window, cx));
                });
            com(
                LinhaDeOpcao::new("cfg-pasta-trabalho", "Pasta dos chats novos")
                    .info("O + e o Novo chat criam o chat nesta pasta. Os chats que já existem ficam onde estão; um chat em outra pasta sai do “Novo chat em outra pasta…”.")
                    .descricao(escolhida.map_or_else(|| "Nenhuma escolhida".to_string(), super::dialogos::com_til)),
                botao.into_any_element(),
            )
        };
        let conversas = GrupoDeOpcoes::new(Some("Chats".into()))
            .linha(pasta_de_trabalho)
            .linha(com(
                LinhaDeOpcao::new("cfg-arquivar", "Arquivar parados")
                    .info(
                        "Vale para os chats sem novidade há esse tempo, já lidos, sem fila e sem pedido aberto. Os \
                         fixados ficam.",
                    ),
                prazo("cfg-arquivar", &PRAZOS_ARQUIVAR, config.arquivar_apos_dias, estreito, m(), |c, v| {
                    c.arquivar_apos_dias = v
                }, window, cx),
            ))
            .linha(
                LinhaDeOpcao::new("cfg-sugerir", "Sugerir mensagem")
                    .info(
                        "Depois de cada resposta do Claude, o Claude Code adivinha o que você escreveria em seguida \
                         (“roda os testes”, “commita”). A sugestão aparece apagada na caixa vazia, e o Tab a põe na \
                         caixa. Quando o próximo passo não é óbvio, ele não sugere nada. É uma chamada curta, que \
                         reaproveita o cache da conversa.",
                    )
                    .interruptor(sugerir, move |ligado, _, cx| {
                        alternar_sugerir.update(cx, |m, cx| m.mudar_config(|c| c.sugerir_resposta = ligado, cx))
                    }),
            )
            .linha(
                LinhaDeOpcao::new("cfg-titulo-claude", "Título pelo Claude")
                    .info("O Claude Code dá um título à conversa depois da primeira mensagem, e o Vitral o põe no lugar das primeiras palavras. Nunca troca um nome que você deu. Desligado, o título fica o das primeiras palavras. É uma chamada curta ao modelo, uma vez por chat.")
                    .interruptor(titulo_claude, move |ligado, _, cx| {
                        alternar_titulo.update(cx, |m, cx| m.mudar_config(|c| c.titulo_pelo_claude = ligado, cx))
                    }),
            );
        // As abas: o chat que outro chat põe para trabalhar e o fechar (dono, 24/11/2026).
        let (abre_aba, alternar_abre_aba) = (config.chat_chamado_abre_aba, m());
        let (fecha_para, alternar_fecha_para) = (config.fechar_aba_para_o_chat, m());
        let abas_grupo = GrupoDeOpcoes::new(Some("Abas".into()))
            .linha(
                LinhaDeOpcao::new("cfg-chat-chamado", "Chamado abre em aba")
                    .info("Quando um chat manda mensagem a outro, ou abre um, o destino aparece numa aba do painel ativo. A aba entra de fundo: não troca o que você está lendo nem traz a janela para a frente, e o contorno dela mostra que está rodando. Se o chat já está à vista ou já é aba, nada muda.")
                    .interruptor(abre_aba, move |ligado, _, cx| {
                        alternar_abre_aba.update(cx, |m, cx| m.mudar_config(|c| c.chat_chamado_abre_aba = ligado, cx))
                    }),
            )
            .linha(
                LinhaDeOpcao::new("cfg-fechar-aba", "Fechar aba para o chat")
                    .info("Fechar a aba de um chat que está trabalhando para o turno dele, como o botão Parar. Se ele continua à vista em outro painel ou aba, só a aba fecha. Se parar derrubaria agentes em segundo plano, o Vitral pergunta antes. Chat parado ou esperando você só fecha a aba.")
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
                LinhaDeOpcao::new("cfg-seta-acima", "Seta ↑")
                    .info("“Percorrer as enviadas” traz o texto das mensagens que você já enviou neste chat, uma por vez, sem desfazer nada; ↓ volta, e editar o texto devolve as setas ao cursor. O corrigir continua no lápis da mensagem e no Ctrl+↑."),
                ControleSegmentado::new(
                    "cfg-seta-acima",
                    ["Corrigir", "Percorrer"],
                    usize::from(seta_percorre),
                    move |novo, _, cx| mudar_seta.update(cx, |m, cx| m.mudar_config(|c| c.seta_acima_percorre = novo == 1, cx)),
                )
                .with_size(Size::Small)
                .chapado(true)
                .largura_total(estreito)
                .into_any_element(),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-enter-turno", "Enter rodando")
                    .info("“Entra no turno” manda a mensagem ao agente sem pará-lo. “Vai para a fila” espera o turno acabar. O Ctrl+Enter faz o outro. Respondendo a um pedido de permissão ou a uma pergunta, o Enter continua sendo a resposta."),
                ControleSegmentado::new(
                    "cfg-enter-turno",
                    ["No turno", "Na fila"],
                    usize::from(enter_fila),
                    move |novo, _, cx| mudar_enter.update(cx, |m, cx| m.mudar_config(|c| c.enter_na_fila = novo == 1, cx)),
                )
                .with_size(Size::Small)
                .chapado(true)
                .largura_total(estreito)
                .into_any_element(),
            ));
        let (lembrar, alternar_lembrar) = (config.lembrar_arquivos, m());
        let arquivos_grupo = GrupoDeOpcoes::new(Some("Arquivos".into())).linha(
            LinhaDeOpcao::new("cfg-lembrar-arquivos", "Lembrar a vista")
                .info("Guarda “Mostrar ignorados” (a aba Arquivos) e “Ignorar espaço em branco” (o “…” do arquivo com o diff) entre uma abertura e outra. Desligado, os dois abrem desligados, como sempre.")
                .interruptor(lembrar, move |ligado, _, cx| {
                    alternar_lembrar.update(cx, |m, cx| m.mudar_config(|c| c.lembrar_arquivos = ligado, cx))
                }),
        );
        let agentes = GrupoDeOpcoes::new(Some("Agentes".into()))
            .linha(com(
                LinhaDeOpcao::new("cfg-modo", "Modo")
                    .info("“Sem perguntar” é o mesmo do seu VS Code. Nos outros, os pedidos de permissão aparecem na conversa."),
                menu_escolha("cfg-permissao", ancora, modos, &config.permissao, &padrao.permissao, gravar(m(), |c, v| c.permissao = v), window, cx),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-agente", "Agente")
                    .info("O Codex é o agente de código da OpenAI, e o Gemini, o do Google (pelo Antigravity CLI). Cada um usa o login e o plano dele, e precisa estar instalado neste computador."),
                menu_escolha(
                    "cfg-agente",
                    ancora,
                    // Os três de antes sempre; os ACP novos só instalados (a lista não fica com seis
                    // linhas de "não achei").
                    Agente::TODOS
                        .into_iter()
                        .filter(|a| !a.acp() || *a == Agente::Gemini || a.instalado())
                        .map(|a| {
                            let descricao = match a {
                                Agente::Claude => "Claude Code".to_string(),
                                Agente::Codex if crate::agente_codex::instalado() => "Codex, da OpenAI".into(),
                                Agente::Codex => "Não achei o Codex neste computador".into(),
                                Agente::Gemini if crate::agente_gemini::instalado() => "Gemini, do Google, pelo Antigravity CLI".into(),
                                Agente::Gemini => "Não achei o Antigravity CLI (agy) neste computador".into(),
                                outro => outro.programa().to_string(),
                            };
                            Opcao::new(a.nome().to_lowercase(), a.nome()).descricao(descricao).simbolo(Simbolo::Marca(Marca::do_agente(a)))
                        })
                        .collect(),
                    &config.agente_padrao.nome().to_lowercase(),
                    "claude",
                    gravar(m(), |c, v| {
                        c.agente_padrao = Agente::TODOS.into_iter().find(|a| a.nome().eq_ignore_ascii_case(&v)).unwrap_or_default();
                    }),
                    window,
                    cx,
                ),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-modelo-trab", "Modelo")
                    .info("Padrão é o modelo que o seu Claude Code escolher. As versões exatas e as contas ficam no seletor de modelo da caixa."),
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
            ;
        let instrucoes_grupo = GrupoDeOpcoes::new(Some("Instruções".into())).linha({
            let abrir = m();
            let vazio = config.instrucoes.trim().is_empty();
            let botao = botao_cfg("cfg-instrucoes-editar", if vazio { "Escrever…" } else { "Editar…" }, window, cx)
                .on_click(move |_, window, cx| instrucoes::abrir(abrir.clone(), window, cx));
            com(
                LinhaDeOpcao::new("cfg-instrucoes", "Para todos os chats")
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
        let versoes_grupo = GrupoDeOpcoes::new(Some("Versões".into()))
            .linha(com(
                LinhaDeOpcao::new("cfg-consultar-versoes", "Buscar versão nova")
                    .info("O Vitral pergunta ao npm qual é a versão mais nova do Claude Code e do Codex e diz “há a X” nas linhas de versão. “Só pelo botão” não consulta nada ao abrir: a consulta sai quando você clica em Consultar agora."),
                ControleSegmentado::new(
                    "cfg-consultar-versoes",
                    ["Sozinho", "No botão"],
                    usize::from(!consulta_sozinha),
                    move |novo, _, cx| mudar_consulta.update(cx, |m, cx| m.mudar_config(|c| c.consultar_versoes = novo == 0, cx)),
                )
                .with_size(Size::Small)
                .chapado(true)
                .largura_total(estreito)
                .into_any_element(),
            ))
            .when(!consulta_sozinha, |g| {
                let botao = botao_cfg("cfg-consultar-agora", "Consultar agora", window, cx)
                    .on_click(cx.listener(|this, _, _, cx| this.ler_os_instalados(true, cx)));
                g.linha(com(
                    LinhaDeOpcao::new("cfg-consultar-agora", "Ver se há versão nova"),
                    botao.into_any_element(),
                ))
            });
        let memoria_grupo = GrupoDeOpcoes::new(Some("Memória da equipe".into()))
            .linha({
                let ligada = !config.memoria_da_equipe.is_empty();
                let abrir = m();
                LinhaDeOpcao::new("cfg-memoria-equipe", "Servidor")
                    .info(
                        "Um servidor ai-memory da empresa. Ligado, os chats do Claude ganham as ferramentas de memória e \
                         passam a consultar e a registrar o que vale para todos. Só o que o agente escreve de caso pensado \
                         sobe; o que você digita e a saída das ferramentas ficam neste computador.",
                    )
                    .descricao(if ligada { format!("Ligada a {}", config.memoria_da_equipe) } else { "Desligada".to_string() })
                    .controle(
                        botao_cfg("cfg-memoria-equipe-abrir", if ligada { "Trocar…" } else { "Ligar…" }, window, cx)
                            .on_click(move |_, window, cx| super::memoria_equipe::abrir(abrir.clone(), window, cx)),
                    )
            })
            .linha(
                LinhaDeOpcao::new("cfg-memoria", "Rever ao terminar")
                    .info(
                        "Quando um turno que mexeu em arquivo termina, o Vitral pede ao agente, uma vez por sessão, \
                         para ver se algo vale guardar na memória ou nas instruções do projeto. Quase sempre a \
                         resposta é que não há nada a registrar.",
                    )
                    .interruptor(memoria, move |ligado, _, cx| {
                        alternar_memoria.update(cx, |m, cx| m.mudar_config(|c| c.memoria_ao_terminar = ligado, cx))
                    }),
            );
        // A versão do Claude Code e os servidores MCP dele num cartão só, como as outras partes.
        let claude_grupo = self.grupo_do_claude(estreito, window, cx)
            .linha(
                LinhaDeOpcao::new("cfg-mcp", "Servidores MCP")
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
            )
            // A lista dos servidores (era "Servidores MCP" no menu do logo).
            .linha({
                let janela = self.janela.clone();
                let botao = botao_cfg("cfg-mcp-ver", "Ver…", window, cx)
                    .on_click(move |_, window, cx| {
                        let _ = janela.update(cx, |this, cx| this.abrir_mcp(window, cx));
                    });
                com(
                    LinhaDeOpcao::new("cfg-mcp-lista", "Lista dos servidores MCP"),
                    botao.into_any_element(),
                )
            });

        let (animacoes, sistema_sem) = movimento::animacoes(cx);
        let segmentos = ["Do sistema", "Não"];
        let atual = usize::from(animacoes == Animacoes::Desligadas);
        let interface = GrupoDeOpcoes::new(Some("Aparência".into()))
            .linha(
                LinhaDeOpcao::new("cfg-uso-no-canto", "Uso do plano")
                    .info("Cada anel é a janela de 5 horas por fora e a da semana por dentro. O clique abre os detalhes e as contas.")
                    // Os anéis voltaram para baixo do × no mesmo dia em que foram ao cabeçalho
                    // (dono, 16/11/2026).
                    .interruptor(uso_no_canto, move |ligado, _, cx| {
                        alternar_uso.update(cx, |m, cx| m.mudar_config(|c| c.uso_no_canto = ligado, cx))
                    }),
            )
            .linha(
                LinhaDeOpcao::new("cfg-anel-claude", "Anel do Claude")
                    .interruptor(config.anel_claude, move |ligado, _, cx| {
                        anel_claude.update(cx, |m, cx| m.mudar_config(|c| c.anel_claude = ligado, cx))
                    }),
            )
            .linha(
                LinhaDeOpcao::new("cfg-anel-codex", "Anel do Codex")
                    .interruptor(config.anel_codex, move |ligado, _, cx| {
                        anel_codex.update(cx, |m, cx| m.mudar_config(|c| c.anel_codex = ligado, cx))
                    }),
            )
            .linha(
                LinhaDeOpcao::new("cfg-anel-gemini", "Anel do Gemini")
                    .interruptor(config.anel_gemini, move |ligado, _, cx| {
                        anel_gemini.update(cx, |m, cx| m.mudar_config(|c| c.anel_gemini = ligado, cx))
                    }),
            )
            .linha(com(
            LinhaDeOpcao::new("cfg-animacoes", "Animações")
                .info(
                    "Desligadas, os popovers, os menus, as mensagens novas, a pílula dos segmentados e os anéis \
                     aparecem direto no lugar, e o spinner fica parado. Vale para o app todo.",
                )
                .when(animacoes == Animacoes::SeguirGnome && sistema_sem, |l| l.descricao(movimento::SISTEMA_SEM_ANIMACOES)),
            ControleSegmentado::new("cfg-animacoes", segmentos, atual, |novo, _, cx| {
                let escolha = if novo == 1 { Animacoes::Desligadas } else { Animacoes::SeguirGnome };
                movimento::escolher_animacoes(escolha, cx);
            })
            .with_size(Size::Small)
                .chapado(true)
            .largura_total(estreito)
            .into_any_element(),
        ));

        let mudar_transparencia = m();
        let interface = interface.linha(com(
            LinhaDeOpcao::new("cfg-transparencia", "Transparência")
                .info(
                    "Quanto o fundo azul da lista e o cinza da conversa deixam ver o que está atrás da janela.                      Com papel de parede claro, os níveis de cima deixam o texto mais fraco difícil de ler.",
                ),
            ControleSegmentado::new(
                "cfg-transparencia",
                crate::tema::NOMES_DA_TRANSPARENCIA,
                usize::from(config.transparencia.min(3)),
                move |novo, _, cx| mudar_transparencia.update(cx, |m, cx| m.mudar_config(|c| c.transparencia = novo as u8, cx)),
            )
            .with_size(Size::Small)
                .chapado(true)
            .largura_total(estreito)
            .into_any_element(),
        ));

        let mudar_numeros = m();
        let interface = interface.linha(com(
            LinhaDeOpcao::new("cfg-numeros", "Números")
                .info("Os números ligam a aba ao chat na lista (e valem para o Alt+1 a 4). “Só segurando Ctrl” esconde os dois e mostra depois de uns 150 ms com o Ctrl apertado; somem ao soltar."),
            ControleSegmentado::new(
                "cfg-numeros",
                ["Sempre", "Com Ctrl"],
                usize::from(config.numeros_so_com_ctrl),
                move |novo, _, cx| mudar_numeros.update(cx, |m, cx| m.mudar_config(|c| c.numeros_so_com_ctrl = novo == 1, cx)),
            )
            .with_size(Size::Small)
                .chapado(true)
            .largura_total(estreito)
            .into_any_element(),
        ));

        let zoom_de_agora = zoom::porcentagem(cx);
        let tamanho = GrupoDeOpcoes::new(Some("Tamanho".into()))
            .linha(com(
                LinhaDeOpcao::new("cfg-zoom", "Zoom")
                    .info("O mesmo zoom do Ctrl+roda, do Ctrl+= e do Ctrl+-: a lista, a caixa de mensagem, os menus e as conversas."),
                Contador::new("cfg-zoom", zoom_de_agora, zoom::FAIXA, move |novo, _, cx| zoom::mudar(if novo > zoom_de_agora { 1 } else { -1 }, cx))
                    .sufixo(SUFIXO_DE_PORCENTO)
                    .map(solto),
            ))
            .linha(com(
                LinhaDeOpcao::new("cfg-letra", "Letra")
                    .info("Muda só as mensagens: o texto, o código e os cartões delas. A lista, a caixa de mensagem e os menus seguem o zoom."),
                contador("cfg-letra", &crate::tema::LETRAS_DA_CONVERSA, config.letra_da_conversa, m(), |c, v| c.letra_da_conversa = v)
                    .sufixo(SUFIXO_DE_PORCENTO)
                    .map(solto),
            ))
            .linha({
                let alternar = m();
                LinhaDeOpcao::new("cfg-letra-caixa", "Mesma letra na caixa")
                    .info("A caixa de mensagem passa a escrever no mesmo tamanho das mensagens. Desligado, ela segue o zoom da interface.")
                    .interruptor(config.letra_na_caixa, move |ligado, _, cx| {
                        alternar.update(cx, |m, cx| m.mudar_config(|c| c.letra_na_caixa = ligado, cx))
                    })
            });

        // O diagnóstico para pedir ajuda, na parte das Atualizações (é do Vitral, não dos chats).
        // A pasta de dados (era "Abrir a pasta de dados" no menu do logo).
        let pasta_de_dados = {
            let pasta = self.motor.read(cx).dados.pasta.clone();
            let descricao = super::dialogos::com_til(&pasta);
            let botao = botao_cfg("cfg-pasta-dados-abrir", "Abrir", window, cx)
                .on_click(move |_, _, cx| crate::sistema::abrir_pasta(&pasta, cx));
            com(
                LinhaDeOpcao::new("cfg-pasta-dados", "Pasta de dados").descricao(descricao),
                botao.into_any_element(),
            )
        };
        let ajuda = GrupoDeOpcoes::new(Some("Ajuda".into())).linha(pasta_de_dados).linha({
            let botao = botao_cfg("cfg-diagnostico-copiar", "Copiar", window, cx)
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(this.diagnostico(cx)));
                    this.diagnostico_copiado = true;
                    cx.notify();
                }));
            com(
                LinhaDeOpcao::new("cfg-diagnostico", "Diagnóstico")
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

        // Notificações (protótipo Linear, 25/11/2026): o aviso dentro do app e o do sistema.
        let notificacoes_grupo = GrupoDeOpcoes::new(Some("Como".into()))
            .linha(
                // Rodada de uso, item 3.1 (dono, 16/11/2026).
                LinhaDeOpcao::new("cfg-avisos-no-app", "No app")
                    .info("Só para chat fora de vista e só para o que trava trabalho: pedido de permissão, plano para aprovar e falha. O aviso fica até ser respondido e nunca pega o foco da caixa.")
                    .interruptor(avisos_no_app, move |ligado, _, cx| {
                        alternar_avisos.update(cx, |m, cx| m.mudar_config(|c| c.avisos_no_app = ligado, cx))
                    }),
            )
            .linha({
                let botao = botao_cfg("cfg-notificacao-testar", "Testar", window, cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.teste_da_notificacao = Some(this.motor.read(cx).testar_notificacao());
                        cx.notify();
                    }));
                let linha = LinhaDeOpcao::new("cfg-notificacao", "No sistema")
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
            });
        let revisao_grupos = self.grupos_da_revisao(&config, window, cx);
        let contas_grupos = self.grupos_das_contas(window, cx);
        let atalhos_grupos = grupos_dos_atalhos();
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
                self.grupo_das_ias(estreito, window, cx).into_any_element(),
                agentes.into_any_element(),
                instrucoes_grupo.into_any_element(),
                versoes_grupo.into_any_element(),
            ],
            Parte::Revisao => revisao_grupos,
            Parte::Notificacoes => vec![notificacoes_grupo.into_any_element()],
            Parte::Memoria => vec![memoria_grupo.into_any_element()],
            Parte::Aparencia => std::iter::once(tamanho.into_any_element())
                .chain(self.grupos_das_cores(&config.paleta, estreito, window, cx))
                .chain([interface.into_any_element()])
                .collect(),
            Parte::Atalhos => atalhos_grupos,
            Parte::ClaudeCode => std::iter::once(claude_grupo.into_any_element()).chain(contas_grupos).collect(),
            Parte::Codex => vec![
                self.grupo_do_agente(Agente::Codex, estreito, window, cx).into_any_element(),
                self.grupo_das_contas_codex(window, cx).into_any_element(),
            ],
            Parte::Gemini => vec![self.grupo_do_agente(Agente::Gemini, estreito, window, cx).into_any_element()],
            Parte::Atualizacoes => {
                atualizacoes.into_iter().map(IntoElement::into_any_element).chain([ajuda.into_any_element()]).collect()
            }
            Parte::OutraIa(agente) => vec![self.grupo_da_ia_acp(agente, window, cx).into_any_element()],
        };
        // A coluna das partes (protótipo Linear, 25/11/2026): o título "Configurações" no alto e as
        // partes de 26, com o ícone de 14; a escolhida no azul do escolhido. No diálogo estreito, só
        // o ícone, com o nome na dica.
        let atual = self.parte;
        let entra_de = self.entra_de.take();
        let partes = Parte::TODAS.into_iter().map(|parte| {
            let escolhida = parte == atual.na_coluna();
            div()
                .id(SharedString::from(format!("cfg-parte-{}", parte.nome())))
                .flex()
                .flex_none()
                .items_center()
                .gap(linear::PASSO_VAO)
                .h(r(26.))
                .px(esp::E8)
                .rounded(raio::CONTROLE)
                .cursor_pointer()
                .text_size(r(12.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if escolhida { Cores::texto() } else { Cores::texto_da_lista() })
                .when(escolhida, |s| s.bg(Cores::selecao()))
                .when(!escolhida, |s| s.hover(|s| s.bg(Cores::hover()).text_color(Cores::texto())))
                .child(div().flex_none().text_color(if escolhida { Cores::texto() } else { Cores::tenue_da_lista() }).child(parte.simbolo().desenhar(14., window)))
                .when(!estreito, |s| s.child(parte.nome()))
                .when(estreito, |s| s.dica_no_ponteiro(move |window, cx| Tooltip::new(parte.nome()).build(window, cx)))
                .on_click(cx.listener(move |this, _, _, cx| this.ir_para(parte, cx)))
        });
        let coluna = v_flex()
            .flex_none()
            .w(if estreito { r(40.) } else { r(168.) })
            .h_full()
            .py(esp::E8)
            .px(linear::CAIXA_BAIXO)
            .when(!estreito, |s| {
                s.child(
                    div()
                        .flex_none()
                        .h(r(26.))
                        .px(esp::E8)
                        .flex()
                        .items_center()
                        .text_size(r(13.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(Cores::texto())
                        .child("Configurações"),
                )
            })
            .children(partes);
        let fechar = botao_icone("cfg-fechar", Lucide::X, "Fechar (Esc)", TamanhoBotao::Caixa, window, cx).on_click(|_, window, cx| window.close_dialog(cx));
        // Na parte de uma IA, a seta volta à lista das IAs, em Agentes.
        let voltar = (atual.na_coluna() != atual).then(|| {
            botao_icone("cfg-voltar", Lucide::ArrowLeft, "Voltar a Agentes", TamanhoBotao::Caixa, window, cx)
                .on_click(cx.listener(|this, _, _, cx| this.ir_para(Parte::Agentes, cx)))
        });
        let cabeca = h_flex()
            .flex_none()
            .h(r(26.))
            .pl(esp::E4)
            .gap(esp::E4)
            .children(voltar)
            .child(div().flex_1().text_size(r(14.)).font_weight(FontWeight::BOLD).text_color(Cores::texto()).child(atual.nome()))
            .child(fechar);
        div()
            .flex()
            .h(px(altura_do_corpo(
                f32::from(window.viewport_size().height),
                f32::from(tam(window, ALTURA_DAS_CONFIGURACOES)),
                f32::from(tam(window, FOLGA_DO_DIALOGO)),
                f32::from(tam(window, MENOR_CORPO)),
            )))
            .child(coluna)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pt(esp::E8)
                    .pl(linear::SECAO_LADO)
                    .pr(linear::SECAO_LADO)
                    .child(cabeca)
                    .child(
                        v_flex()
                            .id("cfg-parte")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .pt(linear::CONTEXTO_VAO)
                            .pb(esp::E12)
                            // A parte escolhida entra com fade e desliza do lado para onde se foi (de
                            // baixo descendo na coluna, de cima subindo); abrir as Configurações não
                            // anima aqui (o diálogo já entra).
                            .child(movimento::entrada(
                                SharedString::from(format!("cfg-mostra-{}", atual.nome())),
                                entra_de.is_some(),
                                entra_de.unwrap_or(movimento::De::Parado),
                                v_flex().w_full().gap(esp::E8).children(grupos),
                            )),
                    ),
            )
    }
}

#[cfg(test)]
mod testes {
    use super::{Atualizacao, Parte, altura_do_corpo, linha_do_claude, vizinhos};

    #[test]
    fn o_corpo_das_configuracoes_nunca_passa_da_janela() {
        // Janela alta: a altura de sempre.
        assert_eq!(altura_do_corpo(1200., 520., 150., 240.), 520.);
        // 150%: o corpo de 780 não cabe numa janela de 920 com a folga de 225.
        assert_eq!(altura_do_corpo(920., 780., 225., 360.), 695.);
        // Janela minúscula: o piso.
        assert_eq!(altura_do_corpo(300., 780., 225., 360.), 360.);
    }

    /// As partes da 0.9.6 continuam as mesmas, na mesma ordem, os Perfis entraram depois dos
    /// Agentes e o Codex e o Gemini, depois do Claude Code; cada nome é um só, e cabe na coluna.
    #[test]
    fn as_partes_da_coluna_sao_as_do_prototipo() {
        // Protótipo Linear (25/11/2026): entram Revisão, Notificações e Atalhos; os Perfis saem. As
        // Contas ficam dentro do Claude Code (são só do Claude).
        let nomes: Vec<&str> = Parte::TODAS.into_iter().map(Parte::nome).collect();
        // As IAs (Claude Code, Codex, Gemini) moram dentro de Agentes desde 26/11/2026.
        assert_eq!(nomes, ["Chats", "Agentes", "Revisão", "Notificações", "Memória", "Aparência", "Atalhos", "Atualizações"]);
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
