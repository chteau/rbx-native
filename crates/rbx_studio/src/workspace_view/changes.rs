//! Two ways the rest of the shell reaches into the 3D view from outside its
//! own render loop: passing on a selection, and reflecting an edit to the
//! DOM.

use rbx_dom::{Change, Ref, WeakDom};

use super::WorkspaceView;

impl WorkspaceView {
    /// Forwards the Explorer's selection, forcing one frame even at rest.
    pub(crate) fn set_selection(&mut self, referents: &[Ref]) {
        self.pump.select(referents.to_vec());
    }

    /// Reflects one edit's `Change` log in the render thread's scene, every
    /// instance it names patched in place there, between two frames (see
    /// `rbx_viewer::Headless::apply_changes`) — the render thread owns the
    /// viewer, so that is where the patch happens. `dom` is what the log is
    /// read against: the tree as it stands after the edit, whichever way the
    /// edit went (an undo hands over the mutation's own log with the
    /// restored tree).
    pub(crate) fn apply_changes(&mut self, dom: WeakDom, changes: Vec<Change>) {
        self.pump.apply_changes(dom, changes);
    }
}
