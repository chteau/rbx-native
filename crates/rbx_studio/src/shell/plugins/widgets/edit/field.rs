//! The keyboard side of a `TextBox` being typed in: the focus, the text the
//! platform types (IME included) and the editing keys, bound in a key
//! context of their own so they out-rank the editor's shortcuts (Delete
//! deleting the selection, Ctrl+Z undoing a place edit) while a box has the
//! keys.

use std::ops::Range;

use gpui_kit::*;
use rbx_viewer::GuiTextMap;

use super::model::TextModel;

pub(super) const CONTEXT: &str = "PluginTextBox";

actions!(
    plugin_text_box,
    [
        Backspace,
        BackspaceWord,
        Delete,
        DeleteWord,
        Left,
        Right,
        WordLeft,
        WordRight,
        SelectLeft,
        SelectRight,
        SelectWordLeft,
        SelectWordRight,
        Up,
        Down,
        SelectUp,
        SelectDown,
        Home,
        End,
        SelectHome,
        SelectEnd,
        Top,
        Bottom,
        SelectTop,
        SelectBottom,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        Enter,
        Escape,
    ]
);

/// Puts the editing keys in GPUI's keymap, once at startup.
pub(crate) fn install(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("shift-backspace", Backspace, context),
        KeyBinding::new("ctrl-backspace", BackspaceWord, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("ctrl-delete", DeleteWord, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("ctrl-left", WordLeft, context),
        KeyBinding::new("ctrl-right", WordRight, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, context),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("shift-up", SelectUp, context),
        KeyBinding::new("shift-down", SelectDown, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("shift-home", SelectHome, context),
        KeyBinding::new("shift-end", SelectEnd, context),
        KeyBinding::new("ctrl-home", Top, context),
        KeyBinding::new("ctrl-end", Bottom, context),
        KeyBinding::new("ctrl-shift-home", SelectTop, context),
        KeyBinding::new("ctrl-shift-end", SelectBottom, context),
        KeyBinding::new("ctrl-a", SelectAll, context),
        KeyBinding::new("ctrl-c", Copy, context),
        KeyBinding::new("ctrl-x", Cut, context),
        KeyBinding::new("ctrl-v", Paste, context),
        KeyBinding::new("ctrl-z", Undo, context),
        KeyBinding::new("ctrl-y", Redo, context),
        KeyBinding::new("ctrl-shift-z", Redo, context),
        KeyBinding::new("enter", Enter, context),
        KeyBinding::new("shift-enter", Enter, context),
        KeyBinding::new("escape", Escape, context),
    ]);
}

pub(super) enum FieldEvent {
    /// The text changed (the caret with it).
    Changed,
    /// Only the caret or the selection moved.
    Moved,
    /// Enter in a single-line box: focus lost with Enter.
    Submit,
    /// Focus went elsewhere, or Escape.
    Blur,
}

pub(super) struct Field {
    pub(super) model: TextModel,
    pub(super) focus: FocusHandle,
    /// `MultiLine`: Enter (Shift+Enter too, measured in Studio) types a line
    /// break instead of ending the edit.
    pub(super) multi_line: bool,
    /// `TextEditable`: off, the text can be selected and copied only.
    pub(super) editable: bool,
    /// Where the last draw put the characters, in the widget's pixels, and
    /// where the widget sits in the window.
    pub(super) map: Option<GuiTextMap>,
    pub(super) origin: Point<Pixels>,
    /// The x Up and Down keep to, from the first of a run of them.
    sticky_x: Option<f32>,
    _blur: Subscription,
}

impl EventEmitter<FieldEvent> for Field {}

impl Field {
    pub(super) fn new(
        model: TextModel,
        multi_line: bool,
        editable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let blur = cx.on_blur(&focus, window, |_, _, cx| cx.emit(FieldEvent::Blur));
        Field {
            model,
            focus,
            multi_line,
            editable,
            map: None,
            origin: Point::default(),
            sticky_x: None,
            _blur: blur,
        }
    }

    fn moved(&mut self, cx: &mut Context<Self>) {
        self.sticky_x = None;
        cx.emit(FieldEvent::Moved);
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.sticky_x = None;
        cx.emit(FieldEvent::Changed);
    }

    fn step(&mut self, forward: bool, select: bool, word: bool, cx: &mut Context<Self>) {
        self.model.step(forward, select, word);
        self.moved(cx);
    }

    fn erase(&mut self, forward: bool, word: bool, cx: &mut Context<Self>) {
        if self.editable {
            self.model.erase(forward, word);
            self.changed(cx);
        }
    }

    /// The caret's line on the last draw, and the line's first and last stop.
    fn line_ends(&self) -> Option<(usize, usize)> {
        let map = self.map.as_ref()?;
        let line = &map.lines[map.line_of(self.model.caret)?];
        let first = line.stops.first()?.0;
        let mut last = line.stops.last()?.0;
        // A wrapped line ends where the next begins; End stops before the
        // space it wrapped at, so the caret stays on this line.
        let next_starts = map
            .lines
            .iter()
            .any(|l| l.stops.first().map(|s| s.0) == Some(last));
        if next_starts && last > first && self.model.text[..last].ends_with(char::is_whitespace) {
            last -= 1;
        }
        Some((first, last))
    }

    fn line_end(&mut self, end: bool, select: bool, cx: &mut Context<Self>) {
        let (first, last) = self.line_ends().unwrap_or((0, self.model.text.len()));
        self.model.move_to(if end { last } else { first }, select);
        self.moved(cx);
    }

    /// Up and Down, along the drawn lines: a box's lines past its height are
    /// not drawn and Studio's caret does not go there either.
    fn vertical(&mut self, down: bool, select: bool, cx: &mut Context<Self>) {
        let Some(map) = self.map.as_ref() else {
            return;
        };
        let Some(line) = map.line_of(self.model.caret) else {
            return;
        };
        let x = self
            .sticky_x
            .or_else(|| map.caret(self.model.caret).map(|c| c[0]));
        let target = match down {
            true => line + 1,
            false => line.wrapping_sub(1),
        };
        let Some((x, to)) = x.and_then(|x| Some((x, map.nearest(target, x)?))) else {
            return;
        };
        self.model.move_to(to, select);
        self.sticky_x = Some(x);
        cx.emit(FieldEvent::Moved);
    }

    fn edge(&mut self, end: bool, select: bool, cx: &mut Context<Self>) {
        let to = if end { self.model.text.len() } else { 0 };
        self.model.move_to(to, select);
        self.moved(cx);
    }

    fn type_text(&mut self, range: Option<Range<usize>>, text: &str, cx: &mut Context<Self>) {
        if !self.editable {
            return;
        }
        // A single-line box keeps to one line, as Roblox's does: a pasted
        // line break becomes nothing.
        let text = match self.multi_line {
            true => text.replace("\r\n", "\n").replace('\r', "\n"),
            false => text.replace(['\r', '\n'], ""),
        };
        self.model.replace(range, &text, true);
        self.changed(cx);
    }

    /// Wires every editing action onto `element`, the one that tracks the
    /// focus.
    pub(super) fn keys(field: &Entity<Field>, element: Div, window: &mut Window) -> Div {
        macro_rules! on {
            ($element:expr, $action:ty, $body:expr) => {
                $element.on_action(window.listener_for(
                    field,
                    move |this: &mut Field, _: &$action, window, cx| {
                        #[allow(clippy::redundant_closure_call)]
                        ($body)(this, window, cx)
                    },
                ))
            };
        }
        let element = element.key_context(CONTEXT);
        let element = on!(
            element,
            Backspace,
            |f: &mut Field, _, cx: &mut Context<Field>| f.erase(false, false, cx)
        );
        let element = on!(
            element,
            BackspaceWord,
            |f: &mut Field, _, cx: &mut Context<Field>| f.erase(false, true, cx)
        );
        let element = on!(
            element,
            Delete,
            |f: &mut Field, _, cx: &mut Context<Field>| f.erase(true, false, cx)
        );
        let element = on!(
            element,
            DeleteWord,
            |f: &mut Field, _, cx: &mut Context<Field>| f.erase(true, true, cx)
        );
        let element = on!(
            element,
            Left,
            |f: &mut Field, _, cx: &mut Context<Field>| f.step(false, false, false, cx)
        );
        let element = on!(
            element,
            Right,
            |f: &mut Field, _, cx: &mut Context<Field>| f.step(true, false, false, cx)
        );
        let element = on!(
            element,
            WordLeft,
            |f: &mut Field, _, cx: &mut Context<Field>| f.step(false, false, true, cx)
        );
        let element = on!(
            element,
            WordRight,
            |f: &mut Field, _, cx: &mut Context<Field>| f.step(true, false, true, cx)
        );
        let element = on!(
            element,
            SelectLeft,
            |f: &mut Field, _, cx: &mut Context<Field>| f.step(false, true, false, cx)
        );
        let element = on!(
            element,
            SelectRight,
            |f: &mut Field, _, cx: &mut Context<Field>| f.step(true, true, false, cx)
        );
        let element = on!(
            element,
            SelectWordLeft,
            |f: &mut Field, _, cx: &mut Context<Field>| f.step(false, true, true, cx)
        );
        let element = on!(
            element,
            SelectWordRight,
            |f: &mut Field, _, cx: &mut Context<Field>| f.step(true, true, true, cx)
        );
        let element = on!(element, Up, |f: &mut Field, _, cx: &mut Context<Field>| f
            .vertical(false, false, cx));
        let element = on!(
            element,
            Down,
            |f: &mut Field, _, cx: &mut Context<Field>| f.vertical(true, false, cx)
        );
        let element = on!(
            element,
            SelectUp,
            |f: &mut Field, _, cx: &mut Context<Field>| f.vertical(false, true, cx)
        );
        let element = on!(
            element,
            SelectDown,
            |f: &mut Field, _, cx: &mut Context<Field>| f.vertical(true, true, cx)
        );
        let element = on!(
            element,
            Home,
            |f: &mut Field, _, cx: &mut Context<Field>| f.line_end(false, false, cx)
        );
        let element = on!(element, End, |f: &mut Field, _, cx: &mut Context<Field>| f
            .line_end(true, false, cx));
        let element = on!(
            element,
            SelectHome,
            |f: &mut Field, _, cx: &mut Context<Field>| f.line_end(false, true, cx)
        );
        let element = on!(
            element,
            SelectEnd,
            |f: &mut Field, _, cx: &mut Context<Field>| f.line_end(true, true, cx)
        );
        let element = on!(element, Top, |f: &mut Field, _, cx: &mut Context<Field>| f
            .edge(false, false, cx));
        let element = on!(
            element,
            Bottom,
            |f: &mut Field, _, cx: &mut Context<Field>| f.edge(true, false, cx)
        );
        let element = on!(
            element,
            SelectTop,
            |f: &mut Field, _, cx: &mut Context<Field>| f.edge(false, true, cx)
        );
        let element = on!(
            element,
            SelectBottom,
            |f: &mut Field, _, cx: &mut Context<Field>| f.edge(true, true, cx)
        );
        let element = on!(
            element,
            SelectAll,
            |f: &mut Field, _, cx: &mut Context<Field>| {
                f.model.select(0..f.model.text.len());
                f.moved(cx);
            }
        );
        let element = on!(
            element,
            Copy,
            |f: &mut Field, _, cx: &mut Context<Field>| {
                if !f.model.selected().is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        f.model.selected().to_string(),
                    ));
                }
            }
        );
        let element = on!(element, Cut, |f: &mut Field, _, cx: &mut Context<Field>| {
            if f.editable && !f.model.selected().is_empty() {
                cx.write_to_clipboard(ClipboardItem::new_string(f.model.selected().to_string()));
                f.model.replace(None, "", false);
                f.changed(cx);
            }
        });
        let element = on!(
            element,
            Paste,
            |f: &mut Field, _, cx: &mut Context<Field>| {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    f.type_text(None, &text, cx);
                }
            }
        );
        let element = on!(
            element,
            Undo,
            |f: &mut Field, _, cx: &mut Context<Field>| {
                if f.editable && f.model.undo() {
                    f.changed(cx);
                }
            }
        );
        let element = on!(
            element,
            Redo,
            |f: &mut Field, _, cx: &mut Context<Field>| {
                if f.editable && f.model.redo() {
                    f.changed(cx);
                }
            }
        );
        let element = on!(
            element,
            Enter,
            |f: &mut Field, _, cx: &mut Context<Field>| {
                match f.multi_line {
                    true => f.type_text(None, "\n", cx),
                    false => cx.emit(FieldEvent::Submit),
                }
            }
        );
        on!(
            element,
            Escape,
            |_: &mut Field, _, cx: &mut Context<Field>| cx.emit(FieldEvent::Blur)
        )
    }
}

