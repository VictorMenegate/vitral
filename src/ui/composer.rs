//! A caixa de mensagem, com as opções da conversa como na extensão do Claude Code: anexos,
//! modo de permissão, modelo, esforço, conta, uso do contexto e o menu de comandos de barra.
//! Os chips da barra, com os menus e os popovers deles, ficam em `composer_seletores.rs`.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use gpui_kit::component::{
    ElementExt as _,
    button::{Button, ButtonVariants as _},
    input::{Textarea, TextareaState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Janela;
use super::componentes::{
    Anel, BotaoRedondo, FaixaCaixa, Flutuante as _, TamanhoBotao, VarianteRedonda, botao_fantasma, cor_da_faixa,
    popover, variante_fantasma,
};
use super::movimento::{self, Curva, com_estado};
use crate::{
    modelo::{Chat, EstadoTarefa, Papel},
    motor::{self, Motor},
    tema::{Cores, Lucide, Tipo as _, esp, gradiente_quente, halo_suave, r, raio, tam},
};

#[path = "composer_seletores.rs"]
mod seletores;

use seletores::Barra;

/// (valor, nome, descrição) dos modos de permissão do Claude Code.
pub const MODOS: [(&str, &str, &str); 5] = [
    (
        "manual",
        "Manual",
        "Pede aprovação antes de cada edição ou comando",
    ),
    (
        "acceptEdits",
        "Aceitar edições",
        "Edita arquivos sem perguntar; comandos ainda pedem",
    ),
    (
        "plan",
        "Planejar",
        "Explora o código e apresenta um plano antes de mexer",
    ),
    (
        "auto",
        "Automático",
        "Um classificador decide o que precisa de aprovação",
    ),
    (
        "bypassPermissions",
        "Sem perguntar",
        "Faz tudo sem pedir permissão",
    ),
];

pub const MODELOS: [(&str, &str); 5] = [
    ("", "Padrão"),
    ("fable", "Fable"),
    ("opus", "Opus"),
    ("sonnet", "Sonnet"),
    ("haiku", "Haiku"),
];

/// Uma linha sobre cada família, na ordem de `MODELOS` (sem o Padrão): no seletor da caixa e nos
/// menus de modelo das Configurações.
pub const DESCRICOES_DE_MODELO: [&str; 4] = [
    "Para os desafios mais difíceis",
    "Para trabalho complexo e o dia a dia",
    "O mais eficiente para tarefas simples",
    "O mais rápido para respostas curtas",
];

/// Os nomes do seletor do Claude Code ("Extra" é o xhigh); os valores gravados não mudam.
pub const ESFORCOS: [(&str, &str); 6] = [
    ("", "Auto"),
    ("low", "Baixo"),
    ("medium", "Médio"),
    ("high", "Alto"),
    ("xhigh", "Extra"),
    ("max", "Máximo"),
];

/// Comandos de barra sugeridos antes de a conversa começar (depois vêm do Claude Code).
const COMANDOS_INICIAIS: [&str; 8] = [
    "clear",
    "compact",
    "context",
    "usage",
    "init",
    "review",
    "security-review",
    "simplify",
];

// Esc na caixa de mensagem: fecha o menu "/" ou para o turno do chat.
actions!(vitral, [PararTurno]);
// Shift+Tab na caixa de mensagem: passa para o próximo modo de permissão.
actions!(vitral, [AlternarModo]);
// Ctrl+Shift+M e Ctrl+E abrem os popovers de modelo e de esforço; dentro do de esforço, Espaço
// liga o Ultrathink e U, o Ultracode.
actions!(vitral, [AbrirModelo, AbrirEsforco, AlternarUltrathink, AlternarUltracode]);

/// Popover da caixa (um por vez em toda a janela, guardado em `Janela.pop_caixa`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PopCaixa {
    Modelo,
    Esforco,
}

/// Linhas da caixa antes de ela rolar por dentro, com a janela alta.
pub const LINHAS_CAIXA: usize = 12;

/// No máximo tantas sugestões no menu da caixa.
const MAX_SUGESTOES: usize = 8;

/// Uma sugestão do menu da caixa: um comando de barra, um projeto (`#`) ou um chat (`@`).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Sugestao {
    /// O que entra no lugar do token ("/compact", "#ninee", "@12").
    pub valor: String,
    pub titulo: String,
    pub detalhe: Option<String>,
    /// O começo que casa primeiro (o nome do comando, a chave do projeto, o número do chat).
    pub chave: String,
    /// Onde a busca olha, sem acento.
    pub busca: String,
    /// A ordem entre as que casam igual (o tamanho do comando, a ordem do trilho, a do mais recente).
    pub peso: usize,
    /// O projeto da sugestão, para o quadrado (`Some("")` é Avulsos).
    pub projeto: Option<String>,
}

/// O que a última palavra da caixa pede: o sinal, o que veio depois dele e onde a palavra começa.
/// `/` só no começo e sem espaço depois; `#` e `@` no começo ou depois de um espaço.
pub(super) fn gatilho(texto: &str) -> Option<(char, &str, usize)> {
    if let Some(resto) = texto.strip_prefix('/') {
        return (!resto.contains(char::is_whitespace)).then_some(('/', resto, 0));
    }
    let inicio = texto.char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8());
    let palavra = &texto[inicio..];
    let sinal = palavra.chars().next().filter(|c| matches!(c, '#' | '@'))?;
    Some((sinal, &palavra[sinal.len_utf8()..], inicio))
}

/// As sugestões que casam com o digitado: quem começa com ele primeiro, depois quem só o contém,
/// cada grupo pelo peso; no máximo 8.
pub(super) fn menu_sugestoes(itens: Vec<Sugestao>, digitado: &str) -> Vec<Sugestao> {
    let digitado = crate::dados::chave(digitado);
    let mut lista: Vec<Sugestao> = itens
        .into_iter()
        .filter(|s| digitado.is_empty() || s.chave.contains(&digitado) || s.busca.contains(&digitado))
        .collect();
    lista.sort_by_key(|s| (!s.chave.starts_with(&digitado), s.peso));
    lista.truncate(MAX_SUGESTOES);
    lista
}

/// Troca o texto da caixa e deixa o cursor no fim (o `set_value` do kit o põe no começo).
pub(super) fn por_texto(c: &mut TextareaState, texto: String, window: &mut Window, cx: &mut Context<TextareaState>) {
    let fim = texto.len();
    c.set_value(texto, window, cx);
    c.set_selected_range(fim..fim, cx);
}

/// O texto com a última palavra trocada pela escolhida e um espaço depois.
pub(super) fn completar(texto: &str, inicio: usize, valor: &str) -> String {
    let inicio = if texto.is_char_boundary(inicio) { inicio } else { texto.len() };
    format!("{}{valor} ", &texto[..inicio])
}

pub fn nome_do_modo(valor: &str) -> &'static str {
    MODOS
        .iter()
        .find(|(v, _, _)| *v == valor)
        .map(|(_, n, _)| *n)
        .unwrap_or("Sem perguntar")
}

