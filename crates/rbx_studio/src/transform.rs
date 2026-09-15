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
use rbx_viewer::gizmo::{self, Kind};
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

/// Everything the transform toolbar holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Transform {
    pub(crate) tool: Tool,
    /// Handles along the part's own axes rather than the world's.
    pub(crate) local: bool,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Use(Tool),
    ToggleLocal,
}

/// Resolves a keystroke to a toolbar action, or `None` for anything else.
///
/// Bound where the 3D view has focus rather than window-wide (see
/// `WorkspaceView::key`): a bare digit is a character everywhere else in the
/// editor, and a tool shortcut that ate keystrokes out of the Command Bar or a
/// property field would be a bug, not a feature. `Shift`+`2` is deliberately
/// not Move either — Studio gives that chord to the snap increment field,
/// which does not exist here yet.
pub(crate) fn action_for(key: &str, modifiers: Modifiers) -> Option<Action> {
    let plain = !modifiers.control && !modifiers.alt && !modifiers.shift && !modifiers.platform;
    match key {
        // `platform` is Cmd on a Mac, where creator-docs gives the toggle as
        // ⌘L rather than Ctrl+L.
        "l" if (modifiers.control || modifiers.platform) && !modifiers.shift => {
            Some(Action::ToggleLocal)
        }
        _ if !plain => None,
        "1" => Some(Action::Use(Tool::Select)),
        "2" => Some(Action::Use(Tool::Move)),
        "3" => Some(Action::Use(Tool::Scale)),
        "4" => Some(Action::Use(Tool::Rotate)),
        _ => None,
    }
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
}

impl Target {
    /// Reads one instance's placement out of the DOM, or `None` for anything
    /// that isn't a part with a transform to drag (a `Folder`, a service, a
    /// `Model` — whose aggregate bounds nothing here derives yet).
    pub(crate) fn read(dom: &WeakDom, referent: Option<Ref>) -> Option<Self> {
        let referent = referent?;
        Some(Target {
            referent,
            model: rbx_viewer::pick::model_of(dom, referent)?,
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
    fn placed(self, orientation: Mat3, size: Vec3, position: Vec3) -> Self {
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

#[cfg(test)]
#[path = "transform/tests.rs"]
mod tests;
