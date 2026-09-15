//! What the left mouse button in the 3D view asks of the editor: resolve a
//! click against the DOM, or move a part a drag is carrying.
//!
//! The split is deliberate. `WorkspaceView` has the cursor, the camera and the
//! draggers' geometry but no DOM; `Shell` has the DOM, the Explorer and the
//! undo stack but no idea where the cursor is. Everything in between travels
//! as a [`ViewportAction`].

use glam::Vec3;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_viewer::pick::{self, Ray};

use crate::properties;
use crate::transform::Target;
use crate::workspace_view::ViewportAction;

use super::{selection, Shell};

/// The property a viewport move writes. Only its position changes — the
/// rotation `properties::edit::commit` carries through untouched is what keeps
/// a dragged part facing the way it was.
const CFRAME_PROPERTY: &str = "CFrame";

impl Shell {
    pub(super) fn handle_viewport_action(
        &mut self,
        action: &ViewportAction,
        cx: &mut Context<Self>,
    ) {
        match *action {
            ViewportAction::Pick { ray, cycling } => self.pick_in_viewport(ray, cycling, cx),
            ViewportAction::Moved {
                referent,
                position,
                first,
            } => self.move_part(referent, position, first, cx),
            ViewportAction::Tool(action) => self.transform_action(action, cx),
        }
    }

    /// A click in the 3D view: select whatever it resolves to, or clear the
    /// selection when it resolves to nothing — clicking the sky deselects,
    /// the same as clicking empty space in the Explorer would.
    fn pick_in_viewport(&mut self, ray: Ray, cycling: bool, cx: &mut Context<Self>) {
        let hits = pick::parts_along(&self.dom, &self.database, ray);
        let picked =
            selection::from_click(&self.dom, &self.database, &hits, self.selected(), cycling);

        match picked {
            Some(referent) => self.select(referent, cx),
            None => self.deselect(cx),
        }
    }

    /// One step of a drag.
    ///
    /// History is pushed once, on `first`: a snapshot is a whole `WeakDom`
    /// clone (see `crate::history`), so one per mouse move would both cost
    /// a copy of the place per frame and flush every earlier undo step out of
    /// a fifty-deep stack in under a second. The write itself goes through the
    /// same `properties::edit::commit` the Properties panel uses, so a drag
    /// and a typed coordinate cannot disagree about what moving a part means.
    fn move_part(&mut self, referent: Ref, position: Vec3, first: bool, cx: &mut Context<Self>) {
        if first {
            self.push_history();
        }

        let text = format!("{}, {}, {}", position.x, position.y, position.z);
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let written =
            properties::edit::commit(&mut dom, &self.database, referent, CFRAME_PROPERTY, &text);
        self.dom = dom;

        if let Err(err) = written {
            self.output.push_warning(&format!("viewport drag: {err}"));
            return;
        }

        self.reflect_in_viewport(referent, CFRAME_PROPERTY, cx);
        cx.notify();
    }

    /// Tells the viewport where the selected part stands now, so its draggers
    /// follow an edit that moved or resized it — a typed coordinate, an undo,
    /// or a Command Bar script.
    pub(super) fn sync_gizmo_target(&mut self, reference: Ref, cx: &mut Context<Self>) {
        if self.selected() != Some(reference) {
            return;
        }

        let target = Target::read(&self.dom, Some(reference));
        self.viewport
            .update(cx, |viewport, _| viewport.set_target(target));
    }
}
