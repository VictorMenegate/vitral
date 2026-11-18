//! Uma vista própria, desenhada em cache pelo GPUI, para o que anima sozinho (o anel girando, o
//! metal da janela). O compasso dos spinners avisa a vista em que o desenho está; sem isto, essa
//! vista era a Janela inteira, e cada batida (12 por segundo com um chat trabalhando) avisava a
//! Janela, que avisava as mensagens de todos os painéis (ver `ui::vista_mensagens`). Aqui a
//! batida avisa só esta vista pequena: a Janela redesenha como ancestral, mas as mensagens, que
//! não foram avisadas, saem do cache.
//!
//! As vistas ficam num mapa por chave (não no `use_keyed_state`, cujo estado avisa a vista de
//! fora a cada mudança). A que passa um tempo sem ser desenhada sai do mapa.

use std::{collections::HashMap, rc::Rc};

use gpui_kit::*;

type Desenho = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

pub struct VistaViva {
    desenho: Option<Desenho>,
    versao: u64,
}

impl Render for VistaViva {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.desenho.clone() {
            Some(desenho) => desenho(window, cx),
            None => div().into_any_element(),
        }
    }
}

#[derive(Default)]
struct VistasVivas {
    vistas: HashMap<String, (Entity<VistaViva>, u64)>,
    quadro: u64,
}

impl Global for VistasVivas {}

/// Conta um quadro da janela (a Janela chama no `render`): o que não foi desenhado nos últimos
/// 600 quadros sai do mapa.
pub fn novo_quadro(cx: &mut App) {
    let vivas = cx.default_global::<VistasVivas>();
    vivas.quadro += 1;
    if vivas.quadro % 600 == 0 {
        let corte = vivas.quadro.saturating_sub(600);
        vivas.vistas.retain(|_, (_, usado)| *usado >= corte);
    }
}

/// Desenha `desenho` numa vista própria de `chave`, laid out em `estilo`. A vista só se redesenha
/// quando ela mesma pede (a batida do compasso) ou quando `versao` muda (o que ela mostra mudou).
pub fn em_vista_propria(
    chave: String,
    estilo: StyleRefinement,
    versao: u64,
    cx: &mut App,
    desenho: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
) -> AnyElement {
    let quadro = cx.default_global::<VistasVivas>().quadro;
    let existente = cx.default_global::<VistasVivas>().vistas.get(&chave).map(|(v, _)| v.clone());
    let vista = match existente {
        Some(vista) => vista,
        None => cx.new(|_| VistaViva { desenho: None, versao: u64::MAX }),
    };
    cx.default_global::<VistasVivas>().vistas.insert(chave, (vista.clone(), quadro));
    vista.update(cx, |v, cx| {
        v.desenho = Some(Rc::new(desenho));
        if v.versao != versao {
            v.versao = versao;
            cx.notify();
        }
    });
    AnyView::from(vista).cached(estilo).into_any_element()
}
