//! The `NumberSequence`/`ColorSequence` graph: a second window of the
//! editor's own, opened from a sequence row's drawing of itself.
//!
//! A real window rather than a layer over the docks, because that is what
//! the thing is — it floats above the editor (`WindowKind::Floating`, which
//! is `WM_TRANSIENT_FOR` on X11), it moves where you put it, and the
//! viewport it is steering stays visible behind it. Fixed size, though: the
//! plot is the content, and there is nothing in here that wants more room
//! than it was given. It draws the editor's own title bar rather than the
//! platform's (`shell::chrome::panel_topbar`), so it reads as part of this
//! application and not as a dialog from somewhere else.
//!
//! **This window holds no copy of the value.** Every frame it rebuilds a
//! [`Editor`] from whatever the `Shell`'s DOM holds, and every edit leaves
//! through `Shell::commit_row_step` — the same textual path a typed row
//! takes. So the viewport repaints on each drag step, an undo or a Command
//! Bar script shows up in the graph immediately, and a whole drag is still
//! one undo entry (the discipline `shell::drag` already follows).

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::color_picker::{ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::Root;
use gpui_kit::*;

use rbx_dom::Color3Data;

use crate::properties;
use crate::sequence_editor::{Drag, Editor, Field, Handle, Kind, Rect};
use crate::shell::Shell;
use crate::tokens;

mod paint;
mod view;

/// The window's fixed *content* size, by what it has to hold: a curve wants
/// height to be read as a curve, a ramp carries one dimension and needs
/// almost none. Wide enough that keypoints a few hundredths apart are
/// separable by eye and by pointer, which is the whole reason this is not a
/// text field.
const CONTENT_WIDTH: f32 = 620.0;
const CURVE_CONTENT_HEIGHT: f32 = 400.0;
const RAMP_CONTENT_HEIGHT: f32 = 210.0;

/// What the toolkit's client-side window frame takes off every side before
/// the content gets any (`gpui_component::window_border::SHADOW_SIZE`, 20px
/// on Linux) — the same rounded shadow the main window has. That constant is
/// not public, so the allowance is spelled here rather than read: a window
/// sized to its content alone clips its own footer, which is exactly what
/// the first attempt did.
const WINDOW_CHROME: f32 = 40.0;

/// The panel's own fields, in the order [`sequence_fields`] hands them out.
/// A colour stop only has the first.
const FIELDS: [(Field, &str); 3] = [
    (Field::Time, "Time"),
    (Field::Value, "Value"),
    (Field::Envelope, "Envelope"),
];

/// One sequence drawn small and inert, for the row that opens this window —
/// the same painters the graph uses, with the grid and the handles off, so
/// a row can never disagree with the graph about the shape of a value.
pub(crate) fn preview(
    kind: Kind,
    stops: Vec<crate::sequence_editor::Stop>,
    ceiling: f32,
) -> impl IntoElement {
    paint::plot(kind, stops, ceiling, paint::Look::preview(), None)
}

/// Which footer fields a sequence of this kind has. A colour stop carries a
/// time and a colour and nothing else — a `ColorSequenceKeypoint`'s envelope
/// has no effect in Roblox's engine, so a field for it would be a control
/// that does nothing.
pub(crate) fn sequence_fields(kind: Kind) -> &'static [(Field, &'static str)] {
    match kind {
        Kind::Number => &FIELDS,
        Kind::Color => &FIELDS[..1],
    }
}

/// One open graph. `row` is the `PropertyRow::name` every edit commits
/// through, which is what lets the same window edit an ordinary property and
/// an `Attribute:`-prefixed one without knowing the difference.
pub(crate) struct SequenceWindow {
    shell: Entity<Shell>,
    row: String,
    title: String,
    kind: Kind,
    /// What **Reset** goes back to, as the commit text the row held when
    /// this window opened — the sequence already in the file, not the type's
    /// default, because that is the thing somebody is afraid of losing while
    /// they drag. Committed like any other edit, so Reset is undoable too.
    original: String,
    selected: usize,
    drag: Option<Drag>,
    /// The value axis as it stood when the current drag began.
    ///
    /// Frozen for the whole gesture on purpose. The axis is fitted to the
    /// stops (see [`Editor::ceiling`]), so letting it move mid-drag makes the
    /// pointer-to-value mapping depend on its own output: a pointer held
    /// still above the plot would read a larger value each frame, which
    /// would grow the axis, which would read a larger value again. Frozen,
    /// dragging above the top edge is simply a value above the ceiling — the
    /// axis refits around it the moment the drag ends.
    drag_ceiling: Option<f32>,
    /// Whether the press currently in flight began on the title bar — see
    /// `shell::panel_topbar`. Cleared by [`Self::begin_drag`], because a
    /// keypoint dragged up out of the plot passes straight over that bar.
    title_grab: Rc<Cell<bool>>,
    /// Set by the plot's own layout every frame — the only thing that can
    /// turn a pointer position into a `(time, value)`, and `None` until the
    /// window has been laid out once.
    plot: Rc<Cell<Option<Rect>>>,
    fields: Vec<Entity<InputState>>,
    color: Option<Entity<ColorPickerState>>,
    _subscriptions: Vec<Subscription>,
}

