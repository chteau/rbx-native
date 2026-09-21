//! The viewport's transform toolbar: which of Studio's tools is active,
//! whether its handles follow the world's axes or the part's own, and the
//! keystrokes that change either.
//!
//! The state lives in [`crate::shell::Shell`] — the toolbar renders from it —
//! and is pushed down to [`crate::workspace_view::WorkspaceView`], which
//! hit-tests the cursor against the handles, and on to the render thread,
//! which draws them.
//!
//! Shortcuts and behaviour follow `creator-docs`
//! (`parts/index.md#transform-parts`): `2` for Move, `3` for Scale, `4` for
//! Rotate, `Ctrl`/`Cmd`+`L` for local orientation.

use glam::{Mat3, Mat4, Vec3};
use gpui_kit::Modifiers;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::gizmo::{self, Kind};
use rbx_viewer::pick;
use rbx_viewer::Gizmo;

/// A transform tool the viewport can carry out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Tool {
    /// Click to select, and nothing else — Studio's own default.
    #[default]
    Select,
    Move,
    Scale,
    Rotate,
}

impl Tool {
    pub(crate) const ALL: [Tool; 4] = [Tool::Select, Tool::Move, Tool::Scale, Tool::Rotate];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Move => "Move",
            Tool::Scale => "Scale",
            Tool::Rotate => "Rotate",
        }
    }

    /// The key that picks this tool, for the toolbar button's own label.
    pub(crate) fn shortcut(self) -> &'static str {
        match self {
            Tool::Select => "1",
            Tool::Move => "2",
            Tool::Scale => "3",
            Tool::Rotate => "4",
        }
    }

    /// Which handles this tool puts over the selection, or `None` for Select,
    /// which has none of its own.
    pub(crate) fn kind(self) -> Option<Kind> {
        match self {
            Tool::Select => None,
            Tool::Move => Some(Kind::Move),
            Tool::Scale => Some(Kind::Scale),
            Tool::Rotate => Some(Kind::Rotate),
        }
    }
}

/// Which of the toolbar's two snap increments a control belongs to.
///
/// Two, not three: `creator-docs` gives Move and Scale one field between them
/// ("**snapping** increments are based on **studs** for moving/scaling or
/// **degrees** for rotating"), and confirms it with the shortcuts — `Shift`+`2`
/// jumps to "the **move/scale** increment input", `Alt`+`R` to "the **rotate**
/// increment input".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapKind {
    /// Studs, shared by Move and Scale.
    Translate,
    /// Degrees, Rotate's own.
    Rotate,
}

impl SnapKind {
    pub(crate) const ALL: [SnapKind; 2] = [SnapKind::Translate, SnapKind::Rotate];

    pub(crate) fn label(self) -> &'static str {
        match self {
            SnapKind::Translate => "Move/Scale",
            SnapKind::Rotate => "Rotate",
        }
    }

    /// What the increment is measured in, for the field's own suffix.
    pub(crate) fn unit(self) -> &'static str {
        match self {
            SnapKind::Translate => "studs",
            SnapKind::Rotate => "degrees",
        }
    }
}

/// One snap increment and whether it is switched on — Studio's toolbar pairs a
/// checkbox with a number, rather than carrying a single fixed flag.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Snap {
    pub(crate) enabled: bool,
    pub(crate) increment: f32,
}

impl Snap {
    /// Whether this drag actually snaps, given whether `Shift` is held.
    ///
    /// The docs say `Shift` temporarily "**toggle**[s] snapping"
    /// (`parts/index.md#transform-parts`) without saying which way. Studio's
    /// own draggers only ever suspend it: `DraggerFramework`'s
    /// `shouldGridSnap` is `LinearSnapEnabled and not Shift`, so `Shift`
    /// frees a snapped drag and does nothing to a free one.
    pub(crate) fn active(self, shift: bool) -> bool {
        self.enabled && !shift
    }

    /// The increment this drag should round to, or `0.0` for no grid at all —
    /// the value [`rbx_viewer::snap::round_to`] passes through untouched.
    pub(crate) fn grid(self, shift: bool) -> f32 {
        if self.active(shift) {
            self.increment
        } else {
            0.0
        }
    }
}