/// UTF-16 offsets, which the platform's text input speaks, to and from the
/// text's byte offsets.
fn to_utf16(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].encode_utf16().count()
}

fn from_utf16(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (at, c) in text.char_indices() {
        if units >= offset {
            return at;
        }
        units += c.len_utf16();
    }
    text.len()
}

impl EntityInputHandler for Field {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text = &self.model.text;
        let bytes = from_utf16(text, range.start)..from_utf16(text, range.end);
        *adjusted = Some(to_utf16(text, bytes.start)..to_utf16(text, bytes.end));
        Some(text[bytes].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let text = &self.model.text;
        let range = self.model.selection();
        Some(UTF16Selection {
            range: to_utf16(text, range.start)..to_utf16(text, range.end),
            reversed: self.model.caret < self.model.anchor,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let model = &self.model.text;
        let range = range.map(|r| from_utf16(model, r.start)..from_utf16(model, r.end));
        self.type_text(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_text_in_range(range, text, window, cx);
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        element: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let Some([x, top, height]) = self
            .map
            .as_ref()
            .and_then(|map| map.caret(self.model.caret))
        else {
            return Some(element);
        };
        Some(Bounds::new(
            self.origin + point(px(x), px(top)),
            size(px(1.0), px(height)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        at: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let local = at - self.origin;
        let offset = self
            .map
            .as_ref()?
            .hit([f32::from(local.x), f32::from(local.y)])?;
        Some(to_utf16(&self.model.text, offset))
    }
}

#[cfg(test)]
mod tests {
    use super::{from_utf16, to_utf16};

    #[test]
    fn utf16_offsets_round_trip_through_wide_characters() {
        let text = "a😀b";
        assert_eq!(to_utf16(text, 5), 3);
        assert_eq!(from_utf16(text, 3), 5);
        assert_eq!(from_utf16(text, 99), text.len());
    }
}
