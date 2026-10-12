//! Studio's handle summoning, and the free-drag ball at the gizmo's origin.
//!
//! Roblox's own announcement ("Pivot Points - Studio Beta Update: Handle
//! Summoning", DevForum topic 1335668, carried into "Pivot Editor: Full
//! Release", topic 1403027): "simply **hold the tab key**, and the handles
//! of the tool you have selected will be **summoned** directly to the
//! location of your cursor … When you're done, you *release* the tab key,
//! and the handles will return to their normal location." Read off it:
//!
//! - Move's, Scale's and Rotate's handles all summon (Transform's too, being
//!   the three at once).
//! - "When summoning the rotate handles, the selection will be rotated
//!   around the point that you summoned them to" — Rotate's rings turn the
//!   selection about wherever they stand, so that comes for free.
//! - "if you place your cursor close to an edge or vertex when summoning,
//!   the handles will snap to that edge or vertex" — said of Rotate, and
//!   kept to the tools with Rotate's rings here; see [`onto_edges`].
//! - Scale's balls are kept "within the bounds of [the] selected object" —
//!   see `rbx_viewer::gizmo::Faces::summoned` for how that is read here.
//!
//! The announcement does not say whether the handles follow the cursor
//! while `Tab` stays down. They are placed once, at the press: handles that
//! kept chasing the cursor could never be reached by it.
//!
//! The free-drag ball (`rbx_viewer::gizmo::Handles::grab_origin`) is this
//! editor's own addition, not Studio's: it is Move's body drag held from the
//! gizmo's origin, which is what still lets a summoned Move gizmo carry the
//! selection freely when the part itself is nowhere near the cursor.

use glam::{Mat4, Vec3};
use gpui_kit::{Context, Window};
use rbx_viewer::gizmo::{self, Gizmo};
use rbx_viewer::pick::{self, FlatFace, Ray, Solid};

use crate::dragger::surface::{SurfaceFrame, TargetKind};
use crate::dragger::target;
use crate::dragger::{handle_scale, pixel_size, Dot};
use crate::transform::Tool;

use super::gizmo::Drag;
use super::WorkspaceView;

gpui_kit::actions!(rbx_viewport, [SummonHandles]);

/// The key context the 3D view carries, so its `Tab` out-ranks the window's
/// own focus-cycling binding (see `shell::roving`) — and hands the key back
/// to it whenever there are no handles to summon.
pub(crate) const CONTEXT: &str = "RbxViewport";

/// A binding rather than a key listener for the reason `shell::roving`
/// gives: GPUI resolves `Tab` to an action before any listener sees it. The
/// release still arrives as an ordinary key-up (see `WorkspaceView::key`).
pub(crate) fn install(cx: &mut gpui_kit::App) {
    cx.bind_keys([gpui_kit::KeyBinding::new(
        "tab",
        SummonHandles,
        Some(CONTEXT),
    )]);
}

/// How close to a face's edge or corner the cursor has to be for the
/// summoned handles to snap onto it, in (logical) pixels on screen. Roblox
/// published Studio's in the same topic (staff replies #9 and #14): "within
/// 16 pixels of an edge or vertex".
const EDGE_SNAP: f32 = 16.0;

/// The summoning state the view keeps between events.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Summon {
    /// Where the handles were summoned to, or `None` to stand them at the
    /// selection's own pivot.
    point: Option<Vec3>,
    /// Where the summon snapped onto an edge or vertex, marked by
    /// [`snap_marker`] for as long as the handles stand there.
    snapped: Option<Vec3>,
    /// Whether `Tab` is down: the handles stay summoned through a drag that
    /// began with it held, and go home at the end of it if it was let go.
    tab: bool,
    /// How far a handle drag's rays are moved before they are measured —
    /// see [`WorkspaceView::measure_from_handle`].
    shift: Vec3,
    /// The gizmo last asked of the renderer, before the summon point is
    /// filled in.
    pub(super) shown: Option<Gizmo>,
}

