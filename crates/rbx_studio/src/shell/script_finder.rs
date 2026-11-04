//! The script editor's function filter overlay: Studio's Script Function
//! Filter (Alt+F), a query field over the active script's functions. (Find
//! and Replace over the whole place is the Find in Place dock,
//! `shell::find_in_place`.)
//!
//! The overlay's keys are taken as the query field's own actions in the
//! capture phase: GPUI runs a focused input's key bindings before any
//! `on_key_down`, so Up/Down/Enter/Escape never reach a key listener while
//! the field has focus.

use std::ops::Range;

use gpui_kit::component::input::{Enter, Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::script_editor::{find, outline, Finder};

use super::Shell;
use crate::shortcuts::Command;

/// One function the overlay can jump to.
pub(super) struct Hit {
    reference: Ref,
    range: Range<usize>,
    label: SharedString,
    detail: SharedString,
}

impl Shell {
    /// The Script Editor panel's `on_key_down`: opens the overlay. Only
    /// reached from inside that panel, so Alt+F elsewhere is left alone.
    pub(super) fn handle_finder_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !crate::shortcuts::pressed(Command::FindFunction, keystroke) {
            return false;
        }
        self.open_finder(window, cx);
        true
    }

    fn open_finder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.scripts.tabs.active().is_none() {
            return;
        }
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Filter functions"));
        let subscription = cx.subscribe(&query, |shell, _, event: &InputEvent, cx| {
            if let (InputEvent::Change, Some(finder)) = (event, shell.scripts.finder.as_mut()) {
                finder.selected = 0;
                cx.notify();
            }
        });
        query.update(cx, |state, cx| {
            state.select_all(window, cx);
            state.focus(window, cx);
        });
        self.scripts.finder = Some(Finder {
            query,
            selected: 0,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn close_finder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.scripts.finder = None;
        if let Some(active) = self.scripts.tabs.active() {
            if let Some(open) = self.scripts.open.get(&active) {
                window.focus(&open.state.focus_handle(cx), cx);
            }
        }
        cx.notify();
    }

    fn finder_hits(&self, cx: &App) -> Vec<Hit> {
        let (Some(finder), Some(active)) = (&self.scripts.finder, self.scripts.tabs.active())
        else {
            return Vec::new();
        };
        let needle = finder.query.read(cx).value().to_lowercase();
        let text = self
            .scripts
            .open
            .get(&active)
            .map(|open| open.state.read(cx).value().to_string())
            .unwrap_or_default();
        outline::functions(&text)
            .into_iter()
            .filter(|function| function.name.to_lowercase().contains(&needle))
            .map(|function| Hit {
                reference: active,
                detail: format!("line {}", find::line_at(&text, function.range.start).0).into(),
                range: function.range,
                label: function.name.into(),
            })
            .collect()
    }

    /// Brings `hit`'s tab to the front with the hit selected, and hands focus
    /// back to that editor.
    fn jump_to(&mut self, hit: &Hit, window: &mut Window, cx: &mut Context<Self>) {
        self.scripts.finder = None;
        self.activate_script(hit.reference, cx);
        if let Some(open) = self.scripts.open.get(&hit.reference) {
            let range = hit.range.clone();
            open.state.update(cx, |state, cx| {
                state.set_selected_range(range, cx);
                state.focus(window, cx);
            });
        }
    }

    fn jump_to_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let hits = self.finder_hits(cx);
        let Some(finder) = &self.scripts.finder else {
            return;
        };
        if let Some(hit) = hits.get(finder.selected.min(hits.len().saturating_sub(1))) {
            self.jump_to(hit, window, cx);
        }
    }

    fn move_finder_selection(&mut self, down: bool, cx: &mut Context<Self>) {
        let count = self.finder_hits(cx).len();
        if let Some(finder) = self.scripts.finder.as_mut() {
            let current = finder.selected.min(count.saturating_sub(1));
            finder.selected = if down {
                (current + 1).min(count.saturating_sub(1))
            } else {
                current.saturating_sub(1)
            };
            cx.notify();
        }
    }

    /// The overlay, if it is up; drawn over the top of the editor.
    pub(super) fn script_finder(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let finder = self.scripts.finder.as_ref()?;
        let hits = self.finder_hits(cx);
        let selected = finder.selected.min(hits.len().saturating_sub(1));
        let theme = cx.theme();
        let (border, popover, active, muted) = (
            theme.border,
            theme.popover,
            theme.list_active,
            theme.muted_foreground,
        );
        let empty = hits.is_empty() && !finder.query.read(cx).value().is_empty();

        let rows = hits.into_iter().enumerate().map(|(index, hit)| {
            h_flex()
                .id(("script-finder-hit", index))
                .w_full()
                .px_2()
                .py(px(2.))
                .gap_2()
                .cursor_pointer()
                .when(index == selected, |row| row.bg(active))
                .child(div().flex_1().truncate().text_xs().child(hit.label.clone()))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(hit.detail.clone()),
                )
                .on_click(cx.listener(move |shell, _, window, cx| {
                    shell.jump_to(&hit, window, cx);
                }))
        });

        let overlay = v_flex()
            .id("script-finder")
            .absolute()
            .top_2()
            .right_4()
            .w(px(440.))
            .p_2()
            .gap_1()
            .bg(popover)
            .border_1()
            .border_color(border)
            .rounded_md()
            .shadow_lg()
            .capture_action(cx.listener(|shell, _: &MoveUp, _, cx| {
                shell.move_finder_selection(false, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|shell, _: &MoveDown, _, cx| {
                shell.move_finder_selection(true, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|shell, _: &Enter, window, cx| {
                shell.jump_to_selected(window, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|shell, _: &Escape, window, cx| {
                shell.close_finder(window, cx);
                cx.stop_propagation();
            }))
            .child(Input::new(&finder.query).xsmall())
            .child(
                v_flex()
                    .id("script-finder-hits")
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .children(rows)
                    .when(empty, |list| {
                        list.child(div().px_2().text_xs().text_color(muted).child("No matches"))
                    }),
            );
        Some(overlay.into_any_element())
    }
}
