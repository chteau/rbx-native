//! The gutter's right-click menu and the Edit Breakpoint popup — Studio's
//! Insert Breakpoint / Conditional / Logpoint / Temporary, and the window
//! that edits a breakpoint's Condition, Log Message, Continue Execution,
//! Remove Breakpoint on Hit and Enabled.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::Sizable as _;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::debugger::{Kind, Stored};
use crate::tokens;

use super::super::{menu, Shell};

/// The right-click menu, while it is up.
pub(in crate::shell) struct Menu {
    script: Ref,
    line: u32,
    position: Point<Pixels>,
}

/// The Edit Breakpoint popup, while it is up: a working copy, written back
/// only on Save.
pub(in crate::shell) struct Editing {
    script: Ref,
    stored: Stored,
    condition: Entity<InputState>,
    log_message: Entity<InputState>,
    position: Point<Pixels>,
}

impl Menu {
    pub(super) fn new(script: Ref, line: u32, position: Point<Pixels>) -> Self {
        Menu {
            script,
            line,
            position,
        }
    }
}

impl Shell {
    fn insert_from_menu(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Menu {
            script,
            line,
            position,
        }) = self.debug.menu.take()
        else {
            return;
        };
        self.debug.breakpoints.insert(script, line, kind);
        // A conditional breakpoint without a condition, or a logpoint
        // without a message, is not what was asked for: both go straight to
        // the edit popup, as Studio's own popup asks for them.
        if matches!(kind, Kind::Conditional | Kind::Logpoint) {
            self.open_breakpoint_editor(script, line, position, window, cx);
        }
        self.sync_gutters(cx);
        cx.notify();
    }

    fn open_breakpoint_editor(
        &mut self,
        script: Ref,
        line: u32,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(stored) = self.debug.breakpoints.get(script, line).cloned() else {
            return;
        };
        let field = |text: &Option<String>,
                     placeholder: &'static str,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            let seed = text.clone().unwrap_or_default();
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(seed)
            })
        };
        let condition = field(&stored.breakpoint.condition, "var == 10", window, cx);
        let log_message = field(
            &stored.breakpoint.log_message,
            "\"The value of var:\", var",
            window,
            cx,
        );
        condition.update(cx, |state, cx| state.focus(window, cx));
        self.debug.editing = Some(Editing {
            script,
            stored,
            condition,
            log_message,
            position,
        });
        cx.notify();
    }

    fn save_breakpoint_edit(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = self.debug.editing.take() else {
            return;
        };
        let text = |input: &Entity<InputState>| {
            let value = input.read(cx).value().trim().to_owned();
            (!value.is_empty()).then_some(value)
        };
        let mut stored = editing.stored;
        stored.breakpoint.condition = text(&editing.condition);
        stored.breakpoint.log_message = text(&editing.log_message);
        self.debug.breakpoints.set(editing.script, stored);
        self.sync_gutters(cx);
        cx.notify();
    }

    /// The right-click menu or the edit popup, whichever is up.
    pub(in crate::shell) fn breakpoint_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if let Some(editing) = &self.debug.editing {
            return Some(overlay(
                editing.position,
                self.breakpoint_editor(editing, cx),
            ));
        }
        let menu = self.debug.menu.as_ref()?;
        let stored = self.debug.breakpoints.get(menu.script, menu.line);
        let (script, line, position) = (menu.script, menu.line, menu.position);
        let rows: Vec<AnyElement> = match stored {
            None => [
                ("insert", "Insert Breakpoint", Kind::Standard),
                (
                    "insert-conditional",
                    "Insert Conditional Breakpoint",
                    Kind::Conditional,
                ),
                ("insert-logpoint", "Insert Logpoint", Kind::Logpoint),
                (
                    "insert-temporary",
                    "Insert Temporary Breakpoint",
                    Kind::Temporary,
                ),
            ]
            .into_iter()
            .map(|(id, label, kind)| {
                menu::row_chrome(SharedString::from(id), None, label.into(), true, false)
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        shell.insert_from_menu(kind, window, cx);
                    }))
                    .into_any_element()
            })
            .collect(),
            Some(stored) => vec![
                menu::row_chrome(
                    "edit-breakpoint",
                    None,
                    "Edit Breakpoint".into(),
                    true,
                    false,
                )
                .on_click(cx.listener(move |shell, _, window, cx| {
                    shell.debug.menu = None;
                    shell.open_breakpoint_editor(script, line, position, window, cx);
                }))
                .into_any_element(),
                menu::row_chrome(
                    "toggle-breakpoint",
                    None,
                    if stored.enabled {
                        "Disable Breakpoint"
                    } else {
                        "Enable Breakpoint"
                    }
                    .into(),
                    true,
                    false,
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.debug.menu = None;
                    shell.debug.breakpoints.toggle_enabled(script, line);
                    shell.sync_gutters(cx);
                    cx.notify();
                }))
                .into_any_element(),
                menu::row_chrome(
                    "delete-breakpoint",
                    None,
                    "Delete Breakpoint".into(),
                    true,
                    false,
                )
                .on_click(cx.listener(move |shell, _, _, cx| {
                    shell.debug.menu = None;
                    shell.debug.breakpoints.remove(script, line);
                    shell.sync_gutters(cx);
                    cx.notify();
                }))
                .into_any_element(),
            ],
        };
        let surface = menu::surface()
            .id("breakpoint-menu")
            .occlude()
            .on_mouse_down_out(cx.listener(|shell, _: &MouseDownEvent, _, cx| {
                shell.debug.menu = None;
                cx.notify();
            }))
            .children(rows);
        Some(overlay(position, surface))
    }

    fn breakpoint_editor(&self, editing: &Editing, cx: &mut Context<Self>) -> impl IntoElement {
        let stored = &editing.stored;
        let toggle = |id: &'static str, label: &'static str, on: bool, flip: fn(&mut Stored)| {
            menu::row_chrome(id, None, label.into(), true, on).on_click(cx.listener(
                move |shell, _, _, cx| {
                    if let Some(editing) = shell.debug.editing.as_mut() {
                        flip(&mut editing.stored);
                        cx.notify();
                    }
                },
            ))
        };
        let label = |text: &'static str| {
            div()
                .px(px(8.))
                .pt(px(4.))
                .text_size(tokens::text_xs())
                .text_color(tokens::text2())
                .child(text)
        };
        let button = |id: &'static str, text: &'static str| {
            div()
                .id(id)
                .px(px(10.))
                .py(px(2.))
                .rounded(tokens::radius())
                .text_size(tokens::text_sm())
                .text_color(tokens::text_strong())
                .cursor_pointer()
                .hover(|this| tokens::hover_fx(this).bg(tokens::hover()))
                .child(text)
        };

        menu::surface()
            .id("breakpoint-editor")
            .occlude()
            .w(px(300.))
            .gap(px(2.))
            .child(
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .text_size(tokens::text_sm())
                    .text_color(tokens::text_strong())
                    .child(format!("Edit Breakpoint — line {}", stored.breakpoint.line)),
            )
            .child(label("Condition"))
            .child(
                div()
                    .px(px(8.))
                    .child(Input::new(&editing.condition).xsmall()),
            )
            .child(label("Log Message"))
            .child(
                div()
                    .px(px(8.))
                    .child(Input::new(&editing.log_message).xsmall()),
            )
            .child(toggle(
                "breakpoint-continue",
                "Continue Execution",
                stored.breakpoint.continue_execution,
                |stored| {
                    stored.breakpoint.continue_execution = !stored.breakpoint.continue_execution
                },
            ))
            .child(toggle(
                "breakpoint-remove-on-hit",
                "Remove Breakpoint on Hit",
                stored.temporary,
                |stored| stored.temporary = !stored.temporary,
            ))
            .child(toggle(
                "breakpoint-enabled",
                "Enabled",
                stored.enabled,
                |stored| stored.enabled = !stored.enabled,
            ))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(4.))
                    .pt(px(4.))
                    .child(button("breakpoint-cancel", "Cancel").on_click(cx.listener(
                        |shell, _, _, cx| {
                            shell.debug.editing = None;
                            cx.notify();
                        },
                    )))
                    .child(
                        button("breakpoint-save", "Save").on_click(
                            cx.listener(|shell, _, _, cx| shell.save_breakpoint_edit(cx)),
                        ),
                    ),
            )
    }
}

fn overlay(position: Point<Pixels>, content: impl IntoElement) -> AnyElement {
    deferred(
        anchored()
            .position(position)
            .snap_to_window_with_margin(px(8.))
            .child(content),
    )
    .into_any_element()
}
