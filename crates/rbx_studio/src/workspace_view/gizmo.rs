//! The left mouse button over the 3D view: clicking to select, and dragging a
//! part by the Move tool's draggers or by its own body.
//!
//! All of it happens here on the UI thread, against `rbx_viewer`'s own
//! geometry ([`rbx_viewer::pick`], [`rbx_viewer::gizmo`]) and the camera the
//! render thread reports with each frame — the renderer builds the handles the
//! user sees from exactly the same functions, so what is grabbable is what is
//! drawn. Nothing here touches the DOM: `Shell` owns that, and hears about a
//! click or a drag through [`super::ViewportAction`].
//!
//! The camera's own gestures are untouched: look and orbit are the *right*
//! button (see `WorkspaceView::begin_look`), so a left-button drag never
//! competes with them.

use glam::{Vec2, Vec3};
use gpui_kit::{Modifiers, Pixels, Point};
use rbx_viewer::gizmo::{self, Handles};
use rbx_viewer::pick::{self, Ray};

use super::{ViewportAction, WorkspaceView};
use crate::settle::Settle;

/// A left-button drag in progress.
#[derive(Debug, Clone, Copy)]
pub(super) enum Drag {
    /// One of the Move tool's arrows is held: the axis line it slides along,
    /// and how far along that line the cursor stood when it was grabbed.
    Axis {
        origin: Vec3,
        axis: Vec3,
        grabbed: f32,
    },
    /// The part's own body is held — Studio's "cursor dragging". The part
    /// comes to rest on whatever the cursor is over (see [`crate::settle`]);
    /// with nothing under the cursor it travels instead in the plane that
    /// faced the camera through the grab point, keeping the part where it was
    /// relative to the cursor.
    ///
    /// The plane is fixed in the world at the moment of the grab rather than
    /// recomputed from the live camera: a plane that turned with the view
    /// would slide the part every time the camera did, which is not what
    /// holding it still means.
    ///
    /// Studio additionally soft-snaps onto nearby *edges*; nothing here does.
    Plane {
        point: Vec3,
        normal: Vec3,
        offset: Vec3,
    },
}

impl Drag {
    /// What `Shell` needs to rest the part on the scene for this cursor ray:
    /// the grab, as the ray that made it, and where the part stood then. An
    /// axis drag is pinned to its line and never settles.
    pub(super) fn settle(self, cursor: Ray) -> Option<Settle> {
        match self {
            Drag::Axis { .. } => None,
            Drag::Plane {
                point,
                normal,
                offset,
            } => Some(Settle {
                cursor,
                // The plane faces back along the grab ray, so the ray itself
                // is the plane's normal reversed, starting where it hit.
                grab: Ray::new(point, -normal),
                centre: point + offset,
            }),
        }
    }
}

impl WorkspaceView {
    /// The world-space ray under a window-space cursor position, or `None`
    /// before the panel has a size or the render thread has reported a camera.
    pub(super) fn cursor_ray(&self, position: Point<Pixels>, scale: f32) -> Option<Ray> {
        let pose = self.view?;
        let viewport = self.viewport.get();
        if viewport.size.0 == 0 || viewport.size.1 == 0 {
            return None;
        }

        // GPUI reports logical pixels against the window; the panel's own rect
        // is kept in physical ones (see `frame::Viewport`), which is also what
        // the frame under the cursor was rendered at.
        let pixel = Vec2::new(
            f32::from(position.x) * scale - viewport.origin.0 as f32,
            f32::from(position.y) * scale - viewport.origin.1 as f32,
        );
        let extent = Vec2::new(viewport.size.0 as f32, viewport.size.1 as f32);
        let projection = pose.view_projection(self.orthographic, extent.x / extent.y);
        Some(pick::ray_through(projection, pick::ndc_of(pixel, extent)))
    }

    /// Where the draggers stand this frame, built the same way
    /// `rbx_viewer::renderer` builds the ones on screen.
    fn handles(&self) -> Option<Handles> {
        let target = self.target?;
        let pose = self.view?;
        let origin = target.position();
        Some(Handles::new(
            origin,
            gizmo::basis(self.transform.local.then(|| target.rotation())),
            gizmo::arm_length(origin, pose, self.orthographic),
        ))
    }

