//! The command palette: Ctrl+Alt+P, Studio's own key for searching its
//! actions in Quick Open ("Quickest Open in the West!", DevForum, 2020) —
//! a filter field over every command the editor already has (see
//! [`commands`]), each row with its shortcut, so a command can be found
//! by name instead of remembered by key.
//!
//! Keyboard and semantics follow the APG combobox-with-listbox pattern:
//! focus stays in the field, Up/Down/Home/End move the highlighted option
//! (wrapping, as the menus do), Enter runs it, Escape or a click outside
//! closes, and focus goes back to wherever it was. The field's own
//! bindings are taken in the capture phase, as `shell::script_finder`
//! does, because a focused input runs its key bindings before any key
//! listener sees the key.
//!
//! The shortcut is a keymap binding for View › Command Palette…
//! (`menu_bar::install_key_bindings`), so the key and the menu item are
//! one action.

mod commands;

use gpui_kit::component::input::{
    Enter, Escape, Input, InputEvent, InputState, MoveDown, MoveEnd, MoveHome, MoveUp,
};
use gpui_kit::component::{h_flex, v_flex, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens;

use super::roving::Move;
use super::Shell;
use commands::{Command, Run};

#[derive(Default)]
pub(super) struct Palette {
    open: Option<Open>,
    /// Labels of the commands run this session, most recent first.
    recent: Vec<SharedString>,
}

struct Open {
    query: Entity<InputState>,
    commands: Vec<Command>,
    /// Index into the filtered rows, not into `commands`.
    highlighted: usize,
    /// What held focus before the palette took it.
    restore: Option<FocusHandle>,
    scroll: ScrollHandle,
    _subscription: Subscription,
}

impl Shell {
    pub(crate) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.open.is_some() {
            return;
        }
        let panels = {
            let hidden = self.document_hides();
            super::Panel::ALL
                .into_iter()
                .filter(move |p| !hidden.contains(p))
        };
        let menus = crate::menu_bar::menus(self.script_templates.extras());
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Type a command"));
        let subscription = cx.subscribe(&query, |shell, _, event: &InputEvent, cx| {
            if let (InputEvent::Change, Some(open)) = (event, shell.palette.open.as_mut()) {
                open.highlighted = 0;
                open.scroll.scroll_to_item(0);
                cx.notify();
            }
        });
        let restore = window.focused(cx);
        query.update(cx, |state, cx| state.focus(window, cx));
        self.palette.open = Some(Open {
            query,
            commands: commands::registry(&menus, panels),
            highlighted: 0,
            restore,
            scroll: ScrollHandle::new(),
            _subscription: subscription,
        });
        cx.notify();
    }

    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Open> {
        let open = self.palette.open.take()?;
        if let Some(handle) = &open.restore {
            window.focus(handle, cx);
        }
        cx.notify();
        Some(open)
    }

    fn palette_rows(&self, cx: &App) -> Vec<usize> {
        let Some(open) = &self.palette.open else {
            return Vec::new();
        };
        commands::filter(
            &open.commands,
            &open.query.read(cx).value(),
            &self.palette.recent,
        )
    }

    fn move_palette_highlight(&mut self, movement: Move, cx: &mut Context<Self>) {
        let count = self.palette_rows(cx).len();
        if let Some(open) = self.palette.open.as_mut() {
            if count > 0 {
                open.highlighted = movement.apply(open.highlighted.min(count - 1), count);
                open.scroll.scroll_to_item(open.highlighted);
            }
            cx.notify();
        }
    }

    /// Closes first, so the command runs against the focus it would have
    /// had if its shortcut had been typed instead.
    fn run_palette_row(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(&index) = self.palette_rows(cx).get(row) else {
            return;
        };
        let Some(mut open) = self.close_palette(window, cx) else {
            return;
        };
        let command = open.commands.swap_remove(index);
        self.palette.recent.retain(|label| *label != command.label);
        self.palette.recent.insert(0, command.label);
        match command.run {
            // Deferred by GPUI, which matters: every menu handler updates
            // `Shell`, which is mid-update right here.
            Run::Action(action) => window.dispatch_action(action, cx),
            Run::Tool(tool) => self.transform_action(crate::transform::Action::Use(tool), cx),
            Run::Focus(panel) => {
                self.set_panel_open(panel, true, cx);
                // The Explorer is the one dock with a single keyboard entry
                // point of its own; the rest are brought forward only.
                if panel == super::Panel::Explorer {
                    window.focus(&self.tree_focus_handle, cx);
                }
            }
        }
    }

    pub(super) fn command_palette(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.palette.open.as_ref()?;
        let rows = self.palette_rows(cx);
        let count = rows.len();
        let highlighted = open.highlighted.min(count.saturating_sub(1));
        let options = rows.into_iter().enumerate().map(|(row, index)| {
            let command = &open.commands[index];
            let hint = command.hint.map(commands::display);
            let selected = row == highlighted;
            super::menu::row_chrome(
                ("palette-row", row),
                None,
                command.label.clone(),
                true,
                false,
            )
            .role(Role::ListBoxOption)
            .aria_label(command.label.clone())
            .aria_selected(selected)
            .aria_position_in_set(row + 1)
            .aria_size_of_set(count)
            .when_some(hint.clone(), |row, hint| row.aria_keyshortcuts(hint))
            .when(selected, |row| row.bg(tokens::selection()))
            .when_some(hint, |row, hint| {
                // The muted grey is too faint on the highlight's accent.
                let color = match selected {
                    true => tokens::text_strong(),
                    false => tokens::text2(),
                };
                row.child(div().flex_none().text_color(color).child(hint))
            })
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

        let panel = super::menu::surface()
            .id("command-palette")
            .role(Role::Dialog)
            .aria_label("Command Palette")
            .w(px(560.))
            .max_w_full()
            .gap(px(4.))
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
            .child(Input::new(&open.query).small())
            .child(
                v_flex()
                    .id("command-palette-rows")
                    .role(Role::ListBox)
                    .aria_label("Commands")
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
                                .child("No matching commands"),
                        )
                    }),
            );
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

#[cfg(test)]
#[path = "palette/tests.rs"]
mod tests;