impl SequenceWindow {
    /// Opens the graph for `row`, on the sequence the caller has already
    /// read out of the DOM.
    ///
    /// Everything about the value arrives as an argument rather than being
    /// read back off `shell` here: this is called while that entity is
    /// already borrowed (see `Shell::open_sequence_editor`), and reading it
    /// again is a panic, not a borrow error the compiler would have caught.
    pub(crate) fn open(
        shell: Entity<Shell>,
        row: String,
        title: String,
        original: String,
        editor: Editor,
        cx: &mut App,
    ) -> Option<WindowHandle<Root>> {
        let content_height = match editor.kind {
            Kind::Number => CURVE_CONTENT_HEIGHT,
            Kind::Color => RAMP_CONTENT_HEIGHT,
        };
        let window_size = size(
            tokens::scaled_width(CONTENT_WIDTH + WINDOW_CHROME),
            tokens::scaled_width(content_height + WINDOW_CHROME),
        );
        let bounds = Bounds::centered(None, window_size, cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            // Floats above the editor it is steering (`WM_TRANSIENT_FOR` on
            // X11): a graph the compositor can drop behind the main window
            // is one you cannot compare a gradient against.
            kind: WindowKind::Floating,
            // Moved by its own title bar (see `shell::panel_topbar`), never
            // resized: the plot is the content, and nothing in here wants
            // more room than it was given.
            //
            // `is_resizable` is honoured on macOS and Windows. GPUI's X11
            // backend does not read it at all — it always writes a
            // `WM_NORMAL_HINTS` maximum of the GPU's texture limit — so on
            // X11 a window manager can still grow this window. The minimum
            // below *is* honoured there, which stops the one direction that
            // would clip the footer, and the plot takes any extra height
            // rather than leaving dead space (see `view`), so a forced
            // resize gives a bigger graph rather than a broken one.
            is_resizable: false,
            is_minimizable: false,
            window_min_size: Some(window_size),
            app_owns_titlebar_drag: true,
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from(title.clone())),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            ..Default::default()
        };

