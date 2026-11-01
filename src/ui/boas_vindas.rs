//! Tela de boas-vindas, antes de escolher a pasta de trabalho: o logo, o botão de escolher e as
//! pastas recentes.

use std::{path::PathBuf, sync::Arc};

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{CabecalhoDeSecao, CartaoDeLista, Simbolo, TamanhoBotao, TelaCheia, botao_primario},
    dialogos::com_til,
};
use crate::{
    modelo,
    tema::{Lucide, esp, r},
};

/// O logo completo em PNG de 128 px, mostrado em 64: do SVG o GPUI rasterizaria 256 px e
/// reduziria sem mipmaps, serrilhando o chumbo (o trilho usa o logo pequeno).
const LOGO: &[u8] = include_bytes!("../../assets/vitral-128.png");

/// Quantas pastas recentes aparecem.
const RECENTES: usize = 5;

impl Janela {
    pub(super) fn boas_vindas(&mut self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let recentes: Vec<PathBuf> = modelo::recentes().into_iter().filter(|p| p.is_dir()).take(RECENTES).collect();
        div()
            .flex()
            .flex_col()
            .size_full()
            .p(esp::E8)
            .child(
                div()
                    .flex()
                    .w_full()
                    .child(self.arrastavel("arrastar-inicio").flex_1().h(px(36.)))
                    .child(self.botoes_janela(cx)),
            )
            .child(BoasVindas { recentes, escolhendo: self.escolhendo_pasta, janela: cx.entity().downgrade() })
    }
}

/// O cartão do meio; desenhado com a janela (os botões crescem com o zoom).
#[derive(IntoElement)]
struct BoasVindas {
    recentes: Vec<PathBuf>,
    /// O seletor de pasta do sistema está aberto.
    escolhendo: bool,
    janela: WeakEntity<Janela>,
}

impl RenderOnce for BoasVindas {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let logo = Arc::new(Image::from_bytes(ImageFormat::Png, LOGO.to_vec()));
        let escolher = self.janela.clone();
        let botao = botao_primario("escolher-pasta", Some(Lucide::FolderOpen.into()), "Escolher pasta…", TamanhoBotao::Controle, window, cx)
            .loading(self.escolhendo)
            .on_click(move |_, window, cx| {
                let _ = escolher.update(cx, |this, cx| this.escolher_pasta(window, cx));
            });
        let recentes = (!self.recentes.is_empty()).then(|| {
            let cartoes = self.recentes.into_iter().enumerate().map(|(i, caminho)| {
                let nome = caminho
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| caminho.display().to_string());
                let completo: SharedString = com_til(&caminho).into();
                let dica = completo.clone();
                let janela = self.janela.clone();
                CartaoDeLista::new(("recente", i), nome)
                    .uma_linha()
                    .simbolo(Simbolo::icone(Lucide::Folder))
                    .meta(
                        div()
                            .id(("recente-caminho", i))
                            .max_w(r(240.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_middle()
                            .child(completo)
                            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx)),
                    )
                    .on_click(move |_, window, cx| {
                        let caminho = caminho.clone();
                        let _ = janela.update(cx, |this, cx| this.abrir_raiz(caminho, window, cx));
                    })
            });
            div()
                .flex()
                .flex_col()
                .gap(r(2.))
                .child(CabecalhoDeSecao::new("Recentes").primeira(true))
                .children(cartoes)
        });
        TelaCheia::new("Boas-vindas ao Vitral")
            .simbolo(img(logo).flex_none().size(r(64.)))
            .texto(
                "Escolha a pasta de trabalho. Cada subpasta vira um projeto; as conversas ficam em .vitral \
                 dentro dela.",
            )
            .child(div().flex().child(botao))
            .children(recentes)
    }
}