fn descricao_do_modo(valor: &str) -> &'static str {
    MODOS
        .iter()
        .find(|(v, _, _)| *v == valor)
        .map(|(_, _, d)| *d)
        .unwrap_or(MODOS[4].2)
}

/// Posição do esforço em `ESFORCOS` (valor desconhecido conta como Auto).
fn indice_esforco(valor: &str) -> usize {
    ESFORCOS.iter().position(|(v, _)| *v == valor).unwrap_or(0)
}

/// Nomes dos níveis no segmentado: na faixa estreita, "Máximo" não cabe no segmento.
fn nomes_dos_niveis(estreita: bool) -> [&'static str; 6] {
    let mut nomes = ESFORCOS.map(|(_, n)| n);
    if estreita {
        nomes[5] = "Máx";
    }
    nomes
}

fn descricao_do_esforco(indice: usize) -> &'static str {
    match indice {
        1 => "Rápido e direto, bom para tarefas simples",
        2 => "Equilíbrio entre rapidez e cuidado",
        3 => "Implementação completa, com testes",
        4 => "Raciocínio estendido e análise minuciosa",
        5 => "O máximo de raciocínio; gasta muito do plano",
        _ => "Usa o padrão do modelo.",
    }
}

/// A chave armada para a próxima mensagem desliga ao enviar; um comando de barra não a consome.
pub fn gastar_chave(armados: &mut HashSet<String>, chat: &str, texto: &str) -> bool {
    !texto.trim_start().starts_with('/') && armados.remove(chat)
}

/// O Haiku 4 não usa esforço: o Claude Code descarta o valor sozinho.
fn usa_esforco(em_uso: Option<&str>) -> bool {
    !em_uso.is_some_and(|m| m.starts_with("claude-haiku-4"))
}

/// Nome legível do modelo em uso: "claude-opus-5-5" vira "Opus 5.5" e
/// "claude-sonnet-4-5-20250929[1m]" vira "Sonnet 4.5 (1M)". O que não for do Claude fica igual.
pub fn nome_curto_modelo(id: &str) -> String {
    let (base, extra) = match id.split_once('[') {
        Some((base, resto)) => (base, Some(resto.trim_end_matches(']'))),
        None => (id, None),
    };
    let Some(resto) = base.strip_prefix("claude-") else {
        return id.to_string();
    };
    let mut familia = None;
    let mut versao = Vec::new();
    for parte in resto.split('-') {
        if !parte.is_empty() && parte.chars().all(|c| c.is_ascii_digit()) {
            // Datas (20250929) não entram no nome.
            if parte.len() < 6 {
                versao.push(parte);
            }
        } else if familia.is_none() && !parte.is_empty() {
            familia = Some(parte);
        } else {
            return id.to_string();
        }
    }
    let Some(familia) = familia else {
        return id.to_string();
    };
    let mut letras = familia.chars();
    let mut nome: String = letras.next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    nome.push_str(letras.as_str());
    if !versao.is_empty() {
        nome = format!("{nome} {}", versao.join("."));
    }
    match extra.filter(|e| !e.is_empty()) {
        Some(extra) => format!("{nome} ({})", extra.to_uppercase()),
        None => nome,
    }
}

/// "Opus 5.5" vira "Claude Opus 5.5"; "Padrão" e nomes de fora do Claude ficam como estão.
fn com_claude(rotulo: &str) -> String {
    if MODELOS[1..].iter().any(|(_, familia)| rotulo.starts_with(familia)) {
        format!("Claude {rotulo}")
    } else {
        rotulo.to_string()
    }
}

/// Uso do contexto: a fração (para o anel) e a porcentagem do meio, de 0 a 99. Antes do
/// primeiro `result`, a janela é a padrão do modelo em uso.
pub(super) fn uso_do_contexto(usados: u64, janela: Option<u64>, modelo: Option<&str>) -> (f32, u32, u64) {
    let janela = janela.unwrap_or_else(|| motor::janela_padrao(modelo)).max(1);
    let fracao = (usados as f64 / janela as f64) as f32;
    let numero = ((fracao * 100.).round().max(0.) as u32).min(99);
    (fracao, numero, janela)
}

/// "120 mil" (ou "1 milhão", para a janela de 1M).
pub(super) fn milhares(n: u64) -> String {
    match n {
        1_000_000 => "1 milhão".into(),
        n if n > 1_000_000 && n % 1_000_000 == 0 => format!("{} milhões", n / 1_000_000),
        n => format!("{} mil", n / 1000),
    }
}

/// "200 mil tokens", "1 milhão de tokens".
fn tokens(n: u64) -> String {
    let quantia = milhares(n);
    if quantia.ends_with("mil") { format!("{quantia} tokens") } else { format!("{quantia} de tokens") }
}

/// Onde começa o texto da caixa, da borda de fora dela: 1 de borda, a margem (8; na estreita, 6
/// com o campo 2 para a esquerda) e os 10 do campo do kit, que crescem com o zoom como o resto.
/// Cai na coluna do ícone do primeiro chip, e a faixa do turno se alinha por aqui.
pub(super) fn recuo_do_texto(estreita: bool, window: &Window) -> Pixels {
    tam(window, if estreita { 15. } else { 19. })
}

/// Da borda de fora da caixa ao conteúdo (o enviar, à direita).
pub(super) fn borda_ao_conteudo(estreita: bool, window: &Window) -> Pixels {
    tam(window, if estreita { 7. } else { 9. })
}

/// O que o botão redondo da direita faz agora.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Envio {
    /// Caixa vazia e chat parado: o botão só mostra que existe.
    Vazio,
    Enviar,
    /// Chat rodando (ou na fila) com a caixa vazia: o botão vira parar.
    Parar,
    /// Chat rodando com texto: parar ao lado e enviar, que entra na fila.
    PararEEnviar,
}

fn envio(rodando: bool, vazio: bool) -> Envio {
    match (rodando, vazio) {
        (false, true) => Envio::Vazio,
        (false, false) => Envio::Enviar,
        (true, true) => Envio::Parar,
        (true, false) => Envio::PararEEnviar,
    }
}

