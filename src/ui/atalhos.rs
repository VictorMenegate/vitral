//! A tabela de atalhos do app: a mesma lista registra as teclas no keymap e preenche o diálogo
//! "Atalhos do teclado", então o que a tela mostra nunca desencontra do que o teclado faz. Cada
//! linha diz as teclas, a ação, o contexto, a parte do app e o que ela faz; rótulo vazio deixa a
//! linha fora da lista (a segunda tecla de uma mesma coisa, como o Ctrl++ do zoom).

use gpui_kit::component::{StyledExt as _, WindowExt as _, kbd::Kbd, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    busca_conversa,
    componentes::{CabecalhoDialogo, RodapeDialogo, TamanhoBotao, botao_primario, desenho_teclas, largura},
    composer, popover_issues, revisao, visor_imagem,
};
use crate::tema::{Cores, Tipo as _, esp, r, raio};

/// A parte do app em que o atalho vale, na ordem das seções do diálogo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grupo {
    Conversa,
    Lista,
    Paineis,
    Revisao,
    Janela,
}

impl Grupo {
    /// Na ordem em que as seções aparecem.
    pub const TODOS: [Grupo; 5] = [Grupo::Conversa, Grupo::Lista, Grupo::Paineis, Grupo::Revisao, Grupo::Janela];

    pub fn titulo(self) -> &'static str {
        match self {
            Grupo::Conversa => "Na conversa",
            Grupo::Lista => "Conversas e projetos",
            Grupo::Paineis => "Painéis",
            Grupo::Revisao => "Revisão",
            Grupo::Janela => "Janela",
        }
    }
}

/// Uma linha da tabela, do jeito que a lista a mostra.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Linha {
    /// A combinação como vai para o keymap ("ctrl-shift-n").
    pub teclas: &'static str,
    /// O que a lista desenha no lugar das teclas, separado por "+" ("Ctrl+1…9"); vazio desenha
    /// a própria combinação.
    pub exibir: &'static str,
    /// O que o atalho faz; vazio deixa a linha fora da lista.
    pub rotulo: &'static str,
    pub grupo: Grupo,
}

impl Linha {
    /// As teclas uma a uma, para o desenho de tecla: o `exibir`, quando há, ou a combinação lida
    /// pelo kit (ctrl-shift-n vira Ctrl, Shift, N).
    pub fn partes(&self) -> Vec<SharedString> {
        if !self.exibir.is_empty() {
            return self.exibir.split('+').map(SharedString::from).collect();
        }
        match Keystroke::parse(self.teclas) {
            Ok(tecla) => Kbd::new(tecla).partes(),
            Err(_) => vec![SharedString::from(self.teclas)],
        }
    }
}

/// Registra as teclas no keymap e devolve as linhas da lista, de uma tabela só. A ordem importa:
/// uma combinação registrada depois ganha de uma anterior com o mesmo alcance (o Shift+Tab da
/// caixa ganha do recuo de linha do campo).
macro_rules! tabela {
    ($($teclas:literal, $acao:expr, $contexto:expr, $grupo:expr, $rotulo:expr, $exibir:expr;)*) => {
        /// Liga todos os atalhos do app (chamada uma vez, na abertura).
        pub fn ligar(cx: &mut App) {
            cx.bind_keys([$(KeyBinding::new($teclas, $acao, $contexto)),*]);
        }

        /// As linhas da tabela, na ordem em que foram registradas.
        pub fn linhas() -> Vec<Linha> {
            vec![$(Linha { teclas: $teclas, exibir: $exibir, rotulo: $rotulo, grupo: $grupo }),*]
        }
    };
}

