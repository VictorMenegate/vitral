//! A área de mensagens de um painel como vista própria, desenhada em cache pelo GPUI: um quadro da
//! janela que não mexeu nela (o spinner da caixa, o metal, a lista, outro painel) reaproveita o
//! layout e a pintura dela inteira, em vez de refazer as 60 mensagens. Medido em 17/11/2026: com
//! um chat trabalhando, cada quadro da janela custava ~40 ms de processador, quase tudo no layout
//! e na pintura do GPUI; montar a árvore do Vitral era só ~3 ms.
//!
//! Quem desenha continua sendo a Janela (`Janela::corpo_do_painel`), pelo `update`, que não conta
//! como dependência para o GPUI: a vista só se redesenha quando ela mesma é avisada. Avisam: o
//! motor (as mensagens leem o motor, e a leitura conta), a rolagem e as animações de dentro dela
//! (que pedem o quadro à vista em que estão) e a Janela, que avisa todas as vistas dos painéis
//! sempre que ela mesma é avisada (`Janela::avisar_vistas`), porque o estado que as mensagens
//! usam (abertos, busca, rolagem, alvo) mora nela.

use gpui_kit::*;

use super::Janela;

pub struct VistaDasMensagens {
    janela: WeakEntity<Janela>,
    painel: u64,
}

impl VistaDasMensagens {
    pub fn new(janela: WeakEntity<Janela>, painel: u64) -> Self {
        Self { janela, painel }
    }
}

impl Render for VistaDasMensagens {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let painel = self.painel;
        // A vista em cache é desenhada como raiz, no tamanho do lugar dela: sem o `size_full`
        // aqui, a raiz crescia até o tamanho do conteúdo e a área deixava de rolar.
        let corpo = self
            .janela
            .update(cx, |janela, cx| janela.corpo_do_painel(painel, window, cx))
            .unwrap_or_else(|_| div().into_any_element());
        div().size_full().flex().flex_col().child(corpo)
    }
}

/// A lista da esquerda como vista própria, em cache como as mensagens: uma batida do spinner ou
/// a rolagem de um painel não refazem o layout dela. Quem desenha é a Janela (`Janela::lateral`).
pub struct VistaDaLista {
    janela: WeakEntity<Janela>,
}

impl VistaDaLista {
    pub fn new(janela: WeakEntity<Janela>) -> Self {
        Self { janela }
    }
}

impl Render for VistaDaLista {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lista = self
            .janela
            .update(cx, |janela, cx| match janela.motor.clone() {
                Some(motor) => janela.lateral(&motor, window, cx).into_any_element(),
                None => div().into_any_element(),
            })
            .unwrap_or_else(|_| div().into_any_element());
        div().size_full().flex().flex_col().child(lista)
    }
}

/// A casca da janela (tudo o que a Janela desenha fora das vistas em cache) como vista própria,
/// sem cache: é ela, e não a Janela, a vista em que os elementos da casca estão. O GPUI avisa
/// essa vista a cada hover, dica, quadro de transição e estado guardado que muda; quando ela era
/// a Janela, cada um desses avisos passava pelo `Janela::avisar_vistas` e refazia as mensagens de
/// todos os painéis e a lista (medido em 23/11/2026: de 13 a 25 ms por painel, a cada tecla na
/// caixa e a cada passada do ponteiro por um botão). O que o código do Vitral muda continua
/// avisando a Janela (`cx.notify()`), e aí sim as vistas se refazem.
pub struct VistaDaCasca {
    janela: WeakEntity<Janela>,
}

impl VistaDaCasca {
    pub fn new(janela: WeakEntity<Janela>) -> Self {
        Self { janela }
    }
}

impl Render for VistaDaCasca {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.janela.update(cx, |janela, cx| janela.montar_casca(window, cx)).unwrap_or_else(|_| div().into_any_element())
    }
}