impl Janela {
    /// A caixa de mensagem do painel: 96 de altura vazia, o texto na coluna do ícone do primeiro
    /// chip, e a borda e o halo azul que acendem com o foco (180 ms; apagam em 120).
    pub(super) fn caixa_mensagem(
        &mut self,
        motor: &Motor,
        chat: &Chat,
        ix: usize,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        // Sem cópia isolada, o "Sem perguntar" mexe direto na pasta do projeto.
        let sem_copia = match chat.papel {
            Papel::Trabalhador | Papel::Projeto => motor.pasta_do_chat(chat).is_some_and(|p| p == chat.cwd),
            Papel::Coordenador | Papel::Mestre => !self.usa_git(motor, &chat.projeto),
        };
        let painel = &self.paineis[ix];
        let (painel_id, faixa, linhas) = (painel.id, painel.medida.faixa, painel.linhas_caixa);
        let composer = painel.composer.clone();
        let estreita = faixa == FaixaCaixa::Estreita;
        // A borda acende na caixa com foco (é para ela que vão Enter, Ctrl+V e @) ou com um
        // popover dela aberto.
        let focada = composer.read(cx).focus_handle(cx).contains_focused(window, cx)
            || self.pop_caixa.is_some_and(|(p, _)| p == painel_id);
        let anexos = self.anexos.get(&chat.id).cloned().unwrap_or_default();
        let janela = cx.entity().downgrade();
        let (colar, chat_colar) = (janela.clone(), chat.id.clone());
        let pasta_anexos = motor.dados.pasta_anexos();
        let barra = Barra {
            painel: painel_id,
            chat: chat.id.clone(),
            faixa,
            sem_copia,
            composer: composer.clone(),
            rodando: motor.execucao(&chat.id).rodando() || motor.esperando_vaga(&chat.id),
        };
        let barra = self.barra_da_caixa(motor, chat, ix, &barra, anexos.is_empty(), window, cx);
        let raio_caixa = if estreita { raio::CARTAO } else { raio::PECA };
        let foco_da_caixa = composer.read(cx).focus_handle(cx);

        let caixa = div()
            .relative()
            .flex()
            .flex_col()
            .min_w_0()
            .rounded(raio_caixa)
            .border_1()
            .bg(gradiente_quente())
            // O vão entre o texto e os chips, a margem e o espaço entre os chips são da caixa: o
            // clique ali não leva o foco para a janela. O campo e os botões tratam o deles antes.
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if !window.default_prevented() {
                    window.focus(&foco_da_caixa, cx);
                    window.prevent_default();
                }
            })
            .map(|s| if estreita { s.pt(esp::E8).pb(esp::E6).px(esp::E6) } else { s.pt(esp::E12).pb(esp::E8).px(esp::E8) })
            // Por baixo de tudo, o reflexo quente da direita, com o próprio raio (o recorte do pai
            // não arredonda).
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(raio_caixa - px(1.))
                    .bg(Cores::caixa_brilho()),
            )
            .when(!anexos.is_empty(), |s| s.child(self.anexos_pendentes(&chat.id, anexos, window, cx)))
            .child(
                // Só o campo da mensagem fica em "Composer": o Shift+Tab troca o modo aqui, e não
                // na busca de um popover da caixa. Sem margem própria, o texto (com os 10 do campo
                // do kit) cai na coluna do ícone do primeiro chip.
                div()
                    .w_full()
                    .key_context("Composer")
                    .when(estreita, |s| s.ml(r(-2.)))
                    .child(
                        Textarea::new(&composer)
                            .appearance(false)
                            // Colar imagem ou arquivos copiados no gerenciador vira anexo.
                            .on_paste(move |item, _, cx| {
                                let caminhos = anexos_da_area_de_transferencia(item, &pasta_anexos);
                                if caminhos.is_empty() {
                                    return false;
                                }
                                let chat = chat_colar.clone();
                                let _ = colar.update(cx, |this, cx| {
                                    this.anexos.entry(chat).or_default().extend(caminhos);
                                    cx.notify();
                                });
                                true
                            }),
                    ),
            )
            .child(div().h(esp::E8))
            .child(barra);
        // Borda e halo andam juntos, conservando o brilho de cima e a sombra da caixa.
        let caixa = com_estado(move |window, cx| {
            let (alvo, duracao) = if focada {
                ((Cores::destaque_borda(), 1f32), movimento::MOVER)
            } else {
                ((Cores::borda(), 0.), movimento::COR)
            };
            let (borda, forca) =
                movimento::valor(("halo-caixa", painel_id as usize), alvo, duracao, Curva::Mover, window, cx);
            let mut sombras = vec![
                BoxShadow::new(px(0.), px(1.), Cores::brilho_topo()).inset(),
                BoxShadow::new(px(0.), px(6.), Cores::sombra_caixa()).blur_radius(px(10.)),
            ];
            if forca > 0. {
                sombras.push(halo_suave(forca));
            }
            caixa.border_color(borda).shadow(sombras).into_any_element()
        });

        div()
            .map(super::mensagens::coluna)
            .when(estreita, |s| s.px(esp::E12))
            .pb(esp::E12)
            .on_prepaint(ajustar_linhas(janela.clone(), painel_id, linhas))
            .key_context("Caixa")
            .on_action(cx.listener({
                let (chat, composer) = (chat.id.clone(), composer.clone());
                move |this, _: &PararTurno, window, cx| this.esc_na_caixa(&chat, &composer, window, cx)
            }))
            .on_action(cx.listener({
                let chat = chat.id.clone();
                move |this, _: &AlternarModo, _, cx| this.alternar_modo(&chat, cx)
            }))
            .child(self.menu_comandos(motor, chat, &composer, painel_id, cx))
            .child(caixa)
    }

    /// "Modo · modelo · esforço · conta" à esquerda e, à direita, o anel do contexto, o clipe, o
    /// "…" e o enviar. O modo fica sempre à vista; os outros vêm num grupo que quebra linha com
    /// a segunda linha escondida, então o que não cabe some inteiro, a conta primeiro.
    #[allow(clippy::too_many_arguments)]
    fn barra_da_caixa(
        &self,
        motor: &Motor,
        chat: &Chat,
        ix: usize,
        b: &Barra,
        sem_anexos: bool,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let estreita = b.faixa == FaixaCaixa::Estreita;
        let altura = if estreita { 28. } else { 32. };
        let vazio = sem_anexos && b.composer.read(cx).value().trim().is_empty();
        let grupo = |e: AnyElement| div().flex_none().ml(esp::E4).child(e);
        // Mensagem direta a uma tarefa em revisão abre uma rodada nova: quem avisa é o enviar.
        let rodada_nova = chat.papel == Papel::Trabalhador
            && chat.tarefa.and_then(|t| motor.tarefa(t)).is_some_and(|t| t.estado == EstadoTarefa::EmRevisao);
        let envios = self.paineis[ix].envios;
        div()
            .flex()
            .items_center()
            .h(r(altura))
            .child(div().flex_none().child(if chat.papel == Papel::Mestre {
                self.chip_divisao(motor, b, window, cx)
            } else {
                self.chip_modo(motor, chat, b, window, cx)
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .h(r(altura))
                    .overflow_hidden()
                    // A linha de cima nunca fica vazia: sem este marcador, a primeira opção que
                    // não coubesse ficaria nela, cortada. Com a altura da linha, a segunda começa
                    // abaixo do recorte.
                    .child(div().h(r(altura)))
                    .child(grupo(self.chip_modelo(motor, chat, ix, b, window, cx)))
                    .child(grupo(self.chip_esforco(motor, chat, ix, b, window, cx)))
                    .when(!estreita, |s| s.child(grupo(self.chip_conta(motor, chat, b, window, cx)))),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(r(4.))
                    .ml(r(8.))
                    .children(self.anel_do_contexto(chat, b, cx))
                    .when(!estreita, |s| s.child(self.botao_anexar(b, window, cx)))
                    .when(b.faixa != FaixaCaixa::Media, |s| {
                        s.child(self.botao_mais(motor, chat, b, window, cx))
                    })
                    .children(self.botoes_de_envio(b, vazio, motor.esperando_vaga(&chat.id), rodada_nova, envios, cx)),
            )
    }

    /// O enviar claro (apagado com a caixa vazia) e, com o chat rodando, o parar. O botão grande
    /// guarda o mesmo id como enviar e como parar: o pulso do envio (`envios`) toca nele mesmo
    /// quando o chat já começou a rodar e ele virou o parar.
    #[allow(clippy::too_many_arguments)]
    fn botoes_de_envio(
        &self,
        b: &Barra,
        vazio: bool,
        na_fila: bool,
        rodada_nova: bool,
        envios: u64,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let estreita = b.faixa == FaixaCaixa::Estreita;
        let (grande, pequeno) = if estreita { (32., 28.) } else { (36., 32.) };
        let painel = b.painel;
        let parar = || {
            let (motor, chat) = (self.motor.clone(), b.chat.clone());
            move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                if let Some(m) = &motor {
                    m.update(cx, |m, cx| m.parar(&chat, cx));
                }
            }
        };
        let dica_parar = if na_fila { "Tirar da fila" } else { "Parar esta conversa" };
        let principal = |icone: Option<Lucide>, dica: &'static str| {
            BotaoRedondo::new(("enviar", painel as usize), icone.map(Into::into), dica)
                .variante(VarianteRedonda::Claro)
                .diametro(grande)
                .pulso(envios)
        };
        let enviar = |dica: &'static str| {
            principal(Some(Lucide::ArrowUp), dica)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.ativar_painel(painel, cx);
                    this.enviar_do_painel(painel, window, cx);
                }))
                .into_any_element()
        };
        let dica_enviar = if rodada_nova {
            "Enviar\nAbre uma nova rodada de ajuste, com verificação e revisor"
        } else {
            "Enviar"
        };
        match envio(b.rodando, vazio) {
            Envio::Vazio => vec![principal(Some(Lucide::ArrowUp), "Escreva uma mensagem").vazio(true).into_any_element()],
            Envio::Enviar => vec![enviar(dica_enviar)],
            Envio::Parar => vec![
                principal(None, dica_parar)
                    .dica_acao(Box::new(PararTurno), Some("Caixa"))
                    .on_click(parar())
                    .into_any_element(),
            ],
            Envio::PararEEnviar => vec![
                BotaoRedondo::new(("parar", painel as usize), None, dica_parar)
                    .variante(VarianteRedonda::Neutro)
                    .diametro(pequeno)
                    .dica_acao(Box::new(PararTurno), Some("Caixa"))
                    .on_click(parar())
                    .into_any_element(),
                enviar("Enviar\nEntra na fila deste turno"),
            ],
        }
    }

    /// Anel do uso do contexto (some antes do primeiro turno). O clique abre o uso da conversa:
    /// o contexto e o custo, o modelo, a conta e o Compactar.
    fn anel_do_contexto(&self, chat: &Chat, b: &Barra, cx: &Context<Self>) -> Option<AnyElement> {
        let usados = chat.contexto?;
        let (fracao, numero, janela) = uso_do_contexto(usados, chat.janela, chat.modelo_atual.as_deref());
        let estreita = b.faixa == FaixaCaixa::Estreita;
        let anel = Anel::new(("anel-contexto", b.painel as usize), fracao).diametro(if estreita { 20. } else { 22. });
        // O número acompanha o arco quando ele sai do destaque (âmbar a 70%, vermelho a 90%).
        let cor_arco = cor_da_faixa(fracao, Cores::destaque());
        let cor_numero = if cor_arco == Cores::destaque() { Cores::texto() } else { cor_arco };
        let dica: SharedString = format!(
            "Contexto: {} de {}\nO custo, o modelo e o Compactar",
            milhares(usados),
            tokens(janela)
        )
        .into();
        let lado = if estreita { 28. } else { 32. };
        let gatilho = Button::new(("contexto", b.painel as usize))
            .custom(variante_fantasma(cx))
            .size(r(lado))
            .px_0()
            .rounded(raio::PILULA)
            .tooltip(dica)
            .child(
                anel.centro(
                    div()
                        .text_size(r(if estreita { 9. } else { 10. }))
                        .line_height(r(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cor_numero)
                        .child(numero.to_string()),
                ),
            );
        let (motor, janela_e, chat_id, painel) = (self.motor.clone(), cx.entity().downgrade(), chat.id.clone(), b.painel);
        Some(
            popover(("pop-contexto", painel as usize))
                .anchor(Anchor::BottomRight)
                .offset(px(8.))
                .trigger(gatilho)
                .content(move |_, window, cx| {
                    let pop = cx.entity().downgrade();
                    let janela_e = janela_e.clone();
                    let compactar = botao_fantasma(
                        ("compactar", painel as usize),
                        Some(Lucide::Minimize.into()),
                        "Compactar a conversa",
                        TamanhoBotao::Barra,
                        window,
                        cx,
                    )
                    .on_click(move |_, window, cx| {
                        if let Some(pop) = pop.upgrade() {
                            pop.update(cx, |p, cx| p.dismiss(window, cx));
                        }
                        let _ = janela_e.update(cx, |this, cx| this.por_na_caixa(painel, "/compact ", window, cx));
                    });
                    let Some(m) = motor.as_ref().map(|m| m.read(cx)) else {
                        return div().into_any_element();
                    };
                    match m.chat(&chat_id) {
                        Some(c) => super::conversa::uso_da_conversa(m, c, compactar.into_any_element()).into_any_element(),
                        None => div().into_any_element(),
                    }
                })
                .into_any_element(),
        )
    }

    /// Escreve na caixa do painel e põe o foco nela (quem envia é você, com Enter).
    fn por_na_caixa(&mut self, painel: u64, texto: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.paineis.iter().find(|p| p.id == painel).map(|p| p.composer.clone()) else {
            return;
        };
        composer.update(cx, |c, cx| por_texto(c, texto.to_string(), window, cx));
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    /// Ctrl+Shift+M e Ctrl+E: abre o popover no painel ativo (com conversa), ou fecha se é o que
    /// está aberto. O foco passa antes pela caixa, para voltar a ela quando o popover fechar.
    pub(super) fn alternar_pop(&mut self, qual: PopCaixa, window: &mut Window, cx: &mut Context<Self>) {
        let Some(painel) = self.paineis.get(self.ativo).filter(|p| p.chat.is_some()) else {
            return;
        };
        let (id, chat, composer, lista) = (
            painel.id,
            painel.chat.clone().unwrap_or_default(),
            painel.composer.clone(),
            painel.busca_modelo.clone(),
        );
        if self.pop_caixa == Some((id, qual)) {
            self.pop_caixa = None;
            cx.notify();
            return;
        }
        let Some(motor) = self.motor.clone() else {
            return;
        };
        if qual == PopCaixa::Esforco
            && !usa_esforco(motor.read(cx).chat(&chat).and_then(|c| c.modelo_atual.as_deref()))
        {
            return;
        }
        if qual == PopCaixa::Modelo {
            seletores::preparar_modelos(&lista, &motor, &chat, window, cx);
        }
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        self.pop_caixa = Some((id, qual));
        cx.notify();
    }

    /// Os anexos da próxima mensagem, em chips acima do texto, alinhados com o primeiro chip.
    fn anexos_pendentes(
        &self,
        chat: &str,
        anexos: Vec<PathBuf>,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        div()
            .flex()
            .flex_wrap()
            .gap(esp::E6)
            .pb(esp::E8)
            .children(anexos.into_iter().enumerate().map(|(i, caminho)| {
                let chat = chat.to_string();
                let nome = caminho
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| caminho.display().to_string());
                let imagem = crate::claude::tipo_imagem(&caminho).is_some();
                div()
                    .flex()
                    .items_center()
                    .gap(esp::E6)
                    .h(r(28.))
                    .pl(esp::E8)
                    .pr(esp::E2)
                    .rounded(raio::PILULA)
                    .bg(Cores::hover())
                    .border_1()
                    .border_color(Cores::borda())
                    .child(
                        div()
                            .flex_none()
                            .text_color(Cores::tenue())
                            .child(super::componentes::icone(if imagem { Lucide::Image } else { Lucide::File }, 14., window)),
                    )
                    .child(
                        div()
                            .max_w(r(220.))
                            .truncate()
                            .metadado()
                            .text_color(Cores::texto())
                            .child(nome),
                    )
                    .child(
                        BotaoRedondo::new(("tirar-anexo", i), Some(Lucide::X.into()), "Remover o anexo")
                            .variante(VarianteRedonda::Plano)
                            .diametro(24.)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(lista) = this.anexos.get_mut(&chat)
                                    && i < lista.len()
                                {
                                    lista.remove(i);
                                }
                                cx.notify();
                            })),
                    )
            }))
    }

    /// As sugestões da última palavra da caixa: os comandos de barra (o texto começa com "/") e,
    /// no Mestre, os projetos (`#`) e os chats (`@`). Fechado com Esc, o menu fica fechado até o
    /// texto mudar.
    pub(super) fn sugestoes_da_caixa(&self, motor: &Motor, chat: &Chat, painel: u64, texto: &str) -> Option<(char, usize, Vec<Sugestao>)> {
        let (sinal, digitado, inicio) = gatilho(texto)?;
        if sinal != '/' && self.sugestao_fechada.as_ref().is_some_and(|(p, t)| *p == painel && t == texto) {
            return None;
        }
        let itens: Vec<Sugestao> = match sinal {
            '/' => self
                .comandos_de(chat)
                .into_iter()
                .map(|c| Sugestao {
                    valor: format!("/{c}"),
                    titulo: format!("/{c}"),
                    detalhe: descricao_comando(&c).map(str::to_string),
                    busca: c.clone(),
                    peso: c.len(),
                    chave: c,
                    projeto: None,
                })
                .collect(),
            '#' if chat.papel == Papel::Mestre => sugestoes_de_projetos(motor),
            '@' if chat.papel == Papel::Mestre => sugestoes_de_chats(motor),
            _ => return None,
        };
        let lista = menu_sugestoes(itens, digitado);
        (!lista.is_empty()).then_some((sinal, inicio, lista))
    }

    /// O menu das sugestões, acima da caixa. Entra como os popovers: fade e 6 px vindos de baixo,
    /// do lado da caixa. A primeira está marcada (o Enter a escolhe).
    fn menu_comandos(
        &self,
        motor: &Motor,
        chat: &Chat,
        composer: &Entity<TextareaState>,
        painel: u64,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let texto = composer.read(cx).value().to_string();
        let sugestoes = self.sugestoes_da_caixa(motor, chat, painel, &texto);
        div().relative().when_some(sugestoes, |s, (sinal, inicio, lista)| {
            let menu = div()
                .flex()
                .flex_col()
                .gap(esp::E2)
                .w_full()
                .p(esp::E8)
                .flutuante(cx)
                .children(lista.into_iter().enumerate().map(|(i, sugestao)| {
                    let simbolo = sugestao.projeto.as_ref().map(|p| {
                        if p.is_empty() {
                            div()
                                .flex_none()
                                .size(r(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(svg().path(Lucide::MessageCircle.path()).size(r(14.)).text_color(Cores::tenue()))
                                .into_any_element()
                        } else {
                            super::componentes::quadrado_do_projeto(motor, p, 18.).into_any_element()
                        }
                    });
                    let valor = sugestao.valor.clone();
                    div()
                        .id(("sugestao-caixa", i))
                        .flex()
                        .items_center()
                        .h(r(32.))
                        .px(esp::E8)
                        .gap(esp::E8)
                        .rounded(raio::CONTROLE)
                        .cursor_pointer()
                        .when(i == 0, |s| s.bg(Cores::selecao()))
                        .when(i != 0, |s| s.hover(|s| s.bg(Cores::hover())))
                        .children(simbolo)
                        .child(
                            div()
                                .flex_shrink_0()
                                .max_w(relative(0.6))
                                .truncate()
                                .map(|d| if sinal == '/' { d.mono().text_color(Cores::codigo()) } else { d.corpo_ui().text_color(Cores::texto()) })
                                .child(sugestao.titulo.clone()),
                        )
                        .when_some(sugestao.detalhe.clone(), |s, d| {
                            s.child(div().flex_1().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(d))
                        })
                        .on_click(cx.listener(move |this, _, window, cx| this.escolher_sugestao(painel, inicio, &valor, window, cx)))
                }))
                .with_animation(
                    ("entra-comandos", painel as usize),
                    movimento::animacao(movimento::ENTRAR, Curva::Entrar),
                    |menu, t| menu.opacity(t).top(r(6. * (1. - t))),
                );
            // O absoluto fica por fora: o deslize mexe no `top` de quem está dentro.
            s.child(div().absolute().bottom(esp::E4).left_0().w_full().max_w(r(420.)).child(menu))
        })
    }

    /// Troca a última palavra da caixa pela sugestão escolhida (o comando, `#chave ` ou `@12 `).
    pub(super) fn escolher_sugestao(&mut self, painel: u64, inicio: usize, valor: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.paineis.iter().find(|p| p.id == painel).map(|p| p.composer.clone()) else {
            return;
        };
        // A palavra é medida de novo no texto de agora (o clique pode vir depois de outra tecla).
        let atual = composer.read(cx).value().to_string();
        let inicio = gatilho(&atual).map_or(inicio, |(_, _, i)| i);
        let texto = completar(&atual, inicio, valor);
        composer.update(cx, |c, cx| por_texto(c, texto, window, cx));
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    /// O Enter com o menu de `#`/`@` aberto no painel ativo: escolhe a primeira sugestão.
    pub(super) fn completar_citacao(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let (Some(motor), Some(painel)) = (self.motor.clone(), self.paineis.get(self.ativo)) else {
            return false;
        };
        let (id, texto) = (painel.id, painel.composer.read(cx).value().to_string());
        let escolha = {
            let m = motor.read(cx);
            let Some(chat) = painel.chat.as_deref().and_then(|c| m.chat(c)) else {
                return false;
            };
            self.sugestoes_da_caixa(m, chat, id, &texto).filter(|(sinal, ..)| *sinal != '/').map(|(_, inicio, lista)| (inicio, lista[0].valor.clone()))
        };
        match escolha {
            Some((inicio, valor)) => {
                self.escolher_sugestao(id, inicio, &valor, window, cx);
                true
            }
            None => false,
        }
    }

    /// "Mandar para um projeto…" e "Falar de um chat…" do clipe: o sinal no fim da caixa, depois
    /// de um espaço, com o menu aberto.
    pub(super) fn acrescentar_gatilho(&mut self, painel: u64, sinal: char, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.paineis.iter().find(|p| p.id == painel).map(|p| p.composer.clone()) else {
            return;
        };
        let antes = composer.read(cx).value().to_string();
        let texto = if antes.is_empty() || antes.ends_with(char::is_whitespace) { format!("{antes}{sinal}") } else { format!("{antes} {sinal}") };
        self.sugestao_fechada = None;
        self.por_na_caixa(painel, &texto, window, cx);
    }

    pub(super) fn comandos_de(&self, chat: &Chat) -> Vec<String> {
        let mut lista: Vec<String> = if chat.comandos.is_empty() {
            COMANDOS_INICIAIS.iter().map(|c| c.to_string()).collect()
        } else {
            chat.comandos.clone()
        };
        if !lista.iter().any(|c| c == "clear") {
            lista.insert(0, "clear".into());
        }
        lista
    }

    /// O texto digitado depois da barra, enquanto ainda é o nome de um comando.
    pub(super) fn prefixo_comando(&self, cx: &App) -> Option<String> {
        prefixo_de(&self.composer_ativo()?, cx)
    }

    pub(super) fn completar_comando(&mut self, comando: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.composer_ativo() else {
            return;
        };
        composer.update(cx, |c, cx| por_texto(c, format!("/{comando} "), window, cx));
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    /// Esc na caixa: com o menu "/" aberto, fecha o menu; com o chat trabalhando (ou na fila),
    /// para. Fora disso, a tecla segue adiante.
    fn esc_na_caixa(
        &mut self,
        chat: &str,
        composer: &Entity<TextareaState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(motor) = self.motor.clone() else {
            cx.propagate();
            return;
        };
        let texto = composer.read(cx).value().to_string();
        let painel = self.paineis.iter().find(|p| p.composer == *composer).map(|p| p.id);
        let (menu_aberto, citacao, ocupado) = {
            let m = motor.read(cx);
            let menu = match (prefixo_de(composer, cx), m.chat(chat)) {
                (Some(f), Some(c)) => self.comandos_de(c).iter().any(|x| x.contains(f.as_str())),
                _ => false,
            };
            let citacao = match (painel, m.chat(chat)) {
                (Some(p), Some(c)) => self.sugestoes_da_caixa(m, c, p, &texto).is_some_and(|(sinal, ..)| sinal != '/'),
                _ => false,
            };
            (menu, citacao, m.execucao(chat).rodando() || m.esperando_vaga(chat))
        };
        if citacao {
            // O menu de # e @ fecha sem apagar o que foi escrito.
            self.sugestao_fechada = painel.map(|p| (p, texto));
            cx.notify();
        } else if menu_aberto {
            composer.update(cx, |c, cx| c.set_value("", window, cx));
            cx.notify();
        } else if ocupado {
            motor.update(cx, |m, cx| m.parar(chat, cx));
        } else {
            cx.propagate();
        }
    }

    /// Shift+Tab na caixa: o próximo modo de permissão, na ordem do menu. Vale a partir do
    /// próximo turno, como no menu.
    fn alternar_modo(&mut self, chat: &str, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let atual = {
            let m = motor.read(cx);
            m.chat(chat).map(|c| m.permissao_de(c))
        };
        if let Some(atual) = atual {
            let proximo = proximo_modo(&atual);
            motor.update(cx, |m, cx| m.mudar_chat(chat, |c| c.permissao = Some(proximo.into()), cx));
        }
    }

    fn escolher_anexos(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        let escolha = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Anexar".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(caminhos))) = escolha.await {
                let _ = this.update(cx, |this, cx| {
                    this.anexos.entry(chat).or_default().extend(caminhos);
                    cx.notify();
                });
            }
        })
        .detach();
    }
}

