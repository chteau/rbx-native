//! The viewport's transform toolbar: which of Studio's tools is active,
//! whether its draggers follow the world's axes or the part's own, and the
//! keystrokes that change either.
//!
//! The state lives in [`crate::shell::Shell`] — the toolbar renders from it —
//! and is pushed down to [`crate::workspace_view::WorkspaceView`], which
//! hit-tests the cursor against the draggers, and on to the render thread,
//! which draws them.
//!
//! Shortcuts and behaviour follow `creator-docs`
//! (`parts/index.md#transform-parts`): `2` for Move, `Ctrl`/`Cmd`+`L` for
//! local orientation. Scale (`3`) and Rotate (`4`) are not implemented yet and
//! are deliberately not bound — a shortcut that silently does nothing is worse
//! than one that visibly isn't there.

use glam::Mat4;
use gpui_kit::Modifiers;
use rbx_dom::{Ref, WeakDom};
use rbx_viewer::Gizmo;

/// A transform tool the viewport can actually carry out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Tool {
    /// Click to select, and nothing else — Studio's own default.
    #[default]
    Select,
    Move,
}

impl Tool {
    pub(crate) const ALL: [Tool; 2] = [Tool::Select, Tool::Move];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Move => "Move",
        }
    }

    /// The key that picks this tool, for the toolbar button's own label.
    pub(crate) fn shortcut(self) -> &'static str {
        match self {
            Tool::Select => "1",
            Tool::Move => "2",
        }
    }
}

/// Everything the transform toolbar holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Transform {
    pub(crate) tool: Tool,
    /// Draggers along the part's own axes rather than the world's.
    pub(crate) local: bool,
}

impl Transform {
    /// What the renderer should draw over the selection, if anything — the
    /// Select tool has no draggers of its own.
    pub(crate) fn gizmo(self) -> Option<Gizmo> {
        matches!(self.tool, Tool::Move).then_some(Gizmo { local: self.local })
    }

    /// Whether dragging in the viewport moves the selected part at all.
    pub(crate) fn drags(self) -> bool {
        matches!(self.tool, Tool::Move)
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
        _ => None,
    }
}

/// Where the draggers stand: the selected part, and the matrix it is drawn
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

    pub(crate) fn position(&self) -> glam::Vec3 {
        self.model.w_axis.truncate()
    }

    /// The same part standing somewhere else — what a drag in progress shows
    /// while `Shell` is still writing the move into the DOM.
    pub(crate) fn moved_to(self, position: glam::Vec3) -> Self {
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

    /// The part's own axes, still carrying its `Size` in their lengths —
    /// `rbx_viewer::gizmo::basis` normalizes them.
    pub(crate) fn rotation(&self) -> glam::Mat3 {
        glam::Mat3::from_mat4(self.model)
    }
}

#[cfg(test)]
#[path = "transform/tests.rs"]
mod tests;
