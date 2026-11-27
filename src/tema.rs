//! Tema do Vitral. Nasceu como "vidro azul-marinho", tirado da Stained Glass UI do VS Code (janela
//! translúcida com tom navy, peças de 20 px, destaque azul #3A7FFF). Em 12/11/2026 o dono pediu
//! "mais moderno e com menos cara de IA", e o acabamento em uso passou a ser o "Grafite": as mesmas
//! peças em degraus, num cinza frio quase sem cor, chapado (sem gradiente, reflexo nem halo) e com
//! um destaque só, o azul. Os cantos continuam bem arredondados (10, 14 e 20, pílula nos botões e
//! chips): o dono viu a versão de cantos curtos e disse que prefere assim. O marinho de antes
//! continua em `superficie::MARINHO`: trocar o `ATUAL` volta a ele.

use std::{borrow::Cow, rc::Rc};

use gpui_kit::component::{Theme, ThemeSet};
use gpui_kit::{
    Anchor, App, AssetSource, Background, BoxShadow, FontFeatures, FontWeight, Hsla, Pixels, Rems, SharedString,
    Styled, Window, linear_color_stop, linear_gradient, px, rems, rgba,
};

/// Medida de desenho a 100% (1 rem = 15 px); cresce com o zoom.
pub const fn r(px: f32) -> Rems {
    rems(px / 15.)
}

/// A mesma medida em Pixels, para as APIs que só aceitam Pixels (`Icon::with_size`,
/// `PopupMenu::min_w`, `canvas`).
pub fn tam(window: &Window, px: f32) -> Pixels {
    window.rem_size() * (px / 15.)
}

/// Raios: três medidas e a pílula. As três são em rem, para crescerem com o zoom como o resto (a
/// 150% o canto fixo em px ficava bem menos redondo: dono, 13/11/2026); quem só aceita `Pixels`
/// usa `controle()`, `cartao()` e `peca()`. Dentro de quem tem padding p, o raio é max(raio de fora − borda − p, CONTROLE),
/// para os cantos ficarem concêntricos; o teste `raios_na_escala` barra o resto.
pub mod raio {
    use gpui_kit::{Pixels, px};
    /// Campos, linhas (lista, menu, árvore, log), teclas, blocos dentro de cartão, miniaturas.
    pub const CONTROLE: gpui_kit::Rems = gpui_kit::rems(10. / 15.);
    /// Cartões dentro de painéis e popovers, blocos de código e tabelas, balões, alertas, grupos.
    pub const CARTAO: gpui_kit::Rems = gpui_kit::rems(14. / 15.);
    /// Peças: trilho, lista, conversa, Equipe, gavetas, popovers e menus, diálogos, avisos e a
    /// caixa de mensagem.
    pub const PECA: gpui_kit::Rems = gpui_kit::rems(20. / 15.);
    /// Cheio (o GPUI corta o raio na metade do lado menor): o `rounded_full()` de quem só aceita
    /// Pixels. Botões, chips, abas, segmentado, selos, avatares, pontos, anéis e barras.
    pub const PILULA: Pixels = px(999.);

    /// As três medidas em px no zoom de agora, para quem só aceita `Pixels` (os cantos de um
    /// recorte de metal, a conta de um raio de dentro).
    pub fn controle() -> Pixels {
        px((10. * super::zoom_dos_raios()).round())
    }
    pub fn cartao() -> Pixels {
        px((14. * super::zoom_dos_raios()).round())
    }
    pub fn peca() -> Pixels {
        px((20. * super::zoom_dos_raios()).round())
    }
    /// A moldura preta da caixa de mensagem: o miolo de 14 (`cartao`) com 3 de folga.
    pub fn moldura() -> Pixels {
        px((17. * super::zoom_dos_raios()).round())
    }

    /// O padrão de canto dos controles (botões, campos, abas, chips, selos): bem arredondado,
    /// nunca redondo. É o desenho do logo em escala: 45% da altura (16 num controle de 36, 14 num
    /// de 32, 9 num chip de 20; redondo seria 50%). Só ponto, barra fina e anel continuam redondos.
    pub fn de(altura: f32) -> Pixels {
        px((altura * 0.45 * super::zoom_dos_raios()).round())
    }

}

/// Espaços da grade, em rem: só 2, 4, 6, 8, 12, 16, 24 e 32 (o teste `espacos_na_escala` barra
/// o resto nos `gap`, `p*` e `m*`). Os atalhos do Tailwind (`gap_2()`, `p_3()`) medem em rem e,
/// com 1 rem = 15 px, saem fora da grade.
/// Opacidades com nome: o que está em segundo plano e o que está desabilitado. O repouso nunca
/// desce abaixo de 0,55, para não ler como desabilitado (0,45).
pub mod opacidade {
    /// O que recua até o ponteiro chegar (as ações do cabeçalho, os controles da caixa dos painéis
    /// de fundo).
    pub const EM_REPOUSO: f32 = 0.55;
    pub const DESABILITADO: f32 = 0.45;
}

/// As medidas do protótipo do redesenho Linear (`tarefas/linear/vitral-linear.html`), que é a
/// especificação desde 25/11/2026 (dono: "tem que ficar igual o protótipo"). Ficam fora da escala
/// de `esp` e valem só na peça que o nome diz.
pub mod linear {
    use super::r;
    use gpui_kit::Rems;
    /// A lista: 12 de recuo na esquerda, 10 na direita (até o cartão) e 10 no alto.
    pub const LISTA_ESQUERDA: Rems = r(12.);
    pub const LISTA_DIREITA: Rems = r(10.);
    pub const LISTA_TOPO: Rems = r(10.);
    /// Da busca às abas da lista e antes de cada seção.
    pub const ANTES_DA_SECAO: Rems = r(10.);
    /// Busca: 10 até a lupa, 11 depois do atalho.
    pub const BUSCA_ESQUERDA: Rems = r(10.);
    pub const BUSCA_DIREITA: Rems = r(11.);
    /// As abas da lista (Conversas, Arquivos, Quadro): 9 dos lados e 9 entre o ícone e o nome.
    pub const ITEM_LADO: Rems = r(9.);
    /// Uma linha de chat: 7 entre as partes (os recuos de 11 e 9 vão com `dentro_da_borda`).
    pub const CHAT_VAO: Rems = r(7.);
    /// Seção da lista: 10 dos lados.
    pub const SECAO_LADO: Rems = r(10.);
    /// As abas do cabeçalho: 11 dos lados (6 na direita da escolhida, com o número) e 7 entre o
    /// nome e o número; o cabeçalho, 7 dos lados.
    pub const ABA_LADO: Rems = r(11.);
    pub const ABAS_VAO: Rems = r(5.);
    pub const ABA_VAO: Rems = r(7.);
    pub const CABECA_LADO: Rems = r(7.);
    /// O seu balão: 6 em cima e embaixo (os 11 dos lados vêm de `mensagens::folga_do_balao`).
    pub const BALAO_ALTO: Rems = r(6.);
    /// Entre as falas da conversa (e entre os parágrafos), dentro de uma fala e os lados da
    /// rolagem.
    pub const ENTRE_FALAS: Rems = r(10.);
    pub const NA_FALA: Rems = r(7.);
    /// Os grupos do painel da direita: 1 entre as linhas (e entre o rótulo e a primeira).
    pub const GRUPO_LINHAS: Rems = r(1.);
    /// O passo da trilha: 7 entre o ícone de 14 e o texto.
    pub const PASSO_VAO: Rems = r(7.);
    /// O canto do seu balão.
    pub const RAIO_DO_BALAO: Rems = r(13.);
    pub const ROLAGEM_LADO: Rems = r(18.);
    pub const ROLAGEM_BAIXO: Rems = r(20.);
    /// A caixa de mensagem: a moldura preta com 3 de folga; dentro, 8 em cima, 6 na direita, 5
    /// embaixo e 10 na esquerda; os controles a 4 do campo, puxados 4 para a esquerda, com 1 entre
    /// eles e 5 dentro de cada um.
    pub const MOLDURA_FOLGA: Rems = r(3.);
    pub const CAIXA_ALTO: Rems = r(8.);
    pub const CAIXA_DIREITA: Rems = r(6.);
    pub const CAIXA_BAIXO: Rems = r(5.);
    pub const CONTROLES_VAO: Rems = r(1.);
    pub const CONTROLE_LADO: Rems = r(5.);
    /// O respiro de cima das pastilhas de número: o algarismo, centrado pela altura da linha,
    /// ficava meio pixel acima do meio da pastilha (medido no print a 130%; dono, 26/11/2026:
    /// "não tá centralizado"). Um de respiro em cima desce o centro meio.
    pub const AJUSTE_DO_NUMERO: Rems = r(1.);
    /// O contexto: o anel e a porcentagem com 5 entre eles.
    pub const CONTEXTO_VAO: Rems = r(5.);
    /// A linha de estado embaixo da caixa: 8 na esquerda, 9 na direita.
    pub const ESTADO_ESQUERDA: Rems = r(8.);
    pub const ESTADO_DIREITA: Rems = r(9.);
    /// A barra de baixo: 7 entre as partes de uma pílula de uso, 10 na direita dela.
    pub const USO_VAO: Rems = r(7.);
    pub const USO_DIREITA: Rems = r(10.);
    /// O botão de texto das Cores (Importar, Copiar): 8 antes e 10 depois do texto.
    pub const BOTAO_TEXTO_DIREITA: Rems = r(10.);
    /// O aviso do Copiar e do Importar, recuado como as linhas.
    pub const AVISO_LADO: Rems = r(14.);
}

pub mod esp {
    use super::r;
    use gpui_kit::Rems;
    /// Só óptico, dentro de controle: entre segmentos, entre cartões da lista.
    pub const E2: Rems = r(2.);
    /// Entre botões de ícone vizinhos; entre linhas de um mesmo bloco.
    pub const E4: Rems = r(4.);
    /// Só dentro de controle: ícone e rótulo em chip, selo e pílula.
    pub const E6: Rems = r(6.);
    /// Padding das peças; frestas; entre cartões; ícone e texto em linhas.
    pub const E8: Rems = r(8.);
    /// Padding de toda caixa no fluxo (cartão, código, balão, log, alerta); vão dentro do turno.
    pub const E12: Rems = r(12.);
    /// Padding de popover e da caixa de mensagem; respiro antes de cabeçalho de seção.
    pub const E16: Rems = r(16.);
    /// Entre turnos; margem de diálogo.
    pub const E24: Rems = r(24.);
    /// Estados vazios e telas cheias.
    pub const E32: Rems = r(32.);

    /// Padding horizontal da lista, da Equipe e da gaveta.
    pub const PECA: Rems = E8;
    /// Conteúdo de qualquer caixa no fluxo.
    pub const CAIXA: Rems = E12;
    /// Popover e caixa de mensagem.
    pub const FLUTUANTE: Rems = E16;
    /// Da borda da peça até a coluna-guia da lista.
    pub const GUIA_LISTA: Rems = E16;
    /// Da borda do painel até a coluna-guia da Equipe.
    pub const GUIA_EQUIPE: Rems = E24;
    /// Antes do seu balão e do separador de dia: mais de 2 vezes o `NO_TURNO` (relatório de
    /// aparência, 16/11/2026, V3; não passar de 32, com 40 os balões ficam soltos).
    /// No Linear (25/11/2026), os 10 entre as falas e os 7 dentro delas do protótipo.
    pub const ENTRE_TURNOS: Rems = super::linear::ENTRE_FALAS;
    pub const NO_TURNO: Rems = super::linear::NA_FALA;
    /// Respiro visível, da peça de cima ao topo da linha de 24 do cabeçalho de seção.
    pub const ANTES_DE_SECAO: Rems = E16;
}

/// As medidas da tela inicial (o `.inicio` do protótipo Linear): o cartão de 600, o alto com o
/// logo e os passos, o palco do passo e o pé com os botões.
pub mod inicio {
    use super::r;
    use gpui_kit::Rems;
    pub const LARGURA: f32 = 600.;
    pub const TOPO_Y: Rems = r(22.);
    pub const TOPO_X: Rems = r(24.);
    pub const PILULA_ESQUERDA: Rems = r(7.);
    pub const PILULA_DIREITA: Rems = r(9.);
    pub const PALCO_Y: Rems = r(26.);
    pub const PALCO_X: Rems = r(28.);
    pub const PASSO_VAO: Rems = r(14.);
    pub const LINHA_Y: Rems = r(8.);
    pub const LINHA_X: Rems = r(10.);
    pub const PASTA_VAO: Rems = r(10.);
    pub const BOTAO_X: Rems = r(11.);
    pub const PE_Y: Rems = r(14.);
    pub const PE_X: Rems = r(20.);
}

/// As medidas da página do .md (o `.pagina` do protótipo Linear): 52 à esquerda para a alça, 18 à
/// direita, 10 em cima; 3 de cada lado do texto de um bloco.
pub mod pagina {
    use super::r;
    use gpui_kit::Rems;
    pub const LARGURA: f32 = 720.;
    pub const ESQUERDA: Rems = r(52.);
    pub const DIREITA: Rems = r(18.);
    pub const TOPO: Rems = r(10.);
    pub const CAMPO_X: Rems = r(3.);
    pub const H1_ANTES: Rems = r(14.);
    pub const H3_ANTES: Rems = r(10.);
    pub const FIM: Rems = r(90.);
    pub const COLUNA: Rems = r(14.);
    pub const CELULA_CABECA: Rems = r(5.);
    pub const CAIXINHA_DIREITA: Rems = r(1.);
}

/// Padding de quem tem `border_1()`: o conteúdo fica a `px` da borda de fora contando o traço de
/// 1 (o cartão com conteúdo a 12 da borda leva `.p(dentro_da_borda(12.))`).
pub const fn dentro_da_borda(px: f32) -> Rems {
    r(px - 1.)
}

/// Alturas de controles e linhas, em rem.
pub mod altura {
    use super::r;
    use gpui_kit::Rems;
    /// Selo (o kbd tem 18).
    pub const MINI: Rems = r(20.);
    /// Botão só ícone nas ações de hover e dentro de cartões; Chip pequeno.
    pub const ACAO_LINHA: Rems = r(24.);
    /// Botão só ícone em cabeçalho e barra; Chip médio; segmentado pequeno.
    pub const BARRA: Rems = r(28.);
    /// Campo, botão com texto, Chip grande, segmentado médio, linha de uma linha.
    pub const CONTROLE: Rems = r(32.);
    /// Botões do trilho, segmentado grande.
    pub const GRANDE: Rems = r(36.);
    pub const CABECALHO: Rems = r(44.);
    /// Item de menu ou de lista de escolha com descrição (era 48; dono, 18/11/2026: "deixa os menus
    /// menor").
    pub const ESCOLHA_2: Rems = r(40.);
    /// Cartão de lista com duas linhas.
    pub const LINHA_2: Rems = r(52.);
}

