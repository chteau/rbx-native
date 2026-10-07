//! Typing where the words are drawn: a press on a chip opens a field over
//! it holding the raw literal (a string without its quotes), and a
//! double-click on a group's title one holding the title. Enter or a press
//! elsewhere keeps it, Escape drops it.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::Sizable as _;
use gpui_kit::*;
use rbx_dom::Ref;

use crate::script_editor::graph::layout;
use crate::script_editor::graph::End;
use crate::tokens;

use super::super::Shell;

/// What the field writes back to.
#[derive(Debug, Clone)]
pub(super) enum Target {
    Pin(End),
    Group(usize),
}

pub(in crate::shell) struct LiteralEdit {
    target: Target,
    input: Entity<InputState>,
    _subscription: Subscription,
}

impl Shell {
    pub(super) fn begin_literal_edit(
        &mut self,
        reference: Ref,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        let value = match &target {
            Target::Pin(end) => editor.graph.value(end).unwrap_or_default(),
            Target::Group(index) => editor
                .graph
                .groups
                .get(*index)
                .map(|group| group.title.clone())
                .unwrap_or_default(),
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |shell, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    shell.end_literal_edit(reference, true, cx);
                    if let Some(editor) = shell.graphs.get(&reference) {
                        window.focus(&editor.focus, cx);
                    }
                }
                InputEvent::Blur => shell.end_literal_edit(reference, true, cx),
                _ => {}
            },
        );
        // After the press that opened it: the canvas takes focus on its own
        // mouse-down, and would take it straight back.
        let field = input.clone();
        window.defer(cx, move |window, cx| {
            field.update(cx, |state, cx| {
                state.focus(window, cx);
                state.select_all(window, cx);
            });
        });
        editor.literal = Some(LiteralEdit {
            target,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Closes the field, keeping what it holds when `keep` and it changed.
    pub(super) fn end_literal_edit(&mut self, reference: Ref, keep: bool, cx: &mut Context<Self>) {
        let Some(editor) = self.graphs.get_mut(&reference) else {
            return;
        };
        let Some(edit) = editor.literal.take() else {
            return;
        };
        let text = edit.input.read(cx).value().to_string();
        if !keep {
            cx.notify();
            return;
        }
        let changed = match &edit.target {
            Target::Pin(end) => {
                let changed = editor.graph.value(end).as_deref() != Some(text.as_str());
                if changed {
                    editor.graph.set_value(end, text);
                }
                changed
            }
            Target::Group(index) => match editor.graph.groups.get_mut(*index) {
                // An emptied title keeps the old one: a frame with no name
                // has nothing left to be picked up by.
                Some(group) if !text.trim().is_empty() && group.title != text => {
                    group.title = text;
                    true
                }
                _ => false,
            },
        };
        if changed {
            self.commit_graph(reference, cx);
        }
        cx.notify();
    }

    pub(super) fn literal_element(
        &mut self,
        reference: Ref,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let editor = self.graphs.get(&reference)?;
        let edit = editor.literal.as_ref()?;
        let chip = match &edit.target {
            Target::Pin(end) => {
                let node = editor.graph.node(end.node)?;
                layout::chip_rect(&editor.graph, node, &end.pin)?
            }
            Target::Group(index) => layout::group_title(editor.graph.groups.get(*index)?),
        };
        let at = editor.view.to_view([chip.x, chip.y]);
        let z = editor.view.zoom;
        Some(
            div()
                .absolute()
                .left(px(at[0] - 2.0))
                .top(px(at[1] - 4.0))
                .w(px((chip.w * z).max(140.0)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .capture_key_down(cx.listener(move |shell, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        shell.end_literal_edit(reference, false, cx);
                        if let Some(editor) = shell.graphs.get(&reference) {
                            window.focus(&editor.focus, cx);
                        }
                        cx.stop_propagation();
                    }
                }))
                .child(
                    Input::new(&edit.input)
                        .xsmall()
                        .font_family(tokens::FONT_FAMILY_MONO),
                )
                .into_any_element(),
        )
    }
}