        cx.open_window(options, move |window, cx| {
            let view =
                cx.new(|cx| SequenceWindow::new(shell, row, title, original, &editor, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        shell: Entity<Shell>,
        row: String,
        title: String,
        original: String,
        editor: &Editor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let kind = editor.kind;
        let mut fields = Vec::new();
        let mut subscriptions = Vec::new();
        for &(field, _) in sequence_fields(kind) {
            let seed = field_text(editor, field);
            let input = cx.new(|cx| InputState::new(window, cx).default_value(seed));
            subscriptions.push(
                cx.subscribe(&input, move |view, input, event: &InputEvent, cx| {
                    if !matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        return;
                    }
                    let text = input.read(cx).value().to_string();
                    view.edit(true, |editor| editor.set_field(field, &text), cx);
                    // Notified whether or not it was accepted: a refused
                    // value has to snap back to what the stop actually holds
                    // rather than sit there looking committed.
                    cx.notify();
                }),
            );
            fields.push(input);
        }

        let color = (kind == Kind::Color).then(|| {
            let stop = editor.selected_stop();
            let state =
                cx.new(|cx| ColorPickerState::new(window, cx).default_value(hsla_of(stop.color)));
            subscriptions.push(cx.subscribe(
                &state,
                move |view, _, event: &ColorPickerEvent, cx| {
                    let ColorPickerEvent::Change(Some(picked)) = event else {
                        return;
                    };
                    let rgba = picked.to_rgb();
                    let color = Color3Data {
                        r: rgba.r,
                        g: rgba.g,
                        b: rgba.b,
                    };
                    view.edit(
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

        SequenceWindow {
            shell,
            row,
            title,
            kind,
            original,
            selected: 0,
            drag: None,
            drag_ceiling: None,
            title_grab: Rc::new(Cell::new(false)),
            plot: Rc::new(Cell::new(None)),
            fields,
            color,
            _subscriptions: subscriptions,
        }
    }

    /// The sequence as the DOM holds it *now*, with this window's selection
    /// and in-flight drag laid over it. Rebuilt per call rather than cached:
    /// that is what keeps the graph honest about a value anything else may
    /// have moved (see this module's doc comment).
    fn editor(&self, cx: &App) -> Option<Editor> {
        let (color, text) = self.shell.read(cx).sequence_row(&self.row)?;
        let value = properties::edit::sequence_value(color, &text)?;
        let mut editor = Editor::open(&value)?;
        editor.selected = self.selected.min(editor.stops.len().saturating_sub(1));
        editor.drag = self.drag;
        Some(editor)
    }

    /// The value axis this frame: whatever the stops fit, unless a drag is
    /// in flight — see [`Self::drag_ceiling`].
    fn ceiling(&self, editor: &Editor) -> f32 {
        self.drag_ceiling.unwrap_or_else(|| editor.ceiling())
    }

    /// Applies one change to the sequence the DOM currently holds and writes
    /// the result back. `push` is false for every step of a drag after the
    /// first, which is what keeps a gesture to one undo entry.
    fn edit(
        &mut self,
        push: bool,
        change: impl FnOnce(&mut Editor) -> bool,
        cx: &mut Context<Self>,
    ) {
        let Some(mut editor) = self.editor(cx) else {
            return;
        };
        if !change(&mut editor) {
            return;
        }
        self.selected = editor.selected;
        self.drag = editor.drag;
        let (row, text) = (self.row.clone(), editor.text());
        self.shell
            .update(cx, |shell, cx| shell.commit_row_step(&row, &text, push, cx));
        cx.notify();
    }

    /// Pointer down inside the plot: grab the handle under it, or add a stop
    /// where there is none. Either way the stop under the cursor becomes the
    /// selected one, which is what the footer then edits.
    fn begin_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(editor) = self.editor(cx) else {
            return;
        };
        let Some(rect) = self.plot.get() else {
            return;
        };
        let ceiling = editor.ceiling();
        let (time, value) = rect.at(f32::from(position.x), f32::from(position.y), ceiling);
        self.drag_ceiling = Some(ceiling);
        self.title_grab.set(false);

        match editor.grab(time, value) {
            // Selecting a stop changes nothing in the DOM, so it is not an
            // edit and must not push an undo entry of its own.
            Some(drag) => {
                self.selected = drag.index;
                self.drag = Some(drag);
                cx.notify();
            }
            // A click on empty plot adds a stop there and picks it up, so one
            // press both creates and places it. Ctrl+Z takes it back, since
            // this commits like any other edit.
            None => self.edit(
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

    fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.drag.is_none() {
            return;
        }
        let (Some(rect), Some(ceiling)) = (self.plot.get(), self.drag_ceiling) else {
            return;
        };
        let (time, value) = rect.at(f32::from(position.x), f32::from(position.y), ceiling);
        self.edit(
            false,
            |editor| {
                editor.drag_to(time, value);
                true
            },
            cx,
        );
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            // Dropping the frozen axis is what lets it refit around a value
            // the drag pushed past the old ceiling.
            self.drag_ceiling = None;
            cx.notify();
        }
    }

    fn delete_stop(&mut self, cx: &mut Context<Self>) {
        self.edit(true, |editor| editor.remove_selected(), cx);
    }

    /// Back to the sequence this window opened on, through the same commit
    /// path everything else here takes — so Reset is one more undo step
    /// rather than a trapdoor out of the history.
    fn reset(&mut self, cx: &mut Context<Self>) {
        self.selected = 0;
        self.drag = None;
        self.drag_ceiling = None;
        let (row, original) = (self.row.clone(), self.original.clone());
        self.shell.update(cx, |shell, cx| {
            shell.commit_row_step(&row, &original, true, cx)
        });
        cx.notify();
    }
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
    crate::shell::format_scrubbed(value)
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
#[path = "sequence_window/tests.rs"]
mod tests;
