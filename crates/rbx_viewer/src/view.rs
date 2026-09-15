//! The renderer state that does not come from the place, and so does not
//! survive rebuilding the renderer from it.
//!
//! [`crate::Headless::reload`] — a Command Bar script's edit, a `Parent` move
//! that crossed the `Workspace` boundary, any fast path that reported it could
//! not take the shortcut — throws the whole renderer away and builds a new one
//! from the new DOM, which is the only thing guaranteed to draw the right
//! picture. Everything the *editor* asked for rather than the file goes with
//! it: the projection mode, the outlined selection, the transform gizmo. Each
//! one silently missing afterwards is a visual regression with nothing on
//! screen to explain it — an outline that vanished, draggers that stopped
//! being drawn, a parallel projection that went back to perspective — and the
//! user's only recourse is to toggle the setting off and on again.
//!
//! Collecting them into one value that `Offscreen::new` *requires* is the
//! point: there is no way to build a renderer without saying what view it is
//! being built for, so a rebuild cannot quietly drop one of these again.

use rbx_dom::Ref;

use crate::gizmo::Gizmo;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct View {
    pub(crate) orthographic: bool,
    /// Outlined in the viewport. At most one instance today — `rbxstudio`
    /// selects one at a time — but kept as a list because the outline pass
    /// already takes one, and a multi-select would otherwise have to change
    /// this too.
    pub(crate) selected: Vec<Ref>,
    /// `None` whenever no transform tool is active, which is every `rbxview`
    /// frame: the standalone viewer edits nothing.
    pub(crate) gizmo: Option<Gizmo>,
}

impl View {
    /// Replaces the outlined selection — never adds to it, the way selecting
    /// another row in the Explorer replaces rather than extends.
    pub(crate) fn select(&mut self, referents: &[Ref]) {
        self.selected.clear();
        self.selected.extend_from_slice(referents);
    }

    pub(crate) fn set_gizmo(&mut self, gizmo: Option<Gizmo>) {
        self.gizmo = gizmo;
    }

    pub(crate) fn set_orthographic(&mut self, orthographic: bool) {
        self.orthographic = orthographic;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_view_outlines_nothing_and_draws_no_draggers() {
        // What `rbxview` renders every frame with, and what the editor starts
        // from before anything is selected.
        let view = View::default();

        assert!(view.selected.is_empty());
        assert_eq!(view.gizmo, None);
        assert!(!view.orthographic);
    }

    #[test]
    fn selecting_replaces_rather_than_accumulates() {
        let mut view = View::default();
        view.select(&[Ref::new(1), Ref::new(2)]);
        view.select(&[Ref::new(3)]);

        assert_eq!(view.selected, [Ref::new(3)]);
    }

    #[test]
    fn selecting_nothing_clears_the_outline() {
        let mut view = View::default();
        view.select(&[Ref::new(1)]);
        view.select(&[]);

        assert!(view.selected.is_empty());
    }

    /// The regression this type exists for: everything the editor asked for
    /// between one rebuild and the next has to still be here when the next one
    /// happens, because this value is the whole of what `Offscreen::new`
    /// carries across. Before it existed, a Command Bar script's reload left
    /// the gizmo (and the outline, and the projection mode) switched off until
    /// the user toggled each one by hand.
    #[test]
    fn a_rebuild_is_handed_everything_the_editor_last_asked_for() {
        let mut view = View::default();
        view.set_orthographic(true);
        view.select(&[Ref::new(9)]);
        view.set_gizmo(Some(Gizmo { local: true }));

        // A tool switched away and back, and the local toggle flipped: what a
        // rebuild gets is the latest of each, not the first.
        view.set_gizmo(None);
        view.set_gizmo(Some(Gizmo { local: false }));

        assert!(view.orthographic);
        assert_eq!(view.selected, [Ref::new(9)]);
        assert_eq!(view.gizmo, Some(Gizmo { local: false }));
    }
}