fn descricao_comando(comando: &str) -> Option<&'static str> {
    Some(match comando {
        "clear" => "Começa uma sessão nova (o Claude esquece a conversa)",
        "compact" => "Resume a conversa para liberar contexto",
        "context" => "Mostra o uso do contexto",
        "usage" => "Uso do plano",
        "init" => "Cria o CLAUDE.md do projeto",
        "review" => "Revisa as mudanças",
        "security-review" => "Revisão de segurança das mudanças",
        "simplify" => "Simplifica o código alterado",
        "model" => "Troca o modelo",
        "effort" => "Troca o esforço",
        "code-review" => "Revisão de código",
        "debug" => "Ajuda a depurar",
        "verify" => "Verifica o trabalho",
        _ => return None,
    })
}

/// Os projetos para o `#` do Mestre (na ordem do trilho) e Avulsos no fim.
fn sugestoes_de_projetos(motor: &Motor) -> Vec<Sugestao> {
    let mut lista: Vec<Sugestao> = motor
        .projetos_visiveis()
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let descricao = p.descricao.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string);
            Sugestao {
                valor: format!("#{}", p.chave()),
                titulo: p.nome.clone(),
                busca: crate::dados::chave(&format!("{} {}", p.nome, descricao.clone().unwrap_or_default())),
                detalhe: descricao,
                chave: p.chave(),
                peso: i,
                projeto: Some(p.id.clone()),
            }
        })
        .collect();
    lista.push(Sugestao {
        valor: "#avulsos".into(),
        titulo: "Avulsos".into(),
        detalhe: Some("chats sem projeto".into()),
        chave: "avulsos".into(),
        busca: "avulsos sem projeto".into(),
        peso: lista.len(),
        projeto: Some(String::new()),
    });
    lista
}

