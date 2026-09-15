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
use gpui_kit::component::IndexPath;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::properties::{self, edit::NAME_PROPERTY, EditKind, PropertyRow};

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
    /// One `Input` per label, in the same order as `EditKind::Fields`'
    /// `labels` — carried alongside so `shell::rows` can pair each field
    /// with its caption without reaching back into `EditKind`.
    Fields(&'static [&'static str], Vec<Entity<InputState>>),
    Color(Entity<ColorPickerState>),
    Enum(Entity<SelectState<EnumOptions>>),
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
fn resync_row_widget(widget: &RowEditor, kind: &EditKind, window: &mut Window, cx: &mut App) {
    match (widget, kind) {
        (RowEditor::Text(input), EditKind::Text(seed)) => resync_field(input, seed, window, cx),
        (RowEditor::Fields(_, inputs), EditKind::Fields { values, .. }) => {
            for (input, seed) in inputs.iter().zip(values) {
                resync_field(input, seed, window, cx);
            }
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

/// Writes `seed` into `input` unless it currently holds focus (a keystroke
/// in progress, which must win over an external update) or already shows
/// `seed` — `InputState::set_value` unconditionally resets the caret and
/// scroll position, which would be visible jitter on every throttled sync if
/// applied to a value that has not actually changed.
fn resync_field(input: &Entity<InputState>, seed: &str, window: &mut Window, cx: &mut App) {
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
            resync_row_widget(&widget, kind, window, cx);
            return (widget, error);
        }

        let (widget, subscriptions) = self.build_row_widget(row.name.clone(), kind, window, cx);
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
                let subscription = self.commit_on_change(&input, name, cx);
                (RowEditor::Text(input), vec![subscription])
            }
            EditKind::Fields { labels, values } => {
                let mut inputs = Vec::with_capacity(values.len());
                let mut subscriptions = Vec::with_capacity(values.len());
                for seed in values {
                    let input =
                        cx.new(|cx| InputState::new(window, cx).default_value(seed.clone()));
                    subscriptions.push(self.commit_on_change(&input, name.clone(), cx));
                    inputs.push(input);
                }
                (RowEditor::Fields(labels, inputs), subscriptions)
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

    /// Reads every `Input` making up row `name` (one for `Text`, several for
    /// `Fields`) and commits their values joined the same way
    /// `edit::edit_text` already joins a compound value's numbers.
    fn commit_row_from_inputs(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(edit) = self.edits.rows.get(name) else {
            return;
        };
        let text = match &edit.widget {
            RowEditor::Text(input) => input.read(cx).value().to_string(),
            RowEditor::Fields(_, inputs) => inputs
                .iter()
                .map(|input| input.read(cx).value().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            // Color and Enum commit straight from their own event instead
            // (see `build_row_widget`); an `Input` subscription never fires
            // for them.
            RowEditor::Color(_) | RowEditor::Enum(_) => return,
        };
        self.commit_row(name, &text, cx);
    }

    /// Writes one row's text into the DOM and either drops the row's editor
    /// (so the next render rebuilds it from the freshly written, normalized
    /// value — clamped colors, a resolved enum ordinal) or records the
    /// error for that row to show.
    pub(super) fn commit_row(&mut self, name: &str, text: &str, cx: &mut Context<Self>) {
        match self.apply_edit(name, text, cx) {
            Ok(()) => {
                self.edits.rows.remove(name);
            }
            Err(message) => {
                if let Some(edit) = self.edits.rows.get_mut(name) {
                    edit.error = Some(message);
                }
            }
        }
        cx.notify();
    }

    /// Writes one edit to the selected instance through the Command Bar's own
    /// take/put-back path, then reflects it exactly as a script's mutation
    /// would: the Properties panel always re-reads `self.dom` fresh, the
    /// Explorer only when `Name` moved a row, and the viewport always.
    fn apply_edit(&mut self, name: &str, text: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let reference = self
            .selected()
            .ok_or_else(|| "nothing is selected".to_string())?;

        // See `shell::history`: snapshotted before the write below.
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let result = properties::edit::commit(&mut dom, &self.database, reference, name, text);
        self.dom = dom;
        result?;

        if name == NAME_PROPERTY {
            self.rebuild_explorer(cx);
        }
        self.reflect_in_viewport(reference, name, cx);
        self.scroll_to_row(reference, name);
        Ok(())
    }

    /// Reflects one committed edit in the 3D view, through whichever of
    /// [`ViewportEdit`]'s four paths `name`/the edited instance's class say
    /// is safe — see its doc comment. A referent that stopped resolving
    /// (should not happen right after a successful commit, but a fallback
    /// costs nothing) gets the always-correct full reload too.
    fn reflect_in_viewport(&mut self, reference: Ref, name: &str, cx: &mut Context<Self>) {
        let class = self.dom.get(reference).map(|instance| instance.class());
        let edit = class.map_or(ViewportEdit::Full, |class| {
            classify_edit(&self.database, class, name)
        });

        match edit {
            ViewportEdit::Lighting => {
                let dom = self.dom.clone();
                self.viewport
                    .update(cx, |viewport, _| viewport.update_lighting(dom));
            }
            ViewportEdit::Instance => {
                let dom = self.dom.clone();
                self.viewport
                    .update(cx, |viewport, _| viewport.patch_instance(dom, reference));
            }
            ViewportEdit::Effect => {
                let dom = self.dom.clone();
                self.viewport
                    .update(cx, |viewport, _| viewport.patch_effect(dom, reference));
            }
            ViewportEdit::Full => self.reload_viewport(cx),
        }
    }

    /// Brings the row that was just written into view. A property far down
    /// the alphabet starts scrolled out of the panel's fixed-height list, and
    /// nothing can scroll it into frame for a screenshot afterwards — see
    /// `AGENTS.md`'s ban on synthetic input.
    fn scroll_to_row(&self, reference: rbx_dom::Ref, name: &str) {
        let rows = self.properties.rows(&self.dom, reference);
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

    /// Replaces the whole collapsed-category set, from an accordion click.
    pub(super) fn set_collapsed_categories(
        &mut self,
        collapsed: HashSet<String>,
        cx: &mut Context<Self>,
    ) {
        self.edits.collapsed = collapsed;
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
        if self.apply_edit(&name, value.trim(), cx).is_ok() {
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

/// Where a committed property edit can reach the viewport without rebuilding
/// the whole scene (`marked.rbxl`'s 16k-plus instances is what makes a full
/// rebuild freeze the viewport for seconds — see `rbx_viewer::Headless`'s own
/// `update_lighting`/`patch_instance` doc comments for the mechanism each of
/// these actually drives).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ViewportEdit {
    /// `Lighting`, `Atmosphere`, `Clouds`, a `PostEffect`, or a `Light`
    /// (`Point`/`Spot`/`Surface`): recompute the lighting uniform and the
    /// local-light buffer in place.
    Lighting,
    /// A `BasePart` (or descendant): try to patch its one GPU instance,
    /// falling back to a full reload if the edit turns out to cross a GPU
    /// bucket (see `rbx_viewer::scene::Scene::patch_part`, and
    /// `Scene::patch_mesh_instance` for a part drawn as a resolved mesh).
    Instance,
    /// A `ParticleEmitter`, `Beam` or `Trail`: re-read that one effect list
    /// from the DOM and hand it to the renderer, which keeps its textures and
    /// running simulations (see `rbx_viewer::Headless::patch_effect`),
    /// falling back to a full reload only for a texture never downloaded.
    Effect,
    /// Anything else — including every `Sky` edit (see below) and a `Parent`
    /// change on any class — where a full rebuild is the only thing
    /// guaranteed to draw the right picture.
    Full,
}

/// Every class classified [`ViewportEdit::Lighting`], as the ancestor its
/// class is checked against — including `Light` itself, so `PointLight`/
/// `SpotLight`/`SurfaceLight` all match without naming each one.
const LIGHTING_LIKE: [&str; 5] = ["Lighting", "Atmosphere", "Clouds", "PostEffect", "Light"];

/// Every class classified [`ViewportEdit::Effect`]. `Attachment` is
/// deliberately absent even though moving one moves a beam's or trail's
/// endpoint: one attachment can anchor any number of either, and only a full
/// reload re-reads them all.
const EFFECT_LIKE: [&str; 3] = ["ParticleEmitter", "Beam", "Trail"];

/// Classifies a committed edit by the class of the instance it touched and by
/// `name`.
///
/// `Parent` always forces [`ViewportEdit::Full`], even on a `BasePart`:
/// reparenting can move an instance in or out of `Workspace` (see
/// `rbx_viewer::scene::workspace_descendants`), which neither fast path
/// accounts for. `Sky` is deliberately never [`ViewportEdit::Lighting`]
/// either, unlike the rest of that class list: its skybox panels, prefiltered
/// environment probe, sun/moon discs and star field are all GPU state built
/// once when the scene loads (see `rbx_viewer::renderer::Renderer::new`), and
/// making every one of those live is future work — a full reload is the
/// documented fallback for now, not a missed case.
pub(super) fn classify_edit(
    database: &ReflectionDatabase,
    class: &str,
    name: &str,
) -> ViewportEdit {
    if name == "Parent" {
        return ViewportEdit::Full;
    }
    if database.is_subclass_of(class, "Sky") {
        return ViewportEdit::Full;
    }
    if LIGHTING_LIKE
        .iter()
        .any(|ancestor| database.is_subclass_of(class, ancestor))
    {
        return ViewportEdit::Lighting;
    }
    if database.is_subclass_of(class, "BasePart") {
        return ViewportEdit::Instance;
    }
    if EFFECT_LIKE
        .iter()
        .any(|ancestor| database.is_subclass_of(class, ancestor))
    {
        return ViewportEdit::Effect;
    }
    ViewportEdit::Full
}

#[cfg(test)]
mod tests {
    use rbx_reflection::ReflectionDatabase;

    use super::{classify_edit, hsla_to_rgb, rgb_to_hsla, ViewportEdit};

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

    #[test]
    fn lighting_like_classes_take_the_lighting_fast_path() {
        let database = ReflectionDatabase::embedded();
        for (class, name) in [
            ("Lighting", "TimeOfDay"),
            ("Atmosphere", "Density"),
            ("Clouds", "Cover"),
            ("BloomEffect", "Intensity"),
            ("SunRaysEffect", "Intensity"),
            ("PointLight", "Brightness"),
            ("SpotLight", "Range"),
            ("SurfaceLight", "Color"),
        ] {
            assert_eq!(
                classify_edit(&database, class, name),
                ViewportEdit::Lighting,
                "{class}.{name}"
            );
        }
    }

    // `Sky` is in the same service as `Atmosphere`/`Clouds` but is not in the
    // lighting fast path yet (see `classify_edit`'s doc comment) — a
    // regression here would silently stop rebuilding its skybox/probe/stars.
    #[test]
    fn sky_falls_back_to_a_full_reload() {
        let database = ReflectionDatabase::embedded();
        assert_eq!(
            classify_edit(&database, "Sky", "SkyboxUp"),
            ViewportEdit::Full
        );
    }

    #[test]
    fn a_base_part_and_its_subclasses_patch_the_single_instance() {
        let database = ReflectionDatabase::embedded();
        for class in [
            "Part",
            "MeshPart",
            "WedgePart",
            "TrussPart",
            "UnionOperation",
        ] {
            assert_eq!(
                classify_edit(&database, class, "Color3uint8"),
                ViewportEdit::Instance,
                "{class}"
            );
        }
    }

    #[test]
    fn effect_classes_take_the_effect_fast_path() {
        let database = ReflectionDatabase::embedded();
        for (class, name) in [
            ("ParticleEmitter", "Enabled"),
            ("ParticleEmitter", "Rate"),
            ("Beam", "Width0"),
            ("Trail", "Lifetime"),
        ] {
            assert_eq!(
                classify_edit(&database, class, name),
                ViewportEdit::Effect,
                "{class}.{name}"
            );
        }
    }

    // An `Attachment` anchors any number of beams and trails at once, so its
    // edits stay on the one path that re-reads every one of them.
    #[test]
    fn an_attachment_edit_falls_back_to_a_full_reload() {
        let database = ReflectionDatabase::embedded();
        assert_eq!(
            classify_edit(&database, "Attachment", "CFrame"),
            ViewportEdit::Full
        );
    }

    // Reparenting can move an instance in or out of `Workspace`, which no
    // fast path accounts for — an edit of exactly this one property must
    // still force a full reload whatever the class.
    #[test]
    fn a_parent_change_always_forces_a_full_reload() {
        let database = ReflectionDatabase::embedded();
        for class in ["Part", "ParticleEmitter", "PointLight"] {
            assert_eq!(
                classify_edit(&database, class, "Parent"),
                ViewportEdit::Full,
                "{class}"
            );
        }
    }

    #[test]
    fn an_unrelated_class_falls_back_to_a_full_reload() {
        let database = ReflectionDatabase::embedded();
        assert_eq!(
            classify_edit(&database, "Script", "Source"),
            ViewportEdit::Full
        );
        assert_eq!(
            classify_edit(&database, "Folder", "Name"),
            ViewportEdit::Full
        );
    }
}
