//! Typing in a widget's `TextBox`. The widget renderer draws the box as it
//! is edited, its text as typed, the selection behind it and a single line
//! scrolled to the caret, so the text wraps on the same words and every
//! glyph sits where it does unfocused (see `rbx_viewer`'s `GuiTextEdit`).
//! The draw says where each character went, which is all the caret, a
//! click, a drag or Up and Down need; the keys and the platform's text
//! input go to a [`Field`] that holds the text.

mod field;
mod model;

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::*;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_lua::plugin::{GuiEventKind, UiId};
use rbx_viewer::{GuiTextEdit, GuiTextMap};

use crate::shell::Shell;
pub(crate) use field::install as install_keys;
use field::{Field, FieldEvent};
use model::TextModel;

/// How long the caret shows and hides for: Windows' default caret blink,
/// which Studio's matches.
const BLINK: Duration = Duration::from_millis(530);

/// What a `TextBox` says about editing it, as last rendered.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::shell::plugins) struct TextBox {
    text: String,
    /// `ClearTextOnFocus`, `true` unless set otherwise, as in Roblox.
    clear_on_focus: bool,
    multi_line: bool,
    editable: bool,
    /// `TextColor3` with `1 - TextTransparency`: the caret's colour.
    color: Hsla,
}

/// The `TextBox` being typed in.
pub(super) struct Editing {
    pub(super) widget: UiId,
    pub(super) referent: Ref,
    field: Entity<Field>,
    color: Hsla,
    /// The click that focused the box, placed once a draw says where the
    /// characters are: `(point, clicks)`.
    click: Option<([f32; 2], usize)>,
    /// The left button went down on the box and is still held.
    dragging: bool,
    /// The single line's scroll, as the last draw left it.
    scroll: f32,
    caret_shown: bool,
    blink: Option<Task<()>>,
    /// Not focused yet: a focused element missing from the frame blurs, so
    /// the focus waits until a render has placed it.
    unfocused: bool,
    _events: Subscription,
}

fn color3(dom_value: Option<&Variant>, fallback: [f32; 3], alpha: f32) -> Hsla {
    let [r, g, b] = match dom_value {
        Some(Variant::Color3(c)) => [c.r, c.g, c.b],
        _ => fallback,
    };
    Rgba { r, g, b, a: alpha }.into()
}

/// Every `TextBox` in a widget's tree.
pub(super) fn text_boxes(dom: &WeakDom) -> HashMap<Ref, TextBox> {
    let mut found = HashMap::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        if instance.class() == "TextBox" {
            let props = instance.properties();
            let flag = |name: &str, default: bool| match props.get(name) {
                Some(Variant::Bool(value)) => *value,
                _ => default,
            };
            let transparency = match props.get("TextTransparency") {
                Some(Variant::Float32(t)) => *t,
                _ => 0.0,
            };
            found.insert(
                referent,
                TextBox {
                    text: match props.get("Text") {
                        Some(Variant::String(text)) => text.clone(),
                        _ => String::new(),
                    },
                    clear_on_focus: flag("ClearTextOnFocus", true),
                    multi_line: flag("MultiLine", false),
                    editable: flag("TextEditable", true),
                    // Roblox's default TextColor3: 27, 42, 53.
                    color: color3(
                        props.get("TextColor3"),
                        [0.106, 0.165, 0.208],
                        (1.0 - transparency).clamp(0.0, 1.0),
                    ),
                },
            );
        }
        stack.extend_from_slice(instance.children());
    }
    found
}

impl Editing {
    /// What the renderer draws the box with, its text written into `dom`.
    pub(super) fn prepare(&self, dom: &mut WeakDom, cx: &App) -> GuiTextEdit {
        let model = &self.field.read(cx).model;
        let _ = dom.set_property(self.referent, "Text", Variant::String(model.text.clone()));
        GuiTextEdit {
            referent: self.referent,
            selection: model.selection(),
            caret: model.caret,
            scroll: self.scroll,
            empty: model.text.is_empty(),
        }
    }
}

