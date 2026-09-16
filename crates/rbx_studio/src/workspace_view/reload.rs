//! Two ways the rest of the shell reaches into the 3D view from outside its
//! own render loop: passing on a selection, and reflecting a Command Bar
//! script's edit to the DOM.

use rbx_dom::{Ref, WeakDom};
use rbx_viewer::pick::Selected;

use super::WorkspaceView;

impl WorkspaceView {
    /// Forwards the Explorer's selection, forcing one frame even at rest.
    pub(crate) fn set_selection(&mut self, selected: &[Selected]) {
        self.pump.select(selected.to_vec());
    }

    /// Rebuilds the render thread's scene from a mutated DOM, forcing one
    /// frame even at rest — the render thread owns the viewer, so the rebuild
    /// happens there, between two frames.
    pub(crate) fn reload(&mut self, dom: WeakDom) {
        self.pump.reload(dom);
    }

    /// The fast path for a `Lighting`/`Atmosphere`/`Clouds`/`PostEffect`/
    /// `Light` edit (see `shell::edit::ViewportEdit::Lighting`): recomputes
    /// just the lighting uniform and local-light buffer on the render thread,
    /// falling back to a full [`WorkspaceView::reload`] there if that itself
    /// reports it cannot (see `rbx_viewer::Headless::update_lighting`).
    pub(crate) fn update_lighting(&mut self, dom: WeakDom) {
        self.pump.update_lighting(dom);
    }

    /// The fast path for a single `BasePart` edit (see
    /// `shell::edit::ViewportEdit::Instance`): patches `referent`'s one GPU
    /// instance on the render thread, falling back the same way.
    pub(crate) fn patch_instance(&mut self, dom: WeakDom, referent: Ref) {
        self.pump.patch_instance(dom, referent);
    }

    /// The fast path for a `ParticleEmitter`/`Beam`/`Trail` edit (see
    /// `shell::edit::ViewportEdit::Effect`): re-plans that effect list on the
    /// render thread, falling back the same way.
    pub(crate) fn patch_effect(&mut self, dom: WeakDom, referent: Ref) {
        self.pump.patch_effect(dom, referent);
    }

    /// The fast path for a `Parent` change (see
    /// `shell::edit::ViewportEdit::Reparent`): checks on the render thread
    /// that the move stayed inside `Workspace`, and redraws nothing if so,
    /// falling back the same way if not.
    pub(crate) fn reparent(&mut self, dom: WeakDom, referent: Ref) {
        self.pump.reparent(dom, referent);
    }
}