/// Os chats para o `@` do Mestre: "chat 12 · título", com o projeto e o estado, o mais recente
/// primeiro.
fn sugestoes_de_chats(motor: &Motor) -> Vec<Sugestao> {
    let mut chats: Vec<&Chat> = motor
        .chats
        .iter()
        .filter(|c| matches!(c.papel, Papel::Projeto | Papel::Trabalhador) && c.tarefa.is_some() && !motor.arquivada(c))
        .collect();
    chats.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
    chats
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let n = c.tarefa.unwrap_or_default();
            let estado = if motor.execucao(&c.id).rodando() {
                "rodando".to_string()
            } else if motor.espera_do_chat(c).is_some_and(|e| e != motor::Espera::Nova) {
                "esperando você".to_string()
            } else {
                super::tempo_relativo(c.atualizado_em)
            };
            let projeto = motor.nome_do_projeto(&c.projeto);
            Sugestao {
                valor: format!("@{n}"),
                titulo: format!("chat {n} · {}", c.titulo),
                detalhe: Some(format!("{projeto} · {estado}")),
                chave: n.to_string(),
                busca: crate::dados::chave(&format!("{} {projeto}", c.titulo)),
                peso: i,
                projeto: Some(c.projeto.clone()),
            }
        })
        .collect()
}