tabela! {
    // ---------- na conversa ----------
    "escape", composer::PararTurno, Some("Caixa"), Grupo::Conversa, "Parar o turno (ou fechar o menu “/”)", "";
    // Do detalhe das issues volta à lista; da lista, fecha (antes do Esc do popover do kit).
    "escape", popover_issues::VoltarIssues, Some("PopoverIssues"), Grupo::Conversa, "", "";
    // Registrado depois do shift-tab do campo (recuar a linha), ganha dele dentro da caixa. Só
    // no campo da mensagem: a busca de um popover da caixa continua recuando.
    "shift-tab", composer::AlternarModo, Some("Composer > Input"), Grupo::Conversa, "Passar para o próximo modo de permissão", "";
    // Só com a caixa vazia (o "Vazia" sai do contexto quando há texto): a seta continua movendo
    // o cursor enquanto se escreve.
    "up", super::EditarUltima, Some("Vazia > Input"), Grupo::Conversa, "Corrigir a última mensagem (caixa vazia)", "↑";
    // O campo do kit liga o Ctrl+F ao `Search` dele, que ninguém atende: o evento segue para cá.
    "ctrl-f", busca_conversa::BuscarNaConversa, None, Grupo::Conversa, "Buscar nesta conversa", "";
    "f3", busca_conversa::AchadoSeguinte, None, Grupo::Conversa, "Ir ao achado seguinte", "";
    "shift-f3", busca_conversa::AchadoAnterior, None, Grupo::Conversa, "Ir ao achado anterior", "";
    "escape", busca_conversa::FecharBuscaConversa, Some("BuscaConversa"), Grupo::Conversa, "", "";
    "ctrl-shift-m", composer::AbrirModelo, Some("Vitral"), Grupo::Conversa, "Escolher o modelo", "";
    "ctrl-e", composer::AbrirEsforco, Some("Vitral"), Grupo::Conversa, "Escolher o esforço", "";
    // Dentro do popover de esforço (Lote 1).
    "left", gpui_kit::base::actions::SelectLeft, Some("Esforco"), Grupo::Conversa, "", "";
    "right", gpui_kit::base::actions::SelectRight, Some("Esforco"), Grupo::Conversa, "", "";
    "home", gpui_kit::base::actions::SelectFirst, Some("Esforco"), Grupo::Conversa, "", "";
    "end", gpui_kit::base::actions::SelectLast, Some("Esforco"), Grupo::Conversa, "", "";
    "space", composer::AlternarUltrathink, Some("Esforco"), Grupo::Conversa, "Ligar o Ultrathink (no popover de esforço)", "";
    "u", composer::AlternarUltracode, Some("Esforco"), Grupo::Conversa, "Ligar o Ultracode (no popover de esforço)", "";
    "enter", gpui_kit::base::actions::Confirm { secondary: false }, Some("Esforco"), Grupo::Conversa, "", "";
    "ctrl-end", super::IrAoFim, None, Grupo::Conversa, "Ir ao fim da conversa", "";
    // No visor de imagem (o Esc que fecha é o do diálogo).
    "left", visor_imagem::ImagemAnterior, Some("Visor"), Grupo::Conversa, "Imagem anterior e seguinte no visor", "←+→";
    "right", visor_imagem::ImagemSeguinte, Some("Visor"), Grupo::Conversa, "", "";

    // ---------- conversas e projetos ----------
    "ctrl-n", super::NovaConversa, None, Grupo::Lista, "Nova conversa", "";
    "ctrl-shift-n", super::NovoProjeto, None, Grupo::Lista, "Novo projeto", "";
    "ctrl-k", super::Buscar, None, Grupo::Lista, "Buscar conversas e projetos", "";
    "ctrl-b", super::AlternarLista, None, Grupo::Lista, "Mostrar ou recolher a lista", "";
    "ctrl-m", super::AbrirMestre, None, Grupo::Lista, "Falar com o Mestre", "";
    "ctrl-g", super::ProximaPendencia, None, Grupo::Lista, "Ir à próxima pendência", "";
    "ctrl-1", super::IrParaProjeto(0), None, Grupo::Lista, "Filtrar pelo projeto 1 a 9 do trilho", "Ctrl+1…9";
    "ctrl-2", super::IrParaProjeto(1), None, Grupo::Lista, "", "";
    "ctrl-3", super::IrParaProjeto(2), None, Grupo::Lista, "", "";
    "ctrl-4", super::IrParaProjeto(3), None, Grupo::Lista, "", "";
    "ctrl-5", super::IrParaProjeto(4), None, Grupo::Lista, "", "";
    "ctrl-6", super::IrParaProjeto(5), None, Grupo::Lista, "", "";
    "ctrl-7", super::IrParaProjeto(6), None, Grupo::Lista, "", "";
    "ctrl-8", super::IrParaProjeto(7), None, Grupo::Lista, "", "";
    "ctrl-9", super::IrParaProjeto(8), None, Grupo::Lista, "", "";

    // ---------- painéis ----------
    "ctrl-j", super::AlternarPainel, None, Grupo::Paineis, "Mostrar ou fechar o painel da direita", "";
    "alt-1", super::IrParaPainel1, None, Grupo::Paineis, "Ir ao painel 1 a 4", "Alt+1…4";
    "alt-2", super::IrParaPainel2, None, Grupo::Paineis, "", "";
    "alt-3", super::IrParaPainel3, None, Grupo::Paineis, "", "";
    "alt-4", super::IrParaPainel4, None, Grupo::Paineis, "", "";
    "escape", super::FecharGaveta, Some("Gaveta"), Grupo::Paineis, "Recolher a gaveta", "";
    "ctrl-w", super::FecharPainel, None, Grupo::Paineis, "Fechar a aba à vista e, na última, o painel (a conversa fica na lista)", "";
    "ctrl-pageup", super::PainelAnterior, None, Grupo::Paineis, "Painel anterior", "";
    "ctrl-pagedown", super::PainelSeguinte, None, Grupo::Paineis, "Painel seguinte", "";
    "ctrl-shift-pageup", super::MoverPainelEsquerda, None, Grupo::Paineis, "Mover o painel para a esquerda", "";
    "ctrl-shift-pagedown", super::MoverPainelDireita, None, Grupo::Paineis, "Mover o painel para a direita", "";

    // ---------- revisão ----------
    "f7", revisao::MudancaSeguinte, Some("Revisao"), Grupo::Revisao, "Mudança seguinte", "";
    "shift-f7", revisao::MudancaAnterior, Some("Revisao"), Grupo::Revisao, "Mudança anterior", "";
    // Ficam em contextos da revisão: o Ctrl+PgUp/PgDn ganha do painel vizinho só aqui dentro.
    "ctrl-pagedown", revisao::ArquivoSeguinte, Some("Revisao"), Grupo::Revisao, "Arquivo seguinte", "";
    "ctrl-pageup", revisao::ArquivoAnterior, Some("Revisao"), Grupo::Revisao, "Arquivo anterior", "";
    "enter", revisao::AbrirRascunho, Some("Revisao > Diff"), Grupo::Revisao, "Comentar as linhas escolhidas", "";
    "c", revisao::AbrirRascunho, Some("Revisao > Diff"), Grupo::Revisao, "", "";
    "ctrl-enter", revisao::AcaoPrincipal, Some("Revisao"), Grupo::Revisao, "Fazer a ação principal do rodapé", "";

    // ---------- janela ----------
    "ctrl-=", super::AumentarZoom, None, Grupo::Janela, "Aumentar o zoom", "";
    "ctrl-+", super::AumentarZoom, None, Grupo::Janela, "", "";
    "ctrl-add", super::AumentarZoom, None, Grupo::Janela, "", "";
    "ctrl--", super::DiminuirZoom, None, Grupo::Janela, "Diminuir o zoom", "";
    "ctrl-subtract", super::DiminuirZoom, None, Grupo::Janela, "", "";
    "ctrl-0", super::ZoomPadrao, None, Grupo::Janela, "Zoom de 100%", "";
    "f10", super::AbrirMenuVitral, None, Grupo::Janela, "Abrir o menu do Vitral", "";
    "ctrl-,", super::AbrirConfiguracoes, None, Grupo::Janela, "Abrir as Configurações", "";
    // Fora de campo de texto (o "!Input" tira a tecla de quem está escrevendo).
    "?", super::AbrirAtalhos, Some("Vitral && !Input"), Grupo::Janela, "Esta lista de atalhos", "";
}