impl Default for Snap {
    /// Studio ships with snapping on. The docs publish no default increment,
    /// so a whole stud is this editor's own choice (see [`Transform::default`]
    /// for the rotate one).
    fn default() -> Self {
        Snap {
            enabled: true,
            increment: 1.0,
        }
    }
}

/// Everything the transform toolbar holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Transform {
    pub(crate) tool: Tool,
    /// Handles along the part's own axes rather than the world's.
    pub(crate) local: bool,
    /// The move/scale increment, in studs.
    pub(crate) translate: Snap,
    /// The rotate increment, in degrees.
    pub(crate) rotate: Snap,
}

impl Default for Transform {
    /// The rotate increment starts at an eighth of a turn: the docs give no
    /// default, and a degree increment that doesn't divide 90° evenly leaves a
    /// part unable to come back to square.
    fn default() -> Self {
        Transform {
            tool: Tool::default(),
            local: false,
            translate: Snap::default(),
            rotate: Snap {
                increment: 45.0,
                ..Snap::default()
            },
        }
    }
}

impl Transform {
    /// What the renderer should draw over the selection, if anything.
    pub(crate) fn gizmo(self) -> Option<Gizmo> {
        self.tool.kind().map(|kind| Gizmo {
            kind,
            local: self.local,
        })
    }

    /// Whether dragging in the viewport transforms the selected part at all.
    pub(crate) fn drags(self) -> bool {
        self.tool.kind().is_some()
    }
}

/// What a keystroke over the 3D view asks of the toolbar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Action {
    Use(Tool),
    ToggleLocal,
    /// The checkbox beside one of the increment fields.
    ToggleSnap(SnapKind),
    /// A new increment, already parsed out of the field's text.
    SetIncrement(SnapKind, f32),
    /// Put the caret in one of the increment fields — Studio's `Shift`+`2`.
    FocusIncrement(SnapKind),
}

/// Resolves a keystroke to a toolbar action, or `None` for anything else.
///
/// Bound where the 3D view has focus rather than window-wide (see
/// `WorkspaceView::key`): a bare digit is a character everywhere else in the
/// editor, and a tool shortcut that ate keystrokes out of the Command Bar or a
/// property field would be a bug, not a feature. `Shift`+`2` is deliberately
/// not Move either: creator-docs gives that chord to the move/scale increment
/// field, so it jumps to the field instead.
///
/// `Alt`/`⌥`+`R` is the docs' own chord for the *rotate* increment field,
/// the counterpart of `Shift`+`2`.
pub(crate) fn action_for(key: &str, modifiers: Modifiers) -> Option<Action> {
    let plain = !modifiers.control && !modifiers.alt && !modifiers.shift && !modifiers.platform;
    let only_shift = modifiers.shift && !modifiers.control && !modifiers.alt && !modifiers.platform;
    let only_alt = modifiers.alt && !modifiers.control && !modifiers.shift && !modifiers.platform;
    match key {
        // `platform` is Cmd on a Mac, where creator-docs gives the toggle as
        // ⌘L rather than Ctrl+L.
        "l" if (modifiers.control || modifiers.platform) && !modifiers.shift => {
            Some(Action::ToggleLocal)
        }
        "2" if only_shift => Some(Action::FocusIncrement(SnapKind::Translate)),
        "r" if only_alt => Some(Action::FocusIncrement(SnapKind::Rotate)),
        _ if !plain => None,
        "1" => Some(Action::Use(Tool::Select)),
        "2" => Some(Action::Use(Tool::Move)),
        "3" => Some(Action::Use(Tool::Scale)),
        "4" => Some(Action::Use(Tool::Rotate)),
        _ => None,
    }
}

/// Reads an increment out of the toolbar's own field.
///
/// Anything unreadable leaves the increment alone rather than silently
/// becoming zero — a field mid-edit passes through `""` and `"1."` on its way
/// to a number, and neither should turn snapping off under the user.
/// Negatives fold to their magnitude: a grid has no direction.
pub(crate) fn parse_increment(text: &str) -> Option<f32> {
    let value: f32 = text.trim().parse().ok()?;
    value.is_finite().then(|| value.abs())
}

