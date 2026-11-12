use crate::{ActiveTheme, Disableable, StyledExt, h_flex};
use gpui::{
    AnyElement, App, BoxShadow, ClickEvent, Corners, ElementId, InteractiveElement, IntoElement,
    MouseButton, ParentElement, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    StyleRefinement, Styled, Window, canvas, div, fill, hsla, prelude::FluentBuilder as _, px,
};
use gpui_base::TestSupportExt as _;
use smallvec::SmallVec;

#[derive(IntoElement)]
pub(crate) struct MenuItemElement {
    id: ElementId,
    aria_label: Option<SharedString>,
    style: StyleRefinement,
    disabled: bool,
    selected: bool,
    /// Vitral: item perigoso (vermelho, realce em vermelho a 14%).
    danger: bool,
    /// Vitral: uma camada debaixo do item inteiro (a ficha de metal do escolhido).
    fundo: Option<AnyElement>,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    on_hover: Option<Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
    children: SmallVec<[AnyElement; 2]>,
}

impl MenuItemElement {
    /// Create a new MenuItem with the given ID.
    pub(crate) fn new(id: impl Into<ElementId>) -> Self {
        let id: ElementId = id.into();
        Self {
            id: id.clone(),
            aria_label: None,
            style: StyleRefinement::default(),
            disabled: false,
            selected: false,
            danger: false,
            fundo: None,
            on_click: None,
            on_hover: None,
            children: SmallVec::new(),
        }
    }

    /// Set ListItem as the selected item style.
    pub(crate) fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Vitral: pinta o item como perigoso.
    pub(crate) fn danger(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    /// Vitral: a camada que enche o item por baixo do conteúdo; o realce vai por cima dela.
    pub(crate) fn fundo(mut self, fundo: AnyElement) -> Self {
        self.fundo = Some(fundo);
        self
    }

    /// Set the accessible label for the menu item.
    pub(crate) fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    /// Set the disabled state of the MenuItem.
    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set a handler for when the MenuItem is clicked.
    pub(crate) fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Set a handler for when the mouse enters the MenuItem.
    #[allow(unused)]
    pub fn on_hover(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Box::new(handler));
        self
    }
}

impl Disableable for MenuItemElement {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Styled for MenuItemElement {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for MenuItemElement {
    fn extend(&mut self, elements: impl IntoIterator<Item = gpui::AnyElement>) {
        self.children.extend(elements);
    }
}

/// Vitral: o realce do item sob o ponteiro (ou o teclado): a tecla em relevo, com o canto no
/// padrão dos controles do app, 45% da altura de verdade do item (o `raio::de` do Vitral, sem
/// crescer com o zoom: 14 num item de 32, 22 num de 48). Medido no quadro, por isso um `canvas`.
fn realce_de_tecla(cor: gpui::Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |limites, _, window, _| {
            let base = window.rem_size().as_f32() / 15.;
            let raio = px((limites.size.height.as_f32() / base * 0.45).round());
            let cantos = Corners::all(raio).clamp_radii_for_quad_size(limites.size);
            window.paint_quad(fill(limites, cor).corner_radii(cantos));
            window.paint_inset_shadows(
                limites,
                cantos,
                &[
                    BoxShadow::new(px(0.), px(1.), hsla(0., 0., 1., 0.16)).inset(),
                    BoxShadow::new(px(0.), px(-1.), hsla(0., 0., 0., 0.30)).inset(),
                ],
            );
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

impl RenderOnce for MenuItemElement {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        // Vitral: o perigoso fica vermelho também no realce, sobre vermelho a 14%.
        let (realce, texto_realce) = if self.danger {
            (cx.theme().danger.opacity(0.14), cx.theme().danger)
        } else {
            (cx.theme().tokens.accent.into(), cx.theme().accent_foreground)
        };
        let aceso = self.selected && !self.disabled;
        h_flex()
            .id(self.id)
            .test_support()
            .role(Role::MenuItem)
            .when_some(self.aria_label, |this, label| this.aria_label(label))
            .aria_selected(self.selected)
            .gap_x_1()
            .py_1()
            .px_2()
            .text_base()
            .text_color(cx.theme().foreground)
            .when(self.danger, |this| this.text_color(cx.theme().danger))
            .relative()
            .items_center()
            .justify_between()
            .refine_style(&self.style)
            .when_some(self.on_hover, |this, on_hover| {
                this.on_hover(move |hovered, window, cx| (on_hover)(hovered, window, cx))
            })
            .when(!self.disabled, |this| {
                // Not `group_hover` on the item's own group: GPUI styles an
                // element before registering its group, and that fallback keeps
                // the pointer's item lit after a key moves the highlight away.
                // Vitral: o fundo do realce é a tecla do `realce_de_tecla`, um filho; aqui só a cor
                // do texto.
                this.hover(|this| this.text_color(texto_realce))
                    .when(self.selected, |this| this.text_color(texto_realce))
                .when_some(self.on_click, |this, on_click| {
                    this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(on_click)
                })
            })
            // Vitral: o desabilitado esmaece pela opacidade, no popup_menu.rs. Por baixo do
            // conteúdo, a ficha (se houver) e, por cima dela, o realce.
            .children(self.fundo.map(|fundo| div().absolute().top_0().left_0().size_full().child(fundo)))
            .when(aceso, |this| this.child(realce_de_tecla(realce)))
            .children(self.children)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn aria_label_sets_accessible_name(_cx: &mut gpui::TestAppContext) {
        let item = MenuItemElement::new("open").aria_label("Open");

        assert_eq!(item.aria_label, Some("Open".into()));
    }
}