pub const FONTE_MONO: &str = "JetBrains Mono";
/// A da interface (a do tema), para o que fica no meio de um texto em mono.
pub const FONTE_UI: &str = "Inter";

/// Só no rótulo "Ultrathink": a referência clareada para passar de 4,5:1 no flutuante.
pub const ARCO_IRIS: [u32; 8] = [
    0xf08a5d, 0xf5b642, 0x9bd46a, 0x4ade80, 0x3fd1b5, 0x5cc8f0, 0x7aa7ff, 0xa98bff,
];

/// Dígitos de largura igual (`tnum` do OpenType; a Inter embutida tem a tabela): o número que muda
/// ("4 s", "5 s", "24%") não faz a coluna andar. Um `Arc` só, clonado a cada uso.
static TABULAR: std::sync::LazyLock<FontFeatures> =
    std::sync::LazyLock::new(|| FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)])));

/// Tipografia da linguagem, em rem (cresce com o zoom). Duas vozes: a Inter, que conversa, e a
/// JetBrains Mono miúda (`etiqueta`, `rotulo_secao`, as teclas), que põe à vista os fatos e as
/// medidas, como num aparelho de medição.
pub trait Tipo: Styled + Sized {
    /// Dígitos de largura igual (já vem no `metadado`, no `micro` e no `numero`).
    fn tabular(self) -> Self {
        self.font_features(TABULAR.clone())
    }
    /// O título grande da tela vazia (28/34).
    fn titulo_heroi(self) -> Self {
        self.text_size(r(28.)).line_height(r(34.)).font_weight(FontWeight::SEMIBOLD)
    }
    fn titulo_tela(self) -> Self {
        self.text_size(r(18.)).line_height(r(24.)).font_weight(FontWeight::SEMIBOLD)
    }
    fn titulo_cartao(self) -> Self {
        self.text_size(r(15.)).line_height(r(20.)).font_weight(FontWeight::SEMIBOLD)
    }
    /// Rótulo de seção: mono de 11 apagado, como etiqueta técnica. A caixa alta e o tracinho na
    /// frente ficam no desenho (`etiqueta_de_secao`, `CabecalhoDeSecao`), nunca no dado.
    fn rotulo_secao(self) -> Self {
        self.font_family(FONTE_MONO)
            .text_size(r(11.))
            .line_height(r(16.))
            .font_weight(FontWeight::NORMAL)
            .text_color(Cores::apagado())
    }
    /// A segunda voz: mono 11/14 regular, das etiquetas de fio, numerais e medidas.
    fn etiqueta(self) -> Self {
        self.font_family(FONTE_MONO).text_size(r(11.)).line_height(r(14.)).font_weight(FontWeight::NORMAL)
    }
    /// O texto da resposta: 14/22 em 400 (protótipo Linear, 25/11/2026; era 15/23).
    fn corpo(self) -> Self {
        self.text_size(r(14.)).line_height(r(22.)).font_weight(FontWeight::NORMAL)
    }
    /// O que você escreveu (o balão): 14/21 em 400 (protótipo Linear, 25/11/2026).
    fn fala(self) -> Self {
        self.text_size(r(14.)).line_height(r(21.)).font_weight(FontWeight::NORMAL)
    }
    fn corpo_ui(self) -> Self {
        self.text_size(r(14.)).line_height(r(20.))
    }
    fn rotulo(self) -> Self {
        self.text_size(r(13.)).line_height(r(18.)).font_weight(FontWeight::MEDIUM)
    }
    fn metadado(self) -> Self {
        self.text_size(r(12.)).line_height(r(16.)).tabular()
    }
    fn micro(self) -> Self {
        self.text_size(r(11.)).line_height(r(14.)).font_weight(FontWeight::SEMIBOLD).tabular()
    }
    fn mono(self) -> Self {
        self.font_family(FONTE_MONO).text_size(r(13.)).line_height(r(20.))
    }
    fn mono_pequeno(self) -> Self {
        self.font_family(FONTE_MONO).text_size(r(12.)).line_height(r(16.))
    }
    fn numero(self) -> Self {
        self.text_size(r(20.)).line_height(r(24.)).font_weight(FontWeight::SEMIBOLD).tabular()
    }
}

impl<T: Styled> Tipo for T {}

/// Elevação 2 (caixa de mensagem, prévia de arrasto). No GPUI o blur é o desvio padrão: metade
/// do blur do CSS.
/// Preto de sombra com o alfa `base` (0 a 255) multiplicado pelo da superfície.
fn preto_de_sombra(base: u32) -> Hsla {
    let alfa = ((base as f32 * superficie::atual().sombra).round() as u32).min(0xff);
    rgba(alfa).into()
}

pub fn sombra_elevado() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(1.), preto_de_sombra(0x40)).blur_radius(px(1.5)),
        BoxShadow::new(px(0.), px(4.), preto_de_sombra(0x33))
            .blur_radius(px(4.))
            .spread_radius(px(-2.)),
    ]
}

/// Elevação 3: menu, popover, dica, aviso (a mesma do `popover_style` do vendor).
pub fn sombra_flutuante() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(12.), preto_de_sombra(0x66))
            .blur_radius(px(12.))
            .spread_radius(px(-4.)),
        BoxShadow::new(px(0.), px(2.), preto_de_sombra(0x4d))
            .blur_radius(px(3.))
            .spread_radius(px(-1.)),
    ]
}

/// Elevação 4: diálogo (a mesma do `dialog_shadow` do vendor).
pub fn sombra_dialogo() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(24.), preto_de_sombra(0x80))
            .blur_radius(px(24.))
            .spread_radius(px(-8.)),
        BoxShadow::new(px(0.), px(2.), preto_de_sombra(0x4d)).blur_radius(px(4.)),
    ]
}

/// Os três materiais do Vitral, feitos só de luz e sombra de 1 px por dentro (nada vaza para fora
/// da peça, que é translúcida e escureceria o que está atrás).
///
/// Vidro, nas peças (lista, conversa, painéis da direita): um fio claro de 1 px na volta toda,
/// luz na própria quina de cima, sombra de 1 px na base e um brilho que entra uns 40 px pelo topo.
/// Vai na camada do `camada_de_aresta`, o último filho da peça: a sombra interna pinta antes dos
/// filhos, e o que viesse depois (o pontilhado do pé) passaria por cima do fio.
pub fn aresta_de_vidro() -> Vec<BoxShadow> {
    // Cada lado é uma sombra interna deslocada 1 px (o furo anda 1 px e sobra uma tira de 1 px
    // daquele lado, que acompanha o arco dos cantos). Um anel só (`spread_radius(1)`) passaria
    // também pela base, e o claro dele com a sombra por cima dava o tom do fundo: a base sumia.
    let lado = |x: f32, y: f32, cor: Hsla| BoxShadow::new(px(x), px(y), cor).inset();
    vec![
        // O brilho primeiro, para o fio e a luz pintarem por cima dele.
        BoxShadow::new(px(0.), px(28.), Cores::brilho_de_dentro())
            .blur_radius(px(12.))
            .spread_radius(px(-12.))
            .inset(),
        // O fio claro só nos dois lados. Em cima havia o fio e uma luz na quina; o dono pediu
        // para tirar "essa bordinha de cima" (13/11/2026).
        lado(1., 0., Cores::aro_da_peca()),
        lado(-1., 0., Cores::aro_da_peca()),
        // A sombra na base.
        lado(0., -1., Cores::sombra_de_baixo()),
    ]
}

/// A aresta de uma peça da fileira das conversas, que encosta no topo, na direita e embaixo da
/// janela (dono, 16/11/2026: "não tenha em cima, na direita e no meio dos chats; no meio dos chats
/// uma divisão simples e moderna" e "sem embaixo também, com os cantos da esquerda redondos").
/// Sem o fio da direita (lá é a beirada da janela ou a vizinha). À esquerda, depende do que há
/// lá (`Esquerda`): o fio claro no vão da lista, a divisória de 1 px no tom do traço na emenda
/// com a vizinha, nada na beirada da janela. O brilho do topo fica; a sombra da base sai (ela dá
/// na beirada da janela).
pub fn aresta_encostada(esquerda: Esquerda) -> Vec<BoxShadow> {
    let lado = |x: f32, y: f32, cor: Hsla| BoxShadow::new(px(x), px(y), cor).inset();
    let brilho = BoxShadow::new(px(0.), px(28.), Cores::brilho_de_dentro())
        .blur_radius(px(12.))
        .spread_radius(px(-12.))
        .inset();
    match esquerda {
        Esquerda::Solta => vec![brilho, lado(1., 0., Cores::aro_da_peca())],
        // A divisória da emenda não é sombra: é o fio que esmaece nas pontas
        // (`gradiente_da_divisoria`), desenhado à parte.
        Esquerda::Emendada | Esquerda::Beirada => vec![brilho],
    }
}


/// A casca da gaveta (lista ou painel da direita por cima das conversas): a do popover do kit
/// (`popover_shadow` em vendor/gpui-component/src/styled.rs, com o anel de 14%) sem a luz de 1 px
/// no topo e a sombra de 1 px na base, que a gaveta encosta no topo e embaixo da janela. Por fora
/// ficam a sombra, o fio preto e o anel claro, que a separam das conversas no lado solto.
pub fn sombra_da_gaveta() -> Vec<BoxShadow> {
    let preto = |a: f32| gpui_kit::hsla(0., 0., 0., a);
    let branco = |a: f32| gpui_kit::hsla(0., 0., 1., a);
    vec![
        BoxShadow::new(px(0.), px(24.), preto(0.60)).blur_radius(px(16.)).spread_radius(px(-10.)),
        BoxShadow::new(px(0.), px(4.), preto(0.35)).blur_radius(px(2.5)).spread_radius(px(-1.)),
        BoxShadow::new(px(0.), px(0.), preto(0.60)).spread_radius(px(2.)),
        BoxShadow::new(px(0.), px(0.), branco(0.14)).spread_radius(px(1.)),
        BoxShadow::new(px(0.), px(28.), branco(0.03)).blur_radius(px(12.)).spread_radius(px(-12.)).inset(),
    ]
}

/// O que fica à esquerda de uma peça da fileira das conversas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Esquerda {
    /// O vão até a lista: os dois cantos da esquerda redondos e o fio claro.
    Solta,
    /// Outra peça da fileira: cantos retos e a divisória de 1 px.
    Emendada,
    /// A beirada da janela, com a lista recolhida (dono, 16/11/2026: "sem a lista tem que ficar
    /// sem a borda da esquerda também"): cantos retos e nada de fio.
    Beirada,
}

/// O raio dos cantos da esquerda de uma peça encostada: redondos só com o vão da lista ao lado
/// (os outros cantos dão na beirada da janela ou na emenda com a vizinha). Uma vez e meia o das
/// peças: a fileira das conversas é a peça grande da janela (dono, 17/11/2026: "deixa o border
/// radius dos cantos da esquerda maior").
pub fn raio_encostado(esquerda: Esquerda) -> gpui_kit::Pixels {
    // Linear (25/11/2026): a fileira é um cartão só, com 8 de margem da lista ou da beirada e os
    // cantos de 20 do protótipo; só a emenda com a vizinha fica reta.
    if esquerda == Esquerda::Emendada { px(0.) } else { raio_da_peca() }
}

/// Tecla, no que está em relevo e escolhido (a pílula do segmentado, a tecla de atalho): luz de
/// 1 px em cima e sombra de 1 px embaixo, por dentro. Nunca em cada linha de uma lista.
pub fn aresta_de_tecla() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(1.), Cores::forte().opacity(0.16)).inset(),
        BoxShadow::new(px(0.), px(-1.), Cores::preto_puro().opacity(0.30)).inset(),
    ]
}

/// Poço, no que é cavado (o trilho do segmentado, o campo de busca, o contador): a sombra cai
/// de cima para dentro e a quina de baixo pega 1 px de luz.
pub fn aresta_de_poco() -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(2.), Cores::preto_puro().opacity(0.55)).blur_radius(px(3.)).inset(),
        BoxShadow::new(px(0.), px(-1.), Cores::forte().opacity(0.07)).inset(),
    ]
}

/// Anel de foco pelo teclado, o mesmo do kit (`FOCUS_RING_WIDTH`).
pub fn halo_foco() -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(0.), Cores::destaque().opacity(0.5)).spread_radius(px(3.))]
}

/// Linha de luz de 1 px no topo de dentro da peça (o reflexo do vidro), a `forca` de 0 a 1.
pub fn linha_de_luz(forca: f32) -> BoxShadow {
    BoxShadow::new(px(0.), px(1.), Cores::brilho_topo().opacity(forca)).inset()
}

/// O gradiente do topo da peça, na camada de 40 px do `.reflexo()`: um escuro que vai sumindo,
/// no lugar do claro de antes. O topo é onde fica o título; escurecer, em vez de clarear, afunda
/// a peça no fundo e deixa o texto com mais contraste.
pub fn gradiente_reflexo() -> Background {
    let alfa = ((26. * superficie::atual().reflexo).round() as u32).min(0xff);
    linear_gradient(
        180.,
        linear_color_stop(rgba(0x00000000 | alfa), 0.),
        linear_color_stop(rgba(0x00000000), 1.),
    )
}

/// A beirada de cima da doca (o metal no pé da conversa): o escuro do cartão some para baixo.
pub fn gradiente_da_doca() -> Background {
    let cartao = Cores::cartao_opaco();
    linear_gradient(180., linear_color_stop(cartao, 0.), linear_color_stop(cartao.opacity(0.), 1.))
}

/// O pé do cartão da conversa: o cartão é meio transparente em cima e vai fechando até ficar
/// opaco onde mora a caixa de mensagem (é onde os degradês da luz de baixo somem, e não pode
/// haver emenda).
pub fn gradiente_do_pe_do_cartao() -> Background {
    let cartao = Cores::cartao_opaco();
    linear_gradient(180., linear_color_stop(cartao.opacity(0.), 0.), linear_color_stop(cartao, 1.))
}