/// Where the handles stand: the selected part, and the matrix it is drawn
/// with.
///
/// Carried on the UI thread so a click can be hit-tested against the handles
/// without asking the render thread, which owns the scene and answers only
/// between frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Target {
    pub(crate) referent: Ref,
    pub(crate) model: Mat4,
    /// Whether this resolves to a `Ball` right now — read once, here, rather
    /// than re-resolved on every drag step: a Scale drag holds a `Target`
    /// snapshotted at the grab (see `Drag::Size`), and a part's `Shape`
    /// cannot change mid-gesture anyway. Reuses `rbx_viewer`'s own shape
    /// resolution (`resolved_shape_label`) rather than a second, possibly-
    /// diverging check of the `Shape` property — see
    /// `shell::keys::apply_part_defaults` for why only `Part` itself ever
    /// carries one.
    pub(crate) sphere: bool,
    /// Whether this resolves to a real `Part.Shape == Cylinder` right now —
    /// same reasoning and same read as `sphere`, checked against
    /// `resolved_shape_label`'s `"CylinderX"` rather than the legacy
    /// mesh-child `"CylinderY"` case (`rbx_viewer::scene::shape::part_type`):
    /// `Enum.PartType.Cylinder` always draws with its length along the
    /// part's own local X, round in Y/Z — a fixed convention, not something
    /// read per-instance, which is why nothing here stores *which* axis is
    /// which.
    pub(crate) cylinder: bool,
}

impl Target {
    /// Reads one instance's placement out of the DOM, or `None` for anything
    /// that isn't a part with a transform to drag. A container (a `Folder`, a
    /// service, a `Model`) has no `CFrame` or `Size` of its own to write, so
    /// it never becomes a target itself — [`Targets::read`] resolves one to
    /// the parts beneath it instead.
    pub(crate) fn read(
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Option<Ref>,
    ) -> Option<Self> {
        let referent = referent?;
        let shape = rbx_viewer::resolved_shape_label(dom, database, referent);
        Some(Target {
            referent,
            model: rbx_viewer::pick::model_of(dom, referent)?,
            sphere: shape == Some("Ball"),
            cylinder: shape == Some("CylinderX"),
        })
    }

    pub(crate) fn position(&self) -> Vec3 {
        self.model.w_axis.truncate()
    }

    /// The part's own axes, still carrying its `Size` in their lengths —
    /// `rbx_viewer::gizmo::basis` normalizes them.
    pub(crate) fn rotation(&self) -> Mat3 {
        Mat3::from_mat4(self.model)
    }

    /// The part's `Size`, which is exactly what `pick::part_model` folded into
    /// the lengths of those columns.
    pub(crate) fn size(&self) -> Vec3 {
        Vec3::new(
            self.model.x_axis.length(),
            self.model.y_axis.length(),
            self.model.z_axis.length(),
        )
    }

    /// The part's own orientation with its `Size` divided back out — the
    /// rotation a `CFrame` carries.
    pub(crate) fn orientation(&self) -> Mat3 {
        let [x, y, z] = gizmo::basis(Some(self.rotation()));
        Mat3::from_cols(x, y, z)
    }

    /// The same part standing somewhere else — what a drag in progress shows
    /// while `Shell` is still writing the move into the DOM.
    pub(crate) fn moved_to(self, position: Vec3) -> Self {
        Target {
            model: Mat4::from_cols(
                self.model.x_axis,
                self.model.y_axis,
                self.model.z_axis,
                position.extend(1.0),
            ),
            ..self
        }
    }

    /// The same part at a new `Size`, standing where the drag put it.
    pub(crate) fn resized_to(self, size: Vec3, position: Vec3) -> Self {
        self.placed(self.orientation(), size, position)
    }

    /// The same part turned, keeping its size and where it stands.
    pub(crate) fn rotated_to(self, orientation: Mat3) -> Self {
        self.placed(orientation, self.size(), self.position())
    }

