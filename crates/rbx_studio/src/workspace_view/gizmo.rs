//! The left mouse button over the 3D view: clicking to select, and dragging a
//! part by a transform tool's handles or by its own body.
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

use glam::{Mat3, Mat4, Vec2, Vec3};
use gpui_kit::{Modifiers, Pixels, Point};
use rbx_viewer::gizmo::{self, Handles};
use rbx_viewer::pick::{self, Ray};
use rbx_viewer::snap;

use crate::transform::{Target, Tool};

use super::{ViewportAction, WorkspaceView};
use crate::settle::Settle;

/// `BasePart.Size`'s documented range: "the individual dimensions (length,
/// height, width) can be as low as `0.001` and as high as `2048`"
/// (`creator-docs`, `reference/engine/classes/BasePart.yaml`). A drag that ran
/// past either end would write a value the engine rejects.
const MIN_SIZE: f32 = 0.001;
const MAX_SIZE: f32 = 2048.0;

/// How near a part's surface a free drag's grab point has to pass before it
/// soft-snaps onto it, as a fraction of a dragger arm.
///
/// A fraction of the arm rather than a fixed number of studs because the arm
/// is itself screen-relative (see `gizmo::arm_length`): the pull then reaches
/// the same distance on screen whether the camera is on top of the part or
/// across the map from it, which is how it behaves in Studio. The docs
/// describe soft snapping and illustrate it but publish no threshold, so the
/// fraction is this editor's own.
const SOFT_SNAP_REACH: f32 = 0.35;

/// A left-button drag in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// With snapping off, the grab point soft-snaps onto the surfaces and
    /// edges of parts it passes near (see [`Landing`]), which is what settles
    /// a part against its neighbours instead of sliding it flat across the
    /// view.
    Plane {
        point: Vec3,
        normal: Vec3,
        offset: Vec3,
    },
    /// One of the Scale tool's blocks is held: the part's own axis the grabbed
    /// face sits on (pointing *out* through that face), where the cursor stood
    /// along it, and the placement the drag started from.
    Size {
        origin: Vec3,
        axis: Vec3,
        grabbed: f32,
        size: Vec3,
        /// Which component of `size` this axis is — `Size` is expressed in the
        /// part's own frame, so the axis alone does not say.
        component: usize,
    },
    /// One of the Rotate tool's rings is held: the ring's own frame, frozen at
    /// the grab (see [`gizmo::ring_crossing`]), the orientation the part
    /// started at, the angle last measured and how far the drag has turned in
    /// total.
    Ring {
        origin: Vec3,
        frame: (Vec3, Vec3, Vec3),
        orientation: Mat3,
        last: f32,
        turned: f32,
    },
}

/// What one step of a drag does to the part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Change {
    Position(Vec3),
    /// A new `Size`, and the centre that keeps the face *opposite* the grabbed
    /// one standing still.
    Size {
        size: Vec3,
        position: Vec3,
    },
    Orientation(Mat3),
}