    /// The left button going down: grab a dragger, grab the selected part, or
    /// — failing both — ask `Shell` to resolve a pick against the DOM.
    pub(super) fn press(
        &mut self,
        position: Point<Pixels>,
        modifiers: Modifiers,
        scale: f32,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        self.drag = None;
        let Some(ray) = self.cursor_ray(position, scale) else {
            return;
        };

        if self.transform.drags() {
            if let Some(drag) = self.grab(ray) {
                self.drag = Some(drag);
                self.dragged = false;
                return;
            }
        }

        cx.emit(ViewportAction::Pick {
            ray,
            // Studio's selection cycling: `Alt`/`⌥`-click steps to the next
            // object behind the current one instead of selecting a model.
            cycling: modifiers.alt,
        });
    }

    /// What this ray grabs on the current selection, if anything: a dragger
    /// first, then the part's own body.
    fn grab(&self, ray: Ray) -> Option<Drag> {
        let handles = self.handles()?;
        if let Some(axis) = handles.grab(ray) {
            let axis = handles.direction(axis);
            let origin = handles.origin();
            return Some(Drag::Axis {
                origin,
                axis,
                grabbed: gizmo::along_axis(origin, axis, ray)?,
            });
        }

        let target = self.target?;
        let point = ray.at(pick::ray_hits_box(ray, target.model)?);
        Some(Drag::Plane {
            point,
            // Square to the view at the moment of the grab, which is the one
            // orientation every cursor position on screen has an answer in.
            normal: -ray.direction,
            offset: target.position() - point,
        })
    }

    /// The cursor moving with a drag held: works out where the part stands
    /// now and tells `Shell`, which is what writes it into the DOM.
    pub(super) fn drag_to(
        &mut self,
        position: Point<Pixels>,
        scale: f32,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        let (Some(drag), Some(target), Some(ray)) =
            (self.drag, self.target, self.cursor_ray(position, scale))
        else {
            return;
        };
        let Some(position) = moved_to(drag, ray) else {
            return;
        };
        if position == target.position() {
            return;
        }

        // Kept here as well as written into the DOM: the draggers have to
        // follow the cursor within this same gesture, and the DOM's answer
        // only comes back through `set_target` once `Shell` has applied it.
        self.target = Some(target.moved_to(position));

        let first = !std::mem::replace(&mut self.dragged, true);
        cx.emit(ViewportAction::Moved {
            referent: target.referent,
            position,
            first,
            settle: drag.settle(ray),
        });
    }

    /// Where `Shell` actually put the part for the move this gesture just
    /// asked for, when it rested it on a surface the view itself cannot see.
    /// Unlike [`WorkspaceView::set_target`] this is taken mid-gesture: it is
    /// the drag's own answer, finished with the DOM, not a round trip that
    /// could land a frame late.
    pub(crate) fn settle_at(&mut self, position: Vec3) {
        if self.drag.is_some() {
            self.target = self.target.map(|target| target.moved_to(position));
        }
    }

    /// Whether a drag is under way, which is what keeps a moving cursor from
    /// also being reported to the camera.
    pub(super) fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub(super) fn end_drag(&mut self) {
        self.drag = None;
    }
}

/// Where the dragged part's centre stands for this cursor ray, or `None` when
/// the gesture has no answer at this angle — an axis sighted end-on, or a
/// drag plane the ray has turned parallel to (or ended up behind).
///
/// Pure, and the whole of what a drag computes: the state above is a grab's
/// worth of geometry, and this turns it plus a ray into a position.
pub(super) fn moved_to(drag: Drag, ray: Ray) -> Option<Vec3> {
    match drag {
        Drag::Axis {
            origin,
            axis,
            grabbed,
        } => Some(origin + axis * (gizmo::along_axis(origin, axis, ray)? - grabbed)),
        Drag::Plane {
            point,
            normal,
            offset,
        } => Some(pick::ray_hits_plane(ray, point, normal)? + offset),
    }
}

#[cfg(test)]
#[path = "gizmo/tests.rs"]
mod tests;