impl WorkspaceView {
    /// `Tab` pressed over the view (and again, repeated, for as long as it
    /// is held — only the first press places the handles).
    pub(super) fn summon_handles(
        &mut self,
        _: &SummonHandles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Edit Pivot's handles *are* the pivot: summoning them elsewhere
        // would draw them somewhere the pivot is not.
        if self.transform.gizmo().is_none()
            || self.transform.tool == Tool::Pivot
            || self.targets.anchor().is_none()
        {
            // Nothing to summon: `Tab` keeps moving the keyboard focus on.
            cx.propagate();
            return;
        }
        if std::mem::replace(&mut self.summon.tab, true) || self.drag.is_some() {
            return;
        }
        // `Shell`'s last hover answer is where the cursor met the scene when
        // the mouse last moved: a camera flown with the keys since has
        // carried the scene out from under it. Ask again where the cursor
        // stands now, and place the handles once that answer is in — the
        // hover event is handled before the deferred placement runs, GPUI
        // flushing its effects in order (see the test beside this).
        if let Some(at) = self.cursor.filter(|_| !self.looking) {
            self.hover_pending = None;
            self.hover_moved(at, self.guides.modifiers, window.scale_factor(), cx);
        }
        cx.defer_in(window, |view, window, cx| view.place_summon(window, cx));
    }

    /// The second half of [`WorkspaceView::summon_handles`], once the hover
    /// is fresh.
    fn place_summon(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.summon.tab || self.drag.is_some() {
            return;
        }
        let (point, snapped) = self.summon_point(window.scale_factor()).unzip();
        self.summon.point = point;
        self.summon.snapped = snapped.flatten();
        self.refresh_gizmo();
        self.refresh_guides();
        cx.notify();
    }

    /// `Tab` let go, or the view lost the keyboard: the handles go home,
    /// once whatever drag is under way has finished.
    pub(super) fn release_summon(&mut self) {
        self.summon.tab = false;
        if self.drag.is_none() && self.summon.point.take().is_some() {
            self.refresh_gizmo();
            self.refresh_guides();
        }
    }

    /// A drag ending: summoned handles stay where the drag carried them for
    /// as long as `Tab` is still down.
    pub(super) fn end_summon(&mut self, summoned: Option<Vec3>) {
        self.summon.point = summoned.filter(|_| self.summon.tab);
    }

    /// Where the summoned handles stand right now: a Move carries them with
    /// the selection, so the arrow or ball in hand stays under the cursor.
    pub(super) fn summoned(&self) -> Option<Vec3> {
        let point = self.summon.point?;
        let carried = match self.drag {
            Some(Drag::Axis { .. } | Drag::Plane { .. }) => self
                .targets
                .anchor()
                .zip(self.held.anchor())
                .map_or(Vec3::ZERO, |(now, then)| now.position() - then.position()),
            _ => Vec3::ZERO,
        };
        Some(point + carried)
    }

    /// Studio's magenta indicator on the edge or vertex the handles
    /// snapped to (staff reply #14: "you'll get a magenta colored indicator
    /// letting you know that the handles snapped") — shown while they stand
    /// there and no drag is under way. Its size is not published; it is the
    /// hover ruler's dot's.
    pub(super) fn snap_marker(&self) -> Option<Dot> {
        let snapped = self.summon.snapped?;
        let pose = self.view?;
        (self.drag.is_none() && self.summon.point == Some(snapped)).then(|| Dot {
            centre: snapped,
            radius: 0.15 * handle_scale(snapped, pose, self.orthographic),
            color: [1.0, 0.0, 1.0],
        })
    }

    /// Asks the renderer to draw `gizmo`, summoned wherever the handles are.
    pub(super) fn show_gizmo(&mut self, gizmo: Option<Gizmo>) {
        self.summon.shown = gizmo;
        self.refresh_gizmo();
    }

