//! The side panels' building blocks: section headings, labelled fields,
//! buttons and drop-down pickers, in the editor's tokens.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::Shell;
use crate::shell::chrome;
use crate::shell::menu::{self, MenuId};
use crate::tokens;

pub(super) fn title(text: &'static str) -> impl IntoElement {
    div()
        .text_size(tokens::text_sm())
        .text_color(tokens::text_strong())
        .child(text)
}

pub(super) fn section(name: &'static str) -> Div {
    v_flex().gap(px(6.)).child(
        div()
            .text_size(tokens::text_xs())
            .text_color(tokens::text_muted())
            .child(name),
    )
}

pub(super) fn label(text: String) -> impl IntoElement {
    div()
        .text_size(tokens::text_sm())
        .text_color(tokens::text_label())
        .child(text)
}

pub(super) fn field_row(name: &'static str, input: &Entity<InputState>) -> impl IntoElement {
    h_flex()
        .gap(tokens::label_gap())
        .items_center()
        .child(
            div()
                .w(px(44.))
                .flex_none()
                .text_size(tokens::text_sm())
                .text_color(tokens::text_label())
                .child(name),
        )
        .child(field(input).flex_1())
}

/// [`field_row`] for a label known only at run time (a node property).
pub(super) fn field_row_owned(name: &str, input: &Entity<InputState>) -> impl IntoElement {
    h_flex()
        .gap(tokens::label_gap())
        .items_center()
        .child(
            div()
                .w(px(72.))
                .flex_none()
                .truncate()
                .text_size(tokens::text_sm())
                .text_color(tokens::text_label())
                .child(name.to_owned()),
        )
        .child(field(input).flex_1())
}

pub(super) fn field(input: &Entity<InputState>) -> Div {
    div()
        .h(tokens::input_height())
        .px(tokens::input_padding())
        .bg(tokens::field_select())
        .rounded(px(4.))
        .flex()
        .items_center()
        .child(
            Input::new(input)
                .appearance(false)
                .with_size(tokens::field_size())
                .h_full(),
        )
}

pub(super) fn xyz_row(name: &'static str, inputs: &[Entity<InputState>; 3]) -> impl IntoElement {
    v_flex()
        .gap(px(3.))
        .child(
            div()
                .text_size(tokens::text_xs())
                .text_color(tokens::text_muted())
                .child(name),
        )
        .child(
            h_flex()
                .gap(px(4.))
                .children(inputs.iter().zip(["X", "Y", "Z"]).map(|(input, axis)| {
                    h_flex()
                        .flex_1()
                        .gap(px(3.))
                        .items_center()
                        .child(
                            div()
                                .text_size(tokens::text_xs())
                                .text_color(tokens::text_muted())
                                .child(axis),
                        )
                        .child(field(input).flex_1())
                })),
        )
}

pub(super) fn wide_button(id: &'static str, text: &'static str, enabled: bool) -> Stateful<Div> {
    chrome::button(id, text, false)
        .flex_1()
        .debug_selector(move || id.into())
        .when(!enabled, |this| this.opacity(0.4).cursor_not_allowed())
}

/// [`wide_button`] for a button repeated in a list, keyed per row.
pub(super) fn keyed_button(
    id: ElementId,
    text: impl Into<SharedString>,
    enabled: bool,
) -> Stateful<Div> {
    chrome::button(id, text.into(), false)
        .flex_1()
        .when(!enabled, |this| this.opacity(0.4).cursor_not_allowed())
}

/// The panel's primary action: accented, but the size of its neighbours, so
/// a row of buttons reads as one row.
pub(super) fn primary_button(id: &'static str, text: &'static str) -> Stateful<Div> {
    chrome::button(id, text, true)
        .flex_1()
        .debug_selector(move || id.into())
}

/// The Publish button of the Auto-rig and Rigging sections: dimmed and
/// reading "Publishing\u{2026}" while an upload runs, so a second click
/// cannot start another (`publish_autorig` ignores it as well).
pub(super) fn publish_button(id: &'static str, busy: bool) -> Stateful<Div> {
    primary_button(
        id,
        if busy {
            "Publishing\u{2026}"
        } else {
            "Publish"
        },
    )
    .when(busy, |this| this.opacity(0.5).cursor_not_allowed())
}

pub(super) fn picker(
    shell: &Shell,
    menu_id: MenuId,
    id: &'static str,
    text: &str,
    items: Vec<menu::Item>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    menu::dropdown(
        shell,
        menu_id,
        chrome::Trigger::new(
            chrome::button(id, text.to_owned(), false)
                .gap(px(4.))
                .child(component::Icon::new(IconName::ChevronDown).size(px(10.))),
        ),
        items,
        cx,
    )
}