/// Teclas que não são do keymap, mas que todo mundo procura nesta lista: a caixa envia no Enter
/// (`submit_on_enter`), manda depois no Ctrl+Enter e quebra a linha no Shift+Enter, e o Ctrl+clique
/// abre ao lado.
const FIXAS: [(&str, &str, Grupo); 4] = [
    ("Enter", "Enviar a mensagem", Grupo::Conversa),
    ("Ctrl+Enter", "Mandar depois: com o turno rodando, a mensagem espera na fila", Grupo::Conversa),
    ("Shift+Enter", "Quebrar a linha sem enviar", Grupo::Conversa),
    ("Ctrl+clique", "Abrir a conversa num painel ao lado", Grupo::Lista),
];

/// As linhas de um grupo, na ordem da tabela: primeiro as do keymap com rótulo, depois as fixas.
pub fn do_grupo(grupo: Grupo) -> Vec<(Vec<SharedString>, &'static str)> {
    let mut linhas: Vec<(Vec<SharedString>, &'static str)> = linhas()
        .into_iter()
        .filter(|l| l.grupo == grupo && !l.rotulo.is_empty())
        .map(|l| (l.partes(), l.rotulo))
        .collect();
    linhas.extend(
        FIXAS
            .iter()
            .filter(|(.., g)| *g == grupo)
            .map(|(teclas, rotulo, _)| (teclas.split('+').map(SharedString::from).collect(), *rotulo)),
    );
    linhas
}

/// O diálogo "Atalhos do teclado": as seções na ordem dos grupos, em duas colunas quando cabe.
/// Abre com `?` fora de um campo de texto e pelo menu do logo.
pub fn abrir(window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialogo, window, cx| {
        let duas = !super::dialogos::estreito(window, largura::G);
        let secoes: Vec<AnyElement> = Grupo::TODOS
            .iter()
            .map(|grupo| secao(*grupo, do_grupo(*grupo)))
            .collect();
        super::dialogos::moldura(dialogo, largura::G, window)
            .title(super::dialogos::topo(
                CabecalhoDialogo::new("Atalhos do teclado")
                    .subtitulo(div().font_weight(FontWeight::NORMAL).child("Com o foco fora de um campo de texto, ? abre esta lista")),
            ))
            .child(
                div()
                    .id("atalhos-lista")
                    .max_h(window.viewport_size().height * 0.62)
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_start()
                            .gap_x(esp::E24)
                            .children(secoes.into_iter().map(move |s| {
                                div().when(duas, |d| d.w(relative(0.5)).max_w(relative(0.5))).child(s)
                            })),
                    ),
            )
            .footer(RodapeDialogo::new().principal(
                botao_primario("atalhos-fechar", None, "Fechar", TamanhoBotao::Controle, window, cx)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            ))
    });
}

