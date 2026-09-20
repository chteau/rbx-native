//! The `NumberSequence`/`ColorSequence` editor: a floating graph over the
//! editor, opened from a sequence row's own drawing of itself.
//!
//! Studio puts this in a second OS window. It is one panel here, laid over
//! the docks and wearing the same title bar the window itself does
//! (`shell::chrome::panel_topbar`), for a reason that is not just plumbing:
//! the thing being edited is a *gradient on screen*, and a window the
//! compositor can drop behind the editor is a window you cannot compare
//! against what it changes. Pinned to the bottom of the editor, the viewport
//! above it stays visible for the whole drag.
//!
//! **The panel holds no copy of the value.** Every frame it rebuilds a
//! `sequence_editor::Editor` from whatever the DOM holds right now, and
//! every edit leaves through [`Shell::commit_row_step`] — the same textual
//! path a typed row takes. So an undo, a Command Bar script, or a second
//! edit of the same property all show up in the graph immediately, the
//! viewport sees each drag step through the ordinary change log, and a whole
//! drag still costs exactly one undo step (see `shell::drag`, which pushes
//! history the same way).

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::color_picker::{ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rbx_dom::Color3Data;

use crate::properties::{self, EditKind};
use crate::sequence_editor::{Drag, Editor, Field, Handle, Kind, Rect};

use super::Shell;

mod paint;
mod view;

/// The panel's own fields, in the order [`sequence_fields`] hands them out.
/// A colour stop only has the first.
const FIELDS: [(Field, &str); 3] = [
    (Field::Time, "Time"),
    (Field::Value, "Value"),
    (Field::Envelope, "Envelope"),
];

/// One open editor: which row it edits, what the user has selected in it,
/// and the widgets its footer needs. No stops — see this module's doc.
pub(super) struct Open {
    row: String,
    title: String,
    /// What **Reset** goes back to, as the commit text the row held when the
    /// panel opened — the sequence already in the file, not the type's
    /// default, because that is the thing somebody is afraid of losing while
    /// they drag. Committed like any other edit, so Reset is undoable too.
    original: String,
    selected: usize,
    drag: Option<Drag>,
    /// Set by the plot's own layout every frame — the only thing that can
    /// turn a pointer position into a `(time, value)`, and `None` until the
    /// panel has been laid out once.
    plot: Rc<Cell<Option<Rect>>>,
    fields: Vec<Entity<InputState>>,
    color: Option<Entity<ColorPickerState>>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    /// Opens the graph for the sequence row named `row`, reading its value
    /// out of the row's own commit text — so an attribute, which is not a
    /// DOM property at all, opens through exactly the same path.
    pub(super) fn open_sequence_editor(
        &mut self,
        row: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((color, text)) = self.sequence_row(row) else {
            return;
        };
        let Some(value) = properties::edit::sequence_value(color, &text) else {
            return;
        };
        let Some(editor) = Editor::open(&value) else {
            return;
        };
        self.sequence = Some(self.build_sequence_panel(
            row.to_owned(),
            self.sequence_title(row),
            editor.kind,
            text,
            &editor,
            window,
            cx,
        ));
        cx.notify();
    }

    pub(super) fn close_sequence_editor(&mut self, cx: &mut Context<Self>) {
        self.sequence = None;
        cx.notify();
    }

    /// The sequence as the DOM holds it *now*, with the panel's selection
    /// and in-flight drag laid over it. Rebuilt per call rather than cached:
    /// that is what keeps the graph honest about a value anything else may
    /// have moved (see this module's doc comment).
    fn sequence_editor(&self) -> Option<Editor> {
        let open = self.sequence.as_ref()?;
        let (color, text) = self.sequence_row(&open.row)?;
        let value = properties::edit::sequence_value(color, &text)?;
        let mut editor = Editor::open(&value)?;
        editor.selected = open.selected.min(editor.stops.len().saturating_sub(1));
        editor.drag = open.drag;
        Some(editor)
    }

    /// The row's `EditKind`, found the same way the Properties panel itself
    /// finds a row rather than kept in a second place that could go stale.
    fn sequence_row(&self, row: &str) -> Option<(bool, String)> {
        let reference = self.selected()?;
        let kind = if let Some(attribute) = properties::attributes::attribute_of_row(row) {
            let value = properties::attributes::attributes(&self.dom, reference)
                .get(attribute)?
                .clone();
            properties::attributes::edit_kind(&value)?
        } else {
            let folder_color = self.folder_color(reference);
            self.properties
                .rows(&self.dom, reference, folder_color)
                .into_iter()
                .find(|candidate| candidate.name == row)
                .and_then(|candidate| candidate.edit)?
        };
        match kind {
            EditKind::Sequence { color, text } => Some((color, text)),
            _ => None,
        }
    }

    /// `ParticleEmitter.Size` — the instance's class and the property, the
    /// way Studio titles the same window. An attribute row drops its
    /// `Attribute:` prefix and reads as the attribute's name.
    fn sequence_title(&self, row: &str) -> String {
        let name = properties::attributes::attribute_of_row(row).unwrap_or(row);
        let class = self
            .selected()
            .and_then(|reference| self.dom.get(reference))
            .map(|instance| instance.class().to_owned());
        match class {
            Some(class) => format!("{class}.{name}"),
            None => name.to_owned(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_sequence_panel(
        &mut self,
        row: String,
        title: String,
        kind: Kind,
        original: String,
        editor: &Editor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Open {
        let mut fields = Vec::new();
        let mut subscriptions = Vec::new();
        for &(field, _) in sequence_fields(kind) {
            let seed = field_text(editor, field);
            let input = cx.new(|cx| InputState::new(window, cx).default_value(seed));
            subscriptions.push(cx.subscribe(
                &input,
                move |shell, input, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        return;
                    }
                    let text = input.read(cx).value().to_string();
                    shell.edit_sequence(true, |editor| editor.set_field(field, &text), cx);
                    // Resynced whether or not it was accepted: a refused
                    // value has to snap back to what the stop actually holds
                    // rather than sit there looking committed.
                    cx.notify();
                },
            ));
            fields.push(input);
        }

        let color = (kind == Kind::Color).then(|| {
            let stop = editor.selected_stop();
            let state =
                cx.new(|cx| ColorPickerState::new(window, cx).default_value(hsla_of(stop.color)));
            subscriptions.push(cx.subscribe(
                &state,
                move |shell, _, event: &ColorPickerEvent, cx| {
                    let ColorPickerEvent::Change(Some(picked)) = event else {
                        return;
                    };
                    let rgba = picked.to_rgb();
                    let color = Color3Data {
                        r: rgba.r,
                        g: rgba.g,
                        b: rgba.b,
                    };
                    shell.edit_sequence(
                        true,
                        |editor| {
                            editor.set_color(color);
                            true
                        },
                        cx,
                    );
                },
            ));
            state
        });

        Open {
            row,
            title,
            original,
            selected: 0,
            drag: None,
            plot: Rc::new(Cell::new(None)),
            fields,
            color,
            _subscriptions: subscriptions,
        }
    }

    /// Applies one change to the sequence the DOM currently holds and writes
    /// the result back. `push` is false for every step of a drag after the
    /// first, which is what keeps a gesture to one undo entry.
    fn edit_sequence(
        &mut self,
        push: bool,
        change: impl FnOnce(&mut Editor) -> bool,
        cx: &mut Context<Self>,
    ) {
        let Some(mut editor) = self.sequence_editor() else {
            return;
        };
        if !change(&mut editor) {
            return;
        }
        let text = editor.text();
        let Some(open) = &mut self.sequence else {
            return;
        };
        open.selected = editor.selected;
        open.drag = editor.drag;
        let row = open.row.clone();
        self.commit_row_step(&row, &text, push, cx);
    }

    /// Pointer down inside the plot: grab the handle under it, or add a stop
    /// where there is none. Either way the stop under the cursor becomes the
    /// selected one, which is what the footer then edits.
    pub(super) fn begin_sequence_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(editor) = self.sequence_editor() else {
            return;
        };
        let Some(rect) = self.sequence.as_ref().and_then(|open| open.plot.get()) else {
            return;
        };
        let (time, value) = rect.at(
            f32::from(position.x),
            f32::from(position.y),
            editor.ceiling(),
        );

        match editor.grab(time, value) {
            // Selecting a stop changes nothing in the DOM, so it is not an
            // edit and must not push an undo entry of its own.
            Some(drag) => {
                if let Some(open) = &mut self.sequence {
                    open.selected = drag.index;
                    open.drag = Some(drag);
                }
                cx.notify();
            }
            // A click on empty plot adds a stop there and picks it up, so one
            // press both creates and places it. Ctrl+Z takes it back, since
            // this commits like any other edit.
            None => self.edit_sequence(
                true,
                |editor| {
                    if !editor.insert(time) {
                        return false;
                    }
                    editor.drag = Some(Drag {
                        index: editor.selected,
                        handle: Handle::Point,
                    });
                    true
                },
                cx,
            ),
        }
    }

    /// One step of that drag, from the mouse-move listeners on the window
    /// and on the panel itself (see `view`, and `shell::scrub` for the same
    /// window-level routing).
    pub(super) fn drag_sequence(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(open) = self.sequence.as_ref() else {
            return;
        };
        if open.drag.is_none() {
            return;
        }
        let Some(rect) = open.plot.get() else {
            return;
        };
        let Some(ceiling) = self.sequence_editor().map(|editor| editor.ceiling()) else {
            return;
        };
        let (time, value) = rect.at(f32::from(position.x), f32::from(position.y), ceiling);
        self.edit_sequence(
            false,
            |editor| {
                editor.drag_to(time, value);
                true
            },
            cx,
        );
    }

    pub(super) fn end_sequence_drag(&mut self) {
        if let Some(open) = &mut self.sequence {
            open.drag = None;
        }
    }

    pub(super) fn delete_sequence_stop(&mut self, cx: &mut Context<Self>) {
        self.edit_sequence(true, |editor| editor.remove_selected(), cx);
    }

    /// Back to the sequence the panel opened on, through the same commit
    /// path everything else here takes — so Reset is one more undo step
    /// rather than a trapdoor out of the history.
    pub(super) fn reset_sequence(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &mut self.sequence else {
            return;
        };
        open.selected = 0;
        open.drag = None;
        let (row, original) = (open.row.clone(), open.original.clone());
        self.commit_row_step(&row, &original, true, cx);
    }
}

/// Which footer fields a sequence of this kind has. A colour stop carries a
/// time and a colour and nothing else — a `ColorSequenceKeypoint`'s envelope
/// has no effect in Roblox's engine, so a field for it would be a control
/// that does nothing.
pub(super) fn sequence_fields(kind: Kind) -> &'static [(Field, &'static str)] {
    match kind {
        Kind::Number => &FIELDS,
        Kind::Color => &FIELDS[..1],
    }
}

/// One sequence drawn small and inert, for the row that opens this panel —
/// the same painters the graph uses, with the grid and the handles off, so
/// a row can never disagree with the graph about the shape of a value.
pub(super) fn preview(
    kind: Kind,
    stops: Vec<crate::sequence_editor::Stop>,
    ceiling: f32,
) -> impl IntoElement {
    paint::plot(kind, stops, ceiling, paint::Look::preview(), None)
}

fn field_text(editor: &Editor, field: Field) -> String {
    let stop = editor.selected_stop();
    let value = match field {
        Field::Time => stop.time,
        Field::Value => stop.value,
        Field::Envelope => stop.envelope,
    };
    // Three decimals, trailing zeros trimmed — the same reading
    // `shell::scrub::format` gives a dragged number, so a value that arrived
    // by drag and one that was typed look alike.
    super::scrub::format(value, crate::properties::FieldKind::Decimal)
}

fn hsla_of(color: Color3Data) -> Hsla {
    Rgba {
        r: color.r,
        g: color.g,
        b: color.b,
        a: 1.0,
    }
    .into()
}

#[cfg(test)]
#[path = "sequence_panel/tests.rs"]
mod tests;