/// O pé do cartão em curva: do transparente ao opaco em trechos que seguem a curva suave
/// (3t² − 2t³), que começa e termina sem quina. O degradê reto de 200 px deixava uma linha onde
/// acabava e o cartão ficava cheio (dono, 24/11/2026: "deixa mais difuso pra não ficar essa
/// repartição", sobre o shader). Cada trecho: início e fim (fração da altura) e o degradê dele.
pub fn pe_do_cartao_em_curva() -> Vec<(f32, f32, Background)> {
    let cartao = Cores::cartao_opaco();
    curva_do_pe(TRECHOS_DO_PE)
        .into_iter()
        .map(|(de, ate, alfa_de, alfa_ate)| {
            (de, ate, linear_gradient(180., linear_color_stop(cartao.opacity(alfa_de), 0.), linear_color_stop(cartao.opacity(alfa_ate), 1.)))
        })
        .collect()
}

/// Em quantos trechos o pé do cartão faz a curva, e a altura de cada um (px): 8 de 48, com um
/// número inteiro de pixels por trecho também a 125% e 150%, para não sobrar fio entre dois.
pub const TRECHOS_DO_PE: usize = 8;
pub const ALTURA_DO_TRECHO_DO_PE: f32 = 48.;

/// Os trechos da curva: (início, fim, opacidade no início, opacidade no fim).
pub fn curva_do_pe(trechos: usize) -> Vec<(f32, f32, f32, f32)> {
    let suave = |t: f32| t * t * (3. - 2. * t);
    (0..trechos)
        .map(|i| {
            let (de, ate) = (i as f32 / trechos as f32, (i + 1) as f32 / trechos as f32);
            (de, ate, suave(de), suave(ate))
        })
        .collect()
}

/// A divisória entre duas peças emendadas: um fio de 1 px que nasce do nada no alto, fica cheio no
/// meio e some de novo embaixo (dono, 16/11/2026: "tem que ser algo diferente, mais moderno"; o
/// fio reto de cima a baixo cortava a fileira das abas e a das caixas). `embaixo` dá o trecho de
/// cima (`false`) ou o de baixo (`true`); o meio é `Cores::divisoria()` liso.
pub fn gradiente_da_divisoria(embaixo: bool) -> Background {
    let (some, cheio) = (Cores::divisoria().opacity(0.), Cores::divisoria());
    let (de, ate) = if embaixo { (cheio, some) } else { (some, cheio) };
    linear_gradient(180., linear_color_stop(de, 0.), linear_color_stop(ate, 1.))
}

/// Os lados da doca: o escuro do cartão some da beirada para o meio (da esquerda para a direita;
/// `direita` espelha). É o que concentra a luz de baixo no meio do painel.
pub fn gradiente_do_lado_da_doca(direita: bool) -> Background {
    linear_gradient(
        if direita { 270. } else { 90. },
        linear_color_stop(Cores::cartao_opaco(), 0.),
        linear_color_stop(Cores::cartao_opaco().opacity(0.), 1.),
    )
}

/// Assinatura do Vitral: azul à esquerda indo para o preto (caixa de mensagem).
pub fn gradiente_quente() -> Background {
    gradiente_quente_em(90.)
}

/// O mesmo gradiente em outro ângulo (135° no círculo do estado vazio amplo).
pub fn gradiente_quente_em(angulo: f32) -> Background {
    linear_gradient(
        angulo,
        linear_color_stop(Cores::caixa_quente(), 0.),
        linear_color_stop(Cores::caixa_preta(), 0.55),
    )
}

/// Catálogo completo do Lucide. Só aparecem os ícones registrados em `IconesExtras` (ou os 101
/// que o kit já embute); um que falte sai em branco, e o teste `icones_lucide_registrados` acusa.
pub use gpui_kit::assets::IconName as Lucide;

gpui_kit::assets::icon_assets!(
    IconesExtras,
    [
        Activity,
        Archive,
        ArchiveRestore,
        AtSign,
        BellOff,
        Binary,
        Bot,
        Braces,
        Brain,
        CalendarClock,
        ChartColumn,
        CheckCheck,
        Circle,
        CircleArrowUp,
        CircleCheckBig,
        CircleDashed,
        CircleDot,
        CirclePause,
        CirclePlay,
        CircleUserRound,
        Clock,
        Code,
        CodeXml,
        Columns2,
        Columns3,
        Compass,
        Container,
        CornerLeftUp,
        Download,
        Eraser,
        FileCheck,
        FileCode,
        FileDiff,
        FileImage,
        FileMinus,
        FilePen,
        FilePenLine,
        FilePlus,
        FileSymlink,
        FileX,
        Files,
        Focus,
        FoldVertical,
        FolderInput,
        FolderPlus,
        FolderSearch,
        FolderX,
        Funnel,
        Gauge,
        GitBranch,
        GitCommitHorizontal,
        GitFork,
        GitMerge,
        GitPullRequest,
        GitPullRequestClosed,
        GitPullRequestDraft,
        GripVertical,
        Hand,
        Image,
        ImageOff,
        KeyRound,
        Kanban,
        Keyboard,
        Leaf,
        Lightbulb,
        LayoutGrid,
        Link,
        LockKeyholeOpen,
        LogIn,
        ListChecks,
        ListTodo,
        ListTree,
        MessageCircle,
        MessageCircleQuestionMark,
        MessageSquare,
        MessageSquarePlus,
        MessageSquareQuote,
        MessagesSquare,
        Milestone,
        MonitorPlay,
        MoveDown,
        MoveUp,
        NotebookPen,
        Orbit,
        Palette,
        Paperclip,
        Pencil,
        Pin,
        PinOff,
        Pipette,
        Reply,
        RotateCcw,
        RotateCcwClock,
        Rows3,
        ScrollText,
        Send,
        ShieldAlert,
        ShieldCheck,
        ShieldOff,
        ShieldQuestionMark,
        Shuffle,
        Slash,
        Sparkles,
        SquareCheck,
        SquareSlash,
        Tag,
        Terminal,
        TextAlignStart,
        TextCursorInput,
        TextSearch,
        TextWrap,
        Trash,
        Unplug,
        UsersRound,
        WandSparkles,
        WifiOff,
        Workflow,
        Wrench,
        X,
        Zap,
        ZoomIn,
        ZoomOut,
    ]
);

/// Os ícones do kit mais os extras do Vitral.
pub struct Icones;

impl AssetSource for Icones {
    fn load(&self, caminho: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = IconesExtras.load(caminho)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(caminho)
    }

    fn list(&self, caminho: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut lista = gpui_kit::assets::Assets.list(caminho)?;
        lista.extend(IconesExtras.list(caminho)?);
        lista.sort();
        lista.dedup();
        Ok(lista)
    }
}

const TEMA: &str = r##"{
  "name": "Vitral",
  "themes": [{
    "name": "Vidro azul-marinho",
    "mode": "dark",
    "font.family": "Inter",
    "font.size": 15,
    "mono_font.family": "JetBrains Mono",
    "mono_font.size": 13,
    "radius": 10,
    "radius.lg": 20,
    "shadow": false,
    "colors": {
      "background": "#060810d4",
      "foreground": "#dfe6fb",
      "border": "#ffffff14",
      "window.border": "#00000000",
      "ring": "#3a7fff",
      "caret": "#9ccbff",
      "selection.background": "#3a7fff59",
      "accent.background": "#3a7fff33",
      "accent.foreground": "#ffffff",
      "primary.background": "#3a7fff",
      "primary.hover.background": "#3374f0",
      "primary.active.background": "#2a64d6",
      "primary.foreground": "#ffffff",
      "button.primary.background": "#2f6fe8",
      "button.primary.hover.background": "#2a64d6",
      "button.primary.active.background": "#2458c0",
      "button.primary.foreground": "#ffffff",
      "button.danger.background": "#f871711f",
      "button.danger.hover.background": "#f8717124",
      "button.danger.active.background": "#f871712e",
      "button.danger.foreground": "#f87171",
      "switch.background": "#ffffff1f",
      "switch.thumb.background": "#dfe6fb",
      "secondary.background": "#ffffff0d",
      "secondary.hover.background": "#ffffff17",
      "secondary.active.background": "#ffffff1f",
      "secondary.foreground": "#dfe6fb",
      "muted.background": "#ffffff0f",
      "muted.foreground": "#9ba8cf",
      "popover.background": "#0c0e13",
      "popover.foreground": "#dfe6fb",
      "input.border": "#ffffff1a",
      "list.background": "#00000000",
      "list.hover.background": "#ffffff12",
      "list.active.background": "#3a7fff33",
      "list.active.border": "#3a7fff00",
      "list.even.background": "#00000000",
      "link.foreground": "#8fc0ff",
      "link.hover.foreground": "#b4d4ff",
      "link.active.foreground": "#8fc0ff",
      "scrollbar.background": "#00000000",
      "scrollbar.thumb.background": "#9ba8cf26",
      "scrollbar.thumb.hover.background": "#9ba8cf4d",
      "sidebar.background": "#00000000",
      "sidebar.border": "#00000000",
      "sidebar.foreground": "#dfe6fb",
      "sidebar.accent.background": "#3a7fff24",
      "sidebar.accent.foreground": "#eaf2ff",
      "sidebar.primary.background": "#3a7fff",
      "sidebar.primary.foreground": "#ffffff",
      "tab.background": "#00000000",
      "tab.active.background": "#121726d1",
      "tab.foreground": "#9ba8cf",
      "tab.active.foreground": "#eaf2ff",
      "tab_bar.background": "#00000000",
      "title_bar.background": "#00000000",
      "title_bar.border": "#00000000",
      "overlay": "#070b1699",
      "success.background": "#4ade80",
      "success.foreground": "#4ade80",
      "warning.background": "#f5b642",
      "warning.foreground": "#f5b642",
      "danger.background": "#f87171",
      "danger.foreground": "#f87171",
      "info.background": "#8fd3f5",
      "info.foreground": "#8fd3f5",
      "base.blue": "#3a7fff",
      "base.green": "#4ade80",
      "base.yellow": "#f5b642",
      "base.red": "#f87171",
      "base.cyan": "#8fd3f5",
      "base.magenta": "#b39dff"
    },
    "highlight": {
      "editor_background": "#00000000",
      "editor_foreground": "#dfe6fb",
      "editor_line_number": "#9ba8cf66",
      "editor_active_line_number": "#dfe6fb",
      "editor_active_line": "#ffffff08",
      "editor.gutter.background": "#00000000",
      "syntax": {
        "keyword": { "color": "#b39dff" },
        "preproc": { "color": "#b39dff" },
        "label": { "color": "#b39dff" },
        "string": { "color": "#8fe3a8" },
        "string.escape": { "color": "#8fd3f5" },
        "string.regex": { "color": "#8fd3f5" },
        "string.special": { "color": "#8fe3a8" },
        "text.literal": { "color": "#8fe3a8" },
        "text.code.span": { "color": "#8fe3a8" },
        "function": { "color": "#8fc0ff" },
        "type": { "color": "#8fd3f5" },
        "constructor": { "color": "#8fd3f5" },
        "enum": { "color": "#8fd3f5" },
        "variant": { "color": "#8fd3f5" },
        "number": { "color": "#f5b642" },
        "boolean": { "color": "#f5b642" },
        "constant": { "color": "#f5b642" },
        "comment": { "color": "#8a97bf", "font_style": "italic" },
        "comment_doc": { "color": "#8a97bf", "font_style": "italic" },
        "punctuation": { "color": "#9ba8cf" },
        "operator": { "color": "#9ba8cf" },
        "property": { "color": "#dfe6fb" },
        "attribute": { "color": "#8fc0ff" },
        "tag": { "color": "#8fc0ff" },
        "variable.special": { "color": "#8fc0ff" },
        "title": { "color": "#8fc0ff", "font_weight": 600 },
        "emphasis": { "font_style": "italic" },
        "emphasis.strong": { "font_weight": 700 },
        "link_text": { "color": "#8fc0ff" },
        "link_uri": { "color": "#8fc0ff", "font_style": "italic" }
      }
    }
  }]
}"##;

/// Largura dos avisos a 100% (o zoom a multiplica, em `zoom.rs`).
pub const LARGURA_AVISO: f32 = 380.;

/// Inter e JetBrains Mono vão dentro do binário (OFL; a licença está em assets/fontes/OFL.txt e vai
/// no pacote): o Windows não tem nenhuma das duas, e no Linux o Vitral não depende das fontes da
/// distribuição. Só os pesos que o Vitral usa: 400, 500, 600 e 700 e o itálico do texto; no código, o
/// normal, o itálico dos comentários e o negrito.
const FONTES: [&[u8]; 8] = [
    include_bytes!("../assets/fontes/Inter-Regular.ttf"),
    include_bytes!("../assets/fontes/Inter-Medium.ttf"),
    include_bytes!("../assets/fontes/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fontes/Inter-Bold.ttf"),
    include_bytes!("../assets/fontes/Inter-Italic.ttf"),
    include_bytes!("../assets/fontes/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fontes/JetBrainsMono-Italic.ttf"),
    include_bytes!("../assets/fontes/JetBrainsMono-Bold.ttf"),
];

/// As fontes do símbolo e do emoji que faltam nas duas, do sistema. O Noto Color Emoji (COLRv1)
/// sai em branco no Linux, então lá vai o monocromático; no Windows, as da Segoe.
pub const FONTES_DE_RESERVA: [&str; 2] =
    if cfg!(windows) { ["Segoe UI Emoji", "Segoe UI Symbol"] } else { ["Noto Emoji", "Noto Sans Symbols 2"] };

