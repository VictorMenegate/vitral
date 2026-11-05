//! Revisão das mudanças de uma tarefa: arquivos à esquerda (por pasta) e o diff do GPUI Kit à
//! direita (cor de sintaxe, unificado ou lado a lado, o que não mudou dobrado), comentário em
//! qualquer linha e, no fim, pedir ajuste ao trabalhador ou aprovar e juntar. A verificação e o
//! parecer do revisor ficam em chips na barra; os problemas que o revisor achou aparecem na própria
//! linha, como sugestões de comentário. Fechar o diálogo não perde nada: a revisão fica guardada
//! por tarefa em `Janela.revisoes`.

use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

use gpui_kit::base::actions::Cancel;
use gpui_kit::component::{
    Disableable as _, Selectable as _, Sizable as _, Size, WindowExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex, v_flex,
    diff::{
        Diff, DiffAnnotation, DiffChangeIndicator, DiffFile, DiffFileStatus, DiffHoverHighlight, DiffHunkSeparator,
        DiffLinePosition, DiffLineRange, DiffMode, DiffSide, DiffState,
    },
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    kbd::Kbd,
    menu::DropdownMenu as _,
    spinner::Spinner,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        Alerta, Anel, CabecalhoDeSecao, CabecalhoDialogo, CabecalhoPopover, CaixaDeCodigo, CartaoDeLista, Chip,
        Confirmacao, ControleSegmentado, CorpoPopover, EstadoVazio, EventoEscolha, GrupoDeEscolha,
        ItemDeEscolha, LarguraPopover, LinhaDeOpcao, ListaDeEscolha, Segmento, Selo, Simbolo, TamanhoBotao,
        TamanhoChip, Tom, TomConfirmacao, aviso, aviso_erro, botao_fantasma, botao_icone, botao_primario,
        campo_de_busca, confirmar, contagem, desenho_teclas, icone, pilula_da_tarefa, popover,
        quadrado_do_projeto, selo_gravidade, variante_perigo_calma,
    },
    menus,
};
use crate::{
    git,
    modelo::{EstadoTarefa, Forma, ModoEntrega, Problema, Revisao, Tarefa, Verificacao},
    motor::{FonteDiff, LeituraDiff, Motor, PedidoPr},
    tema::{Cores, Lucide, Tipo as _, dentro_da_borda, esp, r, raio, sombra_elevado, tam},
};

/// Linhas de um comentário: do começo (lado e número) ao fim, que pode ser do outro lado quando o
/// trecho escolhido vai de uma linha removida a uma nova.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Linhas {
    lado: DiffSide,
    inicio: usize,
    lado_fim: DiffSide,
    fim: usize,
}

impl Linhas {
    fn uma(lado: DiffSide, linha: usize) -> Self {
        Self {
            lado,
            inicio: linha,
            lado_fim: lado,
            fim: linha,
        }
    }

    fn de_faixa(faixa: &DiffLineRange) -> Self {
        let (inicio, fim) = if faixa.side() == faixa.end_side() {
            (faixa.start().min(faixa.end()), faixa.start().max(faixa.end()))
        } else {
            (faixa.start(), faixa.end())
        };
        Self {
            lado: faixa.side(),
            inicio,
            lado_fim: faixa.end_side(),
            fim,
        }
    }

    /// Linha embaixo da qual o comentário aparece: a última; num trecho que mistura os dois lados,
    /// a nova (no unificado ela vem depois da removida).
    fn ancora(&self) -> (DiffSide, usize) {
        if self.lado == self.lado_fim || self.lado_fim == DiffSide::Modified {
            (self.lado_fim, self.fim)
        } else {
            (self.lado, self.inicio)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Comentario {
    arquivo: String,
    /// `None`: sobre o arquivo todo.
    linhas: Option<Linhas>,
    trecho: String,
    texto: String,
    /// O problema do revisor de onde ele veio (botão "Usar").
    problema: Option<usize>,
}

/// Comentário sendo escrito: onde fica e, se for a edição de um que já existe, qual.
struct Rascunho {
    arquivo: String,
    linhas: Option<Linhas>,
    editando: Option<usize>,
}

/// O que cada anotação do diff mostra (um retrato do estado, montado a cada quadro).
#[derive(Clone)]
enum Cartao {
    /// `so_ler`: o diff do PR no GitHub, onde nada vira comentário (sem o "Usar").
    Problema { indice: usize, problema: Problema, usado: bool, so_ler: bool },
    Comentario { indice: usize, texto: String, onde: String, gravidade: Option<String> },
    Rascunho { rotulo: String, editando: bool },
}

/// O diff lido, ou por que não deu.
enum Carga {
    Lendo,
    SemCopia,
    Erro(String),
    Pronta(Entity<DiffState>),
}

/// O popover aberto (um por vez).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pop {
    Verificacao,
    Revisor,
    Ajuste,
    Arquivos,
}

/// Rótulos da barra conforme a largura.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rotulos {
    Longos,
    Curtos,
    Icone,
}

/// Como a revisão se arruma na largura do conteúdo (px a 100%).
#[derive(Clone, Copy, PartialEq, Debug)]
struct Disposicao {
    /// Largura da lateral; sem ela, os arquivos vão para o chip "Arquivos".
    lateral: Option<f32>,
    rotulos: Rotulos,
    /// O Unificado | Lado a lado aparece (sem ele, fica unificado).
    segmentado: bool,
    /// O lado a lado cabe.
    lado_a_lado_ok: bool,
}

const LARGO: f32 = 1180.;
const MEDIO: f32 = 760.;
const CABE_LADO_A_LADO: f32 = 960.;

fn disposicao(largura: f32, lado_a_lado: bool) -> Disposicao {
    if largura >= LARGO {
        let recolhe = lado_a_lado && largura - 276. < CABE_LADO_A_LADO;
        Disposicao {
            lateral: (!recolhe).then_some(264.),
            rotulos: Rotulos::Longos,
            segmentado: true,
            lado_a_lado_ok: true,
        }
    } else if largura >= MEDIO {
        let ok = largura >= CABE_LADO_A_LADO;
        Disposicao {
            lateral: (!(lado_a_lado && ok)).then_some(224.),
            rotulos: Rotulos::Curtos,
            segmentado: true,
            lado_a_lado_ok: ok,
        }
    } else {
        Disposicao {
            lateral: None,
            rotulos: Rotulos::Icone,
            segmentado: false,
            lado_a_lado_ok: false,
        }
    }
}

/// Um grupo da lateral: os arquivos da raiz, os de uma pasta ou os gerados.
#[derive(Clone, Debug, PartialEq)]
enum Pasta {
    Raiz,
    Em(String),
    Gerados,
}

#[derive(Clone, Debug, PartialEq)]
struct GrupoDePasta {
    pasta: Pasta,
    arquivos: Vec<usize>,
}

/// Os arquivos (caminho e se é gerado) por pasta: a raiz primeiro, as pastas na ordem em que
/// aparecem e os gerados no fim.
fn grupos_por_pasta(caminhos: &[(&str, bool)]) -> Vec<GrupoDePasta> {
    let (mut raiz, mut gerados) = (Vec::new(), Vec::new());
    let mut pastas: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, (caminho, gerado)) in caminhos.iter().enumerate() {
        if *gerado {
            gerados.push(i);
            continue;
        }
        match caminho.rsplit_once('/') {
            None => raiz.push(i),
            Some((pasta, _)) => match pastas.iter_mut().find(|(p, _)| p == pasta) {
                Some((_, lista)) => lista.push(i),
                None => pastas.push((pasta.to_string(), vec![i])),
            },
        }
    }
    let mut grupos = Vec::new();
    if !raiz.is_empty() {
        grupos.push(GrupoDePasta { pasta: Pasta::Raiz, arquivos: raiz });
    }
    grupos.extend(pastas.into_iter().map(|(p, arquivos)| GrupoDePasta { pasta: Pasta::Em(p), arquivos }));
    if !gerados.is_empty() {
        grupos.push(GrupoDePasta { pasta: Pasta::Gerados, arquivos: gerados });
    }
    grupos
}

/// 2 alta, 1 média, 0 baixa.
fn peso_da_gravidade(gravidade: &str) -> u8 {
    match gravidade.trim().to_lowercase().as_str() {
        "alta" => 2,
        "média" | "media" => 1,
        _ => 0,
    }
}

fn nome_da_gravidade(peso: u8) -> &'static str {
    match peso {
        2 => "alta",
        1 => "média",
        _ => "baixa",
    }
}

fn cor_da_gravidade(peso: u8) -> Hsla {
    match peso {
        2 => Cores::vermelho(),
        1 => Cores::ambar(),
        _ => Cores::tenue(),
    }
}

/// O texto que vai ao trabalhador: o recado, os comentários nas linhas e, se o usuário quis, os
/// apontamentos do revisor que ele não usou.
fn texto_do_ajuste(rodada: u32, recado: &str, comentarios: &[Comentario], apontamentos: &[Problema]) -> String {
    let mut texto = format!("Ajustes pedidos pelo usuário na revisão (rodada {rodada}):\n");
    let recado = recado.trim();
    if !recado.is_empty() {
        texto.push('\n');
        texto.push_str(recado);
        texto.push('\n');
    }
    if !comentarios.is_empty() {
        texto.push_str("\nComentários nas linhas:\n");
        for c in comentarios {
            texto.push_str(&descrever(c));
            texto.push('\n');
        }
    }
    if !apontamentos.is_empty() {
        texto.push_str("\nApontamentos do revisor que o usuário quer resolvidos:\n");
        for p in apontamentos {
            let linha = p.linha.map(|l| format!(":{l}")).unwrap_or_default();
            let gravidade = nome_da_gravidade(peso_da_gravidade(&p.gravidade));
            texto.push_str(&format!("- [{gravidade}] {}{linha}: {}\n", p.arquivo, p.descricao.trim()));
        }
    }
    texto.trim_end().to_string()
}

/// "Ajuste enviado ao chat 2: 3 comentários."
fn resumo_do_ajuste(tarefa: u64, comentarios: usize, apontamentos: usize) -> String {
    let mut partes = Vec::new();
    if comentarios > 0 {
        partes.push(contagem(comentarios, "comentário", "comentários"));
    }
    if apontamentos > 0 {
        partes.push(format!("{} do revisor", contagem(apontamentos, "apontamento", "apontamentos")));
    }
    if partes.is_empty() {
        format!("Ajuste enviado ao chat {tarefa}.")
    } else {
        format!("Ajuste enviado ao chat {tarefa}: {}.", partes.join(" e "))
    }
}

/// Algo que o usuário deveria ver antes de aprovar; a vermelha vai para a ressalva do commit.
#[derive(Clone, Debug, PartialEq)]
struct Pendencia {
    texto: String,
    ressalva: Option<String>,
}

/// O que pesa contra aprovar agora: a verificação vermelha, apontamento de gravidade alta (os
/// dois só desta rodada), o diff que não deu para ler e comentários que não foram enviados.
fn pendencias_de(t: &Tarefa, comentarios: usize, diff_com_erro: bool) -> Vec<Pendencia> {
    let mut lista = Vec::new();
    let valem = t.resultados_valem();
    if valem && let Some(v) = t.ultima_verificacao.as_ref().filter(|v| !v.passou) {
        lista.push(Pendencia {
            texto: format!("A verificação `{}` falhou nesta rodada.", v.comando),
            ressalva: Some(format!("a verificação `{}` falhou", v.comando)),
        });
    }
    let altas = t
        .revisao
        .as_ref()
        .filter(|_| valem)
        .map_or(0, |r| r.problemas.iter().filter(|p| peso_da_gravidade(&p.gravidade) == 2).count());
    if altas > 0 {
        let problemas = contagem(altas, "problema", "problemas");
        lista.push(Pendencia {
            texto: format!("O revisor apontou {problemas} de gravidade alta."),
            ressalva: Some(format!("o revisor apontou {problemas} de gravidade alta")),
        });
    }
    if diff_com_erro {
        lista.push(Pendencia { texto: "Não deu para ler o diff deste chat.".into(), ressalva: None });
    }
    match comentarios {
        0 => {}
        1 => lista.push(Pendencia {
            texto: "Seu comentário ainda não foi enviado e vai ser descartado.".into(),
            ressalva: None,
        }),
        n => lista.push(Pendencia {
            texto: format!("Seus {n} comentários ainda não foram enviados e vão ser descartados."),
            ressalva: None,
        }),
    }
    lista
}

/// Qual botão do rodapé é o azul (e o do Ctrl+Enter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Principal {
    Aprovar,
    Ajuste,
}

/// Com algo que pesa contra aprovar (a verificação vermelha, apontamento alto, o diff que não deu
/// para ler, comentário por enviar), o caminho é pedir ajuste; sem nada, aprovar.
fn principal(pendencias: &[Pendencia]) -> Principal {
    if pendencias.is_empty() { Principal::Aprovar } else { Principal::Ajuste }
}

/// A rodada nova que chegou desde que o diff foi lido (outra rodada, ou o trabalhador que estava
/// rodando entregou).
fn entrega_nova(lida: (u32, EstadoTarefa), rodada: u32, estado: EstadoTarefa) -> Option<u32> {
    (rodada != lida.0 || (lida.1.ativa() && !estado.ativa())).then_some(rodada)
}

/// "1 de 3 arquivos vistos · 1 comentário para enviar".
fn texto_do_rodape(vistos: usize, total: usize, comentarios: usize) -> String {
    let mut partes = Vec::new();
    if total > 0 {
        partes.push(if total == 1 {
            format!("{vistos} de 1 arquivo visto")
        } else {
            format!("{vistos} de {total} arquivos vistos")
        });
    }
    partes.push(match comentarios {
        0 if partes.is_empty() => "Nenhum comentário ainda".to_string(),
        0 => "nenhum comentário ainda".to_string(),
        1 => "1 comentário para enviar".to_string(),
        n => format!("{n} comentários para enviar"),
    });
    partes.join(" · ")
}

// F7 e Shift+F7: a próxima mudança e a anterior; Ctrl+PgDn e Ctrl+PgUp, o arquivo; C ou Enter
// nas linhas escolhidas abre o rascunho; Ctrl+Enter, o botão azul do rodapé.
actions!(vitral, [MudancaSeguinte, MudancaAnterior, ArquivoSeguinte, ArquivoAnterior, AbrirRascunho, AcaoPrincipal]);

/// Largura da área da janela que o diálogo usa (sem a sombra da janela).
fn area_da_janela(window: &Window) -> gpui_kit::Size<Pixels> {
    let folga = gpui_kit::component::window_paddings(window);
    let vista = window.viewport_size();
    size(vista.width - folga.left - folga.right, vista.height - folga.top - folga.bottom)
}

/// O diálogo vai até 1760 com 24 de folga de cada lado.
fn largura_do_dialogo(window: &Window) -> Pixels {
    (area_da_janela(window).width - px(48.)).min(px(1760.))
}

fn nome_do_arquivo(caminho: &str) -> &str {
    caminho.rsplit('/').next().unwrap_or(caminho)
}

fn icone_do_arquivo(arquivo: &DiffFile) -> Lucide {
    match arquivo.status() {
        DiffFileStatus::Added => Lucide::FilePlus,
        DiffFileStatus::Deleted => Lucide::FileMinus,
        DiffFileStatus::Renamed | DiffFileStatus::Copied => Lucide::FileSymlink,
        _ => Lucide::File,
    }
}

pub struct RevisaoDiff {
    motor: Entity<Motor>,
    janela: WeakEntity<Janela>,
    tarefa: u64,
    /// De onde vem o diff: a cópia da tarefa, ou o PR no GitHub (só para ler, da aba PR).
    fonte: FonteDiff,
    /// A pasta entrega por PR: o formulário do PR embaixo do diff ("Revisar e abrir PR").
    pr: Option<FormularioPr>,
    carga: Carga,
    /// Na ordem da lateral (raiz, pastas, gerados).
    arquivos: Vec<DiffFile>,
    grupos: Vec<GrupoDePasta>,
    gerados: HashSet<SharedString>,
    atual: usize,
    comentarios: Vec<Comentario>,
    rascunho: Option<Rascunho>,
    campo: Entity<TextareaState>,
    /// Recado geral do "Pedir ajuste".
    recado: Entity<TextareaState>,
    incluir_revisor: bool,
    filtro: Entity<InputState>,
    /// Os arquivos no chip "Arquivos", quando a lateral não cabe.
    lista_arquivos: Entity<ListaDeEscolha>,
    vistos: HashSet<SharedString>,
    enviando: bool,
    /// Problema a mostrar quando o diff chegar (arquivo, linha).
    foco: Option<(String, Option<u64>)>,
    lado_a_lado: bool,
    inteiro: bool,
    quebrar: bool,
    pop: Option<Pop>,
    /// O que não deu (a junção com conflito): fica no topo até fechar.
    aviso: Option<(Tom, String)>,
    /// Rodada e estado da tarefa quando o diff foi lido.
    lida: (u32, EstadoTarefa),
    /// O diálogo foca a si mesmo ao abrir; a revisão pega o foco no primeiro quadro (o diff,
    /// quando já leu), para as teclas dela chegarem.
    foco_teclado: FocusHandle,
    focada: bool,
    _assinaturas: Vec<Subscription>,
}

