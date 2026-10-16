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
//! Rotate, `Ctrl`/`Cmd`+`L` for local orientation. `5` for Transform is this
//! editor's own extension of that sequence: `creator-docs` documents no
//! shortcut for it (see [`Tool::Transform`]), so continuing the run of
//! digits the other three already use is this editor's own choice, not a
//! confirmed one.

use gpui_kit::{Keystroke, Modifiers};
use rbx_viewer::gizmo::Kind;

use crate::shortcuts::{self, Command};
use rbx_viewer::Gizmo;

mod frame;
mod targets;

pub(crate) use frame::{cframe, rigid};
pub(crate) use targets::{Target, Targets};

/// A transform tool the viewport can carry out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Tool {
    /// Click to select, and nothing else — Studio's own default.
    #[default]
    Select,
    Move,
    Scale,
    Rotate,
    /// Studio's own `Enum.RibbonTool.Transform`: Move's, Scale's and
    /// Rotate's handles all at once, over the same selection. Confirmed
    /// against the real tool rather than guessed at `creator-docs`' prose
    /// alone (which only uses "transform" as the umbrella name for the other
    /// three together, never as a distinct tool of its own): the API dump
    /// this project syncs daily (`assets/API-Dump.json`, `Enums` →
    /// `RibbonTool`, value `4`) and `creator-docs`' own enum reference
    /// (`reference/engine/enums/RibbonTool.yaml`) both give it this exact
    /// summary: "provides combined move, scale, and rotate handles in a
    /// single gizmo."
    Transform,
    /// Point at the scene to place the sun or the moon — see `crate::sun`.
    /// Not a transform: it has no handles and never touches the selection.
    Sun,
    /// Studio's Edit Pivot, on the Model tab (`studio/pivot-tools.md`):
    /// handles on the selected part's or model's pivot that move and turn
    /// the pivot alone, the geometry staying where it is. One part or one
    /// model at a time — a selection of several has no one pivot to edit.
    Pivot,
    /// The Terrain Editor's tool under the cursor (`crate::terrain`): a
    /// brush or the selection region. Like Sun, no handles of its own and
    /// never touches the selection.
    Terrain,
}