    /// Rebuilds the model matrix the way `pick::part_model` does: the
    /// orientation's columns scaled by the size, and the centre in the last.
    pub(crate) fn placed(self, orientation: Mat3, size: Vec3, position: Vec3) -> Self {
        Target {
            model: Mat4::from_cols(
                (orientation.x_axis * size.x).extend(0.0),
                (orientation.y_axis * size.y).extend(0.0),
                (orientation.z_axis * size.z).extend(0.0),
                position.extend(1.0),
            ),
            ..self
        }
    }
}

/// Where every selected part stands, in selection order.
///
/// The transform gizmo stands at the [`centre`](Targets::centre) of the whole
/// selection's bounds, exactly the way
/// `rbx_viewer::renderer::selection::Selection::anchor` places the one it
/// draws — both call `rbx_viewer::gizmo::centre_of`, so the handles the user
/// can grab and the handles they can see cannot disagree about where they
/// are. The [`anchor`](Targets::anchor) is a different question: it is the
/// first entry with a placement, and it is what Scale and Rotate actually
/// transform, and whose own frame the local-orientation toggle takes.
/// [`Targets::translate`] is what a group drag uses to move every other part
/// by the same offset, which is what keeps the whole selection's relative
/// arrangement intact while only the gizmo's own travel is measured.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Targets(Vec<Target>);

impl Targets {
    /// Reads every part the selection covers out of the DOM, in the order
    /// `referents` lists them.
    ///
    /// A container resolves to the parts beneath it (`pick::parts_of`, the
    /// same function the renderer's outline is built from) rather than being
    /// dropped: a viewport click selects the outermost `Model` around what it
    /// hit, so dropping them left the commonest selection of all with no
    /// target, no handles, and nothing on screen to explain why. A container
    /// with no drawable geometry under it still yields nothing — there is
    /// genuinely nothing to transform.
    ///
    /// Selecting a `Model` *and* something inside it would otherwise name the
    /// same part twice, which a group drag would then move twice as far as
    /// the gizmo travelled. `pick::selection` drops the covered entry, here
    /// and for the outline `shell::selection::outlined` sends the renderer
    /// alike — the dedup belongs to both of them, so neither holds a copy of
    /// it that could drift from the other's.
    pub(crate) fn read(dom: &WeakDom, database: &ReflectionDatabase, referents: &[Ref]) -> Self {
        Targets(
            pick::selection(dom, database, referents)
                .iter()
                .flat_map(|entry| entry.parts())
                .filter_map(|&part| Target::read(dom, database, Some(part)))
                .collect(),
        )
    }

    /// The first part the selection covers: what Scale and Rotate transform,
    /// and whose own frame the local-orientation toggle takes. For a selected
    /// `Model` that is its own first descendant part — a `Model` has no
    /// `Size` or `CFrame` to write a resize or a turn into, exactly as a
    /// multi-part selection scales and rotates its anchor alone. Where the gizmo
    /// *sits* is [`Targets::centre`] instead — see this type's own doc
    /// comment for why the two are separate questions.
    pub(crate) fn anchor(&self) -> Option<Target> {
        self.0.first().copied()
    }

