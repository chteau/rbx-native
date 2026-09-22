//! Editing a text element's words where they are drawn: a double-click on
//! a selected `TextLabel`, `TextButton` or `TextBox` opens a field over it
//! holding its `Text`; Enter or clicking away keeps what was typed, Escape
//! drops it. One undo step, the Properties panel's own commit.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::Sizable as _;
use gpui_kit::*;
use rbx_dom::{Ref, Variant};

use super::Shell;
use crate::tokens;
use crate::ui_canvas::{box_of, Rect};

const TEXT_CLASSES: [&str; 3] = ["TextLabel", "TextButton", "TextBox"];

/// The field open over a text element.
pub(super) struct TextEdit {
    referent: Ref,
    input: Entity<InputState>,
    _subscription: Subscription,
}

impl Shell {
    pub(super) fn is_text(&self, referent: Ref) -> bool {
        self.dom.get(referent).is_some_and(|instance| {
            TEXT_CLASSES
                .iter()
                .any(|class| self.database.is_subclass_of(instance.class(), class))
        })
    }

    pub(super) fn begin_text_edit(
        &mut self,
        referent: Ref,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = match self
            .dom
            .get(referent)
            .and_then(|i| i.properties().get("Text"))
        {
            Some(Variant::String(text)) => text.clone(),
            _ => String::new(),
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
        let subscription = cx.subscribe(&input, |shell, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                shell.end_text_edit(true, cx);
            }
        });
        // After the press that opened it is done: the canvas takes focus on
        // its own mouse-down, and would take it straight back.
        let field = input.clone();
        window.defer(cx, move |window, cx| {
            field.update(cx, |state, cx| {
                state.focus(window, cx);
                state.select_all(window, cx);
            });
        });
        self.ui.text_edit = Some(TextEdit {
            referent,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Closes the field, writing what it holds when `keep` and it changed.
    pub(super) fn end_text_edit(&mut self, keep: bool, cx: &mut Context<Self>) {
        let Some(edit) = self.ui.text_edit.take() else {
            return;
        };
        let text = edit.input.read(cx).value().to_string();
        let unchanged = matches!(
            self.dom.get(edit.referent).and_then(|i| i.properties().get("Text")),
            Some(Variant::String(old)) if *old == text
        );
        if keep && !unchanged {
            self.write_drag(true, &[(edit.referent, "Text", text)], cx);
        }
        cx.notify();
    }

    /// The field, laid over the element's box as the canvas shows it.
    pub(super) fn text_edit_field(&self, cx: &App) -> Option<AnyElement> {
        let edit = self.ui.text_edit.as_ref()?;
        let (_, boxes) = self.canvas_boxes(cx)?;
        let rect = Rect::of(box_of(&boxes, edit.referent)?);
        let view = self.ui.view;
        let [x, y] = view.to_view([rect.x, rect.y]);
        Some(
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px((rect.w * view.zoom).max(120.0)))
                .min_h(px(rect.h * view.zoom))
                .flex()
                .items_center()
                .px(px(4.))
                .rounded(tokens::RADIUS)
                .bg(tokens::chrome())
                .shadow(tokens::focus_ring(tokens::black()))
                .child(
                    Input::new(&edit.input)
                        .appearance(false)
                        .with_size(tokens::field_size()),
                )
                .into_any_element(),
        )
    }
}