/// Uma regra de "permitir sempre" como aparece no menu do modo.
fn descricao_regra(regra: &str) -> String {
    if regra == "edicoes" {
        return "Edições de arquivos neste chat".into();
    }
    if let Some(comando) = regra.strip_prefix("bash:") {
        let linha = comando.lines().next().unwrap_or_default();
        return if linha.chars().count() > 48 || comando.contains('\n') {
            format!("$ {}…", linha.chars().take(48).collect::<String>())
        } else {
            format!("$ {linha}")
        };
    }
    regra.strip_prefix("ferramenta:").unwrap_or(regra).to_string()
}

/// Em janela baixa, a caixa cresce menos antes de rolar por dentro, para não espremer as
/// mensagens e o plano; muda no quadro seguinte (avisar a janela durante o desenho não a
/// redesenha).
fn ajustar_linhas(
    janela: WeakEntity<Janela>,
    painel: u64,
    linhas: usize,
) -> impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static {
    move |_, window, _| {
        let agora = linhas_da_caixa(window.viewport_size().height / window.rem_size());
        if agora == linhas {
            return;
        }
        window.on_next_frame(move |_, cx| {
            let _ = janela.update(cx, |this, cx| {
                if let Some(p) = this.paineis.iter_mut().find(|p| p.id == painel) {
                    p.linhas_caixa = agora;
                    p.composer.update(cx, |c, cx| c.set_auto_grow(1, agora, cx));
                    cx.notify();
                }
            });
        });
    }
}