    /// Where the gizmo stands: the centre of the world-axis-aligned box
    /// containing every selected part, which for a single part is simply that
    /// part's own centre.
    ///
    /// Shared with the renderer through `rbx_viewer::gizmo::centre_of` rather
    /// than worked out again here, for the same reason the handle geometry
    /// itself is shared — two derivations of "where the gizmo is" are two
    /// things that can drift apart.
    pub(crate) fn centre(&self) -> Option<Vec3> {
        gizmo::centre_of(self.0.iter().map(|target| target.model))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Target> {
        self.0.iter()
    }

    /// How many parts the selection covers.
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// The box the Scale handles stand on — see `rbx_viewer::gizmo::scale_box`,
    /// which the renderer draws them from.
    pub(crate) fn scale_box(&self) -> Option<Mat4> {
        gizmo::scale_box(self.0.iter().map(|target| target.model))
    }

    /// Every part scaled by the same `factor` about `pivot`, from where it
    /// stood in `held` — the selection as it was when the handle was grabbed,
    /// so a whole gesture is one absolute factor rather than a running
    /// product that drifts. A group has no one `Size` to write, so it scales
    /// the way `Model:ScaleTo` does: each part's size by the factor, each
    /// centre by the factor along its offset from the pivot, orientations
    /// untouched. Replaces this value and returns what `Shell` writes: each
    /// referent's new size and position.
    pub(crate) fn scale_about(
        &mut self,
        held: &Targets,
        pivot: Vec3,
        factor: f32,
    ) -> Vec<(Ref, Vec3, Vec3)> {
        self.0 = held
            .0
            .iter()
            .map(|target| {
                let size = target.size() * factor;
                let position = pivot + (target.position() - pivot) * factor;
                target.placed(target.orientation(), size, position)
            })
            .collect();
        self.0
            .iter()
            .map(|target| (target.referent, target.size(), target.position()))
            .collect()
    }

    /// The largest and smallest factor [`Targets::scale_about`] may take
    /// before some part's size leaves `min..=max` on some axis — the whole
    /// group stops growing when its biggest part hits the ceiling, exactly as
    /// a lone part stops at its own.
    pub(crate) fn factor_within(&self, factor: f32, min: f32, max: f32) -> f32 {
        let mut clamped = factor;
        for target in &self.0 {
            let size = target.size();
            for axis in 0..3 {
                if size[axis] > 0.0 {
                    clamped = clamped.clamp(min / size[axis], max / size[axis]);
                }
            }
        }
        clamped
    }

    /// Every part turned by `rotation` about `centre`, from where it stood in
    /// `held` (see [`Targets::scale_about`] for why the gesture's start is
    /// the reference): its orientation turned, its centre swung round the
    /// pivot with it, so the group keeps its own arrangement. Returns what
    /// `Shell` writes: each referent's new orientation and position.
    pub(crate) fn rotate_about(
        &mut self,
        held: &Targets,
        centre: Vec3,
        rotation: Mat3,
    ) -> Vec<(Ref, Mat3, Vec3)> {
        self.0 = held
            .0
            .iter()
            .map(|target| {
                let orientation = rotation * target.orientation();
                let position = centre + rotation * (target.position() - centre);
                target.placed(orientation, target.size(), position)
            })
            .collect();
        self.0
            .iter()
            .map(|target| (target.referent, target.orientation(), target.position()))
            .collect()
    }

    /// Moves every target by the same offset, which is what keeps a group
    /// drag from rearranging the selection relative to itself — every part
    /// travels exactly as far as the anchor's own gizmo drag did, no more and
    /// no less. Returns each referent's new absolute position (what `Shell`
    /// writes into the DOM) and updates this value in place so the next call
    /// in the same gesture measures from where the parts stand now.
    pub(crate) fn translate(&mut self, delta: glam::Vec3) -> Vec<(Ref, glam::Vec3)> {
        let moves: Vec<(Ref, glam::Vec3)> = self
            .0
            .iter()
            .map(|target| (target.referent, target.position() + delta))
            .collect();
        for target in &mut self.0 {
            *target = target.moved_to(target.position() + delta);
        }
        moves
    }

    /// Every target carried rigidly by `carry` (turned and moved as one)
    /// from where it stands here.
    pub(crate) fn carried(&self, carry: Mat4) -> Targets {
        Targets(
            self.0
                .iter()
                .map(|target| Target {
                    model: carry * target.model,
                    ..*target
                })
                .collect(),
        )
    }

    /// Replaces the anchor's own placement — what a Scale or Rotate drag
    /// updates as it goes, since only the anchor ever carries their gizmo
    /// (a multi-part selection's Size and Orientation have no group meaning
    /// the way a Move's position offset does).
    pub(crate) fn set_anchor(&mut self, target: Target) {
        if let Some(anchor) = self.0.first_mut() {
            *anchor = target;
        }
    }
}

#[cfg(test)]
#[path = "transform/tests.rs"]
mod tests;
