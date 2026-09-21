//! Two ways the rest of the shell reaches into the 3D view from outside its
//! own render loop: passing on a selection or a hover, and reflecting an edit
//! to the DOM.

use rbx_dom::{Change, Snapshot};
use rbx_viewer::pick::Selected;

use super::WorkspaceView;

impl WorkspaceView {
    /// Forwards the Explorer's selection, forcing one frame even at rest.
    pub(crate) fn set_selection(&mut self, selected: &[Selected]) {
        self.pump.select(selected.to_vec());
    }

    /// Forwards the instances a hover covers (empty to clear it), forcing one
    /// frame even at rest — the same reason `set_selection` above does.
    pub(crate) fn set_hover(&mut self, selected: Vec<Selected>) {
        self.pump.hover(selected);
    }

    /// Forwards where a tool being configured would put the selection —
    /// the Align popover's live preview. An empty list clears it.
    pub(crate) fn set_preview(&mut self, boxes: Vec<glam::Mat4>) {
        self.pump.preview(boxes);
    }

    /// Reflects one edit's `Change` log in the render thread's scene, every
    /// instance it names patched in place there, between two frames (see
    /// `rbx_viewer::Headless::apply_changes`) — the render thread owns the
    /// viewer, so that is where the patch happens. `snapshots` is what the
    /// log is read against: the instances it names as they stand after the
    /// edit, whichever way the edit went (an undo hands over the mutation's
    /// own log with snapshots off the restored tree), brought into the
    /// render thread's own copy of the DOM before the patch.
    pub(crate) fn apply_changes(&mut self, snapshots: Vec<Snapshot>, changes: Vec<Change>) {
        self.pump.apply_changes(snapshots, changes);
    }
}