/// Quantas linhas a caixa pode ter com a janela desta altura (em rem): o resto do cartão, o
/// plano e umas linhas de mensagem levam uns 26 rem (420 px); cada linha, 1,5 rem.
fn linhas_da_caixa(altura: f32) -> usize {
    ((altura - 26.25) / 1.5).floor().clamp(4., LINHAS_CAIXA as f32) as usize
}

/// O modo seguinte na ordem do menu, voltando ao primeiro depois do último.
fn proximo_modo(atual: &str) -> &'static str {
    let i = MODOS
        .iter()
        .position(|(v, _, _)| *v == atual)
        .unwrap_or(MODOS.len() - 1);
    MODOS[(i + 1) % MODOS.len()].0
}

/// Rótulo do seletor de modelo: o modelo que vale de verdade. No padrão, o que o Claude Code usou
/// no último turno; num apelido ("opus"), a versão em uso, se for da mesma família.
fn rotulo_modelo(escolhido: &str, em_uso: Option<&str>) -> String {
    let em_uso = em_uso.filter(|m| !m.is_empty()).map(nome_curto_modelo);
    if escolhido.is_empty() {
        return em_uso.unwrap_or_else(|| "Padrão".into());
    }
    match MODELOS.iter().find(|(v, _)| *v == escolhido) {
        Some((_, nome)) => em_uso
            .filter(|m| m.to_lowercase().starts_with(escolhido))
            .unwrap_or_else(|| nome.to_string()),
        // Conversas antigas guardaram o nome completo do modelo, que não está na lista.
        None => nome_curto_modelo(escolhido),
    }
}

/// Imagens coladas viram arquivos na pasta dos anexos (`anexos/` nos dados); arquivos copiados do
/// gerenciador entram pelo caminho.
fn anexos_da_area_de_transferencia(item: &ClipboardItem, pasta: &Path) -> Vec<PathBuf> {
    let mut caminhos = Vec::new();
    for entrada in item.entries() {
        match entrada {
            ClipboardEntry::ExternalPaths(lista) => caminhos.extend(lista.paths().iter().cloned()),
            ClipboardEntry::Image(imagem) => {
                let extensao = match imagem.format {
                    ImageFormat::Png => "png",
                    ImageFormat::Jpeg => "jpg",
                    ImageFormat::Gif => "gif",
                    ImageFormat::Webp => "webp",
                    _ => continue,
                };
                let arquivo = pasta.join(format!(
                    "colado-{}.{extensao}",
                    crate::modelo::novo_id()
                ));
                if std::fs::create_dir_all(pasta).is_ok()
                    && std::fs::write(&arquivo, &imagem.bytes).is_ok()
                {
                    caminhos.push(arquivo);
                }
            }
            ClipboardEntry::String(_) => {}
        }
    }
    caminhos
}

/// O texto depois da barra, enquanto ainda é o nome de um comando.
fn prefixo_de(composer: &Entity<TextareaState>, cx: &App) -> Option<String> {
    let texto = composer.read(cx).value().to_string();
    let resto = texto.strip_prefix('/')?;
    (!resto.contains(char::is_whitespace)).then(|| resto.to_string())
}

#[cfg(test)]
mod testes_sugestoes {
    use super::{Sugestao, completar, gatilho, menu_sugestoes};

    fn sugestao(valor: &str, chave: &str, busca: &str, peso: usize) -> Sugestao {
        Sugestao {
            valor: valor.into(),
            titulo: valor.into(),
            detalhe: None,
            chave: chave.into(),
            busca: busca.into(),
            peso,
            projeto: None,
        }
    }

    #[test]
    fn o_gatilho_e_a_ultima_palavra() {
        assert_eq!(gatilho("/comp"), Some(('/', "comp", 0)));
        assert_eq!(gatilho("/compact agora"), None);
        assert_eq!(gatilho("veja a /barra"), None);
        assert_eq!(gatilho("#nin"), Some(('#', "nin", 0)));
        assert_eq!(gatilho("mande para #nin"), Some(('#', "nin", 11)));
        assert_eq!(gatilho("fale do @"), Some(('@', "", 8)));
        assert_eq!(gatilho("fale do @12 e"), None);
        // No meio da palavra não é citação.
        assert_eq!(gatilho("C#"), None);
        assert_eq!(gatilho("eu@x.com"), None);
        assert_eq!(gatilho("ação #lój"), Some(('#', "lój", 7)));
    }

    #[test]
    fn completar_troca_so_o_ultimo_token() {
        assert_eq!(completar("fale do @1", 8, "@12"), "fale do @12 ");
        assert_eq!(completar("#nin", 0, "#ninee"), "#ninee ");
        assert_eq!(completar("/comp", 0, "/compact"), "/compact ");
    }

    #[test]
    fn quem_comeca_com_o_digitado_vem_antes_e_no_maximo_oito() {
        let itens = vec![
            sugestao("#site-loja", "site-loja", "site-loja loja do site", 0),
            sugestao("#loja", "loja", "loja loja", 1),
            sugestao("#vitral", "vitral", "vitral", 2),
        ];
        let achadas: Vec<String> = menu_sugestoes(itens.clone(), "loj").into_iter().map(|s| s.valor).collect();
        assert_eq!(achadas, ["#loja", "#site-loja"]);
        // Sem acento nem maiúscula; vazio mostra todas, pelo peso.
        assert_eq!(menu_sugestoes(itens.clone(), "LÓJ").len(), 2);
        let todas: Vec<String> = menu_sugestoes(itens, "").into_iter().map(|s| s.valor).collect();
        assert_eq!(todas, ["#site-loja", "#loja", "#vitral"]);
        let muitas: Vec<Sugestao> = (0..12).map(|i| sugestao(&format!("@{i}"), &i.to_string(), "", i)).collect();
        assert_eq!(menu_sugestoes(muitas.clone(), "").len(), 8);
        // O número do chat: "@1" traz o 1 e os que começam com 1 antes dos que só o contêm.
        let achados: Vec<String> = menu_sugestoes(muitas, "1").into_iter().map(|s| s.valor).collect();
        assert_eq!(achados, ["@1", "@10", "@11"]);
    }
}

