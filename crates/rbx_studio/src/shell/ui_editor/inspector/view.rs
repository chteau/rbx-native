//! The inspector's widgets, in the dock's own tokens and laid out the way
//! Figma's Design panel is: a titled section, a compact field whose label
//! drags its value, an icon toggle, a swatch, and the three-by-three grid
//! an anchor or an alignment is picked on.

use gpui_kit::assets::IconName;
use gpui_kit::component::color_picker::ColorPicker;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex, Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::super::chrome;
use super::super::super::rows::field_box;
use super::{Key, Shell};
use crate::tokens;

/// What stands at a field's left edge, and is its drag handle.
#[derive(Clone, Copy)]
pub(super) enum Label {
    Text(&'static str),
    Icon(IconName),
}

/// A section: its title and the buttons beside it, over its rows.
pub(super) fn section(title: &'static str, actions: Vec<AnyElement>, rows: Vec<AnyElement>) -> Div {
    v_flex()
        .w_full()
        .gap(px(6.))
        .px(px(8.))
        .pt(px(8.))
        .pb(px(10.))
        .border_b_1()
        .border_color(tokens::border())
        .child(
            h_flex()
                .w_full()
                .h(px(24.))
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(tokens::text_sm())
                        .font_weight(tokens::WEIGHT_BOLD)
                        .text_color(tokens::text_full())
                        .child(title),
                )
                .child(h_flex().gap(px(2.)).children(actions)),
        )
        .children(rows)
}

/// Fields side by side, sharing the width.
pub(super) fn line(children: Vec<AnyElement>) -> AnyElement {
    h_flex()
        .w_full()
        .gap(px(6.))
        .items_center()
        .children(children)
        .into_any_element()
}

/// An icon button that reads as pressed while `on`.
pub(super) fn toggle(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    on: bool,
) -> Stateful<Div> {
    chrome::icon_button(id, icon, label).when(on, |this| {
        this.bg(tokens::accent_soft())
            .text_color(tokens::check_on())
    })
}

/// A checkbox's words, beside it.
pub(super) fn words(text: &'static str) -> AnyElement {
    div()
        .flex_1()
        .text_size(tokens::text_sm())
        .text_color(tokens::text_label())
        .child(text)
        .into_any_element()
}

/// A caption in a row, muted — what a group of controls is.
pub(super) fn caption(text: &'static str) -> AnyElement {
    div()
        .flex_none()
        .w(px(64.))
        .text_size(tokens::text_sm())
        .text_color(tokens::text_muted())
        .child(text)
        .into_any_element()
}

/// Three by three dots, the one at `picked` lit: an anchor, an alignment.
pub(super) fn grid(
    id: &'static str,
    picked: Option<[usize; 2]>,
    on_pick: impl Fn([usize; 2], &mut Window, &mut App) + 'static,
) -> AnyElement {
    let on_pick = std::rc::Rc::new(on_pick);
    let cells = (0..9).map(|index| {
        let cell = [index % 3, index / 3];
        let lit = picked == Some(cell);
        let on_pick = on_pick.clone();
        div()
            .id((id, index))
            .size(px(18.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(tokens::radius_tiny())
            .cursor_pointer()
            .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
            .on_click(move |_, window, cx| on_pick(cell, window, cx))
            .child(
                div()
                    .size(px(if lit { 6. } else { 3. }))
                    .rounded_full()
                    .bg(if lit {
                        tokens::check_on()
                    } else {
                        tokens::text_muted()
                    }),
            )
            .into_any_element()
    });
    div()
        .flex_none()
        .p(px(2.))
        .rounded(tokens::radius())
        .bg(tokens::chrome())
        .child(div().grid().grid_cols(3).children(cells))
        .into_any_element()
}

impl Shell {
    /// `key`'s field: its label, which a sideways drag scrubs, the value,
    /// and `suffix` — a unit — after it.
    pub(super) fn field(
        &mut self,
        key: Key,
        label: Label,
        suffix: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input = self.key_input(key, window, cx);
        let tab_index = self.tab_order.next();
        let handle = cx.entity();
        let label = match label {
            Label::Text(text) => div().child(text).into_any_element(),
            Label::Icon(icon) => Icon::new(icon).size(tokens::text_sm()).into_any_element(),
        };
        field_box()
            .flex_1()
            .min_w_0()
            .gap(px(4.))
            .child(
                div()
                    .id(("ui-field", key as usize))
                    .flex_none()
                    .w(px(14.))
                    .flex()
                    .justify_center()
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text_muted())
                    .cursor_col_resize()
                    .hover(|this| tokens::hover_fx(this).text_color(tokens::text_full()))
                    .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                        let x = event.position.x;
                        handle.update(cx, |shell, _| shell.begin_key_drag(key, x));
                    })
                    .child(label),
            )
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&input)
                        .appearance(false)
                        .with_size(tokens::field_size())
                        .h_full()
                        .tab_index(tab_index),
                ),
            )
            .when_some(suffix, |this, suffix| {
                this.child(
                    div()
                        .flex_none()
                        .text_size(tokens::text_sm())
                        .text_color(tokens::text_muted())
                        .child(suffix),
                )
            })
            .into_any_element()
    }

    /// `key`'s colour: a swatch that opens the palette, its hex, and the
    /// opacity `alpha` reads as.
    pub(super) fn paint(
        &mut self,
        key: Key,
        alpha: Key,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let state = self.key_color(key, window, cx);
        vec![
            div()
                .flex_none()
                .child(ColorPicker::new(&state).with_size(tokens::field_size()))
                .into_any_element(),
            div()
                .flex_1()
                .min_w_0()
                .child(self.field(key, Label::Text("#"), None, window, cx))
                .into_any_element(),
            div()
                .w(px(92.))
                .flex_none()
                .child(self.field(alpha, Label::Icon(IconName::Droplet), Some("%"), window, cx))
                .into_any_element(),
        ]
    }
}
