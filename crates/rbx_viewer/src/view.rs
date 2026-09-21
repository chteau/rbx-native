//! The renderer state that does not come from the place, and so is not
//! rebuilt from it.
//!
//! [`crate::Headless::reload`] — a Command Bar script's edit, a `Parent` move
//! that crossed the `Workspace` boundary, any fast path that reported it could
//! not take the shortcut — re-derives the whole scene from the new DOM, which
//! is the only thing guaranteed to draw the right picture. Everything the
//! *editor* asked for rather than the file is outside that derivation: the
//! projection mode, the outlined selection, the transform gizmo. Each one
//! silently missing afterwards is a visual regression with nothing on screen
//! to explain it — an outline that vanished, draggers that stopped being
//! drawn, a parallel projection that went back to perspective — and the
//! user's only recourse is to toggle the setting off and on again. (This
//! happened, back when a reload built a whole new renderer and lost the
//! three with the old one.)
//!
//! Collecting them into one value that `Offscreen::new` and
//! `Offscreen::reload` both *require* is the point: there is no way to build
//! or rebuild a renderer without saying what view it is for, so a rebuild
//! cannot quietly drop one of these — whatever it does or does not happen to
//! keep of the renderer's own copy.

use glam::Mat4;

use crate::gizmo::Gizmo;
use crate::pick::Selected;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct View {
    pub(crate) orthographic: bool,
    /// Outlined in the viewport — every instance in the selection, not just
    /// one: `rbxstudio`'s `Shift`/`Ctrl`/`Cmd`-click adds another top-level
    /// object rather than replacing it. Each entry already carries the parts
    /// it covers (see [`Selected`]), which is what survives a rebuild: the
    /// renderer has no DOM to resolve a bare referent against.
    pub(crate) selected: Vec<Selected>,
    /// The "about to click" cue's outline — see `renderer::hover::Hover`.
    /// One entry per hovered instance (a model as its aggregate box, a part
    /// as its own), what a click would select. Empty while `rbxview` runs,
    /// which never asks for a hover outline in the first place.
    pub(crate) hovered: Vec<Selected>,
    /// `None` whenever no transform tool is active, which is every `rbxview`
    /// frame: the standalone viewer edits nothing.
    pub(crate) gizmo: Option<Gizmo>,
    /// The ghost boxes a tool is previewing — where the Align tool would
    /// put the selection, say. Empty unless an editor asked for one, and
    /// carried here for the same reason the selection is: a reload must
    /// not quietly drop it while the tool is still open.
    pub(crate) preview: Vec<Mat4>,
    /// Whether a part standing in front of the selection hides its outline.
    /// `false` — the box shows through everything, which is what Studio
    /// draws — unless the embedder asks otherwise; `rbxview` never does.
    pub(crate) selection_occluded: bool,
}

impl View {
    /// Replaces the outlined selection — never adds to it, the way selecting
    /// another row in the Explorer replaces rather than extends.
    pub(crate) fn select(&mut self, selected: &[Selected]) {
        self.selected.clear();
        self.selected.extend_from_slice(selected);
    }

    /// Replaces the hovered outline — the instances a hover covers, empty to
    /// clear it.
    pub(crate) fn set_hover(&mut self, selected: Vec<Selected>) {
        self.hovered = selected;
    }

    /// Replaces the previewed boxes — empty to clear them.
    pub(crate) fn set_preview(&mut self, boxes: Vec<Mat4>) {
        self.preview = boxes;
    }

    pub(crate) fn set_gizmo(&mut self, gizmo: Option<Gizmo>) {
        self.gizmo = gizmo;
    }

    pub(crate) fn set_orthographic(&mut self, orthographic: bool) {
        self.orthographic = orthographic;
    }

    pub(crate) fn set_selection_occluded(&mut self, occluded: bool) {
        self.selection_occluded = occluded;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gizmo::Kind;
    use rbx_dom::Ref;

    fn part(referent: u32) -> Selected {
        Selected::part(Ref::new(referent))
    }

    #[test]
    fn a_fresh_view_outlines_nothing_and_draws_no_draggers() {
        // What `rbxview` renders every frame with, and what the editor starts
        // from before anything is selected.
        let view = View::default();

        assert!(view.selected.is_empty());
        assert!(view.hovered.is_empty());
        assert_eq!(view.gizmo, None);
        assert!(!view.orthographic);
        assert!(
            !view.selection_occluded,
            "the selection box draws through geometry unless asked otherwise"
        );
    }

    /// The occlusion choice lives in `View` rather than only in the renderer
    /// for the reason this module exists: a reload rebuilds the renderer, and
    /// a preference that only the renderer knew about would silently go back
    /// to its default there with nothing on screen to explain it.
    #[test]
    fn the_selection_occlusion_choice_survives_in_the_view() {
        let mut view = View::default();
        view.set_selection_occluded(true);
        assert!(view.selection_occluded);
        view.set_selection_occluded(false);
        assert!(!view.selection_occluded);
    }

    #[test]
    fn selecting_replaces_rather_than_accumulates() {
        let mut view = View::default();
        view.select(&[part(1), part(2)]);
        view.select(&[part(3)]);

        assert_eq!(view.selected, [part(3)]);
    }

    #[test]
    fn selecting_nothing_clears_the_outline() {
        let mut view = View::default();
        view.select(&[part(1)]);
        view.select(&[]);

        assert!(view.selected.is_empty());
    }

    #[test]
    fn hovering_replaces_rather_than_accumulates() {
        let mut view = View::default();
        view.set_hover(vec![Selected::part(Ref::new(1))]);
        view.set_hover(vec![Selected::part(Ref::new(2))]);

        assert_eq!(view.hovered, vec![Selected::part(Ref::new(2))]);
    }

    #[test]
    fn hovering_nothing_clears_the_outline() {
        let mut view = View::default();
        view.set_hover(vec![Selected::part(Ref::new(1))]);
        view.set_hover(Vec::new());

        assert!(view.hovered.is_empty());
    }

    /// The regression this type exists for: everything the editor asked for
    /// between one rebuild and the next has to still be here when the next one
    /// happens, because this value is the whole of what `Offscreen::reload`
    /// puts back. Before it existed, a Command Bar script's reload left the
    /// gizmo (and the outline, and the projection mode) switched off until
    /// the user toggled each one by hand.
    #[test]
    fn a_rebuild_is_handed_everything_the_editor_last_asked_for() {
        let mut view = View::default();
        view.set_orthographic(true);
        view.select(&[part(9)]);
        view.set_hover(vec![Selected::part(Ref::new(7))]);
        view.set_gizmo(Some(Gizmo {
            kind: Kind::Rotate,
            local: true,
        }));

        // A tool switched away and back, and the local toggle flipped: what a
        // rebuild gets is the latest of each, not the first.
        view.set_gizmo(None);
        view.set_gizmo(Some(Gizmo {
            kind: Kind::Move,
            local: false,
        }));

        assert!(view.orthographic);
        assert_eq!(view.selected, [part(9)]);
        assert_eq!(view.hovered, vec![Selected::part(Ref::new(7))]);
        assert_eq!(
            view.gizmo,
            Some(Gizmo {
                kind: Kind::Move,
                local: false,
            })
        );
    }
}