/// O JSON do tema com as superficies, bordas e realces da superfície no lugar dos de
/// hoje (a variante 1 devolve o TEMA sem mudanca).
fn tema_json() -> String {
    let v = superficie::atual();
    let mut json = TEMA.to_string();
    for (chave, cor) in [
        ("background", v.vidro),
        // O `border` do kit também pinta os separadores dos menus: fica com a divisória, e só o
        // contorno dos campos segue a borda transparente.
        ("border", v.linha),
        ("input.border", v.borda),
        ("popover.background", v.flutuante),
        ("overlay", v.veu),
        ("list.hover.background", v.hover),
        ("list.active.background", v.selecao),
        ("accent.background", v.selecao),
        ("sidebar.accent.background", v.selecao),
        ("tab.active.background", v.cartao_elevado),
        // O anel de foco do kit (menus, campos) é neutro: o azul fica para a ação.
        ("ring", 0xffffff40),
        ("primary.foreground", v.sobre_destaque),
        ("button.primary.foreground", v.sobre_destaque),
        ("sidebar.primary.foreground", v.sobre_destaque),
    ] {
        let alvo = format!("\"{chave}\": \"");
        if let Some(i) = json.find(&alvo) {
            let inicio = i + alvo.len();
            let fim = inicio + json[inicio..].find('"').expect("cor do tema fechada");
            json.replace_range(inicio..fim, &format!("#{cor:08x}"));
        }
    }
    // Os textos do kit (o corpo, o tênue e o dos comentários) seguem os da variante: no JSON eles
    // estão com os do marinho, em várias chaves.
    let m = &superficie::MARINHO;
    for (antes, depois) in [(m.texto, v.texto), (m.tenue, v.tenue), (m.apagado, v.apagado)] {
        json = json.replace(&format!("#{:06x}", antes >> 8), &format!("#{:06x}", depois >> 8));
    }
    // O destaque e as variações dele (o foco, a seleção de texto, o botão primário, os links).
    for (antes, depois) in [
        (0x3a7fff, v.destaque >> 8),
        (0x2f6fe8, v.destaque >> 8),
        (0x3374f0, v.destaque_hover >> 8),
        (0x2a64d6, v.destaque_hover >> 8),
        (0x2458c0, v.destaque_ativo >> 8),
        (0x8fc0ff, v.destaque_texto >> 8),
    ] {
        json = json.replace(&format!("#{antes:06x}"), &format!("#{depois:06x}"));
    }
    json
}

fn config_do_kit() -> Rc<gpui_kit::component::ThemeConfig> {
    let conjunto: ThemeSet = serde_json::from_str(&tema_json()).expect("tema do Vitral");
    Rc::new(conjunto.themes.into_iter().next().unwrap())
}

/// As cores novas no tema do kit (menus, campos, foco), sem mexer no que o zoom ajustou nele.
pub fn reaplicar_cores(cx: &mut App) {
    let config = config_do_kit();
    Theme::update(cx, |tema| {
        let (letra, mono, raio, raio_lg) = (tema.font_size, tema.mono_font_size, tema.radius, tema.radius_lg);
        let aviso = (tema.notification.placement, tema.notification.margins.top, tema.notification.width, tema.notification.max_items);
        tema.apply_config(&config);
        (tema.font_size, tema.mono_font_size, tema.radius, tema.radius_lg) = (letra, mono, raio, raio_lg);
        (tema.notification.placement, tema.notification.margins.top, tema.notification.width, tema.notification.max_items) = aviso;
    });
}

pub fn aplicar(cx: &mut App) {
    if let Err(erro) = cx.text_system().add_fonts(FONTES.iter().map(|f| Cow::Borrowed(*f)).collect()) {
        eprintln!("vitral: as fontes embutidas não carregaram ({erro:#}); ficam as do sistema");
    }
    let config = config_do_kit();
    Theme::update(cx, |tema| {
        tema.apply_config(&config);
        // Avisos no topo, no centro: o canto de cima à direita é o dos botões da janela.
        tema.notification.placement = Anchor::TopCenter;
        tema.notification.margins.top = px(12.);
        tema.notification.width = px(LARGURA_AVISO);
        tema.notification.max_items = 3;
    });
    // As Animações desligadas nas Configurações, o GNOME sem animações ou a bancada com
    // VITRAL_SEM_ANIMACAO=1: tudo direto no estado final, inclusive o spinner, que fica parado.
    crate::ui::movimento::iniciar(cx);
}

/// As cores das superfícies num lugar só: o chumbo das frestas, os degraus de claro de cada peça,
/// as bordas, a luz de cima e os realces. Saiu de um estudo de seis acabamentos que o dono
/// comparou lado a lado; esta é a "Degraus", a escolhida: cada superfície num nível de claro bem
/// diferente, com a moldura quase preta.
pub mod superficie {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Variante {
        /// Fundo da janela (o `background` do tema). No Grafite é opaco e fica debaixo da chapa de
        /// metal: só aparece se o metal não puder ser pintado.
        pub vidro: u32,
        /// Fundo do trilho e da lista.
        pub vidro_leitura: u32,
        /// Fundo da conversa e dos painéis da direita.
        pub cartao: u32,
        pub cartao_elevado: u32,
        /// Popover, menu, dica e aviso.
        pub flutuante: u32,
        pub dialogo: u32,
        pub gaveta: u32,
        /// Código, saída de comando e diff.
        pub preto: u32,
        /// Campo de busca, trilho do segmentado, kbd.
        pub recuo: u32,
        /// Véu atrás dos diálogos.
        pub veu: u32,
        /// O escuro por cima do metal numa placa de cabeçalho (56%): com ele, `forte` e `texto`
        /// passam de 4,5:1 mesmo com a bolha mais clara atrás (o mínimo é 0x84).
        pub veu_de_placa: u32,
        /// O escuro por cima do metal num botão de ação ou numa gota (36%): o ícone branco passa
        /// de 3:1 (o mínimo é 0x4c).
        pub veu_de_botao: u32,
        /// Contorno de controle (campo, chip, selo).
        pub borda: u32,
        /// Contorno de popover, diálogo e lista de escolha.
        pub borda_forte: u32,
        /// Divisória por dentro de uma superfície.
        pub linha: u32,
        /// O que é traço e não contorno: trilha de barra, linha de árvore, separador e o fundo do
        /// selo neutro. Fica visível mesmo com `borda` transparente.
        pub traco: u32,
        /// Aro de 1 px por dentro das peças (alfa 0 = peça sem aro, como hoje).
        pub aro: u32,
        /// Linha de luz no topo de dentro das peças.
        pub brilho_topo: u32,
        /// Sombra de 1 px na base de dentro das peças (a `aresta_de_vidro`).
        pub sombra_de_baixo: u32,
        /// O brilho que entra uns 40 px pelo topo das peças. Travado em 3%: com 5% o apagado
        /// sobre a lista reprova no contraste.
        pub brilho_de_dentro: u32,
        /// Fio de pontos: separador e trilho, no lugar da linha cheia.
        pub fio: u32,
        /// A trama fina de fundo (placa de cabeçalho, cabeçalho de código).
        pub pontilhado_fino: u32,
        /// Quanto do gradiente de reflexo entra no topo das peças.
        pub reflexo: f32,
        /// Multiplicador das sombras dos elevados, flutuantes e diálogos.
        pub sombra: f32,
        pub hover: u32,
        pub selecao: u32,
        /// Começo e fim do gradiente da caixa de mensagem.
        pub caixa_quente: u32,
        pub caixa_preta: u32,
        /// Margem entre a janela e as peças, vão entre elas e o raio dos cantos: o acabamento
        /// "denso", que o dono escolheu olhando as seis opções lado a lado.
        pub margem: f32,
        pub raio_peca: f32,
        /// Corpo (o `foreground` do tema), o tênue, o dos metadados e o decorativo.
        pub texto: u32,
        pub tenue: u32,
        pub apagado: u32,
        pub fantasma: u32,
        /// Código em linha no texto.
        pub codigo: u32,
        /// A divisória de 1 px entre as peças emendadas do cartão das conversas.
        pub divisoria: u32,
        /// O texto das linhas da lista e o tênue dela (seções, horários, atalhos): no Linear, um
        /// pouco abaixo do corpo, para a lista não pesar mais que a conversa.
        pub texto_da_lista: u32,
        pub tenue_da_lista: u32,
        /// O texto da resposta do agente, um degrau abaixo do corpo (o `--texto-2` do protótipo).
        pub texto_2: u32,
        /// O escolhido da lista: o destaque da lista a 22% sobre o fundo dela (no tema padrão, a
        /// mesma cor da `selecao`).
        pub selecao_da_lista: u32,
        /// A base (opaca) onde uma cor é misturada para virar tinta sólida (as abas, o perigo).
        pub base_da_tinta: u32,
        /// O destaque (o único da tela): o cheio, o do ponteiro em cima, o apertado, o de texto
        /// pequeno (que precisa de 4,5:1 nas peças) e o que vai escrito por cima do cheio.
        pub destaque: u32,
        pub destaque_hover: u32,
        pub destaque_ativo: u32,
        pub destaque_texto: u32,
        pub sobre_destaque: u32,
    }

    /// O acabamento de antes de 12/11/2026: "Degraus" no azul-marinho da Stained Glass UI, com o
    /// reflexo no topo das peças, o gradiente na caixa de mensagem e a moldura quase preta.
    pub const MARINHO: Variante = Variante {
        vidro: 0x02030aee,
        vidro_leitura: 0x0e1322ff,
        cartao: 0x171e30ff,
        cartao_elevado: 0x1e2740ff,
        flutuante: 0x1b2339ff,
        dialogo: 0x1e2740ff,
        gaveta: 0x1e2740ff,
        preto: 0x05060aff,
        recuo: 0x00000066,
        veu: 0x020309cc,
        veu_de_placa: 0x0000008f,
        veu_de_botao: 0x0000005c,
        borda: 0x00000000,
        borda_forte: 0xffffff14,
        linha: 0xffffff0f,
        traco: 0xffffff14,
        aro: 0x00000000,
        brilho_topo: 0xffffff0f,
        // O marinho não tem a aresta: sem sombra embaixo nem brilho por dentro.
        sombra_de_baixo: 0x00000000,
        brilho_de_dentro: 0x00000000,
        fio: 0xffffff14,
        pontilhado_fino: 0xffffff0a,
        reflexo: 1.,
        sombra: 1.3,
        hover: 0xffffff14,
        selecao: 0x3a7fff3d,
        caixa_quente: 0x24396bf7,
        caixa_preta: 0x10131cf7,
        margem: 4.,
        raio_peca: 14.,
        texto: 0xdfe6fbff,
        tenue: 0x9ba8cfff,
        apagado: 0x8a97bfff,
        fantasma: 0x9ba8cf99,
        codigo: 0x9ccbffff,
        base_da_tinta: 0x121726ff,
        destaque: 0x3a7fffff,
        destaque_hover: 0x3374f0ff,
        destaque_ativo: 0x2a64d6ff,
        destaque_texto: 0x6b9bffff,
        sobre_destaque: 0xffffffff,
        divisoria: 0x0000009e,
        texto_da_lista: 0xdfe6fbff,
        tenue_da_lista: 0x8a97bfff,
        texto_2: 0xe1e1e3ff,
        selecao_da_lista: 0x3a7fff3d,
    };

    /// "Grafite" (12/11/2026, o dono quis "mais moderno e com menos cara de IA" e apontou o
    /// recent.design e o dithercn.dev como referência): os mesmos degraus de claro, num quase preto
    /// frio e sem cor, com um destaque só, lavanda. Chapado: sem a linha de luz nem o reflexo no topo das
    /// peças, sem gradiente na caixa e sem halo; o que separa as peças é o degrau de tom e o que
    /// marca o foco é a borda. A moldura tem a cor da lista (antes era um chumbo quase preto e a
    /// janela parecia ter uma borda preta em volta do menu). A seleção é neutra: o azul fica para
    /// o que é ação (o "+", o enviar, os links, o foco), um destaque só por tela. Depois ele pediu
    /// "partes transparentes e bordas estilizadas": o vidro voltou (a moldura e a lista translúcidas,
    /// a caixa de mensagem também), com um fio de contorno e a linha de luz no topo. No plano
    /// visual de 12/11 ("muito básico"), as peças ganharam a aresta de vidro inteira (fio nos
    /// lados, luz na quina, sombra na base, brilho de 3%), o painel ativo trocou a borda azul pelas
    /// cantoneiras e as linhas cheias viraram fio de pontos. Ele ainda achou "muito básico", colou
    /// a receita do metal líquido e pediu "tenta focar em vez de glass esse metal líquido": atrás
    /// de tudo passou a haver uma chapa de metal opaca (`componentes/metal.rs`), e o papel de
    /// parede saiu de cena. O app é a casca escura por cima dela.
    pub const GRAFITE: Variante = Variante {
        // O `vidro` é opaco: é a reserva debaixo do metal e o que a raiz do kit pinta. São duas
        // camadas: a BASE é a janela inteira, com a lista direto nela (sem cartão próprio), e é o
        // metal debaixo do `vidro_leitura`, um véu azul-escuro que deixa passar 11% dele; por cima
        // flutuam os cartões (a conversa, os painéis da direita), grafite e quase opacos (5%). O
        // que flutua sobre texto (menus, popovers) é praticamente opaco. Os alfas saem da conta
        // de contraste contra a cor mais clara do metal (`TETO_DO_METAL`).
        vidro: 0x0d0f13ff,
        vidro_leitura: 0x0a1533e3,
        cartao: 0x14161df2,
        cartao_elevado: 0x1c1f29ff,
        // Um degrau acima da peça de onde sai (o campo da caixa, a aba, o cartão), e opaco: com
        // 0x171a23f7 os menus da caixa e o sino ficavam mais escuros que ela e liam como buraco
        // (relatório de aparência, 16/11/2026, V4). Não passar de 0x1f222b: o apagado no recuo
        // do flutuante fica a 0,02 do piso de 4,5:1.
        flutuante: 0x1f222bff,
        dialogo: 0x1c1f29fa,
        gaveta: 0x1c1f29fa,
        preto: 0x08090cff,
        recuo: 0x00000066,
        veu: 0x05060899,
        veu_de_placa: 0x0000008f,
        veu_de_botao: 0x0000005c,
        // As bordas são um fio claro: nos controles, nas peças (o aro) e, mais forte, no que flutua.
        // No topo das peças e da caixa, uma linha de luz, como a quina de um vidro.
        borda: 0xffffff12,
        borda_forte: 0xffffff1f,
        linha: 0xffffff0f,
        traco: 0xffffff14,
        // A aresta de vidro das peças: o fio em volta, a luz na quina de cima, a sombra na base e
        // o brilho de 3% que entra pelo topo (ver `aresta_de_vidro`).
        aro: 0xffffff14,
        brilho_topo: 0xffffff24,
        sombra_de_baixo: 0x00000073,
        brilho_de_dentro: 0xffffff08,
        fio: 0xffffff24,
        pontilhado_fino: 0xffffff0a,
        reflexo: 0.,
        sombra: 1.2,
        hover: 0xffffff14,
        selecao: 0xffffff1c,
        // O campo da caixa de mensagem é opaco: é o miolo da moldura dupla (e do aro de metal).
        caixa_quente: 0x1c1f29ff,
        caixa_preta: 0x1c1f29ff,
        // A base aparece em volta dos cartões: 8 px dão a ela largura de camada, não de fresta.
        margem: 8.,
        // Os cartões (a conversa, os painéis): 16. Foram a 24 a pedido do dono ("mais arredondados
        // ainda", 13/11/2026) e voltaram no mesmo dia: com as abas do chat em pílula logo ao lado
        // do canto, a curva grande destoava ("diminuir a curva dessas bordas… parece errada com as
        // abas do chat").
        raio_peca: 16.,
        texto: 0xe2e5ecff,
        tenue: 0x9da3b3ff,
        apagado: 0x8e94a5ff,
        fantasma: 0x9da3b399,
        codigo: 0xc8cfe0ff,
        base_da_tinta: 0x14161dff,
        // Um azul cheio (o dono pediu "um azul mais azul"), com branco por cima.
        destaque: 0x2b63ffff,
        destaque_hover: 0x4a7bffff,
        destaque_ativo: 0x1f52e6ff,
        destaque_texto: 0x7c9dffff,
        sobre_destaque: 0xffffffff,
        divisoria: 0x0000009e,
        texto_da_lista: 0xe2e5ecff,
        tenue_da_lista: 0x8e94a5ff,
        texto_2: 0xe1e1e3ff,
        selecao_da_lista: 0xffffff1c,
    };

