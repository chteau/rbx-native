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
use crate::settle::{self, Settle};
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
                settle,
            } => self.move_part(referent, position, first, settle, cx),
            ViewportAction::Tool(action) => self.transform_action(action, cx),
        }
    }

    /// A click in the 3D view: select whatever it resolves to, or clear the
    /// selection when it resolves to nothing — clicking the sky deselects,
    /// the same as clicking empty space in the Explorer would.
    fn pick_in_viewport(&mut self, ray: Ray, cycling: bool, cx: &mut Context<Self>) {
        // The viewport holds a handle onto the render thread's own mesh data
        // (see `WorkspaceView::meshes`): what keeps a `MeshPart`'s pick on the
        // triangles actually drawn rather than the box around them.
        let meshes = self.viewport.read(cx).meshes().clone();
        let hits = pick::parts_along(&self.dom, &self.database, &meshes, ray);
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
    ///
    /// A cursor drag asks, through `settle`, to rest the part on whatever the
    /// cursor is over. Only the DOM can answer that, so it is answered here,
    /// and the answer is handed back to the view: its draggers are following
    /// its own flat-plane guess until told otherwise.
    fn move_part(
        &mut self,
        referent: Ref,
        position: Vec3,
        first: bool,
        settle: Option<Settle>,
        cx: &mut Context<Self>,
    ) {
        if first {
            self.push_history();
        }

        let settled =
            settle.and_then(|settle| settle::settled(&self.dom, &self.database, referent, settle));
        let position = settled.unwrap_or(position);

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
        if settled.is_some() {
            self.viewport
                .update(cx, |viewport, _| viewport.settle_at(position));
        }
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
