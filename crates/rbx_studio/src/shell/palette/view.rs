//! The palette's overlay.
//!
//! The APG's combobox keeps DOM focus in the field and points
//! `aria-activedescendant` at the highlighted option. GPUI reports an
//! active descendant only from inside the *focused* node, and the field's
//! own focusable element has no accessibility node — so the combobox
//! wrapper tracks the field's own focus handle: the wrapper becomes the
//! focused node (it has the id and role the field lacks), the list sits
//! inside it, and the highlighted option's claim is honoured. Key dispatch
//! is untouched: GPUI routes keys to the innermost element tracking a
//! handle, which is still the field.

use gpui_kit::component::input::{Enter, Escape, Input, MoveDown, MoveEnd, MoveHome, MoveUp};
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::super::roving::Move;
use super::super::Shell;
use super::commands::{self, Command};

/// One option row: its text, its shortcut or path, and everything a
/// screen reader is told about it. `pub(super)` so `tests` can read the
/// accessibility node it writes.
pub(super) fn option(row: usize, count: usize, command: &Command, selected: bool) -> Stateful<Div> {
    let hint = command.hint.map(commands::display);
    let aside = hint
        .clone()
        .map(SharedString::from)
        .or(command.detail.clone());
    // The muted grey is too faint on the highlight's accent.
    let aside_color = match selected {
        true => tokens::text_strong(),
        false => tokens::text2(),
    };
    super::super::menu::row_chrome(("palette-row", row), None, command.text(), true, false)
        .role(Role::ListBoxOption)
        .aria_label(command.text())
        .when_some(command.detail.clone(), |row, path| {
            row.aria_description(path)
        })
        .when_some(hint, |row, hint| row.aria_keyshortcuts(hint))
        .aria_selected(selected)
        .aria_position_in_set(row + 1)
        .aria_size_of_set(count)
        .when(selected, |row| {
            row.bg(tokens::selection()).aria_active_descendant()
        })
        .when_some(aside, |row, aside| {
            row.child(
                div()
                    .flex_none()
                    .max_w(px(280.))
                    .truncate()
                    .text_color(aside_color)
                    .child(aside),
            )
        })
}

impl Shell {
    pub(in crate::shell) fn command_palette(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.palette.open.as_ref()?;
        let list = open.list();
        let count = open.rows.len();
        let highlighted = open.highlighted.min(count.saturating_sub(1));
        let options = open.rows.iter().enumerate().map(|(row, &index)| {
            option(row, count, &list[index], row == highlighted)
                .on_hover(cx.listener(move |shell, hovered: &bool, _, cx| {
                    if let (true, Some(open)) = (*hovered, shell.palette.open.as_mut()) {
                        open.highlighted = row;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |shell, _, window, cx| {
                    shell.run_palette_row(row, window, cx);
                }))
        });
        let (title, empty) = match open.actions {
            true => ("Command Palette", "No matching commands"),
            false => ("Quick Open", "No matching instances"),
        };
        let field_focus = open.query.read(cx).focus_handle(cx);

        let combobox = v_flex()
            .id("command-palette-combobox")
            .role(Role::ComboBox)
            .aria_label(title)
            .aria_expanded(true)
            .track_focus(&field_focus)
            .gap(px(4.))
            .child(Input::new(&open.query).small())
            .child(
                v_flex()
                    .id("command-palette-rows")
                    .role(Role::ListBox)
                    .aria_label(match open.actions {
                        true => "Commands",
                        false => "Instances",
                    })
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .track_scroll(&open.scroll)
                    .children(options)
                    .when(count == 0, |list| {
                        list.child(
                            div()
                                .px(px(8.))
                                .py(px(4.))
                                .text_size(tokens::text_sm())
                                .text_color(tokens::text2())
                                .child(empty),
                        )
                    }),
            );

        let panel = super::super::menu::surface()
            .id("command-palette")
            .role(Role::Dialog)
            .aria_label(title)
            .w(px(560.))
            .max_w_full()
            .capture_action(cx.listener(|shell, _: &MoveUp, _, cx| {
                shell.move_palette_highlight(Move::Previous, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|shell, _: &MoveDown, _, cx| {
                shell.move_palette_highlight(Move::Next, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|shell, _: &MoveHome, _, cx| {
                shell.move_palette_highlight(Move::First, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|shell, _: &MoveEnd, _, cx| {
                shell.move_palette_highlight(Move::Last, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|shell, _: &Enter, window, cx| {
                if let Some(row) = shell.palette.open.as_ref().map(|open| open.highlighted) {
                    shell.run_palette_row(row, window, cx);
                }
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|shell, _: &Escape, window, cx| {
                shell.close_palette(window, cx);
                cx.stop_propagation();
            }))
            .on_mouse_down_out(cx.listener(|shell, _, window, cx| {
                shell.close_palette(window, cx);
            }))
            .child(combobox);
        // Top-centre under the menu bar, where both Studio's Quick Open and
        // VS Code's palette sit: near the eye, clear of the docks' content.
        Some(
            h_flex()
                .absolute()
                .top(px(72.))
                .left_0()
                .right_0()
                .px(px(16.))
                .justify_center()
                .child(panel)
                .into_any_element(),
        )
    }
}
