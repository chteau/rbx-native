//! The script editor's finder overlay: Studio's Script Function Filter
//! (Alt+F, the active script's functions) and Find All / Replace All
//! (Ctrl+Shift+F, every open tab). One overlay for both — each is a query
//! field over a list of places to jump to, and they differ only in where the
//! list comes from.
//!
//! The overlay's keys are taken as the query field's own actions in the
//! capture phase: GPUI runs a focused input's key bindings before any
//! `on_key_down`, so Up/Down/Enter/Escape never reach a key listener while
//! the field has focus.

use std::ops::Range;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Enter, Escape, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::script_editor::{find, outline, source, Finder, FinderMode};

use super::Shell;

/// Past this many hits the list stops growing: a one-letter Find All over a
/// few large scripts is thousands of rows nobody scrolls through, and each is
/// a laid-out element.
const MAX_HITS: usize = 500;

/// One place the overlay can jump to.
pub(super) struct Hit {
    reference: Ref,
    range: Range<usize>,
    label: SharedString,
    detail: SharedString,
}

/// The shortcut that opens the overlay in `mode`, if `keystroke` is one.
pub(super) fn mode_for(keystroke: &Keystroke) -> Option<FinderMode> {
    let m = keystroke.modifiers;
    match keystroke.key.as_str() {
        "f" if m.alt && !m.control && !m.shift && !m.platform => Some(FinderMode::Functions),
        "f" | "h" if m.secondary() && m.shift && !m.alt => Some(FinderMode::FindAll),
        _ => None,
    }
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
        let Some(mode) = mode_for(keystroke) else {
            return false;
        };
        self.open_finder(mode, window, cx);
        true
    }

    fn open_finder(&mut self, mode: FinderMode, window: &mut Window, cx: &mut Context<Self>) {
        let Some(active) = self.scripts.tabs.active() else {
            return;
        };
        // Find All starts from what's selected, as Studio's does.
        let seed = match mode {
            FinderMode::FindAll => self
                .scripts
                .open
                .get(&active)
                .map(|open| open.state.read(cx).selected_value().to_string())
                .filter(|text| !text.contains('\n'))
                .unwrap_or_default(),
            FinderMode::Functions => String::new(),
        };
        let placeholder = match mode {
            FinderMode::Functions => "Filter functions",
            FinderMode::FindAll => "Find in open scripts",
        };
        let query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(seed)
        });
        let replacement = cx.new(|cx| InputState::new(window, cx).placeholder("Replace with"));
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
            mode,
            query,
            replacement,
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
        let Some(finder) = &self.scripts.finder else {
            return Vec::new();
        };
        let query = finder.query.read(cx).value().to_string();
        let text_of = |reference: Ref| {
            self.scripts
                .open
                .get(&reference)
                .map(|open| open.state.read(cx).value().to_string())
        };
        match finder.mode {
            FinderMode::Functions => {
                let Some(active) = self.scripts.tabs.active() else {
                    return Vec::new();
                };
                let text = text_of(active).unwrap_or_default();
                let needle = query.to_lowercase();
                outline::functions(&text)
                    .into_iter()
                    .filter(|function| function.name.to_lowercase().contains(&needle))
                    .map(|function| Hit {
                        reference: active,
                        detail: format!("line {}", find::line_at(&text, function.range.start).0)
                            .into(),
                        range: function.range,
                        label: function.name.into(),
                    })
                    .collect()
            }
            FinderMode::FindAll => self
                .scripts
                .tabs
                .all()
                .iter()
                .flat_map(|&reference| {
                    let text = text_of(reference).unwrap_or_default();
                    let name = source::label(&self.dom, reference).unwrap_or_default();
                    find::matches(&text, &query)
                        .into_iter()
                        .map(|range| {
                            let (line, content) = find::line_at(&text, range.start);
                            Hit {
                                reference,
                                label: content.to_string().into(),
                                detail: format!("{name}:{line}").into(),
                                range,
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .take(MAX_HITS)
                .collect(),
        }
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

    /// Replace All across every open tab. Goes through the editor rather than
    /// the DOM, so each tab keeps the change on its own undo stack and
    /// reaches `Source` through the same debounced write typing does.
    fn replace_everywhere(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(finder) = &self.scripts.finder else {
            return;
        };
        let query = finder.query.read(cx).value().to_string();
        let replacement = finder.replacement.read(cx).value().to_string();
        for reference in self.scripts.tabs.all().to_vec() {
            let Some(state) = self
                .scripts
                .open
                .get(&reference)
                .map(|open| open.state.clone())
            else {
                continue;
            };
            let text = state.read(cx).value().to_string();
            let Some(replaced) = find::replace_all(&text, &query, &replacement) else {
                continue;
            };
            state.update(cx, |state, cx| state.replace_all(replaced, window, cx));
            self.script_changed(reference, cx);
        }
        cx.notify();
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
        let find_all = finder.mode == FinderMode::FindAll;
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
            .when(find_all, |overlay| {
                overlay.child(
                    h_flex()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&finder.replacement).xsmall()),
                        )
                        .child(
                            Button::new("script-finder-replace-all")
                                .label("Replace All")
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(|shell, _, window, cx| {
                                    shell.replace_everywhere(window, cx);
                                })),
                        ),
                )
            })
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

#[cfg(test)]
mod tests {
    // Not `super::*`: that brings in `gpui_kit::*`, whose own `test` macro
    // would shadow the standard one.
    use super::mode_for;
    use crate::script_editor::FinderMode;
    use gpui_kit::Keystroke;

    fn key(text: &str) -> Keystroke {
        Keystroke::parse(text).unwrap()
    }

    #[test]
    fn shortcuts_open_the_matching_mode() {
        assert_eq!(mode_for(&key("alt-f")), Some(FinderMode::Functions));
        assert_eq!(
            mode_for(&key("secondary-shift-f")),
            Some(FinderMode::FindAll)
        );
        assert_eq!(
            mode_for(&key("secondary-shift-h")),
            Some(FinderMode::FindAll)
        );
        // Plain Ctrl+F is the editor's own in-script Find.
        assert_eq!(mode_for(&key("secondary-f")), None);
        assert_eq!(mode_for(&key("f")), None);
    }
}