#[cfg(test)]
mod testes_caixa {
    use super::{
        Envio, LINHAS_CAIXA, MODOS, com_claude, envio, gastar_chave, indice_esforco, linhas_da_caixa, milhares,
        nomes_dos_niveis, proximo_modo, rotulo_modelo, tokens, usa_esforco, uso_do_contexto,
    };

    #[test]
    fn chave_da_proxima_mensagem_desliga_depois_de_usar() {
        let mut armados = std::collections::HashSet::from(["c1".to_string()]);
        // Comando de barra não gasta.
        assert!(!gastar_chave(&mut armados, "c1", "  /compact"));
        assert!(armados.contains("c1"));
        // Outra conversa não mexe nesta.
        assert!(!gastar_chave(&mut armados, "c2", "oi"));
        assert!(gastar_chave(&mut armados, "c1", "refatore o motor"));
        assert!(armados.is_empty());
        // Só uma vez: a mensagem seguinte vai sem.
        assert!(!gastar_chave(&mut armados, "c1", "e os testes"));
    }

    #[test]
    fn caixa_cresce_menos_em_janela_baixa() {
        // Altura da janela em rem (16 px): 920 px, 750, 700, 600 e uma de 400.
        assert_eq!(linhas_da_caixa(57.5), LINHAS_CAIXA);
        assert_eq!(linhas_da_caixa(46.875), LINHAS_CAIXA);
        assert_eq!(linhas_da_caixa(43.75), 11);
        assert_eq!(linhas_da_caixa(37.5), 7);
        assert_eq!(linhas_da_caixa(25.), 4);
    }

    #[test]
    fn modelo_mostra_o_que_vale() {
        assert_eq!(rotulo_modelo("", None), "Padrão");
        assert_eq!(rotulo_modelo("", Some("")), "Padrão");
        assert_eq!(rotulo_modelo("", Some("claude-opus-5-5")), "Opus 5.5");
        assert_eq!(rotulo_modelo("opus", Some("claude-opus-5-5")), "Opus 5.5");
        // Trocou para outro modelo e o turno ainda não rodou: o nome da lista.
        assert_eq!(rotulo_modelo("sonnet", Some("claude-opus-5-5")), "Sonnet");
        assert_eq!(rotulo_modelo("haiku", None), "Haiku");
        assert_eq!(rotulo_modelo("claude-sonnet-4-5-20250929", None), "Sonnet 4.5");
    }

    #[test]
    fn prefixo_claude_so_nos_modelos_do_claude() {
        assert_eq!(com_claude("Opus 5.5"), "Claude Opus 5.5");
        assert_eq!(com_claude("Opus 5"), "Claude Opus 5");
        assert_eq!(com_claude("Haiku"), "Claude Haiku");
        assert_eq!(com_claude("Padrão"), "Padrão");
        assert_eq!(com_claude("gpt-5"), "gpt-5");
    }

    #[test]
    fn shift_tab_percorre_os_modos_em_ciclo() {
        assert_eq!(proximo_modo("manual"), "acceptEdits");
        assert_eq!(proximo_modo("auto"), "bypassPermissions");
        assert_eq!(proximo_modo("bypassPermissions"), "manual");
        // Valor desconhecido conta como "Sem perguntar", como em nome_do_modo.
        assert_eq!(proximo_modo("default"), "manual");
        let mut modo = "manual";
        for _ in 0..MODOS.len() {
            modo = proximo_modo(modo);
        }
        assert_eq!(modo, "manual");
    }

    #[test]
    fn niveis_de_esforco() {
        assert_eq!(indice_esforco(""), 0);
        assert_eq!(indice_esforco("medium"), 2);
        assert_eq!(indice_esforco("max"), 5);
        assert_eq!(indice_esforco("turbo"), 0);
        assert_eq!(nomes_dos_niveis(false)[5], "Máximo");
        assert_eq!(nomes_dos_niveis(true)[5], "Máx");
        assert_eq!(nomes_dos_niveis(true)[4], "Extra");
        assert!(usa_esforco(Some("claude-opus-5-5")));
        assert!(usa_esforco(None));
        assert!(!usa_esforco(Some("claude-haiku-4-5")));
    }

    #[test]
    fn contexto_usa_a_janela_padrao_antes_do_primeiro_result() {
        // Opus 5 tem 1M: 300 mil são 30%, não 150%.
        let (fracao, numero, janela) = uso_do_contexto(300_000, None, Some("claude-opus-5-5"));
        assert_eq!(janela, 1_000_000);
        assert_eq!(numero, 30);
        assert!((fracao - 0.3).abs() < 1e-6);
        let (_, numero, janela) = uso_do_contexto(150_000, None, Some("claude-haiku-4-5"));
        assert_eq!((numero, janela), (75, 200_000));
        // A janela informada pelo Claude Code vale mais que o palpite.
        assert_eq!(uso_do_contexto(100_000, Some(200_000), Some("claude-opus-5-5")).1, 50);
        // O número do meio para em 99.
        assert_eq!(uso_do_contexto(400_000, Some(200_000), None).1, 99);
        assert_eq!(milhares(123_456), "123 mil");
        assert_eq!(tokens(200_000), "200 mil tokens");
        assert_eq!(tokens(1_000_000), "1 milhão de tokens");
        assert_eq!(tokens(2_000_000), "2 milhões de tokens");
    }

    #[test]
    fn enviar_vira_parar_com_o_chat_rodando() {
        assert_eq!(envio(false, true), Envio::Vazio);
        assert_eq!(envio(false, false), Envio::Enviar);
        assert_eq!(envio(true, true), Envio::Parar);
        assert_eq!(envio(true, false), Envio::PararEEnviar);
    }
}

#[cfg(test)]
mod testes_nome_modelo {
    use super::nome_curto_modelo;

    #[test]
    fn encurta_os_nomes_do_claude() {
        assert_eq!(nome_curto_modelo("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(nome_curto_modelo("claude-sonnet-4-5-20250929"), "Sonnet 4.5");
        assert_eq!(nome_curto_modelo("claude-haiku-4-5[1m]"), "Haiku 4.5 (1M)");
        assert_eq!(nome_curto_modelo("claude-3-5-sonnet-20241022"), "Sonnet 3.5");
        assert_eq!(nome_curto_modelo("claude-fable"), "Fable");
    }

    #[test]
    fn deixa_o_resto_como_veio() {
        assert_eq!(nome_curto_modelo(""), "");
        assert_eq!(nome_curto_modelo("opus"), "opus");
        assert_eq!(nome_curto_modelo("gpt-5"), "gpt-5");
        assert_eq!(nome_curto_modelo("claude-"), "claude-");
        assert_eq!(nome_curto_modelo("claude-opus-mais-5"), "claude-opus-mais-5");
    }
}