    /// The last gizmo shown again, summoned to where the handles are now —
    /// a no-op while nothing is summoned.
    pub(super) fn refresh_gizmo(&self) {
        let (summon, pivot, snapped) = (self.summoned(), self.targets.pivot(), self.snapped);
        self.pump.gizmo(self.summon.shown.map(|gizmo| Gizmo {
            summon,
            pivot,
            snapped,
            ..gizmo
        }));
    }

    /// Re-sends the gizmo while handles are travelling with a drag: summoned
    /// ones, or ones standing on a pivot the drag carries.
    pub(super) fn follow_summon(&self) {
        if self.summon.point.is_some() || self.targets.pivot().is_some() {
            self.refresh_gizmo();
        }
    }

    /// Where the cursor puts the handles: the point under it on whatever
    /// part it is over (`Shell`'s last hover answer) — snapped onto that
    /// face's edge or corner when it is close to one, for the tools with
    /// Rotate's rings only; over nothing, the point under the cursor level
    /// with the selection's centre. A curved region of a mesh has no real
    /// edges (see `FlatFace`), so the handles land on the hit itself there
    /// rather than on some tiny triangle's edge.
    ///
    /// Roblox describes the snap for Rotate alone, and says why (staff reply
    /// #9): "it doesn't matter precisely where the handles are summoned to
    /// for Move -- the result is the same regardless." Transform snaps too,
    /// carrying Rotate's very rings; Scale and Move do not.
    ///
    /// The second half of the answer is the point again when it snapped.
    fn summon_point(&self, scale: f32) -> Option<(Vec3, Option<Vec3>)> {
        let (inside, hover) = self.cursor_over();
        if !inside {
            return None;
        }
        let pose = self.view?;
        if let Some((frame, hit)) = hover {
            if !matches!(self.transform.tool, Tool::Rotate | Tool::Transform) {
                return Some((hit, None));
            }
            let height = self.viewport.get().size.1 as f32;
            let reach =
                |at| EDGE_SNAP * scale * pixel_size(at, pose, self.orthographic, height.max(1.0));
            if let Some((face, hit)) = self.mesh_face(&frame, scale) {
                let snapped = onto_sides(&face.corners, &face.sides, hit, reach(hit));
                return Some((snapped.unwrap_or(hit), snapped));
            }
            let snapped = onto_edges(&frame, hit, reach(hit));
            return Some((snapped.unwrap_or(hit), snapped));
        }
        let ray = self.cursor_ray(self.cursor?, scale)?;
        pick::ray_hits_plane(ray, self.targets.centre()?, -ray.direction).map(|point| (point, None))
    }

    /// The flat face of the mesh under the cursor (see
    /// `PartSurface::flat_face`), and where the cursor meets it — not the
    /// hover's own point, which sits on the grid when Studio's probes found
    /// no edge of the face. Outlined here, on the press, rather than at every
    /// hover: walking a mesh's triangles is more than a hover needs. `None`
    /// off a mesh, or on one not downloaded.
    fn mesh_face(&self, frame: &SurfaceFrame, scale: f32) -> Option<(FlatFace, Vec3)> {
        if !matches!(frame.part, Some((Solid::Mesh, _))) {
            return None;
        }
        let part = self.hover_part.as_ref()?;
        let ray = self.cursor_ray(self.cursor?, scale)?;
        let (distance, _) = part.raycast(ray)?;
        Some((part.flat_face(ray)?, ray.at(distance)))
    }

    /// The free-drag ball at the gizmo's origin, if `ray` is on it: Move's
    /// body drag (see `Drag::Plane`), held by the gizmo's origin rather than
    /// by where a click met the part. Move and Transform only, the two tools
    /// whose body drag it stands in for.
    pub(super) fn grab_origin(&self, ray: Ray) -> Option<Drag> {
        if !matches!(self.transform.tool, Tool::Move | Tool::Transform) {
            return None;
        }
        let handles = self.handles()?;
        if !handles.grab_origin(ray) {
            return None;
        }
        let point = handles.origin();
        Some(Drag::Plane {
            point,
            normal: -ray.direction,
            offset: self.targets.anchor()?.position() - point,
        })
    }

