//! Editing one Properties row in place: a widget per row, picked by
//! `EditKind` and kept alive only while that row is on screen, committed
//! through the exact DOM take/put-back discipline the Command Bar uses (see
//! `shell::command`). Every widget round-trips through the same textual
//! commit path `properties::edit::commit` already used for plain text
//! fields — a `Checkbox` writes `"true"`/`"false"`, a `ColorPicker` writes
//! `"r, g, b"`, a `Select` writes the enum member's name, and a row of
//! numeric fields writes its values joined the same way `edit::edit_text`
//! already joins them. No second mutation path is introduced.
//!
//! `RBX_STUDIO_EDIT='Prop=value'` applies one edit to the selected instance
//! right after startup, through this same commit path — a debugging aid for
//! a screenshot that proves a written property reaches the viewport, since
//! nothing else can type into the panel on the editor's behalf (see
//! `AGENTS.md`'s safety rules).

use std::collections::{HashMap, HashSet};

use gpui_kit::component::color_picker::{ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::select::{SearchableVec, SelectEvent, SelectState};
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::component::IndexPath;
use gpui_kit::*;
use rbx_dom::WeakDom;

use crate::properties::{
    self, edit::NAME_PROPERTY, slider_range, EditKind, Field, FieldGroup, FieldKind, PropertyRow,
};

use super::Shell;

/// Read once at startup by `main`; documented in this module's doc comment.
pub(crate) const EDIT_VARIABLE: &str = "RBX_STUDIO_EDIT";

/// The dropdown list an `EditKind::Enum` row's `Select` is built from: plain
/// member names, since the value a row commits is read back out of the
/// picked label itself (same trick `shell::quality`'s dropdown uses).
pub(super) type EnumOptions = SearchableVec<SharedString>;

/// One row's live widget entity (or entities, for a multi-field row).
/// `Clone` is cheap — every variant only holds `Entity` handles.
#[derive(Clone)]
pub(super) enum RowEditor {
    Text(Entity<InputState>),
    /// The same field, plus a rail for the values that have somewhere to
    /// run between (see `properties::ranges`). The field stays the one
    /// that commits — the rail only writes into it — so a value outside
    /// the rail's reach is still typeable.
    Slider(Entity<InputState>, Entity<SliderState>),
    /// One `Input` per label, in the same order as `EditKind::Fields`'
    /// `labels` — carried alongside so `shell::rows` can pair each field
    /// with its caption without reaching back into `EditKind`, plus the
    /// **summary** input that holds the same value whole (see
    /// [`Self::summary`]).
    Fields(
        &'static [Field],
        Entity<InputState>,
        Vec<Entity<InputState>>,
    ),
    /// The same inputs, but split into captioned lines — see
    /// `properties::FieldGroup`. One flat list, in group order, because the
    /// commit path joins them all into one string regardless.
    Groups(
        &'static [FieldGroup],
        Entity<InputState>,
        Vec<Entity<InputState>>,
    ),
    /// A checkbox per named flag, and the flags as they currently stand.
    /// No entities: a checkbox has no editing state of its own, so the row
    /// commits straight from the click (see `shell::panels`).
    Flags(&'static [&'static str], Vec<bool>),
    Color(Entity<ColorPickerState>),
    Enum(Entity<SelectState<EnumOptions>>),
    /// Whether the value is there, and the editor for it — drawn under the
    /// present/absent checkbox only while it is. Built (and kept) either
    /// way, so the flag flipping is all that changes.
    Optional(bool, &'static str, Box<RowEditor>),
    /// A sequence's own drawing, which is also the button that opens
    /// `crate::sequence_window`. No entity: there is nothing to type into,
    /// and the panel it opens owns whatever state an edit needs.
    Sequence {
        color: bool,
        text: String,
    },
}

impl RowEditor {
    /// The `InputState` behind one numeric field of this editor, if it has
    /// one there.
    pub(super) fn field(&self, index: usize) -> Option<&Entity<InputState>> {
        match self {
            RowEditor::Fields(_, _, inputs) | RowEditor::Groups(_, _, inputs) => inputs.get(index),
            RowEditor::Optional(_, _, inner) => inner.field(index),
            _ => None,
        }
    }

    /// The one `Input` holding this value whole — `"0, 5, 0"` beside the
    /// row's expander, the way Studio keeps a `Vector3`'s own field
    /// editable while its components are showing.
    ///
    /// Deliberately **not** recursive into [`Self::Optional`]: an optional
    /// draws its checkbox and its inner fields as one stacked block, so it
    /// has no expander to put a summary beside, and the inner editor's own
    /// summary is never rendered.
    pub(super) fn summary(&self) -> Option<&Entity<InputState>> {
        match self {
            RowEditor::Fields(_, summary, _) | RowEditor::Groups(_, summary, _) => Some(summary),
            _ => None,
        }
    }

    /// The text this editor's own `Input`s currently hold, joined the way
    /// [`crate::properties::edit::parse`] reads them back. `None` for an
    /// editor that commits from its own event instead (see
    /// [`Shell::build_row_widget`]).
    fn input_text(&self, cx: &App) -> Option<String> {
        match self {
            RowEditor::Text(input) | RowEditor::Slider(input, _) => {
                Some(input.read(cx).value().to_string())
            }
            RowEditor::Fields(_, _, inputs) | RowEditor::Groups(_, _, inputs) => Some(
                inputs
                    .iter()
                    .map(|input| input.read(cx).value().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            RowEditor::Optional(_, _, inner) => inner.input_text(cx),
            RowEditor::Color(_)
            | RowEditor::Enum(_)
            | RowEditor::Flags(..)
            | RowEditor::Sequence { .. } => None,
        }
    }

    /// Whether this editor needs the row's **whole width** rather than its
    /// value column: a `Faces`' six checkboxes, a `PhysicalProperties`'
    /// checkbox-over-fields, a sequence's own strip. None of the three has
    /// a single value to put on the row's own line, so the name goes above
    /// them instead.
    ///
    /// Numeric values are not in the list any more — they keep the ordinary
    /// name/value row and hang their components off an expander (see
    /// [`Self::summary`] and `shell::rows::property_expandable`).
    pub(super) fn is_composite(&self) -> bool {
        matches!(
            self,
            RowEditor::Flags(..) | RowEditor::Optional(..) | RowEditor::Sequence { .. }
        )
    }
}

/// One row's open editor: its widget, the subscriptions that react to it,
/// and the last commit's error (if any), which the row renders below the
/// field until the value changes and commits cleanly.
pub(super) struct RowEdit {
    widget: RowEditor,
    _subscriptions: Vec<Subscription>,
    error: Option<String>,
}

/// Every row currently open for editing, plus which category sections the
/// user has manually collapsed. Bundled into one type — rather than adding
/// a second field to `Shell` in `shell.rs`, which is off limits here (see
/// `AGENTS.md`) — so `Shell::edits`' existing type alone carries both.
#[derive(Default)]
pub(super) struct Edits {
    rows: HashMap<String, RowEdit>,
    collapsed: HashSet<String>,
    /// Which numeric rows are showing their components. Collapsed is the
    /// default — a `BasePart` has five of these, and thirty extra fields
    /// laid out and painted every frame is thirty too many on the hardware
    /// this editor is meant to stay usable on.
    expanded: HashSet<String>,
    /// The row whose slider is being dragged, while it is.
    ///
    /// A commit normally drops the row's widget so the next render rebuilds
    /// it from what the DOM made of the value. That is exactly wrong here:
    /// the widget being dropped is the one holding the gesture, and the
    /// drag would end on its own first frame. So this names the row that
    /// has to survive its own commits — and, since it is set before the
    /// first of them, doubles as "is this the step that opens the undo
    /// entry" (see `Shell::slide_row`).
    sliding: Option<String>,
}

impl Edits {
    /// Drops every open row editor. Called on a selection change (see
    /// `shell.rs`); category collapse state is a panel-wide preference, not
    /// per-instance, so it deliberately survives this.
    pub(super) fn clear(&mut self) {
        self.rows.clear();
    }
}

/// Keeps a cached row widget's text in step with the DOM value that just
/// produced `kind`. Without this, `edit_row`'s cache — needed so an
/// in-progress keystroke survives the panel rebuilding around it — would
/// also hide any value that changes from outside the row itself, such as
/// `Shell::sync_camera_pose` writing a flying camera's `CFrame` on every
/// throttle tick: once a row's widget existed at all, it would show
/// whatever it was first seeded with forever, not "whatever the field
/// currently editing it left behind" (which is the only case actually worth
/// protecting — see `resync_field`).
fn resync_row_widget(
    widget: &RowEditor,
    kind: &EditKind,
    sliding: bool,
    window: &mut Window,
    cx: &mut App,
) {
    match (widget, kind) {
        (RowEditor::Text(input), EditKind::Text(seed)) => resync_field(input, seed, window, cx),
        (RowEditor::Slider(input, rail), EditKind::Text(seed)) => {
            resync_field(input, seed, window, cx);
            // Not while the rail is the thing being dragged: it is already
            // showing where the pointer is, and the value coming back is
            // its own, rounded to what the field prints — writing it back
            // would drag the grip a fraction of a step backwards under the
            // pointer on every frame of the gesture.
            if !sliding {
                if let Ok(value) = seed.trim().parse::<f32>() {
                    rail.update(cx, |state, cx| state.set_value(value, window, cx));
                }
            }
        }
        (RowEditor::Fields(_, summary, inputs), EditKind::Fields { values, .. })
        | (RowEditor::Groups(_, summary, inputs), EditKind::Groups { values, .. }) => {
            resync_field(summary, &summary_text(values), window, cx);
            for (input, seed) in inputs.iter().zip(values) {
                resync_field(input, seed, window, cx);
            }
        }
        (RowEditor::Optional(_, _, inner), EditKind::Optional { inner: kind, .. }) => {
            resync_row_widget(inner, kind, sliding, window, cx);
        }
        // `Color` and `Enum` rows have nothing outside their own widget that
        // writes to an already-selected instance repeatedly the way
        // `Shell::sync_camera_pose` does — the one place that seeds a
        // `Color3uint8`/`Enum` on insertion (`shell::keys`'s new-instance
        // defaults) always does so before that instance is selected, so
        // there is no stale cache for it to fight.
        _ => {}
    }
}

/// The whole value, for the field beside a row's expander — empty when any
/// part is: a multi-selection's parts that differ are left empty (see
/// `properties::common`), and `4, , 2` is not a value anyone could type.
fn summary_text(values: &[String]) -> String {
    if values.iter().any(String::is_empty) {
        String::new()
    } else {
        values.join(", ")
    }
}

/// Writes `seed` into `input` unless it currently holds focus (a keystroke
/// in progress, which must win over an external update) or already shows
/// `seed` — `InputState::set_value` unconditionally resets the caret and
/// scroll position, which would be visible jitter on every throttled sync if
/// applied to a value that has not actually changed.
pub(super) fn resync_field(
    input: &Entity<InputState>,
    seed: &str,
    window: &mut Window,
    cx: &mut App,
) {
    if input.focus_handle(cx).is_focused(window) || input.read(cx).value().as_ref() == seed {
        return;
    }
    input.update(cx, |state, cx| state.set_value(seed.to_owned(), window, cx));
}

impl Shell {
    /// The row's live widget, created the first time this row is rendered
    /// for its `EditKind` and reused on every render after that — this is
    /// what keeps a keystroke (or an open color/enum popover) from being
    /// wiped out by the panel rebuilding around it. Returns the widget to
    /// render and the last commit's error, if any.
    ///
    /// `kind` must not be [`EditKind::Bool`] — a checkbox needs no
    /// persistent entity and is built directly where it renders (see
    /// `shell::panels::properties`).
    pub(super) fn edit_row(
        &mut self,
        row: &PropertyRow,
        kind: &EditKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (RowEditor, Option<String>) {
        if let Some(existing) = self.edits.rows.get(&row.name) {
            let widget = existing.widget.clone();
            let error = existing.error.clone();
            let sliding = self.edits.sliding.as_deref() == Some(row.name.as_str());
            resync_row_widget(&widget, kind, sliding, window, cx);
            return (widget, error);
        }

        let (widget, subscriptions) = self.build_row_widget(row.name.clone(), kind, window, cx);
        // Selected instances of different colours: no one colour to show.
        if let (true, RowEditor::Color(state)) = (row.mixed, &widget) {
            state.update(cx, |state, cx| state.clear_value(window, cx));
        }
        self.edits.rows.insert(
            row.name.clone(),
            RowEdit {
                widget: widget.clone(),
                _subscriptions: subscriptions,
                error: None,
            },
        );
        (widget, None)
    }

    fn build_row_widget(
        &mut self,
        name: String,
        kind: &EditKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (RowEditor, Vec<Subscription>) {
        match kind {
            EditKind::Bool(_) => unreachable!(
                "EditKind::Bool never reaches here — see this method's caller in shell::panels"
            ),
            EditKind::Text(seed) => {
                let input = cx.new(|cx| InputState::new(window, cx).default_value(seed.clone()));
                let mut subscriptions = vec![self.commit_on_change(&input, name.clone(), cx)];

                // A rail only for a value that has one to run along, and
                // only when the row is actually holding a number — the
                // same `EditKind::Text` carries a `BrickColor`'s index and
                // every string in the dump.
                let bounded = slider_range(&name)
                    .zip(seed.trim().parse::<f32>().ok())
                    .map(|(span, value)| {
                        let rail = cx.new(|_| {
                            SliderState::new()
                                .min(span.min)
                                .max(span.max)
                                // Never left to the toolkit's own default
                                // of 1, which would make every 0-1 value on
                                // the panel a two-position switch.
                                .step(span.step)
                                .default_value(value.clamp(span.min, span.max))
                        });
                        subscriptions.push(self.slide_on_change(&rail, name, cx));
                        rail
                    });

                match bounded {
                    Some(rail) => (RowEditor::Slider(input, rail), subscriptions),
                    None => (RowEditor::Text(input), subscriptions),
                }
            }
            EditKind::Fields { fields, values } => {
                let (summary, inputs, subscriptions) =
                    self.build_number_inputs(&name, values, window, cx);
                (RowEditor::Fields(fields, summary, inputs), subscriptions)
            }
            EditKind::Groups { groups, values } => {
                let (summary, inputs, subscriptions) =
                    self.build_number_inputs(&name, values, window, cx);
                (RowEditor::Groups(groups, summary, inputs), subscriptions)
            }
            // Nothing to subscribe to: the strip is a button, and the panel
            // it opens commits through `commit_row` like any other widget.
            EditKind::Sequence { color, text } => (
                RowEditor::Sequence {
                    color: *color,
                    text: text.clone(),
                },
                Vec::new(),
            ),
            // Each flag commits the whole set, because the DOM's value is
            // one bit field — there is no "set only this side" write.
            EditKind::Flags { labels, values } => {
                (RowEditor::Flags(labels, values.clone()), Vec::new())
            }
            // The inner editor is built even while the value is absent: the
            // checkbox turning it on commits, which drops this whole row
            // editor (see `commit_row`) and rebuilds it against the value
            // that write produced — so nothing here has to survive the flip.
            EditKind::Optional {
                present,
                label,
                inner,
            } => {
                let (widget, subscriptions) = self.build_row_widget(name, inner, window, cx);
                (
                    RowEditor::Optional(*present, label, Box::new(widget)),
                    subscriptions,
                )
            }
            EditKind::Color { r, g, b } => {
                let value = rgb_to_hsla(*r, *g, *b);
                let state = cx.new(|cx| ColorPickerState::new(window, cx).default_value(value));
                let subscription =
                    cx.subscribe(&state, move |shell, _, event: &ColorPickerEvent, cx| {
                        let ColorPickerEvent::Change(Some(color)) = event else {
                            return;
                        };
                        let (r, g, b) = hsla_to_rgb(*color);
                        shell.commit_row(&name, &format!("{r}, {g}, {b}"), cx);
                    });
                (RowEditor::Color(state), vec![subscription])
            }
            EditKind::Enum { current, items } => {
                let options = SearchableVec::new(
                    items
                        .iter()
                        .map(|item| SharedString::from(item.as_str()))
                        .collect::<Vec<_>>(),
                );
                let selected = items
                    .iter()
                    .position(|item| item == current)
                    .map(IndexPath::new);
                let state = cx.new(|cx| SelectState::new(options, selected, window, cx));
                let subscription = cx.subscribe(
                    &state,
                    move |shell, _, event: &SelectEvent<EnumOptions>, cx| {
                        let SelectEvent::Confirm(Some(picked)) = event else {
                            return;
                        };
                        shell.commit_row(&name, picked, cx);
                    },
                );
                (RowEditor::Enum(state), vec![subscription])
            }
        }
    }

    /// The `Input`s behind one numeric value: the summary holding it whole,
    /// then one per component.
    ///
    /// The components are built even while the row is collapsed and their
    /// fields are not drawn. Seeding them costs a small entity each, once
    /// per selection; skipping it would mean rebuilding the row's whole
    /// widget on every expander click, which is the more expensive of the
    /// two and the one that happens while someone is looking at it.
    fn build_number_inputs(
        &self,
        name: &str,
        values: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (
        Entity<InputState>,
        Vec<Entity<InputState>>,
        Vec<Subscription>,
    ) {
        let mut subscriptions = Vec::with_capacity(values.len() + 1);

        // The same comma-joined spelling `properties::edit::edit_text`
        // produced and `parse` reads back, so what the summary shows is
        // exactly what committing it writes.
        let summary = cx.new(|cx| InputState::new(window, cx).default_value(summary_text(values)));
        let whole = name.to_owned();
        subscriptions.push(
            cx.subscribe(&summary, move |shell, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    // Its own text, not the components' — the two hold the
                    // same value and only one of them is being typed into.
                    let text = input.read(cx).value().to_string();
                    shell.commit_row(&whole, &text, cx);
                }
            }),
        );

        let mut inputs = Vec::with_capacity(values.len());
        for seed in values {
            let input = cx.new(|cx| InputState::new(window, cx).default_value(seed.clone()));
            subscriptions.push(self.commit_on_change(&input, name.to_owned(), cx));
            inputs.push(input);
        }
        (summary, inputs, subscriptions)
    }

    /// Wires one `Input` (a lone `Text` field, or one of a `Fields` row) so
    /// Enter or a focus loss re-reads every field belonging to `name` and
    /// commits them together.
    fn commit_on_change(
        &self,
        input: &Entity<InputState>,
        name: String,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe(input, move |shell, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                shell.commit_row_from_inputs(&name, cx);
            }
        })
    }

    /// Wires one rail so a drag writes through this row the way typing
    /// does, and lands as a single undo entry however many frames it took.
    fn slide_on_change(
        &self,
        rail: &Entity<SliderState>,
        name: String,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe(rail, move |shell, _, event: &SliderEvent, cx| match event {
            SliderEvent::Change(SliderValue::Single(value)) => shell.slide_row(&name, *value, cx),
            // The gesture is over: let the row rebuild from whatever the
            // DOM made of the last value, and let the next drag open its
            // own undo entry.
            SliderEvent::Release(_) => {
                shell.edits.sliding = None;
                shell.edits.rows.remove(&name);
                cx.notify();
            }
            // A range slider's two-value form, which no property row builds.
            SliderEvent::Change(SliderValue::Range(..)) => {}
        })
    }

    /// One frame of a slider drag: the same textual commit a typed value
    /// takes, spelled the way the field beside the rail prints it.
    fn slide_row(&mut self, name: &str, value: f32, cx: &mut Context<Self>) {
        let text = super::scrub::format(value, FieldKind::Decimal);
        let opening = self.edits.sliding.is_none();
        if opening {
            self.edits.sliding = Some(name.to_owned());
        }
        self.commit_row_step(name, &text, opening, cx);
    }

    /// Reads every `Input` making up row `name` (one for `Text`, several for
    /// `Fields`) and commits their values joined the same way
    /// `edit::edit_text` already joins a compound value's numbers.
    fn commit_row_from_inputs(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(edit) = self.edits.rows.get(name) else {
            return;
        };
        // Color, Enum and Flags commit straight from their own event or click
        // instead (see `build_row_widget`); an `Input` subscription never
        // fires for them, so they have no text to read here.
        let Some(text) = edit.widget.input_text(cx) else {
            return;
        };
        self.commit_row(name, &text, cx);
    }

    /// One open row's numeric field, for the drag in `shell::scrub`.
    pub(super) fn field_input(&self, property: &str, index: usize) -> Option<Entity<InputState>> {
        self.edits
            .rows
            .get(property)
            .and_then(|edit| edit.widget.field(index))
            .cloned()
    }

    /// Writes one row's text into the DOM and either drops the row's editor
    /// (so the next render rebuilds it from the freshly written, normalized
    /// value — clamped colors, a resolved enum ordinal) or records the
    /// error for that row to show.
    pub(super) fn commit_row(&mut self, name: &str, text: &str, cx: &mut Context<Self>) {
        self.commit_row_step(name, text, true, cx);
    }

    /// [`Self::commit_row`] for one step of a gesture: `push` is true only
    /// on the step that starts it, so a whole drag lands as a single undo
    /// entry the way a viewport drag already does (see `shell::drag`).
    pub(crate) fn commit_row_step(
        &mut self,
        name: &str,
        text: &str,
        push: bool,
        cx: &mut Context<Self>,
    ) {
        match self.apply_edit(name, text, push, cx) {
            Ok(()) => {
                // Except the row whose own slider is mid-drag: dropping its
                // widget drops the gesture with it (see `Edits::sliding`).
                if self.edits.sliding.as_deref() != Some(name) {
                    self.edits.rows.remove(name);
                }
            }
            Err(message) => {
                if let Some(edit) = self.edits.rows.get_mut(name) {
                    edit.error = Some(message);
                }
            }
        }
        cx.notify();
    }

    /// Writes one edit to every selected instance through the Command Bar's
    /// own take/put-back path, as one undo step however many there are, then
    /// reflects it exactly as a script's mutation would: the Properties panel
    /// always re-reads `self.dom` fresh, the Explorer only when `Name` moved a
    /// row, and the viewport through the same `Change` log every other
    /// mutation hands it (see `Shell::reflect_changes`). A folder's colour
    /// and an attribute are the anchor's alone: the panel offers neither for
    /// a multi-selection.
    fn apply_edit(
        &mut self,
        name: &str,
        text: &str,
        push: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let reference = self
            .selected()
            .ok_or_else(|| "nothing is selected".to_string())?;

        // Not a DOM write — see `shell::folder_color`; skips undo history
        // and `WeakDom::set_property` entirely.
        if name == properties::edit::FOLDER_COLOR_PROPERTY {
            let result = self.commit_folder_color(reference, text, cx);
            self.scroll_to_row(reference, name);
            return result;
        }

        // An attribute's value, not a real DOM property — see
        // `properties::attributes::row_name`. Reuses this exact push-history/
        // reflect/record sequence so it costs one undo step like any other
        // edit, but the write itself goes through the attribute blob rather
        // than `WeakDom::set_property` directly.
        if let Some(attribute) = properties::attributes::attribute_of_row(name) {
            if push {
                self.push_history();
            }
            let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
            let result = properties::attributes::set_attribute_value(
                &mut dom,
                &self.database,
                reference,
                attribute,
                text,
            );
            self.dom = dom;
            let changes = self.dom.take_changes();
            self.reflect_changes(&changes, cx);
            self.record_history_change(changes);
            return result;
        }

        // See `shell::history`: snapshotted before the write below.
        if push {
            self.push_history();
        }
        let selection = self.selected_all().to_vec();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let result = properties::edit::commit_all(&mut dom, &self.database, &selection, name, text);
        self.dom = dom;
        // Reflected and recorded whether or not the commit below succeeded:
        // a rejected value never reaches `WeakDom::set_property`, so the log
        // is simply empty then — nothing to show, nothing to undo.
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        result?;

        if name == NAME_PROPERTY {
            self.rebuild_explorer(cx);
        }
        self.scroll_to_row(reference, name);
        Ok(())
    }

    /// Brings the row that was just written into view. A property far down
    /// the alphabet starts scrolled out of the panel's fixed-height list, and
    /// nothing can scroll it into frame for a screenshot afterwards — see
    /// `AGENTS.md`'s ban on synthetic input.
    fn scroll_to_row(&self, reference: rbx_dom::Ref, name: &str) {
        let folder_color = self.folder_color(reference);
        let rows = self
            .properties
            .rows(&self.dom, self.selected_all(), folder_color);
        if let Some(index) = rows.iter().position(|row| row.name == name) {
            self.properties_scroll.scroll_to_item(index);
        }
    }

    /// Whether the properties panel's `category` section is manually
    /// collapsed (see `shell::panels::properties`, which forces every
    /// section open instead while a filter is active).
    pub(super) fn is_category_collapsed(&self, category: &str) -> bool {
        self.edits.collapsed.contains(category)
    }

    /// Whether one numeric row is showing its components.
    pub(super) fn is_row_expanded(&self, name: &str) -> bool {
        self.edits.expanded.contains(name)
    }

    /// Shows or hides one numeric row's components, from a click on its
    /// expander. Survives a selection change the way a collapsed category
    /// does: which values someone wants broken out is a preference about
    /// the panel, not about the instance in it.
    pub(super) fn toggle_row_expanded(&mut self, name: &str, cx: &mut Context<Self>) {
        if !self.edits.expanded.remove(name) {
            self.edits.expanded.insert(name.to_owned());
        }
        cx.notify();
    }

    /// Collapses or re-opens one section, from a click on its header.
    pub(super) fn toggle_category(&mut self, category: &str, cx: &mut Context<Self>) {
        if !self.edits.collapsed.remove(category) {
            self.edits.collapsed.insert(category.to_owned());
        }
        cx.notify();
    }

    /// `RBX_STUDIO_EDIT='Prop=value'`: applied once, before the first frame,
    /// to whatever `RBX_STUDIO_SELECT` selected. A malformed spec (no `=`) or
    /// a rejected value is silently ignored — this is a screenshot aid, not
    /// user input, and must never crash a debugging session.
    pub(super) fn apply_debug_edit(
        &mut self,
        spec: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((name, value)) = spec.split_once('=') else {
            return;
        };
        let name = name.trim().to_owned();
        if self.apply_edit(&name, value.trim(), true, cx).is_ok() {
            // Narrows the panel to just this row, which a screenshot needs:
            // nothing can scroll the fixed-height list past however many
            // properties sort before it alphabetically (see `AGENTS.md`'s ban
            // on synthetic input).
            self.filter
                .update(cx, |state, cx| state.set_value(name, window, cx));
        }
        cx.notify();
    }
}

/// A stored sRGB byte triplet as the `Hsla` a `ColorPickerState` edits —
/// plain RGB↔HSL math, the same (non-linear-light) space the read-only
/// column already displays these in (see `properties::color3`).
fn rgb_to_hsla(r: u8, g: u8, b: u8) -> Hsla {
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
    .into()
}

/// The inverse of [`rgb_to_hsla`].
fn hsla_to_rgb(color: Hsla) -> (u8, u8, u8) {
    let rgba = color.to_rgb();
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    (channel(rgba.r), channel(rgba.g), channel(rgba.b))
}

#[cfg(test)]
mod tests {
    use super::{hsla_to_rgb, rgb_to_hsla};

    #[test]
    fn rgb_round_trips_through_hsla_for_primaries_and_greys() {
        for (r, g, b) in [
            (0, 0, 0),
            (255, 255, 255),
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            (128, 64, 200),
            (17, 200, 3),
        ] {
            assert_eq!(hsla_to_rgb(rgb_to_hsla(r, g, b)), (r, g, b), "{r},{g},{b}");
        }
    }
}