/// Uma seção da lista: o título do grupo e as linhas, com as teclas à direita.
fn secao(grupo: Grupo, linhas: Vec<(Vec<SharedString>, &'static str)>) -> AnyElement {
    v_flex()
        .w_full()
        .min_w_0()
        .pb(esp::E8)
        .child(
            div()
                .h(r(24.))
                .flex()
                .items_center()
                .text_size(r(12.))
                .font_semibold()
                .text_color(Cores::tenue())
                .child(grupo.titulo()),
        )
        .children(linhas.into_iter().map(|(teclas, rotulo)| {
            div()
                .flex()
                .items_center()
                .gap(esp::E8)
                .min_h(r(26.))
                .px(esp::E4)
                .rounded(raio::CONTROLE)
                .child(div().flex_1().min_w_0().corpo_ui().text_color(Cores::texto()).child(rotulo))
                .child(desenho_teclas(teclas))
        }))
        .into_any_element()
}

#[cfg(test)]
mod testes {
    use super::{FIXAS, Grupo, do_grupo, linhas};
    use gpui_kit::Keystroke;

    #[test]
    fn toda_tecla_da_tabela_e_valida_e_cabe_num_grupo() {
        for linha in linhas() {
            assert!(Keystroke::parse(linha.teclas).is_ok(), "tecla inválida: {}", linha.teclas);
            assert!(Grupo::TODOS.contains(&linha.grupo), "grupo fora da lista: {linha:?}");
            assert!(!linha.partes().is_empty(), "sem teclas para desenhar: {linha:?}");
        }
    }

    #[test]
    fn a_lista_mostra_so_as_linhas_com_rotulo_e_nenhuma_duas_vezes() {
        let com_rotulo = linhas().iter().filter(|l| !l.rotulo.is_empty()).count();
        let listadas: usize = Grupo::TODOS.iter().map(|g| do_grupo(*g).len()).sum();
        assert_eq!(listadas, com_rotulo + FIXAS.len());
        // Cada rótulo aparece uma vez só: duas teclas para a mesma coisa deixam a segunda sem rótulo.
        let mut rotulos: Vec<&str> = linhas().iter().map(|l| l.rotulo).filter(|r| !r.is_empty()).collect();
        let total = rotulos.len();
        rotulos.sort_unstable();
        rotulos.dedup();
        assert_eq!(rotulos.len(), total, "rótulo repetido na tabela");
        // Nenhum grupo fica vazio na tela.
        for grupo in Grupo::TODOS {
            assert!(!do_grupo(grupo).is_empty(), "grupo vazio: {grupo:?}");
        }
    }

    #[test]
    fn as_teclas_viram_desenho_de_tecla_uma_a_uma() {
        let acha = |rotulo: &str| {
            linhas()
                .into_iter()
                .find(|l| l.rotulo == rotulo)
                .map(|l| l.partes().iter().map(|p| p.to_string()).collect::<Vec<_>>())
                .unwrap()
        };
        assert_eq!(acha("Novo projeto"), ["Ctrl", "Shift", "N"]);
        assert_eq!(acha("Filtrar pelo projeto 1 a 9 do trilho"), ["Ctrl", "1…9"]);
        assert_eq!(acha("Corrigir a última mensagem (caixa vazia)"), ["↑"]);
        assert_eq!(acha("Esta lista de atalhos"), ["?"]);
    }
}