    /// Re-measures a handle grab along the line the grabbed handle actually
    /// stands on, and keeps the offset for the rest of the drag.
    ///
    /// A drag measures the cursor along a line through the part (`origin`,
    /// which is also what its new placement is built from), but the handle
    /// grabbed may stand off that line: a summoned Move arrow at the
    /// cursor, a summoned Scale ball slid across its face, a group's arrows
    /// at the group's centre rather than its anchor's. Under perspective,
    /// where the cursor's ray meets a line depends on which line it is, so
    /// the rays are moved by the offset between the two lines instead —
    /// measuring a moved ray against the part's line is measuring the real
    /// ray against the handle's.
    pub(super) fn measure_from_handle(&mut self, mut drag: Drag, ray: Ray) -> Drag {
        self.summon.shift = match drag {
            Drag::Axis { origin, .. } => self
                .handles()
                .map_or(Vec3::ZERO, |handles| origin - handles.origin()),
            Drag::Size { .. } | Drag::Box { .. } => self
                .faces()
                .and_then(|faces| Some(-faces.slide(faces.grab(ray)?.0)))
                .unwrap_or(Vec3::ZERO),
            Drag::Plane { .. }
            | Drag::Ring { .. }
            | Drag::Sun
            | Drag::Terrain
            | Drag::Joint { .. } => Vec3::ZERO,
        };
        let ray = self.measured(ray);
        if let Drag::Axis {
            origin,
            axis,
            grabbed,
        }
        | Drag::Size {
            origin,
            axis,
            grabbed,
            ..
        }
        | Drag::Box {
            origin,
            axis,
            grabbed,
            ..
        } = &mut drag
        {
            if let Some(along) = gizmo::along_axis(*origin, *axis, ray) {
                *grabbed = along;
            }
        }
        drag
    }

    /// A press that grabs no handle: nothing for later rays to be moved by.
    pub(super) fn measure_from_part(&mut self) {
        self.summon.shift = Vec3::ZERO;
    }

    /// `ray` moved by the drag's offset (see
    /// [`WorkspaceView::measure_from_handle`]).
    pub(super) fn measured(&self, ray: Ray) -> Ray {
        Ray {
            origin: ray.origin + self.summon.shift,
            ..ray
        }
    }
}

/// `hit` on `frame`'s face, moved onto the face's edge or corner when it is
/// within `reach` of one — Studio's summoned handles "snap to that edge or
/// vertex" — or `None` when it is near neither. Only a flat face has edges
/// and corners to snap to; a point on a ball's or a cylinder's curve has
/// none.
///
/// The candidates are the face's real outline, out of the part it is on
/// (`frame.part`): a box's or a wedge's face is the polygon of the solid's
/// corners lying in the face's plane — a wedge's side a triangle, not the
/// rectangle round it the frame's `size` measures — a cylinder's cap is its
/// rim, and a mesh face only the edge its frame was probed from — the one
/// edge of it known for sure until the mesh has downloaded, after which
/// [`WorkspaceView::summon_point`] outlines the face from its triangles
/// instead. A frame with no part is its own rectangle.
pub(super) fn onto_edges(frame: &SurfaceFrame, hit: Vec3, reach: f32) -> Option<Vec3> {
    if frame.kind != TargetKind::Polygon {
        return None;
    }
    let local = frame.local(hit);
    let (sx, sz) = (1f32.copysign(local.x), 1f32.copysign(local.z));
    let Some((solid, model)) = frame.part else {
        // Cornered on the face corner nearest the hit, with `x` and `z` along
        // its two edges; either may point out of the face, which the sign of
        // the hit's own coordinate gives away.
        let (across, along) = (frame.x * sx * frame.size.x, frame.z * sz * frame.size.y);
        let at = frame.corner;
        return onto_outline(
            &[at, at + across, at + across + along, at + along],
            true,
            hit,
            reach,
        );
    };
    match solid {
        Solid::Ball => None,
        Solid::Cylinder => onto_rim(model, hit, reach),
        Solid::Mesh => {
            let edge = [frame.corner, frame.corner + frame.z * sz * frame.size.y];
            onto_outline(&edge, false, hit, reach)
        }
        Solid::Box | Solid::Wedge | Solid::CornerWedge => {
            onto_outline(&face_outline(frame, solid, model, hit)?, true, hit, reach)
        }
    }
}