impl Drag {
    /// What `Shell` needs to rest the part on the scene for this cursor ray:
    /// the grab, as the ray that made it, and where the part stood then. An
    /// axis drag is pinned to its line and never settles.
    pub(super) fn settle(self, cursor: Ray) -> Option<Settle> {
        match self {
            // Only a Move drag's own body-grab (`Plane`) settles onto a
            // surface — Scale and Rotate have no such concept, and an
            // axis-arrow Move drag already slides exactly along its axis.
            Drag::Axis { .. } | Drag::Size { .. } | Drag::Ring { .. } => None,
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

/// What a drag is allowed to land on for this one mouse move: the grid its
/// travel rounds to (`0.0` for no grid), and the parts its grab point can
/// soft-snap onto.
#[derive(Debug, Clone, Copy)]
pub(super) struct Landing<'a> {
    pub(super) grid: f32,
    pub(super) neighbours: &'a [Mat4],
    pub(super) reach: f32,
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

    /// Where the handles stand this frame, built the same way
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

    /// The left button going down: grab a handle, grab the selected part, or
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

    /// What this ray grabs on the current selection, if anything.
    fn grab(&self, ray: Ray) -> Option<Drag> {
        let handles = self.handles()?;
        let target = self.target?;
        match self.transform.tool {
            Tool::Select => None,
            // Only Move falls back to the part's own body: `creator-docs`
            // documents cursor dragging under Move alone.
            Tool::Move => self.grab_axis(&handles, ray).or_else(|| {
                let point = ray.at(pick::ray_hits_box(ray, target.model)?);
                Some(Drag::Plane {
                    point,
                    // Square to the view at the moment of the grab, which is
                    // the one orientation every cursor position on screen has
                    // an answer in.
                    normal: -ray.direction,
                    offset: target.position() - point,
                })
            }),
            Tool::Scale => grab_face(&handles, target, ray),
            Tool::Rotate => {
                let axis = handles.grab_ring(ray)?;
                let frame = handles.ring_frame(axis);
                let (angle, ..) = gizmo::ring_crossing(handles.origin(), frame, ray)?;
                Some(Drag::Ring {
                    origin: handles.origin(),
                    frame,
                    orientation: target.orientation(),
                    last: angle,
                    turned: 0.0,
                })
            }
        }
    }

    fn grab_axis(&self, handles: &Handles, ray: Ray) -> Option<Drag> {
        let axis = handles.direction(handles.grab(ray)?);
        let origin = handles.origin();
        Some(Drag::Axis {
            origin,
            axis,
            grabbed: gizmo::along_axis(origin, axis, ray)?,
        })
    }

    /// What this move is allowed to land on: the grid in force — with `Shift`
    /// already inverting the toolbar's checkbox for the length of the drag —
    /// and the neighbours a free drag can soft-snap onto.
    fn landing(&self, shift: bool) -> Landing<'_> {
        Landing {
            grid: self.transform.translate.grid(shift),
            neighbours: &self.neighbours,
            reach: self.arm() * SOFT_SNAP_REACH,
        }
    }

    /// One dragger arm in studs, the screen-relative length everything the
    /// gizmo measures in the world is scaled by.
    fn arm(&self) -> f32 {
        let (Some(target), Some(pose)) = (self.target, self.view) else {
            return 0.0;
        };
        gizmo::arm_length(target.position(), pose, self.orthographic)
    }

    /// The cursor moving with a drag held: works out where the part stands
    /// now and tells `Shell`, which is what writes it into the DOM.
    pub(super) fn drag_to(
        &mut self,
        position: Point<Pixels>,
        modifiers: Modifiers,
        scale: f32,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        let (Some(drag), Some(target), Some(ray)) =
            (self.drag, self.target, self.cursor_ray(position, scale))
        else {
            return;
        };
        // Read per move rather than latched at the grab: Studio's Shift is
        // held and released mid-drag, and the part follows it either way.
        let Some((drag, change)) = advance(drag, ray, self.landing(modifiers.shift)) else {
            return;
        };
        // Kept even when nothing below changes: a rotate drag measures each
        // step against the previous one, so a sample that moved the part
        // nowhere still has to be the one the next step is measured from.
        self.drag = Some(drag);

        // Kept here as well as written into the DOM: the handles have to
        // follow the cursor within this same gesture, and the DOM's answer
        // only comes back through `set_target` once `Shell` has applied it.
        let Some(moved) = applied(target, change) else {
            return;
        };
        self.target = Some(moved);

        let first = !std::mem::replace(&mut self.dragged, true);
        let referent = target.referent;
        cx.emit(match change {
            Change::Position(position) => ViewportAction::Moved {
                referent,
                position,
                first,
                settle: drag.settle(ray),
            },
            Change::Size { size, position } => ViewportAction::Resized {
                referent,
                size,
                position,
                first,
            },
            Change::Orientation(orientation) => ViewportAction::Rotated {
                referent,
                orientation,
                first,
            },
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

    /// `t`/`r` typed with a part held by its body, reporting whether it was
    /// one of the two and was actually carried out — the caller stops there
    /// only then, so a `t` typed with nothing in hand stays an ordinary key.
    pub(super) fn turn_key(
        &mut self,
        key: &str,
        modifiers: Modifiers,
        cx: &mut gpui_kit::Context<Self>,
    ) -> bool {
        // Unmodified: Ctrl+T and Ctrl+R are creator-docs' *other* quarter
        // turns, the ones that act on the selection with no drag at all, and
        // neither belongs to this gesture.
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.shift {
            return false;
        }
        match key {
            "t" => self.turn(true, cx),
            "r" => self.turn(false, cx),
            _ => false,
        }
    }

    /// `T` or `R` with a part held by its body: a quarter turn about the point
    /// it was picked up by.
    ///
    /// `creator-docs` (`parts/index.md#transform-parts`): "While cursor
    /// dragging, `T` and `R` can be used to quickly rotate the part in 90°
    /// increments around the point you picked it up by. `T` tilts the part 90°
    /// towards the camera, while `R` rotates the part 90° around the normal of
    /// the hovered surface."
    ///
    /// Only for a body drag: an axis dragger is a slide along one line, and
    /// the docs give these two keys to cursor dragging alone.
    pub(super) fn turn(&mut self, tilt: bool, cx: &mut gpui_kit::Context<Self>) -> bool {
        let (Some(Drag::Plane { point, normal, .. }), Some(target)) = (self.drag, self.target)
        else {
            return false;
        };
        let axis = if tilt {
            // The plane's normal was fixed at the grab as `-ray.direction`, so
            // the view direction is its opposite, and `forward × up` is the
            // camera's right — the axis a tilt "towards the camera" turns
            // about. A camera looking straight down has no such axis and the
            // tilt has no meaning, so nothing happens.
            (-normal).cross(Vec3::Y).try_normalize()
        } else {
            // "The hovered surface": the same nearby surface a free drag would
            // soft-snap the grab point onto. With none in reach there is
            // nothing being hovered, and the world's own up stands in for it.
            Some(
                snap::nearest_surface(point, &self.neighbours, self.arm() * SOFT_SNAP_REACH)
                    .map_or(Vec3::Y, |surface| surface.normal),
            )
        };
        let Some(axis) = axis else {
            return false;
        };

        let turn = gizmo::quarter_turn(axis);
        let (linear, position) = gizmo::turned(target.rotation(), target.position(), point, turn);
        self.target = Some(target.turned_to(linear, position));
        // The part turned about the grab point, so the cursor now holds it by
        // a different part of itself; the offset has to turn with it or the
        // next move would snap it back.
        if let Some(Drag::Plane { offset, .. }) = &mut self.drag {
            *offset = turn * *offset;
        }

        let first = !std::mem::replace(&mut self.dragged, true);
        cx.emit(ViewportAction::Turned {
            referent: target.referent,
            pivot: point,
            axis,
            first,
        });
        true
    }

    /// The boxes a free drag can soft-snap onto: every drawn part in the
    /// workspace but the selected one, pushed down from `Shell`, which is the
    /// only side with a DOM to read them out of.
    pub(crate) fn set_neighbours(&mut self, neighbours: Vec<Mat4>) {
        self.neighbours = neighbours;
    }
}

/// Which of the part's own faces a Scale handle stands on, and the drag that
/// grabbing it opens.
///
/// `BasePart.Size` is expressed along the part's *own* axes, so a resize can
/// only ever run along one of those — there is no `Size` that describes a part
/// stretched along a world axis it is not aligned to. In local orientation the
/// grabbed arm already *is* one of them; in world orientation the part's own
/// axis nearest the grabbed arm is the one that stretches, which is exact for
/// any axis-aligned part (where the two frames agree) and still grows the part
/// the way the cursor is pulling for one that is turned.
fn grab_face(handles: &Handles, target: Target, ray: Ray) -> Option<Drag> {
    let (grabbed, sign) = handles.grab_arm(ray)?;
    let arm = handles.direction(grabbed) * sign;

    let orientation = target.orientation();
    let (component, axis) = (0..3)
        .map(|component| (component, orientation.col(component)))
        .max_by(|(_, a), (_, b)| a.dot(arm).abs().total_cmp(&b.dot(arm).abs()))?;
    // Pointing out through the grabbed face, so dragging away from the part
    // always grows it.
    let axis = axis * axis.dot(arm).signum();

    let origin = target.position();
    Some(Drag::Size {
        origin,
        axis,
        grabbed: gizmo::along_axis(origin, axis, ray)?,
        size: target.size(),
        component,
    })
}

/// What this cursor ray does to the part, and the drag state the next step is
/// measured from. `None` when the gesture has no answer at this angle — an
/// axis sighted end-on, or a drag plane the ray has turned parallel to (or
/// ended up behind).
///
/// Pure, and the whole of what a drag computes: [`Drag`] is a grab's worth of
/// geometry, and this turns it plus a ray into the part's new placement.
///
/// `landing` only bears on the Move tool's two gestures (`Axis`, `Plane`) —
/// Scale and Rotate have no grid or soft-snap surface of their own to land on.
/// What it rounds is the *travel* since the handle was grabbed, not the part's
/// world position. The docs say only that increments are "based on studs" and
/// never where the grid is anchored; rounding the travel is what keeps a part
/// that already stood off-grid from jumping the moment it is picked up, and it
/// is the one reading that means the same thing for a dragger along a local
/// axis as for one along a world axis.
pub(super) fn advance(drag: Drag, ray: Ray, landing: Landing) -> Option<(Drag, Change)> {
    match drag {
        Drag::Axis {
            origin,
            axis,
            grabbed,
        } => {
            let travel = gizmo::along_axis(origin, axis, ray)? - grabbed;
            let travel = snap::round_to(travel, landing.grid);
            Some((drag, Change::Position(origin + axis * travel)))
        }
        Drag::Plane {
            point,
            normal,
            offset,
        } => {
            let hit = pick::ray_hits_plane(ray, point, normal)?;
            Some((drag, Change::Position(grabbed_at(hit, point, landing) + offset)))
        }
        Drag::Size {
            origin,
            axis,
            grabbed,
            size,
            component,
        } => {
            let travelled = gizmo::along_axis(origin, axis, ray)? - grabbed;
            let mut resized = size;
            resized[component] = (size[component] + travelled).clamp(MIN_SIZE, MAX_SIZE);
            // Half the growth, so the face opposite the grabbed one holds
            // still and the grabbed one follows the cursor. Taken from what
            // the size *actually* changed by rather than from the travel, so
            // a drag that has run into either end of the range stops moving
            // the part as well as stops resizing it.
            let grown = resized[component] - size[component];
            Some((
                drag,
                Change::Size {
                    size: resized,
                    position: origin + axis * grown * 0.5,
                },
            ))
        }
        Drag::Ring {
            origin,
            frame,
            orientation,
            last,
            turned,
        } => {
            let (angle, ..) = gizmo::ring_crossing(origin, frame, ray)?;
            let turned = turned + gizmo::angle_step(last, angle);
            let (turn, ..) = frame;
            Some((
                Drag::Ring {
                    origin,
                    frame,
                    orientation,
                    last: angle,
                    turned,
                },
                Change::Orientation(Mat3::from_axis_angle(turn, turned) * orientation),
            ))
        }
    }
}

/// The target as this change leaves it, or `None` when it leaves it exactly as
/// it was — which is what keeps a cursor that has not actually moved the part
/// from opening an undo step.
pub(super) fn applied(target: Target, change: Change) -> Option<Target> {
    match change {
        Change::Position(position) => {
            (position != target.position()).then(|| target.moved_to(position))
        }
        Change::Size { size, position } => (size != target.size() || position != target.position())
            .then(|| target.resized_to(size, position)),
        Change::Orientation(orientation) => {
            (orientation != target.orientation()).then(|| target.rotated_to(orientation))
        }
    }
}

/// Where a free drag's grab point actually settles: on the grid, or — with no
/// grid in force — soft-snapped onto whatever surface or edge it is passing.
///
/// The two are alternatives rather than both, which is what `creator-docs`
/// describes for cursor dragging: with snapping enabled a ruler shows the
/// alignment instead, and "if snapping is **disabled**, the part will
/// 'soft&nbsp;snap' to surfaces and edges of nearby parts"
/// (`parts/index.md#transform-parts`). The docs' other soft-snap sentence —
/// dragging a part *by its pivot* under the Move tool — places no condition on
/// the snap setting at all, but this editor has no pivot handle to drag
/// separately from the part's body, so there is nothing yet to treat
/// differently.
fn grabbed_at(hit: Vec3, grabbed: Vec3, landing: Landing) -> Vec3 {
    if landing.grid > 0.0 {
        return grabbed + snap::round_point(hit - grabbed, landing.grid);
    }
    snap::nearest_surface(hit, landing.neighbours, landing.reach)
        .map_or(hit, |surface| surface.point)
}

#[cfg(test)]
#[path = "gizmo/tests.rs"]
mod tests;