impl Tool {
    /// The five the ribbon's Tools group draws; [`Tool::Sun`] has a group
    /// of its own.
    pub(crate) const TRANSFORM: [Tool; 5] = [
        Tool::Select,
        Tool::Move,
        Tool::Scale,
        Tool::Rotate,
        Tool::Transform,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Move => "Move",
            Tool::Scale => "Scale",
            Tool::Rotate => "Rotate",
            Tool::Transform => "Transform",
            Tool::Sun => "Sun",
            Tool::Pivot => "Edit Pivot",
            Tool::Terrain => "Terrain Editor",
        }
    }

    /// The key that picks this tool, for the toolbar button's own label.
    ///
    /// The Sun and Edit Pivot tools have none: any key over the 3D view past
    /// `5` is a camera key or an arbitrary pick nobody would guess, and
    /// `creator-docs` gives Edit Pivot no shortcut of its own.
    pub(crate) fn shortcut(self) -> Option<String> {
        let command = match self {
            Tool::Select => Command::ToolSelect,
            Tool::Move => Command::ToolMove,
            Tool::Scale => Command::ToolScale,
            Tool::Rotate => Command::ToolRotate,
            Tool::Transform => Command::ToolTransform,
            Tool::Sun | Tool::Pivot | Tool::Terrain => return None,
        };
        shortcuts::hint(command)
    }

    /// Which handles this tool puts over the selection, or `None` for Select
    /// and Sun, which have none of their own.
    pub(crate) fn kind(self) -> Option<Kind> {
        match self {
            Tool::Select | Tool::Sun | Tool::Terrain => None,
            Tool::Move => Some(Kind::Move),
            Tool::Scale => Some(Kind::Scale),
            Tool::Rotate => Some(Kind::Rotate),
            Tool::Transform => Some(Kind::Transform),
            Tool::Pivot => Some(Kind::Pivot),
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

/// The Animation Editor's own Move/Rotate mode for the joint gizmo.
///
/// creator-docs (`animation/editor.md`): "When creating poses, you can toggle
/// between Move and Rotate modes by pressing R. These modes work exactly like
/// moving and rotating base objects, including the snap settings and
/// incremental values located in Studio's toolbar."
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum JointTool {
    #[default]
    Rotate,
    Move,
}

impl JointTool {
    /// The toolbar tool whose handles this mode draws.
    pub(crate) fn tool(self) -> Tool {
        match self {
            JointTool::Rotate => Tool::Rotate,
            JointTool::Move => Tool::Move,
        }
    }
}

/// Everything the transform toolbar holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Transform {
    pub(crate) tool: Tool,
    /// Which handles the joint gizmo shows while posing, whatever the toolbar
    /// holds (see [`JointTool`]).
    pub(crate) joint: JointTool,
    /// Handles along the part's own axes rather than the world's.
    pub(crate) local: bool,
    /// The move/scale increment, in studs.
    pub(crate) translate: Snap,
    /// The rotate increment, in degrees.
    pub(crate) rotate: Snap,
    /// Edit Pivot's Snap checkbox: whether a dragged pivot jumps onto the
    /// selection's corners, edges and centres (see
    /// `rbx_viewer::gizmo::Faces::hotspots`).
    pub(crate) pivot_snap: bool,
    /// Whether the viewport is posing a bound rig in the Animation view,
    /// where a drag becomes a key on a joint rather than an edit to a part.
    /// A joint's key holds a position and a turn and nothing else, so the
    /// tools that change a part's size or its pivot draw nothing and drag
    /// nothing while this is on (see [`Transform::keyable`]).
    pub(crate) posing: bool,
}

impl Default for Transform {
    /// The rotate increment starts at an eighth of a turn: the docs give no
    /// default, and a degree increment that doesn't divide 90° evenly leaves a
    /// part unable to come back to square.
    fn default() -> Self {
        Transform {
            tool: Tool::default(),
            joint: JointTool::default(),
            local: false,
            translate: Snap::default(),
            rotate: Snap {
                increment: 45.0,
                ..Snap::default()
            },
            // The docs give no default; on, like the increments beside it.
            pivot_snap: true,
            posing: false,
        }
    }
}

impl Transform {
    /// What the renderer should draw over the selection, if anything.
    pub(crate) fn gizmo(self) -> Option<Gizmo> {
        self.effective().kind().map(|kind| Gizmo {
            kind,
            local: self.local,
            hotspots: self.effective() == Tool::Pivot && self.pivot_snap,
            ..Gizmo::default()
        })
    }

    /// Whether dragging in the viewport transforms the selected part at all.
    pub(crate) fn drags(self) -> bool {
        self.effective().kind().is_some()
    }

    /// Why `tool` can't be used right now, or `None` when it can. While
    /// posing, a drag is keyed onto a joint's `Transform`, which has no size
    /// and turns about the joint's own pivot (`C0`/`C1`).
    pub(crate) fn refusal(self, tool: Tool) -> Option<&'static str> {
        if !self.posing {
            return None;
        }
        match tool {
            Tool::Scale => Some("Scale can't be keyed on joints"),
            Tool::Transform => {
                Some("Its Scale handles can't be keyed on joints; use Move or Rotate")
            }
            Tool::Pivot => Some("A joint turns about its own pivot (C0/C1)"),
            _ => None,
        }
    }

    /// Whether `tool` can be used right now (see [`Transform::refusal`]).
    pub(crate) fn keyable(self, tool: Tool) -> bool {
        self.refusal(tool).is_none()
    }

    /// What the 3D view is handed: this, with the tool it acts with in place
    /// of the chosen one, so nothing in the view can grab a handle of a tool
    /// that can't be used right now.
    pub(crate) fn for_view(self) -> Transform {
        Transform {
            tool: self.effective(),
            ..self
        }
    }

    /// The tool the viewport acts with. While posing, every transform tool
    /// acts as the joint gizmo's mode, so a rig part never gets resize
    /// handles; Sun and Terrain, and everything outside posing, act as chosen.
    pub(crate) fn effective(self) -> Tool {
        if self.posing && !matches!(self.tool, Tool::Sun | Tool::Terrain) {
            self.joint.tool()
        } else {
            self.tool
        }
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
    /// Edit Pivot's Snap checkbox.
    TogglePivotSnap,
    /// Flip the joint gizmo between Move and Rotate while posing.
    ToggleJoint,
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
pub(crate) fn action_for(key: impl AsRef<str>, modifiers: Modifiers) -> Option<Action> {
    let keystroke = Keystroke {
        key: key.as_ref().into(),
        modifiers,
        key_char: None,
    };
    let bindings = [
        (Command::ToggleLocal, Action::ToggleLocal),
        (
            Command::FocusMoveIncrement,
            Action::FocusIncrement(SnapKind::Translate),
        ),
        (
            Command::FocusRotateIncrement,
            Action::FocusIncrement(SnapKind::Rotate),
        ),
        (Command::ToggleJoint, Action::ToggleJoint),
        (Command::ToolSelect, Action::Use(Tool::Select)),
        (Command::ToolMove, Action::Use(Tool::Move)),
        (Command::ToolScale, Action::Use(Tool::Scale)),
        (Command::ToolRotate, Action::Use(Tool::Rotate)),
        (Command::ToolTransform, Action::Use(Tool::Transform)),
    ];
    // ⌘ and Ctrl are one chord to the docs' shortcuts, whichever the
    // binding was recorded with: try the press with the two traded.
    let swapped = Keystroke {
        modifiers: Modifiers {
            control: modifiers.platform,
            platform: modifiers.control,
            ..modifiers
        },
        ..keystroke.clone()
    };
    let one_of_them = modifiers.control != modifiers.platform;
    bindings
        .into_iter()
        .find(|(command, _)| {
            shortcuts::pressed(*command, &keystroke)
                || (one_of_them && shortcuts::pressed(*command, &swapped))
        })
        .map(|(_, action)| action)
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

#[cfg(test)]
#[path = "transform/tests.rs"]
mod tests;
