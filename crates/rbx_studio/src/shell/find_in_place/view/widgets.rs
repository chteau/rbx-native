//! The panel's small pieces: one result row, the input fields and the
//! toggle and text buttons.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::scan::{File, Hit};
use crate::shell::Shell;
use crate::tokens;

pub(super) fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

/// One match: its line number, then the line with the match marked. With a
/// replacement previewed, the old text is struck out and the new follows it.
pub(super) fn hit_row(
    file: &File,
    at: usize,
    row: usize,
    hit: &Hit,
    selected: bool,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let (script, range) = (file.script, hit.range.clone());
    let marked = div().flex_none().rounded(tokens::radius_tiny());
    let found = match &hit.replacement {
        Some(new) => h_flex()
            .child(
                marked
                    .bg(tokens::diff_remove_soft())
                    .text_color(tokens::text_error())
                    .line_through()
                    .child(hit.text.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .rounded(tokens::radius_tiny())
                    .bg(tokens::diff_add_soft())
                    .text_color(tokens::diff_add())
                    .child(new.clone()),
            ),
        None => h_flex().child(
            marked
                .bg(tokens::accent_soft())
                .text_color(tokens::text_strong())
                .child(hit.text.clone()),
        ),
    };
    h_flex()
        .id(("find-hit", (script.value() as usize) << 16 | at.min(0xffff)))
        .w_full()
        .gap(px(8.))
        .pl(px(26.))
        .pr(px(8.))
        .py(px(2.))
        .items_center()
        .font_family(tokens::font_mono())
        .text_size(tokens::text_xs())
        .when(selected, |this| this.bg(tokens::accent_soft()))
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
        .on_click(cx.listener(move |shell, _, window, cx| {
            shell.find_place.selected = row;
            shell.open_find_hit(script, range.clone(), true, window, cx);
        }))
        .child(
            div()
                .flex_none()
                .w(px(32.))
                .text_color(tokens::text3())
                .child(hit.line.to_string()),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(tokens::text2())
                .child(div().flex_none().child(hit.before.clone()))
                .child(found)
                .child(div().flex_none().child(hit.after.clone())),
        )
}

pub(super) fn field_input(
    state: &Entity<gpui_kit::component::input::InputState>,
) -> impl IntoElement {
    div().flex_1().min_w_0().h_full().child(
        Input::new(state)
            .appearance(false)
            .h_full()
            .px(px(0.))
            .text_size(tokens::text_sm())
            .text_color(tokens::text()),
    )
}

/// A labelled secondary field (Where, Replace) in the search field's style.
pub(super) fn labelled(
    label: &'static str,
    state: &Entity<gpui_kit::component::input::InputState>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    h_flex()
        .h(px(28.))
        .px(px(8.))
        .gap(px(6.))
        .items_center()
        .rounded(tokens::radius())
        .bg(tokens::field_select())
        .border_1()
        .border_color(tokens::border())
        .child(
            div()
                .flex_none()
                .w(px(48.))
                .text_size(tokens::text_xs())
                .text_color(tokens::text3())
                .child(label),
        )
        .child(field_input(state))
        .child(clear_button(
            SharedString::from(format!("find-clear-{label}")),
            state,
            cx,
        ))
}

pub(super) fn clear_button(
    id: impl Into<SharedString>,
    state: &Entity<gpui_kit::component::input::InputState>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let state = state.clone();
    div()
        .id(id.into())
        .flex_none()
        .size(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens::radius_tiny())
        .cursor_pointer()
        .text_color(tokens::text3())
        .hover(|this| {
            tokens::hover_fx(this)
                .bg(tokens::hover())
                .text_color(tokens::text())
        })
        .on_click(cx.listener(move |_, _, window, cx| {
            state.update(cx, |state, cx| state.set_value("", window, cx));
        }))
        .child(Icon::new(IconName::X).xsmall())
}

/// One of the search field's `Aa` / `Ab` / `.*` toggles.
pub(super) fn toggle(
    id: &'static str,
    label: &'static str,
    tip: &'static str,
    underline: bool,
    on: bool,
    cx: &mut Context<Shell>,
    flip: fn(&mut crate::script_editor::find::Options),
) -> impl IntoElement {
    div()
        .id(id)
        .flex_none()
        .h(px(20.))
        .min_w(px(22.))
        .px(px(3.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens::radius_tiny())
        .cursor_pointer()
        .font_family(tokens::font_mono())
        .text_size(tokens::text_xs())
        .when(underline, |this| this.underline())
        .text_color(if on {
            tokens::check_on()
        } else {
            tokens::text3()
        })
        .when(on, |this| this.bg(tokens::accent_soft()))
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
        .tooltip(move |window, cx| crate::shell::tooltip::text(tip, window, cx))
        .on_click(cx.listener(move |shell, _, _, cx| {
            flip(&mut shell.find_place.options);
            shell.find_place.selected = 0;
            cx.notify();
        }))
        .child(label)
}

pub(super) fn icon_button(
    id: &'static str,
    icon: IconName,
    tip: &'static str,
    cx: &mut Context<Shell>,
    act: fn(&mut Shell, &mut Window, &mut Context<Shell>),
) -> impl IntoElement {
    div()
        .id(id)
        .flex_none()
        .size(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens::radius_tiny())
        .cursor_pointer()
        .text_color(tokens::text2())
        .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
        .tooltip(move |window, cx| crate::shell::tooltip::text(tip, window, cx))
        .on_click(cx.listener(move |shell, _, window, cx| act(shell, window, cx)))
        .child(Icon::new(icon).xsmall())
}

pub(super) fn text_button(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    cx: &mut Context<Shell>,
    act: fn(&mut Shell, &mut Window, &mut Context<Shell>),
) -> impl IntoElement {
    div()
        .id(id)
        .flex_none()
        .px(px(8.))
        .py(px(2.))
        .rounded(tokens::radius_tiny())
        .border_1()
        .border_color(tokens::border())
        .text_size(tokens::text_xs())
        .when(enabled, |this| {
            this.cursor_pointer()
                .text_color(tokens::text())
                .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                .on_click(cx.listener(move |shell, _, window, cx| act(shell, window, cx)))
        })
        .when(!enabled, |this| this.text_color(tokens::text_disabled()))
        .child(label)
}