impl RevisaoDiff {
    pub fn new(
        motor: Entity<Motor>,
        janela: WeakEntity<Janela>,
        tarefa: u64,
        foco: Option<(String, Option<u64>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::com_fonte(motor, janela, tarefa, foco, FonteDiff::Tarefa(tarefa), window, cx)
    }

    /// A revisão de um diff de outra fonte (o PR no GitHub, quando a cópia já foi limpa).
    pub fn com_fonte(
        motor: Entity<Motor>,
        janela: WeakEntity<Janela>,
        tarefa: u64,
        foco: Option<(String, Option<u64>)>,
        fonte: FonteDiff,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let campo = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 6)
                .submit_on_enter(true)
                .placeholder("O que mudar aqui…")
        });
        let recado = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 6)
                .submit_on_enter(true)
                .placeholder("Recado geral para o chat (opcional)…")
        });
        let filtro = cx.new(|cx| InputState::new(window, cx).placeholder("Filtrar arquivos"));
        let lista_arquivos = cx.new(|cx| ListaDeEscolha::new("Buscar arquivo", window, cx));
        let _assinaturas = vec![
            cx.subscribe_in(&campo, window, |this, _, evento, window, cx| match evento {
                InputEvent::PressEnter { shift: false, .. } => this.salvar_comentario(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
            cx.subscribe_in(&recado, window, |this, _, evento, window, cx| match evento {
                InputEvent::PressEnter { shift: false, .. } => this.pedir_ajuste(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
            cx.subscribe(&filtro, |_, _, evento: &InputEvent, cx| {
                if matches!(evento, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&lista_arquivos, window, |this, _, evento: &EventoEscolha, _, cx| {
                this.pop = None;
                if let EventoEscolha::Escolheu(caminho) = evento {
                    this.ir_ao_arquivo(caminho, cx);
                }
                cx.notify();
            }),
        ];
        let lida = motor.read(cx).tarefa(tarefa).map_or((0, EstadoTarefa::EmRevisao), |t| (t.rodada, t.estado));
        let mut revisao = Self {
            motor,
            janela,
            tarefa,
            fonte,
            pr: None,
            carga: Carga::Lendo,
            arquivos: Vec::new(),
            grupos: Vec::new(),
            gerados: HashSet::new(),
            atual: 0,
            comentarios: Vec::new(),
            rascunho: None,
            campo,
            recado,
            incluir_revisor: false,
            filtro,
            lista_arquivos,
            vistos: HashSet::new(),
            enviando: false,
            foco,
            lado_a_lado: false,
            inteiro: false,
            quebrar: false,
            pop: None,
            aviso: None,
            lida,
            foco_teclado: cx.focus_handle(),
            focada: false,
            _assinaturas,
        };
        revisao.pr = revisao.formulario_novo(window, cx);
        revisao.ler(cx);
        revisao
    }

    /// O formulário do PR, quando a pasta entrega por PR (ou o chat já tem um PR aberto): o título
    /// e a descrição gerados sem gastar turno (`titulo_do_pr`, `corpo_do_pr`), e os segredos e os
    /// arquivos novos lidos em segundo plano.
    fn formulario_novo(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<FormularioPr> {
        let FonteDiff::Tarefa(id) = self.fonte else { return None };
        let m = self.motor.read(cx);
        let t = m.tarefa(id)?.clone();
        let origem = m.origem_da_tarefa(&t);
        let config = m.config_da_pasta(&t.projeto, &origem).github;
        let aberto = t.pr.clone().filter(|p| p.aberto());
        if (config.entrega != ModoEntrega::Pr && aberto.is_none()) || t.forma != Forma::Copia {
            return None;
        }
        let info = m.destino_do_pr(&origem, &config).and_then(|repo| m.repo_github(&repo).valor().map(|r| r.info.clone()));
        let metodo = super::painel_pr::metodos_do_pr(info.as_ref()).remove(0);
        // Os mesmos que a publicação sem formulário geraria (o motor junta as mensagens do chat).
        let (titulo_gerado, corpo) = m.titulo_e_corpo_do_pr(id).unwrap_or_default();
        let titulo = aberto.as_ref().map_or(titulo_gerado, |p| p.titulo.clone());
        let segredos = m.segredos_da_tarefa(id, cx);
        let novos = m.arquivos_novos(id, cx);
        cx.spawn(async move |this, cx| {
            let achados = segredos.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(f) = &mut this.pr {
                    f.achados = Some(achados);
                }
                cx.notify();
            });
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let lista = novos.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(f) = &mut this.pr {
                    f.novos = Some(lista);
                }
                cx.notify();
            });
        })
        .detach();
        let campo_titulo = cx.new(|cx| InputState::new(window, cx).placeholder("O título do PR").default_value(titulo));
        let descricao = cx.new(|cx| {
            TextareaState::new(window, cx).auto_grow(4, 8).placeholder("O que muda e como foi verificado").default_value(corpo)
        });
        Some(FormularioPr {
            atualizar: aberto.as_ref().map(|p| p.numero),
            titulo: campo_titulo,
            descricao,
            atualizar_descricao: false,
            rascunho: config.rascunho,
            ship: aberto.as_ref().is_some_and(|p| p.ao_ficar_pronto.is_some()),
            metodo: aberto.and_then(|p| p.ao_ficar_pronto).unwrap_or(metodo),
            segredos_ok: false,
            achados: None,
            novos: None,
            enviando: false,
        })
    }

    /// Por que "Abrir PR" ainda não vai: os segredos sem a confirmação, o título vazio.
    fn bloqueio_do_pr(&self, cx: &App) -> Option<String> {
        let f = self.pr.as_ref()?;
        if f.achados.as_ref().is_some_and(|a| !a.is_empty()) && !f.segredos_ok {
            return Some("Parece haver um segredo nas mudanças: confira e marque “Confirmo que não são segredos”".into());
        }
        let vai_titulo = f.atualizar.is_none() || f.atualizar_descricao;
        if vai_titulo && f.titulo.read(cx).value().trim().is_empty() {
            return Some("Escreva o título do PR".into());
        }
        None
    }

    /// "Abrir PR" / "Atualizar PR": manda o pedido ao motor e espera; o erro fica no topo do diálogo.
    fn publicar_pr(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.bloqueio_do_pr(cx).is_some() || self.motor.read(cx).recusa_aprovar(self.tarefa).is_some() {
            return;
        }
        let Some(f) = &mut self.pr else { return };
        let vai_texto = f.atualizar.is_none() || f.atualizar_descricao;
        let pedido = PedidoPr {
            titulo: vai_texto.then(|| f.titulo.read(cx).value().trim().to_string()),
            descricao: vai_texto.then(|| f.descricao.read(cx).value().to_string()),
            rascunho: f.rascunho && f.atualizar.is_none(),
            quando_pronto: f.ship.then(|| f.metodo.clone()),
            segredos_confirmados: f.segredos_ok,
        };
        f.enviando = true;
        let atualizar = f.atualizar.is_some();
        let id = self.tarefa;
        let recebe = self.motor.update(cx, |m, cx| m.publicar_pr_pelo_usuario(id, pedido, cx));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(resposta) = recebe.recv().await else { return };
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(f) = &mut this.pr {
                    f.enviando = false;
                }
                if resposta.erro {
                    let titulo = if atualizar { "Não deu para atualizar o PR" } else { "Não deu para abrir o PR" };
                    this.aviso = Some((Tom::Erro, format!("{titulo}: {}", resposta.texto)));
                    cx.notify();
                    return;
                }
                let texto = resposta.texto.lines().next().unwrap_or("Pronto.").to_string();
                window.push_notification(aviso(Tom::Pronto, texto), cx);
                let _ = this.janela.update(cx, |j, _| j.revisoes.remove(&id));
                window.close_dialog(cx);
            });
        })
        .detach();
        cx.notify();
    }

    /// "Pedir ao chat uma descrição": uma mensagem do usuário, visível, e o diálogo fecha.
    fn pedir_descricao(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.motor.read(cx).tarefa(self.tarefa).map(|t| t.trabalhador.clone()) else { return };
        let texto = "Escreva o título e a descrição do PR destas mudanças (o que muda e como verificou) e abra o PR com abrir_pr.".to_string();
        self.motor.update(cx, |m, cx| m.enviar_do_usuario(&chat, texto, Vec::new(), Default::default(), cx));
        window.push_notification(aviso(Tom::Pronto, "Pedi ao chat o título e a descrição do PR."), cx);
        window.close_dialog(cx);
    }

    /// O formulário do PR, embaixo do diff: título e descrição (recolhidos no "Atualizar PR" até
    /// "Atualizar a descrição"), os arquivos novos, os segredos com a confirmação, o aviso dos
    /// workflows e as escolhas de rascunho e de mesclar quando ficar pronto.
    fn formulario_pr(&self, d: Disposicao, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let f = self.pr.as_ref()?;
        let fraca = cx.entity().downgrade();
        let mudar = |mudanca: fn(&mut FormularioPr, bool)| {
            let fraca = fraca.clone();
            move |marcado: &bool, _: &mut Window, cx: &mut App| {
                let marcado = *marcado;
                let _ = fraca.update(cx, |this, cx| {
                    if let Some(f) = &mut this.pr {
                        mudanca(f, marcado);
                    }
                    cx.notify();
                });
            }
        };
        let estreito = d.rotulos == Rotulos::Icone;
        let rotulo = |texto: &'static str| div().flex_none().w(r(88.)).pt(esp::E6).rotulo().text_color(Cores::tenue()).child(texto);
        let linha = |r: &'static str, campo: AnyElement| {
            if estreito {
                v_flex().gap(esp::E4).child(div().rotulo().text_color(Cores::tenue()).child(r)).child(campo).into_any_element()
            } else {
                h_flex().items_start().gap(esp::E12).child(rotulo(r)).child(div().flex_1().min_w_0().child(campo)).into_any_element()
            }
        };
        let mostra_texto = f.atualizar.is_none() || f.atualizar_descricao;
        let mut filhos: Vec<AnyElement> = Vec::new();
        if f.atualizar.is_some() {
            filhos.push(
                Checkbox::new("pr-atualizar-descricao")
                    .label("Atualizar a descrição")
                    .checked(f.atualizar_descricao)
                    .on_click(mudar(|f, v| f.atualizar_descricao = v))
                    .into_any_element(),
            );
        }
        if mostra_texto {
            filhos.push(linha("Título", Input::new(&f.titulo).with_size(Size::Medium).into_any_element()));
            let pedir = botao_fantasma("pr-pedir-descricao", Some(Lucide::MessageSquareQuote.into()), "Pedir ao chat uma descrição", TamanhoBotao::Linha, window, cx)
                .tooltip("Manda ao chat um pedido para escrever o título e a descrição e abrir o PR; o diálogo fecha")
                .on_click(cx.listener(|this, _, window, cx| this.pedir_descricao(window, cx)));
            filhos.push(linha(
                "Descrição",
                v_flex().gap(esp::E4).child(Textarea::new(&f.descricao)).child(h_flex().ml(r(-8.)).child(pedir)).into_any_element(),
            ));
        }
        // Os arquivos que o PR acrescenta (os copiados da origem nunca entram no commit).
        let achados = f.achados.clone().unwrap_or_default();
        if let Some(novos) = f.novos.as_ref().filter(|n| !n.is_empty()) {
            let marcados: Vec<String> = novos
                .iter()
                .map(|a| if achados.iter().any(|x| &x.arquivo == a) { format!("{a} (!)") } else { a.clone() })
                .collect();
            filhos.push(
                h_flex()
                    .gap(esp::E8)
                    .metadado()
                    .child(div().flex_none().text_color(Cores::tenue()).child("Arquivos novos"))
                    .child(div().min_w_0().text_color(Cores::texto()).child(marcados.join(", ")))
                    .into_any_element(),
            );
        }
        if !achados.is_empty() {
            let lista: Vec<String> = achados.iter().map(|a| a.descricao()).collect();
            filhos.push(
                v_flex()
                    .gap(esp::E6)
                    .child(Alerta::new(Tom::Erro, format!("Parece segredo: {}.", lista.join("; "))).compacto())
                    .child(
                        Checkbox::new("pr-segredos-ok")
                            .label("Confirmo que não são segredos")
                            .checked(f.segredos_ok)
                            .on_click(mudar(|f, v| f.segredos_ok = v)),
                    )
                    .into_any_element(),
            );
        }
        let caminhos: Vec<String> = self.arquivos.iter().map(|a| a.path().to_string()).collect();
        if git::mexe_em_workflows(&caminhos) {
            filhos.push(
                Alerta::new(Tom::Atencao, "Este PR muda workflows do GitHub Actions (rodam com os segredos do repositório); o Vitral nunca publica isso sozinho.")
                    .compacto()
                    .into_any_element(),
            );
        }
        let ship = Checkbox::new("pr-ship")
            .label(format!("Mesclar quando ficar pronto ({})", super::painel_pr::metodo_em_texto(&f.metodo)))
            .checked(f.ship)
            .tooltip("O Vitral corrige CI e revisão (até 3 rodadas), publica de novo e mescla sozinho quando passar")
            .on_click(mudar(|f, v| f.ship = v));
        filhos.push(
            h_flex()
                .flex_wrap()
                .gap(esp::E16)
                .when(f.atualizar.is_none(), |s| {
                    s.child(
                        Checkbox::new("pr-rascunho")
                            .label("Abrir como rascunho")
                            .checked(f.rascunho)
                            .on_click(mudar(|f, v| f.rascunho = v)),
                    )
                })
                .child(ship)
                .into_any_element(),
        );
        Some(
            v_flex()
                .id("formulario-pr")
                .flex_none()
                .max_h(r(360.))
                .overflow_y_scroll()
                .mt(esp::E12)
                .gap(esp::E12)
                .p(dentro_da_borda(12.))
                .rounded(raio::CARTAO)
                .bg(Cores::cartao_elevado())
                .border_1()
                .border_color(Cores::linha())
                .children(filhos)
                .into_any_element(),
        )
    }

    /// Lê o diff em segundo plano; o que já foi comentado fica.
    fn ler(&mut self, cx: &mut Context<Self>) {
        enum Leitura {
            Copia(Task<Result<String, LeituraDiff>>),
            Pr(Task<Option<String>>),
        }
        let leitura = match &self.fonte {
            FonteDiff::Tarefa(id) => {
                let id = *id;
                Leitura::Copia(self.motor.update(cx, |m, cx| m.diff_da_tarefa(id, cx)))
            }
            FonteDiff::Pr { .. } => Leitura::Pr(self.motor.read(cx).diff_do_pr(self.fonte.clone(), cx)),
        };
        cx.spawn(async move |this, cx| {
            let resultado = match leitura {
                Leitura::Copia(t) => t.await,
                Leitura::Pr(t) => t.await.ok_or_else(|| LeituraDiff::Git("Não consegui ler o diff do PR no GitHub.".into())),
            };
            let _ = this.update(cx, |this, cx| {
                this.carregar(resultado, cx);
                if let Some((arquivo, linha)) = this.foco.take() {
                    this.focar(&arquivo, linha, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Ao reabrir o diálogo: relê se chegou outra rodada e vai ao problema pedido.
    fn reabrir(&mut self, foco: Option<(String, Option<u64>)>, cx: &mut Context<Self>) {
        self.focada = false;
        self.pop = None;
        let agora = self.motor.read(cx).tarefa(self.tarefa).map(|t| (t.rodada, t.estado));
        let mudou = agora.is_some_and(|(rodada, estado)| entrega_nova(self.lida, rodada, estado).is_some());
        if mudou {
            self.foco = foco;
            self.recarregar(cx);
        } else if let Some((arquivo, linha)) = foco {
            self.focar(&arquivo, linha, cx);
        }
        cx.notify();
    }

    /// Relê o diff: os comentários ficam (os de arquivos que sumiram vão para "fora do diff"), e os
    /// vistos e o aviso saem. O rascunho fica, se o arquivo dele ainda estiver no diff.
    fn recarregar(&mut self, cx: &mut Context<Self>) {
        self.vistos.clear();
        self.aviso = None;
        self.carga = Carga::Lendo;
        self.focada = false;
        self.ler(cx);
        cx.notify();
    }

    /// F7 e Shift+F7: o mesmo das setas da barra.
    fn pular_mudanca(&self, seguinte: bool, cx: &mut Context<Self>) {
        if let Some(estado) = self.estado() {
            estado.update(cx, |estado, cx| {
                if seguinte {
                    estado.next_change(cx)
                } else {
                    estado.previous_change(cx)
                }
            });
        }
    }

    /// Ctrl+PgDn e Ctrl+PgUp: o arquivo seguinte ou o anterior.
    fn pular_arquivo(&mut self, seguinte: bool, cx: &mut Context<Self>) {
        if self.arquivos.is_empty() {
            return;
        }
        let novo = if seguinte {
            (self.atual + 1).min(self.arquivos.len() - 1)
        } else {
            self.atual.saturating_sub(1)
        };
        let caminho = self.arquivos[novo].path().clone();
        self.ir_ao_arquivo(&caminho, cx);
    }

    fn ir_ao_arquivo(&mut self, caminho: &str, cx: &mut Context<Self>) {
        let Some(estado) = self.estado() else {
            return;
        };
        if let Some(i) = self.arquivos.iter().position(|a| a.path().as_ref() == caminho) {
            self.atual = i;
        }
        estado.update(cx, |estado, cx| estado.scroll_to_file(caminho, cx));
        cx.notify();
    }

    /// Arquivos e linhas (+, −) do diff, quando já carregou: vão no subtítulo.
    fn tamanho(&self) -> Option<(usize, usize, usize)> {
        if !matches!(self.carga, Carga::Pronta(_)) {
            return None;
        }
        let mais = self.arquivos.iter().map(DiffFile::additions).sum();
        let menos = self.arquivos.iter().map(DiffFile::deletions).sum();
        Some((self.arquivos.len(), mais, menos))
    }

    fn carregar(&mut self, resultado: Result<String, LeituraDiff>, cx: &mut Context<Self>) {
        if let Some(t) = self.motor.read(cx).tarefa(self.tarefa) {
            self.lida = (t.rodada, t.estado);
        }
        self.atual = 0;
        let texto = match resultado {
            Ok(texto) => texto,
            Err(LeituraDiff::SemCopia) => {
                self.carga = Carga::SemCopia;
                self.arquivos.clear();
                self.grupos.clear();
                return;
            }
            Err(LeituraDiff::Git(erro)) => {
                self.carga = Carga::Erro(erro);
                self.arquivos.clear();
                self.grupos.clear();
                return;
            }
        };
        let lidos = match DiffFile::parse(&texto) {
            Ok(lidos) => lidos,
            Err(erro) => {
                self.carga = Carga::Erro(format!("O diff não se leu (linha {}: {}).", erro.line(), erro.message()));
                self.arquivos.clear();
                self.grupos.clear();
                return;
            }
        };
        let marcas: Vec<(&str, bool)> =
            lidos.iter().map(|a| (a.path().as_ref(), git::eh_gerado(a.path().as_ref()))).collect();
        let ordem: Vec<usize> = grupos_por_pasta(&marcas).into_iter().flat_map(|g| g.arquivos).collect();
        let arquivos: Vec<DiffFile> = ordem.into_iter().map(|i| lidos[i].clone()).collect();
        self.gerados = arquivos
            .iter()
            .filter(|a| git::eh_gerado(a.path().as_ref()))
            .map(|a| a.path().clone())
            .collect();
        let marcas: Vec<(&str, bool)> =
            arquivos.iter().map(|a| (a.path().as_ref(), self.gerados.contains(a.path()))).collect();
        self.grupos = grupos_por_pasta(&marcas);
        let (inteiro, gerados) = (self.inteiro, self.gerados.clone());
        let lista = arquivos.clone();
        let estado = cx.new(move |cx| DiffState::new(lista, cx));
        estado.update(cx, |estado, cx| {
            if inteiro {
                estado.expand_unchanged(cx);
            }
            for caminho in &gerados {
                estado.set_file_collapsed(caminho, true, cx);
            }
        });
        self.arquivos = arquivos;
        self.carga = Carga::Pronta(estado);
        self.focada = false;
        if self.rascunho.as_ref().is_some_and(|r| self.arquivo(&r.arquivo).is_none()) {
            self.rascunho = None;
        }
        self.revelar_anotadas(cx);
    }

    fn estado(&self) -> Option<Entity<DiffState>> {
        match &self.carga {
            Carga::Pronta(estado) => Some(estado.clone()),
            _ => None,
        }
    }

    /// Os apontamentos que viram sugestão no diff: os do revisor e, com o PR do chat (ou o diff do
    /// PR), os comentários em linha da revisão do PR, depois deles ("A revisão do PR apontou").
    fn problemas(&self, cx: &App) -> Vec<Problema> {
        let m = self.motor.read(cx);
        let mut lista = m.tarefa(self.tarefa).and_then(|t| t.revisao.as_ref()).map(|r| r.problemas.clone()).unwrap_or_default();
        let pr = match &self.fonte {
            FonteDiff::Pr { repositorio, numero } => Some((repositorio.clone(), *numero)),
            FonteDiff::Tarefa(id) => m.pr_da_tarefa(*id).filter(|p| p.aberto()).map(|p| (p.repositorio.clone(), p.numero)),
        };
        if let Some(eventos) = pr.as_ref().and_then(|(repo, n)| m.linha_do_tempo(repo, *n).valor().cloned()) {
            for e in eventos {
                if let crate::motor::EventoLinha::ComentarioEmLinha { autor, arquivo, linha, corpo, .. } = e {
                    lista.push(Problema { arquivo, linha, gravidade: format!("{DA_REVISAO_DO_PR}{autor}"), descricao: corpo });
                }
            }
        }
        lista
    }

    fn arquivo(&self, caminho: &str) -> Option<&DiffFile> {
        self.arquivos.iter().find(|a| a.path().as_ref() == caminho)
    }

    /// Onde um ponto apontado pelo revisor fica no diff: o caminho do arquivo e, se der, a linha
    /// nova (a apontada ou uma vizinha).
    fn ancorar(&self, arquivo: &str, linha: Option<u64>) -> Option<(SharedString, Option<usize>)> {
        let caminhos: Vec<&str> = self.arquivos.iter().map(|a| a.path().as_ref()).collect();
        let achado = &self.arquivos[achar_arquivo(&caminhos, arquivo)?];
        let linha = linha.and_then(|n| {
            linha_mais_perto(n as usize, |l| achado.line_text(DiffSide::Modified, l).is_some())
        });
        Some((achado.path().clone(), linha))
    }

    /// Abre as dobras que escondem linhas com problema ou comentário, para os cartões aparecerem.
    fn revelar_anotadas(&self, cx: &mut Context<Self>) {
        let Some(estado) = self.estado() else {
            return;
        };
        let mut posicoes: Vec<DiffLinePosition> = self
            .problemas(cx)
            .iter()
            .filter_map(|p| {
                let (caminho, linha) = self.ancorar(&p.arquivo, p.linha)?;
                Some(DiffLinePosition::new(caminho, DiffSide::Modified, linha?))
            })
            .collect();
        posicoes.extend(self.comentarios.iter().filter_map(|c| {
            let (lado, linha) = c.linhas?.ancora();
            Some(DiffLinePosition::new(c.arquivo.clone(), lado, linha))
        }));
        estado.update(cx, |estado, cx| {
            for posicao in &posicoes {
                estado.reveal_line_in_place(posicao, cx);
            }
        });
    }

    /// Mostra o arquivo de um problema apontado e, se a linha (ou uma vizinha) estiver no diff,
    /// rola até ela e a destaca.
    fn focar(&mut self, arquivo: &str, linha: Option<u64>, cx: &mut Context<Self>) {
        if matches!(self.carga, Carga::Lendo) {
            self.foco = Some((arquivo.to_string(), linha));
            return;
        }
        if let Some((caminho, linha)) = self.ancorar(arquivo, linha) {
            self.ir_para(&caminho, linha, cx);
        }
    }

    fn ir_para(&mut self, caminho: &str, linha: Option<usize>, cx: &mut Context<Self>) {
        let Some(estado) = self.estado() else {
            return;
        };
        if let Some(i) = self.arquivos.iter().position(|a| a.path().as_ref() == caminho) {
            self.atual = i;
        }
        // Algumas linhas acima da apontada, para ela não ficar colada no topo.
        let acima = linha.map(|l| {
            (l.saturating_sub(4).max(1)..=l)
                .find(|&n| self.arquivo(caminho).is_some_and(|a| a.line_text(DiffSide::Modified, n).is_some()))
                .unwrap_or(l)
        });
        let caminho = SharedString::from(caminho.to_string());
        estado.update(cx, |estado, cx| match (linha, acima) {
            (Some(linha), Some(acima)) => {
                estado.set_file_collapsed(&caminho, false, cx);
                estado.reveal_line_in_place(&DiffLinePosition::new(caminho.clone(), DiffSide::Modified, linha), cx);
                estado.scroll_to_line(DiffLinePosition::new(caminho.clone(), DiffSide::Modified, acima), cx);
                estado.set_selected_lines(Some(DiffLineRange::new(caminho, DiffSide::Modified, linha, linha)), cx);
            }
            _ => estado.scroll_to_file(&caminho, cx),
        });
        cx.notify();
    }

    /// Aprovada ou cancelada, ou o diff do PR no GitHub: só leitura (sem "+", rascunho nem
    /// Descartar).
    fn so_leitura(&self, cx: &App) -> bool {
        matches!(self.fonte, FonteDiff::Pr { .. }) || self.motor.read(cx).tarefa(self.tarefa).is_none_or(|t| t.estado.encerrada())
    }

    fn abrir_rascunho(&mut self, arquivo: String, linhas: Option<Linhas>, window: &mut Window, cx: &mut Context<Self>) {
        if self.rascunho.as_ref().is_some_and(|r| r.editando.is_some()) {
            self.campo.update(cx, |c, cx| c.set_value("", window, cx));
        }
        self.rascunho = Some(Rascunho {
            arquivo,
            linhas,
            editando: None,
        });
        let foco = self.campo.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    /// C ou Enter no diff: o rascunho nas linhas escolhidas (um rascunho aberto vem para elas, com
    /// o texto, como no "+"). Fora do diff a tecla segue (o "c" digitado num campo dentro dele,
    /// como o do próprio rascunho).
    fn abrir_rascunho_na_selecao(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(estado) = self.estado() else {
            cx.propagate();
            return;
        };
        let selecao = estado.read(cx).selected_lines();
        let no_diff = estado.read(cx).focus_handle(cx).is_focused(window);
        match selecao {
            Some(faixa) if no_diff && !self.so_leitura(cx) => {
                self.abrir_rascunho(faixa.path().to_string(), Some(Linhas::de_faixa(&faixa)), window, cx)
            }
            _ => cx.propagate(),
        }
    }

    fn cancelar_rascunho(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rascunho = None;
        self.campo.update(cx, |c, cx| c.set_value("", window, cx));
        self.limpar_selecao(cx);
        if let Some(estado) = self.estado() {
            let foco = estado.read(cx).focus_handle(cx);
            window.focus(&foco, cx);
        }
        cx.notify();
    }

    /// Tira o destaque das linhas escolhidas (pelo "+" ou ao ir até um problema).
    fn limpar_selecao(&self, cx: &mut Context<Self>) {
        if let Some(estado) = self.estado() {
            estado.update(cx, |estado, cx| estado.set_selected_lines(None, cx));
        }
    }

    fn editar_comentario(&mut self, indice: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = self.comentarios.get(indice).cloned() else {
            return;
        };
        self.campo.update(cx, |campo, cx| campo.set_value(c.texto, window, cx));
        self.rascunho = Some(Rascunho {
            arquivo: c.arquivo,
            linhas: c.linhas,
            editando: Some(indice),
        });
        let foco = self.campo.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    fn remover_comentario(&mut self, indice: usize, window: &mut Window, cx: &mut Context<Self>) {
        if indice >= self.comentarios.len() {
            return;
        }
        self.comentarios.remove(indice);
        match self.rascunho.as_ref().and_then(|r| r.editando) {
            Some(e) if e == indice => self.cancelar_rascunho(window, cx),
            Some(e) if e > indice => {
                if let Some(r) = &mut self.rascunho {
                    r.editando = Some(e - 1);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// O texto da primeira linha do comentário, que vai junto para o trabalhador achar o ponto.
    fn trecho(&self, arquivo: &str, linhas: Option<Linhas>) -> String {
        let Some(l) = linhas else {
            return String::new();
        };
        let texto: String = self
            .arquivo(arquivo)
            .and_then(|a| a.line_text(l.lado, l.inicio))
            .map(|t| t.trim().chars().take(160).collect())
            .unwrap_or_default();
        if !texto.is_empty() && (l.lado != l.lado_fim || l.inicio != l.fim) {
            format!("{texto} …")
        } else {
            texto
        }
    }

    fn salvar_comentario(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let texto = self.campo.read(cx).value().trim().to_string();
        if texto.is_empty() {
            return;
        }
        let Some(r) = self.rascunho.take() else {
            return;
        };
        match r.editando.and_then(|i| self.comentarios.get_mut(i)) {
            Some(c) => c.texto = texto,
            None => {
                let trecho = self.trecho(&r.arquivo, r.linhas);
                self.comentarios.push(Comentario {
                    arquivo: r.arquivo,
                    linhas: r.linhas,
                    trecho,
                    texto,
                    problema: None,
                });
            }
        }
        self.campo.update(cx, |c, cx| c.set_value("", window, cx));
        self.limpar_selecao(cx);
        if let Some(estado) = self.estado() {
            let foco = estado.read(cx).focus_handle(cx);
            window.focus(&foco, cx);
        }
        cx.notify();
    }

    /// Transforma um problema do revisor em comentário, na mesma linha.
    fn usar_problema(&mut self, indice: usize, rolar: bool, cx: &mut Context<Self>) {
        let Some(p) = self.problemas(cx).get(indice).cloned() else {
            return;
        };
        let (arquivo, linha) = match self.ancorar(&p.arquivo, p.linha) {
            Some((caminho, linha)) => (caminho.to_string(), linha),
            None => (p.arquivo.clone(), None),
        };
        let linhas = linha.map(|l| Linhas::uma(DiffSide::Modified, l));
        let trecho = self.trecho(&arquivo, linhas);
        self.comentarios.push(Comentario {
            arquivo: arquivo.clone(),
            linhas,
            trecho,
            texto: p.descricao,
            problema: Some(indice),
        });
        if rolar && self.arquivo(&arquivo).is_some() {
            self.ir_para(&arquivo, linha, cx);
        }
        cx.notify();
    }

    /// Os apontamentos do revisor que não viraram comentário (os da revisão do PR só vão pelo
    /// "Usar").
    fn nao_usados(&self, cx: &App) -> Vec<Problema> {
        self.problemas(cx)
            .into_iter()
            .enumerate()
            .filter(|(_, p)| !p.gravidade.starts_with(DA_REVISAO_DO_PR))
            .filter(|(i, _)| !self.comentarios.iter().any(|c| c.problema == Some(*i)))
            .map(|(_, p)| p)
            .collect()
    }

    fn pode_pedir_ajuste(&self, cx: &App) -> bool {
        !self.enviando
            && !self.so_leitura(cx)
            && (!self.comentarios.is_empty()
                || !self.recado.read(cx).value().trim().is_empty()
                || (self.incluir_revisor && !self.nao_usados(cx).is_empty()))
    }

    fn mudar_pop(&mut self, qual: Pop, aberto: bool, cx: &mut Context<Self>) {
        if aberto {
            self.pop = Some(qual);
        } else if self.pop == Some(qual) {
            self.pop = None;
        }
        cx.notify();
    }

    /// "Enviar ao chat": o recado, os comentários e, com o interruptor, os apontamentos
    /// que sobraram. O diálogo fecha e a revisão desta rodada sai do cache.
    fn pedir_ajuste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pode_pedir_ajuste(cx) {
            return;
        }
        let Some(rodada) = self.motor.read(cx).tarefa(self.tarefa).map(|t| t.rodada) else {
            return;
        };
        let apontamentos = if self.incluir_revisor { self.nao_usados(cx) } else { Vec::new() };
        let recado = self.recado.read(cx).value().to_string();
        let texto = texto_do_ajuste(rodada, &recado, &self.comentarios, &apontamentos);
        let id = self.tarefa;
        let resposta = self.motor.update(cx, |m, cx| m.ajuste_do_usuario(id, texto, cx));
        if resposta.erro {
            self.aviso = Some((Tom::Erro, resposta.texto));
            self.pop = None;
            cx.notify();
            return;
        }
        window.push_notification(aviso(Tom::Pronto, resumo_do_ajuste(id, self.comentarios.len(), apontamentos.len())), cx);
        self.comentarios.clear();
        self.rascunho = None;
        self.incluir_revisor = false;
        self.pop = None;
        self.recado.update(cx, |c, cx| c.set_value("", window, cx));
        let _ = self.janela.update(cx, |j, _| j.revisoes.remove(&id));
        window.close_dialog(cx);
    }

    /// O que pesa contra aprovar agora.
    fn pendencias(&self, t: &Tarefa) -> Vec<Pendencia> {
        pendencias_de(t, self.comentarios.len(), matches!(self.carga, Carga::Erro(_)))
    }

    /// Ctrl+Enter: o botão azul do momento. Num campo de texto (o Ctrl+Enter dele também sobe até
    /// aqui), com um rascunho ou um popover aberto, fica com eles.
    fn acao_principal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let escrevendo = self.campo.read(cx).focus_handle(cx).is_focused(window)
            || self.recado.read(cx).focus_handle(cx).is_focused(window)
            || self.filtro.read(cx).focus_handle(cx).is_focused(window);
        let parada = matches!(self.carga, Carga::Lendo | Carga::SemCopia) || self.enviando || self.so_leitura(cx);
        if escrevendo || parada || self.rascunho.is_some() || self.pop.is_some() {
            cx.propagate();
            return;
        }
        let Some(t) = self.motor.read(cx).tarefa(self.tarefa).cloned() else {
            return;
        };
        if self.pr.is_some() {
            return self.publicar_pr(window, cx);
        }
        match principal(&self.pendencias(&t)) {
            Principal::Ajuste => self.mudar_pop(Pop::Ajuste, true, cx),
            Principal::Aprovar if self.motor.read(cx).recusa_aprovar(self.tarefa).is_none() => {
                self.pedir_aprovacao(window, cx)
            }
            Principal::Aprovar => {}
        }
    }

    /// "Juntar": com pendências, pergunta antes; a pendência vermelha vai como ressalva.
    fn pedir_aprovacao(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.motor.read(cx).tarefa(self.tarefa).cloned() else {
            return;
        };
        let pendencias = self.pendencias(&t);
        if pendencias.is_empty() {
            self.aprovar(None, window, cx);
            return;
        }
        let ressalvas: Vec<String> = pendencias.iter().filter_map(|p| p.ressalva.clone()).collect();
        let vermelha = !ressalvas.is_empty();
        let ressalva = vermelha.then(|| ressalvas.join("; "));
        let detalhes = pendencias
            .iter()
            .map(|p| {
                let icone = if p.ressalva.is_some() { Lucide::CircleX } else { Lucide::TriangleAlert };
                (icone.into(), SharedString::from(p.texto.clone()), p.ressalva.is_some())
            })
            .collect();
        let fraca = cx.entity().downgrade();
        confirmar(
            Confirmacao {
                tom: if vermelha { TomConfirmacao::Perigo } else { TomConfirmacao::Atencao },
                icone: Lucide::TriangleAlert.into(),
                titulo: "Juntar com pendências?".into(),
                texto: if vermelha {
                    "Dá para juntar assim mesmo; o que está vermelho vai como ressalva no commit.".into()
                } else {
                    "Dá para juntar assim mesmo.".into()
                },
                detalhes,
                ok: "Juntar mesmo assim".into(),
                cancelar: "Voltar".into(),
                ok_perigo: vermelha,
            },
            move |window, cx| {
                let ressalva = ressalva.clone();
                let _ = fraca.update(cx, |this, cx| this.aprovar(ressalva, window, cx));
            },
            None,
            window,
            cx,
        );
    }

    /// O revisado é um chat de projeto (o botão é "Juntar", e o título diz "chat N").
    fn de_chat(&self, cx: &App) -> bool {
        let m = self.motor.read(cx);
        m.tarefa(self.tarefa).and_then(|t| m.chat(&t.trabalhador)).is_some_and(|c| c.papel == crate::modelo::Papel::Projeto)
    }

    fn aprovar(&mut self, ressalva: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.tarefa;
        let recebe = self.motor.update(cx, |m, cx| m.aprovar_pelo_usuario(id, ressalva, cx));
        self.esperar_juntar(recebe, window, cx);
    }

    /// Espera a junção: fecha com o aviso do que entrou, pergunta antes de juntar em outro branch
    /// ou mostra no diálogo o que não deixou.
    fn esperar_juntar(
        &mut self,
        recebe: smol::channel::Receiver<crate::mcp::Resposta>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.enviando = true;
        self.aviso = None;
        cx.notify();
        let id = self.tarefa;
        let de_chat = self.de_chat(cx);
        cx.spawn_in(window, async move |this, cx| {
            let resposta = recebe.recv().await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.enviando = false;
                match resposta {
                    Ok(r) if !r.erro => {
                        let texto = if de_chat {
                            r.texto.lines().next().unwrap_or("As mudanças entraram na pasta original.").to_string()
                        } else {
                            format!("Ajudante #{id} juntado na pasta do chat dele.")
                        };
                        window.push_notification(aviso(Tom::Pronto, texto), cx);
                        let _ = this.janela.update(cx, |j, _| j.revisoes.remove(&id));
                        window.close_dialog(cx);
                    }
                    Ok(r) if r.texto.starts_with(crate::motor::RAMO_TROCADO) => {
                        let fraca = cx.entity().downgrade();
                        confirmar(
                            Confirmacao {
                                tom: TomConfirmacao::Atencao,
                                icone: Lucide::GitBranch.into(),
                                titulo: "Juntar em outro branch?".into(),
                                texto: r.texto.into(),
                                detalhes: Vec::new(),
                                ok: "Juntar".into(),
                                cancelar: "Voltar".into(),
                                ok_perigo: false,
                            },
                            move |window, cx| {
                                let _ = fraca.update(cx, |this, cx| {
                                    let recebe = this.motor.update(cx, |m, cx| m.juntar_no_ramo_atual(id, cx));
                                    this.esperar_juntar(recebe, window, cx);
                                });
                            },
                            None,
                            window,
                            cx,
                        );
                    }
                    Ok(r) => {
                        let tom = if r.texto.starts_with("Conflito") { Tom::Atencao } else { Tom::Erro };
                        this.aviso = Some((tom, r.texto));
                    }
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Pergunta antes de descartar: o arquivo volta ao original e não dá para desfazer.
    fn confirmar_descarte(&mut self, caminho: String, window: &mut Window, cx: &mut Context<Self>) {
        let fraca = cx.entity().downgrade();
        let nome = nome_do_arquivo(&caminho).to_string();
        confirmar(
            Confirmacao {
                tom: TomConfirmacao::Perigo,
                icone: Lucide::Undo2.into(),
                titulo: format!("Descartar as mudanças em {nome}?").into(),
                texto: "O arquivo volta a como está no projeto (se este chat o criou, ele é apagado). \
                        Não dá para desfazer."
                    .into(),
                detalhes: Vec::new(),
                ok: "Descartar".into(),
                cancelar: "Cancelar".into(),
                ok_perigo: true,
            },
            move |window, cx| {
                let caminho = caminho.clone();
                let _ = fraca.update(cx, |this, cx| this.descartar(caminho, window, cx));
            },
            None,
            window,
            cx,
        );
    }

    fn descartar(&mut self, caminho: String, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.tarefa;
        let tarefa = self.motor.update(cx, |m, cx| m.descartar_arquivo(id, caminho.clone(), cx));
        cx.spawn_in(window, async move |this, cx| {
            let resultado = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match resultado {
                    Ok(antigo) => {
                        this.arquivos.retain(|a| a.path().as_ref() != caminho);
                        this.comentarios.retain(|c| c.arquivo != caminho);
                        this.vistos.remove(caminho.as_str());
                        // Os índices dos comentários mudaram: o rascunho aberto fica sem dono.
                        if this.rascunho.is_some() {
                            this.cancelar_rascunho(window, cx);
                        }
                        let marcas: Vec<(&str, bool)> = this
                            .arquivos
                            .iter()
                            .map(|a| (a.path().as_ref(), this.gerados.contains(a.path())))
                            .collect();
                        this.grupos = grupos_por_pasta(&marcas);
                        this.atual = this.atual.min(this.arquivos.len().saturating_sub(1));
                        if let Some(estado) = this.estado() {
                            let (arquivos, inteiro) = (this.arquivos.clone(), this.inteiro);
                            estado.update(cx, |estado, cx| {
                                estado.set_files(arquivos, cx);
                                if inteiro {
                                    estado.expand_unchanged(cx);
                                }
                            });
                        }
                        this.revelar_anotadas(cx);
                        let texto = match antigo {
                            Some(antigo) => format!("{caminho} voltou a ser {antigo}."),
                            None => format!("{caminho} voltou ao original."),
                        };
                        window.push_notification(aviso(Tom::Pronto, texto), cx);
                    }
                    Err(erro) => window.push_notification(
                        aviso_erro(format!("Não deu para descartar as mudanças de {caminho}"), &erro),
                        cx,
                    ),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn mudar_modo(&mut self, lado_a_lado: bool, cx: &mut Context<Self>) {
        self.lado_a_lado = lado_a_lado;
        cx.notify();
    }

    fn alternar_inteiro(&mut self, cx: &mut Context<Self>) {
        self.inteiro = !self.inteiro;
        if let Some(estado) = self.estado() {
            let inteiro = self.inteiro;
            estado.update(cx, |estado, cx| {
                if inteiro {
                    estado.expand_unchanged(cx)
                } else {
                    estado.collapse_unchanged(cx)
                }
            });
        }
        if !self.inteiro {
            self.revelar_anotadas(cx);
        }
        cx.notify();
    }

    /// Visto: o arquivo recolhe; tirar a marca o abre de novo.
    fn marcar_visto(&mut self, caminho: SharedString, visto: bool, cx: &mut Context<Self>) {
        if visto {
            self.vistos.insert(caminho.clone());
        } else {
            self.vistos.remove(&caminho);
        }
        if let Some(estado) = self.estado() {
            estado.update(cx, |estado, cx| estado.set_file_collapsed(&caminho, visto, cx));
        }
        cx.notify();
    }

    /// Título da tarefa e, embaixo, o projeto, a tarefa, a rodada, a pílula do estado e o tamanho
    /// do diff. Refeito a cada quadro, com o diálogo.
    pub fn cabecalho(&self, cx: &App) -> CabecalhoDialogo {
        let m = self.motor.read(cx);
        if let FonteDiff::Pr { repositorio, numero } = &self.fonte {
            let titulo = m
                .tarefa(self.tarefa)
                .and_then(|t| t.pr.iter().map(|p| (p.numero, p.titulo.clone())).chain(t.prs_anteriores.iter().map(|a| (a.numero, a.titulo.clone()))).find(|(n, _)| n == numero))
                .map(|(_, t)| t)
                .unwrap_or_default();
            let subtitulo = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(r(6.))
                .font_weight(FontWeight::NORMAL)
                .child(repositorio.clone())
                .child(div().text_color(Cores::apagado()).child("·"))
                .child("o diff no GitHub, só para ler")
                .map(|s| match self.tamanho() {
                    Some((n, mais, menos)) if n > 0 => s
                        .child(div().text_color(Cores::apagado()).child("·"))
                        .child(contagem(n, "arquivo", "arquivos"))
                        .child(div().text_color(Cores::apagado()).child("·"))
                        .child(format!("+{mais} −{menos}")),
                    _ => s,
                });
            return CabecalhoDialogo::new(format!("PR #{numero} · {titulo}")).uma_linha().subtitulo(subtitulo);
        }
        let Some(t) = m.tarefa(self.tarefa) else {
            return CabecalhoDialogo::new(format!("Chat {}", self.tarefa));
        };
        let ponto = || div().text_color(Cores::apagado()).child("·");
        if self.de_chat(cx) {
            let origem = m.origem_da_tarefa(t);
            let pasta = origem.file_name().map_or_else(|| origem.display().to_string(), |n| n.to_string_lossy().to_string());
            let subtitulo = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(r(6.))
                .font_weight(FontWeight::NORMAL)
                .child(quadrado_do_projeto(m, &t.projeto, 18.))
                .child(m.nome_do_projeto(&t.projeto))
                .child(ponto())
                .child(pasta)
                .when(t.rodada > 1, |s| s.child(ponto()).child(format!("rodada {}", t.rodada)))
                .map(|s| match self.tamanho() {
                    Some((0, _, _)) => s.child(ponto()).child("sem mudanças"),
                    Some((n, mais, menos)) => s
                        .child(ponto())
                        .child(contagem(n, "arquivo", "arquivos"))
                        .child(ponto())
                        .child(format!("+{mais} −{menos}")),
                    None => s,
                });
            let acao = match &self.pr {
                Some(f) if f.atualizar.is_some() => "Revisar e atualizar PR",
                Some(_) => "Revisar e abrir PR",
                None => "Revisar e juntar",
            };
            return CabecalhoDialogo::new(format!("{acao} · chat {} {}", t.id, t.titulo)).uma_linha().subtitulo(subtitulo);
        }
        let subtitulo = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(r(6.))
            .font_weight(FontWeight::NORMAL)
            .child(quadrado_do_projeto(m, &t.projeto, 18.))
            .child(m.nome_do_projeto(&t.projeto))
            .child(ponto())
            .child(format!("ajudante #{}", t.id))
            .when(t.rodada > 1, |s| s.child(ponto()).child(format!("rodada {}", t.rodada)))
            .child(ponto())
            .child(pilula_da_tarefa(m, t))
            .map(|s| match self.tamanho() {
                Some((0, _, _)) => s.child(ponto()).child("sem mudanças"),
                Some((n, mais, menos)) => s
                    .child(ponto())
                    .child(contagem(n, "arquivo", "arquivos"))
                    .child(ponto())
                    .child(format!("+{mais} −{menos}")),
                None => s,
            });
        CabecalhoDialogo::new(t.titulo.clone()).uma_linha().subtitulo(subtitulo)
    }

    /// Um popover controlado pela revisão: abre e fecha pelo `pop`.
    fn com_popover(
        &self,
        qual: Pop,
        id: &'static str,
        ancora: Anchor,
        gatilho: Button,
        conteudo: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        let fraca = cx.entity().downgrade();
        popover(id)
            .anchor(ancora)
            .offset(px(8.))
            .open(self.pop == Some(qual))
            .on_open_change(move |aberto, _, cx| {
                let _ = fraca.update(cx, |this, cx| this.mudar_pop(qual, *aberto, cx));
            })
            .trigger(gatilho)
            .content(move |_, window, cx| conteudo(window, cx))
            .into_any_element()
    }

    fn chip_verificacao(&self, t: &Tarefa, d: Disposicao, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let comando = t.verificacao.clone()?;
        let so_icone = d.rotulos == Rotulos::Icone;
        if t.conferindo() {
            let spinner = Spinner::new().with_size(Size::Size(tam(window, 14.))).color(Cores::destaque());
            return Some(
                Chip::new("chip-verificacao", "Verificando…")
                    .simbolo(Simbolo::Elemento(spinner.into_any_element()))
                    .tamanho(TamanhoChip::Medio)
                    .so_icone(so_icone)
                    .dica(format!("Verificando\nRodando “{comando}” na entrega."))
                    .construir(window, cx)
                    .into_any_element(),
            );
        }
        let v = t.ultima_verificacao.clone()?;
        let valem = t.resultados_valem();
        let (icone, cor) = if v.passou { (Lucide::CircleCheck, Cores::verde()) } else { (Lucide::CircleX, Cores::vermelho()) };
        let rotulo = match (valem, d.rotulos, v.passou) {
            (false, Rotulos::Longos, _) => "Verificação da rodada anterior",
            (false, _, _) => "rodada anterior",
            (true, Rotulos::Longos, true) => "Verificação passou",
            (true, Rotulos::Longos, false) => "Verificação falhou",
            (true, _, true) => "passou",
            (true, _, false) => "falhou",
        };
        let aberto = self.pop == Some(Pop::Verificacao);
        let resultado = if v.passou { "passou" } else { "falhou" };
        let mut chip = Chip::new("chip-verificacao", rotulo)
            .simbolo(Simbolo::icone_cor(icone, if valem { cor } else { Cores::tenue() }))
            .tamanho(TamanhoChip::Medio)
            .mudado(valem)
            .seta(!so_icone)
            .aberto(aberto)
            .so_icone(so_icone);
        if valem && !v.passou {
            chip = chip.alerta(Cores::vermelho());
        }
        if !aberto {
            chip = chip.dica(if valem {
                format!("Verificação {resultado}\n“{}” nesta rodada. Clique para ver a saída.", v.comando)
            } else {
                format!("Verificação da rodada anterior\n“{}” {resultado} antes do ajuste.", v.comando)
            });
        }
        let rodada = t.rodada;
        let gatilho = chip.construir(window, cx);
        Some(self.com_popover(
            Pop::Verificacao,
            "pop-verificacao",
            Anchor::TopLeft,
            gatilho,
            move |_, _| corpo_verificacao(&v, rodada, valem),
            cx,
        ))
    }

    fn chip_revisor(&self, t: &Tarefa, d: Disposicao, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let ligado = self.motor.read(cx).config.revisor;
        if !ligado && t.revisao.is_none() {
            return None;
        }
        let so_icone = d.rotulos == Rotulos::Icone;
        if t.conferindo() && ligado {
            let spinner = Spinner::new().with_size(Size::Size(tam(window, 14.))).color(Cores::destaque());
            return Some(
                Chip::new("chip-revisor", "Revisor olhando…")
                    .simbolo(Simbolo::Elemento(spinner.into_any_element()))
                    .tamanho(TamanhoChip::Medio)
                    .so_icone(so_icone)
                    .dica("Revisor olhando\nO revisor independente confere a entrega.")
                    .construir(window, cx)
                    .into_any_element(),
            );
        }
        let revisao = t.revisao.clone()?;
        let valem = t.resultados_valem();
        let aprovaria = revisao.veredito == "aprovar";
        let maior = revisao.problemas.iter().map(|p| peso_da_gravidade(&p.gravidade)).max();
        let cor = if aprovaria {
            Cores::verde()
        } else {
            maior.map_or(Cores::ambar(), |g| if g == 0 { Cores::ambar() } else { cor_da_gravidade(g) })
        };
        let rotulo = match (valem, d.rotulos, aprovaria) {
            (false, Rotulos::Longos, _) => "Revisor da rodada anterior",
            (false, _, _) => "rodada anterior",
            (true, Rotulos::Longos, true) => "Revisor aprovaria",
            (true, Rotulos::Longos, false) => "Revisor pede ajustes",
            (true, _, true) => "aprovaria",
            (true, _, false) => "pede ajustes",
        };
        let aberto = self.pop == Some(Pop::Revisor);
        let n = revisao.problemas.len();
        let mut chip = Chip::new("chip-revisor", rotulo)
            .simbolo(Simbolo::icone_cor(Lucide::Eye, if valem { cor } else { Cores::tenue() }))
            .tamanho(TamanhoChip::Medio)
            .mudado(valem)
            .seta(true)
            .aberto(aberto)
            .so_icone(so_icone);
        if n > 0 {
            chip = chip.selo(if valem { Selo::contagem(n).tingida(cor) } else { Selo::contagem(n) });
        }
        if !aberto {
            chip = chip.dica(format!(
                "Revisor independente\n{}{}. Clique para ver o parecer.",
                if aprovaria { "Aprovaria" } else { "Pede ajustes" },
                if n > 0 { format!(", {}", contagem(n, "apontamento", "apontamentos")) } else { String::new() }
            ));
        }
        let gatilho = chip.construir(window, cx);
        let dados = Rc::new(DadosRevisor {
            revisao,
            valem,
            rodada: t.rodada,
            usados: self.comentarios.iter().filter_map(|c| c.problema).collect(),
            ancorados: self.problemas(cx).iter().map(|p| self.ancorar(&p.arquivo, p.linha).is_some()).collect(),
            altura: self.altura_maxima_popover(window),
        });
        let fraca = cx.entity().downgrade();
        Some(self.com_popover(
            Pop::Revisor,
            "pop-revisor",
            Anchor::TopLeft,
            gatilho,
            move |window, cx| corpo_revisor(&dados, &fraca, window, cx),
            cx,
        ))
    }

    /// `min(440, altura da revisão − 40)`.
    fn altura_maxima_popover(&self, window: &Window) -> Pixels {
        (self.altura(window) - px(40.)).min(tam(window, 440.))
    }

    fn altura(&self, window: &Window) -> Pixels {
        let vista = area_da_janela(window);
        (vista.height - px(24.) - px(32.) - tam(window, 72.) - px(16.)).clamp(px(280.), px(1400.))
    }

    /// "Arquivos" quando a lateral não cabe: a lista com busca, por pasta.
    fn chip_arquivos(&self, projeto: &str, window: &Window, cx: &Context<Self>) -> AnyElement {
        let aberto = self.pop == Some(Pop::Arquivos);
        let mut chip = Chip::new("chip-arquivos", "Arquivos")
            .simbolo(Simbolo::icone(Lucide::Files))
            .tamanho(TamanhoChip::Medio)
            .selo(Selo::contagem(self.arquivos.len()))
            .seta(true)
            .aberto(aberto);
        if !aberto {
            chip = chip.dica("Arquivos mudados\nClique para ir a um deles.");
        }
        let gatilho = chip.construir(window, cx);
        let grupos = Rc::new(self.grupos_de_escolha(projeto, cx));
        let escolhido = self.arquivos.get(self.atual).map(|a| a.path().clone());
        let lista = self.lista_arquivos.clone();
        let fraca = cx.entity().downgrade();
        let foco = lista.read(cx).focus_handle(cx);
        let largura = LarguraPopover::P560.px();
        let (abrir, conteudo) = (lista.clone(), lista);
        popover("pop-arquivos")
            .anchor(Anchor::TopLeft)
            .offset(px(8.))
            .p(r(6.))
            .open(aberto)
            .track_focus(&foco)
            .on_open_change(move |abriu, window, cx| {
                if *abriu {
                    let (grupos, escolhido) = (grupos.clone(), escolhido.clone());
                    abrir.update(cx, |l, cx| {
                        l.definir(copiar_grupos(&grupos), escolhido, cx);
                        l.abrir(window, cx);
                    });
                }
                let _ = fraca.update(cx, |this, cx| this.mudar_pop(Pop::Arquivos, *abriu, cx));
            })
            .trigger(gatilho)
            .content(move |_, window, _| {
                let largura = (tam(window, largura) - tam(window, 12.)).min(window.viewport_size().width - px(44.));
                div().w(largura).child(conteudo.clone())
            })
            .into_any_element()
    }

    fn grupos_de_escolha(&self, projeto: &str, cx: &App) -> Vec<GrupoDeEscolha> {
        let por_arquivo = self.apontamentos_por_arquivo(cx);
        self.grupos
            .iter()
            .map(|g| GrupoDeEscolha {
                titulo: Some(nome_do_grupo(&g.pasta, projeto).into()),
                itens: g
                    .arquivos
                    .iter()
                    .map(|&i| {
                        let a = &self.arquivos[i];
                        let mut item = ItemDeEscolha::new(a.path().clone(), nome_do_arquivo(a.path()).to_string());
                        item.simbolo = Some(Simbolo::icone_cor(icone_do_arquivo(a), Cores::tenue()));
                        if let Some((_, peso)) = por_arquivo.get(a.path().as_ref()) {
                            item.selos.push(Selo::ponto(cor_da_gravidade(*peso)));
                        }
                        item.selos.push(Selo::neutro(format!("+{} −{}", a.additions(), a.deletions())));
                        item.termos = Some(a.path().clone());
                        item
                    })
                    .collect(),
            })
            .collect()
    }

    /// Quantos apontamentos do revisor caem em cada arquivo do diff, e a maior gravidade.
    fn apontamentos_por_arquivo(&self, cx: &App) -> HashMap<String, (usize, u8)> {
        let mut mapa: HashMap<String, (usize, u8)> = HashMap::new();
        for p in self.problemas(cx) {
            if let Some((caminho, _)) = self.ancorar(&p.arquivo, p.linha) {
                let entrada = mapa.entry(caminho.to_string()).or_insert((0, 0));
                entrada.0 += 1;
                entrada.1 = entrada.1.max(peso_da_gravidade(&p.gravidade));
            }
        }
        mapa
    }

    fn barra(&self, t: Option<&Tarefa>, d: Disposicao, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let pronta = matches!(self.carga, Carga::Pronta(_)) && !self.arquivos.is_empty();
        let lido = !matches!(self.carga, Carga::Lendo);
        let estado = self.estado();
        let (anterior, proxima) = (estado.clone(), estado);
        let curto = d.rotulos != Rotulos::Longos;
        let esquerda = h_flex()
            .flex_1()
            .min_w_0()
            .gap(r(4.))
            .when(pronta && d.lateral.is_none(), |s| {
                let nome = t.map(|t| self.motor.read(cx).nome_do_projeto(&t.projeto)).unwrap_or_default();
                s.child(self.chip_arquivos(&nome, window, cx))
            })
            .when_some(t.filter(|_| lido), |s, t| {
                s.children(self.chip_verificacao(t, d, window, cx))
                    .children(self.chip_revisor(t, d, window, cx))
            });
        let lado_a_lado = self.lado_a_lado && d.lado_a_lado_ok;
        let ok = d.lado_a_lado_ok;
        let segmentado = ControleSegmentado::new(
            "modo-diff",
            [
                Segmento::new("Unificado").dica("Uma coluna, com o que saiu antes do que entrou"),
                if ok {
                    Segmento::new("Lado a lado").dica("O original à esquerda e o novo à direita")
                } else {
                    Segmento::new("Lado a lado").desabilitado("Lado a lado precisa de mais largura: maximize a janela")
                },
            ],
            usize::from(lado_a_lado),
            {
                let fraca = cx.entity().downgrade();
                move |i, _, cx| {
                    let _ = fraca.update(cx, |this, cx| this.mudar_modo(i == 1, cx));
                }
            },
        )
        .with_size(Size::Small);
        let inteiro_dica = "Mostra os arquivos inteiros, não só as mudanças e 3 linhas em volta";
        let inteiro = (if curto {
            botao_icone("arquivo-inteiro", Lucide::ChevronsUpDown, inteiro_dica, TamanhoBotao::Barra, window, cx)
        } else {
            botao_fantasma(
                "arquivo-inteiro",
                Some(Lucide::ChevronsUpDown.into()),
                "Arquivo inteiro",
                TamanhoBotao::Barra,
                window,
                cx,
            )
            .tooltip(inteiro_dica)
        })
        .selected(self.inteiro)
        .disabled(!pronta)
        .on_click(cx.listener(|this, _, _, cx| this.alternar_inteiro(cx)));
        let quebrar = botao_icone("quebrar-linhas", Lucide::TextWrap, "Quebrar linhas longas", TamanhoBotao::Barra, window, cx)
            .selected(self.quebrar)
            .disabled(!pronta)
            .on_click(cx.listener(|this, _, _, cx| {
                this.quebrar = !this.quebrar;
                cx.notify();
            }));
        let anterior = botao_icone("mudanca-anterior", Lucide::ArrowUp, "Mudança anterior", TamanhoBotao::Barra, window, cx)
            .tooltip_with_action("Mudança anterior", &MudancaAnterior, Some("Revisao"))
            .disabled(!pronta)
            .on_click(move |_, _, cx| {
                if let Some(estado) = &anterior {
                    estado.update(cx, |estado, cx| estado.previous_change(cx));
                }
            });
        let proxima = botao_icone("proxima-mudanca", Lucide::ArrowDown, "Próxima mudança", TamanhoBotao::Barra, window, cx)
            .tooltip_with_action("Próxima mudança", &MudancaSeguinte, Some("Revisao"))
            .disabled(!pronta)
            .on_click(move |_, _, cx| {
                if let Some(estado) = &proxima {
                    estado.update(cx, |estado, cx| estado.next_change(cx));
                }
            });
        h_flex()
            .flex_none()
            .h(r(32.))
            .gap(r(12.))
            .child(esquerda)
            // Sem cópia, com erro ou sem mudanças, não há diff para as ferramentas.
            .when(pronta || !lido, |s| s.child(
                h_flex()
                    .flex_none()
                    .gap(r(4.))
                    .when(d.segmentado && pronta, |s| s.child(segmentado).child(div().w(r(8.))))
                    .child(inteiro)
                    .child(quebrar)
                    .child(div().mx(r(4.)).w(px(1.)).h(r(16.)).bg(Cores::borda()))
                    .child(anterior)
                    .child(proxima),
            ))
    }

    fn lateral(&self, largura: f32, projeto: &str, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let filtro = self.filtro.read(cx).value().trim().to_lowercase();
        let com_filtro = self.arquivos.len() > 8;
        let por_arquivo = self.apontamentos_por_arquivo(cx);
        let mut comentarios_por: HashMap<&str, usize> = HashMap::new();
        for c in &self.comentarios {
            *comentarios_por.entry(c.arquivo.as_str()).or_default() += 1;
        }
        let so_leitura = self.so_leitura(cx);
        let mut grupos = Vec::new();
        for (g, grupo) in self.grupos.iter().enumerate() {
            let visiveis: Vec<usize> = grupo
                .arquivos
                .iter()
                .copied()
                .filter(|&i| filtro.is_empty() || self.arquivos[i].path().to_lowercase().contains(&filtro))
                .collect();
            if visiveis.is_empty() {
                continue;
            }
            let titulo = nome_do_grupo(&grupo.pasta, projeto);
            let dica = SharedString::from(titulo.clone());
            grupos.push(
                div()
                    .id(("pasta-rev", g))
                    .h(r(24.))
                    .mt(r(if grupos.is_empty() { 0. } else { 6. }))
                    .px(r(8.))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_start()
                            .mono_pequeno()
                            .text_color(Cores::tenue())
                            .child(titulo),
                    )
                    .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                    .into_any_element(),
            );
            for i in visiveis {
                let a = &self.arquivos[i];
                let caminho = a.path().clone();
                let visto = self.vistos.contains(&caminho);
                let gerado = grupo.pasta == Pasta::Gerados;
                let nome = if gerado { caminho.to_string() } else { nome_do_arquivo(&caminho).to_string() };
                let n_coment = comentarios_por.get(caminho.as_ref()).copied().unwrap_or(0);
                let apontados = por_arquivo.get(caminho.as_ref()).copied();
                let simbolo = if visto {
                    Simbolo::icone_cor(Lucide::Check, Cores::tenue())
                } else {
                    Simbolo::icone_cor(icone_do_arquivo(a), Cores::tenue())
                };
                let mut cartao = CartaoDeLista::new(("arquivo-rev", i), nome.clone())
                    .uma_linha()
                    .simbolo(simbolo)
                    .selecionado(i == self.atual)
                    .meta(meta_do_arquivo(i, a, apontados, n_coment, window))
                    .on_click(cx.listener(move |this, _, _, cx| this.ir_ao_arquivo(&caminho, cx)));
                if visto {
                    let tamanho = nome.len();
                    cartao = cartao.titulo_rico(StyledText::new(SharedString::from(nome)).with_highlights([(
                        0..tamanho,
                        HighlightStyle { color: Some(Cores::tenue()), ..Default::default() },
                    )]));
                }
                if !so_leitura {
                    let descartar = a.path().to_string();
                    cartao = cartao.acao(
                        Button::new(("descartar-lateral", i))
                            .custom(variante_perigo_calma(cx))
                            .h(r(24.))
                            .w(r(24.))
                            .rounded(raio::PILULA)
                            .disabled(self.enviando)
                            .child(icone(Lucide::Undo2, 14., window))
                            .tooltip("Descartar as mudanças deste arquivo")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.confirmar_descarte(descartar.clone(), window, cx)
                            })),
                    );
                }
                grupos.push(cartao.into_any_element());
            }
        }
        let sem_resultado = grupos.is_empty() && !filtro.is_empty();
        v_flex()
            .id("lateral-revisao")
            .w(r(largura))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .child(CabecalhoDeSecao::new("Arquivos").contagem(self.arquivos.len()).primeira(true))
            .when(com_filtro, |s| {
                s.child(div().px(r(2.)).pb(r(6.)).child(campo_de_busca("filtro-arquivos", &self.filtro, None, window, cx)))
            })
            .children(grupos)
            .when(sem_resultado, |s| {
                s.child(
                    div()
                        .px(r(8.))
                        .metadado()
                        .text_color(Cores::tenue())
                        .child(format!("Nenhum arquivo com “{}”", self.filtro.read(cx).value())),
                )
            })
            .child(self.secao_comentarios(window, cx))
    }

    fn secao_comentarios(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let problemas = self.problemas(cx);
        let atalho = self
            .estado()
            .and_then(|e| Kbd::binding_for_action_in(&AbrirRascunho, &e.read(cx).focus_handle(cx), window))
            .map(|kbd| desenho_teclas(kbd.partes()));
        v_flex()
            .child(CabecalhoDeSecao::new("Seus comentários").contagem(self.comentarios.len()))
            .when(self.comentarios.is_empty(), |s| {
                s.child(
                    v_flex()
                        .gap(r(6.))
                        .px(r(8.))
                        .max_w(r(220.))
                        .metadado()
                        .text_color(Cores::tenue())
                        .child("Clique numa linha e use o + para comentar. Arraste nos números para pegar várias linhas.")
                        .when_some(atalho, |s, teclas| {
                            s.child(h_flex().gap(r(6.)).child(teclas).child("comenta as linhas escolhidas"))
                        }),
                )
            })
            .children(self.comentarios.iter().enumerate().map(|(i, c)| {
                let fora = self.arquivo(&c.arquivo).is_none();
                let onde = onde_na_lateral(c, fora);
                let gravidade = c.problema.and_then(|p| problemas.get(p)).map(|p| peso_da_gravidade(&p.gravidade));
                let (arquivo, linhas) = (c.arquivo.clone(), c.linhas);
                h_flex()
                    .id(("comentario-lateral", i))
                    .group("comentario-lateral")
                    .h(r(48.))
                    .px(r(8.))
                    .gap(esp::E8)
                    .rounded(raio::CONTROLE)
                    .cursor_pointer()
                    .hover(|s| s.bg(Cores::hover()))
                    .child(div().flex_none().text_color(Cores::tenue()).child(icone(Lucide::MessageSquare, 14., window)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(r(13.))
                                    .line_height(r(18.))
                                    .text_color(Cores::texto())
                                    .child(c.texto.clone()),
                            )
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap(r(6.))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis_start()
                                            .mono_pequeno()
                                            .text_color(Cores::tenue())
                                            .child(onde),
                                    )
                                    .when_some(gravidade, |s, g| {
                                        s.child(div().flex_none().child(Selo::ponto(cor_da_gravidade(g)).lado(6.)))
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap(r(2.))
                            .opacity(0.)
                            .group_hover("comentario-lateral", |s| s.opacity(1.))
                            .child(
                                botao_icone(("editar-lateral", i), Lucide::Pencil, "Editar", TamanhoBotao::Linha, window, cx)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.editar_comentario(i, window, cx)
                                    })),
                            )
                            .child(
                                botao_icone(
                                    ("apagar-lateral", i),
                                    Lucide::Trash,
                                    "Apagar o comentário",
                                    TamanhoBotao::Linha,
                                    window,
                                    cx,
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.remover_comentario(i, window, cx)
                                })),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let linha = linhas.map(|l| l.ancora().1);
                        if this.arquivo(&arquivo).is_some() {
                            this.ir_para(&arquivo, linha, cx);
                        }
                    }))
            }))
    }

    /// O diff do Kit com os problemas do revisor, os comentários e o rascunho como anotações nas
    /// linhas; no cabeçalho de cada arquivo, o Visto, o selo de gerado e o Descartar.
    fn corpo_do_diff(
        &self,
        estado: &Entity<DiffState>,
        problemas: &[Problema],
        largura_cartao: f32,
        d: Disposicao,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let mut anotacoes = Vec::new();
        let mut cartoes: HashMap<ElementId, Cartao> = HashMap::new();
        let mut anotar = |id: ElementId, caminho: SharedString, ancora: Option<(DiffSide, usize)>, cartao: Cartao| {
            anotacoes.push(match ancora {
                Some((lado, linha)) => DiffAnnotation::line(id.clone(), DiffLinePosition::new(caminho, lado, linha)),
                None => DiffAnnotation::file(id.clone(), caminho),
            });
            cartoes.insert(id, cartao);
        };
        let so_ler = matches!(self.fonte, FonteDiff::Pr { .. });
        for (i, p) in problemas.iter().enumerate() {
            if let Some((caminho, linha)) = self.ancorar(&p.arquivo, p.linha) {
                let usado = self.comentarios.iter().any(|c| c.problema == Some(i));
                let cartao = Cartao::Problema {
                    indice: i,
                    problema: p.clone(),
                    usado,
                    so_ler,
                };
                anotar(("problema", i).into(), caminho, linha.map(|l| (DiffSide::Modified, l)), cartao);
            }
        }
        let editando = self.rascunho.as_ref().and_then(|r| r.editando);
        for (i, c) in self.comentarios.iter().enumerate() {
            if editando != Some(i)
                && let Some(arquivo) = self.arquivo(&c.arquivo)
            {
                let gravidade = c.problema.and_then(|p| problemas.get(p)).map(|p| match p.gravidade.strip_prefix(DA_REVISAO_DO_PR) {
                    Some(autor) => format!("{DA_REVISAO_DO_PR}{autor}"),
                    None => nome_da_gravidade(peso_da_gravidade(&p.gravidade)).to_string(),
                });
                let cartao = Cartao::Comentario {
                    indice: i,
                    texto: c.texto.clone(),
                    onde: onde_rotulo(c.linhas),
                    gravidade,
                };
                anotar(("comentario", i).into(), arquivo.path().clone(), c.linhas.map(|l| l.ancora()), cartao);
            }
        }
        if let Some(r) = &self.rascunho
            && let Some(arquivo) = self.arquivo(&r.arquivo)
        {
            let cartao = Cartao::Rascunho {
                rotulo: rotulo_rascunho(r),
                editando: r.editando.is_some(),
            };
            anotar(("rascunho", 0usize).into(), arquivo.path().clone(), r.linhas.map(|l| l.ancora()), cartao);
        }

        let fraca = cx.entity().downgrade();
        let cartoes = Rc::new(cartoes);
        let campo = self.campo.clone();
        let mut por_arquivo: HashMap<String, usize> = HashMap::new();
        for c in &self.comentarios {
            *por_arquivo.entry(c.arquivo.clone()).or_default() += 1;
        }
        let por_arquivo = Rc::new(por_arquivo);
        let vistos = Rc::new(self.vistos.clone());
        let gerados = Rc::new(self.gerados.clone());
        let so_leitura = self.so_leitura(cx);
        let enviando = self.enviando;
        let curto = d.rotulos != Rotulos::Longos;
        let (no_visto, no_sufixo, no_clique, no_mais, nos_cartoes) =
            (fraca.clone(), fraca.clone(), fraca.clone(), fraca.clone(), fraca);
        let estado_clique = estado.clone();
        Diff::new(estado)
            .flex_1()
            .min_h_0()
            .bg(transparent_black())
            .border_0()
            .syntax_highlight(true)
            .soft_wrap(self.quebrar)
            .change_indicator(DiffChangeIndicator::Bars)
            .hunk_separator(DiffHunkSeparator::LineInfo)
            // Só a calha: pintar a linha toda troca o verde ou o vermelho pelo azul do hover.
            .hover_highlight(DiffHoverHighlight::LineNumber)
            .annotations(anotacoes)
            .render_annotation(move |anotacao, window, cx| match cartoes.get(anotacao.id()) {
                Some(cartao) => desenhar_cartao(cartao, &nos_cartoes, &campo, largura_cartao, window, cx),
                None => div().into_any_element(),
            })
            .render_header_prefix(move |arquivo, _, _| {
                let caminho = arquivo.path().clone();
                let visto = vistos.contains(&caminho);
                let fraca = no_visto.clone();
                div().metadado().text_color(Cores::tenue()).child(
                    Checkbox::new(SharedString::from(format!("visto-{caminho}")))
                        .label("Visto")
                        .checked(visto)
                        .tooltip("Marca como visto e recolhe o arquivo")
                        .on_click(move |marcado, _, cx| {
                            let caminho = caminho.clone();
                            let _ = fraca.update(cx, |this, cx| this.marcar_visto(caminho, *marcado, cx));
                        }),
                )
            })
            .render_header_title_suffix(move |arquivo, _, _| {
                div().when(gerados.contains(arquivo.path()), |s| s.child(Selo::neutro("gerado")))
            })
            .render_header_suffix(move |arquivo, window, cx| {
                let caminho = arquivo.path().to_string();
                let n = por_arquivo.get(&caminho).copied().unwrap_or(0);
                let fraca = no_sufixo.clone();
                let dica = contagem(n, "comentário seu", "comentários seus");
                h_flex()
                    .gap(r(8.))
                    .when(n > 0, |s| {
                        s.child(
                            h_flex()
                                .id("comentarios-do-arquivo")
                                .gap(r(4.))
                                .text_color(Cores::tenue())
                                .child(icone(Lucide::MessageSquare, 12., window))
                                .child(div().metadado().child(n.to_string()))
                                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx)),
                        )
                    })
                    .when(!so_leitura, |s| {
                        let botao = Button::new("descartar-arquivo")
                            .custom(variante_perigo_calma(cx))
                            .h(r(24.))
                            .rounded(raio::PILULA)
                            .gap(r(6.))
                            .disabled(enviando)
                            .child(icone(Lucide::Undo2, 14., window))
                            .tooltip("Descartar as mudanças deste arquivo")
                            .on_click(move |_, window, cx| {
                                let _ = fraca.update(cx, |this, cx| this.confirmar_descarte(caminho.clone(), window, cx));
                            });
                        s.child(if curto {
                            botao.w(r(24.))
                        } else {
                            botao.px(r(8.)).child(div().rotulo().child("Descartar"))
                        })
                    })
            })
            .on_line_click(move |posicao, _, window, cx| {
                let faixa = DiffLineRange::new(posicao.path().clone(), posicao.side(), posicao.line(), posicao.line());
                let caminho = posicao.path().clone();
                let foco = estado_clique.read(cx).focus_handle(cx);
                estado_clique.update(cx, |estado, cx| estado.set_selected_lines(Some(faixa), cx));
                window.focus(&foco, cx);
                let _ = no_clique.update(cx, |this, cx| {
                    if let Some(i) = this.arquivos.iter().position(|a| *a.path() == caminho) {
                        this.atual = i;
                    }
                    cx.notify();
                });
            })
            .when(!so_leitura, |diff| {
                diff.on_add_annotation(move |faixa, window, cx| {
                    let linhas = Linhas::de_faixa(faixa);
                    let _ = no_mais.update(cx, |this, cx| {
                        this.abrir_rascunho(faixa.path().to_string(), Some(linhas), window, cx)
                    });
                })
            })
    }

    /// Spinner e ícone, texto e cor da linha da esquerda do rodapé.
    fn estado_do_rodape(&self, t: Option<&Tarefa>, cx: &App) -> (Option<Lucide>, String) {
        if let FonteDiff::Pr { numero, .. } = &self.fonte {
            return (Some(Lucide::Info), format!("O diff do PR #{numero} no GitHub, só para ler."));
        }
        let Some(t) = t else {
            return (Some(Lucide::Info), "Este chat não existe mais.".into());
        };
        match t.estado {
            EstadoTarefa::Aprovada => return (Some(Lucide::CircleCheck), "Juntada ao projeto.".into()),
            EstadoTarefa::Cancelada => {
                return (Some(Lucide::Info), "A cópia ficou guardada; nada vai ser juntado.".into());
            }
            _ => {}
        }
        if self.enviando {
            return (None, "Juntando as mudanças ao projeto…".into());
        }
        if let Some(recusa) = self.motor.read(cx).recusa_aprovar(self.tarefa) {
            let icone = if t.conferindo() { None } else { Some(Lucide::Info) };
            return (icone, recusa);
        }
        (Some(Lucide::MessageSquare), texto_do_rodape(self.vistos.len(), self.arquivos.len(), self.comentarios.len()))
    }

    fn rodape(&self, t: Option<&Tarefa>, d: Disposicao, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let estreito = d.rotulos == Rotulos::Icone;
        let lendo = matches!(self.carga, Carga::Lendo);
        let sem_copia = matches!(self.carga, Carga::SemCopia);
        let so_leitura = self.so_leitura(cx);
        let (icone_estado, texto) = self.estado_do_rodape(t, cx);
        let total = self.arquivos.len();
        let vistos = self.vistos.len();
        let com_anel = total > 0 && !so_leitura && icone_estado == Some(Lucide::MessageSquare);
        let marcador = if com_anel {
            Anel::new("vistos", vistos as f32 / total as f32)
                .diametro(20.)
                .cor(if vistos == total { Cores::verde() } else { Cores::destaque() })
                .into_any_element()
        } else {
            match icone_estado {
                None => Spinner::new()
                    .with_size(Size::Size(tam(window, 14.)))
                    .color(Cores::destaque())
                    .into_any_element(),
                Some(i) => div()
                    .flex_none()
                    .text_color(if i == Lucide::CircleCheck { Cores::verde() } else { Cores::tenue() })
                    .child(icone(i, 14., window))
                    .into_any_element(),
            }
        };
        let esquerda = h_flex()
            .id("rodape-revisao")
            .flex_1()
            .min_w_0()
            .gap(r(8.))
            .child(marcador)
            .when(!(estreito && com_anel), |s| {
                s.child(div().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(texto.clone()))
            })
            .tooltip({
                let dica = SharedString::from(texto);
                move |window, cx| Tooltip::new(dica.clone()).build(window, cx)
            });
        let fechar = botao_fantasma("fechar-revisao", None, "Fechar", TamanhoBotao::Controle, window, cx)
            .on_click(|_, window, cx| window.close_dialog(cx));
        if so_leitura || sem_copia {
            return h_flex().flex_none().h(r(32.)).mt(r(16.)).gap(r(8.)).child(esquerda).child(fechar);
        }
        let copia = t
            .filter(|t| t.worktree.is_some() && !t.copia_apagada())
            .map(|t| self.botao_copia(t.id, estreito, window, cx));
        // Com o que pesa contra aprovar, o azul passa para o Pedir ajuste, com o que vai junto no
        // selo; sem nada, fica no Aprovar.
        let ajuste_primeiro = t.is_some_and(|t| principal(&self.pendencias(t)) == Principal::Ajuste);
        let vai_junto = self.comentarios.len() + if self.incluir_revisor { self.nao_usados(cx).len() } else { 0 };
        let modo_pr = self.pr.as_ref();
        let rotulo_ajuste = if modo_pr.is_some() { "Enviar comentários ao chat" } else { "Pedir ajuste" };
        let ajuste_primeiro = ajuste_primeiro && modo_pr.is_none();
        let mut ajuste = if ajuste_primeiro {
            botao_primario("pedir-ajuste", None, rotulo_ajuste, TamanhoBotao::Controle, window, cx)
        } else {
            botao_fantasma("pedir-ajuste", None, rotulo_ajuste, TamanhoBotao::Controle, window, cx)
        }
        .disabled(self.enviando || lendo);
        if vai_junto > 0 {
            let selo = Selo::contagem(vai_junto);
            ajuste = ajuste.child(if ajuste_primeiro { selo.tingida(Cores::sobre_cor()) } else { selo });
        }
        if ajuste_primeiro && self.pop != Some(Pop::Ajuste) {
            ajuste = ajuste.tooltip_with_action("Pedir ajuste ao chat", &AcaoPrincipal, Some("Revisao"));
        }
        let ajuste = if self.enviando || lendo {
            ajuste.into_any_element()
        } else {
            let dados = Rc::new(self.dados_ajuste(t, cx));
            let (recado, fraca) = (self.recado.clone(), cx.entity().downgrade());
            self.com_popover(
                Pop::Ajuste,
                "pop-ajuste",
                Anchor::BottomRight,
                ajuste,
                move |window, cx| corpo_ajuste(&dados, &recado, &fraca, window, cx),
                cx,
            )
        };
        let recusa = self.motor.read(cx).recusa_aprovar(self.tarefa);
        if let Some(f) = modo_pr {
            let rotulo = match (f.enviando, f.atualizar.is_some()) {
                (true, false) => "Abrindo o PR…",
                (true, true) => "Atualizando o PR…",
                (false, false) => "Abrir PR",
                (false, true) => "Atualizar PR",
            };
            let motivo = recusa.map(|r| r.replace("para juntar", "para publicar").replace("o que juntar", "o que publicar")).or_else(|| self.bloqueio_do_pr(cx));
            let mut abrir = botao_primario("abrir-pr", Some(Lucide::GitPullRequest.into()), rotulo, TamanhoBotao::Controle, window, cx)
                .loading(f.enviando)
                .disabled(motivo.is_some() || lendo || f.enviando)
                .on_click(cx.listener(|this, _, window, cx| this.publicar_pr(window, cx)));
            abrir = match motivo {
                Some(m) => abrir.tooltip(m),
                None => abrir.tooltip_with_action(if f.atualizar.is_some() { "Mandar as mudanças para o PR" } else { "Abrir o PR no GitHub" }, &AcaoPrincipal, Some("Revisao")),
            };
            return h_flex().flex_none().h(r(32.)).mt(r(16.)).gap(r(8.)).child(esquerda).children(copia).child(ajuste).child(abrir);
        }
        let rotulo = match (self.enviando, self.de_chat(cx)) {
            (true, _) => "Juntando…",
            (false, true) => "Juntar",
            (false, false) => "Juntar",
        };
        let icone_aprovar = Some(Lucide::GitMerge.into());
        let mut aprovar = if ajuste_primeiro {
            botao_fantasma("aprovar-juntar", icone_aprovar, rotulo, TamanhoBotao::Controle, window, cx)
        } else {
            botao_primario("aprovar-juntar", icone_aprovar, rotulo, TamanhoBotao::Controle, window, cx)
        }
        .loading(self.enviando)
        .disabled(recusa.is_some() || lendo || self.enviando)
        .on_click(cx.listener(|this, _, window, cx| this.pedir_aprovacao(window, cx)));
        aprovar = match recusa {
            Some(motivo) => aprovar.tooltip(motivo),
            None if !ajuste_primeiro => {
                aprovar.tooltip_with_action("Juntar as mudanças ao projeto", &AcaoPrincipal, Some("Revisao"))
            }
            None => aprovar,
        };
        h_flex()
            .flex_none()
            .h(r(32.))
            .mt(r(16.))
            .gap(r(8.))
            .child(esquerda)
            .children(copia)
            .child(ajuste)
            .child(aprovar)
    }

    /// "Abrir a cópia": o menu da cópia (rodar, terminal, VS Code, copiar o caminho).
    fn botao_copia(&self, tarefa: u64, estreito: bool, window: &Window, cx: &Context<Self>) -> AnyElement {
        let botao = if estreito {
            botao_icone("abrir-copia", Lucide::GitBranch, "Abrir a cópia", TamanhoBotao::Controle, window, cx)
        } else {
            botao_fantasma("abrir-copia", Some(Lucide::GitBranch.into()), "Abrir a cópia", TamanhoBotao::Controle, window, cx)
                .child(div().flex_none().text_color(Cores::tenue()).child(icone(Lucide::ChevronDown, 12., window)))
        };
        let (janela, motor) = (self.janela.clone(), self.motor.clone());
        botao
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, window, cx| {
                menus::itens_da_copia(menu, janela.clone(), &motor, tarefa, true, window, cx)
            })
            .into_any_element()
    }

    fn dados_ajuste(&self, t: Option<&Tarefa>, cx: &App) -> DadosAjuste {
        let mut arquivos: Vec<(String, usize, Option<usize>)> = Vec::new();
        for (i, c) in self.comentarios.iter().enumerate() {
            match arquivos.iter_mut().find(|(a, _, _)| *a == c.arquivo) {
                Some((_, n, _)) => *n += 1,
                None => arquivos.push((c.arquivo.clone(), 1, Some(i))),
            }
        }
        let nao_usados = self.nao_usados(cx);
        let maior = nao_usados.iter().map(|p| peso_da_gravidade(&p.gravidade)).max();
        DadosAjuste {
            rodada: t.map_or(1, |t| t.rodada),
            arquivos,
            nao_usados: nao_usados.len(),
            maior,
            incluir: self.incluir_revisor,
            pode: self.pode_pedir_ajuste(cx),
        }
    }

    /// Carregando, sem cópia, erro e sem mudanças: o que vai no lugar do diff.
    fn corpo_vazio(&self, t: Option<&Tarefa>, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let vazio = match &self.carga {
            Carga::Lendo => {
                return Some(
                    h_flex()
                        .size_full()
                        .justify_center()
                        .gap(r(8.))
                        .corpo_ui()
                        .text_color(Cores::tenue())
                        .child(Spinner::new().with_size(Size::Size(tam(window, 20.))).color(Cores::destaque()))
                        .child("Lendo as mudanças…")
                        .into_any_element(),
                );
            }
            Carga::SemCopia => {
                let apagada = t.is_some_and(|t| t.copia_apagada());
                let aprovada = t.is_some_and(|t| t.estado == EstadoTarefa::Aprovada);
                let (titulo, texto) = match (apagada, aprovada) {
                    (_, true) => ("A cópia deste chat já foi apagada", "Juntado: as mudanças já estão no projeto."),
                    (true, false) => ("A cópia deste chat já foi apagada", "Sem cópia isolada, não há o que comparar."),
                    (false, false) => (
                        "Este chat não tem cópia isolada",
                        "Sem cópia isolada, não há o que comparar: ele mexe direto na pasta do projeto.",
                    ),
                };
                EstadoVazio::new(Lucide::FolderX, titulo).texto(texto)
            }
            Carga::Erro(erro) => EstadoVazio::new(Lucide::TriangleAlert, "Não deu para ler o diff")
                .texto(erro.clone())
                .acao(
                    botao_fantasma("ler-de-novo", Some(Lucide::RotateCw.into()), "Tentar de novo", TamanhoBotao::Controle, window, cx)
                        .on_click(cx.listener(|this, _, _, cx| this.recarregar(cx))),
                ),
            Carga::Pronta(_) if self.arquivos.is_empty() => {
                EstadoVazio::new(Lucide::FileCheck, "Nenhuma mudança").texto("A cópia está igual ao projeto.")
            }
            Carga::Pronta(_) => return None,
        };
        Some(div().size_full().flex().items_center().justify_center().child(vazio).into_any_element())
    }

    /// O aviso que ficou (junção com conflito ou erro) e a entrega nova que chegou com o diálogo
    /// aberto.
    fn alertas(&self, t: Option<&Tarefa>, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        let mut lista = Vec::new();
        if let Some((tom, texto)) = &self.aviso {
            let titulo = match tom {
                Tom::Atencao => "Conflito na junção: nada foi juntado",
                _ => "Não deu",
            };
            let fraca = cx.entity().downgrade();
            lista.push(
                Alerta::new(*tom, texto.clone())
                    .titulo(titulo)
                    .fechar(move |_, cx| {
                        let _ = fraca.update(cx, |this, cx| {
                            this.aviso = None;
                            cx.notify();
                        });
                    })
                    .into_any_element(),
            );
        }
        let nova = t
            .filter(|_| !matches!(self.carga, Carga::Lendo))
            .and_then(|t| entrega_nova(self.lida, t.rodada, t.estado));
        if let Some(rodada) = nova {
            lista.push(
                Alerta::new(Tom::Info, format!("Chegou uma entrega nova (rodada {rodada})."))
                    .acao(
                        botao_fantasma("recarregar", Some(Lucide::RotateCw.into()), "Recarregar", TamanhoBotao::Barra, window, cx)
                            .on_click(cx.listener(|this, _, _, cx| this.recarregar(cx))),
                    )
                    .into_any_element(),
            );
        }
        lista
    }
}

impl Render for RevisaoDiff {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let escala = window.rem_size().as_f32() / 15.;
        let conteudo = (largura_do_dialogo(window) - px(32.)).as_f32() / escala;
        let d = disposicao(conteudo, self.lado_a_lado);
        let lado_a_lado = self.lado_a_lado && d.lado_a_lado_ok;
        if let Some(estado) = self.estado() {
            let modo = if lado_a_lado { DiffMode::Split } else { DiffMode::Unified };
            estado.update(cx, |estado, cx| estado.set_mode(modo, cx));
        }
        if !self.focada {
            self.focada = true;
            match self.estado() {
                Some(estado) => {
                    let foco = estado.read(cx).focus_handle(cx);
                    window.focus(&foco, cx);
                }
                None => self.foco_teclado.focus(window, cx),
            }
        }
        let t = self.motor.read(cx).tarefa(self.tarefa).cloned();
        let projeto = t.as_ref().map(|t| self.motor.read(cx).nome_do_projeto(&t.projeto)).unwrap_or_default();
        let problemas = self.problemas(cx);
        let com_lateral = match &self.carga {
            Carga::Lendo => d.lateral,
            Carga::Pronta(_) if !self.arquivos.is_empty() => d.lateral,
            _ => None,
        };
        let largura_diff = conteudo - com_lateral.map_or(0., |l| l + 12.);
        let largura_cartao = (largura_diff - 48.).min(680.);
        let corpo = match self.corpo_vazio(t.as_ref(), window, cx) {
            Some(vazio) => vazio,
            None => {
                let estado = self.estado().expect("diff pronto");
                self.corpo_do_diff(&estado, &problemas, largura_cartao, d, cx).into_any_element()
            }
        };
        let focado = self.estado().is_some_and(|e| e.read(cx).focus_handle(cx).is_focused(window));
        let lateral = com_lateral.map(|largura| match self.carga {
            Carga::Lendo => v_flex()
                .w(r(largura))
                .flex_none()
                .gap(r(4.))
                .child(CabecalhoDeSecao::new("Arquivos").primeira(true))
                .children((0..3).map(|_| div().h(r(32.)).mx(r(2.)).rounded(raio::CONTROLE).bg(Cores::hover())))
                .into_any_element(),
            _ => self.lateral(largura, &projeto, window, cx).into_any_element(),
        });
        let alertas = self.alertas(t.as_ref(), window, cx);
        v_flex()
            .id("revisao")
            .track_focus(&self.foco_teclado)
            .key_context("Revisao")
            .on_action(cx.listener(|this, _: &MudancaSeguinte, _, cx| this.pular_mudanca(true, cx)))
            .on_action(cx.listener(|this, _: &MudancaAnterior, _, cx| this.pular_mudanca(false, cx)))
            .on_action(cx.listener(|this, _: &ArquivoSeguinte, _, cx| this.pular_arquivo(true, cx)))
            .on_action(cx.listener(|this, _: &ArquivoAnterior, _, cx| this.pular_arquivo(false, cx)))
            .on_action(cx.listener(|this, _: &AbrirRascunho, window, cx| this.abrir_rascunho_na_selecao(window, cx)))
            .on_action(cx.listener(|this, _: &AcaoPrincipal, window, cx| this.acao_principal(window, cx)))
            .h(self.altura(window))
            .when(!alertas.is_empty(), |s| s.child(v_flex().flex_none().gap(r(8.)).mb(r(12.)).children(alertas)))
            .child(self.barra(t.as_ref(), d, window, cx))
            .child(
                h_flex()
                    .mt(r(12.))
                    .flex_1()
                    .min_h_0()
                    .gap(r(12.))
                    .items_start()
                    .children(lateral)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .py(r(12.))
                            .rounded(raio::CARTAO)
                            .bg(Cores::preto())
                            .border_1()
                            .border_color(if focado { Cores::destaque_borda() } else { transparent_black() })
                            .child(corpo),
                    ),
            )
            .children(self.formulario_pr(d, window, cx))
            .child(self.rodape(t.as_ref(), d, window, cx))
    }
}

/// Marca, na gravidade, o apontamento que veio da revisão do PR (com o autor depois).
const DA_REVISAO_DO_PR: &str = "pr:";

/// O que o formulário do PR guarda enquanto o diálogo está aberto.
struct FormularioPr {
    /// O número do PR aberto ("Revisar e atualizar PR"); `None` abre um PR novo.
    atualizar: Option<u64>,
    titulo: Entity<InputState>,
    descricao: Entity<TextareaState>,
    /// No "Atualizar PR", o título e a descrição só vão com "Atualizar a descrição".
    atualizar_descricao: bool,
    rascunho: bool,
    /// "Mesclar quando ficar pronto" (o Ship), pelo método padrão do repositório.
    ship: bool,
    metodo: String,
    segredos_ok: bool,
    /// `None` enquanto lê.
    achados: Option<Vec<git::Achado>>,
    novos: Option<Vec<String>>,
    enviando: bool,
}

/// O que o popover do revisor mostra.
struct DadosRevisor {
    revisao: Revisao,
    valem: bool,
    rodada: u32,
    usados: Vec<usize>,
    /// Se cada problema achou lugar no diff.
    ancorados: Vec<bool>,
    altura: Pixels,
}

/// O que o popover de "Pedir ajuste" mostra.
struct DadosAjuste {
    rodada: u32,
    /// Arquivo, quantos comentários e o primeiro deles.
    arquivos: Vec<(String, usize, Option<usize>)>,
    nao_usados: usize,
    maior: Option<u8>,
    incluir: bool,
    pode: bool,
}

fn copiar_grupos(grupos: &[GrupoDeEscolha]) -> Vec<GrupoDeEscolha> {
    grupos
        .iter()
        .map(|g| GrupoDeEscolha {
            titulo: g.titulo.clone(),
            itens: g
                .itens
                .iter()
                .map(|i| ItemDeEscolha {
                    chave: i.chave.clone(),
                    simbolo: i.simbolo.as_ref().and_then(Simbolo::copia),
                    titulo: i.titulo.clone(),
                    descricao: i.descricao.clone(),
                    selos: i.selos.clone(),
                    desabilitado: i.desabilitado.clone(),
                    termos: i.termos.clone(),
                })
                .collect(),
        })
        .collect()
}

fn nome_do_grupo(pasta: &Pasta, projeto: &str) -> String {
    match pasta {
        Pasta::Raiz if projeto.is_empty() => "Raiz do projeto".into(),
        Pasta::Raiz => projeto.to_string(),
        Pasta::Em(p) => p.clone(),
        Pasta::Gerados => "Gerados".into(),
    }
}

/// O ponto da maior gravidade, os comentários e o +N −M da linha de arquivo da lateral.
fn meta_do_arquivo(i: usize, a: &DiffFile, apontados: Option<(usize, u8)>, comentarios: usize, window: &Window) -> AnyElement {
    let ponto = apontados.map(|(n, peso)| {
        let altas = if peso == 2 { " (com gravidade alta)" } else { "" };
        let dica: SharedString = format!("{} do revisor{altas}", contagem(n, "apontamento", "apontamentos")).into();
        div()
            .id(("ponto-arquivo", i))
            .child(Selo::ponto(cor_da_gravidade(peso)))
            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
    });
    let comentarios = (comentarios > 0).then(|| {
        let dica: SharedString = contagem(comentarios, "comentário seu", "comentários seus").into();
        h_flex()
            .id(("comentarios-arquivo", i))
            .gap(r(2.))
            .child(icone(Lucide::MessageSquare, 12., window))
            .child(comentarios.to_string())
            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
    });
    h_flex()
        .gap(r(8.))
        .children(ponto)
        .children(comentarios)
        .child(div().mono_pequeno().child(format!("+{} −{}", a.additions(), a.deletions())))
        .into_any_element()
}

/// "precos.py:12", "precos.py · linhas 4 a 9", "src/x.rs · fora do diff".
fn onde_na_lateral(c: &Comentario, fora: bool) -> String {
    if fora {
        return format!("{} · fora do diff", c.arquivo);
    }
    let nome = nome_do_arquivo(&c.arquivo);
    match c.linhas {
        Some(l) if l.lado == l.lado_fim && l.inicio == l.fim && l.lado == DiffSide::Modified => {
            format!("{nome}:{}", l.inicio)
        }
        linhas => format!("{nome} · {}", onde_rotulo(linhas)),
    }
}

/// "linha 12", "linhas 4 a 9", "linha 2 removida", "o arquivo".
fn onde_rotulo(linhas: Option<Linhas>) -> String {
    match linhas {
        None => "o arquivo".into(),
        Some(l) => onde_curto(l).trim_start_matches("na ").trim_start_matches("nas ").to_string(),
    }
}

fn corpo_verificacao(v: &Verificacao, rodada: u32, valem: bool) -> AnyElement {
    let (icone_v, cor) = if v.passou { (Lucide::CircleCheck, Cores::verde()) } else { (Lucide::CircleX, Cores::vermelho()) };
    let mut dica = v.tempo().map(|t| format!("{t} · ")).unwrap_or_default();
    dica.push_str(&if valem { format!("rodada {rodada}") } else { "rodada anterior".to_string() });
    let saida = if v.saida.trim().is_empty() { "(sem saída)".to_string() } else { v.saida.trim_end().to_string() };
    CorpoPopover::new(LarguraPopover::P560)
        .cabecalho(CabecalhoPopover::Escolha {
            simbolo: Some(Simbolo::icone_cor(icone_v, cor)),
            titulo: "Verificação".into(),
            valor: Some((if v.passou { "passou" } else { "falhou" }.into(), cor)),
            dica: Some(dica.into()),
        })
        .when(!valem, |c| {
            c.child(Alerta::new(Tom::Info, "Esta saída é da rodada anterior: a verificação roda de novo na próxima entrega.").compacto())
        })
        .child(
            CaixaDeCodigo::new("saida-verificacao", saida.into())
                .comando(v.comando.clone())
                .when_some(v.tempo(), |c, t| c.tempo(t))
                .erro(!v.passou),
        )
        .into_any_element()
}

fn corpo_revisor(dados: &DadosRevisor, fraca: &WeakEntity<RevisaoDiff>, window: &Window, cx: &App) -> AnyElement {
    let parecer = &dados.revisao;
    let aprovaria = parecer.veredito == "aprovar";
    let maior = parecer.problemas.iter().map(|p| peso_da_gravidade(&p.gravidade)).max().unwrap_or(1);
    let cor = if aprovaria { Cores::verde() } else if maior == 2 { Cores::vermelho() } else { Cores::ambar() };
    let dica = if dados.valem { format!("rodada {}", dados.rodada) } else { "da rodada anterior".into() };
    let linhas = parecer.problemas.iter().enumerate().map(|(i, p)| {
        let usado = dados.usados.contains(&i);
        let ancorado = dados.ancorados.get(i).copied().unwrap_or(false);
        let onde = match (ancorado, p.linha) {
            (false, _) => format!("{} · fora do diff", p.arquivo),
            (true, Some(l)) => format!("{}:{l}", p.arquivo),
            (true, None) => p.arquivo.clone(),
        };
        let descricao: SharedString = p.descricao.trim().to_string().into();
        let dica_descricao = descricao.clone();
        let (arquivo, linha) = (p.arquivo.clone(), p.linha);
        let (ir, usar) = (fraca.clone(), fraca.clone());
        h_flex()
            .id(("apontamento", i))
            .min_h(r(48.))
            .py(r(8.))
            .px(esp::E8)
            .gap(esp::E8)
            .rounded(raio::CONTROLE)
            .cursor_pointer()
            .hover(|s| s.bg(Cores::hover()))
            .child(div().flex_none().child(selo_gravidade(&p.gravidade)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .id(("apontamento-texto", i))
                            .truncate()
                            .corpo_ui()
                            .text_color(Cores::texto())
                            .child(descricao)
                            .tooltip(move |window, cx| Tooltip::new(dica_descricao.clone()).build(window, cx)),
                    )
                    .child(
                        div()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_start()
                            .mono_pequeno()
                            .text_color(Cores::tenue())
                            .child(onde),
                    ),
            )
            .child(if usado {
                h_flex()
                    .flex_none()
                    .gap(r(4.))
                    .metadado()
                    .text_color(Cores::tenue())
                    .child(icone(Lucide::Check, 14., window))
                    .child("virou comentário")
                    .into_any_element()
            } else {
                botao_fantasma(("usar-apontamento", i), Some(Lucide::Plus.into()), "Usar", TamanhoBotao::Linha, window, cx)
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        let _ = usar.update(cx, |this, cx| this.usar_problema(i, true, cx));
                    })
                    .into_any_element()
            })
            .on_click(move |_, _, cx| {
                let _ = ir.update(cx, |this, cx| {
                    this.pop = None;
                    this.focar(&arquivo, linha, cx);
                    cx.notify();
                });
            })
    });
    CorpoPopover::new(LarguraPopover::P560)
        .cabecalho(CabecalhoPopover::Escolha {
            simbolo: Some(Simbolo::icone_cor(Lucide::Eye, if dados.valem { cor } else { Cores::tenue() })),
            titulo: "Revisor independente".into(),
            valor: Some((if aprovaria { "aprovaria" } else { "pede ajustes" }.into(), cor)),
            dica: Some(dica.into()),
        })
        .child(
            v_flex()
                .id("parecer-revisor")
                .max_h(dados.altura)
                .overflow_y_scroll()
                .gap(r(8.))
                .when(!parecer.resumo.trim().is_empty(), |s| {
                    s.child(
                        div()
                            .corpo_ui()
                            .text_color(Cores::texto())
                            .line_clamp(5)
                            .text_ellipsis()
                            .child(parecer.resumo.trim().to_string()),
                    )
                })
                .when(parecer.problemas.is_empty(), |s| {
                    s.child(div().metadado().text_color(Cores::tenue()).child("Nenhum apontamento."))
                })
                .when(!parecer.problemas.is_empty(), |s| {
                    s.child(
                        h_flex()
                            .gap(r(6.))
                            .child(div().rotulo_secao().child("Apontamentos"))
                            .child(Selo::contagem(parecer.problemas.len())),
                    )
                    .child(v_flex().mx(r(-8.)).children(linhas))
                }),
        )
        .into_any_element()
}

fn corpo_ajuste(
    dados: &DadosAjuste,
    recado: &Entity<TextareaState>,
    fraca: &WeakEntity<RevisaoDiff>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let descricao = match (dados.nao_usados, dados.maior) {
        (n, Some(peso)) => format!(
            "{} do revisor (a maior, {})",
            contagem(n, "apontamento", "apontamentos"),
            nome_da_gravidade(peso)
        ),
        (n, None) => contagem(n, "apontamento", "apontamentos"),
    };
    let linhas = dados.arquivos.iter().enumerate().map(|(i, (arquivo, n, primeiro))| {
        let (ir, primeiro) = (fraca.clone(), *primeiro);
        h_flex()
            .id(("vai-junto", i))
            .h(r(28.))
            .px(r(8.))
            .gap(r(8.))
            .rounded(raio::CONTROLE)
            .cursor_pointer()
            .hover(|s| s.bg(Cores::hover()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(r(13.))
                    .text_color(Cores::texto())
                    .child(nome_do_arquivo(arquivo).to_string()),
            )
            .child(div().flex_none().metadado().text_color(Cores::tenue()).child(contagem(*n, "comentário", "comentários")))
            .on_click(move |_, _, cx| {
                let _ = ir.update(cx, |this, cx| {
                    if let Some(c) = primeiro.and_then(|p| this.comentarios.get(p)).cloned() {
                        this.pop = None;
                        let linha = c.linhas.map(|l| l.ancora().1);
                        if this.arquivo(&c.arquivo).is_some() {
                            this.ir_para(&c.arquivo, linha, cx);
                        }
                    }
                    cx.notify();
                });
            })
    });
    let (alternar, cancelar, enviar) = (fraca.clone(), fraca.clone(), fraca.clone());
    CorpoPopover::new(LarguraPopover::P400)
        .cabecalho(CabecalhoPopover::Escolha {
            simbolo: Some(Simbolo::icone(Lucide::MessageSquare)),
            titulo: "Pedir ajuste".into(),
            valor: Some((format!("rodada {}", dados.rodada + 1).into(), Cores::destaque_texto())),
            dica: Some("volta ao chat".into()),
        })
        .when(!dados.arquivos.is_empty(), |c| {
            c.child(
                v_flex()
                    .gap(r(2.))
                    .child(div().rotulo_secao().pb(r(2.)).child("Vão junto"))
                    .child(v_flex().mx(r(-8.)).children(linhas)),
            )
        })
        .child(
            div()
                .px(esp::CAIXA)
                .py(r(6.))
                .rounded(raio::CONTROLE)
                .bg(Cores::recuo_no_flutuante())
                .text_size(r(14.))
                .text_color(Cores::texto())
                .child(Textarea::new(recado).appearance(false)),
        )
        .when(dados.nao_usados > 0, |c| {
            c.child(div().h(px(1.)).bg(Cores::borda())).child(
                div().mx(r(-12.)).child(
                    LinhaDeOpcao::new("incluir-apontamentos", "Incluir os apontamentos que você não usou")
                        .descricao(descricao)
                        .interruptor(dados.incluir, move |ligado, _, cx| {
                            let _ = alternar.update(cx, |this, cx| {
                                this.incluir_revisor = ligado;
                                cx.notify();
                            });
                        }),
                ),
            )
        })
        .rodape(
            h_flex()
                .justify_end()
                .gap(r(8.))
                .child(
                    botao_fantasma("ajuste-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx).on_click(
                        move |_, _, cx| {
                            let _ = cancelar.update(cx, |this, cx| this.mudar_pop(Pop::Ajuste, false, cx));
                        },
                    ),
                )
                .child(
                    botao_primario("ajuste-enviar", None, "Enviar ao chat", TamanhoBotao::Controle, window, cx)
                        .disabled(!dados.pode)
                        .when(!dados.pode, |b| b.tooltip("Escreva um recado, comente uma linha ou inclua os apontamentos"))
                        .on_click(move |_, window, cx| {
                            let _ = enviar.update(cx, |this, cx| this.pedir_ajuste(window, cx));
                        }),
                ),
        )
        .into_any_element()
}

fn desenhar_cartao(
    cartao: &Cartao,
    fraca: &WeakEntity<RevisaoDiff>,
    campo: &Entity<TextareaState>,
    largura: f32,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let fraca = fraca.clone();
    // Largura limitada: a anotação ocupa a largura da linha mais longa, que pode passar da tela.
    let caixa = |fundo: Hsla, barra: Option<Hsla>| {
        v_flex()
            .relative()
            .max_w(r(largura.max(240.)))
            .my(r(4.))
            .py(esp::CAIXA)
            .px(r(12.))
            .gap(r(6.))
            .rounded(raio::CARTAO)
            .bg(fundo)
            .when_some(barra, |s, cor| {
                s.child(div().absolute().left(r(4.)).top(r(8.)).bottom(r(8.)).w(px(3.)).rounded_full().bg(cor))
            })
    };
    match cartao.clone() {
        // Um comentário em linha da revisão do PR, no molde do apontamento do revisor.
        Cartao::Problema { indice, problema, usado, so_ler } if problema.gravidade.starts_with(DA_REVISAO_DO_PR) => {
            let autor = problema.gravidade[DA_REVISAO_DO_PR.len()..].to_string();
            let cor = Cores::destaque();
            let cabecalho = h_flex()
                .gap(r(6.))
                .child(div().text_color(if usado { Cores::tenue() } else { Cores::destaque_texto() }).child(icone(Lucide::GitPullRequest, 14., window)))
                .child(div().rotulo_secao().child("A revisão do PR apontou"))
                .child(div().metadado().text_color(Cores::tenue()).child(format!("@{autor}")));
            if usado {
                return cabecalho
                    .my(r(4.))
                    .px(r(12.))
                    .child(div().metadado().text_color(Cores::tenue()).child("· virou seu comentário"))
                    .into_any_element();
            }
            caixa(Cores::tinta(cor), Some(cor))
                .child(cabecalho)
                .child(div().corpo_ui().text_color(Cores::texto()).child(problema.descricao))
                .when(!so_ler, |s| s.child(
                    h_flex().child(
                        botao_fantasma(("usar-problema", indice), Some(Lucide::Plus.into()), "Usar", TamanhoBotao::Barra, window, cx)
                            .tooltip("Vira seu comentário nesta linha, que vai ao chat com os outros")
                            .on_click(move |_, _, cx| {
                                let _ = fraca.update(cx, |this, cx| this.usar_problema(indice, false, cx));
                            }),
                    ),
                ))
                .into_any_element()
        }
        Cartao::Problema { indice, problema, usado, .. } => {
            let peso = peso_da_gravidade(&problema.gravidade);
            let cor = cor_da_gravidade(peso);
            let cabecalho = h_flex()
                .gap(r(6.))
                .child(div().text_color(if usado { Cores::tenue() } else { cor }).child(icone(Lucide::Eye, 14., window)))
                .child(div().rotulo_secao().child("Revisor"))
                .child(selo_gravidade(&problema.gravidade));
            if usado {
                return cabecalho
                    .my(r(4.))
                    .px(r(12.))
                    .child(div().metadado().text_color(Cores::tenue()).child("· virou seu comentário"))
                    .into_any_element();
            }
            let fundo = if peso == 0 { Cores::hover() } else { Cores::tinta(cor) };
            caixa(fundo, Some(cor))
                .child(cabecalho)
                .child(div().corpo_ui().text_color(Cores::texto()).child(problema.descricao))
                .child(
                    h_flex().child(
                        botao_fantasma(
                            ("usar-problema", indice),
                            Some(Lucide::Plus.into()),
                            "Usar como comentário",
                            TamanhoBotao::Barra,
                            window,
                            cx,
                        )
                        .on_click(move |_, _, cx| {
                            let _ = fraca.update(cx, |this, cx| this.usar_problema(indice, false, cx));
                        }),
                    ),
                )
                .into_any_element()
        }
        Cartao::Comentario { indice, texto, onde, gravidade } => {
            let apagar = fraca.clone();
            let rotulo = match gravidade {
                Some(g) => match g.strip_prefix(DA_REVISAO_DO_PR) {
                    Some(autor) => format!("Seu comentário · {onde} · da revisão do PR, @{autor}"),
                    None => format!("Seu comentário · {onde} · do revisor, {g}"),
                },
                None => format!("Seu comentário · {onde}"),
            };
            caixa(Cores::cartao_elevado(), Some(Cores::borda_forte()))
                .id(("cartao-comentario", indice))
                .group("cartao-comentario")
                .border_1()
                .border_color(Cores::borda())
                .child(
                    h_flex()
                        .gap(r(6.))
                        .child(div().text_color(Cores::tenue()).child(icone(Lucide::MessageSquare, 14., window)))
                        .child(div().flex_1().min_w_0().truncate().rotulo_secao().child(rotulo))
                        .child(
                            h_flex()
                                .flex_none()
                                .gap(r(2.))
                                .opacity(0.)
                                .group_hover("cartao-comentario", |s| s.opacity(1.))
                                .child(
                                    botao_fantasma("editar", Some(Lucide::Pencil.into()), "Editar", TamanhoBotao::Linha, window, cx)
                                        .on_click(move |_, window, cx| {
                                            let _ = fraca.update(cx, |this, cx| this.editar_comentario(indice, window, cx));
                                        }),
                                )
                                .child(
                                    botao_icone("apagar", Lucide::Trash, "Apagar o comentário", TamanhoBotao::Linha, window, cx)
                                        .on_click(move |_, window, cx| {
                                            let _ = apagar.update(cx, |this, cx| this.remover_comentario(indice, window, cx));
                                        }),
                                ),
                        ),
                )
                .child(div().corpo_ui().text_color(Cores::texto()).child(texto))
                .into_any_element()
        }
        Cartao::Rascunho { rotulo, editando } => {
            let (cancelar, salvar, esc) = (fraca.clone(), fraca.clone(), fraca);
            let vazio = campo.read(cx).value().trim().is_empty();
            let tecla = |t: &'static str| desenho_teclas(vec![t.into()]);
            caixa(Cores::cartao_elevado(), None)
                .border_1()
                .border_color(Cores::destaque_borda())
                .shadow(sombra_elevado())
                .on_action(move |_: &Cancel, window, cx| {
                    let _ = esc.update(cx, |this, cx| this.cancelar_rascunho(window, cx));
                })
                .child(div().rotulo_secao().child(rotulo))
                .child(
                    div()
                        .px(esp::CAIXA)
                        .py(r(6.))
                        .rounded(raio::CONTROLE)
                        .bg(Cores::recuo())
                        .text_size(r(14.))
                        .text_color(Cores::texto())
                        .child(Textarea::new(campo).appearance(false)),
                )
                .child(
                    h_flex()
                        .gap(r(8.))
                        .child(
                            h_flex()
                                .flex_1()
                                .min_w_0()
                                .gap(r(4.))
                                .metadado()
                                .text_color(Cores::tenue())
                                .child(tecla("Enter"))
                                .child("salva ·")
                                .child(tecla("Shift"))
                                .child(tecla("Enter"))
                                .child("quebra ·")
                                .child(tecla("Esc"))
                                .child("cancela"),
                        )
                        .child(
                            botao_fantasma("cancelar-comentario", None, "Cancelar", TamanhoBotao::Barra, window, cx)
                                .on_click(move |_, window, cx| {
                                    let _ = cancelar.update(cx, |this, cx| this.cancelar_rascunho(window, cx));
                                }),
                        )
                        .child(
                            botao_primario(
                                "salvar-comentario",
                                None,
                                if editando { "Salvar" } else { "Comentar" },
                                TamanhoBotao::Barra,
                                window,
                                cx,
                            )
                            .disabled(vazio)
                            .on_click(move |_, window, cx| {
                                let _ = salvar.update(cx, |this, cx| this.salvar_comentario(window, cx));
                            }),
                        ),
                )
                .into_any_element()
        }
    }
}

fn rotulo_rascunho(r: &Rascunho) -> String {
    let base = if r.editando.is_some() { "Editando o comentário" } else { "Comentário" };
    match r.linhas {
        None => format!("{base} sobre o arquivo"),
        Some(l) => format!("{base} {}", onde_curto(l)),
    }
}

fn onde_curto(l: Linhas) -> String {
    if l.lado != l.lado_fim {
        return "num trecho com linhas removidas e novas".to_string();
    }
    let removida = l.lado == DiffSide::Original;
    match (l.inicio == l.fim, removida) {
        (true, false) => format!("na linha {}", l.inicio),
        (true, true) => format!("na linha {} removida", l.inicio),
        (false, false) => format!("nas linhas {} a {}", l.inicio, l.fim),
        (false, true) => format!("nas linhas {} a {} removidas", l.inicio, l.fim),
    }
}

/// Uma linha do texto que vai ao trabalhador. Linha removida leva o número da versão antiga e
/// diz isso, para não ser confundida com a linha de mesmo número no arquivo novo.
fn descrever(c: &Comentario) -> String {
    let onde = match c.linhas {
        None => c.arquivo.clone(),
        Some(l) if l.lado != l.lado_fim => {
            let (antiga, nova) = if l.lado == DiffSide::Original { (l.inicio, l.fim) } else { (l.fim, l.inicio) };
            format!("{} (da linha {antiga} do original, removida, até a linha {nova} nova)", c.arquivo)
        }
        Some(l) if l.lado == DiffSide::Original => {
            if l.inicio == l.fim {
                format!("{} (linha removida; era a {} no original)", c.arquivo, l.inicio)
            } else {
                format!("{} (linhas removidas; eram a {} a {} no original)", c.arquivo, l.inicio, l.fim)
            }
        }
        Some(l) if l.inicio == l.fim => format!("{}:{}", c.arquivo, l.inicio),
        Some(l) => format!("{}:{}-{}", c.arquivo, l.inicio, l.fim),
    };
    let mut texto = format!("- {onde}");
    if !c.trecho.trim().is_empty() {
        texto.push_str(&format!(" (`{}`)", c.trecho.trim()));
    }
    texto.push_str(&format!(": {}", c.texto));
    texto
}

/// O arquivo do diff que o revisor apontou. Ele pode escrever "./src/a.rs", o caminho inteiro ou
/// só o fim dele.
fn achar_arquivo(caminhos: &[&str], arquivo: &str) -> Option<usize> {
    let alvo = arquivo.trim().trim_start_matches("./");
    caminhos.iter().position(|c| *c == alvo).or_else(|| {
        caminhos
            .iter()
            .position(|c| alvo.ends_with(&format!("/{c}")) || c.ends_with(&format!("/{alvo}")))
    })
}

/// A linha apontada, se ela está no patch, ou a mais perto dela até 10 linhas de distância.
fn linha_mais_perto(linha: usize, existe: impl Fn(usize) -> bool) -> Option<usize> {
    (0..=10)
        .flat_map(|d| [linha.checked_sub(d), linha.checked_add(d)])
        .flatten()
        .filter(|&l| l >= 1)
        .find(|&l| existe(l))
}

impl Janela {
    pub(super) fn dialogo_diff(&mut self, tarefa: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.dialogo_diff_em(tarefa, None, window, cx)
    }

    /// Abre o Revisar (já no arquivo e na linha de um problema apontado pelo revisor, se pedido).
    /// A revisão de uma tarefa em andamento fica guardada: fechar e reabrir mantém comentários,
    /// rascunho e vistos. As de tarefas encerradas saem.
    pub(super) fn dialogo_diff_em(
        &mut self,
        tarefa: u64,
        foco: Option<(String, Option<u64>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let encerradas: HashSet<u64> = motor
            .read(cx)
            .tarefas
            .iter()
            .filter(|t| t.estado.encerrada())
            .map(|t| t.id)
            .collect();
        self.revisoes.retain(|id, _| !encerradas.contains(id));
        let revisao = match self.revisoes.get(&tarefa).cloned() {
            Some(revisao) => {
                revisao.update(cx, |r, cx| r.reabrir(foco, cx));
                revisao
            }
            None => {
                let janela = cx.entity().downgrade();
                let revisao = cx.new(|cx| RevisaoDiff::new(motor, janela, tarefa, foco, window, cx));
                if !encerradas.contains(&tarefa) {
                    self.revisoes.insert(tarefa, revisao.clone());
                }
                revisao
            }
        };
        abrir_revisao(revisao, window, cx);
    }

    /// O diff de um arquivo do PR (a aba PR): com a cópia, a revisão de sempre já nele; sem ela,
    /// o diff do GitHub, só para ler.
    pub(super) fn dialogo_diff_do_pr(&mut self, tarefa: u64, fonte: FonteDiff, arquivo: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let foco = arquivo.map(|a| (a, None));
        match fonte {
            FonteDiff::Tarefa(id) => self.dialogo_diff_em(id, foco, window, cx),
            FonteDiff::Pr { .. } => {
                let Some(motor) = self.motor.clone() else {
                    return;
                };
                let janela = cx.entity().downgrade();
                let revisao = cx.new(|cx| RevisaoDiff::com_fonte(motor, janela, tarefa, foco, fonte, window, cx));
                abrir_revisao(revisao, window, cx);
            }
        }
    }
}

/// Abre o diálogo da revisão, refeito a cada quadro.
fn abrir_revisao(revisao: Entity<RevisaoDiff>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialogo, window, cx| {
        // Refeito a cada quadro: o subtítulo muda quando o diff carrega e quando o estado
        // muda, e a largura acompanha a janela.
        dialogo
            .bg(Cores::dialogo())
            .w(largura_do_dialogo(window))
            .margin_top(px(24.))
            .rounded(raio::PECA)
            .close_button(false)
            // Enter não fecha: na revisão ele abre o rascunho.
            .on_ok(|_, _, _| false)
            .title(div().pb(r(8.)).child(revisao.read(cx).cabecalho(cx)))
            .child(revisao.clone())
    });
}

#[cfg(test)]
mod testes {
    use super::{
        Comentario, Disposicao, GrupoDePasta, Linhas, Pasta, Pendencia, Principal, Rotulos, achar_arquivo,
        descrever, disposicao, entrega_nova, grupos_por_pasta, linha_mais_perto, pendencias_de, principal,
        resumo_do_ajuste, texto_do_ajuste, texto_do_rodape,
    };
    use crate::modelo::{EstadoTarefa, Problema, Revisao, Tarefa, Verificacao};
    use gpui_kit::component::diff::{DiffFile, DiffLineRange, DiffSide};

    const DIFF: &str = "\
diff --git a/src/precos.py b/src/precos.py
index 1..2 100644
--- a/src/precos.py
+++ b/src/precos.py
@@ -10,3 +10,4 @@ def total():
     a = 1
-    b = 2
+    b = 3
+    c = 4
     return a
diff --git a/README.md b/README.md
--- a/README.md
+++ b/README.md
@@ -1 +1 @@
-velho
+novo
";

    fn tarefa(estado: &str) -> Tarefa {
        let json = format!(
            r#"{{"id": 2, "projeto": "loja", "coordenador": "c", "trabalhador": "t", "titulo": "Preços",
                "objetivo": "o", "criterios": [], "verificacao": "cargo test", "estado": "{estado}",
                "rodada": 2, "branch": "vitral/tarefa-2", "criada_em": 0}}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    fn comentario(arquivo: &str, linhas: Option<Linhas>, texto: &str) -> Comentario {
        Comentario { arquivo: arquivo.into(), linhas, trecho: String::new(), texto: texto.into(), problema: None }
    }

    #[test]
    fn foco_acha_o_arquivo_e_a_linha_mais_perto() {
        let arquivos = DiffFile::parse(DIFF).expect("diff");
        let caminhos: Vec<&str> = arquivos.iter().map(|a| a.path().as_ref()).collect();
        assert_eq!(caminhos, ["src/precos.py", "README.md"]);
        assert_eq!(achar_arquivo(&caminhos, "src/precos.py"), Some(0));
        assert_eq!(achar_arquivo(&caminhos, "./src/precos.py"), Some(0));
        assert_eq!(achar_arquivo(&caminhos, "/home/v/loja/src/precos.py"), Some(0));
        assert_eq!(achar_arquivo(&caminhos, "precos.py"), Some(0));
        assert_eq!(achar_arquivo(&caminhos, "README.md"), Some(1));
        assert_eq!(achar_arquivo(&caminhos, "outro.rs"), None);
        // Lado novo de precos.py: linhas 10 a 13.
        let existe = |l: usize| arquivos[0].line_text(DiffSide::Modified, l).is_some();
        assert_eq!(linha_mais_perto(12, existe), Some(12));
        assert_eq!(linha_mais_perto(15, existe), Some(13));
        assert_eq!(linha_mais_perto(8, existe), Some(10));
        assert_eq!(linha_mais_perto(400, existe), None);
        assert_eq!(arquivos[0].line_text(DiffSide::Original, 11).map(|t| t.to_string()), Some("    b = 2".into()));
    }

    #[test]
    fn linhas_com_tracos_ou_mais_no_comeco_nao_somem() {
        // "---" e "-- comentario" removidos viram "----" e "--- comentario" no diff; "++i" vira "+++i".
        let diff = "\
diff --git a/x.md b/x.md
--- a/x.md
+++ b/x.md
@@ -1,4 +1,3 @@
----
--- comentario
 meio
-fim
+novo
+++i
";
        let arquivos = DiffFile::parse(diff).expect("diff");
        assert_eq!(arquivos.len(), 1);
        let x = &arquivos[0];
        assert_eq!((x.deletions(), x.additions()), (3, 2));
        let texto = |lado, l| x.line_text(lado, l).map(|t| t.to_string());
        assert_eq!(texto(DiffSide::Original, 1), Some("---".into()));
        assert_eq!(texto(DiffSide::Original, 2), Some("-- comentario".into()));
        assert_eq!(texto(DiffSide::Original, 4), Some("fim".into()));
        assert_eq!(texto(DiffSide::Modified, 1), Some("meio".into()));
        assert_eq!(texto(DiffSide::Modified, 3), Some("++i".into()));
    }

    #[test]
    fn comentario_diz_de_que_lado_e_a_linha() {
        let c = |linhas, trecho: &str| Comentario {
            arquivo: "precos.py".into(),
            linhas,
            trecho: trecho.into(),
            texto: "arrumar".into(),
            problema: None,
        };
        let nova = Linhas::uma(DiffSide::Modified, 7);
        let removida = Linhas::uma(DiffSide::Original, 2);
        assert_eq!(descrever(&c(Some(nova), "b = 3")), "- precos.py:7 (`b = 3`): arrumar");
        assert_eq!(
            descrever(&c(Some(removida), "b = 2")),
            "- precos.py (linha removida; era a 2 no original) (`b = 2`): arrumar"
        );
        assert_eq!(descrever(&c(None, "")), "- precos.py: arrumar");
        let faixa = Linhas::de_faixa(&DiffLineRange::new("precos.py", DiffSide::Modified, 9, 4));
        assert_eq!(descrever(&c(Some(faixa), "")), "- precos.py:4-9: arrumar");
        let misto = Linhas::de_faixa(
            &DiffLineRange::new("precos.py", DiffSide::Original, 2, 3).with_end_side(DiffSide::Modified),
        );
        assert_eq!(misto.ancora(), (DiffSide::Modified, 3));
        assert_eq!(
            descrever(&c(Some(misto), "")),
            "- precos.py (da linha 2 do original, removida, até a linha 3 nova): arrumar"
        );
    }

    #[test]
    fn disposicao_pela_largura_do_conteudo() {
        let largo = disposicao(1520., false);
        assert_eq!(
            largo,
            Disposicao { lateral: Some(264.), rotulos: Rotulos::Longos, segmentado: true, lado_a_lado_ok: true }
        );
        // Lado a lado com folga: a lateral fica.
        assert_eq!(disposicao(1520., true).lateral, Some(264.));
        // Largo, mas sem 960 para o diff com a lateral: ela recolhe no chip.
        assert_eq!(disposicao(1200., true).lateral, None);
        assert_eq!(disposicao(1200., false).lateral, Some(264.));
        let medio = disposicao(1020., false);
        assert_eq!((medio.lateral, medio.rotulos, medio.lado_a_lado_ok), (Some(224.), Rotulos::Curtos, true));
        assert_eq!(disposicao(1020., true).lateral, None);
        // Médio estreito: o lado a lado não cabe e a lateral não sai por ele.
        let apertado = disposicao(860., true);
        assert_eq!((apertado.lateral, apertado.lado_a_lado_ok, apertado.segmentado), (Some(224.), false, true));
        // Estreito (960 a 150%): sem lateral, só ícones, sempre unificado.
        assert_eq!(
            disposicao(587., true),
            Disposicao { lateral: None, rotulos: Rotulos::Icone, segmentado: false, lado_a_lado_ok: false }
        );
        assert_eq!(disposicao(1180., false).rotulos, Rotulos::Longos);
        assert_eq!(disposicao(760., false).rotulos, Rotulos::Curtos);
        assert_eq!(disposicao(759.9, false).rotulos, Rotulos::Icone);
    }

    #[test]
    fn grupos_por_pasta_com_a_raiz_primeiro_e_os_gerados_no_fim() {
        let caminhos = [
            ("Cargo.lock", true),
            ("docs/README.md", false),
            ("src/loja/cupom.py", false),
            ("src/loja/precos.py", false),
            ("src/main.rs", false),
            ("web/package-lock.json", true),
            ("zeta.md", false),
        ];
        assert_eq!(
            grupos_por_pasta(&caminhos),
            vec![
                GrupoDePasta { pasta: Pasta::Raiz, arquivos: vec![6] },
                GrupoDePasta { pasta: Pasta::Em("docs".into()), arquivos: vec![1] },
                GrupoDePasta { pasta: Pasta::Em("src/loja".into()), arquivos: vec![2, 3] },
                GrupoDePasta { pasta: Pasta::Em("src".into()), arquivos: vec![4] },
                GrupoDePasta { pasta: Pasta::Gerados, arquivos: vec![0, 5] },
            ]
        );
        assert_eq!(grupos_por_pasta(&[]), Vec::<GrupoDePasta>::new());
        assert_eq!(
            grupos_por_pasta(&[("a.rs", false)]),
            vec![GrupoDePasta { pasta: Pasta::Raiz, arquivos: vec![0] }]
        );
    }

    #[test]
    fn texto_do_ajuste_junta_recado_comentarios_e_apontamentos() {
        let comentarios = [
            comentario("src/precos.py", Some(Linhas::uma(DiffSide::Modified, 12)), "Use Decimal"),
            comentario("README.md", None, "Falta o exemplo"),
        ];
        let apontamento = Problema {
            arquivo: "src/cupom.py".into(),
            linha: Some(4),
            gravidade: "media".into(),
            descricao: "Cupom vencido passa ".into(),
        };
        assert_eq!(
            texto_do_ajuste(2, "  refaça com Decimal \n", &comentarios, std::slice::from_ref(&apontamento)),
            "Ajustes pedidos pelo usuário na revisão (rodada 2):\n\n\
             refaça com Decimal\n\n\
             Comentários nas linhas:\n\
             - src/precos.py:12: Use Decimal\n\
             - README.md: Falta o exemplo\n\n\
             Apontamentos do revisor que o usuário quer resolvidos:\n\
             - [média] src/cupom.py:4: Cupom vencido passa"
        );
        assert_eq!(
            texto_do_ajuste(1, "", &comentarios[..1], &[]),
            "Ajustes pedidos pelo usuário na revisão (rodada 1):\n\nComentários nas linhas:\n- src/precos.py:12: Use Decimal"
        );
        assert_eq!(
            texto_do_ajuste(3, "só o recado", &[], &[]),
            "Ajustes pedidos pelo usuário na revisão (rodada 3):\n\nsó o recado"
        );
        assert_eq!(resumo_do_ajuste(2, 3, 0), "Ajuste enviado ao chat 2: 3 comentários.");
        assert_eq!(
            resumo_do_ajuste(2, 1, 2),
            "Ajuste enviado ao chat 2: 1 comentário e 2 apontamentos do revisor."
        );
        assert_eq!(resumo_do_ajuste(7, 0, 0), "Ajuste enviado ao chat 7.");
    }

    #[test]
    fn pendencias_antes_de_aprovar() {
        let mut t = tarefa("EmRevisao");
        assert_eq!(pendencias_de(&t, 0, false), Vec::<Pendencia>::new());
        t.ultima_verificacao = Some(Verificacao {
            comando: "cargo test".into(),
            passou: false,
            saida: String::new(),
            duracao_s: None,
        });
        t.revisao = Some(Revisao {
            veredito: "ajustar".into(),
            resumo: String::new(),
            problemas: vec![
                Problema { gravidade: "alta".into(), ..Default::default() },
                Problema { gravidade: "baixa".into(), ..Default::default() },
            ],
        });
        let lista = pendencias_de(&t, 3, false);
        let textos: Vec<&str> = lista.iter().map(|p| p.texto.as_str()).collect();
        assert_eq!(
            textos,
            [
                "A verificação `cargo test` falhou nesta rodada.",
                "O revisor apontou 1 problema de gravidade alta.",
                "Seus 3 comentários ainda não foram enviados e vão ser descartados.",
            ]
        );
        let ressalvas: Vec<&str> = lista.iter().filter_map(|p| p.ressalva.as_deref()).collect();
        assert_eq!(ressalvas, ["a verificação `cargo test` falhou", "o revisor apontou 1 problema de gravidade alta"]);
        // Da rodada anterior não pesa: o trabalhador está ajustando.
        let mut ajustando = t.clone();
        ajustando.estado = EstadoTarefa::Ajustando;
        assert_eq!(pendencias_de(&ajustando, 1, true).len(), 2);
        assert!(pendencias_de(&ajustando, 1, true).iter().all(|p| p.ressalva.is_none()));
    }

    #[test]
    fn o_botao_azul_segue_o_que_esta_vermelho() {
        let mut t = tarefa("EmRevisao");
        assert_eq!(principal(&pendencias_de(&t, 0, false)), Principal::Aprovar);
        // Comentário por enviar ou o diff que não deu para ler: o azul vai para o Pedir ajuste.
        assert_eq!(principal(&pendencias_de(&t, 2, false)), Principal::Ajuste);
        assert_eq!(principal(&pendencias_de(&t, 0, true)), Principal::Ajuste);
        t.ultima_verificacao =
            Some(Verificacao { comando: "cargo test".into(), passou: false, saida: String::new(), duracao_s: None });
        assert_eq!(principal(&pendencias_de(&t, 0, false)), Principal::Ajuste);
        // A verificação vermelha de outra rodada não pesa.
        t.estado = EstadoTarefa::Ajustando;
        assert_eq!(principal(&pendencias_de(&t, 0, false)), Principal::Aprovar);
    }

    #[test]
    fn entrega_nova_e_texto_do_rodape() {
        assert_eq!(entrega_nova((2, EstadoTarefa::EmRevisao), 2, EstadoTarefa::EmRevisao), None);
        assert_eq!(entrega_nova((2, EstadoTarefa::EmRevisao), 3, EstadoTarefa::Ajustando), Some(3));
        assert_eq!(entrega_nova((2, EstadoTarefa::Ajustando), 2, EstadoTarefa::EmRevisao), Some(2));
        assert_eq!(entrega_nova((1, EstadoTarefa::Rodando), 1, EstadoTarefa::Rodando), None);
        assert_eq!(texto_do_rodape(1, 3, 1), "1 de 3 arquivos vistos · 1 comentário para enviar");
        assert_eq!(texto_do_rodape(0, 1, 0), "0 de 1 arquivo visto · nenhum comentário ainda");
        assert_eq!(texto_do_rodape(0, 0, 0), "Nenhum comentário ainda");
        assert_eq!(texto_do_rodape(2, 2, 4), "2 de 2 arquivos vistos · 4 comentários para enviar");
    }
}
