//! What the dock's Script Editor panel draws: a tab strip over the active
//! script's editor.
//!
//! The tabs are drawn here rather than made dock panels of their own. A dock
//! tab is a persisted, rearrangeable part of the window layout, and an open
//! script is neither — it comes and goes with a double-click, and a saved
//! layout naming scripts by referent would be meaningless on the next file
//! opened.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Editor, GoToDefinition};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::*;
use rbx_dom::Ref;

use crate::script_editor::{find, outline, source};

use super::Shell;

impl Shell {
    /// The Script Editor panel. Reconciles every open tab against the DOM
    /// first (see [`Shell::resync_scripts`]) so a tab never paints a script
    /// the DOM no longer agrees with.
    pub(super) fn script_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.resync_scripts(window, cx);

        let Some(active) = self.scripts.tabs.active() else {
            return no_scripts_open(cx).into_any_element();
        };

        v_flex()
            .size_full()
            .on_key_down(cx.listener(move |shell, event: &KeyDownEvent, window, cx| {
                // Stopping here also keeps Ctrl+D from reaching the window's
                // own handler, where it is the Explorer's Duplicate.
                if shell.handle_finder_key(&event.keystroke, window, cx)
                    || shell.handle_match_cursor_key(active, &event.keystroke, window, cx)
                    || shell.handle_debug_key(&event.keystroke, cx)
                {
                    cx.stop_propagation();
                }
            }))
            // The right-click menu's Go to Definition. The editor's own
            // handler only jumps to a target a Ctrl-hover already resolved, so
            // from a plain right-click it would do nothing; taken here first,
            // it resolves the name under the cursor itself.
            .capture_action(cx.listener(move |shell, _: &GoToDefinition, window, cx| {
                shell.go_to_declaration(active, window, cx);
                cx.stop_propagation();
            }))
            .child(self.script_tabs(active, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .overflow_hidden()
                    .children(self.scripts.open.get(&active).map(|open| {
                        Editor::new(&open.state)
                            .bordered(false)
                            .h(relative(1.0))
                            .w_full()
                    }))
                    .children(self.script_finder(cx)),
            )
            .children(self.breakpoint_overlay(cx))
            .into_any_element()
    }

    /// Ctrl+D (Cmd+D) adds a cursor at the next match of the selection,
    /// Shift+Alt+L one at every match — Studio's own bindings for both. Only
    /// while the editor itself has focus, so the finder's query field keeps
    /// its keys.
    fn handle_match_cursor_key(
        &mut self,
        reference: Ref,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let m = keystroke.modifiers;
        let every = match keystroke.key.as_str() {
            "d" if m.secondary() && !m.shift && !m.alt => false,
            "l" if m.shift && m.alt && !m.control && !m.platform => true,
            _ => return false,
        };
        let Some(open) = self.scripts.open.get(&reference) else {
            return false;
        };
        if !open.state.focus_handle(cx).is_focused(window) {
            return false;
        }
        open.state.update(cx, |state, cx| {
            let text = state.value().to_string();
            let selections = state.selected_ranges();
            let extend = if every {
                find::every_match(&text, &selections)
            } else {
                find::next_match(&text, &selections)
            };
            let Some(extend) = extend else {
                return;
            };
            if let Some(primary) = extend.primary {
                state.set_selected_range(primary, cx);
            }
            for range in extend.add {
                state.add_selection(range, cx);
            }
        });
        true
    }

    fn go_to_declaration(&mut self, reference: Ref, window: &mut Window, cx: &mut Context<Self>) {
        if !self.go_to_lsp_definition(reference, window, cx) {
            self.go_to_lexer_declaration(reference, window, cx);
        }
    }

    /// Within this script only, from the lexer: what Go to Definition does
    /// before `luau-lsp` is up, or when it finds nothing.
    pub(super) fn go_to_lexer_declaration(
        &mut self,
        reference: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(open) = self.scripts.open.get(&reference) else {
            return;
        };
        open.state.update(cx, |state, cx| {
            let text = state.value().to_string();
            if let Some(target) = outline::declaration(&text, state.cursor()) {
                state.set_selected_range(target, cx);
                state.focus(window, cx);
            }
        });
    }

    fn script_tabs(&self, active: Ref, cx: &mut Context<Self>) -> impl IntoElement {
        let open: Vec<Ref> = self.scripts.tabs.all().to_vec();
        let selected = open.iter().position(|tab| *tab == active).unwrap_or(0);
        let clicked = open.clone();

        let tabs = open.iter().enumerate().map(|(index, reference)| {
            let reference = *reference;
            // Read from the DOM rather than cached at open time, so a rename
            // in the Explorer retitles the tab.
            let label = source::label(&self.dom, reference).unwrap_or_default();
            Tab::new()
                .label(SharedString::from(label))
                // `prefix`, not `icon`: a `Tab` given an icon draws *only*
                // the icon, and a tab strip of identical file glyphs says
                // nothing about which script is which.
                .prefix(Icon::new(IconName::FileCode).xsmall())
                .suffix(
                    Button::new(("close-script-tab", index))
                        .icon(IconName::Close)
                        .ghost()
                        .xsmall()
                        .accessibility_label("Close tab")
                        .on_click(cx.listener(move |shell, _, _, cx| {
                            shell.close_script(reference, cx);
                        })),
                )
        });

        h_flex()
            .w_full()
            .child(
                TabBar::new("script-editor-tabs")
                    .flex_1()
                    .underline()
                    .small()
                    .selected_index(selected)
                    .children(tabs)
                    .on_click(cx.listener(move |shell, index: &usize, _, cx| {
                        if let Some(reference) = clicked.get(*index) {
                            shell.activate_script(*reference, cx);
                        }
                    })),
            )
            .child(self.debug_controls(cx))
    }
}

/// What the panel shows before anything has been opened in it — the panel is
/// part of the default layout, so it is what a fresh window draws.
fn no_scripts_open(cx: &App) -> impl IntoElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .child(
            Icon::new(IconName::FileCode)
                .large()
                .text_color(cx.theme().muted_foreground),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Double-click a Script in the Explorer to edit it"),
        )
}