impl Shell {
    /// Focuses `referent` on widget `id`, its caret at the `click` that did
    /// it once the box has been drawn (`ClearTextOnFocus` empties it first).
    pub(in crate::shell::plugins) fn begin_widget_edit(
        &mut self,
        id: UiId,
        referent: Ref,
        click: ([f32; 2], usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(previous) = self.plugins.widgets.editing.as_ref().map(|e| e.referent) {
            self.end_widget_edit(previous, false, cx);
        }
        let Some(text_box) = self
            .plugins
            .widgets
            .text_boxes
            .get(&id)
            .and_then(|boxes| boxes.get(&referent))
            .cloned()
        else {
            return;
        };
        let cleared = text_box.clear_on_focus && !text_box.text.is_empty();
        let model = TextModel::new(match cleared {
            true => String::new(),
            false => text_box.text.clone(),
        });
        let field =
            cx.new(|cx| Field::new(model, text_box.multi_line, text_box.editable, window, cx));
        let events = cx.subscribe(
            &field,
            move |shell, field, event: &FieldEvent, cx| match event {
                FieldEvent::Changed => {
                    let text = field.read(cx).model.text.clone();
                    shell.send_gui(referent, GuiEventKind::TextChanged(text), cx);
                    shell.restart_blink(cx);
                }
                FieldEvent::Moved => shell.restart_blink(cx),
                FieldEvent::Submit => shell.end_widget_edit(referent, true, cx),
                FieldEvent::Blur => shell.end_widget_edit(referent, false, cx),
            },
        );
        self.plugins.widgets.editing = Some(Editing {
            widget: id,
            referent,
            field,
            color: text_box.color,
            click: (!cleared).then_some(click),
            dragging: true,
            scroll: 0.0,
            caret_shown: true,
            blink: None,
            unfocused: true,
            _events: events,
        });
        if cleared {
            self.send_gui(referent, GuiEventKind::TextChanged(String::new()), cx);
        }
        self.send_gui(referent, GuiEventKind::Focused, cx);
        self.restart_blink(cx);
    }

    pub(in crate::shell::plugins) fn end_widget_edit(
        &mut self,
        referent: Ref,
        enter: bool,
        cx: &mut Context<Self>,
    ) {
        if self
            .plugins
            .widgets
            .editing
            .take_if(|e| e.referent == referent)
            .is_none()
        {
            return;
        }
        self.send_gui(referent, GuiEventKind::FocusLost { enter }, cx);
        cx.notify();
    }

    /// Shows the caret and starts its blink over: it stays solid while the
    /// text or the caret moves, as Studio's does.
    fn restart_blink(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = self.plugins.widgets.editing.as_mut() else {
            return;
        };
        editing.caret_shown = true;
        editing.blink = Some(cx.spawn(async move |shell, cx| loop {
            cx.background_executor().timer(BLINK).await;
            let blinked = shell.update(cx, |shell, cx| {
                let editing = shell.plugins.widgets.editing.as_mut()?;
                editing.caret_shown = !editing.caret_shown;
                cx.notify();
                Some(())
            });
            if !matches!(blinked, Ok(Some(()))) {
                break;
            }
        }));
        cx.notify();
    }

    /// A press on the box being typed in: the caret to the glyph boundary it
    /// lands on (see `GuiTextMap::hit`), a double-click takes the word, a
    /// third click the text.
    pub(in crate::shell::plugins) fn widget_edit_press(
        &mut self,
        at: [f32; 2],
        clicks: usize,
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(editing) = self.plugins.widgets.editing.as_mut() else {
            return;
        };
        editing.dragging = true;
        let Some(map) = editing.field.read(cx).map.clone() else {
            editing.click = Some((at, clicks));
            return;
        };
        place(&editing.field, &map, at, clicks, shift, cx);
    }

    /// The pointer moved with the button held since a press on the box: the
    /// selection follows it.
    pub(in crate::shell::plugins) fn widget_edit_drag(
        &mut self,
        at: [f32; 2],
        held: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(editing) = self.plugins.widgets.editing.as_mut() else {
            return false;
        };
        if !held {
            editing.dragging = false;
        }
        if !editing.dragging {
            return false;
        }
        let field = editing.field.clone();
        let Some(to) = field.read(cx).map.as_ref().and_then(|map| map.hit(at)) else {
            return true;
        };
        field.update(cx, |field, cx| {
            if field.model.caret != to {
                field.model.move_to(to, true);
                cx.emit(FieldEvent::Moved);
            }
        });
        true
    }

    /// A draw of widget `id` landed: if it drew the box being typed in, its
    /// map is what the caret, clicks and keys go by from now on, and a click
    /// waiting for it is placed.
    pub(super) fn widget_edit_drawn(&mut self, id: UiId, cx: &mut Context<Self>) {
        let state = &mut self.plugins.widgets;
        let Some(editing) = state.editing.as_mut().filter(|e| e.widget == id) else {
            return;
        };
        let Some(drawn) = state.drawn.get(&id) else {
            return;
        };
        let (Some(edit), Some(map)) = (drawn.edit.as_ref(), drawn.map.clone()) else {
            return;
        };
        if edit.referent != editing.referent {
            return;
        }
        // The draw moved the scroll to keep the caret in: what is asked next
        // starts from there, and what was asked already was drawn with it.
        if editing.scroll != map.scroll {
            editing.scroll = map.scroll;
            if let Some((_, _, Some(asked))) = state.requested.get_mut(&id) {
                asked.scroll = map.scroll;
            }
        }
        let origin = state.areas.get(&id).and_then(|a| a.get()).map(|a| a.origin);
        editing.field.update(cx, |field, _| {
            field.map = Some(map.clone());
            field.origin = origin.unwrap_or_default();
        });
        if let Some((at, clicks)) = editing.click.take() {
            place(&editing.field, &map, at, clicks, false, cx);
        }
    }

    /// The caret over the box being typed in on widget `id`, and the element
    /// that holds the keyboard focus and takes the platform's text input.
    pub(super) fn widget_edit_overlay(
        &mut self,
        id: UiId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let editing = self
            .plugins
            .widgets
            .editing
            .as_mut()
            .filter(|e| e.widget == id)?;
        let field = editing.field.clone();
        let focus = field.read(cx).focus.clone();
        if std::mem::take(&mut editing.unfocused) {
            let focus = focus.clone();
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
            window.refresh();
        }
        let editing = &*editing;
        let input = canvas(|_, _, _| {}, {
            let field = field.clone();
            let focus = focus.clone();
            move |bounds, _, window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, field), cx);
            }
        })
        .size_full();
        let drawn = self.plugins.widgets.drawn.get(&id);
        // The caret where the image under it put it: both are the same draw.
        let caret = drawn
            .and_then(|drawn| {
                let edit = drawn
                    .edit
                    .as_ref()
                    .filter(|e| e.referent == editing.referent)?;
                drawn.map.as_ref()?.caret(edit.caret)
            })
            .filter(|_| editing.caret_shown && focus.is_focused(window))
            .map(|[x, top, height]| {
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(top))
                    .w(px(1.))
                    .h(px(height))
                    .bg(editing.color)
            });
        let keys = Field::keys(
            &field,
            div().track_focus(&focus).absolute().size_0().child(input),
            window,
        );
        Some(
            div()
                .absolute()
                .inset_0()
                .children(caret)
                .child(keys)
                .into_any_element(),
        )
    }
}

/// Puts the caret where `at` lands on `map`: a click places it (extending the
/// selection with Shift), a double-click selects the word, a third click
/// the whole text.
fn place(
    field: &Entity<Field>,
    map: &GuiTextMap,
    at: [f32; 2],
    clicks: usize,
    shift: bool,
    cx: &mut App,
) {
    let Some(offset) = map.hit(at) else {
        return;
    };
    field.update(cx, |field, cx| {
        match clicks {
            0 | 1 => field.model.move_to(offset, shift),
            2 => {
                let word = field.model.word_at(offset);
                field.model.select(word);
            }
            _ => field.model.select(0..field.model.text.len()),
        }
        cx.emit(FieldEvent::Moved);
    });
}