/// The corners of `solid`'s face through `hit` (the plane of `frame`), in
/// order round it.
fn face_outline(frame: &SurfaceFrame, solid: Solid, model: Mat4, hit: Vec3) -> Option<Vec<Vec3>> {
    let size = (model.x_axis + model.y_axis + model.z_axis)
        .truncate()
        .length();
    let tolerance = 1e-4 * size.max(1.0);
    let mut face: Vec<Vec3> = target::corners(solid)?
        .iter()
        .map(|&corner| model.transform_point3(corner))
        .filter(|&corner| (corner - hit).dot(frame.y).abs() <= tolerance)
        .collect();
    if face.len() < 3 {
        return None;
    }
    let centre = face.iter().sum::<Vec3>() / face.len() as f32;
    let angle = |corner: &Vec3| {
        let offset = *corner - centre;
        offset.dot(frame.z).atan2(offset.dot(frame.x))
    };
    face.sort_by(|a, b| angle(a).total_cmp(&angle(b)));
    Some(face)
}

/// `hit` on a cylinder's cap moved onto its rim, when within `reach` of it.
fn onto_rim(model: Mat4, hit: Vec3, reach: f32) -> Option<Vec3> {
    let (rotation, centre, size) = target::placement(model)?;
    let axis = rotation.x_axis;
    let cap = centre + axis * axis.dot(hit - centre);
    let radial = hit - cap;
    let radius = 0.5 * size.y.min(size.z);
    let out = radial.try_normalize()?;
    ((radial.length() - radius).abs() <= reach).then(|| cap + out * radius)
}

/// `hit` moved onto the nearest corner of `outline` within `reach`, else onto
/// the nearest point of its sides within `reach` — the last corner joined
/// back to the first when `closed`.
fn onto_outline(outline: &[Vec3], closed: bool, hit: Vec3, reach: f32) -> Option<Vec3> {
    let closing = closed.then(|| [outline[outline.len() - 1], outline[0]]);
    let sides: Vec<[Vec3; 2]> = outline
        .windows(2)
        .map(|pair| [pair[0], pair[1]])
        .chain(closing)
        .collect();
    onto_sides(outline, &sides, hit, reach)
}

/// `hit` moved onto the nearest of `corners` within `reach`, else onto the
/// nearest point of `sides` within `reach`.
pub(super) fn onto_sides(
    corners: &[Vec3],
    sides: &[[Vec3; 2]],
    hit: Vec3,
    reach: f32,
) -> Option<Vec3> {
    let nearest = |points: &mut dyn Iterator<Item = Vec3>| {
        points
            .filter(|point| point.distance(hit) <= reach)
            .min_by(|a, b| a.distance(hit).total_cmp(&b.distance(hit)))
    };
    nearest(&mut corners.iter().copied()).or_else(|| {
        nearest(&mut sides.iter().map(|&[a, b]| {
            let side = b - a;
            let t = (hit - a).dot(side) / side.length_squared().max(f32::MIN_POSITIVE);
            a + side * t.clamp(0.0, 1.0)
        }))
    })
}

#[cfg(test)]
#[path = "summon/tests.rs"]
mod tests;