    /// "Linear" (25/11/2026): o acabamento do protótipo no jeito do Linear, que o dono aprovou
    /// ("tudo que fiz lá vc vai fazer aqui", `tarefas/linear/vitral-linear.html`). Chapado e sem
    /// profundidade: sem aro, sem luz na quina, sem sombra na base nem brilho por dentro, sem sombra
    /// de elevação. As cores são as que ele escolheu na Aparência do protótipo (Destaque #3170ff,
    /// painel #090a15, lista #030926, contraste 74): a moldura da caixa, os menus e os diálogos no
    /// mesmo quase preto (#030307), e o escolhido da lista, a aba aberta e o seu balão na MESMA cor
    /// cheia (o destaque a 22% sobre a lista, #0d2056).
    pub const LINEAR: Variante = Variante {
        vidro: 0x030926ff,
        vidro_leitura: 0x030926e3,
        cartao: 0x090a15f2,
        cartao_elevado: 0x191a24ff,
        flutuante: 0x030307ff,
        dialogo: 0x050509ff,
        gaveta: 0x090a15fa,
        preto: 0x030307ff,
        aro: 0x00000000,
        brilho_topo: 0x00000000,
        sombra_de_baixo: 0x00000000,
        brilho_de_dentro: 0x00000000,
        reflexo: 0.,
        sombra: 0.,
        selecao: 0x0d2056ff,
        caixa_quente: 0x13141fff,
        caixa_preta: 0x13141fff,
        raio_peca: 20.,
        texto: 0xf0f0f1ff,
        tenue: 0xacacb0ff,
        apagado: 0x939398ff,
        fantasma: 0xacacb099,
        base_da_tinta: 0x090a15ff,
        destaque: 0x3170ffff,
        destaque_hover: 0x5085ffff,
        destaque_ativo: 0x2459e0ff,
        destaque_texto: 0x8eb0ffff,
        divisoria: 0x03030780,
        texto_da_lista: 0xdcdde1ff,
        tenue_da_lista: 0xa1a3aeff,
        selecao_da_lista: 0x0d2056ff,
        // Sem margem da janela: a lista começa na beirada, com os 12 dela (protótipo).
        margem: 0.,
        ..GRAFITE
    };

    /// Proposta "Noite" (17/11/2026, "dá uma melhorada na paleta"): o Grafite puxado para o azul
    /// profundo da base, com as peças em azul-ardósia e o texto tênue mais separado do apagado.
    pub const NOITE: Variante = Variante {
        vidro: 0x0a0f1fff,
        vidro_leitura: 0x0b1940e6,
        cartao: 0x121a30f2,
        cartao_elevado: 0x1a2440ff,
        flutuante: 0x1e2944ff,
        dialogo: 0x1a2440fa,
        gaveta: 0x1a2440fa,
        preto: 0x070b16ff,
        caixa_quente: 0x1a2440ff,
        caixa_preta: 0x1a2440ff,
        hover: 0xffffff12,
        selecao: 0xffffff1a,
        selecao_da_lista: 0xffffff1a,
        texto: 0xe7ecf8ff,
        tenue: 0xa3aecaff,
        apagado: 0x8994b3ff,
        fantasma: 0xa3aeca99,
        codigo: 0xc3d0ffff,
        base_da_tinta: 0x121a30ff,
        destaque: 0x3170ffff,
        destaque_hover: 0x4d84ffff,
        destaque_ativo: 0x2459e0ff,
        destaque_texto: 0x8eacffff,
        ..GRAFITE
    };

    /// Proposta "Neutro" (17/11/2026): grafite sem tinta azul nas superfícies (o véu da base
    /// quase opaco, o metal mal aparece), com o azul só no que é ação.
    pub const NEUTRO: Variante = Variante {
        vidro: 0x0d0e11ff,
        vidro_leitura: 0x0f1115e8,
        cartao: 0x16181df4,
        cartao_elevado: 0x1e2127ff,
        flutuante: 0x23262dff,
        dialogo: 0x1e2127fa,
        gaveta: 0x1e2127fa,
        preto: 0x09090bff,
        caixa_quente: 0x1e2127ff,
        caixa_preta: 0x1e2127ff,
        texto: 0xe9eaedff,
        tenue: 0xa6aab4ff,
        apagado: 0x8d919bff,
        fantasma: 0xa6aab499,
        codigo: 0xd0d5dfff,
        base_da_tinta: 0x16181dff,
        destaque: 0x3b6dffff,
        destaque_hover: 0x5582ffff,
        destaque_ativo: 0x2a5ae8ff,
        destaque_texto: 0x8ea9ffff,
        ..GRAFITE
    };

    /// O acabamento em uso.
    pub const ATUAL: Variante = LINEAR;

    /// O acabamento que o `VITRAL_PALETA` (noite, neutro, marinho, grafite) pede, para comparar
    /// propostas na bancada: com ele, as cores escolhidas na Aparência não valem.
    fn forcada() -> Option<&'static Variante> {
        static FORCADA: std::sync::OnceLock<Option<&'static Variante>> = std::sync::OnceLock::new();
        *FORCADA.get_or_init(|| match std::env::var("VITRAL_PALETA").as_deref() {
            Ok("noite") => Some(&NOITE),
            Ok("neutro") => Some(&NEUTRO),
            Ok("marinho") => Some(&MARINHO),
            Ok("grafite") => Some(&GRAFITE),
            _ => None,
        })
    }

    /// A paleta das Configurações (`crate::paleta`), depois que o motor a leu.
    static EM_USO: std::sync::RwLock<Option<Variante>> = std::sync::RwLock::new(None);

    /// O acabamento em uso: o das cores escolhidas na Aparência, o `ATUAL` antes de o motor abrir,
    /// ou o que o `VITRAL_PALETA` pedir.
    pub fn atual() -> Variante {
        if let Some(v) = forcada() {
            return *v;
        }
        EM_USO.read().map(|v| v.unwrap_or(ATUAL)).unwrap_or(ATUAL)
    }

    /// Troca a paleta em uso; `true` se ela mudou.
    pub(super) fn definir(nova: Variante) -> bool {
        let Ok(mut em_uso) = EM_USO.write() else { return false };
        if *em_uso == Some(nova) {
            return false;
        }
        *em_uso = Some(nova);
        true
    }
}

/// As cores escolhidas em Configurações › Aparência viram a paleta em uso, e as bordas finas
/// ligam ou desligam. `true` se algo mudou: aí quem chamou reaplica o tema do kit
/// (`reaplicar_cores`) e redesenha as janelas. Trocar de tema não anima (a geração sobe).
pub fn definir_paleta(paleta: &crate::paleta::Paleta) -> bool {
    let bordas = BORDAS_FINAS.swap(paleta.bordas, std::sync::atomic::Ordering::Relaxed) != paleta.bordas;
    let mudou = superficie::definir(paleta.variante()) | bordas;
    if mudou {
        GERACAO_DO_TEMA.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    mudou
}

static BORDAS_FINAS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// O fio escuro de meio px em volta do cartão das conversas e do painel da direita (as "Bordas
/// finas" do protótipo: `box-shadow: 0 0 0 .5px rgba(0, 0, 0, .55)`).
pub fn bordas_finas() -> Option<BoxShadow> {
    BORDAS_FINAS
        .load(std::sync::atomic::Ordering::Relaxed)
        .then(|| BoxShadow::new(px(0.), px(0.), rgba(0x0000008c).into()).spread_radius(px(0.5)))
}

/// O raio dos cantos de cada peça (trilho, lista, conversa, painéis), no zoom de agora.
pub fn raio_da_peca() -> gpui_kit::Pixels {
    gpui_kit::px((superficie::ATUAL.raio_peca * zoom_dos_raios()).round())
}

/// O zoom da interface (1,5 é 150%), para os raios: as medidas dos controles são em rem e
/// crescem com o zoom, mas o raio é em px. Sem acompanhar, a 150% o enviar, os chips e os cartões
/// ficavam bem menos redondos que a 110% (dono, 13/11/2026, comparando a tela dele com as
/// capturas da bancada). Valem daqui o `raio::de` e o `raio_da_peca`; os fixos (`CONTROLE`,
/// `CARTAO`, `PECA`) ainda não crescem.
static ZOOM_DOS_RAIOS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1f32.to_bits());

