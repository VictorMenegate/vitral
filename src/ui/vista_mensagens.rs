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

/// O filho desenhado com outro rem: é como a letra da conversa cresce sem o resto da interface
/// (`Config.letra_da_conversa`). As mensagens medem tudo em rem (`tema::r`), então a letra, o
/// respiro e os ícones crescem juntos, como um zoom só delas. O rem trocado vale nas três fases
/// (o tamanho do texto e as medidas saem no layout; os raios e as sombras, na pintura). O que as
/// mensagens abrem por cima (menu, dica) é pintado depois, fora daqui: mede com a letra da
/// conversa e pinta os cantos com a da interface, diferença que não se nota.
pub struct NaLetraDaConversa {
    fator: f32,
    filho: AnyElement,
}

/// `filho` na letra da conversa de agora (`tema::fator_da_conversa`). Em 100%, é o próprio filho.
pub fn na_letra_da_conversa(filho: impl IntoElement) -> AnyElement {
    let fator = crate::tema::fator_da_conversa();
    if (fator - 1.).abs() < 0.001 {
        return filho.into_any_element();
    }
    NaLetraDaConversa { fator, filho: filho.into_any_element() }.into_any_element()
}

/// `filho` com a letra num `fator` (1 é o do resto da interface): a caixa de mensagem, com
/// "Letra da conversa também na caixa". Em 1, é o próprio filho.
pub fn na_letra(fator: f32, filho: impl IntoElement) -> AnyElement {
    if (fator - 1.).abs() < 0.001 {
        return filho.into_any_element();
    }
    NaLetraDaConversa { fator, filho: filho.into_any_element() }.into_any_element()
}

impl NaLetraDaConversa {
    /// O rem de dentro: o da janela (que já leva o zoom) vezes o fator. Lido fora do bloco de
    /// dentro, é sempre o da janela, e as três fases chegam ao mesmo número.
    fn rem(&self, window: &Window) -> Pixels {
        window.rem_size() * self.fator
    }
}

/// A conversa recortada embaixo da faixa da mensagem presa: o que rola por ali não é desenhado (nem
/// recebe o ponteiro), então a faixa não precisa de fundo e fica transparente como o resto do
/// painel. Antes ela repetia as camadas do cartão por cima delas mesmas para tapar o texto, e com a
/// transparência ligada ficava mais escura que o painel (dono, 23 e 24/11/2026: "o fundo dessa
/// mensagem que fica em cima não tá transparente"). `alto` diz, na hora de desenhar, quanto do
/// topo some: a altura da faixa menos o quanto a mensagem seguinte já a empurrou para cima.
pub fn recortada_no_alto(filho: impl IntoElement, alto: impl Fn() -> Pixels + 'static) -> AnyElement {
    RecorteNoAlto { alto: Box::new(alto), area: None, filho: filho.into_any_element() }.into_any_element()
}

/// `filho` recortado na área das mensagens do painel (`Painel::area_das_mensagens`), lida na hora
/// de desenhar: a barra de ações de uma mensagem é pintada por cima de tudo (`deferred`), e sem o
/// recorte a da última resposta passava por cima da caixa de mensagem (dono, 26/11/2026: "esse
/// negócio de baixo da mensagem tá atravessando as coisas"). Sem a área medida, não recorta.
pub fn recortada_na_area(filho: impl IntoElement, area: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>) -> AnyElement {
    RecorteNoAlto {
        alto: Box::new(|| px(0.)),
        area: Some(area),
        filho: filho.into_any_element(),
    }
    .into_any_element()
}

/// Quanto do topo da conversa fica de fora: a parte da faixa presa que ainda está à vista.
pub fn alto_do_recorte(altura_da_faixa: Pixels, empurrao: Pixels) -> Pixels {
    (altura_da_faixa - empurrao).max(px(0.))
}

struct RecorteNoAlto {
    alto: Box<dyn Fn() -> Pixels>,
    /// Recorta nesta área, e não nos limites do próprio filho (`recortada_na_area`).
    area: Option<std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>>,
    filho: AnyElement,
}

impl RecorteNoAlto {
    fn mascara(&self, limites: Bounds<Pixels>) -> ContentMask<Pixels> {
        if let Some(area) = &self.area {
            let area = area.get();
            // Sem a área medida ainda, a janela inteira.
            let bounds = if area.size.width > px(0.) && area.size.height > px(0.) {
                area
            } else {
                Bounds { origin: point(px(-1e6), px(-1e6)), size: size(px(2e6), px(2e6)) }
            };
            return ContentMask { bounds };
        }
        let alto = (self.alto)().min(limites.size.height);
        ContentMask {
            bounds: Bounds {
                origin: point(limites.origin.x, limites.origin.y + alto),
                size: size(limites.size.width, limites.size.height - alto),
            },
        }
    }
}

impl IntoElement for RecorteNoAlto {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for RecorteNoAlto {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.filho.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        limites: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let mascara = self.mascara(limites);
        window.with_content_mask(Some(mascara), |window| self.filho.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        limites: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let mascara = self.mascara(limites);
        window.with_content_mask(Some(mascara), |window| self.filho.paint(window, cx));
    }
}

impl IntoElement for NaLetraDaConversa {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for NaLetraDaConversa {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let rem = self.rem(window);
        (window.with_rem_size(Some(rem), |window| self.filho.request_layout(window, cx)), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let rem = self.rem(window);
        window.with_rem_size(Some(rem), |window| self.filho.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let rem = self.rem(window);
        window.with_rem_size(Some(rem), |window| self.filho.paint(window, cx));
    }
}

#[cfg(test)]
mod testes_do_recorte {
    use super::alto_do_recorte;
    use gpui_kit::px;

    #[test]
    fn o_recorte_e_a_parte_da_faixa_que_ainda_esta_a_vista() {
        assert_eq!(alto_do_recorte(px(64.), px(0.)), px(64.));
        // Empurrada 20 para cima pela mensagem seguinte, só 44 dela cobrem a conversa.
        assert_eq!(alto_do_recorte(px(64.), px(20.)), px(44.));
        // Empurrada para fora, nada fica recortado.
        assert_eq!(alto_do_recorte(px(64.), px(80.)), px(0.));
    }
}