/// O `zoom` avisa a cada mudança.
pub fn definir_zoom_dos_raios(fator: f32) {
    let fator = if fator.is_finite() && fator > 0. { fator } else { 1. };
    if ZOOM_DOS_RAIOS.swap(fator.to_bits(), std::sync::atomic::Ordering::Relaxed) != fator.to_bits() {
        GERACAO_DO_TEMA.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Sobe a cada troca de tema (a transparência, o zoom). O `movimento` compara com a de cada valor
/// animado: o alvo que muda junto com o tema pula direto, sem transição (dono, 23/11/2026: "turn
/// transitions off while the theme changes").
static GERACAO_DO_TEMA: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn geracao_do_tema() -> u32 {
    GERACAO_DO_TEMA.load(std::sync::atomic::Ordering::Relaxed)
}

fn zoom_dos_raios() -> f32 {
    f32::from_bits(ZOOM_DOS_RAIOS.load(std::sync::atomic::Ordering::Relaxed))
}

/// Quanto os fundos deixam ver o que está atrás da janela, do opaco ao forte: o alfa do véu da
/// base (o fundo azul, onde mora a lista), o alfa do cartão (o fundo cinza da conversa e dos
/// painéis) e a opacidade da chapa de metal. O dono pediu "os fundos um transparente meio fosco"
/// (13/11/2026) e, ao ver que o desfoque do que está atrás não existe em todo sistema (no GNOME
/// só com extensão), preferiu sem: "se não for, deixa sem mesmo". O padrão é o opaco, e a
/// transparência ficou como escolha das Configurações. Só o nível 1 tem o contraste conferido em
/// teste; nos de cima, quem escolhe troca leitura por vidro.
/// Os três níveis transparentes já foram mais fortes; o dono pediu menos em todos (13/11/2026).
// O "Leve" ficou menos transparente em 16/11/2026 (dono: "tenta diminuir a transparência só"):
// o véu da base foi de 0xc8 para 0xdc e o do cartão de 0xe6 para 0xee. Em 26/11/2026, com o fundo
// animado atrás, o véu da base (o fundo da lista) deixa ver a metade do que deixava em todos os
// níveis, e o cartão das conversas fica como estava (dono: "tá muito forte na parte do fundo que
// fica na lista, e no resto o do chat tá até pouco"). O véu era 0xe3, 0xdc, 0xa6 e 0x80.
pub const NIVEIS_DE_TRANSPARENCIA: [(u32, u32, f32); 4] =
    [(0xf1, 0xf2, 1.), (0xee, 0xee, 0.5), (0xd3, 0xcc, 0.35), (0xc0, 0xa6, 0.3)];
/// Os nomes dos níveis, na ordem.
pub const NOMES_DA_TRANSPARENCIA: [&str; 4] = ["Opaco", "Leve", "Média", "Forte"];
/// O nível com que o Vitral abre.
pub const TRANSPARENCIA_PADRAO: u8 = 0;

static TRANSPARENCIA: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(TRANSPARENCIA_PADRAO);

/// A Janela diz o nível escolhido a cada desenho.
pub fn definir_transparencia(nivel: u8) {
    if TRANSPARENCIA.swap(nivel.min(3), std::sync::atomic::Ordering::Relaxed) != nivel.min(3) {
        GERACAO_DO_TEMA.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

pub fn transparencia() -> u8 {
    TRANSPARENCIA.load(std::sync::atomic::Ordering::Relaxed)
}

/// Os tamanhos da letra da conversa, em % do resto da interface (o − e o + das Configurações andam
/// por eles).
pub const LETRAS_DA_CONVERSA: [u32; 8] = [80, 90, 100, 110, 120, 130, 140, 150];

static LETRA_DA_CONVERSA: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100);

/// Quanto as mensagens crescem em relação ao resto: 110 dá 1,1. Um valor fora da faixa (o
/// `config.json` editado à mão) fica na ponta mais perto, e o zero de um arquivo estragado, em 1.
pub fn fator_da_letra(porcento: u32) -> f32 {
    if porcento == 0 {
        return 1.;
    }
    let (menor, maior) = (LETRAS_DA_CONVERSA[0], LETRAS_DA_CONVERSA[LETRAS_DA_CONVERSA.len() - 1]);
    porcento.clamp(menor, maior) as f32 / 100.
}

/// A Janela diz a letra escolhida a cada desenho (como a transparência).
pub fn definir_letra_da_conversa(porcento: u32) {
    LETRA_DA_CONVERSA.store(porcento, std::sync::atomic::Ordering::Relaxed);
}

/// O fator da letra da conversa de agora (`fator_da_letra` da escolha das Configurações).
pub fn fator_da_conversa() -> f32 {
    fator_da_letra(LETRA_DA_CONVERSA.load(std::sync::atomic::Ordering::Relaxed))
}

fn nivel_de_transparencia(nivel: u8) -> (u32, u32, f32) {
    NIVEIS_DE_TRANSPARENCIA[usize::from(nivel.min(3))]
}

/// A opacidade da chapa de metal no nível de agora.
pub fn opacidade_do_metal() -> f32 {
    nivel_de_transparencia(transparencia()).2
}

/// A margem entre a janela e as peças.
pub fn margem_da_janela() -> gpui_kit::Pixels {
    gpui_kit::px(superficie::ATUAL.margem)
}


/// Cores próprias do Vitral que não existem no tema dos componentes.
pub struct Cores;

impl Cores {
    /// Fundo da janela (o `background` do tema), que a raiz do kit pinta. Fica debaixo da chapa
    /// de metal (`componentes::metal::fundo`), que é quem aparece nas frestas e na margem.
    pub fn vidro() -> Hsla {
        rgba(superficie::atual().vidro).into()
    }
    /// Fundo das colunas com texto pequeno (trilho e lista): fumê sobre a chapa de metal, que o
    /// tinge de azul. Garante o contraste no pior caso (a bolha mais clara atrás).
    pub fn vidro_leitura() -> Hsla {
        Self::vidro_leitura_no(crate::tema::transparencia())
    }
    /// O véu da base num nível de transparência (ver `NIVEIS_DE_TRANSPARENCIA`).
    pub fn vidro_leitura_no(nivel: u8) -> Hsla {
        let (veu, _, _) = crate::tema::nivel_de_transparencia(nivel);
        rgba((superficie::atual().vidro_leitura & 0xffffff00) | veu).into()
    }
    /// O cartão num nível de transparência.
    pub fn cartao_no(nivel: u8) -> Hsla {
        let (_, cartao, _) = crate::tema::nivel_de_transparencia(nivel);
        rgba((superficie::atual().cartao & 0xffffff00) | cartao).into()
    }
    /// Tudo o que flutua sobre texto (popover, menu, dica, aviso): opaco, um degrau acima da peça
    /// de onde sai, por cima do vidro azul (o `popover.background` do tema).
    pub fn flutuante() -> Hsla {
        rgba(superficie::atual().flutuante).into()
    }
    /// A gaveta (a lista ou a Equipe por cima da conversa, em janela estreita): flutua, então é
    /// opaca, mas continua no azul das peças.
    pub fn gaveta() -> Hsla {
        rgba(superficie::atual().gaveta).into()
    }
    /// O preto de dentro: código, saída de comando, diff e o disco dos botões redondos do trilho.
    pub fn preto() -> Hsla {
        rgba(superficie::atual().preto).into()
    }
    /// Trilho do segmentado, campo de busca, kbd, selos no vidro.
    pub fn recuo() -> Hsla {
        rgba(superficie::atual().recuo).into()
    }
    /// O recuo dentro de um flutuante: no escuro do flutuante o escuro some, então ali ele é um
    /// claro leve.
    pub fn recuo_no_flutuante() -> Hsla {
        rgba(0xffffff0d).into()
    }
    /// Fundo do GrupoDeOpcoes.
    pub fn grupo() -> Hsla {
        rgba(0xffffff0a).into()
    }
    /// Véu atrás dos diálogos (o `overlay` do tema).
    pub fn veu() -> Hsla {
        rgba(superficie::atual().veu).into()
    }
    /// O escuro por cima do metal numa placa de cabeçalho: em cima dele só `forte` e `texto`.
    pub fn veu_de_placa() -> Hsla {
        rgba(superficie::atual().veu_de_placa).into()
    }
    /// O escuro por cima do metal num botão de ação ou numa gota: em cima dele só ícone branco.
    pub fn veu_de_botao() -> Hsla {
        rgba(superficie::atual().veu_de_botao).into()
    }
    pub fn pressionado() -> Hsla {
        // 15%: acima do escolhido, que no Linear é uma cor cheia (#0d2056) e não um véu.
        rgba(0xffffff26).into()
    }
    /// Escolhido num segmentado de outra cor (ciano no esforço).
    pub fn selecao_de(cor: Hsla) -> Hsla {
        cor.opacity(0.20)
    }
    /// Fundo de pílula de estado.
    pub fn tinta(cor: Hsla) -> Hsla {
        cor.opacity(0.12)
    }
    pub fn tinta_borda(cor: Hsla) -> Hsla {
        cor.opacity(0.30)
    }
    /// Fundo do cartão de erro e do Alerta: a cor fica no ícone, e o fundo é só um véu (balão e
    /// cartão de erro, 16/11/2026; era a `tinta`, de 12%).
    pub fn tinta_leve(cor: Hsla) -> Hsla {
        cor.opacity(0.06)
    }
    /// O fio do cartão de erro e do Alerta (era a `tinta_borda`, de 30%).
    pub fn tinta_borda_leve(cor: Hsla) -> Hsla {
        cor.opacity(0.18)
    }
    /// A cor misturada ao marinho, opaca: o fundo das abas e da moldura de perigo.
    pub fn tinta_solida(cor: Hsla, alfa: f32) -> Hsla {
        Hsla::from(rgba(superficie::atual().base_da_tinta)).blend(cor.opacity(alfa))
    }
    /// Hover e realce de item perigoso.
    pub fn perigo_hover() -> Hsla {
        Self::vermelho().opacity(0.14)
    }
    /// Campos, chip aberto, selos de contorno, separador em superfície opaca.
    pub fn borda() -> Hsla {
        rgba(superficie::atual().borda).into()
    }
    /// Contorno de popover, diálogo e lista de escolha; trilho dos anéis.
    pub fn borda_forte() -> Hsla {
        rgba(superficie::atual().borda_forte).into()
    }
    /// Títulos, item selecionado ou não lido.
    pub fn forte() -> Hsla {
        rgba(0xffffffff).into()
    }
    /// Corpo (o `foreground` do tema).
    pub fn texto() -> Hsla {
        rgba(superficie::atual().texto).into()
    }
    /// Texto sobre selo sólido de cor.
    pub fn sobre_cor() -> Hsla {
        rgba(0x0f1422ff).into()
    }
    /// Só o esforço. Era um ciano (#67E7F9); no acabamento chapado as barrinhas e o rótulo do
    /// esforço são neutros, como os outros chips da caixa: o destaque da tela é um só.
    pub fn ciano() -> Hsla {
        Self::texto()
    }
    /// Automação e IA.
    pub fn violeta() -> Hsla {
        rgba(0xb39dffff).into()
    }
    /// Cores de marca: só no logo e no anel de uso da conta.
    pub fn claude() -> Hsla {
        rgba(0xd97757ff).into()
    }
    pub fn codex() -> Hsla {
        rgba(0x5b6cf9ff).into()
    }
    /// O roxo do meio do degradê do Gemini.
    pub fn gemini() -> Hsla {
        rgba(0x9177c7ff).into()
    }
    pub fn linear() -> Hsla {
        rgba(0x5e6ad2ff).into()
    }
    /// Começo do gradiente da caixa de mensagem: o azul do vidro (o dono trocou o laranja pelo azul).
    pub fn caixa_quente() -> Hsla {
        rgba(superficie::atual().caixa_quente).into()
    }
    /// Fim do gradiente da caixa, com o alfa do começo.
    pub fn caixa_preta() -> Hsla {
        rgba(superficie::atual().caixa_preta).into()
    }
    /// Reflexo azul na borda direita da caixa (camada por cima do gradiente).
    pub fn caixa_brilho() -> Background {
        linear_gradient(
            90.,
            linear_color_stop(rgba(0x3a7fff00), 0.78),
            linear_color_stop(rgba(0x3a7fff00), 1.),
        )
    }
    pub fn brilho_topo() -> Hsla {
        rgba(superficie::atual().brilho_topo).into()
    }
    pub fn sombra_caixa() -> Hsla {
        preto_de_sombra(0x59)
    }
    /// Fundo do BotaoRedondo claro.
    pub fn claro() -> Hsla {
        rgba(0xeef2ffff).into()
    }
    /// Barrinhas de sinal apagadas.
    pub fn barra_apagada() -> Hsla {
        rgba(0xffffff2e).into()
    }
    /// Cartões (barra lateral, conversa, painel): navy mais claro e mais opaco que o fundo.
    pub fn cartao() -> Hsla {
        Self::cartao_no(crate::tema::transparencia())
    }
    /// O cartão sem a transparência: o pé do cartão da conversa (onde mora a caixa de mensagem
    /// e a luz de baixo) e os degradês que somem nele.
    pub fn cartao_opaco() -> Hsla {
        rgba(superficie::atual().cartao | 0xff).into()
    }
    pub fn cartao_elevado() -> Hsla {
        rgba(superficie::atual().cartao_elevado).into()
    }
    /// Diálogos: opacos, senão o texto da conversa aparece através deles.
    pub fn dialogo() -> Hsla {
        rgba(superficie::atual().dialogo).into()
    }
    pub fn destaque() -> Hsla {
        rgba(superficie::atual().destaque).into()
    }
    pub fn destaque_ativo() -> Hsla {
        rgba(superficie::atual().destaque_ativo).into()
    }
    /// O fundo do seu balão e da pílula do comando: o destaque bem baixo misturado à base, opaco.
    /// Desceu de 0,20 para 0,16 com a fala um degrau abaixo da resposta (balão e cartão de erro,
    /// 16/11/2026).
    pub fn balao() -> Hsla {
        // No Linear (25/11/2026), a mesma cor cheia do escolhido da lista e da aba aberta.
        Self::selecao()
    }
    /// O destaque para texto pequeno: o cheio pode ficar abaixo de 4,5:1 nas peças.
    pub fn destaque_texto() -> Hsla {
        rgba(superficie::atual().destaque_texto).into()
    }
    /// Texto e ícone por cima do destaque cheio (o "+", o botão primário).
    pub fn sobre_destaque() -> Hsla {
        rgba(superficie::atual().sobre_destaque).into()
    }
    /// Item escolhido numa lista (par com `hover`): azul, mais forte que o hover.
    pub fn selecao() -> Hsla {
        rgba(superficie::atual().selecao).into()
    }
    /// Mouse por cima de um item de lista: neutro, para não parecer mais escolhido que a seleção.
    pub fn hover() -> Hsla {
        rgba(superficie::atual().hover).into()
    }
    /// Contorno do painel ativo e da caixa com foco: forte o bastante para ver no vidro.
    pub fn destaque_borda() -> Hsla {
        Self::destaque().opacity(0.6)
    }
    pub fn codigo() -> Hsla {
        rgba(superficie::atual().codigo).into()
    }
    pub fn tenue() -> Hsla {
        rgba(superficie::atual().tenue).into()
    }
    /// Metadados (horário, estados vazios, avisos): opaco para dar 4,5:1 até sobre a neblina.
    pub fn apagado() -> Hsla {
        rgba(superficie::atual().apagado).into()
    }
    /// Só para o que é decorativo (separadores, ícone da busca): pode sumir no fundo.
    pub fn fantasma() -> Hsla {
        rgba(superficie::atual().fantasma).into()
    }
    /// Aro de 1 px por dentro das peças (o fio da `aresta_de_vidro`): transparente na variante
    /// que não tem aro.
    pub fn aro_da_peca() -> Hsla {
        rgba(superficie::atual().aro).into()
    }
    /// O fio do meio da divisória entre as conversas emendadas (`gradiente_da_divisoria`): um
    /// sulco escuro, mais fundo que o cartão (dono, 17/11/2026: "faz esse risco de uma cor mais
    /// escura"; era um fio claro de 14%).
    pub fn divisoria() -> Hsla {
        rgba(superficie::atual().divisoria).into()
    }
    /// A pastilha do número do painel na lista (protótipo Linear, 25/11/2026).
    pub fn numero() -> Hsla {
        rgba(0x1a2752ff).into()
    }
    /// O texto da resposta do agente, um pouco abaixo do corpo (protótipo Linear, 25/11/2026).
    pub fn texto_da_fala() -> Hsla {
        rgba(superficie::atual().texto_2).into()
    }
    /// O escolhido da lista (a linha do chat aberto, o item da navegação): o destaque da lista.
    pub fn selecao_da_lista() -> Hsla {
        rgba(superficie::atual().selecao_da_lista).into()
    }
    /// O texto do seu balão, no azul.
    pub fn texto_do_balao() -> Hsla {
        rgba(0xe8ebf5ff).into()
    }
    /// O texto das linhas da lista (os chats, as abas da lista).
    pub fn texto_da_lista() -> Hsla {
        rgba(superficie::atual().texto_da_lista).into()
    }
    /// O tênue da lista: seções, horários e atalhos.
    pub fn tenue_da_lista() -> Hsla {
        rgba(superficie::atual().tenue_da_lista).into()
    }
    /// A sombra de 1 px na base de dentro das peças.
    pub fn sombra_de_baixo() -> Hsla {
        rgba(superficie::atual().sombra_de_baixo).into()
    }
    /// O brilho de 3% que entra pelo topo das peças.
    pub fn brilho_de_dentro() -> Hsla {
        rgba(superficie::atual().brilho_de_dentro).into()
    }
    /// O fio de pontos (separador e trilho) e o tracinho dos rótulos de seção.
    pub fn fio() -> Hsla {
        rgba(superficie::atual().fio).into()
    }
    /// A trama fina de fundo (placa de cabeçalho, cabeçalho de código).
    pub fn pontilhado_fino() -> Hsla {
        rgba(superficie::atual().pontilhado_fino).into()
    }
    /// Preto puro, só como base das sombras de 1 px (a opacidade vem de quem usa).
    pub fn preto_puro() -> Hsla {
        rgba(0x000000ff).into()
    }
    pub fn linha() -> Hsla {
        rgba(superficie::atual().linha).into()
    }
    /// Trilha de barra, linha de árvore, separador e fundo de selo neutro: o traço que continua à
    /// vista com as peças e os controles sem contorno.
    pub fn traco() -> Hsla {
        rgba(superficie::atual().traco).into()
    }
    pub fn verde() -> Hsla {
        rgba(0x4ade80ff).into()
    }
    pub fn ambar() -> Hsla {
        rgba(0xf5b642ff).into()
    }
    /// O que espera você (pedido de permissão, parada, o número no sino): um azul-claro, da
    /// família do destaque e mais leve que ele, para não se confundir com o que está rodando ou
    /// com a resposta nova. Era âmbar; o dono pediu azul (13/11/2026: "por que tem umas coisas
    /// amarelas? tem que ser azul"). O âmbar fica para aviso de verdade: uso do plano alto, modo
    /// Sem perguntar, confirmação de risco.
    pub fn espera() -> Hsla {
        rgba(0x8ecbffff).into()
    }
    pub fn vermelho() -> Hsla {
        rgba(0xf87171ff).into()
    }
}

#[cfg(test)]
mod testes_da_letra {
    use super::{LETRAS_DA_CONVERSA, fator_da_letra};

    #[test]
    fn a_letra_da_conversa_e_uma_fracao_do_resto_e_nunca_sai_da_faixa() {
        assert_eq!(fator_da_letra(100), 1.);
        assert_eq!(fator_da_letra(120), 1.2);
        assert_eq!(fator_da_letra(80), 0.8);
        // Editado à mão para fora da faixa: a ponta mais perto. Zero (arquivo estragado): igual.
        assert_eq!(fator_da_letra(40), 0.8);
        assert_eq!(fator_da_letra(900), 1.5);
        assert_eq!(fator_da_letra(0), 1.);
        // Os degraus têm o 100 (o padrão) e crescem.
        assert!(LETRAS_DA_CONVERSA.contains(&100));
        assert!(LETRAS_DA_CONVERSA.windows(2).all(|par| par[0] < par[1]));
    }
}

#[cfg(test)]
pub(crate) mod testes {
    use super::{Cores, Icones, TEMA, raio};
    use gpui_kit::{Hsla, Rgba, rgb};
    use gpui_kit::component::{
        ThemeMode, ThemeSet,
        highlighter::{HighlightTheme, highlight_code},
    };
    use gpui_kit::{AssetSource as _, assets::IconName};

    #[test]
    fn tema_traz_as_cores_de_sintaxe() {
        let conjunto: ThemeSet = serde_json::from_str(TEMA).expect("tema do Vitral");
        let realce = conjunto.themes[0].highlight.as_ref().expect("seção highlight");
        assert!(realce.editor_background.is_some());
        assert!(realce.syntax.keyword.is_some());
        assert!(realce.syntax.comment_doc.is_some());
        assert!(realce.syntax.title.and_then(|t| t.font_weight).is_some());
    }

    #[test]
    fn codigo_sai_com_as_cores_do_tema() {
        let conjunto: ThemeSet = serde_json::from_str(TEMA).expect("tema do Vitral");
        let estilo = conjunto.themes[0].highlight.clone().expect("seção highlight");
        let palavra_chave = estilo.syntax.keyword.and_then(|k| k.color);
        let tema = HighlightTheme {
            name: "Vitral".into(),
            appearance: ThemeMode::Dark,
            style: estilo,
        };
        let cores = highlight_code("rs", "fn main() { let x = 1; }", &tema);
        assert!(cores.iter().any(|(faixa, cor)| faixa.start == 0 && cor.color == palavra_chave));
    }

    /// Todo ícone do Lucide usado no código precisa carregar, senão sai em branco.
    #[test]
    fn icones_lucide_registrados() {
        let mut pastas = vec![std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))];
        let mut usados = Vec::new();
        while let Some(pasta) = pastas.pop() {
            for entrada in std::fs::read_dir(pasta).unwrap().flatten() {
                let caminho = entrada.path();
                if caminho.is_dir() {
                    pastas.push(caminho);
                } else if caminho.extension().is_some_and(|e| e == "rs") {
                    let texto = std::fs::read_to_string(&caminho).unwrap();
                    for (i, _) in texto.match_indices("Lucide::") {
                        let nome: String = texto[i + 8..].chars().take_while(|c| c.is_alphanumeric()).collect();
                        if !nome.is_empty() {
                            usados.push(nome);
                        }
                    }
                }
            }
        }
        usados.sort();
        usados.dedup();
        assert!(!usados.is_empty());
        for nome in usados {
            let icone = IconName::ALL
                .iter()
                .find(|i| format!("{i:?}") == nome)
                .unwrap_or_else(|| panic!("Lucide::{nome} não existe"));
            let carregado = Icones.load(&icone.path()).ok().flatten();
            assert!(carregado.is_some(), "Lucide::{nome} não está em IconesExtras");
        }
    }

    /// Cor opaca de `cor` pintada sobre `fundo` (opaco).
    fn sobre(cor: Hsla, fundo: Rgba) -> Rgba {
        let c = Rgba::from(cor);
        let mistura = |a: f32, b: f32| a * c.a + b * (1. - c.a);
        Rgba { r: mistura(c.r, fundo.r), g: mistura(c.g, fundo.g), b: mistura(c.b, fundo.b), a: 1. }
    }

    fn luminancia(c: Rgba) -> f32 {
        let canal = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        0.2126 * canal(c.r) + 0.7152 * canal(c.g) + 0.0722 * canal(c.b)
    }

    fn contraste(a: Rgba, b: Rgba) -> f32 {
        let (la, lb) = (luminancia(a), luminancia(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    /// Uma cor do tema ("#rrggbb" ou "#rrggbbaa").
    fn cor_do_tema(texto: &str) -> Rgba {
        let hex = texto.trim_start_matches('#');
        let valor = u32::from_str_radix(hex, 16).unwrap();
        gpui_kit::rgba(if hex.len() == 6 { valor << 8 | 0xff } else { valor })
    }

    fn mesma_cor(a: Hsla, b: Rgba) -> bool {
        let a = Rgba::from(a);
        [(a.r, b.r), (a.g, b.g), (a.b, b.b), (a.a, b.a)].iter().all(|(x, y)| (x - y).abs() < 0.002)
    }

    #[test]
    fn vidro_e_flutuante_iguais_aos_do_tema() {
        let json: serde_json::Value = serde_json::from_str(&super::tema_json()).unwrap();
        let cores = &json["themes"][0]["colors"];
        let fundo = cor_do_tema(cores["background"].as_str().unwrap());
        assert!(mesma_cor(Cores::vidro(), fundo), "Cores::vidro() diferente do background do tema");
        // Os flutuantes do kit (menus, popovers, dicas, avisos) e os desenhados à mão.
        let popover = cor_do_tema(cores["popover.background"].as_str().unwrap());
        assert!(mesma_cor(Cores::flutuante(), popover), "Cores::flutuante() diferente do popover.background");
    }

    /// Texto e cores semânticas passam de 4,5:1 nas superfícies de leitura, com a cor mais clara
    /// que a chapa de metal entrega atrás delas (o pior caso: a janela é opaca, o que fica atrás
    /// do texto é o metal, não o papel de parede).
    #[test]
    fn contraste_dos_tokens_no_pior_caso() {
        let atras = Rgba::from(rgb(crate::ui::componentes::metal::TETO_DO_METAL));
        // A base e o cartão da conversa são meio transparentes (dono, 13/11/2026): atrás deles
        // não há mais só o metal. A conta usa o metal na opacidade do nível sobre um papel de
        // parede cinza médio; com um papel de parede branco nada aqui garante a leitura.
        // O nível conferido é o "Leve" (1): os de cima são escolha de quem usa.
        let (_, _, metal) = crate::tema::NIVEIS_DE_TRANSPARENCIA[1];
        let papel_medio = Rgba::from(rgb(0x808080));
        let atras_da_base = Rgba {
            r: atras.r * metal + papel_medio.r * (1. - metal),
            g: atras.g * metal + papel_medio.g * (1. - metal),
            b: atras.b * metal + papel_medio.b * (1. - metal),
            a: 1.,
        };
        let leitura = sobre(Cores::vidro_leitura_no(1), atras_da_base);
        let painel = sobre(Cores::cartao_no(1), leitura);
        let flutuante = sobre(Cores::flutuante(), atras);
        // O `.reflexo()` escurece o topo das peças, onde fica o título (#0000001a): o texto
        // precisa continuar legível sobre ele. A linha de luz é de 1 px, sem texto em cima.
        let reflexo = |fundo: Rgba| sobre(gpui_kit::rgba(0x0000001a).into(), fundo);
        // A `aresta_de_vidro` clareia o topo das peças com o brilho de dentro: os títulos ficam
        // em cima dele. Travado em 3% (com 5% o apagado sobre a lista reprova).
        let brilho = |fundo: Rgba| sobre(Cores::brilho_de_dentro(), fundo);
        let superficies = [
            // A base (a lista mora nela) não tem a aresta de vidro: sem brilho por cima.
            ("vidro de leitura", leitura),
            ("painel", painel),
            ("painel com brilho", brilho(painel)),
            ("flutuante", flutuante),
            ("diálogo", sobre(Cores::dialogo(), atras)),
            ("gaveta", sobre(Cores::gaveta(), atras)),
            ("painel com reflexo", reflexo(painel)),
            ("flutuante com reflexo", reflexo(flutuante)),
            ("cartão elevado com reflexo", reflexo(sobre(Cores::cartao_elevado(), painel))),
            // As partes pretas: código e diff no painel, o fim da caixa de mensagem.
            ("preto", sobre(Cores::preto(), painel)),
            ("fim preto da caixa", sobre(Cores::caixa_preta(), painel)),
            ("recuo no flutuante", sobre(Cores::recuo_no_flutuante(), flutuante)),
            // O seu balão é opaco: o código em linha e as pílulas vão no poço escuro dentro dele.
            ("balão", Rgba::from(Cores::balao())),
            ("poço no balão", sobre(Cores::recuo(), Rgba::from(Cores::balao()))),
            // O cartão de erro mora no painel translúcido: vale a regra dele.
            ("cartão de erro", sobre(Cores::tinta_leve(Cores::vermelho()), painel)),
            ("poço do cartão de erro", sobre(Cores::recuo(), sobre(Cores::tinta_leve(Cores::vermelho()), painel))),
        ];
        let textos = [
            ("forte", Cores::forte()),
            ("texto", Cores::texto()),
            ("tenue", Cores::tenue()),
            ("apagado", Cores::apagado()),
            ("destaque_texto", Cores::destaque_texto()),
            ("ciano", Cores::ciano()),
            ("verde", Cores::verde()),
            ("ambar", Cores::ambar()),
            ("vermelho", Cores::vermelho()),
            ("violeta", Cores::violeta()),
            ("codigo", Cores::codigo()),
        ];
        for (nome_fundo, fundo) in superficies {
            for (nome, cor) in textos {
                let c = contraste(sobre(cor, fundo), fundo);
                // A base ficou meio transparente e fosca a pedido do dono (13/11/2026: "é pra
                // deixar mais transparente o Vitral, os fundos azul e cinza"): nela, o título e o
                // texto continuam em 4,5:1, e as vozes baixas e as cores de estado valem 3:1 (o
                // que está atrás da janela ainda pesa). Nos cartões de conversa vale o mesmo; no
                // que flutua (menus, diálogos, a caixa de mensagem), tudo em 4,5:1.
                let transparente = matches!(
                    nome_fundo,
                    "vidro de leitura" | "painel" | "painel com brilho" | "painel com reflexo" | "cartão de erro" | "poço do cartão de erro"
                )
                    && !matches!(nome, "forte" | "texto");
                let minimo = if transparente { 3. } else { 4.5 };
                assert!(c >= minimo, "{nome} sobre {nome_fundo}: {c:.2}:1");
            }
        }
        // Sobre o metal com véu: na placa de cabeçalho só vão `forte` e `texto` (4,5:1); no botão
        // de ação e na gota, só o ícone branco (3:1). No metal cru, nada.
        let placa = sobre(Cores::veu_de_placa(), atras);
        for (nome, cor) in [("forte", Cores::forte()), ("texto", Cores::texto())] {
            let c = contraste(sobre(cor, placa), placa);
            assert!(c >= 4.5, "{nome} sobre placa de metal: {c:.2}:1");
        }
        let botao = sobre(Cores::veu_de_botao(), atras);
        let c = contraste(sobre(Cores::sobre_destaque(), botao), botao);
        assert!(c >= 3., "ícone sobre botão de metal: {c:.2}:1");
    }

    /// As cores de sintaxe do tema passam de 4,5:1 no preto dos blocos de código e do diff (as
    /// linhas trocadas ganham só 12% de verde ou vermelho por baixo).
    #[test]
    fn sintaxe_legivel_no_preto() {
        let json: serde_json::Value = serde_json::from_str(TEMA).unwrap();
        let realce = &json["themes"][0]["highlight"];
        let preto = Rgba::from(Cores::preto());
        let fundos = [
            ("preto", preto),
            ("linha que entrou", sobre(Cores::verde().opacity(0.12), preto)),
            ("linha que saiu", sobre(Cores::vermelho().opacity(0.12), preto)),
        ];
        let mut cores: Vec<(String, Rgba)> = realce["syntax"]
            .as_object()
            .unwrap()
            .iter()
            .filter_map(|(nome, estilo)| Some((nome.clone(), cor_do_tema(estilo["color"].as_str()?))))
            .collect();
        cores.push(("editor_foreground".into(), cor_do_tema(realce["editor_foreground"].as_str().unwrap())));
        assert!(cores.len() > 20);
        for (nome_fundo, fundo) in fundos {
            for (nome, cor) in &cores {
                let c = contraste(sobre((*cor).into(), fundo), fundo);
                assert!(c >= 4.5, "{nome} sobre {nome_fundo}: {c:.2}:1");
            }
        }
    }

    /// Hover, seleção e pressionado continuam à vista no quase preto dos flutuantes e no preto do
    /// código, e a seleção segue mais forte que o hover.
    #[test]
    fn realces_visiveis_no_preto() {
        let atras = Rgba::from(rgb(crate::ui::componentes::metal::TETO_DO_METAL));
        let fundos = [("flutuante", sobre(Cores::flutuante(), atras)), ("preto", Rgba::from(Cores::preto()))];
        for (nome_fundo, fundo) in fundos {
            let realce = |cor: Hsla| contraste(sobre(cor, fundo), fundo);
            let (hover, selecao) = (realce(Cores::hover()), realce(Cores::selecao()));
            assert!(hover >= 1.12, "hover sobre {nome_fundo}: {hover:.3}:1");
            assert!(selecao > hover + 0.05, "seleção sobre {nome_fundo}: {selecao:.3}:1 (hover {hover:.3})");
            assert!(realce(Cores::pressionado()) > selecao, "pressionado sobre {nome_fundo}");
            // O acabamento escolhido não contorna as peças (`borda()` é transparente de
            // propósito): quem precisa aparecer é o contorno forte, o de popover e diálogo.
            assert!(realce(Cores::borda_forte()) >= 1.12, "borda forte sobre {nome_fundo}");
            // Mas trilha, linha de árvore, separador e selo neutro não são contorno: continuam
            // à vista (eles usam o `traco()`, e os separadores do kit, a `linha()`).
            assert!(realce(Cores::traco()) >= 1.12, "traço sobre {nome_fundo}");
            assert!(realce(Cores::linha()) >= 1.08, "linha sobre {nome_fundo}");
        }
        // O recuo é um escuro por cima. No acabamento Linear (25/11/2026) o flutuante é o quase
        // preto da moldura da caixa, o tom mais escuro da tela: ali um escuro não aparece, e no
        // protótipo aprovado os recuos dentro do menu também são desse preto. Quem precisa de recuo
        // sobre um fundo já escuro usa o `recuo_no_flutuante`, um claro, que tem de aparecer.
        let flutuante = fundos[0].1;
        assert!(contraste(sobre(Cores::recuo_no_flutuante(), flutuante), flutuante) >= 1.08);
    }

    pub(crate) fn arquivos_rs(pasta: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut pastas = vec![pasta.to_path_buf()];
        let mut arquivos = Vec::new();
        while let Some(pasta) = pastas.pop() {
            for entrada in std::fs::read_dir(pasta).unwrap().flatten() {
                let caminho = entrada.path();
                if caminho.is_dir() {
                    pastas.push(caminho);
                } else if caminho.extension().is_some_and(|e| e == "rs") {
                    arquivos.push(caminho);
                }
            }
        }
        arquivos.sort();
        arquivos
    }

    /// Métodos de espaço que o teste confere.
    const ESPACOS: [&str; 17] =
        ["gap_x", "gap_y", "gap", "px", "py", "pt", "pb", "pl", "pr", "mx", "my", "mt", "mb", "ml", "mr", "p", "m"];

    /// O que está fora da grade num texto: `.gap(r(10.))`, `.px(r(-3.))` e os atalhos do Tailwind
    /// (`.gap_2()`, `.py_0p5()`; o `_0()` e o `_auto()` passam). Valor dentro de `if` escapa.
    fn fora_da_grade(texto: &str) -> Vec<String> {
        const ESCALA: [f32; 9] = [0., 2., 4., 6., 8., 12., 16., 24., 32.];
        let mut achados = Vec::new();
        for (i, _) in texto.match_indices('.') {
            let resto = &texto[i + 1..];
            let nome: String = resto.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            let depois = &resto[nome.len()..];
            if ESPACOS.contains(&nome.as_str()) {
                let Some(dentro) = depois.strip_prefix("(r(") else { continue };
                let numero: String = dentro.chars().take_while(|c| *c != ')').collect();
                let Ok(valor) = numero.trim().trim_end_matches('.').parse::<f32>() else { continue };
                if !ESCALA.contains(&valor.abs()) {
                    achados.push(format!(".{nome}(r({numero}))"));
                }
            } else if depois.starts_with("()")
                && ESPACOS.iter().any(|base| {
                    nome.strip_prefix(*base)
                        .and_then(|s| s.strip_prefix('_'))
                        .is_some_and(|s| s.starts_with(|c: char| c.is_ascii_digit()) && s != "0")
                })
            {
                achados.push(format!(".{nome}()"));
            }
        }
        achados
    }

    #[test]
    fn espacos_na_escala() {
        assert_eq!(fora_da_grade(".gap(r(10.)).px(r(8.)).mt(r(-4.)).p(r(3.))"), [".gap(r(10.))", ".p(r(3.))"]);
        assert_eq!(fora_da_grade(".gap_2().px_0().mx_auto().py_0p5()"), [".gap_2()", ".py_0p5()"]);
        assert!(fora_da_grade(".size(r(10.)).top(r(3.)).px(r(altura))").is_empty());
        let raiz = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui"));
        let mut erros = Vec::new();
        for caminho in arquivos_rs(raiz) {
            let nome = caminho.strip_prefix(raiz).unwrap().to_string_lossy().to_string();
            let texto = std::fs::read_to_string(&caminho).unwrap();
            for achado in fora_da_grade(&texto) {
                erros.push(format!("{nome}: {achado}"));
            }
        }
        assert!(erros.is_empty(), "espaço fora da escala 2/4/6/8/12/16/24/32:\n{}", erros.join("\n"));
    }

    /// Os raios fora da escala num texto: `.rounded(px(6.))`, `.rounded_t(r(4.))`, um número solto,
    /// os atalhos do Tailwind (`.rounded_md()`, `.rounded_tl_lg()`; o `_full()` passa), o
    /// `ButtonRounded` do kit e o `.radius(..)`/`.corner_radii(..)` com valor solto. Passa o que
    /// vem de `raio::` e o que vem por variável (`.rounded(raio_caixa)`).
    fn fora_dos_raios(texto: &str) -> Vec<String> {
        const SOLTOS: [&str; 5] = ["px(", "r(", "rems(", "tam(", "relative("];
        let mut achados = Vec::new();
        for (i, _) in texto.match_indices('.') {
            let resto = &texto[i + 1..];
            let nome: String = resto.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            let depois = &resto[nome.len()..];
            let lados = ["", "_t", "_b", "_l", "_r", "_tl", "_tr", "_bl", "_br"];
            let com_valor = ["corner_radii", "radius"].contains(&nome.as_str())
                || lados.iter().any(|l| nome == format!("rounded{l}"));
            if com_valor && let Some(dentro) = depois.strip_prefix('(') {
                let dentro = dentro.trim_start();
                if SOLTOS.iter().any(|s| dentro.starts_with(s)) || dentro.starts_with(|c: char| c.is_ascii_digit()) {
                    // O argumento até fechar o parêntese.
                    let mut nivel = 1;
                    let fim = dentro
                        .char_indices()
                        .find(|&(_, c)| {
                            nivel += (c == '(') as i32 - (c == ')') as i32;
                            nivel == 0
                        })
                        .map_or(dentro.len(), |(j, _)| j);
                    achados.push(format!(".{nome}({})", &dentro[..fim]));
                }
            } else if depois.starts_with("()")
                && nome.starts_with("rounded_")
                && !nome.ends_with("_full")
                && !lados.contains(&&nome["rounded".len()..])
            {
                achados.push(format!(".{nome}()"));
            }
        }
        for (i, _) in texto.match_indices("ButtonRounded::") {
            let variante: String = texto[i + 15..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            achados.push(format!("ButtonRounded::{variante}"));
        }
        achados
    }

    /// Só três raios e a pílula em `src/ui` (`raio::CONTROLE`, `CARTAO`, `PECA`, `PILULA` e o
    /// `rounded_full()`), e o tema do kit com os mesmos valores.
    #[test]
    fn raios_na_escala() {
        assert_eq!(
            fora_dos_raios(".rounded(px(6.)).rounded_t(r(4.)).rounded(raio::CARTAO).rounded(8.)"),
            [".rounded(px(6.))", ".rounded_t(r(4.))", ".rounded(8.)"]
        );
        assert_eq!(fora_dos_raios(".rounded_md().rounded_full().rounded_tl_lg().rounded_t_full()"), [".rounded_md()", ".rounded_tl_lg()"]);
        assert_eq!(fora_dos_raios("b.rounded(ButtonRounded::Small)"), ["ButtonRounded::Small"]);
        assert!(fora_dos_raios(".rounded(raio::PECA - px(1.)).rounded(raio_caixa).corner_radii(lado / 2.)").is_empty());
        let raiz = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui"));
        // Quem pode fugir da escala, com o porquê. Fica vazia.
        const EXCECOES: [&str; 0] = [];
        let mut erros = Vec::new();
        for caminho in arquivos_rs(raiz) {
            let nome = caminho.strip_prefix(raiz).unwrap().to_string_lossy().to_string();
            let texto = std::fs::read_to_string(&caminho).unwrap();
            for achado in fora_dos_raios(&texto) {
                if !EXCECOES.iter().any(|e| achado.starts_with(e)) {
                    erros.push(format!("{nome}: {achado}"));
                }
            }
        }
        assert!(erros.is_empty(), "raio fora da escala (raio::CONTROLE, CARTAO, PECA, PILULA):\n{}", erros.join("\n"));
        let json: serde_json::Value = serde_json::from_str(TEMA).unwrap();
        let tema = &json["themes"][0];
        assert_eq!(tema["radius"].as_f64(), Some(f64::from(raio::controle().as_f32())));
        assert_eq!(tema["radius.lg"].as_f64(), Some(f64::from(raio::peca().as_f32())));
    }

    /// Toda animação passa por `ui/movimento.rs` (teto de 60 qps, curva curta, movimento
    /// reduzido); fora dele, nada de animação própria nem laço.
    #[test]
    fn movimento_com_teto() {
        let proibidos = [
            concat!("Animation", "::new("),
            concat!(".repeat", "()"),
            concat!("request_animation", "_frame"),
            concat!("transition", "("),
            concat!("spring", "("),
        ];
        let raiz = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        let mut erros = Vec::new();
        for caminho in arquivos_rs(raiz) {
            if caminho.ends_with("ui/movimento.rs") {
                continue;
            }
            let texto = std::fs::read_to_string(&caminho).unwrap();
            for proibido in proibidos {
                if texto.contains(proibido) {
                    erros.push(format!("{}: {proibido}", caminho.strip_prefix(raiz).unwrap().display()));
                }
            }
        }
        assert!(erros.is_empty(), "animação fora de ui/movimento.rs:\n{}", erros.join("\n"));
    }

    /// A retícula (o xadrez e a hachura do shader) só sai pelos invólucros de
    /// `componentes/graficos.rs` (`trama`, `fio_de_pontos`, `hachura`): o lado inteiro em pixel do
    /// aparelho fica num lugar só, e a queda para o liso, se um renderizador perder o padrão, é
    /// uma edição só.
    #[test]
    fn reticula_so_pelos_graficos() {
        let proibidos = [concat!("checkerboard", "("), concat!("pattern_slash", "(")];
        let raiz = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui"));
        let mut erros = Vec::new();
        for caminho in arquivos_rs(raiz) {
            if caminho.ends_with("componentes/graficos.rs") {
                continue;
            }
            let texto = std::fs::read_to_string(&caminho).unwrap();
            for proibido in proibidos {
                if texto.contains(proibido) {
                    erros.push(format!("{}: {proibido}", caminho.strip_prefix(raiz).unwrap().display()));
                }
            }
        }
        assert!(erros.is_empty(), "retícula fora de componentes/graficos.rs:\n{}", erros.join("\n"));
    }

    /// O metal (a chapa da janela e os recortes dela) só sai por `componentes/metal.rs`: a
    /// textura troca a cada batida, e a ordem "pinta a nova, solta a velha" fica num lugar só.
    #[test]
    fn metal_so_pelo_motor() {
        let proibidos = [concat!("paint_", "image("), concat!("drop_", "image(")];
        let raiz = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ui"));
        let mut erros = Vec::new();
        for caminho in arquivos_rs(raiz) {
            if caminho.ends_with("componentes/metal.rs") {
                continue;
            }
            let texto = std::fs::read_to_string(&caminho).unwrap();
            for proibido in proibidos {
                if texto.contains(proibido) {
                    erros.push(format!("{}: {proibido}", caminho.strip_prefix(raiz).unwrap().display()));
                }
            }
        }
        assert!(erros.is_empty(), "metal fora de componentes/metal.rs:\n{}", erros.join("\n"));
    }

    /// Toda dica passa pela gerenciada do kit (`dica_no_ponteiro`, `dica_com_hover` ou a dos
    /// botões): uma por janela, espera só na primeira e troca na hora com outra aberta. O
    /// `.tooltip(closure)` puro do GPUI espera 500 ms em cada uma (dono, 23/11/2026: "delay only
    /// the first one; once one is open, show the rest instantly").
    #[test]
    fn dica_pela_gerenciada() {
        let proibidos = [concat!(".tooltip", "(move |"), concat!(".tooltip", "(|")];
        let raiz = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        let mut erros = Vec::new();
        for caminho in arquivos_rs(raiz) {
            let texto = std::fs::read_to_string(&caminho).unwrap();
            for proibido in proibidos {
                if texto.contains(proibido) {
                    erros.push(format!("{}: {proibido}", caminho.strip_prefix(raiz).unwrap().display()));
                }
            }
        }
        assert!(erros.is_empty(), "dica sem a graça do kit (use dica_no_ponteiro):\n{}", erros.join("\n"));
    }

    /// A barra de rolagem aparece e some seca: o fade do kit pedia quadros no ritmo do monitor
    /// a cada mudança de tamanho do conteúdo.
    #[test]
    fn barra_de_rolagem_sem_fade() {
        use gpui_kit::component::{scroll::ScrollbarMode, scrollbar_motion};
        use std::time::Duration;
        for modo in [ScrollbarMode::Scrolling, ScrollbarMode::Hover, ScrollbarMode::Always] {
            let m = scrollbar_motion(modo);
            assert_eq!(m.enter(), Duration::ZERO);
            assert_eq!(m.exit(), Duration::ZERO);
            assert_eq!(m.expand(), Duration::ZERO);
            assert_eq!(m.idle(), Duration::from_millis(1200));
        }
    }
}

#[cfg(test)]
mod testes_do_pe_em_curva {
    use super::curva_do_pe;

    #[test]
    fn a_curva_do_pe_vai_do_transparente_ao_cheio_sem_quina() {
        let curva = curva_do_pe(8);
        assert_eq!(curva.len(), 8);
        assert_eq!(curva[0].2, 0.0);
        assert!((curva[7].3 - 1.0).abs() < 1e-6);
        // Trechos emendados, sem buraco nem sobra, e sempre subindo.
        for par in curva.windows(2) {
            assert_eq!(par[0].1, par[1].0);
            assert_eq!(par[0].3, par[1].2);
            assert!(par[1].3 > par[0].3);
        }
        // Começa e termina devagar: o primeiro e o último trecho sobem menos que os do meio.
        let subida = |i: usize| curva[i].3 - curva[i].2;
        assert!(subida(0) < subida(3) / 3.0);
        assert!(subida(7) < subida(4) / 3.0);
    }
}
